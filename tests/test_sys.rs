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

// ===== Processes =====

const PROGRAMS: &str = "--allow-run /bin/sh --allow-run /bin/cat --allow-run /no/such/program";

const PROC_ANSWERS: &str = "stderr_not_a_pipe -1\nsent 6\nclosed_input 0\nhello\nread 6\nstatus 0\nstatus_again 0\nwrong_way -8\nclosed_output 0\nread_closed -8\nreleased 0\nwait_released -8\nno_stream -1\nexit_7 7\nkilled 265\nout\nerr\nmerged 8\nmerged_status 2\nnull_status 3\nA-B env=hello\nargs 14\nargs_status 0\nmissing -44\nnot_granted -76\nbad_stdio -28\nbad_stdio_high -28\nbad_handle_wait -8\nbad_handle_read -8\nbad_handle_write -8\nbad_handle_close -8\nbad_handle_stream -8\ntoo_long -21\n";

const EXEC_ANSWERS: &str = "before\nnot_granted -76\nmissing -44\ntoo_long -21\nreplaced A-B env=hello\n";

/// The compile flags for the grants in PROGRAMS (or the ones given).
fn grants(programs: &str) -> Vec<String> {
    programs.split_whitespace().map(str::to_string).collect()
}

/// Compiles `src` (a test program) with the given grants, as wasm, a native
/// executable, and the launcher bundle; the ways to run it, with its name.
fn processes(top: &Path, src: &Path, programs: &str) -> Vec<(&'static str, Command)> {
    let flags = grants(programs);
    // a plain module carries no grants (aipl-run is given them); an executable has them fixed
    let compile_with = |name: &str, extra: &[&str]| -> PathBuf {
        let args: Vec<String> = extra.iter().map(|s| s.to_string()).chain(if extra.is_empty() { vec![] } else { flags.clone() }).collect();
        let out = top.join(name);
        let o = Command::new(AIPL).arg("compile").args(&args).arg(src).arg("-o").arg(&out).output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        out
    };
    let wasm = compile_with("q.wasm", &[]);
    let exe = compile_with("q", &["--exe"]);
    let bundle = compile_with("q_bundle", &["--exe", "--target", "wasm"]);
    let mut vm = Command::new(AIPL);
    vm.arg("eval").args(&flags).arg(src);
    let mut run = Command::new(RUNNER);
    run.args(&flags).arg(&wasm);
    vec![("VM", vm), ("aipl-run", run), ("native", Command::new(exe)), ("bundle", Command::new(bundle))]
}

#[test]
fn processes_are_the_same_in_every_backend() {
    let top = scratch("procs");
    let src = root().join("tests/aipl/sys/spawn_ops.aipl");
    for (how, mut command) in processes(&top, &src, PROGRAMS) {
        let o = command.env("AIPL_PROC_TEST", "hello").current_dir(&top).output().unwrap();
        assert_eq!(answers(&o), PROC_ANSWERS, "{how}");
    }
    let _ = std::fs::remove_dir_all(&top);
}

#[test]
fn exec_replaces_the_program_in_every_backend() {
    let top = scratch("exec");
    let src = root().join("tests/aipl/sys/spawn_exec.aipl");
    for (how, mut command) in processes(&top, &src, "--allow-run /bin/sh --allow-run /no/such/program") {
        let o = command.env("AIPL_PROC_TEST", "hello").current_dir(&top).output().unwrap();
        // the status is the program's: sh's exit 3
        assert_eq!(o.status.code(), Some(3), "{how}: {}", String::from_utf8_lossy(&o.stderr));
        let got: String = String::from_utf8_lossy(&o.stdout).split_inclusive('\n').filter(|l| !l.starts_with("[AIPL")).collect();
        assert_eq!(got, EXEC_ANSWERS, "{how}");
    }
    let _ = std::fs::remove_dir_all(&top);
}

/// Without a grant nothing starts, and `*` grants any program; a native
/// executable's grants are fixed in it, a bundle's in its trailer.
#[test]
fn a_program_starts_only_what_it_was_granted() {
    let top = scratch("grants");
    let src = top.join("g.aipl");
    std::fs::write(&src, r#"(module g
  (fn go [path:str] -> i32
    (unsafe (proc.spawn (str.ptr path) (str.len path) (str.ptr "sh") 2 8)))
  (fn show [label:str n:i32] -> void
    (if (gte n 0) (println "{label} ok") (println "{label} {n}")))
  (fn main [] -> i32
    (call show "sh" (call go "/bin/sh"))
    (call show "cat" (call go "/bin/cat"))
    0))
"#).unwrap();
    for (programs, want) in [
        ("", "sh -76\ncat -76\n"),
        ("--allow-run /bin/sh", "sh ok\ncat -76\n"),
        ("--allow-run /bin/cat --allow-run /bin/sh", "sh ok\ncat ok\n"),
        ("--allow-run *", "sh ok\ncat ok\n"),
    ] {
        for (how, mut command) in processes(&top, &src, programs) {
            let o = command.current_dir(&top).output().unwrap();
            assert_eq!(answers(&o), want, "{how}, grants {programs:?}");
        }
    }
    let _ = std::fs::remove_dir_all(&top);
}

