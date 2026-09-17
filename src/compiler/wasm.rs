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

            let mut wasm_locals = Vec::new();
            for (l_name, l_ty) in &extra_lets {
                if !local_map.contains_key(l_name) {
                    local_map.insert(l_name.clone(), current_idx);
                    wasm_locals.push((1, aipl_to_wasm_type(l_ty)));
                    current_idx += 1;
                }
            }

            let mut func = Function::new(wasm_locals);
            let ctx = Ctx { locals: &local_map, fn_indices: &fn_indices, fn_returns: &fn_returns };

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

        wasm_module.section(&types);
        wasm_module.section(&functions);
        wasm_module.section(&memories);
        wasm_module.section(&exports);
        wasm_module.section(&codes);

        Ok(wasm_module.finish())
    }
}

/// Bundles the read-only context threaded through codegen so it isn't passed
/// as four separate parameters everywhere.
struct Ctx<'a> {
    locals: &'a HashMap<String, u32>,
    fn_indices: &'a HashMap<String, u32>,
    fn_returns: &'a HashMap<String, Type>,
}

fn collect_lets(exprs: &[Expr], lets: &mut Vec<(String, Type)>) {
    for expr in exprs {
        match expr {
            Expr::Let { name, ty, val } => {
                lets.push((name.clone(), ty.clone()));
                collect_lets(&[*(val.clone())], lets);
            }
            Expr::Set { name: _, val } => {
                collect_lets(&[*(val.clone())], lets);
            }
            Expr::If { cond, then_branch, else_branch } => {
                collect_lets(&[*(cond.clone()), *(then_branch.clone()), *(else_branch.clone())], lets);
            }
            // The loop induction variable is never declared via `let` but still
            // needs a wasm local slot - without this, codegen silently drops
            // the whole loop body (see Expr::Loop in compile_expr).
            Expr::Loop { var, body, .. } => {
                lets.push((var.clone(), Type::I32));
                collect_lets(body, lets);
            }
            Expr::While { body, .. } | Expr::Block(body) => {
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
            Expr::Ok(inner) | Expr::Err(inner) => {
                collect_lets(&[*(inner.clone())], lets);
            }
            _ => {}
        }
    }
}

fn aipl_to_wasm_type(ty: &Type) -> ValType {
    match ty {
        Type::I32 | Type::Bool => ValType::I32,
        Type::I64 => ValType::I64,
        Type::F32 => ValType::F32,
        Type::F64 => ValType::F64,
        _ => ValType::I32,
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
        Expr::Lit(lit) => match lit {
            Literal::Int(i) => {
                func.instruction(&Instruction::I32Const(*i as i32));
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
        Expr::Var(name) => {
            if let Some(&idx) = ctx.locals.get(name) {
                func.instruction(&Instruction::LocalGet(idx));
            } else {
                return Err(format!("Wasm Codegen: Unbound local variable '{}'", name));
            }
        }
        Expr::Let { name, ty: _, val } => {
            compile_expr(val, ctx, func)?;
            if let Some(&idx) = ctx.locals.get(name) {
                func.instruction(&Instruction::LocalSet(idx));
            }
        }
        Expr::Set { name, val } => {
            compile_expr(val, ctx, func)?;
            if let Some(&idx) = ctx.locals.get(name) {
                func.instruction(&Instruction::LocalSet(idx));
            }
        }
        Expr::If { cond, then_branch, else_branch } => {
            compile_expr(cond, ctx, func)?;
            let block_ty = if is_void_expr(then_branch, ctx) {
                wasm_encoder::BlockType::Empty
            } else {
                wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)
            };
            func.instruction(&Instruction::If(block_ty));
            compile_expr(then_branch, ctx, func)?;
            func.instruction(&Instruction::Else);
            compile_expr(else_branch, ctx, func)?;
            func.instruction(&Instruction::End);
        }
        Expr::Call { func: f_name, args } => {
            for arg in args {
                compile_expr(arg, ctx, func)?;
            }
            if let Some(&idx) = ctx.fn_indices.get(f_name) {
                func.instruction(&Instruction::Call(idx));
            } else {
                return Err(format!("Wasm Codegen: Call to unknown function '{}'", f_name));
            }
        }
        Expr::Op { op, args } => match op {
            OpCode::Add => {
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I32Add);
            }
            OpCode::Sub => {
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I32Sub);
            }
            OpCode::Mul => {
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I32Mul);
            }
            OpCode::Div => {
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I32DivS);
            }
            OpCode::BitXor => {
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I32Xor);
            }
            OpCode::Shl => {
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I32Shl);
            }
            OpCode::Shr => {
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I32ShrS);
            }
            OpCode::BitAnd => {
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I32And);
            }
            OpCode::BitOr => {
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I32Or);
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
            OpCode::Eq => {
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I32Eq);
            }
            OpCode::Neq => {
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I32Ne);
            }
            OpCode::Lt => {
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I32LtS);
            }
            OpCode::Lte => {
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I32LeS);
            }
            OpCode::Gt => {
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I32GtS);
            }
            OpCode::Gte => {
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I32GeS);
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
            _ => {
                func.instruction(&Instruction::Nop);
            }
        },
        Expr::Block(exprs) => {
            let len = exprs.len();
            for (i, e) in exprs.iter().enumerate() {
                if i + 1 == len {
                    compile_expr(e, ctx, func)?;
                } else {
                    compile_stmt(e, ctx, func)?;
                }
            }
        }
        Expr::While { cond, body } => {
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
        Expr::Loop { var, start, end, step, body } => {
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
            func.instruction(&Instruction::I32GeS);
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
        _ => {
            func.instruction(&Instruction::Nop);
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
        Expr::Block(exprs) => exprs.last().map_or(true, |e| is_void_expr(e, ctx)),
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
                | OpCode::BitXor
                | OpCode::Shl
                | OpCode::Shr
                | OpCode::BitAnd
                | OpCode::BitOr
                | OpCode::MemLoad8
                | OpCode::MemLoad32
                | OpCode::MemLoad64
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
