//! End-to-end check of the self-hosted compiler (`aipl_src/codegen.aipl`):
//! run its `test_compile_compute` self-test in the VM, take the wasm bytes it
//! emits, validate them, execute them in wasmtime, and assert the compiled
//! functions compute the right answers. This is the check that the P5 prompt
//! asked for via `aipl test aipl_src/codegen.aipl --func test_compile_compute`
//! (that subcommand treats a non-zero return as a failure count, and the
//! self-test returns a byte length, so the equivalent contract lives here and
//! in `run_codegen_tests`, which test_suite.aipl runs).
//!
//! The self-test writes its output through `fs.write` to a fixed relative
//! path; this test runs with the crate root as the working directory, reads
//! the file back, and removes it.

use aipl_core::checker::TypeChecker;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use std::path::Path;
use wasmtime::{Engine, Instance, Module as WasmModule, Store, TypedFunc};

/// The self-tests write fixed output paths and change the process cwd, so runs
/// are serialised across the (parallel) test threads in this binary.
static SELF_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn run_codegen_self_test(fn_name: &str, out_rel_path: &str) -> Vec<u8> {
    let _serial = SELF_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out_path = root.join(out_rel_path);
    let _ = std::fs::remove_file(&out_path);

    let module = Resolver::resolve(&root.join("aipl_src/codegen.aipl")).expect("resolve codegen.aipl");
    TypeChecker::new().check_module(&module).expect("check codegen.aipl");
    let mut vm = VM::new();
    vm.load_module(module);

    // The self-test writes via a cwd-relative path.
    let prev_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(root).unwrap();
    let result = vm.invoke(fn_name, vec![]);
    std::env::set_current_dir(prev_cwd).unwrap();

    let len = match result.expect("codegen self-test ran") {
        Value::Int(n) if n > 0 => n as usize,
        other => panic!("{fn_name} reported failure: {other:?}"),
    };
    let bytes = std::fs::read(&out_path).expect("self-hosted compiler wrote its output file");
    let _ = std::fs::remove_file(&out_path);
    assert_eq!(bytes.len(), len, "reported length must match bytes written");
    assert!(bytes.starts_with(&[0x00, 0x61, 0x73, 0x6D]), "missing wasm magic");
    wasmparser::Validator::new()
        .validate_all(&bytes)
        .unwrap_or_else(|e| panic!("self-hosted output failed validation: {e}"));
    bytes
}

#[test]
fn self_hosted_compiler_emits_a_working_add() {
    let bytes = run_codegen_self_test("test_compile_add", "aipl_src/_codegen_out.wasm");
    let engine = Engine::default();
    let module = WasmModule::new(&engine, &bytes).unwrap();
    let mut store = Store::new(&engine, ());
    let instance = Instance::new(&mut store, &module, &[]).unwrap();
    let add: TypedFunc<(i32, i32), i32> = instance.get_typed_func(&mut store, "add").unwrap();
    assert_eq!(add.call(&mut store, (2, 3)).unwrap(), 5);
    assert_eq!(add.call(&mut store, (i32::MAX, 1)).unwrap(), i32::MIN);
}

#[test]
fn self_hosted_compiler_emits_a_working_compute_with_loop_and_call() {
    // Source compiled by the self-test:
    //   (fn add [a:i32 b:i32] -> i32 (+ a b))
    //   (fn compute [x:i32] -> i32
    //     (let y:i32 (call add x 5)) (let z:i32 0)
    //     (loop i 0 9 1 (set! z (+ z i)))
    //     (+ y z))
    // so compute(x) = x + 5 + (0+1+...+9) = x + 50.
    let bytes = run_codegen_self_test("test_compile_compute", "aipl_src/_codegen_out2.wasm");
    let engine = Engine::default();
    let module = WasmModule::new(&engine, &bytes).unwrap();
    let mut store = Store::new(&engine, ());
    let instance = Instance::new(&mut store, &module, &[]).unwrap();
    let compute: TypedFunc<i32, i32> = instance.get_typed_func(&mut store, "compute").unwrap();
    assert_eq!(compute.call(&mut store, 1).unwrap(), 51);
    assert_eq!(compute.call(&mut store, 10).unwrap(), 60);
    assert_eq!(compute.call(&mut store, -50).unwrap(), 0);
    let add: TypedFunc<(i32, i32), i32> = instance.get_typed_func(&mut store, "add").unwrap();
    assert_eq!(add.call(&mut store, (20, 22)).unwrap(), 42);
}

/// The self-hosted compiler must emit the same memory-layout store guard the
/// Rust backend does (AIPL_SPEC.md 7.9): a store to bytes 0-3 or 64-1023 traps,
/// a store to the heap succeeds. `test_compile_store` compiles
/// `(fn add [a:i32 b:i32] -> i32 (mem.store32 a b) (mem.load32 a))`.
#[test]
fn self_hosted_compiler_emits_the_memory_layout_store_guard() {
    let bytes = run_codegen_self_test("test_compile_store", "aipl_src/_codegen_out3.wasm");
    let engine = Engine::default();
    let module = WasmModule::new(&engine, &bytes).unwrap();

    let call = |addr: i32, val: i32| -> Result<i32, String> {
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[]).unwrap();
        let f: TypedFunc<(i32, i32), i32> = instance.get_typed_func(&mut store, "add").unwrap();
        f.call(&mut store, (addr, val)).map_err(|e| {
            e.downcast_ref::<wasmtime::Trap>()
                .map(|t| format!("{t:?}"))
                .unwrap_or_else(|| e.to_string())
        })
    };

    // Heap and runtime-cell writes go through.
    assert_eq!(call(1024, 7).unwrap(), 7);
    assert_eq!(call(65536, -1).unwrap(), -1);
    assert_eq!(call(16, 99).unwrap(), 99, "runtime cell 16 is writable");
    // Reserved block and heap cursor trap, exactly like the Rust backend.
    for bad in [0, 3, 64, 512, 1023] {
        let err = call(bad, 1).unwrap_err();
        assert!(err.contains("nreachable"), "address {bad} should trap, got {err}");
    }
}

/// Belt and braces: the guard bytes the self-hosted compiler emits must be the
/// exact bytes the Rust backend emits for the same store, so the two backends
/// cannot drift apart silently. Compiles the same one-store function both ways
/// and compares the function body byte-for-byte.
#[test]
fn self_hosted_store_guard_bytes_match_the_rust_backend() {
    use aipl_core::compiler::wasm::WasmCompiler;
    use aipl_core::parser::Parser;
    let src = "(module t (fn add [a:i32 b:i32] -> i32 (mem.store32 a b) (mem.load32 a)))";
    let module = Parser::parse(src).unwrap();
    TypeChecker::new().check_module(&module).unwrap();
    let rust_wasm = WasmCompiler::compile(&module).unwrap();
    let self_wasm = run_codegen_self_test("test_compile_store", "aipl_src/_codegen_out3.wasm");

    // Locate the code section (id 10) in each and pull out the single function body.
    fn function_body(wasm: &[u8]) -> Vec<u8> {
        let mut body = None;
        for payload in wasmparser::Parser::new(0).parse_all(wasm) {
            if let wasmparser::Payload::CodeSectionEntry(f) = payload.unwrap() {
                let range = f.range();
                body = Some(wasm[range.start..range.end].to_vec());
            }
        }
        body.expect("one function body")
    }
    let rust_body = function_body(&rust_wasm);
    let self_body = function_body(&self_wasm);
    assert_eq!(
        self_body, rust_body,
        "self-hosted body {:02x?} differs from Rust backend body {:02x?}",
        self_body, rust_body
    );
}
