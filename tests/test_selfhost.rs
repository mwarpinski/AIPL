use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use std::fs;
use std::path::Path;
use wasmtime::{Engine, Instance, Linker, Module as WasmModule, Store, TypedFunc};

/// Compiles `src` as the file `name` (its name in the line table, as the
/// Rust resolver names an entry file).
fn run_self_hosted(src: &str, name: &str) -> Vec<u8> {
    self_host_named(src, name).unwrap_or_else(|e| panic!("{e}"))
}

/// Runs `codegen.compile_module` over `src`: codegen.aipl compiled to wasm
/// by the Rust toolchain, under wasmtime (in the VM until 2026-10-20, many
/// times slower; codegen.aipl compiling itself, below, still runs in the
/// VM). Returns the module bytes or the compile error code it reported
/// (AIPL_SPEC.md 6.4).
fn self_host(src: &str) -> Result<Vec<u8>, String> {
    self_host_named(src, "")
}

/// self_host, naming the source file in the line table (codegen.compile_named).
fn self_host_named(src: &str, name: &str) -> Result<Vec<u8>, String> {
    let src = flat(src);
    let name = name.to_string();
    // the code generator recurses once per nesting level of what it compiles
    std::thread::Builder::new()
        .stack_size(128 << 20)
        .spawn(move || self_host_on_this_thread(&src, &name))
        .unwrap()
        .join()
        .unwrap()
}

/// src as the code generator takes it: a program that imports (std/alloc,
/// to allocate) resolved and printed flat, as the front end hands it over.
fn flat(src: &str) -> String {
    if !src.contains("(import ") && !src.contains("(for ") {
        return src.to_string();
    }
    let path = std::env::temp_dir().join(format!("aipl_selfhost_flat_{}.aipl", std::process::id()));
    let m = Resolver::resolve_source(src, &path).expect("resolve test src");
    aipl_core::printer::print_module(&m)
}

/// codegen.aipl as wasm, compiled once per process.
fn codegen_wasm() -> &'static (wasmtime::Engine, wasmtime::Module) {
    static CODEGEN: std::sync::OnceLock<(wasmtime::Engine, wasmtime::Module)> = std::sync::OnceLock::new();
    CODEGEN.get_or_init(|| {
        let module = Resolver::resolve(&Path::new(env!("CARGO_MANIFEST_DIR")).join("aipl_src/codegen.aipl")).expect("resolve codegen.aipl");
        TypeChecker::new().check_module(&module).expect("check codegen.aipl");
        let wasm = WasmCompiler::compile(&module).expect("compile codegen.aipl");
        let mut config = wasmtime::Config::new();
        config.async_stack_size(80 << 20).max_wasm_stack(64 << 20);
        let engine = wasmtime::Engine::new(&config).unwrap();
        let m = wasmtime::Module::new(&engine, &wasm).unwrap();
        (engine, m)
    })
}

fn self_host_on_this_thread(src: &str, name: &str) -> Result<Vec<u8>, String> {
    use wasmtime::Val;
    let (engine, module) = codegen_wasm();
    let mut linker: wasmtime::Linker<wasmtime_wasi::p1::WasiP1Ctx> = wasmtime::Linker::new(engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t| t).unwrap();
    let mut store = wasmtime::Store::new(engine, wasmtime_wasi::WasiCtxBuilder::new().inherit_stderr().build_p1());
    let inst = linker.instantiate(&mut store, module).expect("instantiate codegen.aipl");
    let memory = inst.get_memory(&mut store, "memory").unwrap();
    // each export's one result as an i32 (a pointer's address: the low half)
    let call = |store: &mut wasmtime::Store<_>, f: &str, args: &[i32]| -> i32 {
        let func = inst.get_func(&mut *store, f).unwrap_or_else(|| panic!("no export {f}"));
        let args: Vec<Val> = args.iter().map(|a| Val::I32(*a)).collect();
        let mut out = [Val::I32(0)];
        func.call(&mut *store, &args, &mut out).unwrap_or_else(|e| panic!("{f} trapped: {e}"));
        match out[0] {
            Val::I32(v) => v,
            Val::I64(v) => v as i32,
            ref other => panic!("{f} gave {other:?}"),
        }
    };
    let res = call(&mut store, "init_keywords", &[]);
    assert!(res >= 0, "init_keywords failed with {}", res);
    let src_bytes = src.as_bytes();
    let src_ptr = call(&mut store, "alloc_src", &[(src_bytes.len() + 16) as i32]);
    memory.write(&mut store, src_ptr as usize, src_bytes).unwrap();
    let name_ptr = call(&mut store, "alloc_src", &[(name.len() + 16) as i32]);
    memory.write(&mut store, name_ptr as usize, name.as_bytes()).unwrap();
    let out_len = call(&mut store, "compile_named", &[src_ptr, src_bytes.len() as i32, name_ptr, name.len() as i32]);
    let data = memory.data(&store);
    let word = |addr: usize| u32::from_le_bytes(data[addr..addr + 4].try_into().unwrap()) as usize;
    if out_len <= 0 {
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
    let out_ptr = word(60);
    let bytes = data[out_ptr..out_ptr + out_len as usize].to_vec();
    wasmparser::Validator::new().validate_all(&bytes).map_err(|e| format!("self-hosted output does not validate: {e}"))?;
    Ok(bytes)
}

/// codegen.compile_named run in the VM, for the one test that keeps the VM
/// tested on the whole compiler (self_hosted_bytes_match_codegen).
fn vm_self_host(src: &str) -> Result<Vec<u8>, String> {
    let src = flat(src);
    // the tree-walking VM recurses once per nested AIPL call
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(move || vm_self_host_on_this_thread(&src, ""))
        .unwrap()
        .join()
        .unwrap()
}

fn vm_self_host_on_this_thread(src: &str, name: &str) -> Result<Vec<u8>, String> {
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
    
    let name_ptr = match vm.invoke("alloc_src", vec![Value::Int((name.len() + 16) as i64)]).expect("alloc_src") {
        Value::Int(p) => p as usize,
        other => panic!("expected Int from alloc_src, got {:?}", other),
    };
    vm.write_bytes(name_ptr, name.as_bytes());
    let args = vec![Value::Int(src_ptr as i64), Value::Int(src_bytes.len() as i64), Value::Int(name_ptr as i64), Value::Int(name.len() as i64)];
    let out_len_val = vm.invoke("compile_named", args).expect("compile_named");
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
            // a program that imports (std/alloc, to allocate) goes to the
            // code generators flat, as the front end hands it over
            // a program with imports or a `for` is resolved first: the code generator reads
            // what the resolver prints (counted_loop), not the sugar
            let src = if src.contains("(import ") || src.contains("(for ") {
                let flat = aipl_core::printer::print_module(&Resolver::resolve(&temp_src).expect("resolve test src"));
                fs::write(&temp_src, &flat).unwrap();
                flat
            } else {
                src
            };

            let rust_bytes = run_rust_backend(&temp_src);
            let self_bytes = run_self_hosted(&src, &format!("{}.aipl", test_name));

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
                        if r_fn_size != s_fn_size || rust_bytes[r_pos..r_pos+r_fn_size] != self_bytes[s_pos..s_pos+s_fn_size] {
                            eprintln!("Function index {} diff: Rust body size={}, Self body size={}", f, r_fn_size, s_fn_size);
                            let max_len = r_fn_size.max(s_fn_size);
                            for b in 0..max_len {
                                let rb = rust_bytes.get(r_pos + b);
                                let sb = self_bytes.get(s_pos + b);
                                if rb != sb {
                                    eprintln!("  Fn {} byte {}: Rust={:?} (0x{:02x?}), Self={:?} (0x{:02x?})", f, b, rb, rb.copied(), sb, sb.copied());
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
                            i, r, r.copied(), s, s.copied());
                        break;
                    }
                }
            }

            assert_eq!(rust_bytes, self_bytes, "Self-hosted compiler produced different WASM bytes than Rust backend in {}!", test_name);
        })
        .unwrap();
    handle.join().unwrap();
}

