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
        // each function's line table, by function index
        let mut lines: Vec<(u32, Vec<(u32, Pos)>)> = Vec::new();

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
            imports.import(w.module(), w.name(), EntityType::Function(idx as u32));
            wasi_indices.insert(*w, idx as u32);
        }
        let import_count = used_wasi.len() as u32;
        // A module that spawns threads is "threaded" (AIPL_SPEC.md 4.D): it
        // imports a shared memory, initialises it once, and gives each
        // spawned thread its own runtime scratch block.
        let threaded = module_uses_op(module, &OpCode::ThreadSpawn);

        let StringLayout { blob: string_blob, addrs: strings, newline_addr, heap_start } = string_layout(module)?;

        let structs = crate::checker::Structs::new(&module.structs, &module.unions);
        let unions: HashMap<String, UnionDef> = module.unions.iter().map(|u| (u.name.clone(), u.clone())).collect();
        let enums: HashMap<String, EnumDef> = module.enums.iter().map(|e| (e.name.clone(), e.clone())).collect();

        // 1. Build type section and function index mapping (after the imports)
        for (i, f) in module.functions.iter().enumerate() {
            let idx = import_count + i as u32;
            // a value struct is its scalars (several parameters, several results)
            let params: Vec<ValType> = f.params.iter().flat_map(|(_, t)| valtypes(t, &structs)).collect();
            let results: Vec<ValType> = valtypes(&f.return_type, &structs);

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
            fn_types.insert(f.name.clone(), Type::Fn(params, Box::new(f.return_type.clone()), false));
        }
        let mut uses_refs = false;
        let mut ref_sigs: Vec<(Vec<ValType>, Vec<ValType>)> = Vec::new();
        walk_module(module, &mut |e| match e {
            Expr::Ref { .. } => uses_refs = true,
            Expr::CallRef { sig, .. } => {
                uses_refs = true;
                let key = wasm_signature(sig, &structs);
                if !ref_sigs.contains(&key) {
                    ref_sigs.push(key);
                }
            }
            _ => {}
        });
        // wasi_thread_start calls the worker through the table as (fn [i32] -> i32)
        let worker_sig = (vec![ValType::I32], vec![ValType::I32]);
        if threaded {
            uses_refs = true;
            if !ref_sigs.contains(&worker_sig) {
                ref_sigs.push(worker_sig.clone());
            }
        }
        let ref_type_base = import_count + module.functions.len() as u32;
        for (params, results) in &ref_sigs {
            types.ty().function(params.clone(), results.clone());
        }
        // Threaded modules end with two compiler-made functions: the start
        // function (one-time memory init) and the exported wasi_thread_start.
        let n_user = module.functions.len() as u32;
        let init_fn = import_count + n_user;
        let thread_start_fn = init_fn + 1;
        if threaded {
            let base = ref_type_base + ref_sigs.len() as u32;
            types.ty().function(vec![], vec![]);
            types.ty().function(vec![ValType::I32, ValType::I32], vec![]);
            functions.function(base);
            functions.function(base + 1);
        }
        let worker_type = ref_type_base + ref_sigs.iter().position(|s| *s == worker_sig).unwrap_or(0) as u32;
        // A module with a zero-argument `main` and no `_start` of its own gets
        // a WASI command entry point: `_start` calls `main` and discards its
        // result (exit status 0 unless the program calls sys.exit). It comes
        // after every other function, with its own [] -> [] type.
        let main_fn = module.functions.iter().position(|f| f.name == "main" && f.params.is_empty());
        let auto_start = main_fn.is_some() && !module.functions.iter().any(|f| f.name == "_start");
        let start_fn = import_count + n_user + if threaded { 2 } else { 0 };
        if auto_start {
            let base = ref_type_base + ref_sigs.len() as u32 + if threaded { 2 } else { 0 };
            types.ty().function(vec![], vec![]);
            functions.function(base);
        }
        // A module with compiled checks (an array or struct access, a req or ens) ends
        // with the six check helpers (`check_functions`), after every other
        // function, each with its own type.
        let uses_checks = module_uses_checks(module);
        let checks_base = start_fn + if auto_start { 1 } else { 0 };
        let check_fns = CheckFns::at(checks_base);
        let may_free = functions_that_may_free(module);
        if uses_checks {
            let base = ref_type_base + ref_sigs.len() as u32 + if threaded { 2 } else { 0 } + if auto_start { 1 } else { 0 };
            for (k, (params, results)) in check_signatures().into_iter().enumerate() {
                types.ty().function(params, results);
                functions.function(base + k as u32);
            }
        }

        // 2. Build code section (body compilation)
        for (fn_i, f) in module.functions.iter().enumerate() {
            let source = module.sources.get(&format!("fn {}", f.name));
            let mut extra_lets = Vec::new();
            collect_lets(&f.body, &mut extra_lets, &unions);

            let mut local_map: HashMap<String, u32> = HashMap::new();
            let mut current_idx = 0;

            // a value struct parameter is several wasm parameters
            for (p_name, p_ty) in &f.params {
                local_map.insert(p_name.clone(), current_idx);
                current_idx += width(p_ty, &structs);
            }

            // Static AIPL type of every local, so codegen can pick i32 vs i64
            // vs f64 instructions. First declaration wins, matching local_map.
            let mut local_types: HashMap<String, Type> = HashMap::new();
            for (p_name, p_ty) in &f.params {
                local_types.insert(p_name.clone(), p_ty.clone());
            }

            let mut order = Vec::new();
            for (l_name, l_ty) in &extra_lets {
                if !local_types.contains_key(l_name) {
                    local_types.insert(l_name.clone(), l_ty.clone());
                    order.push(l_name);
                }
            }
            binder_types(&f.body, &mut local_types, &fn_returns, &fn_types, &structs);
            // numbered once every type is known (a binder's comes from what it matches)
            for n in &order {
                local_map.insert((*n).clone(), current_idx);
                current_idx += width(&local_types[*n], &structs);
            }
            let mut wasm_locals: Vec<(u32, ValType)> = order.iter().flat_map(|n| valtypes(&local_types[*n], &structs)).map(|t| (1, t)).collect();

            // A function with `ens` keeps its result in `res` for the checks
            // (a hidden local after the lets, unless a let already named it).
            // an inv is also checked on return, as an ens is
            let has_ens = f.contracts.iter().any(|c| matches!(c, Contract::Ensures(_) | Contract::Invariant(_)));
            if has_ens && f.return_type != Type::Void && !local_map.contains_key("res") {
                local_map.insert("res".to_string(), current_idx);
                local_types.insert("res".to_string(), f.return_type.clone());
                wasm_locals.extend(valtypes(&f.return_type, &structs).into_iter().map(|t| (1, t)));
                current_idx += width(&f.return_type, &structs);
            }

            // Two extra i32 locals per function: scratch for write-address
            // checks, allocation, and array bounds checks (the array), and the
            // index of a bounds check
            let addr_scratch = current_idx;
            wasm_locals.push((2, ValType::I32));
            current_idx += 2;
            // and an i64: the pointer a generation check is checking
            let fat_scratch = current_idx;
            wasm_locals.push((1, ValType::I64));
            current_idx += 1;

            // Two more i32 temps, only for functions that perform I/O: the
            // WASI lowerings evaluate all their operands first (source order,
            // nested I/O safe) and then unload them into these.
            let io_locals = if fn_uses_io(&f.body) {
                wasm_locals.push((2, ValType::I32));
                current_idx += 2;
                Some((current_idx - 2, current_idx - 1))
            } else {
                None
            };

            // value struct scratch groups (value_groups), after every other local
            let value_return = has_ens && is_value_type(&f.return_type, &structs);
            let typing = Typing { local_types: &local_types, fn_returns: &fn_returns, fn_types: &fn_types, structs: &structs };
            let tyf = |e: &Expr| type_in(e, &typing);
            let body_refs: Vec<&Expr> = f.body.iter().collect();
            let mut found = value_groups(&body_refs, &tyf, &structs, value_return);
            let contract_refs: Vec<&Expr> = f
                .contracts
                .iter()
                .map(|c| match c {
                    Contract::Requires(e) | Contract::Ensures(e) | Contract::Invariant(e) => e,
                })
                .collect();
            found.extend(value_groups(&contract_refs, &tyf, &structs, false));
            let mut groups: HashMap<usize, u32> = HashMap::new();
            for (key, t) in found {
                groups.insert(key, current_idx);
                wasm_locals.extend(valtypes(&t, &structs).into_iter().map(|v| (1, v)));
                current_idx += width(&t, &structs);
            }

            let mut func = Body::new(wasm_locals);
            func.quiet = f.name.starts_with("show.debug.");
            let ctx = Ctx {
                locals: &local_map,
                addr_scratch,
                index_scratch: addr_scratch + 1,
                fat_scratch,
                checks: &check_fns,
                ens_block: has_ens,
                io_locals,
                local_types: &local_types,
                groups: &groups,
                fn_indices: &fn_indices,
                fn_returns: &fn_returns,
                strings: &strings,
                wasi: &wasi_indices,
                newline_addr,
                heap_start,
                threaded,
                structs: &structs,
                unions: &unions,
                enums: &enums,
                fn_types: &fn_types,
                import_count,
                ref_sigs: &ref_sigs,
                ref_type_base,
                labels: std::cell::RefCell::new(Vec::new()),
                source,
                file_names: &module.file_names,
                may_free: &may_free,
                proven: std::cell::RefCell::new(Vec::new()),
                versioning: std::cell::Cell::new(true),
            };
            func.at.push(ctx.position(f.span));

            // Every statement but the last is executed purely for effect: drop
            // any value it leaves behind so it doesn't corrupt the wasm stack.
            // The last statement's value (if any) is the function's implicit
            // return, so it's kept - unless the function is declared void, in
            // which case it must be dropped too.
            let checks = contract_messages(f);
            for (is_ens, expr, msg) in &checks {
                if !is_ens {
                    emit_contract_check(expr, msg, &ctx, &mut func, &strings)?;
                }
            }
            if has_ens {
                // a value struct result goes into res, so the block yields nothing
                func.instruction(&Instruction::Block(if f.return_type == Type::Void || value_return {
                    BlockType::Empty
                } else {
                    BlockType::Result(aipl_to_wasm_type(&f.return_type))
                }));
                ctx.labels.borrow_mut().push(Label::Plain);
            }
            let body_len = f.body.len();
            for (i, expr) in f.body.iter().enumerate() {
                if value_return && i + 1 == body_len {
                    let res = *ctx.locals.get("res").ok_or("Wasm Codegen: no res")?;
                    compile_into(expr, res, &ctx, &mut func)?;
                    continue;
                }
                compile_expr(expr, &ctx, &mut func)?;
                let is_last = i + 1 == body_len;
                let keep_value = is_last && f.return_type != Type::Void;
                if !keep_value && !is_void_expr(expr, &ctx) {
                    emit_drops(&mut func, &ctx, &expr_type(expr, &ctx));
                }
            }
            if has_ens {
                ctx.labels.borrow_mut().pop();
                func.instruction(&Instruction::End);
                let res = ctx.locals.get("res").copied();
                if let (true, Some(r)) = (f.return_type != Type::Void && !value_return, res) {
                    func.instruction(&Instruction::LocalSet(r));
                }
                for (is_ens, expr, msg) in &checks {
                    if *is_ens {
                        emit_contract_check(expr, msg, &ctx, &mut func, &strings)?;
                    }
                }
                if let (true, Some(r)) = (f.return_type != Type::Void, res) {
                    emit_get_slots(&mut func, r, width(&f.return_type, &structs));
                }
            }
            func.instruction(&Instruction::End);
            codes.function(&func.f);
            lines.push((import_count + fn_i as u32, func.lines));
        }

        // 16 pages (1 MiB) to start, matching the VM, so `mem.grow` reports the
        // same old size in both backends; 32768 pages max (2 GiB: every address a non-negative i32), also matching
        // the VM. A threaded module imports it shared ("env" "memory") so every
        // thread's instance uses the same memory.
        let memory_type = wasm_encoder::MemoryType {
            minimum: 16,
            maximum: Some(crate::vm::MAX_PAGES as u64),
            memory64: false,
            shared: threaded,
            page_size_log2: None,
        };
        let mut memories = wasm_encoder::MemorySection::new();
        if threaded {
            imports.import("env", "memory", EntityType::Memory(memory_type));
        } else {
            memories.memory(memory_type);
        }
        exports.export("memory", ExportKind::Memory, 0);
        if threaded {
            exports.export("wasi_thread_start", ExportKind::Func, thread_start_fn);
        }
        if auto_start {
            exports.export("_start", ExportKind::Func, start_fn);
        }

        // Runtime block initialisation: the heap cursor at address 0 starts
        // at heap_start, just past the string literals. Everything else in
        // bytes 0..1024 is zero. A threaded module's segments are passive and
        // copied once by its start function (each thread instantiates the
        // module again, and active segments would reset the cursor).
        let mut data = wasm_encoder::DataSection::new();
        let n_segments = if string_blob.is_empty() { 1 } else { 2 };
        if threaded {
            data.passive(heap_start.to_le_bytes());
            if !string_blob.is_empty() {
                data.passive(string_blob.iter().copied());
            }
            codes.function(&threaded_init_function(string_blob.len() as u32).f);
            codes.function(&thread_start_function(worker_type).f);
        } else {
            data.active(0, &wasm_encoder::ConstExpr::i32_const(0), heap_start.to_le_bytes());
            if !string_blob.is_empty() {
                data.active(0, &wasm_encoder::ConstExpr::i32_const(STRING_DATA_BASE as i32), string_blob.iter().copied());
            }
        }
        if let (true, Some(m)) = (auto_start, main_fn) {
            let mut f = Body::new(vec![]);
            f.instruction(&Instruction::Call(import_count + m as u32));
            for _ in 0..width(&module.functions[m].return_type, &structs) {
                f.instruction(&Instruction::Drop);
            }
            f.instruction(&Instruction::End);
            codes.function(&f.f);
        }
        if uses_checks {
            for f in check_functions(&check_fns) {
                codes.function(&f.f);
            }
        }

        wasm_module.section(&types);
        if import_count > 0 || threaded {
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
        if threaded {
            // per-instance pointer to the runtime scratch cells (64 in the
            // main thread, a private block in each spawned thread)
            let mut globals = wasm_encoder::GlobalSection::new();
            globals.global(
                wasm_encoder::GlobalType { val_type: ValType::I32, mutable: true, shared: false },
                &wasm_encoder::ConstExpr::i32_const(RT_IOV0_BUF),
            );
            wasm_module.section(&globals);
            wasm_module.section(&exports);
            wasm_module.section(&wasm_encoder::StartSection { function_index: init_fn });
        } else {
            wasm_module.section(&memories);
            wasm_module.section(&exports);
        }
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
        if threaded {
            wasm_module.section(&wasm_encoder::DataCountSection { count: n_segments });
        }
        wasm_module.section(&codes);
        wasm_module.section(&data);

        // The name section (a custom section, last): every function's name,
        // so a trap can say where it happened (AIPL_SPEC.md 6.5). The
        // generated functions have names starting with "aipl." or WASI's own.
        let mut fn_names = wasm_encoder::NameMap::new();
        for (i, f) in module.functions.iter().enumerate() {
            fn_names.append(import_count + i as u32, &f.name);
        }
        if threaded {
            fn_names.append(init_fn, "aipl.init");
            fn_names.append(thread_start_fn, "wasi_thread_start");
        }
        if auto_start {
            fn_names.append(start_fn, "_start");
        }
        if uses_checks {
            for (k, n) in ["aipl.dec", "aipl.oob", "aipl.text", "aipl.begin", "aipl.end", "aipl.null", "aipl.uaf"].iter().enumerate() {
                fn_names.append(checks_base + k as u32, n);
            }
        }
        let mut names = wasm_encoder::NameSection::new();
        names.functions(&fn_names);
        wasm_module.section(&names);
        // where each call and trapping instruction is in the source
        if lines.iter().any(|(_, l)| !l.is_empty()) {
            wasm_module.section(&wasm_encoder::CustomSection {
                name: "aipl.lines".into(),
                data: std::borrow::Cow::Owned(encode_lines(&lines)),
            });
        }

        Ok(wasm_module.finish())
    }
}

/// Bundles the read-only context threaded through codegen so it isn't passed
/// as four separate parameters everywhere.
/// A source position in the line table: file name, line, column.
type Pos = (String, u32, u32);

/// A function body being written, and its line table (AIPL_SPEC.md 6.5,
/// docs/design/LINES_PLAN.md): before each call or instruction that can
/// trap, the position of the innermost expression being compiled, unless
/// the previous entry has it. Compiler-made functions have no positions.
pub(crate) struct Body {
    f: Function,
    /// the expressions being compiled, innermost last
    at: Vec<Pos>,
    /// (offset in the body, position)
    lines: Vec<(u32, Pos)>,
    /// A compiler-made function, with no positions: the printers formatting
    /// generates (`show.debug.*`, formats.rs debug_functions).
    quiet: bool,
}

impl Body {
    fn new<L: IntoIterator<Item = (u32, ValType)>>(locals: L) -> Self
    where
        L::IntoIter: ExactSizeIterator,
    {
        Body { f: Function::new(locals), at: Vec::new(), lines: Vec::new(), quiet: false }
    }

    fn instruction(&mut self, i: &Instruction) -> &mut Self {
        if let (Some(p), false) = (self.at.last(), self.quiet) {
            if traps_or_calls(i) && self.lines.last().map(|(_, q)| q) != Some(p) {
                self.lines.push((self.f.byte_len() as u32, p.clone()));
            }
        }
        self.f.instruction(i);
        self
    }
}

/// Whether a frame can point at the instruction: a call, one that can trap,
/// or an `if` (wasmtime reports `if c unreachable end`, a check's trap, at the
/// `if`). docs/design/LINES_PLAN.md lists the opcodes.
fn traps_or_calls(i: &Instruction) -> bool {
    use wasm_encoder::Encode;
    let mut b = Vec::new();
    i.encode(&mut b);
    matches!(b[0], 0x00 | 0x04 | 0x10 | 0x11 | 0x28..=0x3E | 0x6D..=0x70 | 0x7F..=0x82 | 0xA8..=0xB1 | 0xFC | 0xFE)
}

/// A module without its `aipl.lines` section: for comparing compiles of one
/// program from texts laid out differently (a printed or flattened copy),
/// whose line tables differ by design.
pub fn without_lines(wasm: &[u8]) -> Vec<u8> {
    let leb = |at: &mut usize| {
        let (mut v, mut shift) = (0usize, 0);
        loop {
            let b = wasm[*at];
            *at += 1;
            v |= ((b & 0x7f) as usize) << shift;
            if b & 0x80 == 0 {
                return v;
            }
            shift += 7;
        }
    };
    let mut out = wasm[..8].to_vec();
    let mut at = 8;
    while at < wasm.len() {
        let start = at;
        let id = wasm[at];
        at += 1;
        let size = leb(&mut at);
        let end = at + size;
        let mut name_at = at;
        let is_lines = id == 0 && {
            let n = leb(&mut name_at);
            &wasm[name_at..name_at + n] == b"aipl.lines"
        };
        if !is_lines {
            out.extend_from_slice(&wasm[start..end]);
        }
        at = end;
    }
    out
}

/// The `aipl.lines` section's contents (docs/design/LINES_PLAN.md).
fn encode_lines(lines: &[(u32, Vec<(u32, Pos)>)]) -> Vec<u8> {
    fn u(out: &mut Vec<u8>, mut v: u32) {
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                return;
            }
            out.push(b | 0x80);
        }
    }
    fn s(out: &mut Vec<u8>, mut v: i64) {
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if (v == 0 && b & 0x40 == 0) || (v == -1 && b & 0x40 != 0) {
                out.push(b);
                return;
            }
            out.push(b | 0x80);
        }
    }
    let mut files: Vec<&str> = Vec::new();
    for (_, l) in lines {
        for (_, (f, _, _)) in l {
            if !files.contains(&f.as_str()) {
                files.push(f);
            }
        }
    }
    let mut out = Vec::new();
    u(&mut out, files.len() as u32);
    for f in &files {
        u(&mut out, f.len() as u32);
        out.extend_from_slice(f.as_bytes());
    }
    let with: Vec<_> = lines.iter().filter(|(_, l)| !l.is_empty()).collect();
    u(&mut out, with.len() as u32);
    for (index, l) in with {
        u(&mut out, *index);
        u(&mut out, l.len() as u32);
        let (mut off, mut line) = (0u32, 0u32);
        for (o, (f, ln, col)) in l {
            u(&mut out, o - off);
            u(&mut out, files.iter().position(|g| g == f).unwrap() as u32);
            s(&mut out, *ln as i64 - line as i64);
            u(&mut out, *col);
            off = *o;
            line = *ln;
        }
    }
    out
}

