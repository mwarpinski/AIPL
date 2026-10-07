//! The type checker in AIPL (docs/design/CHECKER_PLAN.md): each piece checked
//! against the Rust front end it mirrors, on the same text.

use aipl_core::checker::TypeChecker;
use aipl_core::parser::{Parser, TokenKind};
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// A VM with an AIPL module (and its imports) loaded.
fn vm_with(rel: &str) -> VM {
    let m = Resolver::resolve(&root().join(rel)).unwrap();
    TypeChecker::new().check_module(&m).unwrap();
    let mut vm = VM::new();
    vm.load_module(m);
    vm
}

fn int(v: Value) -> i64 {
    match v {
        Value::Int(i) => i,
        other => panic!("expected Int, got {other:?}"),
    }
}

/// One token as both tokenizers can describe it: kind, line, column, and
/// its value spelled out.
#[derive(Debug, PartialEq)]
struct Tok {
    kind: String,
    line: u32,
    col: u32,
    value: String,
}

/// Line and column of byte offset `pos`, counting characters (as the Rust
/// tokenizer does).
fn line_col(src: &str, pos: usize) -> (u32, u32) {
    let before = &src[..pos];
    let line = before.matches('\n').count() as u32 + 1;
    let col = before.rsplit('\n').next().unwrap().chars().count() as u32 + 1;
    (line, col)
}

fn unescape(raw: &str) -> String {
    let mut out = String::new();
    let mut it = raw.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            out.push(match it.next().unwrap() {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                '0' => '\0',
                other => other,
            });
        } else {
            out.push(c);
        }
    }
    out
}

/// The Rust tokenizer's tokens, or its error message.
fn rust_tokens(src: &str) -> Result<Vec<Tok>, String> {
    let toks = Parser::tokenize(src)?;
    Ok(toks
        .into_iter()
        .map(|t| {
            let (kind, value) = match t.kind {
                TokenKind::LParen => ("lparen", String::new()),
                TokenKind::RParen => ("rparen", String::new()),
                TokenKind::LBracket => ("lbracket", String::new()),
                TokenKind::RBracket => ("rbracket", String::new()),
                TokenKind::Colon => ("colon", String::new()),
                TokenKind::Arrow => ("arrow", String::new()),
                TokenKind::Symbol(s) => ("symbol", s),
                // the AIPL token holds the low 32 bits
                TokenKind::IntLit(i) => ("int", (i as i32).to_string()),
                TokenKind::BoolLit(b) => ("bool", b.to_string()),
                TokenKind::StringLit(s) => ("string", s),
                TokenKind::Int64Lit(i) => ("int64", i.to_string()),
                TokenKind::FloatLit(f, _) => ("float", f.to_bits().to_string()),
            };
            Tok { kind: kind.into(), line: t.line, col: t.col, value }
        })
        .collect())
}

const KINDS: [&str; 13] = ["lparen", "rparen", "lbracket", "rbracket", "colon", "arrow", "symbol", "int", "bool", "string", "int64", "float", "bad"];

/// compiler.tokenize's tokens; a bad token (always last) becomes Err with
/// its position and TokErr.
fn aipl_tokens(vm: &mut VM, src: &str) -> Result<Vec<Tok>, String> {
    let n = src.len();
    let input = int(vm.invoke("host_alloc", vec![Value::Int(n as i64 + 1)]).unwrap());
    vm.write_bytes(input as usize, src.as_bytes());
    let size = int(vm.invoke("token_size", vec![]).unwrap()) as usize;
    let out = int(vm.invoke("host_alloc", vec![Value::Int(((n + 1) * size) as i64)]).unwrap());
    let count = int(vm.invoke("tokenize", vec![Value::Int(input), Value::Int(n as i64), Value::Int(out)]).unwrap());
    let word = |vm: &VM, a: usize| i32::from_le_bytes(vm.read_bytes(a, 4).try_into().unwrap());
    let mut toks = Vec::new();
    for i in 0..count as usize {
        let rec = out as usize + i * size;
        let (kind, a, b, pos) = (word(vm, rec), word(vm, rec + 4), word(vm, rec + 8), word(vm, rec + 12) as usize);
        let kind = KINDS[kind as usize];
        let (line, col) = line_col(src, pos);
        let span = |a: i32, b: i32| src[a as usize..(a + b) as usize].to_string();
        let value = match kind {
            "symbol" => span(a, b),
            // the literal's text; compared as its low 32 bits
            "int" => (span(a, b).parse::<i64>().unwrap() as i32).to_string(),
            "bool" => (a != 0).to_string(),
            "string" => unescape(&span(a, b)),
            "int64" => span(a, b - 3).parse::<i64>().unwrap().to_string(),
            "float" => span(a, b).parse::<f64>().unwrap().to_bits().to_string(),
            "bad" => return Err(format!("{line}:{col} {}", ["unterminated_string", "unknown_escape", "stray_whitespace"][a as usize])),
            _ => String::new(),
        };
        toks.push(Tok { kind: kind.into(), line, col, value });
    }
    Ok(toks)
}

/// The first `L:C` of a Rust error, and which tokenizer error it is.
fn rust_error(e: &str) -> String {
    let pos = e.split(": ").next().unwrap();
    let what = if e.contains("Unterminated string") {
        "unterminated_string"
    } else if e.contains("Unknown escape") {
        "unknown_escape"
    } else if e.contains("unexpected whitespace") {
        "stray_whitespace"
    } else {
        panic!("unexpected tokenizer error {e}")
    };
    format!("{pos} {what}")
}

const TRICKY: &[&str] = &[
    "(+ 1 -2 +3 007 -0 2147483647 2147483648 4294967295 4294967296 -2147483649)",
    "9223372036854775807 9223372036854775808 -9223372036854775808 -9223372036854775809 00000000000000000000001",
    "5i64 -5i64 +5i64 i64 -i64 9223372036854775807i64 9223372036854775808i64 1.5i64",
    "1.5 -0.25 +2.0 .5 5. 1.5e3 1.5E-3 2.e+10 1e5 1.5e 1.5.3 - + -. . e.5",
    "-> true false truex ->x a.b.c x:i32 [a b] ;; comment\n(x)",
    "\"a\\nb\\t\\\"q\\\\\" \"\" \"é ü 日本\" (é) x\r\ny",
    "\"unterminated",
    "\"line\nbreak\"",
    "\"bad \\q escape\"",
    "\"ends in backslash\\",
    "(a\u{000C}b)",
    "(a \u{00A0}b)",
    "(a\u{2003})",
    "(\u{3000})",
    "(a\u{000B})",
    "\"\u{00A0} inside a string is fine\" ;; and \u{2003} in a comment\n1",
    "(é\u{00A0})",
];

