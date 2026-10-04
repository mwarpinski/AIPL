//! The native backend's harness (docs/NATIVE_BACKEND_PLAN.md, from NE5).
//!
//! Every program in `PROGRAMS` is compiled to wasm by the Rust toolchain and
//! run under `aipl-run`, and translated to a native executable by
//! `aipl_src/native/native.aipl` and run directly. Both runs must give the
//! same stdout, stderr, and exit status. Later tasks extend `PROGRAMS`.
//!
//! The native compiler is AIPL; here it runs compiled to wasm under
//! wasmtime (the VM gives the same bytes: see
//! `the_vm_builds_the_same_executable`).

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::parser::Parser;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;
use wasmtime::{Engine, Linker, Module as WasmModule, Store};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::WasiCtxBuilder;

const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

/// (name, source). Each program's observable behaviour (output, exit status)
/// must be identical natively and under aipl-run.
const PROGRAMS: &[(&str, &str)] = &[
    // NE5: constants, locals, direct calls, return, drop, proc_exit
    ("main_returns", "(module m (fn main [] -> i32 42))"),
    ("exit_constant", "(module m (fn main [] -> i32 (sys.exit 3) 0))"),
    (
        "locals_and_calls",
        "(module m
           (fn third [a:i32 b:i32 c:i32] -> i32 (let x:i32 a) (set! x b) c)
           (fn main [] -> i32 (let k:i32 5) (sys.exit (call third 1 2 k)) 0))",
    ),
    (
        "every_parameter_position",
        "(module m
           (fn p0 [a:i32 b:i32 c:i32 d:i32 e:i32] -> i32 a)
           (fn p2 [a:i32 b:i32 c:i32 d:i32 e:i32] -> i32 c)
           (fn p4 [a:i32 b:i32 c:i32 d:i32 e:i32] -> i32 e)
           (fn main [] -> i32
             (sys.exit (call p0 (call p2 1 2 (call p4 9 9 9 9 17) 4 5) 2 3 4 5))
             0))",
    ),
    (
        "locals_start_at_zero",
        "(module m
           (fn f [] -> i32 (let a:i32 0) (let b:i32 0) (let c:i32 0) (set! b 11) b)
           (fn g [] -> i32 (let a:i32 0) (let b:i32 0) (let c:i32 0) c)
           (fn main [] -> i32 (let y:i32 (call f)) (let z:i32 (call g)) (sys.exit z) 0))",
    ),
    (
        // the outer call's first arguments sit below the nested call's: if
        // the nested call's arguments stayed on the stack, p0 would read 34
        "arguments_are_popped_after_a_call",
        "(module m
           (fn p0 [a:i32 b:i32 c:i32 d:i32 e:i32] -> i32 a)
           (fn p4 [a:i32 b:i32 c:i32 d:i32 e:i32] -> i32 e)
           (fn main [] -> i32 (sys.exit (call p0 1 2 (call p4 31 32 33 34 35) 4 5)) 0))",
    ),
    (
        "early_return",
        "(module m
           (fn f [x:i32] -> i32 (return x) 99)
           (fn main [] -> i32 (sys.exit (call f 23)) 0))",
    ),
    (
        "void_functions_and_i64_locals",
        "(module m
           (fn nothing [a:i32] -> void (let big:i64 4294967297i64) (let y:i32 a))
           (fn keep [x:i32] -> i32 (call nothing x) (call nothing 2) x)
           (fn main [] -> i32 (sys.exit (call keep 77)) 0))",
    ),
    ("exit_status_125", "(module m (fn main [] -> i32 (sys.exit 125) 0))"),
    ("exit_status_126_is_an_error", "(module m (fn main [] -> i32 (sys.exit 126) 0))"),
    ("exit_status_negative_is_an_error", "(module m (fn main [] -> i32 (sys.exit -1) 0))"),
];

/// Hand-built wasm for cases AIPL source cannot express (e.g. reading a
/// local before writing it): `f` is called by `_start`, and its result is
/// the exit status. Run both ways like `PROGRAMS`.
fn wasm_programs() -> Vec<(&'static str, Vec<u8>)> {
    use wasm_encoder::Instruction as I;
    vec![
        // wasm locals start at zero
        ("unwritten_locals_are_zero", exit_with(&[0, 3], &[I::LocalGet(2), I::End])),
        // the result is the stack top, not whatever was last in a register
        ("result_is_the_stack_top", exit_with(&[], &[I::I32Const(7), I::I32Const(9), I::Drop, I::End])),
        // local.tee keeps its value on the stack
        ("tee_keeps_the_value", exit_with(&[1], &[I::I32Const(3), I::I32Const(5), I::LocalTee(0), I::Drop, I::End])),
        ("tee_stores_the_value", exit_with(&[1], &[I::I32Const(6), I::LocalTee(0), I::Drop, I::LocalGet(0), I::End])),
        // i64 constants take a full slot; i32.wrap is not needed to exit with the low half
        ("set_then_get", exit_with(&[2], &[I::I32Const(12), I::LocalSet(1), I::I32Const(13), I::LocalSet(0), I::LocalGet(1), I::End])),
    ]
}

