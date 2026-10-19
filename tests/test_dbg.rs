//! (dbg e) (docs/design/FEEDBACK_TOOLS_PLAN.md F3): tests/aipl/dbg/main.aipl
//! prints FILE:LINE:COL: e = value to standard error for every kind of value,
//! in an imported module and a generic function too, nested, and inside a
//! try; the same in the VM, under aipl-run, and natively, and compiled to the
//! same bytes by both toolchains; each mistake one message in both.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_dbg_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Standard error with each address as ADDR (an allocation's address is
/// the same in every backend, but changes with the standard library).
fn masked(o: &Output) -> String {
    let err = String::from_utf8_lossy(&o.stderr);
    err.lines()
        .map(|l| match l.rfind("@ ") {
            Some(i) if l[i + 2..].bytes().all(|b| b.is_ascii_digit()) => format!("{}@ ADDR\n", &l[..i]),
            _ => format!("{l}\n"),
        })
        .collect()
}

#[test]
fn dbg_shows_each_value_alike_in_every_backend_and_toolchain() {
    let src = root().join("tests/aipl/dbg/main.aipl");
    let dir = scratch("main");
    let (wasm, exe) = (dir.join("p.wasm"), dir.join("p"));
    assert!(Command::new(AIPL).arg("compile").arg(&src).arg("-o").arg(&wasm).status().unwrap().success());
    assert!(Command::new(AIPL).args(["compile", "--exe"]).arg(&src).arg("-o").arg(&exe).status().unwrap().success());
    let runs = [
        Command::new(AIPL).arg("eval").arg(&src).output().unwrap(),
        Command::new(RUNNER).arg(&wasm).output().unwrap(),
        Command::new(&exe).output().unwrap(),
    ];
    let want = std::fs::read_to_string(root().join("tests/aipl/dbg/main.expected")).unwrap();
    for (o, backend) in runs.iter().zip(["VM", "aipl-run", "native"]) {
        assert!(o.status.success(), "{backend}: {}", String::from_utf8_lossy(&o.stderr));
        assert_eq!(masked(o), want, "{backend}");
        assert_eq!(o.stderr, runs[1].stderr, "{backend}: the addresses differ from aipl-run's");
        let out = String::from_utf8_lossy(&o.stdout);
        assert!(out.contains("twice: 6 7\n42 5000000000 0.1 true tab\there dark bytes 43 4 1.5\n"), "{backend}: {out}");
    }
    let _ = std::fs::remove_dir_all(&dir);
    same_bytes(&src);
}

fn same_bytes(path: &Path) {
    let module = aipl_core::resolver::Resolver::resolve(path).unwrap();
    aipl_core::checker::TypeChecker::new().check_module(&module).unwrap();
    let rust = aipl_core::compiler::wasm::WasmCompiler::compile(&module).unwrap();
    assert_eq!(aipl_core::selfhost::compile_with_aipl(path).unwrap(), rust, "{}", path.display());
}

#[test]
fn every_dbg_mistake_has_one_message_in_both_toolchains() {
    let cases: &[(&str, &str)] = &[
        ("(dbg)", "5:5: dbg takes one expression, e.g. (dbg (call f x))"),
        ("(dbg 1 2)", "5:5: dbg takes one expression, e.g. (dbg (call f x))"),
        ("(dbg (call nothing))", "5:5: dbg shows a value, and this expression has none (it is void)"),
        ("(println \"{(dbg n)}\")", "5:14: a placeholder cannot hold a dbg; bind the value with let and dbg it there"),
    ];
    let dir = scratch("errors");
    for (k, (body, want)) in cases.iter().enumerate() {
        let file = dir.join(format!("e{k}.aipl"));
        std::fs::write(&file, format!("(module e\n  (fn nothing [] -> void (block))\n  (fn main [] -> i32\n    (let n:i32 1)\n    {body}\n    0))\n")).unwrap();
        let rust = aipl_core::resolver::Resolver::resolve(&file)
            .and_then(|m| aipl_core::checker::TypeChecker::new().check_module(&m))
            .expect_err(body);
        assert!(rust.contains(want), "{body}\n  expected: {want}\n  rust: {rust}");
        let aipl = aipl_core::selfhost::compile_with_aipl(&file).map(|_| ()).expect_err(body);
        assert_eq!(aipl, rust, "{body}");
    }
    let _ = std::fs::remove_dir_all(&dir);
    let err = aipl_core::parser::Parser::parse("(module m (fn main [] -> i32 (dbg 1)))").unwrap_err();
    assert!(err.contains("(dbg ...) is expanded during import resolution"), "{err}");
}
