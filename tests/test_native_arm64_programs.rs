//! The AArch64 native backend (docs/design/ARM64_PLAN.md, NA5 on): AIPL
//! programs compiled to wasm, then to AArch64 Linux executables
//! (aipl_src/native/native_arm64.aipl), run where AArch64 code runs
//! (tests/arm64/mod.rs: set AIPL_ARM64_SSH=pennylane, or run on AArch64 Linux
//! or with qemu-user; elsewhere each test says it did not run) and compared
//! with what the program does under aipl-run: its output and exit status.

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::resolver::Resolver;

mod arm64;

/// The AArch64 executable for the program `src` (a whole module's text).
fn build(src: &str) -> Vec<u8> {
    build_with(src, &[])
}

/// `build`, with the programs the executable may start (`--allow-run`).
fn build_with(src: &str, grants: &[&str]) -> Vec<u8> {
    build_confined(src, grants, false)
}

/// `build_with`, and `--sandbox` if `sandbox`.
fn build_confined(src: &str, grants: &[&str], sandbox: bool) -> Vec<u8> {
    let grants: Vec<String> = grants.iter().map(|g| format!("run:{g}")).collect();
    build_all(src, &grants, sandbox, None)
}

/// The executable with every kind of grant as the backend takes them: `run:PROGRAM` and `ADDR:PORT`
/// lines, and the address to `serve` on.
fn build_all(src: &str, grants: &[String], sandbox: bool, serve: Option<&str>) -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!("aipl_arm64_prog_{}_{}", std::process::id(), src.len()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("prog.aipl");
    std::fs::write(&file, src).unwrap();
    let module = Resolver::resolve(&file).unwrap_or_else(|e| panic!("{e}"));
    TypeChecker::new().check_module(&module).unwrap_or_else(|e| panic!("{e}"));
    let wasm = WasmCompiler::compile(&module).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    aipl_core::native::executable_arm64_with(&wasm, sandbox, grants, serve).unwrap_or_else(|e| panic!("{e}"))
}

/// Runs `src`'s executable: (standard output, exit status), or None where
/// nothing can run it.
fn run(src: &str, args: &[&str]) -> Option<(String, i32)> {
    let exe = build(src);
    let out = arm64::run(&exe, args, b"")?;
    Some((String::from_utf8_lossy(&out.stdout).into_owned(), out.status.code().unwrap_or(-1)))
}

fn expect(src: &str, want: &str, status: i32) {
    match run(src, &[]) {
        Some((out, code)) => assert_eq!((out.as_str(), code), (want, status), "{src}"),
        None => eprintln!("no AArch64 machine to run on: not run"),
    }
}

#[test]
fn a_program_exits_with_its_status() {
    expect("(module t (fn main [] -> i32 (+ 40 2)))", "", 42);
}

#[test]
fn integer_arithmetic_and_comparisons() {
    expect(
        "(module t (fn main [] -> i32 (let a:i32 7) (let b:i32 -3) (let c:i64 100000000000i64) \
         (if (and (eq (+ a b) 4) (and (eq (* a b) -21) (and (eq (/ a b) -2) (eq (% a b) 1)))) \
             (if (and (gt c 5i64) (eq (+ c 1i64) 100000000001i64)) 9 8) 7)))",
        "",
        9,
    );
}

#[test]
fn loops_and_calls() {
    expect(
        "(module t (fn fib [n:i32] -> i32 (if (lt n 2) n (+ (call fib (- n 1)) (call fib (- n 2))))) \
         (fn main [] -> i32 (let s:i32 0) (for i 0 10 (set! s (+ s (call fib i)))) s))",
        "",
        88,
    );
}

#[test]
fn printing_through_wasi() {
    expect("(module t (fn main [] -> i32 (println \"hello {(+ 40 2)}\") 3))", "hello 42\n", 3);
}

/// Files, directories, listing, and file information (the descriptor table, path_open,
/// fd_readdir, path_filestat_get, rename, unlink, mkdir, rmdir): the output the x86-64
/// build prints, from tests/aipl/sys/files_dirs_stat.aipl.
#[test]
fn files_directories_listing_and_file_information() {
    let src = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/aipl/sys/files_dirs_stat.aipl")).unwrap();
    let want = "mkdir ok\nwrite b ok\nwrite a ok\nmkdir sub ok\nread: a longer text\nentry a.txt\nentry b.txt\nentry sub\n\
                d/a.txt: file 13\nd/b.txt: file 3\nd/sub: directory\nd/none: missing\nrename ok\nd/b.txt: missing\n\
                d/c.txt: file 3\nrmdir full directory not empty\nremove a ok\nremove c ok\nremove missing no such file or directory\n\
                rmdir sub ok\nrmdir ok\ngone\n";
    expect(&src, want, 0);
}

