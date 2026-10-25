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
