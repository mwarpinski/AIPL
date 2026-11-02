//! std/deflate (AIPL_SPEC.md 12.6): tests/aipl/deflate/vectors.aipl
//! decompresses Python's raw, zlib, and gzip streams at every level and
//! strategy (every block type), with one bit flipped or cut short
//! (refusing what zlib refuses, else the same bytes), and compresses at
//! every level, each stream checked by Python's zlib and gzip when
//! tools/deflate_vectors.py made the answers; tests/aipl/deflate/api.aipl
//! takes round trips, a 70,000-byte run, the output limit, and the
//! errors; each the same in the VM, under aipl-run, and natively.

use std::path::PathBuf;
use std::process::Command;

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The program's standard output in each backend, run in a scratch directory.
fn outputs(rel: &str, name: &str) -> Vec<(&'static str, String)> {
    let dir = std::env::temp_dir().join(format!("aipl_deflate_{}_{name}", std::process::id()));
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
fn std_deflate_agrees_with_python_both_ways() {
    let want = std::fs::read_to_string(root().join("tests/aipl/deflate/vectors.expected")).unwrap();
    for (backend, got) in outputs("tests/aipl/deflate/vectors.aipl", "vectors") {
        for (k, (g, w)) in got.lines().zip(want.lines()).enumerate() {
            assert_eq!(g, w, "{backend}, case {}", k + 1);
        }
        assert_eq!(got.lines().count(), want.lines().count(), "{backend}");
    }
}

#[test]
fn the_deflate_api_works_alike_everywhere() {
    let want = "zlib: 1800 bytes to 66; ok 66, 1800 bytes; same true\nlimit 100: err the output would pass the limit at 54\nraw: stream of 60 then 8 more; ok 60, 1800 bytes\nerr an incorrect header check at 0 | err an incorrect data check (Adler-32) at 62\ngzip of nothing: 20 bytes; ok 20, 0 bytes\n70000 zeros at 0: 70010 bytes; ok 70010, 70000 bytes; same true\n70000 zeros at 9: 84 bytes; ok 84, 70000 bytes; same true\n";
    for (backend, got) in outputs("tests/aipl/deflate/api.aipl", "api") {
        assert_eq!(got, want, "{backend}");
    }
}
