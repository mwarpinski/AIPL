//! Every ```lisp block in PROMPT_GUIDE_FOR_AIS.md and README.md is a complete
//! module that, run as a program, must print what the document states and
//! exit with its status (AIPL_SPEC.md 6.5), in the VM (`aipl eval`) and
//! compiled under wasmtime (`_start`, with WASI and a scratch directory,
//! since some examples do I/O). AIPL_SPEC.md's blocks are checked too: complete modules
//! run like the others (with the exceptions in SPEC_SPECIAL), and fragments
//! (a few definitions) are wrapped in a module and type-checked, the one
//! deliberately invalid fragment failing with its documented message. A doc
//! example that stops working fails here instead of misleading the next agent
//! that copies it.

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::resolver::Resolver;
use aipl_core::vm::VM;
use std::path::Path;
use wasmtime::{Engine, Linker, Module as WasmModule, Store, TypedFunc};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::{FsPerms, WasiCtxBuilder};

/// Module name -> the exit status and output of the program, as stated in
/// the docs. Modules without a `main` (library-style examples) are checked
/// and compiled only.
const EXPECTED: &[(&str, i32, &str)] = &[
    ("search", 0, "15 is at 5, 4 is at -1\n"),
    ("list_demo", 0, "sum: 55\n"),
    ("parse_demo", 0, "1234 parses to 1234; 12x to -1\n"),
    ("io_demo", 0, "wrote 15 bytes to note.txt\n"),
    ("std_demo", 0, "words: 3\nlines: 2\n"),
    ("generics_demo", 0, "smallest x: 10, swapped second: 7\n"),
    ("expr_demo", 0, "-20; a num is a leaf: true\n"),
    // AIPL_SPEC.md
    ("points", 0, "33\n"),
    ("gcd_demo", 0, "21\n"),
    ("stack_demo", 0, "42\n"),
    ("hello", 0, "hello, aipl\n"),
    ("file_demo", 0, "read back: true\n"),
    ("word_count", 1, ""), // no input.txt in the scratch directory: main's err, on standard error
    ("shapes", 0, "4 7 12 5\n"),
    ("formatting", 0, "Ada ran 3 tests, mean 2.72\n    3|1.5|{} row 1: 001;row 2: 004;\n"),
    ("parse_two", 0, "42\n"),
    ("cleanup", 0, "3\n"),
    ("geom", 0, "total 13, turns 2\n"),
    ("tokens_demo", 0, "sum 1, Tok.minus is 10\n"),
    ("unsafe_demo", 0, "42\n"),
    ("option_demo", 0, "2 nodes, 9 at 3\n"),
    ("interface_demo", 0, "20, 1 key, largest 7\n"),
    ("context_demo", 0, "1 2 0\n"),
    ("values_demo", 0, "y = 5.5\n"),
    // docs/USING_AIPL.md (empty input here; the next test gives them the input the page states)
    ("use_lines", 0, ""),
    ("word_tally", 0, ""),
    ("bits_demo", 0, "8 14 6 16 -4 15 -3 -1\n"),
    // docs/TOUR.md
    ("tour_hello", 0, "7 squared is 49\n3 squared plus 4 squared is 25\n"),
    ("tour_types", 0, "the average of 40 and 3 is 21.5\n"),
    ("tour_control", 0, "evens up to 10 add to 30; the sign of -5 is -1\n"),
    ("tour_structs", 0, "total: 33\n"),
    ("tour_values", 0, "shifted: (7, 20); second y: 10\n"),
    ("tour_results", 0, "areas: 9.0, digit: 7\n"),
    ("tour_contracts", 0, "isqrt 99 = 9, isqrt 100 = 10\n"),
    ("tour_generics", 0, "first 10, 3 elements\n"),
];

