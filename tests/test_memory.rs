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

/// (no_leak) through streams (MEMORY_PLAN.md decision 9): a handler reading
/// a str and a source of its own by lines and writing to a buf and a sink
/// of its own is accepted, gives the same output in every backend, and run
/// 1,000 times on an arena reset after each keeps the arena flat.
#[test]
fn a_no_leak_handler_reads_and_writes_through_streams() {
    for (backend, o) in runs("no_leak_stream") {
        let stdout: String = String::from_utf8_lossy(&o.stdout).split_inclusive('\n').filter(|l| !l.starts_with("[AIPL")).collect();
        assert!(o.status.success(), "{backend}: {}", String::from_utf8_lossy(&o.stderr));
        assert_eq!(stdout, "3 lines, 14 bytes; drip 3 lines\ntally 31\nflat\n", "{backend}");
    }
}

/// `--max-memory` (docs/LAUNCH_CHECKLIST.md section 6): the limit is in the
/// module, so the VM, aipl-run, native executables, and the launcher bundle
/// stop at the same place, having printed the same; without it the program
/// runs to the end. Both toolchains write the same module with the limit,
/// and the AIPL command the same executable as the Rust one; a size that is
/// not one is refused alike by both.
#[test]
fn max_memory_stops_every_backend_at_the_same_place() {
    let dir = std::env::temp_dir().join(format!("aipl_memory_{}_hog", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = root().join("tests/aipl/memory/hog.aipl");
    let (wasm, exe, bundle) = (dir.join("p.wasm"), dir.join("p"), dir.join("p_bundle"));
    for (out, extra) in [(&wasm, vec![]), (&exe, vec!["--exe"]), (&bundle, vec!["--exe", "--target", "wasm"])] {
        let o = Command::new(AIPL).arg("compile").args(&extra).args(["--max-memory", "4M"]).arg(&src).arg("-o").arg(out).output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    }
    let printed = |o: &Output| -> String { String::from_utf8_lossy(&o.stdout).split_inclusive('\n').filter(|l| !l.starts_with("[AIPL")).collect() };
    let want: String = (0..=48).step_by(16).map(|i| format!("{i} blocks\n")).collect();
    for (backend, o) in [
        ("VM", Command::new(AIPL).args(["eval", "--max-memory", "4M"]).arg(&src).output().unwrap()),
        ("aipl-run", Command::new(RUNNER).arg(&wasm).output().unwrap()),
        ("native", Command::new(&exe).output().unwrap()),
        ("bundle", Command::new(&bundle).output().unwrap()),
    ] {
        assert!(!o.status.success(), "{backend}: should stop at 4 MiB");
        assert_eq!(printed(&o), want, "{backend}");
        assert!(String::from_utf8_lossy(&o.stderr).contains("at heap.raw"), "{backend}: {}", String::from_utf8_lossy(&o.stderr));
    }
    // without a limit it runs to the end
    let o = Command::new(AIPL).arg("eval").arg(&src).output().unwrap();
    assert!(o.status.success() && printed(&o).ends_with("992 blocks\n"), "{}", String::from_utf8_lossy(&o.stderr));
    // both toolchains: the module, and the AIPL command's executable
    let m = aipl_core::resolver::Resolver::resolve(&src).unwrap();
    let rust = aipl_core::compiler::wasm::WasmCompiler::compile_limited(&m, 64).unwrap();
    assert!(rust == std::fs::read(&wasm).unwrap());
    assert!(aipl_core::selfhost::compile_with_aipl_limited(&src, 64).unwrap() == rust, "the self-hosted toolchain's module");
    let o = Command::new(AIPL).args(["compile", "--self", "--max-memory", "4M"]).arg(&src).arg("-o").arg(dir.join("s.wasm")).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    // sizes that are not one, from the Rust command
    for bad in ["4", "4m", "1K", "3G", "512K1", "M", "", "100000"] {
        let o = Command::new(AIPL).args(["compile", "--max-memory", bad]).arg(&src).arg("-o").arg(dir.join("x.wasm")).output().unwrap();
        assert_eq!(
            String::from_utf8_lossy(&o.stderr).trim(),
            format!("Error: --max-memory {bad}: expected a size from 1M to 2G in whole 64K pages, as 512M"),
            "{bad}"
        );
    }
    for good in ["1M", "2G", "1048576", "65536K", "512M"] {
        assert!(aipl_core::compiler::memory_pages(good).is_ok(), "{good}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
