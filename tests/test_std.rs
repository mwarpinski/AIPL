//! The standard library (aipl_src/std/) must behave identically in the VM and
//! compiled under WASI. Every function with an i32/bool result and all-i32
//! parameters is called over fixed argument tuples in both backends (the
//! differential rule of tests/test_differential.rs); printing is checked
//! byte for byte on stdout.

use aipl_core::ast::{Module, Type};
use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use std::path::{Path, PathBuf};
use wasmtime::{Engine, Linker, Module as WasmModule, Store, Val};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::p2::pipe::MemoryOutputPipe;
use wasmtime_wasi::{FsPerms, WasiCtxBuilder};

/// The VM resolves paths against the process cwd; serialise VM runs that chdir.
static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_std_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn load(path: &Path) -> (Module, Vec<u8>) {
    let module = Resolver::resolve(path).unwrap_or_else(|e| panic!("{e}"));
    TypeChecker::new().check_module(&module).unwrap_or_else(|e| panic!("{e}"));
    let wasm = WasmCompiler::compile(&module).unwrap_or_else(|e| panic!("{e}"));
    (module, wasm)
}

fn vm_call(module: &Module, f: &str, args: &[i32], dir: &Path) -> Result<i64, String> {
    let _serial = CWD_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let prev = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir).unwrap();
    let mut vm = VM::new();
    vm.load_module(module.clone());
    let r = vm.invoke(f, args.iter().map(|a| Value::Int(*a as i64)).collect());
    std::env::set_current_dir(prev).unwrap();
    match r? {
        Value::Int(i) => Ok(i as i32 as i64),
        Value::Bool(b) => Ok(b as i64),
        other => Err(format!("unexpected VM value {other:?}")),
    }
}

/// Fresh instance per call, so each call sees fresh memory like a fresh VM.
fn wasm_call(wasm: &[u8], f: &str, args: &[i32], dir: &Path) -> (Result<i64, String>, String) {
    let engine = Engine::default();
    let module = WasmModule::new(&engine, wasm).unwrap();
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
    let stdout = MemoryOutputPipe::new(1 << 16);
    let ctx = WasiCtxBuilder::new()
        .stdout(stdout.clone())
        .preopened_dir(dir, ".", FsPerms::ReadWrite)
        .unwrap()
        .build_p1();
    let mut store = Store::new(&engine, ctx);
    let inst = linker.instantiate(&mut store, &module).unwrap();
    let func = inst.get_func(&mut store, f).unwrap();
    let params: Vec<Val> = args.iter().map(|a| Val::I32(*a)).collect();
    let mut out = [Val::I32(0)];
    let r = func.call(&mut store, &params, &mut out).map(|_| out[0].unwrap_i32() as i64).map_err(|e| e.to_string());
    drop(store);
    (r, String::from_utf8(stdout.contents().to_vec()).unwrap())
}

const SAMPLES: [i32; 7] = [0, 1, 7, 50, 100, -1, i32::MAX];

fn arg_tuples(arity: usize) -> Vec<Vec<i32>> {
    let mut v: Vec<Vec<i32>> = SAMPLES.iter().map(|s| vec![*s; arity]).collect();
    if arity > 1 {
        v.push((0..arity).map(|i| SAMPLES[i % SAMPLES.len()]).collect());
        v.push((0..arity).map(|i| SAMPLES[(i + 3) % SAMPLES.len()]).collect());
    }
    v
}

fn is_contract_failure(e: &str) -> bool {
    e.starts_with("Pre-condition") || e.starts_with("Post-condition")
}

