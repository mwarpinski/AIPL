use crate::ast::*;
use std::collections::HashMap;
use wasm_encoder::{
    BlockType, CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection,
    ImportSection, Instruction, MemArg, Module as WasmModule, TypeSection, ValType,
};

pub struct WasmCompiler;

impl WasmCompiler {
    pub fn compile(module: &Module) -> Result<Vec<u8>, String> {
        let mut wasm_module = WasmModule::new();
        let mut types = TypeSection::new();
        let mut functions = FunctionSection::new();
        let mut exports = ExportSection::new();
        let mut codes = CodeSection::new();

        let mut imports = ImportSection::new();
        let mut fn_indices: HashMap<String, u32> = HashMap::new();
        let mut fn_returns: HashMap<String, Type> = HashMap::new();

        // 0. WASI imports, only for the host functions this module actually
        // uses, in a fixed order. A module with no I/O has no import section
        // and instantiates with no imports, exactly as before.
        let used_wasi = collect_wasi_imports(module);
        let mut wasi_indices: HashMap<Wasi, u32> = HashMap::new();
        for (idx, w) in used_wasi.iter().enumerate() {
            let (params, results) = w.signature();
            types.ty().function(params, results);
            imports.import("wasi_snapshot_preview1", w.name(), EntityType::Function(idx as u32));
            wasi_indices.insert(*w, idx as u32);
        }
        let import_count = used_wasi.len() as u32;

        // String literals: interned once into a data segment at 512..1024 as
        // [len: u32 LE][bytes]; a `str` value is the pointer to the bytes.
        let mut string_blob: Vec<u8> = Vec::new();
        let mut strings: HashMap<String, u32> = HashMap::new();
        let intern = |s: &str, blob: &mut Vec<u8>, map: &mut HashMap<String, u32>| -> u32 {
            if let Some(&a) = map.get(s) {
                return a;
            }
            let addr = STRING_DATA_BASE + blob.len() as u32 + 4;
            blob.extend_from_slice(&(s.len() as u32).to_le_bytes());
            blob.extend_from_slice(s.as_bytes());
            map.insert(s.to_string(), addr);
            addr
        };
        let mut newline_addr = 0u32;
        if used_wasi.contains(&Wasi::FdWrite) && module_uses_op(module, &OpCode::SysPrint) {
            newline_addr = intern("\n", &mut string_blob, &mut strings);
        }
        walk_module(module, &mut |e| {
            if let Expr::Lit(Literal::Str(s), _) = e {
                intern(s, &mut string_blob, &mut strings);
            }
        });
        if string_blob.len() > STRING_DATA_CAPACITY as usize {
            return Err(format!(
                "Wasm Codegen: string literals need {} bytes but the string data area (addresses {}..{}) holds {}",
                string_blob.len(),
                STRING_DATA_BASE,
                STRING_DATA_BASE + STRING_DATA_CAPACITY,
                STRING_DATA_CAPACITY
            ));
        }

        let mut structs: HashMap<String, StructDef> = HashMap::new();
        for s in &module.structs {
            structs.insert(s.name.clone(), s.clone());
        }

        // 1. Build type section and function index mapping (after the imports)
        for (i, f) in module.functions.iter().enumerate() {
            let idx = import_count + i as u32;
            let params: Vec<ValType> = f.params.iter().map(|(_, t)| self::aipl_to_wasm_type(t)).collect();
            let results: Vec<ValType> = if f.return_type == Type::Void {
                vec![]
            } else {
                vec![self::aipl_to_wasm_type(&f.return_type)]
            };

            types.ty().function(params, results);
            functions.function(idx);
            exports.export(&f.name, ExportKind::Func, idx);
            fn_indices.insert(f.name.clone(), idx);
            fn_returns.insert(f.name.clone(), f.return_type.clone());
        }

        // Function references: `(ref f)` is f's position among the module's
        // functions, which is its slot in a funcref table holding every
        // function. `call_ref` lowers to call_indirect with a type index from
        // one extra type per distinct signature (wasm-level, first-use order),
        // appended after the function types. Table and element section are
        // emitted only when the module uses ref or call_ref.
        let mut fn_types: HashMap<String, Type> = HashMap::new();
        for f in &module.functions {
            let params = f.params.iter().map(|(_, t)| t.clone()).collect();
            fn_types.insert(f.name.clone(), Type::Fn(params, Box::new(f.return_type.clone())));
        }
        let mut uses_refs = false;
        let mut ref_sigs: Vec<(Vec<ValType>, Vec<ValType>)> = Vec::new();
        walk_module(module, &mut |e| match e {
            Expr::Ref { .. } => uses_refs = true,
            Expr::CallRef { sig, .. } => {
                uses_refs = true;
                let key = wasm_signature(sig);
                if !ref_sigs.contains(&key) {
                    ref_sigs.push(key);
                }
            }
            _ => {}
        });
        let ref_type_base = import_count + module.functions.len() as u32;
        for (params, results) in &ref_sigs {
            types.ty().function(params.clone(), results.clone());
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

            // One extra i32 local per function: scratch for write-address checks and allocation
            let addr_scratch = current_idx;
            wasm_locals.push((1, ValType::I32));
            current_idx += 1;

            // Two more i32 temps, only for functions that perform I/O: the
            // WASI lowerings evaluate all their operands first (source order,
            // nested I/O safe) and then unload them into these.
            let io_locals = if fn_uses_io(&f.body) {
                wasm_locals.push((2, ValType::I32));
                Some((current_idx, current_idx + 1))
            } else {
                None
            };

            let mut func = Function::new(wasm_locals);
            let ctx = Ctx {
                locals: &local_map,
                addr_scratch,
                io_locals,
                local_types: &local_types,
                fn_indices: &fn_indices,
                fn_returns: &fn_returns,
                strings: &strings,
                wasi: &wasi_indices,
                newline_addr,
                structs: &structs,
                fn_types: &fn_types,
                import_count,
                ref_sigs: &ref_sigs,
                ref_type_base,
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
        // 16 pages (1 MiB) to start, matching the VM, so `mem.grow` reports the
        // same old size in both backends; 100 pages max, also matching the VM.
        memories.memory(wasm_encoder::MemoryType {
            minimum: 16,
            maximum: Some(100),
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        exports.export("memory", ExportKind::Memory, 0);

        // Runtime block initialisation: the heap cursor at address 0 starts
        // at 1024 (HEAP_START). Everything else in bytes 0..1024 is zero.
        let mut data = wasm_encoder::DataSection::new();
        data.active(0, &wasm_encoder::ConstExpr::i32_const(0), 1024u32.to_le_bytes());
        if !string_blob.is_empty() {
            data.active(0, &wasm_encoder::ConstExpr::i32_const(STRING_DATA_BASE as i32), string_blob.iter().copied());
        }

        wasm_module.section(&types);
        if import_count > 0 {
            wasm_module.section(&imports);
        }
        wasm_module.section(&functions);
        let n_fns = module.functions.len() as u32;
        if uses_refs {
            let mut tables = wasm_encoder::TableSection::new();
            tables.table(wasm_encoder::TableType {
                element_type: wasm_encoder::RefType::FUNCREF,
                table64: false,
                minimum: n_fns as u64,
                maximum: Some(n_fns as u64),
                shared: false,
            });
            wasm_module.section(&tables);
        }
        wasm_module.section(&memories);
        wasm_module.section(&exports);
        if uses_refs {
            let indices: Vec<u32> = (0..n_fns).map(|i| import_count + i).collect();
            let mut elems = wasm_encoder::ElementSection::new();
            elems.active(
                None,
                &wasm_encoder::ConstExpr::i32_const(0),
                wasm_encoder::Elements::Functions(std::borrow::Cow::Owned(indices)),
            );
            wasm_module.section(&elems);
        }
        wasm_module.section(&codes);
        wasm_module.section(&data);

        Ok(wasm_module.finish())
    }
}

/// Bundles the read-only context threaded through codegen so it isn't passed
/// as four separate parameters everywhere.
struct Ctx<'a> {
    locals: &'a HashMap<String, u32>,
    addr_scratch: u32,
    /// `(a, b)` scratch locals for WASI lowerings; `None` when the function does no I/O.
    io_locals: Option<(u32, u32)>,
    local_types: &'a HashMap<String, Type>,
    fn_indices: &'a HashMap<String, u32>,
    fn_returns: &'a HashMap<String, Type>,
    /// Interned string literal -> address of its bytes in the data segment.
    strings: &'a HashMap<String, u32>,
    /// WASI import -> function index.
    wasi: &'a HashMap<Wasi, u32>,
    /// Address of the interned "\n" used by sys.print (0 if unused).
    newline_addr: u32,
    structs: &'a HashMap<String, StructDef>,
    /// Function name -> its `(fn [...] -> r)` type, for `(ref f)`.
    fn_types: &'a HashMap<String, Type>,
    import_count: u32,
    /// Distinct call_ref signatures; signature i has type index ref_type_base + i.
    ref_sigs: &'a [(Vec<ValType>, Vec<ValType>)],
    ref_type_base: u32,
}

/// The wasm params/results of a `(fn [...] -> r)` type.
fn wasm_signature(sig: &Type) -> (Vec<ValType>, Vec<ValType>) {
    match sig {
        Type::Fn(params, ret) => (
            params.iter().map(aipl_to_wasm_type).collect(),
            if **ret == Type::Void { vec![] } else { vec![aipl_to_wasm_type(ret)] },
        ),
        _ => (vec![], vec![]),
    }
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
        Expr::Let { .. } | Expr::Set { .. } => Type::Void,
        Expr::If { then_branch, .. } => expr_type(then_branch, ctx),
        Expr::Block(exprs, _) => exprs.last().map_or(Type::Void, |e| expr_type(e, ctx)),
        Expr::Loop { .. } | Expr::While { .. } => Type::Void,
        Expr::Call { func, .. } => ctx.fn_returns.get(func).cloned().unwrap_or(Type::I32),
        Expr::Ok(inner, _, _) | Expr::Err(inner, _, _) => expr_type(inner, ctx),
        Expr::MatchResult { ok_body, .. } => ok_body.last().map_or(Type::Void, |e| expr_type(e, ctx)),
        Expr::NewStruct { struct_name, .. } => Type::Ptr(Box::new(Type::Struct(struct_name.clone()))),
        Expr::GetField { struct_name, field_name, .. } => {
            if let Some(def) = ctx.structs.get(struct_name) {
                if let Ok((_, field_ty)) = crate::checker::get_field_offset(def, field_name) {
                    return field_ty;
                }
            }
            Type::I32
        }
        Expr::PutField { .. } => Type::Void,
        Expr::Sizeof { .. } => Type::I32,
        Expr::ArrNew { elem_ty, .. } => Type::Array(Box::new(elem_ty.clone())),
        Expr::ArrLen { .. } | Expr::Addr { .. } => Type::I32,
        Expr::Null { ty, .. } | Expr::Cast { ty, .. } => ty.clone(),
        Expr::Ref { name, .. } => ctx.fn_types.get(name).cloned().unwrap_or(Type::I32),
        Expr::CallRef { sig, .. } => match sig {
            Type::Fn(_, ret) => (**ret).clone(),
            _ => Type::I32,
        },
        Expr::ArrGet { elem_ty, .. } => elem_ty.clone(),
        Expr::ArrSet { .. } => Type::Void,
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
            OpCode::MemLoad64
            | OpCode::I64ExtendS
            | OpCode::I64ExtendU
            | OpCode::I64TruncF64S
            | OpCode::I64ReinterpretF64 => Type::I64,
            OpCode::F64ConvertI64S | OpCode::F64ReinterpretI64 => Type::F64,
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
            | OpCode::SysPrint
            | OpCode::SysExit => Type::Void,
            OpCode::MemLoad8
            | OpCode::MemLoad32
            | OpCode::MemAlloc
            | OpCode::MemGrow
            | OpCode::AtomicAdd
            | OpCode::I32Wrap
            | OpCode::FsOpen
            | OpCode::FsRead
            | OpCode::FsWrite
            | OpCode::FsClose
            | OpCode::FsDelete
            | OpCode::ThreadSpawn
            | OpCode::ThreadJoin
            | OpCode::StrLen
            | OpCode::StrPtr => Type::I32,
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
        (OpCode::Eq, Type::I32 | Type::Bool | Type::Str | Type::Ptr(_) | Type::Array(_) | Type::Fn(_, _)) => I32Eq,
        (OpCode::Neq, Type::I32 | Type::Bool | Type::Str | Type::Ptr(_) | Type::Array(_) | Type::Fn(_, _)) => I32Ne,
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
            Expr::CallRef { func, args, .. } => {
                collect_lets(&[*(func.clone())], lets);
                collect_lets(args, lets);
            }
            Expr::MatchResult { expr, ok_var, ok_body, err_var, err_body, .. } => {
                collect_lets(&[*(expr.clone())], lets);
                lets.push((ok_var.clone(), Type::I32));
                lets.push((err_var.clone(), Type::I32));
                collect_lets(ok_body, lets);
                collect_lets(err_body, lets);
            }
            Expr::Ok(inner, _, _) | Expr::Err(inner, _, _) => {
                collect_lets(&[*(inner.clone())], lets);
            }
            Expr::GetField { ptr, .. }
            | Expr::ArrNew { size: ptr, .. }
            | Expr::ArrLen { arr: ptr, .. }
            | Expr::Cast { addr: ptr, .. }
            | Expr::Addr { val: ptr, .. } => {
                collect_lets(&[*(ptr.clone())], lets);
            }
            Expr::PutField { ptr, val, .. } => {
                collect_lets(&[*(ptr.clone())], lets);
                collect_lets(&[*(val.clone())], lets);
            }
            Expr::ArrGet { ptr, index, .. } => {
                collect_lets(&[*(ptr.clone())], lets);
                collect_lets(&[*(index.clone())], lets);
            }
            Expr::ArrSet { ptr, index, val, .. } => {
                collect_lets(&[*(ptr.clone())], lets);
                collect_lets(&[*(index.clone())], lets);
                collect_lets(&[*(val.clone())], lets);
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
        // Pointers, arrays, results, and function refs are i32 addresses/indices.
        Type::Ptr(_) | Type::Struct(_) | Type::ResultType(_, _) | Type::Array(_) | Type::Fn(_, _) => ValType::I32,
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
            Literal::Str(s) => {
                let addr = *ctx
                    .strings
                    .get(s)
                    .ok_or_else(|| format!("Wasm Codegen: string literal {:?} was not interned", s))?;
                func.instruction(&Instruction::I32Const(addr as i32));
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
            let then_void = is_void_expr(then_branch, ctx);
            if then_void {
                func.instruction(&Instruction::If(BlockType::Empty));
                compile_expr(then_branch, ctx, func)?;
                func.instruction(&Instruction::Else);
                compile_expr(else_branch, ctx, func)?;
                func.instruction(&Instruction::End);
            } else {
                let ty = expr_type(then_branch, ctx);
                func.instruction(&Instruction::If(BlockType::Result(aipl_to_wasm_type(&ty))));
                compile_expr(then_branch, ctx, func)?;
                func.instruction(&Instruction::Else);
                compile_expr(else_branch, ctx, func)?;
                func.instruction(&Instruction::End);
            }
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
                emit_write_address_check(func, ctx.addr_scratch);
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I32Store8(wasm_encoder::MemArg { offset: 0, align: 0, memory_index: 0 }));
            }
            OpCode::MemLoad32 => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::I32Load(wasm_encoder::MemArg { offset: 0, align: 2, memory_index: 0 }));
            }
            OpCode::MemStore32 => {
                compile_expr(&args[0], ctx, func)?;
                emit_write_address_check(func, ctx.addr_scratch);
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I32Store(wasm_encoder::MemArg { offset: 0, align: 2, memory_index: 0 }));
            }
            OpCode::MemLoad64 => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::I64Load(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
            }
            OpCode::MemStore64 => {
                compile_expr(&args[0], ctx, func)?;
                emit_write_address_check(func, ctx.addr_scratch);
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
            }
            OpCode::MemAlloc => {
                // Bump allocator whose cursor is the i32 at linear-memory
                // address 0 (HEAP_PTR_ADDR) - the same word the VM uses, so
                // both backends and self-hosted AIPL share one allocator.
                // Stack: [old] [0] [old] [size] -> add -> [old] [0] [new] -> store -> [old]
                let cursor = wasm_encoder::MemArg { offset: 0, align: 2, memory_index: 0 };
                func.instruction(&Instruction::I32Const(0));
                func.instruction(&Instruction::I32Load(cursor));
                func.instruction(&Instruction::I32Const(0));
                func.instruction(&Instruction::I32Const(0));
                func.instruction(&Instruction::I32Load(cursor));
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::I32Add);
                func.instruction(&Instruction::I32Store(cursor));
            }
            OpCode::MemGrow => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::MemoryGrow(0));
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
            OpCode::F64ConvertI64S => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::F64ConvertI64S);
            }
            OpCode::I64TruncF64S => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::I64TruncF64S);
            }
            OpCode::F64ReinterpretI64 => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::F64ReinterpretI64);
            }
            OpCode::I64ReinterpretF64 => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::I64ReinterpretF64);
            }
            OpCode::SysPrint => {
                // Each argument is written to fd 1 followed by "\n", via a
                // two-entry iovec in the runtime block (see RT_* cells).
                let fd_write = wasi_index(ctx, Wasi::FdWrite)?;
                for arg in args {
                    let ty = expr_type(arg, ctx);
                    if ty != Type::Str {
                        return Err(format!(
                            "Wasm Codegen: sys.print supports str arguments only in the wasm backend, got {:?}",
                            ty
                        ));
                    }
                    compile_expr(arg, ctx, func)?;
                    func.instruction(&Instruction::LocalSet(ctx.addr_scratch));
                    // iov0 = { ptr, len }
                    func.instruction(&Instruction::I32Const(RT_IOV0_BUF));
                    func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
                    func.instruction(&Instruction::I32Store(M4));
                    func.instruction(&Instruction::I32Const(RT_IOV0_LEN));
                    func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
                    func.instruction(&Instruction::I32Const(4));
                    func.instruction(&Instruction::I32Sub);
                    func.instruction(&Instruction::I32Load(M4));
                    func.instruction(&Instruction::I32Store(M4));
                    // iov1 = { "\n", 1 }
                    func.instruction(&Instruction::I32Const(RT_IOV1_BUF));
                    func.instruction(&Instruction::I32Const(ctx.newline_addr as i32));
                    func.instruction(&Instruction::I32Store(M4));
                    func.instruction(&Instruction::I32Const(RT_IOV1_LEN));
                    func.instruction(&Instruction::I32Const(1));
                    func.instruction(&Instruction::I32Store(M4));
                    // One fd_write per iovec: WASI permits a short write and
                    // wasmtime's implementation writes only the first iovec of
                    // a call, so a single 2-iovec call would drop the newline.
                    // fd_write(1, iovs=64, iovs_len=1, nwritten=80); errno dropped
                    func.instruction(&Instruction::I32Const(1));
                    func.instruction(&Instruction::I32Const(RT_IOV0_BUF));
                    func.instruction(&Instruction::I32Const(1));
                    func.instruction(&Instruction::I32Const(RT_NBYTES));
                    func.instruction(&Instruction::Call(fd_write));
                    func.instruction(&Instruction::Drop);
                    // fd_write(1, iovs=72, iovs_len=1, nwritten=80)
                    func.instruction(&Instruction::I32Const(1));
                    func.instruction(&Instruction::I32Const(RT_IOV1_BUF));
                    func.instruction(&Instruction::I32Const(1));
                    func.instruction(&Instruction::I32Const(RT_NBYTES));
                    func.instruction(&Instruction::Call(fd_write));
                    func.instruction(&Instruction::Drop);
                }
            }
            OpCode::StrLen => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::I32Const(4));
                func.instruction(&Instruction::I32Sub);
                func.instruction(&Instruction::I32Load(M4));
            }
            OpCode::StrPtr => {
                // A str value already is the address of its bytes.
                compile_expr(&args[0], ctx, func)?;
            }

            OpCode::MemLoadF32 | OpCode::MemLoadF64 | OpCode::MemStoreF32 | OpCode::MemStoreF64 => {
                return Err(format!("Wasm Codegen: {:?} is not supported in the wasm backend", op));
            }
            OpCode::AtomicAdd | OpCode::AtomicCas | OpCode::AtomicLock | OpCode::AtomicUnlock => {
                return Err(format!("Wasm Codegen: {:?} is not supported in the wasm backend (needs shared memory + atomics)", op));
            }
            OpCode::SysTime => {
                return Err(format!("Wasm Codegen: {:?} is not supported in the wasm backend", op));
            }
            OpCode::SysExit => {
                let proc_exit = wasi_index(ctx, Wasi::ProcExit)?;
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::Call(proc_exit));
            }

            OpCode::FsOpen => {
                // (fs.open ptr len flags) -> path_open on the preopened dir (fd 3).
                // flags == 0: read-only; otherwise create + truncate for writing.
                // Returns the new fd, or -1 on any errno. Mirrors vm.rs FsOpen.
                let path_open = wasi_index(ctx, Wasi::PathOpen)?;
                let (io_a, io_b) = io_locals(ctx)?;
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                compile_expr(&args[2], ctx, func)?;
                func.instruction(&Instruction::LocalSet(io_b)); // flags
                func.instruction(&Instruction::LocalSet(io_a)); // len
                func.instruction(&Instruction::LocalSet(ctx.addr_scratch)); // ptr
                func.instruction(&Instruction::I32Const(WASI_PREOPEN_FD));
                func.instruction(&Instruction::I32Const(0)); // dirflags
                func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
                func.instruction(&Instruction::LocalGet(io_a));
                // oflags: CREAT|TRUNC when flags != 0
                func.instruction(&Instruction::LocalGet(io_b));
                func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
                func.instruction(&Instruction::I32Const(WASI_OFLAGS_CREAT | WASI_OFLAGS_TRUNC));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I32Const(0));
                func.instruction(&Instruction::End);
                // rights_base: FD_READ|FD_WRITE when writing, FD_READ otherwise
                func.instruction(&Instruction::LocalGet(io_b));
                func.instruction(&Instruction::If(BlockType::Result(ValType::I64)));
                func.instruction(&Instruction::I64Const(WASI_RIGHT_FD_READ | WASI_RIGHT_FD_WRITE));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(WASI_RIGHT_FD_READ));
                func.instruction(&Instruction::End);
                func.instruction(&Instruction::I64Const(0)); // rights_inheriting
                func.instruction(&Instruction::I32Const(0)); // fdflags
                func.instruction(&Instruction::I32Const(RT_OPENED_FD));
                func.instruction(&Instruction::Call(path_open));
                emit_errno_to_result(func, Some(RT_OPENED_FD));
            }
            OpCode::FsRead | OpCode::FsWrite => {
                // (fs.read fd buf max) / (fs.write fd buf len) -> one iovec,
                // returns bytes transferred or -1. Mirrors vm.rs.
                let host = wasi_index(ctx, if *op == OpCode::FsRead { Wasi::FdRead } else { Wasi::FdWrite })?;
                let (io_a, io_b) = io_locals(ctx)?;
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                compile_expr(&args[2], ctx, func)?;
                func.instruction(&Instruction::LocalSet(io_b)); // len
                func.instruction(&Instruction::LocalSet(io_a)); // buf
                func.instruction(&Instruction::LocalSet(ctx.addr_scratch)); // fd
                func.instruction(&Instruction::I32Const(RT_IOV0_BUF));
                func.instruction(&Instruction::LocalGet(io_a));
                func.instruction(&Instruction::I32Store(M4));
                func.instruction(&Instruction::I32Const(RT_IOV0_LEN));
                func.instruction(&Instruction::LocalGet(io_b));
                func.instruction(&Instruction::I32Store(M4));
                func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
                func.instruction(&Instruction::I32Const(RT_IOV0_BUF));
                func.instruction(&Instruction::I32Const(1));
                func.instruction(&Instruction::I32Const(RT_NBYTES));
                func.instruction(&Instruction::Call(host));
                emit_errno_to_result(func, Some(RT_NBYTES));
            }
            OpCode::FsClose => {
                let fd_close = wasi_index(ctx, Wasi::FdClose)?;
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::Call(fd_close));
                emit_errno_to_result(func, None);
            }
            OpCode::FsDelete => {
                let unlink = wasi_index(ctx, Wasi::PathUnlinkFile)?;
                let (io_a, _) = io_locals(ctx)?;
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::LocalSet(io_a)); // len
                func.instruction(&Instruction::LocalSet(ctx.addr_scratch)); // ptr
                func.instruction(&Instruction::I32Const(WASI_PREOPEN_FD));
                func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
                func.instruction(&Instruction::LocalGet(io_a));
                func.instruction(&Instruction::Call(unlink));
                emit_errno_to_result(func, None);
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
        Expr::Ok(inner, ..) => compile_result_cell(0, inner, ctx, func)?,
        Expr::Err(inner, ..) => compile_result_cell(1, inner, ctx, func)?,
        Expr::MatchResult {
            expr,
            ok_var,
            ok_body,
            err_var,
            err_body,
            ..
        } => {
            compile_expr(expr, ctx, func)?;
            func.instruction(&Instruction::LocalSet(ctx.addr_scratch));
            func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
            func.instruction(&Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }));
            func.instruction(&Instruction::I32Eqz);

            let last_ok = ok_body.last();
            let block_ty = if last_ok.map_or(true, |e| is_void_expr(e, ctx)) {
                wasm_encoder::BlockType::Empty
            } else {
                let ty = expr_type(last_ok.unwrap(), ctx);
                wasm_encoder::BlockType::Result(aipl_to_wasm_type(&ty))
            };

            func.instruction(&Instruction::If(block_ty));

            if let Some(&ok_idx) = ctx.locals.get(ok_var) {
                func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
                func.instruction(&Instruction::I32Load(wasm_encoder::MemArg {
                    offset: 4,
                    align: 2,
                    memory_index: 0,
                }));
                func.instruction(&Instruction::LocalSet(ok_idx));
            }
            for (i, stmt) in ok_body.iter().enumerate() {
                if i == ok_body.len() - 1 && !matches!(block_ty, wasm_encoder::BlockType::Empty) {
                    compile_expr(stmt, ctx, func)?;
                } else {
                    compile_stmt(stmt, ctx, func)?;
                }
            }

            func.instruction(&Instruction::Else);

            if let Some(&err_idx) = ctx.locals.get(err_var) {
                func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
                func.instruction(&Instruction::I32Load(wasm_encoder::MemArg {
                    offset: 4,
                    align: 2,
                    memory_index: 0,
                }));
                func.instruction(&Instruction::LocalSet(err_idx));
            }
            for (i, stmt) in err_body.iter().enumerate() {
                if i == err_body.len() - 1 && !matches!(block_ty, wasm_encoder::BlockType::Empty) {
                    compile_expr(stmt, ctx, func)?;
                } else {
                    compile_stmt(stmt, ctx, func)?;
                }
            }

            func.instruction(&Instruction::End);
        }
        Expr::NewStruct { struct_name, .. } => {
            let def = ctx
                .structs
                .get(struct_name)
                .ok_or_else(|| format!("Wasm Codegen: Unknown struct '{}'", struct_name))?;
            let size = crate::checker::get_struct_size(def)?;
            let cursor = wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            };
            func.instruction(&Instruction::I32Const(0));
            func.instruction(&Instruction::I32Load(cursor));
            func.instruction(&Instruction::I32Const(0));
            func.instruction(&Instruction::I32Const(0));
            func.instruction(&Instruction::I32Load(cursor));
            func.instruction(&Instruction::I32Const(size as i32));
            func.instruction(&Instruction::I32Add);
            func.instruction(&Instruction::I32Store(cursor));
        }
        Expr::GetField {
            struct_name,
            field_name,
            ptr,
            ..
        } => {
            let def = ctx
                .structs
                .get(struct_name)
                .ok_or_else(|| format!("Wasm Codegen: Unknown struct '{}'", struct_name))?;
            let (offset, field_ty) = crate::checker::get_field_offset(def, field_name)?;
            compile_expr(ptr, ctx, func)?;
            match field_ty {
                Type::I32 | Type::Bool | Type::Str | Type::Ptr(_) | Type::Array(_) | Type::Fn(_, _) => {
                    func.instruction(&Instruction::I32Load(wasm_encoder::MemArg {
                        offset: offset as u64,
                        align: 2,
                        memory_index: 0,
                    }));
                    if field_ty == Type::Bool {
                        normalize_bool(func);
                    }
                }
                Type::I64 => {
                    func.instruction(&Instruction::I64Load(wasm_encoder::MemArg {
                        offset: offset as u64,
                        align: 3,
                        memory_index: 0,
                    }));
                }
                Type::F32 => {
                    func.instruction(&Instruction::F32Load(wasm_encoder::MemArg {
                        offset: offset as u64,
                        align: 2,
                        memory_index: 0,
                    }));
                }
                Type::F64 => {
                    func.instruction(&Instruction::F64Load(wasm_encoder::MemArg {
                        offset: offset as u64,
                        align: 3,
                        memory_index: 0,
                    }));
                }
                _ => return Err(format!("Unsupported field type for struct get: {:?}", field_ty)),
            }
        }
        Expr::PutField {
            struct_name,
            field_name,
            ptr,
            val,
            ..
        } => {
            let def = ctx
                .structs
                .get(struct_name)
                .ok_or_else(|| format!("Wasm Codegen: Unknown struct '{}'", struct_name))?;
            let (offset, field_ty) = crate::checker::get_field_offset(def, field_name)?;
            compile_expr(ptr, ctx, func)?;
            emit_write_address_check(func, ctx.addr_scratch);
            compile_expr(val, ctx, func)?;
            match field_ty {
                Type::I32 | Type::Bool | Type::Str | Type::Ptr(_) | Type::Array(_) | Type::Fn(_, _) => {
                    func.instruction(&Instruction::I32Store(wasm_encoder::MemArg {
                        offset: offset as u64,
                        align: 2,
                        memory_index: 0,
                    }));
                }
                Type::I64 => {
                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                        offset: offset as u64,
                        align: 3,
                        memory_index: 0,
                    }));
                }
                Type::F32 => {
                    func.instruction(&Instruction::F32Store(wasm_encoder::MemArg {
                        offset: offset as u64,
                        align: 2,
                        memory_index: 0,
                    }));
                }
                Type::F64 => {
                    func.instruction(&Instruction::F64Store(wasm_encoder::MemArg {
                        offset: offset as u64,
                        align: 3,
                        memory_index: 0,
                    }));
                }
                _ => return Err(format!("Unsupported field type for struct put: {:?}", field_ty)),
            }
        }
        Expr::Sizeof { struct_name, .. } => {
            let def = ctx
                .structs
                .get(struct_name)
                .ok_or_else(|| format!("Wasm Codegen: Unknown struct '{}'", struct_name))?;
            let size = crate::checker::get_struct_size(def)?;
            func.instruction(&Instruction::I32Const(size as i32));
        }
        Expr::ArrNew { elem_ty, size, .. } => {
            // Same order as the VM: evaluate n, trap if n < 0, then allocate
            // [n:i32][n elements] and return the address just past the header.
            let (elem_size, _) = crate::checker::type_size_and_align(elem_ty)?;
            compile_expr(size, ctx, func)?;
            func.instruction(&Instruction::LocalSet(ctx.addr_scratch));
            func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
            func.instruction(&Instruction::I32Const(0));
            func.instruction(&Instruction::I32LtS);
            func.instruction(&Instruction::If(wasm_encoder::BlockType::Empty));
            func.instruction(&Instruction::Unreachable);
            func.instruction(&Instruction::End);
            // mem[cursor] = n
            func.instruction(&Instruction::I32Const(0));
            func.instruction(&Instruction::I32Load(M4));
            func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
            func.instruction(&Instruction::I32Store(M4));
            // result: cursor + 4
            func.instruction(&Instruction::I32Const(0));
            func.instruction(&Instruction::I32Load(M4));
            func.instruction(&Instruction::I32Const(4));
            func.instruction(&Instruction::I32Add);
            // cursor = cursor + 4 + n * elem_size
            func.instruction(&Instruction::I32Const(0));
            func.instruction(&Instruction::I32Const(0));
            func.instruction(&Instruction::I32Load(M4));
            func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
            func.instruction(&Instruction::I32Const(elem_size as i32));
            func.instruction(&Instruction::I32Mul);
            func.instruction(&Instruction::I32Add);
            func.instruction(&Instruction::I32Const(4));
            func.instruction(&Instruction::I32Add);
            func.instruction(&Instruction::I32Store(M4));
        }
        Expr::Ref { name, .. } => {
            let idx = ctx
                .fn_indices
                .get(name)
                .ok_or_else(|| format!("Wasm Codegen: ref to unknown function '{}'", name))?;
            func.instruction(&Instruction::I32Const((idx - ctx.import_count) as i32));
        }
        Expr::CallRef { sig, func: target, args, .. } => {
            for a in args {
                compile_expr(a, ctx, func)?;
            }
            compile_expr(target, ctx, func)?;
            let key = wasm_signature(sig);
            let pos = ctx.ref_sigs.iter().position(|k| *k == key).ok_or("Wasm Codegen: call_ref signature not collected")?;
            func.instruction(&Instruction::CallIndirect { type_index: ctx.ref_type_base + pos as u32, table_index: 0 });
        }
        // Pointers and arrays are i32 addresses: casts and addr are free.
        Expr::Null { .. } => {
            func.instruction(&Instruction::I32Const(0));
        }
        Expr::Cast { addr: inner, .. } | Expr::Addr { val: inner, .. } => {
            compile_expr(inner, ctx, func)?;
        }
        Expr::ArrLen { arr, .. } => {
            compile_expr(arr, ctx, func)?;
            func.instruction(&Instruction::I32Const(4));
            func.instruction(&Instruction::I32Sub);
            func.instruction(&Instruction::I32Load(M4));
        }
        Expr::ArrGet { elem_ty, ptr, index, .. } => {
            let (elem_size, _) = crate::checker::type_size_and_align(elem_ty)?;
            compile_expr(ptr, ctx, func)?;
            compile_expr(index, ctx, func)?;
            func.instruction(&Instruction::I32Const(elem_size as i32));
            func.instruction(&Instruction::I32Mul);
            func.instruction(&Instruction::I32Add);
            match elem_ty {
                Type::I32 | Type::Bool | Type::Str | Type::Ptr(_) | Type::Array(_) | Type::Fn(_, _) => {
                    func.instruction(&Instruction::I32Load(M4));
                    if *elem_ty == Type::Bool {
                        normalize_bool(func);
                    }
                }
                Type::I64 => {
                    func.instruction(&Instruction::I64Load(wasm_encoder::MemArg {
                        offset: 0,
                        align: 3,
                        memory_index: 0,
                    }));
                }
                Type::F32 => {
                    func.instruction(&Instruction::F32Load(wasm_encoder::MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }));
                }
                Type::F64 => {
                    func.instruction(&Instruction::F64Load(wasm_encoder::MemArg {
                        offset: 0,
                        align: 3,
                        memory_index: 0,
                    }));
                }
                _ => return Err(format!("Unsupported elem type for arr.get: {:?}", elem_ty)),
            }
        }
        Expr::ArrSet { elem_ty, ptr, index, val, .. } => {
            let (elem_size, _) = crate::checker::type_size_and_align(elem_ty)?;
            compile_expr(ptr, ctx, func)?;
            compile_expr(index, ctx, func)?;
            func.instruction(&Instruction::I32Const(elem_size as i32));
            func.instruction(&Instruction::I32Mul);
            func.instruction(&Instruction::I32Add);
            emit_write_address_check(func, ctx.addr_scratch);
            compile_expr(val, ctx, func)?;
            match elem_ty {
                Type::I32 | Type::Bool | Type::Str | Type::Ptr(_) | Type::Array(_) | Type::Fn(_, _) => {
                    func.instruction(&Instruction::I32Store(M4));
                }
                Type::I64 => {
                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 3,
                        memory_index: 0,
                    }));
                }
                Type::F32 => {
                    func.instruction(&Instruction::F32Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }));
                }
                Type::F64 => {
                    func.instruction(&Instruction::F64Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 3,
                        memory_index: 0,
                    }));
                }
                _ => return Err(format!("Unsupported elem type for arr.set: {:?}", elem_ty)),
            }
        }
    }
    Ok(())
}

