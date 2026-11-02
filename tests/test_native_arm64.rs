//! Native ARM64 backend task NA2 (docs/design/ARM64_PLAN.md):
//! aipl_src/native/arm64.aipl encodes AArch64 instructions. Its self-tests
//! compare every form, over registers x0-x30, with the output of Apple's
//! assembler pasted into the module (tools/arm64_vectors.py); here they must
//! pass in the VM and compiled to wasm (where the native compiler will run).

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use std::path::Path;

mod arm64;
use wasmtime::{Engine, Linker, Module as WasmModule, Store};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::p2::pipe::MemoryOutputPipe;
use wasmtime_wasi::WasiCtxBuilder;

const GROUPS: i32 = 11;

fn encoder() -> aipl_core::ast::Module {
    let m = Resolver::resolve(&Path::new(env!("CARGO_MANIFEST_DIR")).join("aipl_src/native/arm64.aipl")).unwrap();
    TypeChecker::new().check_module(&m).unwrap();
    m
}

#[test]
fn the_arm64_encoder_matches_the_assembler_in_the_vm() {
    let mut vm = VM::new();
    vm.load_module(encoder());
    assert_eq!(vm.invoke("run_arm64_tests", vec![]).unwrap(), Value::Int(GROUPS as i64));
}

#[test]
fn the_arm64_encoder_matches_the_assembler_compiled_to_wasm() {
    let wasm = WasmCompiler::compile(&encoder()).unwrap();
    let engine = Engine::default();
    let module = WasmModule::new(&engine, &wasm).unwrap();
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
    let stdout = MemoryOutputPipe::new(1 << 16);
    let mut store = Store::new(&engine, WasiCtxBuilder::new().stdout(stdout.clone()).build_p1());
    let inst = linker.instantiate(&mut store, &module).unwrap();
    let run = inst.get_typed_func::<(), i32>(&mut store, "run_arm64_tests").unwrap();
    let passed = run.call(&mut store, ()).unwrap();
    drop(store);
    let out = String::from_utf8(stdout.contents().to_vec()).unwrap();
    assert_eq!(passed, GROUPS, "{out}");
}

fn runtime() -> aipl_core::ast::Module {
    let m = Resolver::resolve(&Path::new(env!("CARGO_MANIFEST_DIR")).join("aipl_src/native/runtime_arm64.aipl")).unwrap();
    TypeChecker::new().check_module(&m).unwrap();
    m
}

/// runtime_arm64.hello_exe in the VM: the bytes of the executable it builds.
fn hello_from_vm() -> Vec<u8> {
    let mut vm = VM::new();
    vm.load_module(runtime());
    let Value::Int64(p) = vm.invoke("hello_exe", vec![]).unwrap() else { panic!() };
    let p = p as u32 as usize;
    let word = |vm: &mut VM, at: usize| i32::from_le_bytes(vm.read_bytes(at, 4).try_into().unwrap()) as usize;
    let (addr, len) = (word(&mut vm, p), word(&mut vm, p + 4));
    vm.read_bytes(addr, len)
}

/// runtime_arm64.hello_exe compiled to wasm and run under wasmtime.
fn hello_from_wasm() -> Vec<u8> {
    let wasm = WasmCompiler::compile(&runtime()).unwrap();
    let engine = Engine::default();
    let module = WasmModule::new(&engine, &wasm).unwrap();
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
    let mut store = Store::new(&engine, WasiCtxBuilder::new().build_p1());
    let inst = linker.instantiate(&mut store, &module).unwrap();
    let f = inst.get_typed_func::<(), i64>(&mut store, "hello_exe").unwrap();
    let p = f.call(&mut store, ()).unwrap() as u32 as usize;
    let mem = inst.get_memory(&mut store, "memory").unwrap();
    let data = mem.data(&store);
    let word = |at: usize| i32::from_le_bytes(data[at..at + 4].try_into().unwrap()) as usize;
    data[word(p)..word(p) + word(p + 4)].to_vec()
}

#[test]
fn the_elf_writer_lays_out_aarch64_executables_on_64_kib_pages() {
    let elf = Resolver::resolve(&Path::new(env!("CARGO_MANIFEST_DIR")).join("aipl_src/native/elf.aipl")).unwrap();
    TypeChecker::new().check_module(&elf).unwrap();
    let mut vm = VM::new();
    vm.load_module(elf);
    assert_eq!(vm.invoke("run_elf_arm64_tests", vec![]).unwrap(), Value::Int(4));
}

#[test]
fn the_vm_and_compiled_wasm_build_identical_aarch64_executables() {
    let exe = hello_from_vm();
    assert_eq!(exe, hello_from_wasm());
    // an AArch64 ELF: machine 183, the first segment the headers at file offset 0 on a 64 KiB page
    assert_eq!(&exe[..4], b"\x7fELF");
    assert_eq!(u16::from_le_bytes([exe[18], exe[19]]), 183);
    assert_eq!(u64::from_le_bytes(exe[72..80].try_into().unwrap()), 0, "the first segment is not at file offset 0");
    assert_eq!(u64::from_le_bytes(exe[112..120].try_into().unwrap()), 65536);
}

/// The first AArch64 executable prints its line and exits with status 7, run
/// where AArch64 code can run (tests/arm64/mod.rs); elsewhere this says it did not.
#[test]
fn the_first_aarch64_executable_prints_and_exits_7() {
    let exe = hello_from_vm();
    let Some(out) = arm64::run(&exe, &[], b"") else {
        eprintln!("no AArch64 machine to run on (set AIPL_ARM64_SSH, install qemu-user, or run on AArch64 Linux): not run");
        return;
    };
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello from native AIPL\n", "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(out.status.code(), Some(7));
}
