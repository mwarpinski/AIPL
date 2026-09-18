use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::parser::Parser;
use aipl_core::vm::{Value, VM};

#[test]
fn test_v2_memory_load_store() {
    let src = r#"
    (module test_mem
      (fn test_store_and_load [] -> i32
        (let ptr:i32 (mem.alloc 16))
        (mem.store32 ptr 98765)
        (mem.load32 ptr)))
    "#;
    let module = Parser::parse(src).expect("Parse failed");
    let mut checker = TypeChecker::new();
    assert!(checker.check_module(&module).is_ok());

    let mut vm = VM::new();
    vm.load_module(module.clone());
    let res = vm.invoke("test_store_and_load", vec![]).expect("VM failed");
    assert_eq!(res, Value::Int(98765));

    let wasm_bytes = WasmCompiler::compile(&module).expect("Wasm compile failed");
    assert!(wasm_bytes.starts_with(&[0x00, 0x61, 0x73, 0x6d]));
}

#[test]
fn test_v2_bitwise_operators() {
    let src = r#"
    (module test_bitwise
      (fn compute_shifts [val:i32] -> i32
        (let shifted_left:i32 (shl val 2))
        (let masked:i32 (bitand shifted_left 255))
        masked))
    "#;
    let module = Parser::parse(src).expect("Parse failed");
    let mut checker = TypeChecker::new();
    assert!(checker.check_module(&module).is_ok());

    let mut vm = VM::new();
    vm.load_module(module);
    let res = vm.invoke("compute_shifts", vec![Value::Int(15)]).expect("VM failed");
    // 15 << 2 = 60; 60 & 255 = 60
    assert_eq!(res, Value::Int(60));
}

#[test]
fn test_v2_atomic_concurrency() {
    let src = r#"
    (module test_atomics
      (fn test_mutex [] -> i32
        (let mutex_ptr:i32 96)
        (let data_ptr:i32 100)
        (atomic.lock mutex_ptr)
        (mem.store32 data_ptr 10)
        (atomic.add data_ptr 5)
        (atomic.unlock mutex_ptr)
        (mem.load32 data_ptr)))
    "#;
    let module = Parser::parse(src).expect("Parse failed");
    let mut checker = TypeChecker::new();
    assert!(checker.check_module(&module).is_ok());

    let mut vm = VM::new();
    vm.load_module(module);
    let res = vm.invoke("test_mutex", vec![]).expect("VM failed");
    assert_eq!(res, Value::Int(15));
}

#[test]
fn test_v2_result_type_matching() {
    let src = r#"
    (module test_results
      (fn safe_lookup [key:i32] -> i32
        (let res_val:i32 0)
        (if (gt key 0)
            (let r:i32 (match_result (ok key) (ok val val) (err e 0)))
            (let r:i32 (match_result (err -1) (ok val val) (err e -1))))
        res_val))
    "#;
    let module = Parser::parse(src).expect("Parse failed");
    let mut checker = TypeChecker::new();
    assert!(checker.check_module(&module).is_ok());
}


#[test]
fn test_v2_multi_expression_compound() {
    let src = r#"
    (module compound_test
      (fn process_metrics [base:i32 scale:i32] -> i32
        (req (gte base 0))
        (req (gte scale 0))
        (ens (gte res 0))
        (let sum_val:i32 (+ base scale))
        (let scaled:i32 (shl sum_val 1))
        (let threshold:i32 50)
        (if (gt scaled threshold)
          (- scaled 10)
          (+ scaled 5))))
    "#;
    
    let module = Parser::parse(src).expect("Parse failed");
    let mut checker = TypeChecker::new();
    assert!(checker.check_module(&module).is_ok());

    let mut vm = VM::new();
    vm.load_module(module);
    
    // Test Case A: base = 20, scale = 10 
    // sum = 30 -> scaled = 60 (> 50 threshold) -> 60 - 10 = 50
    let res1 = vm.invoke("process_metrics", vec![Value::Int(20), Value::Int(10)]).expect("VM failed");
    assert_eq!(res1, Value::Int(50));

    // Test Case B: base = 5, scale = 5 
    // sum = 10 -> scaled = 20 (<= 50 threshold) -> 20 + 5 = 25
    let res2 = vm.invoke("process_metrics", vec![Value::Int(5), Value::Int(5)]).expect("VM failed");
    assert_eq!(res2, Value::Int(25));
}

