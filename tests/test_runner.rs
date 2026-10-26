//! The aipl-run launcher and standalone executables (AIPL_SPEC.md 6.5),
//! exercised through the real `aipl` and `aipl-run` binaries.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_runner_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

// --no-opt: these compare the launcher with the VM, frame for frame; an
// optimized build has no frame for an inlined function (OPT_PLAN.md)
fn compile(src: &Path, out: &Path, extra: &[&str]) {
    let o = Command::new(AIPL).args(["compile", "--no-opt"]).arg(src).arg("-o").arg(out).args(extra).env("AIPL_RUNNER", RUNNER).output().unwrap();
    assert!(o.status.success(), "compile failed: {}", String::from_utf8_lossy(&o.stderr));
}

fn run_in(dir: &Path, program: &Path, args: &[&str], stdin: &str) -> Output {
    let mut cmd = if program.extension().is_some_and(|e| e == "wasm") {
        let mut c = Command::new(RUNNER);
        c.arg(program);
        c
    } else {
        Command::new(program)
    };
    let mut child = cmd.args(args).current_dir(dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    use std::io::Write;
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn the_runner_gives_programs_args_files_stdin_and_exit_codes() {
    let dir = scratch("basics");
    std::fs::write(
        dir.join("prog.aipl"),
        r#"(module prog
  (import io) (import str) (import os)
  (fn main [] -> i32
    (call io.println_int "argc: " (call os.arg_count))
    (call io.println_int "stdin words: " (call str.count_words (call io.read_stdin)))
    (let f:str.Bytes (call io.read_path (call os.arg 1)))
    (call io.println_int "file bytes: " (get f str.Bytes.len))
    (when (gt (call os.arg_count) 2) (sys.exit 3))
    0))"#,
    )
    .unwrap();
    std::fs::write(dir.join("data.txt"), "12345").unwrap();
    compile(&dir.join("prog.aipl"), &dir.join("prog.wasm"), &[]);
    let o = run_in(&dir, &dir.join("prog.wasm"), &["data.txt"], "a b c");
    assert_eq!(stdout(&o), "argc: 2\nstdin words: 3\nfile bytes: 5\n");
    assert_eq!(o.status.code(), Some(0));
    // an absolute path works (the runner grants /), and sys.exit sets the status
    let abs = dir.join("data.txt");
    let o = run_in(&dir, &dir.join("prog.wasm"), &[abs.to_str().unwrap(), "x"], "");
    assert!(stdout(&o).contains("file bytes: 5"), "{}", stdout(&o));
    assert_eq!(o.status.code(), Some(3));
}

#[test]
fn a_trap_is_reported_with_a_failing_status() {
    let dir = scratch("trap");
    std::fs::write(dir.join("t.aipl"), "(module t (fn main [] -> i32 (/ 1 0)))").unwrap();
    compile(&dir.join("t.aipl"), &dir.join("t.wasm"), &[]);
    let o = run_in(&dir, &dir.join("t.wasm"), &[], "");
    assert_eq!(o.status.code(), Some(134));
    // the program as invoked and the reason, then the call chain
    let program = dir.join("t.wasm").display().to_string();
    assert_eq!(String::from_utf8_lossy(&o.stderr), format!("{program}: wasm trap: integer divide by zero\n  at main (t.aipl:1:30)\n"));
    // the full wasmtime report on request
    let full = Command::new(RUNNER).arg(dir.join("t.wasm")).env("AIPL_BACKTRACE", "1").output().unwrap();
    let full = String::from_utf8_lossy(&full.stderr);
    assert!(full.contains("backtrace") && full.contains("wasm trap: integer divide by zero"), "{full}");
}

/// The VM reports a runtime error with the call chain compiled code prints:
/// each frame with its file, line, and column (in a two-file program, and
/// for a failed contract), after its own wording of the message.
#[test]
fn the_vm_prints_the_same_call_chain() {
    let dir = scratch("vm_chain");
    std::fs::write(dir.join("lib.aipl"), "(module lib\n  (fn ratio [a:i32 b:i32] -> i32 (pub)\n    (/ a b))\n  (fn pos [n:i32] -> i32 (pub) (req (gt n 0)) n))").unwrap();
    for (main, vm_msg) in [
        ("(call lib.ratio x (- n n))", "Error: Division by zero"),
        ("(call lib.pos (- 0 x))", "Error: Pre-condition failed in 'lib.pos' at 4:37: (req (gt n 0)) with n = -6"),
    ] {
        let src = format!("(module main\n  (import lib)\n  (fn f [n:i32] -> i32\n    (let x:i32 (+ n 1))\n    {main})\n  (fn main [] -> i32\n    (call f 5)))");
        std::fs::write(dir.join("main.aipl"), &src).unwrap();
        let vm = Command::new(AIPL).arg("eval").arg("main.aipl").current_dir(&dir).output().unwrap();
        let vm_err = String::from_utf8_lossy(&vm.stderr).to_string();
        let (first, vm_chain) = vm_err.split_once('\n').unwrap_or_else(|| panic!("no chain: {vm_err}"));
        assert_eq!(first, vm_msg, "{vm_err}");
        compile(&dir.join("main.aipl"), &dir.join("main.wasm"), &[]);
        let o = run_in(&dir, &dir.join("main.wasm"), &[], "");
        let wasm_err = String::from_utf8_lossy(&o.stderr).to_string();
        let wasm_chain = wasm_err.split_once('\n').unwrap().1;
        assert_eq!(vm_chain, wasm_chain, "{main}");
        assert!(vm_chain.starts_with("  at lib.") && vm_chain.ends_with("  at main (main.aipl:7:5)\n"), "{vm_chain}");
    }
}

/// A null pointer or array stops the program with the same message in the
/// VM and compiled, before anything else in the expression runs (here, an
/// index that would print).
#[test]
fn null_pointers_stop_the_vm_and_compiled_code_alike() {
    let dir = scratch("null");
    for body in [
        "(get (ptr.null P) P.x)",
        "(block (put (ptr.null P) P.x 1) 0)",
        "(arr.len (arr.null i32))",
        "(arr.get i32 (arr.null i32) (block (call io.println_int \"never \" 1) 0))",
        "(block (arr.set i32 (arr.null i32) (block (call io.println_int \"never \" 1) 0) 5) 0)",
    ] {
        let src = format!("(module t (import io) (struct P [x:i32])\n  (fn main [] -> i32\n    {body}))");
        std::fs::write(dir.join("t.aipl"), &src).unwrap();
        let vm = Command::new(AIPL).arg("eval").arg("t.aipl").current_dir(&dir).output().unwrap();
        let vm_err = String::from_utf8_lossy(&vm.stderr).to_string();
        // at the form that dereferences (the body starts in column 5)
        let at = ["(get", "(put", "(arr."].iter().filter_map(|f| body.find(f)).min().unwrap();
        let chain = format!("  at main (t.aipl:3:{})\n", 5 + at);
        assert_eq!(vm_err, format!("Error: Null pointer dereference\n{chain}"), "{body}");
        assert!(!String::from_utf8_lossy(&vm.stdout).contains("never"), "{body}");
        compile(&dir.join("t.aipl"), &dir.join("t.wasm"), &[]);
        let o = run_in(&dir, &dir.join("t.wasm"), &[], "");
        let program = dir.join("t.wasm").display().to_string();
        assert_eq!(String::from_utf8_lossy(&o.stderr), format!("{program}: Null pointer dereference\n{chain}"), "{body}");
        assert_eq!(stdout(&o), "", "{body}");
    }
}

#[test]
fn threads_run_under_the_runner() {
    let dir = scratch("threads");
    std::fs::write(
        dir.join("th.aipl"),
        r#"(module th
  (import io)
  (fn work [c:i32] -> i32 (unsafe) (for i 1 1001 (let _o:i32 (atomic.add c 1))) 0)
  (fn main [] -> i32 (unsafe)
    (let c:i32 (mem.alloc 4))
    (let a:i32 (thread.spawn (ref work) c))
    (let b:i32 (thread.spawn (ref work) c))
    (let _x:i32 (thread.join a))
    (let _y:i32 (thread.join b))
    (call io.println_int "total: " (mem.load32 c))
    0))"#,
    )
    .unwrap();
    compile(&dir.join("th.aipl"), &dir.join("th.wasm"), &[]);
    let o = run_in(&dir, &dir.join("th.wasm"), &[], "");
    assert_eq!(stdout(&o), "total: 2000\n", "{}", String::from_utf8_lossy(&o.stderr));
}

