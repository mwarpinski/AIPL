//! VM-vs-wasm differential testing.
//!
//! Wasm semantics are the spec (AIPL_SPEC.md, section 8). Every case here runs
//! the same AIPL source in `aipl_core::vm::VM` and, after compilation by
//! `WasmCompiler`, in wasmtime, and asserts the two agree: same value, or both
//! fail. Any divergence is a VM bug and must be fixed in `src/vm.rs`, never by
//! bending the wasm backend to match the interpreter.
//!
//! Contracts (`req` / `ens`) are the one deliberate asymmetry: the VM checks
//! them at call time and the wasm backend does not emit them. A VM contract
//! failure is therefore reported as a skip, not a divergence.

use aipl_core::ast::{Module, Type};
use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::parser::Parser;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use std::path::Path;
use wasmtime::{Engine, Instance, Module as WasmModule, Store, Val, ValType};

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// Result of running one function in one backend, normalised so the two sides
/// can be compared with `==`. `bool` results become `Int(0|1)` because that is
/// their wasm representation.
type Outcome = Result<Value, String>;

fn normalise(v: Value) -> Value {
    match v {
        Value::Bool(b) => Value::Int(b as i64),
        other => other,
    }
}

fn run_vm(module: &Module, fn_name: &str, args: &[i32]) -> Outcome {
    let mut vm = VM::new();
    vm.load_module(module.clone());
    vm.invoke(fn_name, args.iter().map(|&a| Value::Int(a as i64)).collect())
        .map(normalise)
}

fn run_wasmtime(wasm: &[u8], fn_name: &str, args: &[i32]) -> Outcome {
    let engine = Engine::default();
    let module = WasmModule::new(&engine, wasm).map_err(|e| format!("wasmtime module: {e}"))?;
    let mut store = Store::new(&engine, ());
    let instance = Instance::new(&mut store, &module, &[]).map_err(|e| format!("wasmtime instantiate: {e}"))?;
    let func = instance
        .get_func(&mut store, fn_name)
        .ok_or_else(|| format!("wasmtime: export '{fn_name}' not found"))?;
    let ty = func.ty(&store);
    let params: Vec<Val> = args.iter().map(|&a| Val::I32(a)).collect();
    let mut results: Vec<Val> = ty
        .results()
        .map(|t| match t {
            ValType::I32 => Val::I32(0),
            ValType::I64 => Val::I64(0),
            ValType::F32 => Val::F32(0),
            ValType::F64 => Val::F64(0),
            other => panic!("unexpected wasm result type {other:?}"),
        })
        .collect();
    func.call(&mut store, &params, &mut results)
        .map_err(|e| format!("wasmtime trap: {e}"))?;
    Ok(match results.first() {
        None => Value::Void,
        Some(Val::I32(v)) => Value::Int(*v as i64),
        Some(Val::I64(v)) => Value::Int64(*v),
        Some(Val::F64(bits)) => Value::Float(f64::from_bits(*bits)),
        Some(Val::F32(bits)) => Value::Float(f32::from_bits(*bits) as f64),
        Some(other) => panic!("unexpected wasm result {other:?}"),
    })
}

fn is_contract_failure(e: &str) -> bool {
    e.starts_with("Pre-condition") || e.starts_with("Post-condition")
}

/// Runs `fn_name` with `args` in both backends and asserts agreement. Returns
/// the agreed outcome so callers can additionally assert the concrete value.
fn differential(module: &Module, wasm: &[u8], fn_name: &str, args: &[i32]) -> Outcome {
    let vm = run_vm(module, fn_name, args);
    let wt = run_wasmtime(wasm, fn_name, args);
    match (&vm, &wt) {
        (Ok(a), Ok(b)) => assert_eq!(
            a, b,
            "DIVERGENCE in '{fn_name}' with args {args:?}: VM={a:?} wasmtime={b:?} (fix src/vm.rs)"
        ),
        (Err(_), Err(_)) => {}
        (Err(e), Ok(_)) if is_contract_failure(e) => {}
        (Err(e), Ok(b)) => panic!(
            "DIVERGENCE in '{fn_name}' with args {args:?}: VM errored ({e}) but wasmtime returned {b:?} (fix src/vm.rs)"
        ),
        (Ok(a), Err(e)) => panic!(
            "DIVERGENCE in '{fn_name}' with args {args:?}: VM returned {a:?} but wasmtime failed ({e}) (fix src/vm.rs)"
        ),
    }
    vm
}

fn compile_checked(src: &str) -> (Module, Vec<u8>) {
    let module = Parser::parse(src).expect("parse");
    TypeChecker::new().check_module(&module).expect("check");
    let wasm = WasmCompiler::compile(&module).expect("wasm compile");
    (module, wasm)
}

