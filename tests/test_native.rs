//! The native backend's harness (docs/NATIVE_BACKEND_PLAN.md, from NE5).
//!
//! Every program in `PROGRAMS` is compiled to wasm by the Rust toolchain and
//! run under `aipl-run`, and translated to a native executable by
//! `aipl_src/native/native.aipl` and run directly. Both runs must give the
//! same stdout, stderr, and exit status. Later tasks extend `PROGRAMS`.
//!
//! The native compiler is AIPL; here it runs compiled to wasm under
//! wasmtime (the VM gives the same bytes: see
//! `the_vm_builds_the_same_executable`).

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::parser::Parser;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;
use wasmtime::{Engine, Linker, Module as WasmModule, Store};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::WasiCtxBuilder;

const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

/// (name, source). Each program's observable behaviour (output, exit status)
/// must be identical natively and under aipl-run.
const PROGRAMS: &[(&str, &str)] = &[
    // NE5: constants, locals, direct calls, return, drop, proc_exit
    ("main_returns", "(module m (fn main [] -> i32 42))"),
    ("exit_constant", "(module m (fn main [] -> i32 (sys.exit 3) 0))"),
    (
        "locals_and_calls",
        "(module m
           (fn third [a:i32 b:i32 c:i32] -> i32 (let x:i32 a) (set! x b) c)
           (fn main [] -> i32 (let k:i32 5) (sys.exit (call third 1 2 k)) 0))",
    ),
    (
        "every_parameter_position",
        "(module m
           (fn p0 [a:i32 b:i32 c:i32 d:i32 e:i32] -> i32 a)
           (fn p2 [a:i32 b:i32 c:i32 d:i32 e:i32] -> i32 c)
           (fn p4 [a:i32 b:i32 c:i32 d:i32 e:i32] -> i32 e)
           (fn main [] -> i32
             (sys.exit (call p0 (call p2 1 2 (call p4 9 9 9 9 17) 4 5) 2 3 4 5))
             0))",
    ),
    (
        "locals_start_at_zero",
        "(module m
           (fn f [] -> i32 (let a:i32 0) (let b:i32 0) (let c:i32 0) (set! b 11) b)
           (fn g [] -> i32 (let a:i32 0) (let b:i32 0) (let c:i32 0) c)
           (fn main [] -> i32 (let y:i32 (call f)) (let z:i32 (call g)) (sys.exit z) 0))",
    ),
    (
        // the outer call's first arguments sit below the nested call's: if
        // the nested call's arguments stayed on the stack, p0 would read 34
        "arguments_are_popped_after_a_call",
        "(module m
           (fn p0 [a:i32 b:i32 c:i32 d:i32 e:i32] -> i32 a)
           (fn p4 [a:i32 b:i32 c:i32 d:i32 e:i32] -> i32 e)
           (fn main [] -> i32 (sys.exit (call p0 1 2 (call p4 31 32 33 34 35) 4 5)) 0))",
    ),
    (
        "early_return",
        "(module m
           (fn f [x:i32] -> i32 (return x) 99)
           (fn main [] -> i32 (sys.exit (call f 23)) 0))",
    ),
    (
        "void_functions_and_i64_locals",
        "(module m
           (fn nothing [a:i32] -> void (let big:i64 4294967297i64) (let y:i32 a))
           (fn keep [x:i32] -> i32 (call nothing x) (call nothing 2) x)
           (fn main [] -> i32 (sys.exit (call keep 77)) 0))",
    ),
    ("exit_status_125", "(module m (fn main [] -> i32 (sys.exit 125) 0))"),
    // NE6: the i32 cases of tests/test_differential.rs, exit status = low 6 bits
    ("add_wraps", "(module m (fn main [] -> i32 (let x:i32 (+ 2147483647 1)) (sys.exit (bitand (shru x 26) 63)) 0))"),
    ("sub_wraps", "(module m (fn main [] -> i32 (let x:i32 (- -2147483648 1)) (sys.exit (bitand (shru x 26) 63)) 0))"),
    ("shr_is_arithmetic", "(module m (fn main [] -> i32 (sys.exit (bitand (shr -8 1) 63)) 0))"),
    ("shru_is_logical", "(module m (fn main [] -> i32 (sys.exit (shru (shru -8 1) 25)) 0))"),
    ("mul_wraps_to_zero", "(module m (fn main [] -> i32 (sys.exit (* 65536 65536)) 0))"),
    ("div_truncates", "(module m (fn main [] -> i32 (sys.exit (+ 10 (/ -7 2))) 0))"),
    ("rem_follows_dividend", "(module m (fn main [] -> i32 (sys.exit (+ 10 (% -7 2))) 0))"),
    ("divu_unsigned", "(module m (fn main [] -> i32 (sys.exit (shru (divu -1 2) 25)) 0))"),
    ("remu_unsigned", "(module m (fn main [] -> i32 (sys.exit (remu -1 2)) 0))"),
    ("shift_count_masked", "(module m (fn main [] -> i32 (sys.exit (+ (shl 1 33) (bitand (shr -2147483648 32) 1))) 0))"),
    ("min_rem_minus_one", "(module m (fn main [] -> i32 (let z:i32 -1) (sys.exit (% -2147483648 z)) 0))"),
    ("trap_div_by_zero", "(module m (fn main [] -> i32 (let z:i32 0) (sys.exit (/ 7 z)) 0))"),
    ("trap_rem_by_zero", "(module m (fn main [] -> i32 (let z:i32 0) (sys.exit (% 7 z)) 0))"),
    ("trap_divu_by_zero", "(module m (fn main [] -> i32 (let z:i32 0) (sys.exit (divu 7 z)) 0))"),
    ("trap_remu_by_zero", "(module m (fn main [] -> i32 (let z:i32 0) (sys.exit (remu 7 z)) 0))"),
    ("trap_min_div_minus_one", "(module m (fn main [] -> i32 (let z:i32 -1) (sys.exit (/ -2147483648 z)) 0))"),
    ("exit_status_126_is_an_error", "(module m (fn main [] -> i32 (sys.exit 126) 0))"),
    ("exit_status_negative_is_an_error", "(module m (fn main [] -> i32 (sys.exit -1) 0))"),
];

