//! A result or an option that is dropped is an error, and `(drop e)` ignores a value on purpose: in a
//! function body, a block, a loop, a `when`, an `if`, a match arm, a defer, an `unsafe`, a function that
//! returns void. Both toolchains refuse with the same words, at the same place; `drop` works in the VM,
//! under aipl-run, and natively, for every kind of value, and the self-hosted toolchain compiles it to the
//! Rust toolchain's bytes.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_must_use_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A module with a function returning a result, one returning an option, and one returning a value
/// struct, then `items`.
fn module(items: &str) -> String {
    format!(
        "(module m\n  (import option)\n  (struct P [a:i32 b:i64] (value))\n  (fn f [n:i32] -> (result i32 str) (ok:str n))\n  (fn g [] -> (option.Option i32) (make (option.Option i32) none))\n  (fn p [] -> P (make P [a:1 b:2i64]))\n{items})\n"
    )
}

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

#[test]
fn drop_ignores_a_value_of_any_kind_in_every_backend_and_toolchain() {
    let dir = scratch("values");
    let file = dir.join("d.aipl");
    let src = module(
        "  (fn side [n:i32] -> (result i32 str) (println \"side {n}\") (ok:str n))
  (fn quiet [] -> void (drop (call side 7)))
  (fn main [] -> i32
    (drop (call f 3))
    (drop (call g))
    (drop (call p))
    (drop (+ 1 2))
    (drop 7i64)
    (drop \"text\")
    (drop (call side 1))
    (call quiet)
    (when true (drop (call side 2)))
    (for i 0 2 (drop (call side i)))
    (let r:(result i32 str) (call side 5))
    (match_result r (ok v (drop (call side v))) (err e (drop (call side 0))))
    (println \"done\")
    0)",
    );
    std::fs::write(&file, src).unwrap();
    let want = "side 1\nside 7\nside 2\nside 0\nside 1\nside 5\nside 5\ndone\n";
    for (o, backend) in run_all(&file).iter().zip(["VM", "aipl-run", "native"]) {
        assert!(o.status.success(), "{backend}: {}", String::from_utf8_lossy(&o.stderr));
        assert_eq!(printed(o), want, "{backend}");
    }
    let module = aipl_core::resolver::Resolver::resolve(&file).unwrap();
    aipl_core::checker::TypeChecker::new().check_module(&module).unwrap();
    let rust = aipl_core::compiler::wasm::WasmCompiler::compile(&module).unwrap();
    assert!(aipl_core::selfhost::compile_with_aipl(&file).unwrap() == rust, "the self-hosted bytes differ");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_dropped_result_or_option_is_refused_alike_by_both_toolchains() {
    let dir = scratch("errors");
    let me = dir.join("aipl");
    assert!(Command::new(AIPL).args(["compile", "--exe", "aipl_src/aipl.aipl", "-o"]).arg(&me).current_dir(root()).status().unwrap().success());
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&me, std::fs::Permissions::from_mode(0o755)).unwrap();
    const RESULT: &str = "this value, (result i32 str), is dropped, so a failure would go unnoticed: use it (match_result or try), or write (drop e) to ignore it on purpose";
    const OPTION: &str = "this value, option.Option<i32>, is dropped, so a missing value would go unnoticed: use it (match or try_opt), or write (drop e) to ignore it on purpose";
    // (a function body, the message it is refused with)
    let cases: &[(&str, &str)] = &[
        ("(fn main [] -> i32 (call f 1) 0)", RESULT),
        ("(fn main [] -> i32 (call g) 0)", OPTION),
        ("(fn main [] -> void (call f 1))", RESULT),
        ("(fn main [] -> void (call g))", OPTION),
        ("(fn main [] -> i32 (block (call f 1) 0))", RESULT),
        ("(fn main [] -> i32 (when true (call f 1)) 0)", RESULT),
        ("(fn main [] -> i32 (for i 0 3 (call g)) 0)", OPTION),
        ("(fn main [] -> i32 (let k:i32 0) (while (lt k 3) (call f 1) (set! k (+ k 1))) 0)", RESULT),
        ("(fn main [] -> i32 (if true (call f 1) (call f 2)) 0)", RESULT),
        ("(fn main [] -> i32 (match_result (call f 1) (ok v (call f v)) (err e (call f 0))) 0)", RESULT),
        ("(fn main [] -> i32 (defer (call f 1)) 0)", RESULT),
        ("(fn main [] -> i32 (unsafe (call f 1) 0))", RESULT),
        ("(fn main [] -> i32 (cond (true (call f 1)) (else (call f 2))) 0)", RESULT),
    ];
    for (k, (body, msg)) in cases.iter().enumerate() {
        let file = dir.join(format!("e{k}.aipl"));
        std::fs::write(&file, module(&format!("  {body}\n"))).unwrap();
        let (f, out) = (file.to_str().unwrap(), dir.join("x.wasm"));
        let r = Command::new(AIPL).args(["compile", f, "-o"]).arg(&out).output().unwrap();
        let a = Command::new(&me).args(["compile", f, "-o"]).arg(&out).output().unwrap();
        let v = Command::new(AIPL).args(["verify", f]).output().unwrap();
        let (rerr, aerr, verr) = (String::from_utf8_lossy(&r.stderr), String::from_utf8_lossy(&a.stderr), String::from_utf8_lossy(&v.stderr));
        assert!(!r.status.success() && !a.status.success() && !v.status.success(), "{body} compiled");
        assert!(rerr.contains(msg), "{body}: {rerr}");
        assert!(verr.contains(msg), "{body}: {verr}");
        assert_eq!(rerr.trim().trim_start_matches("Error: "), aerr.trim().trim_start_matches("Error: "), "{body}: the toolchains differ");
    }
    // what is not dropped is accepted: a result or option that is bound, returned, matched, or tried
    let fine = [
        "(fn main [] -> i32 (let r:(result i32 str) (call f 1)) (match_result r (ok v v) (err e 1)))",
        "(fn main [] -> (result i32 str) (call f 1))",
        "(fn main [] -> i32 (match_result (call f 1) (ok v v) (err e 1)))",
        "(fn h [] -> (result i32 str) (let v:i32 (try (call f 1))) (call f v)) (fn main [] -> i32 (drop (call h)) 0)",
        "(fn main [] -> i32 (let o:(option.Option i32) (call g)) (match o ((option.Option i32) some [v] v) ((option.Option i32) none 0)))",
        "(fn main [] -> i32 (drop (call f 1)) (drop (call g)) 0)",
    ];
    for (k, body) in fine.iter().enumerate() {
        let file = dir.join(format!("ok{k}.aipl"));
        std::fs::write(&file, module(&format!("  {body}\n"))).unwrap();
        let (f, out) = (file.to_str().unwrap(), dir.join("x.wasm"));
        let r = Command::new(AIPL).args(["compile", f, "-o"]).arg(&out).output().unwrap();
        let a = Command::new(&me).args(["compile", f, "-o"]).arg(&out).output().unwrap();
        assert!(r.status.success(), "{body}: {}", String::from_utf8_lossy(&r.stderr));
        assert!(a.status.success(), "{body}: {}", String::from_utf8_lossy(&a.stderr));
    }
    // drop's own mistakes
    let bad = [
        ("(fn main [] -> i32 (drop (block)) 0)", "drop ignores a value, and this expression has none (it is void)"),
        ("(fn main [] -> i32 (drop) 0)", "drop takes the one expression whose value is ignored: (drop e)"),
        ("(fn main [] -> i32 (drop 1 2) 0)", "drop takes the one expression whose value is ignored: (drop e)"),
    ];
    for (k, (body, msg)) in bad.iter().enumerate() {
        let file = dir.join(format!("bad{k}.aipl"));
        std::fs::write(&file, module(&format!("  {body}\n"))).unwrap();
        let (f, out) = (file.to_str().unwrap(), dir.join("x.wasm"));
        let r = Command::new(AIPL).args(["compile", f, "-o"]).arg(&out).output().unwrap();
        let a = Command::new(&me).args(["compile", f, "-o"]).arg(&out).output().unwrap();
        let (rerr, aerr) = (String::from_utf8_lossy(&r.stderr), String::from_utf8_lossy(&a.stderr));
        assert!(!r.status.success() && !a.status.success(), "{body} compiled");
        assert!(rerr.contains(msg), "{body}: {rerr}");
        assert_eq!(rerr.trim().trim_start_matches("Error: "), aerr.trim().trim_start_matches("Error: "), "{body}: the toolchains differ");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
