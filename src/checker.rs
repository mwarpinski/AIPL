use crate::ast::*;
use std::collections::HashMap;
use crate::parser::nearest_name;

/// A `match` subject's member: its name, and a union variant's fields.
type MatchMember<'a> = (String, Option<&'a Vec<StructField>>);

/// The program's structs, by name (what lays out a value struct held
/// inline), with its unions and which of them are values: what every
/// layout needs. It reads as the struct map.
#[derive(Clone, Default, Debug)]
pub struct Structs {
    defs: HashMap<String, StructDef>,
    pub unions: HashMap<String, UnionDef>,
    /// The unions held by value, with their layouts (AIPL_SPEC.md 4.J): the
    /// others are heap cells, used by address.
    pub by_value: HashMap<String, SumLayout>,
}

impl std::ops::Deref for Structs {
    type Target = HashMap<String, StructDef>;
    fn deref(&self) -> &Self::Target {
        &self.defs
    }
}

impl std::ops::DerefMut for Structs {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.defs
    }
}

/// A union held by value: its scalars (slots), a tag and then the slots its
/// variants share by kind, each variant's fields taking the first slots of
/// each kind it needs (i32-sized ones, then f32, i64, f64), so a variant of
/// one pointer and one of two `f64`s need one i32 slot and two f64 slots in
/// all. In memory the slots are laid out in that order, each aligned to its
/// size. A slot a variant does not use holds zero.
#[derive(Clone, Debug, PartialEq)]
pub struct SumLayout {
    /// Each slot's type (I32, F32, I64, or F64); slot 0 is the tag.
    pub slots: Vec<Type>,
    /// Each slot's offset in memory.
    pub offsets: Vec<u32>,
    pub size: usize,
    pub align: usize,
    /// Per variant: the slot of each scalar of its fields, in order.
    pub variants: Vec<Vec<u32>>,
}

/// The most scalars a value (a value struct, a union by value) may hold.
pub const MAX_SCALARS: usize = 8;

impl Structs {
    /// The program's structs and unions; decides which unions are values.
    pub fn new(structs: &[StructDef], unions: &[UnionDef]) -> Structs {
        let mut st = Structs {
            defs: structs.iter().map(|s| (s.name.clone(), s.clone())).collect(),
            unions: unions.iter().map(|u| (u.name.clone(), u.clone())).collect(),
            by_value: HashMap::new(),
        };
        st.by_value = union_layouts(&st);
        st
    }

    pub fn is_value_union(&self, name: &str) -> bool {
        self.by_value.contains_key(name)
    }

    /// The layout of `(result ok err)`, held by value (AIPL_SPEC.md 4.F): two
    /// variants, ok and err, sharing slots as a union's do; None when it
    /// needs more than MAX_SCALARS slots, which the checker refuses.
    pub fn result_layout(&self, ok: &Type, err: &Type) -> Option<SumLayout> {
        let l = self.result_sum(ok, err);
        (l.slots.len() <= MAX_SCALARS).then_some(l)
    }

    /// How many slots `(result ok err)` needs.
    pub fn result_slots(&self, ok: &Type, err: &Type) -> usize {
        self.result_sum(ok, err).slots.len()
    }

    fn result_sum(&self, ok: &Type, err: &Type) -> SumLayout {
        let side = |name: &str, t: &Type| Variant { name: name.to_string(), fields: vec![StructField { name: "v".to_string(), ty: t.clone() }] };
        sum_layout(&[side("ok", ok), side("err", err)], self)
    }

    /// The layout of a union or result type held by value, if it is one.
    pub fn sum_of(&self, t: &Type) -> Option<SumLayout> {
        match t {
            Type::Union(n) => self.by_value.get(n).cloned(),
            Type::ResultType(a, b) => self.result_layout(a, b),
            _ => None,
        }
    }
}

/// The kind of slot a scalar of type `t` takes: I32 (every 4-byte scalar
/// but f32: bool, enums, pointers, arrays, function references, unions and
/// results held by address), F32, I64, or F64.
/// A conversion's operand and result types.
pub fn conversion(op: &OpCode) -> (Type, Type) {
    use OpCode::*;
    match op {
        F64ConvertI64S | F64ReinterpretI64 => (Type::I64, Type::F64),
        F64Sqrt => (Type::F64, Type::F64),
        I64TruncF64S | I64ReinterpretF64 => (Type::F64, Type::I64),
        F32DemoteF64 => (Type::F64, Type::F32),
        F64PromoteF32 => (Type::F32, Type::F64),
        F32ConvertI32S | F32ReinterpretI32 => (Type::I32, Type::F32),
        F32ConvertI64S => (Type::I64, Type::F32),
        F64ConvertI32S => (Type::I32, Type::F64),
        I32TruncF32S | I32ReinterpretF32 => (Type::F32, Type::I32),
        I32TruncF64S => (Type::F64, Type::I32),
        I64TruncF32S => (Type::F32, Type::I64),
        _ => (Type::Void, Type::Void),
    }
}

/// Whether `e` always leaves its branch: a `return`, `break`, or `continue`,
/// a block ending in one, or an `if` whose branches both do. Such a branch
/// takes the other branches' type (`(if ok v (return (err:T e)))` is a T).
pub fn diverges(e: &Expr) -> bool {
    match e {
        Expr::Return { .. } | Expr::Break(_) | Expr::Continue(_) => true,
        Expr::Block(es, _) | Expr::Unsafe(es, _) => es.last().is_some_and(diverges),
        Expr::If { then_branch, else_branch, .. } => diverges(then_branch) && diverges(else_branch),
        _ => false,
    }
}

/// Whether `e` ends in a jump instruction of its own (a `return`, `break`,
/// or `continue`, last in any blocks around it). A branch that diverges
/// without one (it ends in an `if` whose branches both leave) is followed by
/// `unreachable` where its value is wanted, so the wasm stack checks.
pub fn ends_in_jump(e: &Expr) -> bool {
    match e {
        Expr::Return { .. } | Expr::Break(_) | Expr::Continue(_) => true,
        Expr::Block(es, _) | Expr::Unsafe(es, _) => es.last().is_some_and(ends_in_jump),
        _ => false,
    }
}

/// Whether a body (its last expression) always leaves (diverges).
pub fn body_diverges(body: &[Expr]) -> bool {
    body.last().is_some_and(diverges)
}

/// The type a value read from a field or element of type `t` has: an `i32`
/// for a storage type, else `t`.
pub fn value_of(t: &Type) -> Type {
    if matches!(t, Type::Narrow(_)) { Type::I32 } else { t.clone() }
}

pub fn slot_kind(t: &Type) -> Type {
    match t {
        Type::I64 | Type::Ptr(_) | Type::Array(_) => Type::I64,
        Type::F32 => Type::F32,
        Type::F64 => Type::F64,
        _ => Type::I32,
    }
}

/// The scalars a value of type `t` is, in order: a value struct's fields'
/// (flattened), a union by value's slots, else `t` itself.
pub fn scalars(t: &Type, st: &Structs) -> Vec<Type> {
    match t {
        Type::Struct(n) if st.get(n).is_some_and(|d| d.is_value) => st[n].fields.iter().flat_map(|f| scalars(&f.ty, st)).collect(),
        Type::Union(n) if st.by_value.contains_key(n) => st.by_value[n].slots.clone(),
        Type::ResultType(a, b) => st.result_layout(a, b).map(|l| l.slots).unwrap_or_else(|| vec![t.clone()]),
        Type::Void => vec![],
        _ => vec![t.clone()],
    }
}

/// Which unions are values and how they are laid out (AIPL_SPEC.md 4.J): a
/// union is a value unless it contains itself
/// (through other unions held inline), or it needs more than MAX_SCALARS
/// slots, counting the unions it holds that are values as their slots and
/// the others as one (each union is decided after the unions it holds).
fn union_layouts(st: &Structs) -> HashMap<String, SumLayout> {
    let reaches_itself = |start: &String| reaches(st, std::slice::from_ref(start), start);
    let mut names: Vec<&String> = st.unions.keys().collect();
    names.sort();
    let candidates: Vec<String> = names
        .iter()
        .filter(|n| !reaches_itself(n))
        .map(|n| n.to_string())
        .collect();
    fn decide(n: &str, candidates: &[String], st: &mut Structs, decided: &mut Vec<String>) {
        if decided.iter().any(|d| d == n) {
            return;
        }
        decided.push(n.to_string());
        let u = st.unions[n].clone();
        for v in &u.variants {
            for f in &v.fields {
                if let Type::Union(m) = &f.ty {
                    if candidates.contains(m) {
                        decide(m, candidates, st, decided);
                    }
                }
            }
        }
        let layout = sum_layout(&u.variants, st);
        if layout.slots.len() <= MAX_SCALARS {
            st.by_value.insert(n.to_string(), layout);
        }
    }
    let mut work = Structs { defs: st.defs.clone(), unions: st.unions.clone(), by_value: HashMap::new() };
    let mut decided = Vec::new();
    for n in &candidates {
        decide(n, &candidates, &mut work, &mut decided);
    }
    work.by_value
}

/// The unions u holds inline, in its variants' fields.
fn holds(u: &UnionDef) -> Vec<String> {
    u.variants.iter().flat_map(|v| v.fields.iter()).filter_map(|f| match &f.ty {
        Type::Union(n) => Some(n.clone()),
        _ => None,
    }).collect()
}

/// Whether `target` is among the unions `from` hold, directly or through
/// the unions they hold.
fn reaches(st: &Structs, from: &[String], target: &str) -> bool {
    let mut todo: Vec<String> = from.iter().filter_map(|n| st.unions.get(n)).flat_map(holds).collect();
    let mut seen: Vec<String> = Vec::new();
    while let Some(n) = todo.pop() {
        if n == target {
            return true;
        }
        if seen.contains(&n) {
            continue;
        }
        if let Some(u) = st.unions.get(&n) {
            todo.extend(holds(u));
        }
        seen.push(n);
    }
    false
}

/// Why union u cannot be held by value, if it cannot (AIPL_SPEC.md 4.J):
/// it contains itself (named by the first field that leads back to it), or
/// it needs more than MAX_SCALARS slots (named by its widest variant).
pub fn union_error(u: &UnionDef, st: &Structs) -> Option<String> {
    if st.by_value.contains_key(&u.name) {
        return None;
    }
    let name = shown(&u.name);
    for v in &u.variants {
        for f in &v.fields {
            if let Type::Union(m) = &f.ty {
                if *m == u.name || reaches(st, std::slice::from_ref(m), &u.name) {
                    return Some(format!(
                        "union '{name}' contains itself ({name}.{}'s field '{}' has type {}): a union is a value, so hold that field through a pointer, a (ptr S) to a struct that holds it",
                        v.name, f.name, shown(m)
                    ));
                }
            }
        }
    }
    let n = sum_layout(&u.variants, st).slots.len();
    let width = |v: &Variant| v.fields.iter().map(|f| scalars(&f.ty, st).len()).sum::<usize>();
    let widest = u.variants.iter().fold(&u.variants[0], |w, v| if width(v) > width(w) { v } else { w });
    Some(format!(
        "union '{name}' needs {n} slots, more than the {MAX_SCALARS} a value holds: move the fields of its widest variant, {name}.{} ({} slots), into a struct and hold a (ptr S)",
        widest.name,
        width(widest)
    ))
}

/// The layout of a sum of variants (each a list of fields) held by value.
pub fn sum_layout(variants: &[Variant], st: &Structs) -> SumLayout {
    let kinds = [Type::I32, Type::F32, Type::I64, Type::F64];
    let per_variant: Vec<Vec<Type>> = variants.iter().map(|v| v.fields.iter().flat_map(|f| scalars(&f.ty, st)).map(|t| slot_kind(&t)).collect()).collect();
    let counts: Vec<usize> = kinds.iter().map(|k| per_variant.iter().map(|v| v.iter().filter(|t| *t == k).count()).max().unwrap_or(0)).collect();
    let mut slots = vec![Type::I32];
    let mut base = Vec::new();
    for (k, n) in kinds.iter().zip(&counts) {
        base.push(slots.len() as u32);
        slots.extend(std::iter::repeat_n(k.clone(), *n));
    }
    let variants = per_variant
        .iter()
        .map(|v| {
            let mut used = [0u32; 4];
            v.iter()
                .map(|t| {
                    let k = kinds.iter().position(|x| x == t).unwrap_or(0);
                    used[k] += 1;
                    base[k] + used[k] - 1
                })
                .collect()
        })
        .collect();
    let (mut offset, mut align) = (0usize, 4usize);
    let mut offsets = Vec::new();
    for t in &slots {
        let a = if matches!(t, Type::I64 | Type::F64) { 8 } else { 4 };
        align = align.max(a);
        offset = (offset + a - 1) & !(a - 1);
        offsets.push(offset as u32);
        offset += a;
    }
    SumLayout { slots, offsets, size: (offset + align - 1) & !(align - 1), align, variants }
}

/// A type's size and alignment in memory. A value struct is laid out inline
/// (its fields in order, each aligned to its size; the whole rounded up to
/// its largest alignment).
pub fn type_size_and_align(ty: &Type, structs: &Structs) -> Result<(usize, usize), String> {
    layout_at(ty, structs, 0)
}