/// README.md and PROMPT_GUIDE_FOR_AIS.md examples whose `main` must fail, as
/// the document shows it failing: the VM's message contains the text, and
/// compiled code traps.
const FAILING: &[(&str, &str)] = &[("gcd", "Pre-condition failed in 'gcd' at 6:10: (req (and (gt a 0) (gt b 0))) with a = 10, b = 0")];

/// AIPL_SPEC.md modules that cannot simply run in both backends, and why.
const SPEC_SPECIAL: &[(&str, &str)] = &[
    ("contracts_demo", "main fails its precondition in the VM (the example's point); wasm has no contracts and traps"),
    ("counter_demo", "a threaded module: needs a wasi-threads host, covered by tests/test_threads.rs; runs in the VM"),
    ("test_suite", "abridged; imports toolchain modules and concatenates strings (VM-only)"),
    ("util", "two files in one block (util.aipl and main.aipl)"),
];

/// AIPL_SPEC.md fragments that must fail to check, with the documented message.
const SPEC_INVALID: &[(&str, &str)] = &[("(fn bad ", "If branch type mismatch: then is void, else is i32")];

fn lisp_blocks(doc: &str) -> Vec<String> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(doc)).unwrap();
    text.split("```lisp\n").skip(1).map(|b| b.split("```").next().unwrap().to_string()).collect()
}

/// The compiled program run under wasmtime: its exit status and output.
fn run_wasm(wasm: &[u8], dir: &Path) -> (i32, String) {
    let engine = Engine::default();
    let module = WasmModule::new(&engine, wasm).unwrap();
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
    let stdout = wasmtime_wasi::p2::pipe::MemoryOutputPipe::new(1 << 20);
    let ctx = WasiCtxBuilder::new().stdout(stdout.clone()).preopened_dir(dir, ".", FsPerms::ReadWrite).unwrap().build_p1();
    let mut store = Store::new(&engine, ctx);
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let start: TypedFunc<(), ()> = instance.get_typed_func(&mut store, "_start").unwrap();
    let status = match start.call(&mut store, ()) {
        Ok(()) => 0,
        Err(e) => e.downcast_ref::<wasmtime_wasi::I32Exit>().map(|x| x.0).unwrap_or_else(|| panic!("{e:?}")),
    };
    (status, String::from_utf8(stdout.contents().to_vec()).unwrap())
}

/// The program run by `aipl eval` in `dir`: its exit status and output.
fn run_vm(file: &Path, dir: &Path) -> (i32, String) {
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_aipl")).arg("eval").arg(file).current_dir(dir).output().unwrap();
    let out: String = String::from_utf8_lossy(&o.stdout).split_inclusive('\n').filter(|l| !l.starts_with("[AIPL")).collect();
    (o.status.code().unwrap_or(-1), out)
}

/// Whether the module's main traps when compiled.
fn wasm_traps(wasm: &[u8], dir: &Path) -> bool {
    let engine = Engine::default();
    let module = WasmModule::new(&engine, wasm).unwrap();
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
    let ctx = WasiCtxBuilder::new().preopened_dir(dir, ".", FsPerms::ReadWrite).unwrap().build_p1();
    let mut store = Store::new(&engine, ctx);
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let main: TypedFunc<(), i32> = instance.get_typed_func(&mut store, "main").unwrap();
    main.call(&mut store, ()).is_err()
}

