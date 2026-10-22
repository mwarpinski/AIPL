//! The `aipl_net_v1` host interface (docs/design/NET_PLAN.md section 3) for
//! the two hosts written in Rust: the VM and aipl-run. Native executables
//! do the same in AIPL, as Linux system calls (aipl_src/native/wasi.aipl).
//!
//! Sockets are numbered in their own table, apart from WASI's file
//! descriptors, and closed by `close`. An address arrives as its bytes (4
//! for IPv4, 16 for IPv6; std/net parses the text), and a listen is refused
//! with -2 unless the address and port were granted (`--listen ADDR:PORT`).
//! Blocking calls run on a clone of the socket, outside the table's lock,
//! so one thread's accept does not hold up another's send.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

/// Not granted: the program asked for an address and port no `--listen`
/// names.
pub const NOT_GRANTED: i32 = -2;

enum Sock {
    Listener(TcpListener),
    Stream(TcpStream),
}

#[derive(Default)]
struct Table {
    socks: HashMap<i32, Sock>,
    next: i32,
}

/// One program's sockets and grants, shared by its threads.
#[derive(Clone, Default)]
pub struct Net {
    listen: Arc<Vec<SocketAddr>>,
    table: Arc<Mutex<Table>>,
}

impl Net {
    /// A table with these `--listen` grants.
    pub fn new(listen: Vec<SocketAddr>) -> Net {
        Net { listen: Arc::new(listen), table: Arc::new(Mutex::new(Table { socks: HashMap::new(), next: 3 })) }
    }

    /// `--listen ADDR:PORT`, as `127.0.0.1:8080` or `[::1]:8080`: what
    /// std/ip's parse_endpoint reads (Rust's parser, less zone indexes), so
    /// a grant means the same in every backend.
    pub fn parse_grant(text: &str) -> Result<SocketAddr, String> {
        Self::parse_address("--listen", text)
    }

    /// An ADDR:PORT given with `flag`, named in the message if it is not one.
    pub fn parse_address(flag: &str, text: &str) -> Result<SocketAddr, String> {
        let bad = || format!("{flag} {text}: expected ADDR:PORT, as 127.0.0.1:8080 or [::1]:8080");
        if text.contains('%') {
            return Err(bad());
        }
        text.parse().map_err(|_| bad())
    }

    fn insert(&self, s: Sock) -> i32 {
        let mut t = self.table.lock().unwrap();
        let h = t.next;
        t.next += 1;
        t.socks.insert(h, s);
        h
    }

    /// The address `addr` (4 or 16 bytes) and `port` as a socket address.
    fn address(addr: &[u8], port: i32) -> Option<SocketAddr> {
        let port = u16::try_from(port).ok()?;
        let ip = match addr.len() {
            4 => IpAddr::V4(Ipv4Addr::new(addr[0], addr[1], addr[2], addr[3])),
            16 => IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(addr).ok()?)),
            _ => return None,
        };
        Some(SocketAddr::new(ip, port))
    }

    /// `listen(addr, len, port)`: a listening socket's handle; -1 if the
    /// address is malformed or the system refuses, NOT_GRANTED if no grant
    /// names it.
    pub fn listen(&self, addr: &[u8], port: i32) -> i32 {
        let Some(at) = Self::address(addr, port) else { return -1 };
        if !self.listen.contains(&at) {
            return NOT_GRANTED;
        }
        match TcpListener::bind(at) {
            Ok(l) => self.insert(Sock::Listener(l)),
            Err(_) => -1,
        }
    }

    /// `accept(h)`: the next connection's handle, or -1.
    pub fn accept(&self, h: i32) -> i32 {
        let l = match self.table.lock().unwrap().socks.get(&h) {
            Some(Sock::Listener(l)) => l.try_clone(),
            _ => return -1,
        };
        match l.and_then(|l| l.accept()) {
            Ok((s, _)) => self.insert(Sock::Stream(s)),
            Err(e) => {
                if std::env::var_os("AIPL_NET_DEBUG").is_some() { eprintln!("net debug: accept({h}) failed: {e:?}"); }
                -1
            }
        }
    }

    fn stream(&self, h: i32) -> Option<TcpStream> {
        match self.table.lock().unwrap().socks.get(&h) {
            Some(Sock::Stream(s)) => s.try_clone().ok(),
            _ => None,
        }
    }

    /// `recv(h, buf, max)`: reads into `into`; bytes read, 0 at the end, -1.
    pub fn recv(&self, h: i32, into: &mut [u8]) -> i32 {
        let r = self.stream(h).map(|mut s| s.read(into));
        if std::env::var_os("AIPL_NET_DEBUG").is_some() {
            match &r { Some(Ok(n)) if *n > 0 => {}, other => eprintln!("net debug: recv({h}) -> {other:?}") }
        }
        match r {
            Some(Ok(n)) => n as i32,
            _ => -1,
        }
    }

    /// `send(h, buf, len)`: bytes written, or -1.
    pub fn send(&self, h: i32, bytes: &[u8]) -> i32 {
        match self.stream(h).map(|mut s| s.write(bytes)) {
            Some(Ok(n)) => n as i32,
            _ => -1,
        }
    }

    /// `close(h)`: 0, or -1 for a handle that is not open. A connection is
    /// shut down first, so a read blocked on it in another thread returns.
    pub fn close(&self, h: i32) -> i32 {
        match self.table.lock().unwrap().socks.remove(&h) {
            Some(Sock::Stream(s)) => {
                let _ = s.shutdown(std::net::Shutdown::Both);
                0
            }
            Some(Sock::Listener(_)) => 0,
            None => -1,
        }
    }
}
