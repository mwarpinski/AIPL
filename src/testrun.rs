//! `aipl test FILE|DIR [-k TEXT] [--vm | --native | --all-backends]`
//! (docs/design/FEEDBACK_TOOLS_PLAN.md F4, AIPL_SPEC.md 4.L): runs each
//! `(test "name" ...)` of a file in a fresh process, so tests cannot see
//! each other's state and a trap fails only its own test. The test build
//! (Resolver::resolve_tests) is compiled once; test K runs as `PROGRAM K`
//! under aipl-run (the default: wasm's semantics are the specification),
//! natively, or in the VM (`aipl test --run-one K --vm FILE`, this binary
//! again). A test passes when it exits 0; with --all-backends, only when
//! every backend passes it with the same output.

use crate::checker::TypeChecker;
use crate::compiler::wasm::WasmCompiler;
use crate::resolver::{Resolver, TestInfo};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Backend {
    Vm,
    Wasm,
    Native,
}

impl Backend {
    fn name(self) -> &'static str {
        match self {
            Backend::Vm => "VM",
            Backend::Wasm => "aipl-run",
            Backend::Native => "native",
        }
    }
}

/// What a test did in one backend.
#[derive(PartialEq)]
struct Run {
    code: i32,
    out: String,
}

/// The test files under `path` (a file, or every .aipl file below a
/// directory, sorted) and the tests in each.
pub fn files(path: &Path) -> Result<Vec<PathBuf>, String> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut out = Vec::new();
    let mut dirs = vec![path.to_path_buf()];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))? {
            let p = e.map_err(|e| e.to_string())?.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "aipl") {
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// The tests in the file at `path`, without building it.
pub fn tests_in(path: &Path) -> Vec<TestInfo> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|src| crate::resolver::Resolver::test_names(&src, path).ok())
        .unwrap_or_default()
}

/// Runs the file's tests whose names contain `filter`; prints a line for
/// each and returns (passed, failed). `me` is this executable (for the VM),
/// `runner` the aipl-run launcher.
pub fn run_file(path: &Path, filter: Option<&str>, backends: &[Backend], me: &Path, runner: &Path) -> Result<(usize, usize), String> {
    let (module, tests) = Resolver::resolve_tests(path)?;
    TypeChecker::new().check_module(&module)?;
    let chosen: Vec<(usize, &TestInfo)> = tests.iter().enumerate().filter(|(_, t)| filter.is_none_or(|f| t.name.contains(f))).collect();
    println!("{}", path.display());
    let dir = std::env::temp_dir().join(format!("aipl_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let (wasm_file, exe_file) = (dir.join("tests.wasm"), dir.join("tests"));
    if backends.iter().any(|b| *b != Backend::Vm) {
        let wasm = WasmCompiler::compile(&module)?;
        std::fs::write(&wasm_file, &wasm).map_err(|e| e.to_string())?;
        if backends.contains(&Backend::Native) {
            std::fs::write(&exe_file, crate::native::executable(&wasm, false)?).map_err(|e| e.to_string())?;
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&exe_file, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
        }
    }
    let (mut passed, mut failed) = (0, 0);
    for (k, t) in chosen {
        let runs: Vec<(Backend, Run)> = backends
            .iter()
            .map(|b| {
                let mut cmd = match b {
                    Backend::Vm => {
                        let mut c = Command::new(me);
                        c.args(["test", "--vm", "--run-one", &k.to_string()]).arg(path);
                        c
                    }
                    Backend::Wasm => {
                        let mut c = Command::new(runner);
                        c.arg(&wasm_file).arg(k.to_string());
                        c
                    }
                    Backend::Native => {
                        let mut c = Command::new(&exe_file);
                        c.arg(k.to_string());
                        c
                    }
                };
                let run = match cmd.output() {
                    Ok(o) => {
                        // the program is named as the file, not the temporary build
                        let err = String::from_utf8_lossy(&o.stderr)
                            .replace(&format!("{}: ", wasm_file.display()), "")
                            .replace(&format!("{}: ", exe_file.display()), "");
                        Run { code: o.status.code().unwrap_or(134), out: format!("{}{err}", String::from_utf8_lossy(&o.stdout)) }
                    }
                    Err(e) => Run { code: 134, out: format!("cannot run the {} test build: {e}\n", b.name()) },
                };
                (*b, run)
            })
            .collect();
        let same = runs.windows(2).all(|w| w[0].1 == w[1].1);
        let ok = same && runs.iter().all(|(_, r)| r.code == 0);
        println!("  {}  {}", if ok { "ok  " } else { "FAIL" }, t.name);
        if ok {
            passed += 1;
            continue;
        }
        failed += 1;
        if same {
            print_indented(&runs[0].1.out);
        } else {
            for (b, r) in &runs {
                println!("        {} (exit {}):", b.name(), r.code);
                print_indented(&r.out);
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok((passed, failed))
}

fn print_indented(text: &str) {
    for line in text.lines() {
        println!("        {line}");
    }
}

/// `aipl test --run-one K --vm FILE`: test K in the VM, in this process,
/// exiting with its failure count (or 134 after a trap, as compiled code).
pub fn run_one_in_vm(path: &Path, k: usize) -> Result<i32, String> {
    let (module, _) = Resolver::resolve_tests(path)?;
    TypeChecker::new().check_module(&module)?;
    let mut vm = crate::vm::VM::new();
    vm.set_args(vec![path.display().to_string(), k.to_string()]);
    vm.load_module(module);
    match vm.invoke("_start", vec![]) {
        Ok(_) => Ok(0),
        // the VM's sys.exit ends the run with this error
        Err(e) if e.starts_with("sys.exit(") && e.ends_with(") requested") => {
            Ok(e["sys.exit(".len()..e.len() - ") requested".len()].parse().unwrap_or(1))
        }
        Err(e) => {
            // compiled code's call chain leaves out the generated _start
            let chain: String = vm.call_chain().lines().filter(|l| !l.trim_start().starts_with("at _start ")).map(|l| format!("\n{l}")).collect();
            eprintln!("{e}{chain}");
            Ok(134)
        }
    }
}
