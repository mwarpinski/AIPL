//! Prints a `Module` back as canonical AIPL source. Used to hand a resolved
//! (import-flattened) program to the self-hosted compiler, which takes one
//! import-free module: `aipl compile --self` and the self-hosting tests print
//! `Resolver::resolve`'s output and compile that text.
//!
//! Round trip: `Parser::parse(&print_module(m))` is the same program as `m`
//! (spans aside), so both compilers see identical input. Floats print in plain
//! decimal (never exponent notation, which the self-hosted tokenizer lacks).

use crate::ast::*;

pub fn print_module(m: &Module) -> String {
    let mut out = format!("(module {}\n", m.name);
    for imp in &m.imports {
        match &imp.alias {
            Some(a) => out.push_str(&format!("  (import {} as {})\n", imp.name, a)),
            None => out.push_str(&format!("  (import {})\n", imp.name)),
        }
    }
    for s in &m.structs {
        let fields: Vec<String> = s.fields.iter().map(|f| format!("{}:{}", f.name, type_str(&f.ty))).collect();
        out.push_str(&format!("  (struct {} [{}])\n", s.name, fields.join(" ")));
    }
    for f in &m.functions {
        let params: Vec<String> = f.params.iter().map(|(n, t)| format!("{}:{}", n, type_str(t))).collect();
        out.push_str(&format!("  (fn {} [{}] -> {}", f.name, params.join(" "), type_str(&f.return_type)));
        for c in &f.contracts {
            let (kw, e) = match c {
                Contract::Requires(e) => ("req", e),
                Contract::Ensures(e) => ("ens", e),
                Contract::Invariant(e) => ("inv", e),
            };
            out.push_str(&format!("\n    ({} {})", kw, expr_str(e)));
        }
        for e in &f.body {
            out.push_str("\n    ");
            out.push_str(&expr_str(e));
        }
        out.push_str(")\n");
    }
    out.push(')');
    out
}

pub fn type_str(t: &Type) -> String {
    match t {
        Type::I32 => "i32".into(),
        Type::I64 => "i64".into(),
        Type::F32 => "f32".into(),
        Type::F64 => "f64".into(),
        Type::Bool => "bool".into(),
        Type::Str => "str".into(),
        Type::Void => "void".into(),
        Type::Ptr(inner) => format!("(ptr {})", type_str(inner)),
        Type::Struct(name) => name.clone(),
        Type::Array(e) => format!("(arr {})", type_str(e)),
        Type::ResultType(a, b) => format!("(result {} {})", type_str(a), type_str(b)),
        Type::Fn(params, ret) => {
            let ps: Vec<String> = params.iter().map(type_str).collect();
            format!("(fn [{}] -> {})", ps.join(" "), type_str(ret))
        }
    }
}

fn struct_name(t: &Type) -> String {
    match t {
        Type::Ptr(inner) => type_str(inner),
        other => type_str(other),
    }
}

fn elem(t: &Type) -> String {
    match t {
        Type::Array(e) => type_str(e),
        other => type_str(other),
    }
}

fn lit_str(l: &Literal) -> String {
    match l {
        Literal::Int(i) => i.to_string(),
        Literal::Int64(i) => format!("{}i64", i),
        Literal::Float(f) => {
            let s = f.to_string();
            if s.contains('.') { s } else { format!("{}.0", s) }
        }
        Literal::Bool(b) => b.to_string(),
        Literal::Str(s) => {
            let mut out = String::from("\"");
            for c in s.chars() {
                match c {
                    '\n' => out.push_str("\\n"),
                    '\t' => out.push_str("\\t"),
                    '\r' => out.push_str("\\r"),
                    '\0' => out.push_str("\\0"),
                    '\\' => out.push_str("\\\\"),
                    '"' => out.push_str("\\\""),
                    c => out.push(c),
                }
            }
            out.push('"');
            out
        }
    }
}

fn join(es: &[Expr]) -> String {
    es.iter().map(expr_str).collect::<Vec<_>>().join(" ")
}

fn with_body(head: String, body: &[Expr]) -> String {
    if body.is_empty() { format!("({})", head) } else { format!("({} {})", head, join(body)) }
}

