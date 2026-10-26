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
use crate::parser::{nearest_name, Parser, Token, TokenKind};
use crate::sexpr::Sx;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub struct Resolver;

/// An item of the flat program and the file it came from (for diagnostics).
/// A generic instance is made from its template's file, but the type
/// arguments substituted into it were written elsewhere: `foreign` maps
/// those tokens' positions to their own files.
#[derive(Clone)]
pub struct Item {
    pub sx: Sx,
    pub file: PathBuf,
    pub foreign: HashMap<(u32, u32), PathBuf>,
}

/// The flat program before parsing: entry module name, then every struct
/// and every function (imports first, in resolution order).
#[derive(Clone)]
pub struct FlatProgram {
    pub name: String,
    pub structs: Vec<Item>,
    pub fns: Vec<Item>,
    /// Each file's name in compiled code's line table (AIPL_SPEC.md 6.5):
    /// the entry file by its file name, a module by its import path.
    pub files: HashMap<PathBuf, String>,
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
    files: HashMap<PathBuf, String>,
    vis: Vis,
}

/// What other modules may use (AIPL_SPEC.md 11): the canonical names of
/// `(pub)` items, each struct's fields with their levels, and which structs
/// are value structs.
#[derive(Default)]
struct Vis {
    /// every named item of every module (what exists, whether or not (pub))
    all: HashSet<String>,
    public: HashSet<String>,
    fields: HashMap<String, HashMap<String, Level>>,
    /// each struct's field names in declaration order, for the named forms
    /// (new S a [f:v ...]) and (make V [f:v ...])
    order: HashMap<String, Vec<String>>,
    values: HashSet<String>,
}

/// A field's visibility outside its module: none, `(read x:T)`, `(pub x:T)`.
#[derive(Clone, Copy, PartialEq, PartialOrd)]
enum Level {
    Private,
    Read,
    Pub,
}

/// A parsed `(module name items...)`.
struct ModuleSx {
    name: String,
    imports: Vec<(String, Option<String>)>,
    /// the modules its own `(import ...)` lines name (not those the
    /// compiler adds for `new`, the formatting forms, or a test build)
    uses: HashSet<String>,
    /// a test build's `_start`, which the compiler writes, is in items
    generated_start: bool,
    items: Vec<Sx>,
    /// its `(test "name" ...)` items: each name and where it is
    tests: Vec<TestInfo>,
}

/// One `(test "name" body...)` of a module: its name, line, and column.
#[derive(Debug, Clone, PartialEq)]
pub struct TestInfo {
    pub name: String,
    pub line: u32,
    pub col: u32,
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

    /// `resolve_source` for a resolver's own output (the flat program
    /// aipl_src/resolver.aipl writes, which the tests compare by resolving
    /// it again): it holds the positional `(make V v ...)` the resolver
    /// writes for a value struct's named make, which source may not.
    pub fn resolve_flat(flat: &str, entry_path: &Path) -> Result<Module, String> {
        let (src, path) = (flat.to_string(), entry_path.to_path_buf());
        std::thread::Builder::new()
            .stack_size(256 * 1024 * 1024)
            .spawn(move || {
                READING_FLAT.with(|f| f.set(true));
                Self::resolve_on_this_thread(&src, &path)
            })
            .map_err(|e| e.to_string())?
            .join()
            .map_err(|_| "the resolver thread panicked".to_string())?
    }

    /// A test build of the file at `entry_path` (AIPL_SPEC.md 4.L): its
    /// tests are the functions `__test_0`, `__test_1`, ..., and its `_start`
    /// runs the one its first argument names and exits with that test's
    /// failure count (std/testing); with each test's name and place.
    pub fn resolve_tests(entry_path: &Path) -> Result<(Module, Vec<TestInfo>), String> {
        let src = fs::read_to_string(entry_path).map_err(|e| format!("{}: Cannot read: {}", entry_path.display(), e))?;
        let tests = read_module(&src, &file_name(entry_path), false).map_err(|e| format!("{}: {}", entry_path.display(), e))?.tests;
        let path = entry_path.to_path_buf();
        let module = std::thread::Builder::new()
            .stack_size(256 * 1024 * 1024)
            .spawn(move || Self::resolve_with(&src, &path, true))
            .map_err(|e| e.to_string())?
            .join()
            .map_err(|_| "the resolver thread panicked".to_string())??;
        Ok((module, tests))
    }

    /// The `(test ...)` items of the module in `src` (at `path`), in order.
    pub fn test_names(src: &str, path: &Path) -> Result<Vec<TestInfo>, String> {
        Ok(read_module(src, &file_name(path), false).map_err(|e| format!("{}: {}", path.display(), e))?.tests)
    }

    fn resolve_on_this_thread(entry_src: &str, entry_path: &Path) -> Result<Module, String> {
        Self::resolve_with(entry_src, entry_path, false)
    }

    fn resolve_with(entry_src: &str, entry_path: &Path, tests: bool) -> Result<Module, String> {
        let flat = Self::flatten_with(entry_src, entry_path, tests)?;
        // every template checked once, at placeholders for its type
        // parameters (generics::archetypes); nothing is compiled from these
        if let Some(arch) = crate::generics::archetypes(flat.clone())? {
            let module = Self::parse_flat(crate::consts::expand(arch)?)?;
            crate::checker::TypeChecker::new().check_module(&module)?;
        }
        let flat = crate::generics::expand(flat)?;
        let mut module = Self::parse_flat(crate::consts::expand(flat)?)?;
        crate::formats::lower(&mut module);
        Ok(module)
    }

