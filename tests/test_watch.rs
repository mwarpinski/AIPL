//! aipl watch (docs/design/FEEDBACK_TOOLS_PLAN.md F5): a file runs, then
//! runs again when a file it imports changes; with --test its tests run.
//! (`--max-runs` stops it, so a test can watch.)

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_watch_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Runs `aipl watch ARGS` until it has run `runs` times; `between` is
/// called after each run but the last, once its output is in. Its output.
fn watch(args: &[&str], runs: usize, between: &dyn Fn(usize)) -> String {
    let mut child = Command::new(AIPL).arg("watch").arg("--max-runs").arg(runs.to_string()).args(args).stdout(Stdio::piped()).spawn().unwrap();
    let mut out = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = out.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut text = String::new();
    let start = Instant::now();
    let mut done = 0;
    while start.elapsed() < Duration::from_secs(60) {
        if let Ok(b) = rx.recv_timeout(Duration::from_millis(100)) {
            text.push_str(&String::from_utf8_lossy(&b));
        }
        let finished = text.matches("\n[ran in ").count() + text.matches("\n[tested in ").count();
        if finished > done {
            done = finished;
            if done == runs {
                break;
            }
            // a later modification time than the run read
            std::thread::sleep(Duration::from_millis(50));
            between(done);
        }
    }
    assert!(child.wait().unwrap().success(), "{text}");
    text
}

#[test]
fn a_change_to_an_imported_file_runs_the_program_again() {
    let dir = scratch("run");
    let (main, lib) = (dir.join("main.aipl"), dir.join("lib.aipl"));
    std::fs::write(&main, "(module main\n  (import lib)\n  (fn main [] -> i32 (println \"n = {(call lib.n)}\") 0))\n").unwrap();
    std::fs::write(&lib, "(module lib\n  (fn n [] -> i32 (pub) 1))\n").unwrap();
    let out = watch(&[main.to_str().unwrap()], 2, &|_| std::fs::write(&lib, "(module lib\n  (fn n [] -> i32 (pub) 2))\n").unwrap());
    let one = out.find("n = 1\n").expect(&out);
    let two = out.find("n = 2\n").expect(&out);
    assert!(one < two, "{out}");
    assert_eq!(out.matches("\n[ran in ").count(), 2, "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn with_test_it_runs_the_tests_again() {
    let dir = scratch("test");
    let file = dir.join("t.aipl");
    std::fs::write(&file, "(module t\n  (fn main [] -> i32 0)\n  (test \"one\" (expect (eq 1 2))))\n").unwrap();
    let out = watch(&["--test", file.to_str().unwrap()], 2, &|_| {
        std::fs::write(&file, "(module t\n  (fn main [] -> i32 0)\n  (test \"one\" (expect (eq 1 1))))\n").unwrap()
    });
    let fail = out.find("FAIL  one").expect(&out);
    let ok = out.find("ok    one").expect(&out);
    assert!(fail < ok, "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}
