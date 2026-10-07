//! The native backend's harness (docs/design/NATIVE_BACKEND_PLAN.md, from NE5).
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
    // compiled bounds checks fail with the VM's message (cells 92/96)
    ("trap_index_past_the_end", "(module m (fn main [] -> i32 (let a:(arr i32) (arr.new i32 5)) (arr.get i32 a 5)))"),
    ("trap_negative_index_store", "(module m (fn main [] -> i32 (let a:(arr i64) (arr.new i64 3)) (arr.set i64 a -2147483648 1i64) 0))"),
    // a call chain longer than the 32 frames shown, and one in a spawned thread
    ("trap_deep_recursion", "(module m (fn down [n:i32] -> i32 (if (eq n 0) (/ 1 n) (+ 1 (call down (- n 1))))) (fn main [] -> i32 (call down 40)))"),
    ("trap_in_a_thread", "(module m (fn work [x:i32] -> i32 (/ 10 x)) (fn main [] -> i32 (thread.join (thread.spawn (ref work) 0))))"),
    // and so do compiled contracts (without the position)
    ("trap_failed_req", "(module m (fn f [n:i32 ok:bool] -> i32 (req (gt n 0)) n) (fn main [] -> i32 (call f -3 true)))"),
    (
        "trap_failed_ens_on_return",
        "(module m (fn f [n:i64] -> i64 (ens (lt res 10i64)) (if (gt n 5i64) (return (* n 2i64)) (block)) n) (fn main [] -> i32 (i32.wrap (call f 7i64))))",
    ),
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
            I::I32Const(64), I::F64Const(1.5f64), I::F64Store(m(3, 0)), I::I32Const(68), I::I32Load(m(2, 0)), I::I32Const(0x3FF8_0000), I::I32Eq,
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
    assert_functions_give(name, src, drop, calls, None);
}