/// One-expression program: `(module m (fn f [] -> RET EXPR))`, run in both
/// backends, agreement asserted, agreed value returned.
fn expr(ret: &str, e: &str) -> Outcome {
    let src = format!("(module m (fn f [] -> {ret} {e}))");
    let (module, wasm) = compile_checked(&src);
    differential(&module, &wasm, "f", &[])
}

fn assert_i32(e: &str, expected: i32) {
    assert_eq!(expr("i32", e).unwrap(), Value::Int(expected as i64), "for {e}");
}

fn assert_i64(e: &str, expected: i64) {
    assert_eq!(expr("i64", e).unwrap(), Value::Int64(expected), "for {e}");
}

fn assert_both_fail(ret: &str, e: &str) {
    assert!(expr(ret, e).is_err(), "expected both backends to fail for {e}");
}

// ---------------------------------------------------------------------------
// Explicit i32 cases from the P3 prompt
// ---------------------------------------------------------------------------

#[test]
fn i32_add_wraps() {
    assert_i32("(+ 2147483647 1)", -2147483648);
}

#[test]
fn i32_shr_is_arithmetic() {
    assert_i32("(shr -8 1)", -4);
}

#[test]
fn i32_shru_is_logical() {
    assert_i32("(shru -8 1)", 2147483644);
}

#[test]
fn i32_mul_wraps_to_zero() {
    assert_i32("(* 65536 65536)", 0);
}

#[test]
fn i32_div_truncates_toward_zero() {
    assert_i32("(/ -7 2)", -3);
}

#[test]
fn i32_rem_sign_follows_dividend() {
    assert_i32("(% -7 2)", -1);
}

#[test]
fn i32_divu_treats_operands_as_unsigned() {
    assert_i32("(divu -1 2)", 2147483647);
}

#[test]
fn i32_remu_treats_operands_as_unsigned() {
    assert_i32("(remu -1 2)", 1);
}

#[test]
fn i32_shift_count_is_masked_to_5_bits() {
    assert_i32("(shl 1 33)", 2);
    assert_i32("(shr -2147483648 32)", -2147483648);
}

#[test]
fn i32_sub_wraps() {
    assert_i32("(- -2147483648 1)", 2147483647);
}

#[test]
fn i32_division_by_zero_fails_in_both() {
    assert_both_fail("i32", "(/ 7 0)");
    assert_both_fail("i32", "(% 7 0)");
    assert_both_fail("i32", "(divu 7 0)");
    assert_both_fail("i32", "(remu 7 0)");
}

#[test]
fn i32_min_div_minus_one_fails_in_both() {
    assert_both_fail("i32", "(/ -2147483648 -1)");
}

#[test]
fn i32_min_rem_minus_one_is_zero_in_both() {
    assert_i32("(% -2147483648 -1)", 0);
}

#[test]
fn loop_end_bound_is_inclusive() {
    let src = r#"
    (module m
      (fn f [] -> i32
        (let acc:i32 0)
        (loop i 0 5 1 (set! acc (+ acc i)))
        acc))
    "#;
    let (module, wasm) = compile_checked(src);
    assert_eq!(differential(&module, &wasm, "f", &[]).unwrap(), Value::Int(15));
}

#[test]
fn loop_with_step_and_negative_start() {
    let src = r#"
    (module m
      (fn f [] -> i32
        (let acc:i32 0)
        (let n:i32 0)
        (loop i -6 6 3 (set! acc (+ acc i)) (set! n (+ n 1)))
        (+ (* n 1000) acc)))
    "#;
    let (module, wasm) = compile_checked(src);
    // iterations: -6,-3,0,3,6 -> 5 iterations, sum 0
    assert_eq!(differential(&module, &wasm, "f", &[]).unwrap(), Value::Int(5000));
}

#[test]
fn while_with_set_in_body() {
    let src = r#"
    (module m
      (fn f [] -> i32
        (let n:i32 10)
        (let steps:i32 0)
        (while (gt n 0)
          (set! n (- n 3))
          (set! steps (+ steps 1)))
        (+ (* steps 100) n)))
    "#;
    let (module, wasm) = compile_checked(src);
    // n: 10 -> 7 -> 4 -> 1 -> -2 (4 steps), result 400 + (-2)
    assert_eq!(differential(&module, &wasm, "f", &[]).unwrap(), Value::Int(398));
}

