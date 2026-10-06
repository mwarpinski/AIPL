use crate::ast::*;
use std::collections::HashMap;

pub fn type_size_and_align(ty: &Type) -> Result<(usize, usize), String> {
    match ty {
        Type::I32 | Type::F32 | Type::Bool | Type::Str | Type::Ptr(_) | Type::Array(_) | Type::Fn(_, _) | Type::Enum(_) | Type::Union(_) => Ok((4, 4)),
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

/// Where each field of a union variant lives in its cell: the tag (i32) is
/// at offset 0 and the fields follow from offset 4, laid out like a struct's.
/// Returns the field offsets and the cell's size.
pub fn variant_layout(v: &Variant) -> Result<(Vec<usize>, usize), String> {
    let mut offset = 4;
    let mut max_align = 4;
    let mut offsets = Vec::new();
    for f in &v.fields {
        let (s, a) = type_size_and_align(&f.ty)?;
        max_align = max_align.max(a);
        offset = (offset + a - 1) & !(a - 1);
        offsets.push(offset);
        offset += s;
    }
    Ok((offsets, (offset + max_align - 1) & !(max_align - 1)))
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
    enum_defs: HashMap<String, EnumDef>,
    union_defs: HashMap<String, UnionDef>,
    /// Number of while/loop bodies enclosing the expression being checked.
    loop_depth: std::cell::Cell<u32>,
    /// Every name declared in the function being checked, with its type: a
    /// compiled function has one local per name, so a name declared again in
    /// another scope (a sibling block, another arm) must keep its type.
    declared: std::cell::RefCell<HashMap<String, Type>>,
    /// Return type of the function body being checked; None inside contracts.
    return_type: std::cell::RefCell<Option<Type>>,
}

impl TypeChecker {
    pub fn new() -> Self {
        TypeChecker {
            fn_signatures: HashMap::new(),
            struct_defs: HashMap::new(),
            enum_defs: HashMap::new(),
            union_defs: HashMap::new(),
            declared: std::cell::RefCell::new(HashMap::new()),
            loop_depth: std::cell::Cell::new(0),
            return_type: std::cell::RefCell::new(None),
        }
    }

    pub fn check_module(&mut self, module: &Module) -> Result<(), String> {
        // Register enum definitions (AIPL_SPEC.md 4.I)
        for e in &module.enums {
            let (l, c) = e.span;
            if self.enum_defs.contains_key(&e.name) {
                return Err(format!("{}:{}: Duplicate enum definition '{}'", l, c, e.name));
            }
            if module.structs.iter().any(|s| s.name == e.name) {
                return Err(format!("{}:{}: '{}' is defined as both a struct and an enum", l, c, e.name));
            }
            if e.members.is_empty() {
                return Err(format!("{}:{}: enum '{}' has no members", l, c, e.name));
            }
            for (i, (m, v)) in e.members.iter().enumerate() {
                if !m.starts_with(|ch: char| ch.is_ascii_lowercase() || ch == '_') {
                    return Err(format!("{}:{}: enum member '{}.{}' must start with a lowercase letter or '_'", l, c, e.name, m));
                }
                if let Some((other, _)) = e.members[..i].iter().find(|(o, _)| o == m) {
                    return Err(format!("{}:{}: enum '{}' has two members named '{}'", l, c, e.name, other));
                }
                if let Some((other, _)) = e.members[..i].iter().find(|(_, w)| w == v) {
                    return Err(format!("{}:{}: enum '{}': members '{}' and '{}' both have the value {}", l, c, e.name, other, m, v));
                }
            }
            self.enum_defs.insert(e.name.clone(), e.clone());
        }

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

        // Register union definitions (AIPL_SPEC.md 4.J)
        for u in &module.unions {
            let (l, c) = u.span;
            if self.union_defs.contains_key(&u.name) {
                return Err(format!("{}:{}: Duplicate union definition '{}'", l, c, u.name));
            }
            if self.struct_defs.contains_key(&u.name) || self.enum_defs.contains_key(&u.name) {
                return Err(format!("{}:{}: '{}' is defined as a union and as a struct or enum", l, c, u.name));
            }
            if u.variants.is_empty() {
                return Err(format!("{}:{}: union '{}' has no variants", l, c, u.name));
            }
            for (i, v) in u.variants.iter().enumerate() {
                if !v.name.starts_with(|ch: char| ch.is_ascii_lowercase() || ch == '_') {
                    return Err(format!("{}:{}: union variant '{}.{}' must start with a lowercase letter or '_'", l, c, u.name, v.name));
                }
                if u.variants[..i].iter().any(|w| w.name == v.name) {
                    return Err(format!("{}:{}: union '{}' has two variants named '{}'", l, c, u.name, v.name));
                }
                for (j, f) in v.fields.iter().enumerate() {
                    if v.fields[..j].iter().any(|g| g.name == f.name) {
                        return Err(format!("{}:{}: variant '{}.{}' has two fields named '{}'", l, c, u.name, v.name, f.name));
                    }
                    type_size_and_align(&f.ty)
                        .map_err(|e| format!("{}:{}: Field '{}' of variant '{}.{}': {}", l, c, f.name, u.name, v.name, e))?;
                }
            }
            self.union_defs.insert(u.name.clone(), u.clone());
        }

        // Field types may name structs and unions defined later in the module.
        for s in &module.structs {
            for f in &s.fields {
                self.validate_type(&f.ty, s.span).map_err(|e| format!("{} (field '{}' of struct '{}')", e, f.name, s.name))?;
            }
        }
        for u in &module.unions {
            for v in &u.variants {
                for f in &v.fields {
                    self.validate_type(&f.ty, u.span)
                        .map_err(|e| format!("{} (field '{}' of variant '{}.{}')", e, f.name, u.name, v.name))?;
                }
            }
        }

        // First pass: register function signatures
        for f in &module.functions {
            if self.fn_signatures.contains_key(&f.name) {
                return Err(format!("{}:{}: Duplicate function definition '{}'", f.span.0, f.span.1, f.name));
            }
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
            Type::Enum(name) if self.enum_defs.contains_key(name) => Ok(()),
            // only enum.cast names an enum type outside a type position
            Type::Enum(name) if self.union_defs.contains_key(name) => Err(format!(
                "{}:{}: '{}' is a union, not an enum: its values are made with (make {}.variant ...)",
                span.0, span.1, name, name
            )),
            Type::Union(name) if self.union_defs.contains_key(name) => Ok(()),
            Type::Union(name) => Err(format!("{}:{}: Unknown union '{}'", span.0, span.1, name)),
            Type::Enum(name) if self.struct_defs.contains_key(name) => Err(format!(
                "{}:{}: '{}' is a struct, which is only used through a pointer: write (ptr {})",
                span.0, span.1, name, name
            )),
            Type::Enum(name) => Err(format!("{}:{}: Unknown type '{}' (not a scalar type or an enum)", span.0, span.1, name)),
            _ => Ok(()),
        }
    }

    /// An op's operands: exactly `want.len()` of them, of those types.
    fn expect_operands(
        &self,
        name: &str,
        form: &str,
        args: &[Expr],
        want: &[Type],
        env: &mut HashMap<String, Type>,
        (l, c): (u32, u32),
    ) -> Result<(), String> {
        if args.len() != want.len() {
            return Err(format!("{}:{}: {} takes {} operands, {}; got {}", l, c, name, want.len(), form, args.len()));
        }
        for (i, (a, w)) in args.iter().zip(want).enumerate() {
            let t = self.infer_expr_type(a, env)?;
            if t != *w {
                return Err(format!("{}:{}: {} operand {} must be {:?}, got {:?}", l, c, name, i + 1, w, t));
            }
        }
        Ok(())
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

    /// Records a local's declaration; see `declared`.
    fn declare(&self, name: &str, ty: &Type, (l, c): (u32, u32)) -> Result<(), String> {
        let mut declared = self.declared.borrow_mut();
        match declared.get(name) {
            Some(prev) if prev != ty => Err(format!(
                "{}:{}: '{}' is {:?} here but {:?} elsewhere in this function; a name keeps one type per function (rename one)",
                l, c, name, ty, prev
            )),
            _ => {
                declared.insert(name.to_string(), ty.clone());
                Ok(())
            }
        }
    }

    fn check_fn_def(&self, f: &FnDef) -> Result<(), String> {
        let mut env = HashMap::new();
        self.declared.borrow_mut().clear();
        for (param_name, param_ty) in &f.params {
            self.declared.borrow_mut().insert(param_name.clone(), param_ty.clone());
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
                self.declare(name, ty, (l, c))?;
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
                self.declare(var, &Type::I32, (l, c))?;
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
                OpCode::CheckedAdd | OpCode::CheckedSub | OpCode::CheckedMul => {
                    if args.len() != 2 {
                        return Err(format!("{}:{}: {:?} requires 2 arguments", l, c, op));
                    }
                    let t1 = self.infer_expr_type(&args[0], env)?;
                    let t2 = self.infer_expr_type(&args[1], env)?;
                    if t1 != t2 {
                        return Err(format!("{}:{}: Type mismatch in binary op: {:?} vs {:?}", l, c, t1, t2));
                    }
                    if !matches!(t1, Type::I32 | Type::I64) {
                        return Err(format!("{}:{}: {:?} is integer arithmetic (i32 or i64), got {:?}", l, c, op, t1));
                    }
                    Ok(t1)
                }
                OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Mod | OpCode::BitXor | OpCode::Shl | OpCode::Shr | OpCode::ShrU | OpCode::DivU | OpCode::RemU | OpCode::BitAnd | OpCode::BitOr => {
                    if args.len() != 2 {
                        return Err(format!("{}:{}: Arithmetic/bitwise opcode {:?} requires 2 arguments", l, c, op));
                    }
                    let t1 = self.infer_expr_type(&args[0], env)?;
                    let t2 = self.infer_expr_type(&args[1], env)?;
                    // A pointer, function ref, or enum operand gets its own
                    // explanation even when the other operand is a number:
                    // (+ p 4), (+ 1 e).
                    let special = |t: &Type| matches!(t, Type::Ptr(_) | Type::Array(_) | Type::Fn(_, _) | Type::Enum(_) | Type::Union(_));
                    let t1 = if !special(&t1) && special(&t2) { t2.clone() } else { t1 };
                    if t1 != t2 && !special(&t1) {
                        return Err(format!("{}:{}: Type mismatch in binary op: {:?} vs {:?}", l, c, t1, t2));
                    }
                    if matches!(t1, Type::Ptr(_) | Type::Array(_) | Type::Fn(_, _)) {
                        return Err(format!(
                            "{}:{}: {:?} on {:?}: pointers, arrays, and function refs have no arithmetic; use get/put or arr.get/arr.set, or convert with ptr.addr/arr.addr and ptr.cast/arr.cast",
                            l, c, op, t1
                        ));
                    }
                    if let Type::Union(u) = &t1 {
                        return Err(format!("{}:{}: {:?} on union '{}': unions have no arithmetic; take them apart with match", l, c, op, u));
                    }
                    let integer_only = !matches!(op, OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div);
                    if t1 == Type::Str && matches!(op, OpCode::Add) {
                        return Err(format!("{}:{}: + does not join strings; build them with std/buf (buf.push_str, buf.bytes)", l, c));
                    }
                    if integer_only && !matches!(t1, Type::I32 | Type::I64 | Type::Enum(_)) {
                        return Err(format!("{}:{}: {:?} is integer arithmetic (i32 or i64), got {:?}", l, c, op, t1));
                    }
                    if !matches!(t1, Type::I32 | Type::I64 | Type::F32 | Type::F64 | Type::Enum(_)) {
                        return Err(format!("{}:{}: {:?} needs numbers (i32, i64, f32, f64), got {:?}", l, c, op, t1));
                    }
                    if let Type::Enum(e) = &t1 {
                        return Err(format!(
                            "{}:{}: {:?} on enum '{}': enums have no arithmetic; compare them with eq/neq, or convert with (enum.ord x) and (enum.cast {} n)",
                            l, c, op, e, e
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
                OpCode::AtomicAdd => {
                    self.expect_operands("atomic.add", "(atomic.add p v)", args, &[Type::I32, Type::I32], env, (l, c))?;
                    Ok(Type::I32)
                }
                OpCode::AtomicCas => {
                    self.expect_operands("atomic.cas", "(atomic.cas p expected new)", args, &[Type::I32, Type::I32, Type::I32], env, (l, c))?;
                    Ok(Type::Bool)
                }
                OpCode::AtomicLock | OpCode::AtomicUnlock => {
                    let (name, form) = if matches!(op, OpCode::AtomicLock) { ("atomic.lock", "(atomic.lock p)") } else { ("atomic.unlock", "(atomic.unlock p)") };
                    self.expect_operands(name, form, args, &[Type::I32], env, (l, c))?;
                    Ok(Type::Void)
                }
                OpCode::Eq | OpCode::Neq | OpCode::Lt | OpCode::Lte | OpCode::Gt | OpCode::Gte
                | OpCode::LtU | OpCode::LteU | OpCode::GtU | OpCode::GteU => {
                    if args.len() != 2 {
                        return Err(format!("{}:{}: Comparison opcode {:?} requires 2 arguments", l, c, op));
                    }
                    let t1 = self.infer_expr_type(&args[0], env)?;
                    let t2 = self.infer_expr_type(&args[1], env)?;
                    if t1 != t2 {
                        return Err(format!("{}:{}: Type mismatch in comparison: {:?} vs {:?}", l, c, t1, t2));
                    }
                    if matches!(op, OpCode::LtU | OpCode::LteU | OpCode::GtU | OpCode::GteU) && !matches!(t1, Type::I32 | Type::I64) {
                        return Err(format!("{}:{}: {:?} compares integers (i32 or i64) as unsigned, got {:?}", l, c, op, t1));
                    }
                    if let Type::Union(u) = &t1 {
                        return Err(format!("{}:{}: {:?} on union '{}': union values do not compare; take them apart with match", l, c, op, u));
                    }
                    if matches!(t1, Type::Ptr(_) | Type::Array(_) | Type::Fn(_, _) | Type::Enum(_)) && !matches!(op, OpCode::Eq | OpCode::Neq) {
                        return Err(format!("{}:{}: {:?} on {:?}: pointers, arrays, function refs, and enums compare only with eq/neq", l, c, op, t1));
                    }
                    // lt/lte/gt/gte order numbers; bool and str compare only with eq/neq
                    if matches!(t1, Type::Bool | Type::Str) && !matches!(op, OpCode::Eq | OpCode::Neq) {
                        return Err(format!("{}:{}: {:?} on {:?}: only numbers are ordered; bool and str compare only with eq/neq", l, c, op, t1));
                    }
                    Ok(Type::Bool)
                }
                OpCode::And | OpCode::Or => {
                    if args.len() != 2 {
                        return Err(format!(
                            "{}:{}: {} takes exactly 2 operands, got {}; nest them: ({} a ({} b c))",
                            l, c, if matches!(op, OpCode::And) { "and" } else { "or" }, args.len(),
                            if matches!(op, OpCode::And) { "and" } else { "or" },
                            if matches!(op, OpCode::And) { "and" } else { "or" }
                        ));
                    }
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
                        let t = self.infer_expr_type(arg, env)?;
                        if t != Type::Str {
                            return Err(format!(
                                "{}:{}: sys.print prints str values, got {:?}; for numbers use io.print_int / io.print_i64 / io.print_f64 (import io)",
                                l, c, t
                            ));
                        }
                    }
                    Ok(Type::Void)
                }
                OpCode::SysTime | OpCode::SysMonotonic => {
                    if !args.is_empty() {
                        return Err(format!("{}:{}: {:?} takes no arguments", l, c, op));
                    }
                    Ok(Type::I64)
                }
                OpCode::SysRandom => {
                    if args.len() != 2 {
                        return Err(format!("{}:{}: sys.random requires 2 arguments (ptr, len)", l, c));
                    }
                    for (i, arg) in args.iter().enumerate() {
                        let t = self.infer_expr_type(arg, env)?;
                        if t != Type::I32 {
                            return Err(format!("{}:{}: sys.random argument {} must be i32, got {:?}", l, c, i, t));
                        }
                    }
                    Ok(Type::I32)
                }
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
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("{}:{}: thread.join needs the i32 handle thread.spawn returned, got {:?}", l, c, t));
                    }
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
                OpCode::F64ConvertI64S | OpCode::I64TruncF64S | OpCode::F64ReinterpretI64 | OpCode::I64ReinterpretF64 | OpCode::F64Sqrt => {
                    let (from, to) = match op {
                        OpCode::F64ConvertI64S | OpCode::F64ReinterpretI64 => (Type::I64, Type::F64),
                        OpCode::F64Sqrt => (Type::F64, Type::F64),
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
                self.declare(ok_var, &ok_ty, (l, c))?;
                ok_env.insert(ok_var.clone(), ok_ty);
                let mut last_ok_ty = Type::Void;
                for stmt in ok_body {
                    last_ok_ty = self.infer_expr_type(stmt, &mut ok_env)?;
                }

                if env.contains_key(err_var) {
                    return Err(format!("{}:{}: Cannot shadow existing variable '{}' in match_result err arm", l, c, err_var));
                }
                let mut err_env = env.clone();
                self.declare(err_var, &err_ty, (l, c))?;
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
            Expr::Sizeof { ty, span } => {
                match ty {
                    Type::Struct(name) => {
                        if !self.struct_defs.contains_key(name) {
                            return Err(format!("{}:{}: Unknown struct '{}'", span.0, span.1, name));
                        }
                    }
                    _ => {
                        self.validate_type(ty, *span)?;
                        if type_size_and_align(ty).is_err() {
                            return Err(format!(
                                "{}:{}: sizeof needs a struct or a type that can be stored in memory, got {:?}",
                                span.0, span.1, ty
                            ));
                        }
                    }
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
                    if let Type::Enum(e) = ty {
                        return Err(format!("{}:{}: enum.cast {} needs an i32 value, got {:?}", span.0, span.1, e, t));
                    }
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
            Expr::Make { union_name, variant, args, span } => {
                let (l, c) = *span;
                let Some(u) = self.union_defs.get(union_name) else {
                    return Err(format!("{}:{}: Unknown union '{}' in make", l, c, union_name));
                };
                let Some(v) = u.variants.iter().find(|v| &v.name == variant) else {
                    return Err(format!("{}:{}: union '{}' has no variant '{}'", l, c, union_name, variant));
                };
                if args.len() != v.fields.len() {
                    return Err(format!(
                        "{}:{}: make {}.{} takes {} values (its fields), got {}",
                        l, c, union_name, variant, v.fields.len(), args.len()
                    ));
                }
                for (a, f) in args.iter().zip(v.fields.iter()) {
                    let at = self.infer_expr_type(a, env)?;
                    if at != f.ty {
                        return Err(format!("{}:{}: make {}.{}: field '{}' is {:?}, got {:?}", l, c, union_name, variant, f.name, f.ty, at));
                    }
                }
                Ok(Type::Union(union_name.clone()))
            }
            Expr::Match { value, arms, else_body, span } => {
                let (l, c) = *span;
                let vt = self.infer_expr_type(value, env)?;
                // the members in order, with each union variant's fields
                let (tname, members): (String, Vec<(String, Option<&Vec<StructField>>)>) = match &vt {
                    Type::Union(u) => (u.clone(), self.union_defs[u].variants.iter().map(|v| (v.name.clone(), Some(&v.fields))).collect()),
                    Type::Enum(e) => (e.clone(), self.enum_defs[e].members.iter().map(|(m, _)| (m.clone(), None)).collect()),
                    other => return Err(format!("{}:{}: match needs a union or enum value, got {:?}", l, c, other)),
                };
                let mut seen: Vec<&str> = Vec::new();
                let mut arm_types = Vec::new();
                for arm in arms {
                    let (al, ac) = arm.span;
                    let member = match arm.member.rfind('.') {
                        Some(dot) if arm.member[..dot] == tname => &arm.member[dot + 1..],
                        _ => {
                            return Err(format!("{}:{}: a match arm on {} is ({}.member ...), got {}", al, ac, tname, tname, arm.member))
                        }
                    };
                    let Some((_, fields)) = members.iter().find(|(m, _)| m == member) else {
                        return Err(format!("{}:{}: {} has no member '{}'", al, ac, tname, member));
                    };
                    if seen.contains(&member) {
                        return Err(format!("{}:{}: {}.{} is matched twice", al, ac, tname, member));
                    }
                    seen.push(member);
                    let mut arm_env = env.clone();
                    match (fields, &arm.binders) {
                        (None, Some(_)) => {
                            return Err(format!("{}:{}: {}.{} is an enum member; its arm binds nothing", al, ac, tname, member))
                        }
                        (None, None) => {}
                        (Some(fields), binders) => {
                            let names: &[String] = binders.as_deref().unwrap_or(&[]);
                            if names.len() != fields.len() {
                                let want: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
                                return Err(format!(
                                    "{}:{}: the {}.{} arm binds its {} fields in order: [{}], got {} names",
                                    al, ac, tname, member, fields.len(), want.join(" "), names.len()
                                ));
                            }
                            // `_` binds nothing: the field is not read
                            for (n, f) in names.iter().zip(fields.iter()).filter(|(n, _)| n.as_str() != "_") {
                                if arm_env.contains_key(n) {
                                    return Err(format!("{}:{}: Cannot shadow existing variable '{}' in the {}.{} arm", al, ac, n, tname, member));
                                }
                                self.declare(n, &f.ty, (al, ac))?;
                                arm_env.insert(n.clone(), f.ty.clone());
                            }
                        }
                    }
                    let mut t = Type::Void;
                    for stmt in &arm.body {
                        t = self.infer_expr_type(stmt, &mut arm_env)?;
                    }
                    arm_types.push((format!("{}.{}", tname, member), t));
                }
                match else_body {
                    Some(body) => {
                        if seen.len() == members.len() {
                            return Err(format!("{}:{}: the else arm never runs: every member of {} is matched", l, c, tname));
                        }
                        let mut else_env = env.clone();
                        let mut t = Type::Void;
                        for stmt in body {
                            t = self.infer_expr_type(stmt, &mut else_env)?;
                        }
                        arm_types.push(("else".to_string(), t));
                    }
                    None => {
                        let missing: Vec<String> =
                            members.iter().filter(|(m, _)| !seen.contains(&m.as_str())).map(|(m, _)| format!("{}.{}", tname, m)).collect();
                        if !missing.is_empty() {
                            return Err(format!("{}:{}: match is missing {} (add those arms or an else arm)", l, c, missing.join(", ")));
                        }
                    }
                }
                let (first_name, first) = &arm_types[0];
                for (name, t) in &arm_types[1..] {
                    if t != first {
                        return Err(format!("{}:{}: match arm type mismatch: {} yields {:?}, {} yields {:?}", l, c, first_name, first, name, t));
                    }
                }
                Ok(first.clone())
            }
            Expr::Addr { val, kind, span } => {
                let t = self.infer_expr_type(val, env)?;
                match (kind, &t) {
                    (AddrKind::Ptr, Type::Ptr(_)) | (AddrKind::Arr, Type::Array(_)) | (AddrKind::Enum, Type::Enum(_)) => Ok(Type::I32),
                    (AddrKind::Ptr, _) => Err(format!("{}:{}: ptr.addr needs a (ptr S), got {:?}", span.0, span.1, t)),
                    (AddrKind::Arr, _) => Err(format!("{}:{}: arr.addr needs an (arr T), got {:?}", span.0, span.1, t)),
                    (AddrKind::Enum, _) => Err(format!("{}:{}: enum.ord needs an enum value, got {:?}", span.0, span.1, t)),
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
            | OpCode::MemStore8
            | OpCode::MemStore32
            | OpCode::MemStore64
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