#[test]
fn both_toolchains_compile_the_process_operations_to_the_same_bytes() {
    let top = scratch("procbytes");
    for name in ["spawn_ops.aipl", "spawn_exec.aipl", "std_proc.aipl", "std_proc_exec.aipl"] {
        let src = root().join("tests/aipl/sys").join(name);
        let o = Command::new(AIPL).args(["compile", "--self", "--no-opt"]).arg(&src).arg("-o").arg(top.join("s.wasm")).output().unwrap();
        assert!(o.status.success(), "{name}: {}", String::from_utf8_lossy(&o.stderr));
        assert!(String::from_utf8_lossy(&o.stdout).contains("BIT-FOR-BIT IDENTICAL"), "{name}: {}", String::from_utf8_lossy(&o.stdout));
    }
    let _ = std::fs::remove_dir_all(&top);
}

// ===== std/proc =====

const STD_PROC_ANSWERS: &str = "hello status 3 exited true signal 0\nhello\ninput status 0 exited true signal 0\nping\nkilled status 265 exited false signal 9\nmerged status 0 exited true signal 0\nout\nerr\nargs status 0 exited true signal 0\na b  c\nzero\nnot_granted error: not granted: run the program with --allow-run PROGRAM\nnot_found error: no such program\nwrote true read 2 wrote true read 2\nend 0 status 0 again 0\na\nb\nafter close false\noutput 2 error 2\no\ne\nstatus 0\nfind sh true ends with /sh true\nfind nothing false with a slash true missing false\n";

const STD_PROC_EXEC_ANSWERS: &str = "before\nnot granted: not granted: run the program with --allow-run PROGRAM\nmissing: no such program\nreplaced zero\n";

/// std/proc: running a program, a conversation through pipes, the error on a
/// pipe of its own, looking a name up in the PATH, and every refusal, alike
/// from the VM, aipl-run, a native executable, and the launcher bundle.
#[test]
fn std_proc_is_the_same_in_every_backend() {
    let top = scratch("stdproc");
    let src = root().join("tests/aipl/sys/std_proc.aipl");
    for (how, mut command) in processes(&top, &src, PROGRAMS) {
        let o = command.current_dir(&top).output().unwrap();
        assert_eq!(answers(&o), STD_PROC_ANSWERS, "{how}");
    }
    let _ = std::fs::remove_dir_all(&top);
}

#[test]
fn std_proc_exec_replaces_the_program_in_every_backend() {
    let top = scratch("stdexec");
    let src = root().join("tests/aipl/sys/std_proc_exec.aipl");
    for (how, mut command) in processes(&top, &src, "--allow-run /bin/sh --allow-run /no/such/program") {
        let o = command.current_dir(&top).output().unwrap();
        assert_eq!(o.status.code(), Some(4), "{how}: {}", String::from_utf8_lossy(&o.stderr));
        let got: String = String::from_utf8_lossy(&o.stdout).split_inclusive('\n').filter(|l| !l.starts_with("[AIPL")).collect();
        assert_eq!(got, STD_PROC_EXEC_ANSWERS, "{how}");
    }
    let _ = std::fs::remove_dir_all(&top);
}

const STD_FS_CHMOD_COMPILED: &str = "program ok 0\nfile ok 0\nbytes ok 0\nsetuid the file could not be read or written\nmissing no such file or directory\ndotdot permission denied\n";

/// std/fs's `chmod` and `chmod_path` give an `io.Error` for each refusal
/// and leave the modes the program set, in every backend; the VM's paths are
/// ordinary, so its `..` is followed (SYS_PLAN.md step 4).
#[test]
fn std_fs_chmod_is_the_same_in_every_backend() {
    let top = scratch("stdfs");
    let src = root().join("tests/aipl/sys/std_fs_chmod.aipl");
    let wasm = compile(&top, &src, "p.wasm", &[]);
    let exe = compile(&top, &src, "p", &["--exe"]);
    let bundle = compile(&top, &src, "p_bundle", &["--exe", "--target", "wasm"]);
    let sandboxed = compile(&top, &src, "ps", &["--exe", "--sandbox"]);
    let runs: Vec<Run> = vec![
        ("VM", Box::new(|| { let mut c = Command::new(AIPL); c.arg("eval").arg(&src); c }), 0),
        ("aipl-run", Box::new(|| { let mut c = Command::new(RUNNER); c.arg(&wasm); c }), 1),
        ("native", Box::new(|| Command::new(&exe)), 1),
        ("bundle", Box::new(|| Command::new(&bundle)), 1),
        ("native --sandbox", Box::new(|| Command::new(&sandboxed)), 1),
    ];
    for (how, command, kind) in runs {
        let ordinary = kind == 0;
        let (work, outside, _) = setup(&top);
        let o = command().current_dir(&work).output().unwrap();
        let want = if ordinary { STD_FS_CHMOD_COMPILED.replace("dotdot permission denied", "dotdot ok 0") } else { STD_FS_CHMOD_COMPILED.to_string() };
        assert_eq!(answers(&o), want, "{how}");
        assert_eq!(mode(&work.join("a.txt")), 0o600, "{how}: a.txt");
        assert_eq!(mode(&outside), if ordinary { 0o600 } else { 0o644 }, "{how}: outside.txt");
    }
    let _ = std::fs::remove_dir_all(&top);
}

