//! Import/module resolution.
//!
//! STATUS: this is deliberately temporary, Stage-0-only scaffolding, not a
//! permanent part of the toolchain. AIPL has no file I/O primitive today (no
//! opcode can open/read a file), so this logic - finding files, parsing them,
//! merging and renaming functions - cannot be expressed in AIPL yet. Once
//! minimal WASI file I/O (path_open/fd_read/fd_close) is wired into the wasm
//! backend, this entire module should be deleted and rewritten as real AIPL
//! source (e.g. `aipl_src/resolver.aipl`) that calls those primitives
//! directly. Nothing here should grow new unrelated functionality in the
//! meantime - it exists only so `(import ...)` works for the current Rust
//! bootstrap CLI while the self-hosted compiler catches up.
//!
//! AIPL's import model is deliberately simple, no dynamic linking: every
//! `(import name)` or `(import name as alias)` is resolved at compile time by
//! finding `name.aipl`, parsing it, and merging its functions into one flat
//! program. A module's identity (for qualification and de-duplication) is
//! always the bare name written in the `import` statement - `as alias` only
//! controls how *this file's* call sites are spelled locally
//! (`alias.fn`), it is rewritten to the canonical `name.fn` during
//! resolution and never stored as a separate identity. This means importing
//! the same module under different aliases from different files still
//! produces exactly one merged copy of it.
//!
//! Every function pulled in from an imported module is renamed to
//! `<import_name>.<fn_name>` (one flat level, regardless of import depth),
//! and every call inside that module - whether to its own functions or
//! through its own import aliases - is rewritten to match. The entry file's
//! own functions keep their original bare names.

use crate::ast::*;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub struct Resolver;

impl Resolver {
    /// Parses `entry_path`, recursively resolves its imports, and returns one
    /// flat `Module` ready for the existing checker/VM/wasm backends - none
    /// of which need to know imports exist.
    pub fn resolve(entry_path: &Path) -> Result<Module, String> {
        let entry_src = fs::read_to_string(entry_path)
            .map_err(|e| format!("{}: Cannot read: {}", entry_path.display(), e))?;
        let entry_module = crate::parser::Parser::parse(&entry_src)
            .map_err(|e| format!("{}: {}", entry_path.display(), e))?;

        // `included` tracks which resolved files have already had their
        // functions appended to `output`, so a module reachable both
        // directly and transitively (a diamond dependency) is only emitted
        // once. `in_progress` is the separate cycle-detection stack.
        let mut output: Vec<FnDef> = Vec::new();
        let mut output_structs: Vec<StructDef> = Vec::new();
        let mut included: HashSet<PathBuf> = HashSet::new();
        let mut in_progress: HashSet<PathBuf> = HashSet::new();

        for import in &entry_module.imports {
            Self::resolve_import(import, entry_path, &mut output, &mut output_structs, &mut included, &mut in_progress)?;
        }

        // The entry file's own functions and structs keep bare names; only
        // names that go through an import alias are rewritten.
        let alias_map = build_alias_map(&entry_module.imports);
        let mut rewrite = |_: NameKind, name: &mut String| {
            if let Some((prefix, rest)) = name.split_once('.') {
                if let Some(canonical) = alias_map.get(prefix) {
                    *name = format!("{}.{}", canonical, rest);
                }
            }
        };
        let mut own_structs = entry_module.structs;
        for st in &mut own_structs {
            for field in &mut st.fields {
                walk_type_names(&mut field.ty, &mut rewrite);
            }
        }
        output_structs.extend(own_structs);
        let mut own_functions = entry_module.functions;
        for f in &mut own_functions {
            walk_fn_names(f, &mut rewrite);
        }
        output.extend(own_functions);

        Ok(Module {
            name: entry_module.name,
            imports: vec![],
            structs: output_structs,
            functions: output,
        })
    }