/// `when` read by the self-hosted code generator gives the bytes the Rust
/// backend gives for the tree it reads into, `(if c (block body...) (block))`
/// (CANONICAL_PLAN.md step 4): nested, in a loop with break and continue, with
/// a return, with no body, beside a valued if, and in a cond's clause.
#[test]
fn self_hosted_bytes_match_when() {
    assert_self_hosted_matches_rust(
        "when_forms",
        "(module w
  (fn f [n:i32] -> i32
    (let x:i32 0)
    (when (gt n 0) (set! x 1) (when (gt n 10) (set! x (+ x 10))))
    (when (lt n 0))
    (let i:i32 0)
    (while (lt i n)
      (set! i (+ i 1))
      (when (eq i 3) (continue))
      (when (gt i 7) (break))
      (set! x (+ x i)))
    (when (eq n 99) (return -1))
    (let y:i32 (if (gt x 5) x 0))
    (cond ((eq y 1) (when (gt n 1) (set! x 2))) (else (block)))
    (+ x y)))",
    );
}

#[test]
fn self_hosted_bytes_match_minimal() {
    assert_self_hosted_matches_rust("min", "(module min (fn f [] -> i32 42))");
}

/// String literals are interned by content, as wasm.rs does: a `"\n"`
/// literal shares the newline `sys.print` reserves (codegen.aipl once gave it
/// a second copy, found compiling aipl_src/native/native.aipl), and two
/// spellings of the same text (an escape and the raw character) share one.
#[test]
fn self_hosted_bytes_match_strings_interned_by_content() {
    assert_self_hosted_matches_rust("nl_after", "(module m (fn f [] -> i32 (sys.print \"a\") (str.len \"\\n\")))");
    assert_self_hosted_matches_rust("nl_before", "(module m (fn f [] -> i32 (str.len \"\\n\") (sys.print \"a\") 0))");
    assert_self_hosted_matches_rust("tab", "(module m (fn f [] -> i32 (+ (str.len \"\\t\") (str.len \"\t\"))))");
    assert_self_hosted_matches_rust("repeat", "(module m (fn f [] -> i32 (+ (str.len \"ab\") (+ (str.len \"a\") (str.len \"ab\")))))");
}

/// f64.sqrt (added for the benchmarks), at byte parity.
#[test]
fn self_hosted_bytes_match_f64_sqrt() {
    assert_self_hosted_matches_rust(
        "sqrt",
        "(module m (fn hyp [a:f64 b:f64] -> f64 (f64.sqrt (+ (* a a) (* b b)))) (fn f [] -> f64 (call hyp 3.0 4.0)))",
    );
}

/// Structs may have up to 64 fields in the self-hosted compiler (it was
/// 15, which the native backend's translator state passed in NE8).
#[test]
fn self_hosted_bytes_match_a_wide_struct() {
    let fields: Vec<String> = (0..40).map(|i| format!("f{i}:{}", if i % 3 == 0 { "i64" } else { "i32" })).collect();
    let src = format!(
        "(module wide (import alloc) (struct W [{}]) (fn f [] -> i32 (let w:(ptr W) (new W (call alloc.forever))) (put w W.f37 7) (put w W.f38 5) (+ (get w W.f37) (get w W.f38))))",
        fields.join(" ")
    );
    assert_self_hosted_matches_rust("wide", &src);
}

/// Body items that are atoms, not groups. codegen.is_contract_head once read
/// a bare number's value as a node address: harmless garbage for small
/// numbers, a crash for any number past the end of memory (found compiling
/// aipl_src/native/elf.aipl, whose `base_vaddr` returns 4194304).
#[test]
fn self_hosted_bytes_match_atom_bodies() {
    assert_self_hosted_matches_rust(
        "atoms",
        "(module atoms
           (fn big [] -> i32 4194304)
           (fn past_memory [] -> i32 70000000)
           (fn negative [] -> i32 -4096)
           (fn wide [] -> i64 4294967297i64)
           (fn name [x:i32] -> i32 x)
           (fn text [] -> str \"hi\")
           (fn after_contract [x:i32] -> i32 (req (gt x 0)) 2000000000))",
    );
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
    (counted_loop i 0 9 1
      (set! z (+ z i)))
    (+ y z)))
"#;
    assert_self_hosted_matches_rust("compute", src);
}

#[test]
fn self_hosted_bytes_match_struct() {
    let src = r#"
(module test_struct (import alloc)
  (struct Point [x:i32 y:i32])
  (struct Flags [on:bool name:str])
  (fn make_point [x:i32 y:i32] -> i32
    (let p:(ptr Point) (new Point (call alloc.forever)))
    (put p Point.x x)
    (put p Point.y y)
    (+ (get p Point.x) (get p Point.y)))
  (fn flags [] -> bool
    (let f:(ptr Flags) (new Flags (call alloc.forever)))
    (put f Flags.on true)
    (put f Flags.name "n")
    (let a:(arr bool) (arr.new bool (call alloc.forever) 2))
    (arr.set bool a 1 (get f Flags.on))
    (and (arr.get bool a 1) (eq (str.len (get f Flags.name)) 1))))
"#;
    assert_self_hosted_matches_rust("struct", src);
}