/// A trap prints `<argv[0]>: <reason>` and the call chain with positions, and exits with 134,
/// as the x86-64 build and aipl-run do (the output of `aipl compile --no-opt --exe` on this
/// program, argv[0] left off: it is where the test put the executable).
#[test]
fn a_trap_prints_its_reason_and_call_chain() {
    let src = "(module prog\n  (fn divide [a:i32 b:i32] -> i32 (/ a b))\n  (fn middle [a:i32] -> i32 (+ 1 (call divide a 0)))\n  (fn main [] -> i32 (call middle 7)))\n";
    let exe = build(src);
    let Some(out) = arm64::run(&exe, &[], b"") else {
        eprintln!("no AArch64 machine to run on: not run");
        return;
    };
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    let (_, rest) = err.split_once(": ").expect("argv[0] and the reason");
    assert_eq!(
        rest,
        "wasm trap: integer divide by zero\n  at divide (prog.aipl:2:35)\n  at middle (prog.aipl:3:34)\n  at main (prog.aipl:4:22)\n"
    );
    assert_eq!(out.status.code(), Some(134));
}

/// A load outside memory faults, and the SIGSEGV handler makes it a trap at that instruction:
/// the x86-64 build's text and call chain.
#[test]
fn a_wild_load_is_an_out_of_bounds_trap() {
    let src = "(module prog\n  (fn peek [a:i32] -> i32 (unsafe (mem.load32 a)))\n  (fn main [] -> i32 (call peek -16)))\n";
    let exe = build(src);
    let Some(out) = arm64::run(&exe, &[], b"") else {
        eprintln!("no AArch64 machine to run on: not run");
        return;
    };
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    let (_, rest) = err.split_once(": ").expect("argv[0] and the reason");
    assert_eq!(rest, "wasm trap: out of bounds memory access\n  at peek (prog.aipl:2:35)\n  at main (prog.aipl:3:22)\n");
    assert_eq!(out.status.code(), Some(134));
}

/// The process operations (spawn, stream, read, write, close, wait, exec; the whole of
/// tests/aipl/sys/spawn_ops.aipl, spawn_exec.aipl, and std_proc_exec.aipl) from an AArch64
/// executable: the answers test_sys.rs expects of every other backend.
#[test]
fn processes_start_talk_through_pipes_and_are_waited_for() {
    let sys = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/aipl/sys");
    let go = |file: &str, grants: &[&str]| -> Option<(String, i32)> {
        let src = std::fs::read_to_string(sys.join(file)).unwrap();
        let exe = build_with(&src, grants);
        let out = arm64::run_env(&exe, &[], b"", &[("AIPL_PROC_TEST", "hello")])?;
        Some((String::from_utf8_lossy(&out.stdout).into_owned(), out.status.code().unwrap_or(-1)))
    };
    let shell = ["/bin/sh", "/bin/cat", "/no/such/program"];
    let Some((out, code)) = go("spawn_ops.aipl", &shell) else {
        eprintln!("no AArch64 machine to run on: not run");
        return;
    };
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "stderr_not_a_pipe -1\nsent 6\nclosed_input 0\nhello\nread 6\nstatus 0\nstatus_again 0\nwrong_way -8\nclosed_output 0\nread_closed -8\n\
         released 0\nwait_released -8\nno_stream -1\nexit_7 7\nkilled 265\nout\nerr\nmerged 8\nmerged_status 2\nnull_status 3\nA-B env=hello\n\
         args 14\nargs_status 0\nmissing -44\nnot_granted -76\nbad_stdio -28\nbad_stdio_high -28\nbad_handle_wait -8\nbad_handle_read -8\n\
         bad_handle_write -8\nbad_handle_close -8\nbad_handle_stream -8\ntoo_long -21\n"
    );
    // exec replaces the program: its own status, and what it printed
    let (out, _) = go("spawn_exec.aipl", &shell).unwrap();
    assert_eq!(out, "before\nnot_granted -76\nmissing -44\ntoo_long -21\nreplaced A-B env=hello\n");
    let (out, code) = go("std_proc_exec.aipl", &["/bin/sh", "/no/such/program"]).unwrap();
    assert_eq!(code, 4);
    assert_eq!(out, "before\nnot granted: not granted: run the program with --allow-run PROGRAM\nmissing: no such program\nreplaced zero\n");
}

