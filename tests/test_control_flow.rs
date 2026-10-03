//! P11: `return`, `break`, `continue`, and `cond`. Every program runs in the
//! VM and in wasmtime and both must agree; the checker rejects misplaced forms.

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::parser::Parser;
use aipl_core::vm::{Value, VM};
use wasmtime::{Engine, Instance, Module as WasmModule, Store, TypedFunc};

/// Runs `f(arg)` in both backends and returns the agreed result.
fn run_both(src: &str, f: &str, arg: i32) -> i32 {
    let module = Parser::parse(src).unwrap_or_else(|e| panic!("parse: {e}"));
    TypeChecker::new().check_module(&module).unwrap_or_else(|e| panic!("check: {e}"));
    let mut vm = VM::new();
    vm.load_module(module.clone());
    let vm_val = match vm.invoke(f, vec![Value::Int(arg as i64)]).unwrap() {
        Value::Int(i) => i as i32,
        Value::Bool(b) => b as i32,
        other => panic!("unexpected {other:?}"),
    };
    let wasm = WasmCompiler::compile(&module).unwrap();
    wasmparser::Validator::new().validate_all(&wasm).expect("valid wasm");
    let engine = Engine::default();
    let m = WasmModule::new(&engine, &wasm).unwrap();
    let mut store = Store::new(&engine, ());
    let inst = Instance::new(&mut store, &m, &[]).unwrap();
    let func: TypedFunc<i32, i32> = inst.get_typed_func(&mut store, f).unwrap();
    let wasm_val = func.call(&mut store, arg).unwrap();
    assert_eq!(vm_val, wasm_val, "VM and wasm disagree on {f}({arg})");
    vm_val
}

fn check_err(src: &str) -> String {
    let m = Parser::parse(src).unwrap_or_else(|e| panic!("parse: {e}"));
    TypeChecker::new().check_module(&m).expect_err("expected a checker error")
}

pub const PROGRAM: &str = r#"
(module flow
  ;; early return inside a loop: index of the first multiple of 7 at or above n
  (fn first_mult7 [n:i32] -> i32
    (loop i n (+ n 100) 1
      (if (eq (% i 7) 0) (return i) (block)))
    -1)

  ;; break inside a nested if: sum 1.. until the total passes n
  (fn sum_until [n:i32] -> i32
    (let total:i32 0)
    (let i:i32 0)
    (while true
      (set! i (+ i 1))
      (if (gt i 1000)
          (break)
          (if (gt total n) (break) (set! total (+ total i)))))
    total)

  ;; continue in a counted loop still applies the step: sum of odd i in 0..n
  (fn sum_odd [n:i32] -> i32
    (let total:i32 0)
    (loop i 0 n 1
      (if (eq (% i 2) 0) (continue) (block))
      (set! total (+ total i)))
    total)

  ;; continue in a while loop goes back to the condition
  (fn count_nonzero_digits [n:i32] -> i32
    (let v:i32 n)
    (let count:i32 0)
    (while (gt v 0)
      (let d:i32 (% v 10))
      (set! v (/ v 10))
      (if (eq d 0) (continue) (block))
      (set! count (+ count 1)))
    count)

  ;; break and continue in nested loops target the innermost loop
  (fn nested [n:i32] -> i32
    (let hits:i32 0)
    (loop i 1 n 1
      (loop j 1 n 1
        (if (gt j i) (break) (block))
        (if (eq j 2) (continue) (block))
        (set! hits (+ hits 1))))
    hits)

  ;; return from inside match_result inside a loop
  (fn parse_digit [c:i32] -> (result i32 i32)
    (if (and (gte c 48) (lte c 57)) (ok (- c 48)) (err c)))
  (fn first_non_digit [n:i32] -> i32
    (loop i 0 n 1
      (match_result (call parse_digit (+ 46 i))
        (ok d (block))
        (err e (return e))))
    0)

  ;; cond with several clauses and multi-expression bodies, as a value
  (fn classify [n:i32] -> i32
    (cond
      ((lt n 0) -1)
      ((eq n 0) 0)
      ((lt n 10) (let t:i32 (* n 2)) (+ t 1))
      (else 100)))

  ;; cond as a statement
  (fn bucket_sum [n:i32] -> i32
    (let small:i32 0)
    (let big:i32 0)
    (loop i 0 n 1
      (cond
        ((lt i 5) (set! small (+ small 1)))
        (else (set! big (+ big 1)))))
    (+ (* small 1000) big))

  ;; a body may end in (return v); void functions use (return)
  (fn ends_in_return [n:i32] -> i32
    (let x:i32 (* n 3))
    (return (+ x 1)))
  (fn bump [p:i32] -> void
    (if (lt p 0) (return) (block))
    (mem.store32 p (+ (mem.load32 p) 1)))
  (fn uses_void_return [n:i32] -> i32
    (let p:i32 (mem.alloc 4))
    (mem.store32 p n)
    (call bump p)
    (call bump -1)
    (mem.load32 p))

  ;; loop end and step are evaluated once, before the first pass; the body
  ;; may still set! the variable
  (fn moving_bounds [n:i32] -> i32
    (let limit:i32 n)
    (let count:i32 0)
    (loop i 0 limit 1
      (set! count (+ count 1))
      (if (eq i 2) (set! limit (- limit 1)) (block))
      (if (eq i 0) (set! i 1) (block)))
    count))
