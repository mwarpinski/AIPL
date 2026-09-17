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

            for expr in &f.body {
                self::compile_expr(expr, &local_map, &fn_indices, &mut func)?;
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
            Expr::Loop { body, .. } | Expr::While { body, .. } | Expr::Block(body) => {
                collect_lets(body, lets);
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

fn compile_expr(
    expr: &Expr,
    locals: &HashMap<String, u32>,
    fn_indices: &HashMap<String, u32>,
    func: &mut Function,
) -> Result<(), String> {
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
            if let Some(&idx) = locals.get(name) {
                func.instruction(&Instruction::LocalGet(idx));
            } else {
                return Err(format!("Wasm Codegen: Unbound local variable '{}'", name));
            }
        }
        Expr::Let { name, ty: _, val } => {
            compile_expr(val, locals, fn_indices, func)?;
            if let Some(&idx) = locals.get(name) {
                func.instruction(&Instruction::LocalSet(idx));
            }
        }
        Expr::Set { name, val } => {
            compile_expr(val, locals, fn_indices, func)?;
            if let Some(&idx) = locals.get(name) {
                func.instruction(&Instruction::LocalSet(idx));
            }
        }
        Expr::If { cond, then_branch, else_branch } => {
            compile_expr(cond, locals, fn_indices, func)?;
            let block_ty = if is_void_expr(then_branch) {
                wasm_encoder::BlockType::Empty
            } else {
                wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)
            };
            func.instruction(&Instruction::If(block_ty));
            compile_expr(then_branch, locals, fn_indices, func)?;
            func.instruction(&Instruction::Else);
            compile_expr(else_branch, locals, fn_indices, func)?;
            func.instruction(&Instruction::End);
        }
        Expr::Call { func: f_name, args } => {
            for arg in args {
                compile_expr(arg, locals, fn_indices, func)?;
            }
            if let Some(&idx) = fn_indices.get(f_name) {
                func.instruction(&Instruction::Call(idx));
            } else {
                return Err(format!("Wasm Codegen: Call to unknown function '{}'", f_name));
            }
        }
        Expr::Op { op, args } => match op {
            OpCode::Add => {
                compile_expr(&args[0], locals, fn_indices, func)?;
                compile_expr(&args[1], locals, fn_indices, func)?;
                func.instruction(&Instruction::I32Add);
            }
            OpCode::Sub => {
                compile_expr(&args[0], locals, fn_indices, func)?;
                compile_expr(&args[1], locals, fn_indices, func)?;
                func.instruction(&Instruction::I32Sub);
            }
            OpCode::Mul => {
                compile_expr(&args[0], locals, fn_indices, func)?;
                compile_expr(&args[1], locals, fn_indices, func)?;
                func.instruction(&Instruction::I32Mul);
            }
            OpCode::Div => {
                compile_expr(&args[0], locals, fn_indices, func)?;
                compile_expr(&args[1], locals, fn_indices, func)?;
                func.instruction(&Instruction::I32DivS);
            }
            OpCode::BitXor => {
                compile_expr(&args[0], locals, fn_indices, func)?;
                compile_expr(&args[1], locals, fn_indices, func)?;
                func.instruction(&Instruction::I32Xor);
            }
            OpCode::Shl => {
                compile_expr(&args[0], locals, fn_indices, func)?;
                compile_expr(&args[1], locals, fn_indices, func)?;
                func.instruction(&Instruction::I32Shl);
            }
            OpCode::Shr => {
                compile_expr(&args[0], locals, fn_indices, func)?;
                compile_expr(&args[1], locals, fn_indices, func)?;
                func.instruction(&Instruction::I32ShrS);
            }
            OpCode::BitAnd => {
                compile_expr(&args[0], locals, fn_indices, func)?;
                compile_expr(&args[1], locals, fn_indices, func)?;
                func.instruction(&Instruction::I32And);
            }
            OpCode::BitOr => {
                compile_expr(&args[0], locals, fn_indices, func)?;
                compile_expr(&args[1], locals, fn_indices, func)?;
                func.instruction(&Instruction::I32Or);
            }
            OpCode::MemLoad32 => {
                compile_expr(&args[0], locals, fn_indices, func)?;
                func.instruction(&Instruction::I32Load(wasm_encoder::MemArg { offset: 0, align: 2, memory_index: 0 }));
            }
            OpCode::MemStore32 => {
                compile_expr(&args[0], locals, fn_indices, func)?;
                compile_expr(&args[1], locals, fn_indices, func)?;
                func.instruction(&Instruction::I32Store(wasm_encoder::MemArg { offset: 0, align: 2, memory_index: 0 }));
            }
            OpCode::MemLoad64 => {
                compile_expr(&args[0], locals, fn_indices, func)?;
                func.instruction(&Instruction::I64Load(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
            }
            OpCode::MemStore64 => {
                compile_expr(&args[0], locals, fn_indices, func)?;
                compile_expr(&args[1], locals, fn_indices, func)?;
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
            }
            OpCode::Eq => {
                compile_expr(&args[0], locals, fn_indices, func)?;
                compile_expr(&args[1], locals, fn_indices, func)?;
                func.instruction(&Instruction::I32Eq);
            }
            OpCode::Neq => {
                compile_expr(&args[0], locals, fn_indices, func)?;
                compile_expr(&args[1], locals, fn_indices, func)?;
                func.instruction(&Instruction::I32Ne);
            }
            OpCode::Lt => {
                compile_expr(&args[0], locals, fn_indices, func)?;
                compile_expr(&args[1], locals, fn_indices, func)?;
                func.instruction(&Instruction::I32LtS);
            }
            OpCode::Lte => {
                compile_expr(&args[0], locals, fn_indices, func)?;
                compile_expr(&args[1], locals, fn_indices, func)?;
                func.instruction(&Instruction::I32LeS);
            }
            OpCode::Gt => {
                compile_expr(&args[0], locals, fn_indices, func)?;
                compile_expr(&args[1], locals, fn_indices, func)?;
                func.instruction(&Instruction::I32GtS);
            }
            OpCode::Gte => {
                compile_expr(&args[0], locals, fn_indices, func)?;
                compile_expr(&args[1], locals, fn_indices, func)?;
                func.instruction(&Instruction::I32GeS);
            }
            OpCode::And => {
                compile_expr(&args[0], locals, fn_indices, func)?;
                compile_expr(&args[1], locals, fn_indices, func)?;
                func.instruction(&Instruction::I32And);
            }
            OpCode::Or => {
                compile_expr(&args[0], locals, fn_indices, func)?;
                compile_expr(&args[1], locals, fn_indices, func)?;
                func.instruction(&Instruction::I32Or);
            }
            OpCode::Not => {
                compile_expr(&args[0], locals, fn_indices, func)?;
                func.instruction(&Instruction::I32Eqz);
            }
            _ => {
                func.instruction(&Instruction::Nop);
            }
        },
        Expr::Block(exprs) => {
            for e in exprs {
                compile_expr(e, locals, fn_indices, func)?;
            }
        }
        Expr::While { cond, body } => {
            func.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty));
            func.instruction(&Instruction::Loop(wasm_encoder::BlockType::Empty));
            compile_expr(cond, locals, fn_indices, func)?;
            func.instruction(&Instruction::I32Eqz);
            func.instruction(&Instruction::BrIf(1));
            for e in body {
                compile_expr(e, locals, fn_indices, func)?;
            }
            func.instruction(&Instruction::Br(0));
            func.instruction(&Instruction::End);
            func.instruction(&Instruction::End);
        }
        Expr::Loop { var, start, end, step, body } => {
            compile_expr(start, locals, fn_indices, func)?;
            if let Some(&var_idx) = locals.get(var) {
                func.instruction(&Instruction::LocalSet(var_idx));
                func.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty));
                func.instruction(&Instruction::Loop(wasm_encoder::BlockType::Empty));
                func.instruction(&Instruction::LocalGet(var_idx));
                compile_expr(end, locals, fn_indices, func)?;
                func.instruction(&Instruction::I32GeS);
                func.instruction(&Instruction::BrIf(1));
                for e in body {
                    compile_expr(e, locals, fn_indices, func)?;
                }
                func.instruction(&Instruction::LocalGet(var_idx));
                compile_expr(step, locals, fn_indices, func)?;
                func.instruction(&Instruction::I32Add);
                func.instruction(&Instruction::LocalSet(var_idx));
                func.instruction(&Instruction::Br(0));
                func.instruction(&Instruction::End);
                func.instruction(&Instruction::End);
            }
        }
        _ => {
            func.instruction(&Instruction::Nop);
        }
    }
    Ok(())
}

fn is_void_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Set { .. } => true,
        Expr::Block(exprs) => exprs.last().map_or(true, is_void_expr),
        Expr::Op { op, .. } => match op {
            OpCode::MemStore32 | OpCode::MemStore64 | OpCode::MemStoreF32 | OpCode::MemStoreF64
            | OpCode::MemFree | OpCode::AtomicLock | OpCode::AtomicUnlock => true,
            _ => false,
        },
        _ => false,
    }
}