#[test]
fn tokenizers_agree_on_every_repository_file_and_tricky_input() {
    let mut vm = vm_with("aipl_src/frontend_host.aipl");
    let mut inputs: Vec<(String, String)> = TRICKY.iter().map(|s| (format!("{s:?}"), s.to_string())).collect();
    for dir in ["aipl_src", "aipl_src/std", "aipl_src/native", "examples", "tests/aipl", "benchmarks"] {
        let mut files: Vec<_> = std::fs::read_dir(root().join(dir)).unwrap().map(|e| e.unwrap().path()).collect();
        files.sort();
        for f in files.into_iter().filter(|f| f.extension().is_some_and(|e| e == "aipl")) {
            inputs.push((f.display().to_string(), std::fs::read_to_string(&f).unwrap()));
        }
    }
    assert!(inputs.len() > 40, "{} inputs", inputs.len());
    for (name, src) in &inputs {
        let ours = aipl_tokens(&mut vm, src);
        match rust_tokens(src) {
            Ok(theirs) => {
                let ours = ours.unwrap_or_else(|e| panic!("{name}: AIPL tokenizer error {e}, Rust accepts"));
                for (i, (a, b)) in theirs.iter().zip(ours.iter()).enumerate() {
                    assert_eq!(a, b, "{name}: token {i}");
                }
                assert_eq!(theirs.len(), ours.len(), "{name}: token count");
            }
            Err(e) => assert_eq!(ours.err(), Some(rust_error(&e)), "{name}: Rust says {e}"),
        }
    }
}

/// line_col counts lines from 1 and columns in characters.
#[test]
fn line_col_counts_characters() {
    let mut vm = vm_with("aipl_src/frontend_host.aipl");
    let src = "ab\né日x\n\nz";
    let p = int(vm.invoke("host_alloc", vec![Value::Int(src.len() as i64)]).unwrap());
    vm.write_bytes(p as usize, src.as_bytes());
    for (pos, want) in [(0, (1, 1)), (1, (1, 2)), (3, (2, 1)), (5, (2, 2)), (8, (2, 3)), (10, (3, 1)), (11, (4, 1))] {
        let line = int(vm.invoke("line_of", vec![Value::Int(p), Value::Int(pos)]).unwrap());
        let col = int(vm.invoke("col_of", vec![Value::Int(p), Value::Int(pos)]).unwrap());
        assert_eq!((line as u32, col as u32), want, "offset {pos}");
        assert_eq!(line_col(src, pos as usize), want, "offset {pos} (test helper)");
    }
}

/// The AIPL front end (aipl_src/frontend_host.aipl) compiled to wasm by the
/// Rust toolchain, run under wasmtime: large inputs (the whole toolchain)
/// would take minutes in the VM.
mod front {
    use super::root;
    use aipl_core::checker::TypeChecker;
    use aipl_core::compiler::wasm::WasmCompiler;
    use aipl_core::resolver::Resolver;
    use wasmtime::{Engine, Instance, Linker, Memory, Module, Store, TypedFunc};
    use wasmtime_wasi::p1::WasiP1Ctx;

    pub struct Front {
        engine: Engine,
        module: Module,
    }

    pub fn load() -> Front {
        let m = Resolver::resolve(&root().join("aipl_src/frontend_host.aipl")).unwrap();
        TypeChecker::new().check_module(&m).unwrap();
        let wasm = WasmCompiler::compile(&m).unwrap();
        let mut config = wasmtime::Config::new();
        config.wasm_threads(true);
        let engine = Engine::new(&config).unwrap();
        let module = Module::new(&engine, &wasm).unwrap();
        Front { engine, module }
    }

    impl Front {
        /// Calls `name [src len out]` on a fresh instance; Ok(text) for status
        /// 1, Err(text) for status 0.
        pub fn run(&self, name: &str, src: &str) -> Result<String, String> {
            let mut linker: Linker<WasiP1Ctx> = Linker::new(&self.engine);
            wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
            let mut store = Store::new(&self.engine, wasmtime_wasi::WasiCtxBuilder::new().build_p1());
            let inst: Instance = linker.instantiate(&mut store, &self.module).unwrap();
            let memory: Memory = inst.get_memory(&mut store, "memory").unwrap();
            let alloc: TypedFunc<i32, i32> = inst.get_typed_func(&mut store, "host_alloc").unwrap();
            let f: TypedFunc<(i32, i32, i32), i32> = inst.get_typed_func(&mut store, name).unwrap();
            let p = alloc.call(&mut store, src.len() as i32 + 1).unwrap();
            memory.write(&mut store, p as usize, src.as_bytes()).unwrap();
            let out = alloc.call(&mut store, 12).unwrap();
            f.call(&mut store, (p, src.len() as i32, out)).unwrap_or_else(|e| panic!("{name} trapped: {e:?}"));
            let mut w = [0u8; 12];
            memory.read(&store, out as usize, &mut w).unwrap();
            let word = |i: usize| u32::from_le_bytes(w[i * 4..i * 4 + 4].try_into().unwrap()) as usize;
            let mut text = vec![0u8; word(2)];
            memory.read(&store, word(1), &mut text).unwrap();
            let text = String::from_utf8(text).unwrap();
            if word(0) == 1 { Ok(text) } else { Err(text) }
        }
    }
}

/// Every .aipl file under the given directories, sorted.
fn aipl_files(dirs: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for dir in dirs {
        let mut files: Vec<_> = std::fs::read_dir(root().join(dir)).unwrap().map(|e| e.unwrap().path()).collect();
        files.sort();
        out.extend(files.into_iter().filter(|f| f.extension().is_some_and(|e| e == "aipl")));
    }
    out
}