    fn resolve_import(
        import: &Import,
        importer_path: &Path,
        output: &mut Vec<FnDef>,
        output_structs: &mut Vec<StructDef>,
        included: &mut HashSet<PathBuf>,
        in_progress: &mut HashSet<PathBuf>,
    ) -> Result<(), String> {
        let file_path = Self::find_module_file(&import.name, importer_path)
            .map_err(|e| format!("{}: {}", importer_path.display(), e))?;

        if included.contains(&file_path) {
            return Ok(());
        }
        if in_progress.contains(&file_path) {
            return Err(format!(
                "{}: Circular import detected: '{}' is imported while already being resolved",
                file_path.display(),
                file_path.display()
            ));
        }
        in_progress.insert(file_path.clone());

        let src = fs::read_to_string(&file_path)
            .map_err(|e| format!("{}: Cannot read: {}", file_path.display(), e))?;
        let module = crate::parser::Parser::parse(&src)
            .map_err(|e| format!("{}: {}", file_path.display(), e))?;

        // Resolve this module's own imports first (its dependencies must be
        // fully qualified and emitted before we merge this module in).
        for sub_import in &module.imports {
            Self::resolve_import(sub_import, &file_path, output, output_structs, included, in_progress)?;
        }

        // Qualify this module's own functions and structs as `<module>.<name>`,
        // rewrite its references to them, and rewrite its alias-qualified
        // references to the canonical module name.
        let own_alias_map = build_alias_map(&module.imports);
        let own_fns_names: HashSet<String> = module.functions.iter().map(|f| f.name.clone()).collect();
        let own_struct_names: HashSet<String> = module.structs.iter().map(|s| s.name.clone()).collect();
        let mut rewrite = |kind: NameKind, name: &mut String| {
            if let Some((prefix, rest)) = name.split_once('.') {
                if let Some(canonical) = own_alias_map.get(prefix) {
                    *name = format!("{}.{}", canonical, rest);
                    return;
                }
            }
            let own = match kind {
                NameKind::Function => &own_fns_names,
                NameKind::Struct => &own_struct_names,
            };
            if own.contains(name.as_str()) {
                *name = format!("{}.{}", import.name, name);
            }
        };
        let mut own_structs = module.structs;
        for st in &mut own_structs {
            for field in &mut st.fields {
                walk_type_names(&mut field.ty, &mut rewrite);
            }
            st.name = format!("{}.{}", import.name, st.name);
        }
        output_structs.extend(own_structs);
        let mut own_fns = module.functions;
        for f in &mut own_fns {
            walk_fn_names(f, &mut rewrite);
            f.name = format!("{}.{}", import.name, f.name);
        }
        output.extend(own_fns);

        in_progress.remove(&file_path);
        included.insert(file_path);
        Ok(())
    }

    fn find_module_file(name: &str, importer_path: &Path) -> Result<PathBuf, String> {
        let filename = format!("{}.aipl", name);
        let mut candidates = Vec::new();
        if let Some(dir) = importer_path.parent() {
            candidates.push(dir.join(&filename));
            candidates.push(dir.join("aipl_modules").join(&filename));
        }
        candidates.push(PathBuf::from("aipl_modules").join(&filename));

        for c in &candidates {
            if c.exists() {
                return c
                    .canonicalize()
                    .map_err(|e| format!("Cannot resolve path '{}': {}", c.display(), e));
            }
        }
        Err(format!(
            "Cannot resolve import '{}': no '{}' found in {}",
            name,
            filename,
            candidates.iter().map(|c| format!("'{}'", c.display())).collect::<Vec<_>>().join(", ")
        ))
    }
}

fn build_alias_map(imports: &[Import]) -> HashMap<String, String> {
    imports.iter().filter_map(|i| i.alias.as_ref().map(|a| (a.clone(), i.name.clone()))).collect()
}

/// Which namespace a visited name lives in: functions (`call` targets) or
/// structs (`new`/`get`/`put`/`sizeof` and every `(ptr S)` inside a type).
#[derive(Clone, Copy, PartialEq)]
enum NameKind {
    Function,
    Struct,
}

/// Visits every function and struct name in a function (signature, body,
/// and contracts) and lets the callback rewrite it in place.
fn walk_fn_names(f: &mut FnDef, visit: &mut dyn FnMut(NameKind, &mut String)) {
    for (_, ty) in &mut f.params {
        walk_type_names(ty, visit);
    }
    walk_type_names(&mut f.return_type, visit);
    for c in &mut f.contracts {
        match c {
            Contract::Requires(e) | Contract::Ensures(e) | Contract::Invariant(e) => walk_names_expr(e, visit),
        }
    }
    for e in &mut f.body {
        walk_names_expr(e, visit);
    }
}

