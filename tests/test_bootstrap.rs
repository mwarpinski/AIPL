//! The stage-0 compiler (bootstrap/README.md). The checked-in seed builds the
//! current aipl_src/aipl.aipl (stage 1), stage 1 builds it again (stage 2),
//! and stage 2 is byte for byte the executable the Rust toolchain builds from
//! the same source: two independent compilers agree on the compiler, so the
//! seed carries nothing the source does not say. Stage 2 also rebuilds itself
//! unchanged. And the AIPL `aipl` command compiles every example, benchmark,
//! and test program to the same bytes as the Rust one, with the same errors.
//! Linux x86-64 only, as the seed is.
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// An empty scratch directory under target/, one per test.
fn scratch(name: &str) -> PathBuf {
    let dir = root().join("target").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Runs `cmd` from the repository root with the standard library there,
/// retrying while a just-written executable is still busy (another test
/// thread may hold it open across a fork).
fn run(cmd: &mut Command) -> Output {
    cmd.current_dir(root()).env("AIPL_STD", root().join("aipl_src/std/"));
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for _ in 0..100 {
        match cmd.output() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => std::thread::sleep(std::time::Duration::from_millis(10)),
            r => return r.unwrap(),
        }
    }
    panic!("{cmd:?}: executable stayed busy");
}

/// `compiler` builds the aipl command into `out`, which is made executable.
fn build_aipl(compiler: &Path, out: &Path) {
    let r = run(Command::new(compiler).args(["compile", "--exe", "aipl_src/aipl.aipl", "-o"]).arg(out));
    assert_eq!(r.status.code(), Some(0), "{}: {}", compiler.display(), String::from_utf8_lossy(&r.stderr));
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(out, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn the_seed_builds_the_compiler_the_rust_toolchain_builds() {
    let dir = scratch("bootstrap_stages");
    let rust_built = dir.join("rust_built");
    build_aipl(Path::new(AIPL), &rust_built);
    let seed = root().join("bootstrap/aipl-linux-x86_64");
    let stages: Vec<PathBuf> = (1..=3).map(|n| dir.join(format!("stage{n}"))).collect();
    build_aipl(&seed, &stages[0]);
    build_aipl(&stages[0], &stages[1]);
    build_aipl(&stages[1], &stages[2]);
    let bytes = |p: &Path| std::fs::read(p).unwrap();
    assert!(
        bytes(&stages[1]) == bytes(&rust_built),
        "stage 2 (seed -> stage 1 -> stage 2) differs from the Rust-built compiler: \
         the seed or one of the two toolchains is wrong (bootstrap/README.md)"
    );
    assert!(bytes(&stages[2]) == bytes(&stages[1]), "stage 2 does not rebuild itself unchanged");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_aipl_command_matches_the_rust_command() {
    let dir = scratch("bootstrap_command");
    let aipl = dir.join("aipl");
    build_aipl(Path::new(AIPL), &aipl);

    // every example, benchmark, and test program, as wasm and as an executable
    let mut paths = Vec::new();
    for sub in ["examples", "tests/aipl"] {
        for e in std::fs::read_dir(root().join(sub)).unwrap() {
            paths.push(e.unwrap().path());
        }
    }
    for e in std::fs::read_dir(root().join("benchmarks")).unwrap() {
        let d = e.unwrap().path();
        if d.is_dir() {
            for f in std::fs::read_dir(&d).unwrap() {
                paths.push(f.unwrap().path());
            }
        }
    }
    paths.retain(|p| p.extension().is_some_and(|x| x == "aipl"));
    paths.sort();
    let mut compared = 0;
    for p in &paths {
        let rel = p.strip_prefix(root()).unwrap().display().to_string();
        for exe in [false, true] {
            let (want, got) = (dir.join("want"), dir.join("got"));
            let mut args = vec!["compile"];
            if exe {
                args.push("--exe");
            }
            let r = run(Command::new(AIPL).args(&args).arg(&rel).arg("-o").arg(&want));
            let a = run(Command::new(&aipl).args(&args).arg(&rel).arg("-o").arg(&got));
            assert_eq!(a.status.code(), r.status.code(), "{rel} (exe: {exe}): {}", String::from_utf8_lossy(&a.stderr));
            if r.status.success() {
                assert!(std::fs::read(&want).unwrap() == std::fs::read(&got).unwrap(), "{rel} (exe: {exe}): different bytes");
                compared += 1;
            }
            let _ = std::fs::remove_file(&want);
            let _ = std::fs::remove_file(&got);
        }
    }
    assert!(compared >= 40, "only {compared} compiles compared");

    // a type error: the same message and status
    let bad = dir.join("bad.aipl");
    std::fs::write(&bad, "(module bad\n  (fn f [x:i32] -> i32 x)\n  (fn main [] -> i32 (call f true)))\n").unwrap();
    let r = run(Command::new(AIPL).arg("compile").arg(&bad).arg("-o").arg(dir.join("x.wasm")));
    let a = run(Command::new(&aipl).arg("compile").arg(&bad).arg("-o").arg(dir.join("x.wasm")));
    assert_eq!(a.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&a.stderr), String::from_utf8_lossy(&r.stderr));

    // usage and version
    assert_eq!(run(&mut Command::new(&aipl)).status.code(), Some(2));
    assert_eq!(run(Command::new(&aipl).args(["compile", "--sandbox", "x.aipl", "-o", "y"])).status.code(), Some(2));
    let v = run(Command::new(&aipl).arg("--version"));
    assert_eq!(String::from_utf8_lossy(&v.stdout), format!("aipl {}\n", env!("CARGO_PKG_VERSION")));
    let _ = std::fs::remove_dir_all(&dir);
}