/// A module whose `_start` exits with `f()`; `f` has `locals[i]` i32 locals
/// per group and the given body.
fn exit_with(locals: &[u32], body: &[wasm_encoder::Instruction]) -> Vec<u8> {
    use wasm_encoder::{
        CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection, ImportSection, Instruction as I,
        MemorySection, MemoryType, Module, TypeSection, ValType,
    };
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32], []);
    types.ty().function([], []);
    types.ty().function([], [ValType::I32]);
    let mut imports = ImportSection::new();
    imports.import("wasi_snapshot_preview1", "proc_exit", EntityType::Function(0));
    let mut funcs = FunctionSection::new();
    funcs.function(1);
    funcs.function(2);
    let mut memory = MemorySection::new();
    memory.memory(MemoryType { minimum: 1, maximum: None, memory64: false, shared: false, page_size_log2: None });
    let mut exports = ExportSection::new();
    exports.export("_start", ExportKind::Func, 1);
    exports.export("memory", ExportKind::Memory, 0);
    let mut code = CodeSection::new();
    let mut start = Function::new([]);
    start.instruction(&I::Call(2)).instruction(&I::Call(0)).instruction(&I::End);
    code.function(&start);
    let mut f = Function::new(locals.iter().map(|n| (*n, ValType::I32)));
    for ins in body {
        f.instruction(ins);
    }
    code.function(&f);
    let mut m = Module::new();
    m.section(&types).section(&imports).section(&funcs).section(&memory).section(&exports).section(&code);
    m.finish()
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_native_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn to_wasm(src: &str) -> Vec<u8> {
    let m = Parser::parse(src).unwrap_or_else(|e| panic!("{e}"));
    TypeChecker::new().check_module(&m).unwrap_or_else(|e| panic!("{e}"));
    WasmCompiler::compile(&m).unwrap_or_else(|e| panic!("{e}"))
}

fn native_module() -> aipl_core::ast::Module {
    let m = Resolver::resolve(&root().join("aipl_src/native/native.aipl")).unwrap();
    TypeChecker::new().check_module(&m).unwrap();
    m
}

/// native.aipl compiled to wasm, shared by every test (compiled once).
fn native_compiler() -> &'static (Engine, WasmModule) {
    static C: OnceLock<(Engine, WasmModule)> = OnceLock::new();
    C.get_or_init(|| {
        let engine = Engine::default();
        let module = WasmModule::new(&engine, WasmCompiler::compile(&native_module()).unwrap()).unwrap();
        (engine, module)
    })
}

