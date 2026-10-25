//! Early returns and cleanup (AIPL_SPEC.md 4.F): a branch that always leaves
//! takes the other branches' type; `try`, `try_opt`, and `try_or` return a
//! result's error or an Option's none from the function; `defer` runs its
//! statements however the function returns. Each program in
//! tests/aipl/early prints its .expected in the VM, under aipl-run, and
//! natively, and the self-hosted toolchain compiles it to the Rust
//! toolchain's bytes; every error has one message in both toolchains.

use std::path::{Path, PathBuf};
use std::process::Command;

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_early_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn stdout(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.starts_with("[AIPL")).map(|l| format!("{l}\n")).collect()
}

fn same_bytes(path: &Path) {
    let module = aipl_core::resolver::Resolver::resolve(path).unwrap();
    aipl_core::checker::TypeChecker::new().check_module(&module).unwrap();
    let rust = aipl_core::compiler::wasm::WasmCompiler::compile(&module).unwrap();
    assert_eq!(aipl_core::selfhost::compile_with_aipl(path).unwrap(), rust, "{}", path.display());
}

#[test]
fn every_program_prints_its_expected_output_everywhere() {
    for name in ["diverging", "diverging_values", "try_forms", "try_nested", "defer_basics", "defer_edges"] {
        let src = root().join(format!("tests/aipl/early/{name}.aipl"));
        let want = std::fs::read_to_string(root().join(format!("tests/aipl/early/{name}.expected"))).unwrap();
        let dir = scratch(name);
        let (wasm, exe) = (dir.join("p.wasm"), dir.join("p"));
        assert!(Command::new(AIPL).arg("compile").arg(&src).arg("-o").arg(&wasm).status().unwrap().success(), "{name}");
        assert!(Command::new(AIPL).args(["compile", "--exe"]).arg(&src).arg("-o").arg(&exe).status().unwrap().success(), "{name}");
        let runs = [
            ("VM", Command::new(AIPL).arg("eval").arg(&src).output().unwrap()),
            ("aipl-run", Command::new(RUNNER).arg(&wasm).output().unwrap()),
            ("native", Command::new(&exe).output().unwrap()),
        ];
        for (backend, o) in &runs {
            assert!(o.status.success(), "{name} in {backend}: {}", String::from_utf8_lossy(&o.stderr));
            assert_eq!(stdout(o), want, "{name} in {backend}");
        }
        same_bytes(&src);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn every_error_has_one_message_in_both_toolchains() {
    let header = "(module t\n  (import option)\n  (fn r [] -> (result i32 str) (ok:str 1))\n  (fn o [] -> (option.Option i32) (make (option.Option i32) none))\n";
    let cases: &[(&str, &str)] = &[
        ("(fn f [] -> (result i32 i32) (let v:i32 (try 5)) (ok:i32 v))", "try takes a (result T E), got i32"),
        ("(fn f [] -> (result i32 i32) (let v:i32 (try (call r))) (ok:i32 v))", "try's error is str, and the function's is i32: they must be one type"),
        ("(fn f [] -> i32 (try (call r)))", "try returns an error from the function, which returns i32, not a (result T E)"),
        ("(fn f [] -> (result i32 i32) (let v:i32 (try_opt (call r))) (ok:i32 v))", "try_opt takes an (option.Option T), got (result i32 str)"),
        ("(fn f [] -> (result i32 i32) (let v:i32 (try_opt (call o))) (ok:i32 v))", "try_opt returns none from the function, which returns (result i32 i32), not an (option.Option T)"),
        ("(fn f [] -> (result i32 i32) (let v:i32 (try_or (call o))) (ok:i32 v))", "try_or takes an option and the error to return without a value: (try_or o err)"),
        ("(fn f [] -> (result i32 i32) (let v:i32 (try)) (ok:i32 v))", "try takes one result: (try (call f x))"),
        ("(fn f [] -> (result i32 i32) (req (gt (try (call r)) 0)) (ok:i32 1))", "try returns from the function, and a contract cannot"),
        ("(fn f [] -> i32 (block (defer (call r))) 1)", "defer is a statement of a function's body (it runs when the function returns), not part of an expression or a block"),
        ("(fn f [] -> i32 (defer (return 1)) 1)", "a defer cannot return: it runs as the function returns"),
        ("(fn f [] -> (result i32 str) (defer (let q:i32 (try (call r)))) (ok:str 1))", "a defer cannot try: it runs as the function returns"),
        ("(fn f [n:i32] -> i32 (when (gt n 0) n))", "If branch type mismatch: then is i32, else is void"),
    ];
    let dir = scratch("errors");
    for (k, (item, want)) in cases.iter().enumerate() {
        let file = dir.join(format!("e{k}.aipl"));
        std::fs::write(&file, format!("{header}  {item}\n  (fn main [] -> i32 0))\n")).unwrap();
        let rust = aipl_core::resolver::Resolver::resolve(&file)
            .and_then(|m| aipl_core::checker::TypeChecker::new().check_module(&m))
            .expect_err(item);
        assert!(rust.contains(want), "{item}\n  expected: {want}\n  rust: {rust}");
        let aipl = aipl_core::selfhost::compile_with_aipl(&file).map(|_| ()).expect_err(item);
        assert_eq!(aipl, rust, "{item}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
