use crate::ast::*;
use std::collections::HashMap;
use wasm_encoder::{
    CodeSection, ExportKind, ExportSection, Function, FunctionSection, Instruction,
    Module as WasmModule, TypeSection, ValType,
};

pub struct WasmCompiler;

impl WasmCompiler {
    pub fn compile(module: &Module) -> Result<Vec<u8>, String> {
        let mut wasm_module = WasmModule::new();
        let mut types = TypeSection::new();
        let mut functions = FunctionSection::new();
        let mut exports = ExportSection::new();
        let mut codes = CodeSection::new();

        let mut fn_indices: HashMap<String, u32> = HashMap::new();
        let mut fn_returns: HashMap<String, Type> = HashMap::new();

        // 1. Build type section and function index mapping
        for (idx, f) in module.functions.iter().enumerate() {
            let params: Vec<ValType> = f.params.iter().map(|(_, t)| self::aipl_to_wasm_type(t)).collect();
            let results: Vec<ValType> = if f.return_type == Type::Void {
                vec![]
            } else {
                vec![self::aipl_to_wasm_type(&f.return_type)]
            };

            types.ty().function(params, results);
            functions.function(idx as u32);
            exports.export(&f.name, ExportKind::Func, idx as u32);
            fn_indices.insert(f.name.clone(), idx as u32);
            fn_returns.insert(f.name.clone(), f.return_type.clone());
        }

        // 2. Build code section (body compilation)
        for f in &module.functions {
            let mut extra_lets = Vec::new();
            collect_lets(&f.body, &mut extra_lets);

            let mut local_map: HashMap<String, u32> = HashMap::new();
            let mut current_idx = 0;

            for (p_name, _) in &f.params {
                local_map.insert(p_name.clone(), current_idx);
                current_idx += 1;
            }

            // Static AIPL type of every local, so codegen can pick i32 vs i64
            // vs f64 instructions. First declaration wins, matching local_map.
            let mut local_types: HashMap<String, Type> = HashMap::new();
            for (p_name, p_ty) in &f.params {
                local_types.insert(p_name.clone(), p_ty.clone());
            }

            let mut wasm_locals = Vec::new();
            for (l_name, l_ty) in &extra_lets {
                if !local_map.contains_key(l_name) {
                    local_map.insert(l_name.clone(), current_idx);
                    local_types.insert(l_name.clone(), l_ty.clone());
                    wasm_locals.push((1, aipl_to_wasm_type(l_ty)));
                    current_idx += 1;
                }
            }

            let mut func = Function::new(wasm_locals);
            let ctx = Ctx {
                locals: &local_map,
                local_types: &local_types,
                fn_indices: &fn_indices,
                fn_returns: &fn_returns,
            };

            // Every statement but the last is executed purely for effect: drop
            // any value it leaves behind so it doesn't corrupt the wasm stack.
            // The last statement's value (if any) is the function's implicit
            // return, so it's kept - unless the function is declared void, in
            // which case it must be dropped too.
            let body_len = f.body.len();
            for (i, expr) in f.body.iter().enumerate() {
                compile_expr(expr, &ctx, &mut func)?;
                let is_last = i + 1 == body_len;
                let keep_value = is_last && f.return_type != Type::Void;
                if !keep_value && !is_void_expr(expr, &ctx) {
                    func.instruction(&Instruction::Drop);
                }
            }
            func.instruction(&Instruction::End);
            codes.function(&func);
        }

        let mut memories = wasm_encoder::MemorySection::new();
        memories.memory(wasm_encoder::MemoryType {
            minimum: 1,
            maximum: Some(100),
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        exports.export("memory", ExportKind::Memory, 0);

        let mut globals = wasm_encoder::GlobalSection::new();
        globals.global(
            wasm_encoder::GlobalType {
                val_type: ValType::I32,
                mutable: true,
                shared: false,
            },
            &wasm_encoder::ConstExpr::i32_const(1024),
        );

        wasm_module.section(&types);
        wasm_module.section(&functions);
        wasm_module.section(&memories);
        wasm_module.section(&globals);
        wasm_module.section(&exports);
        wasm_module.section(&codes);

        Ok(wasm_module.finish())
    }
}

/// Bundles the read-only context threaded through codegen so it isn't passed
/// as four separate parameters everywhere.
struct Ctx<'a> {
    locals: &'a HashMap<String, u32>,
    local_types: &'a HashMap<String, Type>,
    fn_indices: &'a HashMap<String, u32>,
    fn_returns: &'a HashMap<String, Type>,
}

/// Static AIPL type of an expression, as the checker would assign it. Codegen
/// uses this to select i32 / i64 / f32 / f64 instruction variants and `if`
/// block result types. The checker has already rejected ill-typed programs,
/// so the fallbacks here (`I32`) are only reached for constructs the checker
/// treats as untyped (e.g. `arr.get`), never for well-typed numeric code.
fn expr_type(expr: &Expr, ctx: &Ctx) -> Type {
    match expr {
        Expr::Lit(lit, _) => match lit {
            Literal::Int(_) => Type::I32,
            Literal::Int64(_) => Type::I64,
            Literal::Float(_) => Type::F64,
            Literal::Bool(_) => Type::Bool,
            Literal::Str(_) => Type::Str,
        },
        Expr::Var(name, _) => ctx.local_types.get(name).cloned().unwrap_or(Type::I32),
        Expr::Let { ty, .. } => ty.clone(),
        Expr::Set { name, .. } => ctx.local_types.get(name).cloned().unwrap_or(Type::I32),
        Expr::If { then_branch, .. } => expr_type(then_branch, ctx),
        Expr::Block(exprs, _) => exprs.last().map_or(Type::Void, |e| expr_type(e, ctx)),
        Expr::Loop { .. } | Expr::While { .. } => Type::Void,
        Expr::Call { func, .. } => ctx.fn_returns.get(func).cloned().unwrap_or(Type::I32),
        Expr::Ok(inner, _) | Expr::Err(inner, _) => expr_type(inner, ctx),
        Expr::MatchResult { ok_body, .. } => ok_body.last().map_or(Type::Void, |e| expr_type(e, ctx)),
        Expr::Op { op, args, .. } => match op {
            OpCode::Add
            | OpCode::Sub
            | OpCode::Mul
            | OpCode::Div
            | OpCode::Mod
            | OpCode::BitXor
            | OpCode::Shl
            | OpCode::Shr
            | OpCode::ShrU
            | OpCode::DivU
            | OpCode::RemU
            | OpCode::BitAnd
            | OpCode::BitOr => args.first().map_or(Type::I32, |a| expr_type(a, ctx)),
            OpCode::Eq
            | OpCode::Neq
            | OpCode::Lt
            | OpCode::Lte
            | OpCode::Gt
            | OpCode::Gte
            | OpCode::And
            | OpCode::Or
            | OpCode::Not
            | OpCode::AtomicCas => Type::Bool,
            OpCode::MemLoad64 | OpCode::I64ExtendS | OpCode::I64ExtendU => Type::I64,
            OpCode::MemLoadF32 => Type::F32,
            OpCode::MemLoadF64 | OpCode::SysTime => Type::F64,
            OpCode::MemStore8
            | OpCode::MemStore32
            | OpCode::MemStore64
            | OpCode::MemStoreF32
            | OpCode::MemStoreF64
            | OpCode::MemFree
            | OpCode::AtomicLock
            | OpCode::AtomicUnlock
            | OpCode::ArrSet
            | OpCode::SysPrint
            | OpCode::SysExit => Type::Void,
            OpCode::MemLoad8
            | OpCode::MemLoad32
            | OpCode::MemAlloc
            | OpCode::AtomicAdd
            | OpCode::ArrGet
            | OpCode::I32Wrap
            | OpCode::FsOpen
            | OpCode::FsRead
            | OpCode::FsWrite
            | OpCode::FsClose
            | OpCode::FsDelete
            | OpCode::ThreadSpawn
            | OpCode::ThreadJoin => Type::I32,
        },
    }
}

/// Picks the wasm instruction for a binary arithmetic/bitwise op given the
/// static operand type. Returns an error for combinations wasm has no single
/// instruction for (e.g. `%` on floats) rather than silently emitting i32 code.
fn arith_instruction(op: &OpCode, ty: &Type) -> Result<Instruction<'static>, String> {
    use Instruction::*;
    let ins = match (op, ty) {
        (OpCode::Add, Type::I32) => I32Add,
        (OpCode::Sub, Type::I32) => I32Sub,
        (OpCode::Mul, Type::I32) => I32Mul,
        (OpCode::Div, Type::I32) => I32DivS,
        (OpCode::Mod, Type::I32) => I32RemS,
        (OpCode::DivU, Type::I32) => I32DivU,
        (OpCode::RemU, Type::I32) => I32RemU,
        (OpCode::BitXor, Type::I32) => I32Xor,
        (OpCode::BitAnd, Type::I32) => I32And,
        (OpCode::BitOr, Type::I32) => I32Or,
        (OpCode::Shl, Type::I32) => I32Shl,
        (OpCode::Shr, Type::I32) => I32ShrS,
        (OpCode::ShrU, Type::I32) => I32ShrU,

        (OpCode::Add, Type::I64) => I64Add,
        (OpCode::Sub, Type::I64) => I64Sub,
        (OpCode::Mul, Type::I64) => I64Mul,
        (OpCode::Div, Type::I64) => I64DivS,
        (OpCode::Mod, Type::I64) => I64RemS,
        (OpCode::DivU, Type::I64) => I64DivU,
        (OpCode::RemU, Type::I64) => I64RemU,
        (OpCode::BitXor, Type::I64) => I64Xor,
        (OpCode::BitAnd, Type::I64) => I64And,
        (OpCode::BitOr, Type::I64) => I64Or,
        (OpCode::Shl, Type::I64) => I64Shl,
        (OpCode::Shr, Type::I64) => I64ShrS,
        (OpCode::ShrU, Type::I64) => I64ShrU,

        (OpCode::Add, Type::F64) => F64Add,
        (OpCode::Sub, Type::F64) => F64Sub,
        (OpCode::Mul, Type::F64) => F64Mul,
        (OpCode::Div, Type::F64) => F64Div,
        (OpCode::Add, Type::F32) => F32Add,
        (OpCode::Sub, Type::F32) => F32Sub,
        (OpCode::Mul, Type::F32) => F32Mul,
        (OpCode::Div, Type::F32) => F32Div,

        _ => {
            return Err(format!(
                "Wasm Codegen: {:?} is not supported for operands of type {:?}",
                op, ty
            ))
        }
    };
    Ok(ins)
}

