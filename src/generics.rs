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
                        work.push((f, Kind::Fn));
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
                    templates.insert(name, Template { params, item, is_struct });
                }
                None => work.push((item, if is_struct { Kind::Struct } else { Kind::Fn })),
            }
        }
    }
    if templates.is_empty() && ifaces.defs.is_empty() {
        return Ok(regroup(prog.name, files, work));
    }
    let mut made: HashSet<String> = HashSet::new();
    let mut i = 0;
    while i < work.len() {
        let file = work[i].0.file.clone();
        let foreign = std::mem::take(&mut work[i].0.foreign);
        let mut sx = std::mem::replace(&mut work[i].0.sx, Sx::Atom(blank()));
        let mut created = Vec::new();
        rewrite(&mut sx, &file, &foreign, &Known { templates: &templates, ifaces: &ifaces }, &mut made, &mut created, false)?;
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
    known: &Known,
    made: &mut HashSet<String>,
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
        rewrite(child, file, foreign, known, made, created, child_callee)?;
        if k == 2 && was_app && matches!(head.as_deref(), Some("get" | "put")) {
            generic_field = true;
        }
        if was_app && paren && ((k == 0 && head.is_none()) || (k == 1 && head.as_deref() == Some("make"))) {
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
                    let written = format!("({} {})", iname, render(&args[0]));
                    return Err(at(place, sx, &format!("no implementation of {}: write (impl {} ...)", written, written)));
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
