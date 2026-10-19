//! Generation checks (docs/design/GENERATIONS_PLAN.md): a pointer used after
//! its object is freed stops the program with "Use after free" and the
//! position, in the VM, under aipl-run, and natively, with the same output
//! and call chain: through a copy, after the memory holds a new object, an
//! array, a heap, an arena, a fast arena (only once the memory is reused),
//! and a std/alloc allocator. Versioned loops (checks once per loop) fail
//! exactly where the unversioned loop would, and not where it would not.
//! An unchecked pointer (ptr.cast) is not checked, and a pointer in a
//! result cell does not spill onto the next object.

use std::path::PathBuf;
use std::process::Command;

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_gen_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// What a backend did: the program's output, and its failure (the message
/// and call chain, without the "Error: " or "<program>: " prefix), if any.
#[derive(Debug, PartialEq)]
struct Run {
    out: String,
    failure: String,
}

/// `main`'s body (with the imports and structs below) run in each backend.
fn run3(name: &str, body: &str) -> [Run; 3] {
    let dir = scratch(name);
    let src = dir.join("prog.aipl");
    std::fs::write(
        &src,
        format!(
            "(module prog (import heap) (import arena) (import alloc) (import io)\n  (struct P [x:i32 y:i32])\n  (fn sum [xs:(arr i32)] -> i32\n    (let t:i32 0)\n    (loop i 0 (- (arr.len xs) 1) 1 (set! t (+ t (arr.get i32 xs i))))\n    t)\n  (fn main [] -> i32 (unsafe)\n{body}\n    0))"
        ),
    )
    .unwrap();
    let vm = Command::new(AIPL).arg("eval").arg(&src).output().unwrap();
    let vm_out: String = String::from_utf8_lossy(&vm.stdout).lines().filter(|l| !l.starts_with("[AIPL")).map(|l| format!("{l}\n")).collect();
    let vm_err = String::from_utf8_lossy(&vm.stderr).trim_end().strip_prefix("Error: ").unwrap_or("").to_string();

    let wasm = dir.join("prog.wasm");
    let exe = dir.join("prog");
    for args in [vec!["compile"], vec!["compile", "--exe"]] {
        let target = if args.len() == 2 { &exe } else { &wasm };
        let c = Command::new(AIPL).args(&args).arg(&src).arg("-o").arg(target).output().unwrap();
        assert!(c.status.success(), "{name}: {}", String::from_utf8_lossy(&c.stderr));
    }
    let run = |cmd: &mut Command, prog: &PathBuf| {
        let o = cmd.output().unwrap();
        let err = String::from_utf8_lossy(&o.stderr).trim_end().to_string();
        let failure = err.strip_prefix(&format!("{}: ", prog.display())).unwrap_or(&err).to_string();
        let code = if failure.is_empty() { Some(0) } else { Some(134) };
        assert_eq!(o.status.code(), code, "{name}: {err}");
        Run { out: String::from_utf8_lossy(&o.stdout).to_string(), failure }
    };
    let wasm_run = run(Command::new(RUNNER).arg(&wasm), &wasm);
    let native_run = run(&mut Command::new(&exe), &exe);
    let _ = std::fs::remove_dir_all(&dir);
    [Run { out: vm_out, failure: vm_err }, wasm_run, native_run]
}

/// Every backend prints `out` and then fails with "Use after free" and the
/// call `chain` (its "  at f (prog.aipl:L:C)" lines), or does not fail if
/// `chain` is "".
fn assert_runs(name: &str, body: &str, out: &str, chain: &str) {
    let [vm, wasm, native] = run3(name, body);
    assert_eq!(wasm, vm, "{name}: aipl-run and the VM differ");
    assert_eq!(native, vm, "{name}: native and the VM differ");
    let failure = if chain.is_empty() { String::new() } else { format!("Use after free\n{chain}") };
    assert_eq!(vm, Run { out: out.to_string(), failure }, "{name}");
}