/// CK2-CK3 print parity: for every repository program, the Rust resolver's
/// module printed by src/printer.rs, parsed by parser.aipl and printed by
/// printer.aipl, is the same text.
#[test]
fn parser_round_trips_every_repository_program() {
    let front = front::load();
    let mut checked = 0;
    for f in aipl_files(&["aipl_src", "aipl_src/std", "aipl_src/native", "examples", "tests/aipl", "benchmarks"]) {
        let Ok(m) = Resolver::resolve(&f) else { continue };
        let text = aipl_core::printer::print_module(&m);
        let ours = front.run("parse_print", &text).unwrap_or_else(|e| panic!("{}: parser.aipl rejects: {e}", f.display()));
        if ours != text {
            let at = ours.bytes().zip(text.bytes()).position(|(a, b)| a != b).unwrap_or(ours.len().min(text.len()));
            panic!(
                "{}: round trip differs at byte {at}\n  rust: {:?}\n  aipl: {:?}",
                f.display(),
                &text[at.saturating_sub(60)..(at + 60).min(text.len())],
                &ours[at.saturating_sub(60)..(at + 60).min(ours.len())]
            );
        }
        checked += 1;
    }
    assert!(checked > 40, "{checked} programs");
}

/// Every form and every operator at least once (cond, match, make, ok:T,
/// call_ref, the address forms, ...), for the direct-parse test below.
const EVERY_FORM: &str = r#"(module every
  (import io as out)
  (enum Color [red (green 5) blue])
  (union Shape [(circle r:f64) (rect w:i32 h:i32) (dot)])
  (struct P [x:i32 next:(ptr P) items:(arr i64) f:(fn [i32] -> bool) r:(result i32 str) c:Color s:Shape])
  (fn helper [n:i32] -> bool (gt n 0))
  (fn every [a:i32 b:i64 c:f64 d:bool e:str p:(ptr P) xs:(arr i32)] -> i32
    (req (gte a 0))
    (ens (gte res 0))
    (inv true)
    (let n:i32 (+ +1 -2))
    (set! n (cond ((lt n 0) 1) ((eq n 0) (block 2)) (else 3)))
    (if d (block) (block (return 1)))
    (loop i 0 10 2 (if (eq i 4) (continue) (block)) (if (eq i 8) (break) (block)))
    (while false)
    (let r:(result i32 str) (ok:str 5))
    (let r2:(result bool i32) (err:bool 7))
    (match_result r (ok v (set! n v)) (err m (block)))
    (let q:(ptr P) (new P))
    (put q P.x (get p P.x))
    (let z:i32 (sizeof P))
    (let arr2:(arr f64) (arr.new f64 3))
    (arr.set f64 arr2 0 (arr.get f64 arr2 1))
    (let l:i32 (arr.len arr2))
    (let np:(ptr P) (ptr.null P))
    (let na:(arr i32) (arr.null i32))
    (let cp:(ptr P) (ptr.cast P 4096))
    (let ca:(arr i32) (arr.cast i32 4096))
    (let ad:i32 (+ (ptr.addr p) (arr.addr xs)))
    (let co:i32 (enum.ord (enum.cast Color 5)))
    (let sh:Shape (make Shape.rect 1 2))
    (let area:i32 (match sh (Shape.circle [_] 0) (Shape.rect [w h] (* w h)) (else 1)))
    (let col:i32 (match Color.red (Color.red 1) (Color.green 2) (Color.blue 3)))
    (let fr:(fn [i32] -> bool) (ref helper))
    (let fb:bool (call_ref (fn [i32] -> bool) fr 3))
    (let s1:str "esc \n \t \r \0 \\ \" é")
    (let x:i32 (+ (- (* (/ (% 1 2) 3) 4) 5) (divu (remu 6 7) 8)))
    (let y:i32 (^ (shl 1 2) (shr (shru 3 4) (bitand 5 (bitor 6 7)))))
    (mem.store8 (mem.alloc 8) (mem.load8 (mem.alloc 8)))
    (mem.store32 (mem.alloc 8) (mem.load32 (mem.alloc 8)))
    (mem.store64 (mem.alloc 8) (mem.load64 (mem.alloc 8)))
    (let g:i32 (mem.grow 0))
    (let aa:i32 (atomic.add (mem.alloc 4) 1))
    (let ac:bool (atomic.cas (mem.alloc 4) 0 1))
    (atomic.lock (mem.alloc 4))
    (atomic.unlock (mem.alloc 4))
    (let cmp:bool (and (eq 1 1) (and (neq 1 2) (and (lt 1 2) (and (lte 1 2) (and (gt 2 1) (gte 2 1)))))))
    (let ucmp:bool (or (ltu 1 2) (or (lteu 1 2) (or (gtu 2 1) (not (gteu 2 1))))))
    (let ck:i64 (checked.add (checked.sub 1i64 2i64) (checked.mul 3i64 -4i64)))
    (sys.print "a" "b")
    (let t:i64 (+ (sys.time) (sys.monotonic)))
    (let rnd:i32 (sys.random (mem.alloc 8) 8))
    (let fo:i32 (fs.open (str.ptr e) (str.len e) 0))
    (let fio:i32 (+ (fs.read fo 0 0) (+ (fs.write fo 0 0) (+ (fs.close fo) (fs.delete 0 0)))))
    (let ar:i32 (+ (args.sizes 0 0) (+ (args.get 0 0) (+ (env.sizes 0 0) (env.get 0 0)))))
    (let th:i32 (thread.join (thread.spawn (ref w) 1)))
    (let cv:i64 (+ (i64.extend_s 1) (+ (i64.extend_u 1) (+ (i64.trunc_f64_s 1.5) (i64.reinterpret_f64 (f64.sqrt (f64.convert_i64_s 4i64)))))))
    (let rf:f64 (f64.reinterpret_i64 4611686018427387904i64))
    (let wr:i32 (i32.wrap 4294967297i64))
    (if (eq n 0) (sys.exit 0) (block))
    (call out.println "x")
    0)
  (fn w [x:i32] -> i32 x)
  (fn nothing [] -> void (return)))"#;

