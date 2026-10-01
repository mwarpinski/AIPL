use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use std::fs;
use std::path::Path;
use wasmtime::{Engine, Instance, Linker, Module as WasmModule, Store, TypedFunc};

fn run_self_hosted(src: &str) -> Vec<u8> {
    self_host(src).unwrap_or_else(|e| panic!("{e}"))
}

/// Runs `codegen.compile_module` over `src` in the VM. Returns the module bytes
/// or the compile error code it reported (AIPL_SPEC.md 6.4).
fn self_host(src: &str) -> Result<Vec<u8>, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let codegen_path = root.join("aipl_src/codegen.aipl");
    let module = Resolver::resolve(&codegen_path).expect("resolve codegen.aipl");
    TypeChecker::new().check_module(&module).expect("check codegen.aipl");
    let mut vm = VM::new();
    vm.load_module(module);
    
    let init_res = vm.invoke("init_keywords", vec![]).expect("init_keywords");
    if let Value::Int(res) = init_res {
        assert!(res >= 0, "init_keywords failed with {}", res);
    }
    
    let src_bytes = src.as_bytes();
    let alloc_res = vm.invoke("alloc_src", vec![Value::Int((src_bytes.len() + 16) as i64)]).expect("alloc_src");
    let src_ptr = match alloc_res {
        Value::Int(p) => p as i32,
        other => panic!("expected Int from alloc_src, got {:?}", other),
    };
    
    vm.write_bytes(src_ptr as usize, src_bytes);
    
    let out_len_val = vm.invoke("compile_module", vec![Value::Int(src_ptr as i64), Value::Int(src_bytes.len() as i64)]).expect("compile_module");
    let out_len = match out_len_val {
        Value::Int(l) => l as i32,
        other => panic!("expected Int from compile_module, got {:?}", other),
    };
    if out_len <= 0 {
        let word = |addr: usize| u32::from_le_bytes(vm.read_bytes(addr, 4)[..4].try_into().unwrap()) as usize;
        let code = word(4);
        // compile_call records the unresolved callee's source span in cells 44/48.
        let detail = if code == 1452 {
            let (pos, len) = (word(44), word(48));
            format!(" (unknown function '{}')", String::from_utf8_lossy(&src_bytes[pos..(pos + len).min(src_bytes.len())]))
        } else {
            String::new()
        };
        return Err(format!("compile_module returned {} with compile error {}{}", out_len, code, detail));
    }

    let out_ptr_bytes = vm.read_bytes(60, 4);
    let out_ptr = u32::from_le_bytes(out_ptr_bytes[..4].try_into().unwrap()) as usize;
    let bytes = vm.read_bytes(out_ptr, out_len as usize);
    wasmparser::Validator::new().validate_all(&bytes).map_err(|e| format!("self-hosted output does not validate: {e}"))?;
    Ok(bytes)
}

fn run_rust_backend(src_path: &Path) -> Vec<u8> {
    let module = Resolver::resolve(src_path).expect("resolve test src");
    TypeChecker::new().check_module(&module).expect("check test src");
    WasmCompiler::compile(&module).expect("compile wasm")
}