"#;

#[test]
fn early_return_inside_a_loop() {
    assert_eq!(run_both(PROGRAM, "first_mult7", 15), 21);
    assert_eq!(run_both(PROGRAM, "first_mult7", 14), 14);
}

#[test]
fn break_inside_a_nested_if() {
    assert_eq!(run_both(PROGRAM, "sum_until", 10), 15);
    assert_eq!(run_both(PROGRAM, "sum_until", -5), 0);
}

#[test]
fn continue_in_a_counted_loop_still_steps() {
    assert_eq!(run_both(PROGRAM, "sum_odd", 9), 1 + 3 + 5 + 7 + 9);
    assert_eq!(run_both(PROGRAM, "sum_odd", 0), 0);
}

#[test]
fn continue_in_a_while_loop() {
    assert_eq!(run_both(PROGRAM, "count_nonzero_digits", 1020304), 4);
}

#[test]
fn break_and_continue_target_the_innermost_loop() {
    // for each i, j runs 1..=i with j == 2 skipped
    assert_eq!(run_both(PROGRAM, "nested", 4), 1 + 1 + 2 + 3);
}

#[test]
fn return_from_a_match_result_arm_inside_a_loop() {
    // i = 0, 1 give '.', '/' (not digits): the first error is '.' = 46
    assert_eq!(run_both(PROGRAM, "first_non_digit", 5), 46);
    assert_eq!(run_both(PROGRAM, "first_non_digit", -1), 0);
}

#[test]
fn cond_as_a_value_and_as_a_statement() {
    assert_eq!(run_both(PROGRAM, "classify", -3), -1);
    assert_eq!(run_both(PROGRAM, "classify", 0), 0);
    assert_eq!(run_both(PROGRAM, "classify", 4), 9);
    assert_eq!(run_both(PROGRAM, "classify", 50), 100);
    assert_eq!(run_both(PROGRAM, "bucket_sum", 7), 5 * 1000 + 3);
}

#[test]
fn bodies_may_end_in_return_and_void_functions_return_early() {
    assert_eq!(run_both(PROGRAM, "ends_in_return", 5), 16);
    assert_eq!(run_both(PROGRAM, "uses_void_return", 41), 42);
}

#[test]
fn loop_bounds_are_evaluated_once_and_the_variable_may_be_set() {
    // i = 0 (set to 1), 2, 3, 4, 5: five passes for n = 5; lowering `limit`
    // inside the body does not change the bound already evaluated
    assert_eq!(run_both(PROGRAM, "moving_bounds", 5), 5);
}

#[test]
fn ens_sees_the_early_returned_value() {
    let src = "(module m (fn f [n:i32] -> i32 (ens (gt res 0)) (if (lt n 0) (return -1) (block)) n))";
    let m = Parser::parse(src).unwrap();
    TypeChecker::new().check_module(&m).unwrap();
    let mut vm = VM::new();
    vm.load_module(m);
    let err = vm.invoke("f", vec![Value::Int(-5)]).unwrap_err();
    assert!(err.starts_with("Post-condition"), "{err}");
    assert_eq!(vm.invoke("f", vec![Value::Int(3)]).unwrap(), Value::Int(3));
}

#[test]
fn misplaced_control_flow_is_rejected() {
    let err = check_err("(module m\n  (fn f [] -> i32\n    (break)\n    0))");
    assert!(err.starts_with("3:5: break is only allowed inside a while or loop body"), "{err}");
    let err = check_err("(module m (fn f [] -> i32 (while (continue) 0) 0))");
    assert!(err.contains("continue is only allowed inside a while or loop body") || err.contains("While condition"), "{err}");
    let err = check_err("(module m\n  (fn f [] -> i32\n    (return true)))");
    assert!(err.starts_with("3:5: return value has type Bool, but the function returns I32"), "{err}");
    let err = check_err("(module m (fn f [] -> void (return 1)))");
    assert!(err.contains("return value has type I32, but the function returns Void"), "{err}");
    let err = check_err("(module m (fn f [n:i32] -> i32 (req (block (return 1) true)) n))");
    assert!(err.contains("return is not allowed in a contract"), "{err}");
    // return is a statement: it cannot stand in for a value in an if
    let err = check_err("(module m (fn f [n:i32] -> i32 (if (lt n 0) (return 0) 5)))");
    assert!(err.contains("If branch type mismatch"), "{err}");
}