    /// Parses each item of an expanded flat program, with its positions.
    fn parse_flat(flat: FlatProgram) -> Result<Module, String> {
        let mut module = Module { name: flat.name.clone(), imports: vec![], structs: vec![], enums: vec![], unions: vec![], functions: vec![], sources: HashMap::new(), file_names: flat.files.clone() };
        let unions: HashSet<String> = flat
            .structs
            .iter()
            .filter(|i| i.sx.head() == Some("union"))
            .filter_map(|i| i.sx.items().get(1).and_then(|n| n.symbol()).map(String::from))
            .collect();
        let values: HashSet<String> = flat.structs.iter().filter(|i| is_value_struct(&i.sx)).filter_map(|i| i.sx.items().get(1).and_then(|n| n.symbol()).map(String::from)).collect();
        for item in flat.structs.iter().chain(flat.fns.iter()) {
            let parsed = parse_item(&flat.name, item, &unions, &values)?;
            let source = Source { file: item.file.clone(), foreign: item.foreign.clone() };
            let keys = parsed.structs.iter().map(|d| format!("struct {}", d.name))
                .chain(parsed.enums.iter().map(|d| format!("enum {}", d.name)))
                .chain(parsed.unions.iter().map(|d| format!("union {}", d.name)))
                .chain(parsed.functions.iter().map(|d| format!("fn {}", d.name)));
            for key in keys {
                module.sources.insert(key, source.clone());
            }
            module.structs.extend(parsed.structs);
            module.enums.extend(parsed.enums);
            module.unions.extend(parsed.unions);
            module.functions.extend(parsed.functions);
        }
        Ok(module)
    }

    /// Resolution without generic expansion or parsing.
    pub fn flatten(entry_src: &str, entry_path: &Path) -> Result<FlatProgram, String> {
        Self::flatten_with(entry_src, entry_path, false)
    }

    /// `flatten`, the entry's tests made functions when `tests` (resolve_tests).
    fn flatten_with(entry_src: &str, entry_path: &Path, tests: bool) -> Result<FlatProgram, String> {
        let entry = read_module(entry_src, &file_name(entry_path), tests).map_err(|e| format!("{}: {}", entry_path.display(), e))?;
        let mut st = State {
            entry_path,
            included: HashSet::new(),
            in_progress: HashSet::new(),
            structs: Vec::new(),
            fns: Vec::new(),
            templates: HashSet::new(),
            names: HashMap::new(),
            files: HashMap::new(),
            vis: Vis::default(),
        };
        let entry_name = entry_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        st.files.insert(entry_path.to_path_buf(), entry_name);
        for (name, _) in &entry.imports {
            resolve_import(&mut st, name, entry_path)?;
        }
        let name = entry.name.clone();
        emit_module(&mut st, entry, None, entry_path)?;
        Ok(FlatProgram { name, structs: st.structs, fns: st.fns, files: st.files })
    }
}

/// Parses one item as `(module NAME item)`, prefixing errors with its file.
/// Whether `sx` is `(struct NAME [fields] (value))` (NAME may be a generic
/// header).
pub(crate) fn is_value_struct(sx: &Sx) -> bool {
    sx.head() == Some("struct") && sx.items().get(3).is_some_and(|m| m.is_paren() && m.items().len() == 1 && m.items()[0].symbol() == Some("value"))
}

fn parse_item(module_name: &str, item: &Item, unions: &HashSet<String>, values: &HashSet<String>) -> Result<Module, String> {
    let (line, col) = item.sx.position();
    let tok = |kind| Token { kind, line, col };
    let mut tokens = vec![tok(TokenKind::LParen), tok(TokenKind::Symbol("module".into())), tok(TokenKind::Symbol(module_name.into()))];
    item.sx.flatten(&mut tokens);
    tokens.push(tok(TokenKind::RParen));
    Parser::parse_tokens_with(tokens, unions.clone(), values.clone()).map_err(|e| {
        // the error's own "L:C: " says which token, and so which file
        Source { file: item.file.clone(), foreign: item.foreign.clone() }.name_file(e)
    })
}