fn assert_self_hosted_matches_rust(test_name: &str, src: &str) {
    let test_name = test_name.to_string();
    let src = src.to_string();
    let handle = std::thread::Builder::new()
        .name(test_name.clone())
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            let temp_dir = std::env::temp_dir().join("aipl_selfhost_tests");
            fs::create_dir_all(&temp_dir).unwrap();
            let temp_src = temp_dir.join(format!("{}.aipl", test_name));
            fs::write(&temp_src, &src).unwrap();

            let rust_bytes = run_rust_backend(&temp_src);
            let self_bytes = run_self_hosted(&src);

            if rust_bytes != self_bytes {
                eprintln!("Byte parity mismatch in {}!", test_name);
                eprintln!("Rust output len: {}, Self-hosted len: {}", rust_bytes.len(), self_bytes.len());
                
                // Parse code section to find which function differs
                let find_code_sec = |bytes: &[u8]| -> Option<(usize, usize)> {
                    let mut pos = 8; // skip header
                    while pos < bytes.len() {
                        let sec_id = bytes[pos];
                        pos += 1;
                        let mut len = 0usize;
                        let mut shift = 0;
                        while pos < bytes.len() {
                            let b = bytes[pos];
                            pos += 1;
                            len |= ((b & 0x7f) as usize) << shift;
                            shift += 7;
                            if (b & 0x80) == 0 { break; }
                        }
                        if sec_id == 10 {
                            return Some((pos, len));
                        }
                        pos += len;
                    }
                    None
                };

                if let (Some((r_code_start, _)), Some((s_code_start, _))) = (find_code_sec(&rust_bytes), find_code_sec(&self_bytes)) {
                    let mut r_pos = r_code_start;
                    let mut s_pos = s_code_start;
                    let read_leb = |bytes: &[u8], pos: &mut usize| -> usize {
                        let mut val = 0usize;
                        let mut shift = 0;
                        while *pos < bytes.len() {
                            let b = bytes[*pos];
                            *pos += 1;
                            val |= ((b & 0x7f) as usize) << shift;
                            shift += 7;
                            if (b & 0x80) == 0 { break; }
                        }
                        val
                    };
                    let r_num_fn = read_leb(&rust_bytes, &mut r_pos);
                    let s_num_fn = read_leb(&self_bytes, &mut s_pos);
                    eprintln!("Code section num_fn: Rust={}, Self={}", r_num_fn, s_num_fn);
                    for f in 0..r_num_fn.max(s_num_fn) {
                        let r_fn_size = read_leb(&rust_bytes, &mut r_pos);
                        let s_fn_size = read_leb(&self_bytes, &mut s_pos);
                        if r_fn_size != s_fn_size || &rust_bytes[r_pos..r_pos+r_fn_size] != &self_bytes[s_pos..s_pos+s_fn_size] {
                            eprintln!("Function index {} diff: Rust body size={}, Self body size={}", f, r_fn_size, s_fn_size);
                            let max_len = r_fn_size.max(s_fn_size);
                            for b in 0..max_len {
                                let rb = rust_bytes.get(r_pos + b);
                                let sb = self_bytes.get(s_pos + b);
                                if rb != sb {
                                    eprintln!("  Fn {} byte {}: Rust={:?} (0x{:02x?}), Self={:?} (0x{:02x?})", f, b, rb, rb.map(|x| *x), sb, sb.map(|x| *x));
                                    break;
                                }
                            }
                        }
                        r_pos += r_fn_size;
                        s_pos += s_fn_size;
                    }
                }

                for i in 0..rust_bytes.len().max(self_bytes.len()) {
                    let r = rust_bytes.get(i);
                    let s = self_bytes.get(i);
                    if r != s {
                        eprintln!("First mismatch at byte index {}: Rust={:?} (0x{:02x?}), Self={:?} (0x{:02x?})", 
                            i, r, r.map(|b| *b), s, s.map(|b| *b));
                        break;
                    }
                }
            }

            assert_eq!(rust_bytes, self_bytes, "Self-hosted compiler produced different WASM bytes than Rust backend in {}!", test_name);
        })
        .unwrap();
    handle.join().unwrap();
}

#[test]
fn self_hosted_bytes_match_minimal() {
    assert_self_hosted_matches_rust("min", "(module min (fn f [] -> i32 42))");
}

#[test]
fn self_hosted_bytes_match_add() {
    assert_self_hosted_matches_rust("add", "(module add (fn add [a:i32 b:i32] -> i32 (+ a b)))");
}

#[test]
fn self_hosted_bytes_match_compute() {
    let src = r#"
(module compute
  (fn add [x:i32] -> i32 (+ x 5))
  (fn compute [x:i32] -> i32
    (let y:i32 (call add x))
    (let z:i32 0)
    (loop i 0 9 1
      (set! z (+ z i)))
    (+ y z)))
"#;
    assert_self_hosted_matches_rust("compute", src);
}

#[test]
fn self_hosted_bytes_match_struct() {
    let src = r#"
(module test_struct
  (struct Point [x:i32 y:i32])
  (struct Flags [on:bool name:str])
  (fn make_point [x:i32 y:i32] -> i32
    (let p:i32 (new Point))
    (put p Point.x x)
    (put p Point.y y)
    (+ (get p Point.x) (get p Point.y)))
  (fn flags [] -> bool
    (let f:i32 (new Flags))
    (put f Flags.on true)
    (put f Flags.name "n")
    (let a:i32 (arr.new bool 2))
    (arr.set bool a 1 (get f Flags.on))
    (and (arr.get bool a 1) (eq (str.len (get f Flags.name)) 1))))
"#;
    assert_self_hosted_matches_rust("struct", src);
}

#[test]
fn self_hosted_bytes_match_array() {
    let src = r#"
(module test_array
  (fn sum_arr [n:i32] -> i32
    (let a:i32 (arr.new i32 n))
    (let i:i32 0)
    (while (lt i n)
      (arr.set i32 a i (* i 2))
      (set! i (+ i 1)))
    (arr.get i32 a (- n 1))))
"#;
    assert_self_hosted_matches_rust("array", src);
}

