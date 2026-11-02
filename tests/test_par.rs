//! std/par (docs/design/GENERATIONS_PLAN.md, "Data races"): parallel work
//! with no unsafe. examples/spectralnorm_par.aipl, whose matrix-vector
//! products run through par.map, prints what the sequential benchmark
//! prints (benchmarks/spectralnorm/expected.txt), on one thread and on
//! several, in the VM, under aipl-run, and natively. (Which type arguments
//! par.map accepts, (where (plain T)), is tests/test_interfaces.rs; its own
//! self-test is in aipl_src/test_suite.aipl.)

use std::path::PathBuf;
use std::process::Command;

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn output(cmd: &mut Command) -> String {
    let o = cmd.output().unwrap();
    assert!(o.status.success(), "{cmd:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.starts_with("[AIPL")).map(|l| format!("{l}\n")).collect()
}

#[test]
fn parallel_spectralnorm_matches_the_sequential_one_in_every_backend() {
    let dir = std::env::temp_dir().join(format!("aipl_par_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = root().join("examples/spectralnorm_par.aipl");
    let want = std::fs::read_to_string(root().join("benchmarks/spectralnorm/expected.txt")).unwrap();
    let wasm = dir.join("snp.wasm");
    let exe = dir.join("snp");
    assert!(Command::new(AIPL).arg("compile").arg(&src).arg("-o").arg(&wasm).status().unwrap().success());
    assert!(Command::new(AIPL).args(["compile", "--exe"]).arg(&src).arg("-o").arg(&exe).status().unwrap().success());
    for threads in ["1", "3", "8"] {
        assert_eq!(output(Command::new(AIPL).arg("eval").arg(&src).args(["--", "100", threads])), want, "VM, {threads} threads");
        assert_eq!(output(Command::new(RUNNER).arg(&wasm).args(["100", threads])), want, "aipl-run, {threads} threads");
        assert_eq!(output(Command::new(&exe).args(["100", threads])), want, "native, {threads} threads");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