/// A path's last component, as dbg labels name a file.
fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Reads a module. Its `(test ...)` items are left out, or, when `tests`
/// (the entry of a test build), become functions and a `_start`
/// (test_functions).
fn read_module(src: &str, file: &str, tests: bool) -> Result<ModuleSx, String> {
    let sx = Sx::read_one(&Parser::tokenize(src)?)?;
    let (line, col) = sx.position();
    if sx.head() != Some("module") {
        return Err(format!("{}:{}: a file must be one (module NAME ...) form", line, col));
    }
    let name = sx.items().get(1).and_then(|n| n.symbol()).ok_or(format!("{}:{}: the module needs a name", line, col))?;
    let mut imports = Vec::new();
    let mut items = Vec::new();
    let mut test_items = Vec::new();
    let mut infos = Vec::new();
    for item in &sx.items()[2..] {
        if item.head() == Some("test") {
            let (l, c) = item.position();
            let name = match item.items().get(1) {
                Some(Sx::Atom(Token { kind: TokenKind::StringLit(s), .. })) => s.clone(),
                _ => return Err(format!("{}:{}: a test is (test \"name\" body...)", l, c)),
            };
            infos.push(TestInfo { name, line: l, col: c });
            test_items.push(item.clone());
            continue;
        }
        if let Some((l, c)) = item.find_head("expect") {
            return Err(format!("{}:{}: expect is only for a test's body: (test \"name\" ... (expect c))", l, c));
        }
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
    let uses: HashSet<String> = imports.iter().map(|(m, _)| module_name(m).to_string()).collect();
    let generated_start = tests && !test_items.is_empty();
    if generated_start {
        test_functions(&test_items, &mut imports, &mut items)?;
    }
    // when, for, variadic and/or, and new and arr.new (sugar.rs): new and
    // arr.new call std/alloc, which a module that uses them imports, after
    // its own imports (and a test build's). (std/alloc's own allocators,
    // which it imports, would make a cycle, reported as one.)
    let alloc: Result<String, String> = match name {
        "alloc" => Ok(String::new()),
        _ => Ok(match imports.iter().find(|(m, _)| module_name(m) == "alloc") {
            Some((_, Some(alias))) => format!("{alias}."),
            _ => "alloc.".to_string(),
        }),
    };
    let mut uses_new = false;
    for item in items.iter_mut() {
        crate::sugar::rewrite(item, &alloc, &mut uses_new, &|sx, msg| {
            let (l, c) = sx.position();
            format!("{}:{}: {}", l, c, msg)
        })?;
    }
    if uses_new && name != "alloc" && !imports.iter().any(|(m, _)| module_name(m) == "alloc") {
        imports.push(("alloc".to_string(), None));
    }
    // the formatting forms become std/show calls (formats.rs): a module that
    // uses one imports it, after its own imports; their strings are split
    // here, before names are qualified, so a placeholder's names are renamed
    // like the rest of the module's
    if crate::formats::uses_formats(&items) {
        if !imports.iter().any(|(m, _)| module_name(m) == "show") {
            imports.push(("show".to_string(), None));
        }
        for item in items.iter_mut() {
            crate::formats::split(item, src, file, &|sx, msg| {
                let (l, c) = sx.position();
                format!("{}:{}: {}", l, c, msg)
            })?;
        }
    }
    Ok(ModuleSx { name: name.to_string(), imports, uses, generated_start, items, tests: infos })
}

/// A test build's entry: test K as `(fn __test_K [] -> void body...)`, and
/// `(fn _start [] -> void ...)` running the one argument 1 names, then
/// exiting with its failure count; std/testing and std/os imported. The
/// generated tokens are all at the test's place (the `_start`'s at the first
/// test's), as aipl_src/resolver.aipl writes them.
fn test_functions(tests: &[Sx], imports: &mut Vec<(String, Option<String>)>, items: &mut Vec<Sx>) -> Result<(), String> {
    for m in ["testing", "os"] {
        if !imports.iter().any(|(i, _)| module_name(i) == m) {
            imports.push((m.to_string(), None));
        }
    }
    let at = tests[0].position();
    if let Some(start) = items.iter().find(|i| i.head() == Some("fn") && i.items().get(1).and_then(|n| n.symbol()) == Some("_start")) {
        let (l, c) = start.position();
        return Err(format!("{}:{}: a test build makes its own _start: test a module without one", l, c));
    }
    let read = |text: &str, pos: (u32, u32)| -> Result<Sx, String> {
        let mut sx = Sx::read_one(&Parser::tokenize(text)?)?;
        crate::formats::move_to(&mut sx, pos);
        Ok(sx)
    };
    let mut arms = String::new();
    for (k, t) in tests.iter().enumerate() {
        let mut f = read(&format!("(fn __test_{k} [] -> void)"), t.position())?;
        let body: Vec<Sx> = t.items()[2..].to_vec();
        let items_of = f.items_mut().ok_or("a test function")?;
        if body.is_empty() {
            items_of.push(read("(block)", t.position())?);
        }
        items_of.extend(body);
        items.push(f);
        arms.push_str(&format!(" ((eq k {k}) (call __test_{k}))"));
    }
    items.push(read(
        &format!("(fn _start [] -> void (let k:i32 (call os.arg_int 1 -1)) (cond{arms} (else (block))) (sys.exit (call testing.status (call testing.failures))))"),
        at,
    )?);
    Ok(())
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
    st.files.entry(file_path.clone()).or_insert_with(|| format!("{name}.aipl"));
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
    let module = read_module(&src, &file_name(&file_path), false).map_err(|e| format!("{}: {}", file_path.display(), e))?;
    for (sub, _) in &module.imports {
        resolve_import(st, sub, &file_path)?;
    }
    emit_module(st, module, Some(module_name(name).to_string()), &file_path)?;
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

/// Renames the module's items and appends them to the program, checking
/// that what it uses of other modules is theirs to give, and taking the
/// visibility marks out (nothing after the resolver sees them).
fn emit_module(st: &mut State, module: ModuleSx, prefix: Option<String>, file: &Path) -> Result<(), String> {
    let mut module = module;
    let canonical = |n: &str| match &prefix {
        Some(p) => format!("{}.{}", p, n),
        None => n.to_string(),
    };
    // (pub) and field marks: recorded, then removed
    let mut own_pub: HashSet<String> = HashSet::new();
    let mut own_levels: HashMap<String, HashMap<String, Level>> = HashMap::new();
    for item in module.items.iter_mut() {
        let public = take_pub(item, file)?;
        let named = item_name(item).map(|(n, _)| n.to_string());
        if let (Some(n), true) = (&named, item.head() != Some("import")) {
            st.vis.all.insert(canonical(n));
        }
        if item.head() == Some("struct") {
            let levels = take_field_marks(item, file)?;
            // with the marks out, the list is name : type, three by three
            let order: Vec<String> = item.items().get(2).map(|l| l.items().iter().step_by(3).filter_map(|e| e.symbol().map(str::to_string)).collect()).unwrap_or_default();
            if let Some(n) = &named {
                st.vis.order.insert(canonical(n), order);
                if is_value_struct(item) {
                    st.vis.values.insert(canonical(n));
                }
                st.vis.fields.insert(canonical(n), levels.clone());
                own_levels.insert(n.clone(), levels);
            }
        }
        if let (true, Some(n)) = (public, named) {
            st.vis.public.insert(canonical(&n));
            own_pub.insert(n);
        }
    }
    if prefix.is_some() {
        check_exposure(&module.items, &own_pub, &own_levels, file)?;
    }
    let modules: HashSet<String> = st.names.keys().cloned().collect();
    let mut names = Names {
        prefix: prefix.clone(),
        fns: HashSet::new(),
        structs: HashSet::new(),
        decls: HashSet::new(),
        own_templates: HashSet::new(),
        aliases: module.imports.iter().filter_map(|(m, a)| a.clone().map(|a| (a, module_name(m).to_string()))).collect(),
        known_templates: &st.templates,
        module: prefix.clone().unwrap_or_else(|| module.name.clone()),
        modules,
        uses: module.uses.clone(),
        generated: std::cell::Cell::new(false),
        vis: &st.vis,
        file: file.to_path_buf(),
        err: std::cell::RefCell::new(None),
    };
    for item in &module.items {
        if let Some((n, generic)) = item_name(item) {
            match item.head() {
                Some("fn") => names.fns.insert(n.to_string()),
                // a value struct is also named where types and makes go
                Some("struct") if is_value_struct(item) => names.structs.insert(n.to_string()) | names.decls.insert(n.to_string()),
                Some("struct") => names.structs.insert(n.to_string()),
                Some("const" | "enum" | "union") => names.decls.insert(n.to_string()),
                _ => false,
            };
            // an interface is named like a template, (interface (Hash K) ...);
            // (impl (Hash i32) ...) only refers to one
            if generic && item.head() != Some("impl") {
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
        // a test build's _start is the compiler's, not the file's
        names.generated.set(module.generated_start && item.head() == Some("fn") && item.items().get(1).and_then(|n| n.symbol()) == Some("_start"));
        names.walk(&mut item, 0);
        if let Some(e) = names.err.borrow_mut().take() {
            return Err(e);
        }
        let it = Item { sx: item, file: file.to_path_buf(), foreign: HashMap::new() };
        if matches!(it.sx.head(), Some("struct" | "const" | "enum" | "union" | "interface")) {
            out_structs.push(it);
        } else {
            out_fns.push(it);
        }
    }
    st.structs.extend(out_structs);
    st.fns.extend(out_fns);
    st.templates.extend(new_templates);
    Ok(())
}

const PUB_PLACE: &str = "(pub) comes right after the signature: (fn f [params] -> type (pub) ...), (struct S [fields] (pub)), (struct S [fields] (value) (pub)), (union U [variants] (pub)), (enum E [members] (pub)), (const NAME:type value (pub)), (interface (Name T) (pub) ...)";

/// Takes an item's `(pub)` out; whether it had one, where it belongs.
fn take_pub(item: &mut Sx, file: &Path) -> Result<bool, String> {
    let head = item.head().map(str::to_string);
    // a value struct's (value) is part of its signature
    let value = head.as_deref() == Some("struct") && is_value_struct(item);
    let place = match head.as_deref() {
        Some("fn" | "const") => Some(5),
        Some("struct") if value => Some(4),
        Some("struct" | "union" | "enum") => Some(3),
        Some("interface") => Some(2),
        _ => None,
    };
    let is_pub = |p: &Sx| p.is_paren() && p.items().len() == 1 && p.head() == Some("pub");
    let parts = match item.items_mut() {
        Some(p) => p,
        None => return Ok(false),
    };
    let mut found = false;
    // (struct S [fields] (pub) (value)): (value) is part of the signature
    let is_value_mark = |p: &Sx| p.is_paren() && p.items().len() == 1 && p.head() == Some("value");
    if head.as_deref() == Some("struct") && parts.get(3).is_some_and(is_pub) && parts.get(4).is_some_and(is_value_mark) {
        return Err(at_sx(file, &parts[3], PUB_PLACE));
    }
    if let Some(k) = place {
        if parts.get(k).is_some_and(is_pub) {
            parts.remove(k);
            found = true;
        }
    }
    if let Some(p) = parts.iter().skip(1).find(|p| is_pub(p)) {
        let msg = if head.as_deref() == Some("impl") {
            "an implementation takes no (pub): it is as visible as its interface and its type"
        } else {
            PUB_PLACE
        };
        return Err(at_sx(file, p, msg));
    }
    Ok(found)
}

/// A struct's fields with their levels, its `(read x:T)` and `(pub x:T)`
/// made plain `x:T`.
fn take_field_marks(item: &mut Sx, file: &Path) -> Result<HashMap<String, Level>, String> {
    let mut levels = HashMap::new();
    let Some(list) = item.items_mut().and_then(|p| p.get_mut(2)) else { return Ok(levels) };
    let Some(entries) = list.items_mut() else { return Ok(levels) };
    let mut plain = Vec::new();
    let mut pending = Level::Private;
    let mut k = 0;
    for e in std::mem::take(entries) {
        match e.head() {
            Some(h @ ("pub" | "read")) => {
                let parts = e.items();
                let ok = parts.len() == 4 && parts[1].symbol().is_some() && matches!(&parts[2], Sx::Atom(Token { kind: TokenKind::Colon, .. }));
                if !ok || k % 3 != 0 {
                    return Err(at_sx(file, &e, "a field is written x:T, (read x:T), or (pub x:T)"));
                }
                let level = if h == "pub" { Level::Pub } else { Level::Read };
                levels.insert(parts[1].symbol().unwrap().to_string(), level);
                plain.extend(parts[1..].iter().cloned());
                k += 3;
            }
            _ => {
                if k % 3 == 0 {
                    if let Some(n) = e.symbol() {
                        levels.entry(n.to_string()).or_insert(pending);
                    }
                }
                plain.push(e);
                k += 1;
            }
        }
        pending = Level::Private;
    }
    *entries = plain;
    Ok(levels)
}

/// A (pub) item may not show a private type of its module in what it
/// gives others: a function's parameters and result, a struct's readable
/// fields, a union's variants, an interface's signatures.
fn check_exposure(items: &[Sx], own_pub: &HashSet<String>, own_levels: &HashMap<String, HashMap<String, Level>>, file: &Path) -> Result<(), String> {
    let own_types: HashSet<String> = items
        .iter()
        .filter(|i| matches!(i.head(), Some("struct" | "union" | "enum")))
        .filter_map(|i| item_name(i).map(|(n, _)| n.to_string()))
        .collect();
    for item in items {
        let Some((name, _)) = item_name(item) else { continue };
        if !own_pub.contains(name) {
            continue;
        }
        let parts = item.items();
        let mut types: Vec<&Sx> = Vec::new();
        match item.head() {
            Some("fn") if parts.len() >= 5 => {
                types.extend(parts[2].items().chunks(3).filter_map(|c| c.get(2)));
                types.push(&parts[4]);
            }
            Some("struct") => {
                let levels = own_levels.get(name);
                for c in parts.get(2).map(|b| b.items()).unwrap_or(&[]).chunks(3) {
                    let readable = c[0].symbol().and_then(|f| levels.and_then(|l| l.get(f))).is_some_and(|l| *l != Level::Private);
                    if readable {
                        types.extend(c.get(2));
                    }
                }
            }
            Some("union") => {
                for v in parts.get(2).map(|b| b.items()).unwrap_or(&[]) {
                    types.extend(v.items().iter().skip(1).collect::<Vec<_>>().chunks(3).filter_map(|c| c.get(2).copied()));
                }
            }
            Some("interface") => {
                for f in parts.iter().skip(2).filter(|f| f.items().len() >= 5) {
                    types.extend(f.items()[2].items().chunks(3).filter_map(|c| c.get(2)));
                    types.push(&f.items()[4]);
                }
            }
            _ => {}
        }
        for t in types {
            if let Some(bad) = private_in(t, &own_types, own_pub) {
                let msg = format!("'{}' is (pub) but its signature uses '{}', which is private: mark '{}' (pub) too", name, bad.symbol().unwrap_or(""), bad.symbol().unwrap_or(""));
                return Err(at_sx(file, bad, &msg));
            }
        }
    }
    Ok(())
}

/// The first name in type `t` of one of this module's types that is not (pub).
fn private_in<'a>(t: &'a Sx, own_types: &HashSet<String>, own_pub: &HashSet<String>) -> Option<&'a Sx> {
    match t {
        Sx::Atom(_) => t.symbol().filter(|s| own_types.contains(*s) && !own_pub.contains(*s)).map(|_| t),
        Sx::List { items, .. } => {
            let head_bad = items.first().and_then(|h| h.symbol()).filter(|s| own_types.contains(*s) && !own_pub.contains(*s));
            if head_bad.is_some() {
                return items.first();
            }
            items.iter().skip(1).find_map(|i| private_in(i, own_types, own_pub))
        }
    }
}

fn at_sx(file: &Path, sx: &Sx, msg: &str) -> String {
    let (l, c) = sx.position();
    format!("{}: {}:{}: {}", file.display(), l, c, msg)
}

thread_local! {
    /// Set on the thread of Resolver::resolve_flat, which reads a resolver's
    /// output: forms only the resolvers write are accepted there.
    static READING_FLAT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
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
    /// This module's name, every module's name, and what they give others.
    module: String,
    modules: HashSet<String>,
    /// The modules this file imports by its own lines (ModuleSx uses), and
    /// whether the item being walked is one the compiler wrote.
    uses: HashSet<String>,
    generated: std::cell::Cell<bool>,
    vis: &'a Vis,
    file: PathBuf,
    /// The first use of something another module keeps private.
    err: std::cell::RefCell<Option<String>>,
}

impl Names<'_> {
    /// The module a canonical name belongs to, if it is another module;
    /// naming one this file does not import is an error (AIPL_SPEC.md 11:
    /// every module a file names, it imports, so a file shows what it
    /// depends on). The names `new` and `arr.new` are written as
    /// (alloc.create, alloc.array) and a test build's _start are the
    /// compiler's.
    fn foreign<'s>(&self, name: &'s str, at: (u32, u32)) -> Option<&'s str> {
        let (m, _) = name.split_once('.')?;
        let other = m != self.module && self.modules.contains(m);
        if other && !self.uses.contains(m) && !self.generated.get() && name != "alloc.create" && name != "alloc.array" {
            self.fail(at, format!("'{name}' is in module {m}, which this file does not import: add (import {m})"));
        }
        other.then_some(m)
    }

    fn fail(&self, at: (u32, u32), msg: String) {
        let mut e = self.err.borrow_mut();
        if e.is_none() {
            *e = Some(format!("{}: {}:{}: {}", self.file.display(), at.0, at.1, msg));
        }
    }

    /// `name` (canonical) may be used here: it is this module's, or (pub).
    fn check_item(&self, name: &str, at: (u32, u32)) {
        if let Some(m) = self.foreign(name, at) {
            if !self.vis.public.contains(name) {
                if self.vis.all.contains(name) {
                    self.fail(at, format!("'{}' is private to module {}: mark it (pub) there to use it here", name, m));
                } else {
                    let prefix = format!("{m}.");
                    let hint = match nearest_name(&name[prefix.len()..], self.vis.public.iter().filter_map(|p| p.strip_prefix(prefix.as_str()))) {
                        Some(near) => format!("did you mean {m}.{near}?"),
                        None => format!("aipl doc {m} lists what it has"),
                    };
                    self.fail(at, format!("'{}' is not an item of module {}: {}", name, m, hint));
                }
            }
        }
    }

    /// A constant, enum, union, or one of their members (`m.Color.red`):
    /// the declaration is the first two parts.
    fn check_decl(&self, name: &str, at: (u32, u32)) {
        if self.foreign(name, at).is_some() {
            let decl: Vec<&str> = name.splitn(3, '.').take(2).collect();
            self.check_item(&decl.join("."), at);
        }
    }

    /// Made outside its module (`new`, or a struct type argument, which
    /// lets a template make one): every field must be (pub).
    fn check_makes(&self, name: &str, at: (u32, u32)) {
        if let Some(m) = self.foreign(name, at) {
            if let Some(fields) = self.vis.fields.get(name) {
                if fields.values().any(|l| *l != Level::Pub) {
                    self.fail(at, format!("'{}' has fields private to module {}, so only {} can make one", name, m, m));
                }
            }
        }
    }

    /// Field `f` of struct `s` (canonical), read (get) or written (put).
    fn check_field(&self, s: &str, f: &str, put: bool, at: (u32, u32)) {
        let Some(m) = self.foreign(s, at) else { return };
        let level = self.vis.fields.get(s).and_then(|fs| fs.get(f)).copied().unwrap_or(Level::Private);
        if level == Level::Private {
            self.fail(at, format!("field '{}' of {} is private to module {}", f, s, m));
        } else if put && level == Level::Read {
            self.fail(at, format!("field '{}' of {} is read-only outside module {}", f, s, m));
        }
    }

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
        // an interface's function, (Hash.hash K): the interface's name, qualified
        if let Some((iface, f)) = h.rsplit_once('.') {
            if let Some(c) = self.template_ref(iface) {
                return Some(format!("{}.{}", c, f));
            }
        }
        None
    }

    /// Whether item k of a form with n items is a further step of a field
    /// path (the value a put or arr.set stores comes last).
    fn path_step(head: Option<&str>, k: usize, n: usize) -> bool {
        match head {
            Some("get") => k >= 3,
            Some("put") => k >= 3 && k + 1 < n,
            Some("arr.get") => k >= 4,
            Some("arr.set") => k >= 4 && k + 1 < n,
            _ => false,
        }
    }

    fn child_role(head: Option<&str>, i: usize) -> u8 {
        match (head, i) {
            (Some("fn" | "call" | "ref"), 1) => 1,
            (Some("struct" | "new" | "sizeof" | "ptr" | "ptr.null" | "ptr.cast" | "ptr.make"), 1) => 2,
            (Some("get" | "put" | "with"), 2) => 3,
            _ => 0,
        }
    }

    /// A struct made with its fields named, (new S a [f:v ...]) or
    /// (make V [f:v ...]) (LANGUAGE_DECISIONS.md item 4): every field once,
    /// in declaration order, or an error; then the form it is,
    /// (block (let _new_L_C:(ptr S) (call (alloc.create S) a)) (put _new_L_C S.f v) ... _new_L_C)
    /// or (make V v ...), at the form's place (aipl_src/resolver.aipl
    /// emit_named_fields alike). None for any other form.
    fn named_fields(&self, sx: &Sx) -> Option<Sx> {
        let items = sx.items();
        let bracket = |e: &Sx| matches!(e, Sx::List { open: Token { kind: TokenKind::LBracket, .. }, .. });
        let is_new = match sx.head() {
            Some("new") if items.len() == 4 && bracket(&items[3]) => true,
            Some("make") if items.len() == 3 && bracket(&items[2]) => false,
            _ => return None,
        };
        let at = sx.position();
        let form = if is_new { "new" } else { "make" };
        let list = &items[if is_new { 3 } else { 2 }];
        let entries = list.items();
        let colon = |e: &Sx| matches!(e, Sx::Atom(Token { kind: TokenKind::Colon, .. }));
        if !entries.len().is_multiple_of(3) || entries.chunks(3).any(|c| c[0].symbol().is_none() || !colon(&c[1])) {
            self.fail(list.position(), format!("{form}'s fields are given as name:value, each once and in order: ({form} S{} [x:1 y:2])", if is_new { " a" } else { "" }));
            return None;
        }
        let given: Vec<&str> = entries.chunks(3).map(|c| c[0].symbol().unwrap()).collect();
        // the struct, canonical: a name, or a generic one's template
        let s = &items[1];
        let canonical = match (s.symbol(), s.head()) {
            (Some(n), _) => Some(self.rename(n, 2)),
            (None, Some(h)) => self.template_ref(h),
            _ => None,
        };
        if let Some(order) = canonical.as_ref().and_then(|c| self.vis.order.get(c)) {
            if given != *order {
                self.fail(at, format!("{form} gives the fields [{}]; {}'s fields, each once and in order, are [{}]", given.join(" "), canonical.as_deref().unwrap_or(""), order.join(" ")));
                return None;
            }
        }
        let list_at = |items: Vec<Sx>| Sx::List { open: Token { kind: TokenKind::LParen, line: at.0, col: at.1 }, items, close: Token { kind: TokenKind::RParen, line: at.0, col: at.1 } };
        let sym = |t: &str| Sx::Atom(Token { kind: TokenKind::Symbol(t.to_string()), line: at.0, col: at.1 });
        let values = entries.chunks(3).map(|c| c[2].clone());
        if !is_new {
            let mut out = vec![items[0].clone(), s.clone()];
            out.extend(values);
            return Some(list_at(out));
        }
        let name = format!("_new_{}_{}", at.0, at.1);
        let make = list_at(vec![sym("call"), list_at(vec![sym("alloc.create"), s.clone()]), items[2].clone()]);
        let colon_tok = Sx::Atom(Token { kind: TokenKind::Colon, line: at.0, col: at.1 });
        let mut out = vec![sym("block"), list_at(vec![sym("let"), sym(&name), colon_tok, list_at(vec![sym("ptr"), s.clone()]), make])];
        for (f, v) in given.iter().zip(values) {
            out.push(match s.symbol() {
                Some(n) => list_at(vec![sym("put"), sym(&name), sym(&format!("{n}.{f}")), v]),
                // a generic struct's field: (put p (Name T) f v)
                None => list_at(vec![sym("put"), sym(&name), s.clone(), sym(f), v]),
            });
        }
        out.push(sym(&name));
        Some(list_at(out))
    }

    /// `(make V v ...)` of a value struct, its fields by position: refused
    /// as written in source (LANGUAGE_DECISIONS.md item 4); named_fields
    /// writes this form, after (aipl_src/resolver.aipl positional_make).
    fn positional_make(&self, sx: &Sx) {
        if READING_FLAT.with(|f| f.get()) {
            return;
        }
        let items = sx.items();
        if sx.head() != Some("make") || items.len() < 2 || matches!(items.last(), Some(Sx::List { open: Token { kind: TokenKind::LBracket, .. }, .. })) {
            return;
        }
        let s = &items[1];
        let canonical = match (s.symbol(), s.head()) {
            (Some(n), _) => Some(self.rename(n, 2)),
            (None, Some(h)) => self.template_ref(h),
            _ => None,
        };
        let Some(c) = canonical.filter(|c| self.vis.values.contains(c)) else { return };
        let fields: Vec<String> = self.vis.order.get(&c).map(|o| o.iter().map(|f| format!("{f}:v")).collect()).unwrap_or_default();
        self.fail(sx.position(), format!("a value struct's fields are given by name, each once and in order: (make {} [{}])", c, fields.join(" ")));
    }

    fn walk(&self, sx: &mut Sx, role: u8) {
        match self.named_fields(sx) {
            Some(r) => *sx = r,
            // (call (make T) ...) calls a generic function named make (std/vec's)
            None if role != 1 => self.positional_make(sx),
            None => {}
        }
        let at = sx.position();
        match sx {
            Sx::Atom(Token { kind: TokenKind::Symbol(s), .. }) => {
                *s = self.rename(s, role);
                match role {
                    1 | 2 => self.check_item(s, at),
                    // a field reference S.f is checked with its get or put
                    3 => {}
                    _ => self.check_decl(s, at),
                }
            }
            Sx::Atom(_) => {}
            Sx::List { .. } => {
                let head = sx.head().map(|h| h.to_string());
                // a template named like the make form (std/vec's make) is
                // named only as a function, after call or ref: anywhere else
                // (make ...) is the form
                let template = head.as_deref().filter(|h| *h != "make" || role == 1).and_then(|h| self.template_ref(h));
                let field_op = matches!(head.as_deref(), Some("get" | "put" | "with"));
                // (with v V.f x) makes a value with field f written
                let put = matches!(head.as_deref(), Some("put" | "with"));
                let n_items = sx.items().len();
                // the struct of a generic field form, (get p (Name T) f)
                let mut generic_struct: Option<String> = None;
                let items = sx.items_mut().unwrap();
                for (i, child) in items.iter_mut().enumerate() {
                    if i == 0 {
                        if let Some(t) = &template {
                            // a template, or an interface's function (Hash.hash T)
                            let item = if self.known_templates.contains(t) { t.as_str() } else { t.rsplit_once('.').map_or(t.as_str(), |(i, _)| i) };
                            self.check_item(item, child.position());
                            *child = Sx::symbol_at(t.clone(), child);
                            continue;
                        }
                        // the head of a (...) form is a keyword or an op
                        // (`mem.alloc`), never a renamed name
                        if head.is_some() {
                            continue;
                        }
                    }
                    // a union's variants and an enum's members, [(circle r:f64) ...]:
                    // the head of each is a member's name, even one a template
                    // of this module also has
                    if matches!(head.as_deref(), Some("union" | "enum")) && i == 2 && !child.is_paren() {
                        // anything but a [...] list is the parser's error to report
                        let Some(members) = child.items_mut() else { continue };
                        for member in members {
                            match member.items_mut() {
                                Some(parts) => {
                                    for part in parts.iter_mut().skip(1) {
                                        self.walk(part, 0);
                                    }
                                }
                                None => self.walk(member, 0),
                            }
                        }
                        continue;
                    }
                    // the functions of an interface or an implementation: their
                    // names are the interface's, not this module's. An
                    // implementation's are qualified with this module, so the
                    // expander can tell where it was written (the orphan rule).
                    if matches!(head.as_deref(), Some("interface" | "impl")) && i >= 2 && child.head() == Some("fn") {
                        let qualify = head.as_deref() == Some("impl");
                        for (j, part) in child.items_mut().unwrap().iter_mut().enumerate() {
                            match (j, part) {
                                (0, _) => {}
                                (1, Sx::Atom(Token { kind: TokenKind::Symbol(s), .. })) => {
                                    if let (true, Some(p)) = (qualify, &self.prefix) {
                                        *s = format!("{}.{}", p, s);
                                    }
                                }
                                (1, _) => {}
                                (j, part) => self.walk(part, Names::child_role(Some("fn"), j)),
                            }
                        }
                        continue;
                    }
                    // a match arm's head names a member of the matched type:
                    // (Shape.circle [r] ...) in module m is m.Shape.circle
                    if head.as_deref() == Some("match") && i >= 2 {
                        if let Some(arm) = child.items_mut() {
                            for (j, part) in arm.iter_mut().enumerate() {
                                let part_at = part.position();
                                match part {
                                    Sx::Atom(Token { kind: TokenKind::Symbol(s), .. }) if j == 0 && s != "else" => {
                                        *s = self.rename(s, 0);
                                        self.check_decl(s, part_at);
                                    }
                                    // a generic union's arm: ((Name T...) variant ...)
                                    Sx::List { .. } if j == 0 => self.walk(part, 0),
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
                    let child_at = child.position();
                    match (child.symbol(), i) {
                        // a struct type argument lets the template make one (with
                        // `new`); a value struct's does not (a value-kind argument)
                        (Some(n), _) if template.is_some() && !self.vis.values.contains(n) => self.check_makes(n, child_at),
                        (Some(n), 1) if head.as_deref() == Some("new") => self.check_makes(n, child_at),
                        // (make V ...) of a value struct (not std/vec's make
                        // called with a value struct, (call (make V) ...))
                        (Some(n), 1) if template.is_none() && head.as_deref() == Some("make") && self.vis.fields.contains_key(n) => self.check_makes(n, child_at),
                        // (get p S.f), (put p S.f v)
                        (Some(n), 2) if field_op => {
                            if let Some((st, f)) = n.rsplit_once('.') {
                                self.check_item(st, child_at);
                                self.check_field(st, f, put, child_at);
                            }
                        }
                        (None, 2) if field_op => generic_struct = child.head().map(str::to_string),
                        (Some(f), 3) if field_op && generic_struct.is_some() => {
                            if let Some(st) = &generic_struct {
                                self.check_field(st, f, put, child_at);
                            }
                        }
                        // a field path's further steps: (get p S.f V.g),
                        // (put p S.f V.g x), (arr.get V a i V.f), (arr.set V a i V.f x)
                        (Some(n), k) if Names::path_step(head.as_deref(), k, n_items) => {
                            if let Some((st, f)) = n.rsplit_once('.') {
                                self.check_field(st, f, put || head.as_deref() == Some("arr.set"), child_at);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

/// Search order for `(import name)` (AIPL_SPEC.md section 11): the
/// importing file's directory, the entry file's directory, the standard
/// library (`$AIPL_STD`, else the library's std/: src/library.rs; the
/// toolchain's own modules always the library's), then each directory of
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
    let std = if crate::library::contains(entry_path) { crate::library::dir().join("std") } else { crate::library::std_dir() };
    candidates.push(std.join(&filename));
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