const STD_FS_RENAME_COMPILED: &str = "plain ok 0\nreplace ok 0\nbytes ok 0\nmissing no such file or directory\ndotdot permission denied\n";

/// std/fs's `rename` and `rename_path` move and replace files and give an
/// `io.Error` for each refusal, in every backend; the VM's paths are ordinary,
/// so its `..` is followed (SYS_PLAN.md section 10).
#[test]
fn std_fs_rename_is_the_same_in_every_backend() {
    let top = scratch("stdfs_rename");
    let src = root().join("tests/aipl/sys/std_fs_rename.aipl");
    let wasm = compile(&top, &src, "p.wasm", &[]);
    let exe = compile(&top, &src, "p", &["--exe"]);
    let bundle = compile(&top, &src, "p_bundle", &["--exe", "--target", "wasm"]);
    let sandboxed = compile(&top, &src, "ps", &["--exe", "--sandbox"]);
    let runs: Vec<Run> = vec![
        ("VM", Box::new(|| { let mut c = Command::new(AIPL); c.arg("eval").arg(&src); c }), 0),
        ("aipl-run", Box::new(|| { let mut c = Command::new(RUNNER); c.arg(&wasm); c }), 1),
        ("native", Box::new(|| Command::new(&exe)), 1),
        ("bundle", Box::new(|| Command::new(&bundle)), 1),
        ("native --sandbox", Box::new(|| Command::new(&sandboxed)), 1),
    ];
    for (how, command, kind) in runs {
        let ordinary = kind == 0;
        let (work, outside, _) = setup(&top);
        std::fs::write(work.join("a.txt"), "A").unwrap();
        std::fs::write(work.join("b.txt"), "B").unwrap();
        let o = command().current_dir(&work).output().unwrap();
        let want = if ordinary { STD_FS_RENAME_COMPILED.replace("dotdot permission denied", "dotdot ok 0") } else { STD_FS_RENAME_COMPILED.to_string() };
        assert_eq!(answers(&o), want, "{how}");
        let at = |name: &str| std::fs::read_to_string(work.join(name)).ok();
        assert_eq!((at("a.txt"), at("b.txt"), at("c.txt")), (None, None, None), "{how}: the moved names are gone");
        if ordinary {
            assert_eq!(at("d.txt"), None, "{how}: d.txt");
            assert_eq!(std::fs::read_to_string(&outside).unwrap(), "A", "{how}: outside.txt");
        } else {
            assert_eq!(at("d.txt").as_deref(), Some("A"), "{how}: d.txt");
            assert_eq!(std::fs::read_to_string(&outside).unwrap(), "x", "{how}: outside.txt");
        }
    }
    let _ = std::fs::remove_dir_all(&top);
}

const STD_FS_REMOVE_COMPILED: &str = "plain ok 0\nagain no such file or directory\nbytes ok 0\ndirectory is a directory\nlink ok 0\nmissing no such file or directory\ndotdot permission denied\n";

/// std/fs's `remove` and `remove_path` delete a file or a link (itself, not
/// its target) and give an `io.Error` for each refusal, in every backend; the
/// VM's paths are ordinary, so its `..` is followed (SYS_PLAN.md section 10).
#[test]
fn std_fs_remove_is_the_same_in_every_backend() {
    let top = scratch("stdfs_remove");
    let src = root().join("tests/aipl/sys/std_fs_remove.aipl");
    let wasm = compile(&top, &src, "p.wasm", &[]);
    let exe = compile(&top, &src, "p", &["--exe"]);
    let bundle = compile(&top, &src, "p_bundle", &["--exe", "--target", "wasm"]);
    let sandboxed = compile(&top, &src, "ps", &["--exe", "--sandbox"]);
    let runs: Vec<Run> = vec![
        ("VM", Box::new(|| { let mut c = Command::new(AIPL); c.arg("eval").arg(&src); c }), 0),
        ("aipl-run", Box::new(|| { let mut c = Command::new(RUNNER); c.arg(&wasm); c }), 1),
        ("native", Box::new(|| Command::new(&exe)), 1),
        ("bundle", Box::new(|| Command::new(&bundle)), 1),
        ("native --sandbox", Box::new(|| Command::new(&sandboxed)), 1),
    ];
    for (how, command, kind) in runs {
        let ordinary = kind == 0;
        let (work, outside, _) = setup(&top);
        std::fs::write(work.join("b.txt"), "B").unwrap();
        let o = command().current_dir(&work).output().unwrap();
        let want = if ordinary { STD_FS_REMOVE_COMPILED.replace("dotdot permission denied", "dotdot ok 0") } else { STD_FS_REMOVE_COMPILED.to_string() };
        assert_eq!(answers(&o), want, "{how}");
        for gone in ["a.txt", "b.txt", "link"] {
            assert!(work.join(gone).symlink_metadata().is_err(), "{how}: {gone} is still there");
        }
        assert!(work.join("dir").is_dir(), "{how}: dir");
        assert_eq!(outside.exists(), !ordinary, "{how}: outside.txt (the link's target) or the file a `..` names");
    }
    let _ = std::fs::remove_dir_all(&top);
}

