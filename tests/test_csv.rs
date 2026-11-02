//! std/csv (AIPL_SPEC.md 12.6): tests/aipl/csv/vectors.aipl reads 400
//! generated texts and writes 200 generated rows, giving what Python's csv
//! module gives (tools/csv_vectors.py), and tests/aipl/csv/api.aipl uses
//! headers, columns by name, typed fields and their errors, a whole text at
//! once, tabs, and a file round trip; each the same in the VM, under
//! aipl-run, and natively.

use std::path::PathBuf;
use std::process::Command;

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The program's standard output in each backend, run in a scratch directory.
fn outputs(rel: &str, name: &str) -> Vec<(&'static str, String)> {
    let dir = std::env::temp_dir().join(format!("aipl_csv_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = root().join(rel);
    let (wasm, exe) = (dir.join("p.wasm"), dir.join("p"));
    assert!(Command::new(AIPL).arg("compile").arg(&src).arg("-o").arg(&wasm).status().unwrap().success());
    assert!(Command::new(AIPL).args(["compile", "--exe"]).arg(&src).arg("-o").arg(&exe).status().unwrap().success());
    let runs = [
        ("VM", Command::new(AIPL).arg("eval").arg(&src).current_dir(&dir).output().unwrap()),
        ("aipl-run", Command::new(RUNNER).arg(&wasm).current_dir(&dir).output().unwrap()),
        ("native", Command::new(&exe).current_dir(&dir).output().unwrap()),
    ];
    let out = runs
        .into_iter()
        .map(|(b, o)| {
            assert!(o.status.success(), "{b}: {}", String::from_utf8_lossy(&o.stderr));
            (b, String::from_utf8_lossy(&o.stdout).split_inclusive('\n').filter(|l| !l.starts_with("[AIPL")).collect())
        })
        .collect();
    let _ = std::fs::remove_dir_all(&dir);
    out
}

#[test]
fn std_csv_reads_and_writes_as_python_does() {
    let want = std::fs::read_to_string(root().join("tests/aipl/csv/vectors.expected")).unwrap();
    for (backend, got) in outputs("tests/aipl/csv/vectors.aipl", "vectors") {
        for (k, (g, w)) in got.lines().zip(want.lines()).enumerate() {
            assert_eq!(g, w, "{backend}, case {}", k + 1);
        }
        assert_eq!(got.lines().count(), want.lines().count(), "{backend}");
    }
}

#[test]
fn the_csv_api_works_alike_everywhere() {
    let want = "total 6.5\nn 1\nrow 3, column 1: x2 is not an integer (row 3, column 1)\na=1 b=? c=-1\nrow 2 has no column 2\n3 rows; two\nlines\ntab: 3 fields, second b,c\n";
    for (backend, got) in outputs("tests/aipl/csv/api.aipl", "api") {
        assert_eq!(got, want, "{backend}");
    }
}
