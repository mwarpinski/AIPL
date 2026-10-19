//! std/set, std/random and std/uuid, std/path, and std/cli (AIPL_SPEC.md
//! 12.6): each demo in tests/aipl/libs prints its .expected in the VM,
//! under aipl-run, and natively. path_demo's answers are Python's posixpath
//! (tools/path_vectors.py); random_demo's start with PCG-32's reference
//! numbers; cli_demo parses lists and its own arguments, prints its usage
//! for --help (exit 0), and names a bad argument (exit 2).

use std::path::PathBuf;
use std::process::{Command, Output};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The demo run in each backend with `args`.
fn runs(name: &str, args: &[&str]) -> Vec<(&'static str, Output)> {
    let dir = std::env::temp_dir().join(format!("aipl_libs_{}_{name}_{}", std::process::id(), args.len()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = root().join(format!("tests/aipl/libs/{name}.aipl"));
    let (wasm, exe) = (dir.join("p.wasm"), dir.join("p"));
    assert!(Command::new(AIPL).arg("compile").arg(&src).arg("-o").arg(&wasm).status().unwrap().success());
    assert!(Command::new(AIPL).args(["compile", "--exe"]).arg(&src).arg("-o").arg(&exe).status().unwrap().success());
    let out = vec![
        ("VM", Command::new(AIPL).arg("eval").arg(&src).arg("--").args(args).output().unwrap()),
        ("aipl-run", Command::new(RUNNER).arg(&wasm).args(args).output().unwrap()),
        ("native", Command::new(&exe).args(args).output().unwrap()),
    ];
    let _ = std::fs::remove_dir_all(&dir);
    out
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).split_inclusive('\n').filter(|l| !l.starts_with("[AIPL")).collect()
}

fn expected(name: &str) -> String {
    std::fs::read_to_string(root().join(format!("tests/aipl/libs/{name}.expected"))).unwrap()
}

#[test]
fn each_library_demo_prints_its_expected_output_everywhere() {
    for (name, args) in [("set_demo", vec![]), ("random_demo", vec![]), ("path_demo", vec![]), ("cli_demo", vec!["-v", "in.txt"])] {
        let want = expected(name);
        for (backend, o) in runs(name, &args) {
            assert!(o.status.success(), "{name} {backend}: {}", String::from_utf8_lossy(&o.stderr));
            assert_eq!(stdout(&o), want, "{name} {backend}");
        }
    }
}

#[test]
fn cli_prints_usage_for_help_and_names_a_bad_argument() {
    for (backend, o) in runs("cli_demo", &["--help"]) {
        assert_eq!(o.status.code(), Some(0), "{backend}");
        assert!(stdout(&o).ends_with("usage: wc [options] file [more...]\ncount the words in files\n\n  -v, --verbose  print each file's count\n  -q, --quiet    print nothing\n  -m, --min MIN  skip words shorter than this (default 1)\n      --out OUT  write here\n  file           the file to read\n  more...        other files\n  -h, --help     show this help\n"), "{backend}: {}", stdout(&o));
    }
    for (backend, o) in runs("cli_demo", &["--frob"]) {
        assert_eq!(o.status.code(), Some(2), "{backend}");
        assert!(String::from_utf8_lossy(&o.stderr).contains("wc: unknown option '--frob' (wc --help shows the options)"), "{backend}: {}", String::from_utf8_lossy(&o.stderr));
    }
}