/// Picks the wasm comparison instruction for the static operand type. All of
/// these leave an i32 0/1 on the stack, which is how AIPL `bool` is represented.
fn compare_instruction(op: &OpCode, ty: &Type) -> Result<Instruction<'static>, String> {
    use Instruction::*;
    let ins = match (op, ty) {
        // bool and str are i32 in wasm (str is a placeholder 0 today).
        (OpCode::Eq, Type::I32 | Type::Bool | Type::Str) => I32Eq,
        (OpCode::Neq, Type::I32 | Type::Bool | Type::Str) => I32Ne,
        (OpCode::Lt, Type::I32) => I32LtS,
        (OpCode::Lte, Type::I32) => I32LeS,
        (OpCode::Gt, Type::I32) => I32GtS,
        (OpCode::Gte, Type::I32) => I32GeS,

        (OpCode::Eq, Type::I64) => I64Eq,
        (OpCode::Neq, Type::I64) => I64Ne,
        (OpCode::Lt, Type::I64) => I64LtS,
        (OpCode::Lte, Type::I64) => I64LeS,
        (OpCode::Gt, Type::I64) => I64GtS,
        (OpCode::Gte, Type::I64) => I64GeS,

        (OpCode::Eq, Type::F64) => F64Eq,
        (OpCode::Neq, Type::F64) => F64Ne,
        (OpCode::Lt, Type::F64) => F64Lt,
        (OpCode::Lte, Type::F64) => F64Le,
        (OpCode::Gt, Type::F64) => F64Gt,
        (OpCode::Gte, Type::F64) => F64Ge,

        (OpCode::Eq, Type::F32) => F32Eq,
        (OpCode::Neq, Type::F32) => F32Ne,
        (OpCode::Lt, Type::F32) => F32Lt,
        (OpCode::Lte, Type::F32) => F32Le,
        (OpCode::Gt, Type::F32) => F32Gt,
        (OpCode::Gte, Type::F32) => F32Ge,

        _ => {
            return Err(format!(
                "Wasm Codegen: {:?} is not supported for operands of type {:?}",
                op, ty
            ))
        }
    };
    Ok(ins)
}

