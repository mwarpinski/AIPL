//! Native backend task NE4: aipl_src/native/elf.aipl and runtime.aipl build
//! a Linux x86-64 executable entirely from AIPL. The executable must run,
//! print its line, and exit with status 7; building it in the VM and in
//! compiled wasm must give the same bytes.

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use wasmtime::{Engine, Linker, Module as WasmModule, Store};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::WasiCtxBuilder;

fn runtime() -> aipl_core::ast::Module {
    let m = Resolver::resolve(&Path::new(env!("CARGO_MANIFEST_DIR")).join("aipl_src/native/runtime.aipl")).unwrap();
    TypeChecker::new().check_module(&m).unwrap();
    m
}

/// runtime.hello_exe in the VM: the bytes of the (ptr str.Bytes) it returns.
fn hello_from_vm() -> Vec<u8> {
    let mut vm = VM::new();
    vm.load_module(runtime());
    let Value::Int(p) = vm.invoke("hello_exe", vec![]).unwrap() else { panic!() };
    let word = |vm: &mut VM, at: usize| i32::from_le_bytes(vm.read_bytes(at, 4).try_into().unwrap()) as usize;
    let (addr, len) = (word(&mut vm, p as usize), word(&mut vm, p as usize + 4));
    vm.read_bytes(addr, len)
}

/// runtime.hello_exe compiled to wasm and run under wasmtime.
fn hello_from_wasm() -> Vec<u8> {
    let wasm = WasmCompiler::compile(&runtime()).unwrap();
    let engine = Engine::default();
    let module = WasmModule::new(&engine, &wasm).unwrap();
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
    let mut store = Store::new(&engine, WasiCtxBuilder::new().build_p1());
    let inst = linker.instantiate(&mut store, &module).unwrap();
    let f = inst.get_typed_func::<(), i32>(&mut store, "hello_exe").unwrap();
    let p = f.call(&mut store, ()).unwrap() as usize;
    let mem = inst.get_memory(&mut store, "memory").unwrap();
    let data = mem.data(&store);
    let word = |at: usize| i32::from_le_bytes(data[at..at + 4].try_into().unwrap()) as usize;
    data[word(p)..word(p) + word(p + 4)].to_vec()
}

fn write_executable(name: &str, bytes: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(format!("aipl_native_{}_{}", std::process::id(), name));
    std::fs::write(&path, bytes).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[test]
fn the_first_native_executable_prints_and_exits_7() {
    let exe = hello_from_vm();
    let path = write_executable("hello", &exe);
    // retry while another test thread's fork briefly holds the new file
    // open for writing ("Text file busy"; see tests/test_native.rs)
    let out = loop {
        match Command::new(&path).output() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => std::thread::sleep(std::time::Duration::from_millis(10)),
            r => break r.unwrap(),
        }
    };
    std::fs::remove_file(&path).ok();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello from native AIPL\n");
    assert_eq!(out.stderr, b"");
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn the_vm_and_compiled_wasm_build_identical_executables() {
    assert_eq!(hello_from_vm(), hello_from_wasm());
}

/// readelf, when installed, accepts the file without warnings.
#[test]
fn readelf_accepts_the_executable_when_installed() {
    let path = write_executable("readelf", &hello_from_vm());
    let Ok(out) = Command::new("readelf").args(["-h", "-l", "-W"]).arg(&path).output() else {
        std::fs::remove_file(&path).ok();
        eprintln!("readelf not installed; skipped");
        return;
    };
    std::fs::remove_file(&path).ok();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success() && out.stderr.is_empty(), "{}", String::from_utf8_lossy(&out.stderr));
    for expected in [
        "ELF64",
        "EXEC (Executable file)",
        "Advanced Micro Devices X86-64",
        "Entry point address:               0x402000",
        "LOAD           0x001000 0x0000000000401000 0x0000000000401000 0x000017 0x000017 RW  0x1000",
        "LOAD           0x002000 0x0000000000402000 0x0000000000402000 0x000024 0x000024 R E 0x1000",
        "GNU_STACK      0x000000 0x0000000000000000 0x0000000000000000 0x000000 0x000000 RW  0x10",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in\n{text}");
    }
}

/// elf.aipl's own self-tests (also in aipl_src/test_suite.aipl).
#[test]
fn the_elf_self_tests_pass() {
    let m = Resolver::resolve(&Path::new(env!("CARGO_MANIFEST_DIR")).join("aipl_src/native/elf.aipl")).unwrap();
    let mut vm = VM::new();
    vm.load_module(m);
    assert_eq!(vm.invoke("run_elf_tests", vec![]).unwrap(), Value::Int(4));
}
