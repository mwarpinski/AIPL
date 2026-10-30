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

/// `compiler` builds the aipl command into `out`, which it makes executable
/// itself (the self-hosted compiler with `fs.chmod`).
fn build_aipl(compiler: &Path, out: &Path) {
    // on AArch64 Linux the executable is for AArch64, which no compiler there builds by default (Rust's
    // default is the launcher bundle) and every one knows how to build
    let target: &[&str] = if cfg!(target_arch = "aarch64") { &["--target", "arm64"] } else { &[] };
    let r = run(Command::new(compiler).args(["compile", "--exe"]).args(target).args(["--allow-run", "*", "aipl_src/aipl.aipl", "-o"]).arg(out));
    assert_eq!(r.status.code(), Some(0), "{}: {}", compiler.display(), String::from_utf8_lossy(&r.stderr));
    assert_executable(out, compiler);
}

/// `out` has mode 0755, whatever the umask: the compiler set it.
fn assert_executable(out: &Path, compiler: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(out).unwrap().permissions().mode() & 0o7777;
    assert_eq!(mode, 0o755, "{} wrote {} without its execute bit", compiler.display(), out.display());
}

#[test]
#[cfg_attr(not(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64"))),
           ignore = "runs Linux x86-64 and AArch64 executables (the seeds)")]
fn the_seed_builds_the_compiler_the_rust_toolchain_builds() {
    let dir = scratch("bootstrap_stages");
    let rust_built = dir.join("rust_built");
    build_aipl(Path::new(AIPL), &rust_built);
    let seed = root().join(if cfg!(target_arch = "aarch64") { "bootstrap/aipl-linux-aarch64" } else { "bootstrap/aipl-linux-x86_64" });
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

/// A compiler writes its output beside it and renames it into place, so it can
/// replace the very file that is running (an in-place write is "Text file
/// busy" on Linux), and leaves nothing behind, a failure included (SYS_PLAN.md section 10).
#[test]
#[cfg_attr(not(all(target_os = "linux", target_arch = "x86_64")),
           ignore = "runs Linux x86-64 executables (the native seed)")]
fn a_compiler_replaces_the_file_it_is_running_from() {
    let dir = scratch("bootstrap_replace");
    let compiler = dir.join("aipl");
    build_aipl(Path::new(AIPL), &compiler);
    let before = std::fs::read(&compiler).unwrap();
    let r = run(Command::new(&compiler).args(["compile", "--exe", "--allow-run", "*", "aipl_src/aipl.aipl", "-o"]).arg(&compiler));
    assert_eq!(r.status.code(), Some(0), "{}", String::from_utf8_lossy(&r.stderr));
    assert_executable(&compiler, &compiler);
    assert!(std::fs::read(&compiler).unwrap() == before, "the replacement differs from the Rust-built compiler");
    let left: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(left.len(), 1, "files left beside the output: {left:?}");
    // an output that cannot be replaced (a directory) is an error that leaves
    // no temporary file either
    let into = dir.join("into");
    std::fs::create_dir(&into).unwrap();
    let r = run(Command::new(&compiler).args(["compile", "--exe", "--allow-run", "*", "aipl_src/aipl.aipl", "-o"]).arg(&into));
    assert_eq!(r.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&r.stderr).contains("cannot write"), "{}", String::from_utf8_lossy(&r.stderr));
    let mut left: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    left.sort();
    assert_eq!(left, ["aipl", "into"], "files left beside the output");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `aipl run FILE.aipl ARGS`: the program is built into a temporary file,
/// started with the arguments and this command's own standard input, and
/// removed; its exit status is the command's (SYS_PLAN.md; aipl_standalone.md,
/// S2). Only an `aipl` built with `--allow-run '*'` can start it.
#[test]
#[cfg_attr(not(all(target_os = "linux", target_arch = "x86_64")),
           ignore = "runs Linux x86-64 executables (the native seed)")]