/// The same source parsed directly by both parsers (no resolver), printed by
/// both printers: the same text, for the corpus above and for every
/// repository file Parser::parse accepts (those without constants).
#[test]
fn parsers_agree_on_original_sources() {
    let front = front::load();
    let mut inputs = vec![("every_form".to_string(), EVERY_FORM.to_string())];
    for f in aipl_files(&["aipl_src", "aipl_src/std", "aipl_src/native", "examples", "tests/aipl", "benchmarks"]) {
        inputs.push((f.display().to_string(), std::fs::read_to_string(&f).unwrap()));
    }
    let mut compared = 0;
    for (name, src) in &inputs {
        let m = match Parser::parse(src) {
            Ok(m) => m,
            Err(_) if name != "every_form" => continue,
            Err(e) => panic!("the corpus must parse: {e}"),
        };
        let theirs = aipl_core::printer::print_module(&m);
        // parser.aipl keeps literals as written; normalize through Rust
        let ours = front.run("parse_print", src).unwrap_or_else(|e| panic!("{name}: parser.aipl rejects: {e}"));
        let ours = aipl_core::printer::print_module(&Parser::parse(&ours).unwrap_or_else(|e| panic!("{name}: AIPL output does not parse: {e}\n{ours}")));
        assert_eq!(ours, theirs, "{name}");
        compared += 1;
    }
    assert!(compared >= 25, "{compared}");
}

/// CK4: malformed programs reaching each of src/parser.rs's errors (and the
/// tokenizer's): parser.aipl reports the identical message, position
/// included.
const BAD_PARSES: &[&str] = &[
    "",
    "x",
    "(foo m)",
    "(module",
    "(module 5)",
    "(module m",
    "(module m) x",
    "(module m)\n  )",
    "(module m (const XX:i32 1))",
    "(module m (import))",
    "(module m (import a as))",
    "(module m (import a as 5))",
    "(module m (import a b))",
    "(module m (struct))",
    "(module m (struct S x))",
    "(module m (struct S [5:i32]))",
    "(module m (struct S [x i32]))",
    "(module m (struct S [x:]))",
    "(module m (struct S [x:i32] y))",
    "(module m (enum))",
    "(module m (enum E a))",
    "(module m (enum E [a (b)]))",
    "(module m (enum E [a (b x)]))",
    "(module m (enum E [a 5]))",
    "(module m (enum E [(a 3000000000)]))",
    "(module m (enum E [a (5 1)]))",
    "(module m (enum E [a [b]]))",
    "(module m (enum E [a",
    "(module m (enum E [(a 1 2)]))",
    "(module m (union U [a]))",
    "(module m (union U [(5)]))",
    "(module m (union U [(a x)]))",
    "(module m (union U [(a x:)]))",
    "(module m (union U (a)))",
    "(module m (fn))",
    "(module m (fn f))",
    "(module m (fn f x))",
    "(module m (fn f [x]))",
    "(module m (fn f [5:i32]))",
    "(module m (fn f [] i32))",
    "(module m (fn f [] -> ))",
    "(module m (5 f [] -> i32))",
    "(module m (fn 5 [] -> i32))",
    "(module m (fn f [x:(ptr i32)] -> i32 0))",
    "(module m (fn f [x:(ptr)] -> i32 0))",
    "(module m (fn f [x:(arr i32 5)] -> i32 0))",
    "(module m (fn f [x:(foo i32)] -> i32 0))",
    "(module m (fn f [x:5x] -> i32 0))",
    "(module m (fn f [x:-y] -> i32 0))",
    "(module m (fn f [x:(5)] -> i32 0))",
    "(module m (fn f [x:(fn i32)] -> i32 0))",
    "(module m (fn f [x:(fn [i32] i32)] -> i32 0))",
    "(module m (fn f [x:(result i32)] -> i32 0))",
    "(module m (fn f [x:[i32]] -> i32 0))",
    "(module m (fn f [] -> i32 (5)))",
    "(module m (fn f [] -> i32 (1.50)))",
    "(module m (fn f [] -> i32 (0.0)))",
    "(module m (fn f [] -> i32 (\"a\\nb\\t\\\"c\")))",
    "(module m (fn f [] -> i32 (true)))",
    "(module m (fn f [] -> i32 (-5i64)))",
    "(module m (fn f [] -> i32 (+007)))",
    "(module m (fn f [] -> i32 ([x])))",
    "(module m (fn f [] -> i32 (foo 1)))",
    "(module m (fn f [] -> i32 ]))",
    "(module m (fn f [] -> i32 :))",
    "(module m (fn f [] -> i32 ->))",
    "(module m (fn f [] -> i32 (if 1 2)))",
    "(module m (fn f [] -> i32 (if 1 2 3 4)))",
    "(module m (fn f [] -> i32 (let 5 1)))",
    "(module m (fn f [] -> i32 (let x 1)))",
    "(module m (fn f [] -> i32 (let x:i32)))",
    "(module m (fn f [] -> i32 (set! 5 1)))",
    "(module m (fn f [] -> i32 (loop 5 0 1 1)))",
    "(module m (fn f [] -> i32 (call 5)))",
    "(module m (fn f [] -> i32 (ok:5 1)))",
    "(module m (fn f [] -> i32 (err:)))",
    "(module m (fn f [] -> i32 (make 5)))",
    "(module m (fn f [] -> i32 (make nodot 1)))",
    "(module m (fn f [] -> i32 (match x 5)))",
    "(module m (fn f [] -> i32 (match x (5))))",
    "(module m (fn f [] -> i32 (match x (else 1) (A.b 2))))",
    "(module m (fn f [] -> i32 (match x (else [y] 1))))",
    "(module m (fn f [] -> i32 (match x (A.b [5] 1))))",
    "(module m (fn f [] -> i32 (match x (A.b [y 1))))",
    "(module m (fn f [] -> i32 (match_result r (x v))))",
    "(module m (fn f [] -> i32 (match_result r (ok 5))))",
    "(module m (fn f [] -> i32 (match_result r (ok v) (x e))))",
    "(module m (fn f [] -> i32 (match_result r (ok v) (err 5))))",
    "(module m (fn f [] -> i32 (match_result r (ok v) 5)))",
    "(module m (fn f [] -> i32 (new 5)))",
    "(module m (fn f [] -> i32 (get p x)))",
    "(module m (fn f [] -> i32 (get p .x)))",
    "(module m (fn f [] -> i32 (get p S.)))",
    "(module m (fn f [] -> i32 (get p 5)))",
    "(module m (fn f [] -> i32 (put p x 1)))",
    "(module m (fn f [] -> i32 (put p 5 1)))",
    "(module m (fn f [] -> i32 (sizeof 5)))",
    "(module m (fn f [] -> i32 (sizeof)))",
    "(module m (fn f [] -> i32 (sizeof (ptr))))",
    "(module m (fn f [] -> i32 (return 1 2)))",
    "(module m (fn f [] -> i32 (break 1)))",
    "(module m (fn f [] -> i32 (cond)))",
    "(module m (fn f [] -> i32 (cond (else 1))))",
    "(module m (fn f [] -> i32 (cond ((eq 1 1)) (else 2))))",
    "(module m (fn f [] -> i32 (cond (else 1) ((eq 1 1) 2))))",
    "(module m (fn f [] -> i32 (cond ((eq 1 1) 1) (else))))",
    "(module m (fn f [] -> i32 (cond ((eq 1 1) 1) 5)))",
    "(module m (fn f [] -> i32 (ref 5)))",
    "(module m (fn f [] -> i32 (call_ref 5)))",
    "(module m (fn f [] -> i32 (ptr.null 5)))",
    "(module m (fn f [] -> i32 (ptr.cast 5 1)))",
    "(module m (fn f [] -> i32 (enum.cast 5 1)))",
    "(module m (fn f [] -> i32 (arr.new)))",
    "(module m (fn f [] -> i32 (arr.null)))",
    "(module m (fn f [] -> i32 (mem.free 8)))",
    "(module m (fn f [] -> i32 (mem.load_f64 8)))",
    "(module m (fn f [] -> i32 (+ 5000000000 1)))",
    "(module m (fn f [] -> i32 (+ -2147483649 1)))",
    "(module m (fn f [] -> i32 (req) 0))",
    "(module m (fn f [] -> i32 (ens 1 2) 0))",
    "(module m (fn f [] -> i32 (let x:i32",
    "(module m (fn f [] -> i32 (+ 1",
    "(module m (fn f [] -> i32 \"unterminated",
    "(module m (fn f [] -> i32 (if 1 2)) \"unterminated",
    "(module m (fn f [] -> str \"bad \\q\"))",
    "(module m (fn f [] -> str \"line\nbreak\"))",
    "(module m (fn f [] -> i32\u{000C}0))",
    "(module m (fn f [] -> i32\n  (+ é\u{00A0}1)))",
    "(module m\n  (fn f [] -> i32\n    (let s:str \"é日\")\n    (foo s)))",
];