/// assert_functions_match with the expected results given (for programs the
/// in-process wasmtime cannot run, such as threaded ones, which need
/// aipl-run's thread host).
fn assert_functions_give(name: &str, src: &str, drop: &[&str], calls: &[(&str, &[i32])], given: Option<&[i32]>) {
    use aipl_core::ast::Type;
    let mut module = parse_program(src);
    module.functions.retain(|f| !drop.contains(&f.name.as_str()));
    for f in module.functions.iter_mut().filter(|f| f.name == "main") {
        f.name = "orig_main".into();
    }
    TypeChecker::new().check_module(&module).unwrap_or_else(|e| panic!("{name}: {e}"));
    let wasm = WasmCompiler::compile(&module).unwrap();
    // the calls run in order in one instance, as in the generated main, since
    // results such as heap addresses depend on what ran before
    let calls: Vec<(&str, &[i32])> = calls.iter().map(|(f, a)| (if *f == "main" { "orig_main" } else { *f }, *a)).collect();
    let wants = match given {
        Some(v) => v.iter().map(|x| wasmtime::Val::I32(*x)).collect(),
        None => call_exports(&wasm, &calls),
    };
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
            out[0]
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
            ("test_i64_array", &[3]), ("test_index_oob", &[1]), ("test_index_oob", &[2]), ("test_size_allocates", &[]),
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
        ("word_freq", include_str!("../examples/word_freq.aipl").into(), with_input(&[], false)),
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

/// NE15-NE16: threads. The thread programs of tests/test_threads.rs, each
/// run 20 times natively to shake out races (the expected values are those
/// tests' results; aipl-run must give them too).
#[test]
fn thread_programs_match_natively() {
    let threads = program_from(include_str!("test_threads.rs"), "const THREADS");
    for round in 0..20 {
        assert_functions_give(
            &format!("threads_{round}"),
            &threads,
            &["concurrent_print"],
            &[("atomic_counter", &[]), ("join_results", &[]), ("mutex_counter", &[]), ("concurrent_alloc", &[])],
            Some(&[4000, 25, 4000, 4]),
        );
    }
    // threads really run at once: each side waits for the other to start, so
    // this finishes only if both run concurrently (else the time limit fails it)
    let handshake = "(module m
       (fn worker [flags:i32] -> i32
         (let _a:i32 (atomic.add flags 1))
         (while (eq (atomic.add (+ flags 4) 0) 0) (block))
         7)
       (fn main [] -> i32
         (let f:i32 (mem.alloc 8))
         (let h:i32 (thread.spawn (ref worker) f))
         (while (eq (atomic.add f 0) 0) (block))
         (let _b:i32 (atomic.add (+ f 4) 1))
         (sys.exit (thread.join h))
         0))";
    for round in 0..5 {
        assert_native_matches(&format!("handshake_{round}"), handshake);
    }
    let out = run_native(&scratch("handshake_checked"), &to_native(&to_wasm(handshake)).unwrap());
    assert_eq!(out.status.code(), Some(7));
    // a trap in a spawned thread ends the whole program, with its message
    let thread_trap = "(module m
       (fn worker [z:i32] -> i32 (/ 1 z))
       (fn main [] -> i32 (let h:i32 (thread.spawn (ref worker) 0)) (sys.exit (thread.join h)) 0))";
    assert_native_matches("thread_trap_in_worker", thread_trap);
    let exit_in_thread = "(module m
       (fn worker [n:i32] -> i32 (sys.exit n) 0)
       (fn main [] -> i32 (let h:i32 (thread.spawn (ref worker) 9)) (let r:i32 (thread.join h)) (sys.exit 1) 0))";
    assert_native_matches("exit_in_a_thread", exit_in_thread);
    // waits and wakes on shared memory directly: a wait that times out (2),
    // one whose word differs (1), and a notify nobody waits for (0)
    for (name, wasm, status) in [
        ("wait_times_out", shared_module(&[I::I32Const(64), I::I32Const(0), I::I64Const(2_000_000), I::MemoryAtomicWait32(m4()), I::End]), 2),
        ("wait_on_a_different_value", shared_module(&[I::I32Const(64), I::I32Const(5), I::I64Const(-1), I::MemoryAtomicWait32(m4()), I::End]), 1),
        ("notify_without_waiters", shared_module(&[I::I32Const(64), I::I32Const(3), I::MemoryAtomicNotify(m4()), I::End]), 0),
        ("trap_wait_unaligned", shared_module(&[I::I32Const(66), I::I32Const(0), I::I64Const(1), I::MemoryAtomicWait32(m4()), I::End]), 134),
        // memory.init copies the passive segment; both ranges are checked
        ("memory_init_copies", shared_module(&[
            I::I32Const(100), I::I32Const(1), I::I32Const(2), I::MemoryInit { mem: 0, data_index: 0 },
            I::I32Const(100), I::I32Load8U(wasm_encoder::MemArg { offset: 0, align: 0, memory_index: 0 }),
            I::I32Const(101), I::I32Load8U(wasm_encoder::MemArg { offset: 0, align: 0, memory_index: 0 }), I::I32Add, I::I32Const(100), I::I32Sub, I::End,
        ]), 97),
        ("trap_memory_init_past_the_segment", shared_module(&[I::I32Const(100), I::I32Const(2), I::I32Const(2), I::MemoryInit { mem: 0, data_index: 0 }, I::I32Const(0), I::End]), 134),
        ("trap_memory_init_past_memory", shared_module(&[I::I32Const(1048575), I::I32Const(0), I::I32Const(3), I::MemoryInit { mem: 0, data_index: 0 }, I::I32Const(0), I::End]), 134),
        ("memory_init_empty_at_the_end", shared_module(&[I::I32Const(1048576), I::I32Const(3), I::I32Const(0), I::MemoryInit { mem: 0, data_index: 0 }, I::I32Const(5), I::End]), 5),
    ] {
        assert_wasm_native_matches(name, &wasm);
        assert_eq!(run_native(&scratch("shared_checked"), &to_native(&wasm).unwrap()).status.code(), Some(status), "{name}");
    }
    // the main thread of a threaded module uses its globals too (printing
    // goes through its runtime scratch block)
    let main_prints = "(module m (import io)
       (fn sq [x:i32] -> i32 (* x x))
       (fn main [] -> i32
         (call io.println \"before\")
         (let h:i32 (thread.spawn (ref sq) 6))
         (call io.println_int \"joined: \" (thread.join h))
         0))";
    assert_native_matches("main_thread_prints", main_prints);
    // concurrent memory.grow: 8 threads x 50 one-page grows; every grow
    // returns a different old size, so they sum to 16 + 17 + ... + 415
    let growers = "(module m
       (fn grower [total:i32] -> i32
         (loop i 1 50 1 (let _a:i32 (atomic.add total (mem.grow 1))))
         0)
       (fn main [] -> i32
         (let total:i32 (mem.alloc 8))
         (let hs:(arr i32) (arr.new i32 8))
         (loop i 0 7 1 (arr.set i32 hs i (thread.spawn (ref grower) total)))
         (loop i 0 7 1 (let _j:i32 (thread.join (arr.get i32 hs i))))
         (sys.exit (+ (if (eq (atomic.add total 0) 86200) 1 0) (if (eq (mem.grow 0) 416) 2 0)))
         0))";
    for round in 0..10 {
        assert_native_matches(&format!("concurrent_grow_{round}"), growers);
    }
    assert_eq!(run_native(&scratch("grow_checked"), &to_native(&to_wasm(growers)).unwrap()).status.code(), Some(3));
    // a spawned thread's globals start at their initial values
    let wasm = spawn_reads_global();
    assert_wasm_native_matches("thread_global_initial_value", &wasm);
    assert_eq!(run_native(&scratch("global_checked"), &to_native(&wasm).unwrap()).status.code(), Some(42));
    // many threads, each joined, results summed
    let many = "(module m
       (fn sq [x:i32] -> i32 (* x x))
       (fn main [] -> i32
         (let hs:(arr i32) (arr.new i32 40))
         (loop i 0 39 1 (arr.set i32 hs i (thread.spawn (ref sq) i)))
         (let sum:i32 0)
         (loop i 0 39 1 (set! sum (+ sum (thread.join (arr.get i32 hs i)))))
         (sys.exit (% sum 101)) 0))";
    assert_native_matches("forty_threads", many);
    // concurrent printing: whole texts intact, line order free (as in
    // test_threads.rs); checked natively and under aipl-run
    let mut module = parse_program(&threads);
    module.functions.retain(|f| f.name != "main");
    let printed = aipl_core::printer::print_module(&module);
    let end = printed.rfind(')').unwrap();
    let program = format!("{}\n(fn main [] -> i32 (sys.exit (call concurrent_print)) 0))", &printed[..end]);
    let wasm = to_wasm(&program);
    let exe = to_native(&wasm).unwrap();
    let dir = scratch("concurrent_print");
    for round in 0..20 {
        for (who, out) in [("aipl-run", run_wasm(&dir, &wasm)), ("native", run_native(&dir, &exe))] {
            if who == "aipl-run" && round > 0 {
                continue;
            }
            let text = String::from_utf8_lossy(&out.stdout).to_string();
            assert_eq!(out.status.code(), Some(0), "{who}: {}", String::from_utf8_lossy(&out.stderr));
            assert_eq!(text.matches("one one one one one one").count(), 50, "{who}: {text}");
            assert_eq!(text.matches("two two two two two two").count(), 50, "{who}: {text}");
            assert_eq!(text.matches('\n').count(), 100, "{who}");
            assert_eq!(text.len(), 100 * ("one one one one one one".len() + 1), "{who}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

use wasm_encoder::Instruction as I;

fn m4() -> wasm_encoder::MemArg {
    wasm_encoder::MemArg { offset: 0, align: 2, memory_index: 0 }
}

/// A threaded-shape module (an imported shared memory, thread-spawn, and a
/// passive data segment "abc", as AIPL's threaded modules have) whose
/// `_start` exits with `f()`, `body`.
fn shared_module(body: &[wasm_encoder::Instruction]) -> Vec<u8> {
    use wasm_encoder::{
        CodeSection, DataCountSection, DataSection, EntityType, ExportKind, ExportSection, Function, FunctionSection,
        ImportSection, MemoryType, Module, TypeSection, ValType,
    };
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32], []); // 0 proc_exit
    types.ty().function([], []); // 1 _start
    types.ty().function([], [ValType::I32]); // 2 f
    types.ty().function([ValType::I32], [ValType::I32]); // 3 thread-spawn
    types.ty().function([ValType::I32, ValType::I32], []); // 4 wasi_thread_start
    let mut imports = ImportSection::new();
    imports.import("wasi_snapshot_preview1", "proc_exit", EntityType::Function(0));
    imports.import("wasi", "thread-spawn", EntityType::Function(3));
    imports.import("env", "memory", EntityType::Memory(MemoryType { minimum: 16, maximum: Some(32768), memory64: false, shared: true, page_size_log2: None }));
    let mut funcs = FunctionSection::new();
    funcs.function(1);
    funcs.function(2);
    funcs.function(4);
    let mut exports = ExportSection::new();
    exports.export("_start", ExportKind::Func, 2);
    exports.export("wasi_thread_start", ExportKind::Func, 4);
    exports.export("memory", ExportKind::Memory, 0);
    let mut code = CodeSection::new();
    let mut start = Function::new([]);
    start.instruction(&I::Call(3)).instruction(&I::Call(0)).instruction(&I::End);
    code.function(&start);
    let mut f = Function::new([]);
    for ins in body {
        f.instruction(ins);
    }
    code.function(&f);
    let mut ts = Function::new([]);
    ts.instruction(&I::End);
    code.function(&ts);
    let mut data = DataSection::new();
    data.passive(b"abc".iter().copied());
    let mut m = Module::new();
    m.section(&types).section(&imports).section(&funcs).section(&exports).section(&DataCountSection { count: 1 }).section(&code).section(&data);
    m.finish()
}

/// A threaded module with a global (mutable i32, initially 42): `_start`
/// spawns a thread that stores the global's value at address 128 and sets
/// a flag at 132; the main thread waits for the flag and exits with the
/// value.
fn spawn_reads_global() -> Vec<u8> {
    use wasm_encoder::{
        BlockType, CodeSection, ConstExpr, EntityType, ExportKind, ExportSection, Function, FunctionSection, GlobalSection,
        GlobalType, ImportSection, MemArg, MemoryType, Module, TypeSection, ValType,
    };
    let m = |offset| MemArg { offset, align: 2, memory_index: 0 };
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32], []); // 0 proc_exit
    types.ty().function([], []); // 1 _start
    types.ty().function([ValType::I32], [ValType::I32]); // 2 thread-spawn
    types.ty().function([ValType::I32, ValType::I32], []); // 3 wasi_thread_start
    let mut imports = ImportSection::new();
    imports.import("wasi_snapshot_preview1", "proc_exit", EntityType::Function(0));
    imports.import("wasi", "thread-spawn", EntityType::Function(2));
    imports.import("env", "memory", EntityType::Memory(MemoryType { minimum: 16, maximum: Some(32768), memory64: false, shared: true, page_size_log2: None }));
    let mut funcs = FunctionSection::new();
    funcs.function(1); // 2: _start
    funcs.function(3); // 3: wasi_thread_start
    let mut globals = GlobalSection::new();
    globals.global(GlobalType { val_type: ValType::I32, mutable: true, shared: false }, &ConstExpr::i32_const(42));
    let mut exports = ExportSection::new();
    exports.export("_start", ExportKind::Func, 2);
    exports.export("wasi_thread_start", ExportKind::Func, 3);
    exports.export("memory", ExportKind::Memory, 0);
    let mut code = CodeSection::new();
    let mut start = Function::new([]);
    // the main thread changes its own copy first: the new thread must not see it
    start.instruction(&I::I32Const(7)).instruction(&I::GlobalSet(0));
    start.instruction(&I::I32Const(0)).instruction(&I::Call(1)).instruction(&I::Drop);
    start.instruction(&I::Loop(BlockType::Empty));
    start.instruction(&I::I32Const(132)).instruction(&I::I32AtomicLoad(m(0))).instruction(&I::I32Eqz).instruction(&I::BrIf(0));
    start.instruction(&I::End);
    start.instruction(&I::I32Const(128)).instruction(&I::I32Load(m(0))).instruction(&I::Call(0)).instruction(&I::End);
    code.function(&start);
    let mut ts = Function::new([]);
    ts.instruction(&I::I32Const(128)).instruction(&I::GlobalGet(0)).instruction(&I::I32Store(m(0)));
    ts.instruction(&I::I32Const(132)).instruction(&I::I32Const(1)).instruction(&I::I32AtomicStore(m(0))).instruction(&I::End);
    code.function(&ts);
    let mut module = Module::new();
    module.section(&types).section(&imports).section(&funcs).section(&globals).section(&exports).section(&code);
    module.finish()
}

