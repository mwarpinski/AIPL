//! Import resolution and generic expansion (AIPL_SPEC.md 11): the Rust
//! reference for `aipl_src/resolver.aipl` and `aipl_src/generics.aipl`, which
//! follow the same rules on the same S-expression trees.
//!
//! Every `(import name)` / `(import name as alias)` is resolved at compile
//! time by finding `name.aipl`, reading it, and merging its items into one
//! flat program. Imports resolve depth-first, each file once (diamonds), and
//! a file that imports itself through a chain is an error. Inside an imported
//! module `m` its own functions, structs, and generic templates become
//! `m.name`, and alias-qualified names (`u.f` for `(import util as u)`)
//! become canonical (`util.f`); the entry module keeps bare names.
//!
//! Generic templates are then expanded (`crate::generics`), and each concrete
//! item is parsed with the positions of the file it came from.

use crate::ast::*;
use crate::parser::{Parser, Token, TokenKind};
use crate::sexpr::Sx;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub struct Resolver;

/// An item of the flat program and the file it came from (for diagnostics).
/// A generic instance is made from its template's file, but the type
/// arguments substituted into it were written elsewhere: `foreign` maps
/// those tokens' positions to their own files.
pub struct Item {
    pub sx: Sx,
    pub file: PathBuf,
    pub foreign: HashMap<(u32, u32), PathBuf>,
}

impl Item {
    /// The file a token at `pos` of this item was written in.
    pub fn file_of(&self, pos: (u32, u32)) -> &Path {
        self.foreign.get(&pos).map(|p| p.as_path()).unwrap_or(&self.file)
    }
}

/// The flat program before parsing: entry module name, then every struct
/// and every function (imports first, in resolution order).
pub struct FlatProgram {
    pub name: String,
    pub structs: Vec<Item>,
    pub fns: Vec<Item>,
}

struct State<'a> {
    entry_path: &'a Path,
    included: HashSet<PathBuf>,
    in_progress: HashSet<PathBuf>,
    structs: Vec<Item>,
    fns: Vec<Item>,
    /// Canonical names of the generic templates of every resolved module.
    templates: HashSet<String>,
    /// Module name -> its file: two different files may not share a name.
    names: HashMap<String, PathBuf>,
}

/// A parsed `(module name items...)`.
struct ModuleSx {
    name: String,
    imports: Vec<(String, Option<String>)>,
    items: Vec<Sx>,
}

impl Resolver {
    /// Reads `entry_path`, resolves its imports, expands generics, constants,
    /// and enum members, and returns
    /// one flat `Module` for the checker, the VM, and the wasm backend.
    pub fn resolve(entry_path: &Path) -> Result<Module, String> {
        let entry_src = fs::read_to_string(entry_path)
            .map_err(|e| format!("{}: Cannot read: {}", entry_path.display(), e))?;
        Self::resolve_source(&entry_src, entry_path)
    }

    /// `resolve` for source text that is not in a file (the agent server's
    /// request bodies): imports are searched as if the text were at
    /// `entry_path`, which need not exist.
    pub fn resolve_source(entry_src: &str, entry_path: &Path) -> Result<Module, String> {
        // Reading, renaming, expansion, and parsing all recurse once per
        // nesting level; large programs need more than a default thread's
        // stack, so the work always runs on its own large-stack thread.
        let (src, path) = (entry_src.to_string(), entry_path.to_path_buf());
        std::thread::Builder::new()
            .stack_size(256 * 1024 * 1024)
            .spawn(move || Self::resolve_on_this_thread(&src, &path))
            .map_err(|e| e.to_string())?
            .join()
            .map_err(|_| "the resolver thread panicked".to_string())?
    }

    fn resolve_on_this_thread(entry_src: &str, entry_path: &Path) -> Result<Module, String> {
        let flat = Self::flatten(entry_src, entry_path)?;
        let flat = crate::generics::expand(flat)?;
        let flat = crate::consts::expand(flat)?;
        let mut module = Module { name: flat.name.clone(), imports: vec![], structs: vec![], enums: vec![], unions: vec![], functions: vec![] };
        let unions: HashSet<String> = flat
            .structs
            .iter()
            .filter(|i| i.sx.head() == Some("union"))
            .filter_map(|i| i.sx.items().get(1).and_then(|n| n.symbol()).map(String::from))
            .collect();
        for item in flat.structs.iter().chain(flat.fns.iter()) {
            let parsed = parse_item(&flat.name, item, &unions)?;
            module.structs.extend(parsed.structs);
            module.enums.extend(parsed.enums);
            module.unions.extend(parsed.unions);
            module.functions.extend(parsed.functions);
        }
        Ok(module)
    }