fn aipl_run_builds_starts_and_removes_a_program() {
    use std::io::Write;
    use std::process::Stdio;
    let dir = scratch("bootstrap_run");
    let aipl = dir.join("aipl");
    build_aipl(Path::new(AIPL), &aipl);
    let tmp = dir.join("tmp");
    std::fs::create_dir(&tmp).unwrap();
    let target = "tests/aipl/sys/run_target.aipl";
    let run_with = |args: &[&str], input: &str| {
        let mut child = Command::new(&aipl).arg("run").args(args).env("TMPDIR", &tmp).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        child.wait_with_output().unwrap()
    };
    let text = |b: &[u8]| String::from_utf8_lossy(b).to_string();
    let nothing_left = || assert_eq!(std::fs::read_dir(&tmp).unwrap().count(), 0, "a temporary file was left");

    // arguments and standard input reach the program; its output is ours
    let o = run_with(&[target, "a", "b c"], "hello");
    assert_eq!((o.status.code(), text(&o.stdout)), (Some(0), "args 3\narg a\narg b c\nstdin hello\n".to_string()), "{}", text(&o.stderr));
    nothing_left();

    // its exit status is ours
    let o = run_with(&[target, "seven"], "");
    assert_eq!(o.status.code(), Some(7), "{}", text(&o.stderr));
    nothing_left();

    // flags before the file are the compiler's; after it, the program's
    let o = run_with(&["--sandbox", "--no-opt", target, "--sandbox"], "");
    assert_eq!((o.status.code(), text(&o.stdout)), (Some(0), "args 2\narg --sandbox\nstdin \n".to_string()), "{}", text(&o.stderr));
    nothing_left();

    // a program that does not compile: the compile error and status 1, nothing started
    let bad = dir.join("bad.aipl");
    std::fs::write(&bad, "(module bad\n  (fn f [x:i32] -> i32 x)\n  (fn main [] -> i32 (call f true)))\n").unwrap();
    let want = run(Command::new(AIPL).arg("compile").arg(&bad).arg("-o").arg(dir.join("x.wasm")));
    let o = run_with(&[bad.to_str().unwrap()], "");
    assert_eq!((o.status.code(), text(&o.stdout), text(&o.stderr)), (Some(1), String::new(), text(&want.stderr)));
    nothing_left();

    // a compiled module (x.wasm) runs as the Rust command runs it under aipl-run,
    // built natively as it stands: the same output and status, a trap named by
    // the file, and (a status of 126 or more cannot be exited) a line more for it
    let wasm = dir.join("target.wasm");
    let r = run(Command::new(AIPL).args(["compile", target, "-o"]).arg(&wasm));
    assert!(r.status.success(), "{}", text(&r.stderr));
    let trap_src = dir.join("trap.aipl");
    std::fs::write(&trap_src, "(module trap\n  (import alloc)\n  (fn main [] -> i32\n    (println \"before\")\n    (let xs:(arr i32) (arr.new i32 (call alloc.forever) 2))\n    (println \"{(arr.get i32 xs 5)}\")\n    0))\n").unwrap();
    let trap_wasm = dir.join("trap.wasm");
    let r = run(Command::new(AIPL).arg("compile").arg(&trap_src).arg("-o").arg(&trap_wasm));
    assert!(r.status.success(), "{}", text(&r.stderr));
    for args in [&["a", "b c"][..], &["seven"]] {
        let want = {
            let mut child = Command::new(AIPL).arg("run").arg(&wasm).arg("--").args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
            child.stdin.take().unwrap().write_all(b"hello").unwrap();
            child.wait_with_output().unwrap()
        };
        let got = run_with(&[wasm.to_str().unwrap(), "--"].iter().chain(args.iter()).copied().collect::<Vec<_>>(), "hello");
        assert_eq!((got.status.code(), text(&got.stdout), text(&got.stderr)), (want.status.code(), text(&want.stdout), text(&want.stderr)), "{args:?}");
    }
    let want = run(Command::new(AIPL).arg("run").arg(&trap_wasm));
    let got = run_with(&[trap_wasm.to_str().unwrap()], "");
    assert_eq!((got.status.code(), want.status.code(), text(&got.stdout)), (Some(134), Some(134), text(&want.stdout)));
    assert!(text(&got.stderr).starts_with(&text(&want.stderr)), "{}\n{}", text(&got.stderr), text(&want.stderr));
    assert!(text(&got.stderr).contains("trap.wasm: Array index out of bounds"), "{}", text(&got.stderr));
    nothing_left();
    let gone = run_with(&[dir.join("no_such.wasm").to_str().unwrap()], "");
    assert!(gone.status.code() == Some(1) && text(&gone.stderr).contains("cannot read"), "{}", text(&gone.stderr));

    // usage: no file, or the compile command's -o, which run has no use for
    assert_eq!(run_with(&[], "").status.code(), Some(2));
    assert_eq!(run_with(&["-o", "x", target], "").status.code(), Some(2));

    // built without the grant, the command says so (and exits 1)
    let plain = dir.join("plain");
    let r = run(Command::new(AIPL).args(["compile", "--exe", "aipl_src/aipl.aipl", "-o"]).arg(&plain));
    assert!(r.status.success(), "{}", text(&r.stderr));
    let o = run(Command::new(&plain).args(["run", target]).env("TMPDIR", &tmp));
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o.stderr).contains("built without --allow-run"), "{}", text(&o.stderr));
    nothing_left();
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
        let r = run(Command::new("python3").arg(&helper).args(how).arg(compiler).args(["compile", "--exe", "--allow-run", "*", "aipl_src/aipl.aipl", "-o"]).arg(out));
        assert_eq!(r.status.code(), Some(0), "{how:?} {}: {}", compiler.display(), String::from_utf8_lossy(&r.stderr));
        assert_executable(out, compiler);
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
        // `aipl verify`: the same message or error, and status
        let r = run(Command::new(AIPL).args(["verify", &rel]));
        let a = run(Command::new(&aipl).args(["verify", &rel]));
        assert_eq!((a.status.code(), &a.stdout, &a.stderr), (r.status.code(), &r.stdout, &r.stderr), "{rel}: verify");
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

    // the programs an executable may start (--allow-run), which only a program
    // that starts one carries: the same bytes from both commands; one grant too
    // long for a native executable refused with the same message and status
    for args in [&["compile", "--exe", "--allow-run", "/bin/sh", "--allow-run", "/bin/cat"][..], &["compile", "--exe", "--allow-run", "*", "--listen", "127.0.0.1:8080"]] {
        let (want, got) = (dir.join("want"), dir.join("got"));
        let r = run(Command::new(AIPL).args(args).arg("tests/aipl/sys/spawn_ops.aipl").arg("-o").arg(&want));
        let a = run(Command::new(&aipl).args(args).arg("tests/aipl/sys/spawn_ops.aipl").arg("-o").arg(&got));
        assert!(r.status.success() && a.status.success(), "{args:?}: {}", String::from_utf8_lossy(&a.stderr));
        assert!(std::fs::read(&want).unwrap() == std::fs::read(&got).unwrap(), "{args:?}: different bytes");
    }
    let long = "/".repeat(300);
    let args = ["compile", "--exe", "--allow-run", long.as_str()];
    let r = run(Command::new(AIPL).args(args).arg("tests/aipl/sys/spawn_ops.aipl").arg("-o").arg(dir.join("x")));
    let a = run(Command::new(&aipl).args(args).arg("tests/aipl/sys/spawn_ops.aipl").arg("-o").arg(dir.join("x")));
    assert_eq!(r.status.code(), Some(1));
    assert_eq!((a.status.code(), String::from_utf8_lossy(&a.stderr).to_string()), (r.status.code(), String::from_utf8_lossy(&r.stderr).to_string()));

    // usage and version
    assert_eq!(run(&mut Command::new(&aipl)).status.code(), Some(2));
    assert_eq!(run(Command::new(&aipl).arg("verify")).status.code(), Some(2));
    assert_eq!(run(Command::new(&aipl).args(["verify", "a.aipl", "b.aipl"])).status.code(), Some(2));
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