fn walk_type_names(ty: &mut Type, visit: &mut dyn FnMut(NameKind, &mut String)) {
    match ty {
        Type::Struct(name) => visit(NameKind::Struct, name),
        Type::Ptr(inner) | Type::Array(inner) => walk_type_names(inner, visit),
        Type::ResultType(a, b) => {
            walk_type_names(a, visit);
            walk_type_names(b, visit);
        }
        Type::Fn(params, ret) => {
            for p in params {
                walk_type_names(p, visit);
            }
            walk_type_names(ret, visit);
        }
        Type::I32 | Type::I64 | Type::F32 | Type::F64 | Type::Bool | Type::Str | Type::Void => {}
    }
}

fn walk_names_expr(expr: &mut Expr, visit: &mut dyn FnMut(NameKind, &mut String)) {
    let mut each = |es: &mut [Expr], visit: &mut dyn FnMut(NameKind, &mut String)| {
        for e in es {
            walk_names_expr(e, visit);
        }
    };
    match expr {
        Expr::Lit(..) | Expr::Var(..) => {}
        Expr::Call { func, args, .. } => {
            visit(NameKind::Function, func);
            each(args, visit);
        }
        Expr::Let { ty, val, .. } => {
            walk_type_names(ty, visit);
            walk_names_expr(val, visit);
        }
        Expr::Set { val, .. } => walk_names_expr(val, visit),
        Expr::Ok(val, ty, _) | Expr::Err(val, ty, _) => {
            if let Some(t) = ty {
                walk_type_names(t, visit);
            }
            walk_names_expr(val, visit);
        }
        Expr::If { cond, then_branch, else_branch, .. } => {
            walk_names_expr(cond, visit);
            walk_names_expr(then_branch, visit);
            walk_names_expr(else_branch, visit);
        }
        Expr::Loop { start, end, step, body, .. } => {
            walk_names_expr(start, visit);
            walk_names_expr(end, visit);
            walk_names_expr(step, visit);
            each(body, visit);
        }
        Expr::While { cond, body, .. } => {
            walk_names_expr(cond, visit);
            each(body, visit);
        }
        Expr::Op { args, .. } => each(args, visit),
        Expr::MatchResult { expr, ok_body, err_body, .. } => {
            walk_names_expr(expr, visit);
            each(ok_body, visit);
            each(err_body, visit);
        }
        Expr::Block(body, _) => each(body, visit),
        Expr::NewStruct { struct_name, .. } | Expr::Sizeof { struct_name, .. } => visit(NameKind::Struct, struct_name),
        Expr::GetField { struct_name, ptr, .. } => {
            visit(NameKind::Struct, struct_name);
            walk_names_expr(ptr, visit);
        }
        Expr::PutField { struct_name, ptr, val, .. } => {
            visit(NameKind::Struct, struct_name);
            walk_names_expr(ptr, visit);
            walk_names_expr(val, visit);
        }
        Expr::ArrNew { elem_ty, size, .. } => {
            walk_type_names(elem_ty, visit);
            walk_names_expr(size, visit);
        }
        Expr::ArrGet { elem_ty, ptr, index, .. } => {
            walk_type_names(elem_ty, visit);
            walk_names_expr(ptr, visit);
            walk_names_expr(index, visit);
        }
        Expr::ArrSet { elem_ty, ptr, index, val, .. } => {
            walk_type_names(elem_ty, visit);
            walk_names_expr(ptr, visit);
            walk_names_expr(index, visit);
            walk_names_expr(val, visit);
        }
        Expr::ArrLen { arr, .. } => walk_names_expr(arr, visit),
        Expr::Null { ty, .. } => walk_type_names(ty, visit),
        Expr::Cast { ty, addr, .. } => {
            walk_type_names(ty, visit);
            walk_names_expr(addr, visit);
        }
        Expr::Addr { val, .. } => walk_names_expr(val, visit),
    }
}