#[test]
fn parse_errors_match_word_for_word() {
    let front = front::load();
    for src in BAD_PARSES {
        let theirs = match Parser::parse(src) {
            Ok(_) => panic!("Rust accepts {src:?}"),
            Err(e) => e,
        };
        let ours = match front.run("parse_print", src) {
            Ok(text) => panic!("parser.aipl accepts {src:?}:\n{text}\nRust says {theirs}"),
            Err(e) => e,
        };
        assert_eq!(ours, theirs, "{src:?}");
    }
}

/// CK5: the AIPL resolver's flat output (constants, enums, unions, imported
/// names) through expand.aipl and parser.aipl is the program the Rust
/// resolver makes of the same text.
#[test]
fn constants_expand_like_rust() {
    let front = front::load();
    let mut checked = 0;
    for rel in [
        "tests/aipl/consts_enums.aipl",
        "tests/aipl/sum_types.aipl",
        "examples/word_count.aipl",
        "aipl_src/consts.aipl",
        "aipl_src/codegen.aipl",
        "aipl_src/native/x64.aipl",
        "benchmarks/pidigits/pidigits.aipl",
    ] {
        let path = root().join(rel);
        let flat = aipl_core::selfhost::resolve_with_aipl(&path).unwrap_or_else(|e| panic!("{rel}: {e}"));
        let theirs = aipl_core::printer::print_module(&Resolver::resolve_source(&flat, &path).unwrap());
        let ours = front.run("expand_print", &flat).unwrap_or_else(|e| panic!("{rel}: {e}"));
        let ours = aipl_core::printer::print_module(&Parser::parse(&ours).unwrap_or_else(|e| panic!("{rel}: {e}\n{ours}")));
        assert!(ours == theirs, "{rel}: expanded programs differ");
        checked += 1;
    }
    assert_eq!(checked, 7);
}

/// CK5: constant and enum errors, word for word (Rust prefixes the file).
#[test]
fn constant_errors_match_word_for_word() {
    let front = front::load();
    let cases = [
        "(const NN:i32 1) (const NN:i32 2)",
        "(const NN:i32)",
        "(const NN i32 1)",
        "(const 5:i32 1)",
        "(const nn:i32 1)",
        "(const N:i32 1)",
        "(const N_a:i32 1)",
        "(const lib.MAX_X:i32 1) (const lib.max:i32 1)",
        "(const NN:i32 true)",
        "(const NN:i32 3000000000)",
        "(const NN:i64 5)",
        "(const NN:f64 5)",
        "(const NN:bool 1)",
        "(const NN:str 1)",
        "(const NN:f32 1.0)",
        "(const NN:(arr i32) 1)",
        "(const NN:i32 (+ 1 2))",
        "(const NN:i32 1) (fn f [NN:i32] -> i32 0)",
        "(const NN:i32 1) (fn f [] -> i32 (let NN:i32 2) 0)",
        "(const NN:i32 1) (struct S [NN:i32])",
        "(enum C [a b]) (fn f [] -> C C.z)",
        "(enum C [a]) (fn f [] -> i32 (match (enum.ord C.q) (else 1)))",
        "(enum C [a]) (union U [(v x:C)]) (fn f [] -> U (make U.v C.nope))",
    ];
    for body in cases {
        let flat = format!("(module m\n  {body}\n  (fn main [] -> i32 0))");
        let theirs = Resolver::resolve_source(&flat, Path::new("m.aipl")).err().unwrap_or_else(|| panic!("Rust accepts {body}"));
        let theirs = theirs.strip_prefix("m.aipl: ").unwrap_or(&theirs).to_string();
        let ours = front.run("expand_print", &flat).err().unwrap_or_else(|| panic!("expand.aipl accepts {body}"));
        assert_eq!(ours, theirs, "{body}");
    }
}

