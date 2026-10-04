//! Native backend task NE1: aipl_src/native/wasm_reader.aipl decodes the
//! module structure of every wasm module AIPL compiles. Its summary of each
//! module is compared, line for line, with the same summary built from
//! wasmparser. NE2 adds every function body: its local declarations and
//! each instruction's offset, opcode and immediates.

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use std::fmt::Write;
use std::path::{Path, PathBuf};
use wasmparser::{BlockType, DataKind, ElementItems, ElementKind, ExternalKind, Operator, Parser, Payload, TypeRef, ValType};

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
            Payload::CodeSectionEntry(body) => {
                writeln!(code, "code len {}", body.range().len()).unwrap();
                code.push_str(&body_summary(wasm, &body));
            }
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

fn block_type(b: BlockType) -> u32 {
    match b {
        BlockType::Empty => 0x40,
        BlockType::Type(ValType::I32) => 0x7f,
        BlockType::Type(ValType::I64) => 0x7e,
        BlockType::Type(ValType::F32) => 0x7d,
        BlockType::Type(ValType::F64) => 0x7c,
        other => panic!("unexpected block type {other:?}"),
    }
}

/// The opcode at `at`: one byte, or 0xFC00 / 0xFE00 plus the LEB128
/// sub-opcode for the prefixed instructions.
fn opcode(wasm: &[u8], at: usize) -> u32 {
    let first = wasm[at] as u32;
    if first != 0xfc && first != 0xfe {
        return first;
    }
    let (mut sub, mut shift, mut i) = (0u32, 0, at + 1);
    loop {
        sub |= ((wasm[i] & 0x7f) as u32) << shift;
        if wasm[i] & 0x80 == 0 {
            break;
        }
        shift += 7;
        i += 1;
    }
    (first << 8) + sub
}

