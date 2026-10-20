//! Views are checked (docs/design/MEMORY_PLAN.md M1): a str.Bytes of memory
//! that has been freed stops the program with "Use after free" when read,
//! in the VM, under aipl-run, and natively, where before it read whatever
//! reused the memory. tests/aipl/memory/: a buffer's view after free and
//! after the buffer grows, and a reader's line after close (a reader of
//! memory, whose line outlives it, then a file's, whose line does not).

use std::path::PathBuf;
use std::process::{Command, Output};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Each backend's output for the program, run in a scratch directory.
fn runs(name: &str) -> Vec<(&'static str, Output)> {
    let dir = std::env::temp_dir().join(format!("aipl_memory_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = root().join(format!("tests/aipl/memory/{name}.aipl"));
    let (wasm, exe) = (dir.join("p.wasm"), dir.join("p"));
    assert!(Command::new(AIPL).arg("compile").arg(&src).arg("-o").arg(&wasm).status().unwrap().success());
    assert!(Command::new(AIPL).args(["compile", "--exe"]).arg(&src).arg("-o").arg(&exe).status().unwrap().success());
    let out = vec![
        ("VM", Command::new(AIPL).arg("eval").arg(&src).current_dir(&dir).output().unwrap()),
        ("aipl-run", Command::new(RUNNER).arg(&wasm).current_dir(&dir).output().unwrap()),
        ("native", Command::new(&exe).current_dir(&dir).output().unwrap()),
    ];
    let _ = std::fs::remove_dir_all(&dir);
    out
}

/// The program stops with "Use after free" at `at` (file:line:col) in every
/// backend, having printed `before` first.
fn stops_with_use_after_free(name: &str, before: &str, at: &str) {
    for (backend, o) in runs(name) {
        let stdout: String = String::from_utf8_lossy(&o.stdout).split_inclusive('\n').filter(|l| !l.starts_with("[AIPL")).collect();
        let stderr = String::from_utf8_lossy(&o.stderr);
        assert!(!o.status.success(), "{backend}: {name} should stop; stdout {stdout:?}");
        assert!(stderr.contains("Use after free"), "{backend}: {name}: {stderr}");
        assert!(stderr.contains(at), "{backend}: {name} should stop at {at}: {stderr}");
        assert_eq!(stdout, before, "{backend}: {name}");
    }
}

#[test]
fn a_view_of_a_freed_buffer_stops_the_program() {
    stops_with_use_after_free("view_after_free", "", "str.byte_at");
}

#[test]
fn a_view_taken_before_a_buffer_grows_stops_the_program() {
    stops_with_use_after_free("view_after_grow", "", "view_after_grow.aipl:12:14");
}

#[test]
fn a_line_kept_past_its_readers_close_stops_the_program_unless_the_reader_read_memory() {
    stops_with_use_after_free("line_after_close", "memory: a line\nbefore close: windowed\n", "str.byte_at");
}

/// (no_leak) (MEMORY_PLAN.md M3): each function that breaks the promise is
/// refused at its first offending site, with the chain down to the
/// allocation; fine (an allocator it is handed) and raw_in_unsafe (the
/// programmer's word) pass. test_bootstrap checks the AIPL toolchain gives
/// the same errors.
#[test]
fn no_leak_names_each_broken_promise_and_its_chain() {
    let o = Command::new(AIPL).current_dir(root()).args(["verify", "tests/aipl/memory/no_leak_refused.aipl"]).output().unwrap();
    assert!(!o.status.success());
    let f = "tests/aipl/memory/no_leak_refused.aipl";
    let want = format!(
        "Error: {f}: 10:5: uses_concat is marked (no_leak) but calls str.concat, which may make memory that is never freed (str.concat calls str.room; str.room uses mem.alloc)\n\
         {f}: 12:5: uses_format is marked (no_leak) but uses format (a str), whose memory is never freed\n\
         {f}: 14:27: uses_forever is marked (no_leak) but uses alloc.forever, whose memory is never freed\n"
    );
    assert_eq!(String::from_utf8_lossy(&o.stderr), want);
}

/// A (no_leak) handler run 10,000 times on a request arena reset after
/// each: accepted, and the arena holds no more after the last request than
/// after the first, in every backend.
#[test]
fn a_no_leak_handler_on_a_request_arena_keeps_memory_flat() {
    for (backend, o) in runs("no_leak_loop") {
        let stdout: String = String::from_utf8_lossy(&o.stdout).split_inclusive('\n').filter(|l| !l.starts_with("[AIPL")).collect();
        assert!(o.status.success(), "{backend}: {}", String::from_utf8_lossy(&o.stderr));
        assert_eq!(stdout, "250000 bytes of replies; arena 4096 bytes after the first request, 4096 after the last\n", "{backend}");
    }
}