    /// Resolution without generic expansion or parsing.
    pub fn flatten(entry_src: &str, entry_path: &Path) -> Result<FlatProgram, String> {
        let entry = read_module(entry_src).map_err(|e| format!("{}: {}", entry_path.display(), e))?;
        let mut st = State {
            entry_path,
            included: HashSet::new(),
            in_progress: HashSet::new(),
            structs: Vec::new(),
            fns: Vec::new(),
            templates: HashSet::new(),
            names: HashMap::new(),
        };
        for (name, _) in &entry.imports {
            resolve_import(&mut st, name, entry_path)?;
        }
        let name = entry.name.clone();
        emit_module(&mut st, entry, None, entry_path);
        Ok(FlatProgram { name, structs: st.structs, fns: st.fns })
    }
}

/// Parses one item as `(module NAME item)`, prefixing errors with its file.
fn parse_item(module_name: &str, item: &Item, unions: &HashSet<String>) -> Result<Module, String> {
    let (line, col) = item.sx.position();
    let tok = |kind| Token { kind, line, col };
    let mut tokens = vec![tok(TokenKind::LParen), tok(TokenKind::Symbol("module".into())), tok(TokenKind::Symbol(module_name.into()))];
    item.sx.flatten(&mut tokens);
    tokens.push(tok(TokenKind::RParen));
    Parser::parse_tokens_with(tokens, unions.clone()).map_err(|e| {
        // the error's own "L:C: " says which token, and so which file
        let pos = e.split(':').take(2).map(|n| n.trim().parse::<u32>()).collect::<Result<Vec<_>, _>>();
        let file = match pos.as_deref() {
            Ok([l, c]) => item.file_of((*l, *c)),
            _ => &item.file,
        };
        format!("{}: {}", file.display(), e)
    })
}

fn read_module(src: &str) -> Result<ModuleSx, String> {
    let sx = Sx::read_one(&Parser::tokenize(src)?)?;
    let (line, col) = sx.position();
    if sx.head() != Some("module") {
        return Err(format!("{}:{}: a file must be one (module NAME ...) form", line, col));
    }
    let name = sx.items().get(1).and_then(|n| n.symbol()).ok_or(format!("{}:{}: the module needs a name", line, col))?;
    let mut imports = Vec::new();
    let mut items = Vec::new();
    for item in &sx.items()[2..] {
        if item.head() == Some("import") {
            let parts = item.items();
            let (l, c) = item.position();
            let malformed = || format!("{}:{}: an import is (import NAME) or (import NAME as ALIAS)", l, c);
            let module = parts.get(1).and_then(|p| p.symbol()).ok_or_else(malformed)?;
            let alias = match parts.len() {
                2 => None,
                4 if parts[2].symbol() == Some("as") => Some(parts[3].symbol().ok_or_else(malformed)?.to_string()),
                _ => return Err(malformed()),
            };
            imports.push((module.to_string(), alias));
        } else {
            items.push(item.clone());
        }
    }
    Ok(ModuleSx { name: name.to_string(), imports, items })
}

/// A module's name: the last segment of its import path
/// (`(import native/wasm_reader)` is the module `wasm_reader`).
pub fn module_name(import: &str) -> &str {
    import.rsplit('/').next().unwrap_or(import)
}

/// An import path is names separated by '/': relative, without `..`.
fn check_import_path(import: &str) -> Result<(), String> {
    let ok = !import.is_empty()
        && import.split('/').all(|seg| !seg.is_empty() && seg != "." && seg != ".." && !seg.contains('.'));
    if ok {
        Ok(())
    } else {
        Err(format!("'{}' is not an import path: use names separated by '/', e.g. (import native/wasm_reader)", import))
    }
}

fn resolve_import(st: &mut State, name: &str, importer_path: &Path) -> Result<(), String> {
    check_import_path(name).map_err(|e| format!("{}: {}", importer_path.display(), e))?;
    let file_path = find_module_file(name, importer_path, st.entry_path).map_err(|e| format!("{}: {}", importer_path.display(), e))?;
    let short = module_name(name).to_string();
    if let Some(other) = st.names.get(&short) {
        if *other != file_path {
            return Err(format!(
                "{}: two different modules are named '{}': {} and {}",
                importer_path.display(),
                short,
                other.display(),
                file_path.display()
            ));
        }
    }
    st.names.insert(short, file_path.clone());
    if st.included.contains(&file_path) {
        return Ok(());
    }
    if st.in_progress.contains(&file_path) {
        return Err(format!(
            "{}: Circular import detected: '{}' is imported while already being resolved",
            file_path.display(),
            file_path.display()
        ));
    }
    st.in_progress.insert(file_path.clone());
    let src = fs::read_to_string(&file_path).map_err(|e| format!("{}: Cannot read: {}", file_path.display(), e))?;
    let module = read_module(&src).map_err(|e| format!("{}: {}", file_path.display(), e))?;
    for (sub, _) in &module.imports {
        resolve_import(st, sub, &file_path)?;
    }
    emit_module(st, module, Some(module_name(name).to_string()), &file_path);
    st.in_progress.remove(&file_path);
    st.included.insert(file_path);
    Ok(())
}

