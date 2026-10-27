//! The stage-0 compiler (bootstrap/README.md). The checked-in seed builds the
//! current aipl_src/aipl.aipl (stage 1), stage 1 builds it again (stage 2),
//! and stage 2 is byte for byte the executable the Rust toolchain builds from
//! the same source: two independent compilers agree on the compiler, so the
//! seed carries nothing the source does not say. Stage 2 also rebuilds itself
//! unchanged. And the AIPL `aipl` command compiles every example, benchmark,
//! and test program to the same bytes as the Rust one, with the same errors,
//! and gives the same `aipl doc` output.
//! Linux x86-64 only, as the seed is.

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
#[cfg_attr(not(all(target_os = "linux", target_arch = "x86_64")),
           ignore = "runs Linux x86-64 executables (the native seed)")]
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

/// The bootstrap where openat2 is refused (Linux before 5.6, Rosetta 2, an old
/// container profile: tools/without_openat2.py): the seed, and the compiler it
/// builds, open the source files with openat, and build the same bytes as
/// where openat2 works.
#[test]
#[cfg_attr(not(all(target_os = "linux", target_arch = "x86_64")),
           ignore = "runs Linux x86-64 executables (the native seed)")]
fn the_bootstrap_finishes_where_openat2_is_refused() {
    let dir = scratch("bootstrap_no_openat2");
    let seed = root().join("bootstrap/aipl-linux-x86_64");
    let plain = dir.join("plain");
    build_aipl(&seed, &plain);
    let helper = root().join("tools/without_openat2.py");
    let build = |how: &[&str], compiler: &Path, out: &Path| {
        let r = run(Command::new("python3").arg(&helper).args(how).arg(compiler).args(["compile", "--exe", "aipl_src/aipl.aipl", "-o"]).arg(out));
        assert_eq!(r.status.code(), Some(0), "{how:?} {}: {}", compiler.display(), String::from_utf8_lossy(&r.stderr));
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(out, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    let (enosys, eperm, again) = (dir.join("enosys"), dir.join("eperm"), dir.join("again"));
    build(&[], &seed, &enosys);
    build(&["--eperm"], &seed, &eperm);
    // and the compiler the seed built, building itself, with the call refused
    build(&[], &enosys, &again);
    let bytes = |p: &Path| std::fs::read(p).unwrap();
    assert!(bytes(&enosys) == bytes(&plain), "the seed builds other bytes where openat2 returns ENOSYS");
    assert!(bytes(&eperm) == bytes(&plain), "the seed builds other bytes where openat2 returns EPERM");
    assert!(bytes(&again) == bytes(&plain), "the compiler the seed built builds other bytes where openat2 is refused");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[cfg_attr(not(all(target_os = "linux", target_arch = "x86_64")),
           ignore = "runs Linux x86-64 executables (the native seed)")]
fn the_aipl_command_matches_the_rust_command() {
    let dir = scratch("bootstrap_command");
    let aipl = dir.join("aipl");
    build_aipl(Path::new(AIPL), &aipl);

    // every example, benchmark, and test program, as wasm and as an executable
    let mut paths = Vec::new();
    // every program below tests/aipl too, a directory per feature
    let mut dirs = vec![root().join("examples"), root().join("tests/aipl")];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() && d != root().join("examples") {
                dirs.push(p);
            } else {
                paths.push(p);
            }
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

    // flags: a memory limit, listen grants, and a served address in the
    // module or executable, and a size or address that is not one refused
    // with the same message
    for args in [
        &["compile", "--max-memory", "4M"][..],
        &["compile", "--exe", "--max-memory", "4M"],
        &["compile", "--exe", "--listen", "127.0.0.1:8080", "--listen", "[::1]:9000"],
        &["compile", "--exe", "--serve", "[::1]:8080", "--listen", "127.0.0.1:9000"],
    ] {
        let (want, got) = (dir.join("want"), dir.join("got"));
        let r = run(Command::new(AIPL).args(args).arg("tests/aipl/memory/hog.aipl").arg("-o").arg(&want));
        let a = run(Command::new(&aipl).args(args).arg("tests/aipl/memory/hog.aipl").arg("-o").arg(&got));
        assert!(r.status.success() && a.status.success(), "{args:?}: {}", String::from_utf8_lossy(&a.stderr));
        assert!(std::fs::read(&want).unwrap() == std::fs::read(&got).unwrap(), "{args:?}: different bytes");
    }
    for args in [&["compile", "--max-memory", "3G"][..], &["compile", "--exe", "--listen", "127.0.0.1"], &["compile", "--exe", "--serve", "8080"]] {
        let r = run(Command::new(AIPL).args(args).arg("tests/aipl/memory/hog.aipl").arg("-o").arg(dir.join("x")));
        let a = run(Command::new(&aipl).args(args).arg("tests/aipl/memory/hog.aipl").arg("-o").arg(dir.join("x")));
        assert_eq!((a.status.code(), String::from_utf8_lossy(&a.stderr).to_string()), (r.status.code(), String::from_utf8_lossy(&r.stderr).to_string()), "{args:?}");
    }

    // usage and version
    assert_eq!(run(&mut Command::new(&aipl)).status.code(), Some(2));
    assert_eq!(run(Command::new(&aipl).args(["compile", "--sandbox", "x.aipl", "-o", "y"])).status.code(), Some(2));
    let v = run(Command::new(&aipl).arg("--version"));
    assert_eq!(String::from_utf8_lossy(&v.stdout), format!("aipl {}\n", env!("CARGO_PKG_VERSION")));

    // aipl doc (aipl_src/doc.aipl, which both run): the same output, errors,
    // and status
    let docs: &[&[&str]] = &[
        &["doc"],
        &["doc", "stream"],
        &["doc", "text.split"],
        &["doc", "--find", "utf"],
        &["doc", "--private", "tests/aipl/doc/shapes.aipl"],
        &["doc", "--check", "tests/aipl/doc/shapes.aipl"],
        &["doc", "no_such"],
    ];
    for args in docs {
        let r = run(Command::new(AIPL).args(*args));
        let a = run(Command::new(&aipl).args(*args));
        assert_eq!((a.status.code(), &a.stdout, &a.stderr), (r.status.code(), &r.stdout, &r.stderr), "{args:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
