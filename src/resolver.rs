//! Import/module resolution.
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
            .map_err(|e| format!("Cannot read '{}': {}", entry_path.display(), e))?;
        let entry_module = crate::parser::Parser::parse(&entry_src)?;

        // `included` tracks which resolved files have already had their
        // functions appended to `output`, so a module reachable both
        // directly and transitively (a diamond dependency) is only emitted
        // once. `in_progress` is the separate cycle-detection stack.
        let mut output: Vec<FnDef> = Vec::new();
        let mut included: HashSet<PathBuf> = HashSet::new();
        let mut in_progress: HashSet<PathBuf> = HashSet::new();

        for import in &entry_module.imports {
            Self::resolve_import(import, entry_path, &mut output, &mut included, &mut in_progress)?;
        }

        let alias_map = build_alias_map(&entry_module.imports);
        let mut own_functions = entry_module.functions;
        for f in &mut own_functions {
            // Entry file's own functions keep bare names; only rewrite calls
            // that go through an import alias. Calls to its own sibling
            // functions are already correct as written.
            walk_calls(&mut f.body, &mut |func| {
                if let Some((prefix, rest)) = func.split_once('.') {
                    if let Some(canonical) = alias_map.get(prefix) {
                        *func = format!("{}.{}", canonical, rest);
                    }
                }
            });
        }
        output.extend(own_functions);

        Ok(Module { name: entry_module.name, imports: vec![], functions: output })
    }

    fn resolve_import(
        import: &Import,
        importer_path: &Path,
        output: &mut Vec<FnDef>,
        included: &mut HashSet<PathBuf>,
        in_progress: &mut HashSet<PathBuf>,
    ) -> Result<(), String> {
        let file_path = Self::find_module_file(&import.name, importer_path)?;

        if included.contains(&file_path) {
            return Ok(());
        }
        if in_progress.contains(&file_path) {
            return Err(format!(
                "Circular import detected: '{}' is imported while already being resolved",
                file_path.display()
            ));
        }
        in_progress.insert(file_path.clone());

        let src = fs::read_to_string(&file_path)
            .map_err(|e| format!("Cannot read '{}': {}", file_path.display(), e))?;
        let module = crate::parser::Parser::parse(&src)
            .map_err(|e| format!("Parse error in '{}': {}", file_path.display(), e))?;

        // Resolve this module's own imports first (its dependencies must be
        // fully qualified and emitted before we merge this module in).
        for sub_import in &module.imports {
            Self::resolve_import(sub_import, &file_path, output, included, in_progress)?;
        }

        let own_alias_map = build_alias_map(&module.imports);
        let own_names: HashSet<String> = module.functions.iter().map(|f| f.name.clone()).collect();
        let mut own_fns = module.functions;
        for f in &mut own_fns {
            walk_calls(&mut f.body, &mut |func| {
                if let Some((prefix, rest)) = func.split_once('.') {
                    if let Some(canonical) = own_alias_map.get(prefix) {
                        *func = format!("{}.{}", canonical, rest);
                        return;
                    }
                }
                if own_names.contains(func.as_str()) {
                    *func = format!("{}.{}", import.name, func);
                }
            });
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

/// Recursively visits every `Expr::Call` target name in a function body,
/// letting the callback rewrite it in place.
fn walk_calls<F: FnMut(&mut String)>(body: &mut [Expr], f: &mut F) {
    for expr in body {
        walk_calls_expr(expr, f);
    }
}

fn walk_calls_expr<F: FnMut(&mut String)>(expr: &mut Expr, f: &mut F) {
    match expr {
        Expr::Call { func, args } => {
            f(func);
            for a in args {
                walk_calls_expr(a, f);
            }
        }
        Expr::Let { val, .. } | Expr::Set { val, .. } | Expr::Ok(val) | Expr::Err(val) => {
            walk_calls_expr(val, f);
        }
        Expr::If { cond, then_branch, else_branch } => {
            walk_calls_expr(cond, f);
            walk_calls_expr(then_branch, f);
            walk_calls_expr(else_branch, f);
        }
        Expr::Loop { start, end, step, body, .. } => {
            walk_calls_expr(start, f);
            walk_calls_expr(end, f);
            walk_calls_expr(step, f);
            walk_calls(body, f);
        }
        Expr::While { cond, body } => {
            walk_calls_expr(cond, f);
            walk_calls(body, f);
        }
        Expr::Op { args, .. } => {
            for a in args {
                walk_calls_expr(a, f);
            }
        }
        Expr::MatchResult { expr, ok_body, err_body, .. } => {
            walk_calls_expr(expr, f);
            walk_calls(ok_body, f);
            walk_calls(err_body, f);
        }
        Expr::Block(body) => walk_calls(body, f),
        _ => {}
    }
}