pub fn op_name(op: &OpCode) -> &'static str {
    use OpCode::*;
    match op {
        Add => "+",
        Sub => "-",
        Mul => "*",
        Div => "/",
        Mod => "%",
        BitXor => "^",
        Shl => "shl",
        Shr => "shr",
        ShrU => "shru",
        DivU => "divu",
        RemU => "remu",
        BitAnd => "bitand",
        BitOr => "bitor",
        MemLoad8 => "mem.load8",
        MemLoad32 => "mem.load32",
        MemLoad64 => "mem.load64",
        MemLoadF32 => "mem.load_f32",
        MemLoadF64 => "mem.load_f64",
        MemStore8 => "mem.store8",
        MemStore32 => "mem.store32",
        MemStore64 => "mem.store64",
        MemStoreF32 => "mem.store_f32",
        MemStoreF64 => "mem.store_f64",
        MemAlloc => "mem.alloc",
        MemFree => "mem.free",
        MemGrow => "mem.grow",
        StrLen => "str.len",
        StrPtr => "str.ptr",
        AtomicAdd => "atomic.add",
        AtomicCas => "atomic.cas",
        AtomicLock => "atomic.lock",
        AtomicUnlock => "atomic.unlock",
        Eq => "eq",
        Neq => "neq",
        Lt => "lt",
        Lte => "lte",
        Gt => "gt",
        Gte => "gte",
        And => "and",
        Or => "or",
        Not => "not",
        SysPrint => "sys.print",
        SysTime => "sys.time",
        SysMonotonic => "sys.monotonic",
        SysRandom => "sys.random",
        SysExit => "sys.exit",
        FsOpen => "fs.open",
        FsRead => "fs.read",
        FsWrite => "fs.write",
        FsClose => "fs.close",
        FsDelete => "fs.delete",
        ArgsSizes => "args.sizes",
        ArgsGet => "args.get",
        EnvSizes => "env.sizes",
        EnvGet => "env.get",
        ThreadSpawn => "thread.spawn",
        ThreadJoin => "thread.join",
        I64ExtendS => "i64.extend_s",
        I64ExtendU => "i64.extend_u",
        I32Wrap => "i32.wrap",
        F64ConvertI64S => "f64.convert_i64_s",
        I64TruncF64S => "i64.trunc_f64_s",
        F64ReinterpretI64 => "f64.reinterpret_i64",
        I64ReinterpretF64 => "i64.reinterpret_f64",
    }
}

pub fn expr_str(e: &Expr) -> String {
    match e {
        Expr::Lit(l, _) => lit_str(l),
        Expr::Var(n, _) => n.clone(),
        Expr::Let { name, ty, val, .. } => format!("(let {}:{} {})", name, type_str(ty), expr_str(val)),
        Expr::Set { name, val, .. } => format!("(set! {} {})", name, expr_str(val)),
        Expr::If { cond, then_branch, else_branch, .. } => {
            format!("(if {} {} {})", expr_str(cond), expr_str(then_branch), expr_str(else_branch))
        }
        Expr::Loop { var, start, end, step, body, .. } => with_body(
            format!("loop {} {} {} {}", var, expr_str(start), expr_str(end), expr_str(step)),
            body,
        ),
        Expr::While { cond, body, .. } => with_body(format!("while {}", expr_str(cond)), body),
        Expr::Call { func, args, .. } => with_body(format!("call {}", func), args),
        Expr::Op { op, args, .. } => with_body(op_name(op).to_string(), args),
        Expr::Ok(v, ty, _) => match ty {
            Some(t) => format!("(ok:{} {})", type_str(t), expr_str(v)),
            None => format!("(ok {})", expr_str(v)),
        },
        Expr::Err(v, ty, _) => match ty {
            Some(t) => format!("(err:{} {})", type_str(t), expr_str(v)),
            None => format!("(err {})", expr_str(v)),
        },
        Expr::MatchResult { expr, ok_var, ok_body, err_var, err_body, .. } => format!(
            "(match_result {} {} {})",
            expr_str(expr),
            with_body(format!("ok {}", ok_var), ok_body),
            with_body(format!("err {}", err_var), err_body)
        ),
        Expr::Block(body, _) => with_body("block".to_string(), body),
        Expr::NewStruct { struct_name, .. } => format!("(new {})", struct_name),
        Expr::GetField { struct_name, field_name, ptr, .. } => {
            format!("(get {} {}.{})", expr_str(ptr), struct_name, field_name)
        }
        Expr::PutField { struct_name, field_name, ptr, val, .. } => {
            format!("(put {} {}.{} {})", expr_str(ptr), struct_name, field_name, expr_str(val))
        }
        Expr::Sizeof { struct_name, .. } => format!("(sizeof {})", struct_name),
        Expr::ArrNew { elem_ty, size, .. } => format!("(arr.new {} {})", type_str(elem_ty), expr_str(size)),
        Expr::ArrGet { elem_ty, ptr, index, .. } => {
            format!("(arr.get {} {} {})", type_str(elem_ty), expr_str(ptr), expr_str(index))
        }
        Expr::ArrSet { elem_ty, ptr, index, val, .. } => format!(
            "(arr.set {} {} {} {})",
            type_str(elem_ty),
            expr_str(ptr),
            expr_str(index),
            expr_str(val)
        ),
        Expr::ArrLen { arr, .. } => format!("(arr.len {})", expr_str(arr)),
        Expr::Null { ty, .. } => match ty {
            Type::Array(_) => format!("(arr.null {})", elem(ty)),
            _ => format!("(ptr.null {})", struct_name(ty)),
        },
        Expr::Cast { ty, addr, .. } => match ty {
            Type::Array(_) => format!("(arr.cast {} {})", elem(ty), expr_str(addr)),
            _ => format!("(ptr.cast {} {})", struct_name(ty), expr_str(addr)),
        },
        Expr::Ref { name, .. } => format!("(ref {})", name),
        Expr::Return { val, .. } => match val {
            Some(v) => format!("(return {})", expr_str(v)),
            None => "(return)".to_string(),
        },
        Expr::Break(_) => "(break)".to_string(),
        Expr::Continue(_) => "(continue)".to_string(),
        Expr::CallRef { sig, func, args, .. } => {
            with_body(format!("call_ref {} {}", type_str(sig), expr_str(func)), args)
        }
        Expr::Addr { val, array, .. } => {
            format!("({} {})", if *array { "arr.addr" } else { "ptr.addr" }, expr_str(val))
        }
    }
}