/// NE14: function references (the table and call_indirect).
#[test]
fn function_reference_programs_match_natively() {
    assert_functions_match(
        "refs",
        &program_from(include_str!("test_refs.rs"), "const REFS_PROGRAM"),
        &[],
        &[("main", &[])],
    );
    // std/vec: sort_by with a comparator reference, among its self-tests
    assert_functions_match("std_vec", include_str!("../aipl_src/std/vec.aipl"), &[], &[("run_vec_tests", &[])]);
    assert_functions_match(
        "refs_more",
        "(module m
           (fn a [] -> i32 0) (fn b [] -> i32 1)
           (fn same [] -> bool (eq (ref b) (ref b)))
           (fn differ [] -> bool (neq (ref a) (ref b)))
           (fn sq [x:i32] -> i32 (* x x))
           (fn neg [x:i32] -> i32 (- 0 x))
           (fn apply [f:(fn [i32] -> i32) x:i32] -> i32 (call_ref (fn [i32] -> i32) f x))
           (fn pick [k:i32] -> i32 (call apply (if (eq k 0) (ref sq) (ref neg)) 7))
           (fn wide [x:i64 y:f64] -> f64 (+ (f64.convert_i64_s x) y))
           (fn calls_wide [] -> f64 (call_ref (fn [i64 f64] -> f64) (ref wide) 3i64 0.5)))",
        &[],
        &[("same", &[]), ("differ", &[]), ("pick", &[0]), ("pick", &[1]), ("calls_wide", &[])],
    );
    for (name, wasm) in [
        ("call_indirect_works", table_module(1, 0, 0)),
        // a structurally equal type under another index is the same type
        ("call_indirect_equal_type", table_module(1, 1, 0)),
        ("trap_table_index_out_of_range", table_module(9, 0, 0)),
        ("trap_empty_table_slot", table_module(2, 0, 0)),
        ("trap_type_mismatch", table_module(1, 2, 0)),
        ("trap_index_wraps_no_further", table_module(-1, 0, 0)),
        ("trap_index_equal_to_the_size", table_module(3, 0, 0)),
        // the element segment at slot 1: slot 0 is empty, slot 2 holds f
        ("call_indirect_offset_segment", table_module(2, 0, 1)),
        ("trap_slot_before_an_offset_segment", table_module(0, 0, 1)),
    ] {
        assert_wasm_native_matches(name, &wasm);
        // and they do what their names say
        let out = run_native(&scratch("table_checked"), &to_native(&wasm).unwrap());
        let err = String::from_utf8_lossy(&out.stderr).to_string();
        let want = match name {
            "trap_table_index_out_of_range" | "trap_index_wraps_no_further" | "trap_index_equal_to_the_size" => {
                "out of bounds table access"
            }
            "trap_empty_table_slot" | "trap_slot_before_an_offset_segment" => "uninitialized element",
            "trap_type_mismatch" => "indirect call type mismatch",
            _ => "",
        };
        if want.is_empty() {
            assert_eq!(out.status.code(), Some(42), "{name}: {err}");
        } else {
            assert_eq!(out.status.code(), Some(134), "{name}");
            assert!(err.ends_with(&format!("{want}\n")), "{name}: {err}");
        }
    }
}

