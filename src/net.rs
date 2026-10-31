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
//!
//! Beyond addresses, these hosts (and only these: a native executable has no resolver and no TLS) can
//! connect to a host by name, through the system's resolver (`connect_host`), and speak TLS over a
//! connection with rustls (`tls_connect`): the certificate is checked against the name, with the
//! Mozilla roots (webpki-roots) and any in the PEM file `AIPL_CA_FILE` names. The program then reads
//! and writes plain bytes as ever.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex, OnceLock};

/// Why a fresh instance's answer was not sent: it passed the most a host
/// keeps (aipl-run FRESH_CAP, aipl_src/native/fresh.aipl cap), alike in each.
pub const FRESH_TOO_LONG: &str = "the answer is more than 64 MiB, the most a fresh instance may write: sent 500 Internal Server Error";

/// Not granted: the program asked for an address and port no `--listen`
/// (or, for `connect`, `--allow-connect`) names.
pub const NOT_GRANTED: i32 = -2;

/// The host a `--allow-connect` grant names: an address, or a name (matched against the name a program
/// gives `connect_host`, ignoring case; whatever it resolves to is then reached).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrantHost {
    Ip(IpAddr),
    Name(String),
}

/// One `--allow-connect HOST:PORT` grant: a host, and a port or any port (`HOST:*`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectGrant {
    pub host: GrantHost,
    pub port: Option<u16>,
}

/// `connect_host`'s and `tls_connect`'s results besides a handle or 0: not granted, not supported (a
/// native executable), a name that does not resolve, a handshake that failed, a bad name.
pub const NOT_SUPPORTED: i32 = -3;
pub const NO_SUCH_HOST: i32 = -5;
pub const TLS_FAILED: i32 = -1;
pub const BAD_NAME: i32 = -6;

type TlsStream = rustls::StreamOwned<rustls::ClientConnection, TcpStream>;

enum Sock {
    Listener(TcpListener),
    Stream(TcpStream),
    /// A connection made secure by `tls_connect`: one lock for reading and writing (a read waiting for
    /// the peer holds up a write on the same connection)
    Tls(Arc<Mutex<TlsStream>>, TcpStream),
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
    connect: Arc<Vec<ConnectGrant>>,
    table: Arc<Mutex<Table>>,
}

impl Net {
    /// A table with these `--listen` grants.
    pub fn new(listen: Vec<SocketAddr>) -> Net {
        Net { listen: Arc::new(listen), connect: Arc::default(), table: Arc::new(Mutex::new(Table { socks: HashMap::new(), next: 3 })) }
    }

    /// The same table with these `--allow-connect` grants.
    pub fn with_connect(&self, connect: Vec<ConnectGrant>) -> Net {
        Net { connect: Arc::new(connect), ..self.clone() }
    }

    /// `--allow-connect ADDR:PORT` or `ADDR:*` (any port), as `127.0.0.1:8080`, `[::1]:*`: an address,
    /// not a name (there is no resolver yet), in the form std/ip's parse_endpoint reads.
    pub fn parse_connect_grant(text: &str) -> Result<ConnectGrant, String> {
        let bad = || format!("--allow-connect {text}: expected HOST:PORT or HOST:*, as 127.0.0.1:8080, [::1]:*, or example.com:443");
        let Some((host, port)) = text.rsplit_once(':') else { return Err(bad()) };
        let port = if port == "*" { None } else { Some(port.parse::<u16>().map_err(|_| bad())?) };
        let bracketed = host.strip_prefix('[').and_then(|h| h.strip_suffix(']'));
        let host = bracketed.unwrap_or(host);
        if host.contains('%') {
            return Err(bad());
        }
        if let Ok(ip) = host.parse() {
            return Ok(ConnectGrant { host: GrantHost::Ip(ip), port });
        }
        if bracketed.is_some() || !Self::is_host_name(host) {
            return Err(bad());
        }
        Ok(ConnectGrant { host: GrantHost::Name(host.to_ascii_lowercase()), port })
    }

