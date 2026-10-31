//! Sockets (docs/design/NET_PLAN.md step 2): tests/aipl/net/echo.aipl, a
//! server whose handler is proven to keep no memory, serves four
//! connections over loopback, and every backend answers byte for byte
//! alike: the VM, aipl-run, a native executable (its grant fixed when it
//! was compiled), and the launcher bundle (`--target wasm`). A listen
//! without a grant is refused everywhere with the message naming the flag;
//! a native executable granted another port refuses this one. Both
//! toolchains compile the server to the same bytes.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn echo() -> PathBuf {
    root().join("tests/aipl/net/echo.aipl")
}

/// A port nothing listens on now, for a server a test is about to start,
/// and one no other test here was given. Two causes of empty answers, both
/// found 2026-10-21 to 25 (docs/HANDOFF.md), shape it:
///
/// - It comes from below the range the system takes ports from for bind(0)
///   and for every outgoing connection (/proc/sys/net/ipv4/ip_local_port_range,
///   32768-60999 by default), so the thousands of connections these tests
///   make cannot take it before the server binds it.
/// - It is found free by connecting (refused: nothing listens), never by
///   listening on it. A process another test starts holds a copy of every
///   socket this process has open until it runs its program, so a probe that
///   listened, though closed at once, could stay listening there for a
///   moment; a test whose client connected then had the connection reset and
///   its server never saw it (tools/flake/inherit.py shows it: hundreds of
///   such connects in 20 seconds while processes start, none without).
///
/// The search starts at a point that depends on the process id, so two runs
/// at once start apart.
fn free_port(ip: &str) -> u16 {
    static GIVEN: std::sync::Mutex<Vec<u16>> = std::sync::Mutex::new(Vec::new());
    let low: u16 = std::fs::read_to_string("/proc/sys/net/ipv4/ip_local_port_range")
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse().ok())
        .unwrap_or(32768);
    let (first, span) = (10000u16, low.saturating_sub(10000));
    let mut given = GIVEN.lock().unwrap();
    if span < 1000 {
        // an unusual range: the system's choice, as before (this listens for a
        // moment, the risk the doc comment describes; no CI machine is set up so)
        loop {
            let port = TcpListener::bind((ip, 0)).unwrap().local_addr().unwrap().port();
            if !given.contains(&port) {
                given.push(port);
                return port;
            }
        }
    }
    let start = (u64::from(std::process::id()) * 7919 + given.len() as u64) % u64::from(span);
    for k in 0..u64::from(span) {
        let port = first + ((start + k) % u64::from(span)) as u16;
        if !given.contains(&port) && nothing_listens(ip, port) {
            given.push(port);
            return port;
        }
    }
    panic!("no free port from {first} to {low}");
}

/// Whether nothing listens on ip:port, asked by connecting: refused means
/// nothing does. See free_port for why it does not listen to find out.
fn nothing_listens(ip: &str, port: u16) -> bool {
    let host = if ip.contains(':') { format!("[{ip}]") } else { ip.to_string() };
    let addr: std::net::SocketAddr = format!("{host}:{port}").parse().unwrap();
    matches!(TcpStream::connect_timeout(&addr, Duration::from_millis(200)), Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused)
}

/// What each of the four connections sends, and the reply it must get: the
/// last two alike (a line longer than a reader's first window), so the
/// server's arena must hold no more after the fourth than after the third.
fn exchanges() -> Vec<(Vec<u8>, Vec<u8>)> {
    let long = "x".repeat(20000);
    vec![
        (b"alpha\nbeta\r\ngamma".to_vec(), b"1: alpha\n2: beta\n3: gamma\nbye 3\n".to_vec()),
        (Vec::new(), b"bye 0\n".to_vec()),
        (format!("{long}\nshort\n").into_bytes(), format!("1: {long}\n2: short\nbye 2\n").into_bytes()),
        (format!("{long}\nshort\n").into_bytes(), format!("1: {long}\n2: short\nbye 2\n").into_bytes()),
    ]
}