/// native.compile over `wasm`: Ok(executable) or Err(message).
fn to_native(wasm: &[u8]) -> Result<Vec<u8>, String> {
    let (engine, module) = native_compiler();
    let mut linker: Linker<WasiP1Ctx> = Linker::new(engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
    let mut store = Store::new(engine, WasiCtxBuilder::new().build_p1());
    let inst = linker.instantiate(&mut store, module).unwrap();
    let alloc = inst.get_typed_func::<i32, i32>(&mut store, "host_alloc").unwrap();
    let compile = inst.get_typed_func::<(i32, i32), i32>(&mut store, "compile_at").unwrap();
    let mem = inst.get_memory(&mut store, "memory").unwrap();
    let addr = alloc.call(&mut store, wasm.len() as i32 + 8).unwrap() as usize;
    mem.write(&mut store, addr, wasm).unwrap();
    let out = compile.call(&mut store, (addr as i32, wasm.len() as i32)).unwrap() as usize;
    output_of(mem.data(&store), out)
}

/// Reads a native.Output [ok:bool error:(ptr Bytes) exe:(ptr Bytes)] at `out`.
fn output_of(data: &[u8], out: usize) -> Result<Vec<u8>, String> {
    let word = |at: usize| i32::from_le_bytes(data[at..at + 4].try_into().unwrap()) as usize;
    let bytes = |p: usize| data[word(p)..word(p) + word(p + 4)].to_vec();
    if word(out) != 0 {
        Ok(bytes(word(out + 8)))
    } else {
        Err(String::from_utf8(bytes(word(out + 4))).unwrap())
    }
}

fn run_wasm(dir: &Path, wasm: &[u8]) -> Output {
    let path = dir.join("prog.wasm");
    std::fs::write(&path, wasm).unwrap();
    Command::new(RUNNER).arg(&path).current_dir(dir).output().unwrap()
}

fn run_native(dir: &Path, exe: &[u8]) -> Output {
    let path = dir.join("prog");
    std::fs::write(&path, exe).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    Command::new(&path).current_dir(dir).output().unwrap()
}

/// Builds `src` both ways and requires identical stdout, stderr, and status.
fn assert_native_matches(name: &str, src: &str) {
    assert_wasm_native_matches(name, &to_wasm(src));
}

fn assert_wasm_native_matches(name: &str, wasm: &[u8]) {
    let wasm = wasm.to_vec();
    let exe = to_native(&wasm).unwrap_or_else(|e| panic!("{name}: native compile failed: {e}"));
    let dir = scratch(name);
    let (want, got) = (run_wasm(&dir, &wasm), run_native(&dir, &exe));
    let _ = std::fs::remove_dir_all(&dir);
    if want.status.code() == Some(134) && got.status.code() == Some(134) {
        // A trap: aipl-run adds wasmtime's backtrace, which native code cannot
        // reproduce. Each line the native program prints must appear in
        // aipl-run's report. (Exact trap messages are NE6's design.)
        let theirs = String::from_utf8_lossy(&want.stderr).to_string();
        let ours = String::from_utf8_lossy(&got.stderr).to_string();
        assert!(!ours.trim().is_empty(), "{name}: the native trap printed nothing");
        for line in ours.lines() {
            assert!(theirs.contains(line.trim()), "{name}: native trap line {line:?} not in aipl-run's report:\n{theirs}");
        }
        assert_eq!(want.stdout, got.stdout, "{name}: stdout before the trap differs");
        return;
    }
    let show = |o: &Output| {
        format!("status {:?}, stdout {:?}, stderr {:?}", o.status.code(), String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
    };
    assert!(
        want.status.code() == got.status.code() && want.stdout == got.stdout && want.stderr == got.stderr,
        "{name}: native differs from aipl-run\n  aipl-run: {}\n  native:   {}",
        show(&want),
        show(&got)
    );
}

#[test]
fn every_program_matches_natively() {
    for (name, src) in PROGRAMS {
        assert_native_matches(name, src);
    }
    for (name, wasm) in wasm_programs() {
        assert_wasm_native_matches(name, &wasm);
    }
}

/// Guards against a harness that passes vacuously: the programs must not all
/// exit 0, and a deliberately different program must be told apart.
#[test]
fn the_harness_tells_programs_apart() {
    let dir = scratch("apart");
    let statuses: Vec<Option<i32>> =
        PROGRAMS.iter().map(|(_, src)| run_native(&dir, &to_native(&to_wasm(src)).unwrap()).status.code()).collect();
    assert!(statuses.iter().filter(|s| **s != Some(0)).count() >= 5, "{statuses:?}");
    // the hand-built programs exit with distinct, non-zero statuses
    let hand: Vec<Option<i32>> =
        wasm_programs().iter().map(|(_, w)| run_native(&dir, &to_native(w).unwrap()).status.code()).collect();
    assert_eq!(hand, vec![Some(0), Some(7), Some(3), Some(6), Some(12)]);
    let three = to_native(&to_wasm("(module m (fn main [] -> i32 (sys.exit 3) 0))")).unwrap();
    let four = run_wasm(&dir, &to_wasm("(module m (fn main [] -> i32 (sys.exit 4) 0))"));
    assert_ne!(run_native(&dir, &three).status.code(), four.status.code());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Anything not translated yet is an error naming it, never a wrong program.
#[test]
fn unsupported_instructions_and_imports_are_named() {
    for (src, expected) in [
        ("(module m (fn main [] -> i32 (sys.exit (+ 1 2)) 0))", "not supported natively yet: i32.add (opcode 106)"),
        ("(module m (fn main [] -> i32 (sys.print \"hi\") 0))", "import not supported natively yet: wasi_snapshot_preview1.fd_write"),
        ("(module m (fn f [] -> i32 7))", "the module has no _start export"),
    ] {
        assert_eq!(to_native(&to_wasm(src)), Err(expected.to_string()), "{src}");
    }
}

/// The native compiler run in the VM produces the same executable as the
/// compiled one used above.
#[test]
fn the_vm_builds_the_same_executable() {
    let wasm = to_wasm(PROGRAMS[2].1);
    let mut vm = VM::new();
    vm.load_module(native_module());
    let Value::Int(addr) = vm.invoke("host_alloc", vec![Value::Int(wasm.len() as i64 + 8)]).unwrap() else { panic!() };
    vm.write_bytes(addr as usize, &wasm);
    let Value::Int(out) = vm.invoke("compile_at", vec![Value::Int(addr), Value::Int(wasm.len() as i64)]).unwrap() else { panic!() };
    let word = |vm: &mut VM, at: usize| i32::from_le_bytes(vm.read_bytes(at, 4).try_into().unwrap()) as usize;
    assert_ne!(word(&mut vm, out as usize), 0, "the VM build failed");
    let exe_ptr = word(&mut vm, out as usize + 8);
    let (a, n) = (word(&mut vm, exe_ptr), word(&mut vm, exe_ptr + 4));
    assert_eq!(vm.read_bytes(a, n), to_native(&wasm).unwrap());
}