/// Hand-built wasm for cases AIPL source cannot express (e.g. reading a
/// local before writing it): `f` is called by `_start`, and its result is
/// the exit status. Run both ways like `PROGRAMS`.
fn wasm_programs() -> Vec<(&'static str, Vec<u8>)> {
    use wasm_encoder::{BlockType as B, Instruction as I, ValType as V};
    vec![
        // wasm locals start at zero
        ("unwritten_locals_are_zero", exit_with(&[0, 3], &[I::LocalGet(2), I::End])),
        // the result is the stack top, not whatever was last in a register
        ("result_is_the_stack_top", exit_with(&[], &[I::I32Const(7), I::I32Const(9), I::Drop, I::End])),
        // local.tee keeps its value on the stack
        ("tee_keeps_the_value", exit_with(&[1], &[I::I32Const(3), I::I32Const(5), I::LocalTee(0), I::Drop, I::End])),
        ("tee_stores_the_value", exit_with(&[1], &[I::I32Const(6), I::LocalTee(0), I::Drop, I::LocalGet(0), I::End])),
        // i64 constants take a full slot; i32.wrap is not needed to exit with the low half
        ("set_then_get", exit_with(&[2], &[I::I32Const(12), I::LocalSet(1), I::I32Const(13), I::LocalSet(0), I::LocalGet(1), I::End])),
        // NE7: branches drop what is above their label and carry its result
        ("br_carries_a_value", exit_with(&[], &[
            I::Block(B::Result(V::I32)), I::I32Const(1), I::I32Const(2), I::I32Const(42), I::Br(0), I::End, I::End,
        ])),
        ("br_if_taken_carries", exit_with(&[], &[
            I::Block(B::Result(V::I32)), I::I32Const(5), I::I32Const(7), I::I32Const(1), I::BrIf(0), I::Drop, I::End, I::End,
        ])),
        ("br_if_not_taken_falls_through", exit_with(&[], &[
            I::Block(B::Result(V::I32)), I::I32Const(5), I::I32Const(7), I::I32Const(0), I::BrIf(0), I::Drop, I::End, I::End,
        ])),
        ("loop_counts_to_ten", exit_with(&[1], &[
            I::Loop(B::Empty),
            I::LocalGet(0), I::I32Const(1), I::I32Add, I::LocalTee(0), I::I32Const(10), I::I32LtS, I::BrIf(0),
            I::End, I::LocalGet(0), I::End,
        ])),
        // out of two nested blocks, over junk in dead code
        ("br_out_of_nested_blocks", exit_with(&[], &[
            I::Block(B::Result(V::I32)), I::I32Const(1),
            I::Block(B::Empty), I::Block(B::Empty), I::I32Const(3), I::I32Const(9), I::Br(2),
            I::I32Add, I::Drop, I::Unreachable, I::End, I::I32Const(77), I::Drop, I::End,
            I::Drop, I::I32Const(0), I::End, I::End,
        ])),
        ("if_without_else_skips", exit_with(&[1], &[
            I::I32Const(7), I::LocalSet(0), I::I32Const(0), I::If(B::Empty), I::I32Const(1), I::LocalSet(0), I::End, I::LocalGet(0), I::End,
        ])),
        ("if_without_else_runs", exit_with(&[1], &[
            I::I32Const(7), I::LocalSet(0), I::I32Const(1), I::If(B::Empty), I::I32Const(1), I::LocalSet(0), I::End, I::LocalGet(0), I::End,
        ])),
        ("if_else_values", exit_with(&[], &[
            I::I32Const(0), I::If(B::Result(V::I32)), I::I32Const(11), I::Else, I::I32Const(22), I::End,
            I::I32Const(1), I::If(B::Result(V::I32)), I::I32Const(3), I::Else, I::I32Const(4), I::End, I::I32Add, I::End,
        ])),
        // br to the function's own label is a return with its value
        ("br_to_the_function", exit_with(&[], &[
            I::Block(B::Empty), I::I32Const(4), I::I32Const(8), I::Br(1), I::End, I::I32Const(0), I::End,
        ])),
        ("return_from_deep_inside", exit_with(&[], &[
            I::I32Const(1), I::Block(B::Empty), I::Loop(B::Empty), I::I32Const(2), I::I32Const(19), I::Return, I::End, I::End, I::Drop, I::I32Const(0), I::End,
        ])),
        ("unreachable_traps", exit_with(&[], &[I::Unreachable, I::End])),
        // a branch must leave the values below its label intact for what
        // follows: 50 + 42; with locals, 100 + 42 + local 5 - 100 (the frame's locals sit between
        // rbp and the value stack)
        ("br_keeps_values_below_the_block", exit_with(&[], &[
            I::I32Const(50), I::Block(B::Result(V::I32)), I::I32Const(1), I::I32Const(2), I::I32Const(42), I::Br(0), I::End, I::I32Add, I::End,
        ])),
        ("br_keeps_values_below_with_locals", exit_with(&[2], &[
            I::I32Const(5), I::LocalSet(1),
            I::I32Const(100), I::Block(B::Result(V::I32)), I::I32Const(1), I::I32Const(2), I::I32Const(42), I::Br(0), I::End, I::I32Add,
            I::LocalGet(1), I::I32Add, I::I32Const(100), I::I32Sub, I::End,
        ])),
        // after an if's value the depth is one higher: a branch from there
        // must still drop the if's value (50 + 42, not 7 + 42)
        ("br_after_an_if_value", exit_with(&[], &[
            I::I32Const(50), I::Block(B::Result(V::I32)),
            I::I32Const(1), I::If(B::Result(V::I32)), I::I32Const(7), I::Else, I::I32Const(8), I::End,
            I::I32Const(42), I::Br(0), I::End, I::I32Add, I::End,
        ])),
        ("br_if_keeps_values_below", exit_with(&[1], &[
            I::I32Const(9), I::LocalSet(0), I::I32Const(30),
            I::Block(B::Empty), I::I32Const(1), I::I32Const(2), I::I32Const(1), I::BrIf(0), I::Drop, I::Drop, I::End,
            I::LocalGet(0), I::I32Add, I::End,
        ])),
    ]
}

