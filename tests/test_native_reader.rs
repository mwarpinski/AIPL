//! Native backend task NE1: aipl_src/native/wasm_reader.aipl decodes the
//! module structure of every wasm module AIPL compiles. Its summary of each
//! module is compared, line for line, with the same summary built from
//! wasmparser.

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use std::fmt::Write;
use std::path::{Path, PathBuf};
use wasmparser::{DataKind, ElementItems, ElementKind, ExternalKind, Operator, Parser, Payload, TypeRef, ValType};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// Every program in the repository that compiles to wasm, plus the thread
/// and generics test programs (threaded modules, passive data, globals).
fn programs() -> Vec<(String, Vec<u8>)> {
    let mut paths = Vec::new();
    for dir in ["examples", "aipl_src/std", "aipl_src"] {
        for e in std::fs::read_dir(root().join(dir)).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "aipl") {
                paths.push(p);
            }
        }
    }
    paths.sort();
    let mut out = Vec::new();
    for p in paths {
        let Ok(m) = Resolver::resolve(&p) else { continue };
        if TypeChecker::new().check_module(&m).is_err() {
            continue;
        }
        if let Ok(w) = WasmCompiler::compile(&m) {
            out.push((p.display().to_string(), w));
        }
    }
    let threads = include_str!("test_threads.rs");
    for name in ["(module threads", "(module atomics"] {
        let start = threads.find(name).unwrap();
        let end = threads[start..].find("\"#;").unwrap();
        let m = aipl_core::parser::Parser::parse(&threads[start..start + end]).unwrap();
        out.push((name.to_string(), WasmCompiler::compile(&m).unwrap()));
    }
    out
}

fn ty(t: ValType) -> &'static str {
    match t {
        ValType::I32 => "i32",
        ValType::I64 => "i64",
        ValType::F32 => "f32",
        ValType::F64 => "f64",
        _ => "?",
    }
}

fn limits(initial: u64, maximum: Option<u64>, shared: bool) -> String {
    let max = maximum.map_or("none".to_string(), |m| m.to_string());
    format!("{} {}{}", initial, max, if shared { " shared" } else { "" })
}

fn const_i32(expr: &wasmparser::ConstExpr) -> i64 {
    match expr.get_operators_reader().read().unwrap() {
        Operator::I32Const { value } => value as i64,
        Operator::I64Const { value } => value,
        other => panic!("unexpected constant {other:?}"),
    }
}

/// The summary wasm_reader.summary prints, built with wasmparser.
fn wasmparser_summary(wasm: &[u8]) -> String {
    let (mut types, mut imports, mut funcs, mut tables, mut memories, mut globals, mut exports) =
        (String::new(), String::new(), String::new(), String::new(), String::new(), String::new(), String::new());
    let (mut start, mut elems, mut datacount, mut code, mut data) = (String::new(), String::new(), String::new(), String::new(), String::new());
    for payload in Parser::new(0).parse_all(wasm) {
        match payload.unwrap() {
            Payload::TypeSection(r) => {
                for t in r.into_iter_err_on_gc_types() {
                    let t = t.unwrap();
                    let list = |ts: &[ValType]| ts.iter().map(|t| ty(*t)).collect::<Vec<_>>().join(" ");
                    writeln!(types, "type [{}] -> [{}]", list(t.params()), list(t.results())).unwrap();
                }
            }
            Payload::ImportSection(r) => {
                for i in r {
                    let i = i.unwrap();
                    match i.ty {
                        TypeRef::Func(t) => writeln!(imports, "import {}.{} func {}", i.module, i.name, t).unwrap(),
                        TypeRef::Memory(m) => writeln!(imports, "import {}.{} memory {}", i.module, i.name, limits(m.initial, m.maximum, m.shared)).unwrap(),
                        other => panic!("unexpected import {other:?}"),
                    }
                }
            }
            Payload::FunctionSection(r) => {
                for f in r {
                    writeln!(funcs, "func type {}", f.unwrap()).unwrap();
                }
            }
            Payload::TableSection(r) => {
                for t in r {
                    let t = t.unwrap();
                    writeln!(tables, "table funcref {}", limits(t.ty.initial, t.ty.maximum, false)).unwrap();
                }
            }
            Payload::MemorySection(r) => {
                for m in r {
                    let m = m.unwrap();
                    writeln!(memories, "memory {}", limits(m.initial, m.maximum, m.shared)).unwrap();
                }
            }
            Payload::GlobalSection(r) => {
                for g in r {
                    let g = g.unwrap();
                    let m = if g.ty.mutable { "mut" } else { "const" };
                    writeln!(globals, "global {} {} {}", ty(g.ty.content_type), m, const_i32(&g.init_expr) as i32).unwrap();
                }
            }
            Payload::ExportSection(r) => {
                for e in r {
                    let e = e.unwrap();
                    let k = match e.kind {
                        ExternalKind::Func => "func",
                        ExternalKind::Memory => "memory",
                        other => panic!("unexpected export {other:?}"),
                    };
                    writeln!(exports, "export {} {} {}", e.name, k, e.index).unwrap();
                }
            }
            Payload::StartSection { func, .. } => writeln!(start, "start {}", func).unwrap(),
            Payload::ElementSection(r) => {
                for e in r {
                    let e = e.unwrap();
                    let ElementKind::Active { offset_expr, .. } = e.kind else { panic!("passive element") };
                    let ElementItems::Functions(fs) = e.items else { panic!("expression elements") };
                    let fs: Vec<String> = fs.into_iter().map(|f| format!(" {}", f.unwrap())).collect();
                    writeln!(elems, "elem offset {} funcs{}", const_i32(&offset_expr), fs.concat()).unwrap();
                }
            }
            Payload::DataCountSection { count, .. } => writeln!(datacount, "datacount {}", count).unwrap(),
            Payload::CodeSectionEntry(body) => writeln!(code, "code len {}", body.range().len()).unwrap(),
            Payload::DataSection(r) => {
                for d in r {
                    let d = d.unwrap();
                    match d.kind {
                        DataKind::Passive => writeln!(data, "data passive len {}", d.data.len()).unwrap(),
                        DataKind::Active { offset_expr, .. } => {
                            writeln!(data, "data active offset {} len {}", const_i32(&offset_expr), d.data.len()).unwrap()
                        }
                    }
                }
            }
            _ => {}
        }
    }
    [types, imports, funcs, tables, memories, globals, exports, start, elems, datacount, code, data].concat()
}