#[test]
fn malformed_cond_is_rejected() {
    let err = Parser::parse("(module m (fn f [n:i32] -> i32 (cond ((lt n 0) 1))))").unwrap_err();
    assert!(err.contains("cond needs a final (else ...) clause"), "{err}");
    let err = Parser::parse("(module m (fn f [n:i32] -> i32 (cond (else 1) ((lt n 0) 2))))").unwrap_err();
    assert!(err.contains("the else clause must be last"), "{err}");
    let err = Parser::parse("(module m (fn f [n:i32] -> i32 (cond ((lt n 0)) (else 1))))").unwrap_err();
    assert!(err.contains("cond clause needs a body"), "{err}");
}

/// `and`/`or` short-circuit (audit N1): the second operand runs only when it
/// decides the result, in both backends.
pub const SHORT_CIRCUIT: &str = r#"
(module sc
  ;; a zero divisor would trap if the second operand ran
  (fn safe_ratio_is_two [d:i32] -> bool (and (neq d 0) (eq (/ 10 d) 2)))
  (fn zero_or_divides [d:i32] -> bool (or (eq d 0) (eq (% 10 d) 0)))

  ;; side effects: count how often each second operand runs
  (fn bump [p:i32] -> bool (mem.store32 p (+ (mem.load32 p) 1)) true)
  (fn side_effects [n:i32] -> i32
    (let p:i32 (mem.alloc 4))
    (loop i 0 (- n 1) 1
      (let a:bool (and (lt i 3) (call bump p)))
      (let b:bool (or (lt i 3) (call bump p))))
    (mem.load32 p))

  ;; break inside the second operand: the and's own if is one more label
  ;; the first i in 0..n above 3, found by breaking out of the loop from
  ;; inside the and's second operand
  (fn first_index_over [n:i32] -> i32
    (let found:i32 -1)
    (loop i 0 n 1
      (if (and (gt i 3) (block (set! found i) (break) true)) (block) (block)))
    found)

  ;; break and return inside arithmetic operands: later operands must not run
  (fn jump_in_operand [n:i32] -> i32
    (let p:i32 (mem.alloc 4))
    (loop i 0 n 1
      (let x:i32 (+ (block (if (eq i 2) (break) (block)) i) (block (mem.store32 p (+ (mem.load32 p) 1)) 0))))
    (+ (* 100 (mem.load32 p)) (+ 1 (block (if (gt n 50) (return -7) (block)) 0))))

  ;; nested, as the checker requires for more than two operands
  (fn in_range [x:i32] -> bool (and (gte x 0) (and (lt x 10) (neq x 5)))))
"#;

#[test]
fn and_or_short_circuit_in_both_backends() {
    assert_eq!(run_both(SHORT_CIRCUIT, "safe_ratio_is_two", 0), 0);
    assert_eq!(run_both(SHORT_CIRCUIT, "safe_ratio_is_two", 5), 1);
    assert_eq!(run_both(SHORT_CIRCUIT, "zero_or_divides", 0), 1);
    assert_eq!(run_both(SHORT_CIRCUIT, "zero_or_divides", 3), 0);
    // 10 iterations: `and` runs bump for i < 3 (3 times), `or` for i >= 3 (7 times)
    assert_eq!(run_both(SHORT_CIRCUIT, "side_effects", 10), 10);
    assert_eq!(run_both(SHORT_CIRCUIT, "first_index_over", 2), -1);
    assert_eq!(run_both(SHORT_CIRCUIT, "first_index_over", 9), 4);
    // i = 0, 1 run the second operand; i = 2 breaks before it
    assert_eq!(run_both(SHORT_CIRCUIT, "jump_in_operand", 9), 201);
    assert_eq!(run_both(SHORT_CIRCUIT, "jump_in_operand", 60), -7);
    assert_eq!(run_both(SHORT_CIRCUIT, "in_range", 5), 0);
    assert_eq!(run_both(SHORT_CIRCUIT, "in_range", 7), 1);
}

#[test]
fn and_or_take_exactly_two_operands() {
    let err = check_err("(module m (fn f [] -> bool (and true true false)))");
    assert!(err.contains("and takes exactly 2 operands, got 3"), "{err}");
    let err = check_err("(module m (fn f [] -> bool (or true)))");
    assert!(err.contains("or takes exactly 2 operands, got 1"), "{err}");
}

/// A call in the end bound runs once, not once per pass (it used to consume
/// input on every pass: the NE1 wasm_reader bug).
#[test]
fn a_call_in_the_loop_bound_runs_once() {
    let src = r#"(module m
  (fn next_count [p:i32] -> i32 (mem.store32 p (+ (mem.load32 p) 1)) 3)
  (fn f [n:i32] -> i32
    (let p:i32 (mem.alloc 4))
    (let passes:i32 0)
    (loop i 1 (call next_count p) (+ 0 1) (set! passes (+ passes 1)))
    (+ (* 10 (mem.load32 p)) passes)))"#;
    assert_eq!(run_both(src, "f", 0), 13);
}
