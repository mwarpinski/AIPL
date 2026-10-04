//! The aipl-run launcher and standalone executables (AIPL_SPEC.md 6.5),
//! exercised through the real `aipl` and `aipl-run` binaries.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_runner_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn compile(src: &Path, out: &Path, extra: &[&str]) {
    let o = Command::new(AIPL).arg("compile").arg(src).arg("-o").arg(out).args(extra).env("AIPL_RUNNER", RUNNER).output().unwrap();
    assert!(o.status.success(), "compile failed: {}", String::from_utf8_lossy(&o.stderr));
}

fn run_in(dir: &Path, program: &Path, args: &[&str], stdin: &str) -> Output {
    let mut cmd = if program.extension().is_some_and(|e| e == "wasm") {
        let mut c = Command::new(RUNNER);
        c.arg(program);
        c
    } else {
        Command::new(program)
    };
    let mut child = cmd.args(args).current_dir(dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    use std::io::Write;
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn the_runner_gives_programs_args_files_stdin_and_exit_codes() {
    let dir = scratch("basics");
    std::fs::write(
        dir.join("prog.aipl"),
        r#"(module prog
  (import io) (import str) (import os)
  (fn main [] -> i32
    (call io.println_int "argc: " (call os.arg_count))
    (call io.println_int "stdin words: " (call str.count_words (call io.read_stdin)))
    (let f:(ptr str.Bytes) (call io.read_path (call os.arg 1)))
    (call io.println_int "file bytes: " (get f str.Bytes.len))
    (if (gt (call os.arg_count) 2) (sys.exit 3) (block))
    0))"#,
    )
    .unwrap();
    std::fs::write(dir.join("data.txt"), "12345").unwrap();
    compile(&dir.join("prog.aipl"), &dir.join("prog.wasm"), &[]);
    let o = run_in(&dir, &dir.join("prog.wasm"), &["data.txt"], "a b c");
    assert_eq!(stdout(&o), "argc: 2\nstdin words: 3\nfile bytes: 5\n");
    assert_eq!(o.status.code(), Some(0));
    // an absolute path works (the runner grants /), and sys.exit sets the status
    let abs = dir.join("data.txt");
    let o = run_in(&dir, &dir.join("prog.wasm"), &[abs.to_str().unwrap(), "x"], "");
    assert!(stdout(&o).contains("file bytes: 5"), "{}", stdout(&o));
    assert_eq!(o.status.code(), Some(3));
}

#[test]
fn a_trap_is_reported_with_a_failing_status() {
    let dir = scratch("trap");
    std::fs::write(dir.join("t.aipl"), "(module t (fn main [] -> i32 (/ 1 0)))").unwrap();
    compile(&dir.join("t.aipl"), &dir.join("t.wasm"), &[]);
    let o = run_in(&dir, &dir.join("t.wasm"), &[], "");
    assert_eq!(o.status.code(), Some(134));
    // one line: the program as invoked, then the reason
    let program = dir.join("t.wasm").display().to_string();
    assert_eq!(String::from_utf8_lossy(&o.stderr), format!("{program}: wasm trap: integer divide by zero\n"));
    // the full wasmtime report on request
    let full = Command::new(RUNNER).arg(dir.join("t.wasm")).env("AIPL_BACKTRACE", "1").output().unwrap();
    let full = String::from_utf8_lossy(&full.stderr);
    assert!(full.contains("backtrace") && full.contains("wasm trap: integer divide by zero"), "{full}");
}

#[test]
fn threads_run_under_the_runner() {
    let dir = scratch("threads");
    std::fs::write(
        dir.join("th.aipl"),
        r#"(module th
  (import io)
  (fn work [c:i32] -> i32 (loop i 1 1000 1 (let _o:i32 (atomic.add c 1))) 0)
  (fn main [] -> i32
    (let c:i32 (mem.alloc 4))
    (let a:i32 (thread.spawn (ref work) c))
    (let b:i32 (thread.spawn (ref work) c))
    (let _x:i32 (thread.join a))
    (let _y:i32 (thread.join b))
    (call io.println_int "total: " (mem.load32 c))
    0))"#,
    )
    .unwrap();
    compile(&dir.join("th.aipl"), &dir.join("th.wasm"), &[]);
    let o = run_in(&dir, &dir.join("th.wasm"), &[], "");
    assert_eq!(stdout(&o), "total: 2000\n", "{}", String::from_utf8_lossy(&o.stderr));
}

/// `aipl compile --exe`: one file that runs anywhere with no toolchain, and
/// `--sandbox` confines it to the working directory. The self-hosted compiler
/// itself, built this way, compiles a program to the Rust toolchain's bytes.
#[test]
fn standalone_executables() {
    let dir = scratch("exe");
    let wc = dir.join("word_count");
    compile(&root().join("examples/word_count.aipl"), &wc, &["--exe"]);
    let elsewhere = scratch("exe_elsewhere");
    std::fs::write(elsewhere.join("notes.txt"), "one two\nthree\n").unwrap();
    let o = run_in(&elsewhere, &wc, &["notes.txt"], "");
    assert_eq!(stdout(&o), "lines: 2\nwords: 3\nbytes: 14\n");

    let boxed = dir.join("word_count_boxed");
    compile(&root().join("examples/word_count.aipl"), &boxed, &["--exe", "--sandbox"]);
    let abs = elsewhere.join("notes.txt");
    let o = run_in(&elsewhere, &boxed, &[abs.to_str().unwrap()], "");
    assert!(stdout(&o).is_empty(), "sandboxed program read an absolute path: {}", stdout(&o));
    let o = run_in(&elsewhere, &boxed, &["notes.txt"], "");
    assert!(stdout(&o).starts_with("lines: 2"));

    // the AIPL compiler as a standalone command
    let aiplc = dir.join("aiplc");
    compile(&root().join("aipl_src/driver.aipl"), &aiplc, &["--exe"]);
    let out = dir.join("wc.wasm");
    let o = run_in(&root(), &aiplc, &["examples/word_count.aipl", out.to_str().unwrap()], "");
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    let rust = dir.join("wc_rust.wasm");
    compile(&root().join("examples/word_count.aipl"), &rust, &[]);
    assert!(std::fs::read(&out).unwrap() == std::fs::read(&rust).unwrap(), "aiplc output differs from the Rust toolchain");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&elsewhere);
}
