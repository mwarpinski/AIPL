//! A binary that works wherever it is (docs/LAUNCH_CHECKLIST.md): a copy of
//! the Rust `aipl` outside its checkout's target/ runs every kind of command
//! from the library built into it (src/library.rs), written once to its
//! cache; `AIPL_STD` replaces the standard library for programs, not for
//! the toolchain's own modules; and the self-hosted `aipl`, installed as
//! `<prefix>/bin/aipl-native` beside `<prefix>/share/aipl/std/`, finds its
//! standard library there from any directory (aipl_src/stdpath.aipl).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// An empty directory outside the checkout.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_portable_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.starts_with("[AIPL")).map(|l| format!("{l}\n")).collect()
}

fn ok(o: Output, what: &str) -> Output {
    assert!(o.status.success(), "{what}: {}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    o
}

const PROG: &str = r#"(module prog
  (import alloc)
  (import str)
  (import text)
  (import vec)
  (fn main [] -> i32
    (let parts:i32 (call (vec.len str.Bytes) (call text.split (call alloc.default) (call str.from_str "a,b,c") (call str.from_str ","))))
    (println "fields {parts}")
    0)
  (test "three" (expect (eq 3 3))))
"#;

/// The Rust aipl and aipl-run copied to <dir>/bin, run from <dir>/work with
/// their own cache: eval, compile (optimized) and run, a standalone
/// executable, a test, a test build, doc, and eval -e.
#[test]
fn a_copied_aipl_runs_every_command_from_its_built_in_library() {
    let dir = scratch("copy");
    let (bin, work, cache) = (dir.join("bin"), dir.join("work"), dir.join("cache"));
    for d in [&bin, &work] {
        std::fs::create_dir_all(d).unwrap();
    }
    let aipl = bin.join("aipl");
    std::fs::copy(AIPL, &aipl).unwrap();
    std::fs::copy(RUNNER, bin.join("aipl-run")).unwrap();
    std::fs::write(work.join("prog.aipl"), PROG).unwrap();
    let run = |args: &[&str]| Command::new(&aipl).args(args).current_dir(&work).env("AIPL_CACHE", &cache).env_remove("AIPL_STD").output().unwrap();
    assert_eq!(stdout(&ok(run(&["eval", "prog.aipl"]), "eval")), "fields 3\n");
    ok(run(&["compile", "prog.aipl", "-o", "p.wasm"]), "compile");
    assert_eq!(stdout(&ok(Command::new(bin.join("aipl-run")).arg(work.join("p.wasm")).output().unwrap(), "aipl-run")), "fields 3\n");
    ok(run(&["compile", "--exe", "prog.aipl", "-o", "p"]), "compile --exe");
    assert_eq!(stdout(&ok(Command::new(work.join("p")).output().unwrap(), "the executable")), "fields 3\n");
    ok(run(&["test", "prog.aipl"]), "test");
    ok(run(&["compile", "--tests", "prog.aipl", "-o", "t.wasm"]), "compile --tests");
    assert!(stdout(&ok(run(&["doc", "text.split"]), "doc")).starts_with("fn text.split "));
    assert_eq!(stdout(&ok(run(&["eval", "-e", "(+ 40 2)"]), "eval -e")).trim_end(), "42 : i32");
    // the library was written to the cache, and read from there
    let written: Vec<String> = std::fs::read_dir(&cache).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert!(written.iter().any(|n| n.starts_with("library-")), "no library in the cache: {written:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `AIPL_STD` replaces the standard library for a program (here one with a
/// module the real std/ lacks, and without the rest), while the
/// optimizer, built from the library's own std/, still compiles it.
#[test]
fn aipl_std_is_the_standard_library_for_programs_only() {
    let dir = scratch("std");
    let (std, work) = (dir.join("mystd"), dir.join("work"));
    for d in [&std, &work] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(std.join("greet.aipl"), "(module greet\n  (fn answer [] -> i32 (pub) 42))\n").unwrap();
    std::fs::write(work.join("prog.aipl"), "(module prog\n  (import greet)\n  (fn main [] -> i32 (call greet.answer)))\n").unwrap();
    let run = |args: &[&str]| Command::new(AIPL).args(args).current_dir(&work).env("AIPL_STD", &std).output().unwrap();
    let o = run(&["eval", "prog.aipl"]);
    assert_eq!(o.status.code(), Some(42), "eval: {}", String::from_utf8_lossy(&o.stderr));
    ok(run(&["compile", "prog.aipl", "-o", "p.wasm"]), "compile with AIPL_STD");
    let o = Command::new(RUNNER).arg(work.join("p.wasm")).output().unwrap();
    assert_eq!(o.status.code(), Some(42));
    // without it, greet is not found, and the message says where it looked
    let o = Command::new(AIPL).args(["verify", "prog.aipl"]).current_dir(&work).env_remove("AIPL_STD").output().unwrap();
    assert!(!o.status.success() && String::from_utf8_lossy(&o.stderr).contains("Cannot resolve import 'greet'"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The self-hosted aipl (aipl_src/aipl.aipl, built natively) as
/// <prefix>/bin/aipl-native beside <prefix>/share/aipl/std/, started by its
/// path from another directory without AIPL_STD: it compiles a program, and
/// its module is the Rust command's byte for byte.
#[test]
#[cfg_attr(not(all(target_os = "linux", target_arch = "x86_64")), ignore = "builds a Linux x86-64 executable (the native backend)")]
fn the_self_hosted_aipl_finds_its_library_beside_it() {
    let dir = scratch("prefix");
    let (bin, share, work) = (dir.join("bin"), dir.join("share/aipl"), dir.join("work"));
    for d in [&bin, &share, &work] {
        std::fs::create_dir_all(d).unwrap();
    }
    let native = bin.join("aipl-native");
    ok(Command::new(AIPL).args(["compile", "--exe"]).arg(root().join("aipl_src/aipl.aipl")).arg("-o").arg(&native).current_dir(root()).output().unwrap(), "build the self-hosted aipl");
    let copy = Command::new("cp").arg("-r").arg(root().join("aipl_src/std")).arg(share.join("std")).status().unwrap();
    assert!(copy.success());
    std::fs::write(work.join("prog.aipl"), PROG).unwrap();
    ok(Command::new(&native).args(["compile", "prog.aipl", "-o", "n.wasm"]).current_dir(&work).env_remove("AIPL_STD").output().unwrap(), "aipl-native compile");
    ok(Command::new(AIPL).args(["compile", "prog.aipl", "-o", "r.wasm"]).current_dir(&work).output().unwrap(), "aipl compile");
    assert!(std::fs::read(work.join("n.wasm")).unwrap() == std::fs::read(work.join("r.wasm")).unwrap(), "the two commands' modules differ");
    let _ = std::fs::remove_dir_all(&dir);
}
