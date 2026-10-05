//! Every operator, with well-typed operands, must work everywhere the checker
//! lets it through: it parses and checks, runs in the VM, and compiles to
//! valid wasm both where its value is used and as a statement (where a
//! value must be dropped), and the self-hosted compiler emits the same
//! bytes. Nothing is skipped: an op that fails any step fails the test.
//! (An earlier version passed whenever either backend failed, and skipped
//! ops the parser or checker rejected; `checked.add` and `sys.time` as
//! statements compiled to invalid wasm under it.)

use aipl_core::ast::OpCode;
use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::parser::Parser;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use std::path::Path;
use wasmparser::Validator;

/// (result type, expression) for each op. Memory and atomic ops work on
/// heap words from mem.alloc, never on address 0, the heap cursor.
fn case(op: &OpCode) -> (&'static str, &'static str) {
    use OpCode::*;
    match op {
        Add => ("i32", "(+ 10 20)"),
        Sub => ("f64", "(- 2.5 1.0)"),
        Mul => ("i64", "(* 3i64 4i64)"),
        Div => ("i32", "(/ 20 4)"),
        DivU => ("i32", "(divu 20 4)"),
        Mod => ("i32", "(% 20 6)"),
        RemU => ("i64", "(remu 20i64 6i64)"),
        BitXor => ("i32", "(^ 15 3)"),
        Shl => ("i32", "(shl 2 3)"),
        Shr => ("i32", "(shr 16 2)"),
        ShrU => ("i32", "(shru 16 2)"),
        BitAnd => ("i32", "(bitand 15 3)"),
        BitOr => ("i32", "(bitor 12 3)"),
        CheckedAdd => ("i32", "(checked.add 2 3)"),
        CheckedSub => ("i64", "(checked.sub 2i64 3i64)"),
        CheckedMul => ("i32", "(checked.mul 4 5)"),
        MemLoad8 => ("i32", "(mem.load8 (mem.alloc 8))"),
        MemLoad32 => ("i32", "(mem.load32 (mem.alloc 8))"),
        MemLoad64 => ("i64", "(mem.load64 (mem.alloc 8))"),
        MemStore8 => ("void", "(mem.store8 (mem.alloc 8) 42)"),
        MemStore32 => ("void", "(mem.store32 (mem.alloc 8) 42)"),
        MemStore64 => ("void", "(mem.store64 (mem.alloc 8) 42i64)"),
        MemAlloc => ("i32", "(mem.alloc 16)"),
        MemGrow => ("i32", "(mem.grow 1)"),
        AtomicAdd => ("i32", "(atomic.add (mem.alloc 4) 1)"),
        AtomicCas => ("bool", "(atomic.cas (mem.alloc 4) 0 1)"),
        AtomicLock => ("void", "(atomic.lock (mem.alloc 4))"),
        AtomicUnlock => ("void", "(atomic.unlock (call locked))"),
        Eq => ("bool", "(eq \"a\" \"a\")"),
        Neq => ("bool", "(neq true false)"),
        Lt => ("bool", "(lt 1.5 2.5)"),
        Lte => ("bool", "(lte 1i64 2i64)"),
        Gt => ("bool", "(gt 2 1)"),
        Gte => ("bool", "(gte 2 1)"),
        LtU => ("bool", "(ltu 1 -1)"),
        LteU => ("bool", "(lteu 1i64 -1i64)"),
        GtU => ("bool", "(gtu -1 1)"),
        GteU => ("bool", "(gteu -1 1)"),
        And => ("bool", "(and true false)"),
        Or => ("bool", "(or true false)"),
        Not => ("bool", "(not true)"),
        SysPrint => ("void", "(sys.print \"conformance\")"),
        SysTime => ("i64", "(sys.time)"),
        SysMonotonic => ("i64", "(sys.monotonic)"),
        SysRandom => ("i32", "(sys.random (mem.alloc 8) 8)"),
        SysExit => ("void", "(sys.exit 0)"),
        FsOpen => ("i32", "(fs.open (str.ptr \"no-such-file\") 12 0)"),
        FsRead => ("i32", "(fs.read 99 (mem.alloc 8) 8)"),
        FsWrite => ("i32", "(fs.write 99 (mem.alloc 8) 8)"),
        FsClose => ("i32", "(fs.close 99)"),
        FsDelete => ("i32", "(fs.delete (str.ptr \"no-such-file\") 12)"),
        ArgsSizes => ("i32", "(args.sizes (mem.alloc 8) (+ (mem.alloc 8) 4))"),
        ArgsGet => ("i32", "(args.get (mem.alloc 8) (mem.alloc 64))"),
        EnvSizes => ("i32", "(env.sizes (mem.alloc 8) (+ (mem.alloc 8) 4))"),
        EnvGet => ("i32", "(env.get (mem.alloc 1024) (mem.alloc 65536))"),
        ThreadSpawn => ("i32", "(thread.spawn (ref helper) 42)"),
        ThreadJoin => ("i32", "(thread.join (thread.spawn (ref helper) 42))"),
        I64ExtendS => ("i64", "(i64.extend_s -1)"),
        I64ExtendU => ("i64", "(i64.extend_u -1)"),
        I32Wrap => ("i32", "(i32.wrap 4294967297i64)"),
        F64ConvertI64S => ("f64", "(f64.convert_i64_s 3i64)"),
        F64Sqrt => ("f64", "(f64.sqrt 2.0)"),
        I64TruncF64S => ("i64", "(i64.trunc_f64_s -2.75)"),
        F64ReinterpretI64 => ("f64", "(f64.reinterpret_i64 4611686018427387904i64)"),
        I64ReinterpretF64 => ("i64", "(i64.reinterpret_f64 2.0)"),
        StrLen => ("i32", "(str.len \"hello\")"),
        StrPtr => ("i32", "(mem.load8 (str.ptr \"hello\"))"),
    }
}

