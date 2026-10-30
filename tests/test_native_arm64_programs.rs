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
    let dir = std::env::temp_dir().join(format!("aipl_arm64_prog_{}_{}", std::process::id(), src.len()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("prog.aipl");
    std::fs::write(&file, src).unwrap();
    let module = Resolver::resolve(&file).unwrap_or_else(|e| panic!("{e}"));
    TypeChecker::new().check_module(&module).unwrap_or_else(|e| panic!("{e}"));
    let wasm = WasmCompiler::compile(&module).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    let grants: Vec<String> = grants.iter().map(|g| format!("run:{g}")).collect();
    aipl_core::native::executable_arm64_with(&wasm, &grants).unwrap_or_else(|e| panic!("{e}"))
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
