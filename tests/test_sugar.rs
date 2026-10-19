//! when, for, and variadic and/or (AIPL_SPEC.md 4.M, src/sugar.rs):
//! tests/aipl/sugar/main.aipl prints the same in the VM, under aipl-run, and
//! natively; the self-hosted aipl compiles it (and its test build) to the
//! Rust toolchain's bytes; and each mistake is one message in both, a test's
//! body read only in a test build.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");
const MAIN: &str = "tests/aipl/sugar/main.aipl";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_sugar_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(exe: &Path, args: &[&str]) -> Output {
    Command::new(exe).args(args).current_dir(root()).env("AIPL_STD", root().join("aipl_src/std/")).output().unwrap()
}

#[test]
fn the_forms_run_alike_in_every_backend() {
    let want = "end evaluated\nsum 6\nodd 16\npairs 3\na1 a2 a3 = false\no1 o2 = true\nlib 3\n";
    let dir = scratch("run");
    let (wasm, exe) = (dir.join("p.wasm"), dir.join("p"));
    assert!(run(Path::new(AIPL), &["compile", MAIN, "-o", wasm.to_str().unwrap()]).status.success());
    assert!(run(Path::new(AIPL), &["compile", "--exe", MAIN, "-o", exe.to_str().unwrap()]).status.success());
    let outs = [run(Path::new(AIPL), &["eval", MAIN]), run(Path::new(RUNNER), &[wasm.to_str().unwrap()]), run(&exe, &[])];
    for (o, backend) in outs.iter().zip(["VM", "aipl-run", "native"]) {
        assert!(o.status.success(), "{backend}: {}", String::from_utf8_lossy(&o.stderr));
        let got: String = String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.starts_with("[AIPL")).map(|l| format!("{l}\n")).collect();
        assert_eq!(got, want, "{backend}");
    }
    let t = run(Path::new(AIPL), &["test", MAIN]);
    assert!(t.status.success(), "{}", String::from_utf8_lossy(&t.stdout));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn both_toolchains_rewrite_alike_with_the_same_errors() {
    let dir = scratch("parity");
    let me = dir.join("aipl");
    assert!(run(Path::new(AIPL), &["compile", "--exe", "aipl_src/aipl.aipl", "-o", me.to_str().unwrap()]).status.success());
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&me, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (want, got) = (dir.join("want.wasm"), dir.join("got.wasm"));
    for tests in [false, true] {
        let mut args = vec!["compile"];
        if tests {
            args.push("--tests");
        }
        let r = run(Path::new(AIPL), &[&args[..], &[MAIN, "-o", want.to_str().unwrap()]].concat());
        let a = run(&me, &[&args[..], &[MAIN, "-o", got.to_str().unwrap()]].concat());
        assert!(r.status.success() && a.status.success(), "{}", String::from_utf8_lossy(&a.stderr));
        assert!(std::fs::read(&want).unwrap() == std::fs::read(&got).unwrap(), "tests {tests}: the builds differ");
    }

    // (item, test build?, the message, or "" for none)
    let cases: &[(&str, bool, &str)] = &[
        ("(fn f [] -> void (when))", false, "3:20: when takes a condition and a body: (when c body...)"),
        ("(fn f [] -> void (for i 0))", false, "3:20: for is (for i start end body...): i from start up to end, end left out"),
        ("(fn f [] -> void (for 1 0 5 (block)))", false, "3:20: for is (for i start end body...): i from start up to end, end left out"),
        // a test's body is read only in a test build
        ("(test \"t\" (for i 0))", false, ""),
        ("(test \"t\" (for i 0))", true, "3:13: for is (for i start end body...): i from start up to end, end left out"),
        ("(test \"t\" (println \"{\"))", false, ""),
        ("(test \"t\" (println \"{\"))", true, "3:22: unclosed { in a format string (write {{ for a brace)"),
    ];
    for (k, (item, tests, msg)) in cases.iter().enumerate() {
        let file = dir.join(format!("e{k}.aipl"));
        std::fs::write(&file, format!("(module e\n  (fn main [] -> i32 0)\n  {item})\n")).unwrap();
        let mut args = vec!["compile"];
        if *tests {
            args.push("--tests");
        }
        let (f, o) = (file.to_str().unwrap(), dir.join("x.wasm"));
        let r = run(Path::new(AIPL), &[&args[..], &[f, "-o", o.to_str().unwrap()]].concat());
        let a = run(&me, &[&args[..], &[f, "-o", o.to_str().unwrap()]].concat());
        let rerr = String::from_utf8_lossy(&r.stderr);
        if msg.is_empty() {
            assert!(r.status.success() && a.status.success(), "{item} (tests {tests}): {rerr} / {}", String::from_utf8_lossy(&a.stderr));
        } else {
            assert!(rerr.contains(msg), "{item}\n  expected: {msg}\n  rust: {rerr}");
            assert_eq!(String::from_utf8_lossy(&a.stderr), rerr, "{item}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
