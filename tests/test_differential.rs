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
use wasmtime::{Engine, Module as WasmModule, Store, Val, ValType};

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
    // Link WASI so modules that print or touch files (which import from
    // wasi_snapshot_preview1) instantiate too. No preopened directory: file
    // I/O in a differential case would be a host-state dependency, not a
    // semantics check. Modules without imports are unaffected.
    let mut linker: wasmtime::Linker<wasmtime_wasi::p1::WasiP1Ctx> = wasmtime::Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t| t).map_err(|e| format!("wasi link: {e}"))?;
    let ctx = wasmtime_wasi::WasiCtxBuilder::new().inherit_stdout().inherit_stderr().build_p1();
    let mut store = Store::new(&engine, ctx);
    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|e| format!("wasmtime instantiate: {e}"))?;
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

/// The VM bounds-checks `arr.get`/`arr.set` against the length header; the
/// wasm backend does not (AIPL_SPEC.md 4.E). Like contracts, this is a check
/// the VM adds on top of wasm semantics, so VM-error/wasm-success is accepted.
fn is_vm_bounds_check(e: &str) -> bool {
    e.starts_with("Array index out of bounds")
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
        (Err(e), Ok(_)) if is_contract_failure(e) || is_vm_bounds_check(e) => {}
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

// i64 <-> f64 conversions. Bit patterns are compared as i64 so that -0.0 and
// NaN payloads are checked exactly (f64 == would hide them).
#[test]
fn f64_i64_conversions_agree() {
    assert_eq!(expr("f64", "(f64.convert_i64_s -7i64)").unwrap(), Value::Float(-7.0));
    // 2^53 + 1 rounds to even
    assert_eq!(expr("f64", "(f64.convert_i64_s 9007199254740993i64)").unwrap(), Value::Float(9007199254740992.0));
    assert_i64("(i64.trunc_f64_s 2.75)", 2);
    assert_i64("(i64.trunc_f64_s -2.75)", -2);
    assert_i64("(i64.trunc_f64_s -9223372036854775808.0)", i64::MIN);
    assert_both_fail("i64", "(i64.trunc_f64_s 9223372036854775808.0)");
    assert_both_fail("i64", "(i64.trunc_f64_s (f64.reinterpret_i64 9221120237041090560i64))"); // NaN
    assert_i64("(i64.reinterpret_f64 1.0)", 0x3FF0_0000_0000_0000);
    assert_i64("(i64.reinterpret_f64 -0.0)", i64::MIN);
    assert_i64("(i64.reinterpret_f64 (f64.reinterpret_i64 9221120237041090561i64))", 9221120237041090561);
    assert_i64("(i64.reinterpret_f64 (/ (f64.convert_i64_s 1i64) (f64.convert_i64_s 10i64)))", 0.1f64.to_bits() as i64);
}

/// f64.sqrt: correctly rounded; NaN for a negative argument; -0.0 stays
/// -0.0; infinity stays infinity. Compared by bits.
#[test]
fn f64_sqrt_agrees() {
    assert_i64("(i64.reinterpret_f64 (f64.sqrt 2.0))", 2.0f64.sqrt().to_bits() as i64);
    assert_i64("(i64.reinterpret_f64 (f64.sqrt 0.25))", 0.5f64.to_bits() as i64);
    assert_i64("(i64.reinterpret_f64 (f64.sqrt -0.0))", (-0.0f64).to_bits() as i64);
    assert_i64("(i64.reinterpret_f64 (f64.sqrt (/ 1.0 (- 0.0 0.0))))", f64::INFINITY.to_bits() as i64);
    let nan = expr("i64", "(i64.reinterpret_f64 (f64.sqrt -1.0))").unwrap();
    assert!(matches!(nan, Value::Int64(b) if f64::from_bits(b as u64).is_nan()), "{nan:?}");
}

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
/// and sizes (`matrix_mult.aipl` multiplies n x n matrices), and a tree-walking VM at
/// `i32::MAX` iterations is a multi-hour run, not a test. Wrap-around edge
/// cases are covered by the explicit single-expression tests above.
const SAMPLE_ARGS: &[i32] = &[0, 1, 3, 7, 50, -1, -9];

/// Examples that are known not to parse against the current language and are
/// tracked elsewhere. Anything not on this list must parse, check, and compile.
const KNOWN_STALE_EXAMPLES: &[(&str, &str)] = &[];

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
        // Examples that do I/O (they import from WASI) depend on host files
        // and stdout; their VM-vs-wasm agreement is checked with a preopened
        // directory and captured output in tests/test_wasi.rs instead.
        let has_imports = wasmparser::Parser::new(0)
            .parse_all(&wasm)
            .any(|p| matches!(p, Ok(wasmparser::Payload::ImportSection(_))));
        if has_imports {
            eprintln!("SKIP {name}: does I/O (WASI imports); compared in tests/test_wasi.rs");
            continue;
        }

        let mut compared_here = 0;
        for f in &module.functions {
            if !matches!(f.return_type, Type::I32 | Type::I64 | Type::Bool) {
                continue;
            }
            if !f.params.iter().all(|(_, t)| *t == Type::I32) {
                continue;
            }
            if f.params.is_empty() {
                let _ = differential(&module, &wasm, &f.name, &[]);
                compared_here += 1;
                continue;
            }
            // Try each sample value in every position, plus a few mixed tuples.
            let n = f.params.len();
            let mut tuples: Vec<Vec<i32>> = SAMPLE_ARGS.iter().map(|&a| vec![a; n]).collect();
            tuples.push((0..n).map(|i| SAMPLE_ARGS[(i * 2) % SAMPLE_ARGS.len()]).collect());
            tuples.push((0..n).map(|i| SAMPLE_ARGS[(i + 3) % SAMPLE_ARGS.len()]).collect());
            for args in tuples {
                let _ = differential(&module, &wasm, &f.name, &args);
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

/// `aipl_src/codegen.aipl` (the self-hosted compiler front end and code
/// generator, plus the `compiler` module it imports) compiled to wasm and run
/// under wasmtime versus the VM. Until P6 this was pinned as "not
/// wasm-compilable" because two of its self-tests use `fs.*`; with WASI
/// lowering in place the whole module compiles and `test_signatures_and_locals`
/// (zero-arg, returns i32, no file I/O) is compared for real. The `Err` arm is
/// kept so a regression in fs lowering is reported as such rather than as a
/// silent skip.
#[test]
fn codegen_self_test_agrees_between_vm_and_wasmtime() {
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

#[test]
fn p7_typing_and_scoping_rules() {
    // Rule 1 & 2 & 3 & 4: void set!, void let, block scoping with outer mutation, void if
    let src = r#"
(module p7_test
  (fn test_scoping_and_void [n:i32] -> i32
    (let acc:i32 n)
    (if (gt n 0)
        (set! acc (+ acc 10))
        (set! acc (- acc 10)))
    (block
      (let acc_local:i32 999)
      (set! acc (+ acc acc_local)))
    acc)
  (fn test_result_matching [val:i32] -> i32
    (let r:(result i32 str) (if (gt val 0) (ok:str val) (err:i32 "neg")))
    (match_result r
      (ok v (+ v 100))
      (err e -1)))
)
"#;
    let module = Parser::parse(src).expect("p7_test parses");
    let mut checker = TypeChecker::new();
    checker.check_module(&module).expect("p7_test type-checks");

    let wasm = WasmCompiler::compile(&module).expect("p7_test compiles to WASM");

    let out1 = differential(&module, &wasm, "test_scoping_and_void", &[5]);
    assert_eq!(out1, Ok(Value::Int(1014))); // 5 + 10 + 999 = 1014

    let out2 = differential(&module, &wasm, "test_scoping_and_void", &[-5]);
    assert_eq!(out2, Ok(Value::Int(984))); // -5 - 10 + 999 = 984

    let out3 = differential(&module, &wasm, "test_result_matching", &[10]);
    assert_eq!(out3, Ok(Value::Int(110)));

    let out4 = differential(&module, &wasm, "test_result_matching", &[-10]);
    assert_eq!(out4, Ok(Value::Int(-1)));

    // Test Rule 4 Shadowing Error
    let shadow_src = r#"
(module shadow_test
  (fn bad_shadow [x:i32] -> i32
    (let x:i32 10)
    x)
)
"#;
    let m = Parser::parse(shadow_src).expect("shadow_test parses");
    let err = TypeChecker::new().check_module(&m).unwrap_err();
    assert!(err.contains("Cannot shadow"), "expected shadowing error, got: {err}");

    // Test Rule 5 Set on Undeclared Variable
    let set_err_src = r#"
(module set_test
  (fn bad_set [] -> i32
    (set! unassigned 5)
    0)
)
"#;
    let m2 = Parser::parse(set_err_src).expect("set_test parses");
    let err2 = TypeChecker::new().check_module(&m2).unwrap_err();
    assert!(err2.contains("Undefined variable"), "expected undefined variable error, got: {err2}");
}

#[test]
fn p8_structs_and_arrays() {
    let src = r#"
(module p8_test
  (struct Point [x:i32 y:i32])
  (struct Mixed [flag:bool val:i64 tag:i32])
  (struct Floats [a:f32 b:f64])
  (struct Named [id:i32 name:str])

  (fn test_point_ops [x:i32 y:i32] -> i32
    (let p:(ptr Point) (new Point))
    (put p Point.x x)
    (put p Point.y y)
    (+ (get p Point.x) (get p Point.y)))

  (fn test_sizeof [] -> i32
    (+ (sizeof Point) (+ (sizeof Mixed) (sizeof Floats))))

  ;; i64 field at offset 8 (after bool + 4 bytes padding), tag at 16
  (fn test_mixed [n:i32] -> i32
    (let m:(ptr Mixed) (new Mixed))
    (put m Mixed.flag true)
    (put m Mixed.val (* (i64.extend_s n) 4294967296i64))
    (put m Mixed.tag 7)
    (if (get m Mixed.flag)
        (+ (i32.wrap (shr (get m Mixed.val) 32i64)) (+ (get m Mixed.tag) (mem.load32 (+ (ptr.addr m) 16))))
        -1))

  (fn test_floats [] -> i32
    (let f:(ptr Floats) (new Floats))
    (put f Floats.b 2.5)
    (if (gt (get f Floats.b) 2.0) 1 0))

  (fn test_array_ops [n:i32] -> i32
    (let arr:(arr i32) (arr.new i32 n))
    (loop i 0 (- n 1) 1
      (arr.set i32 arr i (* (+ i 1) 10)))
    (let sum:i32 0)
    (loop i 0 (- n 1) 1
      (set! sum (+ sum (arr.get i32 arr i))))
    sum)

  ;; header holds the count; the bump cursor advances by 4 + n * 8
  (fn test_i64_array [n:i32] -> i32
    (let a:(arr i64) (arr.new i64 n))
    (arr.set i64 a (- n 1) 5i64)
    (let next:i32 (mem.alloc 4))
    (+ (arr.len a) (+ (- next (arr.addr a)) (i32.wrap (arr.get i64 a (- n 1))))))

  (fn test_index_oob [i:i32] -> i32
    (let a:(arr i32) (arr.new i32 3))
    (arr.get i32 a i))

  (fn alloc_four [] -> i32
    (let _p:i32 (mem.alloc 4))
    2)

  ;; the size expression allocates; the array must not overlap that block
  (fn test_size_allocates [] -> i32
    (let a:(arr i32) (arr.new i32 (call alloc_four)))
    (- (arr.addr a) (mem.load32 0)))

  ;; ok/err take 8 heap bytes in both backends, before the payload runs
  (fn test_result_heap [] -> i32
    (let r:(result i32 i32) (ok 5))
    (let e:(result i32 i32) (err 6))
    (mem.alloc 4))

  (fn test_result_payload_allocates [] -> i32
    (match_result (ok (arr.new i32 2))
      (ok v (arr.addr v))
      (err e 0)))

  ;; AIPL_SPEC.md 4.E example
  (fn test_points [] -> i32
    (let ps:(arr (ptr Point)) (arr.new (ptr Point) 3))
    (loop i 0 2 1
      (let p:(ptr Point) (new Point))
      (put p Point.x i)
      (put p Point.y (* i 10))
      (arr.set (ptr Point) ps i p))
    (let sum:i32 0)
    (loop i 0 2 1
      (let p:(ptr Point) (arr.get (ptr Point) ps i))
      (set! sum (+ sum (+ (get p Point.x) (get p Point.y)))))
    sum)

  ;; a bool word holding 2 (written raw) reads as true in both backends
  (fn test_bool_word [] -> i32
    (let p:(ptr Mixed) (new Mixed))
    (mem.store32 (ptr.addr p) 2)
    (let a:(arr bool) (arr.new bool 1))
    (mem.store32 (arr.addr a) 7)
    (if (and (eq (get p Mixed.flag) true) (and (arr.get bool a 0) true)) 1 0))

  ;; str fields and elements hold the address of the bytes; the VM copies the
  ;; string into the heap, wasm points at the interned literal
  (fn test_str_field [] -> i32
    (let n:(ptr Named) (new Named))
    (put n Named.name "abc")
    (let a:(arr str) (arr.new str 2))
    (arr.set str a 1 (get n Named.name))
    (+ (str.len (arr.get str a 1)) (if (eq (get n Named.name) "abc") 10 0)))

  (fn test_put_reserved [] -> i32
    (put (ptr.cast Point (- 600 88)) Point.x 1)
    0)

  (fn test_arr_set_reserved [] -> i32
    (arr.set i32 (arr.cast i32 (- 600 88)) 0 1)
    0)
)
"#;
    let module = Parser::parse(src).expect("p8_test parses");
    TypeChecker::new().check_module(&module).expect("p8_test type-checks");
    let wasm = WasmCompiler::compile(&module).expect("p8_test compiles to WASM");

    assert_eq!(differential(&module, &wasm, "test_point_ops", &[15, 27]), Ok(Value::Int(42)));
    // Point 8; Mixed 24 (bool 4 + pad 4 + i64 8 + i32 4 + pad 4); Floats 16 (f32 4 + pad 4 + f64 8)
    assert_eq!(differential(&module, &wasm, "test_sizeof", &[]), Ok(Value::Int(48)));
    // high word of n << 32 is n; tag read via get and via raw offset 16
    assert_eq!(differential(&module, &wasm, "test_mixed", &[3]), Ok(Value::Int(3 + 7 + 7)));
    assert_eq!(differential(&module, &wasm, "test_floats", &[]), Ok(Value::Int(1)));
    assert_eq!(differential(&module, &wasm, "test_array_ops", &[5]), Ok(Value::Int(150)));
    assert_eq!(differential(&module, &wasm, "test_points", &[]), Ok(Value::Int(33)));
    assert_eq!(differential(&module, &wasm, "test_bool_word", &[]), Ok(Value::Int(1)));
    assert_eq!(differential(&module, &wasm, "test_str_field", &[]), Ok(Value::Int(13)));
    // count 3 + (array pointer to the next block: the 4 + 3 * 8 = 28-byte block
    // rounded to 32, minus the 4-byte header) + element 5
    assert_eq!(differential(&module, &wasm, "test_i64_array", &[3]), Ok(Value::Int(3 + 28 + 5)));
    // relative to the heap start H: alloc_four's block at H (4 bytes, rounded
    // to 8), then the array block at H+8 (4 + 2 * 4 = 12 bytes, rounded to
    // 16): array at H+12, cursor H+24, so array - cursor = -12
    assert_eq!(differential(&module, &wasm, "test_size_allocates", &[]), Ok(Value::Int(-12)));
    // The module's string literal occupies 1024..1032, so the heap starts at
    // 1032 in both backends: two 8-byte result cells at 1032 and 1040, then
    // the 4-byte block.
    assert_eq!(differential(&module, &wasm, "test_result_heap", &[]), Ok(Value::Int(1048)));
    // result cell at 1032, array header at 1040, array at 1044
    assert_eq!(differential(&module, &wasm, "test_result_payload_allocates", &[]), Ok(Value::Int(1044)));

    // Negative array size: VM error, wasm trap.
    assert!(differential(&module, &wasm, "test_array_ops", &[-1]).is_err());
    // put / arr.set into the reserved block: VM error, wasm trap.
    let e = differential(&module, &wasm, "test_put_reserved", &[]).unwrap_err();
    assert!(e.contains("reserved runtime block"), "got {e}");
    // (the VM's bounds check sees no length header there and fires first)
    assert!(differential(&module, &wasm, "test_arr_set_reserved", &[]).is_err());

    // In-bounds index agrees; out-of-bounds index is a VM error (wasm reads past the array).
    assert_eq!(differential(&module, &wasm, "test_index_oob", &[2]), Ok(Value::Int(0)));
    for i in [3, -1] {
        let e = differential(&module, &wasm, "test_index_oob", &[i]).unwrap_err();
        assert_eq!(e, format!("Array index out of bounds: index {i} for array of length 3"));
    }
}

#[test]
fn p8_wasm_rejects_non_32_bit_result_payloads() {
    let src = "(module m (fn f [] -> (result i64 i32) (ok 1i64)))";
    let module = Parser::parse(src).expect("parses");
    TypeChecker::new().check_module(&module).expect("checks");
    let err = WasmCompiler::compile(&module).unwrap_err();
    assert!(err.contains("result payloads must be 32-bit"), "got {err}");
}

// Allocation grows memory: new, arr.new, ok/err cells, and mem.alloc past the
// initial 1 MiB need no explicit mem.grow, and both backends end with the same
// number of pages.
#[test]
fn allocation_grows_memory_identically() {
    let src = r#"
(module grow
  (struct P [x:i32 y:i64])
  (fn main [] -> i32
    (let total:i32 0)
    (loop i 1 3000 1
      (let p:(ptr P) (new P))
      (put p P.x i)
      (let a:(arr i32) (arr.new i32 100))
      (arr.set i32 a 99 i)
      (let r:(result i32 i32) (ok i))
      (set! total (+ total (- (arr.get i32 a 99) (get p P.x)))))
    (let big:i32 (mem.alloc 2000000))
    (mem.store32 (+ big 1999996) 7)
    (+ (* 1000 total) (+ (* 100 (mem.load32 (+ big 1999996))) (mem.grow 0))))
  ;; past the 1024-page cap allocation stops growing and the store fails in both
  (fn too_big [] -> i32
    (let p:i32 (mem.alloc 68000000))
    (mem.store32 (+ p 67999996) 1)
    0))
"#;
    let (module, wasm) = compile_checked(src);
    // 51 pages: 1 MiB start + ~1.3 MB of structs/arrays/cells + 2 MB block
    assert_eq!(differential(&module, &wasm, "main", &[]), Ok(Value::Int(700 + 51)));
    assert!(differential(&module, &wasm, "too_big", &[]).is_err());
}

/// An allocation whose size expression itself allocates: the inner block must
/// not be handed out again. Compiled code used to read the cursor before
/// evaluating the size, so both allocations started at the same address in
/// wasm (the VM was right). Allocation is now one atomic read-and-add after
/// the size is known.
#[test]
fn nested_allocations_do_not_overlap() {
    let src = r#"
(module nest
  (fn inner_size [] -> i32
    (let p:i32 (mem.alloc 8))
    (mem.store32 p 99)
    8)
  (fn main [] -> i32
    (let q:i32 (mem.alloc (call inner_size)))
    (mem.store32 q 7)
    (mem.load32 (- q 8)))
  ;; the same through arr.new's length and a struct inside a result payload
  (fn arr_len_allocates [] -> i32
    (let a:(arr i32) (arr.new i32 (call inner_size)))
    (arr.set i32 a 0 5)
    (mem.load32 (- (arr.addr a) 12))))
"#;
    let (module, wasm) = compile_checked(src);
    assert_eq!(differential(&module, &wasm, "main", &[]), Ok(Value::Int(99)));
    assert_eq!(differential(&module, &wasm, "arr_len_allocates", &[]), Ok(Value::Int(99)));
}

/// Locals declared inside operands, loop bounds, and the matched expression
/// of a match_result. The wasm backend used to skip operator operands when
/// collecting locals, so `(+ 1 (match_result ...))` failed to compile with
/// "Unbound local variable" while the VM ran it.
const NESTED_LOCALS: &str = "(module nested
  (fn g [n:i32] -> (result i32 i32) (if (gt n 0) (ok n) (err n)))
  (fn main [] -> i32
    (let total:i32 0)
    (loop i 0 (block (let last:i32 3) last) 1
      (set! total (+ total (match_result (call g i) (ok k (+ k 1)) (err e (- e 1))))))
    (+ total
       (+ (block (let x:i32 40) (+ x 2))
          (match_result (block (let y:i32 5) (call g y)) (ok v (* v 100)) (err w 0))))))";

#[test]
fn locals_inside_operands_agree() {
    let (module, wasm) = compile_checked(NESTED_LOCALS);
    // -1 + 2 + 3 + 4, then 42, then 500
    assert_eq!(differential(&module, &wasm, "main", &[]).unwrap(), Value::Int(550));
}

/// ltu/lteu/gtu/gteu compare i32 and i64 as unsigned, at the edges where
/// signed and unsigned order disagree.
#[test]
fn unsigned_comparisons_agree() {
    let yes = |e: &str| assert_eq!(expr("bool", e).unwrap(), Value::Int(1), "{e}");
    let no = |e: &str| assert_eq!(expr("bool", e).unwrap(), Value::Int(0), "{e}");
    yes("(ltu 1 -1)");
    no("(ltu -1 1)");
    yes("(gtu -2147483648 2147483647)");
    yes("(lteu -1 -1)");
    no("(gteu 0 1)");
    yes("(gteu -1 0)");
    yes("(ltu 5i64 -1i64)");
    yes("(gtu -9223372036854775808i64 9223372036854775807i64)");
    no("(lteu -1i64 0i64)");
    yes("(gteu 7i64 7i64)");
    let m = Parser::parse("(module m (fn f [] -> bool (ltu 1.0 2.0)))").unwrap();
    let e = TypeChecker::new().check_module(&m).unwrap_err();
    assert!(e.contains("LtU compares integers (i32 or i64) as unsigned, got F64"), "{e}");
}

/// checked.add/sub/mul on every pair of edge values, both widths: the VM and
/// wasmtime agree with each other and with Rust's checked_* (a value, or an
/// overflow error in the VM and a trap in wasm).
#[test]
fn checked_arithmetic_agrees_with_rust() {
    let e32: [i32; 12] = [0, 1, -1, 2, -2, 46340, 46341, -46341, 65536, i32::MAX, i32::MIN, i32::MAX - 1];
    let e64: [i64; 12] = [0, 1, -1, 2, -2, 3037000499, 3037000500, -3037000500, 4294967296, i64::MAX, i64::MIN, i64::MIN + 1];
    let ops: [(&str, fn(i64, i64, bool) -> Option<i64>); 3] = [
        ("checked.add", |a, b, w| if w { a.checked_add(b) } else { (a as i32).checked_add(b as i32).map(i64::from) }),
        ("checked.sub", |a, b, w| if w { a.checked_sub(b) } else { (a as i32).checked_sub(b as i32).map(i64::from) }),
        ("checked.mul", |a, b, w| if w { a.checked_mul(b) } else { (a as i32).checked_mul(b as i32).map(i64::from) }),
    ];
    let mut checked = 0;
    for (name, f) in ops {
        for &a in &e32 {
            for &b in &e32 {
                let got = expr("i32", &format!("({name} {a} {b})"));
                match f(a as i64, b as i64, false) {
                    Some(v) => assert_eq!(got.unwrap(), Value::Int(v), "({name} {a} {b})"),
                    None => assert!(got.unwrap_err().contains("Integer overflow"), "({name} {a} {b})"),
                }
                checked += 1;
            }
        }
        for &a in &e64 {
            for &b in &e64 {
                let got = expr("i64", &format!("({name} {a}i64 {b}i64)"));
                match f(a, b, true) {
                    Some(v) => assert_eq!(got.unwrap(), Value::Int64(v), "({name} {a}i64 {b}i64)"),
                    None => assert!(got.unwrap_err().contains("Integer overflow"), "({name} {a}i64 {b}i64)"),
                }
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 864);
    // nested: an operand that is itself checked
    assert_eq!(expr("i64", "(checked.add (checked.mul 3i64 4i64) (checked.sub 10i64 (checked.add 1i64 2i64)))").unwrap(), Value::Int64(19));
    let m = Parser::parse("(module m (fn f [] -> f64 (checked.add 1.0 2.0)))").unwrap();
    let e = TypeChecker::new().check_module(&m).unwrap_err();
    assert!(e.contains("CheckedAdd is integer arithmetic (i32 or i64), got F64"), "{e}");
}