/// `aipl compile --exe`: one file that runs anywhere with no toolchain, and
/// `--sandbox` confines it to the working directory. The self-hosted compiler
/// itself, built this way, compiles a program to the Rust toolchain's bytes.
/// Both kinds: native machine code (`--target native`, the default on Linux
/// x86-64) and the launcher bundle (`--target wasm`).
#[test]
fn standalone_executables() {
    // The native backend targets Linux x86-64 only.
    let targets: &[&str] = if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        &["native", "wasm"]
    } else {
        &["wasm"]
    };
    for target in targets {
        standalone_executables_for(target);
    }
    // the default: native code on Linux x86-64, the launcher bundle elsewhere
    let dir = scratch("exe_default");
    let wc = dir.join("word_count");
    compile(&root().join("examples/word_count.aipl"), &wc, &["--exe"]);
    assert_kind(&wc, if cfg!(all(target_os = "linux", target_arch = "x86_64")) { "native" } else { "wasm" });
    let _ = std::fs::remove_dir_all(&dir);
}

/// Which kind of executable `path` is: an ELF file AIPL wrote (no launcher
/// inside), or the launcher with a module appended.
fn assert_kind(path: &Path, target: &str) {
    let bytes = std::fs::read(path).unwrap();
    if cfg!(target_os = "macos") {
        assert!(
            bytes.starts_with(&[0xcf, 0xfa, 0xed, 0xfe])
                || bytes.starts_with(&[0xfe, 0xed, 0xfa, 0xcf])
                || bytes.starts_with(&[0xca, 0xfe, 0xba, 0xbe]),
            "{}: not a Mach-O executable",
            path.display()
        );
    } else {
        assert!(bytes.starts_with(b"\x7fELF"), "{}: not an executable", path.display());
    }
    match target {
        "native" => {
            assert!(!bytes.ends_with(b"AIPLEXE2"), "{}: a launcher bundle, not native code", path.display());
            // native code is a few MB at most; a launcher bundle is about 18 MB
            assert!(bytes.len() < 5_000_000, "{}: {} bytes is not a native AIPL executable", path.display(), bytes.len());
        }
        _ => assert!(bytes.ends_with(b"AIPLEXE2"), "{}: not a launcher bundle", path.display()),
    }
}

