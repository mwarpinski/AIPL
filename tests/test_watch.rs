//! aipl watch (docs/design/FEEDBACK_TOOLS_PLAN.md F5): a file runs, then
//! runs again when a file it imports changes; with --test its tests run.
//! (`--max-runs` stops it, so a test can watch.)

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_watch_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Runs `aipl watch ARGS` until it has run `runs` times; `between` is
/// called after each run but the last, once its output is in. Its output.
fn watch(args: &[&str], runs: usize, between: &dyn Fn(usize)) -> String {
    watch_with(Path::new(AIPL), args, runs, between)
}

/// watch, by the command at `exe` (the self-hosted aipl needs the library's place).
fn watch_with(exe: &Path, args: &[&str], runs: usize, between: &dyn Fn(usize)) -> String {
    let mut child = Command::new(exe).env("AIPL_STD", root().join("aipl_src/std/")).arg("watch").arg("--max-runs").arg(runs.to_string()).args(args).stdout(Stdio::piped()).spawn().unwrap();
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

/// The self-hosted aipl (aipl_src/watchcmd.aipl): the same watching, a change
/// to an imported file running the program again (natively, with its
/// arguments) or its tests, and what it says about each run.
#[test]
fn the_self_hosted_aipl_watches_too() {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return;
    }
    let dir = scratch("selfhosted");
    let me = dir.join("aipl");
    let o = Command::new(AIPL).args(["compile", "--exe", "--allow-run", "*", "aipl_src/aipl.aipl", "-o"]).arg(&me).current_dir(root()).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));

    // a change to an imported file runs the program again, with its arguments
    let (main, lib) = (dir.join("main.aipl"), dir.join("lib.aipl"));
    std::fs::write(&main, "(module main\n  (import lib)\n  (import os)\n  (fn main [] -> i32 (println \"n = {(call lib.n)}, args {(call os.arg_count)}\") 0))\n").unwrap();
    std::fs::write(&lib, "(module lib\n  (fn n [] -> i32 (pub) 1))\n").unwrap();
    let out = watch_with(&me, &[main.to_str().unwrap(), "--", "x", "y"], 2, &|_| std::fs::write(&lib, "(module lib\n  (fn n [] -> i32 (pub) 2))\n").unwrap());
    let one = out.find("n = 1, args 3\n").expect(&out);
    let two = out.find("n = 2, args 3\n").expect(&out);
    assert!(one < two, "{out}");
    assert_eq!(out.matches("\n[ran in ").count(), 2, "{out}");
    assert!(out.starts_with(&format!("aipl watch {}: ", main.display())) && out.contains(" files, Ctrl-C to stop\n"), "{out}");
    assert!(out.contains(" s, exit 0]\n"), "{out}");

    // with --test, its tests again, and a program that does not compile is watched too
    let file = dir.join("t.aipl");
    std::fs::write(&file, "(module t\n  (fn main [] -> i32 0)\n  (test \"one\" (expect (eq 1 2))))\n").unwrap();
    let out = watch_with(&me, &["--test", file.to_str().unwrap()], 2, &|_| {
        std::fs::write(&file, "(module t\n  (fn main [] -> i32 0)\n  (test \"one\" (expect (eq 1 1))))\n").unwrap()
    });
    let (fail, ok) = (out.find("FAIL  one").expect(&out), out.find("ok    one").expect(&out));
    assert!(fail < ok, "{out}");
    assert_eq!(out.matches("\n[tested in ").count(), 2, "{out}");
    let broken = dir.join("broken.aipl");
    std::fs::write(&broken, "(module broken (fn main [] -> i32 (call nope)))\n").unwrap();
    let out = watch_with(&me, &[broken.to_str().unwrap()], 2, &|_| {
        std::fs::write(&broken, "(module broken (fn main [] -> i32 (println \"fixed\") 0))\n").unwrap()
    });
    // (the compile error is on standard error, which this does not read)
    let (failed, fixed) = (out.find(" s, exit 1]\n").expect(&out), out.find("fixed\n").expect(&out));
    assert!(failed < fixed && out.ends_with(" s, exit 0]\n"), "{out}");

    // usage
    for args in [&["watch"][..], &["watch", "a.aipl", "b.aipl"], &["watch", "--max-runs"], &["watch", "--test", "a.aipl", "--", "x"]] {
        assert_eq!(Command::new(&me).args(args).output().unwrap().status.code(), Some(2), "{args:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

