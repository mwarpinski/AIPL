//! `(assert c)` and `(assert c msg)` stop the program when c is false, with the assert's place and text, both
//! sides of a comparison, and the message; `(abort msg)` stops it with a message of its own. Both trap as a
//! failed contract does (the message, then the call chain) in the VM, under aipl-run, and natively, and the
//! self-hosted toolchain compiles them to the Rust toolchain's bytes and refuses their misuse in the same words.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scratch(name: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("aipl_assert_{}_{}_{n}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The three backends' runs of `src`: the VM, aipl-run, and a native executable.
fn run_all(src: &Path) -> [Output; 3] {
    let dir = scratch("run");
    let (wasm, exe) = (dir.join("p.wasm"), dir.join("p"));
    let o = Command::new(AIPL).arg("compile").arg(src).arg("-o").arg(&wasm).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let o = Command::new(AIPL).args(["compile", "--exe"]).arg(src).arg("-o").arg(&exe).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let out = [
        Command::new(AIPL).arg("eval").arg(src).output().unwrap(),
        Command::new(RUNNER).arg(&wasm).output().unwrap(),
        Command::new(&exe).output().unwrap(),
    ];
    let _ = std::fs::remove_dir_all(&dir);
    out
}

fn printed(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.starts_with("[AIPL")).map(|l| format!("{l}\n")).collect()
}

/// The trap's message: standard error from the word `from` to the end of its line.
fn message(o: &Output, from: &str) -> String {
    let err = String::from_utf8_lossy(&o.stderr).into_owned();
    let at = err.find(from).unwrap_or_else(|| panic!("no '{from}' in: {err}"));
    err[at..].lines().next().unwrap().to_string()
}

/// The text of a program whose `main` is `body`, with `@` before the statement that must fail, and the
/// "line:column" of that statement (the file is t.aipl).
fn program(items: &str, body: &str) -> (String, String) {
    let text = format!("(module t\n{items}  (fn main [] -> i32\n    {body}\n    0))\n");
    let at = text.find('@').expect("a case marks the failing statement with @");
    let before = &text[..at];
    let line = before.matches('\n').count() + 1;
    let col = before.len() - before.rfind('\n').map_or(0, |p| p + 1) + 1;
    (text.replace('@', ""), format!("{line}:{col}"))
}