fn standalone_executables_for(target: &str) {
    let t = format!("--target={target}");
    let dir = scratch(&format!("exe_{target}"));
    let wc = dir.join("word_count");
    compile(&root().join("examples/word_count.aipl"), &wc, &["--exe", &t]);
    assert_kind(&wc, target);
    let elsewhere = scratch(&format!("exe_elsewhere_{target}"));
    std::fs::write(elsewhere.join("notes.txt"), "one two\nthree\n").unwrap();
    let o = run_in(&elsewhere, &wc, &["notes.txt"], "");
    assert_eq!(stdout(&o), "lines: 2\nwords: 3\nbytes: 14\n");

    let boxed = dir.join("word_count_boxed");
    compile(&root().join("examples/word_count.aipl"), &boxed, &["--exe", "--sandbox", &t]);
    assert_kind(&boxed, target);
    let abs = elsewhere.join("notes.txt");
    let o = run_in(&elsewhere, &boxed, &[abs.to_str().unwrap()], "");
    assert!(stdout(&o).is_empty(), "sandboxed program read an absolute path: {}", stdout(&o));
    let o = run_in(&elsewhere, &boxed, &["notes.txt"], "");
    assert!(stdout(&o).starts_with("lines: 2"));

    // the AIPL compiler as a standalone command
    let aiplc = dir.join("aiplc");
    compile(&root().join("aipl_src/driver.aipl"), &aiplc, &["--exe", &t]);
    assert_kind(&aiplc, target);
    let out = dir.join("wc.wasm");
    let o = run_in(&root(), &aiplc, &["examples/word_count.aipl", out.to_str().unwrap()], "");
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    let rust = dir.join("wc_rust.wasm");
    compile(&root().join("examples/word_count.aipl"), &rust, &[]);
    assert!(std::fs::read(&out).unwrap() == std::fs::read(&rust).unwrap(), "aiplc output differs from the Rust toolchain");
    // a trap and threads, as executables of this kind
    std::fs::write(dir.join("t.aipl"), "(module t (fn main [] -> i32 (/ 1 0)))").unwrap();
    let trapper = dir.join("trapper");
    compile(&dir.join("t.aipl"), &trapper, &["--exe", &t]);
    let o = run_in(&dir, &trapper, &[], "");
    assert_eq!(o.status.code(), Some(134));
    assert_eq!(String::from_utf8_lossy(&o.stderr), format!("{}: wasm trap: integer divide by zero\n  at main (t.aipl:1:30)\n", trapper.display()));
    std::fs::write(
        dir.join("th.aipl"),
        "(module th (import io) (fn sq [x:i32] -> i32 (unsafe) (* x x)) (fn main [] -> i32 (unsafe) (call io.println_int \"joined: \" (thread.join (thread.spawn (ref sq) 9))) 0))",
    )
    .unwrap();
    let threaded = dir.join("threaded");
    compile(&dir.join("th.aipl"), &threaded, &["--exe", &t]);
    assert_kind(&threaded, target);
    assert_eq!(stdout(&run_in(&dir, &threaded, &[], "")), "joined: 81\n");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&elsewhere);
}

