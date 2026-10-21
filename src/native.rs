//! Host side of the native backend (docs/design/NATIVE_BACKEND_PLAN.md, NE18):
//! runs aipl_src/native/native.aipl, compiled to wasm, in wasmtime to turn a
//! module into a Linux x86-64 executable. Every decision is in the AIPL;
//! this only moves bytes in and out (as `aipl compile --exe` needs a host
//! until the compiler itself ships as a native executable: NE17 shows it
//! can).

use crate::checker::TypeChecker;
use crate::compiler::wasm::WasmCompiler;
use crate::resolver::Resolver;
use std::path::Path;
use wasmtime::{Engine, Linker, Module, Store};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::WasiCtxBuilder;

/// True on the platforms the native backend targets.
pub fn supported() -> bool {
    cfg!(all(target_os = "linux", target_arch = "x86_64"))
}

/// The executable for `wasm` (a module AIPL compiled), or the native
/// backend's error. `sandbox`: only the working directory, no absolute paths.
pub fn executable(wasm: &[u8], sandbox: bool) -> Result<Vec<u8>, String> {
    executable_with(wasm, sandbox, &[])
}

/// `executable`, with the addresses and ports the program may listen on
/// (`--listen ADDR:PORT`) fixed in it; the native backend reads them with
/// std/ip.
pub fn executable_with(wasm: &[u8], sandbox: bool, listen: &[String]) -> Result<Vec<u8>, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("aipl_src/native/native.aipl");
    let backend = Resolver::resolve(&path).map_err(|e| format!("native backend: {e}"))?;
    TypeChecker::new().check_module(&backend).map_err(|e| format!("native backend: {e}"))?;
    let backend = WasmCompiler::compile(&backend).map_err(|e| format!("native backend: {e}"))?;
    let engine = Engine::default();
    let module = Module::new(&engine, &backend).map_err(|e| e.to_string())?;
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).map_err(|e| e.to_string())?;
    let mut store = Store::new(&engine, WasiCtxBuilder::new().inherit_stderr().build_p1());
    let inst = linker.instantiate(&mut store, &module).map_err(|e| e.to_string())?;
    let alloc = inst.get_typed_func::<i32, i32>(&mut store, "host_alloc").map_err(|e| e.to_string())?;
    let compile = inst.get_typed_func::<(i32, i32, i32, i32, i32), i64>(&mut store, "compile_full_at").map_err(|e| e.to_string())?;
    let mem = inst.get_memory(&mut store, "memory").ok_or("native backend: no memory")?;
    let addr = alloc.call(&mut store, wasm.len() as i32 + 8).map_err(|e| e.to_string())?;
    mem.write(&mut store, addr as usize, wasm).map_err(|e| e.to_string())?;
    let grants: String = listen.iter().map(|g| format!("{g}\n")).collect();
    let gaddr = alloc.call(&mut store, grants.len() as i32 + 8).map_err(|e| e.to_string())?;
    mem.write(&mut store, gaddr as usize, grants.as_bytes()).map_err(|e| e.to_string())?;
    // a (ptr Output): the address in the low half (GENERATIONS_PLAN.md)
    let out = compile
        .call(&mut store, (addr, wasm.len() as i32, sandbox as i32, gaddr, grants.len() as i32))
        .map_err(|e| format!("native backend trapped: {e}"))? as u32 as usize;
    // native.Output starts [addr:i32 len:i32 ok:bool]: the executable's
    // bytes when ok, else the error's
    let data = mem.data(&store);
    let word = |at: usize| u32::from_le_bytes(data[at..at + 4].try_into().unwrap()) as usize;
    let (addr, len, ok) = (word(out), word(out + 4), word(out + 8) != 0);
    let bytes = data.get(addr..addr + len).ok_or("native backend: its result is outside its memory")?.to_vec();
    if !ok {
        return Err(String::from_utf8_lossy(&bytes).into_owned());
    }
    // never report an executable that is not one (a wrong reading of the
    // result once wrote an empty file and said it had compiled)
    if !bytes.starts_with(b"\x7fELF") {
        return Err(format!("native backend: its result is not an ELF executable ({} bytes)", bytes.len()));
    }
    Ok(bytes)
}
