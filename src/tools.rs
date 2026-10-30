//! Commands written in AIPL, run by the Rust CLI: the tool's source (such
//! as aipl_src/doc.aipl, `aipl doc`) is compiled to wasm and run in
//! wasmtime as a WASI command, with the working directory and `/` open, the
//! environment passed through, and `AIPL_STD` set to this repository's
//! standard library unless it is set already. Every decision is in the
//! AIPL, which the self-hosted `aipl` also includes; this only starts it
//! (docs/design/FEEDBACK_TOOLS_PLAN.md, decision D1).

use crate::checker::TypeChecker;
use crate::compiler::wasm::WasmCompiler;
use crate::resolver::Resolver;
use wasmtime::{Engine, Linker, Module, Store};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::{FsPerms, I32Exit, WasiCtxBuilder};

/// Runs the tool at `source` (relative to the library, src/library.rs) with
/// `args` (argument 0 is `name`); its exit status.
pub fn run(source: &str, name: &str, args: &[String]) -> Result<i32, String> {
    let module = Resolver::resolve(&crate::library::dir().join(source)).map_err(|e| format!("{name}: {e}"))?;
    TypeChecker::new().check_module(&module).map_err(|e| format!("{name}: {e}"))?;
    let wasm = WasmCompiler::compile(&module).map_err(|e| format!("{name}: {e}"))?;

    let engine = Engine::default();
    let module = Module::new(&engine, &wasm).map_err(|e| e.to_string())?;
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).map_err(|e| e.to_string())?;
    crate::sys::add_chmod(&mut linker)?;
    let argv: Vec<String> = std::iter::once(name.to_string()).chain(args.iter().cloned()).collect();
    let mut b = WasiCtxBuilder::new();
    b.inherit_stdio().inherit_env().args(&argv);
    b.env("AIPL_STD", crate::library::std_dir().to_string_lossy());
    b.preopened_dir(".", ".", FsPerms::ReadWrite).map_err(|e| format!("cannot open the working directory: {e}"))?;
    b.preopened_dir("/", "/", FsPerms::ReadOnly).map_err(|e| format!("cannot open /: {e}"))?;
    let mut store = Store::new(&engine, b.build_p1());
    let inst = linker.instantiate(&mut store, &module).map_err(|e| e.to_string())?;
    let start = inst.get_typed_func::<(), ()>(&mut store, "_start").map_err(|e| e.to_string())?;
    match start.call(&mut store, ()) {
        Ok(()) => Ok(0),
        Err(e) => match e.downcast_ref::<I32Exit>() {
            Some(exit) => Ok(exit.0),
            None => Err(format!("{name} trapped: {e}")),
        },
    }
}
