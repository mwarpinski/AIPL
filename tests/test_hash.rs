//! std/hash (AIPL_SPEC.md 12.6): tests/aipl/hash/vectors.aipl hashes
//! random bytes of every length up to a few blocks with MD5, SHA-1,
//! SHA-256, and SHA-512, whole and in pieces, computes HMACs (RFC 4231's
//! cases and random keys up to twice a block), CRC-32 and Adler-32, giving
//! what Python's hashlib, hmac, and zlib give (tools/hash_vectors.py), and
//! NIST's examples; tests/aipl/hash/api.aipl uses the hex functions, the
//! checksums' check values, and a stream; each the same in the VM, under
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
    let dir = std::env::temp_dir().join(format!("aipl_hash_{}_{name}", std::process::id()));
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
fn std_hash_hashes_as_python_does() {
    let want = std::fs::read_to_string(root().join("tests/aipl/hash/vectors.expected")).unwrap();
    for (backend, got) in outputs("tests/aipl/hash/vectors.aipl", "vectors") {
        for (k, (g, w)) in got.lines().zip(want.lines()).enumerate() {
            assert_eq!(g, w, "{backend}, case {}", k + 1);
        }
        assert_eq!(got.lines().count(), want.lines().count(), "{backend}");
    }
}

#[test]
fn the_hash_api_works_alike_everywhere() {
    let want = "sha256 abc ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\nhmac f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8\ncrc32 cbf43926 adler32 11e60398\nstream: 20 bytes, same as whole: true\nsizes 64 128 16; equal true\n";
    for (backend, got) in outputs("tests/aipl/hash/api.aipl", "api") {
        assert_eq!(got, want, "{backend}");
    }
}
