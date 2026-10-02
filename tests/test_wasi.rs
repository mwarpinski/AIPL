//! Compiled AIPL that does I/O (P6): `sys.print`, `fs.*`, `sys.exit`, and
//! string literals lower to WASI preview1 imports plus a data segment, and
//! run under wasmtime with a WASI context. Every case compares against the
//! VM, which is the same program's other implementation.

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::parser::Parser;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use std::path::{Path, PathBuf};
use wasmtime::{Engine, Instance, Linker, Module as WasmModule, Store, TypedFunc};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::p2::pipe::MemoryOutputPipe;
use wasmtime_wasi::{FsPerms, WasiCtxBuilder};

/// The VM resolves file paths against the process cwd, so VM runs that touch
/// files are serialised and chdir'd into their own scratch directory.
static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aipl_wasi_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn compile(src: &str) -> (aipl_core::ast::Module, Vec<u8>) {
    let module = Parser::parse(src).expect("parse");
    TypeChecker::new().check_module(&module).expect("check");
    let wasm = WasmCompiler::compile(&module).expect("wasm compile");
    wasmparser::Validator::new().validate_all(&wasm).expect("validate");
    (module, wasm)
}

fn vm_run_in(module: &aipl_core::ast::Module, f: &str, cwd: &Path) -> Result<Value, String> {
    let _serial = CWD_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let prev = std::env::current_dir().unwrap();
    std::env::set_current_dir(cwd).unwrap();
    let mut vm = VM::new();
    vm.load_module(module.clone());
    let r = vm.invoke(f, vec![]);
    std::env::set_current_dir(prev).unwrap();
    r
}

struct Wasi {
    store: Store<WasiP1Ctx>,
    instance: Instance,
    stdout: MemoryOutputPipe,
}

fn instantiate(wasm: &[u8], preopen: &Path) -> Wasi {
    let engine = Engine::default();
    let module = WasmModule::new(&engine, wasm).expect("wasmtime module");
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).expect("link wasi");
    let stdout = MemoryOutputPipe::new(64 * 1024);
    let ctx = WasiCtxBuilder::new()
        .stdout(stdout.clone())
        .inherit_stderr()
        .preopened_dir(preopen, ".", FsPerms::ReadWrite)
        .expect("preopen")
        .build_p1();
    let mut store = Store::new(&engine, ctx);
    let instance = linker.instantiate(&mut store, &module).expect("instantiate with WASI");
    Wasi { store, instance, stdout }
}

fn call_i32(w: &mut Wasi, name: &str) -> Result<i32, wasmtime::Error> {
    let f: TypedFunc<(), i32> = w.instance.get_typed_func(&mut w.store, name).expect("export");
    f.call(&mut w.store, ())
}

fn import_names(wasm: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        if let wasmparser::Payload::ImportSection(reader) = payload.unwrap() {
            for imp in reader {
                let imp = imp.unwrap();
                names.push(format!("{}::{}", imp.module, imp.name));
            }
        }
    }
    names
}

// ---------------------------------------------------------------------------

#[test]
fn file_io_self_test_gives_the_same_result_in_vm_and_wasi() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let module = Resolver::resolve(&root.join("aipl_src/file_io.aipl")).expect("resolve file_io.aipl");
    TypeChecker::new().check_module(&module).expect("check");
    let wasm = WasmCompiler::compile(&module).expect("file_io.aipl must compile to wasm now");
    wasmparser::Validator::new().validate_all(&wasm).expect("validate");

    let imports = import_names(&wasm);
    for needed in ["fd_write", "fd_read", "path_open", "fd_close", "path_unlink_file"] {
        assert!(
            imports.contains(&format!("wasi_snapshot_preview1::{needed}")),
            "missing import {needed}; have {imports:?}"
        );
    }

    let vm_dir = scratch_dir("fileio_vm");
    let vm_result = vm_run_in(&module, "run_file_io_tests", &vm_dir).expect("VM run");

    let wasi_dir = scratch_dir("fileio_wasi");
    let mut w = instantiate(&wasm, &wasi_dir);
    let wasm_result = call_i32(&mut w, "run_file_io_tests").expect("wasm run");

    assert_eq!(vm_result, Value::Int(1), "VM: real disk round-trip must pass");
    assert_eq!(wasm_result, 1, "wasm+WASI: the same round-trip must pass");
    // Both self-tests delete their file afterwards.
    assert!(std::fs::read_dir(&vm_dir).unwrap().next().is_none(), "VM left a file behind");
    assert!(std::fs::read_dir(&wasi_dir).unwrap().next().is_none(), "wasm left a file behind");
    let _ = std::fs::remove_dir_all(&vm_dir);
    let _ = std::fs::remove_dir_all(&wasi_dir);
}