struct Ctx<'a> {
    locals: &'a HashMap<String, u32>,
    addr_scratch: u32,
    /// The index of an array bounds check (addr_scratch holds the array).
    index_scratch: u32,
    /// The pointer a generation check is checking (an i64).
    fat_scratch: u32,
    /// The check helpers' function indices (meaningful when the module has checks).
    checks: &'a CheckFns,
    /// The function has `ens`: its body is a block, and `return` branches to
    /// its end, where the `ens` checks run.
    ens_block: bool,
    /// `(a, b)` scratch locals for WASI lowerings; `None` when the function does no I/O.
    io_locals: Option<(u32, u32)>,
    local_types: &'a HashMap<String, Type>,
    /// Value struct scratch groups (value_groups): key -> first local.
    groups: &'a HashMap<usize, u32>,
    fn_indices: &'a HashMap<String, u32>,
    fn_returns: &'a HashMap<String, Type>,
    /// Interned string literal -> address of its bytes in the data segment.
    strings: &'a HashMap<String, u32>,
    /// WASI import -> function index.
    wasi: &'a HashMap<Wasi, u32>,
    /// Address of the interned "\n" used by sys.print (0 if unused).
    newline_addr: u32,
    /// First heap address: 1024 plus the string literals, 8-aligned.
    heap_start: u32,
    /// The module spawns threads: runtime scratch cells are per thread.
    threaded: bool,
    structs: &'a crate::checker::Structs,
    unions: &'a HashMap<String, UnionDef>,
    enums: &'a HashMap<String, EnumDef>,
    /// Function name -> its `(fn [...] -> r)` type, for `(ref f)`.
    fn_types: &'a HashMap<String, Type>,
    import_count: u32,
    /// Distinct call_ref signatures; signature i has type index ref_type_base + i.
    ref_sigs: &'a [(Vec<ValType>, Vec<ValType>)],
    ref_type_base: u32,
    /// Enclosing structured instructions that can contain user code,
    /// innermost last, so break/continue can compute their branch depth.
    labels: std::cell::RefCell<Vec<Label>>,
    /// The functions that may free memory (functions_that_may_free).
    may_free: &'a std::collections::HashSet<String>,
    /// Pointer and array variables an enclosing versioned loop has checked
    /// (compile_versioned_loop): their accesses skip the null and generation
    /// checks.
    proven: std::cell::RefCell<Vec<String>>,
    /// False inside a versioned loop's slow copy, whose loops are not
    /// versioned again (so the code grows linearly with nesting).
    versioning: std::cell::Cell<bool>,
    /// Where the function's item was written (None for a module parsed from
    /// one text), and each file's name in the line table.
    source: Option<&'a Source>,
    file_names: &'a HashMap<std::path::PathBuf, String>,
}

impl Ctx<'_> {
    /// A line-table position: the file the token at `span` was written in.
    /// An expression's position: in its item's file (a generic instance's
    /// expressions are all the template's; only type arguments come from
    /// elsewhere, and a position alone cannot tell them apart from a
    /// template token at the same line and column in another file).
    fn position(&self, span: (u32, u32)) -> Pos {
        let name = self.source.and_then(|s| self.file_names.get(&s.file)).cloned().unwrap_or_default();
        (name, span.0, span.1)
    }
}

/// What a wasm label is for. Blocks emitted around compiler-generated code
/// only (store guards, traps) never contain break/continue and are not tracked.
#[derive(Clone, Copy, PartialEq)]
enum Label {
    /// an if / else
    Plain,
    /// the block around a while/loop: break target
    Break,
    /// the loop header: continue target of a while
    LoopTop,
    /// the block around a loop's body: continue target (falls into the step)
    Continue,
}

/// Branch depth from the innermost label to the innermost label of a kind in `kinds`.
fn label_depth(ctx: &Ctx, kinds: &[Label]) -> Result<u32, String> {
    let labels = ctx.labels.borrow();
    labels
        .iter()
        .rev()
        .position(|l| kinds.contains(l))
        .map(|d| d as u32)
        .ok_or_else(|| "Wasm Codegen: break/continue outside a loop".to_string())
}

/// Compiles `body` with `label` pushed on the label stack.
fn with_label<T>(ctx: &Ctx, label: Label, body: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    ctx.labels.borrow_mut().push(label);
    let r = body();
    ctx.labels.borrow_mut().pop();
    r
}

/// The wasm params/results of a `(fn [...] -> r)` type.
fn wasm_signature(sig: &Type, structs: &crate::checker::Structs) -> (Vec<ValType>, Vec<ValType>) {
    match sig {
        Type::Fn(params, ret, _) => (params.iter().flat_map(|t| valtypes(t, structs)).collect(), valtypes(ret, structs)),
        _ => (vec![], vec![]),
    }
}

// ===== Value structs (AIPL_SPEC.md 4.E; docs/design/VALUES_PLAN.md V2) =====
//
// A value struct is its fields' scalars, flattened in declaration order: a
// local or parameter is that many wasm locals, a result that many results.
// It is never a block's result: an if or match that yields one stores it
// in a group of scratch locals (assigned per function, after every other
// local, by `value_groups`) and pushes them after its end; so do `with`
// and a field read from a value that is computed (not a local, not in
// memory). In memory it is laid out as AIPL_SPEC.md 4.E says and moved one
// scalar at a time: a load pushes each scalar from addr_scratch; a store
// spills the value to a group first (a wasm store needs its address right
// under its value), then stores each scalar from there.

/// A value of type `ty` as wasm sees it: its scalars and their byte offsets
/// in its memory layout (one at 0 for a scalar, none for void).
fn flat(ty: &Type, structs: &crate::checker::Structs) -> Vec<(Type, u32)> {
    match ty {
        Type::Void => vec![],
        Type::Struct(n) => match structs.get(n) {
            Some(d) if d.is_value => {
                let mut out = Vec::new();
                for f in &d.fields {
                    let off = crate::checker::get_field_offset(d, &f.name, structs).map(|(o, _)| o as u32).unwrap_or(0);
                    out.extend(flat(&f.ty, structs).into_iter().map(|(t, o)| (t, off + o)));
                }
                out
            }
            _ => vec![(ty.clone(), 0)],
        },
        // a union or result by value: its slots (checker::SumLayout)
        Type::Union(_) | Type::ResultType(..) if structs.sum_of(ty).is_some() => {
            let l = structs.sum_of(ty).unwrap_or_else(|| unreachable!());
            l.slots.iter().cloned().zip(l.offsets.iter().copied()).collect()
        }
        _ => vec![(ty.clone(), 0)],
    }
}

fn is_value_type(ty: &Type, structs: &crate::checker::Structs) -> bool {
    match ty {
        Type::Struct(n) => structs.get(n).is_some_and(|d| d.is_value),
        Type::Union(n) => structs.is_value_union(n),
        Type::ResultType(a, b) => structs.result_layout(a, b).is_some(),
        _ => false,
    }
}

/// How many wasm values (locals, results) a value of type `ty` is.
fn width(ty: &Type, structs: &crate::checker::Structs) -> u32 {
    flat(ty, structs).len() as u32
}

fn valtypes(ty: &Type, structs: &crate::checker::Structs) -> Vec<ValType> {
    flat(ty, structs).iter().map(|(t, _)| aipl_to_wasm_type(t)).collect()
}

/// Field `f` of value struct `s`: its first scalar among the struct's, how
/// many it is, its byte offset, and its type.
fn field_slots(s: &str, f: &str, structs: &crate::checker::Structs) -> Result<(u32, u32, u32, Type), String> {
    let def = structs.get(s).ok_or_else(|| format!("Wasm Codegen: Unknown struct '{}'", s))?;
    let mut slot = 0;
    for g in &def.fields {
        let w = width(&g.ty, structs);
        if g.name == f {
            let (off, _) = crate::checker::get_field_offset(def, f, structs)?;
            return Ok((slot, w, off as u32, g.ty.clone()));
        }
        slot += w;
    }
    Err(format!("Wasm Codegen: struct '{}' has no field '{}'", s, f))
}

fn emit_get_slots(func: &mut Body, base: u32, n: u32) {
    for k in 0..n {
        func.instruction(&Instruction::LocalGet(base + k));
    }
}

fn emit_set_slots(func: &mut Body, base: u32, n: u32) {
    for k in (0..n).rev() {
        func.instruction(&Instruction::LocalSet(base + k));
    }
}

/// Pushes the value of type `ty` stored `offset` bytes past the address in
/// addr_scratch, scalar by scalar.
fn emit_load_scalars(func: &mut Body, ctx: &Ctx, ty: &Type, offset: u32) -> Result<(), String> {
    for (t, o) in flat(ty, ctx.structs) {
        func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
        func.instruction(&load_instruction(&t, (offset + o) as u64)?);
        if t == Type::Bool {
            normalize_bool(func);
        }
    }
    Ok(())
}

/// With an address on the stack: stores the value struct `val` computes
/// there, `offset` bytes on. The value goes to its spill group (its
/// scalars all arrive at once), then the address to addr_scratch (only
/// now: computing the value may use it), then each scalar is stored.
fn emit_store_value(func: &mut Body, ctx: &Ctx, ty: &Type, offset: u32, val: &Expr) -> Result<(), String> {
    let g = *ctx.groups.get(&(2 * (val as *const Expr as usize) + 1)).ok_or("Wasm Codegen: a value store has no group")?;
    compile_expr(val, ctx, func)?;
    emit_set_slots(func, g, width(ty, ctx.structs));
    func.instruction(&Instruction::LocalSet(ctx.addr_scratch));
    for (k, (t, o)) in flat(ty, ctx.structs).iter().enumerate() {
        func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
        func.instruction(&Instruction::LocalGet(g + k as u32));
        func.instruction(&store_instruction(t, (offset + o) as u64)?);
    }
    Ok(())
}

/// Drops the value an expression of type `ty` leaves.
fn emit_drops(func: &mut Body, ctx: &Ctx, ty: &Type) {
    for _ in 0..width(ty, ctx.structs).max(1) {
        func.instruction(&Instruction::Drop);
    }
}

/// A read from a value, (get v V.f) or a chain of them: from the locals of
/// a variable, from memory for a value in a struct's field or an array
/// element, else from the root's spill group.
fn compile_value_read(top: &Expr, ctx: &Ctx, func: &mut Body) -> Result<(), String> {
    let tyf = |e: &Expr| expr_type(e, ctx);
    let (root, steps) = value_chain(top, &tyf, ctx.structs);
    let (mut slot, mut off, mut t) = (0, 0, tyf(root));
    for (st, f) in steps {
        let (sl, _, o, ft) = field_slots(st, f, ctx.structs)?;
        slot += sl;
        off += o;
        t = ft;
    }
    let n = width(&t, ctx.structs);
    match root {
        Expr::Var(name, _) => {
            let base = *ctx.locals.get(name).ok_or_else(|| format!("Wasm Codegen: Unbound local variable '{}'", name))?;
            emit_get_slots(func, base + slot, n);
        }
        Expr::GetField { struct_name, field_name, ptr, .. } if !is_value_type(&tyf(ptr), ctx.structs) => {
            let def = ctx.structs.get(struct_name).ok_or_else(|| format!("Wasm Codegen: Unknown struct '{}'", struct_name))?;
            let (foff, _) = crate::checker::get_field_offset(def, field_name, ctx.structs)?;
            compile_pointer(ptr, ctx, func)?;
            func.instruction(&Instruction::LocalSet(ctx.addr_scratch));
            emit_load_scalars(func, ctx, &t, foff as u32 + off)?;
        }
        Expr::ArrGet { elem_ty, ptr, index, .. } => {
            emit_element_address(elem_ty, ptr, index, ctx, func)?;
            func.instruction(&Instruction::LocalSet(ctx.addr_scratch));
            emit_load_scalars(func, ctx, &t, off)?;
        }
        _ => {
            let g = *ctx.groups.get(&(2 * (root as *const Expr as usize) + 1)).ok_or("Wasm Codegen: a value read has no group")?;
            compile_expr(root, ctx, func)?;
            emit_set_slots(func, g, width(&tyf(root), ctx.structs));
            emit_get_slots(func, g + slot, n);
        }
    }
    Ok(())
}

/// arr.get's and arr.set's element address: the array (null-checked), the
/// index (bounds-checked), times the element's size, added.
fn emit_element_address(elem_ty: &Type, ptr: &Expr, index: &Expr, ctx: &Ctx, func: &mut Body) -> Result<(), String> {
    let (elem_size, _) = crate::checker::type_size_and_align(elem_ty, ctx.structs)?;
    compile_pointer(ptr, ctx, func)?;
    compile_expr(index, ctx, func)?;
    emit_bounds_check(func, ctx);
    func.instruction(&Instruction::I32Const(elem_size as i32));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    Ok(())
}

/// Compiles `expr`, a value struct, into the locals from `base`: the
/// branches of an if or match, and a block's last expression, go there
/// directly; anything else is pushed and stored.
fn compile_into(expr: &Expr, base: u32, ctx: &Ctx, func: &mut Body) -> Result<(), String> {
    func.at.push(ctx.position(expr.span()));
    let r = compile_into_at(expr, base, ctx, func);
    func.at.pop();
    r
}

fn compile_into_at(expr: &Expr, base: u32, ctx: &Ctx, func: &mut Body) -> Result<(), String> {
    match expr {
        Expr::If { cond, then_branch, else_branch, .. } => {
            compile_expr(cond, ctx, func)?;
            func.instruction(&Instruction::If(BlockType::Empty));
            with_label(ctx, Label::Plain, || {
                compile_into(then_branch, base, ctx, func)?;
                func.instruction(&Instruction::Else);
                compile_into(else_branch, base, ctx, func)
            })?;
            func.instruction(&Instruction::End);
        }
        Expr::Block(exprs, _) | Expr::Unsafe(exprs, _) if !exprs.is_empty() => {
            let last = exprs.len() - 1;
            for e in &exprs[..last] {
                compile_stmt(e, ctx, func)?;
            }
            compile_into(&exprs[last], base, ctx, func)?;
        }
        Expr::Match { value, arms, else_body, .. } => compile_match(value, arms, else_body, Some(base), ctx, func)?,
        Expr::MatchResult { .. } => compile_match_result(expr, Some(base), ctx, func)?,
        _ => {
            compile_expr(expr, ctx, func)?;
            emit_set_slots(func, base, width(&expr_type(expr, ctx), ctx.structs));
        }
    }
    Ok(())
}

/// The root of a chain of field reads from a value, (get (get e A.f) B.g):
/// the first expression down the chain that is not such a read, and the
/// reads from it outward.
fn value_chain<'e>(expr: &'e Expr, ty: &dyn Fn(&Expr) -> Type, structs: &crate::checker::Structs) -> (&'e Expr, Vec<(&'e String, &'e String)>) {
    let mut steps = Vec::new();
    let mut e = expr;
    while let Expr::GetField { struct_name, field_name, ptr, .. } = e {
        if !is_value_type(&ty(ptr), structs) {
            break;
        }
        steps.push((struct_name, field_name));
        e = ptr;
    }
    steps.reverse();
    (e, steps)
}

/// Where the root of a value chain is: a local, memory (a field of a struct
/// behind a pointer, an array element), or computed (any other expression).
fn root_in_place(root: &Expr, ty: &dyn Fn(&Expr) -> Type, structs: &crate::checker::Structs) -> bool {
    match root {
        Expr::Var(..) | Expr::ArrGet { .. } => true,
        Expr::GetField { ptr, .. } => !is_value_type(&ty(ptr), structs),
        _ => false,
    }
}

