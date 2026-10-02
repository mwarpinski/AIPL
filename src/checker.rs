use crate::ast::*;
use std::collections::HashMap;

pub fn type_size_and_align(ty: &Type) -> Result<(usize, usize), String> {
    match ty {
        Type::I32 | Type::F32 | Type::Bool | Type::Str | Type::Ptr(_) | Type::Array(_) | Type::Fn(_, _) => Ok((4, 4)),
        Type::I64 | Type::F64 => Ok((8, 8)),
        _ => Err(format!("Unsupported type for memory layout: {:?}", ty)),
    }
}

pub fn get_field_offset(def: &StructDef, field_name: &str) -> Result<(usize, Type), String> {
    let mut offset = 0;
    for f in &def.fields {
        let (s, a) = type_size_and_align(&f.ty)?;
        offset = (offset + a - 1) & !(a - 1);
        if f.name == field_name {
            return Ok((offset, f.ty.clone()));
        }
        offset += s;
    }
    Err(format!("Struct '{}' has no field '{}'", def.name, field_name))
}

pub fn get_struct_size(def: &StructDef) -> Result<usize, String> {
    let mut offset = 0;
    let mut max_align = 1;
    for f in &def.fields {
        let (s, a) = type_size_and_align(&f.ty)?;
        if a > max_align {
            max_align = a;
        }
        offset = (offset + a - 1) & !(a - 1);
        offset += s;
    }
    let total = (offset + max_align - 1) & !(max_align - 1);
    Ok(total)
}

#[derive(Debug, Clone)]
pub struct TypeChecker {
    fn_signatures: HashMap<String, (Vec<Type>, Type)>,
    struct_defs: HashMap<String, StructDef>,
    /// Number of while/loop bodies enclosing the expression being checked.
    loop_depth: std::cell::Cell<u32>,
    /// Return type of the function body being checked; None inside contracts.
    return_type: std::cell::RefCell<Option<Type>>,
}

impl TypeChecker {
    pub fn new() -> Self {
        TypeChecker {
            fn_signatures: HashMap::new(),
            struct_defs: HashMap::new(),
            loop_depth: std::cell::Cell::new(0),
            return_type: std::cell::RefCell::new(None),
        }
    }

    pub fn check_module(&mut self, module: &Module) -> Result<(), String> {
        // Register struct definitions
        for s in &module.structs {
            if self.struct_defs.contains_key(&s.name) {
                return Err(format!(
                    "{}:{}: Duplicate struct definition '{}'",
                    s.span.0, s.span.1, s.name
                ));
            }
            for (i, f) in s.fields.iter().enumerate() {
                if s.fields[..i].iter().any(|g| g.name == f.name) {
                    return Err(format!("{}:{}: Duplicate field '{}' in struct '{}'", s.span.0, s.span.1, f.name, s.name));
                }
                type_size_and_align(&f.ty).map_err(|e| {
                    format!(
                        "{}:{}: Field '{}' in struct '{}': {}",
                        s.span.0, s.span.1, f.name, s.name, e
                    )
                })?;
            }
            self.struct_defs.insert(s.name.clone(), s.clone());
        }

        // Field types may name structs defined later in the module.
        for s in &module.structs {
            for f in &s.fields {
                self.validate_type(&f.ty, s.span).map_err(|e| format!("{} (field '{}' of struct '{}')", e, f.name, s.name))?;
            }
        }

        // First pass: register function signatures
        for f in &module.functions {
            let param_types: Vec<Type> = f.params.iter().map(|(_, t)| t.clone()).collect();
            self.fn_signatures
                .insert(f.name.clone(), (param_types, f.return_type.clone()));
        }

        // Second pass: type check bodies and verify contracts
        for f in &module.functions {
            self.check_fn_def(f)?;
        }

        Ok(())
    }

    /// Every type written in source must be well formed: `(ptr S)` names a
    /// known struct, `(arr T)` holds something with a memory layout, and a
    /// struct is never used by value.
    fn validate_type(&self, ty: &Type, span: (u32, u32)) -> Result<(), String> {
        match ty {
            Type::Ptr(inner) => match inner.as_ref() {
                Type::Struct(name) if self.struct_defs.contains_key(name) => Ok(()),
                Type::Struct(name) => Err(format!("{}:{}: Unknown struct '{}' in (ptr {})", span.0, span.1, name, name)),
                other => Err(format!("{}:{}: ptr must point to a struct, got {:?}", span.0, span.1, other)),
            },
            Type::Struct(name) => Err(format!(
                "{}:{}: struct '{}' cannot be used by value; use (ptr {})",
                span.0, span.1, name, name
            )),
            Type::Array(elem) => {
                self.validate_type(elem, span)?;
                type_size_and_align(elem)
                    .map(|_| ())
                    .map_err(|_| format!("{}:{}: (arr T) element must be a scalar, (ptr S), or (arr T), got {:?}", span.0, span.1, elem))
            }
            Type::ResultType(a, b) => {
                self.validate_type(a, span)?;
                self.validate_type(b, span)
            }
            Type::Fn(params, ret) => {
                for p in params {
                    self.validate_type(p, span)?;
                }
                self.validate_type(ret, span)
            }
            _ => Ok(()),
        }
    }

