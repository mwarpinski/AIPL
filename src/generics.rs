//! Generic expansion (AIPL_SPEC.md 4.H), the reference for
//! `aipl_src/generics.aipl`. Runs on the flat program after import
//! resolution, before type checking.
//!
//! A template is `(struct (Name T...) [fields])` or `(fn (name T...) ...)`.
//! A use names every type argument: `(Name i32)` in a type, `(new (Name i32))`,
//! `(get p (Name i32) field)`, `(call (name i32) args...)`, `(ref (name i32))`.
//! Each distinct instantiation becomes an ordinary item named
//! `Name<arg,...>` (e.g. `Vec<i32>`, `Map<i32,ptr<Point>>`), made by
//! substituting the arguments for the parameters; `(get p (Name i32) f)`
//! becomes `(get p Name<i32>.f)`. Templates themselves are dropped, so the
//! checker and the code generators only see concrete code.
//!
//! Order (both implementations must agree, for byte parity): the program's
//! items are walked in order (structs, then functions), then each instance
//! as it is created; each item depth-first, a node's children before the
//! node. Instances are numbered in the order they are first met. The output
//! is the concrete structs, the struct instances, the concrete functions,
//! then the function instances.

use crate::parser::{Token, TokenKind};
use crate::resolver::{item_name, FlatProgram, Item};
use crate::sexpr::Sx;
use std::collections::{HashMap, HashSet};

/// More distinct instantiations than this, or an instance name longer than
/// MAX_NAME, means unbounded recursion such as
/// `(fn (f T) ... (call (f (ptr T)) ...))`, which nests deeper each time.
const MAX_INSTANCES: usize = 4096;
const MAX_NAME: usize = 1024;
const RUNAWAY: &str = "a generic probably instantiates itself with ever larger types";

struct Template {
    params: Vec<String>,
    item: Item,
    is_struct: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Struct,
    Fn,
    StructInstance,
    FnInstance,
}

pub fn expand(prog: FlatProgram) -> Result<FlatProgram, String> {
    let files = prog.files;
    let mut templates: HashMap<String, Template> = HashMap::new();
    let mut work: Vec<(Item, Kind)> = Vec::new();
    for (items, is_struct) in [(prog.structs, true), (prog.fns, false)] {
        for item in items {
            match generic_header(&item)? {
                Some((name, params)) => {
                    if templates.contains_key(&name) {
                        return Err(at(&item.file, &item.sx, &format!("generic '{}' is defined twice", name)));
                    }
                    templates.insert(name, Template { params, item, is_struct });
                }
                None => work.push((item, if is_struct { Kind::Struct } else { Kind::Fn })),
            }
        }
    }
    if templates.is_empty() {
        return Ok(regroup(prog.name, files, work));
    }
    let mut made: HashSet<String> = HashSet::new();
    let mut i = 0;
    while i < work.len() {
        let file = work[i].0.file.clone();
        let foreign = std::mem::take(&mut work[i].0.foreign);
        let mut sx = std::mem::replace(&mut work[i].0.sx, Sx::Atom(blank()));
        let mut created = Vec::new();
        rewrite(&mut sx, &file, &foreign, &templates, &mut made, &mut created, false)?;
        work[i].0.foreign = foreign;
        work[i].0.sx = sx;
        work.extend(created);
        if made.len() > MAX_INSTANCES {
            return Err(format!("more than {} generic instances: {}", MAX_INSTANCES, RUNAWAY));
        }
        i += 1;
    }
    Ok(regroup(prog.name, files, work))
}

fn regroup(name: String, files: HashMap<std::path::PathBuf, String>, work: Vec<(Item, Kind)>) -> FlatProgram {
    let mut by_kind: [Vec<Item>; 4] = [vec![], vec![], vec![], vec![]];
    for (item, kind) in work {
        by_kind[kind as usize].push(item);
    }
    let [s, f, si, fi] = by_kind;
    FlatProgram { name, structs: s.into_iter().chain(si).collect(), fns: f.into_iter().chain(fi).collect(), files }
}

