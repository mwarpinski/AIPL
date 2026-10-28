//! The system host imports (docs/design/SYS_PLAN.md, aipl_sys_v1). `fs.chmod`:
//! tests/aipl/sys/chmod.aipl runs in a directory set up for it under every
//! backend, and what each case gives, and the mode each file ends with, are
//! the same; under `--sandbox` a path that leaves the working directory, in
//! any way, is refused, and the file outside is left as it was. Both
//! toolchains compile the program to the same bytes.

use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_sys_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o7777
}

fn set_mode(path: &Path, m: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(m)).unwrap();
}

/// A directory `work` to run in, beside `outside.txt` and `abs.txt`
/// (an absolute path's file), all mode 0644.
fn setup(top: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let work = top.join("work");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(work.join("dir")).unwrap();
    let (outside, abs) = (top.join("outside.txt"), top.join("abs.txt"));
    for f in [work.join("a.txt"), outside.clone(), abs.clone()] {
        std::fs::write(&f, "x").unwrap();
        set_mode(&f, 0o644);
    }
    set_mode(&work.join("dir"), 0o755);
    symlink("../outside.txt", work.join("link")).unwrap();
    symlink("a.txt", work.join("inside_link")).unwrap();
    (work, outside, abs)
}

fn program(top: &Path, abs: &Path) -> PathBuf {
    let text = std::fs::read_to_string(root().join("tests/aipl/sys/chmod.aipl")).unwrap().replace("@ABS@", abs.to_str().unwrap());
    let src = top.join("chmod.aipl");
    std::fs::write(&src, text).unwrap();
    src
}

fn compile(top: &Path, src: &Path, name: &str, flags: &[&str]) -> PathBuf {
    let out = top.join(name);
    let o = Command::new(AIPL).arg("compile").args(flags).arg(src).arg("-o").arg(&out).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    out
}

fn answers(o: &std::process::Output) -> String {
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).split_inclusive('\n').filter(|l| !l.starts_with("[AIPL")).collect()
}

/// The VM's paths are ordinary: nothing confines them.
/// How a program is run: its name, the command, and what its paths are
/// (0 ordinary, 1 confined to the working directory, 2 `--sandbox`).
type Run<'a> = (&'a str, Box<dyn Fn() -> Command + 'a>, u8);

const ORDINARY: &str = "plain 0\nsetuid -28\nnegative -28\nmissing -44\ndirectory 0\ndotdot 0\nlink 0\ninside_link 0\nabsolute 0\nzero 0\nrestore 0\ntoo_long -21\n";
/// Compiled programs: a relative path stays in the working directory.
const CONFINED: &str = "plain 0\nsetuid -28\nnegative -28\nmissing -44\ndirectory 0\ndotdot -63\nlink -63\ninside_link 0\nabsolute 0\nzero 0\nrestore 0\ntoo_long -21\n";
/// `--sandbox`: and there is no absolute path to name.
const SANDBOXED: &str = "plain 0\nsetuid -28\nnegative -28\nmissing -44\ndirectory 0\ndotdot -63\nlink -63\ninside_link 0\nabsolute -8\nzero 0\nrestore 0\ntoo_long -21\n";

