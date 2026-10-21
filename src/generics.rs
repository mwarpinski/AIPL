//! Generic expansion (AIPL_SPEC.md 4.H), the reference for
//! `aipl_src/generics.aipl`. Runs on the flat program after import
//! resolution, before type checking.
//!
//! A template is `(struct (Name T...) [fields])` or `(fn (name T...) ...)`.
//! A use names every type argument: `(Name i32)` in a type, `(new (Name i32))`,
//! `(get p (Name i32) field)`, `(call (name i32) args...)`, `(ref (name i32))`,
//! and for a generic union `(make (Name i32) variant args...)` and the match
//! arm `((Name i32) variant [binders] body...)`.
//! Each distinct instantiation becomes an ordinary item named
//! `Name<arg,...>` (e.g. `Vec<i32>`, `Map<i32,ptr<Point>>`), made by
//! substituting the arguments for the parameters; `(get p (Name i32) f)`
//! becomes `(get p Name<i32>.f)`, `(make (Name i32) v x)` becomes
//! `(make Name<i32>.v x)`, and the arm `((Name i32) v ...)` becomes
//! `(Name<i32>.v ...)`. Templates themselves are dropped, so the
//! checker and the code generators only see concrete code.
//!
//! Order (both implementations must agree, for byte parity): the program's
//! items are walked in order (structs, then functions), then each instance
//! as it is created; each item depth-first, a node's children before the
//! node. Instances are numbered in the order they are first met. The output
//! is the concrete structs, the struct instances, the concrete functions,
//! then the function instances.

use crate::checker::shown;
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
    /// The template without its where clauses.
    item: Item,
    is_struct: bool,
    /// `(where (Interface T))`: the clause, its interface, and T's index.
    wheres: Vec<(Sx, String, usize)>,
    /// Per parameter: a struct (`(where (struct T))`), or else a value.
    struct_kind: Vec<bool>,
    /// Per parameter: plain (`(where (plain T))`): a value holding no
    /// pointer or array, so threads may share it (std/par).
    plain_kind: Vec<bool>,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Struct,
    Fn,
    StructInstance,
    FnInstance,
}

pub fn expand(prog: FlatProgram) -> Result<FlatProgram, String> {
    expand_with(prog, false).map(|p| p.unwrap())
}

/// Every template made once at placeholders for its type parameters (an
/// enum with no members for a value, a struct with no fields for
/// `(where (struct T))`, named `'template.T`), with the functions its where
/// clauses grant declared, and every ordinary function cut to its signature:
/// checking this program checks each template once, for every type
/// argument its clauses allow (docs/design/CONSTRAINTS_PLAN.md, IC3).
/// Nothing is compiled from it. None when there are no templates.
pub fn archetypes(prog: FlatProgram) -> Result<Option<FlatProgram>, String> {
    expand_with(prog, true)
}