fn collect_lets(exprs: &[Expr], lets: &mut Vec<(String, Type)>) {
    for expr in exprs {
        match expr {
            Expr::Let { name, ty, val, .. } => {
                lets.push((name.clone(), ty.clone()));
                collect_lets(&[*(val.clone())], lets);
            }
            Expr::Set { name: _, val, .. } => {
                collect_lets(&[*(val.clone())], lets);
            }
            Expr::If { cond, then_branch, else_branch, .. } => {
                collect_lets(&[*(cond.clone()), *(then_branch.clone()), *(else_branch.clone())], lets);
            }
            // The loop induction variable is never declared via `let` but still
            // needs a wasm local slot - without this, codegen silently drops
            // the whole loop body (see Expr::Loop in compile_expr).
            Expr::Loop { var, body, .. } => {
                lets.push((var.clone(), Type::I32));
                collect_lets(body, lets);
            }
            Expr::While { body, .. } | Expr::Block(body, _) => {
                collect_lets(body, lets);
            }
            Expr::Call { args, .. } => {
                collect_lets(args, lets);
            }
            Expr::MatchResult { expr, ok_body, err_body, .. } => {
                collect_lets(&[*(expr.clone())], lets);
                collect_lets(ok_body, lets);
                collect_lets(err_body, lets);
            }
            Expr::Ok(inner, _) | Expr::Err(inner, _) => {
                collect_lets(&[*(inner.clone())], lets);
            }
            _ => {}
        }
    }
}