#[test]
fn fs_chmod_is_the_same_in_every_backend() {
    let top = scratch("chmod");
    let (_, _, abs) = setup(&top);
    let src = program(&top, &abs);
    let wasm = compile(&top, &src, "p.wasm", &[]);
    let exe = compile(&top, &src, "p", &["--exe"]);
    let bundle = compile(&top, &src, "p_bundle", &["--exe", "--target", "wasm"]);
    let sandboxed_exe = compile(&top, &src, "ps", &["--exe", "--sandbox"]);
    let sandboxed_bundle = compile(&top, &src, "ps_bundle", &["--exe", "--target", "wasm", "--sandbox"]);
    let runs: Vec<Run> = vec![
        ("VM", Box::new(|| { let mut c = Command::new(AIPL); c.arg("eval").arg(&src); c }), 0),
        ("aipl-run", Box::new(|| { let mut c = Command::new(RUNNER); c.arg(&wasm); c }), 1),
        ("native", Box::new(|| Command::new(&exe)), 1),
        ("bundle", Box::new(|| Command::new(&bundle)), 1),
        ("aipl-run --sandbox", Box::new(|| { let mut c = Command::new(RUNNER); c.arg("--sandbox").arg(&wasm); c }), 2),
        ("native --sandbox", Box::new(|| Command::new(&sandboxed_exe)), 2),
        ("bundle --sandbox", Box::new(|| Command::new(&sandboxed_bundle)), 2),
    ];
    for (how, command, kind) in runs {
        let (work, outside, abs) = setup(&top);
        let o = command().current_dir(&work).output().unwrap();
        let want = [ORDINARY, CONFINED, SANDBOXED][kind as usize];
        assert_eq!(answers(&o), want, "{how}");
        // the files' modes: what the program set, and nothing outside what it may reach
        assert_eq!(mode(&work.join("a.txt")), 0o644, "{how}: a.txt");
        assert_eq!(mode(&work.join("dir")), 0o700, "{how}: dir");
        assert_eq!(mode(&outside), if kind == 0 { 0o600 } else { 0o644 }, "{how}: outside.txt");
        assert_eq!(mode(&abs), if kind == 2 { 0o644 } else { 0o640 }, "{how}: abs.txt");
    }
    let _ = std::fs::remove_dir_all(&top);
}

#[test]
fn both_toolchains_compile_fs_chmod_to_the_same_bytes() {
    let top = scratch("bytes");
    let (_, _, abs) = setup(&top);
    let src = program(&top, &abs);
    for flags in [&["--no-opt"][..], &[][..]] {
        let rust = compile(&top, &src, "r.wasm", flags);
        let o = Command::new(AIPL).arg("compile").args(flags).arg("--self").arg(&src).arg("-o").arg(top.join("s.wasm")).output().unwrap();
        // --self implies --no-opt, and checks the bytes itself
        if flags.contains(&"--no-opt") {
            assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
            assert!(String::from_utf8_lossy(&o.stdout).contains("BIT-FOR-BIT IDENTICAL"), "{}", String::from_utf8_lossy(&o.stdout));
            assert_eq!(std::fs::read(&rust).unwrap(), std::fs::read(top.join("s.wasm")).unwrap());
        }
    }
    let _ = std::fs::remove_dir_all(&top);
}

/// Where `openat2` is refused (Linux before 5.6, Rosetta 2, a container
/// profile: tools/without_openat2.py, Linux x86-64 only), an executable
/// without `--sandbox` opens with plain `openat`, so `fs.chmod` still works,
/// the file that cannot be read included; it gives up the confinement of a
/// relative path, as every open does (aipl_src/native/wasi.aipl).
#[test]
fn fs_chmod_works_where_openat2_is_refused() {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return;
    }
    let top = scratch("fallback");
    let (_, _, abs) = setup(&top);
    let src = program(&top, &abs);
    let exe = compile(&top, &src, "p", &["--exe"]);
    for flag in [&[][..], &["--eperm"][..]] {
        let (work, _, abs) = setup(&top);
        let o = Command::new("python3").arg(root().join("tools/without_openat2.py")).args(flag).arg(&exe).current_dir(&work).output().unwrap();
        let got = answers(&o);
        for line in ["plain 0", "setuid -28", "missing -44", "directory 0", "inside_link 0", "zero 0", "restore 0", "too_long -21"] {
            assert!(got.lines().any(|l| l == line), "{flag:?}: {line} in {got}");
        }
        assert_eq!(mode(&work.join("a.txt")), 0o644, "{flag:?}");
        assert_eq!(mode(&abs), 0o640, "{flag:?}");
    }
    let _ = std::fs::remove_dir_all(&top);
}
