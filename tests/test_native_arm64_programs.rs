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
    let dir = std::env::temp_dir().join(format!("aipl_arm64_prog_{}_{}", std::process::id(), src.len()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("prog.aipl");
    std::fs::write(&file, src).unwrap();
    let module = Resolver::resolve(&file).unwrap_or_else(|e| panic!("{e}"));
    TypeChecker::new().check_module(&module).unwrap_or_else(|e| panic!("{e}"));
    let wasm = WasmCompiler::compile(&module).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    aipl_core::native::executable_arm64(&wasm).unwrap_or_else(|e| panic!("{e}"))
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
