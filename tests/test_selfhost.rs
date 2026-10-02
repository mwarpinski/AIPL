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
    // The tree-walking VM recurses once per nested AIPL call; give the compile a big stack.
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(move || self_host_on_this_thread(&src))
        .unwrap()
        .join()
        .unwrap()
}

fn self_host_on_this_thread(src: &str) -> Result<Vec<u8>, String> {
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
    (let p:(ptr Point) (new Point))
    (put p Point.x x)
    (put p Point.y y)
    (+ (get p Point.x) (get p Point.y)))
  (fn flags [] -> bool
    (let f:(ptr Flags) (new Flags))
    (put f Flags.on true)
    (put f Flags.name "n")
    (let a:(arr bool) (arr.new bool 2))
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
    (let a:(arr i32) (arr.new i32 n))
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

/// P9: i64 code (literals at both extremes, arithmetic, unsigned ops, shifts,
/// comparisons, if blocks typed i64, conversions, an i64 struct field after a
/// bool, i64 arrays, mem.load64/store64).
#[test]
fn self_hosted_bytes_match_i64() {
    let src = r#"
(module i64demo
  (struct Acc [flag:bool total:i64 n:i32])
  (fn add [a:i64 b:i64] -> i64 (+ a b))
  (fn mix [x:i32] -> i64
    (let big:i64 9223372036854775807i64)
    (let neg:i64 -9223372036854775808i64)
    (let a:i64 (i64.extend_s x))
    (if (gt a 100i64)
        (bitand (shl a 3i64) big)
        (- (divu a 7i64) (remu neg 3i64))))
  (fn cmp [a:i64 b:i64] -> i32
    (if (and (lte a b) (neq a 0i64)) (i32.wrap (shru b 1i64)) -1))
  (fn acc [n:i32] -> i64
    (let s:(ptr Acc) (new Acc))
    (put s Acc.total 0i64)
    (loop i 1 n 1
      (put s Acc.total (+ (get s Acc.total) (i64.extend_u i))))
    (let arr:(arr i64) (arr.new i64 4))
    (arr.set i64 arr 3 (get s Acc.total))
    (+ (arr.get i64 arr 3) (mem.load64 (+ (ptr.addr s) 8)))))
"#;
    assert_self_hosted_matches_rust("i64", src);
    assert_self_hosted_matches_rust(
        "i64_mem",
        "(module s64 (fn f [p:i32 v:i64] -> i64 (mem.store64 p v) (mem.load64 p)))",
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
/// digits, random sign and decimal point position, all inside the exact range
/// (mantissa <= 2^53, <= 22 fractional digits), compiled in one module. Literals
/// outside that range must be compile error 973, never a different rounding.
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
    src.push(')');
    assert_self_hosted_matches_rust("float_literals", &src);

    for lit in ["9007199254740993.0", "0.00000000000000000000001", "1234567890123456789.0"] {
        let err = self_host(&format!("(module m (fn f [] -> f64 {lit}))")).unwrap_err();
        assert!(err.contains("compile error 973"), "{lit}: {err}");
    }
}

/// Typed pointers and arrays (the programs from tests/test_pointers.rs): a
/// linked list through (ptr Node), arrays of pointers and of arrays, arr.len,
/// casts, and typed nulls.
#[test]
fn self_hosted_bytes_match_pointers() {
    assert_self_hosted_matches_rust("pointers_0", r#"
(module list
  (struct Node [val:i32 next:(ptr Node)])
  (fn push [head:(ptr Node) v:i32] -> (ptr Node)
    (let n:(ptr Node) (new Node))
    (put n Node.val v)
    (put n Node.next head)
    n)
  (fn sum [] -> i32
    (let h:(ptr Node) (ptr.null Node))
    (loop i 1 10 1 (set! h (call push h i)))
    (let total:i32 0)
    (while (neq h (ptr.null Node))
      (set! total (+ total (get h Node.val)))
      (set! h (get h Node.next)))
    total))
"#);
    assert_self_hosted_matches_rust("pointers_1", r#"
(module arrs
  (struct P [x:i32])
  (fn f [] -> i32
    (let ps:(arr (ptr P)) (arr.new (ptr P) 4))
    (loop i 0 3 1
      (let p:(ptr P) (new P))
      (put p P.x (* i i))
      (arr.set (ptr P) ps i p))
    (let total:i32 0)
    (loop i 0 (- (arr.len ps) 1) 1
      (set! total (+ total (get (arr.get (ptr P) ps i) P.x))))
    ;; round trip through an address: same pointer, same field
    (let back:(ptr P) (ptr.cast P (ptr.addr (arr.get (ptr P) ps 3))))
    (let nested:(arr (arr i32)) (arr.new (arr i32) 1))
    (arr.set (arr i32) nested 0 (arr.new i32 7))
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
        ("std_strmap", "aipl_src/std/strmap.aipl"),
        ("std_buf", "aipl_src/std/buf.aipl"),
        ("word_count", "examples/word_count.aipl"),
    ] {
        let module = Resolver::resolve(&root.join(rel)).unwrap_or_else(|e| panic!("{e}"));
        assert_self_hosted_matches_rust(name, &aipl_core::printer::print_module(&module));
    }
}

/// Function references (P10): ref, call_ref through params, arrays, and struct
/// fields, an i64 signature, and ref equality; the funcref table, element
/// section, and call_indirect types must match the Rust backend.
#[test]
fn self_hosted_bytes_match_function_refs() {
    assert_self_hosted_matches_rust("refs", r#"
(module refs
  (struct Op [apply:(fn [i32 i32] -> i32) name:str])
  (fn add [a:i32 b:i32] -> i32 (+ a b))
  (fn mul [a:i32 b:i32] -> i32 (* a b))
  (fn neg [a:i64] -> i64 (- 0i64 a))
  (fn twice [f:(fn [i32 i32] -> i32) x:i32] -> i32
    (call_ref (fn [i32 i32] -> i32) f x x))
  (fn main [] -> i32
    (let ops:(arr (fn [i32 i32] -> i32)) (arr.new (fn [i32 i32] -> i32) 2))
    (arr.set (fn [i32 i32] -> i32) ops 0 (ref add))
    (arr.set (fn [i32 i32] -> i32) ops 1 (ref mul))
    (let o:(ptr Op) (new Op))
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
  (fn first_mult7 [n:i32] -> i32
    (loop i n (+ n 100) 1
      (if (eq (% i 7) 0) (return i) (block)))
    -1)

  ;; break inside a nested if: sum 1.. until the total passes n
  (fn sum_until [n:i32] -> i32
    (let total:i32 0)
    (let i:i32 0)
    (while true
      (set! i (+ i 1))
      (if (gt i 1000)
          (break)
          (if (gt total n) (break) (set! total (+ total i)))))
    total)

  ;; continue in a counted loop still applies the step: sum of odd i in 0..n
  (fn sum_odd [n:i32] -> i32
    (let total:i32 0)
    (loop i 0 n 1
      (if (eq (% i 2) 0) (continue) (block))
      (set! total (+ total i)))
    total)

  ;; continue in a while loop goes back to the condition
  (fn count_nonzero_digits [n:i32] -> i32
    (let v:i32 n)
    (let count:i32 0)
    (while (gt v 0)
      (let d:i32 (% v 10))
      (set! v (/ v 10))
      (if (eq d 0) (continue) (block))
      (set! count (+ count 1)))
    count)

  ;; break and continue in nested loops target the innermost loop
  (fn nested [n:i32] -> i32
    (let hits:i32 0)
    (loop i 1 n 1
      (loop j 1 n 1
        (if (gt j i) (break) (block))
        (if (eq j 2) (continue) (block))
        (set! hits (+ hits 1))))
    hits)

  ;; return from inside match_result inside a loop
  (fn parse_digit [c:i32] -> (result i32 i32)
    (if (and (gte c 48) (lte c 57)) (ok (- c 48)) (err c)))
  (fn first_non_digit [n:i32] -> i32
    (loop i 0 n 1
      (match_result (call parse_digit (+ 46 i))
        (ok d (block))
        (err e (return e))))
    0)

  ;; cond with several clauses and multi-expression bodies, as a value
  (fn classify [n:i32] -> i32
    (cond
      ((lt n 0) -1)
      ((eq n 0) 0)
      ((lt n 10) (let t:i32 (* n 2)) (+ t 1))
      (else 100)))

  ;; cond as a statement
  (fn bucket_sum [n:i32] -> i32
    (let small:i32 0)
    (let big:i32 0)
    (loop i 0 n 1
      (cond
        ((lt i 5) (set! small (+ small 1)))
        (else (set! big (+ big 1)))))
    (+ (* small 1000) big))

  ;; a body may end in (return v); void functions use (return)
  (fn ends_in_return [n:i32] -> i32
    (let x:i32 (* n 3))
    (return (+ x 1)))
  (fn bump [p:i32] -> void
    (if (lt p 0) (return) (block))
    (mem.store32 p (+ (mem.load32 p) 1)))
  (fn uses_void_return [n:i32] -> i32
    (let p:i32 (mem.alloc 4))
    (mem.store32 p n)
    (call bump p)
    (call bump -1)
    (mem.load32 p))

  ;; loop bound and step are evaluated every iteration, and the body may set! the variable
  (fn moving_bounds [n:i32] -> i32
    (let limit:i32 n)
    (let count:i32 0)
    (loop i 0 limit 1
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
  (struct Node [val:i32 next:(ptr Node)])
  (fn process [n:i32] -> (result i32 i32)
    (let head:(ptr Node) (new Node))
    (put head Node.val n)
    (put head Node.next (ptr.null Node))
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
    (let a:(arr i32) (arr.new i32 n))
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
/// miscompile: non-scalar struct fields / array elements (95), unknown structs
/// (96), and string literals beyond its 64 KiB literal buffer (768).
#[test]
fn self_hosted_rejects_what_it_cannot_compile() {
    for src in [
        "(module m (struct S [a:i32 b:(result i32 i32)]) (fn f [] -> i32 (sizeof S)))",
        "(module m (fn f [] -> i32 (arr.new (result i32 i32) 3)))",
    ] {
        let err = self_host(src).unwrap_err();
        assert!(err.contains("compile error 95"), "{src}: {err}");
    }
    let err = self_host("(module m (fn f [] -> i32 (sizeof Missing)))").unwrap_err();
    assert!(err.contains("compile error 96"), "{err}");
    // The self-hosted literal buffer holds 64 KiB (the Rust backend allows up to 1 MiB).
    let long = "x".repeat(70_000);
    let err = self_host(&format!("(module m (fn f [] -> i32 (str.len \"{long}\")))")).unwrap_err();
    assert!(err.contains("compile error 768"), "{err}");
}

/// String literals sit at 1024 with the heap after them, so a program is not
/// limited to a fixed literal area; stores into a literal trap in both
/// backends, like stores into the runtime block.
#[test]
fn self_hosted_string_literals_past_the_old_area_are_read_only() {
    let a = "a".repeat(400);
    let b = "b".repeat(400);
    let src = format!(
        "(module lits\n  (fn lens [] -> i32 (+ (str.len \"{a}\") (str.len \"{b}\")))\n  (fn first_free [] -> i32 (mem.alloc 0))\n  (fn poke [] -> i32 (mem.store8 (str.ptr \"{b}\") 0) 1))"
    );
    assert_self_hosted_matches_rust("lits", &src);
    let bytes = run_self_hosted(&src);
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
    let m = aipl_core::parser::Parser::parse(&src).unwrap();
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
    assert!(!src.contains("(import"), "combined module must be import-free");
    src
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
