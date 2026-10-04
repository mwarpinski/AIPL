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
    // NE8: linear memory without allocation (allocation is atomic: NE9)
    ("string_length_from_data", "(module m (fn main [] -> i32 (sys.exit (str.len \"hello\")) 0))"),
    ("string_bytes_from_data", "(module m (fn main [] -> i32 (sys.exit (mem.load8 (+ (str.ptr \"AIPL\") 1))) 0))"),
    ("heap_cursor_from_data", "(module m (fn main [] -> i32 (let a:i32 0) (sys.exit (- (mem.load32 a) 1000)) 0))"),
    ("store_then_load", "(module m (fn main [] -> i32 (mem.store32 8192 77) (mem.store8 8193 1) (sys.exit (- (mem.load32 8192) 256)) 0))"),
    (
        "grow_to_the_cap",
        "(module m (fn main [] -> i32
           (let a:i32 (mem.grow 0)) (let b:i32 (mem.grow 1008)) (let c:i32 (mem.grow 0)) (let d:i32 (mem.grow 1))
           (sys.exit (+ (+ a b) (+ (- c 1000) (* 50 (+ d 1))))) 0))",
    ),
    (
        "top_of_grown_memory",
        "(module m (fn main [] -> i32 (let _g:i32 (mem.grow 1008)) (let top:i32 (- (* 1024 65536) 4))
           (mem.store32 top 9) (sys.exit (mem.load32 top)) 0))",
    ),
    ("last_word_of_initial_memory", "(module m (fn main [] -> i32 (let a:i32 (- (* 16 65536) 4)) (sys.exit (+ 3 (mem.load32 a))) 0))"),
    ("trap_load_past_memory", "(module m (fn main [] -> i32 (let a:i32 (* 16 65536)) (sys.exit (mem.load32 a)) 0))"),
    ("trap_store_past_memory", "(module m (fn main [] -> i32 (let a:i32 (- (* 16 65536) 2)) (mem.store32 a 1) 0))"),
    ("trap_negative_address", "(module m (fn main [] -> i32 (let a:i32 -8) (sys.exit (mem.load8 a)) 0))"),
    // NE9: allocation and atomics
    ("heap_cursor_advances", "(module m (fn main [] -> i32 (let a:i32 (mem.alloc 16)) (let b:i32 (mem.alloc 4)) (sys.exit (- (mem.load32 0) a)) 0))"),
    (
        "lock_round_trip",
        "(module m (fn main [] -> i32 (let m:i32 (mem.alloc 4)) (let d:i32 (mem.alloc 4)) (atomic.lock m) (mem.store32 d 41)
           (let _o:i32 (atomic.add d 1)) (atomic.unlock m) (atomic.lock m) (atomic.unlock m) (sys.exit (mem.load32 d)) 0))",
    ),
    ("trap_unaligned_atomic", "(module m (fn main [] -> i32 (let p:i32 (mem.alloc 8)) (sys.exit (atomic.add (+ p 2) 1)) 0))"),
    ("trap_unaligned_before_bounds", "(module m (fn main [] -> i32 (let p:i32 (mem.alloc 8)) (sys.exit (atomic.add (+ p 1000000002) 1)) 0))"),
    ("trap_atomic_past_memory", "(module m (fn main [] -> i32 (let p:i32 (mem.alloc 8)) (sys.exit (atomic.add (+ p 1000000000) 1)) 0))"),
    ("trap_unlock_a_free_lock", "(module m (fn main [] -> i32 (let l:i32 (mem.alloc 4)) (atomic.unlock l) 0))"),
    ("trap_lock_twice_waits", "(module m (fn main [] -> i32 (let l:i32 (mem.alloc 4)) (atomic.lock l) (atomic.lock l) 0))"),
    ("trap_lock_a_non_lock_word", "(module m (fn main [] -> i32 (let p:i32 (mem.alloc 4)) (mem.store32 p 1024) (atomic.lock p) 0))"),
    ("trap_store_into_reserved_block", "(module m (fn main [] -> i32 (let a:i32 (* 8 64)) (mem.store32 a 7) 0))"),
    ("exit_status_126_is_an_error", "(module m (fn main [] -> i32 (sys.exit 126) 0))"),
    ("exit_status_negative_is_an_error", "(module m (fn main [] -> i32 (sys.exit -1) 0))"),
];