/// The scratch groups of a function's body and contracts (value_groups'
/// walk, in source order): each node that needs one (by address), with the
/// type of the value it holds.
fn value_groups(exprs: &[&Expr], ty: &dyn Fn(&Expr) -> Type, structs: &crate::checker::Structs, body_into: bool) -> Vec<(usize, Type)> {
    fn walk(e: &Expr, into: bool, ty: &dyn Fn(&Expr) -> Type, structs: &crate::checker::Structs, out: &mut Vec<(usize, Type)>) {
        // a node's own group (an if, match, or with) is keyed 2*address, a
        // chain root's spill group 2*address+1 (a root may have both)
        let key = 2 * (e as *const Expr as usize);
        let body = |b: &[Expr], into: bool, out: &mut Vec<(usize, Type)>| {
            for (i, x) in b.iter().enumerate() {
                walk(x, into && i + 1 == b.len(), ty, structs, out);
            }
        };
        match e {
            Expr::If { cond, then_branch, else_branch, .. } => {
                let t = ty(then_branch);
                let v = is_value_type(&t, structs);
                if v && !into {
                    out.push((key, t));
                }
                walk(cond, false, ty, structs, out);
                walk(then_branch, v, ty, structs, out);
                walk(else_branch, v, ty, structs, out);
            }
            Expr::Match { value, arms, else_body, .. } => {
                let t = ty(e);
                let v = is_value_type(&t, structs);
                if v && !into {
                    out.push((key, t));
                }
                // a union by value matched where it is computed spills
                let vt = ty(value);
                if matches!(&vt, Type::Union(n) if structs.is_value_union(n)) && !matches!(value.as_ref(), Expr::Var(..)) {
                    out.push((2 * (value.as_ref() as *const Expr as usize) + 1, vt));
                }
                walk(value, false, ty, structs, out);
                for a in arms {
                    body(&a.body, v, out);
                }
                if let Some(b) = else_body {
                    body(b, v, out);
                }
            }
            Expr::MatchResult { expr, ok_body, err_body, .. } => {
                let t = ty(e);
                let v = is_value_type(&t, structs);
                if v && !into {
                    out.push((key, t));
                }
                // a result by value matched where it is computed spills
                let rt = ty(expr);
                if is_value_type(&rt, structs) && !matches!(expr.as_ref(), Expr::Var(..)) {
                    out.push((2 * (expr.as_ref() as *const Expr as usize) + 1, rt));
                }
                walk(expr, false, ty, structs, out);
                body(ok_body, v, out);
                body(err_body, v, out);
            }
            Expr::Block(b, _) | Expr::Unsafe(b, _) => body(b, into, out),
            Expr::Ok(inner, ..) | Expr::Err(inner, ..) if is_value_type(&ty(e), structs) => {
                out.push((key, ty(e)));
                walk(inner, false, ty, structs, out);
            }
            Expr::Make { union_name, args, .. } if structs.is_value_union(union_name) => {
                out.push((key, ty(e)));
                for a in args {
                    walk(a, false, ty, structs, out);
                }
            }
            Expr::With { value, new_val, .. } => {
                out.push((key, ty(e)));
                walk(value, false, ty, structs, out);
                walk(new_val, false, ty, structs, out);
            }
            Expr::GetField { ptr, .. } if is_value_type(&ty(ptr), structs) => {
                let (root, _) = value_chain(e, ty, structs);
                if !root_in_place(root, ty, structs) {
                    out.push((2 * (root as *const Expr as usize) + 1, ty(root)));
                    walk(root, false, ty, structs, out);
                } else {
                    match root {
                        Expr::GetField { ptr, .. } => walk(ptr, false, ty, structs, out),
                        Expr::ArrGet { ptr, index, .. } => {
                            walk(ptr, false, ty, structs, out);
                            walk(index, false, ty, structs, out);
                        }
                        _ => {}
                    }
                }
            }
            _ => {
                // a value struct stored to memory goes through its spill group
                for v in stored_values(e, ty, structs) {
                    out.push((2 * (v as *const Expr as usize) + 1, ty(v)));
                }
                let mut kids: Vec<&Expr> = Vec::new();
                children(e, &mut kids);
                for k in kids {
                    walk(k, false, ty, structs, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    for (i, e) in exprs.iter().enumerate() {
        walk(e, body_into && i + 1 == exprs.len(), ty, structs, &mut out);
    }
    out
}

/// The value structs node `e` stores to memory: a put's or arr.set's value
/// whose target (after any path) is one, an ok's or err's, a make's fields
/// that are (in order).
fn stored_values<'e>(e: &'e Expr, ty: &dyn Fn(&Expr) -> Type, structs: &crate::checker::Structs) -> Vec<&'e Expr> {
    let value = |t: &Type| is_value_type(t, structs);
    let follow = |mut t: Type, path: &[(String, String)]| {
        for (st, f) in path {
            if let Ok((_, _, _, ft)) = field_slots(st, f, structs) {
                t = ft;
            }
        }
        t
    };
    match e {
        Expr::PutField { struct_name, field_name, val, path, .. } => {
            let first = structs.get(struct_name).and_then(|d| crate::checker::get_field_offset(d, field_name, structs).ok()).map(|(_, t)| t).unwrap_or(Type::I32);
            if value(&follow(first, path)) { vec![val.as_ref()] } else { vec![] }
        }
        Expr::ArrSet { elem_ty, val, path, .. } => {
            if value(&follow(elem_ty.clone(), path)) { vec![val.as_ref()] } else { vec![] }
        }
        Expr::Ok(inner, ..) | Expr::Err(inner, ..) => {
            if value(&ty(inner)) { vec![inner.as_ref()] } else { vec![] }
        }
        Expr::Make { args, .. } => args.iter().filter(|a| value(&ty(a))).collect(),
        _ => vec![],
    }
}

/// An expression's direct subexpressions, in source (evaluation) order.
fn children<'e>(e: &'e Expr, out: &mut Vec<&'e Expr>) {
    match e {
        Expr::Lit(..) | Expr::Var(..) | Expr::Sizeof { .. } | Expr::Null { .. } | Expr::Ref { .. } => {}
        Expr::Break(_) | Expr::Continue(_) => {}
        Expr::Let { val, .. } | Expr::Set { val, .. } => out.push(val),
        Expr::If { cond, then_branch, else_branch, .. } => out.extend([cond.as_ref(), then_branch, else_branch]),
        Expr::Loop { start, end, step, body, .. } => {
            out.extend([start.as_ref(), end, step]);
            out.extend(body.iter());
        }
        Expr::While { cond, body, .. } => {
            out.push(cond);
            out.extend(body.iter());
        }
        Expr::Call { args, .. } | Expr::Op { args, .. } | Expr::Make { args, .. } | Expr::Record { args, .. } => out.extend(args.iter()),
        Expr::Ok(inner, ..) | Expr::Err(inner, ..) => out.push(inner),
        Expr::MatchResult { expr, ok_body, err_body, .. } => {
            out.push(expr);
            out.extend(ok_body.iter());
            out.extend(err_body.iter());
        }
        Expr::Block(b, _) | Expr::Unsafe(b, _) => out.extend(b.iter()),
        Expr::Cast { addr, gen: Some(g), .. } => out.extend([addr.as_ref(), g]),
        Expr::GetField { ptr, .. } | Expr::ArrLen { arr: ptr, .. } | Expr::Cast { addr: ptr, .. } | Expr::Addr { val: ptr, .. } => out.push(ptr),
        Expr::PutField { ptr, val, .. } => out.extend([ptr.as_ref(), val]),
        Expr::ArrGet { ptr, index, .. } => out.extend([ptr.as_ref(), index]),
        Expr::ArrSet { ptr, index, val, .. } => out.extend([ptr.as_ref(), index, val]),
        Expr::With { value, new_val, .. } => out.extend([value.as_ref(), new_val]),
        Expr::Match { value, arms, else_body, .. } => {
            out.push(value);
            for a in arms {
                out.extend(a.body.iter());
            }
            if let Some(b) = else_body {
                out.extend(b.iter());
            }
        }
        Expr::Return { val, .. } => {
            if let Some(v) = val {
                out.push(v);
            }
        }
        Expr::CallRef { func, args, .. } => {
            out.push(func);
            out.extend(args.iter());
        }
    }
}

/// Static AIPL type of an expression, as the checker would assign it. Codegen
/// uses this to select i32 / i64 / f32 / f64 instruction variants and `if`
/// block result types. The checker has already rejected ill-typed programs,
/// so the fallbacks here (`I32`) are only reached for constructs the checker
/// treats as untyped (e.g. `arr.get`), never for well-typed numeric code.
fn expr_type(expr: &Expr, ctx: &Ctx) -> Type {
    type_in(expr, &Typing { local_types: ctx.local_types, fn_returns: ctx.fn_returns, fn_types: ctx.fn_types, structs: ctx.structs })
}

/// What expr_type reads: available before a function's Ctx exists, while
/// its locals' types are still being worked out (binder_types).
struct Typing<'a> {
    local_types: &'a HashMap<String, Type>,
    fn_returns: &'a HashMap<String, Type>,
    fn_types: &'a HashMap<String, Type>,
    structs: &'a crate::checker::Structs,
}

fn type_in(expr: &Expr, ctx: &Typing) -> Type {
    let expr_type = type_in;
    match expr {
        Expr::Lit(lit, _) => match lit {
            Literal::Int(_) => Type::I32,
            Literal::Int64(_) => Type::I64,
            Literal::Float(_) => Type::F64,
            Literal::Float32(..) => Type::F32,
            Literal::Bool(_) => Type::Bool,
            Literal::Str(_) => Type::Str,
        },
        Expr::Var(name, _) => ctx.local_types.get(name).cloned().unwrap_or(Type::I32),
        Expr::Let { .. } | Expr::Set { .. } => Type::Void,
        // a branch that always leaves takes the other's type (checker::diverges)
        Expr::If { then_branch, else_branch, .. } => {
            expr_type(if crate::checker::diverges(then_branch) { else_branch } else { then_branch }, ctx)
        }
        Expr::Block(exprs, _) | Expr::Unsafe(exprs, _) => exprs.last().map_or(Type::Void, |e| expr_type(e, ctx)),
        Expr::Loop { .. } | Expr::While { .. } => Type::Void,
        Expr::Call { func, .. } => ctx.fn_returns.get(func).cloned().unwrap_or(Type::I32),
        Expr::Ok(inner, other, _) => Type::ResultType(Box::new(expr_type(inner, ctx)), Box::new(other.clone().unwrap_or(Type::I32))),
        Expr::Err(inner, other, _) => Type::ResultType(Box::new(other.clone().unwrap_or(Type::I32)), Box::new(expr_type(inner, ctx))),
        Expr::MatchResult { ok_body, err_body, .. } => {
            let body = if crate::checker::body_diverges(ok_body) { err_body } else { ok_body };
            body.last().map_or(Type::Void, |e| expr_type(e, ctx))
        }
        Expr::Make { union_name, .. } => Type::Union(union_name.clone()),
        Expr::Record { struct_name, .. } | Expr::With { struct_name, .. } => Type::Struct(struct_name.clone()),
        Expr::Match { arms, else_body, .. } => match_type_body(arms, else_body).last().map_or(Type::Void, |e| expr_type(e, ctx)),
        Expr::GetField { struct_name, field_name, .. } => {
            if let Some(def) = ctx.structs.get(struct_name) {
                if let Ok((_, field_ty)) = crate::checker::get_field_offset(def, field_name, ctx.structs) {
                    return crate::checker::value_of(&field_ty);
                }
            }
            Type::I32
        }
        Expr::PutField { .. } => Type::Void,
        Expr::Sizeof { .. } => Type::I32,
        Expr::ArrLen { .. } | Expr::Addr { .. } => Type::I32,
        Expr::Null { ty, .. } | Expr::Cast { ty, .. } => ty.clone(),
        Expr::Ref { name, .. } => ctx.fn_types.get(name).cloned().unwrap_or(Type::I32),
        Expr::Return { .. } | Expr::Break(_) | Expr::Continue(_) => Type::Void,
        Expr::CallRef { sig, .. } => match sig {
            Type::Fn(_, ret, _) => (**ret).clone(),
            _ => Type::I32,
        },
        Expr::ArrGet { elem_ty, .. } => crate::checker::value_of(elem_ty),
        Expr::ArrSet { .. } => Type::Void,
        Expr::Op { op, args, .. } => match op {
            OpCode::FormatStr => Type::Str,
            OpCode::FormatTo | OpCode::FormatPrint | OpCode::FormatPrintln | OpCode::FormatEprintln | OpCode::FormatPiece => Type::Void,
            OpCode::Try | OpCode::TryOpt | OpCode::TryOr | OpCode::Defer | OpCode::FormatDbg | OpCode::FormatExpect => Type::Void,
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
            | OpCode::CheckedAdd
            | OpCode::CheckedSub
            | OpCode::CheckedMul => args.first().map_or(Type::I32, |a| expr_type(a, ctx)),
            OpCode::Eq
            | OpCode::Neq
            | OpCode::Lt
            | OpCode::Lte
            | OpCode::Gt
            | OpCode::Gte
            | OpCode::LtU
            | OpCode::LteU
            | OpCode::GtU
            | OpCode::GteU
            | OpCode::And
            | OpCode::Or
            | OpCode::Not
            | OpCode::AtomicCas => Type::Bool,
            OpCode::MemLoad64
            | OpCode::I64ExtendS
            | OpCode::I64ExtendU
            | OpCode::I64TruncF64S
            | OpCode::I64ReinterpretF64 => Type::I64,
            OpCode::F64ConvertI64S | OpCode::F64ReinterpretI64 | OpCode::F64Sqrt => Type::F64,
            OpCode::F32DemoteF64
            | OpCode::F64PromoteF32
            | OpCode::F32ConvertI32S
            | OpCode::F32ConvertI64S
            | OpCode::F64ConvertI32S
            | OpCode::I32TruncF32S
            | OpCode::I32TruncF64S
            | OpCode::I64TruncF32S
            | OpCode::F32ReinterpretI32
            | OpCode::I32ReinterpretF32 => crate::checker::conversion(op).1,
            OpCode::SysTime | OpCode::SysMonotonic => Type::I64,
            OpCode::SysRandom => Type::I32,
            OpCode::MemStore8
            | OpCode::MemStore16
            | OpCode::MemStore32
            | OpCode::MemStore64
            | OpCode::AtomicLock
            | OpCode::AtomicUnlock
            | OpCode::SysPrint
            | OpCode::SysExit => Type::Void,
            OpCode::MemLoad8
            | OpCode::MemLoad8S
            | OpCode::MemLoad16U
            | OpCode::MemLoad16S
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
            | OpCode::FsOpendir
            | OpCode::FsReaddir
            | OpCode::FsStat
            | OpCode::ArgsSizes
            | OpCode::ArgsGet
            | OpCode::EnvSizes
            | OpCode::EnvGet
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
                "Wasm Codegen: {} is not supported for operands of type {}",
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
        (OpCode::Eq, Type::I32 | Type::Bool | Type::Str | Type::Fn(..) | Type::Enum(_)) => I32Eq,
        (OpCode::Neq, Type::I32 | Type::Bool | Type::Str | Type::Fn(..) | Type::Enum(_)) => I32Ne,
        // pointers and arrays: address and generation (a cast of 0 is null)
        (OpCode::Eq, Type::Ptr(_) | Type::Array(_)) => I64Eq,
        (OpCode::Neq, Type::Ptr(_) | Type::Array(_)) => I64Ne,
        (OpCode::Lt, Type::I32) => I32LtS,
        (OpCode::Lte, Type::I32) => I32LeS,
        (OpCode::Gt, Type::I32) => I32GtS,
        (OpCode::Gte, Type::I32) => I32GeS,
        (OpCode::LtU, Type::I32) => I32LtU,
        (OpCode::LteU, Type::I32) => I32LeU,
        (OpCode::GtU, Type::I32) => I32GtU,
        (OpCode::GteU, Type::I32) => I32GeU,

        (OpCode::Eq, Type::I64) => I64Eq,
        (OpCode::Neq, Type::I64) => I64Ne,
        (OpCode::Lt, Type::I64) => I64LtS,
        (OpCode::Lte, Type::I64) => I64LeS,
        (OpCode::Gt, Type::I64) => I64GtS,
        (OpCode::Gte, Type::I64) => I64GeS,
        (OpCode::LtU, Type::I64) => I64LtU,
        (OpCode::LteU, Type::I64) => I64LeU,
        (OpCode::GtU, Type::I64) => I64GtU,
        (OpCode::GteU, Type::I64) => I64GeU,

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
                "Wasm Codegen: {} is not supported for operands of type {}",
                op, ty
            ))
        }
    };
    Ok(ins)
}

/// The hidden i64 locals of a function that uses checked arithmetic: the
/// operands and the result (codegen.aipl markers Ty.ck_a, ck_b, ck_r).
const CHECKED_LOCALS: [&str; 3] = ["#ck_a", "#ck_b", "#ck_r"];

/// checked.add/sub/mul: the operation, then a trap if it overflowed.
/// i32: computed exactly in i64 and trapped unless it fits in i32. i64 add
/// and sub: the sign test ((a^r)&(b^r) < 0, (a^b)&(a^r) < 0); i64 mul: when
/// a != 0, r / a must be b (a = -1, b = MIN traps in the division itself).
/// Both operands are evaluated before any hidden local is written, so a
/// checked operation nested in an operand cannot clobber them.
fn compile_checked(op: &OpCode, args: &[Expr], ctx: &Ctx, func: &mut Body) -> Result<(), String> {
    use Instruction::*;
    let local = |n: &str| *ctx.locals.get(n).expect("checked arithmetic locals");
    let (a, b, r) = (local(CHECKED_LOCALS[0]), local(CHECKED_LOCALS[1]), local(CHECKED_LOCALS[2]));
    // The trap is i32.div_s(MIN, -1), which wasm defines to trap with
    // "integer overflow", so every host reports the real reason.
    let trap_if = |func: &mut Body| {
        func.instruction(&If(wasm_encoder::BlockType::Empty));
        func.instruction(&I32Const(i32::MIN));
        func.instruction(&I32Const(-1));
        func.instruction(&I32DivS);
        func.instruction(&Drop);
        func.instruction(&End);
    };
    if expr_type(&args[0], ctx) == Type::I32 {
        compile_expr(&args[0], ctx, func)?;
        func.instruction(&I64ExtendI32S);
        compile_expr(&args[1], ctx, func)?;
        func.instruction(&I64ExtendI32S);
        func.instruction(match op {
            OpCode::CheckedAdd => &I64Add,
            OpCode::CheckedSub => &I64Sub,
            _ => &I64Mul,
        });
        func.instruction(&LocalTee(r));
        func.instruction(&I64Const(2147483648));
        func.instruction(&I64Add);
        func.instruction(&I64Const(4294967296));
        func.instruction(&I64GeU);
        trap_if(func);
        func.instruction(&LocalGet(r));
        func.instruction(&I32WrapI64);
        return Ok(());
    }
    compile_expr(&args[0], ctx, func)?;
    compile_expr(&args[1], ctx, func)?;
    func.instruction(&LocalSet(b));
    func.instruction(&LocalSet(a));
    func.instruction(&LocalGet(a));
    func.instruction(&LocalGet(b));
    match op {
        OpCode::CheckedAdd | OpCode::CheckedSub => {
            func.instruction(if *op == OpCode::CheckedAdd { &I64Add } else { &I64Sub });
            func.instruction(&LocalSet(r));
            func.instruction(&LocalGet(a));
            func.instruction(&LocalGet(if *op == OpCode::CheckedAdd { r } else { b }));
            func.instruction(&I64Xor);
            func.instruction(&LocalGet(if *op == OpCode::CheckedAdd { b } else { a }));
            func.instruction(&LocalGet(r));
            func.instruction(&I64Xor);
            func.instruction(&I64And);
            func.instruction(&I64Const(0));
            func.instruction(&I64LtS);
            trap_if(func);
        }
        _ => {
            func.instruction(&I64Mul);
            func.instruction(&LocalSet(r));
            func.instruction(&LocalGet(a));
            func.instruction(&I64Const(0));
            func.instruction(&I64Ne);
            func.instruction(&If(wasm_encoder::BlockType::Empty));
            func.instruction(&LocalGet(r));
            func.instruction(&LocalGet(a));
            func.instruction(&I64DivS);
            func.instruction(&LocalGet(b));
            func.instruction(&I64Ne);
            trap_if(func);
            func.instruction(&End);
        }
    }
    func.instruction(&LocalGet(r));
    Ok(())
}

/// Every local a body declares, in the order the self-hosted compiler
/// numbers them (codegen.aipl `collect_locals_walk`): a pre-order walk, each
/// node's own names first, then its children in source order. No wildcard:
/// a new expression form must say what it declares and what it contains (a
/// `(+ 1 (match_result ...))` once lost its arm variables to a `_` arm).
fn collect_lets(exprs: &[Expr], lets: &mut Vec<(String, Type)>, unions: &HashMap<String, UnionDef>) {
    for expr in exprs {
        match expr {
            Expr::Let { name, ty, val, .. } => {
                lets.push((name.clone(), ty.clone()));
                collect_lets(std::slice::from_ref(val.as_ref()), lets, unions);
            }
            // The induction variable is never declared via `let` but needs a
            // slot, and its end and step are evaluated once into two hidden
            // locals named after it (sequential loops over the same variable
            // share them; nested loops cannot reuse a name).
            Expr::Loop { var, start, end, step, body, .. } => {
                lets.push((var.clone(), Type::I32));
                lets.push((format!("{}#end", var), Type::I32));
                lets.push((format!("{}#step", var), Type::I32));
                collect_lets(&[*start.clone(), *end.clone(), *step.clone()], lets, unions);
                collect_lets(body, lets, unions);
            }
            Expr::MatchResult { expr, ok_var, ok_body, err_var, err_body, .. } => {
                lets.push((ok_var.clone(), Type::I32));
                lets.push((err_var.clone(), Type::I32));
                collect_lets(std::slice::from_ref(expr.as_ref()), lets, unions);
                collect_lets(ok_body, lets, unions);
                collect_lets(err_body, lets, unions);
            }
            Expr::Set { val: e, .. }
            | Expr::Ok(e, _, _)
            | Expr::Err(e, _, _)
            | Expr::Return { val: Some(e), .. }
            | Expr::GetField { ptr: e, .. }
            | Expr::ArrLen { arr: e, .. }
            | Expr::Cast { addr: e, gen: None, .. }
            | Expr::Addr { val: e, .. } => collect_lets(std::slice::from_ref(e.as_ref()), lets, unions),
            Expr::Cast { addr, gen: Some(g), .. } => collect_lets(&[*addr.clone(), *g.clone()], lets, unions),
            Expr::If { cond, then_branch, else_branch, .. } => {
                collect_lets(&[*cond.clone(), *then_branch.clone(), *else_branch.clone()], lets, unions);
            }
            Expr::While { cond, body, .. } => {
                collect_lets(std::slice::from_ref(cond.as_ref()), lets, unions);
                collect_lets(body, lets, unions);
            }
            Expr::Block(body, _) | Expr::Unsafe(body, _) => collect_lets(body, lets, unions),
            // Every arm's binders first (a union arm binds its variant's
            // fields, typed as declared), then the value, the arm bodies, and
            // the else body.
            Expr::Match { value, arms, else_body, .. } => {
                for arm in arms {
                    let Some(names) = &arm.binders else { continue };
                    let dot = arm.member.rfind('.').unwrap_or(0);
                    let fields = unions
                        .get(&arm.member[..dot])
                        .and_then(|u| u.variants.iter().find(|v| v.name == arm.member[dot + 1..]))
                        .map(|v| v.fields.clone())
                        .unwrap_or_default();
                    for (n, f) in names.iter().zip(fields.iter()).filter(|(n, _)| n.as_str() != "_") {
                        lets.push((n.clone(), f.ty.clone()));
                    }
                }
                collect_lets(std::slice::from_ref(value.as_ref()), lets, unions);
                for arm in arms {
                    collect_lets(&arm.body, lets, unions);
                }
                if let Some(body) = else_body {
                    collect_lets(body, lets, unions);
                }
            }
            Expr::Make { args, .. } | Expr::Record { args, .. } => collect_lets(args, lets, unions),
            Expr::With { value, new_val, .. } => {
                collect_lets(std::slice::from_ref(value), lets, unions);
                collect_lets(std::slice::from_ref(new_val), lets, unions);
            }
            Expr::Op { op: OpCode::CheckedAdd | OpCode::CheckedSub | OpCode::CheckedMul, args, .. } => {
                for n in CHECKED_LOCALS {
                    lets.push((n.to_string(), Type::I64));
                }
                collect_lets(args, lets, unions)
            }
            Expr::Call { args, .. } | Expr::Op { args, .. } => collect_lets(args, lets, unions),
            Expr::CallRef { func, args, .. } => {
                collect_lets(std::slice::from_ref(func.as_ref()), lets, unions);
                collect_lets(args, lets, unions);
            }
            Expr::PutField { ptr, val, .. } => {
                collect_lets(&[*ptr.clone(), *val.clone()], lets, unions);
            }
            Expr::ArrGet { ptr, index, .. } => {
                collect_lets(&[*ptr.clone(), *index.clone()], lets, unions);
            }
            Expr::ArrSet { ptr, index, val, .. } => {
                collect_lets(&[*ptr.clone(), *index.clone(), *val.clone()], lets, unions);
            }
            Expr::Lit(..)
            | Expr::Var(..)
            | Expr::Sizeof { .. }
            | Expr::Null { .. }
            | Expr::Ref { .. }
            | Expr::Return { val: None, .. }
            | Expr::Break(_)
            | Expr::Continue(_) => {}
        }
    }
}

