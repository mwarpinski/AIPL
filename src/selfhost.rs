//! Host side of the self-hosted toolchain (P14): runs aipl_src/resolver.aipl
//! in the VM. `aipl compile --self` and the tests use it.

use crate::checker::TypeChecker;
use crate::resolver::Resolver;
use crate::vm::{Value, VM};
use std::path::Path;

/// The standard library directory, as `Resolver` uses it. The AIPL resolver
/// reads AIPL_PATH itself (the VM sees the process environment).
pub fn std_dir() -> String {
    format!("{}/aipl_src/std/", env!("CARGO_MANIFEST_DIR"))
}

/// Resolves the program whose entry file is `path` with `resolver.resolve_file`
/// in the VM: the flat, import-free source, or the resolver's error message.
/// Runs on a thread with a large stack (the VM recurses once per AIPL call).
pub fn resolve_with_aipl(path: &Path) -> Result<String, String> {
    let path = path.to_string_lossy().into_owned();
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(move || resolve_on_this_thread(&path))
        .map_err(|e| e.to_string())?
        .join()
        .map_err(|_| "the resolver thread panicked".to_string())?
}

fn resolve_on_this_thread(path: &str) -> Result<String, String> {
    let resolver_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("aipl_src/resolver.aipl");
    let module = Resolver::resolve(&resolver_path).map_err(|e| format!("resolve resolver.aipl: {e}"))?;
    TypeChecker::new().check_module(&module).map_err(|e| format!("check resolver.aipl: {e}"))?;
    let mut vm = VM::new();
    vm.load_module(module);
    let int = |v: Value| match v {
        Value::Int(i) => Ok(i as i32),
        other => Err(format!("expected Int, got {other:?}")),
    };
    let put = |vm: &mut VM, s: &str| -> Result<i32, String> {
        let addr = int(vm.invoke("host_alloc", vec![Value::Int(s.len() as i64 + 1)])?)?;
        vm.write_bytes(addr as usize, s.as_bytes());
        Ok(addr)
    };
    let dirs = std_dir();
    let p = put(&mut vm, path)?;
    let d = put(&mut vm, &dirs)?;
    let args = [p, path.len() as i32, d, dirs.len() as i32].map(|v| Value::Int(v as i64)).to_vec();
    let out = int(vm.invoke("resolve_file", args)?)? as usize;
    // Out [ok addr len]
    let word = |at: usize| i32::from_le_bytes(vm.read_bytes(at, 4).try_into().unwrap());
    let (ok, addr, len) = (word(out), word(out + 4), word(out + 8));
    let text = String::from_utf8(vm.read_bytes(addr as usize, len as usize)).map_err(|e| e.to_string())?;
    if ok == 1 { Ok(text) } else { Err(text) }
}
