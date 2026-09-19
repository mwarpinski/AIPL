//! `i64` as a first-class type: literals (`42i64`), 64-bit wrapping
//! arithmetic in the VM, explicit i32<->i64 conversions, 64-bit memory ops,
//! and type-directed instruction selection in the wasm backend (validated
//! with wasmparser). Also covers the f64 codegen fix that landed with it:
//! before type-directed selection, `(+ 1.0 2.0)` compiled to `i32.add` and
//! failed validation.

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::parser::Parser;
use aipl_core::vm::{Value, VM};
use wasmparser::Validator;

/// Parses, type-checks, runs `fn_name` in the VM, and compiles + validates
/// the wasm. Returns the VM result so callers can assert on it.
fn run_both(src: &str, fn_name: &str) -> Value {
    let module = Parser::parse(src).expect("parse");
    TypeChecker::new().check_module(&module).expect("check");

    let wasm = WasmCompiler::compile(&module).expect("wasm compile");
    Validator::new()
        .validate_all(&wasm)
        .unwrap_or_else(|e| panic!("wasm failed validation: {e}"));

    let mut vm = VM::new();
    vm.load_module(module);
    vm.invoke(fn_name, vec![]).expect("VM run")
}

#[test]
fn i64_literal_and_declared_type() {
    let v = run_both(
        "(module m (fn f [] -> i64 (let x:i64 4294967296i64) x))",
        "f",
    );
    assert_eq!(v, Value::Int64(4_294_967_296));
}

#[test]
fn i64_arithmetic_wraps_at_64_not_32() {
    // 2^32 * 2^31 = 2^63 wraps to i64::MIN; in i32 the operands don't even exist.
    let v = run_both(
        "(module m (fn f [] -> i64 (* 4294967296i64 2147483648i64)))",
        "f",
    );
    assert_eq!(v, Value::Int64(i64::MIN));

    let v = run_both("(module m (fn f [] -> i64 (+ 9223372036854775807i64 1i64)))", "f");
    assert_eq!(v, Value::Int64(i64::MIN));

    // Values that would wrap in i32 stay exact in i64.
    let v = run_both("(module m (fn f [] -> i64 (+ 2147483647i64 1i64)))", "f");
    assert_eq!(v, Value::Int64(2_147_483_648));
}

#[test]
fn i64_division_shift_and_unsigned_ops() {
    assert_eq!(run_both("(module m (fn f [] -> i64 (/ -7i64 2i64)))", "f"), Value::Int64(-3));
    assert_eq!(run_both("(module m (fn f [] -> i64 (% -7i64 2i64)))", "f"), Value::Int64(-1));
    assert_eq!(
        run_both("(module m (fn f [] -> i64 (divu -1i64 2i64)))", "f"),
        Value::Int64(i64::MAX)
    );
    assert_eq!(run_both("(module m (fn f [] -> i64 (remu -1i64 2i64)))", "f"), Value::Int64(1));
    // shift count masked to 6 bits: 65 & 63 == 1
    assert_eq!(run_both("(module m (fn f [] -> i64 (shl 1i64 65i64)))", "f"), Value::Int64(2));
    assert_eq!(run_both("(module m (fn f [] -> i64 (shr -8i64 1i64)))", "f"), Value::Int64(-4));
    assert_eq!(
        run_both("(module m (fn f [] -> i64 (shru -8i64 1i64)))", "f"),
        Value::Int64(0x7FFF_FFFF_FFFF_FFFC)
    );
}

#[test]
fn i64_division_by_zero_is_an_error_not_a_default() {
    let module = Parser::parse("(module m (fn f [] -> i64 (/ 1i64 0i64)))").unwrap();
    TypeChecker::new().check_module(&module).unwrap();
    let mut vm = VM::new();
    vm.load_module(module);
    let err = vm.invoke("f", vec![]).unwrap_err();
    assert!(err.contains("Division by zero"), "got {err}");
}

#[test]
fn i64_comparisons_and_if_with_i64_branches() {
    // The `if` here has i64-typed branches, which needs BlockType::Result(I64)
    // in wasm - the old backend hard-coded I32 and would fail validation.
    let src = r#"
    (module m
      (fn f [] -> i64
        (let a:i64 4294967296i64)
        (let b:i64 1i64)
        (if (gt a b) a b)))
    "#;
    assert_eq!(run_both(src, "f"), Value::Int64(4_294_967_296));

    let v = run_both("(module m (fn f [] -> bool (lt -1i64 0i64)))", "f");
    assert_eq!(v, Value::Bool(true));
    let v = run_both("(module m (fn f [] -> bool (eq 5i64 5i64)))", "f");
    assert_eq!(v, Value::Bool(true));
}

