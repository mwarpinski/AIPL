//! std/stream (AIPL_SPEC.md 12.6): tests/aipl/stream/lines.aipl reads its
//! standard input a line at a time and writes through a buffered stdout
//! writer with format_to, then writes a file and counts its lines back with
//! try and defer. Its output is what Rust's str::lines gives for the same
//! input (CRLF, a bare \r, a line longer than the reader's window, a last
//! line with no newline), the same in the VM, under aipl-run, and natively;
//! the self-hosted toolchain compiles it to the Rust toolchain's bytes, and
//! a writer format_to cannot name twice is one error in both.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_stream_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Runs `cmd` in `dir` with `input` on its standard input.
fn run_with_input(cmd: &mut Command, dir: &Path, input: &[u8]) -> std::process::Output {
    let mut child = cmd.current_dir(dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn lines_through_a_stream_match_rusts_lines_everywhere() {
    let long = "x".repeat(20000);
    let parts = ["first", "crlf\r", "", &long, "tab\there", "cr\rin middle", "", "é ünïcode"];
    let input = format!("{}\nlast no newline\r", parts.join("\n"));

    let mut want = String::new();
    let mut total = 0;
    for (k, line) in input.lines().enumerate() {
        want.push_str(&format!("{:>5}|{}|{}\n", k + 1, line.len(), line));
        total += line.len();
    }
    want.push_str(&format!("{} lines, {} bytes\n", input.lines().count(), total));
    want.push_str("counted 5000\ncannot open no_such_dir/missing.txt to read\n");

    let src = root().join("tests/aipl/stream/lines.aipl");
    let dir = scratch("lines");
    let wasm = dir.join("lines.wasm");
    let exe = dir.join("lines");
    assert!(Command::new(AIPL).arg("compile").arg(&src).arg("-o").arg(&wasm).status().unwrap().success());
    assert!(Command::new(AIPL).args(["compile", "--exe"]).arg(&src).arg("-o").arg(&exe).status().unwrap().success());
    let runs = [
        ("VM", run_with_input(Command::new(AIPL).arg("eval").arg(&src), &dir, input.as_bytes())),
        ("aipl-run", run_with_input(Command::new(RUNNER).arg(&wasm), &dir, input.as_bytes())),
        ("native", run_with_input(&mut Command::new(&exe), &dir, input.as_bytes())),
    ];
    for (backend, o) in &runs {
        assert!(o.status.success(), "{backend}: {}", String::from_utf8_lossy(&o.stderr));
        // split on \n alone: lines() would drop the \r the last line keeps
        let got: String = String::from_utf8_lossy(&o.stdout).split_inclusive('\n').filter(|l| !l.starts_with("[AIPL")).collect();
        assert_eq!(got, want, "{backend}");
    }
    assert!(!dir.join("_lines_test.tmp").exists(), "the program deletes its file");
    let _ = std::fs::remove_dir_all(&dir);

    let module = aipl_core::resolver::Resolver::resolve(&src).unwrap();
    aipl_core::checker::TypeChecker::new().check_module(&module).unwrap();
    let rust = aipl_core::compiler::wasm::WasmCompiler::compile(&module).unwrap();
    assert_eq!(aipl_core::selfhost::compile_with_aipl(&src).unwrap(), rust);
}

/// (format_to w ...) names a writer twice once lowered, so it takes one only
/// as a variable: the same message from both toolchains.
#[test]
fn format_to_takes_a_writer_as_a_variable() {
    let dir = scratch("errors");
    let file = dir.join("e.aipl");
    std::fs::write(
        &file,
        "(module e\n  (import stream)\n  (import alloc)\n  (fn main [] -> i32\n    (format_to (call stream.stdout (call alloc.default)) \"x\")\n    0))\n",
    )
    .unwrap();
    let rust = aipl_core::resolver::Resolver::resolve(&file)
        .and_then(|m| aipl_core::checker::TypeChecker::new().check_module(&m))
        .unwrap_err();
    assert!(rust.contains("5:5: format_to takes a (ptr stream.Writer) as a variable: bind it with let first"), "{rust}");
    let aipl = aipl_core::selfhost::compile_with_aipl(&file).map(|_| ()).unwrap_err();
    assert_eq!(aipl, rust);
    let _ = std::fs::remove_dir_all(&dir);
}
