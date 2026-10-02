//! Threads and atomics in compiled code (AIPL_SPEC.md 4.D). A threaded module
//! imports a shared memory and the wasi-threads `thread-spawn` function; this
//! file's host provides both (wasmtime dropped wasi-threads in v47, so AIPL's
//! hosts implement it). Every program also runs in the VM and must agree.

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::parser::Parser;
use aipl_core::vm::{Value, VM};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, OnceLock};
use wasmtime::{Config, Engine, Linker, MemoryType, Module, SharedMemory, Store};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::p2::pipe::MemoryOutputPipe;
use wasmtime_wasi::WasiCtxBuilder;

fn compile(src: &str) -> (aipl_core::ast::Module, Vec<u8>) {
    let m = Parser::parse(src).unwrap_or_else(|e| panic!("parse: {e}"));
    TypeChecker::new().check_module(&m).unwrap_or_else(|e| panic!("check: {e}"));
    let wasm = WasmCompiler::compile(&m).unwrap_or_else(|e| panic!("compile: {e}"));
    let mut features = wasmparser::WasmFeatures::default();
    features.insert(wasmparser::WasmFeatures::THREADS);
    wasmparser::Validator::new_with_features(features).validate_all(&wasm).expect("valid wasm");
    (m, wasm)
}

fn vm_run(m: &aipl_core::ast::Module, f: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    vm.load_module(m.clone());
    vm.invoke(f, vec![])
}

/// What one host-side thread needs to instantiate the module again.
struct Shared {
    engine: Engine,
    module: Module,
    linker: Linker<WasiP1Ctx>,
    stdout: MemoryOutputPipe,
}

/// Runs a zero-argument export of a threaded module. `thread-spawn` starts an
/// OS thread with its own store and WASI context (stdout shared), instantiates
/// the module on the same shared memory, and calls wasi_thread_start.
fn run_threaded(wasm: &[u8], f: &str) -> (Result<i32, wasmtime::Error>, String) {
    let mut config = Config::new();
    config.wasm_threads(true).shared_memory(true);
    let engine = Engine::new(&config).unwrap();
    let module = Module::new(&engine, wasm).expect("module");
    let memory = SharedMemory::new(&engine, MemoryType::shared(16, 1024)).unwrap();
    let stdout = MemoryOutputPipe::new(1 << 20);
    let ctx = || WasiCtxBuilder::new().stdout(stdout.clone()).inherit_stderr().build_p1();

    let shared: Arc<OnceLock<Shared>> = Arc::new(OnceLock::new());
    let next_tid = Arc::new(AtomicI32::new(1));
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
    let mut store = Store::new(&engine, ctx());
    linker.define(&mut store, "env", "memory", memory.clone()).unwrap();
    let (s2, t2) = (shared.clone(), next_tid.clone());
    linker
        .func_wrap("wasi", "thread-spawn", move |_caller: wasmtime::Caller<'_, WasiP1Ctx>, start_arg: i32| -> i32 {
            let tid = t2.fetch_add(1, Ordering::SeqCst);
            let s = s2.clone();
            std::thread::spawn(move || {
                let sh = s.get().unwrap();
                let wasi = WasiCtxBuilder::new().stdout(sh.stdout.clone()).inherit_stderr().build_p1();
                let mut store = Store::new(&sh.engine, wasi);
                let inst = sh.linker.instantiate(&mut store, &sh.module).expect("thread instance");
                let start = inst.get_typed_func::<(i32, i32), ()>(&mut store, "wasi_thread_start").unwrap();
                if let Err(e) = start.call(&mut store, (tid, start_arg)) {
                    eprintln!("thread {tid} trapped: {e}");
                    std::process::abort();
                }
            });
            tid
        })
        .unwrap();
    let inst = linker.instantiate(&mut store, &module).expect("main instance");
    let _ = shared.set(Shared { engine: engine.clone(), module: module.clone(), linker: linker.clone(), stdout: stdout.clone() });
    let func = inst.get_typed_func::<(), i32>(&mut store, f).unwrap();
    let r = func.call(&mut store, ());
    (r, String::from_utf8_lossy(&stdout.contents()).into_owned())
}

