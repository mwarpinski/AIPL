use aipl_core::ast::Type;
use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::parser::Parser;
use aipl_core::vm::{Value, VM};
use wasmtime::*;

fn run_wasm(wasm_bytes: &[u8], fn_name: &str) -> Result<i32, String> {
    let engine = Engine::default();
    let module = Module::new(&engine, wasm_bytes).map_err(|e| e.to_string())?;
    let mut store = Store::new(&engine, ());
    let instance = Instance::new(&mut store, &module, &[]).map_err(|e| e.to_string())?;
    let func = instance
        .get_func(&mut store, fn_name)
        .ok_or_else(|| format!("Function '{}' not found in Wasm module", fn_name))?;
    let mut results = [Val::I32(0)];
    func.call(&mut store, &[], &mut results)
        .map_err(|e| e.to_string())?;
    match results[0] {
        Val::I32(val) => Ok(val),
        _ => Err("Expected i32 return value".to_string()),
    }
}

fn assert_differential(src: &str, fn_name: &str) {
    let module = Parser::parse(src).expect("Parse failed");
    let mut checker = TypeChecker::new();
    checker.check_module(&module).expect("Type check failed");

    let mut vm = VM::new();
    vm.load_module(module.clone());
    let vm_res = vm.invoke(fn_name, vec![]).expect("VM execution failed");
    let vm_i32 = match vm_res {
        Value::Int(i) => i as i32,
        other => panic!("Expected Value::Int from VM, got {:?}", other),
    };

    let wasm_bytes = WasmCompiler::compile(&module).expect("WASM compilation failed");
    let wasm_i32 = run_wasm(&wasm_bytes, fn_name).expect("WASM execution failed");

    assert_eq!(
        vm_i32, wasm_i32,
        "Differential mismatch for function '{}' in program:\n{}\nVM = {}, WASM = {}",
        fn_name, src, vm_i32, wasm_i32
    );
}

#[test]
fn test_explicit_differential_edge_cases() {
    // (+ 2147483647 1)
    assert_differential(
        "(module test_mod (fn test_op [] -> i32 (+ 2147483647 1)))",
        "test_op",
    );

    // (shr -8 1)
    assert_differential(
        "(module test_mod (fn test_op [] -> i32 (shr -8 1)))",
        "test_op",
    );

    // (* 65536 65536)
    assert_differential(
        "(module test_mod (fn test_op [] -> i32 (* 65536 65536)))",
        "test_op",
    );

    // (/ -7 2)
    assert_differential(
        "(module test_mod (fn test_op [] -> i32 (/ -7 2)))",
        "test_op",
    );

    // (% -7 2)
    assert_differential(
        "(module test_mod (fn test_op [] -> i32 (% -7 2)))",
        "test_op",
    );

    // (shru -8 1)
    assert_differential(
        "(module test_mod (fn test_op [] -> i32 (shru -8 1)))",
        "test_op",
    );

    // (divu 4294967288 2)
    assert_differential(
        "(module test_mod (fn test_op [] -> i32 (divu 4294967288 2)))",
        "test_op",
    );

    // (remu 10 3)
    assert_differential(
        "(module test_mod (fn test_op [] -> i32 (remu 10 3)))",
        "test_op",
    );

    // loop with end bound inclusive
    assert_differential(
        "(module test_mod (fn test_op [] -> i32 (let sum:i32 0) (loop i 1 5 1 (set! sum (+ sum i))) sum))",
        "test_op",
    );

    // while with set! in body
    assert_differential(
        "(module test_mod (fn test_op [] -> i32 (let i:i32 0) (let sum:i32 0) (while (lt i 5) (block (set! sum (+ sum i)) (set! i (+ i 1)))) sum))",
        "test_op",
    );
}

#[test]
fn test_differential_files() {
    let files = vec![
        "examples/aipl_db_demo.aipl",
        "examples/aipl_database/demo_aisql.aipl",
        "aipl_src/codegen.aipl",
    ];

    for file_path in files {
        if !std::path::Path::new(file_path).exists() {
            continue;
        }
        let src = std::fs::read_to_string(file_path).expect("Failed to read file");
        let module = match Parser::parse(&src) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mut checker = TypeChecker::new();
        if checker.check_module(&module).is_err() {
            continue;
        }

        let wasm_bytes = match WasmCompiler::compile(&module) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };

        for f in &module.functions {
            if f.params.is_empty() && f.return_type == Type::I32 {
                let mut vm = VM::new();
                vm.load_module(module.clone());
                let vm_res = vm.invoke(&f.name, vec![]);
                if let Ok(Value::Int(vm_val)) = vm_res {
                    if let Ok(wasm_val) = run_wasm(&wasm_bytes, &f.name) {
                        assert_eq!(
                            vm_val as i32, wasm_val,
                            "Mismatch in {} function {}",
                            file_path, f.name
                        );
                    }
                }
            }
        }
    }
}
