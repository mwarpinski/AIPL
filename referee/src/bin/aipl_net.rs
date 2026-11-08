//! aipl-net: host names and TLS for a native executable (docs/design/SHIP_SELFHOSTED_PLAN.md, L3).
//!
//!   aipl-net connect HOST PORT [--tls]
//!
//! A native executable is static machine code: it cannot link rustls or call the system's resolver. std/net starts
//! this program with pipes instead and speaks to it as to a connection. It connects to HOST:PORT (the system's
//! resolver finds the address), makes the connection secure when asked, and then relays: what arrives on its
//! standard input goes to the host, what the host sends goes to its standard output, until either side ends.
//!
//! Before the relay it writes ONE status byte to its standard output, and nothing else if that is not 0:
//! 0 connected (and, with --tls, the handshake done), 1 refused or failed, 2 not granted, 4 the TLS handshake
//! failed, 5 the name did not resolve, 6 not a host name.
//!
//! The grants are the executable's, not the caller's. A program that could start this one with any arguments
//! must still reach only the hosts it was granted, so this reads them from the executable of its parent, where
//! the compiler put them (`--allow-connect`): a trailer at the end of the file, the grants one HOST:PORT a line,
//! then their length as four little-endian bytes and the eight bytes `AIPLNET1`. A parent without that trailer
//! (a shell, say) has granted nothing, and every host is refused. Linux only (the parent's executable is
//! /proc/PPID/exe), as the native executables are.
//! The trusted certificates are the Mozilla roots and the PEM file `AIPL_CA_FILE` names, as in the other hosts.

#[cfg(target_os = "linux")]
fn main() {
    std::process::exit(linux::run(std::env::args().skip(1).collect()));
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("aipl-net: only on Linux, where there are native executables to serve");
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
mod linux {
    use aipl_core::net::{ConnectGrant, Net};
    use std::io::{Read, Seek, SeekFrom, Write};

    const MAGIC: &[u8; 8] = b"AIPLNET1";

    /// The status byte std/net reads.
    const OK: u8 = 0;
    const REFUSED: u8 = 1;
    const NOT_GRANTED: u8 = 2;
    const TLS_FAILED: u8 = 4;
    const NO_SUCH_HOST: u8 = 5;
    const BAD_NAME: u8 = 6;

    pub fn run(args: Vec<String>) -> i32 {
        let (host, port, tls) = match args.as_slice() {
            [c, host, port] if c == "connect" => (host, port, false),
            [c, host, port, f] if c == "connect" && f == "--tls" => (host, port, true),
            _ => {
                eprintln!("usage: aipl-net connect HOST PORT [--tls]");
                return 2;
            }
        };
        let Ok(port) = port.parse::<u16>() else {
            eprintln!("aipl-net: {port}: not a port");
            return 2;
        };
        let net = Net::new(Vec::new()).with_connect(parent_grants());
        let h = net.connect_host(host.as_bytes(), i32::from(port));
        let status = match h {
            -2 => NOT_GRANTED,
            -5 => NO_SUCH_HOST,
            -1 if !Net::is_host_name(host) && host.parse::<std::net::IpAddr>().is_err() => BAD_NAME,
            h if h < 0 => REFUSED,
            h if tls => match net.tls_connect(h, host.as_bytes()) {
                0 => OK,
                -6 => BAD_NAME,
                _ => TLS_FAILED,
            },
            _ => OK,
        };
        let mut out = std::io::stdout().lock();
        if out.write_all(&[status]).and_then(|_| out.flush()).is_err() || status != OK {
            return i32::from(status);
        }
        // a hook for tests (referee/tests/test_net.rs): wait before relaying, so that what a server sends right after the
        // handshake (TLS 1.3 session tickets) is already waiting when the relay starts
        if let Some(ms) = std::env::var("AIPL_NET_TEST_DELAY_MS").ok().and_then(|v| v.parse().ok()) {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
        relay(&net, h, &mut out);
        0
    }

    /// Moves bytes both ways until one side ends. One thread: a read waiting on a secure connection holds the
    /// lock a write needs, so the connection is asked whether it can be read (poll) before it is.
    fn relay(net: &Net, h: i32, out: &mut impl Write) {
        let mut stdin = std::io::stdin().lock();
        let mut buf = vec![0u8; 16384];
        loop {
            // what the host has sent, as long as it has some
            while net.poll(h, 1, 0) == 1 {
                let n = net.recv(h, &mut buf);
                if n <= 0 {
                    let _ = net.close(h);
                    return;
                }
                if out.write_all(&buf[..n as usize]).and_then(|_| out.flush()).is_err() {
                    let _ = net.close(h);
                    return;
                }
            }
            // what the program has sent, waiting a moment for it so that an idle relay does not spin
            let mut fd = libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 };
            // SAFETY: one valid pollfd
            let ready = unsafe { libc::poll(&mut fd, 1, 5) };
            if ready > 0 {
                match stdin.read(&mut buf) {
                    Ok(0) | Err(_) => {
                        // the program has ended or closed its side
                        let _ = net.close(h);
                        return;
                    }
                    Ok(n) => {
                        let mut sent = 0;
                        while sent < n {
                            let k = net.send(h, &buf[sent..n]);
                            if k <= 0 {
                                let _ = net.close(h);
                                return;
                            }
                            sent += k as usize;
                        }
                    }
                }
            }
        }
    }

    /// The connect grants in the trailer of the parent's executable; none if there is no parent trailer.
    fn parent_grants() -> Vec<ConnectGrant> {
        // SAFETY: getppid has no arguments and cannot fail
        let ppid = unsafe { libc::getppid() };
        grants_in(&format!("/proc/{ppid}/exe")).unwrap_or_default()
    }

    fn grants_in(path: &str) -> Option<Vec<ConnectGrant>> {
        let mut f = std::fs::File::open(path).ok()?;
        let size = f.seek(SeekFrom::End(0)).ok()?;
        if size < 12 {
            return None;
        }
        f.seek(SeekFrom::End(-12)).ok()?;
        let mut tail = [0u8; 12];
        f.read_exact(&mut tail).ok()?;
        if &tail[4..] != MAGIC {
            return None;
        }
        let len = u64::from(u32::from_le_bytes(tail[..4].try_into().ok()?));
        if len + 12 > size {
            return None;
        }
        f.seek(SeekFrom::End(-12 - len as i64)).ok()?;
        let mut text = vec![0u8; len as usize];
        f.read_exact(&mut text).ok()?;
        Some(String::from_utf8_lossy(&text).lines().filter_map(|l| Net::parse_connect_grant(l.trim()).ok()).collect())
    }
}