/// A module with a 3-slot table holding f (type 0: [i32] -> i32) in two
/// slots from `elem_offset` (0: slots 0-1, slot 2 empty; 1: slots 1-2, slot
/// 0 empty); `_start` exits with call_indirect(index) through `call_type`
/// (0: [i32] -> i32; 1: the same type again; 2: [] -> i32).
fn table_module(index: i32, call_type: u32, elem_offset: i32) -> Vec<u8> {
    use wasm_encoder::{
        CodeSection, ConstExpr, ElementSection, Elements, EntityType, ExportKind, ExportSection, Function, FunctionSection,
        ImportSection, Instruction as I, MemorySection, MemoryType, Module, RefType, TableSection, TableType, TypeSection, ValType,
    };
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32], [ValType::I32]); // 0
    types.ty().function([ValType::I32], [ValType::I32]); // 1, equal to 0
    types.ty().function([], [ValType::I32]); // 2
    types.ty().function([ValType::I32], []); // 3: proc_exit
    types.ty().function([], []); // 4: _start
    let mut imports = ImportSection::new();
    imports.import("wasi_snapshot_preview1", "proc_exit", EntityType::Function(3));
    let mut funcs = FunctionSection::new();
    funcs.function(0); // 1: f(x) = x + 40
    funcs.function(4); // 2: _start
    let mut tables = TableSection::new();
    tables.table(TableType { element_type: RefType::FUNCREF, minimum: 3, maximum: Some(3), table64: false, shared: false });
    let mut memory = MemorySection::new();
    memory.memory(MemoryType { minimum: 1, maximum: Some(1), memory64: false, shared: false, page_size_log2: None });
    let mut exports = ExportSection::new();
    exports.export("_start", ExportKind::Func, 2);
    exports.export("memory", ExportKind::Memory, 0);
    let mut elems = ElementSection::new();
    elems.active(None, &ConstExpr::i32_const(elem_offset), Elements::Functions(std::borrow::Cow::Borrowed(&[1, 1])));
    let mut code = CodeSection::new();
    let mut f = Function::new([]);
    f.instruction(&I::LocalGet(0)).instruction(&I::I32Const(40)).instruction(&I::I32Add).instruction(&I::End);
    code.function(&f);
    let mut start = Function::new([]);
    if call_type == 2 {
        start.instruction(&I::I32Const(index));
    } else {
        start.instruction(&I::I32Const(2)).instruction(&I::I32Const(index));
    }
    start
        .instruction(&I::CallIndirect { type_index: call_type, table_index: 0 })
        .instruction(&I::Call(0))
        .instruction(&I::End);
    code.function(&start);
    let mut m = Module::new();
    m.section(&types).section(&imports).section(&funcs).section(&tables).section(&memory).section(&exports).section(&elems).section(&code);
    m.finish()
}

/// NE13: floats. f64 results are compared by their bits (NaN payloads and
/// -0.0 included).
#[test]
fn float_programs_match_natively() {
    assert_functions_match(
        "f64_cases",
        "(module m
           (struct P [x:f64 y:f32 z:f64])
           (fn convert_neg [] -> f64 (f64.convert_i64_s -7i64))
           (fn convert_rounds_to_even [] -> f64 (f64.convert_i64_s 9007199254740993i64))
           (fn convert_min [] -> f64 (f64.convert_i64_s -9223372036854775808i64))
           (fn trunc_pos [] -> i64 (i64.trunc_f64_s 2.75))
           (fn trunc_neg [] -> i64 (i64.trunc_f64_s -2.75))
           (fn trunc_min [] -> i64 (i64.trunc_f64_s -9223372036854775808.0))
           (fn trunc_just_below_max [] -> i64 (i64.trunc_f64_s 9223372036854774784.0))
           (fn trunc_minus_point_nine [] -> i64 (i64.trunc_f64_s -0.9))
           (fn bits_one [] -> i64 (i64.reinterpret_f64 1.0))
           (fn bits_neg_zero [] -> i64 (i64.reinterpret_f64 -0.0))
           (fn nan_payload [] -> i64 (i64.reinterpret_f64 (f64.reinterpret_i64 9221120237041090561i64)))
           (fn tenth [] -> f64 (/ (f64.convert_i64_s 1i64) (f64.convert_i64_s 10i64)))
           (fn arithmetic [] -> f64 (/ (+ 1.5 2.25) 0.5))
           (fn lt_true [] -> bool (lt 1.5 2.25))
           (fn zero [] -> f64 0.0)
           (fn nan [] -> f64 (/ (call zero) (call zero)))
           (fn neg_nan_sub [] -> f64 (- 0.0 (call nan)))
           (fn inf [] -> f64 (/ 1.0 (call zero)))
           (fn inf_minus_inf [] -> f64 (- (call inf) (call inf)))
           (fn neg_zero_times [] -> f64 (* -0.0 5.0))
           (fn zero_plus_neg_zero [] -> f64 (+ 0.0 -0.0))
           (fn nan_eq [] -> bool (eq (call nan) (call nan)))
           (fn nan_neq [] -> bool (neq (call nan) (call nan)))
           (fn nan_lt [] -> bool (lt (call nan) 1.0))
           (fn nan_gte [] -> bool (gte 1.0 (call nan)))
           (fn zeros_equal [] -> bool (eq 0.0 -0.0))
           (fn lerp [a:f64 b:f64 t:f64] -> f64 (+ a (* (- b a) t)))
           (fn uses_lerp [] -> f64 (call lerp 1.0 3.0 0.25))
           (fn pick [a:f64 b:f64] -> f64 (if (gt a b) a (/ b a)))
           (fn uses_pick [] -> f64 (+ (call pick 4.0 2.0) (call pick 2.0 4.0)))
           (fn struct_field [] -> f64 (let p:(ptr P) (new P)) (put p P.z 6.5) (put p P.x -1.25) (+ (get p P.z) (get p P.x)))
           (fn struct_size [] -> i32 (sizeof P))
           (fn sqrt_two [] -> f64 (f64.sqrt 2.0))
           (fn sqrt_neg_zero [] -> f64 (f64.sqrt -0.0))
           (fn sqrt_negative [] -> f64 (f64.sqrt -4.0)))",
        &[],
        &[
            ("convert_neg", &[]), ("convert_rounds_to_even", &[]), ("convert_min", &[]), ("trunc_pos", &[]), ("trunc_neg", &[]),
            ("trunc_min", &[]), ("trunc_just_below_max", &[]), ("trunc_minus_point_nine", &[]), ("bits_one", &[]),
            ("bits_neg_zero", &[]), ("nan_payload", &[]), ("tenth", &[]), ("arithmetic", &[]), ("lt_true", &[]), ("nan", &[]),
            ("neg_nan_sub", &[]), ("inf", &[]), ("inf_minus_inf", &[]), ("neg_zero_times", &[]), ("zero_plus_neg_zero", &[]),
            ("nan_eq", &[]), ("nan_neq", &[]), ("nan_lt", &[]), ("nan_gte", &[]), ("zeros_equal", &[]), ("uses_lerp", &[]),
            ("uses_pick", &[]), ("struct_field", &[]), ("struct_size", &[]), ("sqrt_two", &[]), ("sqrt_neg_zero", &[]),
            ("sqrt_negative", &[]),
        ],
    );
    for (name, src) in [
        ("trap_trunc_too_big", "(module m (fn main [] -> i32 (let t:i64 (i64.trunc_f64_s 9223372036854775808.0)) 0))"),
        ("trap_trunc_too_small", "(module m (fn main [] -> i32 (let t:i64 (i64.trunc_f64_s -9223372036854777856.0)) 0))"),
        ("trap_trunc_nan", "(module m (fn main [] -> i32 (let z:f64 0.0) (let t:i64 (i64.trunc_f64_s (/ z z))) 0))"),
        ("trap_trunc_infinity", "(module m (fn main [] -> i32 (let z:f64 0.0) (let t:i64 (i64.trunc_f64_s (/ 1.0 z))) 0))"),
    ] {
        assert_wasm_native_matches(name, &to_wasm(src));
    }
    assert_functions_match(
        "matrix_mult",
        include_str!("../examples/matrix_mult.aipl"),
        &[],
        &[("main", &[]), ("trace_of_product", &[1]), ("trace_of_product", &[4]), ("trace_of_product", &[9])],
    );
    // the float literals of tests/test_selfhost.rs (same generator)
    let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = |n: u64| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) % n
    };
    let mut src = String::from("(module floats\n");
    let mut made = 0;
    while made < 200 {
        let ndigits = 1 + next(16) as usize;
        let digits: String = (0..ndigits).map(|_| char::from(b'0' + next(10) as u8)).collect();
        let m: u64 = digits.parse().unwrap();
        let dot = next(ndigits as u64 + 1) as usize;
        if m > 1 << 53 || ndigits - dot > 22 {
            continue;
        }
        let sign = if next(2) == 0 { "" } else { "-" };
        let lit = format!("{sign}{}.{}", &digits[..dot], &digits[dot..]);
        if !lit.chars().any(|c| c.is_ascii_digit()) {
            continue;
        }
        src.push_str(&format!("  (fn f{made} [] -> f64 {lit})\n"));
        made += 1;
    }
    src.push(')');
    let names: Vec<String> = (0..200).map(|i| format!("f{i}")).collect();
    let calls: Vec<(&str, &[i32])> = names.iter().map(|n| (n.as_str(), &[][..])).collect();
    assert_functions_match("float_literals", &src, &[], &calls);
    for (name, wasm) in float_operator_programs() {
        let status = run_wasm(&scratch("float_expected"), &wasm).status.code().unwrap();
        assert_eq!(status & 1, 0, "{name}: aipl-run disagrees with the expected values");
        assert_wasm_native_matches(&name, &wasm);
    }
}