/// `--sandbox`: only the working directory. A directory made there works; `..` is refused (EPERM, 63) in
/// every executable, as the x86-64 build and aipl-run do; an absolute path works in an ordinary
/// executable (the `/` preopen) and is a bad descriptor (8) under `--sandbox`, which has no such preopen.
#[test]
fn a_sandboxed_executable_reaches_only_its_working_directory() {
    let src = "(module prog\n  (fn mk [p:str] -> i32 (unsafe (fs.mkdir (str.ptr p) (str.len p))))\n  (fn rm [p:str] -> i32 (unsafe (fs.rmdir (str.ptr p) (str.len p))))\n  \
               (fn show [l:str n:i32] -> void (println \"{l} {n}\"))\n  \
               (fn main [] -> i32\n    (call show \"plain\" (call mk \"made\"))\n    (call show \"dotdot\" (call mk \"../outside\"))\n    \
               (call show \"absolute\" (call mk \"/tmp/aipl_sandbox_probe\"))\n    (call show \"absolute_gone\" (call rm \"/tmp/aipl_sandbox_probe\"))\n    0))\n";
    let ordinary = build_confined(src, &[], false);
    let sandboxed = build_confined(src, &[], true);
    assert_ne!(ordinary, sandboxed, "--sandbox builds a different executable");
    let Some(first) = arm64::run(&sandboxed, &[], b"") else {
        eprintln!("no AArch64 machine to run on: not run");
        return;
    };
    // where openat2 is not there (qemu-user), a sandboxed executable says so at start and stops: that
    // is the behaviour to see, and there is no confinement to test (an ordinary one falls back to openat)
    if String::from_utf8_lossy(&first.stderr).contains("needs the openat2 system call") {
        assert_eq!(first.status.code(), Some(1));
        assert!(first.stdout.is_empty());
        eprintln!("no openat2 here: the sandboxed executable stopped at start, as it should; confinement not tested");
        return;
    }
    assert_eq!(String::from_utf8_lossy(&first.stdout), "plain 0\ndotdot -63\nabsolute -8\nabsolute_gone -8\n");
    let ordinary_out = arm64::run(&ordinary, &[], b"").unwrap();
    assert_eq!(String::from_utf8_lossy(&ordinary_out.stdout), "plain 0\ndotdot -63\nabsolute 0\nabsolute_gone 0\n");
}

/// Sockets (listen, accept, recv, send, close; tests/aipl/net/echo.aipl, which serves four connections
/// and numbers each one's lines): the bytes test_net.rs expects of every other backend, and a listen
/// on an address the executable was not granted refused. Where the server cannot be reached from the
/// test (a container on another machine) the test says it did not run.
#[test]
fn sockets_serve_connections_and_refuse_what_was_not_granted() {
    use std::io::{Read, Write};
    use std::net::{Shutdown, TcpListener, TcpStream};
    let src = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/aipl/net/echo.aipl")).unwrap();
    // a port nothing is using now
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let at = format!("127.0.0.1:{port}");
    let exe = build_all(&src, std::slice::from_ref(&at), false, None);
    let Some(mut server) = arm64::spawn(&exe, &[&at]) else {
        eprintln!("no AArch64 machine on this one's network to run on: not run");
        return;
    };
    let long = "x".repeat(20000);
    let exchanges = [
        ("alpha\nbeta\r\ngamma".to_string(), "1: alpha\n2: beta\n3: gamma\nbye 3\n".to_string()),
        (String::new(), "bye 0\n".to_string()),
        (format!("{long}\nshort\n"), format!("1: {long}\n2: short\nbye 2\n")),
        (format!("{long}\nshort\n"), format!("1: {long}\n2: short\nbye 2\n")),
    ];
    for (k, (send, want)) in exchanges.iter().enumerate() {
        let start = std::time::Instant::now();
        let mut conn = loop {
            match TcpStream::connect(&at) {
                Ok(c) => break c,
                Err(_) if start.elapsed().as_secs() < 20 => std::thread::sleep(std::time::Duration::from_millis(50)),
                Err(e) => {
                    let _ = server.kill();
                    panic!("connection {k}: {e}")
                }
            }
        };
        conn.write_all(send.as_bytes()).unwrap();
        conn.shutdown(Shutdown::Write).unwrap();
        let mut got = String::new();
        conn.read_to_string(&mut got).unwrap();
        assert!(got == *want, "connection {k}: {got:?}");
    }
    let out = server.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    // another port than the one granted: refused, with status 1
    let other = format!("127.0.0.1:{}", port.wrapping_add(1));
    let refused = arm64::spawn(&exe, &[&other]).unwrap().wait_with_output().unwrap();
    assert_eq!(refused.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("not granted: run the program with --listen ADDR:PORT"), "{}", String::from_utf8_lossy(&refused.stderr));
}

