//! The benchmarks (benchmarks/<name>/) as correctness tests: each AIPL
//! program, built as wasm and natively, run with its `test_args` (and
//! `stdin`, if any), must print exactly `expected.txt`, which the C
//! reference version produced. Timing is tools/bench.py's job.

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::resolver::Resolver;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn run(cmd: &mut Command, stdin: &[u8]) -> (Option<i32>, Vec<u8>, String) {
    use std::io::Write;
    let mut child = loop {
        match cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => std::thread::sleep(std::time::Duration::from_millis(10)),
            r => break r.unwrap(),
        }
    };
    let mut input = child.stdin.take().unwrap();
    let stdin = stdin.to_vec();
    std::thread::spawn(move || {
        let _ = input.write_all(&stdin);
    });
    let out = child.wait_with_output().unwrap();
    (out.status.code(), out.stdout, String::from_utf8_lossy(&out.stderr).into_owned())
}

#[test]
fn every_benchmark_prints_its_expected_output() {
    let mut checked = 0;
    for entry in std::fs::read_dir(root().join("benchmarks")).unwrap() {
        let dir = entry.unwrap().path();
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        let args: Vec<String> = std::fs::read_to_string(dir.join("test_args")).unwrap().split_whitespace().map(String::from).collect();
        let expected = std::fs::read(dir.join("expected.txt")).unwrap();
        let stdin = std::fs::read(dir.join("stdin")).unwrap_or_default();
        let module = Resolver::resolve(&dir.join(format!("{name}.aipl"))).unwrap_or_else(|e| panic!("{name}: {e}"));
        TypeChecker::new().check_module(&module).unwrap_or_else(|e| panic!("{name}: {e}"));
        let wasm = WasmCompiler::compile(&module).unwrap();
        let scratch = std::env::temp_dir().join(format!("aipl_bench_{}_{name}", std::process::id()));
        std::fs::create_dir_all(&scratch).unwrap();
        let wasm_path = scratch.join(format!("{name}.wasm"));
        std::fs::write(&wasm_path, &wasm).unwrap();
        let exe = scratch.join(&name);
        std::fs::write(&exe, aipl_core::native::executable(&wasm, false).unwrap_or_else(|e| panic!("{name}: {e}"))).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        for (way, out) in [
            ("wasm", run(Command::new(RUNNER).arg(&wasm_path).args(&args).current_dir(&dir), &stdin)),
            ("native", run(Command::new(&exe).args(&args).current_dir(&dir), &stdin)),
        ] {
            assert_eq!(out.0, Some(0), "{name} ({way}): {}", out.2);
            assert!(out.1 == expected, "{name} ({way}): output differs from expected.txt:\n{}", String::from_utf8_lossy(&out.1));
        }
        let _ = std::fs::remove_dir_all(&scratch);
        checked += 1;
    }
    assert!(checked >= 1);
}