/// The name of a `(fn NAME ...)` / `(struct NAME ...)` item, and whether it
/// is a generic template (`(fn (NAME T...) ...)`).
pub fn item_name(item: &Sx) -> Option<(&str, bool)> {
    let n = item.items().get(1)?;
    match n.symbol() {
        Some(s) => Some((s, false)),
        None if n.is_paren() => n.head().map(|h| (h, true)),
        None => None,
    }
}

/// Renames the module's items and appends them to the program.
fn emit_module(st: &mut State, module: ModuleSx, prefix: Option<String>, file: &Path) {
    let mut names = Names {
        prefix: prefix.clone(),
        fns: HashSet::new(),
        structs: HashSet::new(),
        decls: HashSet::new(),
        own_templates: HashSet::new(),
        aliases: module.imports.iter().filter_map(|(m, a)| a.clone().map(|a| (a, module_name(m).to_string()))).collect(),
        known_templates: &st.templates,
    };
    for item in &module.items {
        if let Some((n, generic)) = item_name(item) {
            match item.head() {
                Some("fn") => names.fns.insert(n.to_string()),
                Some("struct") => names.structs.insert(n.to_string()),
                Some("const" | "enum" | "union") => names.decls.insert(n.to_string()),
                _ => false,
            };
            if generic {
                names.own_templates.insert(n.to_string());
            }
        }
    }
    let mut new_templates = Vec::new();
    for t in &names.own_templates {
        new_templates.push(match &prefix {
            Some(p) => format!("{}.{}", p, t),
            None => t.clone(),
        });
    }
    let mut out_structs = Vec::new();
    let mut out_fns = Vec::new();
    for mut item in module.items {
        names.walk(&mut item, 0);
        let it = Item { sx: item, file: file.to_path_buf(), foreign: HashMap::new() };
        if matches!(it.sx.head(), Some("struct" | "const" | "enum" | "union")) {
            out_structs.push(it);
        } else {
            out_fns.push(it);
        }
    }
    st.structs.extend(out_structs);
    st.fns.extend(out_fns);
    st.templates.extend(new_templates);
}

/// The renaming rules (resolver.aipl `rename` and `child_role`). Roles: 0
/// none, 1 function name, 2 struct name, 3 field reference `S.f`.
/// Constants and enums are renamed wherever their names appear (a constant
/// may not double as a variable name, and enum names are types), as are
/// `alias.x` names in any position.
struct Names<'a> {
    prefix: Option<String>,
    fns: HashSet<String>,
    structs: HashSet<String>,
    /// The module's constants and enums.
    decls: HashSet<String>,
    own_templates: HashSet<String>,
    aliases: HashMap<String, String>,
    known_templates: &'a HashSet<String>,
}

