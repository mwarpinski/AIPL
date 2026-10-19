//! std/stats (AIPL_SPEC.md 12.6): tests/aipl/stats/vectors.aipl computes
//! every statistic of 300 generated data sets as Python's statistics module
//! and math.fsum do, to the last bit (tools/stats_vectors.py), and
//! tests/aipl/stats/api.aipl the ones Python has no counterpart for; each
//! the same in the VM, under aipl-run, and natively (tests/test_bootstrap.rs
//! compares both toolchains' bytes for every test program).

use std::path::PathBuf;
use std::process::{Command, Output};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn runs(rel: &str, name: &str) -> Vec<(&'static str, Output)> {
    let dir = std::env::temp_dir().join(format!("aipl_stats_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = root().join(rel);
    let (wasm, exe) = (dir.join("p.wasm"), dir.join("p"));
    assert!(Command::new(AIPL).arg("compile").arg(&src).arg("-o").arg(&wasm).status().unwrap().success());
    assert!(Command::new(AIPL).args(["compile", "--exe"]).arg(&src).arg("-o").arg(&exe).status().unwrap().success());
    let out = vec![
        ("VM", Command::new(AIPL).arg("eval").arg(&src).output().unwrap()),
        ("aipl-run", Command::new(RUNNER).arg(&wasm).output().unwrap()),
        ("native", Command::new(&exe).output().unwrap()),
    ];
    let _ = std::fs::remove_dir_all(&dir);
    out
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).split_inclusive('\n').filter(|l| !l.starts_with("[AIPL")).collect()
}

#[test]
fn std_stats_gives_pythons_answers_to_the_bit() {
    let want = std::fs::read_to_string(root().join("tests/aipl/stats/vectors.expected")).unwrap();
    for (backend, o) in runs("tests/aipl/stats/vectors.aipl", "vectors") {
        assert!(o.status.success(), "{backend}: {}", String::from_utf8_lossy(&o.stderr));
        let got = stdout(&o);
        for (k, (g, w)) in got.lines().zip(want.lines()).enumerate() {
            assert_eq!(g, w, "{backend}, case {}", k + 1);
        }
        assert_eq!(got.lines().count(), want.lines().count(), "{backend}");
    }
}

#[test]
fn histogram_zscore_fit_and_too_little_data() {
    let want = "3 1 0 1 1\nz -0.01710263992780911\nfit 2.5 -5.833333333333334 0.9213759213759214\n";
    for (backend, o) in runs("tests/aipl/stats/api.aipl", "api") {
        assert_eq!(stdout(&o), want, "{backend}");
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(err.contains("Pre-condition failed in 'stats.variance'") && err.contains("(req (gt (arr.len xs) 1))"), "{backend}: {err}");
        assert_ne!(o.status.code(), Some(0), "{backend}");
    }
}