#[test]
fn self_hosted_bytes_match_sys_print() {
    let src = r#"
(module test_print
  (fn main [] -> void
    (sys.print "Hello, WASI!")))
"#;
    assert_self_hosted_matches_rust("sys_print", src);
}

#[test]
fn self_hosted_bytes_match_match_result() {
    let src = r#"
(module test_match
  (fn div [a:i32 b:i32] -> (result i32 i32)
    (if (eq b 0)
        (err -1)
        (ok (/ a b))))
  (fn test [a:i32 b:i32] -> i32
    (match_result (call div a b)
      (ok v v)
      (err e e)))
  ;; void arms in statement position (no drop after the if)
  (fn count_ok [a:i32] -> i32
    (let n:i32 0)
    (match_result (call div a 1)
      (ok v (set! n v))
      (err e (set! n -1)))
    n))
"#;
    assert_self_hosted_matches_rust("match_result", src);
}

#[test]
fn self_hosted_bytes_match_strings_alloc_grow_exit() {
    let src = r#"
(module p9
  (fn main [] -> i32
    (sys.print "x" "yy" "x")
    (let s:str "hello")
    (let p:i32 (mem.alloc 16))
    (let g:i32 (mem.grow 1))
    (+ (str.len s) (+ (str.ptr s) (+ p g))))
  (fn quit [] -> void (sys.exit 3)))
"#;
    assert_self_hosted_matches_rust("strings_alloc_grow_exit", src);
}

#[test]
fn self_hosted_bytes_match_file_io() {
    let src = r#"
(module test_io
  (fn write_and_read [] -> i32
    (let fd:i32 (fs.open (str.ptr "test.txt") (str.len "test.txt") 1))
    (if (gte fd 0)
        (block
          (fs.write fd (str.ptr "data") (str.len "data"))
          (fs.close fd)
          0)
        -1)))
"#;
    assert_self_hosted_matches_rust("file_io", src);
}

#[test]
fn self_hosted_bytes_match_complex() {
    let src = r#"
(module complex
  (struct Node [val:i32 next:i32])
  (fn process [n:i32] -> (result i32 i32)
    (let head:i32 (new Node))
    (put head Node.val n)
    (put head Node.next 0)
    (if (gt (get head Node.val) 10)
        (ok (get head Node.val))
        (err -1))))
"#;
    assert_self_hosted_matches_rust("complex", src);
}

#[test]
fn self_hosted_bytes_match_void_and_scoping() {
    let src = r#"
(module void_scope
  (fn test_scoping [x:i32] -> i32
    (let res:i32 0)
    (if (gt x 0)
        (block
          (let inner:i32 10)
          (set! res (+ x inner)))
        (block
          (let inner:i32 20)
          (set! res (+ x inner))))
    (if (gt x 100) (set! res 100) (block))
    res))
"#;
    assert_self_hosted_matches_rust("void_scope", src);
}