/// --target without --exe is an error, and so is an unknown target.
#[test]
fn target_needs_exe_and_a_known_name() {
    let dir = scratch("target_flags");
    std::fs::write(dir.join("m.aipl"), "(module m (fn main [] -> i32 0))").unwrap();
    for args in [vec!["--target=native"], vec!["--exe", "--target=arm"]] {
        let o = Command::new(AIPL).arg("compile").arg(dir.join("m.aipl")).arg("-o").arg(dir.join("m")).args(&args).output().unwrap();
        assert!(!o.status.success(), "{args:?} was accepted");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Value structs (docs/design/VALUES_PLAN.md V3) build for both targets,
/// and both executables run the program to its end (a trap would be status
/// 134; main's checksum is checked in tests/test_native.rs).
#[test]
fn value_structs_build_for_both_targets() {
    let dir = scratch("exe_values");
    let src = root().join("tests/aipl/values_flow.aipl");
    let targets: &[&str] = if cfg!(all(target_os = "linux", target_arch = "x86_64")) { &["wasm", "native"] } else { &["wasm"] };
    for target in targets {
        let exe = dir.join(format!("flow_{target}"));
        compile(&src, &exe, &["--exe", &format!("--target={target}")]);
        assert_kind(&exe, target);
        assert_eq!(run_in(&dir, &exe, &[], "").status.code(), Some(0), "{target}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// On Apple Silicon (macOS), standalone executables are launcher bundles
/// (aipl-run with wasm and an AIPLEXE2 trailer appended). This test checks
/// that the bundle executes on macOS, passes arguments, and exits cleanly.
#[test]
#[cfg(target_os = "macos")]
fn macos_launcher_bundle_executes_and_preserves_behavior() {
    let dir = scratch("macos_bundle");
    let src = dir.join("prog.aipl");
    std::fs::write(
        &src,
        r#"(module prog
  (import io) (import os) (import str)
  (fn main [] -> i32
    (call io.println_int "args: " (call os.arg_count))
    (let f:str.Bytes (call os.arg 1))
    (call io.println_int "arg1 len: " (get f str.Bytes.len))
    0))"#,
    )
    .unwrap();
    let exe = dir.join("bundle_test");
    compile(&src, &exe, &["--exe"]);
    assert_kind(&exe, "wasm");
    let o = run_in(&dir, &exe, &["hello"], "");
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(stdout(&o), "args: 2\narg1 len: 5\n");
    let _ = std::fs::remove_dir_all(&dir);
}