/// Connects to `at`, retrying while the server starts.
fn connect(at: &str, server: &mut Child) -> TcpStream {
    let start = Instant::now();
    loop {
        if let Ok(s) = TcpStream::connect(at) {
            return s;
        }
        if let Ok(Some(status)) = server.try_wait() {
            panic!("the server exited ({status}) before listening on {at}");
        }
        assert!(start.elapsed() < Duration::from_secs(30), "nothing listens on {at}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Waits for a server that was sent all its requests to stop, for at most
/// two minutes; one that does not is killed and the test fails, rather
/// than holding CI for ever. (A test waited 42 minutes for a server that
/// got fewer connections than were sent, 2026-10-20.)
fn finish(mut server: Child) -> std::process::Output {
    let start = Instant::now();
    while server.try_wait().unwrap().is_none() {
        if start.elapsed() > Duration::from_secs(120) {
            let _ = server.kill();
            let o = server.wait_with_output().unwrap();
            panic!("the server did not stop after its last request: it did not get every connection sent to it\n{}", String::from_utf8_lossy(&o.stderr));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    server.wait_with_output().unwrap()
}

/// Runs the server, holds the four exchanges with it, and returns the
/// replies and what it printed.
fn serve(mut cmd: Command, at: &str) -> (Vec<Vec<u8>>, String) {
    let mut server = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut replies = Vec::new();
    for (send, _) in exchanges() {
        let mut s = connect(at, &mut server);
        s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
        s.write_all(&send).unwrap();
        s.shutdown(Shutdown::Write).unwrap();
        let mut got = Vec::new();
        s.read_to_end(&mut got).unwrap();
        replies.push(got);
    }
    let o = finish(server);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    (replies, String::from_utf8_lossy(&o.stdout).split_inclusive('\n').filter(|l| !l.starts_with("[AIPL")).collect())
}

fn vm(listen: Option<&str>, at: &str) -> Command {
    let mut c = Command::new(AIPL);
    c.arg("eval");
    if let Some(g) = listen {
        c.args(["--listen", g]);
    }
    c.arg(echo()).arg("--").arg(at);
    c
}

fn compile(dir: &Path, name: &str, args: &[&str]) -> PathBuf {
    let out = dir.join(name);
    let o = Command::new(AIPL).arg("compile").args(args).arg(echo()).arg("-o").arg(&out).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    out
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aipl_net_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const PRINTED: &str = "connection 0: 3 lines\nconnection 1: 0 lines\nconnection 2: 2 lines\nconnection 3: 2 lines\narena flat\n";

#[test]
fn every_backend_serves_the_same_bytes() {
    let dir = scratch("serve");
    let wasm = compile(&dir, "echo.wasm", &[]);
    for backend in ["VM", "aipl-run", "native", "bundle"] {
        let at = format!("127.0.0.1:{}", free_port("127.0.0.1"));
        let cmd = match backend {
            "VM" => vm(Some(&at), &at),
            "aipl-run" => {
                let mut c = Command::new(RUNNER);
                c.args(["--listen", &at]).arg(&wasm).arg(&at);
                c
            }
            "native" => {
                let mut c = Command::new(compile(&dir, "echo_native", &["--exe", "--listen", &at]));
                c.arg(&at);
                c
            }
            _ => {
                let mut c = Command::new(compile(&dir, "echo_bundle", &["--exe", "--target", "wasm", "--listen", &at]));
                c.arg(&at);
                c
            }
        };
        let (replies, printed) = serve(cmd, &at);
        for (k, (got, (_, want))) in replies.iter().zip(exchanges()).enumerate() {
            assert!(*got == want, "{backend}, connection {k}: {:?}", String::from_utf8_lossy(got));
        }
        assert_eq!(printed, PRINTED, "{backend}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ipv6_loopback_serves_too() {
    if TcpListener::bind("[::1]:0").is_err() {
        eprintln!("no IPv6 loopback here; skipped");
        return;
    }
    let dir = scratch("v6");
    let at = format!("[::1]:{}", free_port("::1"));
    let mut native = Command::new(compile(&dir, "echo6", &["--exe", "--listen", &at]));
    native.arg(&at);
    for (backend, cmd) in [("VM", vm(Some(&at), &at)), ("native", native)] {
        let (replies, printed) = serve(cmd, &at);
        assert!(replies.iter().zip(exchanges()).all(|(g, (_, w))| *g == w), "{backend}");
        assert_eq!(printed, PRINTED, "{backend}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_listen_not_granted_is_refused_everywhere() {
    let dir = scratch("refused");
    let at = format!("127.0.0.1:{}", free_port("127.0.0.1"));
    let other = format!("127.0.0.1:{}", free_port("127.0.0.1"));
    let wasm = compile(&dir, "echo.wasm", &[]);
    let mut runner = Command::new(RUNNER);
    runner.arg(&wasm).arg(&at);
    let mut plain = Command::new(compile(&dir, "plain", &["--exe"]));
    plain.arg(&at);
    let mut elsewhere = Command::new(compile(&dir, "elsewhere", &["--exe", "--listen", &other]));
    elsewhere.arg(&at);
    for (backend, mut cmd) in [("VM", vm(None, &at)), ("VM, another grant", vm(Some(&other), &at)), ("aipl-run", runner), ("native", plain), ("native, another grant", elsewhere)] {
        let o = cmd.output().unwrap();
        // main's err: the message on standard error, and status 1
        assert_eq!(String::from_utf8_lossy(&o.stderr), "not granted: run the program with --listen ADDR:PORT\n", "{backend}");
        assert_eq!(o.status.code(), Some(1), "{backend}");
    }
    // a malformed grant, before anything is compiled or run
    let o = Command::new(AIPL).args(["eval", "--listen", "127.0.0.1"]).arg(echo()).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&o.stderr).trim(), "Error: --listen 127.0.0.1: expected ADDR:PORT, as 127.0.0.1:8080 or [::1]:8080");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn both_toolchains_compile_the_server_alike() {
    let rust = {
        let m = aipl_core::resolver::Resolver::resolve(&echo()).unwrap();
        aipl_core::checker::TypeChecker::new().check_module(&m).unwrap();
        aipl_core::compiler::wasm::WasmCompiler::compile(&m).unwrap()
    };
    assert_eq!(aipl_core::selfhost::compile_with_aipl(&echo()).unwrap(), rust);
}

// ===== Connecting out (docs/design/NET_PLAN.md step 5): net.connect and http.fetch =====

fn fetch_demo() -> PathBuf {
    root().join("tests/aipl/net/fetch_demo.aipl")
}

/// What a test server answers each connection, in order, and what the client must print.
fn fetch_cases() -> Vec<(&'static str, &'static str)> {
    vec![
        ("HTTP/1.1 200 OK\r\nX-Test: abc\r\nContent-Length: 5\r\n\r\nhello", "status 200 OK\nx-test abc\nbody 5 hello\n"),
        (
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n",
            "status 200 OK\nx-test none\nbody 11 hello world\n",
        ),
        ("HTTP/1.1 404 Not Found\r\nX-Test: n\r\n\r\nmissing", "status 404 Not Found\nx-test n\nbody 7 missing\n"),
        ("HTTP/1.1 302 Found\r\nLocation: /elsewhere\r\nContent-Length: 0\r\n\r\n", "status 302 Found\nx-test none\nbody 0 \n"),
        ("nonsense\r\n\r\n", "error a start line without spaces (400)\n"),
        ("HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc", "error the body ends early (400)\n"),
    ]
}

/// A server in this process on `listener`: for each reply, accepts one connection, reads the request's
/// head, writes the reply, and closes. The handle gives the request heads it saw.
fn http_server_on(listener: TcpListener, replies: Vec<&'static str>) -> std::thread::JoinHandle<Vec<String>> {
    std::thread::spawn(move || {
        let mut heads = Vec::new();
        for reply in replies {
            let (mut s, _) = listener.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") && s.read(&mut byte).map(|n| n == 1).unwrap_or(false) {
                head.push(byte[0]);
            }
            heads.push(String::from_utf8_lossy(&head).into_owned());
            let _ = s.write_all(reply.as_bytes());
            let _ = s.shutdown(Shutdown::Both);
        }
        heads
    })
}

/// A server on a port of the system's choosing.
fn http_server(replies: Vec<&'static str>) -> (std::net::SocketAddr, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let at = listener.local_addr().unwrap();
    (at, http_server_on(listener, replies))
}

/// A backend's name, and how to start fetch_demo on a URL in it.
type Client = (&'static str, Box<dyn Fn(&str) -> Command>);

/// The commands that run fetch_demo on `url`, each backend granted `grants`.
fn fetch_commands(dir: &Path, grants: &[&str]) -> Vec<Client> {
    let wasm = dir.join("fetch.wasm");
    let o = Command::new(AIPL).arg("compile").arg(fetch_demo()).arg("-o").arg(&wasm).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    // a native executable can hold only an address's grant (it has no resolver)
    let is_address = |g: &str| {
        let host = g.rsplit_once(':').map_or(g, |(h, _)| h);
        host.trim_start_matches('[').trim_end_matches(']').parse::<std::net::IpAddr>().is_ok()
    };
    let exe = |name: &str, extra: &[&str]| {
        let out = dir.join(name);
        let mut c = Command::new(AIPL);
        c.args(["compile", "--exe"]).args(extra);
        for g in grants.iter().filter(|g| extra.contains(&"--target") || is_address(g)) {
            c.args(["--allow-connect", g]);
        }
        let o = c.arg(fetch_demo()).arg("-o").arg(&out).output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        out
    };
    let native = exe("fetch_native", &[]);
    let bundle = exe("fetch_bundle", &["--target", "wasm"]);
    let grants: Vec<String> = grants.iter().map(|g| g.to_string()).collect();
    let (g1, g2) = (grants.clone(), grants);
    vec![
        (
            "VM",
            Box::new(move |url: &str| {
                let mut c = Command::new(AIPL);
                c.arg("eval");
                for g in &g1 {
                    c.args(["--allow-connect", g]);
                }
                c.arg(fetch_demo()).arg("--").arg(url);
                c
            }) as Box<dyn Fn(&str) -> Command>,
        ),
        (
            "aipl-run",
            Box::new(move |url: &str| {
                let mut c = Command::new(RUNNER);
                for g in &g2 {
                    c.args(["--allow-connect", g]);
                }
                c.arg(&wasm).arg(url);
                c
            }),
        ),
        ("native", Box::new(move |url: &str| { let mut c = Command::new(&native); c.arg(url); c })),
        ("bundle", Box::new(move |url: &str| { let mut c = Command::new(&bundle); c.arg(url); c })),
    ]
}

/// What a client run printed, less the VM's own status lines.
fn printed(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stdout).split_inclusive('\n').filter(|l| !l.starts_with("[AIPL")).collect()
}

#[test]
fn every_backend_fetches_the_same_bytes() {
    let dir = scratch("fetch");
    let cases = fetch_cases();
    // the grant is fixed in the native executable and the bundle when they are compiled: the server's
    // port is fixed first, by the one listener every backend's client goes to in turn
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let grant = format!("127.0.0.1:{port}");
    let backends = fetch_commands(&dir, &[&grant]);
    for (name, cmd) in &backends {
        let listener = loop {
            match TcpListener::bind(("127.0.0.1", port)) {
                Ok(l) => break l,
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        };
        let handle = http_server_on(listener, cases.iter().map(|(r, _)| *r).collect());
        for (k, (_, want)) in cases.iter().enumerate() {
            let o = cmd(&format!("http://{grant}/p{k}?x=1#frag")).output().unwrap();
            assert_eq!(printed(&o), *want, "{name}, case {k}: {}", String::from_utf8_lossy(&o.stderr));
        }
        let heads = handle.join().unwrap();
        for (k, head) in heads.iter().enumerate() {
            assert_eq!(
                *head,
                format!("GET /p{k}?x=1 HTTP/1.1\r\nHost: {grant}\r\nConnection: close\r\nAccept: */*\r\n\r\n"),
                "{name}, request {k}"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_fetch_that_cannot_happen_says_why_in_every_backend() {
    let (at, server) = http_server(vec!["HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok"]);
    let granted = format!("127.0.0.1:{}", at.port());
    let not_granted = "error not granted: run the program with --allow-connect HOST:PORT (0)\n";
    let no_host = "error host names and TLS are not in native executables: use an address and plain http, run it as a wasm module (aipl eval, aipl-run, compile --exe --target wasm), or put a proxy in front (0)\n";
    // a grant for another port, none at all, a port nothing listens on (granted), a name that was not
    // granted (a native executable has no resolver at all), another scheme, and https (which needs a
    // host's TLS, and fails before it connects where there is none)
    let silent = format!("127.0.0.1:{}", free_port("127.0.0.1"));
    // (grant, url, what the hosts with a resolver and TLS print, what a native executable prints)
    let cases: Vec<(Option<&str>, String, &str, &str)> = vec![
        (Some(&granted), format!("http://127.0.0.1:{}/", at.port() + 1), not_granted, not_granted),
        (None, format!("http://{granted}/"), not_granted, not_granted),
        (Some("127.0.0.1:*"), format!("http://{silent}/"), "error refused by the system (0)\n", "error refused by the system (0)\n"),
        (Some(&granted), "http://example.com/".to_string(), not_granted, no_host),
        (Some(&granted), "ftp://127.0.0.1/".to_string(), "error not an http:// or https:// URL (0)\n", "error not an http:// or https:// URL (0)\n"),
        (Some(&granted), "http:///x".to_string(), "error a URL without a host (0)\n", "error a URL without a host (0)\n"),
        (None, format!("https://{granted}/"), not_granted, no_host),
    ];
    for (n, (grant, url, want_host, want_native)) in cases.iter().enumerate() {
        let sub = scratch(&format!("fetch_refused_{n}"));
        for (name, cmd) in fetch_commands(&sub, &grant.iter().copied().collect::<Vec<_>>()) {
            let o = cmd(url).output().unwrap();
            let want = if name == "native" { want_native } else { want_host };
            assert_eq!(printed(&o), *want, "{name}, {url}: {}", String::from_utf8_lossy(&o.stderr));
        }
        let _ = std::fs::remove_dir_all(&sub);
    }
    // the one connection the server was waiting for: a wildcard grant lets the same client through
    let sub = scratch("fetch_wild");
    for (name, cmd) in fetch_commands(&sub, &["127.0.0.1:*"]).into_iter().take(1) {
        let o = cmd(&format!("http://{granted}/")).output().unwrap();
        assert_eq!(printed(&o), "status 200 OK\nx-test none\nbody 2 ok\n", "{name}");
    }
    let _ = server.join();
    let _ = std::fs::remove_dir_all(&sub);
}

#[test]
fn a_malformed_connect_grant_is_refused_before_anything_runs() {
    for bad in ["127.0.0.1", "127.0.0.1:99999", "[::1]:x", "127.0.0.1:*:*", "bad-.example:80", "999.1.1.1:80", "[example.com]:80", "example.com:"] {
        let o = Command::new(AIPL).args(["eval", "--allow-connect", bad]).arg(fetch_demo()).output().unwrap();
        assert_eq!(
            String::from_utf8_lossy(&o.stderr).trim(),
            format!("Error: --allow-connect {bad}: expected HOST:PORT or HOST:*, as 127.0.0.1:8080, [::1]:*, or example.com:443"),
            "{bad}"
        );
        let o = Command::new(AIPL).args(["compile", "--exe", "--allow-connect", bad]).arg(fetch_demo()).args(["-o", "/dev/null"]).output().unwrap();
        assert_eq!(
            String::from_utf8_lossy(&o.stderr).trim(),
            format!("Error: --allow-connect {bad}: expected HOST:PORT or HOST:*, as 127.0.0.1:8080, [::1]:*, or example.com:443"),
            "{bad}"
        );
    }
}

#[test]
fn both_toolchains_compile_the_client_alike() {
    let rust = {
        let m = aipl_core::resolver::Resolver::resolve(&fetch_demo()).unwrap();
        aipl_core::checker::TypeChecker::new().check_module(&m).unwrap();
        aipl_core::compiler::wasm::WasmCompiler::compile(&m).unwrap()
    };
    assert_eq!(aipl_core::selfhost::compile_with_aipl(&fetch_demo()).unwrap(), rust);
}

// ===== Host names and TLS (NET_PLAN step 6, the clients): the Rust hosts only =====

/// A TLS server in this process for `names` (its certificate's subject names), accepting one connection
/// for each reply: reads the request's head, writes the reply, and closes. Returns its port, the
/// certificate as a PEM file (what AIPL_CA_FILE names), and how many handshakes it finished. With
/// `tls` false it speaks plain HTTP on the same kind of port.
fn tls_server(dir: &Path, names: &[&str], replies: Vec<&'static str>, tls: bool) -> (u16, PathBuf, std::thread::JoinHandle<usize>) {
    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
    let key = rcgen::generate_simple_self_signed(names.iter().map(|n| n.to_string()).collect::<Vec<_>>()).unwrap();
    let pem = dir.join(format!("ca_{}.pem", names.join("_")));
    std::fs::write(&pem, key.cert.pem()).unwrap();
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let config = std::sync::Arc::new(
        rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![key.cert.der().clone()], PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.signing_key.serialize_der())))
            .unwrap(),
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let mut finished = 0;
        for reply in replies {
            let (tcp, _) = listener.accept().unwrap();
            tcp.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
            let mut stream: Box<dyn ReadWrite> = if tls {
                let conn = rustls::ServerConnection::new(config.clone()).unwrap();
                Box::new(rustls::StreamOwned::new(conn, tcp))
            } else {
                Box::new(tcp)
            };
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).map(|n| n == 1).unwrap_or(false) {
                head.push(byte[0]);
            }
            if head.ends_with(b"\r\n\r\n") {
                finished += 1;
                let _ = stream.write_all(reply.as_bytes());
                let _ = stream.flush();
            }
        }
        finished
    });
    (port, pem, handle)
}

trait ReadWrite: Read + Write + Send {}
impl<T: Read + Write + Send> ReadWrite for T {}

const TLS_REPLY: &str = "HTTP/1.1 200 OK\r\nX-Test: secure\r\nContent-Length: 5\r\n\r\nhello";
const TLS_ANSWER: &str = "status 200 OK\nx-test secure\nbody 5 hello\n";
const HANDSHAKE_FAILED: &str = "error the TLS handshake failed: an untrusted or mismatched certificate, or a server that does not speak TLS (0)\n";

#[test]
fn https_and_names_work_in_the_rust_hosts() {
    let dir = scratch("tls");
    // a certificate for the name and for the address: each URL below must be accepted
    let (port, pem, server) = tls_server(&dir, &["localhost", "127.0.0.1"], vec![TLS_REPLY; 6], true);
    let (name_grant, ip_grant) = (format!("localhost:{port}"), format!("127.0.0.1:{port}"));
    let backends = fetch_commands(&dir, &[&name_grant, &ip_grant]);
    for (name, cmd) in backends.iter().filter(|(n, _)| *n != "native") {
        for url in [format!("https://localhost:{port}/p?x=1"), format!("https://127.0.0.1:{port}/p?x=1")] {
            let o = cmd(&url).env("AIPL_CA_FILE", &pem).output().unwrap();
            assert_eq!(printed(&o), TLS_ANSWER, "{name}, {url}: {}", String::from_utf8_lossy(&o.stderr));
        }
    }
    // the same bytes of request the plain fetches send, over TLS
    assert_eq!(server.join().unwrap(), 6);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_certificate_that_is_not_trusted_or_not_for_the_name_is_refused() {
    let dir = scratch("tls_bad");
    // untrusted: the certificate is not in the roots (no AIPL_CA_FILE)
    let (port, pem, server) = tls_server(&dir, &["localhost"], vec![TLS_REPLY], true);
    let grant = format!("localhost:{port}");
    for (name, cmd) in fetch_commands(&dir, &[&grant]).iter().filter(|(n, _)| *n != "native") {
        // the server's single connection is spent by the first client; the rest find the port closed
        let o = cmd(&format!("https://localhost:{port}/")).output().unwrap();
        assert!(printed(&o) == HANDSHAKE_FAILED || printed(&o).starts_with("error refused"), "{name}: {} {}", printed(&o), String::from_utf8_lossy(&o.stderr));
        if name == &"VM" {
            assert_eq!(printed(&o), HANDSHAKE_FAILED, "{name}");
        }
    }
    let _ = server.join();
    let _ = pem;
    // trusted but for another name: refused where the URL names localhost
    let (port, pem, server) = tls_server(&dir, &["other.example"], vec![TLS_REPLY], true);
    let grant = format!("localhost:{port}");
    let sub = scratch("tls_bad_name");
    let (_, cmd) = fetch_commands(&sub, &[&grant]).into_iter().next().unwrap();
    let o = cmd(&format!("https://localhost:{port}/")).env("AIPL_CA_FILE", &pem).output().unwrap();
    assert_eq!(printed(&o), HANDSHAKE_FAILED);
    assert_eq!(server.join().unwrap(), 0);
    // a server that does not speak TLS
    let (port, pem, server) = tls_server(&dir, &["localhost"], vec![TLS_REPLY], false);
    let grant = format!("localhost:{port}");
    let (_, cmd) = fetch_commands(&sub, &[&grant]).into_iter().next().unwrap();
    let o = cmd(&format!("https://localhost:{port}/")).env("AIPL_CA_FILE", &pem).output().unwrap();
    assert_eq!(printed(&o), HANDSHAKE_FAILED);
    let _ = server.join();
    let _ = std::fs::remove_dir_all(&sub);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_name_is_granted_by_name_and_plain_http_to_one_works() {
    let dir = scratch("names");
    let (at, server) = http_server(vec!["HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok"; 2]);
    let port = at.port();
    let refused = "error not granted: run the program with --allow-connect HOST:PORT (0)\n";
    // a grant for the address does not let the program connect by name, nor the other way round; the
    // name matches ignoring case; a name that is not a resolvable one is its own error
    for (name, cmd) in fetch_commands(&dir, &[&format!("127.0.0.1:{port}")]).iter().take(2) {
        let o = cmd(&format!("http://localhost:{port}/")).output().unwrap();
        assert_eq!(printed(&o), refused, "{name}, address granted, name used");
    }
    let sub = scratch("names_by_name");
    for (name, cmd) in fetch_commands(&sub, &[&format!("LocalHost:{port}")]).iter().take(2) {
        let o = cmd(&format!("http://127.0.0.1:{port}/")).output().unwrap();
        assert_eq!(printed(&o), refused, "{name}, name granted, address used");
        let o = cmd(&format!("http://localhost:{port}/")).output().unwrap();
        assert_eq!(printed(&o), "status 200 OK\nx-test none\nbody 2 ok\n", "{name}, name granted, name used");
    }
    let _ = server.join();
    let nowhere = "no-such-host.invalid";
    let (_, cmd) = fetch_commands(&sub, &[&format!("{nowhere}:80")]).into_iter().next().unwrap();
    let o = cmd(&format!("http://{nowhere}/")).output().unwrap();
    assert_eq!(printed(&o), "error the name did not resolve (0)\n");
    let _ = std::fs::remove_dir_all(&sub);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_native_executable_says_it_has_no_names_or_tls() {
    let dir = scratch("native_no_tls");
    let (_, cmd) = fetch_commands(&dir, &["127.0.0.1:*"]).into_iter().find(|(n, _)| *n == "native").unwrap();
    let want = "error host names and TLS are not in native executables: use an address and plain http, run it as a wasm module (aipl eval, aipl-run, compile --exe --target wasm), or put a proxy in front (0)\n";
    for url in ["https://127.0.0.1:9/", "http://localhost:9/", "https://example.com/"] {
        assert_eq!(printed(&cmd(url).output().unwrap()), want, "{url}");
    }
    // a name grant cannot be fixed in a native executable
    let o = Command::new(AIPL).args(["compile", "--exe", "--allow-connect", "example.com:443"]).arg(fetch_demo()).arg("-o").arg(dir.join("x")).output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&o.stderr).trim(),
        "Error: --allow-connect example.com:443: a native executable has no name resolver: grant an address (127.0.0.1:8080, [::1]:*), or build with --target wasm"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