/// A program from another test file: the raw string after `const NAME: &str = r#"`.
fn program_from(file: &str, name: &str) -> String {
    let start = file.find(&format!("const {name}: &str = r#\"")).unwrap_or_else(|| panic!("no {name}"));
    let body = &file[start..];
    let open = body.find("r#\"").unwrap() + 3;
    body[open..open + body[open..].find("\"#;").unwrap()].to_string()
}

/// Functions called natively and checked against wasmtime. `src` minus the
/// functions in `drop` (they need instructions a later task adds) gets a new
/// `main` (any existing one becomes `orig_main`) that makes every call in
/// `calls`, compares each result with wasmtime's for the same call, and
/// exits with the number that differ. Run under aipl-run (which must exit 0:
/// the expected values are right) and natively, as every program is.
fn assert_functions_match(name: &str, src: &str, drop: &[&str], calls: &[(&str, &[i32])]) {
    use aipl_core::ast::Type;
    let mut module = Parser::parse(src).unwrap_or_else(|e| panic!("{name}: {e}"));
    module.functions.retain(|f| !drop.contains(&f.name.as_str()));
    for f in module.functions.iter_mut().filter(|f| f.name == "main") {
        f.name = "orig_main".into();
    }
    TypeChecker::new().check_module(&module).unwrap_or_else(|e| panic!("{name}: {e}"));
    let wasm = WasmCompiler::compile(&module).unwrap();
    let mut checks = String::new();
    for (f, args) in calls {
        let f = if *f == "main" { "orig_main" } else { f };
        let def = module.functions.iter().find(|d| d.name == f).unwrap_or_else(|| panic!("{name}: no function {f}"));
        let want = call_export(&wasm, f, args);
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let call = format!("(call {f} {})", args.join(" "));
        let value = if def.return_type == Type::Bool { format!("(if {call} 1 0)") } else { call };
        checks.push_str(&format!("(if (neq {value} {want}) (set! bad (+ bad 1)) (block))\n"));
    }
    let printed = aipl_core::printer::print_module(&module);
    let end = printed.rfind(')').unwrap();
    let program = format!("{}\n(fn main [] -> i32 (let bad:i32 0)\n{checks}(sys.exit bad) 0))", &printed[..end]);
    let dir = scratch(&format!("{name}_expected"));
    let status = run_wasm(&dir, &to_wasm(&program)).status.code();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(status, Some(0), "{name}: aipl-run disagrees with wasmtime's results");
    assert_native_matches(name, &program);
}