/// A `bool` read from memory is true iff its word is nonzero, as in the VM.
/// Normalising to 0/1 keeps `and`/`or` (bitwise in wasm) and `eq` correct
/// when the word was written by something other than `put`/`arr.set`.
fn normalize_bool(func: &mut Function) {
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Ne);
}

/// `ok`/`err`: allocate an 8-byte cell `[tag:i32 payload:i32]` (tag 0 = ok,
/// 1 = err) from the heap cursor before evaluating the payload, and leave the
/// cell pointer on the stack. The pointer is pushed twice before the payload is
/// compiled, so a payload that itself uses the scratch local cannot clobber it.
/// The VM allocates the same cell in the same order.
fn compile_result_cell(tag: i32, inner: &Expr, ctx: &Ctx, func: &mut Function) -> Result<(), String> {
    let payload_ty = expr_type(inner, ctx);
    if aipl_to_wasm_type(&payload_ty) != ValType::I32 {
        return Err(format!(
            "Wasm Codegen: result payloads must be 32-bit (i32, bool, str), got {:?}",
            payload_ty
        ));
    }
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Load(M4));
    func.instruction(&Instruction::LocalSet(ctx.addr_scratch));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Store(M4));

    func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
    func.instruction(&Instruction::I32Const(tag));
    func.instruction(&Instruction::I32Store(M4));

    func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
    func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
    compile_expr(inner, ctx, func)?;
    func.instruction(&Instruction::I32Store(wasm_encoder::MemArg { offset: 4, align: 2, memory_index: 0 }));
    Ok(())
}

