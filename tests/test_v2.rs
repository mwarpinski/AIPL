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
        (let ptr:i32 100)
        (atomic.lock ptr)
        (mem.store32 ptr 10)
        (atomic.add ptr 5)
        (atomic.unlock ptr)
        (mem.load32 ptr)))
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
fn test_self_hosted_wasm_emitter() {
    let src = std::fs::read_to_string("aipl_src/compiler.aipl").expect("Read compiler.aipl failed");
    let module = Parser::parse(&src).expect("Parse self-hosted compiler failed");
    let mut checker = TypeChecker::new();
    assert!(checker.check_module(&module).is_ok());

    let mut vm = VM::new();
    vm.load_module(module);

    // Write source AIPL string into VM linear memory at offset 100
    let input_src = "(module sample (fn add [a:i32 b:i32] -> i32 (+ a b)))";
    let src_ptr = 100usize;
    let wasm_out_ptr = 1000usize;
    for (i, byte) in input_src.bytes().enumerate() {
        vm.linear_memory[src_ptr + i] = byte;
    }

    let res = vm.invoke(
        "compile_aipl",
        vec![
            Value::Int(src_ptr as i64),
            Value::Int(input_src.len() as i64),
            Value::Int(wasm_out_ptr as i64),
        ],
    ).expect("VM invocation of self-hosted compile_aipl failed");

    if let Value::Int(written_bytes) = res {
        assert!(written_bytes > 20, "Wasm compiler should emit at least 20 bytes");
        // Verify Wasm header \0asm magic bytes in linear memory
        let header = &vm.linear_memory[wasm_out_ptr..wasm_out_ptr + 8];
        assert_eq!(header, &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00], "Generated Wasm header must match standard Wasm magic");
    } else {
        panic!("Expected Int return value for written Wasm bytes");
    }
}

#[test]
fn test_sovereign_aipl_diagnostics() {
    let src = std::fs::read_to_string("aipl_src/diagnostics.aipl").expect("Read diagnostics.aipl failed");
    let module = Parser::parse(&src).expect("Parse diagnostics failed");
    let mut checker = TypeChecker::new();
    assert!(checker.check_module(&module).is_ok());

    let mut vm = VM::new();
    vm.load_module(module);

    let out_ptr = 500usize;
    let res = vm.invoke(
        "format_diagnostic",
        vec![
            Value::Int(1001), // ERR_CONTRACT_VIOLATION
            Value::Int(42),   // fn_id
            Value::Int(7),    // node_idx
            Value::Int(1024), // mem_offset
            Value::Int(out_ptr as i64),
        ],
    ).expect("format_diagnostic failed");

    assert_eq!(res, Value::Int(20));

    // Verify written diagnostic fields in linear memory
    let err_code = i32::from_le_bytes(vm.linear_memory[out_ptr..out_ptr + 4].try_into().unwrap());
    let category = i32::from_le_bytes(vm.linear_memory[out_ptr + 4..out_ptr + 8].try_into().unwrap());
    let fn_id = i32::from_le_bytes(vm.linear_memory[out_ptr + 8..out_ptr + 12].try_into().unwrap());

    assert_eq!(err_code, 1001);
    assert_eq!(category, 1001);
    assert_eq!(fn_id, 42);
}

#[test]
fn test_sovereign_aipl_test_runner() {
    let src = std::fs::read_to_string("aipl_src/aipl_test.aipl").expect("Read aipl_test.aipl failed");
    let module = Parser::parse(&src).expect("Parse aipl_test failed");
    let mut checker = TypeChecker::new();
    assert!(checker.check_module(&module).is_ok());

    let mut vm = VM::new();
    vm.load_module(module);

    let res = vm.invoke("main", vec![]).expect("main failed");
    assert_eq!(res, Value::Int(5));
}