fn aipl_to_wasm_type(ty: &Type) -> ValType {
    match ty {
        Type::I32 | Type::Bool | Type::Str | Type::Void | Type::Narrow(_) => ValType::I32,
        Type::I64 => ValType::I64,
        Type::F32 => ValType::F32,
        Type::F64 => ValType::F64,
        // Pointers and arrays: the address in the low half, the generation in
        // the high half (docs/design/GENERATIONS_PLAN.md).
        Type::Ptr(_) | Type::Array(_) => ValType::I64,
        // Results, unions, and function refs are i32 addresses/indices.
        Type::Struct(_) | Type::ResultType(_, _) | Type::Fn(..) | Type::Enum(_) | Type::Union(_) => ValType::I32,
    }
}

/// Compiles a statement that appears in a purely-effectful position (a
/// non-final entry in a block/function body, or any statement in a while/loop
/// body): the statement runs, and any value it leaves behind is dropped so it
/// never corrupts the surrounding block's stack balance.
fn compile_stmt(expr: &Expr, ctx: &Ctx, func: &mut Body) -> Result<(), String> {
    compile_expr(expr, ctx, func)?;
    if !is_void_expr(expr, ctx) {
        emit_drops(func, ctx, &expr_type(expr, ctx));
    }
    Ok(())
}

/// Compiles one expression, its position the innermost one while it is
/// compiled (for the line table).
fn compile_expr(expr: &Expr, ctx: &Ctx, func: &mut Body) -> Result<(), String> {
    func.at.push(ctx.position(expr.span()));
    let r = compile_expr_at(expr, ctx, func);
    func.at.pop();
    r
}