#[test]
fn nested_if_and_block_values() {
    let src = r#"
    (module m
      (fn f [] -> i32
        (let x:i32 7)
        (if (gt x 5)
            (block (set! x (* x 2)) (+ x 1))
            (if (eq x 5) 500 0))))
    "#;
    let (module, wasm) = compile_checked(src);
    assert_eq!(differential(&module, &wasm, "f", &[]).unwrap(), Value::Int(15));
}

#[test]
fn memory_round_trip_and_bump_allocator_agree() {
    let src = r#"
    (module m
      (fn f [] -> i32
        (let p:i32 (mem.alloc 8))
        (let q:i32 (mem.alloc 4))
        (mem.store32 p -559038737)
        (mem.store8 q 200)
        (+ (+ (mem.load8 q) (mem.load32 p)) (- q p))))
    "#;
    let (module, wasm) = compile_checked(src);
    // 200 + 0xDEADBEEF(as i32) + 8
    assert_eq!(
        differential(&module, &wasm, "f", &[]).unwrap(),
        Value::Int((200i32.wrapping_add(-559038737).wrapping_add(8)) as i64)
    );
}

#[test]
fn recursion_and_calls_agree() {
    let src = r#"
    (module m
      (fn fib [n:i32] -> i32
        (if (lte n 1) n (+ (call fib (- n 1)) (call fib (- n 2)))))
      (fn f [] -> i32 (call fib 20)))
    "#;
    let (module, wasm) = compile_checked(src);
    assert_eq!(differential(&module, &wasm, "f", &[]).unwrap(), Value::Int(6765));
}

// ---------------------------------------------------------------------------
// i64 cases (added when i64 became a first-class type)
// ---------------------------------------------------------------------------

#[test]
fn i64_add_wraps_at_64() {
    assert_i64("(+ 9223372036854775807i64 1i64)", i64::MIN);
}

#[test]
fn i64_does_not_wrap_at_32() {
    assert_i64("(+ 2147483647i64 1i64)", 2147483648);
}

#[test]
fn i64_conversions_agree() {
    assert_i32("(i32.wrap 4294967301i64)", 5);
    assert_i64("(i64.extend_u -1)", 4294967295);
    assert_i64("(i64.extend_s -1)", -1);
}

#[test]
fn i64_shift_count_is_masked_to_6_bits() {
    assert_i64("(shl 1i64 65i64)", 2);
}

#[test]
fn i64_unsigned_division_agrees() {
    assert_i64("(divu -1i64 2i64)", i64::MAX);
    assert_i64("(remu -1i64 2i64)", 1);
}

#[test]
fn i64_division_edge_cases_agree() {
    assert_both_fail("i64", "(/ 1i64 0i64)");
    assert_both_fail("i64", "(/ -9223372036854775808i64 -1i64)");
    assert_i64("(% -9223372036854775808i64 -1i64)", 0);
}

#[test]
fn if_with_i64_branches_agrees() {
    let src = r#"
    (module m
      (fn f [] -> i64
        (let a:i64 4294967296i64)
        (let b:i64 1i64)
        (if (gt a b) a b)))
    "#;
    let (module, wasm) = compile_checked(src);
    assert_eq!(differential(&module, &wasm, "f", &[]).unwrap(), Value::Int64(4294967296));
}

#[test]
fn i64_memory_round_trip_agrees() {
    let src = r#"
    (module m
      (fn f [] -> i64
        (let p:i32 (mem.alloc 8))
        (mem.store64 p -2i64)
        (+ (mem.load64 p) (i64.extend_u (mem.load32 p)))))
    "#;
    let (module, wasm) = compile_checked(src);
    // -2 + 0xFFFFFFFE = -2 + 4294967294
    assert_eq!(differential(&module, &wasm, "f", &[]).unwrap(), Value::Int64(4294967292));
}

// ---------------------------------------------------------------------------
// f64 (type-directed codegen regression)
// ---------------------------------------------------------------------------

#[test]
fn f64_arithmetic_agrees() {
    assert_eq!(expr("f64", "(/ (+ 1.5 2.25) 0.5)").unwrap(), Value::Float(7.5));
    assert_eq!(expr("bool", "(lt 1.5 2.25)").unwrap(), Value::Int(1));
}

// ---------------------------------------------------------------------------
// Whole-program cases: examples/*.aipl and the codegen self-test
// ---------------------------------------------------------------------------

/// Argument values tried for every function whose parameters are all i32.
/// Contract failures skip a tuple; everything else must agree. Values are kept
/// small on purpose: example functions use their arguments as loop bounds
/// (`matrix_mult.aipl` loops `iterations` times), and a tree-walking VM at
/// `i32::MAX` iterations is a multi-hour run, not a test. Wrap-around edge
/// cases are covered by the explicit single-expression tests above.
const SAMPLE_ARGS: &[i32] = &[0, 1, 3, 7, 50, -1, -9];