#[test]
fn self_hosted_bytes_match_array() {
    let src = r#"
(module test_array (import alloc)
  (fn sum_arr [n:i32] -> i32
    (let a:(arr i32) (arr.new i32 (call alloc.forever) n))
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
        (err:i32 -1)
        (ok:i32 (/ a b))))
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

/// Payloads of every width, and every way a match_result binder's type is
/// found (codegen.aipl result_side): a let, a parameter, a call, call_ref,
/// ok/err themselves (with and without `:T`), if, block, unsafe, cond,
/// match, and a nested match_result.
#[test]
fn self_hosted_bytes_match_wide_results() {
    let src = r#"
(module wide_results
  (union U [(one) (two)])
  (fn big [n:i32] -> (result i64 f64)
    (if (gt n 0) (ok:f64 (i64.extend_s n)) (err:i64 1.5)))
  (fn nested [] -> (result (result f64 bool) i64) (ok:i64 (ok:bool 2.5)))
  (fn from_let [] -> i64
    (let r:(result i64 f64) (call big 3))
    (match_result r (ok a a) (err b (i64.trunc_f64_s b))))
  (fn from_param [r:(result f64 i32)] -> f64
    (match_result r (ok c c) (err d 0.0)))
  (fn from_ref [] -> i64
    (let g:(fn [i32] -> (result i64 f64)) (ref big))
    (match_result (call_ref (fn [i32] -> (result i64 f64)) g 1) (ok a a) (err b 0i64)))
  (fn from_literals [] -> i64
    (let s:i64 0i64)
    (match_result (ok:i32 7i64) (ok e1 (set! s e1)) (err e2 (set! s (i64.extend_s e2))))
    (match_result (err:f64 7i64) (ok e3 (set! s (i64.trunc_f64_s e3))) (err e4 (set! s e4)))
    (match_result (ok:f64 1i64) (ok e5 (set! s e5)) (err e6 (set! s (i64.trunc_f64_s e6))))
    s)
  (fn from_forms [k:i32 u:U] -> i64
    (let t:i64 0i64)
    (match_result (if (gt k 0) (call big k) (call big 0)) (ok f1 (set! t f1)) (err f2 (set! t 1i64)))
    (match_result (block (call big k)) (ok f3 (set! t f3)) (err f4 (set! t 2i64)))
    (match_result (unsafe (call big k)) (ok f5 (set! t f5)) (err f6 (set! t 3i64)))
    (match_result (cond ((gt k 1) (call big k)) (else (call big 1))) (ok f7 (set! t f7)) (err f8 (set! t 4i64)))
    (match_result (match u (U.one (call big 1)) (U.two (call big 2))) (ok f9 (set! t f9)) (err f10 (set! t 5i64)))
    t)
  (fn from_nested [] -> f64
    (match_result (call nested)
      (ok r (match_result r (ok x x) (err y (if y 1.0 0.0))))
      (err z 0.0))))
"#;
    assert_self_hosted_matches_rust("wide_results", src);
}

/// Visibility marks in a module compiled on its own: both compilers leave
/// them out (compiler.aipl strip_marks, parser.rs strip_marks).
#[test]
fn self_hosted_bytes_match_with_visibility_marks() {
    assert_self_hosted_matches_rust(
        "marks",
        "(module m (import alloc) (struct P [(read x:i32) (pub y:i32) z:i32] (pub)) (fn f [] -> i32 (pub) (let p:(ptr P) (new P (call alloc.forever))) (put p P.y 4) (+ (get p P.y) (get p P.x))))",
    );
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

/// P9: i64 code (literals at both extremes, arithmetic, unsigned ops, shifts,
/// comparisons, if blocks typed i64, conversions, an i64 struct field after a
/// bool, i64 arrays, mem.load64/store64).
#[test]
fn self_hosted_bytes_match_i64() {
    let src = r#"
(module i64demo (import alloc)
  (struct Acc [flag:bool total:i64 n:i32])
  (fn add [a:i64 b:i64] -> i64 (unsafe) (+ a b))
  (fn mix [x:i32] -> i64 (unsafe)
    (let big:i64 9223372036854775807i64)
    (let neg:i64 -9223372036854775808i64)
    (let a:i64 (i64.extend_s x))
    (if (gt a 100i64)
        (bitand (shl a 3i64) big)
        (- (divu a 7i64) (remu neg 3i64))))
  (fn cmp [a:i64 b:i64] -> i32 (unsafe)
    (if (and (lte a b) (neq a 0i64)) (i32.wrap (shru b 1i64)) -1))
  (fn acc [n:i32] -> i64 (unsafe)
    (let s:(ptr Acc) (new Acc (call alloc.forever)))
    (put s Acc.total 0i64)
    (counted_loop i 1 n 1
      (put s Acc.total (+ (get s Acc.total) (i64.extend_u i))))
    (let arr:(arr i64) (arr.new i64 (call alloc.forever) 4))
    (arr.set i64 arr 3 (get s Acc.total))
    (+ (arr.get i64 arr 3) (mem.load64 (+ (ptr.addr s) 8)))))
"#;
    assert_self_hosted_matches_rust("i64", src);
    assert_self_hosted_matches_rust(
        "i64_mem",
        "(module s64 (fn f [p:i32 v:i64] -> i64 (unsafe) (mem.store64 p v) (mem.load64 p)))",
    );
}

/// f64 / f32 parameters, arithmetic, comparisons, if blocks typed f64, and
/// struct fields with 8-byte alignment.
#[test]
fn self_hosted_bytes_match_floats() {
    let src = r#"
(module f64demo
  (struct P [x:f64 y:f32 z:f64])
  (fn lerp [a:f64 b:f64 t:f64] -> f64 (+ a (* (- b a) t)))
  (fn pick [a:f64 b:f64] -> f64 (if (gt a b) a (/ b a)))
  (fn eq32 [a:f32 b:f32] -> bool (eq a b))
  (fn sz [] -> i32 (sizeof P))
  (fn sz2 [] -> i32 (+ (sizeof f64) (+ (sizeof (ptr P)) (sizeof f32))))
  (fn st [p:(ptr P) v:f64] -> f64 (put p P.z v) (get p P.z)))
"#;
    assert_self_hosted_matches_rust("floats", src);
}

/// (ok:T v) / (err:T e) with scalar and compound T, and a \" escape inside a
/// string literal (the tokenizer must not end the string there).
#[test]
fn self_hosted_bytes_match_typed_results_and_escapes() {
    let src = r#"
(module okt
  (fn f [] -> (result i32 bool) (ok:bool 1))
  (fn g [] -> (result (result i32 i32) i32) (err:(result i32 i32) 3))
  (fn h [] -> i32 (str.len "a\"b")))
"#;
    assert_self_hosted_matches_rust("typed_results", src);
}

/// Float literals: f64.const bytes must equal what Rust's parse::<f64> gives.
/// 200 pseudo-random literals (deterministic LCG) with up to 16 significant
/// digits, random sign and decimal point position (std/float's fast path);
/// then 300 with up to 40 digits and exponents to +-330 (its exact path:
/// halfway cases, subnormals, infinities), and fixed hard cases, which were
/// compile error 973 before std/float.
#[test]
fn self_hosted_float_literals_match_rust() {
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
    while made < 500 {
        let ndigits = 1 + next(40) as usize;
        let digits: String = (0..ndigits).map(|_| char::from(b'0' + next(10) as u8)).collect();
        let dot = next(ndigits as u64 + 1) as usize;
        let exp = next(661) as i64 - 330;
        let sign = if next(2) == 0 { "" } else { "-" };
        let e = ["e", "E"][next(2) as usize];
        let lit = format!("{sign}{}.{}{e}{exp}", &digits[..dot], &digits[dot..]);
        src.push_str(&format!("  (fn f{made} [] -> f64 {lit})\n"));
        made += 1;
    }
    for (k, lit) in [
        "9007199254740993.0", "9007199254740995.0", "0.00000000000000000000001", "1234567890123456789.0",
        "1.7976931348623157e308", "1.7976931348623158e308", "1.8e308", "-1.0e400", "1.0e-400",
        "4.9406564584124654e-324", "2.4703282292062327e-324", "2.4703282292062328e-324",
        "2.2250738585072011e-308", "2.2250738585072012e-308", "0.1e1", "5.", ".5", "+1.5E+3", "1.e-2",
        "0.000e99999999999", "123.456e-0", "7.0e22", "7.0e23", "8.988465674311579e307",
    ]
    .iter()
    .enumerate()
    {
        src.push_str(&format!("  (fn edge{k} [] -> f64 {lit})\n"));
    }
    src.push(')');
    assert_self_hosted_matches_rust("float_literals", &src);
}

/// The line table (docs/design/LINES_PLAN.md): positions after multi-byte
/// characters on the same line count characters, not bytes; constants and
/// enums (reprinted by consts.aipl) keep their positions; a cond clause's
/// code is at the clause. Both compilers must emit the same section.
#[test]
fn self_hosted_line_table_matches_rust() {
    let src = "(module m (const NN:i32 0) (enum E [a b])
  (fn f [x:i32] -> i32 (str.len \"héllo wörld\") (/ (str.len \"ü\") x))
  (fn g [k:E] -> i32 (cond ((eq k E.a) (/ 1 NN)) ((eq k E.b) (call f NN)) (else (% 5 NN))))
  (fn main [] -> i32 (call g (enum.cast E 1))))";
    assert_self_hosted_matches_rust("line_table", src);
}

/// Typed pointers and arrays (the programs from tests/test_pointers.rs): a
/// linked list through (ptr Node), arrays of pointers and of arrays, arr.len,
/// casts, and typed nulls.
#[test]
fn self_hosted_bytes_match_pointers() {
    assert_self_hosted_matches_rust("pointers_0", r#"
(module list (import alloc)
  (struct Node [val:i32 next:(ptr Node)])
  (fn push [head:(ptr Node) v:i32] -> (ptr Node)
    (let n:(ptr Node) (new Node (call alloc.forever)))
    (put n Node.val v)
    (put n Node.next head)
    n)
  (fn sum [] -> i32
    (let h:(ptr Node) (ptr.null Node))
    (counted_loop i 1 10 1 (set! h (call push h i)))
    (let total:i32 0)
    (while (neq h (ptr.null Node))
      (set! total (+ total (get h Node.val)))
      (set! h (get h Node.next)))
    total))
"#);
    assert_self_hosted_matches_rust("pointers_1", r#"
(module arrs (import alloc)
  (struct P [x:i32])
  (fn f [] -> i32 (unsafe)
    (let ps:(arr (ptr P)) (arr.new (ptr P) (call alloc.forever) 4))
    (counted_loop i 0 3 1
      (let p:(ptr P) (new P (call alloc.forever)))
      (put p P.x (* i i))
      (arr.set (ptr P) ps i p))
    (let total:i32 0)
    (counted_loop i 0 (- (arr.len ps) 1) 1
      (set! total (+ total (get (arr.get (ptr P) ps i) P.x))))
    ;; round trip through an address: same pointer, same field
    (let back:(ptr P) (ptr.cast P (ptr.addr (arr.get (ptr P) ps 3))))
    (let nested:(arr (arr i32)) (arr.new (arr i32) (call alloc.forever) 1))
    (arr.set (arr i32) nested 0 (arr.new i32 (call alloc.forever) 7))
    (+ (* 100 (arr.len (arr.get (arr i32) nested 0))) (+ total (get back P.x)))))
"#);
    assert_self_hosted_matches_rust("pointers_2", r#"
(module nulls
  (struct S [v:i32])
  (fn f [] -> bool
    (let a:(ptr S) (ptr.null S))
    (let b:(arr i32) (arr.null i32))
    (and (eq a (ptr.null S)) (and (eq (ptr.addr a) 0) (eq (arr.addr b) 0)))))
"#);
}

/// The standard library and the example built on it, through imports: each
/// file is resolved and printed (as `aipl compile --self` does), then both
/// compilers must emit the same bytes.
#[test]
fn self_hosted_bytes_match_std_library() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (name, rel) in [
        ("std_str", "aipl_src/std/str.aipl"),
        ("std_fmt", "aipl_src/std/fmt.aipl"),
        ("std_io", "aipl_src/std/io.aipl"),
        ("std_vec", "aipl_src/std/vec.aipl"),
        ("std_map", "aipl_src/std/map.aipl"),
        ("std_cmp", "aipl_src/std/cmp.aipl"),
        ("std_option", "aipl_src/std/option.aipl"),
        ("std_buf", "aipl_src/std/buf.aipl"),
        ("std_os", "aipl_src/std/os.aipl"),
        ("std_time", "aipl_src/std/time.aipl"),
        ("std_arena", "aipl_src/std/arena.aipl"),
        ("std_heap", "aipl_src/std/heap.aipl"),
        ("std_alloc", "aipl_src/std/alloc.aipl"),
        ("std_bigint", "aipl_src/std/bigint.aipl"),
        ("wasm_reader", "aipl_src/native/wasm_reader.aipl"),
        ("word_count", "examples/word_count.aipl"),
        ("word_freq", "examples/word_freq.aipl"),
        ("quicksort", "examples/quicksort.aipl"),
        ("matrix_mult", "examples/matrix_mult.aipl"),
        ("accounts", "examples/accounts.aipl"),
        ("math_core", "examples/math_core.aipl"),
    ] {
        let module = Resolver::resolve(&root.join(rel)).unwrap_or_else(|e| panic!("{e}"));
        assert_self_hosted_matches_rust(name, &aipl_core::printer::print_module(&module));
    }
}

/// Short-circuit and/or (audit N1), including a break inside the second
/// operand, at byte parity.
#[test]
fn self_hosted_bytes_match_short_circuit() {
    let src = include_str!("test_control_flow.rs");
    let start = src.find("(module sc").unwrap();
    let end = src[start..].find("\"#;").unwrap();
    assert_self_hosted_matches_rust("sc", &src[start..start + end]);
}

/// The completed WASI surface: stdin through std/io, both clocks, randomness,
/// and absolute paths in fs.open / fs.delete, at byte parity.
#[test]
fn self_hosted_bytes_match_clock_random_stdin_paths() {
    let dir = std::env::temp_dir().join(format!("aipl_sh_surface_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("surface.aipl");
    std::fs::write(&path, r#"(module surface
  (import io)
  (import os)
  (fn surface [] -> i64 (unsafe)
    (let b:str.Bytes (call io.read_stdin))
    (let p:i32 (mem.alloc 8))
    (let ok:i32 (sys.random p 8))
    (let fd:i32 (fs.open (str.ptr "/tmp/x") (str.len "/tmp/x") 0))
    (let gone:i32 (fs.delete (str.ptr "rel.txt") (str.len "rel.txt")))
    (+ (- (sys.time) (sys.monotonic)) (i64.extend_s (+ (call os.random_i32) (+ ok (+ fd gone)))))))"#).unwrap();
    let module = Resolver::resolve(&path).unwrap();
    assert_self_hosted_matches_rust("surface", &aipl_core::printer::print_module(&module));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Threads and atomics (AIPL_SPEC.md 4.G): the threaded module layout
/// (imported shared memory, global, start function, passive data,
/// wasi_thread_start) and the atomic lowerings, at byte parity.
#[test]
fn self_hosted_bytes_match_threads_and_atomics() {
    let src = include_str!("test_threads.rs");
    for name in ["(module threads", "(module atomics"] {
        let start = src.find(name).unwrap();
        let end = src[start..].find("\"#;").unwrap();
        assert_self_hosted_matches_rust(name, &src[start..start + end]);
    }
    let sync = std::fs::read_to_string("aipl_src/thread_sync.aipl").unwrap();
    assert_self_hosted_matches_rust("thread_sync", &sync);
}

/// Past the old capacities (256 functions, 31 structs): the toolchain itself
/// is about 250 functions, so the old limit was close to breaking self-compile.
#[test]
fn self_hosted_bytes_match_past_the_old_table_limits() {
    let mut src = String::from("(module big\n");
    for i in 0..40 {
        src.push_str(&format!("  (struct S{i} [a:i32 b:i64])\n"));
    }
    for i in 0..300 {
        let prev = if i == 0 { "0".to_string() } else { format!("(call f{} x)", i - 1) };
        src.push_str(&format!("  (fn f{i} [x:i32] -> i32 (+ x {prev}))\n"));
    }
    src.push_str("  (fn sz [] -> i32 (sizeof S39)))");
    assert_self_hosted_matches_rust("big", &src);
}

/// args.* and env.* (P14): all ten WASI imports in their fixed order, mixed
/// with file and print ops, and the args/env types and lowerings.
#[test]
fn self_hosted_bytes_match_args_and_env() {
    assert_self_hosted_matches_rust("argsenv", r#"
(module argsenv
  (fn only_env [p:i32] -> i32 (unsafe) (env.sizes p (+ p 4)))
  (fn all [p:i32] -> i32 (unsafe)
    (sys.print "x")
    (let a:i32 (args.sizes p (+ p 4)))
    (let b:i32 (args.get (+ p 8) (+ p 64)))
    (let c:i32 (env.get (+ p 8) (+ p 64)))
    (let fd:i32 (fs.open p 1 0))
    (fs.close fd)
    (fs.delete p 1)
    (if (lt a 0) (sys.exit 1) (block))
    (+ a (+ b (+ c (+ (fs.read fd p 1) (fs.write fd p 1)))))))"#);
    assert_self_hosted_matches_rust("envonly", "(module envonly (fn f [p:i32] -> i32 (unsafe) (env.get p (+ p 4))))");
}

/// Function references (P10): ref, call_ref through params, arrays, and struct
/// fields, an i64 signature, and ref equality; the funcref table, element
/// section, and call_indirect types must match the Rust backend.
#[test]
fn self_hosted_bytes_match_function_refs() {
    assert_self_hosted_matches_rust("refs", r#"
(module refs (import alloc)
  (struct Op [apply:(fn [i32 i32] -> i32) name:str])
  (fn add [a:i32 b:i32] -> i32 (+ a b))
  (fn mul [a:i32 b:i32] -> i32 (* a b))
  (fn neg [a:i64] -> i64 (- 0i64 a))
  (fn twice [f:(fn [i32 i32] -> i32) x:i32] -> i32
    (call_ref (fn [i32 i32] -> i32) f x x))
  (fn main [] -> i32
    (let ops:(arr (fn [i32 i32] -> i32)) (arr.new (fn [i32 i32] -> i32) (call alloc.forever) 2))
    (arr.set (fn [i32 i32] -> i32) ops 0 (ref add))
    (arr.set (fn [i32 i32] -> i32) ops 1 (ref mul))
    (let o:(ptr Op) (new Op (call alloc.forever)))
    (put o Op.apply (ref mul))
    (let n:i64 (call_ref (fn [i64] -> i64) (ref neg) 5i64))
    (+ (call twice (arr.get (fn [i32 i32] -> i32) ops 0) 20)
       (+ (call_ref (fn [i32 i32] -> i32) (get o Op.apply) 3 4)
          (+ (i32.wrap n) (if (and (eq (ref add) (ref add)) (neq (ref add) (ref mul))) 1 0))))))
"#);
}

/// P11 control flow, given as raw source so the self-hosted compiler sees
/// `cond` itself (the Rust parser desugars it to nested ifs): early return in
/// a loop and in a match_result arm, break/continue in while and loop (the
/// loop's continue block), nested loops, cond as value and statement.
#[test]
fn self_hosted_bytes_match_control_flow() {
    assert_self_hosted_matches_rust("control_flow", r#"
(module flow
  ;; early return inside a loop: index of the first multiple of 7 at or above n
  (fn first_mult7 [n:i32] -> i32 (unsafe)
    (counted_loop i n (+ n 100) 1
      (if (eq (% i 7) 0) (return i) (block)))
    -1)

  ;; break inside a nested if: sum 1.. until the total passes n
  (fn sum_until [n:i32] -> i32 (unsafe)
    (let total:i32 0)
    (let i:i32 0)
    (while true
      (set! i (+ i 1))
      (if (gt i 1000)
          (break)
          (if (gt total n) (break) (set! total (+ total i)))))
    total)

  ;; continue in a counted loop still applies the step: sum of odd i in 0..n
  (fn sum_odd [n:i32] -> i32 (unsafe)
    (let total:i32 0)
    (counted_loop i 0 n 1
      (if (eq (% i 2) 0) (continue) (block))
      (set! total (+ total i)))
    total)

  ;; continue in a while loop goes back to the condition
  (fn count_nonzero_digits [n:i32] -> i32 (unsafe)
    (let v:i32 n)
    (let count:i32 0)
    (while (gt v 0)
      (let d:i32 (% v 10))
      (set! v (/ v 10))
      (if (eq d 0) (continue) (block))
      (set! count (+ count 1)))
    count)

  ;; break and continue in nested loops target the innermost loop
  (fn nested [n:i32] -> i32 (unsafe)
    (let hits:i32 0)
    (counted_loop i 1 n 1
      (counted_loop j 1 n 1
        (if (gt j i) (break) (block))
        (if (eq j 2) (continue) (block))
        (set! hits (+ hits 1))))
    hits)

  ;; return from inside match_result inside a loop
  (fn parse_digit [c:i32] -> (result i32 i32) (unsafe)
    (if (and (gte c 48) (lte c 57)) (ok:i32 (- c 48)) (err:i32 c)))
  (fn first_non_digit [n:i32] -> i32 (unsafe)
    (counted_loop i 0 n 1
      (match_result (call parse_digit (+ 46 i))
        (ok d (block))
        (err e (return e))))
    0)

  ;; cond with several clauses and multi-expression bodies, as a value
  (fn classify [n:i32] -> i32 (unsafe)
    (cond
      ((lt n 0) -1)
      ((eq n 0) 0)
      ((lt n 10) (let t:i32 (* n 2)) (+ t 1))
      (else 100)))

  ;; cond as a statement
  (fn bucket_sum [n:i32] -> i32 (unsafe)
    (let small:i32 0)
    (let big:i32 0)
    (counted_loop i 0 n 1
      (cond
        ((lt i 5) (set! small (+ small 1)))
        (else (set! big (+ big 1)))))
    (+ (* small 1000) big))

  ;; a body may end in (return v); void functions use (return)
  (fn ends_in_return [n:i32] -> i32 (unsafe)
    (let x:i32 (* n 3))
    (return (+ x 1)))
  (fn bump [p:i32] -> void (unsafe)
    (if (lt p 0) (return) (block))
    (mem.store32 p (+ (mem.load32 p) 1)))
  (fn uses_void_return [n:i32] -> i32 (unsafe)
    (let p:i32 (mem.alloc 4))
    (mem.store32 p n)
    (call bump p)
    (call bump -1)
    (mem.load32 p))

  ;; loop bound and step are evaluated every iteration, and the body may set! the variable
  (fn moving_bounds [n:i32] -> i32 (unsafe)
    (let limit:i32 n)
    (let count:i32 0)
    (counted_loop i 0 limit 1
      (set! count (+ count 1))
      (if (eq i 2) (set! limit (- limit 1)) (block))
      (if (eq i 0) (set! i 1) (block)))
    count))
"#);
}

#[test]
fn self_hosted_bytes_match_file_io() {
    let src = r#"
(module test_io
  (fn write_and_read [] -> i32 (unsafe)
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
(module complex (import alloc)
  (struct Node [val:i32 next:(ptr Node)])
  (fn process [n:i32] -> (result i32 i32)
    (let head:(ptr Node) (new Node (call alloc.forever)))
    (put head Node.val n)
    (put head Node.next (ptr.null Node))
    (if (gt (get head Node.val) 10)
        (ok:i32 (get head Node.val))
        (err:i32 -1))))
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
  (fn main [] -> i32 (unsafe)
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
    let wasm_bytes = run_self_hosted(src, "");

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
    let src = "(module store (fn add [a:i32 b:i32] -> i32 (unsafe) (mem.store32 a b) (mem.load32 a)))";
    assert_self_hosted_matches_rust("store", src);
    let bytes = run_self_hosted(src, "");
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
(module ar (import alloc)
  (fn sum_arr [n:i32] -> i32
    (let a:(arr i32) (arr.new i32 (call alloc.forever) n))
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
    (if (eq b 0) (err:i32 -1) (ok:i32 (/ a b))))
  (fn safe_div [a:i32 b:i32] -> i32
    (match_result (call div a b) (ok v v) (err e (* e 100)))))
"#;
    assert_self_hosted_matches_rust("arrays_results", src);
    let bytes = run_self_hosted(src, "");
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

    let module_ast = aipl_core::parser::Parser::parse(&flat(src)).unwrap();
    let mut vm = VM::new();
    vm.load_module(module_ast);
    assert_eq!(vm.invoke("sum_arr", vec![Value::Int(5)]).unwrap(), Value::Int(20));
    assert_eq!(vm.invoke("safe_div", vec![Value::Int(1), Value::Int(0)]).unwrap(), Value::Int(-100));
}

/// Inputs the self-hosted backend cannot compile are compile errors, never a
/// miscompile: non-scalar struct fields / array elements (95), unknown structs
/// (96), and string literals past the first 1 MiB of memory (768), the
/// limit the Rust backend has too.
#[test]
fn self_hosted_rejects_what_it_cannot_compile() {
    let src = "(module m (struct S [a:i32 b:(result i32 i32)]) (fn f [] -> i32 (sizeof S)))";
    let err = self_host(src).unwrap_err();
    assert!(err.contains("compile error 95"), "{src}: {err}");
    let err = self_host("(module m (fn f [] -> i32 (sizeof Missing)))").unwrap_err();
    assert!(err.contains("compile error 96"), "{err}");
    let long = "x".repeat(1_100_000);
    let src = format!("(module m (fn f [] -> i32 (str.len \"{long}\")))");
    let err = self_host(&src).unwrap_err();
    assert!(err.contains("compile error 768"), "{err}");
    let m = aipl_core::parser::Parser::parse(&flat(&src)).unwrap();
    assert!(WasmCompiler::compile(&m).unwrap_err().contains("string literals need"));
}

/// A literal past the old 64 KiB buffer compiles byte for byte as in Rust.
/// More literals than the table starts with (it doubles) are covered by
/// tests/test_bootstrap.rs: the aipl command has about 2,000.
#[test]
fn self_hosted_long_literal_matches_rust() {
    let long = "y".repeat(70_000);
    assert_self_hosted_matches_rust("long_literal", &format!("(module long_literal (fn f [] -> i32 (str.len \"{long}\")))"));
}

/// String literals sit at 1024 with the heap after them, so a program is not
/// limited to a fixed literal area; stores into a literal trap in both
/// backends, like stores into the runtime block.
#[test]
fn self_hosted_string_literals_past_the_old_area_are_read_only() {
    let a = "a".repeat(400);
    let b = "b".repeat(400);
    let src = format!(
        "(module lits\n  (fn lens [] -> i32 (unsafe) (+ (str.len \"{a}\") (str.len \"{b}\")))\n  (fn first_free [] -> i32 (unsafe) (mem.alloc 0))\n  (fn poke [] -> i32 (unsafe) (mem.store8 (str.ptr \"{b}\") 0) 1))"
    );
    assert_self_hosted_matches_rust("lits", &src);
    let bytes = run_self_hosted(&src, "");
    let engine = Engine::default();
    let module = WasmModule::new(&engine, &bytes).unwrap();
    let call = |name: &str| -> Result<i32, wasmtime::Error> {
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[]).unwrap();
        let f: TypedFunc<(), i32> = instance.get_typed_func(&mut store, name).unwrap();
        f.call(&mut store, ())
    };
    assert_eq!(call("lens").unwrap(), 800);
    // two [len][bytes] literals: 1024 + 808, 8-aligned
    assert_eq!(call("first_free").unwrap(), 1832);
    let trap = call("poke").unwrap_err().downcast::<wasmtime::Trap>().unwrap();
    assert_eq!(trap, wasmtime::Trap::UnreachableCodeReached);
    // the VM places the literals identically and rejects the same store
    let m = aipl_core::parser::Parser::parse(&flat(&src)).unwrap();
    let mut vm = VM::new();
    vm.load_module(m);
    assert_eq!(vm.invoke("first_free", vec![]).unwrap(), Value::Int(1832));
    let err = vm.invoke("poke", vec![]).unwrap_err();
    assert!(err.contains("string literals, which are read-only"), "{err}");
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
/// accepts: the resolver's output (compiler.aipl merged in, names qualified)
/// printed back as source, exactly what `aipl compile --self` does.
fn codegen_combined_source() -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let module = Resolver::resolve(&root.join("aipl_src/codegen.aipl")).expect("resolve codegen.aipl");
    let src = aipl_core::printer::print_module(&module);
    // (a line, not a string literal: printer.aipl prints imports)
    assert!(!src.lines().any(|l| l.trim_start().starts_with("(import")), "combined module must be import-free");
    src
}

/// The self-hosted compiler compiles itself to the same bytes as the Rust
/// backend, under wasmtime and, run in the VM, there too: the one test that
/// keeps the VM running the whole compiler.
#[test]
fn self_hosted_bytes_match_codegen() {
    let src = codegen_combined_source();
    assert_self_hosted_matches_rust("codegen", &src);
    let rust = WasmCompiler::compile(&aipl_core::parser::Parser::parse(&flat(&src)).unwrap()).unwrap();
    assert!(vm_self_host(&src).unwrap() == rust, "codegen.aipl run in the VM: its bytes differ from the Rust backend's");
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
    let module_ast = aipl_core::parser::Parser::parse(&flat(&src)).unwrap();
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

/// Constants and enums (AIPL_SPEC.md 4.I): consts.aipl erases them before
/// codegen; the Rust toolchain keeps enums as types for its checker. Both
/// must emit the same bytes: constants of every kind (a string constant is
/// interned like the literal), enum types in parameters, results, struct
/// fields, arrays, and function references, members with explicit values,
/// enum.cast and enum.ord.
#[test]
fn self_hosted_bytes_match_consts_and_enums() {
    assert_self_hosted_matches_rust(
        "consts_enums",
        "(module ce (import alloc)
  (const LIMIT:i32 -12)
  (const WIDE:i64 4294967296i64)
  (const HALF:f64 0.5)
  (const YES:bool true)
  (const WORD:str \"word\")
  (enum Kind [atom (group 20) square])
  (struct Node [kind:Kind next:i32])
  (fn kind_of [n:(ptr Node)] -> Kind (get n Node.kind))
  (fn same [a:Kind b:Kind] -> bool (eq a b))
  (fn bump [k:Kind] -> (result Kind i32) (ok:i32 (enum.cast Kind (+ (enum.ord k) 1))))
  (fn main [] -> i32
    (let n:(ptr Node) (new Node (call alloc.forever)))
    (put n Node.kind Kind.group)
    (let ks:(arr Kind) (arr.new Kind (call alloc.forever) 3))
    (arr.set Kind ks 2 Kind.square)
    (let f:(fn [Kind Kind] -> bool) (ref same))
    (let w:str WORD)
    (+ (str.len w)
       (+ (match_result (call bump (call kind_of n)) (ok k (enum.ord k)) (err e 0))
          (+ (if (call_ref (fn [Kind Kind] -> bool) f (arr.get Kind ks 2) Kind.square) LIMIT 0)
             (+ (i32.wrap (/ WIDE 65536i64)) (if YES (i32.wrap (i64.trunc_f64_s (* HALF 10.0))) 0)))))))",
    );
}

/// Locals declared inside operands, loop bounds, and a match_result's matched
/// expression are numbered in the same order by both compilers (a pre-order
/// walk); see tests/test_differential.rs NESTED_LOCALS.
#[test]
fn self_hosted_bytes_match_nested_locals() {
    assert_self_hosted_matches_rust(
        "nested_locals",
        "(module nested
  (fn g [n:i32] -> (result i32 i32) (if (gt n 0) (ok:i32 n) (err:i32 n)))
  (fn main [] -> i32
    (let total:i32 0)
    (counted_loop i 0 (block (let last:i32 3) last) 1
      (set! total (+ total (match_result (call g i) (ok k (+ k 1)) (err e (- e 1))))))
    (+ total
       (+ (block (let x:i32 40) (+ x 2))
          (match_result (block (let y:i32 5) (call g y)) (ok v (* v 100)) (err w 0))))))",
    );
}

/// Past its table limits the self-hosted compiler reports an error rather
/// than failing: more than 1024 locals in a function is compile error 93,
/// and more than 8192 functions is 92 (codegen.aipl's tables are bounded
/// arrays since the CR6 rewrite, so the counts stop at the limit).
#[test]
fn self_hosted_table_limits_are_compile_errors() {
    let lets: String = (0..1100).map(|i| format!("(let v{i}:i32 {i}) ")).collect();
    let err = self_host(&format!("(module m (fn f [] -> i32 {lets} 0))")).unwrap_err();
    assert!(err.contains("compile error 93"), "{err}");
    let fns: String = (0..8200).map(|i| format!("(fn f{i} [] -> i32 {i}) ")).collect();
    let err = self_host(&format!("(module m {fns})")).unwrap_err();
    assert!(err.contains("compile error 92"), "{err}");
}

#[test]
fn self_hosted_bytes_match_unsigned_comparisons() {
    assert_self_hosted_matches_rust(
        "unsigned",
        "(module u (fn f [a:i32 b:i32 c:i64 d:i64] -> i32
           (+ (if (ltu a b) 1 0) (+ (if (lteu a b) 2 0) (+ (if (gtu a b) 4 0) (+ (if (gteu a b) 8 0)
           (+ (if (ltu c d) 16 0) (+ (if (lteu c d) 32 0) (+ (if (gtu c d) 64 0) (if (gteu c d) 128 0))))))))))",
    );
}

/// Checked arithmetic: three hidden i64 locals per function that uses it
/// (registered where the first checked operation is met, in the pre-order
/// walk), and the same instruction sequences in both compilers.
#[test]
fn self_hosted_bytes_match_checked_arithmetic() {
    assert_self_hosted_matches_rust(
        "checked",
        "(module ck
  (fn f [a:i32 b:i32] -> i32 (let x:i32 (checked.add a b)) (checked.sub (checked.mul x 3) b))
  (fn g [a:i64 b:i64] -> i64 (checked.add (checked.mul a b) (checked.sub a b)))
  (fn h [] -> i32 (let n:i32 1) (counted_loop i 0 3 1 (set! n (checked.mul n 2))) n))",
    );
}

/// Unions, make, and match: variant layouts (i64/f64 fields aligned to 8,
/// bool fields normalized when read), a recursive union (its rest behind a
/// pointer), enum arms (their
/// heads numbers after consts.aipl), an else-only match, void arms, and
/// make/match nested in each other's operands, so the scratch local is
/// reused while a cell is being filled.
#[test]
fn self_hosted_bytes_match_sum_types() {
    assert_self_hosted_matches_rust(
        "sum_types",
        "(module sum (import alloc)
  (union Shape [(circle r:f64) (rect w:i32 h:i32) (big n:i64 flag:bool) (empty)])
  (union List [(nil) (cons head:i32 tail:(ptr Rest))])
  (struct Rest [l:List])
  (enum Color [red green (blue 10)])
  (struct Holder [s:Shape c:Color])
  (fn area [s:Shape] -> i64 (unsafe)
    (match s
      (Shape.circle [r] (i64.trunc_f64_s (* r r)))
      (Shape.rect [w h] (i64.extend_s (* w h)))
      (Shape.big [n flag] (if flag n 0i64))
      (Shape.empty -1i64)))
  (fn sum [l:List] -> i32 (unsafe)
    (match l (List.nil 0) (List.cons [x rest] (+ x (call sum (get rest Rest.l))))))
  (fn cons [x:i32 l:List] -> List (unsafe)
    (let r:(ptr Rest) (new Rest (call alloc.forever)))
    (put r Rest.l l)
    (make List.cons x r))
  (fn code [c:Color] -> i32 (unsafe) (match c (Color.red 1) (Color.blue 3) (else 2)))
  (fn always [c:Color] -> i32 (unsafe) (match c (else 7)))
  (fn effect [s:Shape] -> void (unsafe)
    (match s (Shape.empty (mem.store32 2048 1)) (else (mem.store32 2048 2))))
  (fn main [] -> i32 (unsafe)
    (let h:(ptr Holder) (new Holder (call alloc.forever)))
    (put h Holder.s (make Shape.rect 3 (match (make Shape.circle 2.0) (Shape.circle [q] (i32.wrap (i64.trunc_f64_s q))) (else 0))))
    (put h Holder.c Color.blue)
    (call effect (get h Holder.s))
    (let l:List (call cons 1 (call cons 2 (make List.nil))))
    (+ (i32.wrap (call area (get h Holder.s))) (+ (call sum l) (+ (call code (get h Holder.c)) (call always Color.red))))))",
    );
}

/// Literal spellings the two tokenizers now agree on: a leading '+', and a
/// 32-bit pattern written unsigned (4294967295 is -1).
#[test]
fn self_hosted_bytes_match_signed_literal_spellings() {
    assert_self_hosted_matches_rust(
        "literal_spellings",
        "(module lits
  (fn a [] -> i64 (+ +5i64 -5i64))
  (fn b [] -> f64 (+ +2.0 -0.5))
  (fn c [] -> i32 (+ +7 (+ 4294967295 2147483648))))",
    );
}

#[test]
fn self_hosted_bytes_match_wildcard_binders() {
    assert_self_hosted_matches_rust(
        "wildcards",
        "(module wild (union U [(a x:i64 y:bool) (b x:f64 y:i32)])
  (fn f [u:U] -> i32 (match u (U.a [_ _] 1) (U.b [_ y] y))))",
    );
}

/// Value structs (AIPL_SPEC.md 4.E): every form (tests/aipl/values.aipl,
/// imports and generics included) and every way through control flow
/// (values_flow.aipl), at byte parity.
#[test]
fn self_hosted_bytes_match_value_structs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for name in ["values", "values_flow", "unions_by_value", "results_by_value"] {
        let module = Resolver::resolve(&root.join(format!("tests/aipl/{name}.aipl"))).unwrap_or_else(|e| panic!("{e}"));
        assert_self_hosted_matches_rust(name, &aipl_core::printer::print_module(&module));
    }
    // as written, so codegen.aipl sees the cond the printer would make ifs of
    let flow = fs::read_to_string(root.join("tests/aipl/values_flow.aipl")).unwrap();
    assert_self_hosted_matches_rust("values_flow_as_written", &flow);
}