#[test]
fn doc_examples_run_in_both_backends() {
    let scratch = std::env::temp_dir().join(format!("aipl_doc_examples_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    // The VM resolves file paths against the cwd; this test binary has one test, so chdir is safe.
    let prev = std::env::current_dir().unwrap();
    std::env::set_current_dir(&scratch).unwrap();

    let mut seen = Vec::new();
    for doc in ["PROMPT_GUIDE_FOR_AIS.md", "README.md", "docs/TOUR.md", "docs/USING_AIPL.md"] {
        for src in lisp_blocks(doc) {
            // Resolved from a file, so examples may import the standard library.
            let file = scratch.join("example.aipl");
            std::fs::write(&file, &src).unwrap();
            let module = Resolver::resolve(&file).unwrap_or_else(|e| panic!("{doc}: {e}\n{src}"));
            TypeChecker::new().check_module(&module).unwrap_or_else(|e| panic!("{doc}: {e}\n{src}"));
            let wasm = WasmCompiler::compile(&module).unwrap_or_else(|e| panic!("{doc}: {e}\n{src}"));
            if let Some(&(_, msg)) = FAILING.iter().find(|(n, _)| *n == module.name) {
                let mut vm = VM::new();
                vm.load_module(module.clone());
                let e = vm.invoke("main", vec![]).unwrap_err();
                assert!(e.contains(msg), "{doc}: module {}: expected `{msg}`, got `{e}`", module.name);
                assert!(wasm_traps(&wasm, &scratch), "{doc}: module {} must trap when compiled", module.name);
                seen.push(module.name.clone());
                continue;
            }
            let Some(&(_, status, out)) = EXPECTED.iter().find(|(n, _, _)| *n == module.name) else {
                assert!(!module.functions.iter().any(|f| f.name == "main"), "{doc}: add '{}' to EXPECTED", module.name);
                continue;
            };
            assert_eq!(run_vm(&file, &scratch), (status, out.to_string()), "{doc}: VM, module {}", module.name);
            assert_eq!(run_wasm(&wasm, &scratch), (status, out.to_string()), "{doc}: wasm, module {}", module.name);
            seen.push(module.name.clone());
        }
    }
    let mut fragments = 0;
    for src in lisp_blocks("AIPL_SPEC.md") {
        let code: String = src.lines().filter(|l| !l.trim_start().starts_with(";;")).collect::<Vec<_>>().join("\n");
        let file = scratch.join("example.aipl");
        if !code.trim_start().starts_with("(module") {
            // memory.aipl's runner calls functions defined elsewhere in that module
            if code.contains("aipl_heap_alloc") {
                continue;
            }
            // a fragment that allocates names its allocator, from std/alloc
            let imports = if code.contains("alloc.") { "  (import alloc)\n" } else { "" };
            std::fs::write(&file, format!("(module fragment\n{imports}{code}\n)")).unwrap();
            let checked = Resolver::resolve(&file).and_then(|m| TypeChecker::new().check_module(&m).map(|_| m));
            match SPEC_INVALID.iter().find(|(head, _)| code.trim_start().starts_with(head)) {
                Some((_, msg)) => {
                    let e = checked.err().unwrap_or_else(|| panic!("AIPL_SPEC.md: this fragment is documented as invalid but checks:\n{src}"));
                    assert!(e.contains(msg), "AIPL_SPEC.md: expected `{msg}`, got `{e}`");
                }
                None => {
                    checked.unwrap_or_else(|e| panic!("AIPL_SPEC.md fragment: {e}\n{src}"));
                }
            }
            fragments += 1;
            continue;
        }
        let name = code.trim_start()["(module".len()..].split_whitespace().next().unwrap().to_string();
        if let Some((_, why)) = SPEC_SPECIAL.iter().find(|(n, _)| *n == name) {
            match name.as_str() {
                "contracts_demo" => {
                    std::fs::write(&file, &code).unwrap();
                    let module = Resolver::resolve(&file).unwrap();
                    TypeChecker::new().check_module(&module).unwrap();
                    let mut vm = VM::new();
                    vm.load_module(module);
                    let e = vm.invoke("main", vec![]).unwrap_err();
                    assert!(e.starts_with("Pre-condition failed in 'safe_div' at 3:10: (req (neq den 0)) with num = 10, den = 0"), "{e}");
                }
                "counter_demo" => {
                    std::fs::write(&file, &code).unwrap();
                    let module = Resolver::resolve(&file).unwrap();
                    TypeChecker::new().check_module(&module).unwrap();
                    WasmCompiler::compile(&module).unwrap();
                    assert_eq!(run_vm(&file, &scratch), (0, "2000\n".to_string()), "counter_demo");
                }
                _ => eprintln!("AIPL_SPEC.md: not run: {name}: {why}"),
            }
            seen.push(name);
            continue;
        }
        std::fs::write(&file, &code).unwrap();
        let module = Resolver::resolve(&file).unwrap_or_else(|e| panic!("AIPL_SPEC.md: {e}\n{src}"));
        TypeChecker::new().check_module(&module).unwrap_or_else(|e| panic!("AIPL_SPEC.md: {e}\n{src}"));
        let wasm = WasmCompiler::compile(&module).unwrap_or_else(|e| panic!("AIPL_SPEC.md: {e}\n{src}"));
        if let Some(&(_, status, out)) = EXPECTED.iter().find(|(n, _, _)| *n == module.name) {
            assert_eq!(run_vm(&file, &scratch), (status, out.to_string()), "AIPL_SPEC.md: VM, module {}", module.name);
            assert_eq!(run_wasm(&wasm, &scratch), (status, out.to_string()), "AIPL_SPEC.md: wasm, module {}", module.name);
        } else {
            assert!(!module.functions.iter().any(|f| f.name == "main"), "AIPL_SPEC.md: add '{}' to EXPECTED", module.name);
        }
        seen.push(module.name.clone());
    }
    assert!(fragments >= 10, "expected the spec's fragments to be checked, got {fragments}");
    for (name, _) in SPEC_SPECIAL {
        assert!(seen.iter().any(|s| s == name), "AIPL_SPEC.md example '{name}' not found");
    }

    std::env::set_current_dir(prev).unwrap();
    let _ = std::fs::remove_dir_all(&scratch);
    for name in EXPECTED.iter().map(|(n, _, _)| n).chain(FAILING.iter().map(|(n, _)| n)) {
        assert!(seen.iter().any(|s| s == name), "documented example '{name}' not found in the docs");
    }
}

/// `loop` is retired (CANONICAL_PLAN.md step 3): no program in the
/// repository, and no comment in one (what `aipl doc` prints), writes it.
/// The internal form is `counted_loop`; `(loop n:...)` in ast.aipl and
/// `(loop 12)` in codegen.aipl's tables are a variant and a table entry.
#[test]
fn no_program_or_doc_comment_writes_loop() {
    fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "aipl") {
                out.push(p);
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for d in ["aipl_src", "examples", "benchmarks", "tests/aipl"] {
        walk(&root.join(d), &mut files);
    }
    let mut found = Vec::new();
    for f in &files {
        for (k, line) in std::fs::read_to_string(f).unwrap().lines().enumerate() {
            if let Some(at) = line.find("(loop ") {
                let rest = &line[at + 6..];
                let var: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
                let is_form = !var.is_empty() && !var.starts_with(|c: char| c.is_ascii_digit()) && rest[var.len()..].starts_with(' ');
                if is_form {
                    found.push(format!("{}:{}: {}", f.display(), k + 1, line.trim()));
                }
            }
        }
    }
    assert!(files.len() > 100, "{} files", files.len());
    assert!(found.is_empty(), "loop is retired; these write it:\n{}", found.join("\n"));
}

/// docs/USING_AIPL.md says what its two input programs print for a given
/// input, what `aipl test` reports for its test example, and what three
/// failures look like: all of it is run here, through the commands the page
/// teaches (compile, aipl-run, test, verify).
#[test]
fn the_usage_page_says_what_the_commands_do() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let aipl = env!("CARGO_BIN_EXE_aipl");
    let runner = env!("CARGO_BIN_EXE_aipl-run");
    let dir = std::env::temp_dir().join(format!("aipl_using_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let block = |name: &str| lisp_blocks("docs/USING_AIPL.md").into_iter().find(|b| b.starts_with(&format!("(module {name}"))).unwrap_or_else(|| panic!("no block for {name}"));
    let run = |name: &str, input: &str| {
        let (src, wasm) = (dir.join(format!("{name}.aipl")), dir.join(format!("{name}.wasm")));
        std::fs::write(&src, block(name)).unwrap();
        let o = Command::new(aipl).arg("compile").arg(&src).arg("-o").arg(&wasm).output().unwrap();
        assert!(o.status.success(), "{name}: {}", String::from_utf8_lossy(&o.stderr));
        let mut child = Command::new(runner).arg(&wasm).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        let o = child.wait_with_output().unwrap();
        assert_eq!(o.status.code(), Some(0), "{name}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap()
    };
    assert_eq!(run("use_lines", "hello\nworld\n"), "1: HELLO\n2: WORLD\n");
    assert_eq!(run("word_tally", "the cat The dog\nthe end\n"), "cat 1\ndog 1\nend 1\nthe 3\n");
    // the test example passes, as the page shows
    let src = dir.join("doubling.aipl");
    std::fs::write(&src, block("doubling")).unwrap();
    let o = Command::new(aipl).arg("test").arg(&src).output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success() && out.contains("ok    doubles a number") && out.contains("2 tests: 2 passed, 0 failed"), "{out}");
    // a failing test says where and why
    std::fs::write(&src, "(module t\n  (fn double [x:i32] -> i32 (* x 2))\n  (test \"fails\" (let n:i32 (call double 3)) (expect (eq n 7))))\n").unwrap();
    let o = Command::new(aipl).arg("test").arg(&src).output().unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stdout).contains("(expect (eq n 7)): left 6, right 7"));
    // the two compile-time messages and the trap on the page
    let verify = |name: &str, text: &str| {
        let f = dir.join(name);
        std::fs::write(&f, text).unwrap();
        let o = Command::new(aipl).arg("verify").arg(name).current_dir(&dir).output().unwrap();
        assert_eq!(o.status.code(), Some(1));
        String::from_utf8_lossy(&o.stderr).to_string() + &String::from_utf8_lossy(&o.stdout)
    };
    let bad = verify("bad.aipl", "(module bad\n  (fn add [a:i32 b:i32] -> i32 (+ a b))\n  (fn main [] -> i32\n    (let x:i32 (call add 1 true))\n    (let y:i64 x)\n    (println \"{y}\")\n    0))\n");
    assert!(bad.contains("Error: bad.aipl: 4:16: Arg 1 of 'add' expects i32, got bool"), "{bad}");
    let syn = verify("syn.aipl", "(module syn (fn main [] -> i32 (if true 1)))");
    assert!(syn.contains("Error: syn.aipl: 1:42: Unexpected token parsing expression: ')'; if needs both branches: (if c then else); for one branch, write (when c then)"), "{syn}");
    let trap = dir.join("trap.aipl");
    std::fs::write(&trap, "(module trap\n  (import alloc)\n  (fn get [a:(arr i32) i:i32] -> i32 (arr.get i32 a i))\n  (fn main [] -> i32\n    (let a:(arr i32) (arr.new i32 (call alloc.default) 3))\n    (println \"{(call get a 5)}\")\n    0))\n").unwrap();
    let o = Command::new(aipl).args(["compile"]).arg(&trap).arg("-o").arg(dir.join("trap.wasm")).current_dir(&dir).output().unwrap();
    assert!(o.status.success());
    let o = Command::new(runner).arg(dir.join("trap.wasm")).current_dir(&dir).output().unwrap();
    assert_eq!(o.status.code(), Some(134));
    assert!(String::from_utf8_lossy(&o.stderr).contains("Array index out of bounds: index 5 for array of length 3\n  at main (trap.aipl:6:14)"), "{}", String::from_utf8_lossy(&o.stderr));
    let _ = std::fs::remove_dir_all(&dir);
}