    /// Checks a while/loop body with break/continue allowed inside it.
    fn check_loop_body(&self, body: &[Expr], env: &mut HashMap<String, Type>) -> Result<(), String> {
        self.loop_depth.set(self.loop_depth.get() + 1);
        let r = body.iter().try_for_each(|stmt| self.infer_expr_type(stmt, env).map(|_| ()));
        self.loop_depth.set(self.loop_depth.get() - 1);
        r
    }

    pub fn get_struct_def(&self, name: &str) -> Option<&StructDef> {
        self.struct_defs.get(name)
    }

    fn check_fn_def(&self, f: &FnDef) -> Result<(), String> {
        let mut env = HashMap::new();
        for (param_name, param_ty) in &f.params {
            self.validate_type(param_ty, f.span)
                .map_err(|e| format!("{} (parameter '{}' of '{}')", e, param_name, f.name))?;
            env.insert(param_name.clone(), param_ty.clone());
        }
        self.validate_type(&f.return_type, f.span)
            .map_err(|e| format!("{} (return type of '{}')", e, f.name))?;

        // Check contracts
        for c in &f.contracts {
            match c {
                Contract::Requires(expr) | Contract::Invariant(expr) => {
                    let cond_ty = self.infer_expr_type(expr, &mut env)?;
                    if cond_ty != Type::Bool {
                        let (l, c_col) = expr.span();
                        return Err(format!(
                            "{}:{}: Contract expression in '{}' must evaluate to Bool, got {:?}",
                            l, c_col, f.name, cond_ty
                        ));
                    }
                }
                Contract::Ensures(expr) => {
                    let mut ens_env = env.clone();
                    ens_env.insert("res".to_string(), f.return_type.clone());
                    let cond_ty = self.infer_expr_type(expr, &mut ens_env)?;
                    if cond_ty != Type::Bool {
                        let (l, c_col) = expr.span();
                        return Err(format!(
                            "{}:{}: Ensures contract expression in '{}' must evaluate to Bool, got {:?}",
                            l, c_col, f.name, cond_ty
                        ));
                    }
                }
            }
        }

        // Check function body expressions
        *self.return_type.borrow_mut() = Some(f.return_type.clone());
        self.loop_depth.set(0);
        let mut last_ty = Type::Void;
        let mut result = Ok(());
        for expr in &f.body {
            match self.infer_expr_type(expr, &mut env) {
                Ok(t) => last_ty = t,
                Err(e) => {
                    result = Err(e);
                    break;
                }
            }
        }
        *self.return_type.borrow_mut() = None;
        result?;

        // A body may end in (return v) instead of a bare value.
        let ends_in_return = matches!(f.body.last(), Some(Expr::Return { .. }));
        if f.return_type != Type::Void && last_ty != f.return_type && !ends_in_return {
            return Err(format!(
                "{}:{}: Function '{}' expects return type {:?}, but body returned {:?}",
                f.span.0, f.span.1, f.name, f.return_type, last_ty
            ));
        }

        Ok(())
    }

