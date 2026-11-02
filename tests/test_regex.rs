//! std/regex (AIPL_SPEC.md 12.6): tests/aipl/regex/vectors.aipl compiles
//! 1,500 generated patterns (every quantifier greedy and lazy, groups of
//! each kind, classes, anchors, flags) and matches them against generated
//! texts with a newline and a non-ASCII letter, giving what Python's re
//! gives under re.ASCII: whether the pattern compiles, the first match's
//! groups, every match, the text replaced, and the text split
//! (tools/regex_vectors.py); tests/aipl/regex/api.aipl uses errors and
//! their offsets, named groups, templates, find_at, byte spans, and
//! (a|aa)*c on 3,000 a's, which takes a backtracking matcher exponential
//! time; each the same in the VM, under aipl-run, and natively.

use std::path::PathBuf;
use std::process::Command;

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The program's standard output in each backend, run in a scratch directory.
fn outputs(rel: &str, name: &str) -> Vec<(&'static str, String)> {
    let dir = std::env::temp_dir().join(format!("aipl_regex_{}_{name}", std::process::id()));
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
fn std_regex_matches_as_python_does() {
    let want = std::fs::read_to_string(root().join("tests/aipl/regex/vectors.expected")).unwrap();
    for (backend, got) in outputs("tests/aipl/regex/vectors.aipl", "vectors") {
        for (k, (g, w)) in got.lines().zip(want.lines()).enumerate() {
            assert_eq!(g, w, "{backend}, case {}", k + 1);
        }
        assert_eq!(got.lines().count(), want.lines().count(), "{backend}");
    }
}

#[test]
fn the_regex_api_works_alike_everywhere() {
    let want = "missing ), unterminated subpattern at 2 | min repeat greater than max repeat at 1 | lookahead is not supported at 0\ngroups 2: key 3..8, value 9..12, whole 3..12\nx: 640<-width ($), 480<-height ($)\ntemplate: the template names a group the pattern does not have at 0\nfrom 12: 14..24\nsplit [a][b][c][d]\nutf-8: 3..8\n3000 a's: match false\n";
    for (backend, got) in outputs("tests/aipl/regex/api.aipl", "api") {
        assert_eq!(got, want, "{backend}");
    }
}
