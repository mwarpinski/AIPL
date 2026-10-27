//! when, for, and variadic and/or (AIPL_SPEC.md 4.M, src/sugar.rs):
//! tests/aipl/sugar/main.aipl prints the same in the VM, under aipl-run, and
//! natively; the self-hosted aipl compiles it (and its test build) to the
//! Rust toolchain's bytes; and each mistake is one message in both, a test's
//! body read only in a test build. new, arr.new, and arr.of (the array
//! literal), which name their allocator, are rewritten here too.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");
const MAIN: &str = "tests/aipl/sugar/main.aipl";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_sugar_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(exe: &Path, args: &[&str]) -> Output {
    Command::new(exe).args(args).current_dir(root()).env("AIPL_STD", root().join("aipl_src/std/")).output().unwrap()
}

#[test]
fn the_forms_run_alike_in_every_backend() {
    let want = "end evaluated\nsum 6\nodd 16\npairs 3\na1 a2 a3 = false\no1 o2 = true\nlib 3\narr.of 5 4 0 8\nnamed 30 40 6 9\nzero 0 0 true low 0.0 []\n";
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
    let t = run(Path::new(AIPL), &["test", MAIN]);
    assert!(t.status.success(), "{}", String::from_utf8_lossy(&t.stdout));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn both_toolchains_rewrite_alike_with_the_same_errors() {
    let dir = scratch("parity");
    let me = dir.join("aipl");
    assert!(run(Path::new(AIPL), &["compile", "--exe", "aipl_src/aipl.aipl", "-o", me.to_str().unwrap()]).status.success());
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&me, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (want, got) = (dir.join("want.wasm"), dir.join("got.wasm"));
    for tests in [false, true] {
        let mut args = vec!["compile"];
        if tests {
            args.push("--tests");
        }
        let r = run(Path::new(AIPL), &[&args[..], &[MAIN, "-o", want.to_str().unwrap()]].concat());
        let a = run(&me, &[&args[..], &[MAIN, "-o", got.to_str().unwrap()]].concat());
        assert!(r.status.success() && a.status.success(), "{}", String::from_utf8_lossy(&a.stderr));
        assert!(std::fs::read(&want).unwrap() == std::fs::read(&got).unwrap(), "tests {tests}: the builds differ");
    }

    // (item, test build?, the message, or "" for none)
    let cases: &[(&str, bool, &str)] = &[
        ("(fn f [] -> void (when))", false, "3:20: when takes a condition and a body: (when c body...)"),
        ("(fn f [] -> void (for i 0))", false, "3:20: for is (for i start end body...): i from start up to end, end left out"),
        ("(fn f [] -> void (for 1 0 5 (block)))", false, "3:20: for is (for i start end body...): i from start up to end, end left out"),
        // a test's body is read only in a test build
        ("(test \"t\" (for i 0))", false, ""),
        ("(test \"t\" (for i 0))", true, "3:13: for is (for i start end body...): i from start up to end, end left out"),
        ("(test \"t\" (println \"{\"))", false, ""),
        ("(test \"t\" (println \"{\"))", true, "3:22: unclosed { in a format string (write {{ for a brace)"),
        // new and arr.new name their allocator (sugar.rs), std/alloc imported for them
        ("(struct P [x:i32]) (fn g [] -> i32 (let p:(ptr P) (new P (call alloc.forever) [x:4])) (get p P.x))", false, ""),
        // the zeroed (new S a) is gone: every field is named, an empty one given its zero
        ("(struct P [x:i32]) (fn g [] -> i32 (let p:(ptr P) (new P (call alloc.forever))) (get p P.x))", false, "3:53: new is (new S a [x:v y:v]): the struct, the allocator to make it from, then each field by name; a field that starts empty is given its zero, (zero T)"),
        ("(struct V [x:i32 y:i32] (value)) (fn g [] -> V (make V 1 2))", false, "3:50: a value struct's fields are given by name, each once and in order: (make V [x:v y:v])"),
        ("(struct P [x:i32]) (fn g [a:alloc.Allocator] -> i32 (arr.len (arr.new i32 a 4)))", false, ""),
        ("(struct P [x:i32]) (fn g [] -> void (let p:(ptr P) (new P)))", false, "3:54: new takes the allocator to make it from, then each field by name: (new S a [x:v y:v]), e.g. (new S (call alloc.default) [x:1 y:2]), or (call alloc.forever) for memory kept until the program exits"),
        ("(struct P [x:i32]) (fn g [] -> void (let p:(ptr P) (new P 1 2)))", false, "3:54: new is (new S a [x:v y:v]): the struct, the allocator to make it from, then each field by name; a field that starts empty is given its zero, (zero T)"),
        ("(fn g [] -> void (let xs:(arr i32) (arr.new i32 3)))", false, "3:38: arr.new takes the allocator to make it from: (arr.new T a n), e.g. (arr.new T (call alloc.default) n), or (arr.new T (call alloc.forever) n) for memory kept until the program exits"),
        ("(fn g [] -> void (let xs:(arr i32) (arr.new i32)))", false, "3:38: arr.new is (arr.new T a n): the element type, the allocator to make the array from, then its length"),
        // arr.of, the array literal: an allocator, then the values (or none)
        ("(fn g [a:alloc.Allocator] -> i32 (arr.len (arr.of i32 a 4 5 6)))", false, ""),
        ("(fn g [a:alloc.Allocator] -> i32 (arr.len (arr.of i32 a)))", false, ""),
        ("(fn g [] -> i32 (arr.len (arr.of i32 1 2 3)))", false, "3:28: arr.of takes the allocator to make it from, then the values: (arr.of T a x y ...), e.g. (arr.of i32 (call alloc.default) 1 2 3), or (arr.of i32 a) for an empty array"),
        ("(fn g [] -> i32 (arr.len (arr.of i32)))", false, "3:28: arr.of takes the allocator to make it from"),
        ("(fn g [] -> i32 (arr.len (arr.of (arr i32) 7)))", false, "3:28: arr.of takes the allocator to make it from"),
        // the forms inside a struct's named fields are read like any others
        ("(struct P [xs:(arr i32)]) (fn g [a:alloc.Allocator] -> (ptr P) (new P a [xs:(arr.new i32 a)]))", false, "3:79: arr.new takes the allocator to make it from"),
        // the forms in a formatting placeholder's code are rewritten too
        ("(struct P [x:i32]) (fn g [a:alloc.Allocator] -> void (println \"{(arr.len (arr.new i32 a 3))} {(arr.len (arr.of i32 a 1 2))} {(get (new P a [x:7]) P.x)} {(and true true false)} {(zero f64)}\"))", false, ""),
        ("(fn g [] -> void (println \"{(arr.len (arr.new i32 3))}\"))", false, "arr.new takes the allocator to make it from"),
        // (zero T): the zero of any type that has one, written out with generics (generics.rs zero_of)
        ("(fn (d T) [] -> T (zero T)) (fn g [] -> f64 (call (d f64)))", false, ""),
        ("(union U [(a) (b x:i32)]) (fn g [] -> void (let u:U (zero U)))", false, "3:55: U is a union, which has no zero: write the variant you mean with make"),
        ("(fn (d T) [] -> T (zero T)) (union U [(a)]) (fn g [] -> void (let u:U (call (d U))))", false, "3:21: U is a union, which has no zero"),
        ("(struct P [x:i32]) (fn g [] -> void (let p:P (zero P)))", false, "3:48: P is a struct, used only through a pointer: (zero (ptr P)) is a null pointer"),
        ("(fn g [] -> void (let f:(fn [] -> i32) (zero (fn [] -> i32))))", false, "3:42: (fn [] -> i32) is a function reference, which has no zero"),
        ("(fn g [] -> i32 (zero i32 i32))", false, "3:19: zero is (zero T): the zero of the type T"),
        ("(struct P [x:i32]) (test \"t\" (let p:(ptr P) (new P)))", false, ""),
        ("(struct P [x:i32]) (test \"t\" (let p:(ptr P) (new P)))", true, "3:47: new takes the allocator to make it from"),
    ];
    for (k, (item, tests, msg)) in cases.iter().enumerate() {
        let file = dir.join(format!("e{k}.aipl"));
        // (import alloc) on the first line: a file names what it imports
        // (AIPL_SPEC.md 11), and these items' errors are on line 3
        std::fs::write(&file, format!("(module e (import alloc)\n  (fn main [] -> i32 0)\n  {item})\n")).unwrap();
        let mut args = vec!["compile"];
        if *tests {
            args.push("--tests");
        }
        let (f, o) = (file.to_str().unwrap(), dir.join("x.wasm"));
        let r = run(Path::new(AIPL), &[&args[..], &[f, "-o", o.to_str().unwrap()]].concat());
        let a = run(&me, &[&args[..], &[f, "-o", o.to_str().unwrap()]].concat());
        let rerr = String::from_utf8_lossy(&r.stderr);
        if msg.is_empty() {
            assert!(r.status.success() && a.status.success(), "{item} (tests {tests}): {rerr} / {}", String::from_utf8_lossy(&a.stderr));
        } else {
            assert!(rerr.contains(msg), "{item}\n  expected: {msg}\n  rust: {rerr}");
            assert_eq!(String::from_utf8_lossy(&a.stderr), rerr, "{item}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