    fn infer_expr_type(&self, expr: &Expr, env: &mut HashMap<String, Type>) -> Result<Type, String> {
        let (l, c) = expr.span();
        match expr {
            Expr::Lit(lit, _) => match lit {
                Literal::Int(_) => Ok(Type::I32),
                Literal::Int64(_) => Ok(Type::I64),
                Literal::Float(_) => Ok(Type::F64),
                Literal::Bool(_) => Ok(Type::Bool),
                Literal::Str(_) => Ok(Type::Str),
            },
            Expr::Var(name, _) => {
                if let Some(ty) = env.get(name) {
                    Ok(ty.clone())
                } else {
                    Err(format!("{}:{}: Undefined variable '{}'", l, c, name))
                }
            }
            Expr::Let { name, ty, val, .. } => {
                if env.contains_key(name) {
                    return Err(format!("{}:{}: Cannot shadow existing variable '{}'", l, c, name));
                }
                self.validate_type(ty, (l, c))?;
                let val_ty = self.infer_expr_type(val, env)?;
                if val_ty != *ty {
                    return Err(format!(
                        "{}:{}: Type mismatch in 'let': expected {:?}, got {:?}",
                        l, c, ty, val_ty
                    ));
                }
                env.insert(name.clone(), ty.clone());
                Ok(Type::Void)
            }
            Expr::Set { name, val, .. } => {
                let var_ty = env
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("{}:{}: Undefined variable '{}' in set!", l, c, name))?;
                let val_ty = self.infer_expr_type(val, env)?;
                if var_ty != val_ty {
                    return Err(format!(
                        "{}:{}: Type mismatch in 'set!': variable is {:?}, value is {:?}",
                        l, c, var_ty, val_ty
                    ));
                }
                Ok(Type::Void)
            }
            Expr::If { cond, then_branch, else_branch, .. } => {
                let cond_ty = self.infer_expr_type(cond, env)?;
                if cond_ty != Type::Bool {
                    return Err(format!("{}:{}: If condition must be Bool, got {:?}", l, c, cond_ty));
                }
                let then_ty = self.infer_expr_type(then_branch, &mut env.clone())?;
                let else_ty = self.infer_expr_type(else_branch, &mut env.clone())?;
                if then_ty == Type::Void && else_ty == Type::Void {
                    Ok(Type::Void)
                } else if then_ty != Type::Void && else_ty != Type::Void && then_ty == else_ty {
                    Ok(then_ty)
                } else {
                    Err(format!(
                        "{}:{}: If branch type mismatch: then is {:?}, else is {:?}. If mixing void and non-void, consider wrapping in (block ... value)",
                        l, c, then_ty, else_ty
                    ))
                }
            }
            Expr::Loop { var, start, end, step, body, .. } => {
                let start_ty = self.infer_expr_type(start, env)?;
                let end_ty = self.infer_expr_type(end, env)?;
                let step_ty = self.infer_expr_type(step, env)?;
                if start_ty != Type::I32 || end_ty != Type::I32 || step_ty != Type::I32 {
                    return Err(format!("{}:{}: Loop bounds and step must be i32", l, c));
                }
                if env.contains_key(var) {
                    return Err(format!("{}:{}: Cannot shadow existing variable '{}' in loop", l, c, var));
                }
                let mut local_env = env.clone();
                local_env.insert(var.clone(), Type::I32);
                self.check_loop_body(body, &mut local_env)?;
                Ok(Type::Void)
            }
            Expr::While { cond, body, .. } => {
                let cond_ty = self.infer_expr_type(cond, env)?;
                if cond_ty != Type::Bool {
                    return Err(format!("{}:{}: While condition must be Bool", l, c));
                }
                let mut local_env = env.clone();
                self.check_loop_body(body, &mut local_env)?;
                Ok(Type::Void)
            }
            Expr::Return { val, .. } => {
                let expected = self.return_type.borrow().clone().ok_or_else(|| {
                    format!("{}:{}: return is not allowed in a contract", l, c)
                })?;
                let got = match val {
                    Some(v) => self.infer_expr_type(v, env)?,
                    None => Type::Void,
                };
                if got != expected {
                    return Err(format!(
                        "{}:{}: return value has type {:?}, but the function returns {:?}",
                        l, c, got, expected
                    ));
                }
                Ok(Type::Void)
            }
            Expr::Break(_) | Expr::Continue(_) => {
                if self.loop_depth.get() == 0 {
                    let what = if matches!(expr, Expr::Break(_)) { "break" } else { "continue" };
                    return Err(format!("{}:{}: {} is only allowed inside a while or loop body", l, c, what));
                }
                Ok(Type::Void)
            }
            Expr::Call { func, args, .. } => {
                let (param_types, ret_type) = self
                    .fn_signatures
                    .get(func)
                    .ok_or_else(|| format!("{}:{}: Call to unknown function '{}'", l, c, func))?;
                if args.len() != param_types.len() {
                    return Err(format!(
                        "{}:{}: Function '{}' expects {} arguments, got {}",
                        l, c, func, param_types.len(), args.len()
                    ));
                }
                for (i, arg) in args.iter().enumerate() {
                    let arg_ty = self.infer_expr_type(arg, env)?;
                    if arg_ty != param_types[i] {
                        return Err(format!(
                            "{}:{}: Arg {} of '{}' expects {:?}, got {:?}",
                            l, c, i, func, param_types[i], arg_ty
                        ));
                    }
                }
                Ok(ret_type.clone())
            }
            Expr::Op { op, args, .. } => {
                check_literal_address(op, args, l, c)?;
                match op {
                OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Mod | OpCode::BitXor | OpCode::Shl | OpCode::Shr | OpCode::ShrU | OpCode::DivU | OpCode::RemU | OpCode::BitAnd | OpCode::BitOr => {
                    if args.len() != 2 {
                        return Err(format!("{}:{}: Arithmetic/bitwise opcode {:?} requires 2 arguments", l, c, op));
                    }
                    let t1 = self.infer_expr_type(&args[0], env)?;
                    let t2 = self.infer_expr_type(&args[1], env)?;
                    if t1 != t2 {
                        return Err(format!("{}:{}: Type mismatch in binary op: {:?} vs {:?}", l, c, t1, t2));
                    }
                    if matches!(t1, Type::Ptr(_) | Type::Array(_) | Type::Fn(_, _)) {
                        return Err(format!(
                            "{}:{}: {:?} on {:?}: pointers, arrays, and function refs have no arithmetic; use get/put or arr.get/arr.set, or convert with ptr.addr/arr.addr and ptr.cast/arr.cast",
                            l, c, op, t1
                        ));
                    }
                    Ok(t1)
                }
                OpCode::MemLoad8 | OpCode::MemLoad32 => {
                    if args.len() != 1 {
                        return Err(format!("{}:{}: {:?} requires 1 argument (ptr: i32)", l, c, op));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("{}:{}: {:?} requires i32 ptr, got {:?}", l, c, op, t));
                    }
                    Ok(Type::I32)
                }
                OpCode::MemLoad64 => {
                    if args.len() != 1 {
                        return Err(format!("{}:{}: mem.load64 requires 1 argument (ptr: i32)", l, c));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("{}:{}: mem.load64 requires i32 ptr, got {:?}", l, c, t));
                    }
                    Ok(Type::I64)
                }
                OpCode::MemLoadF32 => {
                    if args.len() != 1 {
                        return Err(format!("{}:{}: mem.load_f32 requires 1 argument (ptr: i32)", l, c));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("{}:{}: mem.load_f32 requires i32 ptr, got {:?}", l, c, t));
                    }
                    Ok(Type::F32)
                }
                OpCode::MemLoadF64 => {
                    if args.len() != 1 {
                        return Err(format!("{}:{}: mem.load_f64 requires 1 argument (ptr: i32)", l, c));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("{}:{}: mem.load_f64 requires i32 ptr, got {:?}", l, c, t));
                    }
                    Ok(Type::F64)
                }
                OpCode::MemStore8 | OpCode::MemStore32 => {
                    if args.len() != 2 {
                        return Err(format!("{}:{}: {:?} requires 2 arguments (ptr: i32, val: i32)", l, c, op));
                    }
                    let t1 = self.infer_expr_type(&args[0], env)?;
                    let t2 = self.infer_expr_type(&args[1], env)?;
                    if t1 != Type::I32 || t2 != Type::I32 {
                        return Err(format!("{}:{}: {:?} requires (i32, i32), got ({:?}, {:?})", l, c, op, t1, t2));
                    }
                    Ok(Type::Void)
                }
                OpCode::MemStore64 => {
                    if args.len() != 2 {
                        return Err(format!("{}:{}: mem.store64 requires 2 arguments (ptr: i32, val: i64)", l, c));
                    }
                    let t1 = self.infer_expr_type(&args[0], env)?;
                    let t2 = self.infer_expr_type(&args[1], env)?;
                    if t1 != Type::I32 || t2 != Type::I64 {
                        return Err(format!("{}:{}: mem.store64 requires (i32, i64), got ({:?}, {:?})", l, c, t1, t2));
                    }
                    Ok(Type::Void)
                }
                OpCode::MemStoreF32 => {
                    if args.len() != 2 {
                        return Err(format!("{}:{}: mem.store_f32 requires 2 arguments (ptr: i32, val: f32)", l, c));
                    }
                    let t1 = self.infer_expr_type(&args[0], env)?;
                    let t2 = self.infer_expr_type(&args[1], env)?;
                    if t1 != Type::I32 || t2 != Type::F32 {
                        return Err(format!("{}:{}: mem.store_f32 requires (i32, f32), got ({:?}, {:?})", l, c, t1, t2));
                    }
                    Ok(Type::Void)
                }
                OpCode::MemStoreF64 => {
                    if args.len() != 2 {
                        return Err(format!("{}:{}: mem.store_f64 requires 2 arguments (ptr: i32, val: f64)", l, c));
                    }
                    let t1 = self.infer_expr_type(&args[0], env)?;
                    let t2 = self.infer_expr_type(&args[1], env)?;
                    if t1 != Type::I32 || t2 != Type::F64 {
                        return Err(format!("{}:{}: mem.store_f64 requires (i32, f64), got ({:?}, {:?})", l, c, t1, t2));
                    }
                    Ok(Type::Void)
                }
                OpCode::MemAlloc => {
                    if args.len() != 1 {
                        return Err(format!("{}:{}: mem.alloc requires 1 argument (size: i32)", l, c));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("{}:{}: mem.alloc requires i32 size, got {:?}", l, c, t));
                    }
                    Ok(Type::I32)
                }
                OpCode::MemFree => {
                    if args.len() != 1 {
                        return Err(format!("{}:{}: mem.free requires 1 argument (ptr: i32)", l, c));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("{}:{}: mem.free requires i32 ptr, got {:?}", l, c, t));
                    }
                    Ok(Type::Void)
                }
                OpCode::MemGrow => {
                    if args.len() != 1 {
                        return Err(format!("{}:{}: mem.grow requires 1 argument (pages: i32)", l, c));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("{}:{}: mem.grow requires i32 pages, got {:?}", l, c, t));
                    }
                    Ok(Type::I32)
                }
                OpCode::AtomicAdd => Ok(Type::I32),
                OpCode::AtomicCas => Ok(Type::Bool),
                OpCode::AtomicLock | OpCode::AtomicUnlock => Ok(Type::Void),
                OpCode::Eq | OpCode::Neq | OpCode::Lt | OpCode::Lte | OpCode::Gt | OpCode::Gte => {
                    if args.len() != 2 {
                        return Err(format!("{}:{}: Comparison opcode {:?} requires 2 arguments", l, c, op));
                    }
                    let t1 = self.infer_expr_type(&args[0], env)?;
                    let t2 = self.infer_expr_type(&args[1], env)?;
                    if t1 != t2 {
                        return Err(format!("{}:{}: Type mismatch in comparison: {:?} vs {:?}", l, c, t1, t2));
                    }
                    if matches!(t1, Type::Ptr(_) | Type::Array(_) | Type::Fn(_, _)) && !matches!(op, OpCode::Eq | OpCode::Neq) {
                        return Err(format!("{}:{}: {:?} on {:?}: pointers, arrays, and function refs compare only with eq/neq", l, c, op, t1));
                    }
                    Ok(Type::Bool)
                }
                OpCode::And | OpCode::Or => {
                    for arg in args {
                        let t = self.infer_expr_type(arg, env)?;
                        if t != Type::Bool {
                            return Err(format!("{}:{}: Logical op expects Bool, got {:?}", l, c, t));
                        }
                    }
                    Ok(Type::Bool)
                }
                OpCode::Not => {
                    if args.len() != 1 {
                        return Err(format!("{}:{}: Not op expects 1 argument", l, c));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::Bool {
                        return Err(format!("{}:{}: Not op expects Bool, got {:?}", l, c, t));
                    }
                    Ok(Type::Bool)
                }
                OpCode::SysPrint => {
                    for arg in args {
                        self.infer_expr_type(arg, env)?;
                    }
                    Ok(Type::Void)
                }
                OpCode::SysTime => Ok(Type::F64),
                OpCode::SysExit => {
                    if args.len() != 1 {
                        return Err(format!("{}:{}: sys.exit requires 1 argument (code: i32)", l, c));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("{}:{}: sys.exit requires i32 code, got {:?}", l, c, t));
                    }
                    Ok(Type::Void)
                }
                OpCode::StrLen | OpCode::StrPtr => {
                    let name = if *op == OpCode::StrLen { "str.len" } else { "str.ptr" };
                    if args.len() != 1 {
                        return Err(format!("{}:{}: {} requires 1 argument (s: str)", l, c, name));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::Str {
                        return Err(format!("{}:{}: {} requires str, got {:?}", l, c, name, t));
                    }
                    Ok(Type::I32)
                }
                // fs.* take raw i32 pointers/lengths/fds, never a `str` (a str is a
                // pointer in wasm but a Rust string in the VM, so letting one
                // through here would work in one backend and fail in the other).
                OpCode::FsOpen | OpCode::FsRead | OpCode::FsWrite => {
                    if args.len() != 3 {
                        return Err(format!("{}:{}: {:?} requires 3 arguments", l, c, op));
                    }
                    for (i, arg) in args.iter().enumerate() {
                        let t = self.infer_expr_type(arg, env)?;
                        if t != Type::I32 {
                            return Err(format!(
                                "{}:{}: {:?} argument {} must be i32 (pointer/length/fd), got {:?}",
                                l, c, op, i, t
                            ));
                        }
                    }
                    Ok(Type::I32)
                }
                OpCode::FsClose => {
                    if args.len() != 1 {
                        return Err(format!("{}:{}: fs.close requires 1 argument", l, c));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("{}:{}: fs.close requires i32 fd, got {:?}", l, c, t));
                    }
                    Ok(Type::I32)
                }
                // args.*/env.* take two i32 addresses the host writes through.
                OpCode::ArgsSizes | OpCode::ArgsGet | OpCode::EnvSizes | OpCode::EnvGet => {
                    if args.len() != 2 {
                        return Err(format!("{}:{}: {:?} requires 2 arguments (two i32 addresses)", l, c, op));
                    }
                    for (i, arg) in args.iter().enumerate() {
                        let t = self.infer_expr_type(arg, env)?;
                        if t != Type::I32 {
                            return Err(format!("{}:{}: {:?} argument {} must be an i32 address, got {:?}", l, c, op, i, t));
                        }
                    }
                    Ok(Type::I32)
                }
                OpCode::FsDelete => {
                    if args.len() != 2 {
                        return Err(format!("{}:{}: fs.delete requires 2 arguments (path_ptr, path_len)", l, c));
                    }
                    for (i, arg) in args.iter().enumerate() {
                        let t = self.infer_expr_type(arg, env)?;
                        if t != Type::I32 {
                            return Err(format!(
                                "{}:{}: fs.delete argument {} must be i32 (pointer/length), got {:?}",
                                l, c, i, t
                            ));
                        }
                    }
                    Ok(Type::I32)
                }
                OpCode::ThreadSpawn => {
                    // (thread.spawn (ref worker) arg): the worker takes the i32 arg.
                    if args.len() != 2 {
                        return Err(format!("{}:{}: thread.spawn requires 2 arguments (worker: (fn [i32] -> i32), arg: i32)", l, c));
                    }
                    let worker = Type::Fn(vec![Type::I32], Box::new(Type::I32));
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != worker {
                        return Err(format!("{}:{}: thread.spawn needs a worker of type (fn [i32] -> i32), got {:?}", l, c, t));
                    }
                    let a = self.infer_expr_type(&args[1], env)?;
                    if a != Type::I32 {
                        return Err(format!("{}:{}: thread.spawn argument must be i32, got {:?}", l, c, a));
                    }
                    Ok(Type::I32)
                }
                OpCode::ThreadJoin => {
                    if args.len() != 1 {
                        return Err(format!("{}:{}: thread.join requires 1 argument (thread handle)", l, c));
                    }
                    self.infer_expr_type(&args[0], env)?;
                    Ok(Type::I32)
                }
                OpCode::I64ExtendS | OpCode::I64ExtendU => {
                    if args.len() != 1 {
                        return Err(format!("{}:{}: {:?} requires 1 argument (x: i32)", l, c, op));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("{}:{}: {:?} requires i32, got {:?}", l, c, op, t));
                    }
                    Ok(Type::I64)
                }
                OpCode::F64ConvertI64S | OpCode::I64TruncF64S | OpCode::F64ReinterpretI64 | OpCode::I64ReinterpretF64 => {
                    let (from, to) = match op {
                        OpCode::F64ConvertI64S | OpCode::F64ReinterpretI64 => (Type::I64, Type::F64),
                        _ => (Type::F64, Type::I64),
                    };
                    if args.len() != 1 {
                        return Err(format!("{}:{}: {:?} requires 1 argument (x: {:?})", l, c, op, from));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != from {
                        return Err(format!("{}:{}: {:?} requires {:?}, got {:?}", l, c, op, from, t));
                    }
                    Ok(to)
                }
                OpCode::I32Wrap => {
                    if args.len() != 1 {
                        return Err(format!("{}:{}: i32.wrap requires 1 argument (x: i64)", l, c));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I64 {
                        return Err(format!("{}:{}: i32.wrap requires i64, got {:?}", l, c, t));
                    }
                    Ok(Type::I32)
                }
                }
            }
            Expr::Ok(val, extra_ty, _) => {
                let inner_ty = self.infer_expr_type(val, env)?;
                let err_ty = extra_ty.clone().unwrap_or(Type::I32);
                self.validate_type(&err_ty, (l, c))?;
                Ok(Type::ResultType(Box::new(inner_ty), Box::new(err_ty)))
            }
            Expr::Err(err, extra_ty, _) => {
                let err_ty = self.infer_expr_type(err, env)?;
                let ok_ty = extra_ty.clone().unwrap_or(Type::I32);
                self.validate_type(&ok_ty, (l, c))?;
                Ok(Type::ResultType(Box::new(ok_ty), Box::new(err_ty)))
            }
            Expr::MatchResult { expr, ok_var, ok_body, err_var, err_body, .. } => {
                let res_ty = self.infer_expr_type(expr, env)?;
                let (ok_ty, err_ty) = match res_ty {
                    Type::ResultType(ok_t, err_t) => (*ok_t, *err_t),
                    other => return Err(format!("{}:{}: match_result expected ResultType, got {:?}", l, c, other)),
                };

                if env.contains_key(ok_var) {
                    return Err(format!("{}:{}: Cannot shadow existing variable '{}' in match_result ok arm", l, c, ok_var));
                }
                let mut ok_env = env.clone();
                ok_env.insert(ok_var.clone(), ok_ty);
                let mut last_ok_ty = Type::Void;
                for stmt in ok_body {
                    last_ok_ty = self.infer_expr_type(stmt, &mut ok_env)?;
                }

                if env.contains_key(err_var) {
                    return Err(format!("{}:{}: Cannot shadow existing variable '{}' in match_result err arm", l, c, err_var));
                }
                let mut err_env = env.clone();
                err_env.insert(err_var.clone(), err_ty);
                let mut last_err_ty = Type::Void;
                for stmt in err_body {
                    last_err_ty = self.infer_expr_type(stmt, &mut err_env)?;
                }

                if last_ok_ty == Type::Void && last_err_ty == Type::Void {
                    Ok(Type::Void)
                } else if last_ok_ty != Type::Void && last_err_ty != Type::Void && last_ok_ty == last_err_ty {
                    Ok(last_ok_ty)
                } else {
                    Err(format!(
                        "{}:{}: match_result arm type mismatch: ok arm yields {:?}, err arm yields {:?}",
                        l, c, last_ok_ty, last_err_ty
                    ))
                }
            }
            Expr::Block(exprs, _) => {
                let mut local_env = env.clone();
                let mut last_ty = Type::Void;
                for e in exprs {
                    last_ty = self.infer_expr_type(e, &mut local_env)?;
                }
                Ok(last_ty)
            }
            Expr::NewStruct { struct_name, span } => {
                if !self.struct_defs.contains_key(struct_name) {
                    return Err(format!("{}:{}: Unknown struct '{}'", span.0, span.1, struct_name));
                }
                Ok(Type::Ptr(Box::new(Type::Struct(struct_name.clone()))))
            }
            Expr::GetField {
                struct_name,
                field_name,
                ptr,
                span,
            } => {
                let def = self.struct_defs.get(struct_name).ok_or_else(|| {
                    format!("{}:{}: Unknown struct '{}'", span.0, span.1, struct_name)
                })?;
                let ptr_ty = self.infer_expr_type(ptr, env)?;
                if ptr_ty != Type::Ptr(Box::new(Type::Struct(struct_name.clone()))) {
                    return Err(format!(
                        "{}:{}: get {}.{} needs a (ptr {}), got {:?}",
                        span.0, span.1, struct_name, field_name, struct_name, ptr_ty
                    ));
                }
                let (_offset, field_ty) = get_field_offset(def, field_name).map_err(|e| {
                    format!("{}:{}: {}", span.0, span.1, e)
                })?;
                Ok(field_ty)
            }
            Expr::PutField {
                struct_name,
                field_name,
                ptr,
                val,
                span,
            } => {
                let def = self.struct_defs.get(struct_name).ok_or_else(|| {
                    format!("{}:{}: Unknown struct '{}'", span.0, span.1, struct_name)
                })?;
                let ptr_ty = self.infer_expr_type(ptr, env)?;
                if ptr_ty != Type::Ptr(Box::new(Type::Struct(struct_name.clone()))) {
                    return Err(format!(
                        "{}:{}: put {}.{} needs a (ptr {}), got {:?}",
                        span.0, span.1, struct_name, field_name, struct_name, ptr_ty
                    ));
                }
                let (_offset, field_ty) = get_field_offset(def, field_name).map_err(|e| {
                    format!("{}:{}: {}", span.0, span.1, e)
                })?;
                let val_ty = self.infer_expr_type(val, env)?;
                if val_ty != field_ty {
                    return Err(format!(
                        "{}:{}: Type mismatch writing to field '{}.{}': expected {:?}, got {:?}",
                        span.0, span.1, struct_name, field_name, field_ty, val_ty
                    ));
                }
                Ok(Type::Void)
            }
            Expr::Sizeof { struct_name, span } => {
                if !self.struct_defs.contains_key(struct_name) {
                    return Err(format!("{}:{}: Unknown struct '{}'", span.0, span.1, struct_name));
                }
                Ok(Type::I32)
            }
            Expr::ArrNew { elem_ty, size, span } => {
                self.validate_type(&Type::Array(Box::new(elem_ty.clone())), *span)?;
                let sz_ty = self.infer_expr_type(size, env)?;
                if sz_ty != Type::I32 {
                    return Err(format!(
                        "{}:{}: arr.new size must be i32, got {:?}",
                        span.0, span.1, sz_ty
                    ));
                }
                Ok(Type::Array(Box::new(elem_ty.clone())))
            }
            Expr::ArrGet {
                elem_ty,
                ptr,
                index,
                span,
            } => {
                self.validate_type(&Type::Array(Box::new(elem_ty.clone())), *span)?;
                let ptr_ty = self.infer_expr_type(ptr, env)?;
                if ptr_ty != Type::Array(Box::new(elem_ty.clone())) {
                    return Err(format!(
                        "{}:{}: arr.get {:?} needs an (arr {:?}), got {:?}",
                        span.0, span.1, elem_ty, elem_ty, ptr_ty
                    ));
                }
                let idx_ty = self.infer_expr_type(index, env)?;
                if idx_ty != Type::I32 {
                    return Err(format!(
                        "{}:{}: arr.get index must be i32, got {:?}",
                        span.0, span.1, idx_ty
                    ));
                }
                Ok(elem_ty.clone())
            }
            Expr::ArrSet {
                elem_ty,
                ptr,
                index,
                val,
                span,
            } => {
                self.validate_type(&Type::Array(Box::new(elem_ty.clone())), *span)?;
                let ptr_ty = self.infer_expr_type(ptr, env)?;
                if ptr_ty != Type::Array(Box::new(elem_ty.clone())) {
                    return Err(format!(
                        "{}:{}: arr.set {:?} needs an (arr {:?}), got {:?}",
                        span.0, span.1, elem_ty, elem_ty, ptr_ty
                    ));
                }
                let idx_ty = self.infer_expr_type(index, env)?;
                if idx_ty != Type::I32 {
                    return Err(format!(
                        "{}:{}: arr.set index must be i32, got {:?}",
                        span.0, span.1, idx_ty
                    ));
                }
                let val_ty = self.infer_expr_type(val, env)?;
                if val_ty != *elem_ty {
                    return Err(format!(
                        "{}:{}: arr.set value mismatch: expected {:?}, got {:?}",
                        span.0, span.1, elem_ty, val_ty
                    ));
                }
                Ok(Type::Void)
            }
            Expr::ArrLen { arr, span } => match self.infer_expr_type(arr, env)? {
                Type::Array(_) => Ok(Type::I32),
                other => Err(format!("{}:{}: arr.len needs an (arr T), got {:?}", span.0, span.1, other)),
            },
            Expr::Null { ty, span } => {
                self.validate_type(ty, *span)?;
                Ok(ty.clone())
            }
            Expr::Cast { ty, addr, span } => {
                self.validate_type(ty, *span)?;
                let t = self.infer_expr_type(addr, env)?;
                if t != Type::I32 {
                    return Err(format!("{}:{}: cast needs an i32 address, got {:?}", span.0, span.1, t));
                }
                Ok(ty.clone())
            }
            Expr::Ref { name, span } => match self.fn_signatures.get(name) {
                Some((params, ret)) => Ok(Type::Fn(params.clone(), Box::new(ret.clone()))),
                None => Err(format!("{}:{}: Undefined function '{}' in ref", span.0, span.1, name)),
            },
            Expr::CallRef { sig, func, args, span } => {
                self.validate_type(sig, *span)?;
                let (params, ret) = match sig {
                    Type::Fn(p, r) => (p.clone(), *r.clone()),
                    other => {
                        return Err(format!("{}:{}: call_ref needs a (fn [...] -> r) signature, got {:?}", span.0, span.1, other))
                    }
                };
                let ft = self.infer_expr_type(func, env)?;
                if ft != *sig {
                    return Err(format!("{}:{}: call_ref signature {:?} does not match the function's type {:?}", span.0, span.1, sig, ft));
                }
                if args.len() != params.len() {
                    return Err(format!("{}:{}: call_ref expects {} arguments, got {}", span.0, span.1, params.len(), args.len()));
                }
                for (i, (a, p)) in args.iter().zip(params.iter()).enumerate() {
                    let at = self.infer_expr_type(a, env)?;
                    if at != *p {
                        return Err(format!("{}:{}: call_ref argument {} expects {:?}, got {:?}", span.0, span.1, i + 1, p, at));
                    }
                }
                Ok(ret)
            }
            Expr::Addr { val, array, span } => {
                let t = self.infer_expr_type(val, env)?;
                match (array, &t) {
                    (false, Type::Ptr(_)) | (true, Type::Array(_)) => Ok(Type::I32),
                    (false, _) => Err(format!("{}:{}: ptr.addr needs a (ptr S), got {:?}", span.0, span.1, t)),
                    (true, _) => Err(format!("{}:{}: arr.addr needs an (arr T), got {:?}", span.0, span.1, t)),
                }
            }
        }
    }
}