fn compile_expr_at(expr: &Expr, ctx: &Ctx, func: &mut Body) -> Result<(), String> {
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
            Literal::Float32(f, _) => {
                func.instruction(&Instruction::F32Const(*f));
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
                emit_get_slots(func, idx, width(&expr_type(expr, ctx), ctx.structs).max(1));
            } else {
                return Err(format!("Wasm Codegen: Unbound local variable '{}'", name));
            }
        }
        Expr::Let { name, val, .. } | Expr::Set { name, val, .. } => {
            compile_expr(val, ctx, func)?;
            if let Some(&idx) = ctx.locals.get(name) {
                emit_set_slots(func, idx, width(&expr_type(val, ctx), ctx.structs).max(1));
            }
        }
        // an if that yields a value struct: through its group
        Expr::If { then_branch, .. } if is_value_type(&expr_type(then_branch, ctx), ctx.structs) => {
            let g = *ctx.groups.get(&(2 * (expr as *const Expr as usize))).ok_or("Wasm Codegen: a value if has no group")?;
            compile_into(expr, g, ctx, func)?;
            emit_get_slots(func, g, width(&expr_type(then_branch, ctx), ctx.structs));
        }
        Expr::If { cond, then_branch, else_branch, .. } => {
            compile_expr(cond, ctx, func)?;
            // the block's type is the branch's that does not always leave
            let typed = if crate::checker::diverges(then_branch) { else_branch } else { then_branch };
            let block_ty = if is_void_expr(typed, ctx) {
                BlockType::Empty
            } else {
                BlockType::Result(aipl_to_wasm_type(&expr_type(typed, ctx)))
            };
            func.instruction(&Instruction::If(block_ty));
            let valued = !matches!(block_ty, BlockType::Empty);
            with_label(ctx, Label::Plain, || {
                compile_expr(then_branch, ctx, func)?;
                seal(then_branch, valued, func);
                func.instruction(&Instruction::Else);
                compile_expr(else_branch, ctx, func)?;
                seal(else_branch, valued, func);
                Ok(())
            })?;
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
            OpCode::FormatStr | OpCode::FormatTo | OpCode::FormatPrint | OpCode::FormatPrintln | OpCode::FormatEprintln | OpCode::FormatPiece
            | OpCode::Try | OpCode::TryOpt | OpCode::TryOr | OpCode::Defer | OpCode::FormatDbg | OpCode::FormatExpect => {
                return Err(format!("Wasm Codegen: {op} is lowered by the resolver (formats::lower); compile a resolved module"));
            }
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
            OpCode::CheckedAdd | OpCode::CheckedSub | OpCode::CheckedMul => compile_checked(op, args, ctx, func)?,
            OpCode::MemLoad8 => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::I32Load8U(wasm_encoder::MemArg { offset: 0, align: 0, memory_index: 0 }));
            }
            OpCode::MemStore8 => {
                compile_expr(&args[0], ctx, func)?;
                emit_write_address_check(func, ctx);
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I32Store8(wasm_encoder::MemArg { offset: 0, align: 0, memory_index: 0 }));
            }
            OpCode::MemLoad8S | OpCode::MemLoad16U | OpCode::MemLoad16S => {
                compile_expr(&args[0], ctx, func)?;
                let n = match op {
                    OpCode::MemLoad8S => Narrow::I8,
                    OpCode::MemLoad16U => Narrow::U16,
                    _ => Narrow::I16,
                };
                func.instruction(&narrow_load(n, 0));
            }
            OpCode::MemStore16 => {
                compile_expr(&args[0], ctx, func)?;
                emit_write_address_check(func, ctx);
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&narrow_store(Narrow::U16, 0));
            }
            OpCode::MemLoad32 => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::I32Load(wasm_encoder::MemArg { offset: 0, align: 2, memory_index: 0 }));
            }
            OpCode::MemStore32 => {
                compile_expr(&args[0], ctx, func)?;
                emit_write_address_check(func, ctx);
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I32Store(wasm_encoder::MemArg { offset: 0, align: 2, memory_index: 0 }));
            }
            OpCode::MemLoad64 => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::I64Load(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
            }
            OpCode::MemStore64 => {
                compile_expr(&args[0], ctx, func)?;
                emit_write_address_check(func, ctx);
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
            }
            OpCode::MemAlloc => {
                // Bump allocator whose cursor is the i32 at linear-memory
                // address 0 (HEAP_PTR_ADDR) - the same word the VM uses, so
                // both backends and self-hosted AIPL share one allocator.
                // The size is evaluated first (it may allocate itself), rounded
                // up to a multiple of 8 so every block stays 8-aligned, then
                // one atomic add claims the block: [0] [size] -> rmw.add -> [old].
                func.instruction(&Instruction::I32Const(0));
                compile_expr(&args[0], ctx, func)?;
                emit_round8(func);
                func.instruction(&Instruction::I32AtomicRmwAdd(M4));
                emit_grow_to_cursor(func);
            }
            OpCode::MemGrow => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::MemoryGrow(0));
            }
            OpCode::Eq | OpCode::Neq | OpCode::Lt | OpCode::Lte | OpCode::Gt | OpCode::Gte
            | OpCode::LtU | OpCode::LteU | OpCode::GtU | OpCode::GteU => {
                let ty = expr_type(&args[0], ctx);
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&compare_instruction(op, &ty)?);
            }
            // Short-circuit: the second operand runs only when it decides the
            // result. (and a b) = if a then b else 0; (or a b) = if a then 1 else b.
            OpCode::And => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
                with_label(ctx, Label::Plain, || {
                    compile_expr(&args[1], ctx, func)?;
                    func.instruction(&Instruction::Else);
                    func.instruction(&Instruction::I32Const(0));
                    Ok(())
                })?;
                func.instruction(&Instruction::End);
            }
            OpCode::Or => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
                with_label(ctx, Label::Plain, || {
                    func.instruction(&Instruction::I32Const(1));
                    func.instruction(&Instruction::Else);
                    compile_expr(&args[1], ctx, func)
                })?;
                func.instruction(&Instruction::End);
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
            OpCode::F32DemoteF64
            | OpCode::F64PromoteF32
            | OpCode::F32ConvertI32S
            | OpCode::F32ConvertI64S
            | OpCode::F64ConvertI32S
            | OpCode::I32TruncF32S
            | OpCode::I32TruncF64S
            | OpCode::I64TruncF32S
            | OpCode::F32ReinterpretI32
            | OpCode::I32ReinterpretF32 => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&match op {
                    OpCode::F32DemoteF64 => Instruction::F32DemoteF64,
                    OpCode::F64PromoteF32 => Instruction::F64PromoteF32,
                    OpCode::F32ConvertI32S => Instruction::F32ConvertI32S,
                    OpCode::F32ConvertI64S => Instruction::F32ConvertI64S,
                    OpCode::F64ConvertI32S => Instruction::F64ConvertI32S,
                    OpCode::I32TruncF32S => Instruction::I32TruncF32S,
                    OpCode::I32TruncF64S => Instruction::I32TruncF64S,
                    OpCode::I64TruncF32S => Instruction::I64TruncF32S,
                    OpCode::F32ReinterpretI32 => Instruction::F32ReinterpretI32,
                    _ => Instruction::I32ReinterpretF32,
                });
            }
            OpCode::F64ConvertI64S => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::F64ConvertI64S);
            }
            OpCode::I64TruncF64S => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::I64TruncF64S);
            }
            OpCode::F64Sqrt => {
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::F64Sqrt);
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
                            "Wasm Codegen: sys.print supports str arguments only in the wasm backend, got {}",
                            ty
                        ));
                    }
                    compile_expr(arg, ctx, func)?;
                    func.instruction(&Instruction::LocalSet(ctx.addr_scratch));
                    // iov0 = { ptr, len }
                    emit_rt(func, ctx, RT_IOV0_BUF);
                    func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
                    func.instruction(&Instruction::I32Store(M4));
                    emit_rt(func, ctx, RT_IOV0_LEN);
                    func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
                    func.instruction(&Instruction::I32Const(4));
                    func.instruction(&Instruction::I32Sub);
                    func.instruction(&Instruction::I32Load(M4));
                    func.instruction(&Instruction::I32Store(M4));
                    // iov1 = { "\n", 1 }
                    emit_rt(func, ctx, RT_IOV1_BUF);
                    func.instruction(&Instruction::I32Const(ctx.newline_addr as i32));
                    func.instruction(&Instruction::I32Store(M4));
                    emit_rt(func, ctx, RT_IOV1_LEN);
                    func.instruction(&Instruction::I32Const(1));
                    func.instruction(&Instruction::I32Store(M4));
                    // One fd_write per iovec: WASI permits a short write and
                    // wasmtime's implementation writes only the first iovec of
                    // a call, so a single 2-iovec call would drop the newline.
                    // fd_write(1, iovs=64, iovs_len=1, nwritten=80); errno dropped
                    func.instruction(&Instruction::I32Const(1));
                    emit_rt(func, ctx, RT_IOV0_BUF);
                    func.instruction(&Instruction::I32Const(1));
                    emit_rt(func, ctx, RT_NBYTES);
                    func.instruction(&Instruction::Call(fd_write));
                    func.instruction(&Instruction::Drop);
                    // fd_write(1, iovs=72, iovs_len=1, nwritten=80)
                    func.instruction(&Instruction::I32Const(1));
                    emit_rt(func, ctx, RT_IOV1_BUF);
                    func.instruction(&Instruction::I32Const(1));
                    emit_rt(func, ctx, RT_NBYTES);
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

            // Atomics (AIPL_SPEC.md 4.D). Each address passes the store guard
            // first, as in the VM. Operands are evaluated left to right and
            // only then unloaded into the scratch locals, so nesting is safe.
            OpCode::AtomicAdd => {
                // returns the previous value
                compile_expr(&args[0], ctx, func)?;
                emit_write_address_check(func, ctx);
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::I32AtomicRmwAdd(M4));
            }
            OpCode::AtomicCas => {
                // true if the word held `expected` and now holds `new`
                let (expected, new) = io_locals(ctx)?;
                compile_expr(&args[0], ctx, func)?;
                emit_write_address_check(func, ctx);
                compile_expr(&args[1], ctx, func)?;
                compile_expr(&args[2], ctx, func)?;
                func.instruction(&Instruction::LocalSet(new));
                func.instruction(&Instruction::LocalSet(expected));
                func.instruction(&Instruction::LocalGet(expected));
                func.instruction(&Instruction::LocalGet(new));
                func.instruction(&Instruction::I32AtomicRmwCmpxchg(M4));
                func.instruction(&Instruction::LocalGet(expected));
                func.instruction(&Instruction::I32Eq);
            }
            OpCode::AtomicLock => {
                // Swap 0 -> 1; while the word is 1, sleep until notified. Any
                // other value is not a lock (the VM errors; this traps).
                let (lock, old) = io_locals(ctx)?;
                compile_expr(&args[0], ctx, func)?;
                emit_write_address_check(func, ctx);
                func.instruction(&Instruction::LocalSet(lock));
                func.instruction(&Instruction::Block(BlockType::Empty));
                func.instruction(&Instruction::Loop(BlockType::Empty));
                func.instruction(&Instruction::LocalGet(lock));
                func.instruction(&Instruction::I32Const(0));
                func.instruction(&Instruction::I32Const(1));
                func.instruction(&Instruction::I32AtomicRmwCmpxchg(M4));
                func.instruction(&Instruction::LocalTee(old));
                func.instruction(&Instruction::I32Eqz);
                func.instruction(&Instruction::BrIf(1));
                func.instruction(&Instruction::LocalGet(old));
                func.instruction(&Instruction::I32Const(1));
                func.instruction(&Instruction::I32Ne);
                func.instruction(&Instruction::If(BlockType::Empty));
                func.instruction(&Instruction::Unreachable);
                func.instruction(&Instruction::End);
                func.instruction(&Instruction::LocalGet(lock));
                func.instruction(&Instruction::I32Const(1));
                func.instruction(&Instruction::I64Const(-1));
                func.instruction(&Instruction::MemoryAtomicWait32(M4));
                func.instruction(&Instruction::Drop);
                func.instruction(&Instruction::Br(0));
                func.instruction(&Instruction::End);
                func.instruction(&Instruction::End);
            }
            OpCode::AtomicUnlock => {
                // Swap 1 -> 0 (anything else traps, as the VM errors), then
                // wake every waiter.
                let (lock, _) = io_locals(ctx)?;
                compile_expr(&args[0], ctx, func)?;
                emit_write_address_check(func, ctx);
                func.instruction(&Instruction::LocalTee(lock));
                func.instruction(&Instruction::I32Const(0));
                func.instruction(&Instruction::I32AtomicRmwXchg(M4));
                func.instruction(&Instruction::I32Const(1));
                func.instruction(&Instruction::I32Ne);
                func.instruction(&Instruction::If(BlockType::Empty));
                func.instruction(&Instruction::Unreachable);
                func.instruction(&Instruction::End);
                func.instruction(&Instruction::LocalGet(lock));
                func.instruction(&Instruction::I32Const(-1));
                func.instruction(&Instruction::MemoryAtomicNotify(M4));
                func.instruction(&Instruction::Drop);
            }
            OpCode::SysTime | OpCode::SysMonotonic => {
                // clock_time_get(realtime 0 | monotonic 1, precision 1 ns,
                // out = the 8-byte cell 80); a failing clock traps
                let host = wasi_index(ctx, Wasi::ClockTimeGet)?;
                func.instruction(&Instruction::I32Const(if *op == OpCode::SysTime { 0 } else { 1 }));
                func.instruction(&Instruction::I64Const(1));
                emit_rt(func, ctx, RT_NBYTES);
                func.instruction(&Instruction::Call(host));
                func.instruction(&Instruction::If(BlockType::Empty));
                func.instruction(&Instruction::Unreachable);
                func.instruction(&Instruction::End);
                emit_rt(func, ctx, RT_NBYTES);
                func.instruction(&Instruction::I64Load(MemArg { offset: 0, align: 3, memory_index: 0 }));
            }
            OpCode::SysRandom => {
                let host = wasi_index(ctx, Wasi::RandomGet)?;
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::Call(host));
                emit_errno_to_result(func, ctx, None);
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
                emit_path_dir(func, ctx.addr_scratch, io_a);
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
                emit_rt(func, ctx, RT_OPENED_FD);
                func.instruction(&Instruction::Call(path_open));
                emit_errno_to_result(func, ctx, Some(RT_OPENED_FD));
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
                emit_rt(func, ctx, RT_IOV0_BUF);
                func.instruction(&Instruction::LocalGet(io_a));
                func.instruction(&Instruction::I32Store(M4));
                emit_rt(func, ctx, RT_IOV0_LEN);
                func.instruction(&Instruction::LocalGet(io_b));
                func.instruction(&Instruction::I32Store(M4));
                func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
                emit_rt(func, ctx, RT_IOV0_BUF);
                func.instruction(&Instruction::I32Const(1));
                emit_rt(func, ctx, RT_NBYTES);
                func.instruction(&Instruction::Call(host));
                emit_errno_to_result(func, ctx, Some(RT_NBYTES));
            }
            OpCode::FsClose => {
                let fd_close = wasi_index(ctx, Wasi::FdClose)?;
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::Call(fd_close));
                emit_errno_to_result(func, ctx, None);
            }
            OpCode::ArgsSizes | OpCode::ArgsGet | OpCode::EnvSizes | OpCode::EnvGet => {
                // the two addresses go straight to the host call
                let host = wasi_index(ctx, Wasi::for_op(op).unwrap())?;
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::Call(host));
                emit_errno_to_result(func, ctx, None);
            }
            OpCode::FsOpendir => {
                // (fs.opendir ptr len) -> path_open with DIRECTORY and the
                // readdir right; the new fd, or -1. Mirrors vm.rs FsOpendir.
                let path_open = wasi_index(ctx, Wasi::PathOpen)?;
                let (io_a, _) = io_locals(ctx)?;
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::LocalSet(io_a)); // len
                func.instruction(&Instruction::LocalSet(ctx.addr_scratch)); // ptr
                emit_path_dir(func, ctx.addr_scratch, io_a);
                func.instruction(&Instruction::I32Const(0)); // dirflags
                func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
                func.instruction(&Instruction::LocalGet(io_a));
                func.instruction(&Instruction::I32Const(WASI_OFLAGS_DIRECTORY));
                func.instruction(&Instruction::I64Const(WASI_RIGHT_FD_READDIR));
                func.instruction(&Instruction::I64Const(0)); // rights_inheriting
                func.instruction(&Instruction::I32Const(0)); // fdflags
                emit_rt(func, ctx, RT_OPENED_FD);
                func.instruction(&Instruction::Call(path_open));
                emit_errno_to_result(func, ctx, Some(RT_OPENED_FD));
            }
            OpCode::FsReaddir => {
                // (fs.readdir fd buf len cookie) -> fd_readdir, its operands
                // in its order; the bytes used, or -1
                let readdir = wasi_index(ctx, Wasi::FdReaddir)?;
                for a in args {
                    compile_expr(a, ctx, func)?;
                }
                emit_rt(func, ctx, RT_NBYTES);
                func.instruction(&Instruction::Call(readdir));
                emit_errno_to_result(func, ctx, Some(RT_NBYTES));
            }
            OpCode::FsStat => {
                // (fs.stat ptr len out) -> path_filestat_get following
                // symlinks; 0, or -1
                let stat = wasi_index(ctx, Wasi::PathFilestatGet)?;
                let (io_a, io_b) = io_locals(ctx)?;
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                compile_expr(&args[2], ctx, func)?;
                func.instruction(&Instruction::LocalSet(io_b)); // out
                func.instruction(&Instruction::LocalSet(io_a)); // len
                func.instruction(&Instruction::LocalSet(ctx.addr_scratch)); // ptr
                emit_path_dir(func, ctx.addr_scratch, io_a);
                func.instruction(&Instruction::I32Const(WASI_LOOKUP_SYMLINK_FOLLOW));
                func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
                func.instruction(&Instruction::LocalGet(io_a));
                func.instruction(&Instruction::LocalGet(io_b));
                func.instruction(&Instruction::Call(stat));
                emit_errno_to_result(func, ctx, None);
            }
            OpCode::FsDelete => {
                let unlink = wasi_index(ctx, Wasi::PathUnlinkFile)?;
                let (io_a, _) = io_locals(ctx)?;
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::LocalSet(io_a)); // len
                func.instruction(&Instruction::LocalSet(ctx.addr_scratch)); // ptr
                emit_path_dir(func, ctx.addr_scratch, io_a);
                func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
                func.instruction(&Instruction::LocalGet(io_a));
                func.instruction(&Instruction::Call(unlink));
                emit_errno_to_result(func, ctx, None);
            }

            OpCode::ThreadSpawn => {
                // rec = 16 bytes [done=0 result=0 fn arg]; thread-spawn(rec);
                // a negative thread id traps (the VM errors). The handle is rec.
                let spawn = wasi_index(ctx, Wasi::ThreadSpawn)?;
                let (worker, arg) = io_locals(ctx)?;
                let at = |offset: u64| MemArg { offset, align: 2, memory_index: 0 };
                compile_expr(&args[0], ctx, func)?;
                compile_expr(&args[1], ctx, func)?;
                func.instruction(&Instruction::LocalSet(arg));
                func.instruction(&Instruction::LocalSet(worker));
                func.instruction(&Instruction::I32Const(0));
                func.instruction(&Instruction::I32Const(16));
                func.instruction(&Instruction::I32AtomicRmwAdd(M4));
                func.instruction(&Instruction::LocalSet(ctx.addr_scratch));
                emit_grow_to_cursor(func);
                for (offset, value) in [(0u64, None), (4, None), (8, Some(worker)), (12, Some(arg))] {
                    func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
                    match value {
                        Some(l) => func.instruction(&Instruction::LocalGet(l)),
                        None => func.instruction(&Instruction::I32Const(0)),
                    };
                    func.instruction(&Instruction::I32Store(at(offset)));
                }
                func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
                func.instruction(&Instruction::Call(spawn));
                func.instruction(&Instruction::I32Const(0));
                func.instruction(&Instruction::I32LtS);
                func.instruction(&Instruction::If(BlockType::Empty));
                func.instruction(&Instruction::Unreachable);
                func.instruction(&Instruction::End);
                func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
            }
            OpCode::ThreadJoin => {
                // sleep until the record's done flag is set, then read the result
                compile_expr(&args[0], ctx, func)?;
                func.instruction(&Instruction::LocalSet(ctx.addr_scratch));
                func.instruction(&Instruction::Block(BlockType::Empty));
                func.instruction(&Instruction::Loop(BlockType::Empty));
                func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
                func.instruction(&Instruction::I32AtomicLoad(M4));
                func.instruction(&Instruction::BrIf(1));
                func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
                func.instruction(&Instruction::I32Const(0));
                func.instruction(&Instruction::I64Const(-1));
                func.instruction(&Instruction::MemoryAtomicWait32(M4));
                func.instruction(&Instruction::Drop);
                func.instruction(&Instruction::Br(0));
                func.instruction(&Instruction::End);
                func.instruction(&Instruction::End);
                func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
                func.instruction(&Instruction::I32Load(MemArg { offset: 4, align: 2, memory_index: 0 }));
            }
        },
        Expr::Block(exprs, _) | Expr::Unsafe(exprs, _) => {
            let len = exprs.len();
            for (i, e) in exprs.iter().enumerate() {
                if i + 1 == len {
                    compile_expr(e, ctx, func)?;
                } else {
                    compile_stmt(e, ctx, func)?;
                }
            }
        }
        Expr::While { .. } | Expr::Loop { .. } if ctx.versioning.get() && !loop_invariant_pointers(expr, ctx).is_empty() => {
            compile_versioned_loop(expr, ctx, func)?;
        }
        Expr::While { cond, body, .. } => {
            // block { loop { cond; eqz; br_if 1; body; br 0 } }:
            // break = br to the block, continue = br to the loop header.
            func.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty));
            func.instruction(&Instruction::Loop(wasm_encoder::BlockType::Empty));
            with_label(ctx, Label::Break, || {
                with_label(ctx, Label::LoopTop, || {
                    compile_expr(cond, ctx, func)?;
                    func.instruction(&Instruction::I32Eqz);
                    func.instruction(&Instruction::BrIf(1));
                    for e in body {
                        compile_stmt(e, ctx, func)?;
                    }
                    Ok(())
                })
            })?;
            func.instruction(&Instruction::Br(0));
            func.instruction(&Instruction::End);
            func.instruction(&Instruction::End);
        }
        Expr::Loop { var, start, end, step, body, .. } => {
            let var_idx = *ctx
                .locals
                .get(var)
                .ok_or_else(|| format!("Wasm Codegen: loop variable '{}' has no local slot", var))?;
            let hidden = |suffix: &str| ctx.locals.get(&format!("{}{}", var, suffix)).copied().ok_or("Wasm Codegen: loop has no hidden locals");
            let (end_idx, step_idx) = (hidden("#end")?, hidden("#step")?);
            // start, end, and step are each evaluated once, in that order
            compile_expr(start, ctx, func)?;
            func.instruction(&Instruction::LocalSet(var_idx));
            compile_expr(end, ctx, func)?;
            func.instruction(&Instruction::LocalSet(end_idx));
            compile_expr(step, ctx, func)?;
            func.instruction(&Instruction::LocalSet(step_idx));
            // block { loop { var > end -> br_if 1; block { body } ; var += step; br 0 } }:
            // break = br to the outer block, continue = br to the end of the
            // inner block, which falls into the step.
            func.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty));
            func.instruction(&Instruction::Loop(wasm_encoder::BlockType::Empty));
            with_label(ctx, Label::Break, || {
                with_label(ctx, Label::LoopTop, || {
                    func.instruction(&Instruction::LocalGet(var_idx));
                    func.instruction(&Instruction::LocalGet(end_idx));
                    // Inclusive end bound: exit only once var exceeds end.
                    func.instruction(&Instruction::I32GtS);
                    func.instruction(&Instruction::BrIf(1));
                    func.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty));
                    with_label(ctx, Label::Continue, || {
                        for e in body {
                            compile_stmt(e, ctx, func)?;
                        }
                        Ok(())
                    })?;
                    func.instruction(&Instruction::End);
                    Ok(())
                })
            })?;
            func.instruction(&Instruction::LocalGet(var_idx));
            func.instruction(&Instruction::LocalGet(step_idx));
            func.instruction(&Instruction::I32Add);
            func.instruction(&Instruction::LocalSet(var_idx));
            func.instruction(&Instruction::Br(0));
            func.instruction(&Instruction::End);
            func.instruction(&Instruction::End);
        }
        // a result is held by value (AIPL_SPEC.md 4.F)
        Expr::Ok(inner, ..) | Expr::Err(inner, ..) => compile_value_result(expr, inner, ctx, func)?,
        Expr::MatchResult { .. } => {
            let t = expr_type(expr, ctx);
            if is_value_type(&t, ctx.structs) {
                let g = *ctx.groups.get(&(2 * (expr as *const Expr as usize))).ok_or("Wasm Codegen: a value match_result has no group")?;
                compile_match_result(expr, Some(g), ctx, func)?;
                emit_get_slots(func, g, width(&t, ctx.structs));
            } else {
                compile_match_result(expr, None, ctx, func)?;
            }
        }
        // a union is a value (AIPL_SPEC.md 4.J)
        Expr::Make { union_name, variant, args, .. } => compile_value_make(expr, union_name, variant, args, ctx, func)?,
        // a value struct: its fields' scalars, in order
        Expr::Record { args, .. } => {
            for a in args {
                compile_expr(a, ctx, func)?;
            }
        }
        Expr::With { struct_name, field_name, value, new_val, .. } => {
            let g = *ctx.groups.get(&(2 * (expr as *const Expr as usize))).ok_or("Wasm Codegen: a with has no group")?;
            let n = width(&expr_type(expr, ctx), ctx.structs);
            let (slot, w, _, _) = field_slots(struct_name, field_name, ctx.structs)?;
            compile_expr(value, ctx, func)?;
            emit_set_slots(func, g, n);
            compile_expr(new_val, ctx, func)?;
            emit_set_slots(func, g + slot, w);
            emit_get_slots(func, g, n);
        }
        Expr::Match { value, arms, else_body, .. } => {
            let t = expr_type(expr, ctx);
            if is_value_type(&t, ctx.structs) {
                let g = *ctx.groups.get(&(2 * (expr as *const Expr as usize))).ok_or("Wasm Codegen: a value match has no group")?;
                compile_match(value, arms, else_body, Some(g), ctx, func)?;
                emit_get_slots(func, g, width(&t, ctx.structs));
            } else {
                compile_match(value, arms, else_body, None, ctx, func)?;
            }
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
            if def.is_value {
                return compile_value_read(expr, ctx, func);
            }
            let (offset, field_ty) = crate::checker::get_field_offset(def, field_name, ctx.structs)?;
            if is_value_type(&field_ty, ctx.structs) {
                return compile_value_read(expr, ctx, func);
            }
            compile_pointer(ptr, ctx, func)?;
            match field_ty {
                Type::Narrow(n) => {
                    func.instruction(&narrow_load(n, offset as u64));
                }
                Type::I32 | Type::Bool | Type::Str | Type::Fn(..) | Type::Enum(_) | Type::Union(_) => {
                    func.instruction(&Instruction::I32Load(wasm_encoder::MemArg {
                        offset: offset as u64,
                        align: 2,
                        memory_index: 0,
                    }));
                    if field_ty == Type::Bool {
                        normalize_bool(func);
                    }
                }
                Type::I64 | Type::Ptr(_) | Type::Array(_) => {
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
                _ => return Err(format!("Unsupported field type for struct get: {}", field_ty)),
            }
        }
        Expr::PutField {
            struct_name,
            field_name,
            ptr,
            val,
            path,
            ..
        } => {
            let def = ctx
                .structs
                .get(struct_name)
                .ok_or_else(|| format!("Wasm Codegen: Unknown struct '{}'", struct_name))?;
            let (offset, field_ty) = crate::checker::get_field_offset(def, field_name, ctx.structs)?;
            // a path: one field of the value held inline, further on
            let (mut offset, mut field_ty) = (offset, field_ty);
            for (st, f) in path {
                let (_, _, o, ft) = field_slots(st, f, ctx.structs)?;
                offset += o as usize;
                field_ty = ft;
            }
            compile_pointer(ptr, ctx, func)?;
            emit_write_address_check(func, ctx);
            if is_value_type(&field_ty, ctx.structs) {
                return emit_store_value(func, ctx, &field_ty, offset as u32, val);
            }
            compile_expr(val, ctx, func)?;
            match field_ty {
                Type::Narrow(n) => {
                    func.instruction(&narrow_store(n, offset as u64));
                }
                Type::I32 | Type::Bool | Type::Str | Type::Fn(..) | Type::Enum(_) | Type::Union(_) => {
                    func.instruction(&Instruction::I32Store(wasm_encoder::MemArg {
                        offset: offset as u64,
                        align: 2,
                        memory_index: 0,
                    }));
                }
                Type::I64 | Type::Ptr(_) | Type::Array(_) => {
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
                _ => return Err(format!("Unsupported field type for struct put: {}", field_ty)),
            }
        }
        Expr::Sizeof { ty, .. } => {
            let size = match ty {
                Type::Struct(name) if ctx.enums.contains_key(name) => 4,
                Type::Struct(struct_name) => {
                    let def = ctx
                        .structs
                        .get(struct_name)
                        .ok_or_else(|| format!("Wasm Codegen: Unknown struct '{}'", struct_name))?;
                    crate::checker::get_struct_size(def, ctx.structs)?
                }
                other => crate::checker::type_size_and_align(other, ctx.structs)?.0,
            };
            func.instruction(&Instruction::I32Const(size as i32));
        }
        Expr::Return { val, .. } => {
            if let Some(v) = val {
                compile_expr(v, ctx, func)?;
                // with ens, a value struct waits in res (the body's block yields nothing)
                let t = expr_type(v, ctx);
                if ctx.ens_block && is_value_type(&t, ctx.structs) {
                    let res = *ctx.locals.get("res").ok_or("Wasm Codegen: no res")?;
                    emit_set_slots(func, res, width(&t, ctx.structs));
                }
            }
            if ctx.ens_block {
                // to the end of the function's body block, where ens is checked
                func.instruction(&Instruction::Br(ctx.labels.borrow().len() as u32 - 1));
            } else {
                func.instruction(&Instruction::Return);
            }
        }
        Expr::Break(_) => {
            func.instruction(&Instruction::Br(label_depth(ctx, &[Label::Break])?));
        }
        Expr::Continue(_) => {
            func.instruction(&Instruction::Br(label_depth(ctx, &[Label::LoopTop, Label::Continue])?));
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
            let key = wasm_signature(sig, ctx.structs);
            let pos = ctx.ref_sigs.iter().position(|k| *k == key).ok_or("Wasm Codegen: call_ref signature not collected")?;
            func.instruction(&Instruction::CallIndirect { type_index: ctx.ref_type_base + pos as u32, table_index: 0 });
        }
        // Pointers and arrays are an address and a generation in one i64.
        Expr::Null { .. } => {
            func.instruction(&Instruction::I64Const(0));
        }
        Expr::Cast { ty: Type::Ptr(_) | Type::Array(_), addr, gen, .. } => {
            use Instruction::*;
            match gen {
                // ptr.make / arr.make: addr | gen << 32
                Some(g) => {
                    compile_expr(addr, ctx, func)?;
                    func.instruction(&I64ExtendI32U);
                    compile_expr(g, ctx, func)?;
                    func.instruction(&I64ExtendI32U);
                    func.instruction(&I64Const(32));
                    func.instruction(&I64Shl);
                    func.instruction(&I64Or);
                }
                // ptr.cast / arr.cast: unchecked (GEN_UNCHECKED), and 0 is null
                None => {
                    compile_expr(addr, ctx, func)?;
                    func.instruction(&LocalTee(ctx.addr_scratch));
                    func.instruction(&I32Eqz);
                    func.instruction(&If(BlockType::Result(ValType::I64)));
                    func.instruction(&I64Const(0));
                    func.instruction(&Else);
                    func.instruction(&LocalGet(ctx.addr_scratch));
                    func.instruction(&I64ExtendI32U);
                    func.instruction(&I64Const(((GEN_UNCHECKED as u64) << 32) as i64));
                    func.instruction(&I64Or);
                    func.instruction(&End);
                }
            }
        }
        Expr::Cast { addr: inner, .. } => {
            compile_expr(inner, ctx, func)?;
        }
        Expr::Addr { val, kind, .. } => {
            compile_expr(val, ctx, func)?;
            match kind {
                AddrKind::Ptr | AddrKind::Arr => {
                    func.instruction(&Instruction::I32WrapI64);
                }
                AddrKind::PtrGen | AddrKind::ArrGen => {
                    func.instruction(&Instruction::I64Const(32));
                    func.instruction(&Instruction::I64ShrU);
                    func.instruction(&Instruction::I32WrapI64);
                }
                AddrKind::Enum => {}
            }
        }
        Expr::ArrLen { arr, .. } => {
            compile_pointer(arr, ctx, func)?;
            func.instruction(&Instruction::I32Const(4));
            func.instruction(&Instruction::I32Sub);
            func.instruction(&Instruction::I32Load(M4));
        }
        Expr::ArrGet { elem_ty, ptr, index, .. } => {
            emit_element_address(elem_ty, ptr, index, ctx, func)?;
            if is_value_type(elem_ty, ctx.structs) {
                func.instruction(&Instruction::LocalSet(ctx.addr_scratch));
                return emit_load_scalars(func, ctx, elem_ty, 0);
            }
            match elem_ty {
                Type::Narrow(n) => {
                    func.instruction(&narrow_load(*n, 0));
                }
                Type::I32 | Type::Bool | Type::Str | Type::Fn(..) | Type::Enum(_) | Type::Union(_) => {
                    func.instruction(&Instruction::I32Load(M4));
                    if *elem_ty == Type::Bool {
                        normalize_bool(func);
                    }
                }
                Type::I64 | Type::Ptr(_) | Type::Array(_) => {
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
                _ => return Err(format!("Unsupported elem type for arr.get: {}", elem_ty)),
            }
        }
        Expr::ArrSet { elem_ty, ptr, index, val, path, .. } => {
            emit_element_address(elem_ty, ptr, index, ctx, func)?;
            emit_write_address_check(func, ctx);
            // a path: one field of the element, further on
            let (mut offset, mut target) = (0u32, elem_ty.clone());
            for (st, f) in path {
                let (_, _, o, ft) = field_slots(st, f, ctx.structs)?;
                offset += o;
                target = ft;
            }
            if is_value_type(&target, ctx.structs) {
                return emit_store_value(func, ctx, &target, offset, val);
            }
            if !path.is_empty() {
                compile_expr(val, ctx, func)?;
                func.instruction(&store_instruction(&target, offset as u64)?);
                return Ok(());
            }
            compile_expr(val, ctx, func)?;
            match elem_ty {
                Type::Narrow(n) => {
                    func.instruction(&narrow_store(*n, 0));
                }
                Type::I32 | Type::Bool | Type::Str | Type::Fn(..) | Type::Enum(_) | Type::Union(_) => {
                    func.instruction(&Instruction::I32Store(M4));
                }
                Type::I64 | Type::Ptr(_) | Type::Array(_) => {
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
                _ => return Err(format!("Unsupported elem type for arr.set: {}", elem_ty)),
            }
        }
    }
    Ok(())
}

/// A `bool` read from memory is true iff its word is nonzero, as in the VM.
/// Normalising to 0/1 keeps `and`/`or` (bitwise in wasm) and `eq` correct
/// when the word was written by something other than `put`/`arr.set`.
fn normalize_bool(func: &mut Body) {
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Ne);
}

/// After the heap cursor (address 0) has been bumped: if it is past the end of
/// memory, grow memory by the pages needed to cover it. Uses no locals and has
/// no net stack effect. memory.grow fails (-1, dropped) past the 32768-page cap,
/// and the first access beyond the end then traps. The VM's alloc_bytes does
/// the same.
fn emit_grow_to_cursor(func: &mut Body) {
    use Instruction::*;
    let size_bytes = |func: &mut Body| {
        func.instruction(&MemorySize(0));
        func.instruction(&I32Const(16));
        func.instruction(&I32Shl);
    };
    func.instruction(&I32Const(0));
    func.instruction(&I32Load(M4));
    size_bytes(func);
    func.instruction(&I32GtU);
    func.instruction(&If(wasm_encoder::BlockType::Empty));
    func.instruction(&I32Const(0));
    func.instruction(&I32Load(M4));
    size_bytes(func);
    func.instruction(&I32Sub);
    func.instruction(&I32Const(65535));
    func.instruction(&I32Add);
    func.instruction(&I32Const(16));
    func.instruction(&I32ShrU);
    func.instruction(&MemoryGrow(0));
    func.instruction(&Drop);
    func.instruction(&End);
}

/// The ok and err payload types of a result-typed expression (i32 for
/// anything else, which the checker has ruled out).
fn result_sides(ty: Type) -> (Type, Type) {
    match ty {
        Type::ResultType(a, b) => (*a, *b),
        _ => (Type::I32, Type::I32),
    }
}

/// Gives each match_result binder its payload's type, which collect_lets
/// cannot know: the matched expression's type, worked out in source order
/// so a binder can be matched again inside its arm. A binder named before
/// by a let keeps its type, which the checker makes the same.
fn binder_types(
    body: &[Expr],
    local_types: &mut HashMap<String, Type>,
    fn_returns: &HashMap<String, Type>,
    fn_types: &HashMap<String, Type>,
    structs: &crate::checker::Structs,
) {
    let mut found = Vec::new();
    for e in body {
        walk_expr(e, &mut |x| {
            if let Expr::MatchResult { expr, ok_var, err_var, .. } = x {
                found.push((expr.as_ref().clone(), ok_var.clone(), err_var.clone()));
            }
        });
    }
    let mut set = std::collections::HashSet::new();
    for (scrutinee, ok_var, err_var) in found {
        let ty = type_in(&scrutinee, &Typing { local_types, fn_returns, fn_types, structs });
        let (ok_ty, err_ty) = result_sides(ty);
        for (name, t) in [(ok_var, ok_ty), (err_var, err_ty)] {
            if set.insert(name.clone()) {
                local_types.insert(name, t);
            }
        }
    }
}

/// The load of a memory value of type `ty` at `offset` from the address on the stack.
/// A storage type's load at `offset` (stack: address -> i32), zero- or
/// sign-extended.
fn narrow_load(n: Narrow, offset: u64) -> Instruction<'static> {
    let m = |align| wasm_encoder::MemArg { offset, align, memory_index: 0 };
    match n {
        Narrow::U8 => Instruction::I32Load8U(m(0)),
        Narrow::I8 => Instruction::I32Load8S(m(0)),
        Narrow::U16 => Instruction::I32Load16U(m(1)),
        Narrow::I16 => Instruction::I32Load16S(m(1)),
    }
}

