//! std/math (AIPL_SPEC.md 12.6): thousands of generated calls, answered by
//! Python (tools/math_vectors.py: // and % as Python writes them, math.gcd,
//! math.isqrt, math.floor, an exact round-half-away, i32 and i64 at their
//! edges), the same through the interpreter, under aipl-run, and as an
//! executable; and the ways a call stops the program.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_math_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn with_input(cmd: &mut Command, input: &[u8]) -> std::process::Output {
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn vectors(dir: &Path, seed: &str) {
    let o = Command::new("python3").arg(root().join("tools/math_vectors.py")).arg("gen").arg(dir).args(["--cases", "3000", "--seed", seed]).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

fn check(dir: &Path, got: &[u8], how: &str) {
    let path = dir.join(format!("got_{how}.txt"));
    std::fs::write(&path, got).unwrap();
    let o = Command::new("python3").arg(root().join("tools/math_vectors.py")).arg("check").arg(dir).arg(&path).output().unwrap();
    assert!(o.status.success(), "{how}: {}", String::from_utf8_lossy(&o.stdout));
}

#[test]
fn std_math_gives_pythons_answers() {
    let dir = scratch("vectors");
    let src = root().join("tests/aipl/math/cases.aipl");
    let (wasm, exe) = (dir.join("p.wasm"), dir.join("p"));
    assert!(Command::new(AIPL).arg("compile").arg(&src).arg("-o").arg(&wasm).status().unwrap().success());
    assert!(Command::new(AIPL).args(["compile", "--exe"]).arg(&src).arg("-o").arg(&exe).status().unwrap().success());
    for seed in ["1", "2"] {
        vectors(&dir, seed);
        let input = std::fs::read(dir.join("cases.txt")).unwrap();
        let o = with_input(&mut Command::new(RUNNER).arg(&wasm), &input);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        check(&dir, &o.stdout, "wasm");
        let o = with_input(&mut Command::new(&exe), &input);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        check(&dir, &o.stdout, "exe");
        // the interpreter is slower: its first 600 cases
        let head: Vec<u8> = input.split_inclusive(|&b| b == b'\n').take(600).flatten().copied().collect();
        let o = with_input(Command::new(AIPL).arg("eval").arg(&src), &head);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let text = String::from_utf8_lossy(&o.stdout);
        let answers: String = text.split_inclusive('\n').filter(|l| !l.starts_with("[AIPL")).collect();
        let want = std::fs::read_to_string(dir.join("expected.txt")).unwrap();
        let first: Vec<&str> = want.lines().take(600).collect();
        let got: Vec<&str> = answers.lines().collect();
        let cases = String::from_utf8_lossy(&head).to_string();
        let (c, g): (Vec<&str>, Vec<&str>) = (cases.lines().collect(), got);
        assert_eq!(g.len(), first.len());
        for k in 0..g.len() {
            let op = c[k].split(' ').next().unwrap();
            if op == "powi" {
                let (x, y): (f64, f64) = (g[k].parse().unwrap(), first[k].parse().unwrap());
                assert!(((x - y) / y).abs() < 1e-12, "{}: {} vs {}", c[k], g[k], first[k]);
            } else if ["floor", "ceil", "trunc", "round", "min_f64", "max_f64", "abs_f64"].contains(&op) {
                assert_eq!(g[k].parse::<f64>().unwrap(), first[k].parse::<f64>().unwrap(), "{}", c[k]);
            } else {
                assert_eq!(g[k], first[k], "{}", c[k]);
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn what_cannot_be_answered_stops_the_program() {
    let eval = |e: &str| Command::new(AIPL).args(["eval", "-e", e]).output().unwrap();
    for (e, why) in [
        ("(call math.abs -2147483648)", "Pre-condition failed in 'math.abs'"),
        ("(call math.abs_i64 -9223372036854775808i64)", "Pre-condition failed in 'math.abs_i64'"),
        ("(call math.pow 10 10)", "Integer overflow in checked.mul"),
        ("(call math.pow 2 -1)", "Pre-condition failed in 'math.pow'"),
        ("(call math.floor_div 1 0)", "Pre-condition failed in 'math.floor_div'"),
        ("(call math.floor_mod 1 0)", "Pre-condition failed in 'math.floor_mod'"),
        ("(call math.isqrt -1)", "Pre-condition failed in 'math.isqrt'"),
        ("(call math.clamp 1 5 2)", "Pre-condition failed in 'math.clamp'"),
    ] {
        let o = eval(e);
        assert!(!o.status.success(), "{e} should stop");
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(err.contains(why), "{e}: {err}");
    }
    // and what is answered at the edges
    for (e, want) in [
        ("(call math.floor_div -2147483647 2)", "-1073741824 : i32"),
        ("(call math.isqrt 2147483647)", "46340 : i32"),
        ("(call math.isqrt_i64 9223372036854775807i64)", "3037000499 : i64"),
        ("(call math.gcd 0 0)", "0 : i32"),
        ("(call math.floor -0.5)", "-1.0 : f64"),
        ("(call math.round 0.49999999999999994)", "0.0 : f64"),
        ("(call math.round -2.5)", "-3.0 : f64"),
        ("(call math.pow_i64 3i64 39)", "4052555153018976267 : i64"),
    ] {
        let o = eval(e);
        assert!(o.status.success(), "{e}: {}", String::from_utf8_lossy(&o.stderr));
        assert!(String::from_utf8_lossy(&o.stdout).trim().ends_with(want), "{e}: {}", String::from_utf8_lossy(&o.stdout));
    }
}
