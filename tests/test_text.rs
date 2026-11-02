//! std/text and std/str's searching (AIPL_SPEC.md 12.6): split, split on
//! whitespace, lines, replace, find, rfind, count, and trim give what Python
//! (and, for lines, Rust's str::lines) gives, on 800 generated cases
//! (tests/aipl/text, from tools/text_vectors.py), in the VM, under aipl-run,
//! and natively; the self-hosted toolchain compiles the program to the Rust
//! toolchain's bytes.

use std::path::PathBuf;
use std::process::Command;

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn std_text_does_what_python_does() {
    let src = root().join("tests/aipl/text/vectors.aipl");
    let want = std::fs::read_to_string(root().join("tests/aipl/text/vectors.expected")).unwrap();
    let dir = std::env::temp_dir().join(format!("aipl_text_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (wasm, exe) = (dir.join("p.wasm"), dir.join("p"));
    assert!(Command::new(AIPL).arg("compile").arg(&src).arg("-o").arg(&wasm).status().unwrap().success());
    assert!(Command::new(AIPL).args(["compile", "--exe"]).arg(&src).arg("-o").arg(&exe).status().unwrap().success());
    let runs = [
        ("VM", Command::new(AIPL).arg("eval").arg(&src).output().unwrap()),
        ("aipl-run", Command::new(RUNNER).arg(&wasm).output().unwrap()),
        ("native", Command::new(&exe).output().unwrap()),
    ];
    for (backend, o) in &runs {
        assert!(o.status.success(), "{backend}: {}", String::from_utf8_lossy(&o.stderr));
        let got: String = String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.starts_with("[AIPL")).map(|l| format!("{l}\n")).collect();
        for (k, (g, w)) in got.lines().zip(want.lines()).enumerate() {
            assert_eq!(g, w, "{backend}, case {} (tests/aipl/text/vectors.aipl line {})", k + 1, k + 31);
        }
        assert_eq!(got.lines().count(), want.lines().count(), "{backend}");
    }
    let module = aipl_core::resolver::Resolver::resolve(&src).unwrap();
    let rust = aipl_core::compiler::wasm::WasmCompiler::compile(&module).unwrap();
    assert_eq!(aipl_core::selfhost::compile_with_aipl(&src).unwrap(), rust);
    let _ = std::fs::remove_dir_all(&dir);
}