/// A storage type's store at `offset` (stack: address, i32): its low bits.
fn narrow_store(n: Narrow, offset: u64) -> Instruction<'static> {
    let m = |align| wasm_encoder::MemArg { offset, align, memory_index: 0 };
    match n {
        Narrow::U8 | Narrow::I8 => Instruction::I32Store8(m(0)),
        Narrow::U16 | Narrow::I16 => Instruction::I32Store16(m(1)),
    }
}

fn load_instruction(ty: &Type, offset: u64) -> Result<Instruction<'static>, String> {
    if let Type::Narrow(n) = ty {
        return Ok(narrow_load(*n, offset));
    }
    let m = |align| wasm_encoder::MemArg { offset, align, memory_index: 0 };
    Ok(match aipl_to_wasm_type(ty) {
        _ if matches!(ty, Type::Void | Type::Struct(_) | Type::ResultType(..)) => return Err(format!("no memory layout for {}", ty)),
        ValType::I32 => Instruction::I32Load(m(2)),
        ValType::I64 => Instruction::I64Load(m(3)),
        ValType::F32 => Instruction::F32Load(m(2)),
        ValType::F64 => Instruction::F64Load(m(3)),
        other => return Err(format!("no memory layout for {:?}", other)),
    })
}

/// The store of a value of type `ty` at `offset` from an address (stack: address, value).
fn store_instruction(ty: &Type, offset: u64) -> Result<Instruction<'static>, String> {
    if let Type::Narrow(n) = ty {
        return Ok(narrow_store(*n, offset));
    }
    let m = |align| wasm_encoder::MemArg { offset, align, memory_index: 0 };
    Ok(match aipl_to_wasm_type(ty) {
        _ if matches!(ty, Type::Void | Type::Struct(_) | Type::ResultType(..)) => return Err(format!("no memory layout for {}", ty)),
        ValType::I32 => Instruction::I32Store(m(2)),
        ValType::I64 => Instruction::I64Store(m(3)),
        ValType::F32 => Instruction::F32Store(m(2)),
        ValType::F64 => Instruction::F64Store(m(3)),
        other => return Err(format!("no memory layout for {:?}", other)),
    })
}

/// `(match_result r (ok v ...) (err e ...))`: the result's tag picks the
/// arm; its binder is its side's slots. With `dest`, a value struct result
/// goes into the locals from there (compile_into) and the if yields nothing.
fn compile_match_result(this: &Expr, dest: Option<u32>, ctx: &Ctx, func: &mut Body) -> Result<(), String> {
    let Expr::MatchResult { expr, ok_var, ok_body, err_var, err_body, .. } = this else {
        return Err("Wasm Codegen: not a match_result".to_string());
    };
    let rt = expr_type(expr, ctx);
    let (ok_ty, err_ty) = result_sides(rt.clone());
    // a result is held by value: its tag and binders from its slots (a
    // variable's own locals, else its spill group)
    let l = ctx.structs.sum_of(&rt).ok_or("Wasm Codegen: a result wider than its checker allows")?;
    let base = match expr.as_ref() {
        Expr::Var(name, _) => *ctx.locals.get(name).ok_or_else(|| format!("Wasm Codegen: Unbound local variable '{}'", name))?,
        _ => {
            let g = *ctx.groups.get(&(2 * (expr.as_ref() as *const Expr as usize) + 1)).ok_or("Wasm Codegen: a value match_result has no spill group")?;
            compile_expr(expr, ctx, func)?;
            emit_set_slots(func, g, width(&rt, ctx.structs));
            g
        }
    };
    func.instruction(&Instruction::LocalGet(base));
    func.instruction(&Instruction::I32Eqz);
    let load_binder = |func: &mut Body, side: usize, ty: &Type, idx: u32| -> Result<(), String> {
        for slot in &l.variants[side] {
            func.instruction(&Instruction::LocalGet(base + slot));
        }
        emit_set_slots(func, idx, width(ty, ctx.structs));
        Ok(())
    };

    // the arm that does not always leave gives the block's type
    let last_ok = if crate::checker::body_diverges(ok_body) { err_body.last() } else { ok_body.last() };
    let block_ty = if dest.is_some() || last_ok.is_none_or(|e| is_void_expr(e, ctx)) {
        wasm_encoder::BlockType::Empty
    } else {
        let ty = expr_type(last_ok.unwrap(), ctx);
        wasm_encoder::BlockType::Result(aipl_to_wasm_type(&ty))
    };

    func.instruction(&Instruction::If(block_ty));
    ctx.labels.borrow_mut().push(Label::Plain);

    if let Some(&ok_idx) = ctx.locals.get(ok_var) {
        load_binder(func, 0, &ok_ty, ok_idx)?;
    }
    compile_arm(ok_body, block_ty, dest, ctx, func)?;

    func.instruction(&Instruction::Else);

    if let Some(&err_idx) = ctx.locals.get(err_var) {
        load_binder(func, 1, &err_ty, err_idx)?;
    }
    compile_arm(err_body, block_ty, dest, ctx, func)?;

    ctx.labels.borrow_mut().pop();
    func.instruction(&Instruction::End);
    Ok(())
}

/// An arm's body: its statements; its last expression left as the block's
/// value, or into `dest` (compile_into), or dropped for an empty block type.
/// After a branch whose value a block wants: `unreachable` if the branch
/// always leaves without a jump of its own (checker::ends_in_jump).
fn seal(branch: &Expr, valued: bool, func: &mut Body) {
    if valued && crate::checker::diverges(branch) && !crate::checker::ends_in_jump(branch) {
        func.instruction(&Instruction::Unreachable);
    }
}

fn compile_arm(body: &[Expr], block_ty: BlockType, dest: Option<u32>, ctx: &Ctx, func: &mut Body) -> Result<(), String> {
    for (i, stmt) in body.iter().enumerate() {
        if i + 1 == body.len() && dest.is_some() {
            compile_into(stmt, dest.unwrap_or(0), ctx, func)?;
        } else if i + 1 == body.len() && !matches!(block_ty, BlockType::Empty) {
            compile_expr(stmt, ctx, func)?;
            seal(stmt, true, func);
        } else {
            compile_stmt(stmt, ctx, func)?;
        }
    }
    Ok(())
}

/// `(make U.v args...)`: claim the variant's cell from the heap cursor before
/// evaluating the fields (the VM does the same), then store the tag and each
/// field. The address is pushed n+2 times at once (the result, the tag store,
/// one per field), so fields that use the scratch local cannot clobber it.
/// The cell is fresh heap memory, so the stores need no write-address check.
/// `(make U.v args...)`, a union (a value): into its group, the tag,
/// each field's scalars in their slots (checker::SumLayout), zero in the
/// slots v does not use; then the group pushed.
fn compile_value_make(this: &Expr, union_name: &str, variant: &str, args: &[Expr], ctx: &Ctx, func: &mut Body) -> Result<(), String> {
    let g = *ctx.groups.get(&(2 * (this as *const Expr as usize))).ok_or("Wasm Codegen: a value make has no group")?;
    let def = ctx.unions.get(union_name).ok_or_else(|| format!("Wasm Codegen: Unknown union '{}'", union_name))?;
    let tag = def.variants.iter().position(|v| v.name == variant).ok_or_else(|| format!("Wasm Codegen: Unknown variant '{}'", variant))?;
    let layout = &ctx.structs.by_value[union_name];
    let map = &layout.variants[tag];
    func.instruction(&Instruction::I32Const(tag as i32));
    func.instruction(&Instruction::LocalSet(g));
    let mut at = 0;
    for (a, f) in args.iter().zip(&def.variants[tag].fields) {
        compile_expr(a, ctx, func)?;
        let w = width(&f.ty, ctx.structs) as usize;
        for k in (at..at + w).rev() {
            func.instruction(&Instruction::LocalSet(g + map[k]));
        }
        at += w;
    }
    for (slot, t) in layout.slots.iter().enumerate().skip(1) {
        if !map.contains(&(slot as u32)) {
            emit_zero(func, t);
            func.instruction(&Instruction::LocalSet(g + slot as u32));
        }
    }
    emit_get_slots(func, g, layout.slots.len() as u32);
    Ok(())
}

/// `(ok:E v)` / `(err:T e)` of a result held by value: into its group, the
/// tag, the payload's scalars in their slots, zero in the others; then the
/// group pushed (as compile_value_make).
fn compile_value_result(this: &Expr, inner: &Expr, ctx: &Ctx, func: &mut Body) -> Result<(), String> {
    let g = *ctx.groups.get(&(2 * (this as *const Expr as usize))).ok_or("Wasm Codegen: a value result has no group")?;
    let tag = if matches!(this, Expr::Ok(..)) { 0 } else { 1 };
    let layout = ctx.structs.sum_of(&expr_type(this, ctx)).ok_or("Wasm Codegen: not a result by value")?;
    let map = &layout.variants[tag];
    func.instruction(&Instruction::I32Const(tag as i32));
    func.instruction(&Instruction::LocalSet(g));
    compile_expr(inner, ctx, func)?;
    for slot in map.iter().rev() {
        func.instruction(&Instruction::LocalSet(g + slot));
    }
    for (slot, t) in layout.slots.iter().enumerate().skip(1) {
        if !map.contains(&(slot as u32)) {
            emit_zero(func, t);
            func.instruction(&Instruction::LocalSet(g + slot as u32));
        }
    }
    emit_get_slots(func, g, layout.slots.len() as u32);
    Ok(())
}

/// A zero of scalar type t.
fn emit_zero(func: &mut Body, t: &Type) {
    func.instruction(&match t {
        Type::I64 => Instruction::I64Const(0),
        Type::F32 => Instruction::F32Const(0.0f32),
        Type::F64 => Instruction::F64Const(0.0f64),
        _ => Instruction::I32Const(0),
    });
}

/// The arm body whose last expression gives a match its type: the first
/// arm's, or the else body's when there are no arms (the checker has made
/// every arm agree).
/// The body whose type is the match's: the first arm's (or the else's)
/// that does not always leave (checker::diverges), else the first.
fn match_type_body<'e>(arms: &'e [MatchArm], else_body: &'e Option<Vec<Expr>>) -> &'e [Expr] {
    let mut bodies = arms.iter().map(|a| a.body.as_slice()).chain(else_body.iter().map(|b| b.as_slice()));
    let first = bodies.clone().next().unwrap_or(&[]);
    bodies.find(|b| !crate::checker::body_diverges(b)).unwrap_or(first)
}

/// `(match v arms... [(else ...)])`: an if/else chain tests each arm in
/// order, before any arm body runs: a union's tag in its first slot (its
/// binders from the variant's slots), or an enum's value, from the scratch
/// local. The chain ends in the else body, or in `unreachable` (only an
/// enum.cast value that is no member gets there).
fn compile_match(value: &Expr, arms: &[MatchArm], else_body: &Option<Vec<Expr>>, dest: Option<u32>, ctx: &Ctx, func: &mut Body) -> Result<(), String> {
    let body_of = match_type_body(arms, else_body);
    let block_ty = match body_of.last() {
        Some(e) if dest.is_none() && !is_void_expr(e, ctx) => wasm_encoder::BlockType::Result(aipl_to_wasm_type(&expr_type(e, ctx))),
        _ => wasm_encoder::BlockType::Empty,
    };
    let compile_body = |body: &[Expr], func: &mut Body| -> Result<(), String> { compile_arm(body, block_ty, dest, ctx, func) };
    // a union by value: the tag and the binders from its slots (a
    // variable's own locals, else its spill group)
    let vt = expr_type(value, ctx);
    if let Type::Union(un) = &vt {
        let base = match value {
            Expr::Var(name, _) => *ctx.locals.get(name).ok_or_else(|| format!("Wasm Codegen: Unbound local variable '{}'", name))?,
            _ => {
                let g = *ctx.groups.get(&(2 * (value as *const Expr as usize) + 1)).ok_or("Wasm Codegen: a value match has no spill group")?;
                compile_expr(value, ctx, func)?;
                emit_set_slots(func, g, width(&vt, ctx.structs));
                g
            }
        };
        let def = ctx.unions.get(un).ok_or_else(|| format!("Wasm Codegen: Unknown union '{}'", un))?;
        let layout = &ctx.structs.by_value[un];
        for arm in arms {
            let member = &arm.member[arm.member.rfind('.').map_or(0, |d| d + 1)..];
            let tag = def.variants.iter().position(|v| v.name == member).ok_or_else(|| format!("Wasm Codegen: Unknown variant '{}'", arm.member))?;
            func.instruction(&Instruction::LocalGet(base));
            func.instruction(&Instruction::I32Const(tag as i32));
            func.instruction(&Instruction::I32Eq);
            func.instruction(&Instruction::If(block_ty));
            ctx.labels.borrow_mut().push(Label::Plain);
            let map = &layout.variants[tag];
            let mut at = 0;
            for (k, f) in def.variants[tag].fields.iter().enumerate() {
                let w = width(&f.ty, ctx.structs) as usize;
                if let Some(n) = arm.binders.as_ref().and_then(|b| b.get(k)).filter(|n| n.as_str() != "_") {
                    let idx = *ctx.locals.get(n).ok_or_else(|| format!("Wasm Codegen: Unbound binder '{}'", n))?;
                    for slot in &map[at..at + w] {
                        func.instruction(&Instruction::LocalGet(base + slot));
                    }
                    emit_set_slots(func, idx, w as u32);
                }
                at += w;
            }
            compile_body(&arm.body, func)?;
            func.instruction(&Instruction::Else);
        }
        match else_body {
            Some(body) => compile_body(body, func)?,
            None => {
                func.instruction(&Instruction::Unreachable);
            }
        }
        for _ in arms {
            ctx.labels.borrow_mut().pop();
            func.instruction(&Instruction::End);
        }
        return Ok(());
    }
    // an enum: its value, compared with each arm's member
    compile_expr(value, ctx, func)?;
    func.instruction(&Instruction::LocalSet(ctx.addr_scratch));
    for arm in arms {
        let dot = arm.member.rfind('.').unwrap_or(0);
        let (tname, member) = (&arm.member[..dot], &arm.member[dot + 1..]);
        func.instruction(&Instruction::LocalGet(ctx.addr_scratch));
        let e = ctx.enums.get(tname).ok_or_else(|| format!("Wasm Codegen: Unknown enum '{}'", tname))?;
        let value = e.members.iter().find(|(m, _)| m == member).ok_or_else(|| format!("Wasm Codegen: Unknown member '{}'", arm.member))?.1;
        func.instruction(&Instruction::I32Const(value));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(block_ty));
        ctx.labels.borrow_mut().push(Label::Plain);
        compile_body(&arm.body, func)?;
        func.instruction(&Instruction::Else);
    }
    match else_body {
        Some(body) => compile_body(body, func)?,
        None => {
            func.instruction(&Instruction::Unreachable);
        }
    }
    for _ in arms {
        ctx.labels.borrow_mut().pop();
        func.instruction(&Instruction::End);
    }
    Ok(())
}

fn is_void_expr(expr: &Expr, ctx: &Ctx) -> bool {
    match expr {
        Expr::Set { .. } | Expr::Let { .. } | Expr::PutField { .. } | Expr::ArrSet { .. } => true,
        Expr::Block(exprs, _) | Expr::Unsafe(exprs, _) => exprs.last().is_none_or(|e| is_void_expr(e, ctx)),
        // An if/else is void only if BOTH branches are void - if they disagreed,
        // whichever branch actually produced a value would leave the wasm value
        // stack unbalanced relative to this if's declared block type.
        Expr::If { then_branch, else_branch, .. } => {
            is_void_expr(then_branch, ctx) && is_void_expr(else_branch, ctx)
        }
        Expr::While { .. } | Expr::Loop { .. } => true,
        Expr::Call { func, .. } => ctx.fn_returns.get(func).is_some_and(|t| *t == Type::Void),
        // the op's own type (expr_type's exhaustive table): a new op cannot
        // be left out, as checked.* and sys.time once were
        Expr::Op { .. } => expr_type(expr, ctx) == Type::Void,
        // Same rule as the block type compile_expr gives a match_result.
        Expr::MatchResult { ok_body, err_body, .. } => {
            let body = if crate::checker::body_diverges(ok_body) { err_body } else { ok_body };
            body.last().is_none_or(|e| is_void_expr(e, ctx))
        }
        // Same rule as the block type compile_match gives a match.
        Expr::Match { arms, else_body, .. } => match_type_body(arms, else_body).last().is_none_or(|e| is_void_expr(e, ctx)),
        Expr::Make { .. } | Expr::Record { .. } | Expr::With { .. } => false,
        Expr::Lit(..)
        | Expr::Var(..)
        | Expr::Ok(..)
        | Expr::Err(..)
        | Expr::GetField { .. }
        | Expr::Sizeof { .. }
        | Expr::ArrGet { .. }
        | Expr::ArrLen { .. }
        | Expr::Null { .. }
        | Expr::Cast { .. }
        | Expr::Addr { .. }
        | Expr::Ref { .. } => false,
        Expr::Return { .. } | Expr::Break(_) | Expr::Continue(_) => true,
        Expr::CallRef { sig, .. } => matches!(sig, Type::Fn(_, ret, _) if **ret == Type::Void),
    }
}

/// Emits the runtime memory-layout check for a store whose address is on top
/// of the stack: traps (`unreachable`) if the address is in bytes 0-3 (the
/// heap cursor) or 64..heap_start (the reserved runtime block and the string
/// literals). Mirrors `vm::check_write_address`, whose reserved range is
/// 64-1023 because the VM copies literals onto the heap instead. The address
/// stays on the stack for the store that follows.
///
///   [addr] local.tee s
///   local.get s ; i32.const 4  ; i32.lt_u              -> addr < 4
///   local.get s ; i32.const 64 ; i32.sub ; i32.const (heap_start - 64) ; i32.lt_u
///   i32.or ; if unreachable end
fn emit_write_address_check(func: &mut Body, ctx: &Ctx) {
    // two checks, each its own `if`: a compiler that branches on a single
    // comparison (the native backend) need not combine two into a value
    let scratch = ctx.addr_scratch;
    func.instruction(&Instruction::LocalTee(scratch));
    func.instruction(&Instruction::LocalGet(scratch));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(wasm_encoder::BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(scratch));
    func.instruction(&Instruction::I32Const(64));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32Const(ctx.heap_start as i32 - 64));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(wasm_encoder::BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);
}

/// The bounds check of `arr.get`/`arr.set`: with [array index] on the stack,
/// fails through $aipl_oob unless index < length (unsigned, so a negative
/// index fails too); the length is the word before the array. Leaves
/// [array index] on the stack.
///
///   local.set I ; local.tee A ; local.get I
///   local.get A ; i32.const 4 ; i32.sub ; i32.load
///   i32.ge_u ; if ; local.get I ; local.get A ; call $aipl_oob ; unreachable ; end
///   local.get I
///
/// $aipl_oob never returns; the `unreachable` after it says so, so wasmtime
/// keeps no values alive across the call (with the call alone, the checks
/// cost nbody twice as much).
/// With a pointer or array (an i64) on the stack: if it is null, the
/// failure message ($aipl_null) and a trap; if its generation is not the
/// word 8 bytes before the object, "Use after free" ($aipl_uaf) and a trap,
/// unless it is unchecked (ptr.cast). Leaves the i32 address.
fn emit_null_check(func: &mut Body, ctx: &Ctx) {
    use Instruction::*;
    func.instruction(&LocalTee(ctx.fat_scratch));
    func.instruction(&I32WrapI64);
    func.instruction(&LocalTee(ctx.addr_scratch));
    func.instruction(&I32Eqz);
    func.instruction(&If(BlockType::Empty));
    func.instruction(&Call(ctx.checks.null));
    func.instruction(&Unreachable);
    func.instruction(&End);
    let pointer_gen = |func: &mut Body| {
        func.instruction(&LocalGet(ctx.fat_scratch));
        func.instruction(&I64Const(32));
        func.instruction(&I64ShrU);
        func.instruction(&I32WrapI64);
    };
    func.instruction(&LocalGet(ctx.addr_scratch));
    func.instruction(&I32Const(8));
    func.instruction(&I32Sub);
    func.instruction(&I32Load(M4));
    pointer_gen(func);
    func.instruction(&I32Ne);
    func.instruction(&If(BlockType::Empty));
    pointer_gen(func);
    func.instruction(&I32Const(GEN_UNCHECKED as i32));
    func.instruction(&I32Ne);
    func.instruction(&If(BlockType::Empty));
    func.instruction(&Call(ctx.checks.uaf));
    func.instruction(&Unreachable);
    func.instruction(&End);
    func.instruction(&End);
    func.instruction(&LocalGet(ctx.addr_scratch));
}