const STD_FS_DIRS_COMPILED: &str = "mkdir ok 0\nagain the file could not be read or written\nbytes ok 0\nmissing_parent no such file or directory\ndotdot permission denied\nrmdir ok 0\nrmdir_bytes ok 0\nnot_empty the file could not be read or written\nfile no such file or directory\nlink no such file or directory\nmissing no such file or directory\n";

/// std/fs's `mkdir`, `mkdir_path`, `rmdir`, and `rmdir_path` make and remove
/// directories and give an `io.Error` for each refusal, in every backend; the
/// VM's paths are ordinary, so its `..` is followed (SYS_PLAN.md section 11).
#[test]
fn std_fs_directories_are_the_same_in_every_backend() {
    let top = scratch("stdfs_dirs");
    let src = root().join("tests/aipl/sys/std_fs_dirs.aipl");
    let wasm = compile(&top, &src, "p.wasm", &[]);
    let exe = compile(&top, &src, "p", &["--exe"]);
    let bundle = compile(&top, &src, "p_bundle", &["--exe", "--target", "wasm"]);
    let sandboxed = compile(&top, &src, "ps", &["--exe", "--sandbox"]);
    let runs: Vec<Run> = vec![
        ("VM", Box::new(|| { let mut c = Command::new(AIPL); c.arg("eval").arg(&src); c }), 0),
        ("aipl-run", Box::new(|| { let mut c = Command::new(RUNNER); c.arg(&wasm); c }), 1),
        ("native", Box::new(|| Command::new(&exe)), 1),
        ("bundle", Box::new(|| Command::new(&bundle)), 1),
        ("native --sandbox", Box::new(|| Command::new(&sandboxed)), 1),
    ];
    for (how, command, kind) in runs {
        let ordinary = kind == 0;
        let _ = std::fs::remove_dir_all(top.join("work"));
        let _ = std::fs::remove_dir_all(top.join("outside_made"));
        let work = top.join("work");
        std::fs::create_dir_all(work.join("full")).unwrap();
        std::fs::write(work.join("file.txt"), "x").unwrap();
        std::fs::write(work.join("full/f.txt"), "F").unwrap();
        symlink("full", work.join("link")).unwrap();
        let o = command().current_dir(&work).output().unwrap();
        let want = if ordinary { STD_FS_DIRS_COMPILED.replace("dotdot permission denied", "dotdot ok 0") } else { STD_FS_DIRS_COMPILED.to_string() };
        assert_eq!(answers(&o), want, "{how}");
        assert_eq!(tree_text(&work), "file.txt=x\nfull/\nfull/f.txt=F\nlink->full\n", "{how}: the files left");
        assert_eq!(top.join("outside_made").is_dir(), ordinary, "{how}: the directory a `..` names");
    }
    let _ = std::fs::remove_dir_all(&top);
}

/// What `rename.aipl` prints, for the VM (paths ordinary), for compiled
/// programs (a relative path stays in the working directory), and under
/// `--sandbox` (there is no absolute path to name).
const RENAME_ORDINARY: &str = "plain 0\nreplace 0\nmissing -44\ninto_dir 0\nmissing_dir -44\nsame 0\ndirectory 0\ndir_onto_full -55\nfile_onto_dir -31\ndir_onto_file -54\nlink 0\ndotdot_from 0\ndotdot_to 0\nabsolute 0\nabs_to_rel 0\nrel_to_abs 0\nempty -44\n";
const RENAME_CONFINED: &str = "plain 0\nreplace 0\nmissing -44\ninto_dir 0\nmissing_dir -44\nsame 0\ndirectory 0\ndir_onto_full -55\nfile_onto_dir -31\ndir_onto_file -54\nlink 0\ndotdot_from -63\ndotdot_to -63\nabsolute 0\nabs_to_rel 0\nrel_to_abs 0\nempty -44\n";
const RENAME_SANDBOXED: &str = "plain 0\nreplace 0\nmissing -44\ninto_dir 0\nmissing_dir -44\nsame 0\ndirectory 0\ndir_onto_full -55\nfile_onto_dir -31\ndir_onto_file -54\nlink 0\ndotdot_from -63\ndotdot_to -63\nabsolute -8\nabs_to_rel -8\nrel_to_abs -8\nempty -44\n";