/// Ops whose first argument is a linear-memory address.
fn is_address_op(op: &OpCode) -> bool {
    matches!(
        op,
        OpCode::MemLoad8
            | OpCode::MemLoad32
            | OpCode::MemLoad64
            | OpCode::MemLoadF32
            | OpCode::MemLoadF64
            | OpCode::MemStore8
            | OpCode::MemStore32
            | OpCode::MemStore64
            | OpCode::MemStoreF32
            | OpCode::MemStoreF64
            | OpCode::AtomicAdd
            | OpCode::AtomicCas
            | OpCode::AtomicLock
            | OpCode::AtomicUnlock
    )
}

/// Ops that modify (or lock) the word at their address argument.
fn is_write_op(op: &OpCode) -> bool {
    matches!(
        op,
        OpCode::MemStore8
            | OpCode::MemStore32
            | OpCode::MemStore64
            | OpCode::MemStoreF32
            | OpCode::MemStoreF64
            | OpCode::AtomicAdd
            | OpCode::AtomicCas
            | OpCode::AtomicLock
            | OpCode::AtomicUnlock
    )
}

/// Static enforcement of the memory layout (AIPL_SPEC.md, "Memory layout")
/// for the one case that can be seen at check time: a literal address.
/// Bytes 0-3 are the heap cursor (reading it is fine, writing or locking it
/// is not), cells 4-63 are 4-byte-aligned runtime slots, bytes 64-1023 are
/// reserved, and everything from 1024 up is heap that should come from
/// `mem.alloc`. Computed addresses cannot be checked here; the VM catches the
/// most common consequence (locking a non-lock word) at runtime instead.
fn check_literal_address(op: &OpCode, args: &[Expr], l: u32, c: u32) -> Result<(), String> {
    if !is_address_op(op) {
        return Ok(());
    }
    let Some(Expr::Lit(Literal::Int(a), _)) = args.first() else {
        return Ok(());
    };
    let a = *a;
    if a < 0 {
        return Err(format!("{}:{}: {:?} at negative literal address {}", l, c, op, a));
    }
    if a < 4 && is_write_op(op) {
        return Err(format!(
            "{}:{}: {:?} at address {}: bytes 0-3 are the heap cursor owned by mem.alloc; locking it hangs and storing to it corrupts the allocator. Take memory from (mem.alloc n) instead",
            l, c, op, a
        ));
    }
    if (4..64).contains(&a) && a % 4 != 0 {
        return Err(format!(
            "{}:{}: {:?} at address {}: runtime cells 4-63 are 4-byte-aligned i32 slots (4, 8, 12, ...)",
            l, c, op, a
        ));
    }
    if (64..1024).contains(&a) {
        return Err(format!(
            "{}:{}: {:?} at literal address {}: bytes 64-1023 are the reserved runtime block. Take memory from (mem.alloc n) instead",
            l, c, op, a
        ));
    }
    Ok(())
}