/// Calls export `f` with i32 arguments under wasmtime; its i32 result.
fn call_export(wasm: &[u8], f: &str, args: &[i32]) -> i32 {
    use wasmtime::Val;
    let engine = Engine::default();
    let module = WasmModule::new(&engine, wasm).unwrap();
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
    let mut store = Store::new(&engine, WasiCtxBuilder::new().build_p1());
    let inst = linker.instantiate(&mut store, &module).unwrap();
    let func = inst.get_func(&mut store, f).unwrap();
    let params: Vec<Val> = args.iter().map(|a| Val::I32(*a)).collect();
    let mut out = [Val::I32(0)];
    func.call(&mut store, &params, &mut out).unwrap_or_else(|e| panic!("{f}: {e}"));
    out[0].unwrap_i32()
}

/// NE7: the control-flow programs of tests/test_control_flow.rs and
/// examples/math_core.aipl. Functions that use memory (results, mem.*) wait
/// for NE8.
#[test]
fn control_flow_programs_match_natively() {
    let flow_file = include_str!("test_control_flow.rs");
    assert_functions_match(
        "flow",
        &program_from(flow_file, "PROGRAM"),
        &["parse_digit", "first_non_digit", "bump", "uses_void_return"],
        &[
            ("first_mult7", &[15]), ("first_mult7", &[14]),
            ("sum_until", &[10]), ("sum_until", &[-5]),
            ("sum_odd", &[9]), ("sum_odd", &[0]),
            ("count_nonzero_digits", &[1020304]),
            ("nested", &[4]), ("nested", &[7]),
            ("classify", &[-3]), ("classify", &[0]), ("classify", &[4]), ("classify", &[50]),
            ("bucket_sum", &[7]),
            ("ends_in_return", &[5]),
            ("moving_bounds", &[5]), ("moving_bounds", &[1]),
        ],
    );
    assert_functions_match(
        "short_circuit",
        &program_from(flow_file, "SHORT_CIRCUIT"),
        &["side_effects", "jump_in_operand", "bump"],
        &[
            ("safe_ratio_is_two", &[0]), ("safe_ratio_is_two", &[5]),
            ("zero_or_divides", &[0]), ("zero_or_divides", &[3]),
            ("first_index_over", &[2]), ("first_index_over", &[9]),
            ("in_range", &[5]), ("in_range", &[7]), ("in_range", &[-1]),
        ],
    );
    assert_functions_match(
        "math_core",
        include_str!("../examples/math_core.aipl"),
        &[],
        &[
            ("main", &[]),
            ("gcd", &[48, 18]), ("gcd", &[17, 5]),
            ("isqrt", &[625]), ("isqrt", &[2147395600]), ("isqrt", &[0]),
            ("count_primes", &[1000]),
            ("is_prime", &[97]), ("is_prime", &[91]), ("is_prime", &[2]),
            ("clamp", &[5, 0, 3]), ("clamp", &[-5, 0, 3]),
            ("weighted_average", &[90, 80, 50]),
            ("should_throttle", &[90, 80, 50]), ("should_throttle", &[10, 10, 0]),
        ],
    );
}