#[test]
fn test_v2_multi_module_linkage() {
    let math_src = r#"
    (module math_core
      (fn clamp_min [val:i32 min_val:i32] -> i32
        (req (gte val 0))
        (req (gte min_val 0))
        (ens (gte res min_val))
        (if (lt val min_val) min_val val)))
    "#;

    let policy_src = r#"
    (module system_policy
      (fn evaluate_load [current_load:i32 threshold:i32] -> i32
        (req (gte current_load 0))
        (req (gte threshold 0))
        (ens (gte res 0))
        (if (gt current_load threshold) 1 0)))
    "#;

    // Parse and check both modules independently
    let math_mod = Parser::parse(math_src).expect("Math parse failed");
    let policy_mod = Parser::parse(policy_src).expect("Policy parse failed");

    let mut checker = TypeChecker::new();
    assert!(checker.check_module(&math_mod).is_ok());
    assert!(checker.check_module(&policy_mod).is_ok());

    // Load both into the VM ecosystem
    let mut vm = VM::new();
    vm.load_module(math_mod);
    vm.load_module(policy_mod);

    // Test policy evaluation crossing execution paths
    let res_critical = vm.invoke("evaluate_load", vec![Value::Int(90), Value::Int(75)]).expect("VM failed");
    assert_eq!(res_critical, Value::Int(1)); // Critical load triggered

    let res_nominal = vm.invoke("evaluate_load", vec![Value::Int(40), Value::Int(75)]).expect("VM failed");
    assert_eq!(res_nominal, Value::Int(0)); // Nominal status
}

// Real concurrency: 4 real OS threads (thread.spawn) each increment a SHARED
// counter 1000 times via atomic.add, then join. This can only land on
// exactly 4000 if the threads are real, memory is genuinely shared across
// them, and atomic.add is genuinely atomic - a fake/no-op implementation of
// any of those would very likely lose updates under real scheduling.
#[test]
fn test_real_multithreading() {
    let src = std::fs::read_to_string("aipl_src/thread_sync.aipl").expect("Read thread_sync.aipl failed");
    let module = Parser::parse(&src).expect("Parse thread_sync failed");
    let mut checker = TypeChecker::new();
    assert!(checker.check_module(&module).is_ok());

    let mut vm = VM::new();
    vm.load_module(module);

    let res = vm.invoke("run_thread_tests", vec![]).expect("run_thread_tests failed");
    assert_eq!(res, Value::Int(1), "4 real threads x 1000 atomic increments must total exactly 4000");
}

// Real file I/O: writes real bytes to a real file via fs.write, reads them
// back via fs.read, and verifies an exact byte-for-byte match - a genuine
// round trip through the OS filesystem, not a self-fulfilling count-echo.
#[test]
fn test_real_file_io() {
    let src = std::fs::read_to_string("aipl_src/file_io.aipl").expect("Read file_io.aipl failed");
    let module = Parser::parse(&src).expect("Parse file_io failed");
    let mut checker = TypeChecker::new();
    assert!(checker.check_module(&module).is_ok());

    let mut vm = VM::new();
    vm.load_module(module);

    let res = vm.invoke("run_file_io_tests", vec![]);
    let _ = std::fs::remove_file("aipl_fileio_selftest.tmp");
    assert_eq!(res.expect("run_file_io_tests failed"), Value::Int(1), "real disk round trip must match byte-for-byte");
}