/// Built-in forms and types: a template with one of these names would be
/// indistinguishable from the form itself, e.g. `(get p (Vec i32) len)`.
const BUILT_IN: &[&str] = &["module", "import", "fn", "struct", "let", "set!", "if", "while", "loop", "block", "call", "ref", "call_ref", "new", "get", "put", "sizeof", "ptr", "arr", "result", "ok", "err", "match_result", "return", "break", "continue", "cond", "else", "req", "ens", "inv", "+", "-", "*", "/", "%", "^", "eq", "neq", "lt", "lte", "gt", "gte", "and", "or", "not", "shl", "shr", "shru", "bitand", "bitor", "divu", "remu", "i32", "i64", "f32", "f64", "bool", "str", "void"];

/// `Some((name, params))` for a template item.
fn generic_header(item: &Item) -> Result<Option<(String, Vec<String>)>, String> {
    let Some((name, true)) = item_name(&item.sx) else { return Ok(None) };
    let head = &item.sx.items()[1];
    let base = name.rsplit('.').next().unwrap_or(name);
    if BUILT_IN.contains(&base) {
        return Err(at(&item.file, head, &format!("generic name '{}' is a built-in form; choose another name", base)));
    }
    let mut params = Vec::new();
    for p in &head.items()[1..] {
        match p.symbol() {
            Some(s) if s.chars().next().is_some_and(|c| c.is_ascii_uppercase()) => params.push(s.to_string()),
            _ => return Err(at(&item.file, p, "a type parameter is a name starting with an uppercase letter, e.g. T")),
        }
    }
    if params.is_empty() {
        return Err(at(&item.file, head, &format!("generic '{}' needs at least one type parameter", name)));
    }
    Ok(Some((name.to_string(), params)))
}

/// Rewrites `sx` bottom-up: every template application becomes the symbol of
/// its instance (creating the instance the first time), and the 3-part
/// generic field forms of get/put become `Instance.field`.
///
/// A struct template is applied anywhere; a function template only as a
/// callee (`callee`: child 1 of `call` or `ref`), so that `(make U.v x)`
/// stays a union constructor beside a generic named `make`.
fn rewrite(
    sx: &mut Sx,
    file: &std::path::Path,
    foreign: &HashMap<(u32, u32), std::path::PathBuf>,
    templates: &HashMap<String, Template>,
    made: &mut HashSet<String>,
    created: &mut Vec<(Item, Kind)>,
    callee: bool,
) -> Result<(), String> {
    let Sx::List { .. } = sx else { return Ok(()) };
    let head = sx.head().map(|h| h.to_string());
    let applies = |h: Option<&str>, callee: bool| h.and_then(|h| templates.get(h)).is_some_and(|t| t.is_struct || callee);
    let is_app = applies(head.as_deref(), callee);
    let calls = matches!(head.as_deref(), Some("call" | "ref"));
    let items = sx.items_mut().unwrap();
    // the head of an application is the template's name, not something to rewrite
    let start = if is_app { 1 } else { 0 };
    let mut generic_field = false;
    for (k, child) in items.iter_mut().enumerate().skip(start) {
        let child_callee = calls && k == 1;
        let was_app = applies(child.head(), false);
        rewrite(child, file, foreign, templates, made, created, child_callee)?;
        if k == 2 && was_app && matches!(head.as_deref(), Some("get" | "put")) {
            generic_field = true;
        }
    }
    // (get p (Name T) field) -> (get p Name<T>.field), and the same for put
    if generic_field {
        let field = items.get(3).and_then(|f| f.symbol()).map(|f| f.to_string());
        let inst = items[2].symbol().unwrap().to_string();
        match field {
            Some(f) if !f.contains('.') => {
                items[2] = Sx::symbol_at(format!("{}.{}", inst, f), &items[2]);
                items.remove(3);
            }
            _ => return Err(at(foreign.get(&items[2].position()).map(|p| p.as_path()).unwrap_or(file), &items[2], "a generic struct field is written (get p (Name T...) field)")),
        }
    }
    let Some(h) = head else { return Ok(()) };
    let Some(t) = templates.get(&h) else { return Ok(()) };
    if !is_app || !sx.is_paren() {
        return Ok(());
    }
    let args: Vec<Sx> = sx.items()[1..].to_vec();
    if args.len() != t.params.len() {
        return Err(at(
            foreign.get(&sx.position()).map(|p| p.as_path()).unwrap_or(file),
            sx,
            &format!("generic '{}' takes {} type argument(s) ({}), got {}", h, t.params.len(), t.params.join(" "), args.len()),
        ));
    }
    let named = args
        .iter()
        .map(|a| type_name(a).map_err(|e| format!("{}: {}", foreign.get(&a.position()).map(|p| p.as_path()).unwrap_or(file).display(), e)))
        .collect::<Result<Vec<_>, _>>()?;
    let name = format!("{}<{}>", h, named.join(","));
    if name.len() > MAX_NAME {
        return Err(at(foreign.get(&sx.position()).map(|p| p.as_path()).unwrap_or(file), sx, &format!("generic instance name longer than {} characters: {}", MAX_NAME, RUNAWAY)));
    }
    if made.insert(name.clone()) {
        let mut inst = t.item.sx.clone();
        let subst: HashMap<&str, &Sx> = t.params.iter().map(|p| p.as_str()).zip(args.iter()).collect();
        substitute(&mut inst, &subst);
        // the header (Name T...) becomes the instance's name
        let header = &inst.items()[1];
        let renamed = Sx::symbol_at(name.clone(), header);
        inst.items_mut().unwrap()[1] = renamed;
        // the arguments' tokens keep their positions, in the files they were written in
        let mut arg_files = HashMap::new();
        for a in &args {
            let mut toks = Vec::new();
            a.flatten(&mut toks);
            for t in toks {
                let pos = (t.line, t.col);
                arg_files.insert(pos, foreign.get(&pos).cloned().unwrap_or_else(|| file.to_path_buf()));
            }
        }
        created.push((Item { sx: inst, file: t.item.file.clone(), foreign: arg_files }, if t.is_struct { Kind::StructInstance } else { Kind::FnInstance }));
    }
    *sx = Sx::symbol_at(name, sx);
    Ok(())
}