#[test]
fn sys_print_writes_each_argument_and_a_newline_to_stdout() {
    let (_m, wasm) = compile(
        r#"(module m
             (fn main [] -> i32
               (sys.print "hello, wasi")
               (sys.print "second line")
               (sys.print "hello, wasi")
               0))"#,
    );
    assert_eq!(import_names(&wasm), vec!["wasi_snapshot_preview1::fd_write".to_string()]);
    let dir = scratch_dir("print");
    let mut w = instantiate(&wasm, &dir);
    assert_eq!(call_i32(&mut w, "main").unwrap(), 0);
    let out = String::from_utf8(w.stdout.contents().to_vec()).unwrap();
    assert_eq!(out, "hello, wasi\nsecond line\nhello, wasi\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn string_literals_are_interned_with_lengths_and_compare_by_identity() {
    // str.len reads the interned length; identical literals share one address.
    let (m, wasm) = compile(
        r#"(module m
             (fn len [] -> i32 (str.len "hello"))
             (fn same [] -> bool (eq "abc" "abc"))
             (fn diff [] -> bool (eq "abc" "abd"))
             (fn empty [] -> i32 (str.len "")))"#,
    );
    // No I/O here, so no imports: instantiates without WASI too.
    assert!(import_names(&wasm).is_empty());
    let dir = scratch_dir("strings");
    let mut w = instantiate(&wasm, &dir);
    let mut vm = VM::new();
    vm.load_module(m.clone());
    assert_eq!(call_i32(&mut w, "len").unwrap(), 5);
    assert_eq!(vm.invoke("len", vec![]).unwrap(), Value::Int(5));
    assert_eq!(call_i32(&mut w, "same").unwrap(), 1);
    assert_eq!(vm.invoke("same", vec![]).unwrap(), Value::Bool(true));
    assert_eq!(call_i32(&mut w, "diff").unwrap(), 0);
    assert_eq!(vm.invoke("diff", vec![]).unwrap(), Value::Bool(false));
    assert_eq!(call_i32(&mut w, "empty").unwrap(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Literals are no longer squeezed into a fixed area: 600 bytes compile and
/// run in both backends. Past the 1 MiB limit the compile fails with a message
/// rather than truncating.
#[test]
fn large_string_literals_compile_and_the_limit_is_a_compile_error() {
    let big = "x".repeat(600);
    let src = format!("(module m (fn f [] -> i32 (str.len \"{big}\")))");
    let (module, wasm) = compile(&src);
    let mut w = instantiate(&wasm, &std::env::temp_dir());
    assert_eq!(call_i32(&mut w, "f").unwrap(), 600);
    let mut vm = VM::new();
    vm.load_module(module);
    assert_eq!(vm.invoke("f", vec![]).unwrap(), Value::Int(600));

    let huge = "x".repeat(1 << 20);
    let src = format!("(module m (fn f [] -> i32 (str.len \"{huge}\")))");
    let module = Parser::parse(&src).unwrap();
    TypeChecker::new().check_module(&module).unwrap();
    let err = WasmCompiler::compile(&module).unwrap_err();
    assert!(err.contains("string literals need") && err.contains("fit before the heap"), "got {err}");
}

#[test]
fn fs_open_of_a_missing_file_returns_minus_one_in_both_backends() {
    let src = r#"(module m
        (fn main [] -> i32
          (let p:i32 (mem.alloc 8))
          (mem.store8 p 122) (mem.store8 (+ p 1) 122)      ;; "zz"
          (fs.open p 2 0)))"#;
    let (m, wasm) = compile(src);
    let dir = scratch_dir("missing");
    assert_eq!(vm_run_in(&m, "main", &dir).unwrap(), Value::Int(-1));
    let mut w = instantiate(&wasm, &dir);
    assert_eq!(call_i32(&mut w, "main").unwrap(), -1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fs_write_then_read_round_trip_agrees_byte_for_byte() {
    // Writes 4 bytes, reads them back into a fresh buffer, returns the sum of
    // the read bytes plus 1000 * bytes_read. Same value in both backends, and
    // the wasm side's file lands in the preopened directory.
    let src = r#"(module m
        (fn main [] -> i32
          (let path:i32 (mem.alloc 8))
          (mem.store8 path 116) (mem.store8 (+ path 1) 46) (mem.store8 (+ path 2) 98)
          (mem.store8 (+ path 3) 105) (mem.store8 (+ path 4) 110)          ;; "t.bin"
          (let out:i32 (mem.alloc 4))
          (mem.store8 out 1) (mem.store8 (+ out 1) 2) (mem.store8 (+ out 2) 3) (mem.store8 (+ out 3) 250)
          (let w:i32 (fs.open path 5 1))
          (let written:i32 (fs.write w out 4))
          (let _c:i32 (fs.close w))
          (let in:i32 (mem.alloc 4))
          (let r:i32 (fs.open path 5 0))
          (let n:i32 (fs.read r in 4))
          (let _d:i32 (fs.close r))
          (+ (* 1000 (+ n written))
             (+ (+ (mem.load8 in) (mem.load8 (+ in 1))) (+ (mem.load8 (+ in 2)) (mem.load8 (+ in 3)))))))"#;
    let (m, wasm) = compile(src);
    let vm_dir = scratch_dir("rt_vm");
    let wasi_dir = scratch_dir("rt_wasi");
    let expected: i32 = 8 * 1000 + (1 + 2 + 3 + 250);
    assert_eq!(vm_run_in(&m, "main", &vm_dir).unwrap(), Value::Int(expected as i64));
    let mut w = instantiate(&wasm, &wasi_dir);
    assert_eq!(call_i32(&mut w, "main").unwrap(), expected);
    assert_eq!(std::fs::read(wasi_dir.join("t.bin")).unwrap(), vec![1, 2, 3, 250]);
    assert_eq!(std::fs::read(vm_dir.join("t.bin")).unwrap(), vec![1, 2, 3, 250]);
    let _ = std::fs::remove_dir_all(&vm_dir);
    let _ = std::fs::remove_dir_all(&wasi_dir);
}

#[test]
fn sys_exit_terminates_with_the_given_code() {
    let (m, wasm) = compile("(module m (fn main [] -> i32 (sys.exit 7) 0))");
    assert_eq!(import_names(&wasm), vec!["wasi_snapshot_preview1::proc_exit".to_string()]);
    let dir = scratch_dir("exit");
    let mut w = instantiate(&wasm, &dir);
    let err = call_i32(&mut w, "main").unwrap_err();
    let exit = err.downcast_ref::<wasmtime_wasi::I32Exit>().expect("proc_exit should surface as I32Exit");
    assert_eq!(exit.0, 7);
    // The VM refuses to kill its host and reports the request instead.
    let vm_err = vm_run_in(&m, "main", &dir).unwrap_err();
    assert!(vm_err.contains("sys.exit(7)"), "got {vm_err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn modules_without_io_have_no_imports_and_still_run_without_wasi() {
    let (_m, wasm) = compile("(module m (fn f [] -> i32 (+ 40 2)))");
    assert!(import_names(&wasm).is_empty());
    let engine = Engine::default();
    let module = WasmModule::new(&engine, &wasm).unwrap();
    let mut store = Store::new(&engine, ());
    let instance = Instance::new(&mut store, &module, &[]).unwrap();
    let f: TypedFunc<(), i32> = instance.get_typed_func(&mut store, "f").unwrap();
    assert_eq!(f.call(&mut store, ()).unwrap(), 42);
}

// ---------------------------------------------------------------------------
// The example program: AIPL that reads a file, counts, and prints, run in both
// backends. This is the capability demonstrated in AIPL rather than in Rust.
// ---------------------------------------------------------------------------

const WC_INPUT: &str = "hello world\nfoo bar baz\n\nlast line";

#[test]
fn word_count_example_reads_counts_and_prints_in_both_backends() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let module = Resolver::resolve(&root.join("examples/word_count.aipl")).expect("resolve example");
    TypeChecker::new().check_module(&module).expect("check");
    let wasm = WasmCompiler::compile(&module).expect("compile");
    wasmparser::Validator::new().validate_all(&wasm).expect("validate");

    // lines: "hello world", "foo bar baz", "", "last line" (unterminated) = 4
    // words: hello world foo bar baz last line = 7; bytes = input length.
    let expected_stdout = format!("lines: 4\nwords: 7\nbytes: {}\n", WC_INPUT.len());

    let vm_dir = scratch_dir("wc_vm");
    std::fs::write(vm_dir.join("input.txt"), WC_INPUT).unwrap();
    assert_eq!(vm_run_in(&module, "main", &vm_dir).unwrap(), Value::Int(4));

    let wasi_dir = scratch_dir("wc_wasi");
    std::fs::write(wasi_dir.join("input.txt"), WC_INPUT).unwrap();
    let mut w = instantiate(&wasm, &wasi_dir);
    assert_eq!(call_i32(&mut w, "main").unwrap(), 4);
    let out = String::from_utf8(w.stdout.contents().to_vec()).unwrap();
    assert_eq!(out, expected_stdout);

    let _ = std::fs::remove_dir_all(&vm_dir);
    let _ = std::fs::remove_dir_all(&wasi_dir);
}

#[test]
fn word_count_example_reports_a_missing_input_file_in_both_backends() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let module = Resolver::resolve(&root.join("examples/word_count.aipl")).unwrap();
    let wasm = WasmCompiler::compile(&module).unwrap();
    let dir = scratch_dir("wc_missing");
    assert_eq!(vm_run_in(&module, "main", &dir).unwrap(), Value::Int(-1));
    let mut w = instantiate(&wasm, &dir);
    assert_eq!(call_i32(&mut w, "main").unwrap(), -1);
    assert!(w.stdout.contents().is_empty(), "error goes to stderr, not stdout");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_shipped_sample_input_gives_the_documented_counts() {
    // examples/input.txt is what a reader gets when following the header
    // comment of word_count.aipl; pin its numbers so the docs stay honest.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let module = Resolver::resolve(&root.join("examples/word_count.aipl")).unwrap();
    let wasm = WasmCompiler::compile(&module).unwrap();
    let dir = scratch_dir("wc_sample");
    std::fs::copy(root.join("examples/input.txt"), dir.join("input.txt")).unwrap();
    let mut w = instantiate(&wasm, &dir);
    assert_eq!(call_i32(&mut w, "main").unwrap(), 4);
    let out = String::from_utf8(w.stdout.contents().to_vec()).unwrap();
    assert_eq!(out, "lines: 4\nwords: 15\nbytes: 81\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn str_ptr_and_escapes_agree_between_backends() {
    // str.ptr gives a readable pointer in both backends (different addresses,
    // same bytes); "\n" is one byte; a str variable works like a literal.
    let src = r#"(module m
        (fn first_byte [] -> i32 (mem.load8 (str.ptr "Hello")))
        (fn escape_len [] -> i32 (str.len "a\nb\t\"q\"\\"))
        (fn via_var [] -> i32 (let s:str "wxyz") (+ (str.len s) (mem.load8 (+ (str.ptr s) 3)))))"#;
    let (m, wasm) = compile(src);
    let dir = scratch_dir("strptr");
    let mut w = instantiate(&wasm, &dir);
    let mut vm = VM::new();
    vm.load_module(m.clone());
    assert_eq!(call_i32(&mut w, "first_byte").unwrap(), 72);
    assert_eq!(vm.invoke("first_byte", vec![]).unwrap(), Value::Int(72));
    assert_eq!(call_i32(&mut w, "escape_len").unwrap(), 8);
    assert_eq!(vm.invoke("escape_len", vec![]).unwrap(), Value::Int(8));
    assert_eq!(call_i32(&mut w, "via_var").unwrap(), 4 + 122);
    assert_eq!(vm.invoke("via_var", vec![]).unwrap(), Value::Int(4 + 122));
    let _ = std::fs::remove_dir_all(&dir);

    let err = Parser::parse(r#"(module m (fn f [] -> str "bad \q"))"#).unwrap_err();
    assert!(err.contains(r"Unknown escape sequence \q"), "got {err}");
}

#[test]
fn fs_write_to_fd_1_is_stdout_in_both_backends() {
    // Printing without a string value: format bytes in memory, write to fd 1.
    let src = r#"(module m
        (fn main [] -> i32
          (let b:i32 (mem.alloc 4))
          (mem.store8 b 79) (mem.store8 (+ b 1) 75) (mem.store8 (+ b 2) 10)   ;; "OK\n"
          (fs.write 1 b 3)))"#;
    let (m, wasm) = compile(src);
    let dir = scratch_dir("fd1");
    assert_eq!(vm_run_in(&m, "main", &dir).unwrap(), Value::Int(3), "VM writes 3 bytes to stdout");
    let mut w = instantiate(&wasm, &dir);
    assert_eq!(call_i32(&mut w, "main").unwrap(), 3);
    assert_eq!(String::from_utf8(w.stdout.contents().to_vec()).unwrap(), "OK\n");
    let _ = std::fs::remove_dir_all(&dir);
}

/// args.* and env.* (through std/os) report the same command line and
/// environment in the VM (set_args / set_env) and under wasmtime (WASI ctx).
#[test]
fn command_line_and_environment_agree_in_both_backends() {
    let dir = scratch_dir("os");
    let src_path = dir.join("os_probe.aipl");
    std::fs::write(
        &src_path,
        r#"(module os_probe
  (import os)
  (import str)
  (fn argc [] -> i32 (call os.arg_count))
  ;; lengths of argv[1] and argv[2] as 100 * len1 + len2
  (fn arg_lens [] -> i32 (+ (* 100 (get (call os.arg 1) str.Bytes.len)) (get (call os.arg 2) str.Bytes.len)))
  ;; the second byte of argv[2] (a multi-byte argument with a space)
  (fn arg2_byte1 [] -> i32 (call str.byte_at (call os.arg 2) 1))
  (fn missing_arg [] -> i32 (get (call os.arg 9) str.Bytes.len))
  (fn env_value [] -> i32
    (match_result (call str.parse_int (call os.env "AIPL_PROBE")) (ok v v) (err e -1000)))
  ;; a variable whose name is a prefix of another is not confused with it
  (fn env_prefix [] -> i32 (get (call os.env "AIPL_PRO") str.Bytes.len))
  (fn env_empty [] -> i32 (get (call os.env "AIPL_EMPTY") str.Bytes.len))
  (fn env_unset [] -> i32 (get (call os.env "AIPL_UNSET") str.Bytes.len)))"#,
    )
    .unwrap();
    let module = Resolver::resolve(&src_path).expect("resolve");
    TypeChecker::new().check_module(&module).expect("check");
    let wasm = WasmCompiler::compile(&module).expect("compile");
    wasmparser::Validator::new().validate_all(&wasm).expect("validate");

    let args = ["prog.wasm", "hello", "a b"];
    let env = [("AIPL_PROBE", "-42"), ("AIPL_PROBE_LONGER", "1"), ("AIPL_EMPTY", "")];

    let engine = Engine::default();
    let wm = WasmModule::new(&engine, &wasm).unwrap();
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
    let mut b = WasiCtxBuilder::new();
    b.args(&args);
    for (k, v) in env {
        b.env(k, v);
    }
    let mut store = Store::new(&engine, b.build_p1());
    let instance = linker.instantiate(&mut store, &wm).unwrap();
    let mut w = Wasi { store, instance, stdout: MemoryOutputPipe::new(16) };

    let mut vm = VM::new();
    vm.set_args(args.iter().map(|s| s.to_string()).collect());
    vm.set_env(env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect());
    vm.load_module(module);

    for (f, expected) in [
        ("argc", 3),
        ("arg_lens", 503),
        ("arg2_byte1", b' ' as i32),
        ("missing_arg", -1),
        ("env_value", -42),
        ("env_prefix", -1),
        ("env_empty", 0),
        ("env_unset", -1),
    ] {
        assert_eq!(call_i32(&mut w, f).unwrap(), expected, "wasm {f}");
        assert_eq!(vm.invoke(f, vec![]).unwrap(), Value::Int(expected as i64), "VM {f}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The raw ops need 4-aligned out-parameters: wasmtime traps on a misaligned
/// one and the VM fails with an error, rather than either silently working.
#[test]
fn misaligned_args_out_parameters_fail_in_both_backends() {
    let src = "(module m (fn f [] -> i32 (let p:i32 (mem.alloc 16)) (args.sizes (+ p 1) (+ p 8))))";
    let (module, wasm) = compile(src);
    let mut w = instantiate(&wasm, &std::env::temp_dir());
    assert!(call_i32(&mut w, "f").is_err());
    let mut vm = VM::new();
    vm.load_module(module);
    let err = vm.invoke("f", vec![]).unwrap_err();
    assert!(err.contains("not 4-aligned"), "{err}");
}