#[test]
fn a_copy_used_after_destroy() {
    assert_runs(
        "copy",
        "    (let h:(ptr heap.Heap) (call heap.make))
    (let p:(ptr P) (call (heap.create P) h))
    (let q:(ptr P) p)
    (call (heap.destroy P) h p)
    (call io.println_int \"x: \" (get q P.x))",
        "",
        "  at main (prog.aipl:12:32)",
    );
}

#[test]
fn an_old_pointer_after_the_memory_holds_a_new_object() {
    assert_runs(
        "reused",
        "    (let h:(ptr heap.Heap) (call heap.make))
    (let p:(ptr P) (call (heap.create P) h))
    (call (heap.destroy P) h p)
    (let r:(ptr P) (call (heap.create P) h))
    (put r P.x 9)
    (call io.println_int \"same address: \" (if (eq (ptr.addr r) (ptr.addr p)) 1 0))
    (put p P.x 1)",
        "same address: 1\n",
        "  at main (prog.aipl:14:5)",
    );
}

#[test]
fn an_array_after_free_array() {
    assert_runs(
        "array",
        "    (let h:(ptr heap.Heap) (call heap.make))
    (let xs:(arr i32) (call (heap.array i32) h 4))
    (call (heap.free_array i32) h xs)
    (call io.println_int \"len: \" (arr.len xs))",
        "",
        "  at main (prog.aipl:11:34)",
    );
}

#[test]
fn an_arena_pointer_after_reset() {
    assert_runs(
        "arena",
        "    (let a:(ptr arena.Arena) (call arena.make 4096))
    (let p:(ptr P) (call (arena.alloc P) a))
    (call arena.reset a)
    (call io.println_int \"x: \" (get p P.x))",
        "",
        "  at main (prog.aipl:11:32)",
    );
}

#[test]
fn a_fast_arena_pointer_once_its_memory_is_reused() {
    assert_runs(
        "fast_arena",
        "    (let f:(ptr arena.Fast) (call arena.make_fast 4096))
    (let p:(ptr P) (call (arena.fast_alloc P) f))
    (put p P.x 41)
    (call arena.fast_reset f)
    (call io.println_int \"before reuse: \" (get p P.x))
    (let q:(ptr P) (call (arena.fast_alloc P) f))
    (put q P.x 7)
    (call io.println_int \"after: \" (get p P.x))",
        "before reuse: 41\n",
        "  at main (prog.aipl:15:36)",
    );
}

#[test]
fn an_allocator_pointer_after_destroy() {
    assert_runs(
        "allocator",
        "    (let a:alloc.Allocator (call alloc.of_heap (call heap.make)))
    (let p:(ptr P) (call (alloc.create P) a))
    (call (alloc.destroy P) a p)
    (get p P.y)",
        "",
        "  at main (prog.aipl:11:5)",
    );
}

/// A versioned loop over a freed array fails at the access, in the callee.
#[test]
fn a_versioned_loop_fails_at_the_access() {
    assert_runs(
        "loop_stale",
        "    (let h:(ptr heap.Heap) (call heap.make))
    (let xs:(arr i32) (call (heap.array i32) h 10))
    (call (heap.free_array i32) h xs)
    (call io.println_int \"sum: \" (call sum xs))",
        "",
        "  at sum (prog.aipl:5:18)\n  at main (prog.aipl:11:34)",
    );
}

/// A stale or null pointer a loop never touches is no failure; touched on
/// the fourth pass, it fails there.
#[test]
fn a_versioned_loop_fails_only_where_it_uses_the_pointer() {
    assert_runs(
        "loop_untouched",
        "    (let h:(ptr heap.Heap) (call heap.make))
    (let p:(ptr P) (call (heap.create P) h))
    (call (heap.destroy P) h p)
    (let n:(ptr P) (ptr.null P))
    (let t:i32 0)
    (loop i 0 9 1 (if (gt i 100) (set! t (+ (get p P.x) (get n P.x))) (set! t (+ t i))))
    (call io.println_int \"t: \" t)",
        "t: 45\n",
        "",
    );
    assert_runs(
        "loop_fourth",
        "    (let h:(ptr heap.Heap) (call heap.make))
    (let p:(ptr P) (call (heap.create P) h))
    (call (heap.destroy P) h p)
    (let t:i32 0)
    (loop i 0 9 1 (call io.println_int \"pass \" i) (if (eq i 3) (set! t (get p P.x)) (set! t (+ t i))))",
        "pass 0\npass 1\npass 2\npass 3\n",
        "  at main (prog.aipl:12:72)",
    );
}

/// ptr.cast makes an unchecked pointer: unsafe code may use it after the
/// free (it reads std/heap's junk pattern).
#[test]
fn an_unchecked_pointer_is_not_checked() {
    assert_runs(
        "unchecked",
        "    (let h:(ptr heap.Heap) (call heap.make))
    (let p:(ptr P) (call (heap.create P) h))
    (let raw:(ptr P) (ptr.cast P (ptr.addr p)))
    (call (heap.destroy P) h p)
    (call io.println_int \"x: \" (get raw P.x))",
        "x: -559038737\n",
        "",
    );
}

/// A result cell holding a pointer is 16 bytes, the pointer (address, then
/// generation) at 8: in an 8-byte cell, the pointer's generation landed on
/// the generation word of the object allocated next, and a correct program
/// stopped with "Use after free" when compiled (the VM was right).
#[test]
fn a_pointer_in_a_result_cell_stays_in_its_cell() {
    assert_runs(
        "result_cell",
        "    (let a:alloc.Allocator (call alloc.default))
    (let _warm:(ptr P) (call (alloc.create P) a))
    (let inner:(ptr P) (ptr.null P))
    (let r:(result (ptr P) str) (ok:str (block (let q:(ptr P) (call (alloc.create P) a)) (set! inner (call (alloc.create P) (call alloc.forever))) q)))
    (put inner P.x 5)
    (call io.println_int \"x: \" (get inner P.x))
    (match_result r (ok q (call io.println_int \"q: \" (get q P.x))) (err _e (block)))",
        "x: 5\nq: 0\n",
        "",
    );
}