/// Edge values for the i32 operator programs.
const EDGES: [i32; 18] = [0, 1, -1, 2, -2, 5, -7, 31, 32, 33, 63, 64, 65536, i32::MAX, i32::MIN, i32::MIN + 1, 123456789, -987654321];

/// One program per i32 operator: it applies the operator to every pair of
/// EDGES (skipping pairs that trap), checks each result against Rust's, and
/// exits with 2 * (a hash of all results mod 32) + (1 if any result
/// differed). Native and aipl-run must agree, and aipl-run must report no
/// difference (which checks the expected values themselves).
fn i32_operator_programs() -> Vec<(String, Vec<u8>)> {
    use wasm_encoder::Instruction as I;
    type Op = (&'static str, I<'static>, fn(i32, i32) -> Option<i32>);
    let cmp = |b: bool| Some(b as i32);
    let _ = cmp;
    let ops: Vec<Op> = vec![
        ("add", I::I32Add, |a, b| Some(a.wrapping_add(b))),
        ("sub", I::I32Sub, |a, b| Some(a.wrapping_sub(b))),
        ("mul", I::I32Mul, |a, b| Some(a.wrapping_mul(b))),
        ("and", I::I32And, |a, b| Some(a & b)),
        ("or", I::I32Or, |a, b| Some(a | b)),
        ("xor", I::I32Xor, |a, b| Some(a ^ b)),
        ("shl", I::I32Shl, |a, b| Some(a.wrapping_shl(b as u32))),
        ("shr_s", I::I32ShrS, |a, b| Some(a.wrapping_shr(b as u32))),
        ("shr_u", I::I32ShrU, |a, b| Some((a as u32).wrapping_shr(b as u32) as i32)),
        ("div_s", I::I32DivS, |a, b| a.checked_div(b)),
        ("div_u", I::I32DivU, |a, b| (a as u32).checked_div(b as u32).map(|v| v as i32)),
        ("rem_s", I::I32RemS, |a, b| if b == 0 { None } else { Some(a.wrapping_rem(b)) }),
        ("rem_u", I::I32RemU, |a, b| (a as u32).checked_rem(b as u32).map(|v| v as i32)),
        ("eq", I::I32Eq, |a, b| Some((a == b) as i32)),
        ("ne", I::I32Ne, |a, b| Some((a != b) as i32)),
        ("lt_s", I::I32LtS, |a, b| Some((a < b) as i32)),
        ("lt_u", I::I32LtU, |a, b| Some(((a as u32) < (b as u32)) as i32)),
        ("gt_s", I::I32GtS, |a, b| Some((a > b) as i32)),
        ("gt_u", I::I32GtU, |a, b| Some(((a as u32) > (b as u32)) as i32)),
        ("le_s", I::I32LeS, |a, b| Some((a <= b) as i32)),
        ("ge_s", I::I32GeS, |a, b| Some((a >= b) as i32)),
    ];
    let mut out = Vec::new();
    for (name, op, expected) in ops {
        let mut body = Vec::new();
        for a in EDGES {
            for b in EDGES {
                let Some(want) = expected(a, b) else { continue };
                body.extend([I::I32Const(a), I::I32Const(b), op.clone()]);
                body.extend(check_result(want));
            }
        }
        body.extend(status_from_checks());
        out.push((format!("i32_{name}"), exit_with(&[3], &body)));
    }
    // the unary ones: eqz, and wrap_i64 of i64 edge values
    let mut eqz = Vec::new();
    for a in EDGES {
        eqz.extend([I::I32Const(a), I::I32Eqz]);
        eqz.extend(check_result((a == 0) as i32));
    }
    eqz.extend(status_from_checks());
    out.push(("i32_eqz".into(), exit_with(&[3], &eqz)));
    let mut wrap = Vec::new();
    for a in [0i64, -1, 1 << 32, (1 << 32) + 5, i64::MAX, i64::MIN, 0x1234_5678_9abc_def0, -2] {
        wrap.extend([I::I64Const(a), I::I32WrapI64]);
        wrap.extend(check_result(a as i32));
    }
    wrap.extend(status_from_checks());
    out.push(("i32_wrap_i64".into(), exit_with(&[3], &wrap)));
    out
}