#[test]
fn i64_conversions() {
    assert_eq!(
        run_both("(module m (fn f [] -> i64 (i64.extend_s -1)))", "f"),
        Value::Int64(-1)
    );
    assert_eq!(
        run_both("(module m (fn f [] -> i64 (i64.extend_u -1)))", "f"),
        Value::Int64(4_294_967_295)
    );
    // 2^32 + 5 wraps to 5.
    assert_eq!(
        run_both("(module m (fn f [] -> i32 (i32.wrap 4294967301i64)))", "f"),
        Value::Int(5)
    );
    // Round trip through a widening then narrowing keeps i32 wrapping intact.
    assert_eq!(
        run_both("(module m (fn f [] -> i32 (i32.wrap (+ (i64.extend_s 2147483647) 1i64))))", "f"),
        Value::Int(-2_147_483_648)
    );
}

#[test]
fn i64_memory_round_trip() {
    let src = r#"
    (module m
      (fn f [] -> i64
        (let p:i32 (mem.alloc 8))
        (mem.store64 p 1311768467294899696i64)   ;; 0x1234567890ABCDF0
        (mem.load64 p)))
    "#;
    assert_eq!(run_both(src, "f"), Value::Int64(0x1234_5678_90AB_CDF0));

    // Low 32 bits of a little-endian i64 store are readable with load32.
    let src = r#"
    (module m
      (fn f [] -> i32
        (let p:i32 (mem.alloc 8))
        (mem.store64 p 4294967301i64)
        (mem.load32 p)))
    "#;
    assert_eq!(run_both(src, "f"), Value::Int(5));
}

#[test]
fn i64_params_and_calls() {
    let src = r#"
    (module m
      (fn add64 [a:i64 b:i64] -> i64 (+ a b))
      (fn f [] -> i64 (call add64 4294967296i64 4294967296i64)))
    "#;
    assert_eq!(run_both(src, "f"), Value::Int64(8_589_934_592));
}

#[test]
fn i64_loop_accumulator() {
    // loop bounds stay i32; the accumulator is i64 and exceeds i32 range.
    let src = r#"
    (module m
      (fn f [] -> i64
        (let acc:i64 0i64)
        (loop i 1 100 1
          (set! acc (+ acc 100000000i64)))
        acc))
    "#;
    assert_eq!(run_both(src, "f"), Value::Int64(10_000_000_000));
}

#[test]
fn checker_rejects_mixing_i32_and_i64() {
    let module = Parser::parse("(module m (fn f [] -> i64 (+ 1 2i64)))").unwrap();
    let err = TypeChecker::new().check_module(&module).unwrap_err();
    assert!(err.contains("Type mismatch in binary op: I32 vs I64"), "got {err}");

    let module = Parser::parse("(module m (fn f [] -> i64 (let x:i64 5) x))").unwrap();
    let err = TypeChecker::new().check_module(&module).unwrap_err();
    assert!(err.contains("Type mismatch in 'let': expected I64, got I32"), "got {err}");

    let module = Parser::parse("(module m (fn f [] -> i32 (i32.wrap 5)))").unwrap();
    let err = TypeChecker::new().check_module(&module).unwrap_err();
    assert!(err.contains("i32.wrap requires i64, got I32"), "got {err}");
}

#[test]
fn i64_suffix_must_be_attached_to_digits() {
    // A bare `i64` symbol in expression position is still just an identifier.
    let module = Parser::parse("(module m (fn f [] -> i64 i64))").unwrap();
    let err = TypeChecker::new().check_module(&module).unwrap_err();
    assert!(err.contains("Undefined variable 'i64'"), "got {err}");
}

#[test]
fn f64_arithmetic_now_validates_in_wasm() {
    // Regression: type-directed selection means f64 operands get f64.add etc.
    let v = run_both("(module m (fn f [] -> f64 (+ 1.5 2.25)))", "f");
    assert_eq!(v, Value::Float(3.75));
    let v = run_both("(module m (fn f [] -> bool (lt 1.5 2.25)))", "f");
    assert_eq!(v, Value::Bool(true));
    let v = run_both("(module m (fn f [] -> f64 (if (lt 1.0 2.0) 10.0 20.0)))", "f");
    assert_eq!(v, Value::Float(10.0));
}