fn is_void_expr(expr: &Expr, ctx: &Ctx) -> bool {
    match expr {
        Expr::Set { .. } | Expr::Let { .. } | Expr::PutField { .. } | Expr::ArrSet { .. } => true,
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
                | OpCode::DivU
                | OpCode::Mod
                | OpCode::RemU
                | OpCode::BitXor
                | OpCode::Shl
                | OpCode::Shr
                | OpCode::ShrU
                | OpCode::BitAnd
                | OpCode::BitOr
                | OpCode::MemLoad8
                | OpCode::MemLoad32
                | OpCode::MemLoad64
                | OpCode::MemAlloc
                | OpCode::MemGrow
                | OpCode::StrLen
                | OpCode::StrPtr
                | OpCode::FsOpen
                | OpCode::FsRead
                | OpCode::FsWrite
                | OpCode::FsClose
                | OpCode::FsDelete
                | OpCode::I64ExtendS
                | OpCode::F64ConvertI64S
                | OpCode::I64TruncF64S
                | OpCode::F64ReinterpretI64
                | OpCode::I64ReinterpretF64
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
        // Same rule as the block type compile_expr gives a match_result.
        Expr::MatchResult { ok_body, .. } => ok_body.last().map_or(true, |e| is_void_expr(e, ctx)),
        Expr::Lit(..)
        | Expr::Var(..)
        | Expr::Ok(..)
        | Expr::Err(..)
        | Expr::NewStruct { .. }
        | Expr::GetField { .. }
        | Expr::Sizeof { .. }
        | Expr::ArrNew { .. }
        | Expr::ArrGet { .. }
        | Expr::ArrLen { .. }
        | Expr::Null { .. }
        | Expr::Cast { .. }
        | Expr::Addr { .. }
        | Expr::Ref { .. } => false,
        Expr::CallRef { sig, .. } => matches!(sig, Type::Fn(_, ret) if **ret == Type::Void),
    }
}