/// The tree each leaves (directories end in `/`, a link shows its target).
const TREE_ORDINARY: &str = "abs/\nabs/q2.txt=Q\nabs/r.txt=P\nmoved.txt=F\nwork/\nwork/dir2/\nwork/full/\nwork/link2->a.txt\nwork/o2.txt=O\nwork/sub/\nwork/sub/c.txt=B\n";
const TREE_CONFINED: &str = "abs/\nabs/q2.txt=Q\nabs/r.txt=P\noutside.txt=O\nwork/\nwork/dir2/\nwork/full/\nwork/full/f.txt=F\nwork/link2->a.txt\nwork/sub/\nwork/sub/c.txt=B\n";
const TREE_SANDBOXED: &str = "abs/\nabs/p.txt=P\nabs/q.txt=Q\noutside.txt=O\nwork/\nwork/dir2/\nwork/full/\nwork/full/f.txt=F\nwork/link2->a.txt\nwork/sub/\nwork/sub/c.txt=B\n";

/// `top/work` to run in, beside `top/outside.txt` and `top/abs/{p,q}.txt`.
fn rename_setup(top: &Path) -> PathBuf {
    let work = top.join("work");
    let _ = std::fs::remove_dir_all(&work);
    let _ = std::fs::remove_dir_all(top.join("abs"));
    for d in ["dir", "full", "sub"] {
        std::fs::create_dir_all(work.join(d)).unwrap();
    }
    std::fs::create_dir_all(top.join("abs")).unwrap();
    for (f, text) in [("work/a.txt", "A"), ("work/b.txt", "B"), ("work/full/f.txt", "F"), ("outside.txt", "O"), ("abs/p.txt", "P"), ("abs/q.txt", "Q")] {
        let _ = std::fs::remove_file(top.join(f));
        std::fs::write(top.join(f), text).unwrap();
    }
    let _ = std::fs::remove_file(top.join("moved.txt"));
    let _ = std::fs::remove_file(work.join("link"));
    symlink("a.txt", work.join("link")).unwrap();
    work
}

fn tree(top: &Path, at: &Path, out: &mut Vec<String>) {
    let mut names: Vec<_> = std::fs::read_dir(at).unwrap().map(|e| e.unwrap().path()).collect();
    names.sort();
    for p in names {
        let rel = p.strip_prefix(top).unwrap().to_str().unwrap().to_string();
        let meta = std::fs::symlink_metadata(&p).unwrap();
        if meta.file_type().is_symlink() {
            out.push(format!("{rel}->{}", std::fs::read_link(&p).unwrap().display()));
        } else if meta.is_dir() {
            out.push(format!("{rel}/"));
            tree(top, &p, out);
        } else {
            out.push(format!("{rel}={}", std::fs::read_to_string(&p).unwrap()));
        }
    }
}

/// `fs.rename` gives the same results, and leaves the same files, from the
/// VM, aipl-run, a native executable, and the launcher bundle (SYS_PLAN.md):
/// a file replaced, a directory moved, a link moved itself, the errors of a
/// directory onto a file and a file onto a directory, and what the working
/// directory and `--sandbox` confine.
#[test]
fn fs_rename_is_the_same_in_every_backend() {
    let (top, bins) = (scratch("rename"), scratch("rename_bins"));
    let abs = top.join("abs");
    let src = bins.join("rename.aipl");
    std::fs::write(&src, std::fs::read_to_string(root().join("tests/aipl/sys/rename.aipl")).unwrap().replace("@ABS@", abs.to_str().unwrap())).unwrap();
    let wasm = compile(&bins, &src, "p.wasm", &[]);
    let exe = compile(&bins, &src, "p", &["--exe"]);
    let bundle = compile(&bins, &src, "p_bundle", &["--exe", "--target", "wasm"]);
    let sandboxed_exe = compile(&bins, &src, "ps", &["--exe", "--sandbox"]);
    let sandboxed_bundle = compile(&bins, &src, "ps_bundle", &["--exe", "--target", "wasm", "--sandbox"]);
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
        let work = rename_setup(&top);
        let o = command().current_dir(&work).output().unwrap();
        assert_eq!(answers(&o), [RENAME_ORDINARY, RENAME_CONFINED, RENAME_SANDBOXED][kind as usize], "{how}");
        let mut got = Vec::new();
        tree(&top, &top, &mut got);
        let got: String = got.iter().map(|l| format!("{l}\n")).collect();
        assert_eq!(got, [TREE_ORDINARY, TREE_CONFINED, TREE_SANDBOXED][kind as usize], "{how}: the files left");
    }
    let _ = std::fs::remove_dir_all(&top);
    let _ = std::fs::remove_dir_all(&bins);
}

