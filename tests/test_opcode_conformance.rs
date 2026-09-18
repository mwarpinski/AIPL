use aipl_core::ast::OpCode;
use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::parser::Parser;
use aipl_core::vm::VM;
use wasmparser::Validator;

fn get_minimal_program_for_op(op: &OpCode) -> &'static str {
    match op {
        OpCode::Add => "(module test_mod (fn test_op [] -> i32 (+ 10 20)))",
        OpCode::Sub => "(module test_mod (fn test_op [] -> i32 (- 20 10)))",
        OpCode::Mul => "(module test_mod (fn test_op [] -> i32 (* 3 4)))",
        OpCode::Div => "(module test_mod (fn test_op [] -> i32 (/ 20 4)))",
        OpCode::Mod => "(module test_mod (fn test_op [] -> i32 (% 20 6)))",
        OpCode::BitXor => "(module test_mod (fn test_op [] -> i32 (^ 15 3)))",
        OpCode::Shl => "(module test_mod (fn test_op [] -> i32 (shl 2 3)))",
        OpCode::Shr => "(module test_mod (fn test_op [] -> i32 (shr 16 2)))",
        OpCode::ShrU => "(module test_mod (fn test_op [] -> i32 (shru 16 2)))",
        OpCode::DivU => "(module test_mod (fn test_op [] -> i32 (divu 20 4)))",
        OpCode::RemU => "(module test_mod (fn test_op [] -> i32 (remu 20 6)))",
        OpCode::BitAnd => "(module test_mod (fn test_op [] -> i32 (bitand 15 3)))",
        OpCode::BitOr => "(module test_mod (fn test_op [] -> i32 (bitor 12 3)))",
        OpCode::MemLoad8 => "(module test_mod (fn test_op [] -> i32 (mem.load8 0)))",
        OpCode::MemLoad32 => "(module test_mod (fn test_op [] -> i32 (mem.load32 0)))",
        OpCode::MemLoad64 => "(module test_mod (fn test_op [] -> i64 (mem.load64 0)))",
        OpCode::MemLoadF32 => "(module test_mod (fn test_op [] -> f32 (mem.load_f32 0)))",
        OpCode::MemLoadF64 => "(module test_mod (fn test_op [] -> f64 (mem.load_f64 0)))",
        OpCode::MemStore8 => "(module test_mod (fn test_op [] -> void (mem.store8 0 42)))",
        OpCode::MemStore32 => "(module test_mod (fn test_op [] -> void (mem.store32 0 42)))",
        OpCode::MemStore64 => "(module test_mod (fn test_op [] -> void (mem.store64 0 (mem.load64 0))))",
        OpCode::MemStoreF32 => "(module test_mod (fn test_op [] -> void (mem.store_f32 0 (mem.load_f32 0))))",
        OpCode::MemStoreF64 => "(module test_mod (fn test_op [] -> void (mem.store_f64 0 (mem.load_f64 0))))",
        OpCode::MemAlloc => "(module test_mod (fn test_op [] -> i32 (mem.alloc 16)))",
        OpCode::MemFree => "(module test_mod (fn test_op [] -> void (mem.free 0)))",
        OpCode::AtomicAdd => "(module test_mod (fn test_op [] -> i32 (atomic.add 0 1)))",
        OpCode::AtomicCas => "(module test_mod (fn test_op [] -> bool (atomic.cas 0 0 1)))",
        OpCode::AtomicLock => "(module test_mod (fn test_op [] -> void (atomic.lock 0)))",
        OpCode::AtomicUnlock => "(module test_mod (fn test_op [] -> void (atomic.unlock 0)))",
        OpCode::Eq => "(module test_mod (fn test_op [] -> bool (eq 1 1)))",
        OpCode::Neq => "(module test_mod (fn test_op [] -> bool (neq 1 2)))",
        OpCode::Lt => "(module test_mod (fn test_op [] -> bool (lt 1 2)))",
        OpCode::Lte => "(module test_mod (fn test_op [] -> bool (lte 1 2)))",
        OpCode::Gt => "(module test_mod (fn test_op [] -> bool (gt 2 1)))",
        OpCode::Gte => "(module test_mod (fn test_op [] -> bool (gte 2 1)))",
        OpCode::And => "(module test_mod (fn test_op [] -> bool (and true false)))",
        OpCode::Or => "(module test_mod (fn test_op [] -> bool (or true false)))",
        OpCode::Not => "(module test_mod (fn test_op [] -> bool (not true)))",
        OpCode::ArrGet => "(module test_mod (fn test_op [] -> i32 (arr.get 0 0)))",
        OpCode::ArrSet => "(module test_mod (fn test_op [] -> void (arr.set 0 0 1)))",
        OpCode::SysPrint => "(module test_mod (fn test_op [] -> void (sys.print \"test\")))",
        OpCode::SysTime => "(module test_mod (fn test_op [] -> f64 (sys.time)))",
        OpCode::SysExit => "(module test_mod (fn test_op [] -> void (sys.exit 0)))",
        OpCode::FsOpen => "(module test_mod (fn test_op [] -> i32 (fs.open 0 4 0)))",
        OpCode::FsRead => "(module test_mod (fn test_op [] -> i32 (fs.read 3 0 10)))",
        OpCode::FsWrite => "(module test_mod (fn test_op [] -> i32 (fs.write 3 0 10)))",
        OpCode::FsClose => "(module test_mod (fn test_op [] -> i32 (fs.close 3)))",
        OpCode::FsDelete => "(module test_mod (fn test_op [] -> i32 (fs.delete 0 4)))",
        OpCode::ThreadSpawn => "(module test_mod (fn helper [a:i32] -> i32 a) (fn test_op [] -> i32 (thread.spawn 0 6 42)))",
        OpCode::ThreadJoin => "(module test_mod (fn test_op [] -> i32 (thread.join 1)))",
    }
}

