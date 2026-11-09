# AIPL Specification
## AI Programming Language, language version 1

> **This document defines the language.** It says what a program is (sections 1 and 2), what makes one valid (3, 4, 7, and 11), what it does (5 and 8), how it is run and what an implementation must do to be called AIPL (6), how errors are reported (9), and what the standard library is (10). It depends on no implementation. How the tools in this repository work (their commands, stages, backends, optimizer, and tests) is in [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md); the library's contract is in [docs/STD_REFERENCE.md](docs/STD_REFERENCE.md); the programs that referee a claim of conformance are in `tests/conformance/` (section 6.4). Sections 12 and 13 are informative: examples, and mistakes to avoid.

AIPL (AI Programming Language) is an unambiguous, statically typed S-expression systems language with run-time-checked contracts, designed for AI agents to generate. A program has one obvious spelling, and a mistake is caught early with a `line:col` diagnostic: that is what it optimises for, not brevity. With the standard library (section 10), a small I/O program is about twice the length of the Python equivalent (section 12.6), which is roughly the floor for a fully parenthesised, fully annotated syntax.

A program is compiled to WebAssembly (with WASI for I/O) or to a native executable (Linux, x86-64 and AArch64), or run in a reference interpreter; the three agree on everything section 5 defines. The toolchain is itself written in AIPL and compiles itself to the same bytes. Building it the first time needs only the `aipl` binary that a release carries (docs/IMPLEMENTATION.md, "The self-hosted toolchain"); the Rust toolchain that preceded it is used for development.

**How to read it.** The grammar (section 2) is checked against every program in the repository by `tools/grammar_check.py`, and the code blocks tagged `lisp` are compiled and run by the tests, so an example here is an example that works. "Is an error" means a conforming implementation rejects the program before it runs, with a diagnostic (section 9); "stops the program" and "traps" mean the same run-time event (section 5.6). A *form* is a parenthesised expression `(head ...)`; an *operation* is a form whose head is a built-in name such as `+` or `mem.load32`.
---

## 1. Syntax: One Canonical Text Form