fn aipl_to_wasm_type(ty: &Type) -> ValType {
    match ty {
        Type::I32 | Type::Bool | Type::Str | Type::Void => ValType::I32,
        Type::I64 => ValType::I64,
        Type::F32 => ValType::F32,
        Type::F64 => ValType::F64,
        Type::Ptr(_)
        | Type::ResultType(_, _)
        | Type::Array(_, _)
        | Type::Vector(_, _)
        | Type::Fn(_, _) => ValType::I32,
    }
}

/// Compiles a statement that appears in a purely-effectful position (a
/// non-final entry in a block/function body, or any statement in a while/loop
/// body): the statement runs, and any value it leaves behind is dropped so it
/// never corrupts the surrounding block's stack balance.
fn compile_stmt(expr: &Expr, ctx: &Ctx, func: &mut Function) -> Result<(), String> {
    compile_expr(expr, ctx, func)?;
    if !is_void_expr(expr, ctx) {
        func.instruction(&Instruction::Drop);
    }
    Ok(())
}

fn compile_expr(expr: &Expr, ctx: &Ctx, func: &mut Function) -> Result<(), String> {
    match expr {
        Expr::Lit(lit, _) => match lit {
            Literal::Int(i) => {
                func.instruction(&Instruction::I32Const(*i as i32));
            }
            Literal::Int64(i) => {
                func.instruction(&Instruction::I64Const(*i));
            }
            Literal::Float(f) => {
                func.instruction(&Instruction::F64Const(*f));
            }
            Literal::Bool(b) => {
                func.instruction(&Instruction::I32Const(if *b { 1 } else { 0 }));
            }
            Literal::Str(_) => {
                func.instruction(&Instruction::I32Const(0));
            }
        },
        Expr::Var(name, _) => {
            if let Some(&idx) = ctx.locals.get(name) {
                func.instruction(&Instruction::LocalGet(idx));
            } else {
                return Err(format!("Wasm Codegen: Unbound local variable '{}'", name));
            }
        }
        Expr::Let { name, val, .. } => {
            compile_expr(val, ctx, func)?;
            if let Some(&idx) = ctx.locals.get(name) {
                func.instruction(&Instruction::LocalSet(idx));
            }
        }
        Expr::Set { name, val, .. } => {
            compile_expr(val, ctx, func)?;
            if let Some(&idx) = ctx.locals.get(name) {
                func.instruction(&Instruction::LocalSet(idx));
            }
        }
        Expr::If { cond, then_branch, else_branch, .. } => {
            compile_expr(cond, ctx, func)?;
            // The checker guarantees both branches share one type; use it for
            // the block result so i64/f64-valued ifs validate (not always I32).
            let block_ty = if is_void_expr(then_branch, ctx) {
                wasm_encoder::BlockType::Empty
            } else {
                wasm_encoder::BlockType::Result(aipl_to_wasm_type(&expr_type(then_branch, ctx)))
            };
            func.instruction(&Instruction::If(block_ty));
            compile_expr(then_branch, ctx, func)?;
            func.instruction(&Instruction::Else);
            compile_expr(else_branch, ctx, func)?;
            func.instruction(&Instruction::End);
        }
        Expr::Call { func: f_name, args, .. } => {
            for arg in args {
                compile_expr(arg, ctx, func)?;
            }
            if let Some(&idx) = ctx.fn_indices.get(f_name) {
                func.instruction(&Instruction::Call(idx));
            } else {
                return Err(format!("Wasm Codegen: Call to unknown function '{}'", f_name));
            }
        }
        Expr::Op { op, args, .. } => match op {
            OpCode::Add
            | OpCode::Sub
            | OpCode::Mul
            | OpCode::Div
            | OpCode::Mod
            | OpCode::BitXor
            | OpCode::Shl
            | OpCode::Shr
            | OpCode::ShrU
            | OpCode::DivU
            | OpCode::RemU
            | OpCode::BitAnd
            | OpCode::BitOr => {
                // The checker guarantees both operands share one type, so the
                // first operand decides the instruction width (i32/i64/f32/f64).
                let ty = expr_type(&args[0], ctx);
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&arith_instruction(op, &ty)?);
            }
            OpCode::MemLoad8 => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::I32Load8U(wasm_encoder::MemArg { offset: 0, align: 0, memory_index: 0 }));
            }
            OpCode::MemStore8 => {
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I32Store8(wasm_encoder::MemArg { offset: 0, align: 0, memory_index: 0 }));
            }
            OpCode::MemLoad32 => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::I32Load(wasm_encoder::MemArg { offset: 0, align: 2, memory_index: 0 }));
            }
            OpCode::MemStore32 => {
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I32Store(wasm_encoder::MemArg { offset: 0, align: 2, memory_index: 0 }));
            }
            OpCode::MemLoad64 => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::I64Load(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
            }
            OpCode::MemStore64 => {
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
            }
            OpCode::MemAlloc => {
                func.instruction(&Instruction::GlobalGet(0));
                func.instruction(&Instruction::GlobalGet(0));
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::I32Add);
                func.instruction(&Instruction::GlobalSet(0));
            }
            OpCode::MemFree => {
                // No-op for bump allocator
            }
            OpCode::Eq | OpCode::Neq | OpCode::Lt | OpCode::Lte | OpCode::Gt | OpCode::Gte => {
                let ty = expr_type(&args[0], ctx);
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&compare_instruction(op, &ty)?);
            }
            OpCode::And => {
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I32And);
            }
            OpCode::Or => {
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I32Or);
            }
            OpCode::Not => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::I32Eqz);
            }
            OpCode::I64ExtendS => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::I64ExtendI32S);
            }
            OpCode::I64ExtendU => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::I64ExtendI32U);
            }
            OpCode::I32Wrap => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::I32WrapI64);
            }
            OpCode::SysPrint => {
                return Err("sys.print not supported in wasm backend: comes with WASI in P6".to_string());
            }
            OpCode::MemLoadF32 | OpCode::MemLoadF64 | OpCode::MemStoreF32 | OpCode::MemStoreF64 => {
                return Err(format!("Wasm Codegen: {:?} is not supported in the wasm backend", op));
            }
            OpCode::AtomicAdd | OpCode::AtomicCas | OpCode::AtomicLock | OpCode::AtomicUnlock => {
                return Err(format!("Wasm Codegen: {:?} is not supported in the wasm backend (needs shared memory + atomics)", op));
            }
            OpCode::ArrGet | OpCode::ArrSet => {
                return Err(format!("Wasm Codegen: {:?} is not supported in the wasm backend", op));
            }
            OpCode::SysTime | OpCode::SysExit => {
                return Err(format!("Wasm Codegen: {:?} is not supported in the wasm backend", op));
            }
            OpCode::FsOpen | OpCode::FsRead | OpCode::FsWrite | OpCode::FsClose | OpCode::FsDelete => {
                return Err(format!(
                    "Wasm Codegen: {:?} is not yet supported in the wasm backend (needs WASI file I/O imports)",
                    op
                ));
            }
            OpCode::ThreadSpawn | OpCode::ThreadJoin => {
                return Err(format!(
                    "Wasm Codegen: {:?} is not yet supported in the wasm backend (needs shared memory + wasi-threads)",
                    op
                ));
            }
        },
        Expr::Block(exprs, _) => {
            let len = exprs.len();
            for (i, e) in exprs.iter().enumerate() {
                if i + 1 == len {
                    compile_expr(e, ctx, func)?;
                } else {
                    compile_stmt(e, ctx, func)?;
                }
            }
        }
        Expr::While { cond, body, .. } => {
            func.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty));
            func.instruction(&Instruction::Loop(wasm_encoder::BlockType::Empty));
            compile_expr(cond, ctx, func)?;
            func.instruction(&Instruction::I32Eqz);
            func.instruction(&Instruction::BrIf(1));
            for e in body {
                compile_stmt(e, ctx, func)?;
            }
            func.instruction(&Instruction::Br(0));
            func.instruction(&Instruction::End);
            func.instruction(&Instruction::End);
        }
        Expr::Loop { var, start, end, step, body, .. } => {
            let var_idx = *ctx
                .locals
                .get(var)
                .ok_or_else(|| format!("Wasm Codegen: loop variable '{}' has no local slot", var))?;
            compile_expr(start, ctx, func)?;
            func.instruction(&Instruction::LocalSet(var_idx));
            func.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty));
            func.instruction(&Instruction::Loop(wasm_encoder::BlockType::Empty));
            func.instruction(&Instruction::LocalGet(var_idx));
            compile_expr(end, ctx, func)?;
            // VM semantics (vm.rs) run the loop `while curr <= end` - inclusive
            // of the end bound. This must use I32GtS (exit only once the
            // counter exceeds end), not I32GeS, or a wasm-compiled loop runs
            // one fewer iteration than the same source does in the VM.
            func.instruction(&Instruction::I32GtS);
            func.instruction(&Instruction::BrIf(1));
            for e in body {
                compile_stmt(e, ctx, func)?;
            }
            func.instruction(&Instruction::LocalGet(var_idx));
            compile_expr(step, ctx, func)?;
            func.instruction(&Instruction::I32Add);
            func.instruction(&Instruction::LocalSet(var_idx));
            func.instruction(&Instruction::Br(0));
            func.instruction(&Instruction::End);
            func.instruction(&Instruction::End);
        }
        Expr::Ok(inner, _) => {
            compile_expr(inner, ctx, func)?;
        }
        Expr::Err(inner, _) => {
            compile_expr(inner, ctx, func)?;
        }
        Expr::MatchResult { .. } => {
            return Err("Wasm Codegen: MatchResult is not supported in the wasm backend".to_string());
        }
    }
    Ok(())
}