fn layout_at(ty: &Type, structs: &Structs, depth: usize) -> Result<(usize, usize), String> {
    match ty {
        Type::Union(n) if structs.by_value.contains_key(n) => Ok((structs.by_value[n].size, structs.by_value[n].align)),
        Type::ResultType(a, b) if structs.result_layout(a, b).is_some() => {
            let l = structs.result_layout(a, b).unwrap_or_else(|| unreachable!());
            Ok((l.size, l.align))
        }
        // a pointer or array: the address, then the generation (GENERATIONS_PLAN.md)
        Type::Ptr(_) | Type::Array(_) => Ok((8, 8)),
        Type::Narrow(n) => Ok((n.size(), n.size())),
        Type::I32 | Type::F32 | Type::Bool | Type::Str | Type::Fn(..) | Type::Enum(_) | Type::Union(_) => Ok((4, 4)),
        Type::I64 | Type::F64 => Ok((8, 8)),
        Type::Struct(name) if depth < 64 => match structs.get(name) {
            Some(def) if def.is_value => {
                let (mut offset, mut max_align) = (0, 1);
                for f in &def.fields {
                    let (s, a) = layout_at(&f.ty, structs, depth + 1)?;
                    max_align = max_align.max(a);
                    offset = (offset + a - 1) & !(a - 1);
                    offset += s;
                }
                Ok(((offset + max_align - 1) & !(max_align - 1), max_align))
            }
            _ => Err(format!("Unsupported type for memory layout: {}", ty)),
        },
        _ => Err(format!("Unsupported type for memory layout: {}", ty)),
    }
}

/// An ok or err names its result's other side (AIPL_SPEC.md 4.F).
const OK_NEEDS_TYPE: &str = "(ok v) must name its result's err type: write (ok:E v) for a (result T E)";
const ERR_NEEDS_TYPE: &str = "(err e) must name its result's ok type: write (err:T e) for a (result T E)";

/// A type's layout where the language puts it in memory (a field, an array
/// element, sizeof): never a result, which lives only in locals, parameters,
/// results, and other results (held by value or not, AIPL_SPEC.md 4.F).
fn memory_layout(ty: &Type, structs: &Structs) -> Result<(usize, usize), String> {
    if matches!(ty, Type::ResultType(..)) {
        return Err(format!("Unsupported type for memory layout: {}", ty));
    }
    type_size_and_align(ty, structs)
}

/// A result's payload is a value of any type but `void`.
fn check_result_payload(t: &Type, (l, c): (u32, u32)) -> Result<(), String> {
    if *t == Type::Void {
        return Err(format!("{}:{}: a result payload must be a value, got void", l, c));
    }
    Ok(())
}

/// A type parameter's placeholder, when a template is checked once with its
/// type parameters sealed (generics.rs archetypes): `'vec.index_of.T`, an
/// enum with no members (a value parameter) or a struct with no fields
/// (`(where (struct T))`).
pub fn is_sealed(name: &str) -> bool {
    name.starts_with('\'')
}

/// Text with every placeholder shown as the parameter it stands for:
/// `vec.Vec<'vec.index_of.T>` is `vec.Vec<T>`.
pub fn shown(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\'' {
            out.push(c);
            continue;
        }
        let mut name = String::new();
        while let Some(&n) = chars.peek() {
            if n.is_whitespace() || "<>,()[]:".contains(n) {
                break;
            }
            name.push(n);
            chars.next();
        }
        out.push_str(name.rsplit('.').next().unwrap_or(""));
    }
    out
}

/// What to add when the two operands of an arithmetic op have different types.
const BINARY_HINT: &str = "; both operands must have one type: convert one (i64.extend_s, f64.convert_i32_s, ...) or write the literal in that type (5i64, 1.0)";

/// "T is a type parameter that is not a struct", for a struct position.
fn not_a_struct((l, c): (u32, u32), name: &str) -> String {
    if is_sealed(name) {
        let p = shown(name);
        format!("{}:{}: {} is a type parameter that is not a struct: to use it as one, add (where (struct {}))", l, c, p, p)
    } else {
        format!("{}:{}: Unknown struct '{}'", l, c, name)
    }
}

/// The parameter a placeholder enum type stands for.
fn sealed(t: &Type) -> Option<String> {
    match t {
        Type::Enum(n) if is_sealed(n) => Some(shown(n)),
        _ => None,
    }
}

pub fn get_field_offset(def: &StructDef, field_name: &str, structs: &Structs) -> Result<(usize, Type), String> {
    let mut offset = 0;
    for f in &def.fields {
        let (s, a) = type_size_and_align(&f.ty, structs)?;
        offset = (offset + a - 1) & !(a - 1);
        if f.name == field_name {
            return Ok((offset, f.ty.clone()));
        }
        offset += s;
    }
    Err(format!("Struct '{}' has no field '{}'", shown(&def.name), field_name))
}

pub fn get_struct_size(def: &StructDef, structs: &Structs) -> Result<usize, String> {
    let mut offset = 0;
    let mut max_align = 1;
    for f in &def.fields {
        let (s, a) = type_size_and_align(&f.ty, structs)?;
        if a > max_align {
            max_align = a;
        }
        offset = (offset + a - 1) & !(a - 1);
        offset += s;
    }
    let total = (offset + max_align - 1) & !(max_align - 1);
    Ok(total)
}