fn expand_with(prog: FlatProgram, archetypes: bool) -> Result<Option<FlatProgram>, String> {
    let files = prog.files;
    let mut templates: HashMap<String, Template> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut struct_names: HashSet<String> = HashSet::new();
    let mut value_names: HashSet<String> = HashSet::new();
    let mut defs: HashMap<String, Sx> = HashMap::new();
    let mut plain_names: HashSet<String> = HashSet::new();
    let mut ifaces = Ifaces::default();
    let mut work: Vec<(Item, Kind)> = Vec::new();
    for (items, is_struct) in [(prog.structs, true), (prog.fns, false)] {
        for item in items {
            // interfaces are with the structs, so every one is known before
            // the first implementation (with the functions)
            match item.sx.head() {
                Some("interface") => {
                    ifaces.add_interface(item)?;
                    continue;
                }
                Some("impl") => {
                    for f in ifaces.add_impl(&item)? {
                        work.push((if archetypes { stub(f) } else { f }, Kind::Fn));
                    }
                    continue;
                }
                _ => {}
            }
            match generic_header(&item)? {
                Some((name, params)) => {
                    if templates.contains_key(&name) {
                        return Err(at(&item.file, &item.sx, &format!("generic '{}' is defined twice", name)));
                    }
                    let mut item = item;
                    let (wheres, struct_kind, plain_kind) = take_wheres(&mut item, &params)?;
                    order.push(name.clone());
                    templates.insert(name, Template { params, item, is_struct, wheres, struct_kind, plain_kind });
                }
                None => {
                    if let Some(w) = misplaced_where(&item.sx, 2) {
                        return Err(at(&item.file, w, "only a generic function, struct, or union takes where clauses"));
                    }
                    if item.sx.head() == Some("struct") {
                        if let Some(n) = item.sx.items().get(1).and_then(|n| n.symbol()) {
                            // a value struct is a value type argument
                            if crate::resolver::is_value_struct(&item.sx) {
                                value_names.insert(n.to_string());
                                defs.insert(n.to_string(), item.sx.clone());
                            } else {
                                struct_names.insert(n.to_string());
                            }
                        }
                    }
                    // unions and enums, for plain type arguments
                    if matches!(item.sx.head(), Some("union" | "enum")) {
                        if let Some(n) = item.sx.items().get(1).and_then(|n| n.symbol()) {
                            defs.insert(n.to_string(), item.sx.clone());
                        }
                    }
                    let item = if archetypes && !is_struct { stub(item) } else { item };
                    work.push((item, if is_struct { Kind::Struct } else { Kind::Fn }))
                }
            }
        }
    }
    if archetypes && templates.is_empty() {
        return Ok(None);
    }
    if templates.is_empty() && ifaces.defs.is_empty() {
        return Ok(Some(regroup(prog.name, files, work)));
    }
    // the placeholders, and the functions each template's where clauses grant
    let mut apps = Vec::new();
    if archetypes {
        for name in &order {
            let t = &templates[name];
            let header = &t.item.sx.items()[1];
            let mut app = vec![Sx::symbol_at(name.clone(), header)];
            for (k, p) in t.params.iter().enumerate() {
                let sealed = format!("'{}.{}", name, p);
                let kind = if t.struct_kind[k] { "struct" } else { "enum" };
                if t.struct_kind[k] {
                    struct_names.insert(sealed.clone());
                }
                if t.plain_kind[k] {
                    plain_names.insert(sealed.clone());
                }
                let decl = list(header, vec![Sx::symbol_at(kind.to_string(), header), Sx::symbol_at(sealed.clone(), header), bracket(header, vec![])]);
                work.push((Item { sx: decl, file: t.item.file.clone(), foreign: HashMap::new() }, Kind::Struct));
                app.push(Sx::symbol_at(sealed, header));
            }
            for (_, iname, k) in &t.wheres {
                let Some(def) = ifaces.defs.get(iname) else { continue };
                let sealed = Sx::symbol_at(format!("'{}.{}", name, t.params[*k]), header);
                let key = format!("{}<{}>", iname, render(&sealed));
                if !ifaces.impls.insert(key.clone()) {
                    continue;
                }
                let subst: HashMap<&str, &Sx> = [(def.param.as_str(), &sealed)].into_iter().collect();
                for g in &def.fns {
                    work.push((Item { sx: declared(&format!("{}.{}", key, g.name), g, &subst, header), file: t.item.file.clone(), foreign: HashMap::new() }, Kind::Fn));
                }
            }
            apps.push((list(header, app), t.item.file.clone(), !t.is_struct));
        }
    }
    let known = Known { templates: &templates, ifaces: &ifaces, struct_names: &struct_names, value_names: &value_names, defs: &defs, plain_names: &plain_names };
    let mut made = Made::default();
    for (mut app, file, callee) in apps {
        let mut created = Vec::new();
        rewrite(&mut app, &file, &HashMap::new(), &known, &mut made, &mut created, callee)?;
        work.extend(created);
    }
    let mut i = 0;
    while i < work.len() {
        let file = work[i].0.file.clone();
        let foreign = std::mem::take(&mut work[i].0.foreign);
        let mut sx = std::mem::replace(&mut work[i].0.sx, Sx::Atom(blank()));
        let mut created = Vec::new();
        rewrite(&mut sx, &file, &foreign, &known, &mut made, &mut created, false)?;
        work[i].0.foreign = foreign;
        work[i].0.sx = sx;
        work.extend(created);
        if made.names.len() > MAX_INSTANCES {
            return Err(format!("more than {} generic instances: {}", MAX_INSTANCES, RUNAWAY));
        }
        i += 1;
    }
    Ok(Some(regroup(prog.name, files, work)))
}

/// Instances made so far, and which of them are structs.
#[derive(Default)]
struct Made {
    names: HashSet<String>,
    structs: HashSet<String>,
    /// Instances of generic value structs.
    values: HashSet<String>,
    /// Instances of generic value structs and unions, by name (is_plain).
    defs: HashMap<String, Sx>,
}

/// A `(...)` list at the position of `at`.
fn list(at: &Sx, items: Vec<Sx>) -> Sx {
    let (line, col) = at.position();
    Sx::List { open: Token { kind: TokenKind::LParen, line, col }, items, close: Token { kind: TokenKind::RParen, line, col } }
}

fn bracket(at: &Sx, items: Vec<Sx>) -> Sx {
    let (line, col) = at.position();
    Sx::List { open: Token { kind: TokenKind::LBracket, line, col }, items, close: Token { kind: TokenKind::RBracket, line, col } }
}