/// Emits the runtime memory-layout check for a store whose address is on top
/// of the stack: traps (`unreachable`) if the address is in bytes 0-3 (the
/// heap cursor) or 64-1023 (reserved). Mirrors `vm::check_write_address`
/// exactly so both backends fail on the same writes. The address stays on the
/// stack for the store that follows.
///
///   [addr] local.tee s
///   local.get s ; i32.const 4  ; i32.lt_u              -> addr < 4
///   local.get s ; i32.const 64 ; i32.sub ; i32.const 960 ; i32.lt_u  -> 64 <= addr < 1024
///   i32.or ; if unreachable end
fn emit_write_address_check(func: &mut Function, scratch: u32) {
    func.instruction(&Instruction::LocalTee(scratch));
    func.instruction(&Instruction::LocalGet(scratch));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::LocalGet(scratch));
    func.instruction(&Instruction::I32Const(64));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32Const(960));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::I32Or);
    func.instruction(&Instruction::If(wasm_encoder::BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);
}

// ---------------------------------------------------------------------------
// WASI lowering support (AIPL_SPEC.md, sections 6.2 and 7.9)
// ---------------------------------------------------------------------------

const M4: MemArg = MemArg { offset: 0, align: 2, memory_index: 0 };

/// Runtime-block cells used by the WASI lowerings (all below the heap, owned
/// by the runtime, never handed out by mem.alloc):
const RT_IOV0_BUF: i32 = 64;
const RT_IOV0_LEN: i32 = 68;
const RT_IOV1_BUF: i32 = 72;
const RT_IOV1_LEN: i32 = 76;
/// nwritten / nread out-parameter.
const RT_NBYTES: i32 = 80;
/// path_open's opened-fd out-parameter.
const RT_OPENED_FD: i32 = 84;
/// String literals are interned here, as [len u32 LE][bytes]; a `str` value is
/// the address of the bytes.
const STRING_DATA_BASE: u32 = 512;
const STRING_DATA_CAPACITY: u32 = 512;

