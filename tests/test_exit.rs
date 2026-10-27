//! A program's exit status (AIPL_SPEC.md 6.5): main's i32, 0 for void, and
//! for a main returning (result i32 str) ok's value, or 1 after err's
//! message on standard error; sys.exit's anywhere. The same in the VM
//! (`aipl eval`), under aipl-run, natively, and in the launcher bundle; a
//! status outside 0-125 is an error with status 134 everywhere. Both
//! toolchains compile each program to the same bytes.

use std::process::Command;

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

/// The programs whose bytes both toolchains are compared on: one of each
/// shape `_start` takes (an i32, void, a result's ok, and its err).
const PARITY: &[&str] = &["three", "void", "ok_five", "err"];

/// (name, program, exit status, standard output, standard error after the
/// program's path, which each backend names its own way)
const CASES: &[(&str, &str, i32, &str, &str)] = &[
    ("zero", "(module p (fn main [] -> i32 (println \"done\") 0))", 0, "done\n", ""),
    ("three", "(module p (fn main [] -> i32 3))", 3, "", ""),
    ("highest", "(module p (fn main [] -> i32 125))", 125, "", ""),
    ("too_high", "(module p (fn main [] -> i32 200))", 134, "", ": exit with invalid exit status outside of [0..126)\n"),
    ("negative", "(module p (fn main [] -> i32 -1))", 134, "", ": exit with invalid exit status outside of [0..126)\n"),
    ("void", "(module p (fn main [] -> void (println \"hi\")))", 0, "hi\n", ""),
    ("ok_zero", "(module p (fn main [] -> (result i32 str) (println \"fine\") (ok:str 0)))", 0, "fine\n", ""),
    ("ok_five", "(module p (fn main [] -> (result i32 str) (ok:str 5)))", 5, "", ""),
    ("err", "(module p (fn main [] -> (result i32 str) (println \"before\") (err:i32 \"cannot open input.txt\")))", 1, "before\n", "cannot open input.txt\n"),
    ("sys_exit", "(module p (fn main [] -> i32 (sys.exit 7) 0))", 7, "", ""),
    ("sys_exit_in_result", "(module p (fn main [] -> (result i32 str) (sys.exit 9) (ok:str 0)))", 9, "", ""),
];

#[test]
fn every_backend_exits_alike() {
    let dir = std::env::temp_dir().join(format!("aipl_exit_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, src, status, stdout, stderr) in CASES {
        let file = dir.join(format!("{name}.aipl"));
        std::fs::write(&file, src).unwrap();
        let (wasm, exe, bundle) = (dir.join(format!("{name}.wasm")), dir.join(name), dir.join(format!("{name}_bundle")));
        for (out, args) in [(&wasm, vec![]), (&exe, vec!["--exe"]), (&bundle, vec!["--exe", "--target", "wasm"])] {
            let o = Command::new(AIPL).arg("compile").args(&args).arg(&file).arg("-o").arg(out).output().unwrap();
            assert!(o.status.success(), "{name}: {}", String::from_utf8_lossy(&o.stderr));
        }
        let runs = [
            ("VM", Command::new(AIPL).arg("eval").arg(&file).output().unwrap()),
            ("aipl-run", Command::new(RUNNER).arg(&wasm).output().unwrap()),
            ("native", Command::new(&exe).output().unwrap()),
            ("bundle", Command::new(&bundle).output().unwrap()),
        ];
        for (backend, o) in runs {
            let out: String = String::from_utf8_lossy(&o.stdout).split_inclusive('\n').filter(|l| !l.starts_with("[AIPL")).collect();
            let err = String::from_utf8_lossy(&o.stderr).to_string();
            assert_eq!(o.status.code(), Some(*status), "{name}, {backend}: {err}");
            assert_eq!(out, *stdout, "{name}, {backend}");
            // a failure names the program first; the message after it is the same
            let err = if stderr.starts_with(':') { err.split_once(':').map(|(_, m)| format!(":{m}")).unwrap_or(err) } else { err };
            assert_eq!(err, *stderr, "{name}, {backend}");
        }
        if !PARITY.contains(name) {
            continue;
        }
        let rust = {
            let m = aipl_core::resolver::Resolver::resolve(&file).unwrap();
            aipl_core::checker::TypeChecker::new().check_module(&m).unwrap();
            aipl_core::compiler::wasm::WasmCompiler::compile(&m).unwrap()
        };
        assert_eq!(aipl_core::selfhost::compile_with_aipl(&file).unwrap(), rust, "{name}: both toolchains");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A full standard output (/dev/full) is an error the command reports, with
/// status 1, not a panic: `aipl eval` printing its status lines, and `aipl
/// compile` its own.
#[test]
fn a_full_standard_output_is_an_error_not_a_panic() {
    if !std::path::Path::new("/dev/full").exists() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("aipl_exit_{}_full", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("p.aipl");
    std::fs::write(&src, "(module p (fn main [] -> i32 0))").unwrap();
    for args in [vec!["eval"], vec!["compile", "-o", dir.join("p.wasm").to_str().unwrap()]] {
        let full = std::fs::OpenOptions::new().write(true).open("/dev/full").unwrap();
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_aipl")).args(&args).arg(&src).stdout(full).output().unwrap();
        let err = String::from_utf8_lossy(&o.stderr);
        assert_eq!(o.status.code(), Some(1), "{args:?}: {err}");
        assert!(err.contains("standard output: No space left on device") && !err.contains("panicked"), "{args:?}: {err}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