#[derive(Debug, Clone)]
pub struct TypeChecker {
    fn_signatures: HashMap<String, (Vec<Type>, Type)>,
    struct_defs: Structs,
    enum_defs: HashMap<String, EnumDef>,
    union_defs: HashMap<String, UnionDef>,
    /// Number of while/loop bodies enclosing the expression being checked.
    loop_depth: std::cell::Cell<u32>,
    /// Return type of the function body being checked; None inside contracts.
    return_type: std::cell::RefCell<Option<Type>>,
    /// The `Module::sources` key of the item being checked, so an error can name its file.
    item: std::cell::RefCell<String>,
    /// Functions with the `(unsafe)` clause.
    unsafe_fns: std::collections::HashSet<String>,
    /// Whether the code being checked may use unchecked operations: inside an
    /// `(unsafe ...)` block (depth > 0) or an `(unsafe)` function.
    unsafe_depth: std::cell::Cell<u32>,
    in_unsafe_fn: std::cell::Cell<bool>,
    /// The type of each formatting piece's value, by the value's address:
    /// lowering the forms to std/show calls (formats::lower) needs them.
    formats: std::cell::RefCell<HashMap<usize, Type>>,
    /// Inside a `(defer ...)`, which may not return (it runs as the function does).
    in_defer: std::cell::Cell<bool>,
    /// Each checked function's body type, its last expression's: `aipl eval
    /// -e` reads its wrapper's to print the value (body_type).
    bodies: std::cell::RefCell<HashMap<String, Type>>,
    /// Which functions may make memory that is never freed, for the type of
    /// `(ref f)` (MEMORY_PLAN.md decision 9).
    leaks: Option<crate::leaks::Analysis>,
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeChecker {
    pub fn new() -> Self {
        TypeChecker {
            fn_signatures: HashMap::new(),
            struct_defs: Structs::default(),
            enum_defs: HashMap::new(),
            union_defs: HashMap::new(),
            loop_depth: std::cell::Cell::new(0),
            return_type: std::cell::RefCell::new(None),
            item: std::cell::RefCell::new(String::new()),
            unsafe_fns: std::collections::HashSet::new(),
            unsafe_depth: std::cell::Cell::new(0),
            in_unsafe_fn: std::cell::Cell::new(false),
            formats: std::cell::RefCell::new(HashMap::new()),
            in_defer: std::cell::Cell::new(false),
            bodies: std::cell::RefCell::new(HashMap::new()),
            leaks: None,
        }
    }

    /// A defer's statements, each checked, in an environment of their own.
    fn check_defer(&self, body: &[Expr], env: &mut HashMap<String, Type>) -> Result<(), String> {
        self.in_defer.set(true);
        let mut inner = env.clone();
        let r = body.iter().try_for_each(|e| self.infer_expr_type(e, &mut inner).map(|_| ()));
        self.in_defer.set(false);
        r
    }

    /// `(try r)`, `(try_opt o)`, `(try_or o e)`: the value they give, checked
    /// against the function's return type (where their early return goes).
    fn check_try(&self, op: &OpCode, args: &[Expr], env: &mut HashMap<String, Type>, l: u32, c: u32) -> Result<Type, String> {
        let want = if *op == OpCode::TryOr { 2 } else { 1 };
        if args.len() != want {
            return Err(format!(
                "{}:{}: {}",
                l,
                c,
                match op {
                    OpCode::Try => "try takes one result: (try (call f x))",
                    OpCode::TryOpt => "try_opt takes one option: (try_opt (call (map.find K V) m k))",
                    _ => "try_or takes an option and the error to return without a value: (try_or o err)",
                }
            ));
        }
        let t = self.infer_expr_type(&args[0], env)?;
        let ret = self.return_type.borrow().clone();
        let Some(ret) = ret else { return Err(format!("{}:{}: {} returns from the function, and a contract cannot", l, c, op)) };
        let option_of = |t: &Type| -> Option<Type> {
            let Type::Union(u) = t else { return None };
            if !u.starts_with("option.Option<") {
                return None;
            }
            let def = self.union_defs.get(u)?;
            def.variants.iter().find(|v| v.name == "some").and_then(|v| v.fields.first()).map(|f| f.ty.clone())
        };
        let value = match op {
            OpCode::Try => match &t {
                Type::ResultType(ok, _) => (**ok).clone(),
                _ => return Err(format!("{}:{}: try takes a (result T E), got {}", l, c, t)),
            },
            _ => option_of(&t).ok_or_else(|| format!("{}:{}: {} takes an (option.Option T), got {}", l, c, op, shown(&t.to_string())))?,
        };
        match op {
            OpCode::TryOpt => {
                if option_of(&ret).is_none() {
                    return Err(format!("{}:{}: try_opt returns none from the function, which returns {}, not an (option.Option T)", l, c, shown(&ret.to_string())));
                }
            }
            _ => {
                let err = if *op == OpCode::Try {
                    let Type::ResultType(_, e) = &t else { unreachable!() };
                    (**e).clone()
                } else {
                    self.infer_expr_type(&args[1], env)?
                };
                match &ret {
                    Type::ResultType(_, e) if **e == err => {}
                    Type::ResultType(_, e) => {
                        return Err(format!("{}:{}: {}'s error is {}, and the function's is {}: they must be one type", l, c, op, err, e));
                    }
                    _ => return Err(format!("{}:{}: {} returns an error from the function, which returns {}, not a (result T E)", l, c, op, shown(&ret.to_string()))),
                }
            }
        }
        self.formats.borrow_mut().insert(&args[0] as *const Expr as usize, t);
        Ok(value)
    }

    /// The type of every formatting piece's value in `module`, by the value's
    /// address, if the module checks (formats::lower).
    /// The type of the last expression of function `name`'s body, once
    /// check_module has checked it (a `void` function's included).
    pub fn body_type(&self, name: &str) -> Option<Type> {
        self.bodies.borrow().get(name).cloned()
    }

    pub fn format_types(module: &Module) -> Option<HashMap<usize, Type>> {
        let mut checker = TypeChecker::new();
        checker.check_module(module).ok()?;
        Some(checker.formats.into_inner())
    }

    /// Checks a module. Definitions and signatures stop at the first error
    /// (everything after depends on them); then every function body is
    /// checked, and each failing function gives its first error, one per
    /// line in source order. When the resolver recorded where items were
    /// written, each error is prefixed with its item's file.
    pub fn check_module(&mut self, module: &Module) -> Result<(), String> {
        self.check_items(module).map_err(|e| self.name_file(module, e))?;
        self.leaks = Some(crate::leaks::Analysis::new(module));
        let mut errors = Vec::new();
        for f in &module.functions {
            self.at_item("fn", &f.name);
            if let Err(e) = self.check_fn_def(f) {
                errors.push(self.name_file(module, e));
            }
        }
        if errors.is_empty() {
            // (no_leak) promises, once every body checks (docs/design/MEMORY_PLAN.md M3)
            for (name, e) in crate::leaks::check(module) {
                self.at_item("fn", &name);
                errors.push(self.name_file(module, e));
            }
        }
        if errors.is_empty() { Ok(()) } else { Err(errors.join("\n")) }
    }

    fn name_file(&self, module: &Module, e: String) -> String {
        match module.sources.get(&*self.item.borrow()) {
            Some(source) => source.name_file(e),
            None => e,
        }
    }

    fn at_item(&self, kind: &str, name: &str) {
        *self.item.borrow_mut() = format!("{} {}", kind, name);
    }

    /// Definitions and function signatures.
    fn check_items(&mut self, module: &Module) -> Result<(), String> {
        // Register enum definitions (AIPL_SPEC.md 4.I)
        for e in &module.enums {
            self.at_item("enum", &e.name);
            let (l, c) = e.span;
            if self.enum_defs.contains_key(&e.name) {
                return Err(format!("{}:{}: Duplicate enum definition '{}'", l, c, e.name));
            }
            if module.structs.iter().any(|s| s.name == e.name) {
                return Err(format!("{}:{}: '{}' is defined as both a struct and an enum", l, c, e.name));
            }
            if e.members.is_empty() && !is_sealed(&e.name) {
                return Err(format!("{}:{}: enum '{}' has no members", l, c, e.name));
            }
            for (i, (m, v)) in e.members.iter().enumerate() {
                if !m.starts_with(|ch: char| ch.is_ascii_lowercase() || ch == '_') {
                    return Err(format!("{}:{}: enum member '{}.{}' must start with a lowercase letter or '_'", l, c, e.name, m));
                }
                if let Some((other, _)) = e.members[..i].iter().find(|(o, _)| o == m) {
                    return Err(format!("{}:{}: enum '{}' has two members named '{}'", l, c, e.name, other));
                }
                if let Some((other, _)) = e.members[..i].iter().find(|(_, w)| w == v) {
                    return Err(format!("{}:{}: enum '{}': members '{}' and '{}' both have the value {}", l, c, e.name, other, m, v));
                }
            }
            self.enum_defs.insert(e.name.clone(), e.clone());
        }

        // Register struct definitions (all of them first: a value struct's
        // layout depends on the value structs in it)
        for s in &module.structs {
            self.at_item("struct", &s.name);
            if self.struct_defs.contains_key(&s.name) {
                return Err(format!(
                    "{}:{}: Duplicate struct definition '{}'",
                    s.span.0, s.span.1, s.name
                ));
            }
            for (i, f) in s.fields.iter().enumerate() {
                if s.fields[..i].iter().any(|g| g.name == f.name) {
                    return Err(format!("{}:{}: Duplicate field '{}' in struct '{}'", s.span.0, s.span.1, f.name, shown(&s.name)));
                }
            }
            self.struct_defs.insert(s.name.clone(), s.clone());
        }
        for s in module.structs.iter().filter(|s| s.is_value) {
            self.at_item("struct", &s.name);
            self.check_value_struct(s)?;
        }
        for s in &module.structs {
            self.at_item("struct", &s.name);
            for f in &s.fields {
                memory_layout(&f.ty, &self.struct_defs).map_err(|e| {
                    format!(
                        "{}:{}: Field '{}' in struct '{}': {}",
                        s.span.0, s.span.1, f.name, shown(&s.name), e
                    )
                })?;
            }
        }

        // Register union definitions (AIPL_SPEC.md 4.J)
        for u in &module.unions {
            self.at_item("union", &u.name);
            let (l, c) = u.span;
            if self.union_defs.contains_key(&u.name) {
                return Err(format!("{}:{}: Duplicate union definition '{}'", l, c, u.name));
            }
            if self.struct_defs.contains_key(&u.name) || self.enum_defs.contains_key(&u.name) {
                return Err(format!("{}:{}: '{}' is defined as a union and as a struct or enum", l, c, u.name));
            }
            if u.variants.is_empty() {
                return Err(format!("{}:{}: union '{}' has no variants", l, c, u.name));
            }
            for (i, v) in u.variants.iter().enumerate() {
                if !v.name.starts_with(|ch: char| ch.is_ascii_lowercase() || ch == '_') {
                    return Err(format!("{}:{}: union variant '{}.{}' must start with a lowercase letter or '_'", l, c, u.name, v.name));
                }
                if u.variants[..i].iter().any(|w| w.name == v.name) {
                    return Err(format!("{}:{}: union '{}' has two variants named '{}'", l, c, u.name, v.name));
                }
                for (j, f) in v.fields.iter().enumerate() {
                    if v.fields[..j].iter().any(|g| g.name == f.name) {
                        return Err(format!("{}:{}: variant '{}.{}' has two fields named '{}'", l, c, shown(&u.name), v.name, f.name));
                    }
                    memory_layout(&f.ty, &self.struct_defs)
                        .map_err(|e| format!("{}:{}: Field '{}' of variant '{}.{}': {}", l, c, f.name, u.name, v.name, e))?;
                }
            }
            self.union_defs.insert(u.name.clone(), u.clone());
        }
        // which unions are values, which a result's slots count
        self.struct_defs = Structs::new(&module.structs, &module.unions);

        // Field types may name structs and unions defined later in the module.
        for s in &module.structs {
            self.at_item("struct", &s.name);
            for f in &s.fields {
                // a storage type is a field's type only (not a value struct's)
                if matches!(f.ty, Type::Narrow(_)) && !s.is_value {
                    continue;
                }
                self.validate_type(&f.ty, s.span).map_err(|e| format!("{} (field '{}' of struct '{}')", e, f.name, shown(&s.name)))?;
            }
        }
        for u in &module.unions {
            self.at_item("union", &u.name);
            for v in &u.variants {
                for f in &v.fields {
                    self.validate_type(&f.ty, u.span)
                        .map_err(|e| format!("{} (field '{}' of variant '{}.{}')", e, f.name, shown(&u.name), v.name))?;
                }
            }
        }
        // every union is a value (AIPL_SPEC.md 4.J)
        for u in &module.unions {
            self.at_item("union", &u.name);
            if let Some(e) = union_error(u, &self.struct_defs) {
                return Err(format!("{}:{}: {e}", u.span.0, u.span.1));
            }
        }

        // First pass: register function signatures
        for f in &module.functions {
            self.at_item("fn", &f.name);
            if self.fn_signatures.contains_key(&f.name) {
                return Err(format!("{}:{}: Duplicate function definition '{}'", f.span.0, f.span.1, f.name));
            }
            // two parameters of one name: the VM read the last, compiled code the first
            if let Some(dup) = f.params.iter().enumerate().find_map(|(i, (n, _))| f.params[..i].iter().any(|(m, _)| m == n).then_some(n)) {
                return Err(format!("{}:{}: function '{}' has two parameters named '{}'", f.span.0, f.span.1, shown(&f.name), dup));
            }
            if f.is_unsafe {
                self.unsafe_fns.insert(f.name.clone());
            }
            let param_types: Vec<Type> = f.params.iter().map(|(_, t)| t.clone()).collect();
            self.fn_signatures
                .insert(f.name.clone(), (param_types, f.return_type.clone()));
        }

        Ok(())
    }

    /// A value struct's rules (AIPL_SPEC.md 4.E): at least one field; fields
    /// that are numbers, bools, enums, pointers, arrays, function references,
    /// or value structs; not containing itself; at most 8 scalars in all.
    fn check_value_struct(&self, s: &StructDef) -> Result<(), String> {
        let (l, c) = s.span;
        let name = shown(&s.name);
        if s.fields.is_empty() {
            return Err(format!("{}:{}: value struct '{}' has no fields", l, c, name));
        }
        for f in &s.fields {
            if matches!(f.ty, Type::Str | Type::Union(_) | Type::ResultType(_, _) | Type::Void | Type::Narrow(_)) {
                return Err(format!(
                    "{}:{}: Field '{}' of value struct '{}': a value struct holds numbers, bools, enums, pointers, arrays, function references, and value structs, not {}",
                    l, c, f.name, name, f.ty
                ));
            }
        }
        // through its value-struct fields, back to itself?
        let mut stack: Vec<(&str, String)> = s.fields.iter().filter_map(|f| self.value_field(&f.ty).map(|n| (n, f.name.clone()))).collect();
        let mut seen = std::collections::HashSet::new();
        while let Some((n, first)) = stack.pop() {
            if n == s.name {
                return Err(format!("{}:{}: value struct '{}' contains itself (through field '{}'): a value has a fixed size; hold it through a pointer", l, c, name, first));
            }
            if !seen.insert(n) {
                continue;
            }
            if let Some(d) = self.struct_defs.get(n) {
                for f in &d.fields {
                    if let Some(m) = self.value_field(&f.ty) {
                        stack.push((m, first.clone()));
                    }
                }
            }
        }
        let n = self.scalars(&Type::Struct(s.name.clone()));
        if n > 8 {
            return Err(format!(
                "{}:{}: value struct '{}' holds {} scalars, counting the value structs in it; at most 8: keep a larger record behind a pointer",
                l, c, name, n
            ));
        }
        Ok(())
    }

    /// Follows a put or arr.set path from field `first` (of type `ty`): each
    /// step is a field of the value struct the step before holds. Returns
    /// the last field and its type.
    fn walk_path(
        &self,
        first: (&String, &String),
        ty: Type,
        path: &[(String, String)],
        form: &str,
        (l, c): (u32, u32),
    ) -> Result<((String, String), Type), String> {
        let (mut at, mut ty) = ((first.0.clone(), first.1.clone()), ty);
        for (st, f) in path {
            if ty != Type::Struct(st.clone()) || !self.is_value(st) {
                return Err(format!(
                    "{}:{}: {} path: {}.{} is {}, not the value struct {}",
                    l, c, form, shown(&at.0), at.1, ty, shown(st)
                ));
            }
            let (_offset, next) = get_field_offset(&self.struct_defs[st], f, &self.struct_defs).map_err(|e| format!("{}:{}: {}", l, c, e))?;
            at = (st.clone(), f.clone());
            ty = next;
        }
        Ok((at, ty))
    }

    /// The value struct a field of type `t` holds inline, if any.
    fn value_field<'t>(&self, t: &'t Type) -> Option<&'t str> {
        match t {
            Type::Struct(n) if self.struct_defs.get(n).is_some_and(|d| d.is_value) => Some(n.as_str()),
            _ => None,
        }
    }

    /// How many scalars a value is, once its value structs are flattened.
    fn scalars(&self, t: &Type) -> usize {
        match t {
            Type::Struct(n) => match self.struct_defs.get(n) {
                Some(d) if d.is_value => d.fields.iter().map(|f| self.scalars(&f.ty)).sum(),
                _ => 1,
            },
            _ => 1,
        }
    }

    /// Whether `name` is a value struct.
    fn is_value(&self, name: &str) -> bool {
        self.struct_defs.get(name).is_some_and(|d| d.is_value)
    }

    /// Every type written in source must be well formed: `(ptr S)` names a
    /// known struct, `(arr T)` holds something with a memory layout, and a
    /// struct is never used by value.
    fn validate_type(&self, ty: &Type, span: (u32, u32)) -> Result<(), String> {
        match ty {
            Type::Narrow(n) => Err(format!(
                "{}:{}: {} is a storage type: only a struct field or an array element is one (reading it gives an i32); use i32",
                span.0, span.1, n.name()
            )),
            Type::Array(elem) if matches!(elem.as_ref(), Type::Narrow(_)) => Ok(()),
            Type::Ptr(inner) => match inner.as_ref() {
                Type::Struct(name) if self.is_value(name) => Err(format!(
                    "{}:{}: '{}' is a value struct, which has no address: write {}, not (ptr {})",
                    span.0, span.1, shown(name), shown(name), shown(name)
                )),
                Type::Struct(name) if self.struct_defs.contains_key(name) => Ok(()),
                Type::Struct(name) if is_sealed(name) => Err(not_a_struct(span, name)),
                Type::Struct(name) if self.struct_defs.unions.contains_key(name) => Err(format!(
                    "{}:{}: '{}' is a union, which is a value: write {}, not (ptr {}); a union is never null, so use a variant such as (none) for no value",
                    span.0, span.1, shown(name), shown(name), shown(name)
                )),
                Type::Struct(name) => Err(format!("{}:{}: Unknown struct '{}' in (ptr {})", span.0, span.1, name, name)),
                other => Err(format!("{}:{}: ptr must point to a struct, got {}", span.0, span.1, other)),
            },
            Type::Struct(name) if self.is_value(name) => Ok(()),
            Type::Struct(name) => Err(format!(
                "{}:{}: struct '{}' cannot be used by value; use (ptr {})",
                span.0, span.1, shown(name), shown(name)
            )),
            Type::Array(elem) => {
                self.validate_type(elem, span)?;
                memory_layout(elem, &self.struct_defs)
                    .map(|_| ())
                    .map_err(|_| format!("{}:{}: (arr T) element must be a scalar, (ptr S), or (arr T), got {}", span.0, span.1, elem))
            }
            Type::ResultType(a, b) => {
                self.validate_type(a, span)?;
                self.validate_type(b, span)?;
                self.result_fits(a, b, span)
            }
            Type::Fn(params, ret, _) => {
                for p in params {
                    self.validate_type(p, span)?;
                }
                self.validate_type(ret, span)
            }
            Type::Enum(name) if self.enum_defs.contains_key(name) => Ok(()),
            // only enum.cast names an enum type outside a type position
            Type::Enum(name) if self.union_defs.contains_key(name) => Err(format!(
                "{}:{}: '{}' is a union, not an enum: its values are made with (make {}.variant ...)",
                span.0, span.1, name, name
            )),
            Type::Union(name) if self.union_defs.contains_key(name) => Ok(()),
            Type::Union(name) => Err(format!("{}:{}: Unknown union '{}'", span.0, span.1, name)),
            Type::Enum(name) if self.struct_defs.contains_key(name) => Err(format!(
                "{}:{}: '{}' is a struct, which is only used through a pointer: write (ptr {})",
                span.0, span.1, shown(name), shown(name)
            )),
            Type::Enum(name) => Err(format!("{}:{}: Unknown type '{}' (not a scalar type or an enum)", span.0, span.1, name)),
            _ => Ok(()),
        }
    }

    /// Whether a value of type `got` fits where `want` is asked for: the
    /// same type, or a function reference that promises (no_leak) where one
    /// that does not is asked for (dropping a promise is always safe). Only
    /// at the top: inside an array or a pointer the types are the same.
    /// Where two branches' (or two compared values') types meet: the same type,
/// or two function types that differ only in the promise meet at the type
/// without it, which both fit (MEMORY_PLAN.md decision 9).
fn meet(a: &Type, b: &Type) -> Option<Type> {
    match (a, b) {
        _ if a == b => Some(a.clone()),
        (Type::Fn(p, r, _), Type::Fn(q, s, _)) if p == q && r == s => Some(Type::Fn(p.clone(), r.clone(), false)),
        _ => None,
    }
}

