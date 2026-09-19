//! The memory layout (AIPL_SPEC.md, section 7.9) as enforced at runtime, in
//! both backends. The checker catches literal addresses (tests/test_diagnostics.rs);
//! these tests cover what only shows up at runtime: computed addresses, the
//! shared heap cursor's initial state, and the lock-word validity check that
//! turns the old "spin forever on address 0" into an error.

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::parser::Parser;
use aipl_core::vm::{Value, VM};
use wasmtime::{Engine, Instance, Module as WasmModule, Store, TypedFunc};

fn vm_run(src: &str, f: &str) -> Result<Value, String> {
    let module = Parser::parse(src).expect("parse");
    TypeChecker::new().check_module(&module).expect("check");
    let mut vm = VM::new();
    vm.load_module(module);
    vm.invoke(f, vec![])
}

#[test]
fn fresh_vm_has_cursor_1024_at_address_0_and_alloc_advances_it() {
    let vm = VM::new();
    assert_eq!(vm.read_bytes(0, 4), 1024u32.to_le_bytes().to_vec());
    let v = vm_run(
        "(module m (fn f [] -> i32 (let a:i32 (mem.alloc 16)) (let b:i32 (mem.alloc 4)) (+ (* a 100000) (+ (* b 10) (- (mem.load32 0) b)))))",
        "f",
    )
    .unwrap();
    // a = 1024, b = 1040, cursor after = 1044 -> 1024*100000 + 1040*10 + 4
    assert_eq!(v, Value::Int(102_400_000 + 10_400 + 4));
}

#[test]
fn fresh_wasm_instance_has_the_same_cursor_via_data_segment() {
    let src = "(module m (fn f [] -> i32 (mem.alloc 16)))";
    let module = Parser::parse(src).unwrap();
    TypeChecker::new().check_module(&module).unwrap();
    let wasm = WasmCompiler::compile(&module).unwrap();

    let engine = Engine::default();
    let wm = WasmModule::new(&engine, &wasm).unwrap();
    let mut store = Store::new(&engine, ());
    let instance = Instance::new(&mut store, &wm, &[]).unwrap();
    let memory = instance.get_memory(&mut store, "memory").unwrap();
    assert_eq!(memory.size(&store), 16, "16 pages, same as the VM");
    let mut word = [0u8; 4];
    memory.read(&store, 0, &mut word).unwrap();
    assert_eq!(u32::from_le_bytes(word), 1024, "data segment seeds the cursor");

    let f: TypedFunc<(), i32> = instance.get_typed_func(&mut store, "f").unwrap();
    assert_eq!(f.call(&mut store, ()).unwrap(), 1024);
    memory.read(&store, 0, &mut word).unwrap();
    assert_eq!(u32::from_le_bytes(word), 1040, "mem.alloc bumped the word at 0");
}

#[test]
fn locking_a_word_that_is_not_a_lock_errors_instead_of_hanging() {
    // Computed address, so the checker cannot see it. Before the fix this spun
    // forever; now the VM notices the word holds 1024 (not 0 or 1) and fails.
    let err = vm_run(
        "(module m (fn f [] -> void (let p:i32 (mem.alloc 4)) (mem.store32 p 1024) (atomic.lock p)))",
        "f",
    )
    .unwrap_err();
    assert!(err.contains("not a lock state"), "got {err}");
    assert!(err.contains("holds 1024"), "got {err}");
}

#[test]
fn locking_the_heap_cursor_through_a_computed_address_errors() {
    // (- p p) is 0 at runtime but not a literal, so only the VM can catch it.
    // The write-address check fires first (address 0 is the heap cursor); the
    // lock-state check is the backstop for non-reserved data words (tested above).
    let err = vm_run(
        "(module m (fn f [] -> void (let p:i32 (mem.alloc 4)) (atomic.lock (- p p))))",
        "f",
    )
    .unwrap_err();
    assert!(err.contains("heap cursor"), "got {err}");
}

#[test]
fn unlocking_a_free_or_non_lock_word_errors() {
    let err = vm_run(
        "(module m (fn f [] -> void (let p:i32 (mem.alloc 4)) (atomic.unlock p)))",
        "f",
    )
    .unwrap_err();
    assert!(err.contains("never locked"), "got {err}");
}

#[test]
fn a_real_lock_round_trip_still_works() {
    let v = vm_run(
        "(module m (fn f [] -> i32 (let m:i32 (mem.alloc 4)) (let d:i32 (mem.alloc 4)) (atomic.lock m) (mem.store32 d 41) (atomic.add d 1) (atomic.unlock m) (atomic.lock m) (atomic.unlock m) (mem.load32 d)))",
        "f",
    )
    .unwrap();
    assert_eq!(v, Value::Int(42));
}