AIPL source is `.aipl` text: a context-free, parenthesis-delimited S-expression syntax with no operator precedence, no indentation rules, and no statement separators. It is also the interchange format between agents: a program is printed back as canonical flat text, which every implementation accepts. (A MessagePack "binary AST" (`.baipl`) existed until P12; it was a dump of the compiler's internal Rust data structures with no stable format, nothing used it, and it was removed.)

---

## 2. Grammar (EBNF)

```ebnf
program        ::= "(" "module" identifier item* ")" ;
item           ::= import | language_decl | struct_def | enum_def | union_def | const_def | fn_def | interface_def | impl_def | test_def ;   (* in any order *)
import         ::= "(" "import" path [ "as" identifier ] ")" ;   (* path: std names like io, or dir/name *)
language_decl  ::= "(" "language" integer ")" ;                   (* the language version the module is written for (docs/LANGUAGE_VERSIONING.md) *)
pub_mark       ::= "(" "pub" ")" ;                                (* visible to other modules (section 11.1) *)
struct_def     ::= "(" "struct" ( identifier | generic_head ) "[" field* "]" ( value_mark | pub_mark | where )* ")" ;
value_mark     ::= "(" "value" ")" ;                              (* a value struct (section 4.E) *)
field          ::= identifier ":" type
                 | "(" ( "pub" | "read" ) identifier ":" type ")" ;   (* (pub f:T): other modules read and write it; (read f:T): they read it only *)
generic_head   ::= "(" identifier type_param+ ")" ;
enum_def       ::= "(" "enum" identifier "[" ( identifier | "(" identifier integer ")" )+ "]" pub_mark? ")" ;
const_def      ::= "(" "const" identifier ":" type const_value pub_mark? ")" ;   (* NAME in capitals; i32 i64 f64 bool str *)
const_value    ::= literal | identifier | "(" ( identifier | "+" | "-" | "*" | "/" | "%" | "^" ) const_value+ ")" ;   (* a constant expression (section 4.I) *)
union_def      ::= "(" "union" ( identifier | generic_head ) "[" variant+ "]" ( where | pub_mark )* ")" ;
variant        ::= "(" identifier field* ")" ;
test_def       ::= "(" "test" string expr* ")" ;                  (* section 4.L *)

fn_def         ::= "(" "fn" ( identifier | generic_head ) "[" param* "]" "->" type ( fn_clause | expr )* ")" ;   (* a clause may stand anywhere among the body: where it is makes no difference *)
fn_clause      ::= where | contract | pub_mark ;
where          ::= "(" "where" "(" ( identifier | "struct" | "plain" ) type_param ")" ")" ;   (* generics only (section 4.H) *)
interface_def  ::= "(" "interface" "(" identifier type_param ")" pub_mark? fn_sig+ ")" ;
fn_sig         ::= "(" "fn" identifier "[" param* "]" "->" type contract* ")" ;   (* no body *)
impl_def       ::= "(" "impl" "(" identifier type ")" fn_def+ ")" ;
param          ::= identifier ":" type ;
contract       ::= "(" ( "req" | "ens" | "inv" ) expr ")"
                 | "(" "unsafe" ")"                  (* the function is unsafe to call (section 3) *)
                 | "(" "no_leak" ")" ;               (* the function makes no memory that is never freed (section 3) *)

type           ::= "i32" | "i64" | "f32" | "f64" | "bool" | "str" | "void"
                 | "u8" | "i8" | "u16" | "i16"         (* storage types: a struct field or an array element only (section 4.A) *)
                 | "(" "result" type type ")"
                 | "(" "ptr" struct_ref ")"
                 | "(" "arr" type ")"
                 | "(" "fn" "[" type* "]" "->" type [ "(" "no_leak" ")" ] ")"   (* with (no_leak): only functions that keep no memory (section 4.G) *)
                 | "(" identifier type+ ")"           (* a generic struct or union instance: (vec.Vec i32), (option.Option str) *)
                 | type_param                         (* inside a generic definition *)
                 | identifier ;                       (* an enum, a union, or a value struct, e.g. Color, Shape, Vec3 *)
struct_ref     ::= identifier | "(" identifier type+ ")" ;      (* "m.S" for a struct from imported module m *)
field_inits    ::= "[" ( identifier ":" expr )* "]" ;   (* [x:1 y:2]: each field by name, in declaration order *)
field_ref      ::= identifier ;                       (* one symbol, StructName.fieldName, split at its last "." *)
fn_name        ::= identifier | "(" identifier type+ ")" ;   (* the second form instantiates a generic, or names an interface's function: (Hash.hash i32) *)
literal        ::= integer | integer64 | float | float32 | string | boolean ;

expr           ::= literal
                 | identifier
                 | "(" "let" identifier ":" type expr ")"
                 | "(" "set!" identifier expr ")"
                 | "(" "if" expr expr expr ")"
                 | "(" "when" expr expr* ")"
                 | "(" "for" identifier expr expr expr* ")"
                 | "(" "counted_loop" identifier expr expr expr expr* ")"   (* what for becomes: start, end exclusive, step *)
                 | "(" "while" expr expr* ")"
                 | "(" "call" fn_name expr* ")"
                 | "(" "block" expr* ")"
                 | "(" "unsafe" expr+ ")"            (* a block allowing unchecked operations (section 3) *)
                 | "(" "defer" expr+ ")"             (* runs when the function returns (section 4.F) *)
                 | "(" "return" [ expr ] ")" | "(" "break" ")" | "(" "continue" ")"
                 | "(" "cond" ( "(" expr expr+ ")" )+ "(" "else" expr+ ")" ")"
                 | "(" ( "ok" | "err" ) ":" type expr ")"   (* names the result's other side: (ok:E v), (err:T e) *)
                 | "(" "match_result" expr "(" "ok" identifier expr* ")" "(" "err" identifier expr* ")" ")"
                 | "(" "try" expr [ expr ] ")"       (* (try r), (try r (ref convert)) (section 4.F) *)
                 | "(" "try_opt" expr ")"
                 | "(" "try_or" expr expr ")"
                 | "(" "drop" expr ")"
                 | "(" "assert" expr [ expr ] ")"    (* section 3 *)
                 | "(" "abort" expr ")"
                 | "(" ( "print" | "println" | "eprintln" ) string ")"     (* section 4.K *)
                 | "(" "format" string ")"
                 | "(" "format_to" expr string ")"
                 | "(" "dbg" expr ")"
                 | "(" "expect" expr ")"             (* only inside a test (section 4.L) *)
                 | "(" "zero" type ")"
                 | "(" "arr.of" type expr expr* ")"  (* the array literal: the allocator, then the elements (section 4.M) *)
                 | "(" "new" struct_ref expr [ field_inits ] ")"   (* the allocator to make it from, then the fields by name (section 4.E) *)
                 | "(" "get" expr field_ref+ ")"     (* more than one: a path into value structs held inline (section 4.E) *)
                 | "(" "get" expr "(" identifier type+ ")" identifier ")"
                 | "(" "put" expr field_ref+ expr ")"   (* the names before the last item are a path, as for get *)
                 | "(" "put" expr "(" identifier type+ ")" identifier expr ")"
                 | "(" "sizeof" ( struct_ref | type ) ")"
                 | "(" "arr.new" type expr expr ")"    (* the allocator, then the length *)
                 | "(" "arr.get" type expr expr field_ref* ")"      (* a path reads one field of a value struct element *)
                 | "(" "arr.set" type expr expr field_ref* expr ")" (* a path writes one *)
                 | "(" "arr.len" expr ")"
                 | "(" "ptr.null" struct_ref ")" | "(" "arr.null" type ")"
                 | "(" "ptr.cast" struct_ref expr ")" | "(" "arr.cast" type expr ")"
                 | "(" "ptr.make" struct_ref expr expr ")" | "(" "arr.make" type expr expr ")"
                 | "(" "ptr.addr" expr ")" | "(" "arr.addr" expr ")"
                 | "(" "ptr.gen" expr ")" | "(" "arr.gen" expr ")"
                 | "(" "ref" fn_name ")"
                 | "(" "enum.cast" identifier expr ")" | "(" "enum.ord" expr ")"
                 | "(" "call_ref" type expr expr* ")"
                 | "(" "make" identifier ( expr* | field_inits ) ")"     (* Union.variant, or a value struct; fields by name or in order *)
                 | "(" "with" expr field_ref expr ")"  (* a copy of a value struct with one field changed *)
                 | "(" "make" "(" identifier type+ ")" identifier ( expr* | field_inits ) ")"   (* a generic union's variant (section 4.J) *)
                 | "(" "make" "(" identifier type+ ")" ( expr* | field_inits ) ")"   (* a generic value struct (section 4.H) *)
                 | "(" "match" expr match_arm+ ")"
                 | "(" op expr* ")" ;

match_arm      ::= "(" identifier [ "[" identifier* "]" ] expr* ")"   (* Name.member; binders for a union variant *)
                 | "(" "(" identifier type+ ")" identifier [ "[" identifier* "]" ] expr* ")"   (* a generic union's variant *)
                 | "(" "else" expr* ")" ;                             (* last *)

op             ::= arithmetic_op | bitwise_op | memory_op | atomic_op | comp_op
                 | conv_op | sys_op | fs_op | net_op | child_op | proc_op | thread_op | str_op ;

arithmetic_op  ::= "+" | "-" | "*" | "/" | "%" | "divu" | "remu" | "checked.add" | "checked.sub" | "checked.mul" | "wrap.add" | "wrap.sub" | "wrap.mul" ;
bitwise_op     ::= "^" | "shl" | "shr" | "shru" | "rotl" | "rotr" | "bitand" | "bitor"
                 | "bitnot" | "clz" | "ctz" | "popcnt" ;
memory_op      ::= "mem.load8" | "mem.load8_s" | "mem.load16_u" | "mem.load16_s" | "mem.load32" | "mem.load64"
                 | "mem.store8" | "mem.store16" | "mem.store32" | "mem.store64"
                 | "mem.alloc" | "mem.grow" ;
atomic_op      ::= "atomic.add" | "atomic.cas" | "atomic.lock" | "atomic.unlock" ;
comp_op        ::= "eq" | "neq" | "lt" | "lte" | "gt" | "gte" | "ltu" | "lteu" | "gtu" | "gteu" | "and" | "or" | "not" ;
conv_op        ::= "i64.extend_s" | "i64.extend_u" | "i32.wrap"
                 | "f64.convert_i64_s" | "i64.trunc_f64_s" | "f64.reinterpret_i64" | "i64.reinterpret_f64" | "f64.sqrt"
                 | "f32.demote" | "f64.promote" | "f32.convert_i32_s" | "f32.convert_i64_s" | "f64.convert_i32_s"
                 | "i32.trunc_f32_s" | "i32.trunc_f64_s" | "i64.trunc_f32_s" | "f32.reinterpret_i32" | "i32.reinterpret_f32" ;
sys_op         ::= "sys.print" | "sys.time" | "sys.monotonic" | "sys.random" | "sys.poll" | "sys.exit" ;
fs_op          ::= "fs.open" | "fs.read" | "fs.write" | "fs.close" | "fs.delete"
                 | "fs.opendir" | "fs.readdir" | "fs.stat" | "fs.chmod" | "fs.rename" | "fs.rmdir" | "fs.mkdir" ;
net_op         ::= "net.listen" | "net.accept" | "net.recv" | "net.send" | "net.close" | "net.connect" | "net.connect_host" | "net.tls_connect" | "net.poll" ;   (* std/net is the safe interface *)
child_op       ::= "proc.spawn" | "proc.stream" | "proc.read" | "proc.write" | "proc.close" | "proc.wait" | "proc.exec" | "proc.kill" | "proc.try_wait" | "proc.poll" ;   (* unsafe; granted by --allow-run *)
proc_op        ::= "args.sizes" | "args.get" | "env.sizes" | "env.get" ;
thread_op      ::= "thread.spawn" | "thread.join" ;
str_op         ::= "str.len" | "str.ptr" ;
```

**Lexical classes.** The grammar uses these names for tokens it does not spell out. A program is first cut into tokens (the five single characters `(` `)` `[` `]` `:`, the arrow `->`, strings, and symbols, which run to the next space, bracket, or colon), and each token is then a terminal of the grammar (`"let"`, `"+"`) or a member of one class:

| Class | A token that is |
|---|---|
| `identifier` | a symbol of letters, digits, `_`, `.`, and `-` that begins with a letter or `_`: a name, or a qualified one (`io.println`, `Shape.circle`, `counter.Counter.n`) |
| `path` | the same, with `/` allowed as well: the name in an `(import ...)` (`io`, `native/lower`) |
| `type_param` | an `identifier` that begins with an uppercase letter |
| `integer` | an `i32` literal: decimal, `0x` hexadecimal, or `0b` binary, with an optional sign and single `_` between digits (details below) |
| `integer64` | an integer as above with the suffix `i64` |
| `float` | decimal digits with a `.` and an optional exponent: `1.0`, `.5`, `1.5e3` |
| `float32` | a float with the suffix `f32` |
| `string` | text in double quotes, on one line; the escapes are `\n \t \r \0 \\ \"` |
| `boolean` | `true` or `false` |

Comments start with `;;` and run to the end of the line; they are not tokens. `tools/grammar_check.py` reads the EBNF above and every program in the repository by exactly these rules, and CI fails when the two disagree.

Notes on the grammar:
- A module body is imports followed by struct, enum, union, constant, and function definitions, in any order. `let` and `set!` are expressions that appear inside function bodies, not at module level.
- Comments start with `;;` and run to end of line.
- Integer literals are decimal, hexadecimal (`0xEDB88320`), or binary (`0b1010`), optionally signed (`-1`, `+1`, and `-0x10` are one token) and are `i32`; the prefix is lowercase, hexadecimal digits are either case, and a single `_` may stand between two digits (`0xFFFF_0000`, `0b1111_0000`); a radix literal's digits are the bit pattern, so `0xFFFFFFFF` is `-1`, and `0x100000000` is the same error as `4294967296`; the value must fit in 32 bits, read as signed or unsigned (`-2147483648` to `4294967295`, so `4294967295` is the bit pattern of `-1`); anything larger is an error suggesting the `i64` form.
An `i64` literal carries the suffix as part of the token: `42i64`, `-7i64`, `0xFFFFFFFFFFFFFFFFi64` (a hexadecimal or binary one may use all 64 bits, so that is `-1i64`; a sign negates, wrapping). Float literals must contain a `.` and at least one digit (`1.0`, `.5`, `1.5e3`; not `1` or `inf`) and are `f64`; with the suffix `f32` (`1.5f32`, `-0.1f32`, `1.0e10f32`) they are `f32`, the value the nearest `f64` to the text, then the nearest `f32` to that.
- Tokens are separated by spaces, tabs, and newlines (and CR). Any other whitespace character outside strings and comments, such as a form feed or a no-break space, is an error that names it. Strings are double-quoted and support the escapes `\n \t \r \0 \\ \"`; any other `\x` is an error. Booleans are `true` / `false`.
- `(call f ...)` takes a bare function name, never an expression. Imported functions are called as `(call modname.fn ...)`.
- `i64` is fully supported (section 8.1). `(fn [t1 t2] -> r)` is the type of a function reference (section 4.G).
- The generic forms (`generic_head`, `struct_ref`, `fn_name`, the instance type) are expanded away before type checking (section 4.H), and so are constants: an identifier naming a constant is its literal, and `Enum.member` is a value of that enum (section 4.I).
- There is no `mem.free` (memory from `mem.alloc` is never freed; freeable memory comes from `std/heap` or `std/alloc`) and there are no float loads or stores (`mem.load_f64` ...): floats live in struct fields and `(arr f64)`. The parser rejects all five with that advice.
- `(ptr i32)` is rejected (`ptr points to a struct; for a sequence of i32 use (arr i32)`), and so is the old `(arr T N)` form (`(arr T) takes no length`).
- In `(get p S.f)` / `(put p S.f v)` the field reference is one symbol, `StructName.fieldName`, split at its last `.`, so `(get p compiler.Node.next)` names field `next` of struct `compiler.Node`.

---

## 3. Type System & Contracts

AIPL is strongly and statically typed. Every parameter, return type, `let`, and struct field carries an explicit type annotation; the checker infers only the types of expressions.

### Scalar Primitives
- `i32`, `i64`: 32-bit and 64-bit signed integers.
- `f32`, `f64`: 32-bit and 64-bit IEEE-754 floating point numbers.
- `u8`, `i8`, `u16`, `i16`: storage types, 1 or 2 bytes, unsigned or signed. Only a struct field (of a struct used through a pointer) or an array element is one; reading it gives an `i32` (zero- or sign-extended), and writing it keeps the low 8 or 16 bits of an `i32` (section 4.A).
- `bool`: Logical `true` / `false`.
- `str`: Immutable UTF-8 string pointer + length.

### Compound Types
- `(result T_ok T_err)`: the type of `ok`/`err` values, consumed by `match_result` (section 4.F).
- `(ptr S)`: a pointer to a struct `S`. `(arr T)`: an array of `T`, made by `arr.new` from an allocator. Both are checked strictly, are never interchangeable with `i32` or with each other, and lower to `i32` in wasm (section 4.E).
- `(fn [t1 ...] -> r)`: a reference to a function with that signature, made by `(ref f)` and called with `call_ref` (section 4.G).
- `Name` for an enum `Name`: one of its members (section 4.I). An `i32` at run time, a distinct type to the checker.
- `Name` for a union `Name`: one of its variants, carrying that variant's fields (section 4.J). A value, held inline like a value struct, never null.
- `(Name t...)`: a generic struct instantiated with types `t...` (section 4.H), used behind `(ptr ...)` like any struct.

### Contracts
`(req e)` (precondition) and `(ens e)` (postcondition) are `bool` expressions placed before the body; inside `ens`, `res` is the return value. The checker type-checks them; nothing is proven statically. The VM evaluates every `req` before the body and every `ens` after it (including after an early `return`) and fails the call with `Pre-condition failed in 'f' at L:C: (req ...) with x = ...` (or `Post-condition`, which also shows `res`). Compiled code (wasm, and native through it) checks them the same way and stops with the same message, without the position: `Pre-condition failed in 'f': (req ...) with x = ...` (section 7.6). `(inv e)`, an invariant, is checked at both ends: before the body, with the `req`s (`Entry invariant failed in 'f' ...`), and after it, with the `ens`s (`Exit invariant failed in 'f' ...`, showing `res`); it sees the parameters but not `res`.
```lisp
(fn read_slot [slots:(arr i32) i:i32] -> i32
  (req (gte i 0))
  (req (lt i (arr.len slots)))
  (ens (gte res 0))
  (arr.get i32 slots i))
```

**`(assert c)`, `(assert c msg)`, and `(abort msg)`** check inside a body, where `req` and `ens` cannot. `c` is a `bool`; when it is false the program stops, in every backend, as a failed contract does: the message, then the call chain. The message is `Assertion failed: FILE:L:C: (assert c)` (the assert as written, on one line), then `: left L, right R` when `c` is a comparison (`eq`, `neq`, `lt`, `lte`, `gt`, `gte`) whose sides the formatting forms print (section 4.K; each side is evaluated once), then `: MSG` when the assert has a message, which is a `str` expression evaluated only when `c` is false: `(assert (lt i n) (format "i = {i}, n = {n}"))` stops with `Assertion failed: sum.aipl:9:5: (assert (lt i n) (format "i = {i}, n = {n}")): left 7, right 5: i = 7, n = 5`.
`(abort msg)` stops with `msg` alone. Both are statements of type `void`, and a branch that ends in an `abort` fits any type, as one that returns does: `(if (gt n 0) n (abort "no negatives here"))` is an `i32`. An assert is never switched off, like a contract. A module that uses one imports `std/assertion`, as one that uses a formatting form imports `std/show` (the compiler's own source uses neither `assert` nor `abort`, so the seed does not read it).
What builds a failed assert's or an abort's message is no leak: `(no_leak)` functions may use both.

### Memory that is never freed, and `(no_leak)`

What `alloc.forever` hands out, `mem.alloc`, and the `str` values made at run time (`format`, `str.concat`, `buf.to_str`) are never freed: right for a program that runs and exits, fatal for one that runs for days. A function with the clause `(no_leak)` promises to make no such memory, and the checker holds it to that: it may not call `alloc.forever` (so `(new S (call alloc.forever) [...])` is refused, and `(new S a [...])` with an allocator it is handed is not), use `mem.alloc` or `format`, call through a function reference whose type does not promise `(no_leak)` (section 4.G), or call a function that may do any of these, directly or through what it calls. The error names the first site that breaks the promise and the chain down to the allocation:

```
13:5: uses_concat is marked (no_leak) but calls str.concat, which may make memory that is never freed (str.concat calls str.room; str.room uses mem.alloc)
```

What the function allocates through an `alloc.Allocator` it is handed is its caller's choice, so a server passes each request's handler an arena and resets it after. Inside `unsafe` the promise is the programmer's word, as every unsafe promise is: that is how std's allocators, which grow their own pools with `mem.alloc` and hand that memory out again, are marked `(no_leak)` (`alloc.raw`, `alloc.free_raw`, `alloc.default`, `heap.raw`, `arena.raw`, `arena.fast_raw`). A function not marked is not checked, only described. Results and unions are values (sections 4.F, 4.J), so building one allocates nothing and there is no hidden allocation to count.

### Unchecked operations and `unsafe`

Some operations reach memory the program has not been handed through a typed pointer or array, or let threads race, and nothing checks them. They are allowed only in an **unsafe context**:

- **Raw memory:** `mem.load8`, `mem.load32`, `mem.load64`, `mem.store8`, `mem.store32`, `mem.store64`.
- **Casts to a pointer or array:** `ptr.cast`, `arr.cast`, `ptr.make`, `arr.make` (`enum.cast` is checked: a non-member reaches `match`'s `unreachable`).
- **Atomics and threads:** `atomic.add`, `atomic.cas`, `atomic.lock`, `atomic.unlock`, `thread.spawn`, `thread.join` (AIPL does not check for data races).
- **Host calls that take raw addresses or handles:** `fs.open`, `fs.read`, `fs.write`, `fs.delete`, `fs.chmod`, `fs.rename`, `fs.rmdir`, `fs.mkdir`, `sys.poll`, `fs.opendir`, `fs.readdir`, `fs.stat`, `args.sizes`, `args.get`, `env.sizes`, `env.get`, `sys.random`, the sockets `net.listen`, `net.accept`, `net.recv`, `net.send`, `net.close`, `net.connect`, `net.connect_host`, `net.tls_connect` (std/net wraps them), and the process operations `proc.spawn`, `proc.stream`, `proc.read`, `proc.write`, `proc.close`, `proc.wait`, `proc.exec`, `proc.kill`, `proc.try_wait`, `proc.poll`, and `net.poll`.
- **Calling, or taking `(ref f)` of, a function marked `(unsafe)`.**

Getting an address as a number (`mem.alloc`, `str.ptr`, `ptr.addr`, `arr.addr`) is checked: a number cannot be read through without one of the operations above.

An unsafe context is the inside of an `(unsafe e...)` block, which is a `block` in every other way (its value is its last expression; its `let`s end with it), or the whole of a function with the `(unsafe)` clause, its contracts included. The two mean different things:

- `(unsafe e...)` says "these operations are correct as used here": the function vouches for them, and its callers need nothing.
- `(fn f [...] -> T (unsafe) ...)` says "calling `f` can break memory if its arguments are wrong", typically because it takes a raw address. Its callers need an unsafe context.

```lisp
(module unsafe_demo
  ;; unsafe to call: writes wherever `at` points
  (fn put_answer [at:i32] -> void (unsafe)
    (mem.store32 at 42))

  ;; safe to call: the address is its own fresh memory
  (fn answer [] -> i32
    (unsafe
      (let p:i32 (mem.alloc 4))
      (call put_answer p)
      (mem.load32 p)))

  (fn main [] -> i32 (println "{(call answer)}") 0))    ;; prints 42
```

Outside an unsafe context the checker rejects the operation and says how to allow it: `3:5: mem.store32 is unchecked: write it inside (unsafe ...), or mark the function (unsafe)`, and for a call, `'put_answer' is marked (unsafe): use it inside (unsafe ...), or mark this function (unsafe)`. `unsafe` changes nothing at run time: both forms compile exactly as `block` does.

The standard library uses unchecked operations inside functions that are safe to call (`io.read_file`, `str.byte_at`, `vec.push`, ...); the few that take raw addresses are `(unsafe)`: `str.bytes`, the `fmt.*_to_bytes` writers and `fmt.f64_fixed`, `float.from_decimal`, and `alloc.custom`.

**What this does and does not guarantee.** Code without `unsafe` cannot read or write memory through a raw address, cast an address into a pointer, or start a thread. Nor can it forge the standard library's descriptors: their raw addresses are in fields other modules may only read (section 11.1), and every library function that takes a raw address is unsafe to call.
Nor can it use a pointer after its object is freed: every pointer carries its object's generation, and every access compares the two, so a pointer kept past `heap.destroy`, `alloc.destroy`, `free_array`, or `arena.reset` stops the program with `Use after free` (section 4.A, "Generation checks"). So code without `unsafe` is memory-safe, with these limits: threads and atomics are unsafe, and AIPL does not check for data races (`std/par` runs work in parallel without `unsafe`, and without races: what its workers share is plain or their own copy); after an arena reuses its memory, a stale pointer is caught unless the new data at its generation word happens to equal its old generation (a scrambled 32-bit number; the heap has no such case); a fast arena (`arena.Fast`) catches a stale pointer only once its memory is reused; and a pointer made by `ptr.cast` is not checked at all.

---

## 4. Systems Operations (Memory & Atomics)

### A. Raw WebAssembly Linear Memory Loads & Stores
- `(mem.load32 ptr)` -> Reads 4 bytes from linear memory offset `ptr` (`i32.load`).
- `(mem.store32 ptr val)` -> Writes 4 bytes to linear memory offset `ptr` (`i32.store`).
- `(mem.alloc size)` -> Bump allocation: returns the current heap cursor (the `i32` at address 0) and advances it by `size` rounded up to a multiple of 8, so every block is 8-aligned (the heap start is too): atomics, `i64`/`f64` values, and WASI out-parameters placed in any allocated block are aligned. The claim is one atomic add, so threads may allocate concurrently. If the new cursor is past the end of memory, memory grows by the pages needed to cover it (up to the 32768-page (2 GiB) cap, or `--max-memory`'s); when it cannot, the program stops there, at the `mem.alloc`, with `Out of memory: the limit is 2048 MiB` (the limit in MiB) and the call chain, alike in every backend. `alloc.forever` allocates the same way. Never frees. One cursor is shared by the VM, compiled wasm, and AIPL code.
- `(mem.grow pages)` -> Grows linear memory by `pages` × 64 KiB. Returns the previous size in pages, or `-1` if the 32768-page (2 GiB) maximum would be exceeded. The cap is 2 GiB so that every address is a non-negative `i32`: signed comparisons on addresses stay correct.
- There is no `mem.free`: memory from `mem.alloc` is never freed. Memory to be freed comes from an allocator in the standard library, which `new` and `arr.new` name (section 4.E): `std/heap` frees single objects (with checks for double frees and writes after free), `std/arena` frees a region at once, and `std/alloc` makes any of them one `Allocator` value, which the collections take (section 10).

See "Memory layout" (section 7.9) for the reserved runtime block below address 1024.

**8- and 16-bit memory.** `(mem.load8 ptr)` reads a byte zero-extended and `(mem.load8_s ptr)` sign-extended; `(mem.load16_u ptr)` / `(mem.load16_s ptr)` read 2 bytes likewise; `(mem.store8 ptr v)` / `(mem.store16 ptr v)` write the low 8 / 16 bits of an `i32`. All give or take `i32` (wasm `i32.load8_u`, `i32.load8_s`, `i32.load16_u`, `i32.load16_s`, `i32.store8`, `i32.store16`).

**Storage types.** `u8`, `i8`, `u16`, and `i16` are for data laid out tightly: a struct field (`(struct Pixel [r:u8 g:u8 b:u8 a:u8])`, 4 bytes) or an array element (`(arr u8)`, one byte each). `get` and `arr.get` give an `i32`, `put` and `arr.set` take one and keep its low bits (so `(put p Rec.tag 300)` stores 44), and `(sizeof u16)` is 2. Anywhere else (a parameter, a `let`, a result, a value struct's field) the type is an error that says so: `u8 is a storage type: only a struct field or an array element is one (reading it gives an i32); use i32`.

**`f32` conversions.** Floats convert only explicitly, as in wasm: `(f32.demote x)` rounds an `f64` to the nearest `f32`, `(f64.promote x)` is exact; `(f32.convert_i32_s n)`, `(f32.convert_i64_s n)`, and `(f64.convert_i32_s n)` round a signed integer to the nearest float; `(i32.trunc_f32_s x)`, `(i32.trunc_f64_s x)`, and `(i64.trunc_f32_s x)` drop the fraction and stop the program when `x` is NaN (`invalid conversion to integer`) or the result does not fit (`integer overflow`), at exactly wasm's bounds, natively too; `(f32.reinterpret_i32 n)` and `(i32.reinterpret_f32 x)` give the same 32 bits as the other type.

### B. Strings

A `str` is a pointer to immutable UTF-8 bytes preceded by a 4-byte little-endian length. String literals are interned once per module, in first-use order, into the data area that starts at address 1024; the heap starts at the first 8-aligned address after them (section 7.9). Literals are read-only: a store into one traps like a store into the runtime block.
The literals and the heap start must fit in the first 1 MiB of memory. `(str.len s)` returns the byte length as `i32`; `(str.ptr s)` returns the address of the bytes as `i32`, which is how a string becomes the `(ptr, len)` pair that `fs.*` and other pointer-taking ops expect (identity in both backends: the VM places the literals at the same addresses as wasm when it loads a module, and copies a string onto the heap only if it is not one of the first-loaded module's literals).
Two literals with the same text share one address. Strings do not compare with `eq`, `neq`, or `lt`: a string made at run time has an address of its own, so `eq` would compare addresses compiled and contents in the VM. Compare contents with `(call str.same a b)` and order with `(call (cmp.Ord.compare str) a b)`; `(eq s "x")` is the error `eq on str: compare strings by content with (call str.same a b), ...`.
There is no `+` on strings (`+ does not join strings: join two with (call str.concat a b), ...`).

**Which kind of text to use.** AIPL has three, and converts between them only by calling a function, so each copy is visible:

| Kind | What it is | Use it for | Get one from |
|---|---|---|---|
| `str` | immutable UTF-8, never freed | literals, names, keys, finished messages; what `io.println` and `sys.print` take | a literal; `(format "...")` (section 4.K); `str.concat a b`; `str.from_bytes b` (a copy of a byte slice, which must be UTF-8); `buf.to_str b` (a copy of a builder's contents); `date.to_str d` |
| `str.Bytes` | a view of bytes, no copy: a value (three slots: a checked byte array, a start, a length), so making one allocates nothing; reading a view of memory that has been freed stops the program ("Use after free"), as a pointer does | reading and parsing: file contents, slices, tokens | `str.from_str s` (a view of a `str`); `str.slice b start len`; `buf.bytes b`; `io.read_file` |
| `(ptr buf.Buf)` | a growable buffer that owns its bytes | building text piece by piece, in a loop | `buf.make a capacity`, then `(format_to b "...")` (section 4.K), `buf.push_str`, `push_bytes`, `push_int`, `push_i64`, `push_f64`, `push_byte` |

A `str` made at run time is never freed: it has no generation for a use to be checked against, so its memory stays for the rest of the run. Joining in a loop with `str.concat` therefore keeps every intermediate string; push onto one `buf` instead and call `buf.to_str` once at the end.

### C. Host I/O (WASI)

Compiled modules import only the host functions they use from `wasi_snapshot_preview1`, so a module without I/O has no import section.

| Op | Meaning, and the WebAssembly import | Result |
|---|---|---|
| `(sys.print s ...)` | writes each `str` argument and a newline to standard output; import: `fd_write` to fd 1 with a two-entry iovec (bytes, then `"\n"`); `str` arguments only | `void` |
| `(fs.open ptr len flags)` | opens the file named by the path; `flags` 0 opens it for reading, any other value creates it or truncates it, for writing; import: `path_open` on the preopened directory (fd 3): `oflags = CREAT\|TRUNC` and rights `FD_READ\|FD_WRITE` when `flags != 0`, else rights `FD_READ` | new fd, or minus its WASI errno |
| `(fs.read fd buf max)` | import: `fd_read` with one iovec | bytes read, or minus its WASI errno |
| `(fs.write fd buf len)` | import: `fd_write` with one iovec | bytes written, or minus its WASI errno |
| `(fs.close fd)` | import: `fd_close` | `0`, or minus its WASI errno |
| `(fs.delete ptr len)` | import: `path_unlink_file` on fd 3 | `0`, or minus its WASI errno |
| `(fs.rename old_ptr old_len new_ptr new_len)` | import: `path_rename`, each path's directory (fd 3, or 4 for a path from `/`) chosen as `fs.delete` chooses it | `0`, or minus its WASI errno. A relative path stays in the working directory (`-63` for a `..` or link leading out, on either side); under `--sandbox` there is no `/` (`-8`) |
| `(fs.rmdir ptr len)` | import: `path_remove_directory` on fd 3 (or 4 for a path from `/`), chosen as `fs.delete` chooses it | `0`, or minus its WASI errno: a missing path `-44`, a directory with anything in it `-55`, a file or a link (even to a directory) `-54`. A relative path stays in the working directory (`-63` for a `..` or link leading out); under `--sandbox` there is no `/` (`-8`) |
| `(fs.mkdir ptr len)` | import: `path_create_directory` on fd 3 (or 4 for a path from `/`), chosen as `fs.delete` chooses it | `0`, or minus its WASI errno: a path that exists (a file or a link too) `-20`, a missing parent `-44`, a file as the parent `-54`. A relative path stays in the working directory (`-63` for a `..` or link leading out); under `--sandbox` there is no `/` (`-8`) |
| `(fs.opendir ptr len)` | opens a directory for `fs.readdir`; its names come with `.` and `..` first; import: `path_open` on fd 3 with `oflags = DIRECTORY` and rights `FD_READDIR` | new fd (for `fs.readdir` and `fs.close`), or minus its WASI errno |
| `(fs.readdir fd buf len cookie:i64)` | import: `fd_readdir`: WASI dirents (`d_next` u64, `d_ino` u64, `d_namlen` u32, `d_type` u8, 3 bytes, then the name) from the entry `cookie` names (`0` the first, else a `d_next`); the last one may be cut off | bytes written (`len` when more may follow), or minus its WASI errno |
| `(fs.stat ptr len out)` | import: `path_filestat_get` on fd 3, following symbolic links: a 64-byte WASI filestat at `out` (the filetype u8 at 16: 3 a directory, 4 a regular file; the size at 32; access, modification, and change times at 40, 48, 56 in nanoseconds) | `0`, or minus its WASI errno |
| `(fs.chmod ptr len mode)` | import: `chmod(dirfd, ptr, len, mode)` from AIPL's own import module `aipl_sys_v1`, the directory (fd 3 or 4) chosen as `fs.delete` chooses it | `0`, or minus a WASI errno. `mode` is the permission bits, 0 to 0o777; a bit outside them (setuid, setgid, sticky) is `-28`. A relative path stays in the working directory (a `..` or a symbolic link leading out is `-63`); under `--sandbox` there is no `/` (`-8`). The VM's paths are ordinary: nothing confines them. A path whose length runs past memory is `-21`, a value and not a trap |
| `(proc.spawn path len args args_len stdio)` | starts the program at the path exactly as given (no `PATH` search), if the program was granted it (`*` for any); the arguments are `args_len` bytes at `args`, each followed by a zero byte (a last word without one counts), the first the program's own name; `stdio` is three 2-bit fields, standard input in bits 0-1, output in 2-3, error in 4-5: 0 the program's own, 1 a pipe, 2 the null device, and 3 for the error only, the output's; import: `spawn` from `aipl_sys_v1` | a process handle, or minus an errno: `-76` (not granted), `-44` and the other errnos of a program that cannot start, `-28` for a `stdio` outside those values, `-21` for a path or arguments outside memory, `-1` for more than 65,536 bytes of both. The environment and working directory are the program's. Handles are not descriptors, and what a number is differs between backends: print none |
| `(proc.stream p which)` | a pipe `spawn` made; import: `aipl_sys_v1.stream` | the pipe of process `p`'s input (0), output (1), or error (2); `-1` if that stream is not a pipe, `-8` if `p` is not a process this program started |
| `(proc.read h buf max)`, `(proc.write h buf len)` | import: `aipl_sys_v1.read`, `.write` | bytes moved: a read waits for some and gives `0` at the end; `-8` for a handle that is not a pipe this program has open, or one of the wrong way round (the input is written, the output and error read), `-21` for a buffer outside memory, `-64` for a write to a program that has gone |
| `(proc.close h)` | drops the pipe, or the process (which goes on running); import: `aipl_sys_v1.close` | `0`, or `-8`. Closing the input is how a program is told its input has ended |
| `(proc.wait p)` | import: `aipl_sys_v1.wait` | the program's exit status 0 to 255, or 256 plus the number of the signal that ended it (`kill -9` is 265); the same again on a second wait; `-8` for a process that is not this program's |
| `(proc.try_wait p)` | import: `aipl_sys_v1.try_wait` | `proc.wait` that does not wait: the status if the program has ended (and it is waited for, so the same again), `-6` (EAGAIN) while it runs, `-8` for a process that is not this program's |
| `(proc.kill p sig)` | sends the signal to the program; import: `aipl_sys_v1.kill` | `0`; `-8` for a process that is not this program's or has been waited for (its id may be another process's now), `-28` for a signal outside 1 to 64. One thread may kill what another is waiting for |
| `(proc.poll h events timeout_ms)` | import: `aipl_sys_v1.poll` | waits at most `timeout_ms` (-1: no limit, 0: only look) until pipe `h` can be read (`events` 1: the read end of a program's output or error: data, its end, or an error) or written (2: the write end of its input, room for a pipe's worth): `events` if so, `0` if the time ran out, `-8` for a handle that is not such a pipe, `-28` for other events |
| `(proc.exec path len args args_len)` | import: `aipl_sys_v1.exec` | does not return when it works (the program ends with the new program's status); otherwise minus an errno, as `proc.spawn` gives |
| `(net.listen addr len port)` | listens on the address and port, if they were granted; import: `listen` from AIPL's own import module `aipl_net_v1` | a socket's handle; `-1` (a malformed address, a port outside 0-65535, or the system's refusal), or `-2` if not granted. The address is its `len` bytes at `addr`: 4 for IPv4, 16 for IPv6 |
| `(net.accept h)` | import: `aipl_net_v1.accept` | the connection's handle, or `-1` |
| `(net.connect addr len port)` | connects to the address and port, if they were granted (`ADDR:*` grants any port); import: `connect` from `aipl_net_v1` | the connection's handle (for `net.recv`, `net.send`, `net.close`); `-1` (a malformed address, a port outside 0-65535, or a refused or failed connection), or `-2` if not granted. The address is its `len` bytes at `addr`: 4 for IPv4, 16 for IPv6; there is no name resolution |
| `(net.connect_host name len port)` | connects to the host whose name (or address as text) is the `len` bytes at `name`, found by the system's resolver, if that name (ignoring case) or address and the port were granted; import: `connect_host` from `aipl_net_v1` | the connection's handle; `-1` (a malformed name, a bad port, or a refused or failed connection), `-2` if not granted, `-3` where there is no resolver (a native executable), `-5` if the name does not resolve. A name grant lets the program reach whatever the name resolves to |
| `(net.tls_connect h name len)` | makes connection `h` secure, as a client of the server whose certificate must be trusted (the Mozilla roots, and the certificates of the PEM file `AIPL_CA_FILE` names) and carry the name at `name` (a host name, or an address as text); import: `tls_connect` from `aipl_net_v1` | `0` (the handle now carries plain bytes over TLS: `net.recv`, `net.send`, `net.close` as before); `-1` if the handshake fails (an untrusted or mismatched certificate, a server that does not speak TLS), `-3` where there is no TLS (a native executable), `-6` for a bad name. A server's close without TLS's close_notify reads as the end |
| `(net.recv h buf max)` | import: `aipl_net_v1.recv` | bytes read into `buf`, `0` at the end of the connection, or `-1` (also for a buffer outside memory: not a trap) |
| `(net.send h buf len)` | import: `aipl_net_v1.send` | bytes written (perhaps fewer than `len`), or `-1` |
| `(net.poll h events timeout_ms)` | waits on the socket (a secure connection's already decrypted bytes count); import: `aipl_net_v1.poll` | waits at most `timeout_ms` (-1: no limit, 0: only look) until socket `h` can be read (`events` 1: data, the end of the data, an error, or, for a listening socket, a connection to accept) or written (2): `events` if so, `0` if the time ran out, `-1` for a handle that is not an open socket, other events, or an error |
| `(net.close h)` | shuts a connection down, then closes it; import: `aipl_net_v1.close` | `0`, or `-1` for a handle that is not an open socket. Socket handles are not file descriptors: `fs.close` does not close one, and the numbers mean different things in different backends (std/net keeps them private) |
| `(sys.exit code)` | ends the program; import: `proc_exit` | never returns |
| `(sys.time)` | the system clock; import: `clock_time_get` (realtime) | `i64` nanoseconds since 1970-01-01 UTC |
| `(sys.monotonic)` | a monotonic clock; import: `clock_time_get` (monotonic) | `i64` nanoseconds from an arbitrary fixed start; never decreases. Use for durations |
| `(sys.random ptr len)` | the system's secure random generator; import: `random_get` | `0`, or minus its WASI errno; fills `len` bytes at `ptr` from the OS's secure generator |
| `(sys.poll subs out n nevents)` | waits as the one relative clock subscription at `subs` says; the event goes to `out`, the count 1 to `nevents`; import: `poll_oneoff`, as it is (any subscriptions WASI hosts give) | `0`, or minus its WASI errno. The VM and native executables take one relative clock subscription (tag 0, flags 0): `n` not 1 is `-28`, another kind `-58`; a wait a signal cuts short ends early, as success. `std/time.sleep` builds the subscription |
| `(args.sizes count_ptr size_ptr)` | the program's argument list; import: `args_sizes_get` | `0`, or minus its WASI errno; writes the argument count and the total bytes of the NUL-terminated arguments |
| `(args.get argv_ptr buf_ptr)` | import: `args_get` | `0`, or minus its WASI errno; writes one u32 address per argument at `argv_ptr` and the NUL-terminated arguments at `buf_ptr` |
| `(env.sizes count_ptr size_ptr)` | the environment (a host may replace it); import: `environ_sizes_get` | as `args.sizes`, for `KEY=VALUE` entries |
| `(env.get env_ptr buf_ptr)` | import: `environ_get` | as `args.get` |

**Failures.** A WASI call that fails gives minus its WASI errno, the same number in every backend: `-44` (ENOENT) for a missing file or a missing directory on the path, `-54` (ENOTDIR) for a file used as a directory, `-2` (EACCES) for no permission, `-31` (EISDIR) for writing or deleting a directory, `-8` (EBADF) for a handle that is not open or cannot be read (a directory: it opens, and wasmtime's directory handles cannot be read, so the VM and native executables say the same where the OS would say EISDIR), `-63` (EPERM) for a path that leaves the directory it is under (`..`, or a symbolic link out; natively the kernel's EXDEV), `-32` (ELOOP) for a directory opened through a symbolic link, which is not followed. Every implementation reports the same numbers for the same failure; one that finds an error not listed here reports EIO (`-29`). std/io's `io.error_of` turns one into an `io.Error`, which is how programs see it (section 10).

Paths are `(ptr, len)` byte ranges in linear memory (`(str.ptr s)` / `(str.len s)` produce them from a string). In the VM they are ordinary paths. Under WASI a relative path resolves in the first preopened directory (fd 3, the working directory) and an absolute path (`/...`) in the second (fd 4), which a host grants as `/` when the program may use absolute paths: `wasmtime run --dir . --dir / prog.wasm`.
Without that grant, opening an absolute path fails with `-1`. File descriptor 0 is stdin in both backends (`(fs.read 0 buf n)`; `io.read_stdin` reads all of it). A clock failure traps (it does not happen on supported hosts). File descriptors 1 and 2 are stdout and stderr in both backends, so `(fs.write 1 buf n)` prints raw bytes. The standard library builds printing of numbers and whole-file reads on exactly these primitives (section 10).
Every WASI errno collapses to `-1`, matching the VM. Argument expressions are evaluated left to right in both backends. The `args.*`/`env.*` out-parameters `count_ptr`, `size_ptr`, and the address tables must be 4-aligned: WASI hosts trap on a misaligned one and the VM fails with `... is not 4-aligned, which WASI requires`. Like `fs.read`, the host writes through these addresses without the store guard.
Under WASI the host decides what the program sees (wasmtime forwards an environment variable only with `--env`). Use `std/os` rather than these ops directly; time code with `std/time`.

### D. Atomics and threads

> **Threads are experimental.** `thread.spawn` and `thread.join` work in the VM, in native executables, and under `aipl-run` with wasmtime up to version 46, but a threaded module needs the wasi-threads proposal, which wasmtime removed in version 47 and which the language does not control; threads need `unsafe`, and AIPL does not check data races. They are not part of the language version 1 promise (docs/LANGUAGE_VERSIONING.md): they may change or be replaced without a new language version. `std/par` (parallel maps and reductions without `unsafe`) is built on them and is experimental with them.

| Form | Type | Semantics |
|---|---|---|
| `(atomic.add p v)` | `i32` | atomically adds `v` to the word at `p`; returns the previous value. Every atomic address must be 4-aligned (any word in a `mem.alloc` block at a multiple-of-4 offset is); otherwise the VM fails and wasm traps |
| `(atomic.cas p expected new)` | `bool` | if the word at `p` is `expected`, atomically replaces it with `new` and returns `true`; otherwise `false` |
| `(atomic.lock p)` | `void` | acquires the lock word at `p` (0 free, 1 held), waiting while it is held. A word holding anything else is not a lock: the VM fails with an error and compiled code traps |
| `(atomic.unlock p)` | `void` | releases the lock at `p` and wakes waiters; the word must be 1 (held), otherwise an error / trap |
| `(thread.spawn worker arg)` | `i32` | runs `worker(arg)` on a new OS thread; `worker` is a `(fn [i32] -> i32)` reference. Returns a handle. To hand a worker data of its own, use `std/thread`'s `(call (thread.spawn_with C) worker ctx)`, with `worker` a `(fn [C] -> i32)`: the context keeps its type |
| `(thread.join h)` | `i32` | waits for the thread to finish and returns `worker`'s result |

All threads share linear memory and the allocator: `mem.alloc` and `alloc.forever` claim their blocks with one atomic add on the heap cursor, and a heap (`alloc.default`'s included) takes its lock to allocate and free, so threads may allocate freely. Atomic addresses pass the store guard (section 7.9) like stores. A handle is the address of a 16-byte thread record `[done:i32 result:i32 fn:i32 arg:i32]` allocated by `thread.spawn`, identically in both backends; treat it as opaque. Each thread has its own file-descriptor table (stdin/stdout/stderr are shared). Output from concurrent threads may interleave: a `sys.print`'s text and its newline are two writes, so lines from two threads can join; use a lock when whole lines matter.

**Compiled threads.** A module that uses `thread.spawn` is a *threaded module* (section 6.2): it imports a shared memory (`"env" "memory"`) and the wasi-threads function `"wasi" "thread-spawn"`, and exports `wasi_thread_start`. Wasmtime removed wasi-threads in version 47, so these modules run under AIPL's own runner, or any host that still implements wasi-threads; they do not run under the stock `wasmtime` CLI or in the browser demo. Atomics alone need nothing special and work in every module. Wasmtime classes wasm threads as a tier 2 feature and requires shared memory to be enabled in its configuration (`Config::shared_memory`).

### E. Structs, Pointers, and Arrays

```lisp
(struct Point [x:i32 y:i32])
(struct Mixed [flag:bool val:i64 tag:i32])
(struct Node [val:i32 next:(ptr Node)])
```

Struct definitions are module-level. Layout rules, identical in every implementation:

- `i32`, `f32`, `bool`, `str`: 4 bytes, align 4. `i64`, `f64`, `(ptr S)`, `(arr T)`: 8 bytes, align 8 (a pointer or array is its address, then its generation). A `bool` field is a full 4-byte word.
- Fields are laid out in declaration order; each offset is rounded up to the field's alignment.
- The total size is rounded up to the largest field alignment. `(sizeof Point)` is 8, `(sizeof Mixed)` is 24 (`flag` 0, padding, `val` 8, `tag` 16, padding).

**Pointer and array types are strict.** `(ptr S)` and `(arr T)` are distinct from `i32` and from each other: a `(ptr Node)` cannot be passed where a `(ptr Point)` is expected, an `i32` is never accepted as either, and `arr.get` on an `(arr i32)` must say `i32`. Pointers and arrays have no arithmetic and compare only with `eq`/`neq`. The only conversions are the explicit casts below, which are the unchecked points in a program and easy to find. A struct is used only behind `(ptr S)`, unless it is declared a value struct (below), which is never behind a pointer. Arrays are their own type rather than a `(ptr T)` because `arr.get`, `arr.set`, and `arr.len` read the length header that only `arr.new` writes, so the type guarantees the header exists. None of this exists at run time: every pointer and array is an `i32` address in both backends, and the compiled bytes are the same as for untyped code.

| Form | Type | Semantics |
|---|---|---|
| `(new S a [x:v y:v ...])` | `(ptr S)` | an `S` from the allocator `a` (an `alloc.Allocator`), its fields given by name, each once and in declaration order (`new gives the fields [x]; S's fields, each once and in order, are [x y]`): the resolver writes `(call (alloc.create S) a)`, importing std/alloc, then a `put` of each field, the values in the order written. `(new S (call alloc.default) [...])` from the shared heap, `(call alloc.forever)` for memory kept until the program exits, an arena's allocator freed with the arena. The block is 8 + `(sizeof S)` bytes, the generation word (a fresh one), 4 unused bytes, then the struct. A field that starts empty is given its zero, `(zero T)` (below). There is no zeroed `(new S a)`: `new is (new S a [x:v y:v]): ... a field that starts empty is given its zero, (zero T)`; `(new S)` is `new takes the allocator to make it from, then each field by name: ...`. A template, which cannot name the fields of a `T` of `(where (struct T))`, makes one zeroed with the function `new` stands for, `(call (alloc.create T) a)`. A struct's fields are given by name, a variant's payload by position (`make` of a union, section 4.J) |
| `(get p S.f)` | type of `f` | `p` must be a `(ptr S)`; load at `p + offset(f)` with the instruction for `f`'s type |
| `(put p S.f v)` | `void` | `p` must be a `(ptr S)`; store at `p + offset(f)`; `v` must have `f`'s type |
| `(sizeof S)` | `i32` | compile-time constant: a struct's size, or for any type that can be stored in memory its width (`(sizeof i64)` 8, `(sizeof (ptr S))` 8), so generic code can size a `T`; `void`, a result, and an unknown struct are checker errors |
| `(arr.new T a n)` | `(arr T)` | `n` zeroed `T`s from the allocator `a`: `(call (alloc.array T) a n)`; `n < 0` fails its contract. The block is `8 + n * sizeof(T)` bytes: the generation word, `n`, then the elements; the array is the first element's address. `(arr.new T n)` is an error naming the allocator it needs |
| `(arr.of T a x y ...)` | `(arr T)` | the array literal: an `(arr T)` from the allocator `a` holding the values in order, its length their count; `(arr.of T a)` is an empty array (to start with and replace by longer ones; a list that grows is `vec.Vec`). The allocator is evaluated first, then the values, each once. Without an allocator, `arr.of takes the allocator to make it from, then the values: (arr.of T a x y ...), e.g. (arr.of i32 (call alloc.default) 1 2 3), or (arr.of i32 a) for an empty array`. Rewritten as the resolver reads the module (section 4.M) |
| `(arr.get T a i)` | `T` | `a` must be an `(arr T)`; checks `0 <= i < (arr.len a)` (below), then loads at `a + i * sizeof(T)` |
| `(arr.set T a i v)` | `void` | `a` must be an `(arr T)`; checks the index, then evaluates `v` and stores it at `a + i * sizeof(T)` |
| `(arr.len a)` | `i32` | the element count stored in the 4 bytes before `a` |
| `(ptr.null S)` / `(arr.null T)` | `(ptr S)` / `(arr T)` | address 0; `get`, `put`, and every array operation stop on it (below) |
| `(zero T)` | `T` | the zero of `T`, written out by the generics pass where every type is known (a template's `T` included, in each instance): `0` (for a storage type too, `u8`, `i8`, `u16`, `i16`, which a field reads and writes as an `i32`), `0i64`, `0.0`, `0.0f32`, `false`, `""`, `(ptr.null S)`, `(arr.null T)`, an enum's first member, a value struct made of its fields' zeros. A union has none (`U is a union, which has no zero: write the variant you mean with make`), and nor do a function reference, a result, or a struct used through a pointer (`P is a struct, used only through a pointer: (zero (ptr P)) is a null pointer`); a template instantiated with such a type is refused at its `zero`. |
| `(ptr.cast S x)` / `(arr.cast T x)` | `(ptr S)` / `(arr T)` | `x` must be `i32`; reinterprets the address, unchecked: its generation is `0xFFFFFFFF`, which no check compares (a cast of 0 is null) |
| `(ptr.make S x g)` / `(arr.make T x g)` | `(ptr S)` / `(arr T)` | `x` and `g` must be `i32`; a checked pointer to address `x` with generation `g` (unsafe; allocators use it, through `std/gen`) |
| `(ptr.addr p)` / `(arr.addr a)` | `i32` | the address, e.g. for `mem.*` or arithmetic |
| `(ptr.gen p)` / `(arr.gen a)` | `i32` | the generation the pointer carries |

Field and element types are the scalars, `(ptr S)`, `(arr T)`, unions, enums, and function references `(fn [...] -> r)` (called with `call_ref`; how `std/alloc`'s custom allocators hold their functions), so arrays of pointers (`(arr (ptr Point))`) and of arrays (`(arr (arr i32))`) work; so do value structs, held inline (below). A `bool` read by `get`/`arr.get` is `true` iff its word is nonzero (wasm emits `i32.const 0; i32.ne` after the load), so a word written raw with `mem.store32` behaves the same in both backends. A `str` field or element holds the address of the string's bytes, as a `str` value does in wasm. When storing one, the VM copies the string into the heap the way `str.ptr` does, so heap addresses after a `str` store differ between the backends while lengths, contents, and `eq` agree. `put` and `arr.set` go through the same reserved-block write guard as `mem.store*` in both backends (section 7.9).

**Struct names are namespaced like functions** (section 11): inside the module that defines it, a struct is `Node`; an importer writes `compiler.Node` (or `c.Node` after `(import compiler as c)`), in `new`, `sizeof`, `(ptr ...)`, and field references such as `(get p compiler.Node.next)`. Two imported modules may each define a `Node`.

**Bounds checks run everywhere.** Every `arr.get` and `arr.set` checks `0 <= i < n` against the header at `a - 4`, before the value to store is evaluated, and fails with `Array index out of bounds: index I for array of length N`: by an implementation that compiles to WebAssembly by writing that message and trapping (section 7.9), which the host prints as `<program>: <message>` with exit status 134.

**Null checks run everywhere.** `get`, `put`, `arr.get`, `arr.set`, and `arr.len` check their pointer or array for null (address 0) right after evaluating it, before the index or the value to store, and fail with `Null pointer dereference`, followed by the call chain with positions.

**Generation checks run everywhere.** Every object has a generation word 8 bytes before it, and every pointer and array carries the generation its object had when the pointer was made. Right after the null check, `get`, `put`, `arr.get`, `arr.set`, and `arr.len` compare the two, and fail with `Use after free` (with the call chain) unless they match or the pointer is unchecked (made by `ptr.cast`). Every object from an allocator (`new`, `arr.new`, `std/heap`, `std/alloc`, and `std/arena`) gets a fresh generation from `std/gen`, and freeing one changes its word, so every copy of every pointer to it fails from then on, whether or not the memory holds a new object. Freeing twice is caught the same way. `eq` on pointers compares the address and the generation. An implementation may omit a check that cannot fail, as in a loop in which nothing can free memory and every pointer it uses is live when it starts, so long as a program that would fail fails at the same place.

```lisp
(module points
  (import alloc)
  (struct Point [x:i32 y:i32])
  (fn main [] -> i32
    (let ps:(arr (ptr Point)) (arr.new (ptr Point) (call alloc.forever) 3))
    (for i 0 3
      (let p:(ptr Point) (new Point (call alloc.forever) [x:i y:(* i 10)]))
      (arr.set (ptr Point) ps i p))
    (let sum:i32 0)
    (for i 0 (arr.len ps)
      (let p:(ptr Point) (arr.get (ptr Point) ps i))
      (set! sum (+ sum (+ (get p Point.x) (get p Point.y)))))
    (println "{sum}")                     ;; prints 33 in both backends
    0))
```

#### Value structs

A struct declared with `(value)` after its fields is a **value struct**: a value, like an `i32`, not a cell behind a pointer. It is copied where it goes (a `let`, an argument, a result, a field or array element holding it), it cannot be changed, only replaced by a copy with one field changed, and nothing is allocated for it. Small records of numbers (a point, a vector, a body's position and velocity, a file's size and time) are what it is for.

```lisp
(module values_demo
  (import alloc)
  (struct Vec2 [x:f64 y:f64] (value))
  (struct Particle [pos:Vec2 vel:Vec2 alive:bool] (value))

  (fn add [a:Vec2 b:Vec2] -> Vec2
    (make Vec2 [x:(+ (get a Vec2.x) (get b Vec2.x)) y:(+ (get a Vec2.y) (get b Vec2.y))]))

  (fn main [] -> i32
    (let ps:(arr Particle) (arr.new Particle (call alloc.forever) 2))       ;; two particles inline: 80 bytes
    (arr.set Particle ps 0 (make Particle [pos:(make Vec2 [x:1.0 y:2.0])
                                           vel:(make Vec2 [x:0.5 y:0.5])
                                           alive:true]))
    (arr.set Particle ps 1 Particle.vel Vec2.y 3.0)     ;; one field, written in place
    (let p:Particle (arr.get Particle ps 0))
    (let moved:Particle (with p Particle.pos (call add (get p Particle.pos) (get p Particle.vel))))
    (let y:f64 (+ (get moved Particle.pos Vec2.y) (arr.get Particle ps 1 Particle.vel Vec2.y)))
    (println "y = {y}")                                 ;; prints y = 5.5
    0))
```

| Form | Type | Semantics |
|---|---|---|
| `(make V [x:v y:v ...])` | `V` | a value with every field given by name, each once and in declaration order (`make gives the fields [y x]; V's fields, each once and in order, are [x y]`); by position it is an error, `a value struct's fields are given by name, each once and in order: (make V [x:v y:v])` |
| `(get v V.f)` | type of `f` | field `f` of the value `v` |
| `(get e V.f W.g ...)` | type of the last field | a path: field `g` of the value struct held in field `f`, and so on; `e` may be a value or a `(ptr S)` whose field `S.f` holds one |
| `(with v V.f x)` | `V` | a copy of `v` with field `f` set to `x` (`with V.x: expected i32, got bool`) |
| `(put p S.f V.g ... x)` | `void` | a path from a pointer struct's field into the value struct it holds: one field written in place |
| `(arr.get V a i V.f ...)`, `(arr.set V a i V.f ... x)` | | a path from an array element: one field read, or written in place |

The rules, each checked with the message shown:

- At least one field (`value struct 'E' has no fields`), and fields that are numbers, `bool`, enums, `(ptr S)`, `(arr T)`, function references, or other value structs: not `str`, unions, results, or `void` (`Field 's' of value struct 'S': a value struct holds numbers, bools, enums, pointers, arrays, function references, and value structs, not str`).
- Not containing itself, directly or through other value structs (`value struct 'R' contains itself (through field 'r'): a value has a fixed size; hold it through a pointer`), and at most 16 scalars once the value structs in it are counted (`value struct 'Big' holds 17 scalars, counting the value structs in it; at most 16: keep a larger record behind a pointer`).
- No address: `(ptr V)`, `(new V a [...])`, `ptr.null`, and `ptr.cast` are errors (`'V' is a value struct, which has no address: write V, not (ptr V)`), and a field of one cannot be `put` (`a value struct's fields do not change: (with v V.x x) is a copy with one changed`). A field path writes into a value only where it sits in changeable memory: a pointer struct's field or an array element.
- No `eq`: values do not compare (`eq on value struct 'V': value structs do not compare; implement cmp.Eq for it (section 4.H) and call (cmp.Eq.same V)`).

In memory a value struct is laid out inline like any struct (above): its fields in order, each aligned to its own alignment, the whole rounded up to the largest, so `(sizeof Vec2)` is 16 and `(sizeof Particle)` 40 (`alive` at 32, then padding to 8). As a field, element, union field, or result payload it takes that size and alignment. `(make V ...)` and `(with ...)` allocate nothing.

A value struct is a type argument of the value kind (section 4.H): `(vec.Vec Vec2)` keeps its elements inline, and `(where (struct T))` still means a struct used through a pointer (`must be a struct used through a pointer ... is a value struct, which has no address`). `(pub)` comes after `(value)`: `(struct V [fields] (value) (pub))`. Outside its module, `make` needs every field `(pub)`, `with` counts as writing the field it changes, and each step of a path is checked like a `get` or `put` (section 11.1); using a value struct as a type argument is allowed whatever its fields, since a template cannot make one.

Every implementation runs value structs with the same results.

### F. Results

`(ok:E v)` and `(err:T e)` build a value of type `(result T E)`, each naming the side it does not carry, so every result is made with its whole type written (`(ok v) must name its result's err type: write (ok:E v) for a (result T E)`); `match_result` consumes one. A payload may be of any type but `void` (`a result payload must be a value, got void`). A result can be held anywhere a value can: a local, a parameter, a result, another result, a struct field, an array element, a type argument.

**Results by value.** A result is always held by value, laid out like a union held by value (section 4.J): its tag (0 = ok, 1 = err), then the slots its two sides share by kind, the ok side's scalars and the err side's each taking the first slots of their kinds, unused ones zero. A `str` side is one i32-sized slot. `(result i32 i32)` is 2 slots, `(result i32 str)` 2, `(result Vec2 i64)` 4 (the tag, the `i64`, two `f64`s); building one allocates nothing, in every backend.
A result is a value, so it can be a struct field or an array element like a union (`(struct Slot [r:(result i32 str) n:i32])`, `(arr (result i32 str))`), and a type argument; its size in memory is its slots. A result that would need more than 16 slots is refused where its type is written or where one is made: `(result Q I) needs 17 slots, more than the 16 a value holds: keep the larger side's data in a struct that is not (value) and make that side a (ptr S)`.
For a value that may be missing rather than failed, use `(option.Option T)` from `std/option` (section 4.J).

**A result or an option is not dropped.** An expression whose value is thrown away (a statement of a function's body that is not its last, an item of a block, a `when`, a loop, a `defer`, or an `unsafe` that is not last, the last of a function that returns `void`) must not give a `(result T E)` or an `(option.Option T)`: a failure or a missing value would go unnoticed. It is a compile error naming the ways out: use the value (`match_result`, `match`, `try`, `try_opt`), or write `(drop e)`, which evaluates `e` of any type but void and drops its value on purpose: `(drop (call fs.remove path))` for a file that may not be there. Both toolchains give the same message at the same place. Values of other types are still dropped silently in statement position (`atomic.add` returns the old value).

**Early returns: `try`, `try_opt`, `try_or`.**

```lisp
(module parse_two
  (import str)
  (fn digit [c:i32] -> (result i32 str)
    (if (and (gte c 48) (lte c 57)) (ok:str (- c 48)) (err:i32 "not a digit")))
  (fn two [s:str.Bytes] -> (result i32 str)
    (let a:i32 (try (call digit (call str.byte_at s 0))))     ;; or return its error
    (let b:i32 (try (call digit (call str.byte_at s 1))))
    (ok:str (+ (* a 10) b)))
  (fn main [] -> i32
    (match_result (call two (call str.from_str "42")) (ok v (println "{v}")) (err e (println "not a number")))   ;; prints 42
    0))
```

| Form | Gives | Or returns from the function |
|---|---|---|
| `(try r)`, `r:(result T E)` | the ok value, a `T` | `(err:R e)` with r's error: the function returns `(result R E)`, the same `E` |
| `(try r (ref convert))`, `r:(result T E)`, `convert` a function from `E` to `F` | the ok value, a `T` | `(err:R (call convert e))`: the function returns `(result R F)`, so errors of different types meet in one call (`(try (call io.read_file p) (ref io.describe))` into a `(result T str)`) |
| `(try_opt o)`, `o:(option.Option T)` | the value, a `T` | `none`: the function returns an `(option.Option U)` |
| `(try_or o e)`, `o:(option.Option T)` | the value, a `T` | `(err:R e)`: the function returns `(result R E)`, `e` an `E` |

Each is a `match_result` or `match` with a `return` in the failing arm, written out once the program checks (`(try r)` is `(match_result r (ok _try_1 _try_1) (err _try_err_1 (return (err:R _try_err_1))))`), so it costs what that code costs. Not in a contract (`try returns from the function, and a contract cannot`); the mismatches name both types (`try's error is str, and the function's is i32: they must be one type`, `try_opt returns none from the function, which returns (result i32 i32), not an (option.Option T)`).

**Cleanup: `defer`.**

```lisp
(module cleanup
  (import buf)
  (import alloc)
  (fn digit [c:i32] -> (result i32 str)
    (if (lt c 10) (ok:str c) (err:i32 "big")))
  (fn report [n:i32] -> (result i32 str)
    (let b:(ptr buf.Buf) (call buf.make (call alloc.default) 16))
    (defer (call buf.free b))                 ;; however report returns
    (let v:i32 (try (call digit n)))          ;; this early return frees b too
    (format_to b "v={v}")
    (ok:str (call buf.len b)))
  (fn main [] -> i32
    (match_result (call report 7) (ok n (println "{n}")) (err e (println "failed")))   ;; prints 3
    0))
```

`(defer e ...)` is a statement of a function's body. When the function returns after it, by falling off its end, by `return`, or by a `try`, its statements run, the last defer's first, after the return value is computed. A return before the defer in the text does not run it. A defer elsewhere is an error (`defer is a statement of a function's body (it runs when the function returns), not part of an expression or a block`), and so is a defer that would itself return (`a defer cannot return: it runs as the function returns`; likewise `try`). A trap does not run defers: it stops the program. A defer is written out where it runs: `(return v)` after one becomes `(block (let _defer_ret_1:R v) D... (return _defer_ret_1))`, and the body's value is kept in `_defer_val` while the defers run.

### G. Function References

```lisp
(fn add [a:i32 b:i32] -> i32 (+ a b))
(fn twice [f:(fn [i32 i32] -> i32) x:i32] -> i32
  (call_ref (fn [i32 i32] -> i32) f x x))
(fn main [] -> i32 (println "{(call twice (ref add) 20)}") 0)      ;; prints 40
```

| Form | Type | Semantics |
|---|---|---|
| `(ref f)` | `(fn [params of f] -> return of f)`, with `(no_leak)` when `f` is marked `(no_leak)` or proven to keep no memory | a reference to function `f` |
| `(call_ref (fn [t...] -> r) g args...)` | `r` | calls the function `g` refers to; the written signature must be `g`'s type (or `g`'s without `(no_leak)`), and the arguments must match it |

Function types are strict like pointer types: a reference is never an `i32`, a reference with one signature is never accepted for another, there is no arithmetic on references, and they compare only with `eq`/`neq`. There is no cast to a function type, so every reference names a real function. `call_ref` repeats the signature, as `arr.get` repeats the element type, so the call site states what it calls. References can be parameters, results, `let`s, struct fields, and array elements.

**`(no_leak)` in a function type.** `(fn [i32] -> i32 (no_leak))` is the type of a reference to a function that keeps no memory (section 3, "Memory that is never freed"). A call through it is not a leak site, so a `(no_leak)` function may call what it is handed, where through an ordinary `(fn [i32] -> i32)` it may not (its target is not known). Only a function that keeps no memory goes in: one marked `(no_leak)`, or one the checker proves allocates nothing that is never freed, directly or through what it calls, marked or not. Anything else is refused, with the chain that shows why:

```
Argument 1 of 'apply' expects (fn [i32] -> i32 (no_leak)), got (fn [i32] -> i32): keeps may keep memory (keeps calls str.concat; str.concat calls str.room; str.room uses mem.alloc)
```

A promising reference fits an ordinary slot (it promises more than the slot asks), never the other way: a value already typed `(fn [i32] -> i32)` does not go into a promising slot, since what it refers to is no longer known. Where two values meet, the branches of an `if`, `cond`, `match`, or `match_result`, or the two sides of `eq` and `neq`, two function types that differ only in the promise meet at the type without it: `(if c (ref double) (ref keeps))` is a `(fn [i32] -> i32)`. The rule is for the whole value only: `(arr (fn [i32] -> i32 (no_leak)))` and `(arr (fn [i32] -> i32))` are different types, as `(ptr S)` and `(ptr T)` are. A generic instance names the promise: `(vec.Vec (fn [i32] -> i32 (no_leak)))` is `Vec<fn<i32->i32,no_leak>>`.

```lisp
(fn apply [f:(fn [i32] -> i32 (no_leak)) x:i32] -> i32 (no_leak)
  (call_ref (fn [i32] -> i32 (no_leak)) f x))
(fn double [x:i32] -> i32 (* 2 x))                   ;; proven: no mark needed
(fn main [] -> i32 (println "{(call apply (ref double) 21)}") 0)     ;; prints 42
```

**No closures; a context argument instead.** A function sees only its parameters: there are no anonymous functions, nothing captures the variables of the code around it, and there are no global variables. Data a callback needs is passed to it explicitly, as a typed *context*, its first parameter, which the code calling it passes through. This is a design decision, not a gap: a closure would allocate its captured variables behind the program's back (AIPL allocates only through allocators the program names), make every function value larger and every call through one slower, and add rules for what a capture copies or shares. The standard library follows one convention: a function taking a callback with a context has `_with` in its name and takes the context right before the callback, and the callback takes the context first.

```lisp
(module context_demo
  (import vec)
  (import alloc)

  ;; positions, highest score first: the comparison needs the scores
  (fn by_score [scores:(arr i32) a:i32 b:i32] -> i32
    (cond ((gt (arr.get i32 scores a) (arr.get i32 scores b)) -1)
          ((lt (arr.get i32 scores a) (arr.get i32 scores b)) 1)
          (else 0)))

  (fn main [] -> i32
    (let scores:(arr i32) (arr.new i32 (call alloc.forever) 3))
    (arr.set i32 scores 0 40) (arr.set i32 scores 1 90) (arr.set i32 scores 2 60)
    (let order:(ptr (vec.Vec i32)) (call (vec.make i32) (call alloc.default) 3))
    (for i 0 3 (call (vec.push i32) order i))
    (call (vec.sort_by_with i32 (arr i32)) order scores (ref by_score))
    (println "{(call (vec.at i32) order 0)} {(call (vec.at i32) order 1)} {(call (vec.at i32) order 2)}")   ;; prints 1 2 0
    0))
```

In the library: `vec.sort_by_with` (a comparison with a context; `vec.sort_by` for one without), `thread.spawn_with` (a worker's context, section 4.D), and `alloc.custom` (a custom allocator's state, section 10).

At run time a reference is the function's position among the program's functions (after import resolution). The wasm backend emits a funcref table holding every function (min = max = function count) and an active element segment filling it from offset 0, and lowers `call_ref` to `call_indirect` (arguments first, then the reference) with a type index from one extra type per distinct signature, appended after the function types in source order. Table, element section, and extra types are emitted only when the module uses `ref` or `call_ref`, so other programs compile to the same bytes as before. The resolver qualifies `(ref f)` in an imported module like a call, so references work across imports.

### H. Generics

A generic struct or function takes type parameters in its header, and every use names its type arguments explicitly; there is no type inference.

```lisp
(struct (Box T) [value:T next:(ptr (Box T))])

(fn (make T) [v:T] -> (ptr (Box T))
  (new (Box T) (call alloc.forever) [value:v next:(zero (ptr (Box T)))]))

(fn main [] -> i32
  (let b:(ptr (Box i64)) (call (make i64) 5i64))
  (println "{(get b (Box i64) value)}")
  0)
```

| Form | Meaning |
|---|---|
| `(struct (Name T...) [fields])` | a generic struct; parameters are names starting with an uppercase letter |
| `(fn (name T...) [params] -> ret body...)` | a generic function |
| `(Name t...)` | the struct instantiated with types `t...`, wherever a struct name goes: `(ptr (Name i32))`, `(new (Name i32))`, `(sizeof (Name i32))`, `(ptr.null (Name i32))`, `(ptr.cast (Name i32) x)` |
| `(get p (Name t...) field)`, `(put p (Name t...) field v)` | field access on a generic struct (a non-generic struct keeps `(get p S.field)`) |
| `(call (name t...) args...)`, `(ref (name t...))` | calling, or referencing, the function instantiated with `t...` |
| `(union (Name T...) [variants])` | a generic union |
| `(make (Name t...) variant v...)`, the arm `((Name t...) variant [binders] body...)` | building, and matching, a generic union's variant (section 4.J) |

Generics are expanded before type checking: each distinct instantiation becomes an ordinary struct, union, or function named `Name<t,...>` (for example `Box<i64>`, `vec.Vec<ptr<Point>>`, `fn<i32,i32->i32>` for a function type argument), made by substituting the arguments for the parameters. The checker, the VM, both code generators, and any future backend only ever see concrete code, and instance names appear as such in diagnostics and wasm exports. Rules:

- A template is checked once, by itself, with its type parameters sealed (below), whether or not anything uses it: an error inside a generic body is reported at the template's own line, and holds for every type argument its clauses allow. An error inside a type argument is reported where the argument is written.
- Type arguments may be any type, including other instances and function types. Arity is checked: `generic 'Box' takes 1 type argument(s) (T), got 2`.
- A generic may not be named like a built-in form or type (`get`, `put`, `new`, `i32`, ...), since `(get p (Name T) f)` would be ambiguous.
- A generic function is applied only where a function is named: `(call (f t...) ...)` and `(ref (f t...))`. Elsewhere `(f ...)` is not an application, so a generic named `make` (as `std/vec`'s is) does not swallow a union's `(make U.v x)`. A generic struct is applied wherever a type goes.
- A generic whose instances keep growing (`(fn (f T) ... (call (f (ptr T)) ...))`) is an error once an instance name passes 1024 characters.
- Imports qualify generics like everything else: `vec.Vec` from another module, `(vec.Vec i32)`, `(call (vec.push i32) v 5)`; aliases work (`(import vec as v)`, `(v.Vec i32)`).

#### Interfaces, implementations, and where clauses

An interface names functions a type can have; an implementation provides them for one type; a template asks for them with a `where` clause. `std/cmp` defines `(cmp.Eq T)` (`same`), `(cmp.Ord T)` (`compare`, which returns -1, 0, or 1), and `(cmp.Hash T)` (`hash` and `same`), implemented for the built-in types, and `std/map` is keyed by any type with `cmp.Hash`.

```lisp
(module interface_demo
  (import cmp)
  (import map)
  (import alloc)
  (struct Point [x:i32 y:i32])

  ;; points equal field by field hash alike
  (impl (cmp.Hash (ptr Point))
    (fn hash [k:(ptr Point)] -> i32 (+ (* 31 (get k Point.x)) (get k Point.y)))
    (fn same [a:(ptr Point) b:(ptr Point)] -> bool
      (and (eq (get a Point.x) (get b Point.x)) (eq (get a Point.y) (get b Point.y)))))

  (fn point [x:i32 y:i32] -> (ptr Point)
    (let p:(ptr Point) (new Point (call alloc.forever) [x:x y:y]))
    p)

  ;; the largest element, for any T with an order
  (fn (largest T) [xs:(arr T)] -> T (where (cmp.Ord T))
    (req (gt (arr.len xs) 0))
    (let best:T (arr.get T xs 0))
    (for i 1 (arr.len xs)
      (when (gt (call (cmp.Ord.compare T) (arr.get T xs i) best) 0) (set! best (arr.get T xs i))))
    best)

  (fn main [] -> i32
    (let seen:(ptr (map.Map (ptr Point) i32)) (call (map.make (ptr Point) i32) (call alloc.default) 0))
    (call (map.set (ptr Point) i32) seen (call point 1 2) 10)
    (call (map.set (ptr Point) i32) seen (call point 1 2) 20)     ;; the same point: replaces 10
    (let xs:(arr i64) (arr.new i64 (call alloc.forever) 3))
    (arr.set i64 xs 1 7i64)
    (let v:i32 (call (map.get_or (ptr Point) i32) seen (call point 1 2) 0))
    (println "{v}, {(call (map.count (ptr Point) i32) seen)} key, largest {(call (largest i64) xs)}")   ;; prints 20, 1 key, largest 7
    0))
```

| Form | Meaning |
|---|---|
| `(interface (Name T) (fn f [params] -> type contracts...)...)` | an interface: one type parameter, at least one function, each a signature with optional `req`/`ens` and no body |
| `(impl (Name t) (fn f [params] -> type body...)...)` | the interface's functions for type `t`: exactly its functions, each with the interface's parameter names and types (with `t` for the parameter) |
| `(call (Name.f t) args...)`, `(ref (Name.f t))` | calling, or referring to, `t`'s implementation of `f`; from another module `(cmp.Hash.hash i32)`, through an alias `(c.Hash.hash i32)` |
| `(where (Name T))` | in a generic function, struct, or union, right after its signature (a function's return type, a struct's fields, a union's variants): every use needs an implementation of `Name` for its `T`, and the template may call `Name`'s functions for `T` |
| `(where (struct T))` | `T` is a struct, used as `(ptr T)`, `(call (alloc.create T) a)`; without it a type parameter is a value |

Rules (both expanders, with the same messages):

- **One implementation per type.** A type has at most one implementation of an interface in the whole program (`a second implementation of cmp.Hash<i32>: a type has at most one implementation of each interface`), so two maps keyed by the same type cannot hash it two ways.
- **Where it may be written.** In the interface's module or in the module that defines the type (for `(ptr geo.Point)`, `geo`); for a built-in type only in the interface's module (`this implementation must be in the module that defines cmp.Eq, since f64 is built in`). Implementations are for concrete types; there are no generic implementations, defaults, or automatically derived ones.
- **Contracts are inherited.** Each implementation's function runs the interface's `req`/`ens` before its own, and a failure is reported where the interface wrote it.
- **Nothing is inferred.** A call names the interface and the type. An implementation's functions are ordinary functions named `Interface<type>.f` (`cmp.Hash<i32>.hash`): calling one costs a direct call.
- **No floats in std/cmp.** NaN is not equal to itself, so equality and hashing on `f32`/`f64` would break the law every map relies on; a program that needs float keys wraps them and decides what NaN means.

#### Kinds, and checking a template once

Inside a template, a type parameter is sealed: it supports only what every type of its kind supports, plus the functions its `where` clauses grant. Before expanding, both toolchains make each template once at placeholders for its type parameters and type-check that, so the template is proven for every type argument its clauses allow; nothing is compiled from these instances.

| Kind | Arguments | A sealed parameter supports |
|---|---|---|
| value (default) | anything that can be stored: numbers, `bool`, `str`, pointers, arrays, function references, enums, unions | `let`, parameters, results, struct and variant fields, `(arr T)`, `(sizeof T)`, `arr.cast` |
| struct (`(where (struct T))`) | a struct, generic ones included | `(ptr T)`, `(call (alloc.create T) a)`, `(sizeof T)`, `(ptr.null T)`, `ptr.cast` |
| plain (`(where (plain T))`) | a value holding no pointer or array at any depth, so threads may share copies of it: numbers, `bool`, `str`, enums, function references, and value structs and unions whose fields are plain, generic ones included (`(Pair i32 f64)` is plain, `(Pair (ptr Box) i32)` is not) | what a value parameter supports; `std/par` asks it of its types |

Anything else on a parameter is an error that names the clause to add: `eq on T: a type parameter has no equality of its own; add (where (cmp.Eq T)) and call (cmp.Eq.same T)`, likewise for order (`cmp.Ord`), arithmetic, `match`, `enum.ord`, and using a value parameter as a struct (`T is a type parameter that is not a struct: to use it as one, add (where (struct T))`). Calling a template that asks for more than the caller's clauses give is caught the same way: `vec.index_of<T> needs (cmp.Eq T): add (where (cmp.Eq T)) to g`.

What can still fail where a template is used is the use itself: a missing implementation (`has<ptr<P>> needs (cmp.Eq (ptr P)): write (impl (cmp.Eq (ptr P)) ...)`), a type of the wrong kind (`vec.Vec<Point>: T is a value type, and Point is a struct, used only through a pointer: write (ptr Point)`; `option.Option<void>: T is a value type, and void cannot be stored`; `alloc.create<i32>: T must be a struct (where (struct T)), and i32 is not one`), and an instance that grows without bound.

### I. Constants and Enums

```lisp
(module shapes
  (import alloc)
  (const MAX_SIDES:i32 12)
  (const UNIT:f64 0.5)
  (const LABEL:str "shape")
  (enum Kind [triangle square (hexagon 6) octagon])   ;; 0, 1, 6, 7

  (fn sides [k:Kind] -> i32
    (cond
      ((eq k Kind.triangle) 3)
      ((eq k Kind.square) 4)
      (else (enum.ord k))))                            ;; hexagon 6, octagon 7

  (fn main [] -> i32
    (let ks:(arr Kind) (arr.new Kind (call alloc.forever) 2))
    (arr.set Kind ks 0 Kind.square)
    (arr.set Kind ks 1 (enum.cast Kind 7))
    (println "{(call sides (arr.get Kind ks 0))} {(call sides (arr.get Kind ks 1))} {MAX_SIDES} {(str.len LABEL)}")   ;; prints 4 7 12 5
    0))
```

| Form | Meaning |
|---|---|
| `(const NAME:T value)` | a named value. `T` is `i32`, `i64`, `f64`, `bool`, or `str`. The value is a literal of that type (`65536`, `0xEDB88320`, `-1i64`, `0.5`, `true`, `"text"`; an `i32` one may be any 32-bit pattern, `-2147483648` to `4294967295`) or, for every type but `str`, a constant expression: literals and other constants, in any order of definition, combined with `+ - * / % ^ shl shr shru rotl rotr bitand bitor` and (one operand) `bitnot clz ctz popcnt` on `i32` and `i64` (wrapping, as at run time), `+ - * /` on `f64`, and `and or not` on `bool`, both operands of one type: `(const PAGES:i32 (* 4 PAGE_SIZE))`. Folding fails with the reason: `/ by zero`, `/ overflows`, `'Y' is not a constant`, `its value depends on itself`, `+ needs two operands of one type, got i32 and i64`, `constant 'X' is declared i64 but its value is i32`. `NAME` is in capitals: at least two characters from `A-Z`, `0-9`, `_`, starting with a letter |
| `NAME` | anywhere an expression goes: its value, as a literal. It cannot also be the name of a variable, parameter, or field |
| `(enum Name [a b (c 10) ...])` | a new type `Name` with the named members. A member without a value is one more than the member before it, the first `0`. Members start with a lowercase letter or `_`; names and values must be unique; at least one member |
| `Name.member` | a value of type `Name` |
| `Name` as a type | `[k:Name]`, `-> Name`, struct fields, `(arr Name)`, `(result Name E)`, `(fn [Name] -> r)`, generic type arguments `(vec.Vec Name)` |
| `(enum.ord e)` | `i32`: the member's value |
| `(enum.cast Name n)` | `Name`: the `i32` `n` as a member, unchecked (like `ptr.cast`); for values read back from memory |

Enums are checked like pointers: a `Name` is never an `i32` or another enum, has no arithmetic, and compares only with `eq`/`neq`. Write `(eq k Kind.square)`, and convert explicitly with `enum.ord`/`enum.cast` where a number is meant. A bare type name that is not a scalar or an enum is `Unknown type 'Colr'`; a struct name written as a type (`p:Point`) is reported with the fix (`(ptr Point)`).

At run time a constant is its value as a literal (folded at compile time, identically in both toolchains) and an enum is an `i32` (4 bytes in structs and arrays), so neither changes the compiled code. Both are expanded after import resolution and generics: a constant becomes its literal, and `Name.member` becomes the member's number, of the enum's type.

Imports qualify them like structs: `palette.LIMIT`, `palette.Color`, `palette.Color.red`, and through an alias `pal.Color.red`. Two modules may each define `Color`.

### J. Unions and `match`

A union is a type whose value is exactly one of several variants, each carrying its own fields: a value of `Shape` below is a circle with a radius, or a rectangle with a width and a height, or a dot with nothing. `match` takes a value apart, running the arm for its variant with the variant's fields bound to names. It works on enums too.

```lisp
(module geom
  (import alloc)
  (union Shape [(circle r:i32) (rect w:i32 h:i32) (dot)])
  ;; a list of shapes: the rest of it behind a pointer, from an allocator
  (union List [(nil) (cons head:Shape tail:(ptr Rest))])
  (struct Rest [l:List])
  (enum Dir [north east south west])

  (fn area [s:Shape] -> i32
    (match s
      (Shape.circle [r] (* 3 (* r r)))
      (Shape.rect [w h] (* w h))
      (Shape.dot 0)))

  (fn cons [a:alloc.Allocator s:Shape rest:List] -> List
    (let r:(ptr Rest) (new Rest a [l:rest]))
    (make List.cons s r))

  (fn total [l:List] -> i32
    (match l
      (List.nil 0)
      (List.cons [s rest] (+ (call area s) (call total (get rest Rest.l))))))

  (fn turns [d:Dir] -> i32
    (match d (Dir.north 0) (Dir.south 2) (else 1)))

  (fn main [] -> i32
    (let a:alloc.Allocator (call alloc.default))
    (let shapes:List (call cons a (make Shape.rect 2 5)
                      (call cons a (make Shape.circle 1)
                      (call cons a (make Shape.dot) (make List.nil)))))
    (println "total {(call total shapes)}, turns {(call turns Dir.south)}")     ;; prints total 13, turns 2
    0))
```

| Form | Meaning |
|---|---|
| `(union Name [(variant field:T ...) ...])` | a new type `Name`. A variant without fields is `(variant)`. Variant names start with a lowercase letter or `_` and are unique; field names are unique within a variant; field types are those a struct field may have, including any union (so a union may refer to itself); at least one variant |
| `(make Name.variant v ...)` | a new value of that variant: one value per field, in declaration order, each of the field's type. Type `Name` |
| `(match v arm ... [(else body...)])` | `v` is a union or an enum. A union arm is `(Name.variant [x y ...] body...)`, binding the variant's fields in order (every field, each to a new name or to `_`, which skips it and may repeat; a variant without fields may omit the brackets); an enum arm is `(Name.member body...)` |
| `Name` as a type | everywhere a type goes: parameters, results, `let`, struct fields, variant fields, `(arr Name)`, generic type arguments |

Rules the checker enforces:

- **Exhaustive.** Every variant (member) has an arm, or there is an `(else ...)` arm, which comes last and binds nothing. An `else` arm when every variant already has one is an error (`the else arm never runs`), and so is matching one variant twice.
- **One type.** Every arm (and `else`) yields the same type, or all are `void`, as with `if`. That type is the match's.
- **Binders are new names**, scoped to their arm: they may not shadow a name in scope.
- **Union values are opaque.** They have no arithmetic and do not compare (`eq`/`neq` included): take them apart with `match`. There is no null union value, and no cast to one; `make` is the only way to build one.

**Unions are values.** Every union is held by value, like a value struct (section 4.E): `make` allocates nothing, and a union is copied where it goes, inline in struct fields and arrays. So two kinds of union are refused, each with the way to write it:

- one that contains itself, directly or through other unions it holds: its size would have no bound. `union 'List' contains itself (List.cons's field 'tail' has type List): a union is a value, so hold that field through a pointer, a (ptr S) to a struct that holds it`, as `List` above does with `(ptr Rest)`;
- one that needs more than 16 slots. A union is its variant's index (the tag, an `i32`), then slots the variants share by kind: the i32-sized slots (every 4-byte scalar but `f32`: `i32`, `bool`, `str`, enums, function references), then the `f32`, `i64` (with pointers and arrays, each an address and a generation), and `f64` slots, as many of each as the variant needing most has.
Each variant's scalars take the first slots of their kind in order: a value struct field its fields' scalars, a union field its own slots. So `Shape` above is 3 slots (the tag and two i32 slots, which `circle` and `rect` share), `(option.Option i64)` 2, and `std/alloc`'s `Allocator`, five variants of one pointer each, 2; a `str.Bytes` is 3. A variant's fields count once however many variants there are, but slots of different kinds never share: one variant of six `i32`s and another of six `f64`s are 14.
`union 'W' needs 18 slots, more than the 16 a value holds: move the fields of its widest variant, W.many (16 slots), into a struct and hold a (ptr S)`.

The pointer is an ordinary checked `(ptr S)` from the allocator the program names, so a stale one traps like any other.

In memory a union by value is laid out like a struct of its slots in that order, each aligned to its size, the whole rounded up to its largest; a slot its variant does not use holds zero, so the memory is the same in every backend. `(sizeof Shape)` is 12. Locals, parameters, and results hold one as its slots (several wasm values, as for a value struct).

`match` reads the index and compares it with each arm's in order; a match on an enum compares the value. Only a value built with `(enum.cast Name n)` from a number that is no member can reach the end of a `match` without an `else`: the VM reports `unreachable` and compiled code traps.

`Parser::parse` alone knows the unions of the module it parses; across modules the resolver passes them along. Imports qualify unions like structs and enums: `geom.Shape`, `(make geom.Shape.dot)`, and in an arm `(geom.Shape.circle [r] ...)`, or through an alias `(g.Shape.circle [r] ...)`.

#### Generic unions and `Option`

A union may take type parameters, like a struct (section 4.H). Every use writes the type with its arguments, then the variant: `(make (Name t...) variant v...)` builds one, and the arm `((Name t...) variant [binders] body...)` matches one. `std/option` defines `(union (Option T) [(some value:T) (none)])` with `is_some`, `is_none`, and `unwrap_or`; use it for a value that may be missing, instead of `-1` or a null pointer, so the checker makes every `match` handle `none`.

```lisp
(module option_demo
  (import option)
  (import alloc)
  ;; a tree: a node's fields behind a pointer (a union cannot contain itself)
  (union (Tree T) [(leaf) (node n:(ptr (Node T)))])
  (struct (Node T) [left:(Tree T) value:T right:(Tree T)])

  (fn (node T) [a:alloc.Allocator l:(Tree T) v:T r:(Tree T)] -> (Tree T)
    (let n:(ptr (Node T)) (new (Node T) a [left:l value:v right:r]))
    (make (Tree T) node n))

  (fn (count T) [t:(Tree T)] -> i32
    (match t
      ((Tree T) leaf 0)
      ((Tree T) node [n] (+ 1 (+ (call (count T) (get n (Node T) left)) (call (count T) (get n (Node T) right)))))))

  ;; the first index of `want`, or none
  (fn find [xs:(arr i64) want:i64] -> (option.Option i32)
    (for i 0 (arr.len xs)
      (when (eq (arr.get i64 xs i) want) (return (make (option.Option i32) some i))))
    (make (option.Option i32) none))

  (fn main [] -> i32
    (let a:alloc.Allocator (call alloc.default))
    (let empty:(Tree f64) (make (Tree f64) leaf))
    (let t:(Tree f64) (call (node f64) a empty 1.5 (call (node f64) a empty 2.5 empty)))
    (let xs:(arr i64) (arr.new i64 (call alloc.forever) 4))
    (arr.set i64 xs 3 9i64)
    (let at:i32 (match (call find xs 9i64)
                  ((option.Option i32) some [i] i)
                  ((option.Option i32) none -1)))
    (println "{(call (count f64) t)} nodes, 9 at {at}")     ;; prints 2 nodes, 9 at 3
    0))
```

Each instance is an ordinary union named like a struct instance (`Tree<f64>`, `option.Option<i32>`), with the same layout, rules, and messages as any union: every instance is a value, and one that would contain itself or need more than 8 slots is refused (`(option.Option i64)` is 2 slots, `(option.Option str)` 2). A form written wrongly is an error: `a generic union variant is written (make (Name T...) variant fields...)`, `a generic union arm is written ((Name T...) variant [binders] body...)`.

---

### K. Formatting

```lisp
(module formatting
  (import buf)
  (import alloc)
  (struct Point [x:f64 y:f64])
  (fn score [i:i32] -> i32 (* i i))
  (fn main [] -> i32
    (let name:str "Ada")
    (let n:i32 3)
    (let mean:f64 2.71828)
    (println "{name} ran {n} tests, mean {mean:.2}")         ;; Ada ran 3 tests, mean 2.72
    (let p:(ptr Point) (new Point (call alloc.forever) [x:1.5 y:(zero f64)]))
    (let s:str (format "{n:>5}|{(get p Point.x)}|{{}}"))      ;; "    3|1.5|{}"
    (let b:(ptr buf.Buf) (call buf.make (call alloc.default) 16))
    (for i 1 3 (format_to b "row {i}: {(call score i):03};"))
    (println "{s} {(call buf.bytes b)}")                      ;; "    3|1.5|{} row 1: 001;row 2: 004;"
    0))
```

| Form | Gives |
|---|---|
| `(format "...")` | a new `str` (never freed, as every run-time `str`, section 4.B: for finished text, not a loop) |
| `(format_to b "...")` | `void`; appends to `b:(ptr buf.Buf)`, or writes to `b:(ptr stream.Writer)` (section 10), which must then be a variable (it is named twice once lowered: `format_to takes a (ptr stream.Writer) as a variable: bind it with let first`) |
| `(print "...")`, `(println "...")`, `(eprintln "...")` | `void`; writes to standard output (with a newline), or to standard error with a newline, through a buffer it frees |
| `(dbg e)` | `e`'s value, after writing `FILE:LINE:COL: e = VALUE` and a newline to standard error: the file's name, where `(dbg` is, and `e` as written (comments dropped, each run of whitespace one space). A value shows as the forms print it (`P{x: 0}`, `[1, 2]`, `some{value: 3}`); a function reference as `<value> : T`. Lowered with the forms, to `(block (let _dbg_N:T e) (call show.eprintln ...) _dbg_N)`, so `e` is evaluated once. Errors: `dbg takes one expression, e.g. (dbg (call f x))`, `dbg shows a value, and this expression has none (it is void)`, and, since a placeholder's code is not where its text is, `a placeholder cannot hold a dbg; bind the value with let and dbg it there` |

The string is a literal. In it, `{name}` is a variable, parameter, or constant (or a literal, `{0.5}`), `{(expression)}` any expression (every form, `new`, `arr.of`, `zero`, and `for` included), and `{{` and `}}` are braces. A placeholder's code may hold a string, written with an escaped quote at each end as it is inside any string (`{(if ok \"yes\" \"no\")}`, `{(call str.concat a \", \")}`; the empty string `\"\"` too), holding only plain characters: no backslash, brace, or line break (the brackets and semicolons in it are text, and `(dbg` in it is no dbg). Nothing else in a placeholder takes an escape or a line break, and the format after a colon takes no string; bind such a value with `let` first. A quote written without its backslash ends the whole string, and the error says so (`{(f "x")}`). After a colon comes a format, `[[fill]align][+][0][width][.decimals][x|X|b]`:

| Part | Meaning |
|---|---|
| `fill` + `align` | `<` left, `>` right, `^` centre (an extra space on the right), with `fill` (a printable ASCII character, a space by default) around the value; numbers are right-aligned by default, everything else left |
| `+` | a sign on numbers that are not negative |
| `0` | pad a number with zeros after its sign (`{n:05}` gives `-0005`) |
| `width` | at least this many characters (UTF-8 characters, as Python counts), at most 4095 |
| `.decimals` | an `f32` or `f64` with exactly this many decimals, rounded exactly, ties to even (`{x:.2}` is Python's `{x:.2f}`), at most 40 |
| `x`, `X`, `b` | an `i32` or `i64` in hexadecimal, upper-case hexadecimal, or binary (a negative one with a `-` and its magnitude) |

A value prints as: an integer in decimal; a float as Python's `repr` writes it, the fewest digits that read back as the same value (`0.1`, `100.0`, `1e+16`, `1.5e-07`, `inf`, `nan`; an `f32`'s digits are the fewest that read back as an `f32` literal does, so `0.1f32` prints `0.1`); a `bool` as `true` or `false`; a `str` and a `str.Bytes` as their bytes; an enum as its member's name.
Every other value but `void` and a function reference prints through a printer the lowering generates for its type: a struct behind a pointer as its fields, `Row{n: 2, name: "a", q: (ptr Q) @ 6400, at: V{a: 1, b: 2.5}}` (`null` for a null pointer), a pointer or array inside it as its type and address, so a cycle cannot loop; a value struct the same, `V{a: 1, b: 2.5}`; an array as `[1, 2, 3]`, its first 16 elements then `, ... N more`; a union as `variant{field: value}` or `variant`, `some{value: 5}`; a result as `ok(4)` or `err("bad")`; a generic instance's type as source writes it, `(Box i32){item: 9}`.
Inside a value, text is quoted and a function reference is `<fn>`. Every field shows, a private one too: the output is for reading, and gives no way to change the field. A printer is `(fn show.debug.T [b:(ptr buf.Buf) v:T] -> (ptr buf.Buf) ...)`, its name the type with its brackets and spaces as `<`, `>`, and `,` (`show.debug.<ptr,Row>`), made for each type printed and each value type inside one, added after the program's functions in name order, and with no line-table positions.
Such a value takes no format: `formats are for numbers, bool, str, str.Bytes, and enums, not (ptr P)`. A function reference or `void` is an error: `format cannot print (fn [] -> i32): it prints every value but void and function references`. So is a format that does not fit (`decimals (.N) are for f32 and f64, not str`, `x, X, and b are for i32 and i64, not f64`, `+ and 0 are for numbers, not str`), a bad one (`bad format 'q': a format is [[fill]align][+][0][width][.decimals][x|X|b], e.g. {n:>8} or {x:.2}`), and a malformed string (`unclosed { in a format string (write {{ for a brace)`, `a } in a format string closes nothing (write }} for a brace)`, `{} needs a value: {name} or {(expression)}`).
Errors are reported at the string literal.

**How it runs.** The resolver splits each string as it reads a module, before it qualifies names, so a placeholder's names are renamed like the rest of the module: `(println "x = {x:>4}!")` becomes `(format.println "x = " (format.piece x ">4") "!")`. The checker types each piece and checks its format. Then, once the program checks, the forms become the calls a programmer would write to `std/show` (section 10), one per piece, chosen by the value's type, `(call show.println (call show.lit (call show.put_i32 (call show.lit (call show.start) "x = ") x 8388612) "!"))`, so no backend has anything to do with them. A module that uses a form imports `std/show` itself. Every implementation does all three steps and arrives at the same program. A format string only known at run time is not supported.

### L. Tests

A module may hold tests, next to the code they check:

```lisp
(module rows
  (import str)
  (fn parse_row [s:str] -> i32
    (if (eq (str.len s) 0) 0 (+ 1 (call str.count_byte (call str.from_str s) 44))))

  (test "an empty line has no fields"
    (expect (eq (call parse_row "") 0)))

  (test "three fields"
    (let n:i32 (call parse_row "a,b,c"))
    (expect (eq n 3))))
```

`(test "name" body...)` is an item of the module, so its body may call the module's private functions. Its statements run in order; `(expect c)`, allowed only inside a test, checks a `bool`: when it is false, the test fails, and `FILE:LINE:COL: (expect c)` is written to standard error, with both sides when `c` is a comparison (`eq`, `neq`, `lt`, `lte`, `gt`, `gte`) whose sides the formatting forms print (section 4.K): `rows.aipl:9:5: (expect (eq n 3)): left 2, right 3`. Each side is evaluated once. The test goes on after a failed `expect`, so one run shows every failure; a trap ends the test (and only it) with its message and call chain.

Tests are left out of every build but a test build, so they cost an ordinary program nothing. In a test build, test K of the file (not of its imports) is a function, and the program runs the test its first argument names, then exits with that test's failure count (at most 125). A module with its own `_start` cannot have a test build.

A test runs in a fresh process, so tests share no state. It passes when its body ends with no failed `expect` and no trap, and the tool that runs it shows a failing test's output under its name. (The `aipl test` command is described in docs/IMPLEMENTATION.md.)

Errors: `a test is (test "name" body...)`; `expect is only for a test's body: (test "name" ... (expect c))`; `expect takes one condition, e.g. (expect (eq n 3))`; `expect takes a bool, e.g. (expect (eq n 3)), got i32`; `a test build makes its own _start: test a module without one`.

### M. `when`, `for`, `and`/`or` of more than two, and the array literal

`(when c body...)` is the `if` with one branch, and the only one: an `if` whose then or else is an empty `(block)` is an error (`an if has two real branches, and this else is an empty (block): for one branch, write (when c then...)`, and for an empty then `an if has two real branches, and this then is an empty (block): for an else alone, write (when (not c) else...)`). The parsers read `when` themselves, as `(if c (block body... (block)) (block))`: the empty block that ends the body makes every expression of the body a statement, so a value in it is dropped, as in a `while` or `for` body, and the `when` is `void`. The printers write that tree back as `when` (CANONICAL_PLAN.md step 4).

The others are written for the reader and rewritten as the resolver reads a module (referee/src/sugar.rs; aipl_src/resolver.aipl), so nothing after it sees them:

| Form | Is | |
|---|---|---|
| `(for i start end body...)` | the internal counted loop `(counted_loop i start (- end 1) 1 body...)` (inclusive, with a step; not for programs, and `loop` written in a program is an error: CANONICAL_PLAN.md step 3) | `i` from `start` up to `end`, `end` left out: `(for i 0 (arr.len xs) ...)` visits every element; `start` and `end` are evaluated once, before the first iteration; `break` and `continue` work as in `while` (`end` must be above `i32`'s least value, as `(- end 1)` wraps) |
| `(and a b c ...)`, `(or a b c ...)` | `(and a (and b c ...))` | left to right, stopping at the first operand that decides |
| `(arr.of T a x y z)` | `(block (let _arr_of_L_C:(arr T) (call (alloc.array T) a 3)) (arr.set T _arr_of_L_C 0 x) (arr.set T _arr_of_L_C 1 y) (arr.set T _arr_of_L_C 2 z) _arr_of_L_C)`, `L` and `C` the form's line and column | the array literal (section 4.A); `(arr.of T a)` an empty array; not inside a formatting form's placeholder (bind it with `let` first) |

Errors: `when takes a condition and a body: (when c body...)`; `for is (for i start end body...): i from start up to end, end left out`; `a for has no step: ...` for a number right after a `for`'s end. `arr.of takes the allocator to make it from, then the values: ...` for an `arr.of` with no allocator, or a literal where the allocator goes. Every list a rewrite makes is at the form's place, so positions, and bytes, are the same in both toolchains.
---

## 5. Semantics

This section says what a program does, in terms of the language, so that an implementation can be a tree-walking interpreter, a WebAssembly compiler, a native compiler, or anything else, and be checked against the same text. The rules for each form are in sections 3, 4, 7, and 8; this section is the model they share, and the rules that cut across forms. Where a form's rule and this section seem to disagree, that is a defect in this document: the conformance suite (section 6.4) decides until it is fixed.

### 5.1 The machine

A running program has:

- **Linear memory**: a sequence of bytes addressed by non-negative `i32` addresses, a whole number of 64 KiB pages. It starts at 16 pages (1 MiB) and can grow to 32768 pages (2 GiB), or to a lower limit the host sets (`Out of memory: the limit is N MiB`). Because memory never reaches 2 GiB, every address is a non-negative `i32`. Multi-byte loads and stores are little-endian and may be unaligned (section 4.A). The first bytes are laid out as section 7.9 says: a runtime block below 1024, then the program's string literals, then the heap.
- **A call stack** of frames, each holding the values of its parameters and `let` variables. A frame is not in linear memory and has no address.
- **The standard streams and the host's other resources**, reached only through the host operations (section 4.C): files, clocks, randomness, the command line and environment, network sockets, and child processes.

The program's state is these three together. Nothing else is observable: no hidden global variables, no garbage collector, no finalizers.

### 5.2 Values

| Type | A value is |
|---|---|
| `i32`, `i64` | a two's-complement integer of 32 or 64 bits (section 8) |
| `f32`, `f64` | an IEEE 754 binary32 or binary64 number, NaNs and infinities included |
| `bool` | `true` or `false`, held as 1 or 0 |
| `str` | the address of immutable UTF-8 bytes preceded by a 4-byte little-endian length (section 4.B) |
| an `enum` | one of its members, whose number is an `i32` (section 4.I) |
| `(ptr S)`, `(arr T)` | an address and the generation of the object it was made for (section 4.E); address 0 is null |
| a value struct | its fields, held inline wherever the value is held, copied when it is passed, returned, or assigned (section 4.E) |
| a union, a `result` | a tag and the fields of the chosen variant, held inline and copied the same way (sections 4.F and 4.J) |
| `(fn [...] -> T)` | a reference to a function of that signature (section 4.G) |
| `u8`, `i8`, `u16`, `i16` | not values: storage types for fields and elements, read and written as `i32` (section 4.A) |

Every value is held in at most 16 scalars (section 4.E). Two values of different types never meet in one operation, and no type converts to another unless a form says so: `i32` and `i64` are joined by `i64.extend_s`, `i64.extend_u`, and `i32.wrap`; floats by the conversions of section 4.A; an enum and an integer by `enum.ord` and `enum.cast`.

### 5.3 Evaluation

A program is evaluated strictly and in the order it is written.

- **Order.** The operands of an operation, the arguments of a call, and the parts of any other form are evaluated left to right, each completely, before the operation or call itself. `(f (g) (h))` runs `g`, then `h`, then `f`.
- **Conditionals.** `if`, `cond`, `match`, and `match_result` evaluate their condition or subject, then exactly one branch. `(and a b ...)` and `(or a b ...)` stop at the first operand that decides, and evaluate no later one (section 7.10). `(when c body...)` is an `if` with no else.
- **Loops.** `while` evaluates its condition before every pass. `for` evaluates its start and end once, in that order, before the first pass (section 7.5). `break` leaves the innermost loop and `continue` starts its next pass.
- **Jumps.** `return`, `break`, and `continue` abandon the rest of every expression they are inside, operands not yet evaluated included (section 7.10). A function that returns runs its `ens` and `inv` contracts, and then its `defer` statements, the last written first (section 4.F), before control reaches the caller.
- **Calls** pass every argument by value: an integer, a float, a `bool`, or an address is copied; a value struct, a union, or a result is copied whole. A pointer's referent is shared. A function's parameters and variables are its own; there are no globals but constants.
- **Contracts.** Each `req` is checked before the body and each `ens` after it, with `res` bound to the result, and an `inv` at both ends (section 7.6). A failed contract is a trap (section 5.6).
- **Scope.** A `let` is visible to the end of its block or arm, and a name already in scope cannot be declared again (section 7.1).
- **Depth.** Calls may nest only so deep (section 7.11); running out is a trap, not undefined behaviour.

### 5.4 Numbers

- **Integers** follow section 8. `+ - *` on `i32` and `i64` trap when the exact result does not fit; `wrap.add`, `wrap.sub`, `wrap.mul` give it modulo 2^32 or 2^64. `/` and `%` truncate toward zero; division or remainder by zero, and `MIN / -1`, trap; `MIN % -1` is 0. Shift and rotate counts are taken modulo the width. `clz` and `ctz` of 0 are the width. `ltu lteu gtu gteu`, `divu`, `remu`, and `shru` read their operands as unsigned.
- **Floats.** `+ - * /` on `f32` and `f64` are IEEE 754 arithmetic in that format, rounded to nearest with ties to even: a result is the exact result rounded once, with no wider intermediate. Dividing by zero gives an infinity or NaN, and no float arithmetic traps. `(f64.sqrt x)` is the correctly rounded square root. The sign and payload bits of a NaN an operation produces are not specified; a program must not depend on them, and `i32.reinterpret_f32` and `i64.reinterpret_f64` can show them.
- **Comparisons.** `eq neq lt lte gt gte` on floats are IEEE 754's: every comparison with a NaN is false except `neq`, which is true; `0.0` equals `-0.0`. On integers they are signed. `eq` and `neq` also compare `bool`s, enums of the same type, and pointers (by identity); not strings, unions, or value structs, which have their own comparisons (sections 4.B, 4.E, 4.J).
- **Conversions** are explicit and exact as section 4.A and 8.1 say: integer to float rounds to nearest with ties to even; float to integer drops the fraction and traps on NaN (`invalid conversion to integer`) or on a result that does not fit (`integer overflow`); `f32.demote` rounds, `f64.promote` is exact; the `reinterpret` forms keep the bits.

### 5.5 Memory

- **Addresses.** A load or store of `n` bytes at address `a` traps unless `a >= 0` and `a + n` is within memory. Raw loads and stores are unchecked in every other way, and only in `unsafe` code (section 3).
- **Allocation.** `mem.alloc` hands out the next block of an ever-growing bump region and never frees it (section 4.A). Memory that can be freed comes from the allocators of the standard library (section 10).
- **Reserved memory.** Bytes 0-3 (the heap cursor), 64 up to the heap start (runtime cells and string literals) cannot be stored to: a store there traps, however the address was computed (section 7.9). Reads are not checked.
- **Pointers and arrays are checked.** `get`, `put`, `arr.get`, `arr.set`, and `arr.len` trap on a null pointer or array, on a pointer or array whose object has been freed since it was made (its generation no longer matches: `Use after free`), and, for arrays, on an index outside `0 .. len` (section 4.E). The check does not depend on the allocator.
- **Layout.** The sizes and offsets of structs, arrays, strings, unions, and results are part of the language (section 7.9), so a program that prints an address prints the same one everywhere.

### 5.6 Traps

A **trap** stops the program at the point it happens. No later expression is evaluated, no `defer` runs, and nothing can catch a trap. Output already written is delivered. The program ends with an exit status other than 0 (134 for a compiled program) after the host reports a message on standard error and the chain of calls it happened in, innermost first (section 6.1).

These are traps: integer overflow in `+ - *` and in a float-to-integer conversion; integer division or remainder by zero; `MIN / -1`; an invalid float-to-integer conversion; a load or store outside memory; a store into reserved memory; a null pointer or array; use of a freed pointer or array; an array index out of bounds; a failed `req`, `ens`, `inv`, `assert`, or `abort`; a call stack that is exhausted; memory that cannot grow; and a `req` of a standard library function that is not met. Everything else that can fail at run time comes back as a value: a failed host operation is a negative number, a failed library call a `result` or an `option.Option`.

`sys.exit` is not a trap: it ends the program with the status given (section 6.1).

### 5.7 Host operations

A program meets the world only through the host operations of section 4.C, and a host decides what to grant. A program starts with its standard streams, its working directory, and no more: listening on a socket, connecting to one, and starting a program each need a grant from whoever runs it (section 6.3), and without one they fail with a value, never a trap. The same operation on the same inputs and the same host state gives the same results in every implementation.

Output is delivered in the order the program writes it, per stream; standard output and standard error are two streams.

### 5.8 Concurrency

Threads are experimental and outside the version-1 promise (section 4.D). Where a program uses them, `atomic.add`, `atomic.cas`, `atomic.lock`, and `atomic.unlock` are sequentially consistent, `mem.alloc` is atomic, and a plain load or store of memory another thread may write is a data race that the language does not check and does not define.

### 5.9 What an implementation may differ in

A conforming implementation may differ from another only where this document says it may: the sign and payload of a NaN a float operation makes; whether `atomic.lock` or `atomic.unlock` on a word that is not 0 or 1 stops the program (the VM does); the text of a message (its location and kind are the contract: section 9); the numbers a host gives as process and socket handles, which a program must not print (section 4.C); timing, and the values of clocks and of randomness; and the speed and stack depth available to a program, within section 7.11's bounds. A program that avoids threads, the clock, randomness, the environment, files, sockets, child processes, and NaN payloads does the same thing in every implementation: the same output, the same exit status, and the same trap.
---

## 6. Programs, Hosts, and Conformance

How a program is run, what it can reach, what a WebAssembly module of it looks like, and what an implementation has to do to be called one of AIPL. How the `aipl` tool does it (commands, stages, backends, the optimizer) is in [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md); none of that is part of the language.

### 6.1 Running a program

A program is a module and the modules it imports (section 11), flattened into one. It is run by calling its `main`, a function of no parameters that returns one of three types; any other is an error:

- `i32`: the exit status, 0 for success and 1-125 for a failure.
- `(result i32 str)`: `(ok:str 0)` (or another status), or `(err:i32 "why")`, whose message goes to standard error, then a newline, and the status is 1.
- `void`: status 0.

`sys.exit` ends the program with its status from anywhere. A status must be 0-125: any other value, a negative one included, is an error reported with status 134, since shells reserve 126 and up. The value `main` returns is only the status; a program that reports a number prints it (`(println "{n}")`) and returns 0.

The status a program ends with, in every case:

| How the program ends | Status |
|---|---|
| `main` returns an `i32` `n` from 0 to 125 | `n` |
| `main` returns `(ok:str n)` | `n`, under the same limit |
| `main` returns `(err:i32 "why")` | 1, after `why` and a newline on standard error |
| `main` returns `void` | 0 |
| `sys.exit n` with `n` from 0 to 125 | `n`, from anywhere in the program; no later expression runs |
| `main` returns, or `sys.exit` is given, a number outside 0 to 125 | 134, after a message on standard error |
| a trap (section 5.6): overflow, a failed `req`, `ens`, `inv`, `assert`, or `abort`, an invalid access, exhausted memory or stack | 134, after the message and the chain of calls (a host that cannot give 134 gives another non-zero status) |

A tool that runs a program (`aipl run`, `aipl eval`, a compiled executable) ends with the program's status unchanged. A program the tool refuses to run, because it does not read or type-check, ends with a status of its own that the tool defines (the `aipl` command uses 1 for a refused program, and 2 for a command line it does not understand: docs/IMPLEMENTATION.md 2).

The program sees the real standard input, output, and error; its command line, with the program as `argv[0]` (`std/os`); the environment; and a working directory, with relative paths resolved in it (section 4.C). It sees nothing else unless it is granted it (section 6.3).

A trap (section 5.6) ends the program with a non-zero exit status, 134 in a compiled program, after a message on standard error. The message says what happened, then lists the chain of calls, the innermost first, each as the function and where in the source it was: the expression that trapped for the innermost, the call for the others. The text is the host's; its layout is typically

```
./prog: wasm trap: integer divide by zero
  at math.div (math.aipl:4:5)
  at main (prog.aipl:9:22)
```

A function an optimizer has inlined may have no frame of its own, so a chain may be shorter than the program's calls. A chain may be cut to its first 32 frames. The exit-status error above (`(err:i32 "why")`) is not a trap: it is one line.

### 6.2 The WebAssembly target

`aipl compile` writes a core WebAssembly module, and any host that implements WASI preview 1 and the two modules above can run it. What a host may rely on:

- **Exports.** Every function, under its AIPL name (`add`, `compiler.tokenize`), and the memory as `memory`. A module with a zero-argument `main` also exports `_start`, which calls `main`, so the module is a WASI command (section 6.1). A threaded module exports `wasi_thread_start`.
- **Imports.** Only the host functions the program uses, so a program that does no I/O has no import section: from `wasi_snapshot_preview1`, from `aipl_net_v1`, from `aipl_sys_v1`, and, for a threaded module, `wasi` `thread-spawn` and its memory as `env` `memory`, shared.
- **Memory.** One linear memory of 16 pages at least, 32768 at most (or the program's `--max-memory`, which the module declares), exported as `memory`. The layout is section 7.9's: the heap cursor at address 0, string literals from address 1024 as `[length u32 LE][bytes]`, the heap after them.
- **Custom sections.** `name` (every function, for the chain of a trap); `aipl.lines` (for each function, the source position of every call and every instruction that can trap, so the chain can say where); and `aipl.language` (the digits of the language version the compiler implements, as text: `1`). A host may ignore all three.
- **Traps** are WebAssembly traps; a failed check writes its message into memory first, at the addresses section 7.9 gives (cells 92 and 96), for the host to print.
- **A module that compiles also validates** as WebAssembly.

How the compiler lays out the code inside (the order of sections, the locals, the instruction sequences) is not part of the language: it is described in docs/IMPLEMENTATION.md.

### 6.3 Hosts and grants

Everything a program does to the world is a host operation (section 4.C): reading and writing files, the clocks, randomness, the arguments and environment, sockets, and child processes. A host implements them as that section defines. Three kinds of resource are denied until granted, and a program that tries without a grant gets the failure value the operation defines (`-2` for `net.listen` and `net.connect`, `-76` for `proc.spawn`), never a trap:

- listening on an address and port,
- connecting to an address or name and port,
- starting a program (or `*`, any program).

The command-line spelling of a grant (`--listen`, `--allow-connect`, `--allow-run`, `--sandbox` for the working directory only) belongs to the tool that runs the program; what is fixed is that a program cannot reach beyond what it was granted, and that a compiled program carries its grants with it.

The host operations that are not WASI's own are in two import modules, `aipl_net_v1` (sockets) and `aipl_sys_v1` (`chmod` and child processes); a host that provides both, and WASI's `wasi_snapshot_preview1`, can run any compiled module (section 6.2).

### 6.4 Conformance

An implementation **conforms** to language version `N` (section 1 of docs/LANGUAGE_VERSIONING.md) if, for every program:

1. **Reading.** It accepts the program exactly when the program matches the grammar of section 2 and satisfies the static rules of sections 3, 4, 7, and 11. For a program it rejects, it reports an error at the line and column of the offending token or form, in the form section 9 gives, whose message says what that section's table says; the wording may improve from release to release. (An import that cannot be found has no position: section 9.)
2. **Running.** It runs an accepted program so that its output on each stream, its exit status, and whether and how it traps are what section 5 prescribes, up to the differences section 5.9 allows.
3. **Library.** It provides the standard library of section 10, each function behaving as its entry in docs/STD_REFERENCE.md says.

An implementation that only checks programs (a verifier, an editor's language server) conforms to part 1 alone; one that only runs checked programs, to part 2.

The **conformance suite** is the referee. It is in `tests/conformance/`, apart from every compiler: a program per case and what must happen to it, in files any implementation can read. A case is one of three kinds:

- **run**: a program, its arguments and standard input, and its expected standard output, standard error (when it matters), and exit status;
- **trap**: a program that must trap, with the kind of trap, which is the key phrase of its message (`integer divide by zero`, `Array index out of bounds`);
- **reject**: a program that must be refused, with the line and column of the error and a key phrase of its message.

The suite is organised by the sections of this document, and each case names the section it tests. `tools/conformance.py` runs it against any `aipl` that can `compile` and `run`, or `verify`, and reports the cases it fails. The suite does not test speed, and it does not test what section 5.9 leaves open.
---

## 7. Typing and Evaluation Rules by Example

### 7.1 Typing and scoping rules

Six rules govern types and scopes:

1. `set!` has type void.
2. `if` whose two branches are both void is void; otherwise both branches must have the same non-void type — an if mixing void and non-void is a type error with a message suggesting `(block ... value)`. A branch that always leaves, a `return`, `break`, or `continue` (or a block ending in one, or an `if` whose branches both do), takes the other branch's type: `(if (gt n 0) n (return -1))` is an `i32`. The same holds for the arms of `cond`, `match`, and `match_result`. In compiled code such a branch leaves the wasm stack to the jump, or, ending in an `if` whose branches both leave, is followed by `unreachable`.
3. `let` has type void (it declares, it does not yield); a function body's last expression must therefore be a value expression when the return type is non-void.
4. A function's parameters have different names, and `let` is block-scoped: a `let` inside if/while/loop/block/match arms is visible only within that construct; shadowing an outer name is a type error. A name's scope is its block or arm: a sibling scope (another block, another `match` or `match_result` arm) may declare it again with any type, as `(block (let y:f64 1.5)) (block (let y:i32 3))` and two arms each binding `v`. A message names the variable as it was written.
5. `set!` on an undeclared name is a type error (and a VM runtime error); there are no implicit globals.
6. `match_result` binds its `ok` variable to the result's ok type and its `err` variable to the error type. `(ok:E v)` takes its ok type from `v` and names the error type `E`, and `(err:T e)` takes its error type from `e` and names the ok type `T` (section 7.7); a bare `(ok v)` or `(err e)` is an error that says so.

| Form | Type | Example |
|---|---|---|
| integer literal | `i32` | `42`, `-7`, `0xFF`, `0b1010` |
| suffixed integer literal | `i64` | `42i64`, `-7i64`, `4294967296i64`, `0x7FFFFFFFFFFFFFFFi64` |
| float literal | `f64` | `3.5` |
| float literal with `f32` | `f32` | `3.5f32` |
| `true` / `false` | `bool` | |
| `"text"` | `str` | |
| `(let x:T v)` | `void` (declares `x` as `T`; `v` must be `T`) | `(let n:i32 (+ 1 2))` |
| `(set! x v)` | `void` (`v` must match `x`; `x` must be declared) | `(set! n 5)` |
| `(if c a b)` | `void` if both `a`,`b` are `void`; else `T` where `typeof(a)==typeof(b)==T`; neither branch an empty `(block)` | |
| `(when c body*)` | `void`; the body's expressions are statements (a value is dropped, but not a result or an option: section 4.F) | |
| `(assert c)`, `(assert c msg)` | `void`; stops the program when `c` (a `bool`) is false, with the place, the text, both sides of a comparison, and `msg` (a `str`, evaluated only then) | `(assert (gt n 0) "n must be positive")` |
| `(abort msg)` | `void`; stops the program with `msg` (a `str`); a branch ending in one fits any type | `(abort "no negatives here")` |
| `(drop e)` | `void`; `e` is evaluated and its value, of any type but `void`, dropped (the way to ignore a result or an option on purpose, section 4.F) | `(drop (call fs.remove path))` |
| `(for i s e body*)` | `void`; `s e` must be `i32`; `i` is block-scoped to the body | |
| `(while c body*)` | `void` | |
| `(block e1 ... en)` | type of `en` (or `void` if empty); introduces a block scope | |
| `(call f args)` | declared return type of `f`; arity and arg types must match | |
| `(ok:E v)` | `(result (typeof v) E)` | `(ok:i32 42)` |
| `(err:T e)` | `(result T (typeof e))` | `(err:i32 -1)` |
| `(match_result r (ok v body*) (err e body*))` | type of the last expr of the bodies (which must agree); `v` bound as `T_ok`, `e` bound as `T_err` | |
| `NAME` (a constant) | the type of its literal | `PAGE_SIZE` |
| `Name.member` | the enum `Name` | `Kind.square` |
| `(enum.ord e)` / `(enum.cast Name n)` | `i32` / `Name` | `e` must be an enum, `n` an `i32` |
| `(make Name.v x ...)` | the union `Name` | one value per field of `v`, each of the field's type |
| `(match v arm ... [(else ...)])` | the type of the arms' last expressions, which must agree (or `void`) | `v` a union or enum; every member matched, or an `else` arm (section 4.J) |
| `(new S a [x:v ...])` / `(arr.new T a n)` | `(ptr S)` / `(arr T)` | `a` an `alloc.Allocator`; each `v` of its field's type |
| `(sizeof S)` or `(sizeof T)`, `(arr.len a)`, `(ptr.addr p)`, `(arr.addr a)` | `i32` | |
| `(get p S.f)` / `(arr.get T a i)` | the field's type / `T` | `p` must be `(ptr S)`, `a` must be `(arr T)`, `i` must be `i32` |
| `(put p S.f v)` / `(arr.set T a i v)` | `void` | as above; `v` must match the field / `T` |
| binary arithmetic / bitwise | type of the operands, which must be equal | `+ - * /` on numbers (`i32 i64 f32 f64`); `% divu remu ^ shl shr shru rotl rotr bitand bitor` on `i32`/`i64` only |
| `bitnot` / `clz` / `ctz` / `popcnt` | type of the operand | one argument, `i32` or `i64` |
| comparisons | `bool`; operands must have equal type | `eq neq` on anything but a union; `lt lte gt gte` on numbers; `ltu lteu gtu gteu` on `i32`/`i64` only |
| `checked.add` / `checked.sub` / `checked.mul` / `wrap.add` / `wrap.sub` / `wrap.mul` | type of the operands, which must be equal | `i32` or `i64` only |

A function body is a sequence of expressions. The **last** expression's type must equal the declared return type unless the return type is `void`, in which case the last value is discarded (a result or an option is an error there too, as in any statement: write `(drop e)`, section 4.F).

### 7.2 Valid: value-producing `if` as the final expression

```lisp
(module ex_if
  (fn sign [n:i32] -> i32
    (if (lt n 0) -1
        (if (eq n 0) 0 1))))
```
A driver calling `(call sign -5)` returns -1. Compiles to `if (result i32) ... else ... end` in wasm.

### 7.3 Invalid: mixed branch types

```lisp
(fn bad [n:i32] -> i32
  (if (lt n 0)
      (set! n 0)      ;; type void
      1)              ;; type i32
  n)
```
Checker output: `3:5: If branch type mismatch: then is void, else is i32. If mixing void and non-void, consider wrapping in (block ... value)`.

### 7.4 Void statements and void `if`

Since `set!` and `let` have type `void`, an `if` whose branches perform side effects is typed as `void`:

```lisp
(fn clamp_to_100 [n:i32] -> i32
  (let out:i32 n)
  (if (gt n 100)
      (set! out 100)
      (set! out n))
  out)
```
Void `if` statements compile cleanly to `if` (empty block type) in wasm.

### 7.5 Loops

`for` counts `i` from its start up to its end, **leaving the end out**, one at a time (for another step, use a `while` and `set!` its counter). There is no `loop`: it counted to its end inclusive, the most repeated mistake in this document's table of pitfalls, and written in a program it is an error that names `for`.

```lisp
(fn sum_to_ten [] -> i32
  (let acc:i32 0)
  (for i 1 11
    (set! acc (+ acc i)))
  acc)          ;; => 55: i is 1, 2, ..., 10; the end, 11, is left out
```
`(for i 0 10 ...)` runs 10 times. `(for i 0 0 ...)` does not run. `start` and `end` are each evaluated **once**, in that order, before the first pass (like Python's `range`), so `(for i 0 (call count r) ...)` calls `count` once. Changing a variable used in the bound inside the body does not change the bound. The body may still `set!` the loop variable itself. Leave a loop early with `break`, or skip to the next iteration with `continue` (section 7.10).

```lisp
(fn count_down [start:i32] -> i32
  (req (gte start 0))
  (let n:i32 start)
  (let steps:i32 0)
  (while (gt n 0)
    (set! n (- n 1))
    (set! steps (+ steps 1)))
  steps)
```

### 7.6 Contracts: what runs, where, and what happens on failure

- `req` expressions are type-checked with the parameters in scope and **evaluated before the body**, in every backend. Every `req` must be `bool`.
- `ens` expressions are type-checked with parameters plus `res` (bound to the return type) and **evaluated after the body** with `res` bound to the actual result, after an early `return` too.
- `inv` expressions are type-checked like `req` and **evaluated at both ends**: before the body (after the `req`s) and after it (after the `ens`s, `res` not in scope), in every backend: `Entry invariant failed in 'f' ...`, `Exit invariant failed in 'f' ...`.
- **Compiled code checks `req`, `ens`, and `inv` too.** The wasm backend (and so the native one) compiles each `req` (and `inv`) at the function's entry and each `ens` (and `inv`) at its exit; with an `ens`, the body is a block and `(return v)` branches to its end, where `v` is kept in `res`. A failure writes the message into memory, stores its address and length in cells 92 and 96 (section 7.9), and traps; `aipl-run` and native executables print it. The message is the VM's without the position, which is a position in the program text the resolver flattens and so is not stable across toolchains, and with only the values compiled code can show: integers, pointers, `i64`, and `bool` parameters and `res`, but not floats, strings, or results. The texts are string literals of the module, after the program's own.

```lisp
(module contracts_demo
  (fn safe_div [num:i32 den:i32] -> i32
    (req (neq den 0))
    (ens (or (eq num 0) (neq res 0)))   ;; weak, but bool
    (/ num den))

  (fn main [] -> i32
    (call safe_div 10 0)))
```
`aipl eval contracts_demo.aipl` fails with `Pre-condition failed in 'safe_div' at 3:10: (req (neq den 0)) with num = 10, den = 0`: the contract as source, its position, and the arguments (plus `res` for a failed `ens`). Compiled (`aipl compile`, then `aipl run`), it stops with `contracts_demo.wasm: Pre-condition failed in 'safe_div': (req (neq den 0)) with num = 10, den = 0` and exit status 134. A `req` or `ens` that is not `bool` is rejected at check time: `L:C: Contract expression in 'safe_div' must evaluate to bool, got i32`.

### 7.7 Results

`ok`/`err` build a result value; `match_result` consumes one and binds each arm's variable to the payload type from the result type (rule 6). Representation is in section 4.F.

```lisp
(fn parse_digit [c:i32] -> (result i32 i32)
  (if (and (gte c 48) (lte c 57)) (ok:i32 (- c 48)) (err:i32 -1)))

(fn digit_or_zero [c:i32] -> i32
  (match_result (call parse_digit c)
    (ok v v)
    (err e 0)))
```
`(ok:E v)` and `(err:T e)` each name the side of the result they do not carry, always: a bare `(ok v)` or `(err e)` is the error `(ok v) must name its result's err type: write (ok:E v) for a (result T E)`. Here both branches of the `if` are `(result i32 i32)`; `(ok:bool 1)` is `(result i32 bool)`. Both arms of `match_result` must end in the same type.

### 7.8 Memory

Linear memory is byte-addressed. Both backends start with 16 pages (1 MiB) and grow up to 32768 pages (2 GiB): automatically when an allocation needs it (section 4.A), or explicitly with `mem.grow`. Loads and stores are little-endian, unaligned access is allowed, and out-of-bounds access is a VM runtime error (`Memory store out of bounds: ptr N`) and a wasm trap. Get memory from `mem.alloc`; never pick an address yourself (section 7.9). Loads and stores on raw addresses are unchecked operations, allowed only in an unsafe context (section 3, "Unchecked operations"); structs, arrays, and the standard library are the checked way to use memory.

```lisp
(fn pack_two [] -> i32 (unsafe)
  (let p:i32 (mem.alloc 8))        ;; p == 1024 on a fresh VM or module
  (mem.store32 p 7)
  (mem.store32 (+ p 4) 35)
  (+ (mem.load32 p) (mem.load32 (+ p 4))))   ;; 42
```

### 7.9 Memory layout

There is one layout and one allocator, shared by every implementation. Bytes `0..1024` are the **runtime block**; the heap begins at `1024` and grows upward through `mem.alloc`.

| Address | Meaning |
|---|---|
| 0 | the heap cursor, an `i32`: the next address `mem.alloc` returns. It starts at the heap start. |
| 4..63 | cells the standard library and the toolchain own (the failure count of a running test, the command line's table, the shared default allocator, the generation counter, and the like). A program does not store to them; docs/IMPLEMENTATION.md lists them. |
| 64..87 | scratch the host operations use (the buffers and out-parameters of `sys.print`, `fs.read`, `fs.write`, and the others) |
| 88 | in a threaded module, 1 once the shared memory has been initialised (section 6.2) |
| 92, 96 | the address and length of a failed check's message (`Array index out of bounds: ...`, or a contract's `Pre-condition failed in ...`, written at the heap cursor), set just before the trap that ends the program; the host prints it with the trap (0 = none) |
| 128..256 | where a failed bounds check writes its message |
| 88..1024 | reserved: not handed out by `mem.alloc`, not for programs |
| 1024..heap start | string literals: each distinct literal, `[len u32 LE][bytes]`, in first-use order; a `str` value points at the bytes. Empty for a module without literals. Read-only |
| heap start.. | the heap, `mem.alloc`'s region. The heap start is the first 8-aligned address after the literals (1024 without literals); it is the initial value of the cursor at address 0 |

The runtime's own writes into 64-87 are emitted directly and bypass the store guard described below; user code still cannot store anywhere from 64 up to the heap start.

Rules that follow from this:

- **Never write to a literal address.** Take memory from `mem.alloc` and pass pointers around.
- **The checker enforces the block for literal addresses.** Any `mem.*` or `atomic.*` op whose address is a literal is rejected at check time if it stores to or locks bytes 0-3 (`... bytes 0-3 are the heap cursor owned by mem.alloc ...`), touches bytes 64-1023 (`... bytes 64-1023 are the reserved runtime block ...`), or names a misaligned cell in 4-63. Reading the cursor with `(mem.load32 0)` and using the aligned cells 4-60 is allowed. Literal heap addresses (1024 and up) are allowed but discouraged.
- **Both backends enforce the block for writes at runtime.** Every `mem.store*`, `put`, `arr.set`, and `atomic.*` op checks its address before writing: bytes 0-3, 64-1023, and the string literals (1024 up to the heap start) are refused however the address was computed. A store into a literal fails in the VM with `... bytes 1024-N are the program's string literals, which are read-only ...`. The VM fails with `mem.store32 at address 512: bytes 64-1023 are the reserved runtime block; take memory from (mem.alloc n) instead`; compiled wasm traps with `unreachable` on exactly the same addresses (the backend emits two compare-and-trap checks before each store, using the first of the two scratch `i32` locals each function has). This is a shared semantic, not a VM-only guard. Reads are not checked: the block is zero and reading it is harmless.
- **The VM additionally enforces lock validity.** A lock word is only ever `0` (free) or `1` (held). `atomic.lock` on a word holding anything else fails immediately with `atomic.lock: word at ptr N holds V, which is not a lock state ...` instead of spinning forever, and `atomic.unlock` on a word that is not `1` fails with `... a held lock holds 1 ...`. This is what turns "I locked the heap cursor by accident" from a silent hang into an error, whichever way the address was produced.
- **Fresh instances agree.** A fresh instance of any implementation returns the heap start from the first `mem.alloc` (1024 for a module without string literals), then that plus `size` rounded up to a multiple of 8, and so on, and it lays out the first module's string literals as this section says. Memory-heavy programs therefore behave alike everywhere, even in the addresses they print.
- **Threads share the block.** OS threads spawned by `thread.spawn` share the same linear memory and allocator; allocation is atomic, so workers may allocate. In a threaded module, spawned threads use their own copy of cells 64-87 (section 6.2).
- **Codegen state is per compile.** `compile_module` keeps its tables in a fresh `Cg`; `codegen_init` builds the keyword map only when cell 16 is zero and always clears cell 4.


### 7.10 Control flow: `return`, `break`, `continue`, `cond`

| Form | Meaning |
|---|---|
| `(return v)` / `(return)` | leaves the function with `v` (which must have the function's return type), or with nothing from a `void` function; `ens` contracts still run on the returned value |
| `(break)` | leaves the innermost `while` or `for` |
| `(continue)` | starts the next iteration of the innermost loop: a `while` re-tests its condition; a `for` goes on to its next value |
| `(and a b)` / `(or a b)` | short-circuit: `b` runs only if `a` is `true` (for `and`) or `false` (for `or`). Two or more `bool` operands, left to right (more than two are rewritten as nested pairs, section 4.M). So `(and (lt i n) (eq (arr.get i32 a i) x))` is a safe guard |
| `(cond (c1 e...) (c2 e...) ... (else e...))` | the first clause whose test is true runs its body; `else` is required, like `if`'s else branch. The parser rewrites it to `(if c1 (block e...) (if c2 (block e...) ... (block e...)))`, so the `if` typing rules apply |

`return`, `break`, and `continue` are statements of type `void`. They go where a statement goes: in a body, a `block`, a `cond` clause, or an `if`. An early exit is written `(when (lt i 0) (return -1))`; where the `if` gives a value, a branch that always returns, breaks, or continues fits any type (section 7.1), so `(let x:i32 (if (lt i 0) (return -1) i))` is fine too. A function body may end in `(return v)` instead of a bare `v`. `break` and `continue` outside a loop body (including a `while` condition) and `return` inside a contract are checker errors.

```lisp
(fn index_of [a:(arr i32) target:i32] -> i32
  (for i 0 (arr.len a)
    (when (eq (arr.get i32 a i) target) (return i)))
  -1)

(fn classify [n:i32] -> str
  (cond
    ((lt n 0) "negative")
    ((eq n 0) "zero")
    (else "positive")))
```

A jump inside an operand (`(+ x (block (when c (break)) 1))`, legal because a `block` may end in a value) leaves the whole expression at once in both backends: operands after it are not evaluated.

### 7.11 Recursion depth

Calls nest as deep as the stack allows, and an implementation supports at least 10,000 nested calls of a small function. Running out of stack stops the program with a message, never a crash: a trap (section 5.6) whose message says the call stack is exhausted (`call stack exhausted`, or `Call stack exhausted calling 'f'` when the interpreter finds it), with the call chain.

A call's room is its frame: a function with many locals or deeply nested expressions gets fewer calls, and the depth reached differs between implementations. Code that must run anywhere should stay well below 10,000 nested calls; walk long lists and deep structures with `while`.

---

## 8. Integer Semantics

`i32` is a two's-complement 32-bit integer in **both** backends. `+`, `-`, and `*` stop the program when the exact result does not fit (section 8.2); `wrap.add`, `wrap.sub`, and `wrap.mul` give the result modulo 2^32, as wasm's `i32.add` does, for hashes, generators, and checksums that want it. The VM stores integers in an `i64` but truncates every operand to `i32` before each operation, so no value wider than 32 bits is observable.

| Expression | Result | Why |
|---|---|---|
| `(+ 2147483647 1)` | runtime error | VM: `Integer overflow in +`; compiled: trap |
| `(* 65536 65536)` | runtime error | the same |
| `(- 0 -2147483648)` | runtime error | the same |
| `(wrap.add 2147483647 1)` | `-2147483648` | wraps |
| `(wrap.mul 65536 65536)` | `0` | wraps |
| `(wrap.sub 0 -2147483648)` | `-2147483648` | wraps |
| `(/ -7 2)` | `-3` | truncates toward zero (`i32.div_s`) |
| `(% -7 2)` | `-1` | sign follows the dividend (`i32.rem_s`) |
| `(/ 7 0)` | runtime error | VM: `Division by zero`; wasm: trap |
| `(/ -2147483648 -1)` | runtime error | VM: `Integer overflow`; wasm: trap |
| `(% -2147483648 -1)` | `0` | defined, not a trap (`i32.rem_s`) |
| `(divu -1 2)` | `2147483647` | operands reinterpreted as unsigned |
| `(remu -1 2)` | `1` | |
| `(shl 1 33)` | `2` | shift count masked to 5 bits (`33 & 31 == 1`) |
| `(shr -8 1)` | `-4` | arithmetic shift, sign-extending |
| `(shru -8 1)` | `2147483644` | logical shift, zero-filling |
| `(rotl 0x80000001 1)` | `3` | rotate left; the count is masked to 5 bits (`(rotl x 33)` is `(rotl x 1)`); `rotr` rotates right |
| `(bitnot 0)` | `-1` | every bit flipped, `(^ x -1)` |
| `(clz 1)`, `(ctz 8)`, `(popcnt 255)` | `31`, `3`, `8` | leading zero bits, trailing zero bits, one bits; `(clz 0)` and `(ctz 0)` are `32` |

### 8.1 `i64`

`i64` is a first-class declarable type with the same two's-complement semantics at 64 bits (`+ - *` trap on overflow, `wrap.*` wrap). There is **no implicit widening**: an `i32` and an `i64` never meet in one operation, and the checker rejects `(+ 1 2i64)` with `Type mismatch in binary op: i32 vs i64`. Conversions are explicit ops that map one-to-one onto wasm instructions.

| Form | Meaning | wasm |
|---|---|---|
| `42i64`, `-7i64`, `4294967296i64` | 64-bit literal; the suffix is part of the token | `i64.const` |
| `(let x:i64 0i64)`, `[a:i64]`, `-> i64` | declarations | local/param/result `i64` |
| `+ - *` on two `i64` | 64-bit, trapping on overflow (section 8.2); `wrap.add wrap.sub wrap.mul` wrap | `i64.add` ... with the check |
| `/ % divu remu ^ bitand bitor shl shr shru rotl rotr` on two `i64` | 64-bit; shift and rotate counts masked to 6 bits | `i64.div_s` ... `i64.rotr` |
| `(bitnot x)`, `(clz x)`, `(ctz x)`, `(popcnt x)` on `i64` | an `i64` (`clz` and `ctz` of 0 are 64) | `i64.xor` with -1, `i64.clz`, `i64.ctz`, `i64.popcnt` |
| `eq neq lt lte gt gte` on two `i64` | signed comparison, yields `bool` | `i64.eq` ... `i64.ge_s` |
| `(i64.extend_s x)` | `i32 -> i64`, sign-extending | `i64.extend_i32_s` |
| `(i64.extend_u x)` | `i32 -> i64`, zero-extending | `i64.extend_i32_u` |
| `(i32.wrap x)` | `i64 -> i32`, low 32 bits | `i32.wrap_i64` |
| `(f64.sqrt x)` | `f64 -> f64`, the correctly rounded square root (IEEE 754); NaN for a negative `x`, `(f64.sqrt -0.0)` is `-0.0` | `f64.sqrt` |
| `(f64.convert_i64_s x)` | `i64 -> f64`, rounded to nearest (ties to even) | `f64.convert_i64_s` |
| `(i64.trunc_f64_s x)` | `f64 -> i64`, toward zero; NaN or out of range is a VM error / wasm trap | `i64.trunc_f64_s` |
| `(f64.reinterpret_i64 x)` / `(i64.reinterpret_f64 x)` | same 64 bits, other type | `f64.reinterpret_i64` / `i64.reinterpret_f64` |
| `(mem.load64 p)` | reads 8 little-endian bytes as `i64` | `i64.load` |
| `(mem.store64 p v)` | `v` must be `i64` | `i64.store` |

| Expression | Result |
|---|---|
| `(+ 2147483647i64 1i64)` | `2147483648` (no i32 wrap) |
| `(+ 9223372036854775807i64 1i64)` | runtime error / trap |
| `(wrap.add 9223372036854775807i64 1i64)` | `-9223372036854775808` |
| `(* 4294967296i64 2147483648i64)` | `-9223372036854775808` |
| `(divu -1i64 2i64)` | `9223372036854775807` |
| `(shl 1i64 65i64)` | `2` (count masked: `65 & 63 == 1`) |
| `(i64.extend_s -1)` | `-1` |
| `(i64.extend_u -1)` | `4294967295` |
| `(i32.wrap 4294967301i64)` | `5` |
| `(i32.wrap (+ (i64.extend_s 2147483647) 1i64))` | `-2147483648` |

Loop bounds, memory addresses, `mem.alloc` sizes, file descriptors, and thread handles remain `i32`. To index memory with an `i64` computation, narrow it first with `i32.wrap`. The VM prints an `i64` result as `Int64(n)`, an `i32` as `Int(n)`.

### 8.2 Unsigned comparisons and checked arithmetic

The signed comparisons (`lt lte gt gte`) read their operands as two's-complement numbers. `ltu lteu gtu gteu` compare the same bits as unsigned numbers (wasm `i32.lt_u` ... `i64.ge_u`), so `-1` is the largest value: the right test for sizes, hashes, and addresses that may exceed `2^31`.

`+ - *` on `i32` and `i64` stop the program when the exact result does not fit in the operands' type: the VM fails with `Integer overflow in +` (`-`, `*`), and compiled code traps with the host's integer-overflow trap. (On `f32` and `f64` they are IEEE arithmetic and never trap.) A program that wants the wrapped result says so with `wrap.add`, `wrap.sub`, or `wrap.mul`, which take two `i32` or two `i64` and give the low bits of the exact result, as wasm's `i32.add` does. `checked.add`, `checked.sub`, and `checked.mul` are the same as `+ - *` on integers, kept as the older spelling. Constant expressions (`const`) and the optimizer's folds follow the same rule: a constant that overflows is a compile error (`+ overflows`), and the optimizer leaves an overflowing constant expression to trap when it runs.

| Expression | Result |
|---|---|
| `(ltu 1 -1)` | `true` (`-1` is `4294967295` unsigned) |
| `(gtu -2147483648 2147483647)` | `true` |
| `(lteu -1i64 0i64)` | `false` |
| `(checked.add 2147483646 1)` | `2147483647` |
| `(checked.add 2147483647 1)` | runtime error / trap |
| `(checked.sub -2147483648 1)` | runtime error / trap |
| `(checked.mul 46341 46341)` | runtime error / trap (`46340` squared fits) |
| `(checked.mul -1i64 -9223372036854775808i64)` | runtime error / trap |

---

## 9. Diagnostics

Every parser and checker error is a single line of the form

```
<line>:<col>: <message>
```

with 1-based line and column of the offending token or the opening `(` of the offending form. Every error found before run time is prefixed with the path of the file it is in (`examples/x.aipl: 3:5: Unknown op/keyword: badop`), whichever stage finds it. An error inside a generic instance names the template's file, or the file that wrote the type argument when the error is at that argument.
A syntax error, an import error, or an error in a constant is reported alone, since everything after depends on it. Otherwise every definition (enum, struct, union, function signature) is checked on its own and each failing one reports its first error, one per line in that order (enums, structs, unions, signatures; a value struct or a union that fails keeps the checks that look inside them from running). If every definition holds, every function body is checked, and each failing function reports its first error, one per line in source order. At most 20 errors are reported; a last line, `and N more errors`, counts the rest. Every implementation gives the same lines. A runtime error prints its call chain with the source position of each frame (`  at f (math.aipl:4:5)`): a compiled trap and an error in the interpreter alike (section 6.1); only the message's wording differs between them (`Division by zero` against wasmtime's `integer divide by zero`).
A VM contract failure also gives the contract's position in its message (section 7.6). A module parsed from one text (not through the resolver) names no file: `  at f (3:5)`.

Representative messages. What a conforming implementation fixes is *where* (the line and column) and *what the message says* (the situation named in the first column; the key words are the ones shown). The exact wording may improve from release to release, and the conformance suite (section 6.4) pins only the position and a key phrase. An error that has no position in the program's text is the one for a file that cannot be found (`Cannot resolve import 'NAME': no 'NAME.aipl' found in ...`), which names the path instead.

| Situation | Message |
|---|---|
| missing `)` at end of file | `1:1: this bracket is never closed` (the position of the unclosed bracket) |
| extra `)` after the module | `3:1: unexpected input after the module's closing ')'` |
| `if` with no else branch | `1:37: Unexpected token parsing expression: ')'` (the `)` where the else was expected) |
| unknown operator | `3:5: Unknown op/keyword: badop; to call a function, write (call badop args...)`, or `did you mean when?` for a near name |
| an operator as C writes it (`=`, `==`, `!=`, `<`, `>`, `<=`, `>=`, `&&`, `\|\|`, `!`) | `3:5: Unknown op/keyword: =; '=' is not an operator; equality is (eq a b)`, naming the form: `neq`, `lt`, `gt`, `lte`, `gte`, `and`, `or`, `not` |
| unterminated string | `4:12: Unterminated string literal` |
| wrong literal type in `let` | `3:5: Type mismatch in 'let': expected i32, got bool` |
| undefined name | `3:5: Undefined variable 'x'` |
| `set!` before `let` | `3:5: Undefined variable 'x' in set!` |
| `if` branches disagree | `3:5: If branch type mismatch: then is void, else is i32. If mixing void and non-void, consider wrapping in (block ... value)` |
| `let` of a name already in scope | `3:12: Cannot shadow existing variable 'x'` |
| two parameters of one name, `(fn f [a:i32 a:i32] ...)` | `2:3: function 'f' has two parameters named 'a'` |
| function body ends in `let` | `2:3: Function 'f' expects return type i32, but body returned void` |
| mixed `i32` / `i64` operands | `3:5: Type mismatch in binary op: i32 vs i64` |
| unknown struct | `3:5: Unknown struct 'P'` |
| unknown field | `3:5: Struct 'P' has no field 'z'` |
| wrong `put` value type | `3:5: Type mismatch writing to field 'P.x': expected i32, got bool` |
| struct defined twice in one module | `1:30: Duplicate struct definition 'P'` |
| `get`/`put` through the wrong pointer | `4:5: get Point.x needs a (ptr Point), got (ptr Node)` |
| an `i32` where a pointer is expected | `3:5: get Point.x needs a (ptr Point), got i32` |
| arithmetic on two pointers, `(+ p p)` | `3:5: + on (ptr Point): pointers, arrays, and function refs have no arithmetic; use get/put or arr.get/arr.set, or convert with ptr.addr/arr.addr and ptr.cast/arr.cast` |
| a pointer plus a number, `(+ p 4)` | the same message as two pointers |
| arithmetic on an enum, `(+ k 1)` | `3:5: + on enum 'Kind': enums have no arithmetic; compare them with eq/neq, or convert with (enum.ord x) and (enum.cast Kind n)` |
| two different enums compared | `3:5: Type mismatch in comparison: C vs D` |
| an enum member that does not exist | `m.aipl: 3:5: enum 'Kind' has no member 'circle'` |
| a struct written as a type, `p:Point` | `1:30: 'Point' is a struct, which is only used through a pointer: write (ptr Point)` |
| a constant reused as a variable | `m.aipl: 3:10: 'MAX' is a constant; it cannot also name a variable, parameter, or field` |
| a constant whose value does not match its type | `m.aipl: 1:25: constant 'NN' is declared i32 but its value is not an i32 literal` |
| `arr.get` with the wrong element type | `4:5: arr.get i64 needs an (arr i64), got (arr i32)` |
| `(ptr i32)` | `1:20: (ptr i32) is not a type: ptr points to a struct; for a sequence of i32 use (arr i32)` |
| `call_ref` signature differs from the reference's type | `3:5: call_ref signature (fn [i32 i32] -> i64) does not match the function's type (fn [i32 i32] -> i32)` |
| wrong `thread.spawn` worker | `1:51: thread.spawn needs a worker of type (fn [i32] -> i32), got (fn [i64] -> i32)` |
| array index out of range (runtime, VM) | `Array index out of bounds: index 3 for array of length 3` |
| `void` result payload | `1:60: a result payload must be a value, got void` |
| non-bool `if` condition | `3:5: If condition must be bool, got i32` |
| `break` outside a loop | `3:5: break is only allowed inside a while or loop body` |
| `return` with the wrong type | `3:5: return value has type bool, but the function returns i32` |
| `cond` without `else` | `1:32: cond needs a final (else ...) clause, as if needs an else branch` |
| wrong arity | `4:5: Function 'add' expects 2 arguments, got 1` |
| wrong argument type | `4:5: Argument 2 of 'add' expects i32, got bool` |
| body/return mismatch | `2:3: Function 'f' expects return type i32, but body returned bool` |
| an op that does not exist | `3:5: there is no mem.free: memory from mem.alloc is never freed. Free single objects with std/heap, or everything in a region at once with std/arena` |
| non-`str` `sys.print` | `3:5: sys.print prints str values, got i32; for numbers and other values use the format forms, (print "{n}") or (println "n: {n}")` |
| `(+ str str)` | `3:5: + does not join strings: join two with (call str.concat a b), or build text with std/buf (buf.push_str, then buf.to_str)` |
| arithmetic or ordering on something that is not a number | `3:5: + needs numbers (i32, i64, f32, f64), got bool` / `% is integer arithmetic (i32 or i64), got f64` / `lt on bool: only numbers are ordered; bool compares only with eq/neq` |
| wrong operand count or type for an op | `3:5: atomic.cas takes 3 operands, (atomic.cas p expected new); got 2` / `atomic.add operand 2 must be i32, got i64` |
| non-`bool` contract | `1:50: Contract expression in 'safe_div' must evaluate to bool, got i32` |
| store or lock at literal address 0 | `3:5: atomic.lock at address 0: bytes 0-3 are the heap cursor owned by mem.alloc; locking it hangs and storing to it corrupts the allocator. Take memory from (mem.alloc n) instead` |
| literal address in 64-1023 | `3:5: mem.store32 at literal address 512: bytes 64-1023 are the reserved runtime block. Take memory from (mem.alloc n) instead` |
| locking a word that is not 0/1 (runtime, VM) | `atomic.lock: word at ptr 1024 holds 1024, which is not a lock state (0 = free, 1 = held); this address is data, not a mutex. Allocate a dedicated lock word with (mem.alloc 4)` |

Types and ops in messages are written as AIPL source writes them: `i32`, `bool`, `(result i32 i32)`, `(ptr Point)`, `(arr i32)`, `(fn [i32] -> i32)`, an enum or union by its name, and ops as `+`, `lt`, `mem.store32`; both checkers spell them the same.
---

## 10. The Standard Library

The language comes with a standard library, written in AIPL, that a program meets by `(import NAME)` and calls as `(call NAME.function ...)` (section 11). It is part of the language version (docs/LANGUAGE_VERSIONING.md): within a version, a function that is there keeps its name, its types, and its behaviour, and one that is removed or changed is deprecated for a release first.

**What is specified.** The contract of a library function is its signature, its `req` and `ens` contracts, and the comment above it. They are collected, module by module, in [docs/STD_REFERENCE.md](docs/STD_REFERENCE.md), which is generated from the library's source by `aipl doc` and checked against it by CI. A conforming implementation provides every module below with every public function (section 6.4). The reference is the normative text for the library; this section is the rules that hold for all of it, and the list of its modules.

**Rules for every module.**

- A function that makes something that must be freed takes the allocator to make it from (an `alloc.Allocator`), and the matching `free` gives it back. The library never allocates from a hidden place that a program cannot free, except where its entry says so (a `str` made at run time is never freed: section 4.B).
- A failure that a caller can meet comes back as a value: a `result` whose error is the module's error type, or an `option.Option`. A misuse (an index out of range, a freed object) is a `req`, and a failed `req` is a trap (section 5.6).
- A function that is `unsafe` says so in its signature, and is called only in unsafe code (section 3).
- Containers are generic (section 4.H): `(ptr (vec.Vec T))` for any `T`, with no casts. Text is `str.Bytes` (a view) or `str`.
- A module with a behaviour borrowed from another system says whose: `std/csv` reads CSV as Python's `csv.reader` does, `std/stats` computes as Python's `statistics`, `std/regex` runs in time linear in the text, and so on. Where it says so, differences from that system are bugs.
- The library behaves the same on every backend and every host.

**The modules.**

| Module | What it is |
|---|---|
| `alloc` | any allocator as one value: a heap, an arena, a fixed buffer, or a program's own |
| `arena` | a region allocator that frees everything at once |
| `assertion` | what stops the program when an `assert` fails |
| `bigint` | arbitrarily large signed integers |
| `buf` | a growable byte buffer for building text |
| `cli` | command-line options and arguments |
| `cmp` | equality, order, and hashing as interfaces, for generic code |
| `csv` | comma-separated values, read and written as Python's `csv` module does |
| `date` | dates and times in UTC, written and read as ISO 8601 |
| `deflate` | DEFLATE, zlib, and gzip, both ways |
| `encode` | base64, hex, and percent-encoding, decoded strictly |
| `float` | decimal text to the nearest `f64` |
| `fmt` | numbers to text |
| `fs` | directories, file information, and permissions |
| `gen` | generations, which make a use after free detectable |
| `hash` | CRC-32, Adler-32, MD5, SHA-1, SHA-256, SHA-512, and HMAC |
| `heap` | a general-purpose allocator with a free for each object |
| `html` | HTML escaping and character references |
| `http` | HTTP/1.1 messages, and a client `fetch` |
| `io` | text out, and whole-file reads and writes |
| `ip` | IP and socket addresses read from text |
| `json` | JSON to values and back |
| `map` | a hash map from any key with a `cmp.Hash` implementation |
| `math` | `min`, `max`, `abs`, powers, roots, rounding, and the transcendental functions |
| `net` | TCP sockets |
| `option` | a value that may be missing |
| `os` | the command line and the environment |
| `par` | parallel maps and reductions without `unsafe` (experimental, with threads) |
| `path` | file paths as text |
| `proc` | starting other programs, and talking to them |
| `random` | seeded random numbers (PCG-32) |
| `regex` | regular expressions that run in time linear in the text |
| `serve` | an HTTP/1.1 server |
| `set` | a hash set |
| `show` | values as text, for the formatting forms |
| `stats` | statistics over `(arr f64)`, to the last bit as Python's `statistics` gives them |
| `str` | byte slices and byte-level text |
| `stream` | readers and writers over files, standard streams, memory, and buffers |
| `tar` | tar archives read from memory: ustar, GNU, and pax, with checks for damaged ones |
| `testing` | what a test's `expect` calls |
| `text` | splitting, joining, and the other text functions that build something |
| `thread` | threads with a typed context (experimental) |
| `time` | timing code with the monotonic clock |
| `url` | URLs: parse, put back together, resolve |
| `utf8` | text one character at a time |
| `uuid` | version 4 UUIDs |
| `vec` | a growable list |

The reference has, for each module, what it is for and every public item. `aipl doc NAME` and `aipl doc --find WORD` print the same text at a prompt.
---

## 11. Imports and Multi-Module Programs

```lisp
;; util.aipl
(module util
  (fn double [x:i32] -> i32 (pub) (* x 2)))

;; main.aipl
(module app
  (import util)
  (fn main [] -> i32 (println "{(call util.double 21)}") 0))      ;; prints 42
```

- `(import name)` finds `name.aipl` by searching, in order: the importing file's directory, the entry file's directory, the standard library (`$AIPL_STD` when set, else the library's `std/`, below), then each directory listed in the colon-separated `AIPL_PATH` environment variable. The first match wins, so a local `io.aipl` shadows the standard one.
- **Every module a file names, it imports.** A call, a type, a field path, a `ref`, a constant, or an interface written with a module's prefix (`str.Bytes`, `(call vec.push ...)`, `(get b str.Bytes.len)`) needs that module's `(import ...)` in the same file, even when an imported module imports it too; otherwise `'str.Bytes' is in module str, which this file does not import: add (import str)`, from both resolvers. Using a module's functions that use another module needs nothing: that is the other module's import. What the compiler writes for a form (`new` and `arr.new` call `alloc.create` and `alloc.array`, the formatting forms std/show, a test build's `_start` std/testing and std/os) is not the file's to import. The resolver parses it and merges its functions into the entry module renamed as `name.fn`. `(import name as u)` lets you write `(call u.double ...)` locally; it is rewritten to `util.double` before checking.
- An import may name a file in a subdirectory: `(import native/wasm_reader)` finds `native/wasm_reader.aipl` in the same places, relative to each. The module is named by the last segment (`wasm_reader.decode`), so a sibling inside `native/` that writes `(import wasm_reader)` reaches the same module. Two different files with the same name in one program are an error (`two different modules are named 'x'`). An import path is names separated by `/`: no leading `/`, no `.` or `..`.
- Resolution turns the imported modules and the entry module into one program: every struct, then every function, imported modules first in resolution order. A cycle is an error (`PATH: Circular import detected: ...`), and so is a module that cannot be found (`Cannot resolve import 'NAME': no 'NAME.aipl' found in ...`, listing the places searched). `AIPL_PATH` is read by the resolver itself.
- Import depth is flattened to one level: a function from a module imported by an import is still `directimport.fn`, not `a.b.fn`. Diamond imports produce one copy. Cycles are an error naming the file.
- The entry module's own functions keep bare names. In a compiled `.wasm`, exports are `main` and `util.double`.
- Structs follow the same rule: `(struct Node ...)` in `util` is `util.Node` everywhere outside `util` (`(ptr util.Node)`, `(new util.Node a [...])`, `(get p util.Node.val)`), aliases included, so two imports may each define `Node`. An importer's bare `Node` never reaches into an import.
- `(ref f)` is rewritten like `(call f ...)`, so a function reference inside an imported module points at the qualified function, including the worker of a `thread.spawn`.
- Constants and enums follow the struct rule: `util.LIMIT`, `util.Color`, `util.Color.red` outside `util` (and `u.Color.red` through an alias). The head of a form is never renamed, so `(import util as mem)` leaves the op `(mem.alloc n)` alone.

### 11.1 Visibility

Everything a module defines is private to it unless marked `(pub)`: another module may use only what is marked, so a module's interface is exactly what it marks, and its helpers and the fields that hold its invariants cannot be reached from outside.

```lisp
(module counter
  (import alloc)
  ;; others can read n, write nothing, and make a Counter only through start
  (struct Counter [(read n:i32) step:i32] (pub))
  (fn start [step:i32] -> (ptr Counter) (pub)
    (new Counter (call alloc.forever) [n:0 step:step]))
  (fn tick [c:(ptr Counter)] -> void (pub)
    (put c Counter.n (+ (get c Counter.n) (get c Counter.step))))
  (fn helper [] -> i32 0))     ;; private: (call counter.helper) elsewhere is an error
```

| Mark | Where | Meaning outside the module |
|---|---|---|
| `(pub)` | right after the signature: `(fn f [params] -> type (pub) ...)`, `(struct S [fields] (pub))`, `(struct S [fields] (value) (pub))`, `(union U [variants] (pub))`, `(enum E [members] (pub))`, `(const NAME:type value (pub))`, `(interface (Name T) (pub) ...)`; generic ones the same | the item may be named: called, referred to, used as a type, its members and variants used |
| a field `x:T` | in a struct's field list | private: neither `get` nor `put` |
| `(read x:T)` | | `get` only |
| `(pub x:T)` | | `get` and `put` |

Rules (both resolvers, with the same messages):

- **Using a private item** is an error where it is used: `'util.helper' is private to module util: mark it (pub) there to use it here`. Fields: `field 'step' of counter.Counter is private to module counter`, `field 'n' of counter.Counter is read-only outside module counter`.
- **Making a struct** outside its module, with `(new S a [...])` or by passing `S` itself as a type argument (which lets a template such as `alloc.create` make one), needs every field `(pub)`: `'counter.Counter' has fields private to module counter, so only counter can make one`. Code that may fill a struct only through its module's functions cannot get one any other way. A value struct (section 4.E) is the same with `(make V ...)`, and `with` counts as writing the field it changes; but it may be a type argument whatever its fields, since a template cannot make one.
- **A (pub) item shows no private type** of its module in what it gives others (a function's parameters and result, a struct's readable fields, a union's variants, an interface's signatures): `'start' is (pub) but its signature uses 'Counter', which is private: mark 'Counter' (pub) too`.
- **Implementations** take no `(pub)`: an implementation is as visible as its interface and its type (section 4.H).
- The entry module's marks change nothing (nothing imports it), and a module parsed on its own (`Parser::parse`) ignores them. The resolver checks visibility and leaves the marks out of the flat program, so the checker and the code generators never see them.

This closes what the unsafe fence left open (section 3): the standard library's descriptors keep their parts in read-only fields (`str.Bytes`'s array, start, and length), so code outside `std/str` can read a byte string's parts but not forge them, and cannot make one except through `str.from_str`, `str.slice`, `str.of_array`, and the other functions there (`str.bytes`, which takes a raw address, is unsafe to call).

---

## 12. Worked Examples

Each example is complete and runs with the command shown. Expected output is what the current toolchain prints.

### 12.1 Pure computation, both backends

```lisp
;; gcd.aipl
(module gcd_demo
  (fn gcd [a:i32 b:i32] -> i32
    (req (and (gt a 0) (gt b 0)))
    (ens (gt res 0))
    (let x:i32 a)
    (let y:i32 b)
    (while (neq y 0)
      (let t:i32 y)
      (set! y (% x y))
      (set! x t))
    x)

  (fn main [] -> i32 (println "{(call gcd 1071 462)}") 0))
```
```
$ aipl eval gcd.aipl
21
$ aipl compile gcd.aipl -o gcd.wasm
```
The `.wasm` exports `gcd`, `main`, `memory`, and `_start`, which runs `main` and exits with its status (section 6.1). Note the `let t` inside the `while` body: it becomes a wasm local of the function and is re-assigned each iteration, which is the intended semantics.

### 12.2 Memory as a data structure, both backends

Raw memory, laid out by hand. Every function takes a raw address, which nothing checks, so each is `(unsafe)` to call (section 3); a program would use a struct or `std/vec` instead.

```lisp
(module stack_demo
  ;; stack layout: [count:i32][slot0:i32][slot1:i32]...
  (fn stack_new [cap:i32] -> i32 (unsafe)
    (let s:i32 (mem.alloc (+ 4 (* cap 4))))
    (mem.store32 s 0)
    s)
  (fn stack_push [s:i32 v:i32] -> void (unsafe)
    (let n:i32 (mem.load32 s))
    (mem.store32 (+ s (+ 4 (* n 4))) v)
    (mem.store32 s (+ n 1)))
  (fn stack_pop [s:i32] -> i32 (unsafe)
    (req (gt (mem.load32 s) 0))
    (let n:i32 (- (mem.load32 s) 1))
    (mem.store32 s n)
    (mem.load32 (+ s (+ 4 (* n 4)))))
  (fn main [] -> i32 (unsafe)
    (let s:i32 (call stack_new 4))
    (call stack_push s 10)
    (call stack_push s 32)
    (let a:i32 (call stack_pop s))
    (let b:i32 (call stack_pop s))
    (println "{(+ a b)}")     ;; prints 42
    0))
```

### 12.3 Printing and strings, both backends

```lisp
(module hello
  (fn main [] -> i32
    (sys.print "hello, aipl")
    0))
```
`aipl eval hello.aipl` prints `hello, aipl` and exits with status 0. `aipl compile hello.aipl` produces a module importing `wasi_snapshot_preview1::fd_write`; run it with `aipl run hello.wasm`, or any WASI host, and it prints the same line. `(+ "a" "b")` is a checker error: text is built with the formatting forms (section 4.K), `std/buf`, or `str.concat` (which makes a `str` that is never freed).

### 12.4 Threads and atomics, both backends

*Experimental (section 4.D): not part of the language version 1 promise.*

The worker is a function reference of type `(fn [i32] -> i32)`; the single `i32` argument is the natural way to hand it a pointer. Threads and atomics are unchecked operations (AIPL does not check for data races), so they need an unsafe context (section 3).

```lisp
(module counter_demo
  (fn worker [counter:i32] -> i32 (unsafe)
    (for i 1 1001
      (atomic.add counter 1))
    0)

  (fn main [] -> i32 (unsafe)
    (let counter:i32 (mem.alloc 4))
    (mem.store32 counter 0)
    (let t1:i32 (thread.spawn (ref worker) counter))
    (let t2:i32 (thread.spawn (ref worker) counter))
    (let _a:i32 (thread.join t1))
    (let _b:i32 (thread.join t2))
    (println "{(mem.load32 counter)}")   ;; prints 2000, deterministically
    0))
```
`atomic.add` returns the previous value; the call above is in statement position so the value is dropped. Threads share linear memory (and therefore the allocator) and the function table, but not locals. Compiled, this is a threaded module (section 4.D): it runs under AIPL's runner, not the stock `wasmtime` CLI.

### 12.5 File round-trip, both backends

Paths are `(ptr, len)` pairs into linear memory, matching the WASI convention, so the `fs.*` operations that take them are unchecked and need an unsafe context (section 3); `std/io` (`io.read_file`, `io.write_file`) is the checked way to use files. `fs.open` flags: `0` read-only, non-zero write/create/truncate. All `fs.*` return minus a WASI errno on failure rather than raising (section 4.C); `std/io` gives the reason as an `io.Error`. In the VM the path is relative to the process cwd; under WASI it is relative to the preopened directory (fd 3), which `aipl run` and executables built with `aipl compile --exe` set to the working directory.

```lisp
(module file_demo
  (fn main [] -> i32 (unsafe)
    (let path:i32 (mem.alloc 8))
    (let out:i32 (mem.alloc 4))
    (let in:i32 (mem.alloc 4))
    ;; "t.bin"
    (mem.store8 (+ path 0) 116) (mem.store8 (+ path 1) 46) (mem.store8 (+ path 2) 98)
    (mem.store8 (+ path 3) 105) (mem.store8 (+ path 4) 110)
    (mem.store32 out 3735928559)              ;; 0xDEADBEEF, wraps to -559038737
    (let w:i32 (fs.open path 5 1))
    (let _n:i32 (fs.write w out 4))
    (let _c:i32 (fs.close w))
    (let r:i32 (fs.open path 5 0))
    (let _m:i32 (fs.read r in 4))
    (let _d:i32 (fs.close r))
    (let _x:i32 (fs.delete path 5))
    (println "read back: {(eq (mem.load32 in) (mem.load32 out))}")   ;; prints read back: true
    0))
```

### 12.6 A complete I/O program with the standard library, both backends

[examples/word_count.aipl](examples/word_count.aipl) reads `input.txt`, prints its line, word, and byte counts, writes an error to stderr if the file cannot be read, and returns the line count. Twelve code lines, against 49 before the library existed:

```lisp
(module word_count
  (import io)
  (import str)
  (fn main [] -> (result i32 str)
    (match_result (call io.read_file "input.txt")
      (ok b
        (println "lines: {(call str.count_lines b)}")
        (println "words: {(call str.count_words b)}")
        (println "bytes: {(get b str.Bytes.len)}")
        (ok:str 0))
      (err e (err:i32 (format "word_count: input.txt: {(call io.message e)}"))))))
```
```
$ cd examples && ../target/debug/aipl eval word_count.aipl
lines: 4
words: 15
bytes: 81
$ aipl compile --exe examples/word_count.aipl -o word_count
$ cd examples && ../word_count
lines: 4
words: 15
bytes: 81
```
The repository's tests run this program in both backends against a generated input and against the shipped `examples/input.txt`, and check the exact standard output and exit status.

## 13. Pitfalls for Code Generators

Each of these is a real failure mode observed when LLMs write AIPL. The fix is in the second column. The compiler's message for most of them names the fix too (CANONICAL_PLAN.md step 2), and a name close to a real one (an operator, a function, a module's item) gets a "did you mean".

| Mistake | Correct form |
|---|---|
| `(mem.load32 p)`, `(ptr.cast S a)`, or `(thread.spawn ...)` in an ordinary function | wrap it in `(unsafe ...)`, or mark the function `(unsafe)` if wrong arguments could break memory (section 3); usually `new`, arrays, or a std function avoid it |
| `(f x)` to call a user function | `(call f x)`; bare `(name ...)` is only for built-in ops |
| `(if c (set! x 1))` with no else | `if` always takes three arguments; for one branch write `(when c (set! x 1))`; both branches must be void or both the same value type |
| `(if c (set! x 1) (block))` or `(if c (block) (set! x 1))` | an empty `(block)` branch is an error naming `when`: `(when c (set! x 1))`, `(when (not c) (set! x 1))` |
| `(call fs.remove path)` or any `(call f ...)` returning a result or an option, as a statement | the value would be thrown away and a failure go unnoticed: it is an error; use it with `match_result`, `match`, `try`, or `try_opt`, or write `(drop (call fs.remove path))` to ignore it on purpose |
| `(let x 5)` | `(let x:i32 5)`; the type annotation is mandatory |
| `(let x:i32 5)` inside a block when `x` is already in scope | `Cannot shadow existing variable 'x'`: rename the inner one. A name's scope is its block or arm, so a sibling block or arm may reuse the name, with any type (section 4.M) |
| `(set! y 1)` without a prior `let y` | declare first; there are no implicit globals |
| `(call str.from_str ...)` or `str.Bytes` in a file that imports only `io` or `text` | import every module the file names: add `(import str)`; a module another import brings in is not this file's (section 11) |
| `(+ n 1.0)` or `(eq n 0.0)` on an `i32` | all operands to one op share one type; write `1` or convert explicitly |
| returning `void` from an `-> i32` function (body ends in `while`/`for`/`set!` to a `void`) | end the body with a value expression, e.g. the accumulator name |
| recursion over a long list or a deep tree (100,000 nodes) | it stops with `call stack exhausted` (section 7.11); iterate with `while` and an explicit cursor or work list |
| `(loop i 0 n 1 ...)`, a counted loop with an inclusive end | `loop` is retired and refused (`loop is retired: (for i start end body...) counts from start up to end, end left out; for another step, use while`): `(for i 0 n ...)` runs n times, its end left out (section 4.M). `and`/`or` short-circuit, so `(and (lt i n) (eq (arr.get i32 a i) x))` is a safe guard |
| `(fn (i32) -> i32)` or `(fn [i32] i32)` | the function type is `(fn [i32] -> i32)`: brackets around the parameters, then `->` |
| `(call_ref f x)` or `(call f x)` where `f` is a reference | `(call_ref (fn [i32] -> i32) f x)`: the signature is part of the call |
| `(thread.spawn name len arg)` with a name in memory | `(thread.spawn (ref worker) arg)`, `worker` of type `(fn [i32] -> i32)` |
| `(let p:i32 (new Point a [...]))`, or an `i32` parameter that holds a pointer | use `(ptr Point)`; arrays are `(arr T)`. `new` and `arr.new` never return `i32` |
| `(new Point)` or `(arr.new i32 n)` | name the allocator: `(new Point a [x:1 y:2])` with the `alloc.Allocator` the function was handed, `(call alloc.default)`, or `(call alloc.forever)` for memory kept until exit; `(arr.new i32 a n)` |
| `(new Point a)` with no fields | a struct names every field, in order: `(new Point a [x:0 y:0])`; a field that starts empty is `(zero T)`: `(zero i32)`, `(zero (ptr Node))`, `(zero (arr i32))` (section 4.E; a union has no zero: write the variant you mean) |
| `(make Vec2 1.0 2.0)` for a value struct | a struct's fields are named: `(make Vec2 [x:1.0 y:2.0])`; only a union variant's payload is positional: `(make Shape.rect 2.0 3.0)` (sections 4.E and 4.J) |
| `(= a b)`, `(== a b)`, `(!= a b)`, `(&& a b)`, `(\|\| a b)`, `(! a)` | C's operators are named forms: `(eq a b)`, `(neq a b)`, `(and a b)`, `(or a b)`, `(not a)`; the error says which one you meant |
| `(ptr i32)` for a sequence of numbers | `(arr i32)`, made by `(arr.new i32 a n)`, or `(arr.of i32 a 1 2 3)` |
| `(eq p 0)` to test for null | `(eq p (ptr.null Point))` |
| `(+ p 4)` or `(mem.load32 p)` on a pointer | read fields with `get`; for raw memory, `(ptr.addr p)` first |
| a struct from an imported module written bare | qualify it: `(ptr geo.Point)`, `(get p geo.Point.x)` |
| `(mem.store32 512 x)`, `(atomic.lock 0)`, any literal address below 1024 | rejected by the checker: address 0 is the heap cursor and 64-1023 is reserved. Take memory from `(mem.alloc n)` and pass the pointer. A computed address that lands on a non-lock word fails at runtime in the VM instead of hanging |
| `(let x:i64 5)` or `(+ n 1i64)` where `n` is `i32` | no implicit widening: write `5i64`, or convert with `(i64.extend_s n)`; narrow back with `(i32.wrap x)` |
| `(% a b)` with negative `a` expecting a positive result | `%` is `rem_s`; add `b` and take `%` again for a modulo |
| `(+ str str)` | there is no string `+`; build strings with `std/buf` |
| a program using `thread.spawn` run with the stock `wasmtime` CLI | threaded modules need AIPL's runner (`aipl run`, or `aipl compile --exe`) or another wasi-threads host |
| `(get p x)` or `(get p Point x)` | the field is one symbol: `(get p Point.x)`; arrays name the element type every time: `(arr.get i32 a i)` |
| catching a bad index with `arr.get` and carrying on | a bad index stops the program in every backend; check `(lt i (arr.len a))` first where an index may be out of range |
| `(make Option.some 5)` or `(make (Option i32).some 5)` for a generic union | the type with its arguments, then the variant: `(make (option.Option i32) some 5)`, and the arm `((option.Option i32) some [v] ...)` (section 4.J) |
| ending a function in `(let ...)` | `let` is void; end with the value, e.g. the variable name |
| `(sys.print n)` with an `i32` | `sys.print` prints `str` only (a checker error); use the format forms, `(print "{n}")` or `(println "label {n}")` (section 4.K) |
| hand-writing digit formatting, file-reading loops, byte counting, growable arrays, hash tables, or string building | the standard library (section 10): `io`, `str`, `fmt`, `vec`, `map`, `buf`, `os` |
| `(eq a b)` or `(lt a b)` on a type parameter `T` | ask for it: `(where (cmp.Eq T))` and `(call (cmp.Eq.same T) a b)`, or `(cmp.Ord T)` and `cmp.Ord.compare` (section 4.H) |
| `(vec.Vec Point)` with `Point` a struct | a struct is a value only through a pointer: `(vec.Vec (ptr Point))` |
| `(mem.free p)`, or reusing `mem.alloc` memory by hand | there is no `mem.free`; allocate what will be freed from `std/heap` (`(call (heap.create T) h)`, `(call (heap.destroy T) h p)`) or an `std/arena`, or through `std/alloc` |
| `(call (vec.make i32) 16)` | collections take an allocator first: `(call (vec.make i32) (call alloc.default) 16)`, likewise `(map.make K V)`, `buf.make` (`(import alloc)`) |
| `(call vec.push v x)` on a generic container | name the element type: `(call (vec.push i32) v x)`; the container's type is `(ptr (vec.Vec i32))` (section 4.H) |
| `(get b Box.value)` on a generic struct | name the instance: `(get b (Box i32) value)` |
| calling `mem.grow` before allocating | not needed: allocation grows memory itself (up to 32768 pages, 2 GiB) |
| passing a `str` literal where a `(ptr, len)` path or buffer is expected, e.g. `(fs.open "t.bin" 5 0)` | type error: `fs.*` take `i32` pointers. Write `(fs.open (str.ptr "t.bin") (str.len "t.bin") 0)` |
| `(cond ((lt n 0) -1) ((eq n 0) 0))` without `else` | `cond` needs a final `(else ...)` clause; use `(else (block))` when the clauses are statements |
| `else if`, `elif`, `switch`, `case` | do not exist; use `cond` |
| a zero-argument function or a bare number standing for a fixed value | `(const PAGE_SIZE:i32 65536)`; for a set of related codes, `(enum Kind [a b c])` (section 4.I) |
| `(+ k 1)`, `(lt k Kind.b)`, or passing `Kind.b` where an `i32` is expected | enums are not numbers: compare with `eq`/`neq`; convert with `(enum.ord k)` and `(enum.cast Kind n)` |
| `(const max:i32 5)` or `(let MAX:i32 1)` after `(const MAX ...)` | constants are named in capitals and cannot double as variables |
| `p:Point` for a struct | structs are always behind a pointer: `p:(ptr Point)`; a bare type name must be a scalar, an enum, or a union |
| a struct with a `kind` field and fields that mean different things per kind | a union: `(union Shape [(circle r:f64) (rect w:f64 h:f64)])`, built with `make`, taken apart with `match` (section 4.J) |
| `(match s (circle [r] ...))`, `(match s (Shape.circle r ...))` | arms name `Union.variant` in full, and binders go in brackets: `(Shape.circle [r] ...)` |
| a `match` arm per variant plus an `else` "just in case" | the checker rejects an `else` that can never run; leave it out when every variant has an arm |
| `(eq s1 s2)` or `(ptr.null Shape)` on a union | union values do not compare and are never null: use `match`; for "maybe a value", add a variant such as `(none)` |
| `(lt size limit)` on sizes or hashes that may pass `2^31` | `(ltu size limit)`: compares as unsigned |
| `(+ balance amount)` where wrapping would be a silent wrong answer | `(checked.add balance amount)`: stops the program on overflow |
| `(break)` in a `while` condition or outside any loop | only inside a `while` or `for` body |
| a `done`/`found` flag variable to stop a loop | `(break)`, or `(return v)` from the function |
| an extra `)` after the closing `(module` paren | reported as `L:C: unexpected input after the module's closing ')'` |
| `inf`, `nan`, `1e9` as literals | not literals; `1e9` is a symbol and will be reported as an undefined variable |
| `(req n > 0)` | contracts are S-expressions: `(req (gt n 0))` |
| `(ens (gt result 0))` | the result is named `res`, always |
| declaring a test "done" because it returned a number | assert the value; see section 10 |