/// The first preopened directory a WASI host hands to the module.
const WASI_PREOPEN_FD: i32 = 3;
const WASI_OFLAGS_CREAT: i32 = 1 << 0;
const WASI_OFLAGS_TRUNC: i32 = 1 << 3;
const WASI_RIGHT_FD_READ: i64 = 1 << 1;
const WASI_RIGHT_FD_WRITE: i64 = 1 << 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Wasi {
    FdWrite,
    FdRead,
    PathOpen,
    FdClose,
    ProcExit,
    PathUnlinkFile,
}

impl Wasi {
    fn name(self) -> &'static str {
        match self {
            Wasi::FdWrite => "fd_write",
            Wasi::FdRead => "fd_read",
            Wasi::PathOpen => "path_open",
            Wasi::FdClose => "fd_close",
            Wasi::ProcExit => "proc_exit",
            Wasi::PathUnlinkFile => "path_unlink_file",
        }
    }

    /// wasi_snapshot_preview1 signatures.
    fn signature(self) -> (Vec<ValType>, Vec<ValType>) {
        use ValType::*;
        match self {
            Wasi::FdWrite | Wasi::FdRead => (vec![I32, I32, I32, I32], vec![I32]),
            Wasi::PathOpen => (vec![I32, I32, I32, I32, I32, I64, I64, I32, I32], vec![I32]),
            Wasi::FdClose => (vec![I32], vec![I32]),
            Wasi::ProcExit => (vec![I32], vec![]),
            Wasi::PathUnlinkFile => (vec![I32, I32, I32], vec![I32]),
        }
    }

    fn for_op(op: &OpCode) -> Option<Wasi> {
        match op {
            OpCode::SysPrint | OpCode::FsWrite => Some(Wasi::FdWrite),
            OpCode::FsRead => Some(Wasi::FdRead),
            OpCode::FsOpen => Some(Wasi::PathOpen),
            OpCode::FsClose => Some(Wasi::FdClose),
            OpCode::SysExit => Some(Wasi::ProcExit),
            OpCode::FsDelete => Some(Wasi::PathUnlinkFile),
            _ => None,
        }
    }
}