fn fits(got: &Type, want: &Type) -> bool {
        got == want || matches!((got, want), (Type::Fn(p, r, true), Type::Fn(q, s, false)) if p == q && r == s)
    }

    /// What to add to a mismatch's message when a function reference that
    /// does not promise (no_leak) is given where one that does is asked for:
    /// for (ref f), why f may keep memory.
    fn fit_why(&self, got: &Type, want: &Type, e: &Expr) -> String {
        if let (Type::Fn(p, r, false), Type::Fn(q, w, true)) = (got, want) {
            if p == q && r == w {
                return match (e, &self.leaks) {
                    (Expr::Ref { name, .. }, Some(a)) => format!(": {} may keep memory ({})", shown(name), a.chain(name)),
                    _ => ": this function value's type does not promise (no_leak)".to_string(),
                };
            }
        }
        match (got, want) {
            (Type::Ptr(_) | Type::Array(_), Type::I32) => format!(": a pointer or array is not an i32: declare it as {got}"),
            (Type::I32, Type::Ptr(inner)) => match inner.as_ref() {
                Type::Struct(n) => format!(": an i32 is not a pointer: for null write (ptr.null {})", shown(n)),
                _ => String::new(),
            },
            (Type::I32, Type::Array(elem)) => format!(": an i32 is not an array: for null write (arr.null {elem})"),
            (Type::I32, Type::I64) => ": no implicit widening: write a literal as 5i64, or convert with (i64.extend_s x)".to_string(),
            (Type::I64, Type::I32) => ": no implicit narrowing: convert with (i32.wrap x)".to_string(),
            (Type::I32 | Type::I64, Type::F64 | Type::F32) => format!(
                ": integers do not become floats by themselves: write a literal as 1.0, or convert with ({}.convert_{}_s x)",
                if *want == Type::F64 { "f64" } else { "f32" },
                if *got == Type::I32 { "i32" } else { "i64" }
            ),
            _ => String::new(),
        }
    }

    /// A result is held by value (AIPL_SPEC.md 4.F), so one needing more
    /// than MAX_SCALARS slots is refused.
    fn result_fits(&self, ok: &Type, err: &Type, (l, c): (u32, u32)) -> Result<(), String> {
        let n = self.struct_defs.result_slots(ok, err);
        if n <= MAX_SCALARS {
            return Ok(());
        }
        Err(format!(
            "{l}:{c}: {} needs {n} slots, more than the {MAX_SCALARS} a value holds: keep the larger side's data in a struct that is not (value) and make that side a (ptr S)",
            Type::ResultType(Box::new(ok.clone()), Box::new(err.clone()))
        ))
    }

    /// An op's operands: exactly `want.len()` of them, of those types.
    fn expect_operands(
        &self,
        name: &str,
        form: &str,
        args: &[Expr],
        want: &[Type],
        env: &mut HashMap<String, Type>,
        (l, c): (u32, u32),
    ) -> Result<(), String> {
        if args.len() != want.len() {
            return Err(format!("{}:{}: {} takes {} operands, {}; got {}", l, c, name, want.len(), form, args.len()));
        }
        for (i, (a, w)) in args.iter().zip(want).enumerate() {
            let t = self.infer_expr_type(a, env)?;
            if t != *w {
                return Err(format!("{}:{}: {} operand {} must be {}, got {}", l, c, name, i + 1, w, t));
            }
        }
        Ok(())
    }

    /// Checks a while/loop body with break/continue allowed inside it.
    fn check_loop_body(&self, body: &[Expr], env: &mut HashMap<String, Type>) -> Result<(), String> {
        self.loop_depth.set(self.loop_depth.get() + 1);
        let r = body.iter().try_for_each(|stmt| self.infer_expr_type(stmt, env).map(|_| ()));
        self.loop_depth.set(self.loop_depth.get() - 1);
        r
    }

    pub fn get_struct_def(&self, name: &str) -> Option<&StructDef> {
        self.struct_defs.get(name)
    }

    fn check_fn_def(&self, f: &FnDef) -> Result<(), String> {
        self.in_unsafe_fn.set(f.is_unsafe);
        self.unsafe_depth.set(0);
        let mut env = HashMap::new();
        for (param_name, param_ty) in &f.params {
            self.validate_type(param_ty, f.span)
                .map_err(|e| format!("{} (parameter '{}' of '{}')", e, param_name, shown(&f.name)))?;
            env.insert(param_name.clone(), param_ty.clone());
        }
        self.validate_type(&f.return_type, f.span)
            .map_err(|e| format!("{} (return type of '{}')", e, shown(&f.name)))?;
        // the program's main gives its exit status (AIPL_SPEC.md 6.5)
        let status = matches!(&f.return_type, Type::I32 | Type::Void)
            || matches!(&f.return_type, Type::ResultType(ok, err) if **ok == Type::I32 && **err == Type::Str);
        if f.name == "main" && f.params.is_empty() && !status {
            return Err(format!(
                "{}:{}: main gives the program's exit status: it returns i32 (0 for success, 1-125 for a failure), (result i32 str) (ok with the status, or err with a message, printed to stderr, and status 1), or void (status 0); got {}",
                f.span.0, f.span.1, f.return_type
            ));
        }

        // Check contracts
        for c in &f.contracts {
            match c {
                Contract::Requires(expr) | Contract::Invariant(expr) => {
                    let cond_ty = self.infer_expr_type(expr, &mut env)?;
                    if cond_ty != Type::Bool {
                        let (l, c_col) = expr.span();
                        return Err(format!(
                            "{}:{}: Contract expression in '{}' must evaluate to bool, got {}",
                            l, c_col, shown(&f.name), cond_ty
                        ));
                    }
                }
                Contract::Ensures(expr) => {
                    let mut ens_env = env.clone();
                    ens_env.insert("res".to_string(), f.return_type.clone());
                    let cond_ty = self.infer_expr_type(expr, &mut ens_env)?;
                    if cond_ty != Type::Bool {
                        let (l, c_col) = expr.span();
                        return Err(format!(
                            "{}:{}: Ensures contract expression in '{}' must evaluate to bool, got {}",
                            l, c_col, shown(&f.name), cond_ty
                        ));
                    }
                }
            }
        }

        // Check function body expressions
        *self.return_type.borrow_mut() = Some(f.return_type.clone());
        self.loop_depth.set(0);
        let mut last_ty = Type::Void;
        let mut result = Ok(());
        for expr in &f.body {
            // a defer is a statement of the body, and not its value
            let r = match expr {
                Expr::Op { op: OpCode::Defer, args, .. } => self.check_defer(args, &mut env).map(|_| last_ty.clone()),
                _ => self.infer_expr_type(expr, &mut env),
            };
            match r {
                Ok(t) => last_ty = t,
                Err(e) => {
                    result = Err(e);
                    break;
                }
            }
        }
        *self.return_type.borrow_mut() = None;
        result?;
        self.bodies.borrow_mut().insert(f.name.clone(), last_ty.clone());

        // A body may end in (return v) instead of a bare value.
        let ends_in_return = matches!(f.body.iter().rfind(|e| !matches!(e, Expr::Op { op: OpCode::Defer, .. })), Some(Expr::Return { .. }));
        if f.return_type != Type::Void && !Self::fits(&last_ty, &f.return_type) && !ends_in_return {
            let hint = if last_ty == Type::Void { "; end the body with the value to return (a let, a set!, or a loop gives void)" } else { "" };
            return Err(format!(
                "{}:{}: Function '{}' expects return type {}, but body returned {}{}",
                f.span.0, f.span.1, shown(&f.name), f.return_type, last_ty, hint
            ));
        }

        Ok(())
    }

    /// Unchecked operations need an unsafe context: an `(unsafe ...)` block,
    /// or a function with the `(unsafe)` clause.
    fn is_unsafe_context(&self) -> bool {
        self.unsafe_depth.get() > 0 || self.in_unsafe_fn.get()
    }

    fn require_unsafe(&self, what: &str, l: u32, c: u32) -> Result<(), String> {
        if self.is_unsafe_context() {
            return Ok(());
        }
        Err(format!("{}:{}: {} is unchecked: write it inside (unsafe ...), or mark the function (unsafe)", l, c, what))
    }

    fn require_unsafe_fn(&self, name: &str, l: u32, c: u32) -> Result<(), String> {
        if !self.unsafe_fns.contains(name) || self.is_unsafe_context() {
            return Ok(());
        }
        Err(format!("{}:{}: '{}' is marked (unsafe): use it inside (unsafe ...), or mark this function (unsafe)", l, c, shown(name)))
    }

    fn infer_expr_type(&self, expr: &Expr, env: &mut HashMap<String, Type>) -> Result<Type, String> {
        let (l, c) = expr.span();
        match expr {
            Expr::Lit(lit, _) => match lit {
                Literal::Int(_) => Ok(Type::I32),
                Literal::Int64(_) => Ok(Type::I64),
                Literal::Float(_) => Ok(Type::F64),
                Literal::Float32(..) => Ok(Type::F32),
                Literal::Bool(_) => Ok(Type::Bool),
                Literal::Str(_) => Ok(Type::Str),
            },
            Expr::Var(name, _) => {
                if let Some(ty) = env.get(name) {
                    Ok(ty.clone())
                } else {
                    // outside a function body (return_type is None) is a contract
                    let hint = if name == "result" {
                        "; a function's result is named res, in its (ens ...)"
                    } else if self.return_type.borrow().is_none() {
                        "; a contract sees only the function's parameters, and res in (ens ...)"
                    } else {
                        ""
                    };
                    Err(format!("{}:{}: Undefined variable '{}'{}", l, c, name, hint))
                }
            }
            Expr::Let { name, ty, val, .. } => {
                if env.contains_key(name) {
                    return Err(format!("{}:{}: Cannot shadow existing variable '{}'", l, c, shown(name)));
                }
                self.validate_type(ty, (l, c))?;
                let val_ty = self.infer_expr_type(val, env)?;
                if !Self::fits(&val_ty, ty) {
                    return Err(format!(
                        "{}:{}: Type mismatch in 'let': expected {}, got {}{}",
                        l, c, ty, val_ty, self.fit_why(&val_ty, ty, val)
                    ));
                }
                env.insert(name.clone(), ty.clone());
                Ok(Type::Void)
            }
            Expr::Set { name, val, .. } => {
                let var_ty = env
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("{}:{}: Undefined variable '{}' in set!", l, c, name))?;
                let val_ty = self.infer_expr_type(val, env)?;
                if !Self::fits(&val_ty, &var_ty) {
                    return Err(format!(
                        "{}:{}: Type mismatch in 'set!': variable is {}, value is {}",
                        l, c, var_ty, val_ty
                    ));
                }
                Ok(Type::Void)
            }
            Expr::If { cond, then_branch, else_branch, .. } => {
                let cond_ty = self.infer_expr_type(cond, env)?;
                if cond_ty != Type::Bool {
                    return Err(format!("{}:{}: If condition must be bool, got {}", l, c, cond_ty));
                }
                let then_ty = self.infer_expr_type(then_branch, &mut env.clone())?;
                let else_ty = self.infer_expr_type(else_branch, &mut env.clone())?;
                // a branch that always leaves takes the other's type
                if diverges(then_branch) {
                    Ok(if diverges(else_branch) { Type::Void } else { else_ty })
                } else if diverges(else_branch) {
                    Ok(then_ty)
                } else if then_ty == Type::Void && else_ty == Type::Void {
                    Ok(Type::Void)
                } else if let Some(t) = Self::meet(&then_ty, &else_ty).filter(|_| then_ty != Type::Void && else_ty != Type::Void) {
                    Ok(t)
                } else {
                    Err(format!(
                        "{}:{}: If branch type mismatch: then is {}, else is {}. If mixing void and non-void, consider wrapping in (block ... value)",
                        l, c, then_ty, else_ty
                    ))
                }
            }
            Expr::Loop { var, start, end, step, body, .. } => {
                let start_ty = self.infer_expr_type(start, env)?;
                let end_ty = self.infer_expr_type(end, env)?;
                let step_ty = self.infer_expr_type(step, env)?;
                if start_ty != Type::I32 || end_ty != Type::I32 || step_ty != Type::I32 {
                    return Err(format!("{}:{}: Loop bounds and step must be i32", l, c));
                }
                if env.contains_key(var) {
                    return Err(format!("{}:{}: Cannot shadow existing variable '{}' in loop", l, c, shown(var)));
                }
                let mut local_env = env.clone();
                local_env.insert(var.clone(), Type::I32);
                self.check_loop_body(body, &mut local_env)?;
                Ok(Type::Void)
            }
            Expr::While { cond, body, .. } => {
                let cond_ty = self.infer_expr_type(cond, env)?;
                if cond_ty != Type::Bool {
                    return Err(format!("{}:{}: While condition must be bool", l, c));
                }
                let mut local_env = env.clone();
                self.check_loop_body(body, &mut local_env)?;
                Ok(Type::Void)
            }
            Expr::Return { val, .. } => {
                if self.in_defer.get() {
                    return Err(format!("{}:{}: a defer cannot return: it runs as the function returns", l, c));
                }
                let expected = self.return_type.borrow().clone().ok_or_else(|| {
                    format!("{}:{}: return is not allowed in a contract", l, c)
                })?;
                let got = match val {
                    Some(v) => self.infer_expr_type(v, env)?,
                    None => Type::Void,
                };
                if !Self::fits(&got, &expected) {
                    return Err(format!(
                        "{}:{}: return value has type {}, but the function returns {}",
                        l, c, got, expected
                    ));
                }
                Ok(Type::Void)
            }
            Expr::Break(_) | Expr::Continue(_) => {
                if self.loop_depth.get() == 0 {
                    let what = if matches!(expr, Expr::Break(_)) { "break" } else { "continue" };
                    return Err(format!("{}:{}: {} is only allowed inside a while or loop body", l, c, what));
                }
                Ok(Type::Void)
            }
            Expr::Call { func, args, .. } => {
                let (param_types, ret_type) = self.fn_signatures.get(func).ok_or_else(|| {
                    let hint = match env.get(func) {
                        Some(t @ Type::Fn(..)) => format!("; {func} is a function reference: call it with (call_ref {t} {func} args...)"),
                        _ => match nearest_name(func, self.fn_signatures.keys().filter(|k| !k.contains('<')).map(|k| k.as_str())) {
                            Some(near) => format!("; did you mean {}?", shown(near)),
                            None => String::new(),
                        },
                    };
                    format!("{}:{}: Call to unknown function '{}'{}", l, c, func, hint)
                })?;
                self.require_unsafe_fn(func, l, c)?;
                if args.len() != param_types.len() {
                    let takes = if param_types.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", param_types.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(" "))
                    };
                    return Err(format!(
                        "{}:{}: Function '{}' expects {} arguments{}, got {}",
                        l, c, shown(func), param_types.len(), takes, args.len()
                    ));
                }
                for (i, arg) in args.iter().enumerate() {
                    let arg_ty = self.infer_expr_type(arg, env)?;
                    if !Self::fits(&arg_ty, &param_types[i]) {
                        return Err(format!(
                            "{}:{}: Arg {} of '{}' expects {}, got {}{}",
                            l, c, i, shown(func), param_types[i], arg_ty, self.fit_why(&arg_ty, &param_types[i], arg)
                        ));
                    }
                }
                Ok(ret_type.clone())
            }
            Expr::Op { op, args, .. } => {
                if is_unchecked_op(op) {
                    self.require_unsafe(crate::printer::op_name(op), l, c)?;
                }
                check_literal_address(op, args, l, c)?;
                match op {
                OpCode::FormatStr | OpCode::FormatTo | OpCode::FormatPrint | OpCode::FormatPrintln | OpCode::FormatEprintln => {
                    let mut pieces = &args[..];
                    if *op == OpCode::FormatTo {
                        let Some(b) = args.first() else {
                            return Err(format!("{}:{}: format_to takes a buffer and a string literal, e.g. (format_to b \"n = {{n}}\")", l, c));
                        };
                        let t = self.infer_expr_type(b, env)?;
                        let writer = Type::Ptr(Box::new(Type::Struct("stream.Writer".into())));
                        if t != Type::Ptr(Box::new(Type::Struct("buf.Buf".into()))) && t != writer {
                            return Err(format!("{}:{}: format_to writes to a (ptr buf.Buf) or a (ptr stream.Writer), got {}", l, c, t));
                        }
                        // a writer is named twice once lowered (its buffer, then the
                        // flush check), so it must be a variable
                        if t == writer && !matches!(b, Expr::Var(..)) {
                            return Err(format!("{}:{}: format_to takes a (ptr stream.Writer) as a variable: bind it with let first", l, c));
                        }
                        self.formats.borrow_mut().insert(b as *const Expr as usize, t);
                        pieces = &args[1..];
                    }
                    for p in pieces {
                        match p {
                            Expr::Lit(Literal::Str(_), _) => {}
                            Expr::Op { op: OpCode::FormatPiece, args: pa, span: (pl, pc) } if pa.len() == 2 => {
                                let t = self.infer_expr_type(&pa[0], env)?;
                                let Expr::Lit(Literal::Str(text), _) = &pa[1] else {
                                    return Err(format!("{}:{}: a piece's format is a string literal", pl, pc));
                                };
                                if !crate::formats::prints(&t) {
                                    return Err(format!("{}:{}: format cannot print {}: it prints every value but void and function references", pl, pc, t));
                                }
                                let spec = crate::formats::Spec::parse(text).map_err(|m| format!("{}:{}: {}", pl, pc, m))?;
                                if let Some(m) = spec.misfit(&t) {
                                    return Err(format!("{}:{}: {}", pl, pc, m));
                                }
                                self.formats.borrow_mut().insert(&pa[0] as *const Expr as usize, t);
                            }
                            other => {
                                let (pl, pc) = other.span();
                                return Err(format!("{}:{}: {} takes string literals and (format.piece value \"format\")", pl, pc, op));
                            }
                        }
                    }
                    Ok(if *op == OpCode::FormatStr { Type::Str } else { Type::Void })
                }
                OpCode::FormatPiece => Err(format!("{}:{}: format.piece is part of a format string", l, c)),
                OpCode::FormatExpect => {
                    let (Some(Expr::Lit(Literal::Str(_), _)), Some(cond), 2) = (args.first(), args.get(1), args.len()) else {
                        return Err(format!("{}:{}: expect takes one condition, e.g. (expect (eq n 3))", l, c));
                    };
                    let t = self.infer_expr_type(cond, env)?;
                    if t != Type::Bool {
                        return Err(format!("{}:{}: expect takes a bool, e.g. (expect (eq n 3)), got {}", l, c, t));
                    }
                    // a comparison's sides, to show them when it fails (formats::lower)
                    if let Expr::Op { op: cmp, args: sides, .. } = cond {
                        if matches!(cmp, OpCode::Eq | OpCode::Neq | OpCode::Lt | OpCode::Lte | OpCode::Gt | OpCode::Gte) && sides.len() == 2 {
                            let st = self.infer_expr_type(&sides[0], env)?;
                            self.formats.borrow_mut().insert(&sides[0] as *const Expr as usize, st);
                        }
                    }
                    Ok(Type::Void)
                }
                OpCode::FormatDbg => {
                    let (Some(Expr::Lit(Literal::Str(_), _)), Some(v), 2) = (args.first(), args.get(1), args.len()) else {
                        return Err(format!("{}:{}: dbg takes one expression, e.g. (dbg (call f x))", l, c));
                    };
                    let t = self.infer_expr_type(v, env)?;
                    if t == Type::Void {
                        return Err(format!("{}:{}: dbg shows a value, and this expression has none (it is void)", l, c));
                    }
                    // the value's type, for formats::lower
                    self.formats.borrow_mut().insert(v as *const Expr as usize, t.clone());
                    Ok(t)
                }
                OpCode::Try | OpCode::TryOpt | OpCode::TryOr => {
                    if self.in_defer.get() {
                        return Err(format!("{}:{}: a defer cannot {}: it runs as the function returns", l, c, op));
                    }
                    self.check_try(op, args, env, l, c)
                }
                OpCode::Defer => Err(format!(
                    "{}:{}: defer is a statement of a function's body (it runs when the function returns), not part of an expression or a block",
                    l, c
                )),
                OpCode::CheckedAdd | OpCode::CheckedSub | OpCode::CheckedMul => {
                    if args.len() != 2 {
                        return Err(format!("{}:{}: {} requires 2 arguments", l, c, op));
                    }
                    let t1 = self.infer_expr_type(&args[0], env)?;
                    let t2 = self.infer_expr_type(&args[1], env)?;
                    if t1 != t2 {
                        return Err(format!("{}:{}: Type mismatch in binary op: {} vs {}{}", l, c, t1, t2, BINARY_HINT));
                    }
                    if !matches!(t1, Type::I32 | Type::I64) {
                        return Err(format!("{}:{}: {} is integer arithmetic (i32 or i64), got {}", l, c, op, t1));
                    }
                    Ok(t1)
                }
                OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Mod | OpCode::BitXor | OpCode::Shl | OpCode::Shr | OpCode::ShrU | OpCode::DivU | OpCode::RemU | OpCode::BitAnd | OpCode::BitOr => {
                    if args.len() != 2 {
                        return Err(format!("{}:{}: Arithmetic/bitwise opcode {} requires 2 arguments", l, c, op));
                    }
                    let t1 = self.infer_expr_type(&args[0], env)?;
                    let t2 = self.infer_expr_type(&args[1], env)?;
                    if let Some(p) = sealed(&t1).or(sealed(&t2)) {
                        return Err(format!("{}:{}: {} on {}: a type parameter has no arithmetic", l, c, op, p));
                    }
                    // A pointer, function ref, or enum operand gets its own
                    // explanation even when the other operand is a number:
                    // (+ p 4), (+ 1 e).
                    let special = |t: &Type| matches!(t, Type::Ptr(_) | Type::Array(_) | Type::Fn(..) | Type::Enum(_) | Type::Union(_));
                    let t1 = if !special(&t1) && special(&t2) { t2.clone() } else { t1 };
                    if t1 != t2 && !special(&t1) {
                        return Err(format!("{}:{}: Type mismatch in binary op: {} vs {}{}", l, c, t1, t2, BINARY_HINT));
                    }
                    if matches!(t1, Type::Ptr(_) | Type::Array(_) | Type::Fn(..)) {
                        return Err(format!(
                            "{}:{}: {} on {}: pointers, arrays, and function refs have no arithmetic; use get/put or arr.get/arr.set, or convert with ptr.addr/arr.addr and ptr.cast/arr.cast",
                            l, c, op, t1
                        ));
                    }
                    if let Type::Union(u) = &t1 {
                        return Err(format!("{}:{}: {} on union '{}': unions have no arithmetic; take them apart with match", l, c, op, u));
                    }
                    let integer_only = !matches!(op, OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div);
                    if t1 == Type::Str && matches!(op, OpCode::Add) {
                        return Err(format!("{}:{}: + does not join strings: join two with (call str.concat a b), or build text with std/buf (buf.push_str, then buf.to_str)", l, c));
                    }
                    if integer_only && !matches!(t1, Type::I32 | Type::I64 | Type::Enum(_)) {
                        return Err(format!("{}:{}: {} is integer arithmetic (i32 or i64), got {}", l, c, op, t1));
                    }
                    if !matches!(t1, Type::I32 | Type::I64 | Type::F32 | Type::F64 | Type::Enum(_)) {
                        return Err(format!("{}:{}: {} needs numbers (i32, i64, f32, f64), got {}", l, c, op, t1));
                    }
                    if let Type::Enum(e) = &t1 {
                        return Err(format!(
                            "{}:{}: {} on enum '{}': enums have no arithmetic; compare them with eq/neq, or convert with (enum.ord x) and (enum.cast {} n)",
                            l, c, op, e, e
                        ));
                    }
                    Ok(t1)
                }
                OpCode::MemLoad8 | OpCode::MemLoad32 | OpCode::MemLoad8S | OpCode::MemLoad16U | OpCode::MemLoad16S => {
                    if args.len() != 1 {
                        return Err(format!("{}:{}: {} requires 1 argument (ptr: i32)", l, c, op));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("{}:{}: {} requires i32 ptr, got {}", l, c, op, t));
                    }
                    Ok(Type::I32)
                }
                OpCode::MemLoad64 => {
                    if args.len() != 1 {
                        return Err(format!("{}:{}: mem.load64 requires 1 argument (ptr: i32)", l, c));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("{}:{}: mem.load64 requires i32 ptr, got {}", l, c, t));
                    }
                    Ok(Type::I64)
                }
                OpCode::MemStore8 | OpCode::MemStore32 | OpCode::MemStore16 => {
                    if args.len() != 2 {
                        return Err(format!("{}:{}: {} requires 2 arguments (ptr: i32, val: i32)", l, c, op));
                    }
                    let t1 = self.infer_expr_type(&args[0], env)?;
                    let t2 = self.infer_expr_type(&args[1], env)?;
                    if t1 != Type::I32 || t2 != Type::I32 {
                        return Err(format!("{}:{}: {} requires (i32, i32), got ({}, {})", l, c, op, t1, t2));
                    }
                    Ok(Type::Void)
                }
                OpCode::MemStore64 => {
                    if args.len() != 2 {
                        return Err(format!("{}:{}: mem.store64 requires 2 arguments (ptr: i32, val: i64)", l, c));
                    }
                    let t1 = self.infer_expr_type(&args[0], env)?;
                    let t2 = self.infer_expr_type(&args[1], env)?;
                    if t1 != Type::I32 || t2 != Type::I64 {
                        return Err(format!("{}:{}: mem.store64 requires (i32, i64), got ({}, {})", l, c, t1, t2));
                    }
                    Ok(Type::Void)
                }
                OpCode::MemAlloc => {
                    if args.len() != 1 {
                        return Err(format!("{}:{}: mem.alloc requires 1 argument (size: i32)", l, c));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("{}:{}: mem.alloc requires i32 size, got {}", l, c, t));
                    }
                    Ok(Type::I32)
                }
                OpCode::MemGrow => {
                    if args.len() != 1 {
                        return Err(format!("{}:{}: mem.grow requires 1 argument (pages: i32)", l, c));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("{}:{}: mem.grow requires i32 pages, got {}", l, c, t));
                    }
                    Ok(Type::I32)
                }
                OpCode::AtomicAdd => {
                    self.expect_operands("atomic.add", "(atomic.add p v)", args, &[Type::I32, Type::I32], env, (l, c))?;
                    Ok(Type::I32)
                }
                OpCode::AtomicCas => {
                    self.expect_operands("atomic.cas", "(atomic.cas p expected new)", args, &[Type::I32, Type::I32, Type::I32], env, (l, c))?;
                    Ok(Type::Bool)
                }
                OpCode::AtomicLock | OpCode::AtomicUnlock => {
                    let (name, form) = if matches!(op, OpCode::AtomicLock) { ("atomic.lock", "(atomic.lock p)") } else { ("atomic.unlock", "(atomic.unlock p)") };
                    self.expect_operands(name, form, args, &[Type::I32], env, (l, c))?;
                    Ok(Type::Void)
                }
                OpCode::Eq | OpCode::Neq | OpCode::Lt | OpCode::Lte | OpCode::Gt | OpCode::Gte
                | OpCode::LtU | OpCode::LteU | OpCode::GtU | OpCode::GteU => {
                    if args.len() != 2 {
                        return Err(format!("{}:{}: Comparison opcode {} requires 2 arguments", l, c, op));
                    }
                    let t1 = self.infer_expr_type(&args[0], env)?;
                    let t2 = self.infer_expr_type(&args[1], env)?;
                    if let Some(p) = sealed(&t1).or(sealed(&t2)) {
                        return Err(if matches!(op, OpCode::Eq | OpCode::Neq) {
                            format!("{}:{}: {} on {}: a type parameter has no equality of its own; add (where (cmp.Eq {})) and call (cmp.Eq.same {})", l, c, op, p, p, p)
                        } else {
                            format!("{}:{}: {} on {}: a type parameter has no order of its own; add (where (cmp.Ord {})) and call (cmp.Ord.compare {})", l, c, op, p, p, p)
                        });
                    }
                    let Some(t1) = Self::meet(&t1, &t2) else {
                        let null_hint = match (&t1, &t2) {
                            (Type::Ptr(_) | Type::Array(_), Type::I32) | (Type::I32, Type::Ptr(_) | Type::Array(_)) => {
                                "; to test for null, compare with (ptr.null S) or (arr.null T)"
                            }
                            _ => "",
                        };
                        return Err(format!("{}:{}: Type mismatch in comparison: {} vs {}{}", l, c, t1, t2, null_hint));
                    };
                    if matches!(op, OpCode::LtU | OpCode::LteU | OpCode::GtU | OpCode::GteU) && !matches!(t1, Type::I32 | Type::I64) {
                        return Err(format!("{}:{}: {} compares integers (i32 or i64) as unsigned, got {}", l, c, op, t1));
                    }
                    if let Type::Union(u) = &t1 {
                        return Err(format!("{}:{}: {} on union '{}': union values do not compare; take them apart with match", l, c, op, u));
                    }
                    if let Type::Struct(v) = &t1 {
                        return Err(format!(
                            "{}:{}: {} on value struct '{}': value structs do not compare; implement cmp.Eq for it (section 4.H) and call (cmp.Eq.same {})",
                            l, c, op, shown(v), shown(v)
                        ));
                    }
                    // a str made at run time has its own address, so eq would
                    // compare addresses compiled and contents in the VM
                    if t1 == Type::Str {
                        return Err(format!(
                            "{}:{}: {} on str: compare strings by content with (call str.same a b), and order them with (call (cmp.Ord.compare str) a b)",
                            l, c, op
                        ));
                    }
                    if matches!(t1, Type::Ptr(_) | Type::Array(_) | Type::Fn(..) | Type::Enum(_)) && !matches!(op, OpCode::Eq | OpCode::Neq) {
                        return Err(format!("{}:{}: {} on {}: pointers, arrays, function refs, and enums compare only with eq/neq", l, c, op, t1));
                    }
                    // lt/lte/gt/gte order numbers; bool compares only with eq/neq
                    if t1 == Type::Bool && !matches!(op, OpCode::Eq | OpCode::Neq) {
                        return Err(format!("{}:{}: {} on bool: only numbers are ordered; bool compares only with eq/neq", l, c, op));
                    }
                    Ok(Type::Bool)
                }
                OpCode::And | OpCode::Or => {
                    if args.len() != 2 {
                        return Err(format!(
                            "{}:{}: {} takes exactly 2 operands, got {}; nest them: ({} a ({} b c))",
                            l, c, if matches!(op, OpCode::And) { "and" } else { "or" }, args.len(),
                            if matches!(op, OpCode::And) { "and" } else { "or" },
                            if matches!(op, OpCode::And) { "and" } else { "or" }
                        ));
                    }
                    for arg in args {
                        let t = self.infer_expr_type(arg, env)?;
                        if t != Type::Bool {
                            return Err(format!("{}:{}: Logical op expects bool, got {}", l, c, t));
                        }
                    }
                    Ok(Type::Bool)
                }
                OpCode::Not => {
                    if args.len() != 1 {
                        return Err(format!("{}:{}: Not op expects 1 argument", l, c));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::Bool {
                        return Err(format!("{}:{}: Not op expects bool, got {}", l, c, t));
                    }
                    Ok(Type::Bool)
                }
                OpCode::SysPrint => {
                    for arg in args {
                        let t = self.infer_expr_type(arg, env)?;
                        if t != Type::Str {
                            return Err(format!(
                                "{}:{}: sys.print prints str values, got {}; for numbers and other values use the format forms, (print \"{{n}}\") or (println \"n: {{n}}\")",
                                l, c, t
                            ));
                        }
                    }
                    Ok(Type::Void)
                }
                OpCode::SysTime | OpCode::SysMonotonic => {
                    if !args.is_empty() {
                        return Err(format!("{}:{}: {} takes no arguments", l, c, op));
                    }
                    Ok(Type::I64)
                }
                OpCode::SysRandom => {
                    if args.len() != 2 {
                        return Err(format!("{}:{}: sys.random requires 2 arguments (ptr, len)", l, c));
                    }
                    for (i, arg) in args.iter().enumerate() {
                        let t = self.infer_expr_type(arg, env)?;
                        if t != Type::I32 {
                            return Err(format!("{}:{}: sys.random argument {} must be i32, got {}", l, c, i, t));
                        }
                    }
                    Ok(Type::I32)
                }
                OpCode::SysExit => {
                    if args.len() != 1 {
                        return Err(format!("{}:{}: sys.exit requires 1 argument (code: i32)", l, c));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("{}:{}: sys.exit requires i32 code, got {}", l, c, t));
                    }
                    Ok(Type::Void)
                }
                OpCode::StrLen | OpCode::StrPtr => {
                    let name = if *op == OpCode::StrLen { "str.len" } else { "str.ptr" };
                    if args.len() != 1 {
                        return Err(format!("{}:{}: {} requires 1 argument (s: str)", l, c, name));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::Str {
                        return Err(format!("{}:{}: {} requires str, got {}", l, c, name, t));
                    }
                    Ok(Type::I32)
                }
                // fs.* take raw i32 pointers/lengths/fds, never a `str` (a str is a
                // pointer in wasm but a Rust string in the VM, so letting one
                // through here would work in one backend and fail in the other).
                OpCode::FsOpen | OpCode::FsRead | OpCode::FsWrite => {
                    if args.len() != 3 {
                        return Err(format!("{}:{}: {} requires 3 arguments", l, c, op));
                    }
                    for (i, arg) in args.iter().enumerate() {
                        let t = self.infer_expr_type(arg, env)?;
                        if t != Type::I32 {
                            return Err(format!(
                                "{}:{}: {} argument {} must be i32 (pointer/length/fd), got {}",
                                l, c, op, i, t
                            ));
                        }
                    }
                    Ok(Type::I32)
                }
                OpCode::FsClose => {
                    if args.len() != 1 {
                        return Err(format!("{}:{}: fs.close requires 1 argument", l, c));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("{}:{}: fs.close requires i32 fd, got {}", l, c, t));
                    }
                    Ok(Type::I32)
                }
                // args.*/env.* take two i32 addresses the host writes through.
                OpCode::ArgsSizes | OpCode::ArgsGet | OpCode::EnvSizes | OpCode::EnvGet => {
                    if args.len() != 2 {
                        return Err(format!("{}:{}: {} requires 2 arguments (two i32 addresses)", l, c, op));
                    }
                    for (i, arg) in args.iter().enumerate() {
                        let t = self.infer_expr_type(arg, env)?;
                        if t != Type::I32 {
                            return Err(format!("{}:{}: {} argument {} must be an i32 address, got {}", l, c, op, i, t));
                        }
                    }
                    Ok(Type::I32)
                }
                OpCode::FsDelete => {
                    if args.len() != 2 {
                        return Err(format!("{}:{}: fs.delete requires 2 arguments (path_ptr, path_len)", l, c));
                    }
                    for (i, arg) in args.iter().enumerate() {
                        let t = self.infer_expr_type(arg, env)?;
                        if t != Type::I32 {
                            return Err(format!(
                                "{}:{}: fs.delete argument {} must be i32 (pointer/length), got {}",
                                l, c, i, t
                            ));
                        }
                    }
                    Ok(Type::I32)
                }
                OpCode::FsChmod => {
                    if args.len() != 3 {
                        return Err(format!("{}:{}: fs.chmod requires 3 arguments (path_ptr, path_len, mode)", l, c));
                    }
                    for (i, arg) in args.iter().enumerate() {
                        let t = self.infer_expr_type(arg, env)?;
                        if t != Type::I32 {
                            return Err(format!("{}:{}: fs.chmod argument {} must be i32 (pointer, length, mode), got {}", l, c, i, t));
                        }
                    }
                    Ok(Type::I32)
                }
                OpCode::FsRename => {
                    if args.len() != 4 {
                        return Err(format!("{}:{}: fs.rename requires 4 arguments (old_ptr, old_len, new_ptr, new_len)", l, c));
                    }
                    for (i, arg) in args.iter().enumerate() {
                        let t = self.infer_expr_type(arg, env)?;
                        if t != Type::I32 {
                            return Err(format!("{}:{}: fs.rename argument {} must be i32 (pointer/length), got {}", l, c, i, t));
                        }
                    }
                    Ok(Type::I32)
                }
                OpCode::FsOpendir => {
                    self.expect_operands("fs.opendir", "(fs.opendir ptr len)", args, &[Type::I32, Type::I32], env, (l, c))?;
                    Ok(Type::I32)
                }
                OpCode::FsReaddir => {
                    self.expect_operands("fs.readdir", "(fs.readdir fd buf len cookie)", args, &[Type::I32, Type::I32, Type::I32, Type::I64], env, (l, c))?;
                    Ok(Type::I32)
                }
                OpCode::FsStat => {
                    self.expect_operands("fs.stat", "(fs.stat ptr len out)", args, &[Type::I32, Type::I32, Type::I32], env, (l, c))?;
                    Ok(Type::I32)
                }
                OpCode::NetListen => {
                    self.expect_operands("net.listen", "(net.listen addr len port)", args, &[Type::I32, Type::I32, Type::I32], env, (l, c))?;
                    Ok(Type::I32)
                }
                OpCode::NetAccept => {
                    self.expect_operands("net.accept", "(net.accept h)", args, &[Type::I32], env, (l, c))?;
                    Ok(Type::I32)
                }
                OpCode::NetRecv => {
                    self.expect_operands("net.recv", "(net.recv h buf max)", args, &[Type::I32, Type::I32, Type::I32], env, (l, c))?;
                    Ok(Type::I32)
                }
                OpCode::NetSend => {
                    self.expect_operands("net.send", "(net.send h buf len)", args, &[Type::I32, Type::I32, Type::I32], env, (l, c))?;
                    Ok(Type::I32)
                }
                OpCode::NetClose => {
                    self.expect_operands("net.close", "(net.close h)", args, &[Type::I32], env, (l, c))?;
                    Ok(Type::I32)
                }
                OpCode::ProcSpawn => {
                    self.expect_operands("proc.spawn", "(proc.spawn path len args args_len stdio)", args, &[Type::I32, Type::I32, Type::I32, Type::I32, Type::I32], env, (l, c))?;
                    Ok(Type::I32)
                }
                OpCode::ProcStream => {
                    self.expect_operands("proc.stream", "(proc.stream p which)", args, &[Type::I32, Type::I32], env, (l, c))?;
                    Ok(Type::I32)
                }
                OpCode::ProcRead => {
                    self.expect_operands("proc.read", "(proc.read h buf max)", args, &[Type::I32, Type::I32, Type::I32], env, (l, c))?;
                    Ok(Type::I32)
                }
                OpCode::ProcWrite => {
                    self.expect_operands("proc.write", "(proc.write h buf len)", args, &[Type::I32, Type::I32, Type::I32], env, (l, c))?;
                    Ok(Type::I32)
                }
                OpCode::ProcClose => {
                    self.expect_operands("proc.close", "(proc.close h)", args, &[Type::I32], env, (l, c))?;
                    Ok(Type::I32)
                }
                OpCode::ProcWait => {
                    self.expect_operands("proc.wait", "(proc.wait p)", args, &[Type::I32], env, (l, c))?;
                    Ok(Type::I32)
                }
                OpCode::ProcExec => {
                    self.expect_operands("proc.exec", "(proc.exec path len args args_len)", args, &[Type::I32, Type::I32, Type::I32, Type::I32], env, (l, c))?;
                    Ok(Type::I32)
                }
                OpCode::ThreadSpawn => {
                    // (thread.spawn (ref worker) arg): the worker takes the i32 arg.
                    if args.len() != 2 {
                        return Err(format!("{}:{}: thread.spawn requires 2 arguments (worker: (fn [i32] -> i32), arg: i32)", l, c));
                    }
                    let worker = Type::Fn(vec![Type::I32], Box::new(Type::I32), false);
                    let t = self.infer_expr_type(&args[0], env)?;
                    if !Self::fits(&t, &worker) {
                        return Err(format!("{}:{}: thread.spawn needs a worker of type (fn [i32] -> i32), got {}", l, c, t));
                    }
                    let a = self.infer_expr_type(&args[1], env)?;
                    if a != Type::I32 {
                        return Err(format!("{}:{}: thread.spawn argument must be i32, got {}", l, c, a));
                    }
                    Ok(Type::I32)
                }
                OpCode::ThreadJoin => {
                    if args.len() != 1 {
                        return Err(format!("{}:{}: thread.join requires 1 argument (thread handle)", l, c));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("{}:{}: thread.join needs the i32 handle thread.spawn returned, got {}", l, c, t));
                    }
                    Ok(Type::I32)
                }
                OpCode::I64ExtendS | OpCode::I64ExtendU => {
                    if args.len() != 1 {
                        return Err(format!("{}:{}: {} requires 1 argument (x: i32)", l, c, op));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I32 {
                        return Err(format!("{}:{}: {} requires i32, got {}", l, c, op, t));
                    }
                    Ok(Type::I64)
                }
                OpCode::F64ConvertI64S
                | OpCode::I64TruncF64S
                | OpCode::F64ReinterpretI64
                | OpCode::I64ReinterpretF64
                | OpCode::F64Sqrt
                | OpCode::F32DemoteF64
                | OpCode::F64PromoteF32
                | OpCode::F32ConvertI32S
                | OpCode::F32ConvertI64S
                | OpCode::F64ConvertI32S
                | OpCode::I32TruncF32S
                | OpCode::I32TruncF64S
                | OpCode::I64TruncF32S
                | OpCode::F32ReinterpretI32
                | OpCode::I32ReinterpretF32 => {
                    let (from, to) = conversion(op);
                    if args.len() != 1 {
                        return Err(format!("{}:{}: {} requires 1 argument (x: {})", l, c, op, from));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != from {
                        return Err(format!("{}:{}: {} requires {}, got {}", l, c, op, from, t));
                    }
                    Ok(to)
                }
                OpCode::I32Wrap => {
                    if args.len() != 1 {
                        return Err(format!("{}:{}: i32.wrap requires 1 argument (x: i64)", l, c));
                    }
                    let t = self.infer_expr_type(&args[0], env)?;
                    if t != Type::I64 {
                        return Err(format!("{}:{}: i32.wrap requires i64, got {}", l, c, t));
                    }
                    Ok(Type::I32)
                }
                }
            }
            Expr::Ok(val, extra_ty, _) => {
                // a result's whole type is written where it is made
                if extra_ty.is_none() {
                    return Err(format!("{}:{}: {}", l, c, OK_NEEDS_TYPE));
                }
                let inner_ty = self.infer_expr_type(val, env)?;
                check_result_payload(&inner_ty, (l, c))?;
                let err_ty = extra_ty.clone().unwrap_or(Type::I32);
                self.validate_type(&err_ty, (l, c))?;
                self.result_fits(&inner_ty, &err_ty, (l, c))?;
                Ok(Type::ResultType(Box::new(inner_ty), Box::new(err_ty)))
            }
            Expr::Err(err, extra_ty, _) => {
                if extra_ty.is_none() {
                    return Err(format!("{}:{}: {}", l, c, ERR_NEEDS_TYPE));
                }
                let err_ty = self.infer_expr_type(err, env)?;
                check_result_payload(&err_ty, (l, c))?;
                let ok_ty = extra_ty.clone().unwrap_or(Type::I32);
                self.validate_type(&ok_ty, (l, c))?;
                self.result_fits(&ok_ty, &err_ty, (l, c))?;
                Ok(Type::ResultType(Box::new(ok_ty), Box::new(err_ty)))
            }
            Expr::MatchResult { expr, ok_var, ok_body, err_var, err_body, .. } => {
                let res_ty = self.infer_expr_type(expr, env)?;
                let (ok_ty, err_ty) = match res_ty {
                    Type::ResultType(ok_t, err_t) => (*ok_t, *err_t),
                    other => return Err(format!("{}:{}: match_result expected a (result T E), got {}", l, c, other)),
                };

                if env.contains_key(ok_var) {
                    return Err(format!("{}:{}: Cannot shadow existing variable '{}' in match_result ok arm", l, c, shown(ok_var)));
                }
                let mut ok_env = env.clone();
                ok_env.insert(ok_var.clone(), ok_ty);
                let mut last_ok_ty = Type::Void;
                for stmt in ok_body {
                    last_ok_ty = self.infer_expr_type(stmt, &mut ok_env)?;
                }

                if env.contains_key(err_var) {
                    return Err(format!("{}:{}: Cannot shadow existing variable '{}' in match_result err arm", l, c, shown(err_var)));
                }
                let mut err_env = env.clone();
                err_env.insert(err_var.clone(), err_ty);
                let mut last_err_ty = Type::Void;
                for stmt in err_body {
                    last_err_ty = self.infer_expr_type(stmt, &mut err_env)?;
                }

                if body_diverges(ok_body) {
                    Ok(if body_diverges(err_body) { Type::Void } else { last_err_ty })
                } else if body_diverges(err_body) {
                    Ok(last_ok_ty)
                } else if last_ok_ty == Type::Void && last_err_ty == Type::Void {
                    Ok(Type::Void)
                } else if let Some(t) = Self::meet(&last_ok_ty, &last_err_ty).filter(|_| last_ok_ty != Type::Void && last_err_ty != Type::Void) {
                    Ok(t)
                } else {
                    Err(format!(
                        "{}:{}: match_result arm type mismatch: ok arm yields {}, err arm yields {}",
                        l, c, last_ok_ty, last_err_ty
                    ))
                }
            }
            Expr::Block(exprs, _) => {
                let mut local_env = env.clone();
                let mut last_ty = Type::Void;
                for e in exprs {
                    last_ty = self.infer_expr_type(e, &mut local_env)?;
                }
                Ok(last_ty)
            }
            // a block in which unchecked operations are allowed
            Expr::Unsafe(exprs, _) => {
                let mut local_env = env.clone();
                let mut last_ty = Type::Void;
                self.unsafe_depth.set(self.unsafe_depth.get() + 1);
                let r = exprs.iter().try_for_each(|e| {
                    last_ty = self.infer_expr_type(e, &mut local_env)?;
                    Ok::<(), String>(())
                });
                self.unsafe_depth.set(self.unsafe_depth.get() - 1);
                r.map(|_| last_ty)
            }
            Expr::GetField {
                struct_name,
                field_name,
                ptr,
                span,
            } => {
                let def = self.struct_defs.get(struct_name).ok_or_else(|| not_a_struct(*span, struct_name))?;
                let ptr_ty = self.infer_expr_type(ptr, env)?;
                if def.is_value {
                    if ptr_ty != Type::Struct(struct_name.clone()) {
                        return Err(format!(
                            "{}:{}: get {}.{} needs a {}, got {}",
                            span.0, span.1, shown(struct_name), field_name, shown(struct_name), ptr_ty
                        ));
                    }
                } else if ptr_ty != Type::Ptr(Box::new(Type::Struct(struct_name.clone()))) {
                    return Err(format!(
                        "{}:{}: get {}.{} needs a (ptr {}), got {}",
                        span.0, span.1, shown(struct_name), field_name, shown(struct_name), ptr_ty
                    ));
                }
                let (_offset, field_ty) = get_field_offset(def, field_name, &self.struct_defs).map_err(|e| {
                    format!("{}:{}: {}", span.0, span.1, e)
                })?;
                Ok(value_of(&field_ty))
            }
            Expr::PutField {
                struct_name,
                field_name,
                ptr,
                val,
                path,
                span,
            } => {
                let def = self.struct_defs.get(struct_name).ok_or_else(|| not_a_struct(*span, struct_name))?;
                if def.is_value {
                    return Err(format!(
                        "{}:{}: a value struct's fields do not change: (with v {}.{} x) is a copy with one changed",
                        span.0, span.1, shown(struct_name), field_name
                    ));
                }
                let ptr_ty = self.infer_expr_type(ptr, env)?;
                if ptr_ty != Type::Ptr(Box::new(Type::Struct(struct_name.clone()))) {
                    return Err(format!(
                        "{}:{}: put {}.{} needs a (ptr {}), got {}",
                        span.0, span.1, shown(struct_name), field_name, shown(struct_name), ptr_ty
                    ));
                }
                let (_offset, field_ty) = get_field_offset(def, field_name, &self.struct_defs).map_err(|e| {
                    format!("{}:{}: {}", span.0, span.1, e)
                })?;
                let (written, field_ty) = self.walk_path((struct_name, field_name), field_ty, path, "put", *span)?;
                let field_ty = value_of(&field_ty);
                let val_ty = self.infer_expr_type(val, env)?;
                if !Self::fits(&val_ty, &field_ty) {
                    return Err(format!(
                        "{}:{}: Type mismatch writing to field '{}.{}': expected {}, got {}",
                        span.0, span.1, shown(&written.0), written.1, field_ty, val_ty
                    ));
                }
                Ok(Type::Void)
            }
            Expr::Record { struct_name, args, span } => {
                let def = self.struct_defs.get(struct_name).ok_or_else(|| not_a_struct(*span, struct_name))?;
                if !def.is_value {
                    return Err(format!(
                        "{}:{}: '{}' is not a value struct: (new {}) makes one behind a pointer",
                        span.0, span.1, shown(struct_name), shown(struct_name)
                    ));
                }
                if args.len() != def.fields.len() {
                    let names: Vec<&str> = def.fields.iter().map(|f| f.name.as_str()).collect();
                    return Err(format!(
                        "{}:{}: make {} takes {} field value(s) ({}), got {}",
                        span.0, span.1, shown(struct_name), def.fields.len(), names.join(" "), args.len()
                    ));
                }
                let def = def.clone();
                for (a, f) in args.iter().zip(&def.fields) {
                    let t = self.infer_expr_type(a, env)?;
                    if !Self::fits(&t, &f.ty) {
                        return Err(format!(
                            "{}:{}: make {}: field '{}' is {}, got {}{}",
                            span.0, span.1, shown(struct_name), f.name, f.ty, t, self.fit_why(&t, &f.ty, a)
                        ));
                    }
                }
                Ok(Type::Struct(struct_name.clone()))
            }
            Expr::With { struct_name, field_name, value, new_val, span } => {
                let def = self.struct_defs.get(struct_name).ok_or_else(|| not_a_struct(*span, struct_name))?;
                if !def.is_value {
                    return Err(format!(
                        "{}:{}: with makes a copy of a value struct, and '{}' is not one: change a field with (put p {}.{} x)",
                        span.0, span.1, shown(struct_name), shown(struct_name), field_name
                    ));
                }
                let (_offset, field_ty) = get_field_offset(def, field_name, &self.struct_defs).map_err(|e| format!("{}:{}: {}", span.0, span.1, e))?;
                let t = self.infer_expr_type(value, env)?;
                if t != Type::Struct(struct_name.clone()) {
                    return Err(format!(
                        "{}:{}: with {}.{} needs a {}, got {}",
                        span.0, span.1, shown(struct_name), field_name, shown(struct_name), t
                    ));
                }
                let n = self.infer_expr_type(new_val, env)?;
                if !Self::fits(&n, &field_ty) {
                    return Err(format!(
                        "{}:{}: with {}.{}: expected {}, got {}",
                        span.0, span.1, shown(struct_name), field_name, field_ty, n
                    ));
                }
                Ok(t)
            }
            Expr::Sizeof { ty, span } => {
                match ty {
                    // a bare name: a struct, or an enum (an i32)
                    Type::Struct(name) => {
                        if !self.struct_defs.contains_key(name) && !self.enum_defs.contains_key(name) {
                            return Err(not_a_struct(*span, name));
                        }
                    }
                    Type::Narrow(_) => {}
                    _ => {
                        self.validate_type(ty, *span)?;
                        if memory_layout(ty, &self.struct_defs).is_err() {
                            return Err(format!(
                                "{}:{}: sizeof needs a struct or a type that can be stored in memory, got {}",
                                span.0, span.1, ty
                            ));
                        }
                    }
                }
                Ok(Type::I32)
            }
            Expr::ArrGet {
                elem_ty,
                ptr,
                index,
                span,
            } => {
                self.validate_type(&Type::Array(Box::new(elem_ty.clone())), *span)?;
                let ptr_ty = self.infer_expr_type(ptr, env)?;
                if ptr_ty != Type::Array(Box::new(elem_ty.clone())) {
                    return Err(format!(
                        "{}:{}: arr.get {} needs an (arr {}), got {}",
                        span.0, span.1, elem_ty, elem_ty, ptr_ty
                    ));
                }
                let idx_ty = self.infer_expr_type(index, env)?;
                if idx_ty != Type::I32 {
                    return Err(format!(
                        "{}:{}: arr.get index must be i32, got {}",
                        span.0, span.1, idx_ty
                    ));
                }
                Ok(value_of(elem_ty))
            }
            Expr::ArrSet {
                elem_ty,
                ptr,
                index,
                val,
                path,
                span,
            } => {
                self.validate_type(&Type::Array(Box::new(elem_ty.clone())), *span)?;
                let ptr_ty = self.infer_expr_type(ptr, env)?;
                if ptr_ty != Type::Array(Box::new(elem_ty.clone())) {
                    return Err(format!(
                        "{}:{}: arr.set {} needs an (arr {}), got {}",
                        span.0, span.1, elem_ty, elem_ty, ptr_ty
                    ));
                }
                let idx_ty = self.infer_expr_type(index, env)?;
                if idx_ty != Type::I32 {
                    return Err(format!(
                        "{}:{}: arr.set index must be i32, got {}",
                        span.0, span.1, idx_ty
                    ));
                }
                // a path: one field of a value struct element
                let target = match path.split_first() {
                    None => value_of(elem_ty),
                    Some(((st, f), rest)) => {
                        if *elem_ty != Type::Struct(st.clone()) || !self.is_value(st) {
                            return Err(format!(
                                "{}:{}: arr.set path: the elements are {}, not the value struct {}",
                                span.0, span.1, elem_ty, shown(st)
                            ));
                        }
                        let def = &self.struct_defs[st];
                        let (_offset, first_ty) = get_field_offset(def, f, &self.struct_defs).map_err(|e| format!("{}:{}: {}", span.0, span.1, e))?;
                        self.walk_path((st, f), first_ty, rest, "arr.set", *span)?.1
                    }
                };
                let val_ty = self.infer_expr_type(val, env)?;
                if !Self::fits(&val_ty, &target) {
                    return Err(format!(
                        "{}:{}: arr.set value mismatch: expected {}, got {}",
                        span.0, span.1, target, val_ty
                    ));
                }
                Ok(Type::Void)
            }
            Expr::ArrLen { arr, span } => match self.infer_expr_type(arr, env)? {
                Type::Array(_) => Ok(Type::I32),
                other => Err(format!("{}:{}: arr.len needs an (arr T), got {}", span.0, span.1, other)),
            },
            Expr::Null { ty, span } => {
                self.validate_type(ty, *span)?;

                Ok(ty.clone())
            }
            Expr::Cast { ty, addr, gen, span } => {
                match (ty, gen.is_some()) {
                    (Type::Ptr(_), false) => self.require_unsafe("ptr.cast", span.0, span.1)?,
                    (Type::Array(_), false) => self.require_unsafe("arr.cast", span.0, span.1)?,
                    (Type::Ptr(_), true) => self.require_unsafe("ptr.make", span.0, span.1)?,
                    (Type::Array(_), true) => self.require_unsafe("arr.make", span.0, span.1)?,
                    _ => {}
                }
                if let Some(g) = gen {
                    let gt = self.infer_expr_type(g, env)?;
                    if gt != Type::I32 {
                        return Err(format!("{}:{}: the generation in ptr.make/arr.make must be i32, got {}", span.0, span.1, gt));
                    }
                }
                self.validate_type(ty, *span)?;
                if let Some(p) = sealed(ty) {
                    return Err(format!("{}:{}: enum.cast {}: a type parameter is not an enum", span.0, span.1, p));
                }

                let t = self.infer_expr_type(addr, env)?;
                if t != Type::I32 {
                    if let Type::Enum(e) = ty {
                        return Err(format!("{}:{}: enum.cast {} needs an i32 value, got {}", span.0, span.1, e, t));
                    }
                    return Err(format!("{}:{}: cast needs an i32 address, got {}", span.0, span.1, t));
                }
                Ok(ty.clone())
            }
            Expr::Ref { name, span } => match self.fn_signatures.get(name) {
                Some((params, ret)) => {
                    self.require_unsafe_fn(name, span.0, span.1)?;
                    // it promises (no_leak) when the checker proves f keeps
                    // no memory (MEMORY_PLAN.md decision 9)
                    let promise = self.leaks.as_ref().is_some_and(|a| !a.may_leak(name));
                    Ok(Type::Fn(params.clone(), Box::new(ret.clone()), promise))
                }
                None => Err(format!("{}:{}: Undefined function '{}' in ref", span.0, span.1, name)),
            },
            Expr::CallRef { sig, func, args, span } => {
                self.validate_type(sig, *span)?;
                let (params, ret) = match sig {
                    Type::Fn(p, r, _) => (p.clone(), *r.clone()),
                    other => {
                        return Err(format!("{}:{}: call_ref needs a (fn [...] -> r) signature, got {}", span.0, span.1, other))
                    }
                };
                let ft = self.infer_expr_type(func, env)?;
                if !Self::fits(&ft, sig) {
                    return Err(format!("{}:{}: call_ref signature {} does not match the function's type {}", span.0, span.1, sig, ft));
                }
                if args.len() != params.len() {
                    return Err(format!("{}:{}: call_ref expects {} arguments, got {}", span.0, span.1, params.len(), args.len()));
                }
                for (i, (a, p)) in args.iter().zip(params.iter()).enumerate() {
                    let at = self.infer_expr_type(a, env)?;
                    if !Self::fits(&at, p) {
                        return Err(format!("{}:{}: call_ref argument {} expects {}, got {}", span.0, span.1, i + 1, p, at));
                    }
                }
                Ok(ret)
            }
            Expr::Make { union_name, variant, args, span } => {
                let (l, c) = *span;
                let Some(u) = self.union_defs.get(union_name) else {
                    let whole = format!("{}.{}", union_name, variant);
                    if self.struct_defs.contains_key(&whole) {
                        return Err(format!(
                            "{}:{}: '{}' is not a value struct: (new {}) makes one behind a pointer",
                            l, c, shown(&whole), shown(&whole)
                        ));
                    }
                    return Err(format!("{}:{}: Unknown union '{}' in make", l, c, union_name));
                };
                let Some(v) = u.variants.iter().find(|v| &v.name == variant) else {
                    return Err(format!("{}:{}: union '{}' has no variant '{}'", l, c, shown(union_name), variant));
                };
                if args.len() != v.fields.len() {
                    return Err(format!(
                        "{}:{}: make {}.{} takes {} values (its fields), got {}",
                        l, c, shown(union_name), variant, v.fields.len(), args.len()
                    ));
                }
                for (a, f) in args.iter().zip(v.fields.iter()) {
                    let at = self.infer_expr_type(a, env)?;
                    if !Self::fits(&at, &f.ty) {
                        return Err(format!("{}:{}: make {}.{}: field '{}' is {}, got {}{}", l, c, shown(union_name), variant, f.name, f.ty, at, self.fit_why(&at, &f.ty, a)));
                    }
                }
                Ok(Type::Union(union_name.clone()))
            }
            Expr::Match { value, arms, else_body, span } => {
                let (l, c) = *span;
                let vt = self.infer_expr_type(value, env)?;
                if let Some(p) = sealed(&vt) {
                    return Err(format!("{}:{}: match on {}: a type parameter has no variants or members to match", l, c, p));
                }
                // the members in order, with each union variant's fields
                let (tname, members): (String, Vec<MatchMember>) = match &vt {
                    Type::Union(u) => (u.clone(), self.union_defs[u].variants.iter().map(|v| (v.name.clone(), Some(&v.fields))).collect()),
                    Type::Enum(e) => (e.clone(), self.enum_defs[e].members.iter().map(|(m, _)| (m.clone(), None)).collect()),
                    other => return Err(format!("{}:{}: match needs a union or enum value, got {}", l, c, other)),
                };
                let mut seen: Vec<&str> = Vec::new();
                let mut arm_types = Vec::new();
                for arm in arms {
                    let (al, ac) = arm.span;
                    let member = match arm.member.rfind('.') {
                        Some(dot) if arm.member[..dot] == tname => &arm.member[dot + 1..],
                        _ => {
                            return Err(format!("{}:{}: a match arm on {} is ({}.member ...), got {}", al, ac, shown(&tname), shown(&tname), shown(&arm.member)))
                        }
                    };
                    let Some((_, fields)) = members.iter().find(|(m, _)| m == member) else {
                        return Err(format!("{}:{}: {} has no member '{}'", al, ac, shown(&tname), member));
                    };
                    if seen.contains(&member) {
                        return Err(format!("{}:{}: {}.{} is matched twice", al, ac, shown(&tname), member));
                    }
                    seen.push(member);
                    let mut arm_env = env.clone();
                    match (fields, &arm.binders) {
                        (None, Some(_)) => {
                            return Err(format!("{}:{}: {}.{} is an enum member; its arm binds nothing", al, ac, tname, member))
                        }
                        (None, None) => {}
                        (Some(fields), binders) => {
                            let names: &[String] = binders.as_deref().unwrap_or(&[]);
                            if names.len() != fields.len() {
                                let want: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
                                return Err(format!(
                                    "{}:{}: the {}.{} arm binds its {} fields in order: [{}], got {} names",
                                    al, ac, tname, member, fields.len(), want.join(" "), names.len()
                                ));
                            }
                            // `_` binds nothing: the field is not read
                            for (n, f) in names.iter().zip(fields.iter()).filter(|(n, _)| n.as_str() != "_") {
                                if arm_env.contains_key(n) {
                                    return Err(format!("{}:{}: Cannot shadow existing variable '{}' in the {}.{} arm", al, ac, shown(n), tname, member));
                                }
                                arm_env.insert(n.clone(), f.ty.clone());
                            }
                        }
                    }
                    let mut t = Type::Void;
                    for stmt in &arm.body {
                        t = self.infer_expr_type(stmt, &mut arm_env)?;
                    }
                    if !body_diverges(&arm.body) {
                        arm_types.push((format!("{}.{}", tname, member), t));
                    }
                }
                match else_body {
                    Some(body) => {
                        if seen.len() == members.len() {
                            return Err(format!("{}:{}: the else arm never runs: every member of {} is matched", l, c, shown(&tname)));
                        }
                        let mut else_env = env.clone();
                        let mut t = Type::Void;
                        for stmt in body {
                            t = self.infer_expr_type(stmt, &mut else_env)?;
                        }
                        if !body_diverges(body) {
                            arm_types.push(("else".to_string(), t));
                        }
                    }
                    None => {
                        let missing: Vec<String> =
                            members.iter().filter(|(m, _)| !seen.contains(&m.as_str())).map(|(m, _)| format!("{}.{}", tname, m)).collect();
                        if !missing.is_empty() {
                            return Err(format!("{}:{}: match is missing {} (add those arms or an else arm)", l, c, shown(&missing.join(", "))));
                        }
                    }
                }
                // arms that always leave take the others' type
                let Some((first_name, first)) = arm_types.first() else { return Ok(Type::Void) };
                let mut joined = first.clone();
                for (name, t) in &arm_types[1..] {
                    match Self::meet(&joined, t) {
                        Some(m) => joined = m,
                        None => return Err(format!("{}:{}: match arm type mismatch: {} yields {}, {} yields {}", l, c, first_name, first, name, t)),
                    }
                }
                Ok(joined)
            }
            Expr::Addr { val, kind, span } => {
                let t = self.infer_expr_type(val, env)?;
                if let (AddrKind::Enum, Some(p)) = (kind, sealed(&t)) {
                    return Err(format!("{}:{}: enum.ord on {}: a type parameter is not an enum", span.0, span.1, p));
                }
                match (kind, &t) {
                    (AddrKind::Ptr | AddrKind::PtrGen, Type::Ptr(_)) | (AddrKind::Arr | AddrKind::ArrGen, Type::Array(_)) | (AddrKind::Enum, Type::Enum(_)) => Ok(Type::I32),
                    (AddrKind::PtrGen, _) => Err(format!("{}:{}: ptr.gen needs a (ptr S), got {}", span.0, span.1, t)),
                    (AddrKind::ArrGen, _) => Err(format!("{}:{}: arr.gen needs an (arr T), got {}", span.0, span.1, t)),
                    (AddrKind::Ptr, _) => Err(format!("{}:{}: ptr.addr needs a (ptr S), got {}", span.0, span.1, t)),
                    (AddrKind::Arr, _) => Err(format!("{}:{}: arr.addr needs an (arr T), got {}", span.0, span.1, t)),
                    (AddrKind::Enum, _) => Err(format!("{}:{}: enum.ord needs an enum value, got {}", span.0, span.1, t)),
                }
            }
        }
    }
}

