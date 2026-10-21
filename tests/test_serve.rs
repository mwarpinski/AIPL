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

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
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
        let at = format!("127.0.0.1:{}", free_port());
        let mut server = start(backend, &src, &dir, &at, &[&answered.to_string()]);
        for (k, (req, want)) in exchanges().into_iter().enumerate() {
            let got = exchange(&at, &mut server, &req);
            match want {
                Some(w) => assert!(got == w, "{backend}, request {k}: {:?}", String::from_utf8_lossy(&got)),
                None => assert!(got.is_empty(), "{backend}, request {k}: an empty request has no answer"),
            }
        }
        let o = server.wait_with_output().unwrap();
        assert!(o.status.success(), "{backend}: {}", String::from_utf8_lossy(&o.stderr));
        let printed: String = String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.starts_with("[AIPL")).collect();
        // 64 KiB to start, and a chunk for the 70,000-byte head
        assert_eq!(printed, format!("served {answered}, arena 262160 bytes"), "{backend}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// `n` mixed requests (echo targets and bodies of every size up to a few
/// hundred bytes, and 404s) after a thousand: the top of the heap is the
/// same before and after.
fn memory_stays_flat(backend: &str, n: usize) {
    let src = root().join("tests/aipl/net/serve_demo.aipl");
    let dir = scratch(&format!("flat_{backend}"));
    let at = format!("127.0.0.1:{}", free_port());
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
    let o = server.wait_with_output().unwrap();
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
        let at = format!("127.0.0.1:{}", free_port());
        let mut server = start(backend, &src, &dir, &at, &[]);
        let first = exchange(&at, &mut server, b"GET /first HTTP/1.1\r\n\r\n");
        assert!(first.ends_with(b"/first"), "{backend}: {:?}", String::from_utf8_lossy(&first));
        let second = exchange(&at, &mut server, b"GET /second HTTP/1.1\r\n\r\n");
        assert!(second.is_empty(), "{backend}: the second request is not answered");
        let o = server.wait_with_output().unwrap();
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(!o.status.success() && err.contains("Use after free"), "{backend}: {err}");
        assert!(err.contains("serve_keep.aipl:19:41"), "{backend}: the trap names the handler's read: {err}");
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
        assert!(err.contains("Array index out of bounds: index 5 for array of length 1") && err.contains("at handle (fresh_demo.aipl:31:54)"), "{backend}: {err}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// `aipl serve --fresh`: each connection a new instance under aipl-run. A
/// handler that traps answers 500 (its message and call chain go to the
/// server's standard error), and the next request is served.
#[test]
fn a_fresh_instance_per_request_isolates_a_crash() {
    let src = root().join("tests/aipl/net/fresh_demo.aipl");
    let at = format!("127.0.0.1:{}", free_port());
    let requests: [(&str, Vec<u8>); 4] = [
        ("/", text_response("200 OK", "hello\n")),
        ("/crash", b"HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/plain; charset=utf-8\r\nConnection: close\r\nContent-Length: 22\r\n\r\nInternal Server Error\n".to_vec()),
        ("/echo?after=crash", text_response("200 OK", "/echo?after=crash")),
        ("/", text_response("200 OK", "hello\n")),
    ];
    let mut server = Command::new(AIPL)
        .args(["serve", "--fresh", "--listen", &at, "--requests", "4"])
        .arg(&src)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    for (path, want) in &requests {
        let got = exchange(&at, &mut server, format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes());
        assert!(got == *want, "{path}: {:?}", String::from_utf8_lossy(&got));
    }
    let o = server.wait_with_output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success(), "{err}");
    assert!(err.contains("Array index out of bounds") && err.contains("at handle (fresh_demo.aipl:31:54)"), "{err}");
    // without --fresh, the way to a long-running server
    let o = Command::new(AIPL).args(["serve", "--listen", &at]).arg(&src).output().unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("add --fresh"), "{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn both_toolchains_compile_the_fresh_server_alike() {
    let src = root().join("tests/aipl/net/fresh_demo.aipl");
    let m = aipl_core::resolver::Resolver::resolve(&src).unwrap();
    aipl_core::checker::TypeChecker::new().check_module(&m).unwrap();
    let rust = aipl_core::compiler::wasm::WasmCompiler::compile(&m).unwrap();
    assert_eq!(aipl_core::selfhost::compile_with_aipl(&src).unwrap(), rust);
}