/// Every eligible function of the module, in both backends, over every tuple.
fn differential_module(name: &str) -> usize {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let (module, wasm) = load(&root.join(format!("aipl_src/std/{name}.aipl")));
    let dir = scratch(name);
    let mut compared = 0;
    for f in &module.functions {
        // Only this module's own functions (imports are covered in their own run).
        if f.name.contains('.') || !matches!(f.return_type, Type::I32 | Type::Bool) {
            continue;
        }
        if !f.params.iter().all(|(_, t)| *t == Type::I32) {
            continue;
        }
        for args in arg_tuples(f.params.len()).into_iter().take(if f.params.is_empty() { 1 } else { 9 }) {
            let vm = vm_call(&module, &f.name, &args, &dir);
            let (wt, _) = wasm_call(&wasm, &f.name, &args, &dir);
            match (&vm, &wt) {
                (Ok(a), Ok(b)) => assert_eq!(a, b, "{name}.{} {args:?}: VM {a} vs wasm {b}", f.name),
                (Err(_), Err(_)) => {}
                (Err(e), Ok(_)) if is_contract_failure(e) => {}
                _ => panic!("{name}.{} {args:?}: VM {vm:?} vs wasm {wt:?}", f.name),
            }
            compared += 1;
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    compared
}

#[test]
fn std_str_agrees_in_both_backends() {
    assert!(differential_module("str") >= 8);
}

#[test]
fn std_fmt_agrees_in_both_backends() {
    // uint/int/hex_to_bytes and hex_digit over every tuple (writes to address
    // 0..100 fail in both backends: the reserved-block guard), plus the runner
    assert!(differential_module("fmt") >= 30);
}

#[test]
fn std_io_agrees_in_both_backends() {
    // includes run_io_tests, which writes, reads back, and deletes a file
    assert!(differential_module("io") >= 1);
}

#[test]
fn std_collections_agree_in_both_backends() {
    // the run_<module>_tests runners plus every helper with i32 params
    assert!(differential_module("vec") >= 20);
    assert!(differential_module("map") >= 8);
    assert!(differential_module("strmap") >= 1);
    assert!(differential_module("buf") >= 1);
}

#[test]
fn std_printing_is_byte_exact_under_wasi() {
    let dir = scratch("print");
    let src = dir.join("printing.aipl");
    std::fs::write(&src, r#"
(module printing
  (import io)
  (fn main [] -> i32
    (call io.print_int -2147483648)
    (call io.println "")
    (call io.println_int "x=" 0)
    (call io.println_int "max " 2147483647)
    (call io.eprintln "to stderr")
    (call io.println "done")
    7))"#).unwrap();
    let (module, wasm) = load(&src);
    let (r, out) = wasm_call(&wasm, "main", &[], &dir);
    assert_eq!(r, Ok(7));
    assert_eq!(out, "-2147483648\nx=0\nmax 2147483647\ndone\n");
    assert_eq!(vm_call(&module, "main", &[], &dir), Ok(7));
    let _ = std::fs::remove_dir_all(&dir);
}

/// fmt.f64_fixed against Rust's own exact formatting ({:.N}, the exact
/// decimal value rounded half to even, as C's printf and Python's format):
/// 20,000 doubles across the whole range (subnormals, huge values, exact
/// ties), 0-20 digits, compiled to wasm; plus the special values.
#[test]
fn f64_fixed_matches_exact_formatting() {
    let dir = scratch("f64_fixed");
    let src = dir.join("w.aipl");
    std::fs::write(
        &src,
        "(module w (import fmt)
           (fn fmt_at [lo:i32 hi:i32 d:i32 out:i32] -> i32
             (call fmt.f64_fixed (f64.reinterpret_i64 (bitor (i64.extend_u lo) (shl (i64.extend_s hi) 32i64))) d out))
           (fn alloc [n:i32] -> i32 (mem.alloc n)))",
    )
    .unwrap();
    let (_, wasm) = load(&src);
    let engine = Engine::default();
    let module = WasmModule::new(&engine, &wasm).unwrap();
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
    let mut store = Store::new(&engine, WasiCtxBuilder::new().build_p1());
    let inst = linker.instantiate(&mut store, &module).unwrap();
    let alloc = inst.get_typed_func::<i32, i32>(&mut store, "alloc").unwrap();
    let fmt_at = inst.get_typed_func::<(i32, i32, i32, i32), i32>(&mut store, "fmt_at").unwrap();
    let mem = inst.get_memory(&mut store, "memory").unwrap();
    let out = alloc.call(&mut store, 400).unwrap();
    let mut format = |x: f64, d: usize| -> String {
        let bits = x.to_bits();
        let n = fmt_at.call(&mut store, (bits as u32 as i32, (bits >> 32) as u32 as i32, d as i32, out)).unwrap();
        String::from_utf8(mem.data(&store)[out as usize..(out + n) as usize].to_vec()).unwrap()
    };
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut values: Vec<f64> = vec![0.0, -0.0, 0.5, 1.5, 2.5, -2.5, 0.125, 0.375, 1e-310, 5e-324, f64::MAX, f64::MIN_POSITIVE, 1e300, 123456789.987654321];
    for _ in 0..20000 {
        let r = next();
        let x = match r % 4 {
            // any bit pattern (finite)
            0 => f64::from_bits(r >> 1 | (r & 1) << 63),
            // moderate values
            1 => (r % 2_000_000_000) as f64 / 1000.0 - 1e6,
            // exact ties at the digit count: k / 2^j
            2 => (r % 100_000) as f64 / (1u64 << (r % 12)) as f64,
            // small magnitudes
            _ => (r % 1_000_000) as f64 * 1e-12,
        };
        if x.is_finite() {
            values.push(x);
        }
    }
    for (i, x) in values.iter().enumerate() {
        let d = i % 21;
        assert_eq!(format(*x, d), format!("{:.*}", d, x), "{x:e} with {d} digits (bits {:016x})", x.to_bits());
    }
    // special values print as glibc's printf does
    assert_eq!(format(f64::INFINITY, 3), "inf");
    assert_eq!(format(f64::NEG_INFINITY, 3), "-inf");
    assert_eq!(format(f64::NAN, 3), "nan");
    assert_eq!(format(-f64::NAN, 3), "-nan");
    let _ = std::fs::remove_dir_all(&dir);
}