/// Workers share a counter, a lock, and the allocator.
const THREADS: &str = r#"
(module threads
  (struct Job [counter:i32 lock:i32 plain:i32])

  ;; 1000 atomic increments of the shared counter
  (fn add_worker [counter:i32] -> i32
    (loop i 1 1000 1
      (let _old:i32 (atomic.add counter 1)))
    0)

  (fn atomic_counter [] -> i32
    (let c:i32 (mem.alloc 4))
    (let hs:(arr i32) (arr.new i32 4))
    (loop t 0 3 1
      (arr.set i32 hs t (thread.spawn (ref add_worker) c)))
    (loop t 0 3 1
      (let _r:i32 (thread.join (arr.get i32 hs t))))
    (mem.load32 c))

  ;; join returns each worker's result
  (fn square [x:i32] -> i32 (* x x))
  (fn join_results [] -> i32
    (let a:i32 (thread.spawn (ref square) 3))
    (let b:i32 (thread.spawn (ref square) 4))
    (+ (thread.join a) (thread.join b)))

  ;; a lock protects a non-atomic read-modify-write
  (fn locked_worker [job_addr:i32] -> i32
    (let job:(ptr Job) (ptr.cast Job job_addr))
    (let lock:i32 (+ job_addr 4))
    (loop i 1 1000 1
      (atomic.lock lock)
      (put job Job.plain (+ (get job Job.plain) 1))
      (atomic.unlock lock))
    0)
  (fn mutex_counter [] -> i32
    (let job:(ptr Job) (new Job))
    (let hs:(arr i32) (arr.new i32 4))
    (loop t 0 3 1
      (arr.set i32 hs t (thread.spawn (ref locked_worker) (ptr.addr job))))
    (loop t 0 3 1
      (let _r:i32 (thread.join (arr.get i32 hs t))))
    (get job Job.plain))

  ;; concurrent allocation: every block keeps the value its thread wrote
  (fn alloc_worker [tag:i32] -> i32
    (let blocks:(arr i32) (arr.new i32 500))
    (loop i 0 499 1
      (let p:i32 (mem.alloc 8))
      (mem.store32 p tag)
      (mem.store32 (+ p 4) i)
      (arr.set i32 blocks i p))
    (let ok:i32 1)
    (loop i 0 499 1
      (let p:i32 (arr.get i32 blocks i))
      (if (and (eq (mem.load32 p) tag) (eq (mem.load32 (+ p 4)) i)) (block) (set! ok 0)))
    ok)
  (fn concurrent_alloc [] -> i32
    (let hs:(arr i32) (arr.new i32 4))
    (loop t 0 3 1
      (arr.set i32 hs t (thread.spawn (ref alloc_worker) (+ t 100))))
    (let good:i32 0)
    (loop t 0 3 1
      (set! good (+ good (thread.join (arr.get i32 hs t)))))
    good)

  ;; printing from several threads: each thread's text arrives intact
  (fn print_worker [n:i32] -> i32
    (loop i 1 50 1
      (if (eq n 1) (sys.print "one one one one one one") (sys.print "two two two two two two")))
    0)
  (fn concurrent_print [] -> i32
    (let a:i32 (thread.spawn (ref print_worker) 1))
    (let b:i32 (thread.spawn (ref print_worker) 2))
    (+ (thread.join a) (thread.join b))))
"#;

#[test]
fn atomic_counter_across_threads() {
    let (m, wasm) = compile(THREADS);
    assert_eq!(vm_run(&m, "atomic_counter").unwrap(), Value::Int(4000));
    assert_eq!(run_threaded(&wasm, "atomic_counter").0.unwrap(), 4000);
}

#[test]
fn join_returns_the_worker_result() {
    let (m, wasm) = compile(THREADS);
    assert_eq!(vm_run(&m, "join_results").unwrap(), Value::Int(25));
    assert_eq!(run_threaded(&wasm, "join_results").0.unwrap(), 25);
}

#[test]
fn a_lock_protects_plain_updates() {
    let (m, wasm) = compile(THREADS);
    assert_eq!(vm_run(&m, "mutex_counter").unwrap(), Value::Int(4000));
    assert_eq!(run_threaded(&wasm, "mutex_counter").0.unwrap(), 4000);
}

#[test]
fn concurrent_allocations_never_overlap() {
    let (m, wasm) = compile(THREADS);
    assert_eq!(vm_run(&m, "concurrent_alloc").unwrap(), Value::Int(4));
    assert_eq!(run_threaded(&wasm, "concurrent_alloc").0.unwrap(), 4);
}

/// Each thread has its own runtime scratch cells, so concurrent prints never
/// corrupt each other's text. Line breaks may interleave (a print's text and
/// its newline are written as two pieces), as with unsynchronised output in
/// most languages; use a lock when whole lines matter.
#[test]
fn concurrent_printing_keeps_text_intact() {
    let (_, wasm) = compile(THREADS);
    let (r, out) = run_threaded(&wasm, "concurrent_print");
    assert_eq!(r.unwrap(), 0);
    assert_eq!(out.matches("one one one one one one").count(), 50, "{out}");
    assert_eq!(out.matches("two two two two two two").count(), 50, "{out}");
    assert_eq!(out.matches('\n').count(), 100);
    assert_eq!(out.len(), 100 * ("one one one one one one".len() + 1));
}

/// Atomics work in any module, threaded or not.
const ATOMICS: &str = r#"
(module atomics
  (fn add_returns_previous [] -> i32
    (let p:i32 (mem.alloc 4))
    (mem.store32 p 40)
    (let old:i32 (atomic.add p 2))
    (+ (* 100 old) (mem.load32 p)))
  (fn cas [] -> i32
    (let p:i32 (mem.alloc 4))
    (mem.store32 p 5)
    (let a:bool (atomic.cas p 5 9))
    (let b:bool (atomic.cas p 5 7))
    (+ (if a 10 0) (+ (if b 1 0) (* 100 (mem.load32 p)))))
  (fn lock_unlock [] -> i32
    (let l:i32 (mem.alloc 4))
    (atomic.lock l)
    (let held:i32 (mem.load32 l))
    (atomic.unlock l)
    (+ (* 10 held) (mem.load32 l)))
  ;; unlocking a word that is not a held lock fails in both backends
  (fn bad_unlock [] -> i32
    (let l:i32 (mem.alloc 4))
    (atomic.unlock l)
    1))
"#;

#[test]
fn atomics_agree_without_threads() {
    let (m, wasm) = compile(ATOMICS);
    let engine = Engine::default();
    let module = Module::new(&engine, &wasm).unwrap();
    let call = |f: &str| {
        let mut store = Store::new(&engine, ());
        let inst = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
        inst.get_typed_func::<(), i32>(&mut store, f).unwrap().call(&mut store, ())
    };
    for (f, expected) in [("add_returns_previous", 4042i32), ("cas", 910), ("lock_unlock", 10)] {
        assert_eq!(vm_run(&m, f).unwrap(), Value::Int(expected as i64), "VM {f}");
        assert_eq!(call(f).unwrap(), expected, "wasm {f}");
    }
    assert!(vm_run(&m, "bad_unlock").is_err());
    assert!(call("bad_unlock").is_err());
}