#[test]
fn test_dual_target_native_elf_emitter() {
    let src = std::fs::read_to_string("aipl_src/elf_emitter.aipl").expect("Read elf_emitter.aipl failed");
    let module = Parser::parse(&src).expect("Parse elf_emitter failed");
    let mut checker = TypeChecker::new();
    assert!(checker.check_module(&module).is_ok());

    let mut vm = VM::new();
    vm.load_module(module);

    let elf_out_ptr = 1000usize;
    let res = vm.invoke(
        "emit_elf64_binary",
        vec![
            Value::Int(106), // i32.add
            Value::Int(25),  // arg1
            Value::Int(17),  // arg2
            Value::Int(elf_out_ptr as i64),
        ],
    ).expect("emit_elf64_binary failed");

    if let Value::Int(written) = res {
        assert!(written >= 120, "ELF64 binary should emit at least 120 bytes");
        // Verify 64-bit Linux ELF magic header bytes: \x7fELF (0x7f 0x45 0x4c 0x46)
        let header = &vm.linear_memory[elf_out_ptr..elf_out_ptr + 4];
        assert_eq!(header, &[0x7f, 0x45, 0x4c, 0x46], "Generated binary must match ELF64 magic header");
    } else {
        panic!("Expected Int return value for ELF binary emission");
    }
}

#[test]
fn test_heavy_optimizer_constant_folding() {
    let src = std::fs::read_to_string("aipl_src/optimizer.aipl").expect("Read optimizer.aipl failed");
    let module = Parser::parse(&src).expect("Parse optimizer failed");
    let mut checker = TypeChecker::new();
    assert!(checker.check_module(&module).is_ok());

    let mut vm = VM::new();
    vm.load_module(module);

    // Constant fold 15 + 27 -> 42
    let res = vm.invoke(
        "fold_constant_op",
        vec![
            Value::Int(106), // add
            Value::Int(15),
            Value::Int(27),
            Value::Bool(true),
            Value::Bool(true),
        ],
    ).expect("fold_constant_op failed");

    assert_eq!(res, Value::Int(42));
}

#[test]
fn test_sovereign_wasm_roundtrip_execution() {
    let src = std::fs::read_to_string("aipl_src/sovereign_toolchain.aipl").expect("Read sovereign_toolchain.aipl failed");
    let module = Parser::parse(&src).expect("Parse sovereign_toolchain failed");
    let mut checker = TypeChecker::new();
    assert!(checker.check_module(&module).is_ok());

    let mut vm = VM::new();
    vm.load_module(module);

    let input_src = "(module sample (fn add [a:i32 b:i32] -> i32 (+ a b)))";
    let src_ptr = 100usize;
    let wasm_out_ptr = 5000usize;

    for (i, byte) in input_src.bytes().enumerate() {
        vm.linear_memory[src_ptr + i] = byte;
    }

    let res = vm.invoke(
        "compile_to_target",
        vec![
            Value::Int(src_ptr as i64),
            Value::Int(input_src.len() as i64),
            Value::Int(wasm_out_ptr as i64),
            Value::Int(0), // Target 0 = Wasm
        ],
    ).expect("compile_to_target failed");

    if let Value::Int(written_bytes) = res {
        assert!(written_bytes > 20);
        let wasm_bytes = &vm.linear_memory[wasm_out_ptr..wasm_out_ptr + written_bytes as usize];
        assert_eq!(&wasm_bytes[0..8], &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00], "Emitted Wasm header must match spec");
    } else {
        panic!("Expected Int return value for compiled Wasm bytes count");
    }
}

#[test]
fn test_e2e_sovereign_pipeline_bootstrap() {
    let src = std::fs::read_to_string("aipl_src/pipeline.aipl").expect("Read pipeline.aipl failed");
    let module = Parser::parse(&src).expect("Parse pipeline failed");
    let mut checker = TypeChecker::new();
    assert!(checker.check_module(&module).is_ok());

    let mut vm = VM::new();
    vm.load_module(module);

    let res = vm.invoke("run_pipeline_bootstrap_test", vec![]).expect("run_pipeline_bootstrap_test failed");
    assert_eq!(res, Value::Int(1), "End-to-end self-compiling pipeline bootstrap proof must return 1");
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