/// With the result on the stack: local 0 = result; local 1 (hash) =
/// hash * 31 + result; local 2 (differences) += (result != want).
fn check_result(want: i32) -> Vec<wasm_encoder::Instruction<'static>> {
    use wasm_encoder::Instruction as I;
    vec![
        I::LocalSet(0),
        I::LocalGet(1), I::I32Const(31), I::I32Mul, I::LocalGet(0), I::I32Add, I::LocalSet(1),
        I::LocalGet(2), I::LocalGet(0), I::I32Const(want), I::I32Ne, I::I32Add, I::LocalSet(2),
    ]
}

/// 2 * (hash & 31) + (differences != 0), then the end of the function.
fn status_from_checks() -> Vec<wasm_encoder::Instruction<'static>> {
    use wasm_encoder::Instruction as I;
    vec![
        I::LocalGet(1), I::I32Const(31), I::I32And, I::I32Const(1), I::I32Shl,
        I::LocalGet(2), I::I32Const(0), I::I32Ne, I::I32Or, I::End,
    ]
}

/// A module whose `_start` exits with `f()`; `f` has `locals[i]` i32 locals
/// per group and the given body.
fn exit_with(locals: &[u32], body: &[wasm_encoder::Instruction]) -> Vec<u8> {
    use wasm_encoder::{
        CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection, ImportSection, Instruction as I,
        MemorySection, MemoryType, Module, TypeSection, ValType,
    };
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32], []);
    types.ty().function([], []);
    types.ty().function([], [ValType::I32]);
    let mut imports = ImportSection::new();
    imports.import("wasi_snapshot_preview1", "proc_exit", EntityType::Function(0));
    let mut funcs = FunctionSection::new();
    funcs.function(1);
    funcs.function(2);
    let mut memory = MemorySection::new();
    memory.memory(MemoryType { minimum: 1, maximum: None, memory64: false, shared: false, page_size_log2: None });
    let mut exports = ExportSection::new();
    exports.export("_start", ExportKind::Func, 1);
    exports.export("memory", ExportKind::Memory, 0);
    let mut code = CodeSection::new();
    let mut start = Function::new([]);
    start.instruction(&I::Call(2)).instruction(&I::Call(0)).instruction(&I::End);
    code.function(&start);
    let mut f = Function::new(locals.iter().map(|n| (*n, ValType::I32)));
    for ins in body {
        f.instruction(ins);
    }
    code.function(&f);
    let mut m = Module::new();
    m.section(&types).section(&imports).section(&funcs).section(&memory).section(&exports).section(&code);
    m.finish()
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_native_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn to_wasm(src: &str) -> Vec<u8> {
    let m = Parser::parse(src).unwrap_or_else(|e| panic!("{e}"));
    TypeChecker::new().check_module(&m).unwrap_or_else(|e| panic!("{e}"));
    WasmCompiler::compile(&m).unwrap_or_else(|e| panic!("{e}"))
}

fn native_module() -> aipl_core::ast::Module {
    let m = Resolver::resolve(&root().join("aipl_src/native/native.aipl")).unwrap();
    TypeChecker::new().check_module(&m).unwrap();
    m
}

/// native.aipl compiled to wasm, shared by every test (compiled once).
fn native_compiler() -> &'static (Engine, WasmModule) {
    static C: OnceLock<(Engine, WasmModule)> = OnceLock::new();
    C.get_or_init(|| {
        let engine = Engine::default();
        let module = WasmModule::new(&engine, WasmCompiler::compile(&native_module()).unwrap()).unwrap();
        (engine, module)
    })
}