/// Runs the self-hosted output under wasmtime with a WASI context and a
/// preopened directory (as tests/test_wasi.rs does for the Rust backend).
#[test]
fn self_hosted_file_io_runs_under_wasi() {
    use wasmtime_wasi::p1::WasiP1Ctx;
    use wasmtime_wasi::p2::pipe::MemoryOutputPipe;
    use wasmtime_wasi::{FsPerms, WasiCtxBuilder};

    let src = r#"
(module file_io_run
  (fn main [] -> i32
    (let fd:i32 (fs.open (str.ptr "hello.txt") (str.len "hello.txt") 1))
    (if (gte fd 0)
        (block
          (fs.write fd (str.ptr "Hello from WASI!") (str.len "Hello from WASI!"))
          (fs.close fd)
          (sys.print "Wrote file successfully!")
          1)
        (block
          (sys.print "Failed to open file")
          0))))
"#;
    let wasm_bytes = run_self_hosted(src);

    let dir = std::env::temp_dir().join(format!("aipl_selfhost_wasi_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let engine = Engine::default();
    let module = WasmModule::new(&engine, &wasm_bytes).unwrap();
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
    let stdout = MemoryOutputPipe::new(4096);
    let ctx = WasiCtxBuilder::new()
        .stdout(stdout.clone())
        .preopened_dir(&dir, ".", FsPerms::ReadWrite)
        .unwrap()
        .build_p1();
    let mut store = Store::new(&engine, ctx);
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let main: TypedFunc<(), i32> = instance.get_typed_func(&mut store, "main").unwrap();
    assert_eq!(main.call(&mut store, ()).unwrap(), 1);
    drop(store);

    assert_eq!(String::from_utf8(stdout.contents().to_vec()).unwrap(), "Wrote file successfully!\n");
    assert_eq!(fs::read_to_string(dir.join("hello.txt")).unwrap(), "Hello from WASI!");
    let _ = fs::remove_dir_all(&dir);
}

/// The self-hosted output enforces the memory layout like the Rust backend
/// (AIPL_SPEC.md 7.9): stores to bytes 0-3 or 64-1023 trap, others go through.
#[test]
fn self_hosted_store_guard_traps_on_the_reserved_block() {
    let src = "(module store (fn add [a:i32 b:i32] -> i32 (mem.store32 a b) (mem.load32 a)))";
    assert_self_hosted_matches_rust("store", src);
    let bytes = run_self_hosted(src);
    let engine = Engine::default();
    let module = WasmModule::new(&engine, &bytes).unwrap();
    let call = |addr: i32, val: i32| -> Result<i32, wasmtime::Error> {
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[]).unwrap();
        let f: TypedFunc<(i32, i32), i32> = instance.get_typed_func(&mut store, "add").unwrap();
        f.call(&mut store, (addr, val))
    };
    assert_eq!(call(1024, 7).unwrap(), 7);
    assert_eq!(call(65536, -1).unwrap(), -1);
    assert_eq!(call(16, 99).unwrap(), 99, "runtime cell 16 is writable");
    for bad in [0, 3, 64, 512, 1023] {
        let trap = call(bad, 1).unwrap_err().downcast::<wasmtime::Trap>().unwrap();
        assert_eq!(trap, wasmtime::Trap::UnreachableCodeReached, "address {bad}");
    }
}

/// Arrays and results compiled by the self-hosted backend compute the same
/// values as the VM, including the arr.new negative-size trap.
#[test]
fn self_hosted_arrays_and_results_execute() {
    let src = r#"
(module ar
  (fn sum_arr [n:i32] -> i32
    (let a:i32 (arr.new i32 n))
    (let i:i32 0)
    (while (lt i n)
      (arr.set i32 a i (* i 2))
      (set! i (+ i 1)))
    (let s:i32 0)
    (set! i 0)
    (while (lt i n)
      (set! s (+ s (arr.get i32 a i)))
      (set! i (+ i 1)))
    s)
  (fn div [a:i32 b:i32] -> (result i32 i32)
    (if (eq b 0) (err -1) (ok (/ a b))))
  (fn safe_div [a:i32 b:i32] -> i32
    (match_result (call div a b) (ok v v) (err e (* e 100)))))
"#;
    assert_self_hosted_matches_rust("arrays_results", src);
    let bytes = run_self_hosted(src);
    let engine = Engine::default();
    let module = WasmModule::new(&engine, &bytes).unwrap();
    let call = |name: &str, a: i32, b: i32| -> Result<i32, wasmtime::Error> {
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[]).unwrap();
        if name == "sum_arr" {
            let f: TypedFunc<i32, i32> = instance.get_typed_func(&mut store, name).unwrap();
            f.call(&mut store, a)
        } else {
            let f: TypedFunc<(i32, i32), i32> = instance.get_typed_func(&mut store, name).unwrap();
            f.call(&mut store, (a, b))
        }
    };
    assert_eq!(call("sum_arr", 5, 0).unwrap(), 20);
    assert_eq!(call("sum_arr", 0, 0).unwrap(), 0);
    assert!(call("sum_arr", -1, 0).is_err(), "negative arr.new size traps");
    assert_eq!(call("safe_div", 84, 2).unwrap(), 42);
    assert_eq!(call("safe_div", 1, 0).unwrap(), -100);

    let module_ast = aipl_core::parser::Parser::parse(src).unwrap();
    let mut vm = VM::new();
    vm.load_module(module_ast);
    assert_eq!(vm.invoke("sum_arr", vec![Value::Int(5)]).unwrap(), Value::Int(20));
    assert_eq!(vm.invoke("safe_div", vec![Value::Int(1), Value::Int(0)]).unwrap(), Value::Int(-100));
}

/// Inputs the self-hosted backend cannot compile are compile errors, never a
/// miscompile: 64-bit struct fields / array elements (95), unknown structs (96),
/// and string data beyond the 512-byte area (768).
#[test]
fn self_hosted_rejects_what_it_cannot_compile() {
    for src in [
        "(module m (struct S [a:i32 b:i64]) (fn f [] -> i32 (sizeof S)))",
        "(module m (fn f [] -> i32 (arr.new f64 3)))",
    ] {
        let err = self_host(src).unwrap_err();
        assert!(err.contains("compile error 95"), "{src}: {err}");
    }
    let err = self_host("(module m (fn f [] -> i32 (sizeof Missing)))").unwrap_err();
    assert!(err.contains("compile error 96"), "{err}");
    // 600 bytes of string data overflow the 512-byte area, as in the Rust backend.
    let long = "x".repeat(600);
    let err = self_host(&format!("(module m (fn f [] -> i32 (str.len \"{long}\")))")).unwrap_err();
    assert!(err.contains("compile error 768"), "{err}");
}

