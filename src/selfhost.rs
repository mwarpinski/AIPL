//! Host side of the self-hosted toolchain (P14): runs aipl_src/resolver.aipl
//! and aipl_src/driver.aipl, compiled to wasm by the Rust toolchain, under
//! wasmtime. `aipl compile --self` and the tests use it. (They ran in the VM
//! until 2026-10-20, many times slower; tests/test_selfhost.rs still runs
//! codegen.aipl compiling itself in the VM, which keeps the VM tested on the
//! whole compiler.)

use crate::checker::TypeChecker;
use crate::compiler::wasm::WasmCompiler;
use crate::resolver::Resolver;
use std::path::Path;
use std::sync::OnceLock;
use wasmtime::{Engine, Linker, Module as WasmModule, Store, TypedFunc};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::{FsPerms, WasiCtxBuilder};

/// The standard library directory, as `Resolver` uses it. The AIPL resolver
/// reads AIPL_PATH itself (the VM sees the process environment).
pub fn std_dir() -> String {
    format!("{}/aipl_src/std/", env!("CARGO_MANIFEST_DIR"))
}

/// One of the toolchain's modules (`aipl_src/NAME.aipl`) compiled to wasm
/// by the Rust toolchain, once per process.
fn toolchain(name: &'static str, cell: &'static OnceLock<Result<(Engine, WasmModule), String>>) -> Result<&'static (Engine, WasmModule), String> {
    cell.get_or_init(|| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("aipl_src/{name}.aipl"));
        let module = Resolver::resolve(&path).map_err(|e| format!("resolve {name}.aipl: {e}"))?;
        TypeChecker::new().check_module(&module).map_err(|e| format!("check {name}.aipl: {e}"))?;
        let wasm = WasmCompiler::compile(&module).map_err(|e| format!("compile {name}.aipl: {e}"))?;
        let mut config = wasmtime::Config::new();
        // the compiler recurses once per nesting level of the program it
        // reads; on_big_stack gives the thread the room
        config.async_stack_size(80 << 20).max_wasm_stack(64 << 20);
        let engine = Engine::new(&config).map_err(|e| e.to_string())?;
        let m = WasmModule::new(&engine, &wasm).map_err(|e| e.to_string())?;
        Ok((engine, m))
    })
    .as_ref()
    .map_err(|e| e.clone())
}

/// Calls `entry` (resolve_file or compile_file) of a toolchain module with
/// the path and the standard library's directory, in a fresh instance with
/// the working directory and `/` preopened (as aipl-run gives them): the
/// three words of the Out it returns, and the instance's memory.
fn call_toolchain(t: &(Engine, WasmModule), entry: &str, path: &str) -> Result<([i32; 3], Vec<u8>), String> {
    let (engine, module) = t;
    let mut linker: Linker<WasiP1Ctx> = Linker::new(engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |c: &mut WasiP1Ctx| c).map_err(|e| e.to_string())?;
    let mut b = WasiCtxBuilder::new();
    b.inherit_stderr().inherit_env();
    b.preopened_dir(".", ".", FsPerms::ReadWrite).map_err(|e| e.to_string())?;
    b.preopened_dir("/", "/", FsPerms::ReadWrite).map_err(|e| e.to_string())?;
    let mut store = Store::new(engine, b.build_p1());
    let inst = linker.instantiate(&mut store, module).map_err(|e| e.to_string())?;
    let memory = inst.get_memory(&mut store, "memory").ok_or("no memory")?;
    let alloc: TypedFunc<i32, i32> = inst.get_typed_func(&mut store, "host_alloc").map_err(|e| e.to_string())?;
    let mut put = |s: &str| -> Result<i32, String> {
        let a = alloc.call(&mut store, s.len() as i32 + 1).map_err(|e| e.to_string())?;
        memory.write(&mut store, a as usize, s.as_bytes()).map_err(|e| e.to_string())?;
        Ok(a)
    };
    let dirs = std_dir();
    let (p, d) = (put(path)?, put(&dirs)?);
    // a (ptr Out): the address in the low half (GENERATIONS_PLAN.md)
    let f: TypedFunc<(i32, i32, i32, i32), i64> = inst.get_typed_func(&mut store, entry).map_err(|e| e.to_string())?;
    let out = f.call(&mut store, (p, path.len() as i32, d, dirs.len() as i32)).map_err(|e| format!("{entry} trapped: {e}"))? as u32 as usize;
    let data = memory.data(&store).to_vec();
    let word = |at: usize| i32::from_le_bytes(data[at..at + 4].try_into().unwrap());
    Ok(([word(out), word(out + 4), word(out + 8)], data))
}

fn bytes(data: &[u8], addr: i32, len: i32) -> Vec<u8> {
    data[addr as usize..addr as usize + len as usize].to_vec()
}

/// Runs f on a thread with a stack the wasm stack above fits in.
fn on_big_stack<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    std::thread::Builder::new()
        .stack_size(128 << 20)
        .spawn(f)
        .map_err(|e| e.to_string())?
        .join()
        .map_err(|_| "the toolchain thread panicked".to_string())?
}

/// Resolves the program whose entry file is `path` with
/// `resolver.resolve_file`: the flat, import-free source, or the
/// resolver's error message.
pub fn resolve_with_aipl(path: &Path) -> Result<String, String> {
    static RESOLVER: OnceLock<Result<(Engine, WasmModule), String>> = OnceLock::new();
    let t = toolchain("resolver", &RESOLVER)?;
    let path = path.to_string_lossy().into_owned();
    // Out [ok addr len]
    let ([ok, addr, len], data) = on_big_stack(move || call_toolchain(t, "resolve_file", &path))?;
    let text = String::from_utf8(bytes(&data, addr, len)).map_err(|e| e.to_string())?;
    if ok == 1 { Ok(text) } else { Err(text) }
}

/// Compiles the program whose entry file is `path` with the self-hosted
/// toolchain (`driver.compile_file`: resolver, checker, code generator):
/// the module's bytes, or its error. What `aiplc` does natively.
pub fn compile_with_aipl(path: &Path) -> Result<Vec<u8>, String> {
    static DRIVER: OnceLock<Result<(Engine, WasmModule), String>> = OnceLock::new();
    let t = toolchain("driver", &DRIVER)?;
    let path = path.to_string_lossy().into_owned();
    // Out [status addr len]: 0 the module, 1 and 3 an error's text, 2 a compile error's code
    let ([status, addr, len], data) = on_big_stack(move || call_toolchain(t, "compile_file", &path))?;
    match status {
        0 => Ok(bytes(&data, addr, len)),
        2 => Err(format!("compile error {len} (see AIPL_SPEC.md 6.4)")),
        _ => Ok(String::from_utf8_lossy(&bytes(&data, addr, len)).into_owned()).and_then(Err),
    }
}