/// Where `openat2` is refused (tools/without_openat2.py), `fs.rename` still
/// works, opening both parent directories with plain `openat` and giving up
/// the confinement as every open does: the same answers as the VM's.
#[test]
fn fs_rename_works_where_openat2_is_refused() {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return;
    }
    let (top, bins) = (scratch("rename_fallback"), scratch("rename_fallback_bins"));
    let abs = top.join("abs");
    let src = bins.join("rename.aipl");
    std::fs::write(&src, std::fs::read_to_string(root().join("tests/aipl/sys/rename.aipl")).unwrap().replace("@ABS@", abs.to_str().unwrap())).unwrap();
    let exe = compile(&bins, &src, "p", &["--exe"]);
    for flag in [&[][..], &["--eperm"][..]] {
        let work = rename_setup(&top);
        let o = Command::new("python3").arg(root().join("tools/without_openat2.py")).args(flag).arg(&exe).current_dir(&work).output().unwrap();
        assert_eq!(answers(&o), RENAME_ORDINARY, "{flag:?}");
        let mut got = Vec::new();
        tree(&top, &top, &mut got);
        let got: String = got.iter().map(|l| format!("{l}\n")).collect();
        assert_eq!(got, TREE_ORDINARY, "{flag:?}: the files left");
    }
    let _ = std::fs::remove_dir_all(&top);
    let _ = std::fs::remove_dir_all(&bins);
}

/// Both toolchains compile the program to the same bytes.
#[test]
fn both_toolchains_compile_fs_rename_to_the_same_bytes() {
    let top = scratch("rename_bytes");
    let src = top.join("rename.aipl");
    std::fs::write(&src, std::fs::read_to_string(root().join("tests/aipl/sys/rename.aipl")).unwrap().replace("@ABS@", "/tmp/abs")).unwrap();
    let o = Command::new(AIPL).arg("compile").arg("--self").arg(&src).arg("-o").arg(top.join("s.wasm")).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).contains("BIT-FOR-BIT IDENTICAL"), "{}", String::from_utf8_lossy(&o.stdout));
    let _ = std::fs::remove_dir_all(&top);
}

/// What `rmdir.aipl` prints, for the VM (paths ordinary), for compiled
/// programs (a relative path stays in the working directory), and under
/// `--sandbox` (there is no absolute path to name).
const RMDIR_ORDINARY: &str = "plain 0\nagain -44\nnot_empty -55\nfile -54\ninner 0\nouter 0\nlink -54\ndotdot 0\nabsolute 0\nempty_path -44\n";
const RMDIR_CONFINED: &str = "plain 0\nagain -44\nnot_empty -55\nfile -54\ninner 0\nouter 0\nlink -54\ndotdot -63\nabsolute 0\nempty_path -44\n";
const RMDIR_SANDBOXED: &str = "plain 0\nagain -44\nnot_empty -55\nfile -54\ninner 0\nouter 0\nlink -54\ndotdot -63\nabsolute -8\nempty_path -44\n";

/// The tree each leaves: only the directories that were removed differ.
const RMDIR_TREE_ORDINARY: &str = "abs/\nwork/\nwork/file.txt=x\nwork/full/\nwork/full/f.txt=F\nwork/link->target\nwork/target/\n";
const RMDIR_TREE_CONFINED: &str = "abs/\noutside_dir/\nwork/\nwork/file.txt=x\nwork/full/\nwork/full/f.txt=F\nwork/link->target\nwork/target/\n";
const RMDIR_TREE_SANDBOXED: &str = "abs/\nabs/gone/\noutside_dir/\nwork/\nwork/file.txt=x\nwork/full/\nwork/full/f.txt=F\nwork/link->target\nwork/target/\n";

/// `top/work` to run in, beside `top/outside_dir` and `top/abs/gone`.
fn rmdir_setup(top: &Path) -> PathBuf {
    for d in ["work", "outside_dir", "abs"] {
        let _ = std::fs::remove_dir_all(top.join(d));
    }
    for d in ["work/empty", "work/full", "work/sub/inner", "work/target", "outside_dir", "abs/gone"] {
        std::fs::create_dir_all(top.join(d)).unwrap();
    }
    std::fs::write(top.join("work/full/f.txt"), "F").unwrap();
    std::fs::write(top.join("work/file.txt"), "x").unwrap();
    symlink("target", top.join("work/link")).unwrap();
    top.join("work")
}

fn tree_text(top: &Path) -> String {
    let mut got = Vec::new();
    tree(top, top, &mut got);
    got.iter().map(|l| format!("{l}\n")).collect()
}

/// `fs.rmdir` gives the same results, and leaves the same directories, from
/// the VM, aipl-run, a native executable, and the launcher bundle
/// (SYS_PLAN.md section 11): an empty directory removed, a full one `-55`, a
/// file and a link `-54`, and what the working directory and `--sandbox`
/// confine.
#[test]
fn fs_rmdir_is_the_same_in_every_backend() {
    let (top, bins) = (scratch("rmdir"), scratch("rmdir_bins"));
    let src = bins.join("rmdir.aipl");
    std::fs::write(&src, std::fs::read_to_string(root().join("tests/aipl/sys/rmdir.aipl")).unwrap().replace("@ABS@", top.join("abs").to_str().unwrap())).unwrap();
    let wasm = compile(&bins, &src, "p.wasm", &[]);
    let exe = compile(&bins, &src, "p", &["--exe"]);
    let bundle = compile(&bins, &src, "p_bundle", &["--exe", "--target", "wasm"]);
    let sandboxed_exe = compile(&bins, &src, "ps", &["--exe", "--sandbox"]);
    let sandboxed_bundle = compile(&bins, &src, "ps_bundle", &["--exe", "--target", "wasm", "--sandbox"]);
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
        let work = rmdir_setup(&top);
        let o = command().current_dir(&work).output().unwrap();
        assert_eq!(answers(&o), [RMDIR_ORDINARY, RMDIR_CONFINED, RMDIR_SANDBOXED][kind as usize], "{how}");
        assert_eq!(tree_text(&top), [RMDIR_TREE_ORDINARY, RMDIR_TREE_CONFINED, RMDIR_TREE_SANDBOXED][kind as usize], "{how}: the files left");
    }
    let _ = std::fs::remove_dir_all(&top);
    let _ = std::fs::remove_dir_all(&bins);
}