    /// Whether `name` is a host name as a program may give one: letters, digits, `-`, and `.` between
    /// labels of 1 to 63 characters (an internationalized name in its punycode form), whose last label is
    /// not all digits (a name like that is a malformed address). std/ip's is_host_name reads the same.
    pub fn is_host_name(name: &str) -> bool {
        let trimmed = name.trim_end_matches('.');
        !name.is_empty()
            && name.len() <= 253
            && !trimmed.rsplit('.').next().is_some_and(|l| l.bytes().all(|b| b.is_ascii_digit()))
            && trimmed.split('.').all(|l| {
                !l.is_empty() && l.len() <= 63 && !l.starts_with('-') && !l.ends_with('-') && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
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

    /// `connect(addr, len, port)`: a connection's handle; -1 if the address is malformed or the
    /// connection fails, NOT_GRANTED if no `--allow-connect` names the address and port.
    pub fn connect(&self, addr: &[u8], port: i32) -> i32 {
        let Some(at) = Self::address(addr, port) else { return -1 };
        if !self.connect.iter().any(|g| g.host == GrantHost::Ip(at.ip()) && g.port.is_none_or(|p| p == at.port())) {
            return NOT_GRANTED;
        }
        match TcpStream::connect(at) {
            Ok(s) => self.insert(Sock::Stream(s)),
            Err(_) => -1,
        }
    }

    /// `connect_host(name, port)`: a connection to the host `name` (resolved by the system, or an address
    /// as text) as `connect` makes one; NOT_GRANTED unless a grant names it (by that name, ignoring case,
    /// or by the address it is), -1 for a malformed name or port or a connection that fails, NO_SUCH_HOST
    /// for a name that does not resolve.
    pub fn connect_host(&self, name: &[u8], port: i32) -> i32 {
        let (Ok(name), Ok(port)) = (std::str::from_utf8(name), u16::try_from(port)) else { return -1 };
        let name = name.to_ascii_lowercase();
        let granted = |host: &GrantHost| self.connect.iter().any(|g| &g.host == host && g.port.is_none_or(|p| p == port));
        if let Ok(ip) = name.parse::<IpAddr>() {
            return if granted(&GrantHost::Ip(ip)) { self.connect(&Self::ip_bytes(ip), i32::from(port)) } else { NOT_GRANTED };
        }
        if !Self::is_host_name(&name) {
            return -1;
        }
        if !granted(&GrantHost::Name(name.trim_end_matches('.').to_string())) {
            return NOT_GRANTED;
        }
        let addrs: Vec<SocketAddr> = match (name.as_str(), port).to_socket_addrs() {
            Ok(a) => a.collect(),
            Err(_) => return NO_SUCH_HOST,
        };
        match TcpStream::connect(&addrs[..]) {
            Ok(s) => self.insert(Sock::Stream(s)),
            Err(_) => -1,
        }
    }

    fn ip_bytes(ip: IpAddr) -> Vec<u8> {
        match ip {
            IpAddr::V4(a) => a.octets().to_vec(),
            IpAddr::V6(a) => a.octets().to_vec(),
        }
    }

    /// `tls_connect(h, name)`: makes connection `h` secure as a client of the server `name` (the name the
    /// certificate must have: a host name or an address as text): 0, TLS_FAILED if the handshake fails
    /// (an untrusted or mismatched certificate, a server that does not speak TLS), BAD_NAME, or -1 for a
    /// handle that is not a plain connection.
    pub fn tls_connect(&self, h: i32, name: &[u8]) -> i32 {
        let Some(server_name) = std::str::from_utf8(name).ok().and_then(|n| rustls::pki_types::ServerName::try_from(n.to_string()).ok()) else {
            return BAD_NAME;
        };
        let Some(tcp) = self.stream(h) else { return -1 };
        let Ok(mut conn) = rustls::ClientConnection::new(tls_config(), server_name) else { return TLS_FAILED };
        let mut io = match tcp.try_clone() {
            Ok(io) => io,
            Err(_) => return -1,
        };
        // the handshake now, so that a bad certificate is the answer here and not a failed read later
        while conn.is_handshaking() {
            if conn.complete_io(&mut io).is_err() {
                return TLS_FAILED;
            }
        }
        let mut t = self.table.lock().unwrap();
        match t.socks.get(&h) {
            Some(Sock::Stream(_)) => {
                t.socks.insert(h, Sock::Tls(Arc::new(Mutex::new(rustls::StreamOwned::new(conn, io))), tcp));
                0
            }
            _ => -1,
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
            Err(_) => -1,
        }
    }

    /// A plain connection's socket (not a secure one: tls_connect wraps those).
    fn stream(&self, h: i32) -> Option<TcpStream> {
        match self.table.lock().unwrap().socks.get(&h) {
            Some(Sock::Stream(s)) => s.try_clone().ok(),
            _ => None,
        }
    }

    /// The secure stream of `h`, if it has one.
    fn tls(&self, h: i32) -> Option<Arc<Mutex<TlsStream>>> {
        match self.table.lock().unwrap().socks.get(&h) {
            Some(Sock::Tls(s, _)) => Some(s.clone()),
            _ => None,
        }
    }

    /// `recv(h, buf, max)`: reads into `into`; bytes read, 0 at the end, -1.
    pub fn recv(&self, h: i32, into: &mut [u8]) -> i32 {
        if let Some(t) = self.tls(h) {
            return match t.lock().unwrap().read(into) {
                Ok(n) => n as i32,
                // a peer that closes without TLS's close_notify is the end of the data, as most clients take it
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => 0,
                Err(_) => -1,
            };
        }
        match self.stream(h).map(|mut s| s.read(into)) {
            Some(Ok(n)) => n as i32,
            _ => -1,
        }
    }

    /// `send(h, buf, len)`: bytes written, or -1.
    pub fn send(&self, h: i32, bytes: &[u8]) -> i32 {
        if let Some(t) = self.tls(h) {
            let mut t = t.lock().unwrap();
            return match t.write(bytes).and_then(|n| t.flush().map(|_| n)) {
                Ok(n) => n as i32,
                Err(_) => -1,
            };
        }
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
            Some(Sock::Tls(t, raw)) => {
                // close_notify first, if the lock is free (a read waiting on it is woken by the shutdown)
                if let Ok(mut t) = t.try_lock() {
                    t.conn.send_close_notify();
                    let _ = t.flush();
                }
                let _ = raw.shutdown(std::net::Shutdown::Both);
                0
            }
            Some(Sock::Listener(_)) => 0,
            None => -1,
        }
    }
}

/// The TLS client configuration, made once: the Mozilla roots, and the certificates of the PEM file
/// `AIPL_CA_FILE` names, if it is set (for a private authority, or a test), and no client certificate.
fn tls_config() -> Arc<rustls::ClientConfig> {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut roots = rustls::RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            if let Some(file) = std::env::var_os("AIPL_CA_FILE") {
                if let Ok(pem) = std::fs::read(file) {
                    use rustls::pki_types::{pem::PemObject, CertificateDer};
                    for cert in CertificateDer::pem_slice_iter(&pem).flatten() {
                        let _ = roots.add(cert);
                    }
                }
            }
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let config = rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .expect("ring supports the default protocol versions")
                .with_root_certificates(roots)
                .with_no_client_auth();
            Arc::new(config)
        })
        .clone()
}