impl Names<'_> {
    fn rename(&self, t: &str, role: u8) -> String {
        if role == 0 {
            return self.rename_decl(t);
        }
        if role == 3 {
            return match t.rfind('.') {
                Some(dot) => format!("{}{}", self.rename(&t[..dot], 2), &t[dot..]),
                None => t.to_string(),
            };
        }
        if let Some((alias, rest)) = t.split_once('.') {
            return match self.aliases.get(alias) {
                Some(canonical) => format!("{}.{}", canonical, rest),
                None => t.to_string(),
            };
        }
        let own = if role == 1 { &self.fns } else { &self.structs };
        // a struct-name position may also hold an enum (a template's type
        // argument: `(vec.Vec Color)`)
        match &self.prefix {
            Some(p) if own.contains(t) || (role == 2 && self.decls.contains(t)) => format!("{}.{}", p, t),
            _ => t.to_string(),
        }
    }

    /// A name in any position: one of this module's constants or enums
    /// (`MAX`, `Color`), an enum member (`Color.red`), or an alias-qualified
    /// name (`c.MAX`, `c.Color.red`). Anything else is unchanged.
    fn rename_decl(&self, t: &str) -> String {
        if let Some((alias, rest)) = t.split_once('.') {
            if let Some(canonical) = self.aliases.get(alias) {
                return format!("{}.{}", canonical, rest);
            }
        }
        let Some(p) = &self.prefix else { return t.to_string() };
        if self.decls.contains(t) {
            return format!("{}.{}", p, t);
        }
        match t.rfind('.') {
            Some(dot) if self.decls.contains(&t[..dot]) => format!("{}.{}", p, t),
            _ => t.to_string(),
        }
    }

    /// The canonical name of a group head that refers to a generic template:
    /// one of this module's own, or another module's through an alias or by
    /// its canonical name. None if `h` is not a template.
    fn template_ref(&self, h: &str) -> Option<String> {
        if self.own_templates.contains(h) {
            return Some(match &self.prefix {
                Some(p) => format!("{}.{}", p, h),
                None => h.to_string(),
            });
        }
        if let Some((alias, rest)) = h.split_once('.') {
            if let Some(canonical) = self.aliases.get(alias) {
                let c = format!("{}.{}", canonical, rest);
                if self.known_templates.contains(&c) {
                    return Some(c);
                }
            }
        }
        if self.known_templates.contains(h) {
            return Some(h.to_string());
        }
        None
    }

    fn child_role(head: Option<&str>, i: usize) -> u8 {
        match (head, i) {
            (Some("fn" | "call" | "ref"), 1) => 1,
            (Some("struct" | "new" | "sizeof" | "ptr" | "ptr.null" | "ptr.cast"), 1) => 2,
            (Some("get" | "put"), 2) => 3,
            _ => 0,
        }
    }

    fn walk(&self, sx: &mut Sx, role: u8) {
        match sx {
            Sx::Atom(Token { kind: TokenKind::Symbol(s), .. }) => {
                *s = self.rename(s, role);
            }
            Sx::Atom(_) => {}
            Sx::List { .. } => {
                let head = sx.head().map(|h| h.to_string());
                let template = head.as_deref().and_then(|h| self.template_ref(h));
                let items = sx.items_mut().unwrap();
                for (i, child) in items.iter_mut().enumerate() {
                    if i == 0 {
                        if let Some(t) = &template {
                            *child = Sx::symbol_at(t.clone(), child);
                            continue;
                        }
                        // the head of a (...) form is a keyword or an op
                        // (`mem.alloc`), never a renamed name
                        if head.is_some() {
                            continue;
                        }
                    }
                    // a match arm's head names a member of the matched type:
                    // (Shape.circle [r] ...) in module m is m.Shape.circle
                    if head.as_deref() == Some("match") && i >= 2 {
                        if let Some(arm) = child.items_mut() {
                            for (j, part) in arm.iter_mut().enumerate() {
                                match part {
                                    Sx::Atom(Token { kind: TokenKind::Symbol(s), .. }) if j == 0 && s != "else" => *s = self.rename(s, 0),
                                    _ if j == 0 => {}
                                    _ => self.walk(part, 0),
                                }
                            }
                            continue;
                        }
                    }
                    // a template's type arguments are struct names (or types
                    // built from them): `(alloc Pair)` in module m names m.Pair
                    let r = if template.is_some() { 2 } else { Names::child_role(head.as_deref(), i) };
                    self.walk(child, r);
                }
            }
        }
    }
}

/// Search order for `(import name)` (AIPL_SPEC.md section 11): the
/// importing file's directory, the entry file's directory, the standard
/// library (`aipl_src/std/` in this repository), then each directory of
/// the colon-separated `AIPL_PATH` environment variable. First match wins.
fn find_module_file(name: &str, importer_path: &Path, entry_path: &Path) -> Result<PathBuf, String> {
    let filename = format!("{}.aipl", name);
    let mut candidates = Vec::new();
    for p in [importer_path, entry_path] {
        if let Some(dir) = p.parent() {
            let c = dir.join(&filename);
            if !candidates.contains(&c) {
                candidates.push(c);
            }
        }
    }
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("aipl_src/std").join(&filename));
    if let Ok(path) = std::env::var("AIPL_PATH") {
        for dir in path.split(':').filter(|d| !d.is_empty()) {
            candidates.push(PathBuf::from(dir).join(&filename));
        }
    }
    for c in &candidates {
        if c.exists() {
            return c.canonicalize().map_err(|e| format!("Cannot resolve path '{}': {}", c.display(), e));
        }
    }
    Err(format!(
        "Cannot resolve import '{}': no '{}' found in {}",
        name,
        filename,
        candidates.iter().map(|c| format!("'{}'", c.display())).collect::<Vec<_>>().join(", ")
    ))
}
