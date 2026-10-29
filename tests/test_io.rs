//! std/io's errors (LANGUAGE_DECISIONS.md item 2): a read or a write that
//! fails gives an io.Error, the same kind in the VM, under aipl-run, and
//! natively, because every fs.* op fails with minus its WASI errno in every
//! backend (AIPL_SPEC.md 4.C); and both toolchains compile it alike.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.starts_with("[AIPL")).map(|l| format!("{l}\n")).collect()
}

/// A directory holding adir/, afile, and locked.txt, which nobody may read.
fn workdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_io_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("adir")).unwrap();
    std::fs::write(d.join("afile"), "a file\n").unwrap();
    let locked = d.join("locked.txt");
    std::fs::write(&locked, "secret\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    }
    d
}

#[test]
fn each_io_error_is_the_same_in_every_backend() {
    let dir = workdir("errors");
    let src = root().join("tests/aipl/io/errors.aipl");
    let (wasm, exe) = (dir.join("p.wasm"), dir.join("p"));
    assert!(Command::new(AIPL).arg("compile").arg(&src).arg("-o").arg(&wasm).status().unwrap().success());
    assert!(Command::new(AIPL).args(["compile", "--exe"]).arg(&src).arg("-o").arg(&exe).status().unwrap().success());
    // root reads everything: the locked file is then readable, and says so
    let locked = if std::fs::read(dir.join("locked.txt")).is_ok() { "read" } else { "permission denied" };
    let want = format!(
        "missing: no such file or directory\ndirectory: is a directory\nmissing directory: no such file or directory\n\
         file as directory: no such file or directory\nlocked: {locked}\na file: read\n\
         write into a missing directory: no such file or directory\nwrite a directory: is a directory\nbranch on not_found: true\n\
         EEXIST: file exists, a kind of its own: true\nENOTEMPTY: directory not empty, a kind of its own: true\n"
    );
    let runs = [
        ("VM", Command::new(AIPL).arg("eval").arg(&src).current_dir(&dir).output().unwrap()),
        ("aipl-run", Command::new(RUNNER).arg(&wasm).current_dir(&dir).output().unwrap()),
        ("native", Command::new(&exe).current_dir(&dir).output().unwrap()),
    ];
    for (backend, o) in &runs {
        assert!(o.status.success(), "{backend}: {}", String::from_utf8_lossy(&o.stderr));
        assert_eq!(stdout(o), want, "{backend}");
    }
    // the self-hosted toolchain's module is the Rust one's
    let module = aipl_core::resolver::Resolver::resolve(&src).unwrap();
    aipl_core::checker::TypeChecker::new().check_module(&module).unwrap();
    let rust = aipl_core::compiler::wasm::WasmCompiler::compile(&module).unwrap();
    assert!(aipl_core::selfhost::compile_with_aipl(&src).unwrap() == rust, "the two toolchains' modules differ");
    let _ = std::fs::remove_dir_all(&dir);
}
