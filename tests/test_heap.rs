//! std/heap's failure checks (docs/design/HEAP_PLAN.md): a double free, a free of
//! memory another heap (or no heap) allocated, a free with the wrong size
//! or type, and a write after free each stop the program with a contract
//! failure naming the check, in the VM, under aipl-run, and natively. The
//! working cases are std/heap's own self-test (aipl_src/test_suite.aipl).

use std::path::PathBuf;
use std::process::Command;

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_heap_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The failure message of `main` in each backend: the VM's (without its
/// ` at L:C` position, which compiled messages leave out), aipl-run's, and
/// the native executable's (each without the `<program>: ` prefix). The two
/// compiled ones must also print the same call chain after it, ending at
/// main.
fn failures(name: &str, body: &str) -> [String; 3] {
    let dir = scratch(name);
    let src = dir.join("prog.aipl");
    std::fs::write(
        &src,
        format!("(module prog (import heap)\n  (struct P [a:i32 b:i32])\n  (struct Q [a:i64 b:i64])\n  (fn main [] -> i32\n{body}\n    0))"),
    )
    .unwrap();
    let vm = Command::new(AIPL).arg("eval").arg(&src).output().unwrap();
    let vm_out = String::from_utf8_lossy(&vm.stdout).to_string() + &String::from_utf8_lossy(&vm.stderr);
    let line = vm_out.lines().find(|l| l.starts_with("Error: ")).unwrap_or_else(|| panic!("{name}: the VM did not fail: {vm_out}"));
    let msg = line.trim_start_matches("Error: \"").trim_end_matches('"').replace("\\\"", "\"");
    // drop the position: "... in 'f' at 3:10: (req" -> "... in 'f': (req"
    let (head, rest) = msg.split_once("' at ").unwrap();
    let vm_msg = format!("{head}': {}", rest.split_once(": ").unwrap().1);

    let wasm = dir.join("prog.wasm");
    let exe = dir.join("prog");
    let c = Command::new(AIPL).arg("compile").arg(&src).arg("-o").arg(&wasm).output().unwrap();
    assert!(c.status.success(), "{name}: {}", String::from_utf8_lossy(&c.stderr));
    let c = Command::new(AIPL).arg("compile").arg(&src).arg("--exe").arg("-o").arg(&exe).output().unwrap();
    assert!(c.status.success(), "{name}: {}", String::from_utf8_lossy(&c.stderr));
    let run = |cmd: &mut Command, prog: &PathBuf| {
        let o = cmd.output().unwrap();
        assert_eq!(o.status.code(), Some(134), "{name}: {}", String::from_utf8_lossy(&o.stderr));
        let err = String::from_utf8_lossy(&o.stderr).to_string();
        err.trim_end().strip_prefix(&format!("{}: ", prog.display())).unwrap_or(&err).to_string()
    };
    let wasm_out = run(Command::new(RUNNER).arg(&wasm), &wasm);
    let native_out = run(&mut Command::new(&exe), &exe);
    assert_eq!(wasm_out, native_out, "{name}: aipl-run and native print different chains");
    assert!(wasm_out.ends_with("\n  at main"), "{name}: {wasm_out}");
    let wasm_msg = wasm_out.lines().next().unwrap().to_string();
    let native_msg = native_out.lines().next().unwrap().to_string();
    let _ = std::fs::remove_dir_all(&dir);
    [vm_msg, wasm_msg, native_msg]
}

/// Every backend stops with the same message, which names `check`.
fn assert_fails(name: &str, body: &str, check: &str) {
    let [vm, wasm, native] = failures(name, body);
    assert!(vm.contains(check), "{name}: VM: {vm}");
    assert_eq!(vm, wasm, "{name}: VM and wasm differ");
    assert_eq!(vm, native, "{name}: VM and native differ");
}

#[test]
fn double_free() {
    assert_fails(
        "double_free",
        "    (let h:(ptr heap.Heap) (call heap.make))
    (let p:(ptr P) (call (heap.create P) h))
    (call (heap.destroy P) h p)
    (call (heap.destroy P) h p)",
        "(req (call heap.not_freed_already h p))",
    );
}

#[test]
fn free_from_another_heap() {
    assert_fails(
        "another_heap",
        "    (let h:(ptr heap.Heap) (call heap.make))
    (let g:(ptr heap.Heap) (call heap.make))
    (let p:(ptr P) (call (heap.create P) h))
    (call (heap.destroy P) g p)",
        "(req (call heap.allocated_by h p))",
    );
}

#[test]
fn free_of_memory_no_heap_allocated() {
    assert_fails(
        "not_a_heap_block",
        "    (let h:(ptr heap.Heap) (call heap.make))
    (call (heap.destroy P) h (new P))",
        "(req (call heap.allocated_by h p))",
    );
}

#[test]
fn free_with_the_wrong_type() {
    // a P is 8 bytes, a Q 16
    assert_fails(
        "wrong_type",
        "    (let h:(ptr heap.Heap) (call heap.make))
    (let p:(ptr P) (call (heap.create P) h))
    (call (heap.destroy Q) h (ptr.cast Q (ptr.addr p)))",
        "(req (call heap.size_matches p n)) with h = ",
    );
}

#[test]
fn free_with_the_wrong_size() {
    assert_fails(
        "wrong_size",
        "    (let h:(ptr heap.Heap) (call heap.make))
    (let b:i32 (call heap.raw h 100))
    (call heap.free_raw h b 99)",
        "(req (call heap.size_matches p n))",
    );
}

#[test]
fn write_after_free() {
    // caught when the block is handed out again
    assert_fails(
        "write_after_free",
        "    (let h:(ptr heap.Heap) (call heap.make))
    (let a:(arr i32) (call (heap.array i32) h 10))
    (call (heap.free_array i32) h a)
    (mem.store32 (+ (arr.addr a) 20) 7)
    (let b:(arr i32) (call (heap.array i32) h 10))",
        "(req (not (call heap.written_after_free p)))",
    );
}

#[test]
fn an_array_size_that_overflows() {
    assert_fails(
        "overflow",
        "    (let h:(ptr heap.Heap) (call heap.make))
    (let a:(arr i64) (call (heap.array i64) h 235000000))",
        "(req (lte n 1879048184))",
    );
}
