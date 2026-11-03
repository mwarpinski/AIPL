//! What is particular to the Windows build (README: what differs on Windows): the programs in
//! tests/aipl/windows, run through the real binaries. Nothing here runs elsewhere.
#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::Command;

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn case(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/aipl/windows").join(format!("{name}.aipl"))
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aipl_windows_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Compiles tests/aipl/windows/NAME.aipl to a module, and runs it with the arguments.
fn run(name: &str, flags: &[&str], args: &[&str]) -> (String, i32) {
    let dir = scratch(name);
    let wasm = dir.join(format!("{name}.wasm"));
    let o = Command::new(AIPL).arg("compile").arg(case(name)).arg("-o").arg(&wasm).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let o = Command::new(RUNNER).args(flags).arg(&wasm).args(args).output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    (String::from_utf8_lossy(&o.stdout).replace("\r\n", "\n"), o.status.code().unwrap_or(-1))
}

#[test]
fn processes_run_talk_and_are_killed() {
    let (out, code) = run("win_proc", &["--allow-run", "*"], &[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("hello status 3 exited true\nhello\n"), "{out}");
    assert!(out.contains("not_found error: "), "{out}");
    assert!(out.contains("wrote true read") && out.contains("a\nb\n"), "{out}");
    // a signal ends a process at once: status 256 + the signal
    assert!(out.contains("running true killed true status 265 signal 9\n"), "{out}");
}

#[test]
fn a_process_is_not_run_unless_granted() {
    let (out, code) = run("win_proc", &[], &[]);
    assert!(out.contains("error: ") && !out.contains("hello status"), "{out}");
    let _ = code;
}

#[test]
fn exec_runs_the_program_and_ends_with_its_status() {
    let (out, code) = run("win_exec", &["--allow-run", "*"], &[]);
    assert_eq!(code, 5, "{out}");
    assert!(out.starts_with("before\n") && out.contains("replaced") && !out.contains("exec returned"), "{out}");
}

#[test]
fn an_absolute_windows_path_argument_names_the_file() {
    let dir = scratch("abs_path");
    let file = dir.join("data.txt");
    std::fs::write(&file, "from a C: path").unwrap();
    let path = file.to_str().unwrap();
    assert!(path.as_bytes()[1] == b':', "a drive-letter path: {path}");
    let (out, code) = run("win_path", &[], &[path]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("read: from a C: path"), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}