/// native.compile over `wasm`: Ok(executable) or Err(message).
fn to_native(wasm: &[u8]) -> Result<Vec<u8>, String> {
    let (engine, module) = native_compiler();
    let mut linker: Linker<WasiP1Ctx> = Linker::new(engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
    let mut store = Store::new(engine, WasiCtxBuilder::new().build_p1());
    let inst = linker.instantiate(&mut store, module).unwrap();
    let alloc = inst.get_typed_func::<i32, i32>(&mut store, "host_alloc").unwrap();
    let compile = inst.get_typed_func::<(i32, i32), i32>(&mut store, "compile_at").unwrap();
    let mem = inst.get_memory(&mut store, "memory").unwrap();
    let addr = alloc.call(&mut store, wasm.len() as i32 + 8).unwrap() as usize;
    mem.write(&mut store, addr, wasm).unwrap();
    let out = compile.call(&mut store, (addr as i32, wasm.len() as i32)).unwrap() as usize;
    output_of(mem.data(&store), out)
}

/// Reads a native.Output [ok:bool error:(ptr Bytes) exe:(ptr Bytes)] at `out`.
fn output_of(data: &[u8], out: usize) -> Result<Vec<u8>, String> {
    let word = |at: usize| i32::from_le_bytes(data[at..at + 4].try_into().unwrap()) as usize;
    let bytes = |p: usize| data[word(p)..word(p) + word(p + 4)].to_vec();
    if word(out) != 0 {
        Ok(bytes(word(out + 8)))
    } else {
        Err(String::from_utf8(bytes(word(out + 4))).unwrap())
    }
}

/// Both builds run as `<dir>/prog`, so argv[0], which a trap message
/// starts with, is the same string in both.
fn run_wasm(dir: &Path, wasm: &[u8]) -> Output {
    let path = dir.join("prog");
    std::fs::write(&path, wasm).unwrap();
    run_fresh_executable(Command::new(RUNNER).arg(&path).current_dir(dir))
}

fn run_native(dir: &Path, exe: &[u8]) -> Output {
    let path = dir.join("prog");
    std::fs::write(&path, exe).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    run_fresh_executable(Command::new(&path).current_dir(dir))
}

/// How long a test program may run. A miscompiled branch can loop forever;
/// that must fail the test, not hang it.
const TIME_LIMIT: std::time::Duration = std::time::Duration::from_secs(30);

/// Runs an executable this process just wrote, killing it after TIME_LIMIT.
/// Tests run in parallel threads, and a thread that forks a child while the
/// file is still open for writing hands that child the open file until it
/// execs, during which Linux refuses to run the file ("Text file busy").
/// Starting is retried briefly then.
fn run_fresh_executable(cmd: &mut Command) -> Output {
    use std::process::Stdio;
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = None;
    for _ in 0..100 {
        match cmd.spawn() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => std::thread::sleep(std::time::Duration::from_millis(10)),
            r => {
                child = Some(r.unwrap());
                break;
            }
        }
    }
    let child = child.expect("the executable stayed busy for a second");
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(TIME_LIMIT) {
        Ok(out) => out.unwrap(),
        Err(_) => {
            let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();
            panic!("{cmd:?} ran for over {TIME_LIMIT:?} (an infinite loop?) and was killed");
        }
    }
}

/// Builds `src` both ways and requires identical stdout, stderr, and status.
fn assert_native_matches(name: &str, src: &str) {
    assert_wasm_native_matches(name, &to_wasm(src));
}