const ALL_OPCODES: &[OpCode] = {
    use OpCode::*;
    &[
        Add, Sub, Mul, Div, DivU, Mod, RemU, BitXor, Shl, Shr, ShrU, BitAnd, BitOr, CheckedAdd, CheckedSub, CheckedMul,
        MemLoad8, MemLoad32, MemLoad64, MemStore8, MemStore32, MemStore64, MemAlloc, MemGrow, AtomicAdd, AtomicCas,
        AtomicLock, AtomicUnlock, Eq, Neq, Lt, Lte, Gt, Gte, LtU, LteU, GtU, GteU, And, Or, Not, SysPrint, SysTime,
        SysMonotonic, SysRandom, SysExit, FsOpen, FsRead, FsWrite, FsClose, FsDelete, ArgsSizes, ArgsGet, EnvSizes,
        EnvGet, ThreadSpawn, ThreadJoin, I64ExtendS, I64ExtendU, I32Wrap, F64ConvertI64S, F64Sqrt, I64TruncF64S,
        F64ReinterpretI64, I64ReinterpretF64, StrLen, StrPtr,
    ]
};

/// Functions a case may call.
const HELPERS: &str = "(fn helper [a:i32] -> i32 a)
  (fn locked [] -> i32 (let m:i32 (mem.alloc 4)) (atomic.lock m) m)";

/// `value_N` returns the op's value; `stmt_N` evaluates it as a statement.
fn functions(i: usize, ty: &str, expr: &str) -> String {
    format!("(fn value_{i} [] -> {ty} {expr})\n  (fn stmt_{i} [] -> i32 {expr} 0)")
}

fn validate(bytes: &[u8]) -> Result<(), String> {
    Validator::new_with_features(wasmparser::WasmFeatures::all()).validate_all(bytes).map(|_| ()).map_err(|e| e.to_string())
}

#[test]
fn every_op_checks_runs_and_compiles_to_valid_wasm() {
    let mut seen = std::collections::HashSet::new();
    for (i, op) in ALL_OPCODES.iter().enumerate() {
        assert!(seen.insert(format!("{op:?}")), "{op:?} listed twice");
        let (ty, expr) = case(op);
        let src = format!("(module conf\n  {HELPERS}\n  {})", functions(i, ty, expr));
        let module = Parser::parse(&src).unwrap_or_else(|e| panic!("{op:?} does not parse: {e}\n{src}"));
        TypeChecker::new().check_module(&module).unwrap_or_else(|e| panic!("{op:?} does not check: {e}\n{src}"));

        let bytes = WasmCompiler::compile(&module).unwrap_or_else(|e| panic!("{op:?} does not compile: {e}"));
        validate(&bytes).unwrap_or_else(|e| panic!("{op:?} compiles to invalid wasm: {e}\n{src}"));

        // sys.exit stops the VM with its documented error; everything else runs
        for f in [format!("value_{i}"), format!("stmt_{i}")] {
            let mut vm = VM::new();
            vm.load_module(module.clone());
            match (op, vm.invoke(&f, vec![])) {
                (OpCode::SysExit, Err(e)) => assert!(e.contains("sys.exit(0) requested"), "{e}"),
                (OpCode::SysExit, Ok(v)) => panic!("sys.exit returned {v:?}"),
                (_, Err(e)) => panic!("{op:?}: {f} fails in the VM: {e}"),
                (_, Ok(v)) => {
                    if f.starts_with("stmt") {
                        assert_eq!(v, Value::Int(0), "{op:?}: {f}");
                    }
                }
            }
        }
    }
    // every OpCode variant is listed (case() is an exhaustive match)
    assert_eq!(seen.len(), ALL_OPCODES.len());
}

/// One module with every case: the self-hosted compiler (run in the VM)
/// emits the Rust backend's bytes, value and statement positions alike.
#[test]
fn self_hosted_compiler_matches_on_every_op() {
    let body: Vec<String> = ALL_OPCODES.iter().enumerate().map(|(i, op)| { let (ty, e) = case(op); functions(i, ty, e) }).collect();
    let src = format!("(module conf\n  {HELPERS}\n  {})", body.join("\n  "));
    let module = Parser::parse(&src).unwrap();
    TypeChecker::new().check_module(&module).unwrap();
    let rust = WasmCompiler::compile(&module).unwrap();
    validate(&rust).unwrap();
    let ours = std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(move || self_hosted(&src))
        .unwrap()
        .join()
        .unwrap();
    assert!(ours == rust, "self-hosted output differs from the Rust backend's ({} vs {} bytes)", ours.len(), rust.len());
}

fn self_hosted(src: &str) -> Vec<u8> {
    let codegen = Path::new(env!("CARGO_MANIFEST_DIR")).join("aipl_src/codegen.aipl");
    let module = Resolver::resolve(&codegen).unwrap();
    TypeChecker::new().check_module(&module).unwrap();
    let mut vm = VM::new();
    vm.load_module(module);
    vm.invoke("init_keywords", vec![]).unwrap();
    let Value::Int(ptr) = vm.invoke("alloc_src", vec![Value::Int(src.len() as i64 + 16)]).unwrap() else { panic!() };
    vm.write_bytes(ptr as usize, src.as_bytes());
    let Value::Int(len) = vm.invoke("compile_module", vec![Value::Int(ptr), Value::Int(src.len() as i64)]).unwrap() else { panic!() };
    let word = |a: usize| u32::from_le_bytes(vm.read_bytes(a, 4).try_into().unwrap()) as usize;
    assert!(len > 0, "self-hosted compile error {}", word(4));
    vm.read_bytes(word(60), len as usize)
}