/// `--serve` (a fresh instance of the program per connection, tests/aipl/net/fresh_demo.aipl): each request
/// answered by its own copy, a crashing one with a 500 that costs only itself, and, with `requests=2`, the
/// host exiting with status 0 after the second connection. Where the server cannot be reached from the
/// test the test says it did not run.
#[test]
fn a_serving_executable_answers_each_request_with_a_fresh_copy() {
    use std::io::{Read, Write};
    use std::net::{Shutdown, TcpListener, TcpStream};
    let src = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/aipl/net/fresh_demo.aipl")).unwrap();
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let at = format!("127.0.0.1:{port}");
    let exe = build_all(&src, &[], false, Some(&format!("{at} requests=3")));
    let Some(mut server) = arm64::spawn(&exe, &[]) else {
        eprintln!("no AArch64 machine on this one's network to run on: not run");
        return;
    };
    let ask = |target: &str, server: &mut std::process::Child| -> String {
        let start = std::time::Instant::now();
        let mut conn = loop {
            match TcpStream::connect(&at) {
                Ok(c) => break c,
                Err(_) if start.elapsed().as_secs() < 20 => std::thread::sleep(std::time::Duration::from_millis(50)),
                Err(e) => {
                    let _ = server.kill();
                    panic!("{target}: {e}")
                }
            }
        };
        conn.write_all(format!("GET {target} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes()).unwrap();
        conn.shutdown(Shutdown::Write).unwrap();
        let mut got = String::new();
        conn.read_to_string(&mut got).unwrap();
        got
    };
    let hello = ask("/", &mut server);
    assert!(hello.starts_with("HTTP/1.1 200 OK") && hello.ends_with("hello\n"), "{hello:?}");
    let crash = ask("/crash", &mut server);
    assert!(crash.starts_with("HTTP/1.1 500 Internal Server Error"), "{crash:?}");
    let after = ask("/echo/after", &mut server);
    assert!(after.starts_with("HTTP/1.1 200 OK") && after.ends_with("/echo/after"), "{after:?}");
    // three connections answered: the host is done, with status 0
    let out = server.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    // the crashed copy's trap is on the host's standard error, with its call chain
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("Array index out of bounds") && err.contains("at handle (prog.aipl"), "{err}");
}

/// Threads (thread-spawn, a per-thread block behind the thread pointer, futex wait and wake, memory.init):
/// tests/aipl/threads/threads_main.aipl, the workers tests/test_threads.rs runs under a wasi-threads host,
/// from an AArch64 executable. Four threads share an atomic counter and a lock, join returns each result,
/// concurrent allocations keep their contents, and two threads' prints arrive intact (their line breaks
/// may interleave, as in every backend).
#[test]
fn threads_share_counters_locks_and_the_allocator() {
    let src = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/aipl/threads/threads_main.aipl")).unwrap();
    let exe = build(&src);
    let Some(out) = arm64::run(&exe, &[], b"") else {
        eprintln!("no AArch64 machine to run on: not run");
        return;
    };
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(text.starts_with("atomic_counter 4000\njoin_results 25\nmutex_counter 4000\nconcurrent_alloc 4\n"), "{text}");
    assert!(text.ends_with("concurrent_print 0\n"), "{text}");
    assert_eq!(text.matches("one one one one one one").count(), 50, "{text}");
    assert_eq!(text.matches("two two two two two two").count(), 50, "{text}");
}

/// examples/spectralnorm_par.aipl, whose matrix-vector products run on several threads, prints the sequential
/// benchmark's result on 1, 3, and 8 threads.
#[test]
fn the_parallel_spectral_norm_matches_the_sequential_one() {
    let src = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/spectralnorm_par.aipl")).unwrap();
    let exe = build(&src);
    for threads in ["1", "3", "8"] {
        let Some(out) = arm64::run(&exe, &["100", threads], b"") else {
            eprintln!("no AArch64 machine to run on: not run");
            return;
        };
        assert_eq!(String::from_utf8_lossy(&out.stdout), "1.274219991\n", "{threads} threads: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(out.status.code(), Some(0), "{threads} threads");
    }
}

/// The benchmarks (benchmarks/, all but knucleotide, which reads standard input) print their expected output
/// from AArch64 executables, with the arguments of `test_args`: they exercise the optimizer's float and integer
/// registers, spills, and flushes (docs/design/ARM64_OPT_PLAN.md).
#[test]
fn the_benchmarks_print_their_expected_output() {
    let bench = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks");
    for name in ["binarytrees", "fannkuch", "mandelbrot", "nbody", "pidigits", "spectralnorm", "spigot"] {
        let dir = bench.join(name);
        let src = std::fs::read_to_string(dir.join(format!("{name}.aipl"))).unwrap();
        let args = std::fs::read_to_string(dir.join("test_args")).unwrap();
        let args: Vec<&str> = args.split_whitespace().collect();
        let want = std::fs::read(dir.join("expected.txt")).unwrap();
        let exe = build(&src);
        let Some(out) = arm64::run(&exe, &args, b"") else {
            eprintln!("no AArch64 machine to run on: not run");
            return;
        };
        assert!(out.stdout == want, "{name}: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(out.status.code(), Some(0), "{name}");
    }
}

/// Operations with a constant operand (immediates of add, sub, compare, shifts, bitmask immediates, a
/// multiplication or an unsigned division by a power of two, a division with no check to make, the
/// 8-bit float immediates) on a spread of values: what the AArch64 executable prints is what the
/// x86-64 one prints. (Only where an x86-64 Linux machine runs the second.)
#[test]
fn constant_operands_give_what_the_x86_64_build_gives() {
    let mut text = String::from("(module t\n  (fn ops [x:i32 y:i64 f:f64] -> void\n");
    let i32_ops = [
        "(+ x 5)", "(- x 5)", "(+ x -7)", "(- x -7)", "(+ x 4095)", "(- x 4096)", "(* x 8)", "(* x 1)", "(* x 6)", "(/ x 8)", "(/ x 7)",
        "(% x 8)", "(% x -3)", "(divu x 8)", "(remu x 8)", "(divu x 1)", "(remu x 1)", "(divu x 7)", "(remu x -1)",
        "(bitand x 255)", "(bitor x 65280)", "(^ x 2147483647)", "(bitand x -2)", "(bitor x -1073741821)", "(^ x 85)",
        "(shl x 3)", "(shl x 0)", "(shl x 32)", "(shr x 3)", "(shru x 3)", "(shru x 31)", "(shr x 31)",
        "(if (lt x 5) 1 0)", "(if (lt x -5) 1 0)", "(if (eq x 0) 1 0)", "(if (gt x 4095) 1 0)", "(if (gte x -4095) 1 0)",
        "(if (ltu x 4096) 1 0)", "(if (ltu x -1) 1 0)", "(if (neq x -1) 1 0)", "(if (lte x 2147483647) 1 0)",
    ];
    let i64_ops = [
        "(+ y 5i64)", "(- y 5i64)", "(+ y -7i64)", "(* y 8i64)", "(* y 1024i64)", "(/ y 8i64)", "(% y 10i64)", "(divu y 16i64)", "(remu y 16i64)",
        "(bitand y 255i64)", "(bitor y 4278190080i64)", "(^ y 9223372036854775807i64)", "(bitand y -2i64)", "(bitor y -4611686018427387901i64)",
        "(shl y 3i64)", "(shr y 40i64)", "(shru y 63i64)", "(if (lt y 5i64) 1 0)", "(if (lt y -5i64) 1 0)", "(if (eq y 0i64) 1 0)",
        "(if (gt y 4095i64) 1 0)", "(if (ltu y -1i64) 1 0)",
    ];
    for e in i32_ops.iter().chain(i64_ops.iter()) {
        text.push_str(&format!("    (println \"{{{e}}}\")\n"));
    }
    for e in [
        "(+ f 2.0)", "(* f 0.5)", "(- f 31.0)", "(/ f 0.125)", "(+ f 1.0)", "(* f -3.0)", "(+ f 0.0)", "(* f 100.0)", "(- 4.0 f)", "(+ f 1.9375)",
        "(* f 0.1)", "(if (lt f 0.5) 1 0)", "(if (gt f -2.0) 1 0)",
    ] {
        text.push_str(&format!("    (println \"{{{e}}}\")\n"));
    }
    text.push_str("    (println \"-\"))\n  (fn main [] -> i32\n");
    for (x, y, f) in [
        ("0", "0i64", "0.0"), ("1", "1i64", "1.0"), ("-1", "-1i64", "-1.0"), ("7", "-123456789012i64", "3.25"), ("8", "40960i64", "0.4"),
        ("-9", "9223372036854775807i64", "-31.5"), ("2147483647", "-9223372036854775807i64", "1000.0"), ("-2147483648", "281474976710656i64", "2.5"),
        ("305419896", "4294967295i64", "0.03125"), ("4096", "4096i64", "17.0"), ("-4096", "-4096i64", "31.0"),
    ] {
        text.push_str(&format!("    (call ops {x} {y} {f})\n"));
    }
    text.push_str("    0))\n");
    let dir = std::env::temp_dir().join(format!("aipl_arm64_consts_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("prog.aipl");
    std::fs::write(&file, &text).unwrap();
    let module = Resolver::resolve(&file).unwrap_or_else(|e| panic!("{e}"));
    TypeChecker::new().check_module(&module).unwrap_or_else(|e| panic!("{e}"));
    let wasm = WasmCompiler::compile(&module).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        eprintln!("no x86-64 Linux machine to run the reference on: not run");
        return;
    }
    let arm = aipl_core::native::executable_arm64_with(&wasm, false, &[], None).unwrap_or_else(|e| panic!("{e}"));
    let Some(got) = arm64::run(&arm, &[], b"") else {
        eprintln!("no AArch64 machine to run on: not run");
        return;
    };
    let x86 = aipl_core::native::executable(&wasm, false).unwrap_or_else(|e| panic!("{e}"));
    let path = std::env::temp_dir().join(format!("aipl_arm64_consts_x86_{}", std::process::id()));
    std::fs::write(&path, x86).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let want = std::process::Command::new(&path).output().unwrap();
    std::fs::remove_file(&path).ok();
    assert_eq!(want.status.code(), Some(0), "{}", String::from_utf8_lossy(&want.stderr));
    assert_eq!(String::from_utf8_lossy(&got.stdout), String::from_utf8_lossy(&want.stdout));
    assert_eq!(got.status.code(), Some(0));
    assert!(got.stdout.len() > 2000, "the program printed little: {}", got.stdout.len());
}

/// Running out of stack is a trap, not a crash: the call chain's text as the x86-64 build prints it, the
/// number of frames elided aside (a frame's size differs between the machines). A recursion that fits
/// runs, on the main thread and on a spawned one.
#[test]
fn running_out_of_stack_is_a_trap() {
    let src = "(module prog\n  (fn down [n:i32] -> i32 (if (eq n 0) 0 (+ 1 (call down (- n 1)))))\n  \
               (fn main [] -> i32 (println \"{(call down 20000)}\") (println \"{(call down 100000000)}\") 0))\n";
    let exe = build(src);
    let Some(out) = arm64::run(&exe, &[], b"") else {
        eprintln!("no AArch64 machine to run on: not run");
        return;
    };
    assert_eq!(String::from_utf8_lossy(&out.stdout), "20000\n");
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    let (_, rest) = err.split_once(": ").expect("argv[0] and the reason");
    let lines: Vec<&str> = rest.lines().collect();
    assert_eq!(lines[0], "wasm trap: call stack exhausted");
    assert_eq!(lines[1], "  at down");
    assert!(lines[2..33].iter().all(|l| *l == "  at down (prog.aipl:2:47)"), "{rest}");
    assert!(lines[33].starts_with("  ... ") && lines[33].ends_with(" more"), "{rest}");
    assert_eq!(lines.len(), 34);
    assert_eq!(out.status.code(), Some(134));
}

#[test]
fn a_thread_running_out_of_stack_traps_and_one_that_fits_does_not() {
    let program = |n: &str| {
        format!(
            "(module prog\n  (import io)\n  (fn down [n:i32] -> i32 (if (eq n 0) 0 (+ 1 (call down (- n 1)))))\n  \
             (fn work [n:i32] -> i32 (unsafe) (call down n))\n  \
             (fn main [] -> i32 (unsafe)\n    (let a:i32 (thread.spawn (ref work) {n}))\n    (println \"joined {{(thread.join a)}}\")\n    0))\n"
        )
    };
    let Some(out) = arm64::run(&build(&program("20000")), &[], b"") else {
        eprintln!("no AArch64 machine to run on: not run");
        return;
    };
    assert_eq!((String::from_utf8_lossy(&out.stdout).as_ref(), out.status.code()), ("joined 20000\n", Some(0)));
    let out = arm64::run(&build(&program("100000000")), &[], b"").unwrap();
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    let (_, rest) = err.split_once(": ").expect("argv[0] and the reason");
    assert!(rest.starts_with("wasm trap: call stack exhausted\n  at down\n  at down (prog.aipl:3:47)\n"), "{rest}");
    assert_eq!(out.status.code(), Some(134));
}

/// A pointer a WASI function is given must lie in memory: if it does not, the program traps with
/// wasmtime's words and no call chain, exit status 134, as the x86-64 build does (the lengths are the
/// regions' own); a read into a buffer outside memory reads first, so it traps with the bytes read as the
/// length, and reads nothing without trapping.
#[test]
fn a_bad_pointer_to_a_wasi_function_traps_with_the_region() {
    let cases: [(&str, &[u8], &str); 8] = [
        ("(fs.write 1 -16 5)", b"", "Pointer out of bounds: Region { start: 4294967280, len: 5 }\n"),
        ("(fs.write 1 100 2000000000)", b"", "Pointer out of bounds: Region { start: 100, len: 2000000000 }\n"),
        ("(sys.random -16 8)", b"", "Pointer out of bounds: Region { start: 4294967280, len: 8 }\n"),
        ("(sys.poll -16 -16 1 -16)", b"", "Pointer out of bounds: Region { start: 4294967280, len: 48 }\n"),
        ("(fs.stat (str.ptr \".\") 1 -16)", b"", "Pointer out of bounds: Region { start: 4294967280, len: 64 }\n"),
        ("(args.sizes (str.ptr \"01234567\") -16)", b"", "Pointer out of bounds: Region { start: 4294967280, len: 4 }\n"),
        ("(fs.read 0 -16 5)", b"hello\n", "Pointer out of bounds: Region { start: 4294967280, len: 5 }\n"),
        ("(env.sizes -16 -16)", b"", "Pointer out of bounds: Region { start: 4294967280, len: 4 }\n"),
    ];
    for (call, input, want) in cases {
        let src = format!("(module prog\n  (fn main [] -> i32\n    (unsafe (let r:i32 {call}) (println \"result {{r}}\"))\n    0))\n");
        let Some(out) = arm64::run(&build(&src), &[], input) else {
            eprintln!("no AArch64 machine to run on: not run");
            return;
        };
        let err = String::from_utf8_lossy(&out.stderr).into_owned();
        let (_, rest) = err.split_once(": ").unwrap_or_else(|| panic!("{call}: argv[0] and the message in {err:?}, status {:?}, stdout {:?}", out.status.code(), String::from_utf8_lossy(&out.stdout)));
        assert_eq!(rest, want, "{call}");
        assert_eq!((String::from_utf8_lossy(&out.stdout).as_ref(), out.status.code()), ("", Some(134)), "{call}");
    }
    // nothing to read: no trap, 0 bytes
    let src = "(module prog\n  (fn main [] -> i32\n    (unsafe (let r:i32 (fs.read 0 -16 5)) (println \"result {r}\"))\n    0))\n";
    if let Some(out) = arm64::run(&build(src), &[], b"") {
        assert_eq!((String::from_utf8_lossy(&out.stdout).as_ref(), out.status.code()), ("result 0\n", Some(0)));
    }
}
