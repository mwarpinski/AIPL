//! Native executables on a system without openat2 (Linux before 5.6, Rosetta
//! 2, a container profile that refuses it; docs/HANDOFF.md): tools/without_openat2.py
//! makes the system call fail (ENOSYS, or EPERM as an old Docker profile
//! does) for a command and its children. A program built without --sandbox
//! still opens, creates, lists, stats, and deletes files, the same as with
//! openat2; a --sandbox program says it needs openat2 and exits 1 at start.
//! (tests/test_native.rs is also run this way, by tools/ci.sh.)
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_openat_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn build(src: &Path, out: &Path, sandbox: bool) {
    let mut c = Command::new(AIPL);
    c.args(["compile", "--exe"]);
    if sandbox {
        c.arg("--sandbox");
    }
    let o = c.arg(src).arg("-o").arg(out).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

/// `exe args` in `dir`, plainly, or with openat2 failing with ENOSYS or EPERM.
fn run(dir: &Path, exe: &Path, args: &[&str], how: &str) -> Output {
    let mut c = if how == "plain" {
        Command::new(exe)
    } else {
        let mut c = Command::new("python3");
        c.arg(root().join("tools/without_openat2.py"));
        if how == "eperm" {
            c.arg("--eperm");
        }
        c.arg(exe);
        c
    };
    c.args(args).current_dir(dir).output().unwrap()
}

const FILES: &str = r#"(module files
  (import alloc) (import fs) (import io) (import option) (import str) (import vec)
  ;; writes a file, reads it back, lists the directory, stats the file, opens a missing one
  (fn main [] -> i32
    (let a:alloc.Allocator (call alloc.default))
    (match_result (call io.write_file "made.txt" (call str.from_str "twelve bytes"))
      (ok n (println "wrote {n}"))
      (err e (println "write failed: {(call io.message e)}")))
    (match_result (call io.read_file "made.txt")
      (ok b (println "read {(get b str.Bytes.len)}"))
      (err e (println "read failed: {(call io.message e)}")))
    (match_result (call io.read_file "missing.txt")
      (ok b (println "missing was read"))
      (err e (println "missing: {(call io.message e)}")))
    (match (call fs.list a ".")
      ((option.Option (ptr (vec.Vec str.Bytes))) some [names] (println "{(call (vec.len str.Bytes) names)} names"))
      ((option.Option (ptr (vec.Vec str.Bytes))) none (println "no listing")))
    (match (call fs.info "made.txt")
      ((option.Option fs.Info) some [i] (println "size {(get i fs.Info.size)}"))
      ((option.Option fs.Info) none (println "no info")))
    0))
"#;

#[test]
fn a_plain_program_opens_files_without_openat2() {
    let dir = scratch("plain");
    let (src, exe) = (dir.join("files.aipl"), dir.join("files"));
    std::fs::write(&src, FILES).unwrap();
    build(&src, &exe, false);
    let want = run(&dir, &exe, &[], "plain");
    assert!(want.status.success(), "{}", String::from_utf8_lossy(&want.stderr));
    assert_eq!(String::from_utf8_lossy(&want.stdout), "wrote 12\nread 12\nmissing: no such file or directory\n3 names\nsize 12\n");
    for how in ["enosys", "eperm"] {
        let _ = std::fs::remove_file(dir.join("made.txt"));
        let got = run(&dir, &exe, &[], how);
        assert_eq!((got.status.code(), got.stdout, got.stderr), (Some(0), want.stdout.clone(), Vec::new()), "{how}");
    }
    // the word counter reads a named file, the common case
    let (src, exe) = (root().join("examples/word_count.aipl"), dir.join("wc"));
    build(&src, &exe, false);
    std::fs::write(dir.join("input.txt"), "one two\nthree\n").unwrap();
    for how in ["plain", "enosys", "eperm"] {
        let got = run(&dir, &exe, &["input.txt"], how);
        assert_eq!(String::from_utf8_lossy(&got.stdout), "lines: 2\nwords: 3\nbytes: 14\n", "{how}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_sandboxed_program_says_it_needs_openat2() {
    let dir = scratch("sandboxed");
    let exe = dir.join("wcs");
    build(&root().join("examples/word_count.aipl"), &exe, true);
    std::fs::write(dir.join("input.txt"), "one two\nthree\n").unwrap();
    let ok = run(&dir, &exe, &["input.txt"], "plain");
    assert_eq!(String::from_utf8_lossy(&ok.stdout), "lines: 2\nwords: 3\nbytes: 14\n");
    for how in ["enosys", "eperm"] {
        let o = run(&dir, &exe, &["input.txt"], how);
        assert_eq!(o.status.code(), Some(1), "{how}");
        assert!(o.stdout.is_empty(), "{how}");
        assert_eq!(
            String::from_utf8_lossy(&o.stderr),
            "this program was built with --sandbox, which needs the openat2 system call (Linux 5.6 or newer, and not Rosetta 2)\n",
            "{how}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
