use crate::ast::*;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct TypeChecker {
    fn_signatures: HashMap<String, (Vec<Type>, Type)>,
}

impl TypeChecker {
    pub fn new() -> Self {
        TypeChecker {
            fn_signatures: HashMap::new(),
        }
    }

    pub fn check_module(&mut self, module: &Module) -> Result<(), String> {
        // First pass: register function signatures
        for f in &module.functions {
            let param_types: Vec<Type> = f.params.iter().map(|(_, t)| t.clone()).collect();
            self.fn_signatures.insert(f.name.clone(), (param_types, f.return_type.clone()));
        }

        // Second pass: type check bodies and verify contracts
        for f in &module.functions {
            self.check_fn_def(f)?;
        }

        Ok(())
    }

    fn check_fn_def(&self, f: &FnDef) -> Result<(), String> {
        let mut env = HashMap::new();
        for (param_name, param_ty) in &f.params {
            env.insert(param_name.clone(), param_ty.clone());
        }

        // Check contracts
        for c in &f.contracts {
            match c {
                Contract::Requires(expr) | Contract::Invariant(expr) => {
                    let cond_ty = self.infer_expr_type(expr, &mut env)?;
                    if cond_ty != Type::Bool {
                        return Err(format!("Contract expression in '{}' must evaluate to Bool, got {:?}", f.name, cond_ty));
                    }
                }
                Contract::Ensures(expr) => {
                    let mut ens_env = env.clone();
                    ens_env.insert("res".to_string(), f.return_type.clone());
                    let cond_ty = self.infer_expr_type(expr, &mut ens_env)?;
                    if cond_ty != Type::Bool {
                        return Err(format!("Ensures contract expression in '{}' must evaluate to Bool, got {:?}", f.name, cond_ty));
                    }
                }
            }
        }

        // Check function body expressions
        let mut last_ty = Type::Void;
        for expr in &f.body {
            last_ty = self.infer_expr_type(expr, &mut env)?;
        }

        if f.return_type != Type::Void && last_ty != f.return_type {
            return Err(format!(
                "Function '{}' expects return type {:?}, but body returned {:?}",
                f.name, f.return_type, last_ty
            ));
        }

        Ok(())
    }