/// The lines wasm_reader.push_body prints for one body: "  locals N type",
/// then "  @offset op a b c" per instruction (offsets from the first
/// instruction; immediates as described at wasm_reader's Instr).
fn body_summary(wasm: &[u8], body: &wasmparser::FunctionBody) -> String {
    let mut out = String::new();
    for l in body.get_locals_reader().unwrap() {
        let (count, t) = l.unwrap();
        writeln!(out, "  locals {} {}", count, ty(t)).unwrap();
    }
    let mut ops = body.get_operators_reader().unwrap();
    let start = ops.original_position();
    while !ops.eof() {
        let (op, at) = ops.read_with_offset().unwrap();
        let (a, b, c): (u32, u32, i64) = match op {
            Operator::Block { blockty } | Operator::Loop { blockty } | Operator::If { blockty } => (block_type(blockty), 0, 0),
            Operator::Br { relative_depth } | Operator::BrIf { relative_depth } => (relative_depth, 0, 0),
            Operator::Call { function_index } => (function_index, 0, 0),
            Operator::CallIndirect { type_index, table_index } => (type_index, table_index, 0),
            Operator::LocalGet { local_index } | Operator::LocalSet { local_index } | Operator::LocalTee { local_index } => (local_index, 0, 0),
            Operator::GlobalGet { global_index } | Operator::GlobalSet { global_index } => (global_index, 0, 0),
            Operator::MemorySize { mem } | Operator::MemoryGrow { mem } => (0, mem, 0),
            Operator::MemoryInit { data_index, mem } => (data_index, mem, 0),
            Operator::I32Const { value } => (0, 0, value as i64),
            Operator::I64Const { value } => (0, 0, value),
            Operator::F64Const { value } => (0, 0, value.bits() as i64),
            Operator::I32Load { memarg }
            | Operator::I64Load { memarg }
            | Operator::F32Load { memarg }
            | Operator::F64Load { memarg }
            | Operator::I32Load8U { memarg }
            | Operator::I32Store { memarg }
            | Operator::I64Store { memarg }
            | Operator::F32Store { memarg }
            | Operator::F64Store { memarg }
            | Operator::I32Store8 { memarg }
            | Operator::MemoryAtomicNotify { memarg }
            | Operator::MemoryAtomicWait32 { memarg }
            | Operator::I32AtomicLoad { memarg }
            | Operator::I32AtomicStore { memarg }
            | Operator::I32AtomicRmwAdd { memarg }
            | Operator::I32AtomicRmwXchg { memarg }
            | Operator::I32AtomicRmwCmpxchg { memarg } => (memarg.align as u32, memarg.offset as u32, 0),
            _ => (0, 0, 0),
        };
        writeln!(out, "  @{} {} {} {} {}", at - start, opcode(wasm, at), a, b, c).unwrap();
    }
    out
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

/// The reader's own AIPL self-tests (also in aipl_src/test_suite.aipl).
#[test]
fn the_reader_self_tests_pass() {
    let reader = Resolver::resolve(&root().join("aipl_src/native/wasm_reader.aipl")).unwrap();
    let mut vm = VM::new();
    vm.load_module(reader);
    assert_eq!(vm.invoke("run_reader_tests", vec![]).unwrap(), Value::Int(5));
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

/// One body holding every instruction the reader accepts (everything
/// src/compiler/wasm.rs can emit, used by a program or not), with varied
/// immediates, then `extra`. The body need not validate; only its encoding
/// matters.
fn every_instruction_module(extra: &[wasm_encoder::Instruction]) -> Vec<u8> {
    use wasm_encoder::{
        BlockType as B, CodeSection, Function, FunctionSection, Instruction as I, MemArg, Module, TypeSection, ValType as V,
    };
    let m = |align, offset| MemArg { offset, align, memory_index: 0 };
    let mut f = Function::new(vec![(2, V::I32), (1, V::I64), (3, V::F64)]);
    for ins in [
        I::Unreachable, I::Block(B::Empty), I::Loop(B::Result(V::I32)), I::If(B::Result(V::I64)), I::Else, I::End,
        I::Block(B::Result(V::F32)), I::Block(B::Result(V::F64)), I::Br(3), I::BrIf(200), I::Return, I::Call(300),
        I::CallIndirect { type_index: 5, table_index: 0 }, I::Drop, I::LocalGet(1), I::LocalSet(129), I::LocalTee(70000),
        I::GlobalGet(0), I::GlobalSet(1), I::I32Load(m(2, 0)), I::I64Load(m(3, 8)), I::F32Load(m(2, 16)),
        I::F64Load(m(3, 4096)), I::I32Load8U(m(0, 1)), I::I32Store(m(2, 4)), I::I64Store(m(3, 0)), I::F32Store(m(2, 0)),
        I::F64Store(m(3, 24)), I::I32Store8(m(0, 3)), I::MemorySize(0), I::MemoryGrow(0), I::I32Const(0),
        I::I32Const(-1), I::I32Const(i32::MIN), I::I32Const(i32::MAX), I::I64Const(i64::MIN), I::I64Const(i64::MAX),
        I::I64Const(-129), I::F64Const((-0.0f64).into()), I::F64Const(f64::NAN.into()), I::F64Const(1.5f64.into()),
        I::I32Eqz, I::I32Eq, I::I32Ne, I::I32LtS, I::I32LtU, I::I32GtS, I::I32GtU, I::I32LeS, I::I32GeS,
        I::I64Eq, I::I64Ne, I::I64LtS, I::I64GtS, I::I64LeS, I::I64GeS,
        I::F32Eq, I::F32Ne, I::F32Lt, I::F32Gt, I::F32Le, I::F32Ge, I::F64Eq, I::F64Ne, I::F64Lt, I::F64Gt, I::F64Le, I::F64Ge,
        I::I32Add, I::I32Sub, I::I32Mul, I::I32DivS, I::I32DivU, I::I32RemS, I::I32RemU, I::I32And, I::I32Or, I::I32Xor,
        I::I32Shl, I::I32ShrS, I::I32ShrU, I::I64Add, I::I64Sub, I::I64Mul, I::I64DivS, I::I64DivU, I::I64RemS,
        I::I64RemU, I::I64And, I::I64Or, I::I64Xor, I::I64Shl, I::I64ShrS, I::I64ShrU,
        I::F32Add, I::F32Sub, I::F32Mul, I::F32Div, I::F64Add, I::F64Sub, I::F64Mul, I::F64Div,
        I::I32WrapI64, I::I64ExtendI32S, I::I64ExtendI32U, I::I64TruncF64S, I::F64ConvertI64S, I::F64ReinterpretI64,
        I::I64ReinterpretF64, I::MemoryInit { mem: 0, data_index: 7 }, I::MemoryAtomicNotify(m(2, 0)),
        I::MemoryAtomicWait32(m(2, 88)), I::I32AtomicLoad(m(2, 0)), I::I32AtomicStore(m(2, 4)), I::I32AtomicRmwAdd(m(2, 0)),
        I::I32AtomicRmwXchg(m(2, 8)), I::I32AtomicRmwCmpxchg(m(2, 12)),
    ]
    .iter()
    .chain(extra)
    {
        f.instruction(ins);
    }
    f.instruction(&I::End);
    let mut types = TypeSection::new();
    types.ty().function([], []);
    let mut funcs = FunctionSection::new();
    funcs.function(0);
    let mut code = CodeSection::new();
    code.function(&f);
    let mut module = Module::new();
    module.section(&types).section(&funcs).section(&code);
    module.finish()
}

#[test]
fn every_accepted_instruction_decodes_like_wasmparser() {
    let reader = Resolver::resolve(&root().join("aipl_src/native/wasm_reader.aipl")).unwrap();
    let mut vm = VM::new();
    vm.load_module(reader);
    let wasm = every_instruction_module(&[]);
    let theirs = wasmparser_summary(&wasm);
    assert!(theirs.lines().count() > 120, "{theirs}");
    assert_eq!(aipl_summary(&mut vm, &wasm), theirs);
}

#[test]
fn instructions_outside_the_list_are_errors_naming_the_opcode() {
    use wasm_encoder::{Instruction as I, MemArg};
    let reader = Resolver::resolve(&root().join("aipl_src/native/wasm_reader.aipl")).unwrap();
    let mut vm = VM::new();
    vm.load_module(reader);
    let arg = MemArg { offset: 0, align: 3, memory_index: 0 };
    for (ins, op) in [
        (I::Select, 0x1b),
        (I::I32Clz, 0x67),
        (I::I64LtU, 0x54),
        (I::F64Sqrt, 0x9f),
        (I::F32Const(1.0f32.into()), 0x43),
        (I::MemoryCopy { src_mem: 0, dst_mem: 0 }, 0xfc0a),
        (I::I64AtomicLoad(arg), 0xfe11),
    ] {
        let s = aipl_summary(&mut vm, &every_instruction_module(&[ins]));
        assert!(s.starts_with(&format!("error: unsupported instruction {op} at byte")), "{s}");
    }
}

/// Every opcode outside the accepted set is rejected, not just the samples
/// above: each one-byte opcode, and each 0xFC / 0xFE sub-opcode, alone in a
/// body. (The reader rejects before reading immediates.)
#[test]
fn the_accepted_set_is_exactly_the_listed_instructions() {
    let reader = Resolver::resolve(&root().join("aipl_src/native/wasm_reader.aipl")).unwrap();
    let mut vm = VM::new();
    vm.load_module(reader);
    // the accepted opcodes: those in the every-instruction body
    let all = every_instruction_module(&[]);
    let mut accepted = std::collections::BTreeSet::new();
    for p in Parser::new(0).parse_all(&all) {
        if let Payload::CodeSectionEntry(b) = p.unwrap() {
            let mut ops = b.get_operators_reader().unwrap();
            while !ops.eof() {
                accepted.insert(opcode(&all, ops.read_with_offset().unwrap().1));
            }
        }
    }
    let candidates = (0..=255u32).filter(|o| *o != 0xfc && *o != 0xfe).chain((0..=20).map(|s| 0xfc00 + s)).chain((0..=80).map(|s| 0xfe00 + s));
    let mut rejected = 0;
    for op in candidates.filter(|o| !accepted.contains(o)) {
        let ins: Vec<u8> = if op > 255 { vec![(op >> 8) as u8, (op & 0xff) as u8] } else { vec![op as u8] };
        let body = [&[0u8][..], &ins, &[0x0b]].concat();
        let code = [&[1u8, body.len() as u8][..], &body].concat();
        let wasm = [
            &b"\0asm\x01\0\0\0"[..],
            &[1, 4, 1, 0x60, 0, 0],
            &[3, 2, 1, 0],
            &[10, code.len() as u8],
            &code,
        ]
        .concat();
        let s = aipl_summary(&mut vm, &wasm);
        assert!(s.starts_with(&format!("error: unsupported instruction {op} at byte 23")), "{op}: {s}");
        rejected += 1;
    }
    assert!(accepted.len() > 100 && rejected > 200, "{} accepted, {rejected} rejected", accepted.len());
}
