//! std/serve, the long-running server (docs/design/NET_PLAN.md step 3, and
//! MEMORY_PLAN.md M5): tests/aipl/net/serve_demo.aipl answers real
//! requests over loopback with the same bytes in the VM, under aipl-run,
//! natively, and in the launcher bundle; requests std/http refuses get the
//! same status everywhere (400, 413, 431); 100,000 requests natively and
//! under aipl-run (a million with AIPL_MILLION=1, which passed 2026-10-20;
//! 20,000 in the VM) leave the heap exactly where it was after the first
//! thousand; a handler that keeps a request's view past
//! the request stops with "Use after free" on its next use, in every
//! backend; both toolchains compile the server to the same bytes.

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

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aipl_serve_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn compile(src: &Path, out: &Path, args: &[&str]) {
    let o = Command::new(AIPL).arg("compile").args(args).arg(src).arg("-o").arg(out).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

/// The program `src` started in `backend` with a grant for `at` and these
/// arguments after the address.
fn start(backend: &str, src: &Path, dir: &Path, at: &str, args: &[&str]) -> Child {
    let mut cmd = match backend {
        "VM" => {
            let mut c = Command::new(AIPL);
            c.args(["eval", "--listen", at]).arg(src).arg("--");
            c
        }
        "aipl-run" => {
            let wasm = dir.join("p.wasm");
            compile(src, &wasm, &[]);
            let mut c = Command::new(RUNNER);
            c.args(["--listen", at]).arg(wasm);
            c
        }
        "native" => {
            let exe = dir.join("p");
            compile(src, &exe, &["--exe", "--listen", at]);
            Command::new(exe)
        }
        _ => {
            let exe = dir.join("p_bundle");
            compile(src, &exe, &["--exe", "--target", "wasm", "--listen", at]);
            Command::new(exe)
        }
    };
    cmd.arg(at).args(args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap()
}

/// Sends `request` on a new connection (then stops sending) and reads the
/// answer to its end; waits for the server to listen first.
fn exchange(at: &str, server: &mut Child, request: &[u8]) -> Vec<u8> {
    let start = Instant::now();
    let mut s = loop {
        if let Ok(s) = TcpStream::connect(at) {
            break s;
        }
        if let Ok(Some(st)) = server.try_wait() {
            panic!("the server exited ({st}) before listening on {at}");
        }
        assert!(start.elapsed() < Duration::from_secs(60), "nothing listens on {at}");
        std::thread::sleep(Duration::from_millis(20));
    };
    s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    s.write_all(request).unwrap();
    let _ = s.shutdown(Shutdown::Write);
    let mut got = Vec::new();
    let _ = s.read_to_end(&mut got);
    got
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

fn body(r: &[u8]) -> String {
    let t = String::from_utf8_lossy(r).to_string();
    t.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or(t)
}

fn text_response(status: &str, body: &str) -> Vec<u8> {
    format!("HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
}

fn refusal(status: &str, why: &str) -> Vec<u8> {
    format!("HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{why}", why.len()).into_bytes()
}

/// Each request and the exact answer it gets; None for a connection that
/// closes before sending anything, which is not answered (nor counted).
fn exchanges() -> Vec<(Vec<u8>, Option<Vec<u8>>)> {
    let many_headers: String = (0..101).map(|i| format!("X-{i}: v\r\n")).collect();
    let long_head = format!("GET / HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(70000));
    vec![
        (b"GET / HTTP/1.1\r\nHost: x\r\n\r\n".to_vec(), Some(text_response("200 OK", "hello\n"))),
        (b"GET /echo?a=1&b=2 HTTP/1.1\r\n\r\n".to_vec(), Some(text_response("200 OK", "/echo?a=1&b=2"))),
        (b"POST /length HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello".to_vec(), Some(text_response("200 OK", "5 bytes\n"))),
        (b"POST /length HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n".to_vec(), Some(text_response("200 OK", "5 bytes\n"))),
        (b"GET /count HTTP/1.1\r\n\r\n".to_vec(), Some(text_response("200 OK", "4 before this one\n"))),
        (b"GET /nowhere HTTP/1.1\r\n\r\n".to_vec(), Some(text_response("404 Not Found", "not found\n"))),
        (b"GARBAGE\r\n\r\n".to_vec(), Some(refusal("400 Bad Request", "a start line without spaces"))),
        (b"GET / HTTP/1.1\r\nHost".to_vec(), Some(refusal("400 Bad Request", "the head ends early"))),
        (b"POST /length HTTP/1.1\r\nContent-Length: 999999999\r\n\r\nx".to_vec(), Some(refusal("413 Content Too Large", "a body over the limit"))),
        (format!("GET / HTTP/1.1\r\n{many_headers}\r\n").into_bytes(), Some(refusal("431 Request Header Fields Too Large", "more headers than the limit"))),
        (long_head.into_bytes(), Some(refusal("431 Request Header Fields Too Large", "a head over the limit"))),
        (Vec::new(), None),
        // the handler counts what reaches it: the refusals never do
        (b"GET /count HTTP/1.1\r\n\r\n".to_vec(), Some(text_response("200 OK", "6 before this one\n"))),
    ]
}

#[test]
fn every_backend_answers_alike() {
    let src = root().join("tests/aipl/net/serve_demo.aipl");
    let answered = exchanges().iter().filter(|(_, w)| w.is_some()).count();
    for backend in ["VM", "aipl-run", "native", "bundle"] {
        let dir = scratch(&format!("answers_{backend}"));
        let at = format!("127.0.0.1:{}", free_port("127.0.0.1"));
        let mut server = start(backend, &src, &dir, &at, &[&answered.to_string()]);
        for (k, (req, want)) in exchanges().into_iter().enumerate() {
            let got = exchange(&at, &mut server, &req);
            match want {
                Some(w) => assert!(got == w, "{backend}, request {k}: {:?}", String::from_utf8_lossy(&got)),
                None => assert!(got.is_empty(), "{backend}, request {k}: an empty request has no answer"),
            }
        }
        let o = finish(server);
        assert!(o.status.success(), "{backend}: {}", String::from_utf8_lossy(&o.stderr));
        let printed: String = String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.starts_with("[AIPL")).collect();
        // 64 KiB to start, and chunks for the 70,000-byte head as its
        // buffer grows, which depends on how its bytes arrive (262,160
        // when they come at once, 458,784 seen under load): a bound, not
        // a number
        let arena: usize = printed
            .strip_prefix(&format!("served {answered}, arena "))
            .and_then(|r| r.strip_suffix(" bytes"))
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("{backend}: {printed}"));
        assert!((65_552..=8 * 65_552).contains(&arena), "{backend}: {printed}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// `n` mixed requests (echo targets and bodies of every size up to a few
/// hundred bytes, and 404s) after a thousand: the top of the heap is the
/// same before and after.
fn memory_stays_flat(backend: &str, n: usize) {
    let src = root().join("tests/aipl/net/serve_demo.aipl");
    let dir = scratch(&format!("flat_{backend}"));
    let at = format!("127.0.0.1:{}", free_port("127.0.0.1"));
    let total = 1000 + n + 2;
    let mut server = start(backend, &src, &dir, &at, &[&total.to_string()]);
    let request = |k: usize| -> Vec<u8> {
        match k % 3 {
            0 => format!("GET /echo?{} HTTP/1.1\r\n\r\n", "y".repeat(k % 211)).into_bytes(),
            1 => format!("POST /length HTTP/1.1\r\nContent-Length: {}\r\n\r\n{}", k % 307, "x".repeat(k % 307)).into_bytes(),
            _ => b"GET /missing HTTP/1.1\r\n\r\n".to_vec(),
        }
    };
    for k in 0..1000 {
        exchange(&at, &mut server, &request(k));
    }
    let before = body(&exchange(&at, &mut server, b"GET /memory HTTP/1.1\r\n\r\n"));
    for k in 0..n {
        let got = exchange(&at, &mut server, &request(k));
        assert!(got.starts_with(b"HTTP/1.1 "), "{backend}, request {k}");
    }
    let after = body(&exchange(&at, &mut server, b"GET /memory HTTP/1.1\r\n\r\n"));
    assert!(before.starts_with("heap top "), "{backend}: {before}");
    assert_eq!(after, before, "{backend}: the heap grew over {n} requests");
    let o = finish(server);
    assert!(o.status.success(), "{backend}: {}", String::from_utf8_lossy(&o.stderr));
    let _ = std::fs::remove_dir_all(&dir);
}

/// 100,000 requests, or a million with AIPL_MILLION=1 (about 40 seconds
/// each on a quiet machine): the heap either stays put or it does not, and
/// a hundred thousand shows which as surely, in CI's time.
fn many() -> usize {
    if std::env::var("AIPL_MILLION").is_ok_and(|v| v == "1") { 1_000_000 } else { 100_000 }
}

#[test]
fn many_requests_leave_memory_flat_natively() {
    memory_stays_flat("native", many());
}

#[test]
fn many_requests_leave_memory_flat_under_aipl_run() {
    memory_stays_flat("aipl-run", many());
}

#[test]
fn twenty_thousand_requests_leave_memory_flat_in_the_vm() {
    memory_stays_flat("VM", 20_000);
}

#[test]
fn a_view_kept_past_its_request_stops_the_program() {
    let src = root().join("tests/aipl/net/serve_keep.aipl");
    for backend in ["VM", "aipl-run", "native"] {
        let dir = scratch(&format!("keep_{backend}"));
        let at = format!("127.0.0.1:{}", free_port("127.0.0.1"));
        let mut server = start(backend, &src, &dir, &at, &[]);
        let first = exchange(&at, &mut server, b"GET /first HTTP/1.1\r\n\r\n");
        if !first.ends_with(b"/first") {
            // what the server said: an empty answer under load, once (2026-10-20), cause unknown
            let _ = server.kill();
            let o = server.wait_with_output().unwrap();
            panic!("{backend}: {:?}\nserver ({}): {}{}", String::from_utf8_lossy(&first), o.status, String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        }
        let second = exchange(&at, &mut server, b"GET /second HTTP/1.1\r\n\r\n");
        assert!(second.is_empty(), "{backend}: the second request is not answered");
        let o = finish(server);
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(!o.status.success() && err.contains("Use after free"), "{backend}: {err}");
        assert!(err.contains("serve_keep.aipl:20:41"), "{backend}: the trap names the handler's read: {err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn both_toolchains_compile_the_server_alike() {
    for name in ["serve_demo", "serve_keep"] {
        let src = root().join(format!("tests/aipl/net/{name}.aipl"));
        let m = aipl_core::resolver::Resolver::resolve(&src).unwrap();
        aipl_core::checker::TypeChecker::new().check_module(&m).unwrap();
        let rust = aipl_core::compiler::wasm::WasmCompiler::compile(&m).unwrap();
        assert_eq!(aipl_core::selfhost::compile_with_aipl(&src).unwrap(), rust, "{name}");
    }
}

// ---- A fresh instance per request (NET_PLAN.md step 4) ----

/// serve.once answers one request from standard input on standard output,
/// alike in the VM, under aipl-run, and natively: a saved request piped
/// through it, no network needed. A trap stops it with its message; input
/// that ends before a request begins gives status 1.
#[test]
fn once_answers_a_piped_request_alike_everywhere() {
    let src = root().join("tests/aipl/net/fresh_demo.aipl");
    let dir = scratch("once");
    let (wasm, exe) = (dir.join("p.wasm"), dir.join("p"));
    compile(&src, &wasm, &[]);
    compile(&src, &exe, &["--exe"]);
    let pipe = |mut c: Command, input: &[u8]| -> (Option<i32>, String, String) {
        let mut child = c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        let o = child.wait_with_output().unwrap();
        // the VM's banner comes first, on its own line, and its result line
        // last, after the program's output
        let all = String::from_utf8_lossy(&o.stdout).to_string();
        let body = match all.split_once('\n') {
            Some((first, rest)) if first.starts_with("[AIPL VM]") => rest,
            _ => &all,
        };
        let out = body.rfind("[AIPL Result]:").map_or(body, |i| &body[..i]).to_string();
        (o.status.code(), out, String::from_utf8_lossy(&o.stderr).to_string())
    };
    let cases: [(&[u8], &str); 4] = [
        (b"GET /echo?a=1 HTTP/1.1\r\nHost: x\r\n\r\n", "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 9\r\n\r\n/echo?a=1"),
        (b"GET /nowhere HTTP/1.1\r\n\r\n", "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 10\r\n\r\nnot found\n"),
        (b"GARBAGE\r\n\r\n", "HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain; charset=utf-8\r\nConnection: close\r\nContent-Length: 27\r\n\r\na start line without spaces"),
        (b"", ""),
    ];
    for backend in ["VM", "aipl-run", "native"] {
        let cmd = || match backend {
            "VM" => {
                let mut c = Command::new(AIPL);
                c.arg("eval").arg(&src);
                c
            }
            "aipl-run" => {
                let mut c = Command::new(RUNNER);
                c.arg(&wasm);
                c
            }
            _ => Command::new(&exe),
        };
        for (input, want) in cases {
            let (status, out, err) = pipe(cmd(), input);
            assert_eq!(out, want, "{backend}: {:?}", String::from_utf8_lossy(input));
            assert_eq!(status, Some(if input.is_empty() { 1 } else { 0 }), "{backend}: {err}");
        }
        let (status, out, err) = pipe(cmd(), b"GET /crash HTTP/1.1\r\n\r\n");
        assert!(status != Some(0) && out.is_empty(), "{backend}: the crash writes nothing");
        assert!(err.contains("Array index out of bounds: index 5 for array of length 1") && err.contains("at handle (fresh_demo.aipl:32:54)"), "{backend}: {err}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The native backend targets Linux x86-64 only (AIPL_SPEC.md 6.6).
const NATIVE: bool = cfg!(all(target_os = "linux", target_arch = "x86_64"));

/// A fresh instance per connection, from each host: `aipl serve --fresh`
/// (aipl-run), a native executable built with `--serve`, which forks a copy
/// of itself per connection, and the launcher bundle built with it. A
/// handler that traps answers 500 (its message and call chain go to the
/// server's standard error), and the next request is served; so does an
/// answer longer than a host keeps, the server saying why.
#[test]
fn a_fresh_instance_per_request_isolates_a_crash() {
    let src = root().join("tests/aipl/net/fresh_demo.aipl");
    let dir = scratch("fresh");
    let failed = b"HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/plain; charset=utf-8\r\nConnection: close\r\nContent-Length: 22\r\n\r\nInternal Server Error\n".to_vec();
    // /big's answer is more than the 64 MiB a host keeps: 500, not part of it
    let requests: [(&str, Vec<u8>); 5] = [
        ("/", text_response("200 OK", "hello\n")),
        ("/crash", failed.clone()),
        ("/echo?after=crash", text_response("200 OK", "/echo?after=crash")),
        ("/big", failed),
        ("/", text_response("200 OK", "hello\n")),
    ];
    let hosts: &[&str] = if NATIVE {
        &["aipl serve --fresh", "native", "bundle"]
    } else {
        &["aipl serve --fresh", "bundle"]
    };
    for &host in hosts {
        let at = format!("127.0.0.1:{}", free_port("127.0.0.1"));
        let mut cmd = match host {
            "native" | "bundle" => {
                let exe = dir.join(host);
                let target = if host == "native" { "native" } else { "wasm" };
                compile(&src, &exe, &["--exe", "--target", target, "--serve", &at]);
                Command::new(exe)
            }
            _ => {
                let mut c = Command::new(AIPL);
                c.args(["serve", "--fresh", "--listen", &at, "--requests", "5"]).arg(&src);
                c
            }
        };
        let mut server = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        for (path, want) in &requests {
            let got = exchange(&at, &mut server, format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes());
            assert!(got == *want, "{host} {path}: {:?}", String::from_utf8_lossy(&got));
        }
        // the executables serve until they are ended; aipl serve stops after --requests
        if host != "aipl serve --fresh" {
            server.kill().unwrap();
        }
        let o = finish(server);
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(host != "aipl serve --fresh" || o.status.success(), "{host}: {err}");
        assert!(err.contains(": Array index out of bounds: index 5 for array of length 1\n  at handle (fresh_demo.aipl:32:54)\n"), "{host}: {err}");
        assert_eq!(err.matches("Array index").count(), 1, "{host}: {err}");
        // and why /big's answer was not sent
        assert_eq!(err.matches(aipl_core::net::FRESH_TOO_LONG).count(), 1, "{host}: {err}");
    }
    // without --fresh, the way to a long-running server
    let o = Command::new(AIPL).args(["serve", "--listen", "127.0.0.1:1"]).arg(&src).output().unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("add --fresh"), "{}", String::from_utf8_lossy(&o.stderr));
    let _ = std::fs::remove_dir_all(&dir);
}

/// An address that cannot be listened on stops each host with the same
/// message and status 1; an address that is not one is refused when the
/// executable is compiled, and --serve needs --exe.
#[test]
fn a_fresh_server_that_cannot_listen_says_so() {
    let src = root().join("tests/aipl/net/fresh_demo.aipl");
    let dir = scratch("fresh_busy");
    // a port held busy here: no gap in which another socket could take it
    let busy = TcpListener::bind("127.0.0.1:0").unwrap();
    let at = busy.local_addr().unwrap().to_string();
    let err = TcpListener::bind(&at).unwrap_err();
    let why = format!(": cannot listen on {at}: {err}\n");
    let (native, bundle) = (dir.join("native"), dir.join("bundle"));
    if NATIVE {
        compile(&src, &native, &["--exe", "--target", "native", "--serve", &at]);
    }
    compile(&src, &bundle, &["--exe", "--target", "wasm", "--serve", &at]);
    let mut serve = Command::new(AIPL);
    serve.args(["serve", "--fresh", "--listen", &at]).arg(&src);
    let mut targets: Vec<(String, Command)> = Vec::new();
    if NATIVE {
        targets.push((native.display().to_string(), Command::new(&native)));
    }
    targets.push((bundle.display().to_string(), Command::new(&bundle)));
    targets.push((src.display().to_string(), serve));
    for (name, mut cmd) in targets {
        let o = cmd.output().unwrap();
        assert_eq!((o.status.code(), String::from_utf8_lossy(&o.stderr).to_string()), (Some(1), format!("{name}{why}")));
    }
    drop(busy);
    let o = Command::new(AIPL).args(["compile", "--exe", "--serve", "127.0.0.1"]).arg(&src).arg("-o").arg(dir.join("x")).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&o.stderr).trim(), "Error: --serve 127.0.0.1: expected ADDR:PORT, as 127.0.0.1:8080 or [::1]:8080");
    let o = Command::new(AIPL).args(["compile", "--serve", "127.0.0.1:1"]).arg(&src).arg("-o").arg(dir.join("x")).output().unwrap();
    assert!(!o.status.success() && String::from_utf8_lossy(&o.stderr).contains("--exe"));
    // a threaded program: refused when compiled, natively and as a bundle,
    // leaving no file, and by aipl serve
    let par = root().join("examples/spectralnorm_par.aipl");
    let threaded = "a threaded module cannot be served a fresh instance per connection yet";
    let targets: &[&str] = if NATIVE { &["native", "wasm"] } else { &["wasm"] };
    for target in targets {
        let out = dir.join(format!("par_{target}"));
        let o = Command::new(AIPL).args(["compile", "--exe", "--target", target, "--serve", "127.0.0.1:1"]).arg(&par).arg("-o").arg(&out).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&o.stderr).trim(), format!("Error: {threaded}"), "{target}");
        assert!(!out.exists(), "{target}");
    }
    let o = Command::new(AIPL).args(["serve", "--fresh", "--listen", "127.0.0.1:1"]).arg(&par).output().unwrap();
    assert_eq!((o.status.code(), String::from_utf8_lossy(&o.stderr).to_string()), (Some(1), format!("{}: {threaded}\n", par.display())));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn both_toolchains_compile_the_fresh_server_alike() {
    let src = root().join("tests/aipl/net/fresh_demo.aipl");
    let m = aipl_core::resolver::Resolver::resolve(&src).unwrap();
    aipl_core::checker::TypeChecker::new().check_module(&m).unwrap();
    let rust = aipl_core::compiler::wasm::WasmCompiler::compile(&m).unwrap();
    assert_eq!(aipl_core::selfhost::compile_with_aipl(&src).unwrap(), rust);
}

/// `aipl serve --fresh` becomes the aipl-run that serves (it does not start
/// one and wait), so ending it by its process id ends the server: nothing
/// is left listening, and its temporary module is gone.
#[test]
fn ending_aipl_serve_ends_the_server() {
    let src = root().join("tests/aipl/net/fresh_demo.aipl");
    let at = format!("127.0.0.1:{}", free_port("127.0.0.1"));
    let mut server = Command::new(AIPL).args(["serve", "--fresh", "--listen", &at]).arg(&src).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let pid = server.id();
    let start = std::time::Instant::now();
    while std::net::TcpStream::connect(&at).is_err() {
        assert!(start.elapsed() < std::time::Duration::from_secs(60), "the server never listened");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    server.kill().unwrap();
    server.wait().unwrap();
    assert!(std::net::TcpStream::connect(&at).is_err(), "something is still listening on {at}");
    assert!(!std::env::temp_dir().join(format!("aipl_serve_{pid}")).exists());
}

/// `aipl serve --fresh` in the self-hosted aipl (aipl_src/servecmd.aipl):
/// the program built as a native executable that serves on the address and
/// removes itself, which this command becomes. The same answers as the
/// others give, a trap answering 500 and the next request served, an answer
/// past what a host keeps said so, the server stopping by itself after
/// `--requests` (0 being none), nothing left in the temporary directory, and
/// the same messages for an address that cannot be listened on and a missing
/// `--fresh`.
#[test]
fn the_self_hosted_aipl_serves_a_fresh_instance_per_request() {
    if !NATIVE {
        return;
    }
    let src = root().join("tests/aipl/net/fresh_demo.aipl");
    let dir = scratch("fresh_selfhosted");
    let me = dir.join("aipl");
    let o = Command::new(AIPL).args(["compile", "--exe", "--allow-run", "*", "aipl_src/aipl.aipl", "-o"]).arg(&me).current_dir(root()).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let tmp = dir.join("tmp");
    std::fs::create_dir(&tmp).unwrap();
    // `aipl serve ARGS FILE`, in the repository's root, with the library and a temporary directory of its own
    let serve = |args: &[&str]| {
        let mut c = Command::new(&me);
        c.arg("serve").args(args).arg(&src).current_dir(root()).env("AIPL_STD", root().join("aipl_src/std/")).env("TMPDIR", &tmp);
        c
    };
    let failed = b"HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/plain; charset=utf-8\r\nConnection: close\r\nContent-Length: 22\r\n\r\nInternal Server Error\n".to_vec();
    let requests: [(&str, Vec<u8>); 5] = [
        ("/", text_response("200 OK", "hello\n")),
        ("/crash", failed.clone()),
        ("/echo?after=crash", text_response("200 OK", "/echo?after=crash")),
        ("/big", failed),
        ("/", text_response("200 OK", "hello\n")),
    ];
    let at = format!("127.0.0.1:{}", free_port("127.0.0.1"));
    let mut server = serve(&["--fresh", "--listen", &at, "--requests", "5"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    for (path, want) in &requests {
        let got = exchange(&at, &mut server, format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes());
        assert!(got == *want, "{path}: {:?}", String::from_utf8_lossy(&got));
    }
    let o = finish(server);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success(), "{err}");
    assert!(err.contains(": Array index out of bounds: index 5 for array of length 1\n  at handle (fresh_demo.aipl:32:54)\n"), "{err}");
    assert_eq!(err.matches("Array index").count(), 1, "{err}");
    assert_eq!(err.matches(aipl_core::net::FRESH_TOO_LONG).count(), 1, "{err}");
    assert_eq!(std::fs::read_dir(&tmp).unwrap().count(), 0, "the temporary executable was left");

    // no connection to answer: it listens, and ends
    let at0 = format!("127.0.0.1:{}", free_port("127.0.0.1"));
    let o = serve(&["--fresh", "--listen", &at0, "--requests", "0"]).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(std::fs::read_dir(&tmp).unwrap().count(), 0, "the temporary executable was left");

    // an address that cannot be listened on: the message the others give, status 1
    let busy = TcpListener::bind("127.0.0.1:0").unwrap();
    let held = busy.local_addr().unwrap().to_string();
    let why = TcpListener::bind(&held).unwrap_err();
    let o = serve(&["--fresh", "--listen", &held]).output().unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).ends_with(&format!(": cannot listen on {held}: {why}\n")), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(std::fs::read_dir(&tmp).unwrap().count(), 0, "the temporary executable was left");

    // without --fresh, the way to a long-running server; a bad address; usage
    let o = serve(&["--listen", "127.0.0.1:1"]).output().unwrap();
    assert!(o.status.code() == Some(1) && String::from_utf8_lossy(&o.stderr).contains("add --fresh"), "{}", String::from_utf8_lossy(&o.stderr));
    let o = serve(&["--fresh", "--listen", "127.0.0.1"]).output().unwrap();
    assert!(o.status.code() == Some(1) && String::from_utf8_lossy(&o.stderr).contains("expected ADDR:PORT"), "{}", String::from_utf8_lossy(&o.stderr));
    for args in [&["serve"][..], &["serve", "--fresh", "--listen", "127.0.0.1:1"], &["serve", "--fresh", "x.aipl"]] {
        assert_eq!(Command::new(&me).args(args).output().unwrap().status.code(), Some(2), "{args:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