    fn infer_expr_type(&self, expr: &Expr, env: &mut HashMap<String, Type>) -> Result<Type, String> {
        match expr {
            Expr::Lit(lit) => match lit {
                Literal::Int(_) => Ok(Type::I32),
                Literal::Float(_) => Ok(Type::F64),
                Literal::Bool(_) => Ok(Type::Bool),
                Literal::Str(_) => Ok(Type::Str),
            },
            Expr::Var(name) => {
                if let Some(ty) = env.get(name) {
                    Ok(ty.clone())
                } else {
                    Err(format!("Undefined variable '{}'", name))
                }
            }
            Expr::Let { name, ty, val } => {
                let val_ty = self.infer_expr_type(val, env)?;
                if val_ty != *ty {
                    return Err(format!("Type mismatch in 'let': expected {:?}, got {:?}", ty, val_ty));
                }
                env.insert(name.clone(), ty.clone());
                Ok(ty.clone())
            }
            Expr::Set { name, val } => {
                let var_ty = env
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("Undefined variable '{}' in set!", name))?;
                let val_ty = self.infer_expr_type(val, env)?;
                if var_ty != val_ty {
                    return Err(format!("Type mismatch in 'set!': variable is {:?}, value is {:?}", var_ty, val_ty));
                }
                Ok(var_ty)
            }
            Expr::If { cond, then_branch, else_branch } => {
                let cond_ty = self.infer_expr_type(cond, env)?;
                if cond_ty != Type::Bool {
                    return Err(format!("If condition must be Bool, got {:?}", cond_ty));
                }
                let then_ty = self.infer_expr_type(then_branch, env)?;
                let else_ty = self.infer_expr_type(else_branch, env)?;
                if then_ty != else_ty {
                    return Err(format!("If branch type mismatch: then is {:?}, else is {:?}", then_ty, else_ty));
                }
                Ok(then_ty)
            }
            Expr::Loop { var, start, end, step, body } => {
                let start_ty = self.infer_expr_type(start, env)?;
                let end_ty = self.infer_expr_type(end, env)?;
                let step_ty = self.infer_expr_type(step, env)?;
                if start_ty != Type::I32 || end_ty != Type::I32 || step_ty != Type::I32 {
                    return Err("Loop bounds and step must be i32".to_string());
                }
                let mut local_env = env.clone();
                local_env.insert(var.clone(), Type::I32);
                for stmt in body {
                    self.infer_expr_type(stmt, &mut local_env)?;
                }
                Ok(Type::Void)
            }
            Expr::While { cond, body } => {
                let cond_ty = self.infer_expr_type(cond, env)?;
                if cond_ty != Type::Bool {
                    return Err("While condition must be Bool".to_string());
                }
                for stmt in body {
                    self.infer_expr_type(stmt, env)?;
                }
                Ok(Type::Void)
            }
            Expr::Call { func, args } => {
                let (param_types, ret_type) = self
                    .fn_signatures
                    .get(func)
                    .ok_or_else(|| format!("Call to unknown function '{}'", func))?;
                if args.len() != param_types.len() {
                    return Err(format!("Function '{}' expects {} arguments, got {}", func, param_types.len(), args.len()));
                }
                for (i, arg) in args.iter().enumerate() {
                    let arg_ty = self.infer_expr_type(arg, env)?;
                    if arg_ty != param_types[i] {
                        return Err(format!("Arg {} of '{}' expects {:?}, got {:?}", i, func, param_types[i], arg_ty));
                    }
                }
                Ok(ret_type.clone())
            }
            Expr::Op { op, args } => match op {
                OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Mod | OpCode::DivU | OpCode::RemU | OpCode::BitXor | OpCode::Shl | OpCode::Shr | OpCode::ShrU | OpCode::BitAnd | OpCode::BitOr => {
                    if args.len() != 2 {
                        return Err(format!("Arithmetic/bitwise opcode {:?} requires 2 arguments", op));
                    }
                    let t1 = self.infer_expr_type(&args[0], env)?;
                    let t2 = self.infer_expr_type(&args[1], env)?;
                    if t1 != t2 {
                        return Err(format!("Type mismatch in binary op: {:?} vs {:?}", t1, t2));
                    }
                    Ok(t1)
                }
                OpCode::MemLoad8 | OpCode::MemLoad32 => {
                    if args.len() != 1 {
                        return Err(format!("{:?} requires 1 argument (ptr: i32)", op));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("{:?} requires i32 ptr, got {:?}", op, t));
                    }
                    Ok(Type::I32)
                }
                OpCode::MemLoad64 => {
                    if args.len() != 1 {
                        return Err("mem.load64 requires 1 argument (ptr: i32)".to_string());
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("mem.load64 requires i32 ptr, got {:?}", t));
                    }
                    Ok(Type::I64)
                }
                OpCode::MemLoadF32 => {
                    if args.len() != 1 {
                        return Err("mem.load_f32 requires 1 argument (ptr: i32)".to_string());
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("mem.load_f32 requires i32 ptr, got {:?}", t));
                    }
                    Ok(Type::F32)
                }
                OpCode::MemLoadF64 => {
                    if args.len() != 1 {
                        return Err("mem.load_f64 requires 1 argument (ptr: i32)".to_string());
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("mem.load_f64 requires i32 ptr, got {:?}", t));
                    }
                    Ok(Type::F64)
                }
                OpCode::MemStore8 | OpCode::MemStore32 => {
                    if args.len() != 2 {
                        return Err(format!("{:?} requires 2 arguments (ptr: i32, val: i32)", op));
                    }
                    let t1 = self.infer_expr_type(&args[0], env)?;
                    let t2 = self.infer_expr_type(&args[1], env)?;
                    if t1 != Type::I32 || t2 != Type::I32 {
                        return Err(format!("{:?} requires (i32, i32), got ({:?}, {:?})", op, t1, t2));
                    }
                    Ok(Type::Void)
                }
                OpCode::MemStore64 => {
                    if args.len() != 2 {
                        return Err("mem.store64 requires 2 arguments (ptr: i32, val: i64)".to_string());
                    }
                    let t1 = self.infer_expr_type(&args[0], env)?;
                    let t2 = self.infer_expr_type(&args[1], env)?;
                    if t1 != Type::I32 || t2 != Type::I64 {
                        return Err(format!("mem.store64 requires (i32, i64), got ({:?}, {:?})", t1, t2));
                    }
                    Ok(Type::Void)
                }
                OpCode::MemStoreF32 => {
                    if args.len() != 2 {
                        return Err("mem.store_f32 requires 2 arguments (ptr: i32, val: f32)".to_string());
                    }
                    let t1 = self.infer_expr_type(&args[0], env)?;
                    let t2 = self.infer_expr_type(&args[1], env)?;
                    if t1 != Type::I32 || t2 != Type::F32 {
                        return Err(format!("mem.store_f32 requires (i32, f32), got ({:?}, {:?})", t1, t2));
                    }
                    Ok(Type::Void)
                }
                OpCode::MemStoreF64 => {
                    if args.len() != 2 {
                        return Err("mem.store_f64 requires 2 arguments (ptr: i32, val: f64)".to_string());
                    }
                    let t1 = self.infer_expr_type(&args[0], env)?;
                    let t2 = self.infer_expr_type(&args[1], env)?;
                    if t1 != Type::I32 || t2 != Type::F64 {
                        return Err(format!("mem.store_f64 requires (i32, f64), got ({:?}, {:?})", t1, t2));
                    }
                    Ok(Type::Void)
                }
                OpCode::MemAlloc => {
                    if args.len() != 1 {
                        return Err("mem.alloc requires 1 argument (size: i32)".to_string());
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("mem.alloc requires i32 size, got {:?}", t));
                    }
                    Ok(Type::I32)
                }
                OpCode::MemFree => {
                    if args.len() != 1 {
                        return Err("mem.free requires 1 argument (ptr: i32)".to_string());
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("mem.free requires i32 ptr, got {:?}", t));
                    }
                    Ok(Type::Void)
                }
                OpCode::AtomicAdd => Ok(Type::I32),
                OpCode::AtomicCas => Ok(Type::Bool),
                OpCode::AtomicLock | OpCode::AtomicUnlock => Ok(Type::Void),
                OpCode::Eq | OpCode::Neq | OpCode::Lt | OpCode::Lte | OpCode::Gt | OpCode::Gte => {
                    if args.len() != 2 {
                        return Err(format!("Comparison opcode {:?} requires 2 arguments", op));
                    }
                    let t1 = self.infer_expr_type(&args[0], env)?;
                    let t2 = self.infer_expr_type(&args[1], env)?;
                    if t1 != t2 {
                        return Err(format!("Type mismatch in comparison: {:?} vs {:?}", t1, t2));
                    }
                    Ok(Type::Bool)
                }
                OpCode::And | OpCode::Or => {
                    for arg in args {
                        let t = self.infer_expr_type(arg, env)?;
                        if t != Type::Bool {
                            return Err(format!("Logical op expects Bool, got {:?}", t));
                        }
                    }
                    Ok(Type::Bool)
                }
                OpCode::Not => {
                    if args.len() != 1 {
                        return Err("Not op expects 1 argument".to_string());
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::Bool {
                        return Err(format!("Not op expects Bool, got {:?}", t));
                    }
                    Ok(Type::Bool)
                }
                OpCode::ArrGet => {
                    if args.len() != 2 {
                        return Err("arr.get requires 2 arguments (arr, index)".to_string());
                    }
                    for arg in args {
                        self.infer_expr_type(arg, env)?;
                    }
                    Ok(Type::I32)
                }
                OpCode::ArrSet => {
                    if args.len() != 3 {
                        return Err("arr.set requires 3 arguments (arr, index, val)".to_string());
                    }
                    for arg in args {
                        self.infer_expr_type(arg, env)?;
                    }
                    Ok(Type::Void)
                }
                OpCode::SysPrint => Ok(Type::Void),
                OpCode::SysTime => Ok(Type::F64),
                OpCode::SysExit => Ok(Type::Void),
                OpCode::FsOpen | OpCode::FsRead | OpCode::FsWrite => {
                    if args.len() != 3 {
                        return Err(format!("{:?} requires 3 arguments", op));
                    }
                    for arg in args {
                        self.infer_expr_type(arg, env)?;
                    }
                    Ok(Type::I32)
                }
                OpCode::FsClose => {
                    if args.len() != 1 {
                        return Err("fs.close requires 1 argument".to_string());
                    }
                    self.infer_expr_type(&args[0], env)?;
                    Ok(Type::I32)
                }
                OpCode::FsDelete => {
                    if args.len() != 2 {
                        return Err("fs.delete requires 2 arguments (path_ptr, path_len)".to_string());
                    }
                    for arg in args {
                        self.infer_expr_type(arg, env)?;
                    }
                    Ok(Type::I32)
                }
                OpCode::ThreadSpawn => {
                    if args.len() != 3 {
                        return Err("thread.spawn requires 3 arguments (fn_name_ptr, fn_name_len, arg)".to_string());
                    }
                    for arg in args {
                        self.infer_expr_type(arg, env)?;
                    }
                    Ok(Type::I32)
                }
                OpCode::ThreadJoin => {
                    if args.len() != 1 {
                        return Err("thread.join requires 1 argument (thread handle)".to_string());
                    }
                    self.infer_expr_type(&args[0], env)?;
                    Ok(Type::I32)
                }
            },
            Expr::Ok(val) => {
                let inner_ty = self.infer_expr_type(val, env)?;
                Ok(Type::ResultType(Box::new(inner_ty), Box::new(Type::I32)))
            }
            Expr::Err(err) => {
                let err_ty = self.infer_expr_type(err, env)?;
                Ok(Type::ResultType(Box::new(Type::I32), Box::new(err_ty)))
            }
            Expr::MatchResult { expr, ok_var, ok_body, err_var, err_body } => {
                let _res_ty = self.infer_expr_type(expr, env)?;
                let mut ok_env = env.clone();
                ok_env.insert(ok_var.clone(), Type::I32);
                let mut last_ok_ty = Type::Void;
                for stmt in ok_body {
                    last_ok_ty = self.infer_expr_type(stmt, &mut ok_env)?;
                }

                let mut err_env = env.clone();
                err_env.insert(err_var.clone(), Type::I32);
                let mut _last_err_ty = Type::Void;
                for stmt in err_body {
                    _last_err_ty = self.infer_expr_type(stmt, &mut err_env)?;
                }

                Ok(last_ok_ty)
            }
            Expr::Block(exprs) => {
                let mut last_ty = Type::Void;
                for e in exprs {
                    last_ty = self.infer_expr_type(e, env)?;
                }
                Ok(last_ty)
            }
        }
    }
}