/// Where `openat2` is refused (tools/without_openat2.py), `fs.rmdir` still
/// works, opening the parent with plain `openat` and giving up the
/// confinement as every open does: the same answers as the VM's.
#[test]
fn fs_rmdir_works_where_openat2_is_refused() {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return;
    }
    let (top, bins) = (scratch("rmdir_fallback"), scratch("rmdir_fallback_bins"));
    let src = bins.join("rmdir.aipl");
    std::fs::write(&src, std::fs::read_to_string(root().join("tests/aipl/sys/rmdir.aipl")).unwrap().replace("@ABS@", top.join("abs").to_str().unwrap())).unwrap();
    let exe = compile(&bins, &src, "p", &["--exe"]);
    for flag in [&[][..], &["--eperm"][..]] {
        let work = rmdir_setup(&top);
        let o = Command::new("python3").arg(root().join("tools/without_openat2.py")).args(flag).arg(&exe).current_dir(&work).output().unwrap();
        assert_eq!(answers(&o), RMDIR_ORDINARY, "{flag:?}");
        assert_eq!(tree_text(&top), RMDIR_TREE_ORDINARY, "{flag:?}: the files left");
    }
    let _ = std::fs::remove_dir_all(&top);
    let _ = std::fs::remove_dir_all(&bins);
}

/// Both toolchains compile the program to the same bytes.
#[test]
fn both_toolchains_compile_fs_rmdir_to_the_same_bytes() {
    let top = scratch("rmdir_bytes");
    let src = top.join("rmdir.aipl");
    std::fs::write(&src, std::fs::read_to_string(root().join("tests/aipl/sys/rmdir.aipl")).unwrap().replace("@ABS@", "/tmp/abs")).unwrap();
    let o = Command::new(AIPL).arg("compile").arg("--self").arg(&src).arg("-o").arg(top.join("s.wasm")).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).contains("BIT-FOR-BIT IDENTICAL"), "{}", String::from_utf8_lossy(&o.stdout));
    let _ = std::fs::remove_dir_all(&top);
}

/// What `mkdir.aipl` prints, for the VM (paths ordinary), for compiled
/// programs (a relative path stays in the working directory), and under
/// `--sandbox` (there is no absolute path to name).
const MKDIR_ORDINARY: &str = "plain 0\nagain -20\nnested 0\nfile_exists -20\nlink_exists -20\nmissing_parent -44\nunder_file -54\ndotdot 0\nabsolute 0\nempty_path -44\n";
const MKDIR_CONFINED: &str = "plain 0\nagain -20\nnested 0\nfile_exists -20\nlink_exists -20\nmissing_parent -44\nunder_file -54\ndotdot -63\nabsolute 0\nempty_path -44\n";
const MKDIR_SANDBOXED: &str = "plain 0\nagain -20\nnested 0\nfile_exists -20\nlink_exists -20\nmissing_parent -44\nunder_file -54\ndotdot -63\nabsolute -8\nempty_path -44\n";

/// The tree each leaves: only the directories made outside the working
/// directory differ.
const MKDIR_TREE_ORDINARY: &str = "abs/\nabs/made/\noutside_made/\nwork/\nwork/file.txt=x\nwork/link->sub\nwork/made/\nwork/made/inner/\nwork/sub/\n";
const MKDIR_TREE_CONFINED: &str = "abs/\nabs/made/\nwork/\nwork/file.txt=x\nwork/link->sub\nwork/made/\nwork/made/inner/\nwork/sub/\n";
const MKDIR_TREE_SANDBOXED: &str = "abs/\nwork/\nwork/file.txt=x\nwork/link->sub\nwork/made/\nwork/made/inner/\nwork/sub/\n";

/// `top/work` to run in, beside `top/abs`.
fn mkdir_setup(top: &Path) -> PathBuf {
    for d in ["work", "outside_made", "abs"] {
        let _ = std::fs::remove_dir_all(top.join(d));
    }
    for d in ["work/sub", "abs"] {
        std::fs::create_dir_all(top.join(d)).unwrap();
    }
    std::fs::write(top.join("work/file.txt"), "x").unwrap();
    symlink("sub", top.join("work/link")).unwrap();
    top.join("work")
}