#[test]
fn mem_grow_agrees_on_old_size_and_refuses_past_the_cap() {
    let v = vm_run("(module m (fn f [] -> i32 (mem.grow 1)))", "f").unwrap();
    assert_eq!(v, Value::Int(16));
    let v = vm_run("(module m (fn f [] -> i32 (mem.grow 85)))", "f").unwrap();
    assert_eq!(v, Value::Int(-1), "16 + 85 > 100 pages");
    let v = vm_run("(module m (fn f [] -> i32 (let a:i32 (mem.grow 84)) (mem.grow 1)))", "f").unwrap();
    assert_eq!(v, Value::Int(-1), "exactly at the cap, one more page is refused");
}

// ---------------------------------------------------------------------------
// Runtime write check for COMPUTED addresses, in both backends. The checker
// only sees literals; these addresses are built with arithmetic so neither
// backend can know them statically. The VM must error and wasm must trap on
// exactly the same writes (wasm semantics are the spec).
// ---------------------------------------------------------------------------

fn both(src: &str) -> (Result<Value, String>, Result<(), String>) {
    let module = Parser::parse(src).expect("parse");
    TypeChecker::new().check_module(&module).expect("check");
    let wasm = WasmCompiler::compile(&module).expect("compile");
    wasmparser::Validator::new().validate_all(&wasm).expect("validate");

    let mut vm = VM::new();
    vm.load_module(module);
    let vm_res = vm.invoke("f", vec![]);

    let engine = Engine::default();
    let wm = WasmModule::new(&engine, &wasm).unwrap();
    let mut store = Store::new(&engine, ());
    let instance = Instance::new(&mut store, &wm, &[]).unwrap();
    let f: TypedFunc<(), i32> = instance.get_typed_func(&mut store, "f").unwrap();
    // wasmtime puts the trap code in the error chain, not the top-level message.
    let wt_res = f.call(&mut store, ()).map(|_| ()).map_err(|e| {
        e.downcast_ref::<wasmtime::Trap>()
            .map(|t| format!("{t:?}"))
            .unwrap_or_else(|| e.to_string())
    });
    (vm_res, wt_res)
}

#[test]
fn computed_store_into_reserved_block_fails_in_both_backends() {
    // (* 8 64) == 512 at runtime; not a literal, so only the runtime check sees it.
    let (vm, wt) = both("(module m (fn f [] -> i32 (let a:i32 (* 8 64)) (mem.store32 a 7) (mem.load32 a)))");
    let e = vm.unwrap_err();
    assert!(e.contains("reserved runtime block") && e.contains("address 512"), "got {e}");
    let t = wt.unwrap_err();
    assert!(t.contains("nreachable"), "wasm should trap with unreachable, got {t}");
}

#[test]
fn computed_store_to_heap_cursor_fails_in_both_backends() {
    let (vm, wt) = both("(module m (fn f [] -> i32 (let p:i32 (mem.alloc 4)) (mem.store8 (- p p) 1) 0))");
    let e = vm.unwrap_err();
    assert!(e.contains("heap cursor") && e.contains("address 0"), "got {e}");
    assert!(wt.unwrap_err().contains("nreachable"));
}

#[test]
fn computed_store64_at_boundary_1023_fails_and_1024_succeeds() {
    let (vm, wt) = both("(module m (fn f [] -> i32 (let a:i32 (- (mem.alloc 0) 1)) (mem.store64 a 1i64) 0))");
    assert!(vm.is_err() && wt.is_err(), "1023 is reserved");
    let (vm, wt) = both("(module m (fn f [] -> i32 (let a:i32 (mem.alloc 8)) (mem.store64 a 1i64) (mem.load32 a)))");
    assert_eq!(vm.unwrap(), Value::Int(1));
    assert!(wt.is_ok(), "1024 is heap");
}

#[test]
fn computed_writes_to_runtime_cells_and_heap_are_allowed_in_both_backends() {
    // cell 16 via arithmetic, and a heap word from mem.alloc
    let (vm, wt) = both("(module m (fn f [] -> i32 (let c:i32 (* 4 4)) (mem.store32 c 99) (let p:i32 (mem.alloc 4)) (mem.store32 p (mem.load32 c)) (mem.load32 p)))");
    assert_eq!(vm.unwrap(), Value::Int(99));
    assert!(wt.is_ok());
}

#[test]
fn computed_reads_from_the_reserved_block_are_not_checked() {
    // Reads are harmless (the block is zero) and are deliberately unchecked.
    let (vm, wt) = both("(module m (fn f [] -> i32 (let a:i32 (* 8 64)) (mem.load32 a)))");
    assert_eq!(vm.unwrap(), Value::Int(0));
    assert!(wt.is_ok());
}

#[test]
fn computed_atomic_ops_on_reserved_block_fail_in_vm() {
    // Atomics are VM-only, so there is no wasm side to compare here.
    for op in ["(atomic.add a 1)", "(atomic.cas a 0 1)", "(atomic.lock a)", "(atomic.unlock a)"] {
        let src = format!("(module m (fn f [] -> i32 (let a:i32 (* 8 64)) {op} 0))");
        let e = vm_run(&src, "f").unwrap_err();
        assert!(e.contains("reserved runtime block"), "{op}: got {e}");
    }
}