/// The generation of a pointer made by ptr.cast: never compared.
const GEN_UNCHECKED: u32 = 0xFFFF_FFFF;

/// Compiles a pointer or array operand and checks it (emit_null_check),
/// leaving its address, unless it is a variable an enclosing versioned
/// loop has checked already.
fn compile_pointer(ptr: &Expr, ctx: &Ctx, func: &mut Body) -> Result<(), String> {
    compile_expr(ptr, ctx, func)?;
    if let Expr::Var(n, _) = ptr {
        if ctx.proven.borrow().iter().any(|p| p == n) {
            func.instruction(&Instruction::I32WrapI64);
            func.instruction(&Instruction::LocalTee(ctx.addr_scratch));
            return Ok(());
        }
    }
    emit_null_check(func, ctx);
    Ok(())
}

/// An unchecked operation that can change memory other than through a
/// checked access (a raw load cannot, so it frees nothing).
fn may_write_memory(op: &OpCode) -> bool {
    crate::checker::is_unchecked_op(op)
        && !matches!(op, OpCode::MemLoad8 | OpCode::MemLoad32 | OpCode::MemLoad64 | OpCode::MemLoad8S | OpCode::MemLoad16U | OpCode::MemLoad16S)
}

/// The functions that may free memory, or change it other than through
/// checked accesses: those that use an unchecked operation (raw memory,
/// atomics, threads, WASI), call through a function reference, or call a
/// function that may (or one not in the module). Raw loads do not count.
fn functions_that_may_free(module: &Module) -> std::collections::HashSet<String> {
    use std::collections::HashSet;
    let mut may: HashSet<String> = HashSet::new();
    let mut callees: HashMap<String, Vec<String>> = HashMap::new();
    for f in &module.functions {
        let (mut direct, mut calls) = (false, Vec::new());
        let mut visit = |e: &Expr| match e {
            Expr::Op { op, .. } if may_write_memory(op) => direct = true,
            Expr::CallRef { .. } => direct = true,
            Expr::Call { func, .. } => calls.push(func.clone()),
            _ => {}
        };
        for e in &f.body {
            walk_expr(e, &mut visit);
        }
        for c in &f.contracts {
            let (Contract::Requires(e) | Contract::Ensures(e) | Contract::Invariant(e)) = c;
            walk_expr(e, &mut visit);
        }
        if direct {
            may.insert(f.name.clone());
        }
        callees.insert(f.name.clone(), calls);
    }
    loop {
        let more: Vec<String> = callees
            .iter()
            .filter(|(f, cs)| !may.contains(*f) && cs.iter().any(|c| may.contains(c) || !callees.contains_key(c)))
            .map(|(f, _)| f.clone())
            .collect();
        if more.is_empty() {
            return may;
        }
        may.extend(more);
    }
}

/// The pointer and array variables a loop accesses directly (get, put,
/// arr.get, arr.set, arr.len on the variable) and never assigns, when
/// nothing in the loop can free memory: their checks hold for the whole
/// loop if they hold when it starts. Not those already checked.
fn loop_invariant_pointers(lp: &Expr, ctx: &Ctx) -> Vec<String> {
    let (mut barrier, mut used, mut assigned) = (false, Vec::new(), std::collections::HashSet::new());
    walk_expr(lp, &mut |e| match e {
        Expr::Op { op, .. } if may_write_memory(op) => barrier = true,
        Expr::CallRef { .. } => barrier = true,
        Expr::Call { func, .. } if ctx.may_free.contains(func) || !ctx.fn_indices.contains_key(func) => barrier = true,
        Expr::Let { name, .. } | Expr::Set { name, .. } => {
            assigned.insert(name.clone());
        }
        Expr::Loop { var, .. } => {
            assigned.insert(var.clone());
        }
        Expr::MatchResult { ok_var, err_var, .. } => {
            assigned.insert(ok_var.clone());
            assigned.insert(err_var.clone());
        }
        Expr::Match { arms, .. } => {
            for arm in arms {
                assigned.extend(arm.binders.iter().flatten().cloned());
            }
        }
        Expr::GetField { ptr, .. } | Expr::PutField { ptr, .. } | Expr::ArrGet { ptr, .. } | Expr::ArrSet { ptr, .. } | Expr::ArrLen { arr: ptr, .. } => {
            if let Expr::Var(n, _) = ptr.as_ref() {
                used.push(n.clone());
            }
        }
        _ => {}
    });
    if barrier {
        return Vec::new();
    }
    used.sort();
    used.dedup();
    let proven = ctx.proven.borrow();
    used.retain(|n| {
        !assigned.contains(n)
            && !proven.contains(n)
            && matches!(ctx.local_types.get(n), Some(Type::Ptr(_) | Type::Array(_)))
            && ctx.locals.contains_key(n)
    });
    used
}

/// A loop whose pointer and array variables nothing in it can invalidate
/// (loop_invariant_pointers), compiled twice: if each is non-null and live
/// (its generation matches, or it is unchecked) when the loop starts, a
/// copy whose accesses to them skip the null and generation checks; else
/// the loop as usual, whose checks then fail where they would have. The
/// slow copy's loops are not versioned again.
fn compile_versioned_loop(lp: &Expr, ctx: &Ctx, func: &mut Body) -> Result<(), String> {
    use Instruction::*;
    let names = loop_invariant_pointers(lp, ctx);
    for (k, n) in names.iter().enumerate() {
        let idx = *ctx.locals.get(n).ok_or_else(|| format!("Wasm Codegen: no local for '{}'", n))?;
        let pointer_gen = |func: &mut Body| {
            func.instruction(&LocalGet(ctx.fat_scratch));
            func.instruction(&I64Const(32));
            func.instruction(&I64ShrU);
            func.instruction(&I32WrapI64);
        };
        func.instruction(&LocalGet(idx));
        func.instruction(&LocalTee(ctx.fat_scratch));
        func.instruction(&I32WrapI64);
        func.instruction(&LocalTee(ctx.addr_scratch));
        func.instruction(&I32Eqz);
        func.instruction(&If(BlockType::Result(ValType::I32)));
        func.instruction(&I32Const(0));
        func.instruction(&Else);
        func.instruction(&LocalGet(ctx.addr_scratch));
        func.instruction(&I32Const(8));
        func.instruction(&I32Sub);
        func.instruction(&I32Load(M4));
        pointer_gen(func);
        func.instruction(&I32Eq);
        pointer_gen(func);
        func.instruction(&I32Const(GEN_UNCHECKED as i32));
        func.instruction(&I32Eq);
        func.instruction(&I32Or);
        func.instruction(&End);
        if k > 0 {
            func.instruction(&I32And);
        }
    }
    func.instruction(&If(BlockType::Empty));
    with_label(ctx, Label::Plain, || {
        let before = ctx.proven.borrow().len();
        ctx.proven.borrow_mut().extend(names.iter().cloned());
        let fast = compile_expr(lp, ctx, func);
        ctx.proven.borrow_mut().truncate(before);
        fast?;
        func.instruction(&Else);
        let was = ctx.versioning.replace(false);
        let slow = compile_expr(lp, ctx, func);
        ctx.versioning.set(was);
        slow
    })?;
    func.instruction(&End);
    Ok(())
}

fn emit_bounds_check(func: &mut Body, ctx: &Ctx) {
    use Instruction::*;
    let (a, i) = (ctx.addr_scratch, ctx.index_scratch);
    func.instruction(&LocalSet(i));
    func.instruction(&LocalTee(a));
    func.instruction(&LocalGet(i));
    func.instruction(&LocalGet(a));
    func.instruction(&I32Const(4));
    func.instruction(&I32Sub);
    func.instruction(&I32Load(M4));
    func.instruction(&I32GeU);
    func.instruction(&If(BlockType::Empty));
    func.instruction(&LocalGet(i));
    func.instruction(&LocalGet(a));
    func.instruction(&Call(ctx.checks.oob));
    func.instruction(&Unreachable);
    func.instruction(&End);
    func.instruction(&LocalGet(i));
}

/// Where a compiled check writes its failure message (inside the reserved
/// runtime block, so a failure needs no allocation), and the cells holding
/// the message's address and length for the host (AIPL_SPEC.md 7.9).
const RT_FAIL_TEXT: i32 = 128;
const RT_FAIL_ADDR: i32 = 92;
const RT_FAIL_LEN: i32 = 96;
const OOB_PREFIX: &[u8] = b"Array index out of bounds: index ";
const OOB_MIDDLE: &[u8] = b" for array of length ";
/// The VM's message for a null pointer or array (vm.rs NULL_POINTER).
const NULL_TEXT: &[u8] = b"Null pointer dereference";
/// A pointer whose object was freed (GENERATIONS_PLAN.md).
const UAF_TEXT: &[u8] = b"Use after free";

/// Writes `text` at the address `addr` pushes, eight bytes per i64.store
/// (the last chunk zero-padded, so it may write up to 7 bytes past the text).
fn emit_text_store(f: &mut Body, addr: &Instruction, text: &[u8]) {
    use Instruction::*;
    for (k, chunk) in text.chunks(8).enumerate() {
        let mut word = [0u8; 8];
        word[..chunk.len()].copy_from_slice(chunk);
        f.instruction(addr);
        f.instruction(&I64Const(i64::from_le_bytes(word)));
        f.instruction(&I64Store(MemArg { offset: 8 * k as u64, align: 0, memory_index: 0 }));
    }
}

/// The check helpers' function indices, in the order they are emitted.
pub struct CheckFns {
    dec: u32,
    oob: u32,
    text: u32,
    begin: u32,
    end: u32,
    null: u32,
    uaf: u32,
}

impl CheckFns {
    fn at(base: u32) -> CheckFns {
        CheckFns { dec: base, oob: base + 1, text: base + 2, begin: base + 3, end: base + 4, null: base + 5, uaf: base + 6 }
    }
}

/// The check helpers' types, in order (see `check_functions`).
fn check_signatures() -> Vec<(Vec<ValType>, Vec<ValType>)> {
    use ValType::{I32, I64};
    vec![
        (vec![I32, I64], vec![I32]),
        (vec![I32, I32], vec![]),
        (vec![I32, I32, I32], vec![I32]),
        (vec![I32, I32, I32], vec![I32]),
        (vec![I32], vec![]),
        (vec![], vec![]),
        (vec![], vec![]),
    ]
}

/// The check helpers, emitted once at the end of a module with checks:
///
/// - $aipl_dec(at, v:i64) -> end: writes v in signed decimal at `at`;
///   returns the address after it.
/// - $aipl_oob(index, array): writes the VM's bounds message ("Array index
///   out of bounds: index I for array of length N") at RT_FAIL_TEXT, stores
///   its address and length in cells 92 and 96, and traps (`unreachable`).
/// - $aipl_text(at, addr, len) -> end: copies len bytes from addr to at.
/// - $aipl_begin(addr, len, max) -> end: starts a contract message of at most
///   max bytes at the heap cursor (growing memory to hold it; nothing is
///   allocated, since the program is about to end), stores its address in
///   cell 92, and copies the len bytes at addr there.
/// - $aipl_end(end): stores the message's length in cell 96 and traps.
/// - $aipl_null(): writes "Null pointer dereference" at RT_FAIL_TEXT, stores
///   its address and length in cells 92 and 96, and traps.
fn check_functions(fns: &CheckFns) -> Vec<Body> {
    use Instruction::*;
    let b0 = MemArg { offset: 0, align: 0, memory_index: 0 };

    // $aipl_dec: params at (0), v (1); locals t:i64 (2), end (3)
    let mut d = Body::new(vec![(1, ValType::I64), (1, ValType::I32)]);
    d.instruction(&LocalGet(1));
    d.instruction(&I64Const(0));
    d.instruction(&I64LtS);
    d.instruction(&If(BlockType::Empty));
    d.instruction(&LocalGet(0));
    d.instruction(&I32Const(45)); // '-'
    d.instruction(&I32Store8(b0));
    d.instruction(&LocalGet(0));
    d.instruction(&I32Const(1));
    d.instruction(&I32Add);
    d.instruction(&LocalSet(0));
    // the magnitude, read unsigned (so the most negative value works too)
    d.instruction(&I64Const(0));
    d.instruction(&LocalGet(1));
    d.instruction(&I64Sub);
    d.instruction(&LocalSet(1));
    d.instruction(&End);
    // count the digits: end = at + 1 + (times v / 10 stays >= 1)
    d.instruction(&LocalGet(1));
    d.instruction(&LocalSet(2));
    d.instruction(&LocalGet(0));
    d.instruction(&I32Const(1));
    d.instruction(&I32Add);
    d.instruction(&LocalSet(3));
    d.instruction(&Block(BlockType::Empty));
    d.instruction(&Loop(BlockType::Empty));
    d.instruction(&LocalGet(2));
    d.instruction(&I64Const(10));
    d.instruction(&I64LtU);
    d.instruction(&BrIf(1));
    d.instruction(&LocalGet(2));
    d.instruction(&I64Const(10));
    d.instruction(&I64DivU);
    d.instruction(&LocalSet(2));
    d.instruction(&LocalGet(3));
    d.instruction(&I32Const(1));
    d.instruction(&I32Add);
    d.instruction(&LocalSet(3));
    d.instruction(&Br(0));
    d.instruction(&End);
    d.instruction(&End);
    // write them last to first
    d.instruction(&LocalGet(3));
    d.instruction(&LocalSet(0));
    d.instruction(&Loop(BlockType::Empty));
    d.instruction(&LocalGet(0));
    d.instruction(&I32Const(1));
    d.instruction(&I32Sub);
    d.instruction(&LocalTee(0));
    d.instruction(&LocalGet(1));
    d.instruction(&I64Const(10));
    d.instruction(&I64RemU);
    d.instruction(&I32WrapI64);
    d.instruction(&I32Const(48)); // '0'
    d.instruction(&I32Add);
    d.instruction(&I32Store8(b0));
    d.instruction(&LocalGet(1));
    d.instruction(&I64Const(10));
    d.instruction(&I64DivU);
    d.instruction(&LocalTee(1));
    d.instruction(&I64Const(0));
    d.instruction(&I64Ne);
    d.instruction(&BrIf(0));
    d.instruction(&End);
    d.instruction(&LocalGet(3));
    d.instruction(&End);

    // $aipl_oob: params index (0), array (1); local at (2)
    let mut o = Body::new(vec![(1, ValType::I32)]);
    emit_text_store(&mut o, &I32Const(RT_FAIL_TEXT), OOB_PREFIX);
    o.instruction(&I32Const(RT_FAIL_TEXT + OOB_PREFIX.len() as i32));
    o.instruction(&LocalGet(0));
    o.instruction(&I64ExtendI32S);
    o.instruction(&Call(fns.dec));
    o.instruction(&LocalSet(2));
    emit_text_store(&mut o, &LocalGet(2), OOB_MIDDLE);
    o.instruction(&LocalGet(2));
    o.instruction(&I32Const(OOB_MIDDLE.len() as i32));
    o.instruction(&I32Add);
    o.instruction(&LocalGet(1));
    o.instruction(&I32Const(4));
    o.instruction(&I32Sub);
    o.instruction(&I32Load(M4));
    o.instruction(&I64ExtendI32S);
    o.instruction(&Call(fns.dec));
    o.instruction(&LocalSet(2));
    o.instruction(&I32Const(RT_FAIL_ADDR));
    o.instruction(&I32Const(RT_FAIL_TEXT));
    o.instruction(&I32Store(M4));
    o.instruction(&I32Const(RT_FAIL_LEN));
    o.instruction(&LocalGet(2));
    o.instruction(&I32Const(RT_FAIL_TEXT));
    o.instruction(&I32Sub);
    o.instruction(&I32Store(M4));
    o.instruction(&Unreachable);
    o.instruction(&End);

    // $aipl_text: params at (0), addr (1), len (2)
    let mut t = Body::new(vec![]);
    t.instruction(&Block(BlockType::Empty));
    t.instruction(&Loop(BlockType::Empty));
    t.instruction(&LocalGet(2));
    t.instruction(&I32Eqz);
    t.instruction(&BrIf(1));
    t.instruction(&LocalGet(0));
    t.instruction(&LocalGet(1));
    t.instruction(&I32Load8U(b0));
    t.instruction(&I32Store8(b0));
    for k in [0u32, 1] {
        t.instruction(&LocalGet(k));
        t.instruction(&I32Const(1));
        t.instruction(&I32Add);
        t.instruction(&LocalSet(k));
    }
    t.instruction(&LocalGet(2));
    t.instruction(&I32Const(1));
    t.instruction(&I32Sub);
    t.instruction(&LocalSet(2));
    t.instruction(&Br(0));
    t.instruction(&End);
    t.instruction(&End);
    t.instruction(&LocalGet(0));
    t.instruction(&End);

    // $aipl_begin: params addr (0), len (1), max (2); locals at (3), pages (4)
    let mut b = Body::new(vec![(2, ValType::I32)]);
    b.instruction(&I32Const(0));
    b.instruction(&I32Load(M4));
    b.instruction(&LocalSet(3));
    // pages needed past the current size: (at + max + 65535) / 65536 - size
    b.instruction(&LocalGet(3));
    b.instruction(&LocalGet(2));
    b.instruction(&I32Add);
    b.instruction(&I32Const(65535));
    b.instruction(&I32Add);
    b.instruction(&I32Const(16));
    b.instruction(&I32ShrU);
    b.instruction(&MemorySize(0));
    b.instruction(&I32Sub);
    b.instruction(&LocalTee(4));
    b.instruction(&I32Const(0));
    b.instruction(&I32GtS);
    b.instruction(&If(BlockType::Empty));
    b.instruction(&LocalGet(4));
    b.instruction(&MemoryGrow(0));
    b.instruction(&Drop);
    b.instruction(&End);
    b.instruction(&I32Const(RT_FAIL_ADDR));
    b.instruction(&LocalGet(3));
    b.instruction(&I32Store(M4));
    b.instruction(&LocalGet(3));
    b.instruction(&LocalGet(0));
    b.instruction(&LocalGet(1));
    b.instruction(&Call(fns.text));
    b.instruction(&End);

    // $aipl_end: param end (0)
    let mut e = Body::new(vec![]);
    e.instruction(&I32Const(RT_FAIL_LEN));
    e.instruction(&LocalGet(0));
    e.instruction(&I32Const(RT_FAIL_ADDR));
    e.instruction(&I32Load(M4));
    e.instruction(&I32Sub);
    e.instruction(&I32Store(M4));
    e.instruction(&Unreachable);
    e.instruction(&End);

    // $aipl_null
    let mut n = Body::new(vec![]);
    emit_text_store(&mut n, &I32Const(RT_FAIL_TEXT), NULL_TEXT);
    n.instruction(&I32Const(RT_FAIL_ADDR));
    n.instruction(&I32Const(RT_FAIL_TEXT));
    n.instruction(&I32Store(M4));
    n.instruction(&I32Const(RT_FAIL_LEN));
    n.instruction(&I32Const(NULL_TEXT.len() as i32));
    n.instruction(&I32Store(M4));
    n.instruction(&Unreachable);
    n.instruction(&End);

    // $aipl_uaf
    let mut u = Body::new(vec![]);
    emit_text_store(&mut u, &I32Const(RT_FAIL_TEXT), UAF_TEXT);
    u.instruction(&I32Const(RT_FAIL_ADDR));
    u.instruction(&I32Const(RT_FAIL_TEXT));
    u.instruction(&I32Store(M4));
    u.instruction(&I32Const(RT_FAIL_LEN));
    u.instruction(&I32Const(UAF_TEXT.len() as i32));
    u.instruction(&I32Store(M4));
    u.instruction(&Unreachable);
    u.instruction(&End);
    vec![d, o, t, b, e, n, u]
}

/// A contract: whether it is an `ens`, its condition, and its message.
type ContractCheck<'a> = (bool, &'a Expr, (String, Vec<MsgItem>));

/// One piece of a contract failure's message after its fixed text: literal
/// text, or the value of a local (by name) shown as the VM shows it.
enum MsgItem {
    Text(String),
    /// an i32 (or an enum, union, or function reference): decimal
    Int(String),
    /// a pointer or array: its address (the low half), unsigned decimal
    Addr(String),
    /// an i64: decimal, then the text "i64" (a separate item)
    Long(String),
    /// a bool: true or false
    Bool(String),
}

/// How the VM shows a value of type `ty` in a contract message, if compiled
/// code can show it the same way (floats, strings, and results it cannot).
fn shown_as(ty: &Type, name: &str) -> Option<MsgItem> {
    match ty {
        Type::Union(_) => None,
        Type::I32 | Type::Enum(_) | Type::Fn(..) => Some(MsgItem::Int(name.to_string())),
        Type::I64 => Some(MsgItem::Long(name.to_string())),
        Type::Ptr(_) | Type::Array(_) => Some(MsgItem::Addr(name.to_string())),
        Type::Bool => Some(MsgItem::Bool(name.to_string())),
        _ => None,
    }
}