/// Visits every expression node in the module, depth first.
fn walk_module(module: &Module, visit: &mut dyn FnMut(&Expr)) {
    for f in &module.functions {
        for c in &f.contracts {
            match c {
                Contract::Requires(e) | Contract::Ensures(e) | Contract::Invariant(e) => walk_expr(e, visit),
            }
        }
        for e in &f.body {
            walk_expr(e, visit);
        }
    }
}

fn walk_expr(expr: &Expr, visit: &mut dyn FnMut(&Expr)) {
    visit(expr);
    match expr {
        Expr::Lit(..) | Expr::Var(..) => {}
        Expr::Let { val, .. } | Expr::Set { val, .. } => walk_expr(val, visit),
        Expr::If { cond, then_branch, else_branch, .. } => {
            walk_expr(cond, visit);
            walk_expr(then_branch, visit);
            walk_expr(else_branch, visit);
        }
        Expr::Loop { start, end, step, body, .. } => {
            walk_expr(start, visit);
            walk_expr(end, visit);
            walk_expr(step, visit);
            for e in body {
                walk_expr(e, visit);
            }
        }
        Expr::While { cond, body, .. } => {
            walk_expr(cond, visit);
            for e in body {
                walk_expr(e, visit);
            }
        }
        Expr::Call { args, .. } | Expr::Op { args, .. } => {
            for a in args {
                walk_expr(a, visit);
            }
        }
        Expr::Block(exprs, _) => {
            for e in exprs {
                walk_expr(e, visit);
            }
        }
        Expr::Ok(inner, _, _) | Expr::Err(inner, _, _) => walk_expr(inner, visit),
        Expr::MatchResult { expr, ok_body, err_body, .. } => {
            walk_expr(expr, visit);
            for e in ok_body {
                walk_expr(e, visit);
            }
            for e in err_body {
                walk_expr(e, visit);
            }
        }
        Expr::NewStruct { .. } | Expr::Sizeof { .. } | Expr::Null { .. } | Expr::Ref { .. } => {}
        // Source order (function, then arguments), as the self-hosted compiler walks it.
        Expr::CallRef { func, args, .. } => {
            walk_expr(func, visit);
            for a in args {
                walk_expr(a, visit);
            }
        }
        Expr::GetField { ptr, .. }
        | Expr::ArrNew { size: ptr, .. }
        | Expr::ArrLen { arr: ptr, .. }
        | Expr::Cast { addr: ptr, .. }
        | Expr::Addr { val: ptr, .. } => walk_expr(ptr, visit),
        Expr::PutField { ptr, val, .. } => {
            walk_expr(ptr, visit);
            walk_expr(val, visit);
        }
        Expr::ArrGet { ptr, index, .. } => {
            walk_expr(ptr, visit);
            walk_expr(index, visit);
        }
        Expr::ArrSet { ptr, index, val, .. } => {
            walk_expr(ptr, visit);
            walk_expr(index, visit);
            walk_expr(val, visit);
        }
    }
}