/// Rust's verdict on a flat program: Ok, or the first error without the
/// file prefix the resolver adds to parse and constant errors.
fn rust_verdict(flat: &str) -> Result<(), String> {
    // both name the file; check_text is given text, not a file
    let strip = |e: String| e.split('\n').map(|l| l.strip_prefix("m.aipl: ").unwrap_or(l)).collect::<Vec<_>>().join("\n");
    let m = Resolver::resolve_source(flat, Path::new("m.aipl")).map_err(strip)?;
    TypeChecker::new().check_module(&m).map_err(strip)
}

fn aipl_verdict(front: &front::Front, flat: &str) -> Result<(), String> {
    front.run("check_text", flat).map(|_| ())
}

/// CK6-CK10 acceptance: every repository program, as the Rust printer
/// prints it, is accepted by checker.aipl.
#[test]
fn checker_accepts_every_repository_program() {
    let front = front::load();
    let mut checked = 0;
    for f in aipl_files(&["aipl_src", "aipl_src/std", "aipl_src/native", "examples", "tests/aipl", "benchmarks"]) {
        let Ok(m) = Resolver::resolve(&f) else { continue };
        if TypeChecker::new().check_module(&m).is_err() {
            continue;
        }
        let text = aipl_core::printer::print_module(&m);
        if let Err(e) = aipl_verdict(&front, &text) {
            panic!("{}: checker.aipl rejects: {e}", f.display());
        }
        checked += 1;
    }
    assert!(checked > 40, "{checked}");
}

const HEADER: &str = "(struct P [x:i32 f:f64 n:(ptr P)]) (enum E [a b]) (union U [(v x:i32) (w)]) (fn w [x:i32] -> i32 x) (fn nothing [] -> void (block))";

/// CK10: every operator over 0-3 operands of each kind of value: both
/// checkers give the same verdict and, for a rejection, the same message.
#[test]
fn checkers_agree_on_every_operator_and_operand() {
    let front = front::load();
    let ops = "+ - * / % ^ shl shr shru bitand bitor mem.load8 mem.load32 mem.load64 mem.store8 mem.store32 mem.store64 mem.alloc mem.grow str.len str.ptr atomic.add atomic.cas atomic.lock atomic.unlock eq neq lt lte ltu lteu gtu gteu gt gte checked.add checked.sub checked.mul and or not sys.print sys.time sys.monotonic sys.random sys.exit fs.open fs.read fs.write fs.close fs.delete args.sizes args.get env.sizes env.get thread.spawn thread.join divu remu i64.extend_s i64.extend_u i32.wrap f64.convert_i64_s f64.sqrt i64.trunc_f64_s f64.reinterpret_i64 i64.reinterpret_f64";
    let pool = ["2000", "2000i64", "1.5", "\"s\"", "true", "(ref w)", "(new P)", "(arr.new i32 1)", "E.a", "(make U.w)", "0", "70", "6"];
    let mut programs = Vec::new();
    for op in ops.split_whitespace() {
        programs.push(format!("({op})"));
        for a in pool {
            programs.push(format!("({op} {a})"));
            for b in pool {
                programs.push(format!("({op} {a} {b})"));
            }
            for (b, c) in [("2000", "2000"), ("1.5", "1.5"), ("2000", "true"), ("(ref w)", "2000")] {
                programs.push(format!("({op} {a} {b} {c})"));
            }
        }
    }
    let (mut agreed, mut rejected) = (0, 0);
    for body in &programs {
        // as a value and as a statement
        for flat in [format!("(module m {HEADER} (fn main [] -> i32 {body}))"), format!("(module m {HEADER} (fn main [] -> i32 {body} 0))")] {
            let theirs = rust_verdict(&flat);
            let ours = aipl_verdict(&front, &flat);
            assert_eq!(ours, theirs, "{flat}");
            agreed += 1;
            rejected += theirs.is_err() as usize;
        }
    }
    assert!(agreed > 20000 && rejected > 15000, "{agreed} agreed, {rejected} rejections");
}