/// wasm_reader.summarize run in the VM over `wasm`.
fn aipl_summary(vm: &mut VM, wasm: &[u8]) -> String {
    let addr = match vm.invoke("host_alloc", vec![Value::Int(wasm.len() as i64 + 8)]).unwrap() {
        Value::Int(a) => a as usize,
        other => panic!("{other:?}"),
    };
    vm.write_bytes(addr, wasm);
    let out = match vm.invoke("summarize", vec![Value::Int(addr as i64), Value::Int(wasm.len() as i64)]).unwrap() {
        Value::Int(p) => p as usize,
        other => panic!("{other:?}"),
    };
    let word = |at: usize| i32::from_le_bytes(vm.read_bytes(at, 4).try_into().unwrap()) as usize;
    String::from_utf8(vm.read_bytes(word(out), word(out + 4))).unwrap()
}

#[test]
fn the_reader_decodes_every_module_like_wasmparser() {
    let reader = Resolver::resolve(&root().join("aipl_src/native/wasm_reader.aipl")).unwrap();
    TypeChecker::new().check_module(&reader).unwrap();
    let programs = programs();
    assert!(programs.len() >= 20, "expected the repository's programs, found {}", programs.len());
    let mut threaded = 0;
    for (name, wasm) in &programs {
        // a fresh VM per module keeps the reader's heap small
        let mut vm = VM::new();
        vm.load_module(reader.clone());
        let ours = aipl_summary(&mut vm, wasm);
        let theirs = wasmparser_summary(wasm);
        assert_eq!(ours, theirs, "{name}: summaries differ");
        if theirs.contains("shared") {
            threaded += 1;
        }
    }
    assert!(threaded >= 2, "expected threaded modules among the programs");
}

/// The reader's own AIPL self-tests (run here because aipl_src/test_suite.aipl
/// cannot import a module in a subdirectory yet; see PROGRESS.md, NE5).
#[test]
fn the_reader_self_tests_pass() {
    let reader = Resolver::resolve(&root().join("aipl_src/native/wasm_reader.aipl")).unwrap();
    let mut vm = VM::new();
    vm.load_module(reader);
    assert_eq!(vm.invoke("run_reader_tests", vec![]).unwrap(), Value::Int(4));
}

#[test]
fn malformed_modules_are_errors_with_an_offset() {
    let reader = Resolver::resolve(&root().join("aipl_src/native/wasm_reader.aipl")).unwrap();
    let mut vm = VM::new();
    vm.load_module(reader);
    let good = WasmCompiler::compile(&aipl_core::parser::Parser::parse("(module m (fn main [] -> i32 7))").unwrap()).unwrap();
    for (bytes, expected) in [
        (b"\0asx\x01\0\0\0".to_vec(), "error: not a wasm module (bad magic) at byte 4"),
        (good[..good.len() - 3].to_vec(), "error: "),
        ([&good[..], &[13u8, 0][..]].concat(), "error: unsupported section at byte"),
    ] {
        let s = aipl_summary(&mut vm, &bytes);
        assert!(s.starts_with(expected), "{s}");
    }
}