fn module_uses_op(module: &Module, wanted: &OpCode) -> bool {
    let mut found = false;
    walk_module(module, &mut |e| {
        if let Expr::Op { op, .. } = e {
            if op == wanted {
                found = true;
            }
        }
    });
    found
}

/// The WASI functions this module needs, in a fixed canonical order.
fn collect_wasi_imports(module: &Module) -> Vec<Wasi> {
    let mut set: Vec<Wasi> = Vec::new();
    walk_module(module, &mut |e| {
        if let Expr::Op { op, .. } = e {
            if let Some(w) = Wasi::for_op(op) {
                if !set.contains(&w) {
                    set.push(w);
                }
            }
        }
    });
    set.sort();
    set
}

/// True if a function body contains any op lowered through WASI (and so needs
/// the two extra I/O scratch locals).
fn fn_uses_io(body: &[Expr]) -> bool {
    let mut found = false;
    for e in body {
        walk_expr(e, &mut |x| {
            if let Expr::Op { op, .. } = x {
                if Wasi::for_op(op).is_some() {
                    found = true;
                }
            }
        });
    }
    found
}

fn wasi_index(ctx: &Ctx, w: Wasi) -> Result<u32, String> {
    ctx.wasi
        .get(&w)
        .copied()
        .ok_or_else(|| format!("Wasm Codegen: internal error, WASI import {} was not collected", w.name()))
}

fn io_locals(ctx: &Ctx) -> Result<(u32, u32), String> {
    ctx.io_locals
        .ok_or_else(|| "Wasm Codegen: internal error, I/O op in a function without I/O scratch locals".to_string())
}

/// With a WASI errno on the stack: leaves -1 if it is non-zero, otherwise the
/// i32 loaded from `out_cell` (or 0 when there is no out-parameter). This is
/// the VM's convention for every fs.* op: a count/fd on success, -1 on failure.
fn emit_errno_to_result(func: &mut Function, out_cell: Option<i32>) {
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(-1));
    func.instruction(&Instruction::Else);
    match out_cell {
        Some(cell) => {
            func.instruction(&Instruction::I32Const(cell));
            func.instruction(&Instruction::I32Load(M4));
        }
        None => {
            func.instruction(&Instruction::I32Const(0));
        }
    }
    func.instruction(&Instruction::End);
}