/// True if `expr`, as actually compiled by `compile_expr` above, leaves
/// nothing on the wasm value stack - used to decide `if` block result types
/// and whether a statement-position value needs an explicit `drop`. This must
/// track the real codegen above, not the AIPL-level type system: e.g. `set!`
/// has a non-void AIPL type but its codegen never leaves a value.
fn is_void_expr(expr: &Expr, ctx: &Ctx) -> bool {
    match expr {
        Expr::Set { .. } | Expr::Let { .. } => true,
        Expr::Block(exprs, _) => exprs.last().map_or(true, |e| is_void_expr(e, ctx)),
        // An if/else is void only if BOTH branches are void - if they disagreed,
        // whichever branch actually produced a value would leave the wasm value
        // stack unbalanced relative to this if's declared block type.
        Expr::If { then_branch, else_branch, .. } => {
            is_void_expr(then_branch, ctx) && is_void_expr(else_branch, ctx)
        }
        Expr::While { .. } | Expr::Loop { .. } => true,
        Expr::Call { func, .. } => ctx.fn_returns.get(func).map_or(false, |t| *t == Type::Void),
        Expr::Op { op, .. } => !matches!(
            op,
            OpCode::Add
                | OpCode::Sub
                | OpCode::Mul
                | OpCode::Div
                | OpCode::Mod
                | OpCode::BitXor
                | OpCode::Shl
                | OpCode::Shr
                | OpCode::ShrU
                | OpCode::DivU
                | OpCode::RemU
                | OpCode::BitAnd
                | OpCode::BitOr
                | OpCode::MemLoad8
                | OpCode::MemLoad32
                | OpCode::MemLoad64
                | OpCode::MemAlloc
                | OpCode::I64ExtendS
                | OpCode::I64ExtendU
                | OpCode::I32Wrap
                | OpCode::Eq
                | OpCode::Neq
                | OpCode::Lt
                | OpCode::Lte
                | OpCode::Gt
                | OpCode::Gte
                | OpCode::And
                | OpCode::Or
                | OpCode::Not
        ),
        _ => false,
    }
}