/// `aipl compile --exe --target arm64`, in the self-hosted command, writes the AArch64
/// executable the Rust command writes (the compiler itself among them), and refuses the flags
/// that are not built for AArch64 with the same message. tools/arm64_bootstrap.sh runs the
/// result on AArch64 Linux: that compiler compiling itself is stage 2 there.
#[test]
#[cfg_attr(not(all(target_os = "linux", target_arch = "x86_64")),
           ignore = "runs Linux x86-64 executables (the native seed)")]
fn the_aipl_command_builds_the_aarch64_executables_the_rust_command_builds() {
    let dir = scratch("bootstrap_arm64");
    let aipl = dir.join("aipl");
    build_aipl(Path::new(AIPL), &aipl);
    for rel in ["aipl_src/aipl.aipl", "examples/quicksort.aipl", "examples/jsonpp.aipl", "benchmarks/nbody/nbody.aipl"] {
        let (want, got) = (dir.join("want"), dir.join("got"));
        let args = ["compile", "--exe", "--target", "arm64", "--allow-run", "*", rel, "-o"];
        let r = run(Command::new(AIPL).args(args).arg(&want));
        let a = run(Command::new(&aipl).args(args).arg(&got));
        assert_eq!(r.status.code(), Some(0), "{rel}: {}", String::from_utf8_lossy(&r.stderr));
        assert_eq!(a.status.code(), Some(0), "{rel}: {}", String::from_utf8_lossy(&a.stderr));
        assert!(std::fs::read(&want).unwrap() == std::fs::read(&got).unwrap(), "{rel}: different bytes");
        let _ = std::fs::remove_file(&want);
        let _ = std::fs::remove_file(&got);
    }
    for flag in [vec!["--sandbox"], vec!["--listen", "127.0.0.1:8080"], vec!["--serve", "127.0.0.1:8080"]] {
        let out = dir.join("never");
        let mut args = vec!["compile", "--exe", "--target", "arm64"];
        args.extend(flag.iter());
        args.extend(["examples/quicksort.aipl", "-o"]);
        let r = run(Command::new(AIPL).args(&args).arg(&out));
        let a = run(Command::new(&aipl).args(&args).arg(&out));
        assert_eq!(a.status.code(), r.status.code(), "{flag:?}");
        assert_eq!(String::from_utf8_lossy(&a.stderr), String::from_utf8_lossy(&r.stderr), "{flag:?}");
        assert!(!out.exists(), "{flag:?}: a file was left");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