/// Hand-built wasm for cases AIPL source cannot express (e.g. reading a
/// local before writing it): `f` is called by `_start`, and its result is
/// the exit status. Run both ways like `PROGRAMS`.
fn wasm_programs() -> Vec<(&'static str, Vec<u8>)> {
    use wasm_encoder::{BlockType as B, Instruction as I, MemArg, ValType as V};
    let m = |align, offset| MemArg { offset, align, memory_index: 0 };
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
        // NE12: an i32 made by wrapping keeps the i64's high half in its slot;
        // extending it must set that half from the i32 alone
        ("extend_after_wrap", exit_with(&[], &[
            I::I64Const(0x7FFF_FFFF_0000_0005u64 as i64), I::I32WrapI64, I::I64ExtendI32U, I::I64Const(5), I::I64Eq,
            I::I64Const(-0x0000_0001_0000_0000), I::I32WrapI64, I::I64ExtendI32S, I::I64Const(0), I::I64Eq, I::I32Add,
            I::I64Const(0x1234_5678_FFFF_FFF9u64 as i64), I::I32WrapI64, I::I64ExtendI32S, I::I64Const(-7), I::I64Eq, I::I32Add, I::End,
        ])),
        // NE8: memory (one page initially, at most four)
        ("store_i64_load_bytes", exit_with(&[], &[
            I::I32Const(100), I::I64Const(0x0102_0304_0506_0708), I::I64Store(m(3, 8)),
            I::I32Const(108), I::I32Load8U(m(0, 0)), I::I32Const(100), I::I32Load8U(m(0, 15)), I::I32Const(10), I::I32Mul, I::I32Add, I::End,
        ])),
        ("store8_truncates", exit_with(&[], &[
            I::I32Const(200), I::I32Const(0x150), I::I32Store8(m(0, 0)), I::I32Const(200), I::I32Load(m(2, 0)), I::End,
        ])),
        ("last_valid_word", exit_with(&[], &[I::I32Const(65532), I::I32Load(m(2, 0)), I::I32Const(3), I::I32Add, I::End])),
        ("trap_straddling_the_end", exit_with(&[], &[I::I32Const(65533), I::I32Load(m(2, 0)), I::End])),
        ("trap_offset_does_not_wrap", exit_with(&[], &[I::I32Const(-1), I::I32Load(m(2, 4)), I::End])),
        ("trap_huge_offset", exit_with(&[], &[I::I32Const(0), I::I32Load(m(2, 0x8000_0000)), I::End])),
        // the address alone is in bounds; with the offset it is not
        ("trap_offset_past_the_end", exit_with(&[], &[I::I32Const(65528), I::I32Load(m(2, 8)), I::End])),
        // NE9: an atomic store writes 4 bytes too
        ("atomic_store_writes_four_bytes", exit_with(&[], &[
            I::I32Const(44), I::I32Const(9), I::I32Store(m(2, 0)),
            I::I32Const(40), I::I32Const(7), I::I32AtomicStore(m(2, 0)),
            I::I32Const(44), I::I32Load(m(2, 0)), I::I32Const(40), I::I32AtomicLoad(m(2, 0)), I::I32Const(10), I::I32Mul, I::I32Add, I::End,
        ])),
        // a 4-byte store leaves the next 4 bytes alone
        ("f32_store_writes_four_bytes", exit_with(&[], &[
            I::I32Const(44), I::I32Const(9), I::I32Store(m(2, 0)),
            I::I32Const(32), I::I32Const(0x4049_0FDB), I::I32Store(m(2, 0)),
            I::I32Const(40), I::I32Const(32), I::F32Load(m(2, 0)), I::F32Store(m(2, 0)),
            I::I32Const(44), I::I32Load(m(2, 0)), I::End,
        ])),
        // an i32 address made by wrapping an i64: the high half is not part of it
        ("address_ignores_high_bits", exit_with(&[], &[
            I::I32Const(16), I::I32Const(7), I::I32Store(m(2, 0)),
            I::I64Const(0x1_0000_0010), I::I32WrapI64, I::I32Load(m(2, 0)), I::End,
        ])),
        ("grow_stops_at_the_maximum", exit_with(&[], &[
            I::I32Const(3), I::MemoryGrow(0), I::I32Const(1), I::MemoryGrow(0), I::I32Add, I::MemorySize(0), I::I32Add, I::End,
        ])),
        ("grown_pages_are_zero_and_usable", exit_with(&[], &[
            I::I32Const(1), I::MemoryGrow(0), I::Drop,
            I::I32Const(65544), I::I32Const(5), I::I32Store(m(2, 0)),
            I::I32Const(65540), I::I32Load(m(2, 0)), I::I32Const(65544), I::I32Load(m(2, 0)), I::I32Add, I::End,
        ])),
        ("trap_past_the_grown_size", exit_with(&[], &[
            I::I32Const(1), I::MemoryGrow(0), I::Drop, I::I32Const(131072), I::I32Load8U(m(0, 0)), I::End,
        ])),
        // f32/f64 loads and stores move bits
        ("float_bits_round_trip", exit_with(&[], &[
            I::I32Const(64), I::F64Const(1.5f64.into()), I::F64Store(m(3, 0)), I::I32Const(68), I::I32Load(m(2, 0)), I::I32Const(0x3FF8_0000), I::I32Eq,
            I::I32Const(32), I::I32Const(0x4049_0FDB), I::I32Store(m(2, 0)),
            I::I32Const(40), I::I32Const(32), I::F32Load(m(2, 0)), I::F32Store(m(2, 0)),
            I::I32Const(40), I::I32Load(m(2, 0)), I::I32Const(0x4049_0FDB), I::I32Eq, I::I32Const(2), I::I32Mul, I::I32Add, I::End,
        ])),
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

/// A program from another test file: the first raw string after `marker`
/// (e.g. `const PROGRAM` or `fn p8_structs_and_arrays`).
fn program_from(file: &str, marker: &str) -> String {
    let start = file.find(marker).unwrap_or_else(|| panic!("no {marker}"));
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
    // the calls run in order in one instance, as in the generated main, since
    // results such as heap addresses depend on what ran before
    let calls: Vec<(&str, &[i32])> = calls.iter().map(|(f, a)| (if *f == "main" { "orig_main" } else { *f }, *a)).collect();
    let wants = call_exports(&wasm, &calls);
    let mut checks = String::new();
    for ((f, args), want) in calls.iter().zip(wants) {
        let def = module.functions.iter().find(|d| d.name == *f).unwrap_or_else(|| panic!("{name}: no function {f}"));
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let call = format!("(call {f} {})", args.join(" "));
        // floats compare by their bits, so NaN and -0.0 are checked exactly
        let (value, want) = match (&def.return_type, want) {
            (Type::Bool, wasmtime::Val::I32(v)) => (format!("(if {call} 1 0)"), v.to_string()),
            (_, wasmtime::Val::I32(v)) => (call, v.to_string()),
            (_, wasmtime::Val::I64(v)) => (call, format!("{v}i64")),
            (_, wasmtime::Val::F64(bits)) => (format!("(i64.reinterpret_f64 {call})"), format!("{}i64", bits as i64)),
            (t, other) => panic!("{name}: {f} returns {t:?}, which the checker does not compare ({other:?})"),
        };
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

/// Calls exports with i32 arguments, in order, in one wasmtime instance
/// with aipl-run's preopens (a scratch working directory as fd 3, `/` as
/// fd 4); their results (a bool as i32 0 or 1).
fn call_exports(wasm: &[u8], calls: &[(&str, &[i32])]) -> Vec<wasmtime::Val> {
    use wasmtime::Val;
    use wasmtime_wasi::FsPerms;
    let engine = Engine::default();
    let module = WasmModule::new(&engine, wasm).unwrap();
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
    let dir = scratch(&format!("exports_{}", calls.len()));
    let ctx = WasiCtxBuilder::new()
        .preopened_dir(&dir, ".", FsPerms::ReadWrite)
        .unwrap()
        .preopened_dir("/", "/", FsPerms::ReadWrite)
        .unwrap()
        .build_p1();
    let mut store = Store::new(&engine, ctx);
    let inst = linker.instantiate(&mut store, &module).unwrap();
    calls
        .iter()
        .map(|(f, args)| {
            let func = inst.get_func(&mut store, f).unwrap();
            let params: Vec<Val> = args.iter().map(|a| Val::I32(*a)).collect();
            let result = func.ty(&store).results().next().unwrap_or_else(|| panic!("{f} returns nothing"));
            let mut out = [Val::default_for_ty(&result).unwrap()];
            func.call(&mut store, &params, &mut out).unwrap_or_else(|e| panic!("{f}: {e}"));
            out[0].clone()
        })
        .collect()
}

/// NE7: the control-flow programs of tests/test_control_flow.rs and
/// examples/math_core.aipl (the functions that allocate since NE9).
#[test]
fn control_flow_programs_match_natively() {
    let flow_file = include_str!("test_control_flow.rs");
    assert_functions_match(
        "flow",
        &program_from(flow_file, "const PROGRAM"),
        &[],
        &[
            ("first_non_digit", &[5]), ("first_non_digit", &[-1]), ("uses_void_return", &[41]),
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
        &program_from(flow_file, "const SHORT_CIRCUIT"),
        &[],
        &[
            ("side_effects", &[10]), ("jump_in_operand", &[9]), ("jump_in_operand", &[60]),
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

/// NE9: programs that allocate (every allocation is an atomic add on the
/// heap cursor): the single-threaded atomics program, the struct and array
/// cases, and the quicksort and accounts examples.
#[test]
fn allocating_programs_match_natively() {
    assert_functions_match(
        "atomics",
        &program_from(include_str!("test_threads.rs"), "const ATOMICS"),
        // these trap; they are PROGRAMS of their own
        &["unaligned_add", "bad_unlock"],
        &[("add_returns_previous", &[]), ("cas", &[]), ("lock_unlock", &[])],
    );
    assert_functions_match(
        "structs_and_arrays",
        &program_from(include_str!("test_differential.rs"), "fn p8_structs_and_arrays"),
        // i64 arithmetic (NE12) and float comparison (NE13); the reserved-block
        // stores trap and are PROGRAMS of their own
        &["test_floats", "test_put_reserved", "test_arr_set_reserved"],
        &[
            ("test_point_ops", &[3, 4]), ("test_sizeof", &[]), ("test_mixed", &[3]), ("test_mixed", &[-2]), ("test_array_ops", &[5]), ("test_array_ops", &[0]),
            ("test_i64_array", &[3]), ("test_index_oob", &[1]), ("test_index_oob", &[7]), ("test_size_allocates", &[]),
            ("test_result_heap", &[]), ("test_result_payload_allocates", &[]), ("test_points", &[]),
            ("test_bool_word", &[]), ("test_str_field", &[]),
        ],
    );
    assert_functions_match(
        "quicksort",
        include_str!("../examples/quicksort.aipl"),
        &[],
        &[
            ("main", &[]), ("median3", &[1, 5, 3]), ("median3", &[9, 2, 4]),
            ("sorted_median", &[1000, 42]), ("sorted_median", &[1, 7]), ("handles_sorted_and_equal", &[500]),
        ],
    );
    assert_functions_match(
        "accounts",
        include_str!("../examples/accounts.aipl"),
        &[],
        &[("main", &[]), ("frozen_is_atomic", &[]), ("simulate", &[100, 7, 20]), ("simulate", &[10, 5, 3])],
    );
}

/// NE10: the WASI functions other than files. Every program prints, so its
/// whole output is compared; the environment is fixed for both runs.
#[test]
fn wasi_programs_match_natively() {
    let env = |vars: &[(&str, &str)]| Some(vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect());
    let big_input: Vec<u8> = (0..20000).map(|i| format!("word{i} ")).collect::<String>().into_bytes();
    let cases: Vec<(&str, String, Io)> = vec![
        ("print_lines", "(module m (fn main [] -> i32 (sys.print \"hello\" \"native world\") 0))".into(), Io::default()),
        (
            "print_numbers",
            "(module m (import io) (fn main [] -> i32 (call io.println_int \"n: \" -2147483648) (call io.println_int \"m: \" 42) (call io.print_int 7) (call io.println \"\") 0))".into(),
            Io::default(),
        ),
        ("raw_write_to_fd_1", "(module m (fn main [] -> i32 (sys.exit (fs.write 1 (str.ptr \"raw\\n\") 4)) 0))".into(), Io::default()),
        ("write_to_stderr", "(module m (fn main [] -> i32 (sys.exit (fs.write 2 (str.ptr \"err\\n\") 4)) 0))".into(), Io::default()),
        ("write_to_a_bad_fd", "(module m (fn main [] -> i32 (sys.exit (+ 10 (fs.write 9 (str.ptr \"x\") 1))) 0))".into(), Io::default()),
        ("write_to_a_huge_fd", "(module m (fn main [] -> i32 (sys.exit (+ 10 (fs.write 70000 (str.ptr \"x\") 1))) 0))".into(), Io::default()),
        (
            "close_stderr_then_write",
            "(module m (fn main [] -> i32 (let r:i32 (fs.close 2)) (sys.exit (+ (* 10 (+ r 1)) (+ 2 (fs.write 2 (str.ptr \"x\") 1)))) 0))".into(),
            Io::default(),
        ),
        (
            "stdin_words",
            "(module m (import io) (import str) (fn main [] -> i32 (call io.println_int \"words: \" (call str.count_words (call io.read_stdin))) 0))".into(),
            Io { stdin: b"one two\nthree four five\n".to_vec(), ..Io::default() },
        ),
        (
            "stdin_large",
            "(module m (import io) (import str) (fn main [] -> i32 (let b:(ptr str.Bytes) (call io.read_stdin)) (call io.println_int \"bytes: \" (get b str.Bytes.len)) (call io.println_int \"words: \" (call str.count_words b)) 0))".into(),
            Io { stdin: big_input.clone(), ..Io::default() },
        ),
        (
            "stdin_empty",
            "(module m (import io) (import str) (fn main [] -> i32 (call io.println_int \"bytes: \" (get (call io.read_stdin) str.Bytes.len)) 0))".into(),
            Io::default(),
        ),
        (
            "command_line_and_environment",
            "(module m (import io) (import os) (import str)
               (fn main [] -> i32
                 (call io.println_int \"argc: \" (call os.arg_count))
                 (loop i 0 (- (call os.arg_count) 1) 1 (call io.println_int \"len: \" (get (call os.arg i) str.Bytes.len)))
                 (call io.println_int \"byte: \" (call str.byte_at (call os.arg 2) 1))
                 (call io.println_int \"missing: \" (get (call os.arg 9) str.Bytes.len))
                 (call io.println_int \"probe: \" (match_result (call str.parse_int (call os.env \"AIPL_PROBE\")) (ok v v) (err e -1000)))
                 (call io.println_int \"prefix: \" (get (call os.env \"AIPL_PRO\") str.Bytes.len))
                 (call io.println_int \"empty: \" (get (call os.env \"AIPL_EMPTY\") str.Bytes.len))
                 (call io.println_int \"unset: \" (get (call os.env \"AIPL_UNSET\") str.Bytes.len))
                 0))".into(),
            Io {
                args: vec!["hello".into(), "a b".into(), "".into(), "three".into()],
                env: env(&[("AIPL_PROBE", "-42"), ("AIPL_PROBE_LONGER", "1"), ("AIPL_EMPTY", "")]),
                ..Io::default()
            },
        ),
        (
            "no_environment",
            "(module m (import io) (import os) (import str) (fn main [] -> i32 (call io.println_int \"home: \" (get (call os.env \"HOME\") str.Bytes.len)) 0))".into(),
            Io { env: env(&[]), ..Io::default() },
        ),
        (
            "clocks",
            // compared through their 32-bit halves (i64 comparisons are NE12):
            // 2020-01-01 in nanoseconds has high word 367368757
            "(module m (import io) (fn main [] -> i32
               (let p:i32 (mem.alloc 32))
               (mem.store64 p (sys.monotonic)) (mem.store64 (+ p 8) (sys.monotonic)) (mem.store64 (+ p 16) (sys.time))
               (let hi0:i32 (mem.load32 (+ p 4))) (let hi1:i32 (mem.load32 (+ p 12)))
               (call io.println_int \"after 2020: \" (if (gt (mem.load32 (+ p 20)) 367368757) 1 0))
               (call io.println_int \"monotonic: \" (if (or (gt hi1 hi0) (and (eq hi1 hi0) (gte (- (mem.load32 (+ p 8)) (mem.load32 p)) 0))) 1 0))
               0))".into(),
            Io::default(),
        ),
        (
            "random",
            "(module m (import io) (import os) (fn main [] -> i32
               (let p:i32 (mem.alloc 4096)) (let ok:i32 (sys.random p 4096))
               (let nonzero:i32 0)
               (loop i 0 4095 1 (if (neq (mem.load8 (+ p i)) 0) (set! nonzero (+ nonzero 1)) (block)))
               (call io.println_int \"result: \" ok)
               (call io.println_int \"mostly nonzero: \" (if (gt nonzero 3900) 1 0))
               (call io.println_int \"differ: \" (if (neq (call os.random_i32) (call os.random_i32)) 1 0))
               0))".into(),
            Io::default(),
        ),
        // the raw counts and sizes (each string counts its NUL)
        (
            "raw_argument_and_environment_sizes",
            "(module m (import io) (fn main [] -> i32 (let p:i32 (mem.alloc 16))
               (let a:i32 (args.sizes p (+ p 4))) (call io.println_int \"argc: \" (mem.load32 p)) (call io.println_int \"argv bytes: \" (mem.load32 (+ p 4)))
               (let e:i32 (env.sizes (+ p 8) (+ p 12))) (call io.println_int \"envc: \" (mem.load32 (+ p 8))) (call io.println_int \"env bytes: \" (mem.load32 (+ p 12)))
               0))".into(),
            Io { args: vec!["x".into(), "".into(), "yz".into()], env: env(&[("A", "1"), ("BB", "")]), ..Io::default() },
        ),
        // a buffer ending exactly at the end of memory is fine
        ("random_up_to_the_end", "(module m (fn main [] -> i32 (sys.exit (+ 10 (sys.random (- (* 16 65536) 8) 8))) 0))".into(), Io::default()),
        // bad pointers trap with wasmtime's words, start and length included
        ("trap_random_out_of_bounds", "(module m (fn main [] -> i32 (sys.exit (sys.random 2000000000 8)) 0))".into(), Io::default()),
        ("trap_random_straddles_the_end", "(module m (fn main [] -> i32 (sys.exit (sys.random (- (* 16 65536) 4) 8)) 0))".into(), Io::default()),
        ("trap_write_buffer_out_of_bounds", "(module m (fn main [] -> i32 (sys.exit (fs.write 1 2000000000 4)) 0))".into(), Io::default()),
        // wasmtime reads first: nothing read is no error; otherwise the region is what was read
        ("read_outside_memory_with_no_input", "(module m (fn main [] -> i32 (sys.exit (+ 10 (fs.read 0 4294967295 4))) 0))".into(), Io::default()),
        (
            "trap_read_outside_memory",
            "(module m (fn main [] -> i32 (sys.exit (+ 10 (fs.read 0 4294967290 100))) 0))".into(),
            Io { stdin: b"abcdefgh".to_vec(), ..Io::default() },
        ),
        (
            "trap_read_straddling_the_end",
            "(module m (fn main [] -> i32 (sys.exit (+ 10 (fs.read 0 (- (* 16 65536) 2) 100))) 0))".into(),
            Io { stdin: b"abcdefgh".to_vec(), ..Io::default() },
        ),
        ("trap_args_misaligned", "(module m (fn main [] -> i32 (let p:i32 (mem.alloc 16)) (sys.exit (args.sizes (+ p 1) (+ p 8))) 0))".into(), Io::default()),
        ("trap_args_size_written_first", "(module m (fn main [] -> i32 (let p:i32 (mem.alloc 16)) (sys.exit (args.sizes 2000000000 (+ p 2))) 0))".into(), Io::default()),
        ("trap_env_bounds_before_alignment", "(module m (fn main [] -> i32 (sys.exit (env.sizes 8192 2000000001)) 0))".into(), Io::default()),
    ];
    for (name, src, io) in cases {
        assert_wasm_native_matches_with(name, &to_wasm(&src), &io);
    }
}

/// NE11: files, and the access rules: relative paths stay beneath the
/// working directory (no `..` or symlink escapes), absolute paths work
/// unless sandboxed. After each run every file under the run's parent
/// directory is compared, so an escape would show.
#[test]
fn file_programs_match_natively() {
    let input = include_bytes!("../examples/input.txt").to_vec();
    let file = |name: &str, bytes: &[u8]| (name.to_string(), bytes.to_vec());
    let with_input = |args: &[&str], sandbox: bool| Io {
        args: args.iter().map(|a| a.to_string()).collect(),
        files: vec![file("input.txt", &input)],
        sandbox,
        ..Io::default()
    };
    // the run directory both builds use (see assert_wasm_native_matches_with)
    let run_dir = |name: &str| scratch_path(name).join("run").display().to_string();
    let word_count = include_str!("../examples/word_count.aipl");
    let probe = |body: &str| format!("(module m (import io) (import str) (fn main [] -> i32 {body} 0))");
    let open_report = |path: &str, write: bool| {
        format!(
            "(call io.println_int \"{path}: \" (fs.open (str.ptr \"{path}\") {} {}))",
            path.len(),
            if write { 1 } else { 0 }
        )
    };
    let abs_name = "absolute_paths";
    let abs = format!("{}/abs.txt", run_dir(abs_name));
    let abs_boxed_name = "absolute_paths_sandboxed";
    let abs_boxed = format!("{}/abs.txt", run_dir(abs_boxed_name));
    let write_read = |path: &str| {
        probe(&format!(
            "(call io.println_int \"wrote: \" (call io.write_path (call str.from_str \"{path}\") (call str.from_str \"absolute ok\")))
             (call io.println_int \"read: \" (get (call io.read_path (call str.from_str \"{path}\")) str.Bytes.len))"
        ))
    };
    let big: Vec<u8> = (0..30000).map(|i| format!("w{} ", i % 977)).collect::<String>().into_bytes();
    let cases: Vec<(&str, String, Io)> = vec![
        ("word_count", word_count.into(), with_input(&[], false)),
        ("word_count_named_file", word_count.into(), with_input(&["input.txt"], false)),
        ("word_count_missing_file", word_count.into(), with_input(&["nope.txt"], false)),
        ("word_count_big_file", word_count.into(), Io { files: vec![file("input.txt", &big)], ..Io::default() }),
        // (word_freq sorts with a comparator reference: NE14)
        ("word_count_sandboxed", word_count.into(), with_input(&["input.txt"], true)),
        (
            "write_then_read",
            probe(
                "(call io.println_int \"wrote: \" (call io.write_file \"out.txt\" (call str.from_str \"line one\\nline two\\n\")))
                 (call io.println_int \"lines: \" (call str.count_lines (call io.read_file \"out.txt\")))
                 (call io.println_int \"again: \" (call io.write_file \"out.txt\" (call str.from_str \"short\")))",
            ),
            Io::default(),
        ),
        (
            "open_missing_and_subdirectories",
            probe(&[open_report("nope.txt", false), open_report("sub/inner.txt", false), open_report("./data.txt", false),
                    open_report("sub/../data.txt", false), open_report("nodir/new.txt", true), open_report("", false)].concat()),
            Io { files: vec![file("data.txt", b"d"), file("sub/inner.txt", b"i")], ..Io::default() },
        ),
        (
            "escapes_are_refused",
            probe(&[open_report("../escaped.txt", true), open_report("sub/../../escaped.txt", true), open_report("out/escaped.txt", true),
                    open_report("out/run/data.txt", false), open_report("/etc/hostname", false)].concat()),
            Io { files: vec![file("data.txt", b"d"), file("sub/x.txt", b"x")], symlinks: vec![("out".into(), "..".into())], ..Io::default() },
        ),
        (
            "symlinks_inside",
            probe(&[open_report("alias.txt", false), open_report("subalias/inner.txt", false)].concat()),
            Io {
                files: vec![file("data.txt", b"d"), file("sub/inner.txt", b"i")],
                symlinks: vec![("alias.txt".into(), "data.txt".into()), ("subalias".into(), "sub".into())],
                ..Io::default()
            },
        ),
        (
            "delete",
            probe(
                "(call io.println_int \"a: \" (fs.delete (str.ptr \"a.txt\") 5))
                 (call io.println_int \"again: \" (fs.delete (str.ptr \"a.txt\") 5))
                 (call io.println_int \"nested: \" (fs.delete (str.ptr \"sub/b.txt\") 9))
                 (call io.println_int \"directory: \" (fs.delete (str.ptr \"sub\") 3))
                 (call io.println_int \"escape: \" (fs.delete (str.ptr \"../run/c.txt\") 12))
                 (call io.println_int \"symlink: \" (fs.delete (str.ptr \"out/run/c.txt\") 13))
                 (call io.println_int \"link itself: \" (fs.delete (str.ptr \"alias.txt\") 9))",
            ),
            Io {
                files: vec![file("a.txt", b"a"), file("sub/b.txt", b"b"), file("c.txt", b"c")],
                symlinks: vec![("out".into(), "..".into()), ("alias.txt".into(), "c.txt".into())],
                ..Io::default()
            },
        ),
        (
            "read_only_descriptor",
            probe(
                "(let fd:i32 (fs.open (str.ptr \"data.txt\") 8 0))
                 (call io.println_int \"write: \" (fs.write fd (str.ptr \"x\") 1))
                 (call io.println_int \"close: \" (fs.close fd))
                 (call io.println_int \"closed: \" (fs.close fd))",
            ),
            Io { files: vec![file("data.txt", b"data")], ..Io::default() },
        ),
        (
            "descriptor_numbers",
            probe(
                "(let a:i32 (fs.open (str.ptr \"a.txt\") 5 1)) (let b:i32 (fs.open (str.ptr \"b.txt\") 5 1))
                 (call io.println_int \"a: \" a) (call io.println_int \"b: \" b)
                 (call io.println_int \"close a: \" (fs.close a))
                 (call io.println_int \"c: \" (fs.open (str.ptr \"c.txt\") 5 1))
                 (call io.println_int \"close 0: \" (fs.close 0))
                 (call io.println_int \"d: \" (fs.open (str.ptr \"d.txt\") 5 1))
                 (call io.println_int \"close 3: \" (fs.close 3))
                 (call io.println_int \"e: \" (fs.open (str.ptr \"e.txt\") 5 1))",
            ),
            Io::default(),
        ),
        (abs_name, write_read(&abs), Io::default()),
        (abs_boxed_name, write_read(&abs_boxed), Io { sandbox: true, ..Io::default() }),
    ];
    for (name, src, io) in cases.iter() {
        assert_wasm_native_matches_with(name, &to_wasm(src), io);
    }
    // aipl_src/file_io.aipl's self-test (a real file round trip)
    assert_functions_match("file_io", include_str!("../aipl_src/file_io.aipl"), &[], &[("run_file_io_tests", &[])]);
    // agreeing is not enough: every escape must actually be refused
    let (_, src, io) = cases.iter().find(|(n, _, _)| *n == "escapes_are_refused").unwrap();
    // (the last path, /etc/hostname, is absolute: allowed unless sandboxed)
    for sandbox in [false, true] {
        let io = Io { sandbox, ..io.clone() };
        let root = scratch("escapes_checked");
        let dir = prepare(&root, &io);
        let out = run_native_with(&dir, &to_native_with(&to_wasm(src), sandbox).unwrap(), &io);
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 5, "{text}");
        assert!(lines[..4].iter().all(|l| l.ends_with(": -1")), "an escape was allowed:\n{text}");
        assert_eq!(lines[4].ends_with(": -1"), sandbox, "absolute path, sandbox {sandbox}:\n{text}");
        assert!(!root.join("escaped.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// NE12: i64. The cases of tests/test_i64.rs, function by function.
#[test]
fn i64_programs_match_natively() {
    assert_functions_match(
        "i64_cases",
        "(module m
           (fn mul_wraps [] -> i64 (* 4294967296i64 2147483648i64))
           (fn add_overflows [] -> i64 (+ 9223372036854775807i64 1i64))
           (fn past_32_bits [] -> i64 (+ 2147483647i64 1i64))
           (fn div [] -> i64 (/ -7i64 2i64))
           (fn rem [] -> i64 (% -7i64 2i64))
           (fn divu [] -> i64 (divu -1i64 2i64))
           (fn remu [] -> i64 (remu -1i64 2i64))
           (fn shl_masked [] -> i64 (shl 1i64 65i64))
           (fn shr_arith [] -> i64 (shr -8i64 1i64))
           (fn shru_logical [] -> i64 (shru -8i64 1i64))
           (fn lt_neg [] -> bool (lt -1i64 0i64))
           (fn eq_five [] -> bool (eq 5i64 5i64))
           (fn big_compare [] -> bool (gt 4294967296i64 4294967295i64))
           (fn extend_s [] -> i64 (i64.extend_s -1))
           (fn extend_u [] -> i64 (i64.extend_u -1))
           (fn wrap [] -> i32 (i32.wrap 4294967301i64))
           (fn wrap_sum [] -> i32 (i32.wrap (+ (i64.extend_s 2147483647) 1i64)))
           (fn memory_round_trip [] -> i64 (let p:i32 (mem.alloc 8)) (mem.store64 p 1311768467294899696i64) (mem.load64 p))
           (fn add64 [a:i64 b:i64] -> i64 (+ a b))
           (fn calls [] -> i64 (call add64 4294967296i64 4294967296i64))
           (fn accumulate [] -> i64 (let s:i64 0i64) (loop i 1 10 1 (set! s (+ s 1000000000i64))) s)
           (fn if_branches [n:i32] -> i64 (if (gt n 0) 4294967296i64 -4294967296i64))
           (fn param_order [a:i64 b:i64 c:i32] -> i64 (- (* a 3i64) (+ b (i64.extend_s c))))
           (fn uses_param_order [] -> i64 (call param_order 5000000000i64 7i64 -3)))",
        &[],
        &[
            ("mul_wraps", &[]), ("add_overflows", &[]), ("past_32_bits", &[]), ("div", &[]), ("rem", &[]), ("divu", &[]),
            ("remu", &[]), ("shl_masked", &[]), ("shr_arith", &[]), ("shru_logical", &[]), ("lt_neg", &[]), ("eq_five", &[]),
            ("big_compare", &[]), ("extend_s", &[]), ("extend_u", &[]), ("wrap", &[]), ("wrap_sum", &[]),
            ("memory_round_trip", &[]), ("calls", &[]), ("accumulate", &[]), ("if_branches", &[1]), ("if_branches", &[-1]),
            ("uses_param_order", &[]),
        ],
    );
    for (name, src) in [
        ("trap_i64_div_by_zero", "(module m (fn main [] -> i32 (let z:i64 0i64) (sys.exit (i32.wrap (/ 7i64 z))) 0))"),
        ("trap_i64_rem_by_zero", "(module m (fn main [] -> i32 (let z:i64 0i64) (sys.exit (i32.wrap (% 7i64 z))) 0))"),
        ("trap_i64_divu_by_zero", "(module m (fn main [] -> i32 (let z:i64 0i64) (sys.exit (i32.wrap (divu 7i64 z))) 0))"),
        ("trap_i64_min_div_minus_one", "(module m (fn main [] -> i32 (let z:i64 -1i64) (sys.exit (i32.wrap (/ -9223372036854775808i64 z))) 0))"),
        ("i64_min_rem_minus_one", "(module m (fn main [] -> i32 (let z:i64 -1i64) (sys.exit (i32.wrap (% -9223372036854775808i64 z))) 0))"),
        // the NE10 clock check with whole i64 values
        (
            "clocks_compared_as_i64",
            "(module m (import io) (fn main [] -> i32
               (let t0:i64 (sys.monotonic)) (let t1:i64 (sys.monotonic))
               (call io.println_int \"after 2020: \" (if (gt (sys.time) 1577836800000000000i64) 1 0))
               (call io.println_int \"monotonic: \" (if (gte t1 t0) 1 0))
               (call io.println_int \"microseconds apart, not seconds: \" (if (lt (- t1 t0) 1000000000i64) 1 0))
               0))",
        ),
    ] {
        assert_wasm_native_matches(name, &to_wasm(src));
    }
    for (name, wasm) in i64_operator_programs() {
        let status = run_wasm(&scratch("i64_expected"), &wasm).status.code().unwrap();
        assert_eq!(status & 1, 0, "{name}: aipl-run disagrees with the expected values");
        assert_wasm_native_matches(&name, &wasm);
    }
}

const EDGES64: [i64; 16] = [
    0, 1, -1, 2, -7, 31, 32, 63, 64, 65, i64::MAX, i64::MIN, i64::MIN + 1, 4294967296, -4294967297, 0x1234_5678_9ABC_DEF0,
];

/// As i32_operator_programs, for the i64 operators: each applies one
/// operator to every pair of EDGES64 and exits with a hash of the results
/// and whether any differed from Rust's.
fn i64_operator_programs() -> Vec<(String, Vec<u8>)> {
    use wasm_encoder::{Instruction as I, ValType as V};
    enum R {
        Wide(fn(i64, i64) -> Option<i64>),
        Bool(fn(i64, i64) -> bool),
    }
    let ops: Vec<(&str, I<'static>, R)> = vec![
        ("add", I::I64Add, R::Wide(|a, b| Some(a.wrapping_add(b)))),
        ("sub", I::I64Sub, R::Wide(|a, b| Some(a.wrapping_sub(b)))),
        ("mul", I::I64Mul, R::Wide(|a, b| Some(a.wrapping_mul(b)))),
        ("and", I::I64And, R::Wide(|a, b| Some(a & b))),
        ("or", I::I64Or, R::Wide(|a, b| Some(a | b))),
        ("xor", I::I64Xor, R::Wide(|a, b| Some(a ^ b))),
        ("shl", I::I64Shl, R::Wide(|a, b| Some(a.wrapping_shl(b as u32)))),
        ("shr_s", I::I64ShrS, R::Wide(|a, b| Some(a.wrapping_shr(b as u32)))),
        ("shr_u", I::I64ShrU, R::Wide(|a, b| Some((a as u64).wrapping_shr(b as u32) as i64))),
        ("div_s", I::I64DivS, R::Wide(|a, b| a.checked_div(b))),
        ("div_u", I::I64DivU, R::Wide(|a, b| (a as u64).checked_div(b as u64).map(|v| v as i64))),
        ("rem_s", I::I64RemS, R::Wide(|a, b| if b == 0 { None } else { Some(a.wrapping_rem(b)) })),
        ("rem_u", I::I64RemU, R::Wide(|a, b| (a as u64).checked_rem(b as u64).map(|v| v as i64))),
        ("eq", I::I64Eq, R::Bool(|a, b| a == b)),
        ("ne", I::I64Ne, R::Bool(|a, b| a != b)),
        ("lt_s", I::I64LtS, R::Bool(|a, b| a < b)),
        ("gt_s", I::I64GtS, R::Bool(|a, b| a > b)),
        ("le_s", I::I64LeS, R::Bool(|a, b| a <= b)),
        ("ge_s", I::I64GeS, R::Bool(|a, b| a >= b)),
    ];
    let mut out = Vec::new();
    for (name, op, expected) in ops {
        let mut body = Vec::new();
        for a in EDGES64 {
            for b in EDGES64 {
                match &expected {
                    R::Wide(f) => {
                        let Some(want) = f(a, b) else { continue };
                        body.extend([I::I64Const(a), I::I64Const(b), op.clone()]);
                        body.extend(check_result_i64(want));
                    }
                    R::Bool(f) => {
                        body.extend([I::I64Const(a), I::I64Const(b), op.clone()]);
                        body.extend(check_result(f(a, b) as i32));
                    }
                }
            }
        }
        body.extend(status_from_checks());
        out.push((format!("i64_{name}"), exit_with_typed(&[(3, V::I32), (1, V::I64)], &body)));
    }
    // extend_i32_s/u of i32 edges, checked as i64
    for (name, op, f) in [
        ("extend_s", I::I64ExtendI32S, (|a: i32| a as i64) as fn(i32) -> i64),
        ("extend_u", I::I64ExtendI32U, |a: i32| a as u32 as i64),
    ] {
        let mut body = Vec::new();
        for a in EDGES {
            body.extend([I::I32Const(a), op.clone()]);
            body.extend(check_result_i64(f(a)));
        }
        body.extend(status_from_checks());
        out.push((format!("i64_{name}"), exit_with_typed(&[(3, V::I32), (1, V::I64)], &body)));
    }
    out
}

/// check_result for an i64 result, kept in local 3: the hash takes its low
/// and high halves.
fn check_result_i64(want: i64) -> Vec<wasm_encoder::Instruction<'static>> {
    use wasm_encoder::Instruction as I;
    vec![
        I::LocalSet(3),
        I::LocalGet(1), I::I32Const(31), I::I32Mul, I::LocalGet(3), I::I32WrapI64, I::I32Add,
        I::I32Const(31), I::I32Mul, I::LocalGet(3), I::I64Const(32), I::I64ShrU, I::I32WrapI64, I::I32Add, I::LocalSet(1),
        I::LocalGet(2), I::LocalGet(3), I::I64Const(want), I::I64Ne, I::I32Add, I::LocalSet(2),
    ]
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
    let typed: Vec<(u32, wasm_encoder::ValType)> = locals.iter().map(|n| (*n, wasm_encoder::ValType::I32)).collect();
    exit_with_typed(&typed, body)
}

/// exit_with, with locals of any type: (count, type) groups.
fn exit_with_typed(locals: &[(u32, wasm_encoder::ValType)], body: &[wasm_encoder::Instruction]) -> Vec<u8> {
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
    memory.memory(MemoryType { minimum: 1, maximum: Some(4), memory64: false, shared: false, page_size_log2: None });
    let mut exports = ExportSection::new();
    exports.export("_start", ExportKind::Func, 1);
    exports.export("memory", ExportKind::Memory, 0);
    let mut code = CodeSection::new();
    let mut start = Function::new([]);
    start.instruction(&I::Call(2)).instruction(&I::Call(0)).instruction(&I::End);
    code.function(&start);
    let mut f = Function::new(locals.iter().copied());
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

fn scratch_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("aipl_native_{}_{}", std::process::id(), name))
}

fn scratch(name: &str) -> PathBuf {
    let d = scratch_path(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Compiles `src`; a program with imports (the standard library) is
/// resolved from a scratch file.
fn to_wasm(src: &str) -> Vec<u8> {
    let m = if src.contains("(import") {
        let dir = scratch(&format!("resolve_{:x}", src.len() * 31 + src.bytes().map(|b| b as usize).sum::<usize>()));
        let path = dir.join("prog.aipl");
        std::fs::write(&path, src).unwrap();
        let m = Resolver::resolve(&path).unwrap_or_else(|e| panic!("{e}"));
        let _ = std::fs::remove_dir_all(&dir);
        m
    } else {
        Parser::parse(src).unwrap_or_else(|e| panic!("{e}"))
    };
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
    to_native_with(wasm, false)
}

fn to_native_with(wasm: &[u8], sandbox: bool) -> Result<Vec<u8>, String> {
    let (engine, module) = native_compiler();
    let mut linker: Linker<WasiP1Ctx> = Linker::new(engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
    let mut store = Store::new(engine, WasiCtxBuilder::new().build_p1());
    let inst = linker.instantiate(&mut store, module).unwrap();
    let alloc = inst.get_typed_func::<i32, i32>(&mut store, "host_alloc").unwrap();
    let compile = inst
        .get_typed_func::<(i32, i32), i32>(&mut store, if sandbox { "compile_sandboxed_at" } else { "compile_at" })
        .unwrap();
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
    run_wasm_with(dir, wasm, &Io::default())
}

fn run_native(dir: &Path, exe: &[u8]) -> Output {
    run_native_with(dir, exe, &Io::default())
}

/// What a program run gets besides its own file: arguments after argv[0],
/// an environment (None: inherit the test's), and stdin.
/// `files` and `symlinks` (name, target) are created in the run directory
/// before each run; `sandbox` runs `aipl-run --sandbox` and builds the
/// native program sandboxed (only the working directory).
#[derive(Default, Clone)]
struct Io {
    args: Vec<String>,
    env: Option<Vec<(String, String)>>,
    stdin: Vec<u8>,
    files: Vec<(String, Vec<u8>)>,
    symlinks: Vec<(String, String)>,
    sandbox: bool,
}

impl Io {
    fn apply(&self, cmd: &mut Command) {
        cmd.args(&self.args);
        if let Some(env) = &self.env {
            cmd.env_clear().envs(env.iter().map(|(k, v)| (k, v)));
        }
    }
}

fn run_wasm_with(dir: &Path, wasm: &[u8], io: &Io) -> Output {
    let path = dir.join("prog");
    std::fs::write(&path, wasm).unwrap();
    let mut cmd = Command::new(RUNNER);
    if io.sandbox {
        cmd.arg("--sandbox");
    }
    cmd.arg(&path).current_dir(dir);
    io.apply(&mut cmd);
    run_fresh_executable(&mut cmd, &io.stdin)
}

fn run_native_with(dir: &Path, exe: &[u8], io: &Io) -> Output {
    let path = dir.join("prog");
    std::fs::write(&path, exe).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut cmd = Command::new(&path);
    cmd.current_dir(dir);
    io.apply(&mut cmd);
    run_fresh_executable(&mut cmd, &io.stdin)
}

/// How long a test program may run. A miscompiled branch can loop forever;
/// that must fail the test, not hang it.
const TIME_LIMIT: std::time::Duration = std::time::Duration::from_secs(30);

/// Runs an executable this process just wrote, killing it after TIME_LIMIT.
/// Tests run in parallel threads, and a thread that forks a child while the
/// file is still open for writing hands that child the open file until it
/// execs, during which Linux refuses to run the file ("Text file busy").
/// Starting is retried briefly then.
fn run_fresh_executable(cmd: &mut Command, stdin: &[u8]) -> Output {
    use std::process::Stdio;
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
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
    let mut child = child.expect("the executable stayed busy for a second");
    let pid = child.id();
    let mut input = child.stdin.take().unwrap();
    let stdin = stdin.to_vec();
    // a separate thread, so a program that does not read its input cannot
    // block the test on a full pipe
    std::thread::spawn(move || {
        use std::io::Write;
        let _ = input.write_all(&stdin);
    });
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
    assert_wasm_native_matches_with(name, wasm, &Io::default());
}

/// Clears `root`, then makes `root/run` with `io`'s files and symlinks.
fn prepare(root: &Path, io: &Io) -> PathBuf {
    let _ = std::fs::remove_dir_all(root);
    let dir = root.join("run");
    std::fs::create_dir_all(&dir).unwrap();
    for (name, bytes) in &io.files {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    for (name, target) in &io.symlinks {
        std::os::unix::fs::symlink(target, dir.join(name)).unwrap();
    }
    dir
}

/// Every file, directory, and symlink under `root` (but the program
/// itself), with contents, sorted: what a run left behind.
fn snapshot(root: &Path) -> Vec<String> {
    fn walk(base: &Path, at: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(at).unwrap() {
            let p = e.unwrap().path();
            let rel = p.strip_prefix(base).unwrap().display().to_string();
            let meta = std::fs::symlink_metadata(&p).unwrap();
            if rel == "run/prog" {
            } else if meta.file_type().is_symlink() {
                out.push(format!("{rel} -> {}", std::fs::read_link(&p).unwrap().display()));
            } else if meta.is_dir() {
                out.push(format!("{rel}/"));
                walk(base, &p, out);
            } else {
                out.push(format!("{rel}: {:?}", String::from_utf8_lossy(&std::fs::read(&p).unwrap())));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

fn assert_wasm_native_matches_with(name: &str, wasm: &[u8], io: &Io) {
    let wasm = wasm.to_vec();
    let exe = to_native_with(&wasm, io.sandbox).unwrap_or_else(|e| panic!("{name}: native compile failed: {e}"));
    let root = scratch(name);
    let dir = prepare(&root, io);
    let want = run_wasm_with(&dir, &wasm, io);
    let want_files = snapshot(&root);
    let dir = prepare(&root, io);
    let got = run_native_with(&dir, &exe, io);
    let got_files = snapshot(&root);
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(want_files, got_files, "{name}: the files left behind differ (aipl-run, then native)");
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
        ("return_from_deep_inside", 19), ("unreachable_traps", 134), ("extend_after_wrap", 3),
        ("store_i64_load_bytes", 18), ("store8_truncates", 80), ("last_valid_word", 3), ("trap_straddling_the_end", 134),
        ("trap_offset_does_not_wrap", 134), ("trap_huge_offset", 134),
        ("trap_offset_past_the_end", 134), ("atomic_store_writes_four_bytes", 79), ("f32_store_writes_four_bytes", 9), ("address_ignores_high_bits", 7),
        ("grow_stops_at_the_maximum", 4), ("grown_pages_are_zero_and_usable", 5), ("trap_past_the_grown_size", 134),
        ("float_bits_round_trip", 3), ("br_keeps_values_below_the_block", 92),
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
        ("trap_load_past_memory", "wasm trap: out of bounds memory access"),
        ("trap_store_past_memory", "wasm trap: out of bounds memory access"),
        ("trap_negative_address", "wasm trap: out of bounds memory access"),
        ("trap_unaligned_atomic", "wasm trap: unaligned atomic"),
        ("trap_unaligned_before_bounds", "wasm trap: unaligned atomic"),
        ("trap_atomic_past_memory", "wasm trap: out of bounds memory access"),
        ("trap_unlock_a_free_lock", "wasm trap: wasm `unreachable` instruction executed"),
        ("trap_lock_twice_waits", "wasm trap: atomic wait on non-shared memory"),
        ("trap_lock_a_non_lock_word", "wasm trap: wasm `unreachable` instruction executed"),
        ("trap_store_into_reserved_block", "wasm trap: wasm `unreachable` instruction executed"),
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
        ("(module m (fn main [] -> i32 (sys.exit (i32.wrap (i64.trunc_f64_s (+ 1.5 2.5)))) 0))", "not supported natively yet: f64.add (opcode 160)"),
        (
            "(module m (fn work [x:i32] -> i32 x) (fn main [] -> i32 (sys.exit (thread.join (thread.spawn (ref work) 3))) 0))",
            "not supported natively yet: a start function (threaded modules)",
        ),
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