/// CK10: hand-written programs reaching each checker message in every
/// form, and definitions; full modules or bodies of `main` (wrapped with
/// HEADER). Both checkers must give the same verdict and message.
const FORMS: &[&str] = &[
    // registration
    "M:(module m (enum E [a]) (enum E [b]) (fn main [] -> i32 0))",
    "M:(module m (struct E [x:i32]) (enum E [a]) (fn main [] -> i32 0))",
    "M:(module m (enum E []) (fn main [] -> i32 0))",
    "M:(module m (enum E [A]) (fn main [] -> i32 0))",
    "M:(module m (enum E [a a]) (fn main [] -> i32 0))",
    "M:(module m (enum E [a (b 0)]) (fn main [] -> i32 0))",
    "M:(module m (enum E [_x (b -5) c]) (fn main [] -> i32 (enum.ord E.c)))",
    "M:(module m (struct S [x:i32]) (struct S [y:i32]) (fn main [] -> i32 0))",
    "M:(module m (struct S [x:i32 x:i64]) (fn main [] -> i32 0))",
    "M:(module m (struct S [x:void]) (fn main [] -> i32 0))",
    "M:(module m (struct S [x:(result i32 i32)]) (fn main [] -> i32 0))",
    "M:(module m (struct S [p:(ptr Q)]) (fn main [] -> i32 0))",
    "M:(module m (struct S [p:Q]) (fn main [] -> i32 0))",
    "M:(module m (struct S [p:(arr void)]) (fn main [] -> i32 0))",
    "M:(module m (struct S [p:(fn [Q] -> i32)]) (fn main [] -> i32 0))",
    "M:(module m (struct S [p:(arr (ptr Q))]) (fn main [] -> i32 0))",
    "M:(module m (union U [(a)]) (union U [(b)]) (fn main [] -> i32 0))",
    "M:(module m (struct U [x:i32]) (union U [(a)]) (fn main [] -> i32 0))",
    "M:(module m (enum U [z]) (union U [(a)]) (fn main [] -> i32 0))",
    "M:(module m (union U []) (fn main [] -> i32 0))",
    "M:(module m (union U [(A)]) (fn main [] -> i32 0))",
    "M:(module m (union U [(a) (a)]) (fn main [] -> i32 0))",
    "M:(module m (union U [(a x:i32 x:i32)]) (fn main [] -> i32 0))",
    "M:(module m (union U [(a x:void)]) (fn main [] -> i32 0))",
    "M:(module m (union U [(a p:(ptr Q))]) (fn main [] -> i32 0))",
    "M:(module m (union U [(a p:Nope)]) (fn main [] -> i32 0))",
    "M:(module m (union U [(a p:U)]) (struct S [u:U]) (fn main [] -> i32 0))",
    // functions and contracts
    "M:(module m (fn f [] -> i32 0) (fn f [x:i32] -> i32 x) (fn main [] -> i32 0))",
    "M:(module m (fn f [x:Nope] -> i32 0) (fn main [] -> i32 0))",
    "M:(module m (fn f [x:(ptr Nope)] -> i32 0) (fn main [] -> i32 0))",
    "M:(module m (fn f [] -> Nope 0) (fn main [] -> i32 0))",
    "M:(module m (struct S [x:i32]) (fn f [x:S] -> i32 0) (fn main [] -> i32 0))",
    "M:(module m (union U [(a)]) (fn f [x:i32] -> i32 (enum.ord (enum.cast U x))) (fn main [] -> i32 0))",
    "M:(module m (fn f [x:i32] -> i32 (req x) x) (fn main [] -> i32 0))",
    "M:(module m (fn f [x:i32] -> i32 (inv 1) x) (fn main [] -> i32 0))",
    "M:(module m (fn f [x:i32] -> i32 (ens res) x) (fn main [] -> i32 0))",
    "M:(module m (fn f [x:i32] -> i32 (ens (gt res x)) (req (return true)) x) (fn main [] -> i32 0))",
    "M:(module m (fn f [x:i32] -> i32 (req (gt y 0)) x) (fn main [] -> i32 0))",
    "M:(module m (fn f [] -> i32 true) (fn main [] -> i32 0))",
    "M:(module m (fn f [] -> i32) (fn main [] -> i32 0))",
    "M:(module m (fn f [] -> i32 (let x:i32 1)) (fn main [] -> i32 0))",
    "M:(module m (fn f [] -> i32 (return 1)) (fn g [] -> i64 (return 1)) (fn main [] -> i32 0))",
    "M:(module m (fn f [] -> void (return 1)) (fn main [] -> i32 0))",
    "M:(module m (fn f [] -> void (return)) (fn g [] -> i32 (return)) (fn main [] -> i32 0))",
    // variables, let, set!, if, loops
    "y",
    "(let x:i32 1) (let x:i32 2) x",
    "(let x:i32 1i64) x",
    "(let x:Nope 1) 0",
    "(let x:(ptr Q) 1) 0",
    "(set! zz 1) 0",
    "(let x:i32 1) (set! x true) x",
    "(if 1 2 3)",
    "(if true 2 3i64)",
    "(if true (block) 3)",
    "(if true (let q:i32 1) (block)) q",
    "(if true (let q:i32 1) (let q:i32 2)) 0",
    "(match E.a (E.a (let q:i32 1)) (E.b (let q:i32 2))) 0",
    "(block (let q:i32 1)) q",
    "(loop i 0 2i64 1) 0",
    "(loop i 0 2 1.5) 0",
    "(let i:i32 0) (loop i 0 2 1) 0",
    "(loop i 0 2 1 (let i2:i64 1i64)) (let i2:i32 0) i2",
    "(while 1) 0",
    "(while true (break)) (break) 0",
    "(continue) 0",
    "(loop i 0 1 1 (while true (break)) (continue)) 0",
    "(let x:f64 1.5) (block (let y:i32 1)) (block (let y:f64 1.5)) 0",
    "(let x:i32 0) (loop x 0 1 1) 0",
    // calls and references
    "(call nope)",
    "(call w)",
    "(call w 1 2)",
    "(call w true)",
    "(call nothing)",
    "(let r:(fn [i32] -> i32) (ref w)) (call_ref (fn [i32] -> i32) r 1)",
    "(ref nope)",
    "(call_ref i32 (ref w) 1)",
    "(call_ref (fn [i32] -> bool) (ref w) 1)",
    "(call_ref (fn [i32] -> i32) (ref w))",
    "(call_ref (fn [i32] -> i32) (ref w) true)",
    "(call_ref (fn [Nope] -> i32) (ref w) 1)",
    // results
    "(let r:(result i32 i32) (ok 1)) 0",
    "(let r:(result bool str) (ok:str true)) 0",
    "(let r:(result i32 i64) (err 1i64)) 0",
    "(let r:(result i32 i64) (err:bool 1i64)) 0",
    "(let r:(result i32 i32) (ok:Nope 1)) 0",
    "(match_result 5 (ok v v) (err e e))",
    "(let v:i32 0) (match_result (ok 1) (ok v v) (err e e))",
    "(let e:i32 0) (match_result (ok 1) (ok v v) (err e e))",
    "(match_result (ok 1) (ok v v) (err e true))",
    "(match_result (ok 1) (ok v (block)) (err e (block))) 0",
    "(match_result (ok 1) (ok v v) (err e (block)))",
    "(let v:bool true) (match_result (ok:i32 1) (ok v2 v2) (err v e3)) ",
    // structs
    "(let p:(ptr Q) (new Q)) 0",
    "(get (new Q) Q.x)",
    "(get 5 P.x)",
    "(get (new P) P.zz)",
    "(i32.wrap (i64.trunc_f64_s (get (new P) P.f)))",
    "(put (new Q) Q.x 1) 0",
    "(put 5 P.x 1) 0",
    "(put (new P) P.zz 1) 0",
    "(put (new P) P.x 1.5) 0",
    "(put (new P) P.n (ptr.null P)) 0",
    "(sizeof Q)",
    "(sizeof P)",
    "(+ (sizeof i64) (+ (sizeof (ptr P)) (sizeof (arr f64))))",
    "(sizeof void)",
    "(sizeof (result i32 i32))",
    "(sizeof (ptr Nope))",
    "(sizeof bool)",
    "(match_result (ok 1i64) (ok v 0) (err e 1))",
    "(match_result (err:i32 2.5) (ok v 0) (err e 1))",
    // arrays
    "(arr.len (arr.new i32 1i64))",
    "(arr.len (arr.new void 1))",
    "(arr.len (arr.new Nope 1))",
    "(arr.get i32 (arr.new i64 1) 0)",
    "(arr.get i32 5 0)",
    "(arr.get i32 (arr.new i32 1) true)",
    "(arr.get Nope (arr.new i32 1) 0)",
    "(arr.set i32 (arr.new i32 1) 0 1.5) 0",
    "(arr.set i32 (arr.new f64 1) 0 1) 0",
    "(arr.set i32 (arr.new i32 1) 1.5 1) 0",
    "(arr.len 5)",
    "(arr.len (arr.null i32))",
    "(arr.len (arr.null void))",
    "(let p:(ptr Q) (ptr.null Q)) 0",
    "(let a:(arr i32) (arr.cast i32 1.5)) 0",
    "(let p:(ptr P) (ptr.cast P true)) 0",
    "(let p:(ptr P) (ptr.cast Q 4096)) 0",
    "(let a:(arr i32) (arr.cast void 4096)) 0",
    "(ptr.addr 5)",
    "(arr.addr (new P))",
    "(enum.ord 5)",
    "(enum.ord (enum.cast E 1i64))",
    "(enum.ord (enum.cast Nope 1))",
    "(+ (ptr.addr (new P)) (+ (arr.addr (arr.new i32 1)) (enum.ord E.b)))",
    // enums and unions
    "(match 5 (else 0))",
    "(match E.a (E.a 1))",
    "(match E.a (E.a 1) (E.b 2) (else 3))",
    "(match E.a (E.a 1) (E.a 2) (else 3))",
    "(match E.a (E.zz 1) (else 3))",
    "(match E.a (U.v 1) (else 3))",
    "(match E.a (Ea 1) (else 3))",
    "(match E.a (E.a [x] 1) (else 3))",
    "(match E.a (E.a 1) (else true))",
    "(match (make U.w) (U.v [x] x) (U.w 2))",
    "(match (make U.w) (U.v x) (U.w 2))",
    "(match (make U.w) (U.v [x y] x) (U.w 2))",
    "(match (make U.w) (U.v [_] 1) (U.w 2))",
    "(let x:i32 0) (match (make U.w) (U.v [x] x) (U.w 2))",
    "(let x:bool true) (match (make U.w) (U.v [y] y) (U.w (block (let y:bool true) 2)))",
    "(match (make U.w) (U.v [y] (block)) (U.w (block))) 0",
    "(match (make U.w) (U.v [y] y) (else 2))",
    "(make Nope.v)",
    "(make U.zz)",
    "(make U.v)",
    "(make U.v 1 2)",
    "(make U.v true)",
    "(match (make U.v 1) (U.v [x] x) (U.w 0))",
    "(match (make U.w) (U.w 1) (else (+ 1 true)))",
    // literal addresses
    "(mem.load32 -1)", "(mem.store32 0 1) 0", "(mem.load32 0)", "(mem.store8 3 1) 0", "(mem.store32 4 1) 0",
    "(mem.store32 5 1) 0", "(mem.load8 63)", "(mem.store32 64 1) 0", "(mem.load32 1023)", "(mem.load32 1024)",
    "(atomic.add 0 1)", "(atomic.lock 2) 0", "(if (atomic.cas 60 0 1) 1 0)", "(atomic.unlock 512) 0", "(mem.load64 8)",
];