/// Examples that are known not to parse against the current language and are
/// tracked elsewhere. Anything not on this list must parse, check, and compile.
const KNOWN_STALE_EXAMPLES: &[(&str, &str)] = &[(
    "hello_browser.aipl",
    "uses dom.* / web.alert, which P2 removed from the language; needs rewriting or moving to attic/",
)];

#[test]
fn every_example_agrees_between_vm_and_wasmtime() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .expect("examples dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map_or(false, |x| x == "aipl"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no examples found");

    let mut files_compared = 0;
    let mut calls_compared = 0;

    for path in &paths {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if let Some((_, why)) = KNOWN_STALE_EXAMPLES.iter().find(|(n, _)| *n == name) {
            eprintln!("SKIP {name}: {why}");
            assert!(
                Resolver::resolve(path).is_err(),
                "{name} now parses; remove it from KNOWN_STALE_EXAMPLES"
            );
            continue;
        }

        let module = Resolver::resolve(path).unwrap_or_else(|e| panic!("{name}: {e}"));
        TypeChecker::new().check_module(&module).unwrap_or_else(|e| panic!("{name}: {e}"));
        let wasm = match WasmCompiler::compile(&module) {
            Ok(w) => w,
            Err(e) if e.contains("not supported") || e.contains("not yet supported") => {
                eprintln!("SKIP {name}: wasm backend rejects it ({e})");
                continue;
            }
            Err(e) => panic!("{name}: wasm compile failed: {e}"),
        };

        let mut compared_here = 0;
        for f in &module.functions {
            if !matches!(f.return_type, Type::I32 | Type::I64 | Type::Bool) {
                continue;
            }
            if !f.params.iter().all(|(_, t)| *t == Type::I32) {
                continue;
            }
            if f.params.is_empty() {
                differential(&module, &wasm, &f.name, &[]);
                compared_here += 1;
                continue;
            }
            // Try each sample value in every position, plus a few mixed tuples.
            let n = f.params.len();
            let mut tuples: Vec<Vec<i32>> = SAMPLE_ARGS.iter().map(|&a| vec![a; n]).collect();
            tuples.push((0..n).map(|i| SAMPLE_ARGS[(i * 2) % SAMPLE_ARGS.len()]).collect());
            tuples.push((0..n).map(|i| SAMPLE_ARGS[(i + 3) % SAMPLE_ARGS.len()]).collect());
            for args in tuples {
                differential(&module, &wasm, &f.name, &args);
                compared_here += 1;
            }
        }
        if compared_here == 0 {
            eprintln!("NONE {name}: compiles, but has no i32/i64/bool-returning function with all-i32 params to compare");
        } else {
            eprintln!("OK   {name}: {compared_here} calls agree");
        }
        if compared_here > 0 {
            files_compared += 1;
            calls_compared += compared_here;
        }
    }

    // Guard against the test becoming vacuous if examples are moved/renamed.
    assert!(
        files_compared >= 4,
        "expected at least 4 example files to be compared, got {files_compared}"
    );
    assert!(calls_compared >= 40, "expected at least 40 compared calls, got {calls_compared}");
}

/// `aipl_src/codegen.aipl` cannot be compared today: the module also contains
/// `test_compile_add` / `test_compile_compute`, which call `fs.open`,
/// `fs.write`, and `fs.close`, and the wasm backend rejects any module that
/// uses `fs.*` (needs WASI, P6). This test pins that reason so it fails loudly
/// the day it stops being true, at which point `test_signatures_and_locals`
/// (zero-arg, returns i32) should be run through `differential` like the rest.
#[test]
fn codegen_self_test_is_skipped_for_a_pinned_reason() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("aipl_src/codegen.aipl");
    let module = Resolver::resolve(&path).expect("codegen.aipl must resolve");
    TypeChecker::new().check_module(&module).expect("codegen.aipl must type-check");
    let target = module
        .functions
        .iter()
        .find(|f| f.name == "test_signatures_and_locals")
        .expect("test_signatures_and_locals exists");
    assert!(target.params.is_empty() && target.return_type == Type::I32);

    match WasmCompiler::compile(&module) {
        Err(e) => assert!(
            e.contains("Fs"),
            "codegen.aipl is rejected by the wasm backend for a new reason: {e}"
        ),
        Ok(wasm) => {
            // The blocker is gone: run the real comparison instead of skipping.
            let out = differential(&module, &wasm, "test_signatures_and_locals", &[]);
            assert!(out.is_ok(), "test_signatures_and_locals failed in both backends: {out:?}");
        }
    }
}