/// `(fn name [params] -> R (call name params...))`: a function with the
/// signature `sig` (its type parameter put in by `subst`) that checks for
/// any caller and is never run.
fn declared(name: &str, sig: &Sig, subst: &HashMap<&str, &Sx>, at: &Sx) -> Sx {
    let put_in = |t: &Sx| {
        let mut t = t.clone();
        substitute(&mut t, subst);
        t
    };
    let colon = |at: &Sx| {
        let (line, col) = at.position();
        Sx::Atom(Token { kind: TokenKind::Colon, line, col })
    };
    let mut params = Vec::new();
    let mut call = vec![Sx::symbol_at("call".into(), at), Sx::symbol_at(name.into(), at)];
    for (n, t) in &sig.params {
        params.extend([Sx::symbol_at(n.clone(), at), colon(at), put_in(t)]);
        call.push(Sx::symbol_at(n.clone(), at));
    }
    let (line, col) = at.position();
    list(at, vec![
        Sx::symbol_at("fn".into(), at),
        Sx::symbol_at(name.into(), at),
        bracket(at, params),
        Sx::Atom(Token { kind: TokenKind::Arrow, line, col }),
        put_in(&sig.ret),
        list(at, call),
    ])
}

/// An ordinary function cut to its signature (and its `(unsafe)` clause),
/// with a body that calls itself: the archetype check needs only what it
/// takes and returns. Anything not written as a function is left alone.
fn stub(mut item: Item) -> Item {
    let parts = item.sx.items();
    let ok = item.sx.head() == Some("fn")
        && parts.len() >= 5
        && parts[1].symbol().is_some()
        && matches!(&parts[2], Sx::List { open: Token { kind: TokenKind::LBracket, .. }, .. })
        && parts[2].items().len().is_multiple_of(3);
    if !ok {
        return item;
    }
    let name = parts[1].clone();
    let mut call = vec![Sx::symbol_at("call".into(), &name), name.clone()];
    for c in parts[2].items().chunks(3) {
        call.push(c[0].clone());
    }
    let unsafe_clause = parts.get(5).filter(|u| u.is_paren() && u.items().len() == 1 && u.head() == Some("unsafe")).cloned();
    let at = item.sx.clone();
    let parts = item.sx.items_mut().unwrap();
    parts.truncate(5);
    parts.extend(unsafe_clause);
    parts.push(list(&at, call));
    item
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
const BUILT_IN: &[&str] = &["module", "import", "fn", "struct", "let", "set!", "if", "while", "loop", "counted_loop", "block", "call", "ref", "call_ref", "new", "get", "put", "sizeof", "ptr", "arr", "result", "ok", "err", "match_result", "return", "break", "continue", "cond", "else", "req", "ens", "inv", "+", "-", "*", "/", "%", "^", "eq", "neq", "lt", "lte", "gt", "gte", "and", "or", "not", "shl", "shr", "shru", "bitand", "bitor", "divu", "remu", "i32", "i64", "f32", "f64", "bool", "str", "void"];

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
/// A generic written without its type arguments, where a model leaves them
/// out: `(call vec.push v x)`, `(get b Box.value)`, `(make option.Option.some
/// 5)`. The child that names it, and what to write instead (aipl_src/
/// generics.aipl check_bare_generic is the same).
fn bare_generic(sx: &Sx, templates: &HashMap<String, Template>) -> Option<(usize, String)> {
    let head = sx.head()?;
    let items = sx.items();
    let (k, kind) = match head {
        "call" | "ref" => (1, "fn"),
        "get" | "put" | "with" => (2, "struct"),
        "make" => (1, "union"),
        _ => return None,
    };
    let name = items.get(k)?.symbol()?;
    // a field or variant is Name.member: the template is the part before the last '.'
    let base = if kind == "fn" { name } else { name.rsplit_once('.')?.0 };
    let t = templates.get(base)?;
    let is_kind = match kind {
        "fn" => !t.is_struct,
        other => t.item.sx.head() == Some(other),
    };
    if !is_kind {
        return None;
    }
    let how = match head {
        "call" | "ref" => format!("({head} ({base} T) ...)"),
        "make" => format!("(make ({base} T) variant ...)"),
        _ => format!("({head} p ({base} T) field ...)"),
    };
    Some((k, format!("'{base}' is generic: name its types, as in {how}")))
}

/// A struct template is applied anywhere; a function template only as a
/// callee (`callee`: child 1 of `call` or `ref`), so that `(make U.v x)`
/// stays a union constructor beside a generic named `make`.
fn rewrite(
    sx: &mut Sx,
    file: &std::path::Path,
    foreign: &HashMap<(u32, u32), std::path::PathBuf>,
    known: &Known,
    made: &mut Made,
    created: &mut Vec<(Item, Kind)>,
    callee: bool,
) -> Result<(), String> {
    let Sx::List { .. } = sx else { return Ok(()) };
    let (templates, ifaces) = (known.templates, known.ifaces);
    let head = sx.head().map(|h| h.to_string());
    let applies = |h: Option<&str>, callee: bool| h.and_then(|h| templates.get(h)).is_some_and(|t| t.is_struct || callee);
    let is_app = applies(head.as_deref(), callee);
    let calls = matches!(head.as_deref(), Some("call" | "ref"));
    let paren = sx.is_paren();
    if paren {
        if let Some((k, msg)) = bare_generic(sx, templates) {
            let child = &sx.items()[k];
            return Err(at(foreign.get(&child.position()).map(|p| p.as_path()).unwrap_or(file), child, &msg));
        }
    }
    let items = sx.items_mut().unwrap();
    // the head of an application is the template's name, not something to rewrite
    let start = if is_app { 1 } else { 0 };
    let mut generic_field = false;
    // (make (Name T) v ...) and an arm ((Name T) v ...): the instance and the
    // variant become one name, Name<T>.v
    let mut variant_at = None;
    for (k, child) in items.iter_mut().enumerate().skip(start) {
        let child_callee = calls && k == 1;
        let was_app = applies(child.head(), false);
        // a union's variant follows it; a value struct's fields follow its make
        let union_app = child.head().and_then(|h| templates.get(h)).is_some_and(|t| t.item.sx.head() == Some("union"));
        rewrite(child, file, foreign, known, made, created, child_callee)?;
        if k == 2 && was_app && matches!(head.as_deref(), Some("get" | "put" | "with")) {
            generic_field = true;
        }
        if was_app && paren && ((k == 0 && head.is_none()) || (k == 1 && head.as_deref() == Some("make") && union_app)) {
            variant_at = Some(k);
        }
    }
    if let Some(k) = variant_at {
        let variant = items.get(k + 1).and_then(|v| v.symbol()).map(|v| v.to_string());
        let inst = items[k].symbol().unwrap().to_string();
        match variant {
            Some(v) if !v.contains('.') => {
                items[k] = Sx::symbol_at(format!("{}.{}", inst, v), &items[k]);
                items.remove(k + 1);
            }
            _ => {
                let form = if k == 0 { "a generic union arm is written ((Name T...) variant [binders] body...)" } else { "a generic union variant is written (make (Name T...) variant fields...)" };
                return Err(at(foreign.get(&items[k].position()).map(|p| p.as_path()).unwrap_or(file), &items[k], form));
            }
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
    // (call (Interface.function type) ...): the implementation's function
    if callee && paren {
        if let Some((iname, f)) = head.as_deref().and_then(|h| h.rsplit_once('.')) {
            if let Some(def) = ifaces.defs.get(iname) {
                let place = foreign.get(&sx.position()).map(|p| p.as_path()).unwrap_or(file);
                let args = &sx.items()[1..];
                if args.len() != 1 {
                    return Err(at(place, sx, "a call to an interface's function names one type: (call (Interface.function type) args...)"));
                }
                if !def.fns.iter().any(|g| g.name == f) {
                    return Err(at(place, sx, &format!("'{}' is not a function of interface {}", f, iname)));
                }
                let key = format!("{}<{}>", iname, type_name(&args[0]).map_err(|e| format!("{}: {}", place.display(), e))?);
                if !ifaces.impls.contains(&key) {
                    let written = format!("({} {})", iname, shown(&render(&args[0])));
                    let fix = match sealed_param(&args[0]) {
                        Some((owner, p)) => format!("add (where ({} {})) to {}", iname, p, owner),
                        None => format!("write (impl {} ...)", written),
                    };
                    return Err(at(place, sx, &format!("no implementation of {}: {}", written, fix)));
                }
                *sx = Sx::symbol_at(format!("{}.{}", key, f), sx);
                return Ok(());
            }
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
    if made.names.insert(name.clone()) {
        let place = foreign.get(&sx.position()).map(|p| p.as_path()).unwrap_or(file);
        // each type argument is of its parameter's kind: a struct for
        // (where (struct T)), else a value that can be stored
        for (k, arg) in args.iter().enumerate() {
            let (p, a) = (&t.params[k], shown(&render(arg)));
            let is_struct = arg.symbol().is_some_and(|s| known.struct_names.contains(s) || made.structs.contains(s));
            let msg = if t.struct_kind[k] {
                if is_struct {
                    continue;
                }
                match sealed_param(arg) {
                    Some((owner, q)) => format!("{}: {} must be a struct, so {} needs (where (struct {}))", shown(&name), p, owner, q),
                    None if arg.symbol().is_some_and(|s| known.value_names.contains(s) || made.values.contains(s)) => {
                        format!("{}: {} must be a struct used through a pointer (where (struct {})), and {} is a value struct, which has no address", shown(&name), p, p, a)
                    }
                    None => format!("{}: {} must be a struct (where (struct {})), and {} is not one", shown(&name), p, p, a),
                }
            } else if is_struct {
                format!("{}: {} is a value type, and {} is a struct, used only through a pointer: write (ptr {})", shown(&name), p, a, a)
            } else if arg.symbol() == Some("void") || arg.head() == Some("result") {
                format!("{}: {} is a value type, and {} cannot be stored", shown(&name), p, a)
            } else if t.plain_kind[k] && !is_plain(arg, known, &made.defs, &mut HashSet::new()) {
                match sealed_param(arg) {
                    Some((owner, q)) => format!("{}: {} must be plain, so {} needs (where (plain {}))", shown(&name), p, owner, q),
                    None => format!("{}: {} must be plain (where (plain {})), and {} may hold a pointer or an array", shown(&name), p, p, a),
                }
            } else {
                continue;
            };
            return Err(at(place, sx, &msg));
        }
        // the where clauses: each type argument has the implementations asked for
        for (clause, iname, k) in &t.wheres {
            if !ifaces.defs.contains_key(iname) {
                return Err(at(&t.item.file, clause, &format!("'{}' is not an interface", iname)));
            }
            let arg = &args[*k];
            let key = format!("{}<{}>", iname, named[*k]);
            if !ifaces.impls.contains(&key) {
                let wanted = format!("({} {})", iname, shown(&render(arg)));
                let fix = match sealed_param(arg) {
                    Some((owner, q)) => format!("add (where ({} {})) to {}", iname, q, owner),
                    None => format!("write (impl {} ...)", wanted),
                };
                return Err(at(place, sx, &format!("{} needs {}: {}", shown(&name), wanted, fix)));
            }
        }
        if t.is_struct && t.item.sx.head() == Some("struct") {
            if crate::resolver::is_value_struct(&t.item.sx) {
                made.values.insert(name.clone());
            } else {
                made.structs.insert(name.clone());
            }
        }
        let mut inst = t.item.sx.clone();
        let subst: HashMap<&str, &Sx> = t.params.iter().map(|p| p.as_str()).zip(args.iter()).collect();
        substitute(&mut inst, &subst);
        // the header (Name T...) becomes the instance's name
        let header = &inst.items()[1];
        let renamed = Sx::symbol_at(name.clone(), header);
        inst.items_mut().unwrap()[1] = renamed;
        let by_value = match inst.head() {
            Some("struct") => crate::resolver::is_value_struct(&inst),
            Some("union") => true,
            _ => false,
        };
        if by_value {
            made.defs.insert(name.clone(), inst.clone());
        }
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
        Some("fn") if items.len() == 4 || items.len() == 5 => {
            let params = items[1].items().iter().map(type_name).collect::<Result<Vec<_>, _>>()?;
            let promise = if items.len() == 5 { ",no_leak" } else { "" };
            Ok(format!("fn<{}->{}{}>", params.join(","), type_name(&items[3])?, promise))
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

// ---------------------------------------------------------------------------
// Interfaces and implementations (docs/design/CONSTRAINTS_PLAN.md). An
// interface `(interface (Hash K) (fn hash [k:K] -> i32) ...)` declares
// functions; an implementation `(impl (Hash i32) (fn hash [k:i32] -> i32
// body...) ...)` defines them for one type and becomes ordinary functions
// named `Hash<i32>.hash`, each with the interface's contracts before its own;
// `(call (Hash.hash i32) x)` calls one. A type has at most one implementation
// of an interface, written in the interface's module or the type's (the
// resolver qualifies an implementation's function names with its module).
// ---------------------------------------------------------------------------

/// What rewrite looks names up in.
struct Known<'a> {
    templates: &'a HashMap<String, Template>,
    ifaces: &'a Ifaces,
    /// Concrete structs and struct placeholders, for the kinds of type arguments.
    struct_names: &'a HashSet<String>,
    /// Concrete value structs (type arguments of the value kind).
    value_names: &'a HashSet<String>,
    /// Value structs, unions, and enums by name (is_plain).
    defs: &'a HashMap<String, Sx>,
    /// The placeholders of (where (plain T)) parameters.
    plain_names: &'a HashSet<String>,
}

#[derive(Default)]
struct Ifaces {
    defs: HashMap<String, Interface>,
    /// `Interface<type>` of every implementation.
    impls: HashSet<String>,
}

struct Interface {
    param: String,
    fns: Vec<Sig>,
    file: std::path::PathBuf,
}

/// A function's name, parameters, return type, and contracts.
struct Sig {
    name: String,
    params: Vec<(String, Sx)>,
    ret: Sx,
    contracts: Vec<Sx>,
}

const BUILT_IN_TYPES: &[&str] = &["i32", "i64", "f32", "f64", "bool", "str", "void"];

impl Ifaces {
    fn add_interface(&mut self, item: Item) -> Result<(), String> {
        let file = &item.file;
        let Some((name, params)) = generic_header(&item)? else {
            return Err(at(file, &item.sx, "an interface is written (interface (Name T) (fn name [params] -> type) ...)"));
        };
        let header = &item.sx.items()[1];
        if params.len() != 1 {
            return Err(at(file, header, "an interface takes one type parameter: (interface (Name T) ...)"));
        }
        if self.defs.contains_key(&name) {
            return Err(at(file, &item.sx, &format!("interface '{}' is defined twice", name)));
        }
        let members = &item.sx.items()[2..];
        if members.is_empty() {
            return Err(at(file, header, "an interface declares at least one function"));
        }
        let mut fns: Vec<Sig> = Vec::new();
        for f in members {
            let (sig, body) = parse_sig(f, file, "an interface's function is written (fn name [params] -> type contracts...)")?;
            if let Some(b) = f.items().get(body) {
                return Err(at(file, b, "an interface's function has no body: its implementations have one"));
            }
            if fns.iter().any(|g| g.name == sig.name) {
                return Err(at(file, &f.items()[1], &format!("'{}' is declared twice in interface {}", sig.name, name)));
            }
            fns.push(sig);
        }
        let param = params.into_iter().next().unwrap();
        self.defs.insert(name, Interface { param, fns, file: item.file.clone() });
        Ok(())
    }

    /// The functions an implementation becomes, after checking it.
    fn add_impl(&mut self, item: &Item) -> Result<Vec<Item>, String> {
        let file = &item.file;
        let form = "an implementation is written (impl (Interface type) (fn name [params] -> type body...) ...)";
        let header = item.sx.items().get(1).filter(|h| h.is_paren()).ok_or_else(|| at(file, &item.sx, form))?;
        let iname = header.head().ok_or_else(|| at(file, header, form))?;
        let def = self.defs.get(iname).ok_or_else(|| at(file, header, &format!("'{}' is not an interface", iname)))?;
        let args = &header.items()[1..];
        if args.len() != 1 {
            return Err(at(file, header, "an implementation names one type: (impl (Interface type) ...)"));
        }
        let ty = &args[0];
        let key = format!("{}<{}>", iname, type_name(ty).map_err(|e| format!("{}: {}", file.display(), e))?);
        if self.impls.contains(&key) {
            return Err(at(file, header, &format!("a second implementation of {}: a type has at most one implementation of each interface", key)));
        }
        let subst: HashMap<&str, &Sx> = [(def.param.as_str(), ty)].into_iter().collect();
        let mut out = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        let mut module = String::new();
        for f in &item.sx.items()[2..] {
            let (sig, _) = parse_sig(f, file, "an implementation's function is written (fn name [params] -> type body...)")?;
            let (m, base) = match sig.name.rsplit_once('.') {
                Some((m, b)) => (m.to_string(), b.to_string()),
                None => (String::new(), sig.name.clone()),
            };
            module = m;
            let name_sx = &f.items()[1];
            let Some(want) = def.fns.iter().find(|g| g.name == base) else {
                return Err(at(file, name_sx, &format!("'{}' is not a function of interface {}", base, iname)));
            };
            if seen.contains(&base) {
                return Err(at(file, name_sx, &format!("'{}' is defined twice in this implementation", base)));
            }
            seen.push(base.clone());
            // the interface's signature with the type put in
            let put_in = |t: &Sx| {
                let mut t = t.clone();
                substitute(&mut t, &subst);
                t
            };
            let params: Vec<(String, Sx)> = want.params.iter().map(|(n, t)| (n.clone(), put_in(t))).collect();
            let ret = put_in(&want.ret);
            let same_type = |a: &Sx, b: &Sx| matches!((type_name(a), type_name(b)), (Ok(x), Ok(y)) if x == y);
            let matches = sig.params.len() == params.len()
                && sig.params.iter().zip(&params).all(|((n1, t1), (n2, t2))| n1 == n2 && same_type(t1, t2))
                && same_type(&sig.ret, &ret);
            if !matches {
                let ps: Vec<String> = params.iter().map(|(n, t)| format!("{}:{}", n, render(t))).collect();
                return Err(at(file, f, &format!("'{}' must be declared as in {}: (fn {} [{}] -> {})", base, iname, base, ps.join(" "), render(&ret))));
            }
            // (fn Interface<type>.name [params] -> ret <the interface's contracts> <its own>...)
            let mut g = f.clone();
            let parts = g.items_mut().unwrap();
            parts[1] = Sx::symbol_at(format!("{}.{}", key, base), &parts[1]);
            let mut foreign = HashMap::new();
            for (k, c) in want.contracts.iter().enumerate() {
                if def.file != item.file {
                    let mut toks = Vec::new();
                    c.flatten(&mut toks);
                    for t in toks {
                        foreign.insert((t.line, t.col), def.file.clone());
                    }
                }
                parts.insert(5 + k, put_in(c));
            }
            out.push(Item { sx: g, file: item.file.clone(), foreign });
        }
        if let Some(missing) = def.fns.iter().find(|g| !seen.contains(&g.name)) {
            return Err(at(file, header, &format!("the implementation of {} is missing '{}'", key, missing.name)));
        }
        // the orphan rule
        let module_of = |n: &str| n.rsplit_once('.').map_or(String::new(), |(m, _)| m.to_string());
        let owner = type_owner(ty);
        if module != module_of(iname) && owner.as_deref().map(module_of) != Some(module) {
            let msg = match owner {
                Some(o) => format!("this implementation must be in the module that defines {} or the one that defines {}", iname, o),
                None => format!("this implementation must be in the module that defines {}, since {} is built in", iname, render(ty)),
            };
            return Err(at(file, header, &msg));
        }
        self.impls.insert(key);
        Ok(out)
    }
}

/// `(fn name [p:T ...] -> R contracts... body...)`: the signature, and the
/// index of the first body item.
fn parse_sig(f: &Sx, file: &std::path::Path, form: &str) -> Result<(Sig, usize), String> {
    let items = f.items();
    let bad = || at(file, f, form);
    if f.head() != Some("fn") || items.len() < 5 {
        return Err(bad());
    }
    let name = items[1].symbol().ok_or_else(bad)?.to_string();
    let bracket = matches!(&items[2], Sx::List { open: Token { kind: TokenKind::LBracket, .. }, .. });
    let arrow = matches!(&items[3], Sx::Atom(Token { kind: TokenKind::Arrow, .. }));
    let ps = items[2].items();
    if !bracket || !arrow || !ps.len().is_multiple_of(3) {
        return Err(bad());
    }
    let mut params = Vec::new();
    for c in ps.chunks(3) {
        match (c[0].symbol(), &c[1]) {
            (Some(n), Sx::Atom(Token { kind: TokenKind::Colon, .. })) => params.push((n.to_string(), c[2].clone())),
            _ => return Err(bad()),
        }
    }
    let mut k = 5;
    while k < items.len() && matches!(items[k].head(), Some("req" | "ens" | "inv")) {
        k += 1;
    }
    Ok((Sig { name, params, ret: items[4].clone(), contracts: items[5..k].to_vec() }, k))
}

/// The named type a type is about, whose module may implement interfaces
/// for it: `geo.Point` for `(ptr geo.Point)`, `vec.Vec` for `(vec.Vec i32)`;
/// none for a built-in type, a result, or a function type.
fn type_owner(ty: &Sx) -> Option<String> {
    if let Some(s) = ty.symbol() {
        return if BUILT_IN_TYPES.contains(&s) { None } else { Some(s.to_string()) };
    }
    match ty.head() {
        Some("ptr" | "arr") => ty.items().get(1).and_then(type_owner),
        Some("fn" | "result") | None => None,
        Some(h) => Some(h.to_string()),
    }
}

/// A form as AIPL source writes it: `(ptr geo.Point)`, `[k:i32]`.
fn render(sx: &Sx) -> String {
    match sx {
        Sx::Atom(t) => t.kind.to_string(),
        Sx::List { open, items, close } => {
            let mut s = open.kind.to_string();
            let mut prev_colon = true;
            for i in items {
                let colon = matches!(i, Sx::Atom(Token { kind: TokenKind::Colon, .. }));
                if !(prev_colon || colon) {
                    s.push(' ');
                }
                s += &render(i);
                prev_colon = colon;
            }
            s + &close.kind.to_string()
        }
    }
}

/// Takes a template's where clauses out of it: `(where (Interface T))`,
/// right after its signature (a function's return type, a struct's fields,
/// a union's variants), T one of its type parameters.
/// A template's interface where clauses (the clause, its interface, T's
/// index), and per parameter whether it is a struct.
type Wheres = (Vec<(Sx, String, usize)>, Vec<bool>, Vec<bool>);

fn take_wheres(item: &mut Item, params: &[String]) -> Result<Wheres, String> {
    let start = match item.sx.head() {
        Some("fn") => 5,
        Some("struct" | "union") => 3,
        _ => usize::MAX,
    };
    let file = item.file.clone();
    let parts = item.sx.items_mut().unwrap();
    let mut wheres = Vec::new();
    let mut struct_kind = vec![false; params.len()];
    let mut plain_kind = vec![false; params.len()];
    while start < parts.len() && parts[start].head() == Some("where") {
        let clause = parts.remove(start);
        let req = clause.items().get(1).filter(|_| clause.items().len() == 2);
        let found = req.filter(|r| r.is_paren() && r.items().len() == 2).and_then(|r| {
            let iname = r.head()?;
            let k = params.iter().position(|p| Some(p.as_str()) == r.items()[1].symbol())?;
            Some((iname.to_string(), k))
        });
        let Some((iname, k)) = found else {
            return Err(at(&file, &clause, "a where clause is written (where (Interface T)), (where (struct T)), or (where (plain T)), T a type parameter of this generic"));
        };
        // (where (struct T)): T is a struct, used through (ptr T)
        if iname == "struct" {
            struct_kind[k] = true;
        } else if iname == "plain" {
            // (where (plain T)): T holds no pointer or array
            plain_kind[k] = true;
        } else {
            wheres.push((clause, iname, k));
        }
    }
    if let Some(w) = misplaced_where(&item.sx, 2) {
        return Err(at(&file, w, "where clauses come right after the signature: after a function's return type, a struct's fields, or a union's variants"));
    }
    Ok((wheres, struct_kind, plain_kind))
}

/// Whether type `t` is plain: holds no pointer or array, at any depth, so
/// threads may share it. The scalars, `str` (immutable), function
/// references, enums, results of plain types, and value structs and unions
/// whose fields are plain (generic instances among them, `inst`), and the
/// placeholders of (where (plain T)) parameters; not a pointer or an array.
/// A union that holds itself is plain if its other fields are.
fn is_plain(t: &Sx, known: &Known, inst: &HashMap<String, Sx>, seen: &mut HashSet<String>) -> bool {
    if let Some(s) = t.symbol() {
        return match s {
            "i32" | "i64" | "f32" | "f64" | "bool" | "str" => true,
            _ if s.starts_with('\'') => known.plain_names.contains(s),
            _ => match known.defs.get(s).or_else(|| inst.get(s)) {
                Some(def) => !seen.insert(s.to_string()) || def_is_plain(def, known, inst, seen),
                None => false,
            },
        };
    }
    match t.head() {
        Some("fn") => true,
        Some("result") => t.items().iter().skip(1).all(|x| is_plain(x, known, inst, seen)),
        // a generic value struct or union applied, not yet made: its
        // definition with the arguments put in
        Some(h) => match known.templates.get(h) {
            Some(tm) if tm.is_struct && tm.params.len() + 1 == t.items().len() => {
                if !seen.insert(render(t)) {
                    return true;
                }
                let mut def = tm.item.sx.clone();
                let subst: HashMap<&str, &Sx> = tm.params.iter().map(|p| p.as_str()).zip(t.items()[1..].iter()).collect();
                substitute(&mut def, &subst);
                let by_value = match def.head() {
                    Some("struct") => crate::resolver::is_value_struct(&def),
                    Some("union") => true,
                    _ => false,
                };
                by_value && def_is_plain(&def, known, inst, seen)
            }
            _ => false,
        },
        _ => false,
    }
}

fn def_is_plain(def: &Sx, known: &Known, inst: &HashMap<String, Sx>, seen: &mut HashSet<String>) -> bool {
    let Some(body) = def.items().get(2) else { return false };
    match def.head() {
        Some("enum") => true,
        Some("struct") => field_types(body.items()).iter().all(|t| is_plain(t, known, inst, seen)),
        Some("union") => body
            .items()
            .iter()
            .all(|v| field_types(v.items().get(1..).unwrap_or(&[])).iter().all(|t| is_plain(t, known, inst, seen))),
        _ => false,
    }
}

/// The types in a field list, `name : type` each, or `(read name : type)` /
/// `(pub name : type)`.
fn field_types(items: &[Sx]) -> Vec<&Sx> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < items.len() {
        if items[i].is_paren() && matches!(items[i].head(), Some("read" | "pub")) {
            out.extend(items[i].items().get(3));
            i += 1;
        } else {
            out.extend(items.get(i + 2));
            i += 3;
        }
    }
    out
}

/// For a placeholder `'owner.T`: the template it belongs to, and T.
fn sealed_param(arg: &Sx) -> Option<(String, String)> {
    let s = arg.symbol()?.strip_prefix('\'')?;
    let (owner, p) = s.rsplit_once('.')?;
    Some((owner.to_string(), p.to_string()))
}

/// The first `(where ...)` among an item's parts from index `from`.
fn misplaced_where(sx: &Sx, from: usize) -> Option<&Sx> {
    sx.items().iter().skip(from).find(|p| p.head() == Some("where"))
}