const ALL_OPCODES: &[OpCode] = &[
    OpCode::Add,
    OpCode::Sub,
    OpCode::Mul,
    OpCode::Div,
    OpCode::Mod,
    OpCode::BitXor,
    OpCode::Shl,
    OpCode::Shr,
    OpCode::ShrU,
    OpCode::DivU,
    OpCode::RemU,
    OpCode::BitAnd,
    OpCode::BitOr,
    OpCode::MemLoad8,
    OpCode::MemLoad32,
    OpCode::MemLoad64,
    OpCode::MemLoadF32,
    OpCode::MemLoadF64,
    OpCode::MemStore8,
    OpCode::MemStore32,
    OpCode::MemStore64,
    OpCode::MemStoreF32,
    OpCode::MemStoreF64,
    OpCode::MemAlloc,
    OpCode::MemFree,
    OpCode::AtomicAdd,
    OpCode::AtomicCas,
    OpCode::AtomicLock,
    OpCode::AtomicUnlock,
    OpCode::Eq,
    OpCode::Neq,
    OpCode::Lt,
    OpCode::Lte,
    OpCode::Gt,
    OpCode::Gte,
    OpCode::And,
    OpCode::Or,
    OpCode::Not,
    OpCode::ArrGet,
    OpCode::ArrSet,
    OpCode::SysPrint,
    OpCode::SysTime,
    OpCode::SysExit,
    OpCode::FsOpen,
    OpCode::FsRead,
    OpCode::FsWrite,
    OpCode::FsClose,
    OpCode::FsDelete,
    OpCode::ThreadSpawn,
    OpCode::ThreadJoin,
];

#[test]
fn test_all_opcodes_conformance() {
    for op in ALL_OPCODES {
        let src = get_minimal_program_for_op(op);
        let parse_res = Parser::parse(src);
        if parse_res.is_err() {
            // (b) rejected at parse time
            continue;
        }
        let module = parse_res.unwrap();
        let mut checker = TypeChecker::new();
        if checker.check_module(&module).is_err() {
            // (b) rejected at check time
            continue;
        }

        let mut vm = VM::new();
        vm.load_module(module.clone());
        let vm_res = vm.invoke("test_op", vec![]);

        let wasm_res = WasmCompiler::compile(&module);

        let option_a = if let (Ok(_), Ok(wasm_bytes)) = (&vm_res, &wasm_res) {
            let mut validator = Validator::new();
            validator.validate_all(wasm_bytes).is_ok()
        } else {
            false
        };

        let option_b = vm_res.is_err() || wasm_res.is_err();

        assert!(
            option_a || option_b,
            "OpCode {:?} failed conformance: VM result={:?}, Wasm result={:?}",
            op,
            vm_res,
            wasm_res
        );
    }
}
