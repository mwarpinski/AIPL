//! Native backend task NE3: aipl_src/native/x64.aipl encodes x86-64
//! instructions. Its self-tests compare every form, over all sixteen
//! registers, with GNU as output pasted into the test; here they must pass
//! in the VM and compiled to wasm (where the native compiler will run).

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use std::path::Path;
use wasmtime::{Engine, Linker, Module as WasmModule, Store};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::p2::pipe::MemoryOutputPipe;
use wasmtime_wasi::WasiCtxBuilder;

const GROUPS: i32 = 9;

fn encoder() -> aipl_core::ast::Module {
    let m = Resolver::resolve(&Path::new(env!("CARGO_MANIFEST_DIR")).join("aipl_src/native/x64.aipl")).unwrap();
    TypeChecker::new().check_module(&m).unwrap();
    m
}

#[test]
fn the_encoder_matches_the_assembler_in_the_vm() {
    let mut vm = VM::new();
    vm.load_module(encoder());
    assert_eq!(vm.invoke("run_x64_tests", vec![]).unwrap(), Value::Int(GROUPS as i64));
}

#[test]
fn the_encoder_matches_the_assembler_compiled_to_wasm() {
    let wasm = WasmCompiler::compile(&encoder()).unwrap();
    let engine = Engine::default();
    let module = WasmModule::new(&engine, &wasm).unwrap();
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
    let stdout = MemoryOutputPipe::new(1 << 16);
    let mut store = Store::new(&engine, WasiCtxBuilder::new().stdout(stdout.clone()).build_p1());
    let inst = linker.instantiate(&mut store, &module).unwrap();
    let run = inst.get_typed_func::<(), i32>(&mut store, "run_x64_tests").unwrap();
    let passed = run.call(&mut store, ()).unwrap();
    drop(store);
    let out = String::from_utf8(stdout.contents().to_vec()).unwrap();
    assert_eq!(passed, GROUPS, "{out}");
}