/// Replaces each type parameter symbol with its argument.
fn substitute(sx: &mut Sx, subst: &HashMap<&str, &Sx>) {
    match sx {
        Sx::Atom(Token { kind: TokenKind::Symbol(s), .. }) => {
            if let Some(arg) = subst.get(s.as_str()) {
                *sx = (*arg).clone();
            }
        }
        Sx::Atom(_) => {}
        Sx::List { items, .. } => {
            for i in items {
                substitute(i, subst);
            }
        }
    }
}

/// The text of a type inside an instance name: `i32`, `ptr<Point>`,
/// `arr<i64>`, `result<i32,bool>`, `fn<i32,i32->i32>`, `Vec<i32>`.
fn type_name(sx: &Sx) -> Result<String, String> {
    if let Some(s) = sx.symbol() {
        return Ok(s.to_string());
    }
    let items = sx.items();
    match sx.head() {
        Some("fn") if items.len() == 4 => {
            let params = items[1].items().iter().map(type_name).collect::<Result<Vec<_>, _>>()?;
            Ok(format!("fn<{}->{}>", params.join(","), type_name(&items[3])?))
        }
        Some(h) if items.len() > 1 => {
            let args = items[1..].iter().map(type_name).collect::<Result<Vec<_>, _>>()?;
            Ok(format!("{}<{}>", h, args.join(",")))
        }
        _ => {
            let (l, c) = sx.position();
            Err(format!("{}:{}: not a type", l, c))
        }
    }
}

fn at(file: &std::path::Path, sx: &Sx, msg: &str) -> String {
    let (l, c) = sx.position();
    format!("{}: {}:{}: {}", file.display(), l, c, msg)
}

fn blank() -> Token {
    Token { kind: TokenKind::LParen, line: 0, col: 0 }
}
