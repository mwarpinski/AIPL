use aipl_core::checker::TypeChecker;
use aipl_core::compiler::binary_ast::BinaryAstCompiler;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::parser::Parser;
use aipl_core::vm::{Value, VM};

#[test]
fn test_parser_basic() {
    let src = r#"
    (module test_math
      (fn add [a:i32 b:i32] -> i32
        (+ a b)))
    "#;
    let module = Parser::parse(src).expect("Parse failed");
    assert_eq!(module.name, "test_math");
    assert_eq!(module.functions.len(), 1);
    assert_eq!(module.functions[0].name, "add");
}

#[test]
fn test_type_checker_and_contracts() {
    let src = r#"
    (module verified_module
      (fn safe_div [num:i32 den:i32] -> i32
        (req (neq den 0))
        (ens (gte res 0))
        (/ num den)))
    "#;
    let module = Parser::parse(src).expect("Parse failed");
    let mut checker = TypeChecker::new();
    assert!(checker.check_module(&module).is_ok());
}

#[test]
fn test_vm_execution() {
    let src = r#"
    (module compute
      (fn fib [n:i32] -> i32
        (if (lte n 1)
            n
            (+ (call fib (- n 1)) (call fib (- n 2))))))
    "#;
    let module = Parser::parse(src).expect("Parse failed");
    let mut vm = VM::new();
    vm.load_module(module);
    let res = vm.invoke("fib", vec![Value::Int(7)]).expect("VM failed");
    assert_eq!(res, Value::Int(13));
}

#[test]
fn test_wasm_compiler_output() {
    let src = r#"
    (module wasm_demo
      (fn double [x:i32] -> i32
        (* x 2)))
    "#;
    let module = Parser::parse(src).expect("Parse failed");
    let wasm_bytes = WasmCompiler::compile(&module).expect("Wasm compile failed");
    assert!(wasm_bytes.starts_with(&[0x00, 0x61, 0x73, 0x6d])); // Magic Wasm header \0asm
}

#[test]
fn test_binary_ast_roundtrip() {
    let src = r#"
    (module binary_demo
      (fn square [n:i32] -> i32
        (* n n)))
    "#;
    let module = Parser::parse(src).expect("Parse failed");
    let bytes = BinaryAstCompiler::encode(&module).expect("Encode failed");
    let decoded = BinaryAstCompiler::decode(&bytes).expect("Decode failed");
    assert_eq!(module.name, decoded.name);
    assert_eq!(module.functions[0].name, decoded.functions[0].name);
}