/// The function's `req` and `ens` contracts in order, each with its failure
/// message as the VM writes it (`vm.rs contract_failure`) but without the
/// position: the fixed part (`Pre-condition failed in 'f': (req (gt n 0))`),
/// then ` with n = -1, res = 5`, leaving out values compiled code cannot
/// show. The position is left out because it is a position in the program
/// text the resolver flattened, which the Rust and AIPL resolvers lay out
/// differently (and which is not the user's file once there are imports):
/// with it, the bytes would depend on that layout.
fn contract_messages<'f>(f: &'f FnDef) -> Vec<ContractCheck<'f>> {
    let mut out = Vec::new();
    // an inv is checked twice: with the req checks on entry, and with the
    // ens checks on return (the entry message first)
    let checks: Vec<(bool, &Expr, &str, &str)> = f
        .contracts
        .iter()
        .flat_map(|c| match c {
            Contract::Requires(e) => vec![(false, e, "Pre-condition", "req")],
            Contract::Ensures(e) => vec![(true, e, "Post-condition", "ens")],
            Contract::Invariant(e) => vec![(false, e, "Entry invariant", "inv"), (true, e, "Exit invariant", "inv")],
        })
        .collect();
    for (is_ens, expr, kind, form) in checks {
        let text = format!("{} failed in '{}': ({} {})", kind, f.name, form, crate::printer::expr_str(expr));
        let mut items = Vec::new();
        let mut shown: Vec<(String, Option<MsgItem>)> =
            f.params.iter().map(|(n, t)| (n.clone(), shown_as(t, n))).collect();
        if is_ens {
            shown.push(("res".to_string(), if f.return_type == Type::Void { None } else { shown_as(&f.return_type, "res") }));
        }
        for (name, item) in shown {
            let sep = if items.is_empty() { " with " } else { ", " };
            match item {
                Some(v) => {
                    let long = matches!(v, MsgItem::Long(_));
                    items.push(MsgItem::Text(format!("{sep}{name} = ")));
                    items.push(v);
                    if long {
                        items.push(MsgItem::Text("i64".to_string()));
                    }
                }
                None if name == "res" && f.return_type == Type::Void => items.push(MsgItem::Text(format!("{sep}res = void"))),
                None => {}
            }
        }
        out.push((is_ens, expr, (text, items)));
    }
    out
}

/// A `req`/`ens` check: unless the condition holds, write the message (its
/// fixed text and the values shown) to fresh memory, store its address and
/// length in cells 92 and 96, and trap.
fn emit_contract_check(
    expr: &Expr,
    msg: &(String, Vec<MsgItem>),
    ctx: &Ctx,
    func: &mut Body,
    strings: &HashMap<String, u32>,
) -> Result<(), String> {
    // the condition and its failure code are at the condition
    func.at.push(ctx.position(expr.span()));
    let r = emit_contract_check_at(expr, msg, ctx, func, strings);
    func.at.pop();
    r
}

fn emit_contract_check_at(
    expr: &Expr,
    msg: &(String, Vec<MsgItem>),
    ctx: &Ctx,
    func: &mut Body,
    strings: &HashMap<String, u32>,
) -> Result<(), String> {
    use Instruction::*;
    let (text, items) = msg;
    let addr = |s: &str| strings.get(s).map(|a| *a as i32).ok_or_else(|| format!("Wasm Codegen: contract text not interned: {s}"));
    let local = |n: &str| ctx.locals.get(n).copied().ok_or_else(|| format!("Wasm Codegen: unknown local '{n}' in a contract message"));
    // at most: the texts, 24 bytes per number, 5 per bool
    let max: usize = text.len()
        + items
            .iter()
            .map(|i| match i {
                MsgItem::Text(t) => t.len(),
                MsgItem::Int(_) | MsgItem::Long(_) | MsgItem::Addr(_) => 24,
                MsgItem::Bool(_) => 5,
            })
            .sum::<usize>();
    compile_expr(expr, ctx, func)?;
    func.instruction(&I32Eqz);
    func.instruction(&If(BlockType::Empty));
    func.instruction(&I32Const(addr(text)?));
    func.instruction(&I32Const(text.len() as i32));
    func.instruction(&I32Const(max as i32));
    func.instruction(&Call(ctx.checks.begin));
    for item in items {
        match item {
            MsgItem::Text(t) => {
                func.instruction(&I32Const(addr(t)?));
                func.instruction(&I32Const(t.len() as i32));
                func.instruction(&Call(ctx.checks.text));
            }
            MsgItem::Int(n) => {
                func.instruction(&LocalGet(local(n)?));
                func.instruction(&I64ExtendI32S);
                func.instruction(&Call(ctx.checks.dec));
            }
            MsgItem::Long(n) => {
                func.instruction(&LocalGet(local(n)?));
                func.instruction(&Call(ctx.checks.dec));
            }
            MsgItem::Addr(n) => {
                func.instruction(&LocalGet(local(n)?));
                func.instruction(&I32WrapI64);
                func.instruction(&I64ExtendI32U);
                func.instruction(&Call(ctx.checks.dec));
            }
            MsgItem::Bool(n) => {
                // "true" or "false": address, then length
                let l = local(n)?;
                for (t, f) in [(addr("true")?, addr("false")?), (4, 5)] {
                    func.instruction(&LocalGet(l));
                    func.instruction(&If(BlockType::Result(ValType::I32)));
                    func.instruction(&I32Const(t));
                    func.instruction(&Else);
                    func.instruction(&I32Const(f));
                    func.instruction(&End);
                }
                func.instruction(&Call(ctx.checks.text));
            }
        }
    }
    func.instruction(&Call(ctx.checks.end));
    func.instruction(&Unreachable);
    func.instruction(&End);
    Ok(())
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
const STRING_DATA_BASE: u32 = 1024;
/// The literals and the heap start must fit in the initial 16 pages.
const STRING_DATA_LIMIT: u32 = 16 * 65536;

/// Where a module's string literals live. Both backends use it, so a literal
/// has the same address in the VM and in wasm.
pub struct StringLayout {
    /// The data segment placed at 1024: each literal once, as [len u32 LE][bytes].
    pub blob: Vec<u8>,
    /// Literal -> address of its bytes; a `str` value is that address.
    pub addrs: HashMap<String, u32>,
    /// Address of the interned "\n" used by sys.print under WASI (0 if unused).
    pub newline_addr: u32,
    /// First heap address: just past the literals, 8-aligned. Stores below it
    /// (other than to the cells 4..64) trap, so literals are read-only.
    pub heap_start: u32,
}

/// Interns `module`'s string literals in first-use order (the "\n" for
/// sys.print first, when the module prints under WASI).
pub fn string_layout(module: &Module) -> Result<StringLayout, String> {
    let mut blob: Vec<u8> = Vec::new();
    let mut addrs: HashMap<String, u32> = HashMap::new();
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
    if collect_wasi_imports(module).contains(&Wasi::FdWrite) && module_uses_op(module, &OpCode::SysPrint) {
        newline_addr = intern("\n", &mut blob, &mut addrs);
    }
    walk_module(module, &mut |e| {
        if let Expr::Lit(Literal::Str(s), _) = e {
            intern(s, &mut blob, &mut addrs);
        }
    });
    // then the texts of compiled contract messages, function by function
    for f in &module.functions {
        for (_, _, (text, items)) in contract_messages(f) {
            intern(&text, &mut blob, &mut addrs);
            for item in &items {
                match item {
                    MsgItem::Text(t) => {
                        intern(t, &mut blob, &mut addrs);
                    }
                    MsgItem::Bool(_) => {
                        intern("true", &mut blob, &mut addrs);
                        intern("false", &mut blob, &mut addrs);
                    }
                    MsgItem::Int(_) | MsgItem::Long(_) | MsgItem::Addr(_) => {}
                }
            }
        }
    }
    let heap_start = heap_start_after(blob.len());
    if heap_start > STRING_DATA_LIMIT {
        return Err(format!(
            "Wasm Codegen: string literals need {} bytes but at most {} fit before the heap",
            blob.len(),
            STRING_DATA_LIMIT - STRING_DATA_BASE
        ));
    }
    Ok(StringLayout { blob, addrs, newline_addr, heap_start })
}

/// The heap cursor's initial value for a module with `blob_len` bytes of
/// string literals: the first 8-aligned address after them.
fn heap_start_after(blob_len: usize) -> u32 {
    (STRING_DATA_BASE + blob_len as u32 + 7) & !7
}

/// The address of runtime cell `cell` (64..88): the fixed cell, or in a
/// threaded module the same offset in this thread's scratch block, whose
/// address is global 0 (64 in the main thread, so its addresses are unchanged).
fn emit_rt(func: &mut Body, ctx: &Ctx, cell: i32) {
    if ctx.threaded {
        func.instruction(&Instruction::GlobalGet(0));
        if cell != RT_IOV0_BUF {
            func.instruction(&Instruction::I32Const(cell - RT_IOV0_BUF));
            func.instruction(&Instruction::I32Add);
        }
    } else {
        func.instruction(&Instruction::I32Const(cell));
    }
}

/// Bytes each spawned thread allocates for its runtime scratch cells (64..88).
const RT_SCRATCH_SIZE: i32 = 24;

/// Rounds the i32 on the stack up to a multiple of 8: every allocation is,
/// so with an 8-aligned heap start every block is 8-aligned (atomics, which
/// trap when unaligned, i64/f64 fields, and WASI out-parameters).
fn emit_round8(func: &mut Body) {
    func.instruction(&Instruction::I32Const(7));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(-8));
    func.instruction(&Instruction::I32And);
}
/// Threaded modules: 1 once the shared memory has been initialised.
const RT_INIT_FLAG: i32 = 88;

/// A threaded module's start function: the first instance to run it copies
/// the heap cursor and the string literals into the (zeroed) shared memory;
/// later instances (spawned threads) see the flag and skip it.
fn threaded_init_function(blob_len: u32) -> Body {
    use Instruction::*;
    let mut f = Body::new(vec![]);
    f.instruction(&I32Const(RT_INIT_FLAG));
    f.instruction(&I32Const(0));
    f.instruction(&I32Const(1));
    f.instruction(&I32AtomicRmwCmpxchg(M4));
    f.instruction(&I32Eqz);
    f.instruction(&If(BlockType::Empty));
    f.instruction(&I32Const(0));
    f.instruction(&I32Const(0));
    f.instruction(&I32Const(4));
    f.instruction(&MemoryInit { mem: 0, data_index: 0 });
    if blob_len > 0 {
        f.instruction(&I32Const(STRING_DATA_BASE as i32));
        f.instruction(&I32Const(0));
        f.instruction(&I32Const(blob_len as i32));
        f.instruction(&MemoryInit { mem: 0, data_index: 1 });
    }
    f.instruction(&End);
    f.instruction(&End);
    f
}

/// `wasi_thread_start(tid, rec)`, called by the host on a new thread with the
/// thread record `rec` = [done result fn arg] made by thread.spawn: allocate
/// this thread's runtime scratch block, run fn(arg) through the table, store
/// the result, then set done and wake any thread.join waiting on it.
fn thread_start_function(worker_type: u32) -> Body {
    use Instruction::*;
    let mut f = Body::new(vec![]);
    let at = |offset: u64| MemArg { offset, align: 2, memory_index: 0 };
    f.instruction(&I32Const(0));
    f.instruction(&I32Const(RT_SCRATCH_SIZE));
    f.instruction(&I32AtomicRmwAdd(M4));
    f.instruction(&GlobalSet(0));
    emit_grow_to_cursor(&mut f);
    f.instruction(&LocalGet(1));
    f.instruction(&LocalGet(1));
    f.instruction(&I32Load(at(12)));
    f.instruction(&LocalGet(1));
    f.instruction(&I32Load(at(8)));
    f.instruction(&CallIndirect { type_index: worker_type, table_index: 0 });
    f.instruction(&I32Store(at(4)));
    f.instruction(&LocalGet(1));
    f.instruction(&I32Const(1));
    f.instruction(&I32AtomicStore(M4));
    f.instruction(&LocalGet(1));
    f.instruction(&I32Const(-1));
    f.instruction(&MemoryAtomicNotify(M4));
    f.instruction(&Drop);
    f.instruction(&End);
    f
}

/// The preopened directories a host hands the module: fd 3 is the working
/// directory, against which relative paths resolve; fd 4 is "/", against
/// which absolute paths resolve, when the host grants it.
const WASI_PREOPEN_FD: i32 = 3;
const WASI_ROOT_FD: i32 = 4;

/// Pushes the directory fd for the path in locals (ptr, len): an absolute
/// path ("/...") resolves in fd 4 with the leading '/' dropped (ptr and len
/// are adjusted); anything else in fd 3.
fn emit_path_dir(func: &mut Body, ptr: u32, len: u32) {
    use Instruction::*;
    func.instruction(&LocalGet(ptr));
    func.instruction(&I32Load8U(MemArg { offset: 0, align: 0, memory_index: 0 }));
    func.instruction(&I32Const(47));
    func.instruction(&I32Eq);
    func.instruction(&If(BlockType::Result(ValType::I32)));
    func.instruction(&LocalGet(ptr));
    func.instruction(&I32Const(1));
    func.instruction(&I32Add);
    func.instruction(&LocalSet(ptr));
    func.instruction(&LocalGet(len));
    func.instruction(&I32Const(1));
    func.instruction(&I32Sub);
    func.instruction(&LocalSet(len));
    func.instruction(&I32Const(WASI_ROOT_FD));
    func.instruction(&Else);
    func.instruction(&I32Const(WASI_PREOPEN_FD));
    func.instruction(&End);
}
const WASI_OFLAGS_CREAT: i32 = 1 << 0;
const WASI_OFLAGS_TRUNC: i32 = 1 << 3;
const WASI_RIGHT_FD_READ: i64 = 1 << 1;
const WASI_RIGHT_FD_WRITE: i64 = 1 << 6;
const WASI_RIGHT_FD_READDIR: i64 = 1 << 14;
const WASI_OFLAGS_DIRECTORY: i32 = 1 << 1;
const WASI_LOOKUP_SYMLINK_FOLLOW: i32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Wasi {
    FdWrite,
    FdRead,
    PathOpen,
    FdClose,
    ProcExit,
    PathUnlinkFile,
    ArgsSizesGet,
    ArgsGet,
    EnvironSizesGet,
    EnvironGet,
    /// `thread-spawn` from the wasi-threads ABI (module "wasi"): starts an OS
    /// thread that instantiates this module on the shared memory and calls
    /// its exported `wasi_thread_start(tid, start_arg)`. Wasmtime dropped
    /// wasi-threads in v47, so AIPL's own host (the runner) provides it; the
    /// names match the old ABI so any wasi-threads host also works.
    ThreadSpawn,
    ClockTimeGet,
    RandomGet,
    FdReaddir,
    PathFilestatGet,
}

impl Wasi {
    /// The import module: wasi-threads' spawn lives in "wasi", the rest in
    /// "wasi_snapshot_preview1".
    fn module(self) -> &'static str {
        if self == Wasi::ThreadSpawn { "wasi" } else { "wasi_snapshot_preview1" }
    }

    fn name(self) -> &'static str {
        match self {
            Wasi::FdWrite => "fd_write",
            Wasi::FdRead => "fd_read",
            Wasi::PathOpen => "path_open",
            Wasi::FdClose => "fd_close",
            Wasi::ProcExit => "proc_exit",
            Wasi::PathUnlinkFile => "path_unlink_file",
            Wasi::ArgsSizesGet => "args_sizes_get",
            Wasi::ArgsGet => "args_get",
            Wasi::EnvironSizesGet => "environ_sizes_get",
            Wasi::EnvironGet => "environ_get",
            Wasi::ThreadSpawn => "thread-spawn",
            Wasi::ClockTimeGet => "clock_time_get",
            Wasi::RandomGet => "random_get",
            Wasi::FdReaddir => "fd_readdir",
            Wasi::PathFilestatGet => "path_filestat_get",
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
            Wasi::ArgsSizesGet | Wasi::ArgsGet | Wasi::EnvironSizesGet | Wasi::EnvironGet => (vec![I32, I32], vec![I32]),
            Wasi::ThreadSpawn => (vec![I32], vec![I32]),
            Wasi::ClockTimeGet => (vec![I32, I64, I32], vec![I32]),
            Wasi::RandomGet => (vec![I32, I32], vec![I32]),
            Wasi::FdReaddir => (vec![I32, I32, I32, I64, I32], vec![I32]),
            Wasi::PathFilestatGet => (vec![I32, I32, I32, I32, I32], vec![I32]),
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
            OpCode::FsOpendir => Some(Wasi::PathOpen),
            OpCode::FsReaddir => Some(Wasi::FdReaddir),
            OpCode::FsStat => Some(Wasi::PathFilestatGet),
            OpCode::ArgsSizes => Some(Wasi::ArgsSizesGet),
            OpCode::ArgsGet => Some(Wasi::ArgsGet),
            OpCode::EnvSizes => Some(Wasi::EnvironSizesGet),
            OpCode::EnvGet => Some(Wasi::EnvironGet),
            OpCode::ThreadSpawn => Some(Wasi::ThreadSpawn),
            OpCode::SysTime | OpCode::SysMonotonic => Some(Wasi::ClockTimeGet),
            OpCode::SysRandom => Some(Wasi::RandomGet),
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

pub(crate) fn walk_expr(expr: &Expr, visit: &mut dyn FnMut(&Expr)) {
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
        Expr::Block(exprs, _) | Expr::Unsafe(exprs, _) => {
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
        Expr::Make { args, .. } | Expr::Record { args, .. } => {
            for a in args {
                walk_expr(a, visit);
            }
        }
        Expr::With { value, new_val, .. } => {
            walk_expr(value, visit);
            walk_expr(new_val, visit);
        }
        Expr::Match { value, arms, else_body, .. } => {
            walk_expr(value, visit);
            for e in arms.iter().flat_map(|a| a.body.iter()).chain(else_body.iter().flatten()) {
                walk_expr(e, visit);
            }
        }
        Expr::Sizeof { .. } | Expr::Null { .. } | Expr::Ref { .. } => {}
        Expr::Break(_) | Expr::Continue(_) => {}
        Expr::Return { val, .. } => {
            if let Some(v) = val {
                walk_expr(v, visit);
            }
        }
        // Source order (function, then arguments), as the self-hosted compiler walks it.
        Expr::CallRef { func, args, .. } => {
            walk_expr(func, visit);
            for a in args {
                walk_expr(a, visit);
            }
        }
        Expr::GetField { ptr, .. }
        | Expr::ArrLen { arr: ptr, .. }
        | Expr::Cast { addr: ptr, gen: None, .. }
        | Expr::Addr { val: ptr, .. } => walk_expr(ptr, visit),
        Expr::Cast { addr, gen: Some(g), .. } => {
            walk_expr(addr, visit);
            walk_expr(g, visit);
        }
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

/// Whether the module has compiled checks (a req or ens, or an array or
/// struct access anywhere, contracts included), so it needs the check helpers.
fn module_uses_checks(module: &Module) -> bool {
    let mut found = module
        .functions
        .iter()
        .any(|f| !f.contracts.is_empty());
    walk_module(module, &mut |e| {
        if matches!(e, Expr::ArrGet { .. } | Expr::ArrSet { .. } | Expr::ArrLen { .. } | Expr::GetField { .. } | Expr::PutField { .. }) {
            found = true;
        }
    });
    found
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

/// True if a function body needs the two extra scratch locals: an op lowered
/// through WASI, or an atomic op that needs its address or operands twice.
fn fn_uses_io(body: &[Expr]) -> bool {
    let mut found = false;
    for e in body {
        walk_expr(e, &mut |x| match x {
            Expr::Op { op, .. } if Wasi::for_op(op).is_some() => found = true,
            Expr::Op { op: OpCode::AtomicCas | OpCode::AtomicLock | OpCode::AtomicUnlock, .. } => found = true,
            _ => {}
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
fn emit_errno_to_result(func: &mut Body, ctx: &Ctx, out_cell: Option<i32>) {
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(-1));
    func.instruction(&Instruction::Else);
    match out_cell {
        Some(cell) => {
            emit_rt(func, ctx, cell);
            func.instruction(&Instruction::I32Load(M4));
        }
        None => {
            func.instruction(&Instruction::I32Const(0));
        }
    }
    func.instruction(&Instruction::End);
}