#[test]
fn self_hosted_bytes_match_memory() {
    let src = fs::read_to_string("aipl_src/memory.aipl").unwrap();
    assert_self_hosted_matches_rust("memory", &src);
}

#[test]
fn self_hosted_bytes_match_compiler() {
    let src = fs::read_to_string("aipl_src/compiler.aipl").unwrap();
    assert_self_hosted_matches_rust("compiler", &src);
}

/// codegen.aipl as one import-free module, which is what `compile_module`
/// accepts: codegen.aipl without its `(import compiler)`, plus compiler.aipl's
/// structs and functions renamed to `compiler.<fn>` (with their internal calls
/// rewritten), i.e. what the resolver would produce.
fn codegen_combined_source() -> String {
    let compiler_src = fs::read_to_string("aipl_src/compiler.aipl").unwrap();
    let codegen_src = fs::read_to_string("aipl_src/codegen.aipl").unwrap();

    let body_start = compiler_src.find("(struct Token").unwrap();
    let body_end = compiler_src.rfind(')').unwrap();
    let mut body = compiler_src[body_start..body_end].to_string();
    let names: Vec<String> = body
        .match_indices("(fn ")
        .map(|(i, _)| body[i + 4..].split_whitespace().next().unwrap().to_string())
        .collect();
    for name in &names {
        for (prefix, sep) in [("(fn ", " "), ("(call ", " "), ("(call ", ")")] {
            body = body.replace(
                &format!("{prefix}{name}{sep}"),
                &format!("{prefix}compiler.{name}{sep}"),
            );
        }
    }

    let combined_src = codegen_src.replacen("(import compiler)", &body, 1);
    assert!(!combined_src.contains("(import"), "combined module must be import-free");
    combined_src
}

/// The self-hosted compiler, run in the VM, compiles itself to the same bytes
/// as the Rust backend.
#[test]
fn self_hosted_bytes_match_codegen() {
    assert_self_hosted_matches_rust("codegen", &codegen_combined_source());
}

/// Bootstrap fixpoint: compile the self-hosted compiler with the Rust backend,
/// run that wasm module under wasmtime on its own source, and require the
/// output to be byte-identical to the module that produced it. No VM and no
/// Rust compiler logic is involved in the second compile.
#[test]
fn self_hosted_compiler_reproduces_itself_under_wasmtime() {
    use wasmtime::Val;
    use wasmtime_wasi::p1::WasiP1Ctx;

    let src = codegen_combined_source();
    let module_ast = aipl_core::parser::Parser::parse(&src).unwrap();
    TypeChecker::new().check_module(&module_ast).unwrap();
    let stage1 = WasmCompiler::compile(&module_ast).unwrap();

    let engine = Engine::default();
    let module = WasmModule::new(&engine, &stage1).unwrap();
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
    let mut store = Store::new(&engine, wasmtime_wasi::WasiCtxBuilder::new().build_p1());
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let memory = instance.get_memory(&mut store, "memory").unwrap();
    let call = |store: &mut Store<WasiP1Ctx>, name: &str, args: &[i32]| -> i32 {
        let f = instance.get_func(&mut *store, name).unwrap();
        let args: Vec<Val> = args.iter().map(|a| Val::I32(*a)).collect();
        let mut out = [Val::I32(0)];
        f.call(&mut *store, &args, &mut out).unwrap();
        out[0].unwrap_i32()
    };

    call(&mut store, "init_keywords", &[]);
    let src_ptr = call(&mut store, "alloc_src", &[src.len() as i32 + 16]);
    memory.write(&mut store, src_ptr as usize, src.as_bytes()).unwrap();
    let len = call(&mut store, "compile_module", &[src_ptr, src.len() as i32]);
    let mut word = [0u8; 4];
    memory.read(&store, 4, &mut word).unwrap();
    assert!(len > 0, "compile_module returned {len} with compile error {}", u32::from_le_bytes(word));

    memory.read(&store, 60, &mut word).unwrap();
    let mut stage2 = vec![0u8; len as usize];
    memory.read(&store, u32::from_le_bytes(word) as usize, &mut stage2).unwrap();
    assert!(stage2 == stage1, "stage 2 ({} bytes) differs from stage 1 ({} bytes)", stage2.len(), stage1.len());
}