/// f64 edge values: zeros, ones, fractions, extremes, subnormals, infinities, NaN.
fn f64_edges() -> Vec<f64> {
    vec![0.0, -0.0, 1.0, -1.5, 0.1, 3.0, 1e308, -1e-308, f64::MIN_POSITIVE, 5e-324, f64::MAX, f64::INFINITY, f64::NEG_INFINITY, f64::NAN]
}

fn f32_edges() -> Vec<f32> {
    vec![0.0, -0.0, 1.0, -1.5, 0.1, 3.0, 1e38, -1e-38, f32::MIN_POSITIVE, 1e-45, f32::MAX, f32::INFINITY, f32::NEG_INFINITY, f32::NAN]
}

/// Like i32_operator_programs, for the f32 and f64 operators: every pair of
/// edge values; results are checked against Rust's (a NaN only for being a
/// NaN, since payloads are not specified), and hashed by their exact bits,
/// so native and aipl-run must agree on every bit.
fn float_operator_programs() -> Vec<(String, Vec<u8>)> {
    use std::hint::black_box as bb;
    use wasm_encoder::{Instruction as I, MemArg, ValType as V};
    let m = |align, offset| MemArg { offset, align, memory_index: 0 };
    type F64Op = fn(f64, f64) -> f64;
    type F64Cmp = fn(f64, f64) -> bool;
    let arith64: Vec<(&str, I<'static>, F64Op)> = vec![
        ("add", I::F64Add, |a, b| bb(a) + bb(b)),
        ("sub", I::F64Sub, |a, b| bb(a) - bb(b)),
        ("mul", I::F64Mul, |a, b| bb(a) * bb(b)),
        ("div", I::F64Div, |a, b| bb(a) / bb(b)),
    ];
    let cmp64: Vec<(&str, I<'static>, F64Cmp)> = vec![
        ("eq", I::F64Eq, |a, b| a == b), ("ne", I::F64Ne, |a, b| a != b), ("lt", I::F64Lt, |a, b| a < b),
        ("gt", I::F64Gt, |a, b| a > b), ("le", I::F64Le, |a, b| a <= b), ("ge", I::F64Ge, |a, b| a >= b),
    ];
    // locals: 0-2 i32 (result, hash, differences), 3 i64 (bits), 4 f64, 5 f32
    let locals = [(3, V::I32), (1, V::I64), (1, V::F64), (1, V::F32)];
    let mut out = Vec::new();
    for (name, op, f) in arith64 {
        let mut body = Vec::new();
        for a in f64_edges() {
            for b in f64_edges() {
                body.extend([I::F64Const(a), I::F64Const(b), op.clone()]);
                body.extend(check_result_f64(f(a, b)));
            }
        }
        body.extend(status_from_checks());
        out.push((format!("f64_{name}"), exit_with_typed(&locals, &body)));
    }
    for (name, op, f) in cmp64 {
        let mut body = Vec::new();
        for a in f64_edges() {
            for b in f64_edges() {
                body.extend([I::F64Const(a), I::F64Const(b), op.clone()]);
                body.extend(check_result(f(a, b) as i32));
            }
        }
        body.extend(status_from_checks());
        out.push((format!("f64_{name}"), exit_with_typed(&locals, &body)));
    }
    // f32: operands go through memory (AIPL emits no f32.const)
    let f32_operands = |a: f32, b: f32| {
        vec![
            I::I32Const(64), I::I32Const(a.to_bits() as i32), I::I32Store(m(2, 0)),
            I::I32Const(68), I::I32Const(b.to_bits() as i32), I::I32Store(m(2, 0)),
            I::I32Const(64), I::F32Load(m(2, 0)), I::I32Const(68), I::F32Load(m(2, 0)),
        ]
    };
    type F32Op = fn(f32, f32) -> f32;
    type F32Cmp = fn(f32, f32) -> bool;
    let arith32: Vec<(&str, I<'static>, F32Op)> = vec![
        ("add", I::F32Add, |a, b| bb(a) + bb(b)),
        ("sub", I::F32Sub, |a, b| bb(a) - bb(b)),
        ("mul", I::F32Mul, |a, b| bb(a) * bb(b)),
        ("div", I::F32Div, |a, b| bb(a) / bb(b)),
    ];
    let cmp32: Vec<(&str, I<'static>, F32Cmp)> = vec![
        ("eq", I::F32Eq, |a, b| a == b), ("ne", I::F32Ne, |a, b| a != b), ("lt", I::F32Lt, |a, b| a < b),
        ("gt", I::F32Gt, |a, b| a > b), ("le", I::F32Le, |a, b| a <= b), ("ge", I::F32Ge, |a, b| a >= b),
    ];
    for (name, op, f) in arith32 {
        let mut body = Vec::new();
        for a in f32_edges() {
            for b in f32_edges() {
                body.extend(f32_operands(a, b));
                body.push(op.clone());
                body.extend(check_result_f32(f(a, b)));
            }
        }
        body.extend(status_from_checks());
        out.push((format!("f32_{name}"), exit_with_typed(&locals, &body)));
    }
    for (name, op, f) in cmp32 {
        let mut body = Vec::new();
        for a in f32_edges() {
            for b in f32_edges() {
                body.extend(f32_operands(a, b));
                body.push(op.clone());
                body.extend(check_result(f(a, b) as i32));
            }
        }
        body.extend(status_from_checks());
        out.push((format!("f32_{name}"), exit_with_typed(&locals, &body)));
    }
    // f64.sqrt over every edge (negative ones give NaN)
    let mut sqrt = Vec::new();
    for a in f64_edges() {
        sqrt.extend([I::F64Const(a), I::F64Sqrt]);
        sqrt.extend(check_result_f64(bb(a).sqrt()));
    }
    sqrt.extend(status_from_checks());
    out.push(("f64_sqrt".into(), exit_with_typed(&locals, &sqrt)));
    // conversions both ways over in-range edges
    let mut conv = Vec::new();
    for a in [0i64, 1, -7, i64::MAX, i64::MIN, 9007199254740993, -9007199254740993, 0x1234_5678_9ABC_DEF0] {
        conv.extend([I::I64Const(a), I::F64ConvertI64S]);
        conv.extend(check_result_f64(a as f64));
    }
    for a in [0.0f64, -0.0, 0.5, -0.5, 2.75, -2.75, 1e18, -9.223_372_036_854_775e18, 9.223_372_036_854_775e18, 5e-324] {
        conv.extend([I::F64Const(a), I::I64TruncF64S]);
        conv.extend(check_result_i64(a as i64));
    }
    conv.extend(status_from_checks());
    out.push(("f64_conversions".into(), exit_with_typed(&locals, &conv)));
    out
}

