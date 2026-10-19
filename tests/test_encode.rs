//! std/encode (AIPL_SPEC.md 12.6): tests/aipl/encode/vectors.aipl encodes
//! random bytes as base64 (both alphabets, padded or not), hex, and
//! percent-encoding, and decodes texts heavy in near misses, accepting and
//! refusing what Python accepts and refuses (tools/encode_vectors.py);
//! tests/aipl/encode/api.aipl uses the str functions and the error indices;
//! each the same in the VM, under aipl-run, and natively.

use std::path::PathBuf;
use std::process::Command;

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The program's standard output in each backend, run in a scratch directory.
fn outputs(rel: &str, name: &str) -> Vec<(&'static str, String)> {
    let dir = std::env::temp_dir().join(format!("aipl_encode_{}_{name}", std::process::id()));
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
fn std_encode_encodes_and_decodes_as_python_does() {
    let want = std::fs::read_to_string(root().join("tests/aipl/encode/vectors.expected")).unwrap();
    for (backend, got) in outputs("tests/aipl/encode/vectors.aipl", "vectors") {
        for (k, (g, w)) in got.lines().zip(want.lines()).enumerate() {
            assert_eq!(g, w, "{backend}, case {}", k + 1);
        }
        assert_eq!(got.lines().count(), want.lines().count(), "{backend}");
    }
}

#[test]
fn the_encode_api_works_alike_everywhere() {
    let want = "base64 aGVsbG8=\nok hello | err 7 | err 3\nfb ff: +/8= -_8\nhex 4142 | ok AB | err 2 | err 3\npercent a%20b/c%26d%3D%C3%A9 | ok a b/c&d=é | err 3\nform: a b+c (5 bytes)\n";
    for (backend, got) in outputs("tests/aipl/encode/api.aipl", "api") {
        assert_eq!(got, want, "{backend}");
    }
}