#[test]
fn a_failed_assert_stops_the_program_with_its_place_text_sides_and_message_in_every_backend() {
    let dir = scratch("fail");
    // (items, body, what follows "Assertion failed: t.aipl:L:C: " on the first line)
    let cases: &[(&str, &str, &str)] = &[
        ("", "(let n:i32 7) @(assert (eq n 5))", "(assert (eq n 5)): left 7, right 5"),
        ("", "(let n:i32 7) @(assert (gt n 10) \"n is small\")", "(assert (gt n 10) \"n is small\"): left 7, right 10: n is small"),
        ("", "(let n:i32 7) @(assert (gt n 10) (format \"n = {n}, wanted over 10\"))", "(assert (gt n 10) (format \"n = {n}, wanted over 10\")): left 7, right 10: n = 7, wanted over 10"),
        ("", "(let ok:bool false) @(assert ok)", "(assert ok)"),
        ("", "(let ok:bool false) @(assert ok \"not ok\")", "(assert ok \"not ok\"): not ok"),
        ("", "(let a:i64 1i64) @(assert (eq a 2i64))", "(assert (eq a 2i64)): left 1, right 2"),
        ("", "@(assert (lt 2.5 1.5))", "(assert (lt 2.5 1.5)): left 2.5, right 1.5"),
        ("", "@(assert (neq true true))", "(assert (neq true true)): left true, right true"),
        ("", "(assert true) (assert (eq 1 1) \"fine\") @(assert (and true false))", "(assert (and true false))"),
        ("  (enum Shade [dark light])\n", "(let s:Shade Shade.dark) @(assert (eq s Shade.light))", "(assert (eq s Shade.light)): left dark, right light"),
        ("", "(for i 0 3 (assert (lt i 2) \"loop\"))", ""),
    ];
    for (k, (items, body, want)) in cases.iter().enumerate() {
        if want.is_empty() {
            continue;
        }
        let (text, at) = program(items, body);
        let file = dir.join("t.aipl");
        std::fs::write(&file, &text).unwrap();
        let want = format!("Assertion failed: t.aipl:{at}: {want}");
        for (o, backend) in run_all(&file).iter().zip(["VM", "aipl-run", "native"]) {
            assert!(!o.status.success(), "{backend}, case {k}: it ran to the end");
            assert_eq!(message(o, "Assertion failed"), want, "{backend}, case {k}");
            let err = String::from_utf8_lossy(&o.stderr);
            assert!(err.contains("  at main (t.aipl:"), "{backend}, case {k}: no call chain in: {err}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn what_passes_prints_nothing_the_message_is_built_only_on_failure_and_the_sides_are_evaluated_once() {
    let dir = scratch("pass");
    let file = dir.join("t.aipl");
    let items = "  (fn note [what:str] -> str (println \"evaluated {what}\") what)
  (fn bump [p:(ptr Counter)] -> i32 (put p Counter.n (+ (get p Counter.n) 1)) (get p Counter.n))
  (struct Counter [n:i32])
";
    let (text, _) = program(
        items,
        "(let c:(ptr Counter) (new Counter (call alloc.default) [n:0]))
    (assert (eq (call bump c) 1) (call note \"unused\"))
    (assert (eq (call bump c) 2) (call note \"unused\"))
    (assert (lt (call bump c) 4))
    (println \"counted {(get c Counter.n)}\")
    (when (gt (get c Counter.n) 2) (println \"fine\"))
    @(assert (eq (call bump c) 9) (call note \"the third\"))",
    );
    std::fs::write(&file, text.replace("(module t\n", "(module t\n  (import alloc)\n")).unwrap();
    for (o, backend) in run_all(&file).iter().zip(["VM", "aipl-run", "native"]) {
        assert!(!o.status.success(), "{backend}");
        // each side once; the message only for the failing one
        assert_eq!(printed(o), "counted 3\nfine\nevaluated the third\n", "{backend}");
        assert!(message(o, "Assertion failed").ends_with(": left 4, right 9: the third"), "{backend}: {}", message(o, "Assertion failed"));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn abort_stops_with_its_own_message_and_a_branch_ending_in_one_fits_any_type() {
    let dir = scratch("abort");
    let file = dir.join("t.aipl");
    let items = "  (fn pick [n:i32] -> i32 (if (gt n 0) n (abort \"no negatives here\")))
  (fn word [n:i32] -> str (cond ((eq n 1) \"one\") ((eq n 2) \"two\") (else (abort (format \"no word for {n}\")))))
";
    let (text, _) = program(items, "(println \"{(call pick 3)} {(call word 2)}\") @(println \"{(call word 7)}\")");
    std::fs::write(&file, text).unwrap();
    for (o, backend) in run_all(&file).iter().zip(["VM", "aipl-run", "native"]) {
        assert!(!o.status.success(), "{backend}");
        assert_eq!(printed(o), "3 two\n", "{backend}");
        assert_eq!(message(o, "no word"), "no word for 7", "{backend}");
        assert!(String::from_utf8_lossy(&o.stderr).contains("  at main (t.aipl:"), "{backend}");
    }
    let (text, _) = program(items, "@(println \"{(call pick -1)}\")");
    std::fs::write(&file, text).unwrap();
    for (o, backend) in run_all(&file).iter().zip(["VM", "aipl-run", "native"]) {
        assert!(!o.status.success(), "{backend}");
        assert_eq!(message(o, "no negatives"), "no negatives here", "{backend}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_self_hosted_toolchain_compiles_assert_and_abort_to_the_same_bytes_and_refuses_misuse_alike() {
    let dir = scratch("parity");
    let items = "  (struct P [a:i32] (value))
  (fn pick [n:i32] -> i32 (if (gt n 0) n (abort \"no negatives here\")))
  (fn check [n:i32 p:P] -> i32
    (assert (gt n 0))
    (assert (eq n 5) \"n was not five\")
    (assert (lt (get p P.a) 100) (format \"a = {(get p P.a)}\"))
    (assert (and (gt n 1) (lt n 9)) (format \"n = {n}\"))
    (call pick n))
";
    let (text, _) = program(items, "(drop (call check 5 (make P [a:3]))) @(println \"x\")");
    let file = dir.join("t.aipl");
    std::fs::write(&file, text).unwrap();
    let module = aipl_core::resolver::Resolver::resolve(&file).unwrap();
    aipl_core::checker::TypeChecker::new().check_module(&module).unwrap();
    let rust = aipl_core::compiler::wasm::WasmCompiler::compile(&module).unwrap();
    assert!(aipl_core::selfhost::compile_with_aipl(&file).unwrap() == rust, "the self-hosted bytes differ");
    // misuse
    let me = dir.join("aipl");
    assert!(Command::new(AIPL).args(["compile", "--exe", "aipl_src/aipl.aipl", "-o"]).arg(&me).current_dir(root()).status().unwrap().success());
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&me, std::fs::Permissions::from_mode(0o755)).unwrap();
    const USAGE: &str = "assert takes a condition and, if you like, a message: (assert (gt n 0)) or (assert (gt n 0) \"n must be positive\")";
    const ABORT: &str = "abort takes the message, a str: (abort \"what went wrong\")";
    let bad: &[(&str, String)] = &[
        ("(assert)", USAGE.into()),
        ("(assert true \"a\" \"b\")", USAGE.into()),
        ("(assert 1)", "assert takes a bool, e.g. (assert (gt n 0)), got i32".into()),
        ("(assert true 5)", "the message of an assert is a str, e.g. (assert (gt n 0) \"n must be positive\"), got i32".into()),
        ("(abort)", ABORT.into()),
        ("(abort 5)", format!("{ABORT}, got i32")),
        ("(abort \"a\" \"b\")", ABORT.into()),
    ];
    for (k, (stmt, msg)) in bad.iter().enumerate() {
        let file = dir.join(format!("e{k}.aipl"));
        std::fs::write(&file, format!("(module e\n  (fn main [] -> i32\n    {stmt}\n    0))\n")).unwrap();
        let (f, out) = (file.to_str().unwrap(), dir.join("x.wasm"));
        let r = Command::new(AIPL).args(["compile", f, "-o"]).arg(&out).output().unwrap();
        let a = Command::new(&me).args(["compile", f, "-o"]).arg(&out).output().unwrap();
        let (rerr, aerr) = (String::from_utf8_lossy(&r.stderr), String::from_utf8_lossy(&a.stderr));
        assert!(!r.status.success() && !a.status.success(), "{stmt} compiled");
        assert!(rerr.contains(msg.as_str()), "{stmt}: {rerr}");
        assert_eq!(rerr.trim().trim_start_matches("Error: "), aerr.trim().trim_start_matches("Error: "), "{stmt}: the toolchains differ");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_assert_or_abort_is_no_leak_because_the_program_ends_where_its_message_is_built() {
    let dir = scratch("no_leak");
    let file = dir.join("n.aipl");
    // g is not marked (no_leak) and builds a message with format, f is marked and calls g
    std::fs::write(
        &file,
        "(module n
  (fn g [n:i32] -> i32
    (assert (gt n 0) (format \"n = {n}\"))
    n)
  (fn f [n:i32] -> i32 (no_leak)
    (assert (gt n 0))
    (assert (lt n 100) \"too big\")
    (assert (and (gt n 1) (lt n 9)) (format \"n = {n}\"))
    (if (gt n 3) n (abort (format \"small {n}\")))
    (call g n))
  (fn main [] -> i32 (call f 5)))
",
    )
    .unwrap();
    let f = file.to_str().unwrap();
    let r = Command::new(AIPL).args(["verify", f]).output().unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let me = dir.join("aipl");
    assert!(Command::new(AIPL).args(["compile", "--exe", "aipl_src/aipl.aipl", "-o"]).arg(&me).current_dir(root()).status().unwrap().success());
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&me, std::fs::Permissions::from_mode(0o755)).unwrap();
    let a = Command::new(&me).args(["verify", f]).output().unwrap();
    assert!(a.status.success(), "{}", String::from_utf8_lossy(&a.stderr));
    // what is outside the assert still counts: a format in the condition's own function body
    std::fs::write(&file, "(module n\n  (fn f [n:i32] -> i32 (no_leak)\n    (assert (gt n 0))\n    (let s:str (format \"x{n}\"))\n    (println \"{s}\")\n    n)\n  (fn main [] -> i32 (call f 5)))\n").unwrap();
    for tool in [Path::new(AIPL), &me] {
        let o = Command::new(tool).args(["verify", f]).output().unwrap();
        assert!(!o.status.success() && String::from_utf8_lossy(&o.stderr).contains("is marked (no_leak) but"), "{}", String::from_utf8_lossy(&o.stderr));
    }
    let _ = std::fs::remove_dir_all(&dir);
}