/// check_result for an f64 result: hashed by its bits; a difference unless
/// it equals `want` bit for bit, or both are NaN.
fn check_result_f64(want: f64) -> Vec<wasm_encoder::Instruction<'static>> {
    use wasm_encoder::Instruction as I;
    let mut v = vec![I::LocalTee(4), I::I64ReinterpretF64];
    v.extend(check_bits_i64(if want.is_nan() { None } else { Some(want.to_bits() as i64) }));
    if want.is_nan() {
        // differs unless the result is a NaN (x != x)
        v.extend([I::LocalGet(2), I::LocalGet(4), I::LocalGet(4), I::F64Ne, I::I32Eqz, I::I32Add, I::LocalSet(2)]);
    }
    v
}

/// The bits (an i64 on the stack) into local 3 and the hash; with `want`,
/// counts a difference from it.
fn check_bits_i64(want: Option<i64>) -> Vec<wasm_encoder::Instruction<'static>> {
    use wasm_encoder::Instruction as I;
    let mut v = vec![
        I::LocalSet(3),
        I::LocalGet(1), I::I32Const(31), I::I32Mul, I::LocalGet(3), I::I32WrapI64, I::I32Add,
        I::I32Const(31), I::I32Mul, I::LocalGet(3), I::I64Const(32), I::I64ShrU, I::I32WrapI64, I::I32Add, I::LocalSet(1),
    ];
    if let Some(w) = want {
        v.extend([I::LocalGet(2), I::LocalGet(3), I::I64Const(w), I::I64Ne, I::I32Add, I::LocalSet(2)]);
    }
    v
}

/// check_result_f64 for an f32 result (its bits read back through memory).
fn check_result_f32(want: f32) -> Vec<wasm_encoder::Instruction<'static>> {
    use wasm_encoder::{Instruction as I, MemArg};
    let m = MemArg { offset: 0, align: 2, memory_index: 0 };
    let mut v = vec![I::LocalSet(5), I::I32Const(72), I::LocalGet(5), I::F32Store(m), I::I32Const(72), I::I32Load(m), I::I64ExtendI32U];
    v.extend(check_bits_i64(if want.is_nan() { None } else { Some(want.to_bits() as i64) }));
    if want.is_nan() {
        v.extend([I::LocalGet(2), I::LocalGet(5), I::LocalGet(5), I::F32Ne, I::I32Eqz, I::I32Add, I::LocalSet(2)]);
    }
    v
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
        ("lt_u", I::I64LtU, R::Bool(|a, b| (a as u64) < (b as u64))),
        ("gt_u", I::I64GtU, R::Bool(|a, b| (a as u64) > (b as u64))),
        ("le_u", I::I64LeU, R::Bool(|a, b| (a as u64) <= (b as u64))),
        ("ge_u", I::I64GeU, R::Bool(|a, b| (a as u64) >= (b as u64))),
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
        ("le_u", I::I32LeU, |a, b| Some(((a as u32) <= (b as u32)) as i32)),
        ("ge_u", I::I32GeU, |a, b| Some(((a as u32) >= (b as u32)) as i32)),
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

