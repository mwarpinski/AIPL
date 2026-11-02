//! A name's scope is its block or match arm (docs/design/LANGUAGE_DECISIONS.md
//! item 5; src/scopes.rs): tests/aipl/scopes/main.aipl, which declares names
//! again in sibling blocks, arms, loops, and clauses, with other types and in
//! a template, prints the same in the VM, under aipl-run, and natively; the
//! self-hosted aipl compiles it, and a program with no templates, to the
//! Rust toolchain's bytes; and shadowing a name still visible, renamed or
//! not, is one message in both, naming it as written.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");
const MAIN: &str = "tests/aipl/scopes/main.aipl";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_scopes_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(exe: &Path, args: &[&str]) -> Output {
    Command::new(exe).args(args).current_dir(root()).env("AIPL_STD", root().join("aipl_src/std/")).output().unwrap()
}

#[test]
fn sibling_scopes_run_alike_in_every_backend() {
    // 4 + 10 + 7 + 100 + (0 + 1 + 2) + 2 + 5
    let want = "scopes 131 4.0 12.0 4 2\n";
    let dir = scratch("run");
    let (wasm, exe) = (dir.join("p.wasm"), dir.join("p"));
    assert!(run(Path::new(AIPL), &["compile", MAIN, "-o", wasm.to_str().unwrap()]).status.success());
    assert!(run(Path::new(AIPL), &["compile", "--exe", MAIN, "-o", exe.to_str().unwrap()]).status.success());
    let outs = [run(Path::new(AIPL), &["eval", MAIN]), run(Path::new(RUNNER), &[wasm.to_str().unwrap()]), run(&exe, &[])];
    for (o, backend) in outs.iter().zip(["VM", "aipl-run", "native"]) {
        assert!(o.status.success(), "{backend}: {}", String::from_utf8_lossy(&o.stderr));
        let got: String = String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.starts_with("[AIPL")).map(|l| format!("{l}\n")).collect();
        assert_eq!(got, want, "{backend}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn both_toolchains_rename_alike_with_the_same_errors() {
    let dir = scratch("parity");
    let me = dir.join("aipl");
    assert!(run(Path::new(AIPL), &["compile", "--exe", "aipl_src/aipl.aipl", "-o", me.to_str().unwrap()]).status.success());
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&me, std::fs::Permissions::from_mode(0o755)).unwrap();
    // with templates (std's), and a program with none, which the generics
    // pass otherwise copies as it is
    let plain = dir.join("plain.aipl");
    std::fs::write(&plain, "(module plain (fn main [] -> i32 (let t:i32 0) (block (let v:i32 2) (set! t v)) (block (let v:i64 3i64) (set! t (+ t (i32.wrap v)))) t))").unwrap();
    for prog in [root().join(MAIN), plain] {
        let (want, got) = (dir.join("want.wasm"), dir.join("got.wasm"));
        let p = prog.to_str().unwrap();
        let r = run(Path::new(AIPL), &["compile", p, "-o", want.to_str().unwrap()]);
        let a = run(&me, &["compile", p, "-o", got.to_str().unwrap()]);
        assert!(r.status.success() && a.status.success(), "{p}: {}", String::from_utf8_lossy(&a.stderr));
        assert!(std::fs::read(&want).unwrap() == std::fs::read(&got).unwrap(), "{p}: the builds differ");
    }
    // a name still visible is not shadowed, renamed or not
    let cases: &[(&str, &str)] = &[
        ("(fn g [] -> i32 (block (let y:f64 1.5) 0) (block (let y:i32 3) (block (let y:i32 4) y)))", "3:73: Cannot shadow existing variable 'y'"),
        ("(union U [(a x:i32) (b x:f64)]) (fn g [u:U] -> i32 (match u (U.a [x] x) (U.b [x] (let x:i32 1) x)))", "3:84: Cannot shadow existing variable 'x'"),
        ("(fn g [] -> i32 (for i 0 2 (block)) (for i 0 3 (for i 0 1 (block))) 0)", "3:50: Cannot shadow existing variable 'i' in loop"),
        ("(fn g [n:i32] -> i32 (let n:i32 1) n)", "3:24: Cannot shadow existing variable 'n'"),
    ];
    for (k, (item, msg)) in cases.iter().enumerate() {
        let file = dir.join(format!("e{k}.aipl"));
        std::fs::write(&file, format!("(module e\n  (fn main [] -> i32 0)\n  {item})\n")).unwrap();
        let (f, o) = (file.to_str().unwrap(), dir.join("x.wasm"));
        let r = run(Path::new(AIPL), &["compile", f, "-o", o.to_str().unwrap()]);
        let a = run(&me, &["compile", f, "-o", o.to_str().unwrap()]);
        let rerr = String::from_utf8_lossy(&r.stderr);
        assert!(rerr.contains(msg), "{item}\n  expected: {msg}\n  rust: {rerr}");
        assert_eq!(String::from_utf8_lossy(&a.stderr), rerr, "{item}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