/// The mode of a directory this process makes: what `mkdir` gives under the
/// umask, which the program's directory must have too.
fn made_mode(top: &Path) -> u32 {
    let d = top.join("mode_probe");
    let _ = std::fs::remove_dir(&d);
    std::fs::create_dir(&d).unwrap();
    let m = mode(&d);
    std::fs::remove_dir(&d).unwrap();
    m
}

/// `fs.mkdir` gives the same results, and leaves the same directories, from
/// the VM, aipl-run, a native executable, and the launcher bundle
/// (SYS_PLAN.md section 11): a directory made, one that exists (a file or a
/// link too) `-20`, a missing parent `-44`, a file as parent `-54`, and what
/// the working directory and `--sandbox` confine.
#[test]
fn fs_mkdir_is_the_same_in_every_backend() {
    let (top, bins) = (scratch("mkdir"), scratch("mkdir_bins"));
    let src = bins.join("mkdir.aipl");
    std::fs::write(&src, std::fs::read_to_string(root().join("tests/aipl/sys/mkdir.aipl")).unwrap().replace("@ABS@", top.join("abs").to_str().unwrap())).unwrap();
    let wasm = compile(&bins, &src, "p.wasm", &[]);
    let exe = compile(&bins, &src, "p", &["--exe"]);
    let bundle = compile(&bins, &src, "p_bundle", &["--exe", "--target", "wasm"]);
    let sandboxed_exe = compile(&bins, &src, "ps", &["--exe", "--sandbox"]);
    let sandboxed_bundle = compile(&bins, &src, "ps_bundle", &["--exe", "--target", "wasm", "--sandbox"]);
    let runs: Vec<Run> = vec![
        ("VM", Box::new(|| { let mut c = Command::new(AIPL); c.arg("eval").arg(&src); c }), 0),
        ("aipl-run", Box::new(|| { let mut c = Command::new(RUNNER); c.arg(&wasm); c }), 1),
        ("native", Box::new(|| Command::new(&exe)), 1),
        ("bundle", Box::new(|| Command::new(&bundle)), 1),
        ("aipl-run --sandbox", Box::new(|| { let mut c = Command::new(RUNNER); c.arg("--sandbox").arg(&wasm); c }), 2),
        ("native --sandbox", Box::new(|| Command::new(&sandboxed_exe)), 2),
        ("bundle --sandbox", Box::new(|| Command::new(&sandboxed_bundle)), 2),
    ];
    let want_mode = made_mode(&top);
    for (how, command, kind) in runs {
        let work = mkdir_setup(&top);
        let o = command().current_dir(&work).output().unwrap();
        assert_eq!(answers(&o), [MKDIR_ORDINARY, MKDIR_CONFINED, MKDIR_SANDBOXED][kind as usize], "{how}");
        assert_eq!(tree_text(&top), [MKDIR_TREE_ORDINARY, MKDIR_TREE_CONFINED, MKDIR_TREE_SANDBOXED][kind as usize], "{how}: the files left");
        assert_eq!(mode(&work.join("made")), want_mode, "{how}: the mode of the directory made");
    }
    let _ = std::fs::remove_dir_all(&top);
    let _ = std::fs::remove_dir_all(&bins);
}

/// Where `openat2` is refused (tools/without_openat2.py), `fs.mkdir` still
/// works, opening the parent with plain `openat` and giving up the
/// confinement as every open does: the same answers as the VM's.
#[test]
fn fs_mkdir_works_where_openat2_is_refused() {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return;
    }
    let (top, bins) = (scratch("mkdir_fallback"), scratch("mkdir_fallback_bins"));
    let src = bins.join("mkdir.aipl");
    std::fs::write(&src, std::fs::read_to_string(root().join("tests/aipl/sys/mkdir.aipl")).unwrap().replace("@ABS@", top.join("abs").to_str().unwrap())).unwrap();
    let exe = compile(&bins, &src, "p", &["--exe"]);
    for flag in [&[][..], &["--eperm"][..]] {
        let work = mkdir_setup(&top);
        let o = Command::new("python3").arg(root().join("tools/without_openat2.py")).args(flag).arg(&exe).current_dir(&work).output().unwrap();
        assert_eq!(answers(&o), MKDIR_ORDINARY, "{flag:?}");
        assert_eq!(tree_text(&top), MKDIR_TREE_ORDINARY, "{flag:?}: the files left");
    }
    let _ = std::fs::remove_dir_all(&top);
    let _ = std::fs::remove_dir_all(&bins);
}

/// Both toolchains compile the program to the same bytes.
#[test]
fn both_toolchains_compile_fs_mkdir_to_the_same_bytes() {
    let top = scratch("mkdir_bytes");
    let src = top.join("mkdir.aipl");
    std::fs::write(&src, std::fs::read_to_string(root().join("tests/aipl/sys/mkdir.aipl")).unwrap().replace("@ABS@", "/tmp/abs")).unwrap();
    let o = Command::new(AIPL).arg("compile").arg("--self").arg(&src).arg("-o").arg(top.join("s.wasm")).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).contains("BIT-FOR-BIT IDENTICAL"), "{}", String::from_utf8_lossy(&o.stdout));
    let _ = std::fs::remove_dir_all(&top);
}