/// Ops whose first argument is a linear-memory address.
fn is_address_op(op: &OpCode) -> bool {
    matches!(
        op,
        OpCode::MemLoad8
            | OpCode::MemLoad32
            | OpCode::MemLoad64
            | OpCode::MemStore8
            | OpCode::MemStore32
            | OpCode::MemStore64
            | OpCode::MemLoad8S
            | OpCode::MemLoad16U
            | OpCode::MemLoad16S
            | OpCode::MemStore16
            | OpCode::AtomicAdd
            | OpCode::AtomicCas
            | OpCode::AtomicLock
            | OpCode::AtomicUnlock
    )
}

/// Ops that modify (or lock) the word at their address argument.
fn is_write_op(op: &OpCode) -> bool {
    matches!(
        op,
        OpCode::MemStore8
            | OpCode::MemStore16
            | OpCode::MemStore32
            | OpCode::MemStore64
            | OpCode::AtomicAdd
            | OpCode::AtomicCas
            | OpCode::AtomicLock
            | OpCode::AtomicUnlock
    )
}

/// Static enforcement of the memory layout (AIPL_SPEC.md, "Memory layout")
/// for the one case that can be seen at check time: a literal address.
/// Bytes 0-3 are the heap cursor (reading it is fine, writing or locking it
/// is not), cells 4-63 are 4-byte-aligned runtime slots, bytes 64-1023 are
/// reserved, and everything from 1024 up is heap that should come from
/// `mem.alloc`. Computed addresses cannot be checked here; the VM catches the
/// most common consequence (locking a non-lock word) at runtime instead.
fn check_literal_address(op: &OpCode, args: &[Expr], l: u32, c: u32) -> Result<(), String> {
    if !is_address_op(op) {
        return Ok(());
    }
    let Some(Expr::Lit(Literal::Int(a), _)) = args.first() else {
        return Ok(());
    };
    let a = *a;
    if a < 0 {
        return Err(format!("{}:{}: {} at negative literal address {}", l, c, op, a));
    }
    if a < 4 && is_write_op(op) {
        return Err(format!(
            "{}:{}: {} at address {}: bytes 0-3 are the heap cursor owned by mem.alloc; locking it hangs and storing to it corrupts the allocator. Take memory from (mem.alloc n) instead",
            l, c, op, a
        ));
    }
    if (4..64).contains(&a) && a % 4 != 0 {
        return Err(format!(
            "{}:{}: {} at address {}: runtime cells 4-63 are 4-byte-aligned i32 slots (4, 8, 12, ...)",
            l, c, op, a
        ));
    }
    if (64..1024).contains(&a) {
        return Err(format!(
            "{}:{}: {} at literal address {}: bytes 64-1023 are the reserved runtime block. Take memory from (mem.alloc n) instead",
            l, c, op, a
        ));
    }
    Ok(())
}

/// The operations a program can use to touch memory it does not own or to
/// race with itself: raw loads and stores, atomics, threads, and the I/O and
/// system calls that take raw addresses. They need an unsafe context.
pub fn is_unchecked_op(op: &OpCode) -> bool {
    use OpCode::*;
    matches!(
        op,
        MemLoad8 | MemLoad32 | MemLoad64 | MemStore8 | MemStore32 | MemStore64 | MemLoad8S | MemLoad16U | MemLoad16S | MemStore16
            | AtomicAdd | AtomicCas | AtomicLock | AtomicUnlock
            | ThreadSpawn | ThreadJoin
            | FsOpen | FsRead | FsWrite | FsDelete | FsChmod | FsRename | FsOpendir | FsReaddir | FsStat
            | NetListen | NetAccept | NetRecv | NetSend | NetClose
            | ProcSpawn | ProcStream | ProcRead | ProcWrite | ProcClose | ProcWait | ProcExec
            | ArgsSizes | ArgsGet | EnvSizes | EnvGet
            | SysRandom
    )
}
