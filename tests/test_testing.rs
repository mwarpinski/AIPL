//! The test form (docs/design/FEEDBACK_TOOLS_PLAN.md F4, AIPL_SPEC.md 4.L):
//! `aipl test` runs each `(test "name" ...)` of tests/aipl/testing/rows.aipl
//! in a fresh process and reports each, with both sides of a failed
//! comparison and a trap's call chain, the same under aipl-run, in the VM,
//! natively, and with --all-backends; -k picks tests by name. An ordinary
//! build leaves the tests out. The self-hosted aipl compiles the test build
//! to the Rust toolchain's bytes, and each mistake is one message in both.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const ROWS: &str = "tests/aipl/testing/rows.aipl";

/// The native backend targets Linux x86-64 only (AIPL_SPEC.md 6.6).
const NATIVE: bool = cfg!(all(target_os = "linux", target_arch = "x86_64"));

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_testing_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn aipl(args: &[&str]) -> Output {
    Command::new(AIPL).args(args).current_dir(root()).output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn each_test_runs_alone_and_reports_alike_in_every_backend() {
    let want = std::fs::read_to_string(root().join("tests/aipl/testing/rows.expected")).unwrap();
    let backends: &[Option<&str>] = if NATIVE {
        &[None, Some("--vm"), Some("--native"), Some("--all-backends")]
    } else {
        &[None, Some("--vm")]
    };
    for backend in backends {
        let mut args = vec!["test"];
        args.extend(backend);
        args.push(ROWS);
        let o = aipl(&args);
        assert_eq!(o.status.code(), Some(1), "{backend:?}: {}", String::from_utf8_lossy(&o.stderr));
        assert_eq!(stdout(&o), want, "{backend:?}");
    }
}

#[test]
fn k_picks_tests_by_name() {
    let o = aipl(&["test", "-k", "fields", ROWS]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        stdout(&o),
        "tests/aipl/testing/rows.aipl\n  ok    an empty line has no fields\n  ok    three fields\n  FAIL  a struct compares by pointer, and shows its fields\n        rows.aipl:33:5: (expect (eq (call make_row 1) (call make_row 1))): left Row{n: 1, name: \"r\"}, right Row{n: 1, name: \"r\"}\n3 tests: 2 passed, 1 failed\n"
    );
    let o = aipl(&["test", "-k", "afresh", ROWS]);
    assert_eq!(o.status.code(), Some(0));
    assert!(stdout(&o).ends_with("1 test: 1 passed, 0 failed\n"), "{}", stdout(&o));
}

#[test]
fn an_ordinary_build_leaves_the_tests_out() {
    let dir = scratch("ordinary");
    let (plain, tests) = (dir.join("plain.wasm"), dir.join("tests.wasm"));
    assert!(aipl(&["compile", ROWS, "-o", plain.to_str().unwrap()]).status.success());
    assert!(aipl(&["compile", "--tests", ROWS, "-o", tests.to_str().unwrap()]).status.success());
    let has = |p: &Path, s: &str| std::fs::read(p).unwrap().windows(s.len()).any(|w| w == s.as_bytes());
    assert!(!has(&plain, "__test_0") && !has(&plain, "testing.fail"), "the ordinary build has a test");
    assert!(has(&tests, "__test_6") && has(&tests, "testing.fail_cmp"));
    // and the program itself still runs
    let o = aipl(&["eval", ROWS]);
    assert!(o.status.success() && stdout(&o).contains("a,b has 2 fields"), "{}", stdout(&o));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The self-hosted aipl, built natively once for the comparisons below.
fn self_hosted(dir: &Path) -> PathBuf {
    let exe = dir.join("aipl");
    let o = aipl(&["compile", "--exe", "aipl_src/aipl.aipl", "-o", exe.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    exe
}

#[test]
fn both_toolchains_build_tests_alike_with_the_same_errors() {
    let dir = scratch("parity");
    let me = self_hosted(&dir);
    let run = |exe: &Path, args: &[&str]| Command::new(exe).args(args).current_dir(root()).env("AIPL_STD", root().join("aipl_src/std/")).output().unwrap();
    let (want, got) = (dir.join("want.wasm"), dir.join("got.wasm"));
    let r = run(Path::new(AIPL), &["compile", "--tests", ROWS, "-o", want.to_str().unwrap()]);
    let a = run(&me, &["compile", "--tests", ROWS, "-o", got.to_str().unwrap()]);
    assert!(r.status.success() && a.status.success(), "{}", String::from_utf8_lossy(&a.stderr));
    assert!(std::fs::read(&want).unwrap() == std::fs::read(&got).unwrap(), "the test builds differ");

    let cases: &[(&str, bool, &str)] = &[
        ("(fn f [] -> i32 (expect true) 0)", false, "3:19: expect is only for a test's body: (test \"name\" ... (expect c))"),
        ("(test name (expect true))", false, "3:3: a test is (test \"name\" body...)"),
        ("(test \"two\" (expect true false))", true, "3:15: expect takes one condition, e.g. (expect (eq n 3))"),
        ("(test \"int\" (expect 1))", true, "3:15: expect takes a bool, e.g. (expect (eq n 3)), got i32"),
        ("(test \"t\" (expect true)) (fn _start [] -> void (block))", true, "3:28: a test build makes its own _start: test a module without one"),
    ];
    for (k, (item, tests, msg)) in cases.iter().enumerate() {
        let file = dir.join(format!("e{k}.aipl"));
        std::fs::write(&file, format!("(module e\n  (fn main [] -> i32 0)\n  {item})\n")).unwrap();
        let mut args = vec!["compile"];
        if *tests {
            args.push("--tests");
        }
        let f = file.to_str().unwrap();
        let out = dir.join("x.wasm");
        let o = out.to_str().unwrap();
        let r = run(Path::new(AIPL), &[&args[..], &[f, "-o", o]].concat());
        let a = run(&me, &[&args[..], &[f, "-o", o]].concat());
        let rerr = String::from_utf8_lossy(&r.stderr);
        assert!(rerr.contains(msg), "{item}\n  expected: {msg}\n  rust: {rerr}");
        assert_eq!(String::from_utf8_lossy(&a.stderr), rerr, "{item}");
        assert_eq!(a.status.code(), Some(1), "{item}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