#[test]
fn checkers_agree_on_every_form() {
    let front = front::load();
    let mut rejected = 0;
    for case in FORMS {
        let flat = match case.strip_prefix("M:") {
            Some(m) => m.to_string(),
            None => format!("(module m {HEADER}\n  (fn main [] -> i32\n    {case}))"),
        };
        let theirs = rust_verdict(&flat);
        let ours = aipl_verdict(&front, &flat);
        assert_eq!(ours, theirs, "{flat}");
        rejected += theirs.is_err() as usize;
    }
    assert!(rejected >= 140, "{rejected} of {} rejected", FORMS.len());
}

/// Definitions stop at the first error; after them every function is
/// checked, and each failing one gives its first error, one per line in
/// source order, the same in both checkers.
#[test]
fn checkers_report_each_failing_function() {
    let front = front::load();
    let cases: [(&str, &str); 3] = [
        // two failing functions around a good one
        (
            "(module m\n  (fn a [] -> i32 (+ 1 true))\n  (fn good [] -> i32 1)\n  (fn b [] -> i32 (set! zz 1) 0)\n  (fn main [] -> i32 0))",
            "2:19: Type mismatch in binary op: i32 vs bool\n4:19: Undefined variable 'zz' in set!",
        ),
        // a function failing inside a loop leaves no loop behind for the next
        (
            "(module m\n  (fn a [] -> i32 (while true (+ 1 true)) 0)\n  (fn b [] -> i32 (break) 0)\n  (fn main [] -> i32 0))",
            "2:31: Type mismatch in binary op: i32 vs bool\n3:19: break is only allowed inside a while or loop body",
        ),
        // a definition's error is the only one, though later functions fail
        (
            "(module m\n  (struct S [x:(ptr Nope)])\n  (fn a [] -> i32 (+ 1 true))\n  (fn b [] -> i32 zz)\n  (fn main [] -> i32 0))",
            "",
        ),
    ];
    for (flat, expected) in cases {
        let theirs = rust_verdict(flat).unwrap_err();
        let ours = aipl_verdict(&front, flat).unwrap_err();
        assert_eq!(ours, theirs, "{flat}");
        if !expected.is_empty() {
            assert_eq!(theirs, expected, "{flat}");
        }
    }
    assert_eq!(rust_verdict(cases[2].0).unwrap_err().lines().count(), 1);
}