fn assert_wasm_native_matches(name: &str, wasm: &[u8]) {
    let wasm = wasm.to_vec();
    let exe = to_native(&wasm).unwrap_or_else(|e| panic!("{name}: native compile failed: {e}"));
    let dir = scratch(name);
    let (want, got) = (run_wasm(&dir, &wasm), run_native(&dir, &exe));
    let _ = std::fs::remove_dir_all(&dir);
    let show = |o: &Output| {
        format!("status {:?}, stdout {:?}, stderr {:?}", o.status.code(), String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
    };
    assert!(
        want.status.code() == got.status.code() && want.stdout == got.stdout && want.stderr == got.stderr,
        "{name}: native differs from aipl-run\n  aipl-run: {}\n  native:   {}",
        show(&want),
        show(&got)
    );
}

#[test]
fn every_program_matches_natively() {
    for (name, src) in PROGRAMS {
        assert_native_matches(name, src);
    }
    for (name, wasm) in wasm_programs() {
        assert_wasm_native_matches(name, &wasm);
    }
}

#[test]
fn every_i32_operator_matches_natively() {
    let dir = scratch("i32_expected");
    for (name, wasm) in i32_operator_programs() {
        // the expected values are right: aipl-run sees no difference
        let status = run_wasm(&dir, &wasm).status.code().unwrap();
        assert_eq!(status & 1, 0, "{name}: aipl-run disagrees with the expected values");
        assert_wasm_native_matches(&name, &wasm);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Guards against a harness that passes vacuously: the programs must not all
/// exit 0, and a deliberately different program must be told apart.
#[test]
fn the_harness_tells_programs_apart() {
    let dir = scratch("apart");
    let statuses: Vec<Option<i32>> =
        PROGRAMS.iter().map(|(_, src)| run_native(&dir, &to_native(&to_wasm(src)).unwrap()).status.code()).collect();
    assert!(statuses.iter().filter(|s| **s != Some(0)).count() >= 5, "{statuses:?}");
    // the hand-built programs exit with the statuses their comments promise
    let want = [
        ("unwritten_locals_are_zero", 0), ("result_is_the_stack_top", 7), ("tee_keeps_the_value", 3),
        ("tee_stores_the_value", 6), ("set_then_get", 12), ("br_carries_a_value", 42), ("br_if_taken_carries", 7),
        ("br_if_not_taken_falls_through", 5), ("loop_counts_to_ten", 10), ("br_out_of_nested_blocks", 9),
        ("if_without_else_skips", 7), ("if_without_else_runs", 1), ("if_else_values", 25), ("br_to_the_function", 8),
        ("return_from_deep_inside", 19), ("unreachable_traps", 134), ("br_keeps_values_below_the_block", 92),
        ("br_keeps_values_below_with_locals", 47), ("br_after_an_if_value", 92), ("br_if_keeps_values_below", 39),
    ];
    let programs = wasm_programs();
    assert_eq!(programs.len(), want.len());
    for ((name, wasm), (want_name, status)) in programs.iter().zip(want) {
        assert_eq!(*name, want_name);
        assert_eq!(run_native(&dir, &to_native(wasm).unwrap()).status.code(), Some(status), "{name}");
    }
    let three = to_native(&to_wasm("(module m (fn main [] -> i32 (sys.exit 3) 0))")).unwrap();
    let four = run_wasm(&dir, &to_wasm("(module m (fn main [] -> i32 (sys.exit 4) 0))"));
    assert_ne!(run_native(&dir, &three).status.code(), four.status.code());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The trap programs really trap, natively, with exactly aipl-run's line.
#[test]
fn traps_print_one_line_and_exit_134() {
    let dir = scratch("traps");
    let prog = dir.join("prog").display().to_string();
    for (name, reason) in [
        ("trap_div_by_zero", "wasm trap: integer divide by zero"),
        ("trap_rem_by_zero", "wasm trap: integer divide by zero"),
        ("trap_divu_by_zero", "wasm trap: integer divide by zero"),
        ("trap_remu_by_zero", "wasm trap: integer divide by zero"),
        ("trap_min_div_minus_one", "wasm trap: integer overflow"),
        ("exit_status_126_is_an_error", "exit with invalid exit status outside of [0..126)"),
    ] {
        let src = PROGRAMS.iter().find(|(n, _)| *n == name).unwrap().1;
        let o = run_native(&dir, &to_native(&to_wasm(src)).unwrap());
        assert_eq!(o.status.code(), Some(134), "{name}");
        assert_eq!(String::from_utf8_lossy(&o.stderr), format!("{prog}: {reason}\n"), "{name}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Anything not translated yet is an error naming it, never a wrong program.
#[test]
fn unsupported_instructions_and_imports_are_named() {
    for (src, expected) in [
        ("(module m (fn main [] -> i32 (sys.exit (mem.load32 4096)) 0))", "not supported natively yet: i32.load (opcode 40)"),
        ("(module m (fn main [] -> i32 (sys.print \"hi\") 0))", "import not supported natively yet: wasi_snapshot_preview1.fd_write"),
        ("(module m (fn f [] -> i32 7))", "the module has no _start export"),
    ] {
        assert_eq!(to_native(&to_wasm(src)), Err(expected.to_string()), "{src}");
    }
}

/// The native compiler run in the VM produces the same executable as the
/// compiled one used above.
#[test]
fn the_vm_builds_the_same_executable() {
    let wasm = to_wasm(PROGRAMS[2].1);
    let mut vm = VM::new();
    vm.load_module(native_module());
    let Value::Int(addr) = vm.invoke("host_alloc", vec![Value::Int(wasm.len() as i64 + 8)]).unwrap() else { panic!() };
    vm.write_bytes(addr as usize, &wasm);
    let Value::Int(out) = vm.invoke("compile_at", vec![Value::Int(addr), Value::Int(wasm.len() as i64)]).unwrap() else { panic!() };
    let word = |vm: &mut VM, at: usize| i32::from_le_bytes(vm.read_bytes(at, 4).try_into().unwrap()) as usize;
    assert_ne!(word(&mut vm, out as usize), 0, "the VM build failed");
    let exe_ptr = word(&mut vm, out as usize + 8);
    let (a, n) = (word(&mut vm, exe_ptr), word(&mut vm, exe_ptr + 4));
    assert_eq!(vm.read_bytes(a, n), to_native(&wasm).unwrap());
}