/// Parses `src` the way the toolchain does: through the resolver (imports
/// from the standard library, generics expanded), from a scratch file.
fn parse_program(src: &str) -> aipl_core::ast::Module {
    // a folder of its own: tests run in parallel and may parse the same text
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = scratch(&format!("resolve_{}", NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let path = dir.join("prog.aipl");
    std::fs::write(&path, src).unwrap();
    let m = Resolver::resolve(&path).unwrap_or_else(|e| panic!("{e}"));
    let _ = std::fs::remove_dir_all(&dir);
    m
}

/// Compiles `src` (see parse_program).
fn to_wasm(src: &str) -> Vec<u8> {
    let m = parse_program(src);
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

/// Writes a program to run at `path`, removing what was there first. The
/// program run there just before may still count as running for a moment
/// after it has exited (its memory not yet torn down), and overwriting it
/// then fails with "Text file busy" (seen on GitHub's runners); a removed
/// file never blocks, and the new one gets the same name.
fn write_program(path: &Path, bytes: &[u8]) {
    let _ = std::fs::remove_file(path);
    std::fs::write(path, bytes).unwrap();
}

fn run_wasm_with(dir: &Path, wasm: &[u8], io: &Io) -> Output {
    let path = dir.join("prog");
    write_program(&path, wasm);
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
    write_program(&path, exe);
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
    // a folder of its own: tests run in parallel, and two may use one
    // program name (a run once executed another test's program)
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let root = scratch(&format!("{name}_{}", NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
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

/// The trap programs really trap, natively, with exactly aipl-run's line
/// and call chain (the functions the trap happened in, innermost first),
/// and exit with 134. The exit-status error is not a trap: one line.
#[test]
fn traps_print_their_line_and_call_chain() {
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
        ("trap_index_past_the_end", "Array index out of bounds: index 5 for array of length 5"),
        ("trap_negative_index_store", "Array index out of bounds: index -2147483648 for array of length 3"),
        ("trap_failed_req", "Pre-condition failed in 'f': (req (gt n 0)) with n = -3, ok = true"),
        ("trap_failed_ens_on_return", "Post-condition failed in 'f': (ens (lt res 10i64)) with n = 7i64, res = 14i64"),
        ("trap_deep_recursion", "wasm trap: integer divide by zero"),
        ("trap_in_a_thread", "wasm trap: integer divide by zero"),
    ] {
        let src = PROGRAMS.iter().find(|(n, _)| *n == name).unwrap().1;
        let o = run_native(&dir, &to_native(&to_wasm(src)).unwrap());
        assert_eq!(o.status.code(), Some(134), "{name}");
        let deep = "  at down\n".repeat(32) + "  ... 10 more\n";
        let chain = match name {
            n if n.starts_with("exit_status") => "",
            n if n.starts_with("trap_failed") => "  at f\n  at main\n",
            // 41 calls of down and main: 32 shown
            "trap_deep_recursion" => deep.as_str(),
            // a thread's chain starts at its worker
            "trap_in_a_thread" => "  at work\n",
            _ => "  at main\n",
        };
        // positions: traps_name_their_source_positions
        assert_eq!(without_positions(&String::from_utf8_lossy(&o.stderr)), format!("{prog}: {reason}\n{chain}"), "{name}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A call chain without its source positions: "  at f (x.aipl:1:2)" -> "  at f".
fn without_positions(s: &str) -> String {
    s.lines()
        .map(|l| match l.find(" (") {
            Some(k) if l.starts_with("  at ") && l.ends_with(')') => &l[..k],
            _ => l,
        })
        .map(|l| format!("{l}\n"))
        .collect()
}

/// Each frame of a trap's call chain names where in the source it was, the
/// same under aipl-run and natively: the trapping expression, then each
/// call (docs/design/LINES_PLAN.md). The columns were checked by hand.
#[test]
fn traps_name_their_source_positions() {
    let dir = scratch("positions");
    for (name, chain) in [
        ("trap_div_by_zero", "  at main (prog.aipl:1:54)\n"),
        ("trap_load_past_memory", "  at main (prog.aipl:1:65)\n"),
        ("trap_unlock_a_free_lock", "  at main (prog.aipl:1:56)\n"),
        // the store guard (wasmtime reports its trap at the if)
        ("trap_store_into_reserved_block", "  at main (prog.aipl:1:51)\n"),
        ("trap_index_past_the_end", "  at main (prog.aipl:1:64)\n"),
        // a contract at its condition, then the call
        ("trap_failed_req", "  at f (prog.aipl:1:45)\n  at main (prog.aipl:1:77)\n"),
        ("trap_in_a_thread", "  at work (prog.aipl:1:35)\n"),
        ("trap_min_div_minus_one", "  at main (prog.aipl:1:55)\n"),
        ("trap_unaligned_atomic", "  at main (prog.aipl:1:66)\n"),
        // the second of two identical locks
        ("trap_lock_twice_waits", "  at main (prog.aipl:1:72)\n"),
    ] {
        let src = PROGRAMS.iter().find(|(n, _)| *n == name).unwrap().1;
        let wasm = to_wasm(src);
        for (how, o) in [("aipl-run", run_wasm(&dir, &wasm)), ("native", run_native(&dir, &to_native(&wasm).unwrap()))] {
            let err = String::from_utf8_lossy(&o.stderr).to_string();
            let got = err.split_once('\n').map(|(_, rest)| rest).unwrap_or("");
            assert_eq!(got, chain, "{name} {how}: {err}");
        }
    }
    // a call whose result goes straight into another call: the caller's frame
    // is at the inner call, though with no arguments to pop its return
    // address can be where the outer call's code begins
    let src = "(module m (fn g [] -> i32 (let z:i32 0) (/ 1 z)) (fn f [x:i32] -> i32 x) (fn main [] -> i32 (call f (call g))))";
    let wasm = to_wasm(src);
    let want = format!("  at g (prog.aipl:1:{})\n  at main (prog.aipl:1:{})\n", src.find("(/ 1 z)").unwrap() + 1, src.find("(call g)").unwrap() + 1);
    for (how, o) in [("aipl-run", run_wasm(&dir, &wasm)), ("native", run_native(&dir, &to_native(&wasm).unwrap()))] {
        let err = String::from_utf8_lossy(&o.stderr).to_string();
        assert_eq!(err.split_once('\n').map(|(_, r)| r).unwrap_or(""), want, "nested calls {how}: {err}");
    }
    // a deep chain: the division, then each recursive call
    let src = PROGRAMS.iter().find(|(n, _)| *n == "trap_deep_recursion").unwrap().1;
    let want = "  at down (prog.aipl:1:48)\n".to_string() + &"  at down (prog.aipl:1:61)\n".repeat(31) + "  ... 10 more\n";
    let o = run_native(&dir, &to_native(&to_wasm(src)).unwrap());
    assert_eq!(String::from_utf8_lossy(&o.stderr).split_once('\n').unwrap().1, want);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Recursion too deep for the stack traps as under aipl-run (its message,
/// the call chain, exit 134), in the main thread and in a spawned one,
/// instead of faulting; and how deep a program may go does not depend on
/// the user's stack ulimit (the main thread runs on its own stack, like a
/// spawned thread). Found by tools/run_fuzz.py: native executables died
/// with SIGSEGV and no message.
#[test]
fn running_out_of_stack_traps() {
    let dir = scratch("stack");
    let prog = dir.join("prog").display().to_string();
    let down = "(fn down [n:i32] -> i32 (if (eq n 0) 0 (+ 1 (call down (- n 1)))))";
    let in_main = format!("(module m (import io) {down} (fn main [] -> i32 (call io.print_int (call down 10000000)) 0))");
    let in_thread = format!(
        "(module m (import io) {down} (fn work [n:i32] -> i32 (call down n))
           (fn main [] -> i32 (call io.print_int (thread.join (thread.spawn (ref work) 10000000))) 0))"
    );
    for (name, src) in [("main", &in_main), ("thread", &in_thread)] {
        let wasm = to_wasm(src);
        for (how, o) in [("aipl-run", run_wasm(&dir, &wasm)), ("native", run_native(&dir, &to_native(&wasm).unwrap()))] {
            let err = String::from_utf8_lossy(&o.stderr);
            assert_eq!(o.status.code(), Some(134), "{name} {how}: {err}");
            assert!(o.stdout.is_empty(), "{name} {how}");
            // the innermost frame traps in its prologue, before any position;
            // the others are at the recursive call
            let col = src.find("(call down").unwrap() + 1;
            let head = format!("{prog}: wasm trap: call stack exhausted\n  at down\n{}", format!("  at down (prog.aipl:1:{col})\n").repeat(31));
            assert!(err.starts_with(&head), "{name} {how}: {err}");
            let more = err[head.len()..].strip_prefix("  ... ").and_then(|r| r.strip_suffix(" more\n"));
            let more: usize = more.and_then(|n| n.parse().ok()).unwrap_or_else(|| panic!("{name} {how}: {err}"));
            assert!(more > 20_000, "{name} {how}: only {more} frames");
        }
    }
    // a frame larger than the room left below the limit (40,000 locals):
    // the check counts the frame, so this traps too instead of faulting
    let big = exit_with(&[40_000], &[wasm_encoder::Instruction::Call(2), wasm_encoder::Instruction::End]);
    for (how, o) in [("aipl-run", run_wasm(&dir, &big)), ("native", run_native(&dir, &to_native(&big).unwrap()))] {
        let err = String::from_utf8_lossy(&o.stderr);
        assert_eq!(o.status.code(), Some(134), "big frames {how}: {err}");
        assert!(err.starts_with(&format!("{prog}: wasm trap: call stack exhausted\n")), "big frames {how}: {err}");
    }
    // 100,000 calls fit natively, even with a 1 MiB stack ulimit
    let exe = to_native(&to_wasm(&in_main.replace("10000000", "100000"))).unwrap();
    let path = dir.join("prog");
    write_program(&path, &exe);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    // sh's exec of the file this thread just wrote can meet the same "Text
    // file busy" race run_fresh_executable retries (another test's fork
    // holding the file open until it execs), so this retries it too
    let mut o = run_fresh_executable(Command::new("sh").arg("-c").arg("ulimit -s 1024 && exec ./prog").current_dir(&dir), &[]);
    for _ in 0..100 {
        if !String::from_utf8_lossy(&o.stderr).contains("busy") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        o = run_fresh_executable(Command::new("sh").arg("-c").arg("ulimit -s 1024 && exec ./prog").current_dir(&dir), &[]);
    }
    let shown = (o.status.code(), String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string());
    assert_eq!(shown, (Some(0), "100000".to_string(), String::new()));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Anything not translated yet is an error naming it, never a wrong program.
#[test]
fn unsupported_instructions_and_imports_are_named() {
    // everything AIPL emits is translated since NE16; what is left are
    // modules AIPL does not make
    assert_eq!(to_native(&to_wasm("(module m (fn f [] -> i32 7))")), Err("the module has no _start export".to_string()));
    use wasm_encoder::{CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection, ImportSection, Module, TypeSection, ValType};
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32, ValType::I64, ValType::I32, ValType::I32], [ValType::I32]);
    types.ty().function([], []);
    let mut imports = ImportSection::new();
    imports.import("wasi_snapshot_preview1", "fd_seek", EntityType::Function(0));
    let mut funcs = FunctionSection::new();
    funcs.function(1);
    let mut exports = ExportSection::new();
    exports.export("_start", ExportKind::Func, 1);
    let mut code = CodeSection::new();
    let mut f = Function::new([]);
    f.instruction(&I::End);
    code.function(&f);
    let mut m = Module::new();
    m.section(&types).section(&imports).section(&funcs).section(&exports).section(&code);
    assert_eq!(to_native(&m.finish()), Err("import not supported natively yet: wasi_snapshot_preview1.fd_seek".to_string()));
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

/// NE17: the AIPL compiler (aipl_src/driver.aipl) built natively. It must
/// compile every repository program to the Rust toolchain's bytes, compile
/// itself to its own wasm (the fixpoint, with no Rust or wasmtime in the
/// second build), and building it natively twice must give the same file.
#[test]
fn the_compiler_runs_natively() {
    let driver = WasmCompiler::compile(&Resolver::resolve(&root().join("aipl_src/driver.aipl")).unwrap()).unwrap();
    let aiplc = to_native(&driver).unwrap();
    assert_eq!(aiplc, to_native(&driver).unwrap(), "two native builds of the compiler differ");
    let dir = scratch("aiplc");
    let exe = dir.join("aiplc");
    std::fs::write(&exe, &aiplc).unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out_dir = root().join("target/ne17");
    std::fs::create_dir_all(&out_dir).unwrap();
    // every program the Rust toolchain compiles, from the repository root
    let mut paths = Vec::new();
    for sub in ["examples", "aipl_src/std", "aipl_src", "aipl_src/native"] {
        for e in std::fs::read_dir(root().join(sub)).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "aipl") {
                paths.push(p);
            }
        }
    }
    paths.sort();
    let mut compared = 0;
    for p in &paths {
        let Ok(m) = Resolver::resolve(p) else { continue };
        if TypeChecker::new().check_module(&m).is_err() {
            continue;
        }
        let Ok(want) = WasmCompiler::compile(&m) else { continue };
        let rel = p.strip_prefix(root()).unwrap().display().to_string();
        let out = out_dir.join(format!("{}.wasm", rel.replace('/', "_")));
        let run = run_fresh_executable(Command::new(&exe).arg(&rel).arg(&out).current_dir(root()), &[]);
        assert_eq!(run.status.code(), Some(0), "{rel}: {}", String::from_utf8_lossy(&run.stderr));
        assert!(std::fs::read(&out).unwrap() == want, "{rel}: the native compiler's output differs from the Rust toolchain's");
        compared += 1;
    }
    assert!(compared >= 25, "only {compared} programs compared");
    // the fixpoint: the native compiler compiles itself to the wasm it was built from
    let own = out_dir.join("driver_by_native.wasm");
    let run = run_fresh_executable(Command::new(&exe).arg("aipl_src/driver.aipl").arg(&own).current_dir(root()), &[]);
    assert_eq!(run.status.code(), Some(0), "{}", String::from_utf8_lossy(&run.stderr));
    assert!(std::fs::read(&own).unwrap() == driver, "the native compiler does not reproduce itself");
    // and that wasm, built natively again, is the same executable
    assert_eq!(to_native(&std::fs::read(&own).unwrap()).unwrap(), aiplc);
    // errors: usage, and a program that does not resolve
    let usage = run_fresh_executable(Command::new(&exe).current_dir(root()), &[]);
    let usage_wasm = run_wasm(&dir, &driver);
    assert_eq!(usage.status.code(), usage_wasm.status.code());
    let missing = run_fresh_executable(Command::new(&exe).args(["examples/missing.aipl", "target/ne17/x.wasm"]).current_dir(root()), &[]);
    assert_ne!(missing.status.code(), Some(0));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Checked arithmetic natively: the same values as under aipl-run, and an
/// overflow traps with "integer overflow" (exit 134) in both.
#[test]
fn checked_arithmetic_matches_natively() {
    let dir = scratch("checked");
    let prog = dir.join("prog").display().to_string();
    let ops = [
        ("(checked.add 2147483646 1)", None),
        ("(checked.sub -2147483647 1)", None),
        ("(checked.mul 46340 46340)", None),
        ("(i32.wrap (/ (checked.mul -3037000499i64 3037000499i64) 1000000i64))", None),
        ("(i32.wrap (/ (checked.add 9223372036854775806i64 1i64) 4294967296i64))", None),
        ("(checked.add 2147483647 1)", Some("integer overflow")),
        ("(checked.sub -2147483648 1)", Some("integer overflow")),
        ("(checked.mul 65536 32768)", Some("integer overflow")),
        ("(i32.wrap (checked.add 9223372036854775807i64 1i64))", Some("integer overflow")),
        ("(i32.wrap (checked.sub -9223372036854775808i64 1i64))", Some("integer overflow")),
        ("(i32.wrap (checked.mul 3037000500i64 3037000500i64))", Some("integer overflow")),
        ("(i32.wrap (checked.mul -1i64 -9223372036854775808i64))", Some("integer overflow")),
    ];
    for (e, trap) in ops {
        let src = format!("(module m (import io) (fn main [] -> i32 (call io.println_int \"\" {e}) 0))");
        let wasm = to_wasm(&src);
        let native = run_native(&dir, &to_native(&wasm).unwrap());
        let launched = run_wasm(&dir, &wasm);
        assert_eq!(native.stdout, launched.stdout, "{e}");
        assert_eq!(native.status.code(), launched.status.code(), "{e}");
        match trap {
            Some(reason) => {
                assert_eq!(native.status.code(), Some(134), "{e}");
                // at the checked operation (its column in the one-line source)
                let col = src.find("(checked.").unwrap() + 1;
                assert_eq!(String::from_utf8_lossy(&native.stderr), format!("{prog}: wasm trap: {reason}\n  at main (prog.aipl:1:{col})\n"), "{e}");
                assert_eq!(native.stderr, launched.stderr, "{e}");
            }
            None => assert_eq!(native.status.code(), Some(0), "{e}: {}", String::from_utf8_lossy(&native.stderr)),
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
