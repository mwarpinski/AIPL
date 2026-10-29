# AIPL Specification
## AI Programming Language: Formal Specification

> **WebAssembly semantics are the specification.** Where this document is silent, a program means what its compiled wasm does. `i32` and `i64` are wrapping two's-complement, shifts mask their count, and traps are wasm's traps. The VM and the native backend must match wasmtime exactly, and any divergence is a bug in them: `tests/test_differential.rs` checks the VM, `tests/test_native.rs` the native executables.

AIPL (AI Programming Language) is an unambiguous, statically typed S-expression systems language with runtime-checked contracts, designed for AI agents to generate. It compiles to WebAssembly (with WASI for I/O), and on Linux x86-64 to native executables translated from that wasm (section 6.6). It also runs in a reference VM, and has a self-hosted compiler written in AIPL itself (section 6.4), which compiles itself to the same bytes; building it the first time, and writing executables (section 6.5), still take the Rust toolchain.

What AIPL optimises for is that a program has one obvious spelling and that mistakes are caught early with a `line:col` diagnostic, not brevity. With the standard library (section 12.6), a small I/O program is about twice the length of the Python equivalent (section 12.7), which is roughly the floor for a fully parenthesised, fully annotated syntax.

---

## 1. Syntax: One Canonical Text Form

AIPL source is `.aipl` text: a context-free, parenthesis-delimited S-expression syntax with no operator precedence, no indentation rules, and no statement separators. It is also the interchange format between agents. `src/printer.rs` prints any resolved program back as canonical flat text, which both compilers accept. (A MessagePack "binary AST" (`.baipl`) existed until P12; it was a dump of the compiler's internal Rust data structures with no stable format, nothing used it, and it was removed.)

---

## 2. Updated Formal Grammar (EBNF)

```ebnf
program        ::= "(" "module" identifier import* ( struct_def | enum_def | union_def | const_def | fn_def | interface_def | impl_def )* ")" ;
import         ::= "(" "import" import_path [ "as" identifier ] ")" ;
import_path    ::= identifier ( "/" identifier )* ;
struct_def     ::= "(" "struct" ( identifier | generic_head ) "[" field* "]" [ "(" "value" ")" ] where* ")" ;   (* (value): a value struct (section 4.E) *)
field          ::= identifier ":" type ;
generic_head   ::= "(" identifier type_param+ ")" ;
type_param     ::= identifier ;                     (* starts with an uppercase letter *)

enum_def       ::= "(" "enum" identifier "[" ( identifier | "(" identifier integer ")" )+ "]" ")" ;
const_def      ::= "(" "const" identifier ":" type const_value ")" ;   (* NAME in capitals; i32 i64 f64 bool str *)
const_value    ::= literal | identifier | "(" identifier const_value+ ")" ;   (* a constant expression (section 4.I) *)
union_def      ::= "(" "union" ( identifier | generic_head ) "[" ( "(" identifier field* ")" )+ "]" where* ")" ;

fn_def         ::= "(" "fn" ( identifier | generic_head ) "[" param* "]" "->" type where* contract* expr* ")" ;
where          ::= "(" "where" "(" ( identifier | "struct" | "plain" ) type_param ")" ")" ;   (* generics only (section 4.H) *)
interface_def  ::= "(" "interface" "(" identifier type_param ")" fn_sig+ ")" ;
fn_sig         ::= "(" "fn" identifier "[" param* "]" "->" type contract* ")" ;   (* no body *)
impl_def       ::= "(" "impl" "(" identifier type ")" fn_def+ ")" ;
param          ::= identifier ":" type ;
contract       ::= "(" ("req" | "ens" | "inv") expr ")"
                 | "(" "unsafe" ")"                  (* the function is unsafe to call (section 3) *)
                 | "(" "no_leak" ")" ;               (* the function makes no memory that is never freed (section 3) *)

type           ::= "i32" | "i64" | "f32" | "f64" | "bool" | "str" | "void"
                 | "u8" | "i8" | "u16" | "i16"         (* storage types: a struct field or an array element only (section 4.A) *)
                 | "(" "result" type type ")"
                 | "(" "ptr" struct_name ")"
                 | "(" "arr" type ")"
                 | "(" "fn" "[" type* "]" "->" type ( "(" "no_leak" ")" )? ")"   (* with (no_leak): only functions that keep no memory (section 4.G) *)
                 | "(" struct_name type+ ")"          (* a generic struct instance: behind ptr, or a value struct's *)
                 | type_param                         (* inside a generic definition *)
                 | identifier ;                       (* an enum, a union, or a value struct, e.g. Color, Shape, Vec3 *)
struct_name    ::= identifier                       (* "m.S" for a struct from imported module m *)
struct_ref     ::= struct_name | "(" struct_name type+ ")" ;
field_ref      ::= struct_name "." identifier ;
fn_name        ::= identifier | "(" identifier type+ ")" ;   (* the second form instantiates a generic, or names an interface's function: (Hash.hash i32) *)

expr           ::= literal
                 | identifier
                 | "(" "let" identifier ":" type expr ")"
                 | "(" "set!" identifier expr ")"
                 | "(" "if" expr expr expr ")"
                 | "(" "for" identifier expr expr expr* ")"
                 | "(" "while" expr expr* ")"
                 | "(" "call" fn_name expr* ")"
                 | "(" "block" expr* ")"
                 | "(" "unsafe" expr+ ")"            (* a block allowing unchecked operations (section 3) *)
                 | "(" "return" [ expr ] ")" | "(" "break" ")" | "(" "continue" ")"
                 | "(" "cond" ( "(" expr expr+ ")" )+ "(" "else" expr+ ")" ")"
                 | "(" ("ok" | "err") ":" type expr ")"   (* the result's other side: (ok:E v), (err:T e) *)
                 | "(" "match_result" expr "(" "ok" identifier expr* ")" "(" "err" identifier expr* ")" ")"
                 | "(" "new" struct_ref expr ")"        (* the allocator to make it from (section 4.E) *)
                 | "(" "get" expr struct_name "." identifier ")"
                 | "(" "get" expr field_ref field_ref+ ")"   (* a path into value structs held inline (section 4.E) *)
                 | "(" "get" expr "(" struct_name type+ ")" identifier ")"
                 | "(" "put" expr struct_name "." identifier expr ")"
                 | "(" "put" expr field_ref field_ref+ expr ")"   (* one field of a value struct held in the field *)
                 | "(" "put" expr "(" struct_name type+ ")" identifier expr ")"
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
                 | "(" "make" identifier expr* ")"     (* Union.variant, or a value struct *)
                 | "(" "with" expr field_ref expr ")"  (* a copy of a value struct with one field changed *)
                 | "(" "make" "(" identifier type+ ")" identifier expr* ")"   (* a generic union's variant (section 4.J) *)
                 | "(" "match" expr match_arm+ ")"
                 | "(" op expr* ")" ;

match_arm      ::= "(" identifier [ "[" identifier* "]" ] expr* ")"   (* Name.member; binders for a union variant *)
                 | "(" "(" identifier type+ ")" identifier [ "[" identifier* "]" ] expr* ")"   (* a generic union's variant *)
                 | "(" "else" expr* ")" ;                             (* last *)

op             ::= arithmetic_op | bitwise_op | memory_op | atomic_op | comp_op
                 | conv_op | sys_op | fs_op | net_op | child_op | proc_op | thread_op | str_op ;

arithmetic_op  ::= "+" | "-" | "*" | "/" | "%" | "divu" | "remu" | "checked.add" | "checked.sub" | "checked.mul" ;
bitwise_op     ::= "^" | "shl" | "shr" | "shru" | "bitand" | "bitor" ;
memory_op      ::= "mem.load8" | "mem.load8_s" | "mem.load16_u" | "mem.load16_s" | "mem.load32" | "mem.load64"
                 | "mem.store8" | "mem.store16" | "mem.store32" | "mem.store64"
                 | "mem.alloc" | "mem.grow" ;
atomic_op      ::= "atomic.add" | "atomic.cas" | "atomic.lock" | "atomic.unlock" ;
comp_op        ::= "eq" | "neq" | "lt" | "lte" | "gt" | "gte" | "ltu" | "lteu" | "gtu" | "gteu" | "and" | "or" | "not" ;
conv_op        ::= "i64.extend_s" | "i64.extend_u" | "i32.wrap"
                 | "f64.convert_i64_s" | "i64.trunc_f64_s" | "f64.reinterpret_i64" | "i64.reinterpret_f64" | "f64.sqrt"
                 | "f32.demote" | "f64.promote" | "f32.convert_i32_s" | "f32.convert_i64_s" | "f64.convert_i32_s"
                 | "i32.trunc_f32_s" | "i32.trunc_f64_s" | "i64.trunc_f32_s" | "f32.reinterpret_i32" | "i32.reinterpret_f32" ;
sys_op         ::= "sys.print" | "sys.time" | "sys.monotonic" | "sys.random" | "sys.exit" ;
fs_op          ::= "fs.open" | "fs.read" | "fs.write" | "fs.close" | "fs.delete"
                 | "fs.opendir" | "fs.readdir" | "fs.stat" | "fs.chmod" | "fs.rename" | "fs.rmdir" | "fs.mkdir" ;
net_op         ::= "net.listen" | "net.accept" | "net.recv" | "net.send" | "net.close" ;   (* std/net is the safe interface *)
child_op       ::= "proc.spawn" | "proc.stream" | "proc.read" | "proc.write" | "proc.close" | "proc.wait" | "proc.exec" ;   (* unsafe; granted by --allow-run *)
proc_op        ::= "args.sizes" | "args.get" | "env.sizes" | "env.get" ;
thread_op      ::= "thread.spawn" | "thread.join" ;
str_op         ::= "str.len" | "str.ptr" ;
```

Notes on the grammar as implemented by `src/parser.rs`:
- A module body is imports followed by struct, enum, union, constant, and function definitions, in any order. `let` and `set!` are expressions that appear inside function bodies, not at module level.
- Comments start with `;;` and run to end of line.
- Integer literals are decimal, optionally signed (`-1` and `+1` are one token) and are `i32`; the value must fit in 32 bits, read as signed or unsigned (`-2147483648` to `4294967295`, so `4294967295` is the bit pattern of `-1`); anything larger is an error suggesting the `i64` form. An `i64` literal carries the suffix as part of the token: `42i64`, `-7i64`. Float literals must contain a `.` and at least one digit (`1.0`, `.5`, `1.5e3`; not `1` or `inf`) and are `f64`; with the suffix `f32` (`1.5f32`, `-0.1f32`, `1.0e10f32`) they are `f32`, the value the nearest `f64` to the text, then the nearest `f32` to that.
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

### Memory that is never freed, and `(no_leak)`

What `alloc.forever` hands out, `mem.alloc`, and the `str` values made at run time (`format`, `str.concat`, `buf.to_str`) are never freed: right for a program that runs and exits, fatal for one that runs for days. A function with the clause `(no_leak)` promises to make no such memory, and the checker holds it to that: it may not call `alloc.forever` (so `(new S (call alloc.forever) [...])` is refused, and `(new S a [...])` with an allocator it is handed is not), use `mem.alloc` or `format`, call through a function reference whose type does not promise `(no_leak)` (section 4.G), or call a function that may do any of these, directly or through what it calls. The error names the first site that breaks the promise and the chain down to the allocation:

```
13:5: uses_concat is marked (no_leak) but calls str.concat, which may make memory that is never freed (str.concat calls str.room; str.room uses mem.alloc)
```

What the function allocates through an `alloc.Allocator` it is handed is its caller's choice, so a server passes each request's handler an arena and resets it after (tests/aipl/memory/no_leak_loop.aipl runs 10,000 requests with the arena's memory flat). Inside `unsafe` the promise is the programmer's word, as every unsafe promise is: that is how std's allocators, which grow their own pools with `mem.alloc` and hand that memory out again, are marked `(no_leak)` (`alloc.raw`, `alloc.free_raw`, `alloc.default`, `heap.raw`, `arena.raw`, `arena.fast_raw`). A function not marked is not checked, only described. Results and unions are values (sections 4.F, 4.J), so building one allocates nothing and there is no hidden allocation to count. Both checkers (`src/leaks.rs`, `aipl_src/leaks.aipl`) give the same errors.

### Unchecked operations and `unsafe`

Some operations reach memory the program has not been handed through a typed pointer or array, or let threads race, and nothing checks them. They are allowed only in an **unsafe context**:

- **Raw memory:** `mem.load8`, `mem.load32`, `mem.load64`, `mem.store8`, `mem.store32`, `mem.store64`.
- **Casts to a pointer or array:** `ptr.cast`, `arr.cast`, `ptr.make`, `arr.make` (`enum.cast` is checked: a non-member reaches `match`'s `unreachable`).
- **Atomics and threads:** `atomic.add`, `atomic.cas`, `atomic.lock`, `atomic.unlock`, `thread.spawn`, `thread.join` (AIPL does not check for data races).
- **Host calls that take raw addresses or handles:** `fs.open`, `fs.read`, `fs.write`, `fs.delete`, `fs.chmod`, `fs.rename`, `fs.rmdir`, `fs.mkdir`, `fs.opendir`, `fs.readdir`, `fs.stat`, `args.sizes`, `args.get`, `env.sizes`, `env.get`, `sys.random`, the sockets `net.listen`, `net.accept`, `net.recv`, `net.send`, `net.close` (std/net wraps them), and the process operations `proc.spawn`, `proc.stream`, `proc.read`, `proc.write`, `proc.close`, `proc.wait`, `proc.exec`.
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

**What this does and does not guarantee.** Code without `unsafe` cannot read or write memory through a raw address, cast an address into a pointer, or start a thread. Nor can it forge the standard library's descriptors: their raw addresses are in fields other modules may only read (section 11.1), and every library function that takes a raw address is unsafe to call. Nor can it use a pointer after its object is freed: every pointer carries its object's generation, and every access compares the two, so a pointer kept past `heap.destroy`, `alloc.destroy`, `free_array`, or `arena.reset` stops the program with `Use after free` (section 4.A, "Generation checks"). So code without `unsafe` is memory-safe, with these limits: threads and atomics are unsafe, and AIPL does not check for data races (`std/par` runs work in parallel without `unsafe`, and without races: what its workers share is plain or their own copy); after an arena reuses its memory, a stale pointer is caught unless the new data at its generation word happens to equal its old generation (a scrambled 32-bit number; the heap has no such case); a fast arena (`arena.Fast`) catches a stale pointer only once its memory is reused; and a pointer made by `ptr.cast` is not checked at all.

---

## 4. Systems Operations (Memory & Atomics)

### A. Raw WebAssembly Linear Memory Loads & Stores
- `(mem.load32 ptr)` -> Reads 4 bytes from linear memory offset `ptr` (`i32.load`).
- `(mem.store32 ptr val)` -> Writes 4 bytes to linear memory offset `ptr` (`i32.store`).
- `(mem.alloc size)` -> Bump allocation: returns the current heap cursor (the `i32` at address 0) and advances it by `size` rounded up to a multiple of 8, so every block is 8-aligned (the heap start is too): atomics, `i64`/`f64` values, and WASI out-parameters placed in any allocated block are aligned. The claim is one atomic add, so threads may allocate concurrently. If the new cursor is past the end of memory, memory grows by the pages needed to cover it (up to the 32768-page (2 GiB) cap, or `--max-memory`'s); when it cannot, the program stops there, at the `mem.alloc`, with `Out of memory: the limit is 2048 MiB` (the limit in MiB) and the call chain, alike in every backend. `alloc.forever` allocates the same way. Never frees. One cursor is shared by the VM, compiled wasm, and AIPL code.
- `(mem.grow pages)` -> Grows linear memory by `pages` × 64 KiB. Returns the previous size in pages, or `-1` if the 32768-page (2 GiB) maximum would be exceeded. The cap is 2 GiB so that every address is a non-negative `i32`: signed comparisons on addresses stay correct.
- There is no `mem.free`: memory from `mem.alloc` is never freed. Memory to be freed comes from an allocator in the standard library, which `new` and `arr.new` name (section 4.E): `std/heap` frees single objects (with checks for double frees and writes after free), `std/arena` frees a region at once, and `std/alloc` makes any of them one `Allocator` value, which the collections take (section 12.6).

See "Memory layout" (section 7.9) for the reserved runtime block below address 1024.

**8- and 16-bit memory.** `(mem.load8 ptr)` reads a byte zero-extended and `(mem.load8_s ptr)` sign-extended; `(mem.load16_u ptr)` / `(mem.load16_s ptr)` read 2 bytes likewise; `(mem.store8 ptr v)` / `(mem.store16 ptr v)` write the low 8 / 16 bits of an `i32`. All give or take `i32` (wasm `i32.load8_u`, `i32.load8_s`, `i32.load16_u`, `i32.load16_s`, `i32.store8`, `i32.store16`).

**Storage types.** `u8`, `i8`, `u16`, and `i16` are for data laid out tightly: a struct field (`(struct Pixel [r:u8 g:u8 b:u8 a:u8])`, 4 bytes) or an array element (`(arr u8)`, one byte each). `get` and `arr.get` give an `i32`, `put` and `arr.set` take one and keep its low bits (so `(put p Rec.tag 300)` stores 44), and `(sizeof u16)` is 2. Anywhere else (a parameter, a `let`, a result, a value struct's field) the type is an error that says so: `u8 is a storage type: only a struct field or an array element is one (reading it gives an i32); use i32`.

**`f32` conversions.** Floats convert only explicitly, as in wasm: `(f32.demote x)` rounds an `f64` to the nearest `f32`, `(f64.promote x)` is exact; `(f32.convert_i32_s n)`, `(f32.convert_i64_s n)`, and `(f64.convert_i32_s n)` round a signed integer to the nearest float; `(i32.trunc_f32_s x)`, `(i32.trunc_f64_s x)`, and `(i64.trunc_f32_s x)` drop the fraction and stop the program when `x` is NaN (`invalid conversion to integer`) or the result does not fit (`integer overflow`), at exactly wasm's bounds, natively too; `(f32.reinterpret_i32 n)` and `(i32.reinterpret_f32 x)` give the same 32 bits as the other type.

### B. Strings

A `str` is a pointer to immutable UTF-8 bytes preceded by a 4-byte little-endian length. String literals are interned once per module, in first-use order, into the data area that starts at address 1024; the heap starts at the first 8-aligned address after them (section 7.9). Literals are read-only: a store into one traps like a store into the runtime block. The literals and the heap start must fit in the first 1 MiB of memory, in both toolchains (compile error 768 in the self-hosted one, section 6.4). `(str.len s)` returns the byte length as `i32`; `(str.ptr s)` returns the address of the bytes as `i32`, which is how a string becomes the `(ptr, len)` pair that `fs.*` and other pointer-taking ops expect (identity in both backends: the VM places the literals at the same addresses as wasm when it loads a module, and copies a string onto the heap only if it is not one of the first-loaded module's literals). Two literals with the same text share one address. Strings do not compare with `eq`, `neq`, or `lt`: a string made at run time has an address of its own, so `eq` would compare addresses compiled and contents in the VM. Compare contents with `(call str.same a b)` and order with `(call (cmp.Ord.compare str) a b)`; `(eq s "x")` is the error `eq on str: compare strings by content with (call str.same a b), ...`. There is no `+` on strings (`+ does not join strings: join two with (call str.concat a b), ...`). The VM represents a `str` as a Rust string rather than a pointer; the observable semantics above are the same.

**Which kind of text to use.** AIPL has three, and converts between them only by calling a function, so each copy is visible:

| Kind | What it is | Use it for | Get one from |
|---|---|---|---|
| `str` | immutable UTF-8, never freed | literals, names, keys, finished messages; what `io.println` and `sys.print` take | a literal; `(format "...")` (section 4.K); `str.concat a b`; `str.from_bytes b` (a copy of a byte slice, which must be UTF-8); `buf.to_str b` (a copy of a builder's contents); `date.to_str d` |
| `str.Bytes` | a view of bytes, no copy: a value (three slots: a checked byte array, a start, a length), so making one allocates nothing; reading a view of memory that has been freed stops the program ("Use after free"), as a pointer does (docs/design/MEMORY_PLAN.md M1) | reading and parsing: file contents, slices, tokens | `str.from_str s` (a view of a `str`); `str.slice b start len`; `buf.bytes b`; `io.read_file` |
| `(ptr buf.Buf)` | a growable buffer that owns its bytes | building text piece by piece, in a loop | `buf.make a capacity`, then `(format_to b "...")` (section 4.K), `buf.push_str`, `push_bytes`, `push_int`, `push_i64`, `push_f64`, `push_byte` |

A `str` made at run time is never freed: it has no generation for a use to be checked against, so its memory stays for the rest of the run. Joining in a loop with `str.concat` therefore keeps every intermediate string; push onto one `buf` instead and call `buf.to_str` once at the end.

### C. Host I/O (WASI)

Compiled modules import only the host functions they use from `wasi_snapshot_preview1`, so a module without I/O has no import section.

| Op | VM | wasm lowering | Result |
|---|---|---|---|
| `(sys.print s ...)` | prints each argument on its own line | `fd_write` to fd 1 with a two-entry iovec (bytes, then `"\n"`); `str` arguments only | `void` |
| `(fs.open ptr len flags)` | `std::fs` open; `0` = read-only, else create+truncate for writing | `path_open` on the preopened directory (fd 3): `oflags = CREAT|TRUNC` and rights `FD_READ|FD_WRITE` when `flags != 0`, else rights `FD_READ` | new fd, or minus its WASI errno |
| `(fs.read fd buf max)` | | `fd_read` with one iovec | bytes read, or minus its WASI errno |
| `(fs.write fd buf len)` | | `fd_write` with one iovec | bytes written, or minus its WASI errno |
| `(fs.close fd)` | | `fd_close` | `0`, or minus its WASI errno |
| `(fs.delete ptr len)` | | `path_unlink_file` on fd 3 | `0`, or minus its WASI errno |
| `(fs.rename old_ptr old_len new_ptr new_len)` | `std::fs::rename` on the two paths as they are | `path_rename`, each path's directory (fd 3, or 4 for a path from `/`) chosen as `fs.delete` chooses it; natively each path's parent is opened beneath its directory (as `path_unlink_file` does) and `renameat` moves the last component between them. Replaces a file already at the new path (a directory only an empty one, `-55` otherwise; a directory onto a file `-54`, a file onto a directory `-31`); a link is moved itself | `0`, or minus its WASI errno. A relative path stays in the working directory (`-63` for a `..` or link leading out, on either side); under `--sandbox` there is no `/` (`-8`) |
| `(fs.rmdir ptr len)` | `std::fs::remove_dir` on the path as it is | `path_remove_directory` on fd 3 (or 4 for a path from `/`), chosen as `fs.delete` chooses it; natively the parent is opened beneath its directory (as `path_unlink_file` does) and `unlinkat` with `AT_REMOVEDIR` removes the last component | `0`, or minus its WASI errno: a missing path `-44`, a directory with anything in it `-55`, a file or a link (even to a directory) `-54`. A relative path stays in the working directory (`-63` for a `..` or link leading out); under `--sandbox` there is no `/` (`-8`) |
| `(fs.mkdir ptr len)` | `std::fs::create_dir` on the path as it is | `path_create_directory` on fd 3 (or 4 for a path from `/`), chosen as `fs.delete` chooses it; natively the parent is opened beneath its directory (as `path_unlink_file` does) and `mkdirat` makes the last component, mode 0777 less the umask | `0`, or minus its WASI errno: a path that exists (a file or a link too) `-20`, a missing parent `-44`, a file as the parent `-54`. A relative path stays in the working directory (`-63` for a `..` or link leading out); under `--sandbox` there is no `/` (`-8`) |
| `(fs.opendir ptr len)` | `std::fs::read_dir`; the names held from the start, `.` and `..` first, as wasmtime gives them | `path_open` on fd 3 with `oflags = DIRECTORY` and rights `FD_READDIR` | new fd (for `fs.readdir` and `fs.close`), or minus its WASI errno |
| `(fs.readdir fd buf len cookie:i64)` | | `fd_readdir`: WASI dirents (`d_next` u64, `d_ino` u64, `d_namlen` u32, `d_type` u8, 3 bytes, then the name) from the entry `cookie` names (`0` the first, else a `d_next`); the last one may be cut off | bytes written (`len` when more may follow), or minus its WASI errno |
| `(fs.stat ptr len out)` | `std::fs::metadata` | `path_filestat_get` on fd 3, following symbolic links: a 64-byte WASI filestat at `out` (the filetype u8 at 16: 3 a directory, 4 a regular file; the size at 32; access, modification, and change times at 40, 48, 56 in nanoseconds) | `0`, or minus its WASI errno |
| `(fs.chmod ptr len mode)` | `std::fs::set_permissions`, the path confined as below | `chmod(dirfd, ptr, len, mode)` from AIPL's own import module `aipl_sys_v1` (docs/design/SYS_PLAN.md), provided by aipl-run, the directory (fd 3 or 4) chosen as `fs.delete` chooses it; natively the file is opened beneath that directory as `fs.open` opens one, read-only, then `fchmod` and `close` (a file that cannot be read: `fchmodat` on its parent, unless it is a symbolic link) | `0`, or minus a WASI errno. `mode` is the permission bits, 0 to 0o777; a bit outside them (setuid, setgid, sticky) is `-28`. A relative path stays in the working directory (a `..` or a symbolic link leading out is `-63`); under `--sandbox` there is no `/` (`-8`). The VM's paths are ordinary: nothing confines them. A path whose length runs past memory is `-21`, a value and not a trap |
| `(proc.spawn path len args args_len stdio)` | `std::process::Command` on the path exactly as given (no `PATH` search), if `--allow-run` names it (`*` for any); the arguments are `args_len` bytes at `args`, each followed by a zero byte (a last word without one counts), the first the program's own name; `stdio` is three 2-bit fields, standard input in bits 0-1, output in 2-3, error in 4-5: 0 the program's own, 1 a pipe, 2 the null device, and 3 for the error only, the output's | `spawn` from `aipl_sys_v1` (docs/design/SYS_PLAN.md D3), provided by aipl-run; natively `pipe2`, `fork`, `dup2`, `execve` (a pipe that closes on exec tells the parent why it failed) | a process handle, or minus an errno: `-76` (not granted), `-44` and the other errnos of a program that cannot start, `-28` for a `stdio` outside those values, `-21` for a path or arguments outside memory, `-1` for more than 65,536 bytes of both. The environment and working directory are the program's. Handles are not descriptors, and what a number is differs between backends: print none |
| `(proc.stream p which)` | the pipes `spawn` made | `aipl_sys_v1.stream` | the pipe of process `p`'s input (0), output (1), or error (2); `-1` if that stream is not a pipe, `-8` if `p` is not a process this program started |
| `(proc.read h buf max)`, `(proc.write h buf len)` | `Read` and `Write` on the pipe | `aipl_sys_v1.read`, `.write`; natively `read`, `write` (SIGPIPE ignored for the call) | bytes moved: a read waits for some and gives `0` at the end; `-8` for a handle that is not a pipe this program has open, or one of the wrong way round (the input is written, the output and error read), `-21` for a buffer outside memory, `-64` for a write to a program that has gone |
| `(proc.close h)` | drops the pipe, or the process (which goes on running) | `aipl_sys_v1.close`; natively `close`, or the slot freed | `0`, or `-8`. Closing the input is how a program is told its input has ended |
| `(proc.wait p)` | `Child::wait` | `aipl_sys_v1.wait`; natively `wait4` | the program's exit status 0 to 255, or 256 plus the number of the signal that ended it (`kill -9` is 265); the same again on a second wait; `-8` for a process that is not this program's |
| `(proc.exec path len args args_len)` | `CommandExt::exec`, if granted | `aipl_sys_v1.exec`; natively `execve` | does not return when it works (the program ends with the new program's status); otherwise minus an errno, as `proc.spawn` gives |
| `(net.listen addr len port)` | `std::net::TcpListener::bind`, if `--listen` granted the address and port | `listen` from AIPL's own import module `aipl_net_v1` (docs/design/NET_PLAN.md section 3), provided by aipl-run; natively `socket`, `setsockopt(SO_REUSEADDR)`, `bind`, `listen(128)` | a socket's handle; `-1` (a malformed address, a port outside 0-65535, or the system's refusal), or `-2` if not granted. The address is its `len` bytes at `addr`: 4 for IPv4, 16 for IPv6 |
| `(net.accept h)` | `TcpListener::accept` | `aipl_net_v1.accept`; natively `accept4` | the connection's handle, or `-1` |
| `(net.recv h buf max)` | `TcpStream::read` | `aipl_net_v1.recv`; natively `read` | bytes read into `buf`, `0` at the end of the connection, or `-1` (also for a buffer outside memory: not a trap) |
| `(net.send h buf len)` | `TcpStream::write` | `aipl_net_v1.send`; natively `sendto` with `MSG_NOSIGNAL` (a peer gone is `-1`, not a signal) | bytes written (perhaps fewer than `len`), or `-1` |
| `(net.close h)` | shuts a connection down, then closes it | `aipl_net_v1.close`; natively `shutdown`, `close` | `0`, or `-1` for a handle that is not an open socket. Socket handles are not file descriptors: `fs.close` does not close one, and the numbers mean different things in different backends (std/net keeps them private) |
| `(sys.exit code)` | returns the error `sys.exit(N) requested` rather than killing the host process | `proc_exit` | never returns |
| `(sys.time)` | the system clock | `clock_time_get` (realtime) | `i64` nanoseconds since 1970-01-01 UTC |
| `(sys.monotonic)` | a monotonic clock | `clock_time_get` (monotonic) | `i64` nanoseconds from an arbitrary fixed start; never decreases. Use for durations |
| `(sys.random ptr len)` | `/dev/urandom` | `random_get` | `0`, or minus its WASI errno; fills `len` bytes at `ptr` from the OS's secure generator |
| `(args.sizes count_ptr size_ptr)` | the host's argument list (`aipl eval FILE -- a b` gives `FILE a b`; empty unless set) | `args_sizes_get` | `0`, or minus its WASI errno; writes the argument count and the total bytes of the NUL-terminated arguments |
| `(args.get argv_ptr buf_ptr)` | | `args_get` | `0`, or minus its WASI errno; writes one u32 address per argument at `argv_ptr` and the NUL-terminated arguments at `buf_ptr` |
| `(env.sizes count_ptr size_ptr)` | the process environment (a host may replace it) | `environ_sizes_get` | as `args.sizes`, for `KEY=VALUE` entries |
| `(env.get env_ptr buf_ptr)` | | `environ_get` | as `args.get` |

**Failures.** A WASI call that fails gives minus its WASI errno, the same number in every backend: `-44` (ENOENT) for a missing file or a missing directory on the path, `-54` (ENOTDIR) for a file used as a directory, `-2` (EACCES) for no permission, `-31` (EISDIR) for writing or deleting a directory, `-8` (EBADF) for a handle that is not open or cannot be read (a directory: it opens, and wasmtime's directory handles cannot be read, so the VM and native executables say the same where the OS would say EISDIR), `-63` (EPERM) for a path that leaves the directory it is under (`..`, or a symbolic link out; natively the kernel's EXDEV), `-32` (ELOOP) for a directory opened through a symbolic link, which is not followed. The VM and native executables translate their OS's error through one table (`aipl_src/native/wasi.aipl` errno_bytes, `src/vm.rs` wasi_errno); anything unlisted is EIO (`-29`). std/io's `io.error_of` turns one into an `io.Error`, which is how programs see it (section 12.6).

Paths are `(ptr, len)` byte ranges in linear memory (`(str.ptr s)` / `(str.len s)` produce them from a string). In the VM they are ordinary paths. Under WASI a relative path resolves in the first preopened directory (fd 3, the working directory) and an absolute path (`/...`) in the second (fd 4), which a host grants as `/` when the program may use absolute paths: `wasmtime run --dir . --dir / prog.wasm`. Without that grant, opening an absolute path fails with `-1`. File descriptor 0 is stdin in both backends (`(fs.read 0 buf n)`; `io.read_stdin` reads all of it). A clock failure traps (it does not happen on supported hosts). File descriptors 1 and 2 are stdout and stderr in both backends, so `(fs.write 1 buf n)` prints raw bytes. The standard library builds printing of numbers and whole-file reads on exactly these primitives (section 12.6). Every WASI errno collapses to `-1`, matching the VM. Argument expressions are evaluated left to right in both backends. The `args.*`/`env.*` out-parameters `count_ptr`, `size_ptr`, and the address tables must be 4-aligned: WASI hosts trap on a misaligned one and the VM fails with `... is not 4-aligned, which WASI requires`. Like `fs.read`, the host writes through these addresses without the store guard. Under WASI the host decides what the program sees (wasmtime forwards an environment variable only with `--env`). Use `std/os` rather than these ops directly; time code with `std/time`.

### D. Atomics and threads

| Form | Type | Semantics |
|---|---|---|
| `(atomic.add p v)` | `i32` | atomically adds `v` to the word at `p`; returns the previous value. Every atomic address must be 4-aligned (any word in a `mem.alloc` block at a multiple-of-4 offset is); otherwise the VM fails and wasm traps |
| `(atomic.cas p expected new)` | `bool` | if the word at `p` is `expected`, atomically replaces it with `new` and returns `true`; otherwise `false` |
| `(atomic.lock p)` | `void` | acquires the lock word at `p` (0 free, 1 held), waiting while it is held. A word holding anything else is not a lock: the VM fails with an error and compiled code traps |
| `(atomic.unlock p)` | `void` | releases the lock at `p` and wakes waiters; the word must be 1 (held), otherwise an error / trap |
| `(thread.spawn worker arg)` | `i32` | runs `worker(arg)` on a new OS thread; `worker` is a `(fn [i32] -> i32)` reference. Returns a handle. To hand a worker data of its own, use `std/thread`'s `(call (thread.spawn_with C) worker ctx)`, with `worker` a `(fn [C] -> i32)`: the context keeps its type |
| `(thread.join h)` | `i32` | waits for the thread to finish and returns `worker`'s result |

All threads share linear memory and the allocator: `mem.alloc` and `alloc.forever` claim their blocks with one atomic add on the heap cursor, and a heap (`alloc.default`'s included) takes its lock to allocate and free, so threads may allocate freely. Atomic addresses pass the store guard (section 7.9) like stores. A handle is the address of a 16-byte thread record `[done:i32 result:i32 fn:i32 arg:i32]` allocated by `thread.spawn`, identically in both backends; treat it as opaque. Each thread has its own file-descriptor table (stdin/stdout/stderr are shared). Output from concurrent threads may interleave: a `sys.print`'s text and its newline are two writes, so lines from two threads can join; use a lock when whole lines matter.

**Compiled threads.** A module that uses `thread.spawn` is a *threaded module* (section 6.2): it imports a shared memory (`"env" "memory"`) and the wasi-threads function `"wasi" "thread-spawn"`, and exports `wasi_thread_start`. Wasmtime removed wasi-threads in version 47, so these modules run under AIPL's own host (the runner from `aipl compile --exe`, and `tests/test_threads.rs`), or any host that still implements wasi-threads; they do not run under the stock `wasmtime` CLI or in the browser demo. Atomics alone need nothing special and work in every module. Wasmtime classes wasm threads as a tier 2 feature and requires shared memory to be enabled in its configuration (`Config::shared_memory`).

### E. Structs, Pointers, and Arrays

```lisp
(struct Point [x:i32 y:i32])
(struct Mixed [flag:bool val:i64 tag:i32])
(struct Node [val:i32 next:(ptr Node)])
```

Struct definitions are module-level. Layout rules, identical in the checker, VM, and wasm backend (`type_size_and_align`, `get_field_offset`, `get_struct_size` in `src/checker.rs`):

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

**Bounds checks run everywhere.** Every `arr.get` and `arr.set` checks `0 <= i < n` against the header at `a - 4`, before the value to store is evaluated, and fails with `Array index out of bounds: index I for array of length N`: the VM as an error, compiled code (wasm and native) by writing that message and trapping (section 7.9), which `aipl-run` and native executables print as `<program>: <message>` with exit status 134. The wasm backend uses a second scratch local per function for the index and calls the module's `$aipl_oob` helper on failure. The checks cost about 20-30% under wasmtime and 45-55% natively on array-bound loops (docs/design/CHECKS_PLAN.md).

**Null checks run everywhere.** `get`, `put`, `arr.get`, `arr.set`, and `arr.len` check their pointer or array for null (address 0) right after evaluating it, before the index or the value to store, and fail with `Null pointer dereference`, the same message in the VM and in compiled code, followed by the call chain with positions. Compiled code calls the module's `$aipl_null` helper, which writes the message at 128 and traps.

**Generation checks run everywhere** (docs/design/GENERATIONS_PLAN.md). Every object has a generation word 8 bytes before it, and every pointer and array carries the generation its object had when the pointer was made. Right after the null check, `get`, `put`, `arr.get`, `arr.set`, and `arr.len` compare the two, and fail with `Use after free` (VM and compiled code alike, with the call chain) unless they match or the pointer is unchecked (made by `ptr.cast`). Every object from an allocator (`new`, `arr.new`, `std/heap`, `std/alloc`, and `std/arena`) gets a fresh generation from `std/gen` (a counter in runtime cell 56, scrambled), and freeing one changes its word, so every copy of every pointer to it fails from then on, whether or not the memory holds a new object. Freeing twice is caught the same way. `eq` on pointers compares the address and the generation. A loop in which nothing can free memory (no call to a function that may, no unsafe write) is compiled twice: if each pointer it uses and never reassigns is live when it starts, a copy without their checks runs, else the usual one, which fails exactly where it would have; checks once per loop, not once per access. Measured against the day before they came (docs/BENCHMARKS.md, 2026-10-16), the checks cost nothing to 7% on six of the eight benchmarks, 29% on knucleotide (a check inside a small call made once per byte, which no loop's single check can reach), and 94% natively on binarytrees, whose nodes grew from 8 to 32 bytes (two 8-byte pointers, the generation word, the arena's block header). Before these checks a null `get` silently read the heap cursor at address 0 (they found such a read in the self-hosted toolchain's generics pass).

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

A struct declared with `(value)` after its fields is a **value struct**: a value, like an `i32`, not a cell behind a pointer. It is copied where it goes (a `let`, an argument, a result, a field or array element holding it), it cannot be changed, only replaced by a copy with one field changed, and nothing is allocated for it. Small records of numbers (a point, a vector, a body's position and velocity, a file's size and time) are what it is for; docs/design/VALUES_PLAN.md gives the reasons for each rule.

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
- Not containing itself, directly or through other value structs (`value struct 'R' contains itself (through field 'r'): a value has a fixed size; hold it through a pointer`), and at most 8 scalars once the value structs in it are counted (`value struct 'Big' holds 9 scalars, counting the value structs in it; at most 8: keep a larger record behind a pointer`).
- No address: `(ptr V)`, `(new V a [...])`, `ptr.null`, and `ptr.cast` are errors (`'V' is a value struct, which has no address: write V, not (ptr V)`), and a field of one cannot be `put` (`a value struct's fields do not change: (with v V.x x) is a copy with one changed`). A field path writes into a value only where it sits in changeable memory: a pointer struct's field or an array element.
- No `eq`: values do not compare (`eq on value struct 'V': value structs do not compare; implement cmp.Eq for it (section 4.H) and call (cmp.Eq.same V)`).

In memory a value struct is laid out inline like any struct (above): its fields in order, each aligned to its own alignment, the whole rounded up to the largest, so `(sizeof Vec2)` is 16 and `(sizeof Particle)` 40 (`alive` at 32, then padding to 8). As a field, element, union field, or result payload it takes that size and alignment. `(make V ...)` and `(with ...)` allocate nothing.

A value struct is a type argument of the value kind (section 4.H): `(vec.Vec Vec2)` keeps its elements inline, and `(where (struct T))` still means a struct used through a pointer (`must be a struct used through a pointer ... is a value struct, which has no address`). `(pub)` comes after `(value)`: `(struct V [fields] (value) (pub))`. Outside its module, `make` needs every field `(pub)`, `with` counts as writing the field it changes, and each step of a path is checked like a `get` or `put` (section 11.1); using a value struct as a type argument is allowed whatever its fields, since a template cannot make one.

Every backend runs value structs with the same results. The VM keeps a value as its fields' values. The wasm backend flattens one into its scalars: a `Particle` local is five wasm locals, a parameter five parameters, a result five results (multi-value). No wasm block yields one: an `if`, `cond`, `match`, or `match_result` that yields a value writes it into scratch locals from each branch and pushes them after its end, and a value stored to memory goes through scratch locals first. Native code returns one result in `rax` and several through an 8-slot area in the thread's block (section 6.6). The self-hosted compiler emits the same bytes as the Rust one.

### F. Results

`(ok:E v)` and `(err:T e)` build a value of type `(result T E)`, each naming the side it does not carry, so every result is made with its whole type written (`(ok v) must name its result's err type: write (ok:E v) for a (result T E)`); `match_result` consumes one. A payload may be of any type but `void` (`a result payload must be a value, got void`). A result lives only in locals, parameters, results, and other results (never in a struct field or an array).

**Results by value.** A result is always held by value, laid out like a union held by value (section 4.J): its tag (0 = ok, 1 = err), then the slots its two sides share by kind, the ok side's scalars and the err side's each taking the first slots of their kinds, unused ones zero. A `str` side is one i32-sized slot. `(result i32 i32)` is 2 slots, `(result i32 str)` 2, `(result Vec2 i64)` 4 (the tag, the `i64`, two `f64`s); building one allocates nothing, in every backend. A result that would need more than 8 slots is refused where its type is written or where one is made: `(result Q I) needs 9 slots, more than the 8 a value holds: keep the larger side's data in a struct that is not (value) and make that side a (ptr S)`. (Until 2026-10, a result with a `str` side or more than 8 slots was a heap cell that was never freed; docs/design/MEMORY_PLAN.md M4.) For a value that may be missing rather than failed, use `(option.Option T)` from `std/option` (section 4.J).

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
| `(try_opt o)`, `o:(option.Option T)` | the value, a `T` | `none`: the function returns an `(option.Option U)` |
| `(try_or o e)`, `o:(option.Option T)` | the value, a `T` | `(err:R e)`: the function returns `(result R E)`, `e` an `E` |

Each is a `match_result` or `match` with a `return` in the failing arm, written by the resolver once the program checks (src/formats.rs, aipl_src/fmtlower.aipl; `(try r)` is `(match_result r (ok _try_1 _try_1) (err _try_err_1 (return (err:R _try_err_1))))`), so it costs what that code costs. Not in a contract (`try returns from the function, and a contract cannot`); the mismatches name both types (`try's error is str, and the function's is i32: they must be one type`, `try_opt returns none from the function, which returns (result i32 i32), not an (option.Option T)`).

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

`(defer e ...)` is a statement of a function's body. When the function returns after it, by falling off its end, by `return`, or by a `try`, its statements run, the last defer's first, after the return value is computed. A return before the defer in the text does not run it. A defer elsewhere is an error (`defer is a statement of a function's body (it runs when the function returns), not part of an expression or a block`), and so is a defer that would itself return (`a defer cannot return: it runs as the function returns`; likewise `try`). A trap does not run defers: it stops the program. The resolver writes each defer out where it runs (src/formats.rs lower_defers): `(return v)` after one becomes `(block (let _defer_ret_1:R v) D... (return _defer_ret_1))`, and the body's value is kept in `_defer_val` while the defers run.

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
Arg 0 of 'apply' expects (fn [i32] -> i32 (no_leak)), got (fn [i32] -> i32): keeps may keep memory (keeps calls str.concat; str.concat calls str.room; str.room uses mem.alloc)
```

A promising reference fits an ordinary slot (it promises more than the slot asks), never the other way: a value already typed `(fn [i32] -> i32)` does not go into a promising slot, since what it refers to is no longer known. Where two values meet, the branches of an `if`, `cond`, `match`, or `match_result`, or the two sides of `eq` and `neq`, two function types that differ only in the promise meet at the type without it: `(if c (ref double) (ref keeps))` is a `(fn [i32] -> i32)`. The rule is for the whole value only: `(arr (fn [i32] -> i32 (no_leak)))` and `(arr (fn [i32] -> i32))` are different types, as `(ptr S)` and `(ptr T)` are. A generic instance names the promise: `(vec.Vec (fn [i32] -> i32 (no_leak)))` is `Vec<fn<i32->i32,no_leak>>`. tests/aipl/memory/no_leak_slots.aipl uses each form.

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

In the library: `vec.sort_by_with` (a comparison with a context; `vec.sort_by` for one without), `thread.spawn_with` (a worker's context, section 4.D), and `alloc.custom` (a custom allocator's state, section 12.6).

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

Generics are expanded before type checking (`src/generics.rs`, `aipl_src/generics.aipl`): each distinct instantiation becomes an ordinary struct, union, or function named `Name<t,...>` (for example `Box<i64>`, `vec.Vec<ptr<Point>>`, `fn<i32,i32->i32>` for a function type argument), made by substituting the arguments for the parameters. The checker, the VM, both code generators, and any future backend only ever see concrete code, and instance names appear as such in diagnostics and wasm exports. Rules:

- A template is checked once, by itself, with its type parameters sealed (below), whether or not anything uses it: an error inside a generic body is reported at the template's own line, and holds for every type argument its clauses allow. An error inside a type argument is reported where the argument is written.
- Type arguments may be any type, including other instances and function types. Arity is checked: `generic 'Box' takes 1 type argument(s) (T), got 2`.
- A generic may not be named like a built-in form or type (`get`, `put`, `new`, `i32`, ...), since `(get p (Name T) f)` would be ambiguous.
- A generic function is applied only where a function is named: `(call (f t...) ...)` and `(ref (f t...))`. Elsewhere `(f ...)` is not an application, so a generic named `make` (as `std/vec`'s is) does not swallow a union's `(make U.v x)`. A generic struct is applied wherever a type goes.
- A generic whose instances keep growing (`(fn (f T) ... (call (f (ptr T)) ...))`) is an error once an instance name passes 1024 characters.
- Imports qualify generics like everything else: `vec.Vec` from another module, `(vec.Vec i32)`, `(call (vec.push i32) v 5)`; aliases work (`(import vec as v)`, `(v.Vec i32)`).

#### Interfaces, implementations, and where clauses

An interface names functions a type can have; an implementation provides them for one type; a template asks for them with a `where` clause. `std/cmp` defines `(cmp.Eq T)` (`same`), `(cmp.Ord T)` (`compare`, which returns -1, 0, or 1), and `(cmp.Hash T)` (`hash` and `same`), implemented for the built-in types, and `std/map` is keyed by any type with `cmp.Hash`. (docs/design/CONSTRAINTS_PLAN.md has the design and the alternatives.)

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
| `(const NAME:T value)` | a named value. `T` is `i32`, `i64`, `f64`, `bool`, or `str`. The value is a literal of that type (`65536`, `-1i64`, `0.5`, `true`, `"text"`) or, for every type but `str`, a constant expression: literals and other constants, in any order of definition, combined with `+ - * / % ^ shl shr shru bitand bitor` on `i32` and `i64` (wrapping, as at run time), `+ - * /` on `f64`, and `and or not` on `bool`, both operands of one type: `(const PAGES:i32 (* 4 PAGE_SIZE))`. Folding fails with the reason: `/ by zero`, `/ overflows`, `'Y' is not a constant`, `its value depends on itself`, `+ needs two operands of one type, got i32 and i64`, `constant 'X' is declared i64 but its value is i32`. `NAME` is in capitals: at least two characters from `A-Z`, `0-9`, `_`, starting with a letter |
| `NAME` | anywhere an expression goes: its value, as a literal. It cannot also be the name of a variable, parameter, or field |
| `(enum Name [a b (c 10) ...])` | a new type `Name` with the named members. A member without a value is one more than the member before it, the first `0`. Members start with a lowercase letter or `_`; names and values must be unique; at least one member |
| `Name.member` | a value of type `Name` |
| `Name` as a type | `[k:Name]`, `-> Name`, struct fields, `(arr Name)`, `(result Name E)`, `(fn [Name] -> r)`, generic type arguments `(vec.Vec Name)` |
| `(enum.ord e)` | `i32`: the member's value |
| `(enum.cast Name n)` | `Name`: the `i32` `n` as a member, unchecked (like `ptr.cast`); for values read back from memory |

Enums are checked like pointers: a `Name` is never an `i32` or another enum, has no arithmetic, and compares only with `eq`/`neq`. Write `(eq k Kind.square)`, and convert explicitly with `enum.ord`/`enum.cast` where a number is meant. A bare type name that is not a scalar or an enum is `Unknown type 'Colr'`; a struct name written as a type (`p:Point`) is reported with the fix (`(ptr Point)`).

At run time a constant is its value as a literal (folded at compile time, identically in both toolchains) and an enum is an `i32` (4 bytes in structs and arrays), so neither changes the compiled code. Both are expanded after import resolution and generics: in Rust by `src/consts.rs` (constants become their literals and `Name.member` becomes `(enum.cast Name value)` for the checker), and in the self-hosted toolchain by `aipl_src/consts.aipl`, which erases both (types to `i32`, members to numbers) at the start of `codegen.compile_module`; the two produce identical bytes. Like generics, constants need the resolver: `Parser::parse` alone rejects a `(const ...)` item.

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
- one that needs more than 8 slots. A union is its variant's index (the tag, an `i32`), then slots the variants share by kind: the i32-sized slots (every 4-byte scalar but `f32`: `i32`, `bool`, `str`, enums, function references), then the `f32`, `i64` (with pointers and arrays, each an address and a generation), and `f64` slots, as many of each as the variant needing most has. Each variant's scalars take the first slots of their kind in order: a value struct field its fields' scalars, a union field its own slots. So `Shape` above is 3 slots (the tag and two i32 slots, which `circle` and `rect` share), `(option.Option i64)` 2, and `std/alloc`'s `Allocator`, five variants of one pointer each, 2; a `str.Bytes` is 3. A variant's fields count once however many variants there are, but slots of different kinds never share: one variant of six `i32`s and another of six `f64`s are 14. `union 'W' needs 10 slots, more than the 8 a value holds: move the fields of its widest variant, W.nine (8 slots), into a struct and hold a (ptr S)`.

The pointer is an ordinary checked `(ptr S)` from the allocator the program names, so a stale one traps like any other (docs/design/MEMORY_PLAN.md M4; until 2026-10, such unions, and those with a `str` variant, were given a hidden heap cell that was never freed).

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
| `(format_to b "...")` | `void`; appends to `b:(ptr buf.Buf)`, or writes to `b:(ptr stream.Writer)` (section 12.6), which must then be a variable (it is named twice once lowered: `format_to takes a (ptr stream.Writer) as a variable: bind it with let first`) |
| `(print "...")`, `(println "...")`, `(eprintln "...")` | `void`; writes to standard output (with a newline), or to standard error with a newline, through a buffer it frees |
| `(dbg e)` | `e`'s value, after writing `FILE:LINE:COL: e = VALUE` and a newline to standard error: the file's name, where `(dbg` is, and `e` as written (comments dropped, each run of whitespace one space). A value shows as the forms print it (`P{x: 0}`, `[1, 2]`, `some{value: 3}`); a function reference as `<value> : T`. Lowered with the forms, to `(block (let _dbg_N:T e) (call show.eprintln ...) _dbg_N)`, so `e` is evaluated once. Errors: `dbg takes one expression, e.g. (dbg (call f x))`, `dbg shows a value, and this expression has none (it is void)`, and, since a placeholder's code is not where its text is, `a placeholder cannot hold a dbg; bind the value with let and dbg it there` |

The string is a literal. In it, `{name}` is a variable, parameter, or constant (or a literal, `{0.5}`), `{(expression)}` any expression (every form, `new`, `arr.of`, `zero`, and `for` included), and `{{` and `}}` are braces. A placeholder may not hold a string literal, a quote, a backslash, or an escape: bind such a value with `let` first. After a colon comes a format, `[[fill]align][+][0][width][.decimals][x|X|b]`:

| Part | Meaning |
|---|---|
| `fill` + `align` | `<` left, `>` right, `^` centre (an extra space on the right), with `fill` (a printable ASCII character, a space by default) around the value; numbers are right-aligned by default, everything else left |
| `+` | a sign on numbers that are not negative |
| `0` | pad a number with zeros after its sign (`{n:05}` gives `-0005`) |
| `width` | at least this many characters (UTF-8 characters, as Python counts), at most 4095 |
| `.decimals` | an `f32` or `f64` with exactly this many decimals, rounded exactly, ties to even (`{x:.2}` is Python's `{x:.2f}`), at most 40 |
| `x`, `X`, `b` | an `i32` or `i64` in hexadecimal, upper-case hexadecimal, or binary (a negative one with a `-` and its magnitude) |

A value prints as: an integer in decimal; a float as Python's `repr` writes it, the fewest digits that read back as the same value (`0.1`, `100.0`, `1e+16`, `1.5e-07`, `inf`, `nan`; an `f32`'s digits are the fewest that read back as an `f32` literal does, so `0.1f32` prints `0.1`); a `bool` as `true` or `false`; a `str` and a `str.Bytes` as their bytes; an enum as its member's name. Every other value but `void` and a function reference prints through a printer the lowering generates for its type: a struct behind a pointer as its fields, `Row{n: 2, name: "a", q: (ptr Q) @ 6400, at: V{a: 1, b: 2.5}}` (`null` for a null pointer), a pointer or array inside it as its type and address, so a cycle cannot loop; a value struct the same, `V{a: 1, b: 2.5}`; an array as `[1, 2, 3]`, its first 16 elements then `, ... N more`; a union as `variant{field: value}` or `variant`, `some{value: 5}`; a result as `ok(4)` or `err("bad")`; a generic instance's type as source writes it, `(Box i32){item: 9}`. Inside a value, text is quoted and a function reference is `<fn>`. Every field shows, a private one too: the output is for reading, and gives no way to change the field. A printer is `(fn show.debug.T [b:(ptr buf.Buf) v:T] -> (ptr buf.Buf) ...)`, its name the type with its brackets and spaces as `<`, `>`, and `,` (`show.debug.<ptr,Row>`), made for each type printed and each value type inside one, added after the program's functions in name order, and with no line-table positions (formats.rs debug_functions; aipl_src/fmtdebug.aipl writes the same text). Such a value takes no format: `formats are for numbers, bool, str, str.Bytes, and enums, not (ptr P)`. A function reference or `void` is an error: `format cannot print (fn [] -> i32): it prints every value but void and function references`. So is a format that does not fit (`decimals (.N) are for f32 and f64, not str`, `x, X, and b are for i32 and i64, not f64`, `+ and 0 are for numbers, not str`), a bad one (`bad format 'q': a format is [[fill]align][+][0][width][.decimals][x|X|b], e.g. {n:>8} or {x:.2}`), and a malformed string (`unclosed { in a format string (write {{ for a brace)`, `a } in a format string closes nothing (write }} for a brace)`, `{} needs a value: {name} or {(expression)}`). Errors are reported at the string literal.

**How it runs.** The resolver splits each string as it reads a module, before it qualifies names, so a placeholder's names are renamed like the rest of the module: `(println "x = {x:>4}!")` becomes `(format.println "x = " (format.piece x ">4") "!")`. The checker types each piece and checks its format. Then, once the program checks, the forms become the calls a programmer would write to `std/show` (section 12.6), one per piece, chosen by the value's type, `(call show.println (call show.lit (call show.put_i32 (call show.lit (call show.start) "x = ") x 8388612) "!"))`, so no backend has anything to do with them. A module that uses a form imports `std/show` itself. Both toolchains do all three steps (src/formats.rs; aipl_src/fmtsplit.aipl, resolver.aipl, checker.aipl, fmtlower.aipl) and produce the same bytes. `Parser::parse` alone rejects the forms: `(println ...) is expanded during import resolution; parse the file with Resolver, not Parser::parse`. A format string only known at run time is not supported.

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

Tests are left out of every build but a test build, so they cost an ordinary program nothing. `aipl compile --tests FILE -o OUT` makes the test build: test K of the file (not of its imports) is `(fn __test_K [] -> void body...)`, and a `_start` runs the test its first argument names, then exits with that test's failure count (at most 125); std/testing (section 12.6) and std/os are imported. A module with its own `_start` cannot have a test build. Both toolchains make it, byte for byte alike.

`aipl test FILE` runs each test of the file in a fresh process, so tests share no state: under aipl-run by default, natively with `--native`, in the VM with `--vm`, or in all three with `--all-backends`, where a test passes only if every backend passes it with the same output. `aipl test DIR` runs every file below DIR that has tests, and `-k TEXT` only the tests whose names contain TEXT. It prints the file, then `ok` or `FAIL` and each test's name, a failing test's output indented under it, and a total, `7 tests: 3 passed, 4 failed`; the exit status is 1 if any failed. A file with no tests runs its `--func` entry point as before (default `run_all`).

Errors: `a test is (test "name" body...)`; `expect is only for a test's body: (test "name" ... (expect c))`; `expect takes one condition, e.g. (expect (eq n 3))`; `expect takes a bool, e.g. (expect (eq n 3)), got i32`; `a test build makes its own _start: test a module without one`.

### M. `when`, `for`, `and`/`or` of more than two, and the array literal

`(when c body...)` is the `if` with one branch, and the only one: an `if` whose then or else is an empty `(block)` is an error (`an if has two real branches, and this else is an empty (block): for one branch, write (when c then...)`, and for an empty then `an if has two real branches, and this then is an empty (block): for an else alone, write (when (not c) else...)`). The parsers read `when` themselves, as `(if c (block body... (block)) (block))`: the empty block that ends the body makes every expression of the body a statement, so a value in it is dropped, as in a `while` or `for` body, and the `when` is `void`. The printers write that tree back as `when` (CANONICAL_PLAN.md step 4).

The others are written for the reader and rewritten as the resolver reads a module (src/sugar.rs; aipl_src/resolver.aipl), so nothing after it sees them:

| Form | Is | |
|---|---|---|
| `(for i start end body...)` | the internal counted loop `(counted_loop i start (- end 1) 1 body...)` (inclusive, with a step; not for programs, and `loop` written in a program is an error: CANONICAL_PLAN.md step 3) | `i` from `start` up to `end`, `end` left out: `(for i 0 (arr.len xs) ...)` visits every element; `start` and `end` are evaluated once, before the first iteration; `break` and `continue` work as in `while` (`end` must be above `i32`'s least value, as `(- end 1)` wraps) |
| `(and a b c ...)`, `(or a b c ...)` | `(and a (and b c ...))` | left to right, stopping at the first operand that decides |
| `(arr.of T a x y z)` | `(block (let _arr_of_L_C:(arr T) (call (alloc.array T) a 3)) (arr.set T _arr_of_L_C 0 x) (arr.set T _arr_of_L_C 1 y) (arr.set T _arr_of_L_C 2 z) _arr_of_L_C)`, `L` and `C` the form's line and column | the array literal (section 4.A); `(arr.of T a)` an empty array; not inside a formatting form's placeholder (bind it with `let` first) |

Errors: `when takes a condition and a body: (when c body...)`; `for is (for i start end body...): i from start up to end, end left out`; `a for has no step: ...` for a number right after a `for`'s end. `arr.of takes the allocator to make it from, then the values: ...` for an `arr.of` with no allocator, or a literal where the allocator goes. Every list a rewrite makes is at the form's place, so positions, and bytes, are the same in both toolchains.

## 5. WebAssembly Binary Execution Semantics

AIPL code maps 1-to-1 to WebAssembly binary opcodes:
- `(mem.load32 ptr)` -> `i32.load (MemArg { offset: 0, align: 2 })`
- `(mem.store32 ptr val)` -> `i32.store (MemArg { offset: 0, align: 2 })`
- `(shl a b)` -> `i32.shl`
- `(shr a b)` -> `i32.shr_s`
- `(bitand a b)` -> `i32.and`
- `(bitor a b)` -> `i32.or`

---

## 6. Toolchain Pipeline & Compiler Expectations

Every AIPL entry point runs the same stages in order. A failure at any stage aborts with a single diagnostic and a non-zero exit code; nothing downstream runs.

```
source.aipl
   │
   ▼
[1] Resolver   (src/resolver.rs)   reads the file and every (import ...), as S-expressions (src/sexpr.rs);
   │                               renames imported functions and structs to `mod.name`; one flat program
   ▼
[2] Generics   (src/generics.rs)   expands every generic instance into an ordinary struct or function
   │
   ▼
[3] Parser     (src/parser.rs)     each item -> typed AST (Module { name, imports, structs, functions });
   │                               every node keeps its (line, col)
   ▼
[4] Checker    (src/checker.rs)    static types + contract typing; no inference across functions
   │
   ├──────────────────────────────┐
   ▼                              ▼
[5a] VM        (src/vm.rs)       [5b] Wasm backend (src/compiler/wasm.rs)
     tree-walking interpreter         a core wasm module (wasm-encoder)
     runs contracts and               contracts and bounds checks compiled in
     array bounds checks              │
                                      ▼
                                 [6] Native backend (aipl_src/native/, Linux x86-64, `compile --exe`)
                                      translates the wasm to an ELF executable
```

`aipl compile` adds a stage by default, between [4] and the code generator: the optimizer (`aipl_src/codegen_opt.aipl`, section 6.7) rewrites the checked program, which is checked again and compiled by the self-hosted code generator; `aipl compile --no-opt` takes the path drawn, through [5b].

The self-hosted toolchain mirrors every stage in AIPL: resolving and generics (`resolver.aipl`, `generics.aipl`), constants (`expand.aipl`), parsing (`parser.aipl`, into the typed tree of `ast.aipl`), checking (`checker.aipl`), and code generation (`codegen.aipl`). `front.aipl` chains the checking stages; its parser and checker report the same errors as Rust's, word for word (section 6.4).

### 6.1 CLI surface (`src/main.rs`)

| Command | Stages run | Success output |
|---|---|---|
| `aipl verify FILE` | 1-4 | `[AIPL Verifier] OK: module 'NAME' type-checks. Contracts are type-checked, not proven; ...` (nothing is proved: `req`/`ens` run on every call, in the VM and in compiled code, section 3) |
| `aipl eval FILE [--func NAME] [--listen ADDR:PORT]... [--allow-run PROGRAM]... [--max-memory SIZE] [-- ARGS...]` | 1-4, 5a | `[AIPL VM] Executing function 'main' from 'FILE'...` then `[AIPL Result]: Int(0)` (Rust `Debug` of the returned `Value`; default `--func main`), and exits with main's status, as a compiled program does (section 6.5). The program's argv is `FILE ARGS...` (`std/os`); it may listen only on the addresses and ports `--listen` grants (std/net), and start only the programs `--allow-run` names (`*` for any; none by default) |
| `aipl eval -e EXPRS [--with FILE] [--wasm \| --native] [-- ARGS...]` | 1-4, then 5a (or 5b and the launcher, or 6) | the program's own output, then the last expression's value and type, `42 : i32` (any value as the formatting forms print it, `Row{n: 2} : (ptr Row)`; a function reference `<value> : T`; nothing for `void`). The expressions are the body of a function `__eval` added at the end of FILE's module (its private functions in scope; its `main` not run), or of an empty module; standard modules they name (`str.find`) are imported for them. Errors on their lines are at `-e:LINE:COL`, and a call chain leaves out the generated printer. `--wasm` and `--native` run them as the program's `_start` (refused for a FILE that defines one). Exit 0, or 1 on an error (134 for a trap under `--wasm` and `--native`, as any program) |
| `aipl compile FILE [-o out.wasm]` | 1-4, then the optimizer and the self-hosted code generator (section 6.7) | `[AIPL Compiler] Successfully compiled 'FILE' -> 'out.wasm' (N bytes)` |
| `aipl compile --no-opt FILE [-o out.wasm]` (with any of the options below) | 1-4, 5b | the module without the optimizer: the Rust code generator's, byte for byte the self-hosted one's (section 6.4), with the VM's heap layout (the optimizer shrinks the data, which moves the heap, so printed addresses differ from the VM's) |
| `aipl compile --exe [--sandbox] [--listen ADDR:PORT]... [--allow-run PROGRAM]... [--serve ADDR:PORT] [--target native\|wasm] FILE -o prog` | 1-4, 5b, then 6 for native | a standalone executable `prog`: native machine code on Linux x86-64 (section 6.6), else the launcher bundle (section 6.5). Its grants are fixed in it: it may listen where `--listen` said, and nowhere else, and start the programs `--allow-run` named (a native executable keeps each path to 256 bytes), and no others. With `--serve`, `prog` is a server giving a fresh copy of the program per connection on that address, as `aipl serve --fresh` does: natively it forks a copy of itself made once memory is ready (`aipl_src/native/fresh.aipl`), the bundle runs aipl-run's `--serve-fresh`. Until it is ended; a threaded program is refused when compiled |
| `aipl compile [--exe] --max-memory SIZE FILE -o out` | as without it | the module (or executable) with memory's maximum lowered from 2 GiB to SIZE: 1M to 2G in whole 64K pages, as `512M` (bytes, or K, M, G, binary). The module declares it, so the VM (given the same `--max-memory`), aipl-run, native executables, and any wasm host stop at the same place, the `mem.alloc` memory cannot grow to hold, with `Out of memory: the limit is 4 MiB` (for `--max-memory 4M`) and its call chain. Only lowering: below 2 GiB every address is a non-negative i32, which std and the compiler rely on |
| `aipl run [--sandbox] [--listen ADDR:PORT]... [--allow-run PROGRAM]... prog.wasm [-- ARGS...]` | the `aipl-run` launcher | runs a compiled module natively (section 6.5); exits with its status |
| `aipl serve --fresh FILE --listen ADDR:PORT [--requests N] [-- ARGS...]` | 1-4, 5b, then the command becomes `aipl-run --serve-fresh ADDR:PORT --temporary [--requests N] --name FILE prog.wasm` (one process: ending it ends the server) | a fresh instance per connection (docs/design/NET_PLAN.md, mode B): the module compiled once, then for each connection a new instance whose standard input is the connection, its standard output held until it exits (status 0: sent to the client; any other status or a trap: `500 Internal Server Error`, the program's error output on the server's), the connection then closed. An answer longer than 64 MiB is a 500 too, never part of one. FILE answers one request on its standard input, as `serve.once` does; a trap costs only its request. An address that cannot be listened on stops it with `FILE: cannot listen on ADDR:PORT: Address already in use (os error 98)` and status 1, as the executables say it. Until interrupted, or N connections. Without `--fresh` it is refused with the way to a long-running server (`aipl eval --listen`, a program that calls `serve.run`); threaded modules are refused |
| `aipl compile --self FILE [-o out.wasm]` | 1-4, 5b, then `driver.compile_file`, compiled to wasm by the Rust toolchain and run under wasmtime | compiles with the Rust toolchain and with the self-hosted one (`aipl_src/driver.aipl`: resolver, checker, code generator, as `aiplc`) and fails unless the bytes are identical (section 6.4); implies `--no-opt` |
| `aipl doc [MODULE \| MODULE.ITEM \| FILE.aipl] [--private]`, `aipl doc --find WORD`, `aipl doc --check FILE...` | `aipl_src/doc.aipl` compiled and run in wasmtime (the self-hosted `aipl doc` is the same code) | each public item (functions, structs with their readable fields, unions, enums, constants, interfaces) as a caller writes it, its module's own type names qualified (`fn stream.open [a:alloc.Allocator path:str] -> (result (ptr stream.Reader) str)`), then the `;;` block directly above it (a comment after code on its line, a section banner `;; =====`, or one with a blank line before the item documents nothing), then its `req`/`ens`; no MODULE lists the standard modules with their first sentence; MODULE is `./MODULE.aipl`, else the standard one; `--find` searches names and comments across the standard library, any case; `--check` exits 1 naming each public item with no comment (`cargo test` runs it over the standard library). Exit 1 on a name that does not exist, 2 on bad usage |
| `aipl test FILE\|DIR [-k TEXT] [--vm \| --native \| --all-backends]` | the test build (section 4.L), 1-4, 5b, run once per test | each test `ok` or `FAIL` with its output, then `N tests: P passed, F failed`; exit 1 if any failed |
| `aipl test FILE [--func run_all]` (a file with no tests) | 1-4, 5a | `[AIPL Test] All groups passed.` and exit 0; otherwise `N group(s) failed.` and exit 1 |
| `aipl watch FILE [--test] [-- ARGS...]` | `aipl eval FILE` (or `aipl test FILE`), again on each change | runs FILE, then again each time it or a file it imports changes (checked four times a second), clearing the screen first on a terminal; after each run `[ran in 0.02 s, exit 0]`; until interrupted |
| `aipl compile --tests FILE -o out.wasm` | the test build, 1-4, 5b | a module whose `_start` runs the test its first argument names (section 4.L) |

Errors are printed to stderr as `Error: MESSAGE`, and the exit status is 1; a runtime error in `eval` or `test` is followed by its call chain (section 9). `eval` and `test` only invoke zero-argument functions. To exercise a function that takes parameters, wrap it in a zero-arg driver or write a Rust test (section 10.2).

### 6.2 What a compiled `.wasm` module looks like

`WasmCompiler::compile` produces a WebAssembly module with these sections, in this order: **type, import (only if the module does I/O), function, memory, export, code, data**, plus a table and element section when it uses function references. A module with compiled checks (an array index, a `req`, or an `ens`) ends with seven helper functions (`$aipl_dec`, `$aipl_oob`, `$aipl_text`, `$aipl_begin`, `$aipl_end`, `$aipl_null`, `$aipl_uaf`) that write a failed check's message (docs/design/CHECKS_PLAN.md); they are not exported. Allocation uses the threads proposal's `i32.atomic.rmw.add`, which wasmtime and every major browser accept on ordinary memory.

A **threaded module** (one that uses `thread.spawn`) differs: it imports its memory as shared (`"env" "memory"`, min 16, max 32768 pages) instead of defining it, adds a global (the address of this thread's runtime scratch cells, 64 in the main thread), a start function, a data-count section, and two compiler-generated functions after the user's: the start function, which copies the heap cursor and the string literals into memory once (guarded by an atomic flag at address 88, since every thread instantiates the module again), and the exported `wasi_thread_start(tid, record)`, which allocates the thread's 24-byte runtime scratch block, calls the worker through the function table, stores its result, and wakes `thread.join`. Its data segments are passive.

| Item | Value |
|---|---|
| Imports | only those used, in this order: from `wasi_snapshot_preview1` `fd_write`, `fd_read`, `path_open`, `fd_close`, `proc_exit`, `path_unlink_file`, `args_sizes_get`, `args_get`, `environ_sizes_get`, `environ_get`; from `wasi` `thread-spawn`; from `wasi_snapshot_preview1` `clock_time_get`, `random_get`; then AIPL's own: from `aipl_net_v1` `listen`, `accept`, `recv`, `send`, `close`, and from `aipl_sys_v1` `chmod`, `spawn`, `stream`, `read`, `write`, `close`, `wait`, `exec`, and last of all `path_rename`, `path_remove_directory`, and `path_create_directory` from `wasi_snapshot_preview1` (added after the others, so that no earlier import moved). A threaded module also imports its memory (`"env" "memory"`, shared). Their types come first in the type section, and every user function index is offset by the import count. A module that does no I/O has no import section and instantiates with no imports. |
| Memory | one linear memory, min 16 pages (1 MiB, same as the VM), max 32768 pages (2 GiB), exported as `"memory"` |
| Data segments | one writing the heap start at address 0 (1024, or the first 8-aligned address after the string literals); if the module has string literals, a second at address 1024 holding every distinct literal as `[len u32 LE][bytes]` |
| String literal | `i32.const <address of its bytes>`; `str` values are pointers (section 4.B) |
| I/O scratch | functions that do I/O, `atomic.cas/lock/unlock`, or `thread.spawn` get two extra `i32` locals; the WASI lowerings use runtime cells 64-87 for iovecs and out-parameters (section 7.9), or in a threaded module the same offsets in the current thread's scratch block |
| Heap cursor | the `i32` at address 0; `mem.alloc` compiles to `i32.const 0; <size>; i32.atomic.rmw.add` (the size is evaluated first, then one atomic add claims the block), followed by a check that grows memory with `memory.grow` when the cursor passes `memory.size` (no locals used) |
| Loops | `block { loop { ... } }`; a counted loop (what `for` becomes, `counted_loop`) also wraps its body in a block so `continue` falls into the step (section 7.10) |
| Store guard | every `mem.store*`, `put`, and `arr.set` is preceded by two checks, each an `if` that traps (`unreachable`), one if the address is in bytes 0-3 and one if it is between 64 and the heap start (the runtime block and the string literals); each function gets two extra `i32` scratch locals, the first for this check (which `ok`/`err`, `match_result`, and `arr.new` also use) and the second for the index of a bounds check |
| Bounds checks | `arr.get`/`arr.set` compare the index (unsigned) with the length at `a - 4` and on failure call `$aipl_oob`, which writes `Array index out of bounds: ...` and traps (section 4.E) |
| Contracts | each `req` is checked at the function's entry; with an `ens`, the body is a `block` that `return` branches to the end of, where the result is kept in a hidden local `res` and each `ens` is checked; a failure writes the message (section 3) through `$aipl_begin`/`$aipl_text`/`$aipl_dec`/`$aipl_end` and traps |
| Function refs | only when the module uses `ref`/`call_ref`: one extra type per distinct `call_ref` signature after the function types, a funcref table (section id 4) of every function, and an element section (id 9) filling it; `call_ref` is `call_indirect` (section 4.G) |
| Results and arrays | a result is its slots (multi-value); `match_result` tests the tag slot with `i32.eqz`; an array is its address and generation, its count in the header before it (sections 4.E, 4.F) |
| Names | a `name` custom section naming every function (the generated ones `_start`, `wasi_thread_start`, `aipl.init`, and the `aipl.` check helpers), so traps can print where they happened (section 6.5) |
| Lines | an `aipl.lines` custom section, last: for each function, the source position (`file:line:col`) of every call and every instruction that can trap, and of every `if` (docs/design/LINES_PLAN.md has the format). Files are named by the entry file's name and each import's path (`shapes.aipl`, `vec.aipl`, `native/lower.aipl`), so the bytes do not depend on where the files are. It adds about a fifth to a module's size |
| Exports | **every** function in the flat module, under its AIPL name (`add`, `compiler.tokenize`, ...), plus `memory`; `_start` when the module has a zero-argument `main` (section 6.5); `wasi_thread_start` in a threaded module |
| Function types | params map `i32/bool/str` and every pointer, array, enum, union, result, and function reference to `i32`, `i64 -> i64`, `f32 -> f32`, `f64 -> f64`; a `void` return is an empty result list |
| Locals | every `let` anywhere in the body (including nested in `if`/`while`/`for`/`block`) plus every `for` variable becomes one wasm local, allocated after the params; then `res` for a function with `ens`, the two scratch locals, and the two I/O scratch locals |

Bytes 0..8 are always `00 61 73 6D 01 00 00 00` (`\0asm`, version 1). A module that compiles must also pass `wasmparser::Validator::validate_all`; the test suite enforces this.

### 6.3 Backend support matrix (as of 2026-10-19)

"Yes" means the op runs. The native backend (section 6.6) translates the wasm backend's output, so it supports exactly what that column supports. "Err" means the backend returns an explicit error naming the op; there are no silent defaults or no-ops in any backend. Every op the checker accepts compiles: `tests/test_opcode_conformance.rs` checks each one in value and statement position, validates the wasm, and compares the self-hosted compiler's bytes. The self-hosted column is `aipl_src/codegen.aipl` (section 6.4).

| Ops | Checker | VM | Rust wasm backend | Self-hosted |
|---|---|---|---|---|
| `+ - * / % divu remu ^ shl shr shru bitand bitor` on `i32` | Yes | Yes, wrapping | Yes | Yes |
| the same on `i64` | Yes | Yes, wrapping at 64 bits | Yes (`i64.*`) | Yes |
| `+ - * /`, comparisons on `f32` / `f64` | Yes | Yes | Yes (`f32.*` / `f64.*`); `%`, `divu`, `remu`, shifts, bitwise are Err | Yes (same rejections, compile error 99) |
| `eq neq lt lte gt gte` on `i32`; `eq neq` on `bool`; `and or not` | Yes | Yes | Yes | Yes |
| `ltu lteu gtu gteu` on `i32` / `i64` | Yes | Yes | Yes (`i32.lt_u` ... `i64.ge_u`) | Yes |
| `checked.add checked.sub checked.mul` on `i32` / `i64` | Yes | Yes, error `Integer overflow in checked.add` | Yes, trap on overflow (section 8.2) | Yes |
| `i64` literals | Yes | Yes | Yes | Yes |
| `f64` literals | Yes | Yes | Yes | Yes, with Rust's bits for every literal (section 6.4) |
| `f32` literals | Yes | Yes | Yes | Yes, rounded from the `f64` in integer arithmetic, with Rust's bits |
| `i64.extend_s i64.extend_u i32.wrap` | Yes | Yes | Yes | Yes |
| `f64.convert_i64_s i64.trunc_f64_s f64.reinterpret_i64 i64.reinterpret_f64` | Yes | Yes (`trunc` errors on NaN / out of range) | Yes (`trunc` traps) | Yes |
| `f32.demote f64.promote f32.convert_i32_s f32.convert_i64_s f64.convert_i32_s i32.trunc_f32_s i32.trunc_f64_s i64.trunc_f32_s f32.reinterpret_i32 i32.reinterpret_f32` | Yes | Yes (`trunc` errors on NaN / out of range) | Yes (`trunc` traps) | Yes |
| `f64.sqrt` | Yes | Yes (Rust's correctly rounded `sqrt`) | Yes | Yes |
| `mem.load8/8_s/16_u/16_s/32/64`, `mem.store8/16/32/64` | Yes | Yes | Yes | Yes |
| `u8 i8 u16 i16` fields and elements | Yes | Yes | Yes (`i32.load8_u` ... `i32.store16`) | Yes |
| `mem.alloc`, `mem.grow` | Yes | Yes | Yes | Yes |
| `atomic.add/cas/lock/unlock` | Yes | Yes, real across OS threads | Yes (wasm atomics; `lock` waits with `memory.atomic.wait32`) | Yes |
| `struct`, `new`, `get`, `put`, `sizeof` | Yes | Yes | Yes | Yes |
| `arr.new`, `arr.get`, `arr.set` | Yes | Yes, bounds-checked | Yes, bounds-checked | Yes |
| `req`, `ens` contracts | Yes | Yes, checked on every call | Yes, checked on every call (message without the position, section 3) | Yes |
| `inv` contracts | Yes | Yes, at entry and exit | Yes, at entry and exit | Yes |
| `ok`, `err`, `match_result` | Yes | Yes | Yes (held by value, up to 8 slots) | Yes |
| `sys.print` | Yes, `str` arguments only | Yes | Yes via WASI `fd_write` | Yes |
| `sys.exit` | Yes | returns the error `sys.exit(N) requested` | Yes via WASI `proc_exit` | Yes |
| `sys.time`, `sys.monotonic`, `sys.random` | Yes | Yes | Yes via WASI `clock_time_get` / `random_get` | Yes |
| `fs.open/read/write/close/delete/rename` | Yes | Yes, real `std::fs` | Yes via WASI | Yes |
| `args.sizes/get`, `env.sizes/get` | Yes | Yes (host-set args, process environment) | Yes via WASI `args_*` / `environ_*` | Yes |
| `proc.spawn/stream/read/write/close/wait/exec` | Yes, real processes, granted by `--allow-run` | Yes, the same | Yes via `aipl_sys_v1`, which aipl-run provides (a stock WASI host does not) | Yes |
| `fs.chmod` | Yes, `std::fs::set_permissions` | Yes, the same | Yes via `aipl_sys_v1`, which aipl-run provides (a stock WASI host does not) | Yes |
| `net.listen/accept/recv/send/close` | Yes | Yes, real `std::net`, granted by `--listen` | Yes via `aipl_net_v1`, which aipl-run provides (a stock WASI host does not) | Yes |
| `ref`, `call_ref`, `(fn [..] -> r)` types | Yes | Yes | Yes (funcref table, `call_indirect`) | Yes |
| `const`, `enum`, `enum.cast`, `enum.ord` | Yes | Yes | Yes (literals and `i32`; no code of their own) | Yes (erased by `consts.aipl`) |
| `union`, `make`, `match` | Yes | Yes | Yes (held by value, up to 8 slots; `match` is an `if` chain ending in `unreachable`) | Yes |
| `return`, `break`, `continue`, `cond` | Yes | Yes | Yes (`return`, `br`; `cond` is nested `if`) | Yes |
| `thread.spawn / thread.join` | Yes | Yes, real `std::thread`; the worker is a `(fn [i32] -> i32)` reference | Yes, as a threaded module (section 4.D): needs a host that provides `wasi.thread-spawn` | Yes |
| `str` literals, `str.len`, `str.ptr` | Yes | Yes | Yes (interned data segment, pointer identity) | Yes |
| `(import ...)` | resolved before checking | | | resolved first by `resolver.aipl` (`driver.aipl` chains the two); `compile_module` itself takes one import-free module |

Rule of thumb for code generators: whatever `aipl verify` accepts runs in the VM and compiles. Compiled I/O needs a WASI host with a preopened directory (section 10.5), and threads need a wasi-threads host (AIPL's runner). Array bounds checks and `req`/`ens` contracts run in every backend.

### 6.4 The self-hosted backend (`aipl_src/codegen.aipl`)

`codegen.compile_module [src_ptr:i32 src_len:i32] -> i32` tokenizes and parses AIPL source (via `compiler.tokenize` / `compiler.parse_ast`) and emits a complete wasm module. It stores the output pointer in cell 60 and returns the byte length, or `-1` with a nonzero compile error code in cell 4 (the first error encountered; later ones are usually consequences). For everything it accepts, the output is required to be **byte-identical** to `WasmCompiler::compile`. `tests/test_selfhost.rs` enforces this on over 40 programs, including `memory.aipl`, `compiler.aipl`, the standard library, and codegen.aipl itself (flattened with its imports by the Rust resolver first, since `compile_module` does not resolve imports). `aipl compile --self` checks the same thing for any file and, on a mismatch, reports the first differing byte, the section (and code-section function) it falls in, and a hex window of each side.

It infers each expression's static type the way `expr_type` in `src/compiler/wasm.rs` does (`node_type` / `group_type`) and selects `i32.*` / `i64.*` / `f32.*` / `f64.*` instructions, `if` and `match_result` block types, struct field and array element load/store widths, and alignment from it. It runs in the VM today; compiled to wasm it also compiles itself (section 10.6). Limits that differ from the Rust backend:

- **One module, no imports.** `compile_module` itself takes a single import-free source. Imports are flattened first by `aipl_src/resolver.aipl` (section 11), which `aipl compile --self` runs in the VM; `aipl_src/driver.aipl` chains the two (`driver.compile_file`), and its `main`/`_start` make it a command: compiled to wasm, `wasmtime run --dir . --env AIPL_PATH aiplc.wasm IN.aipl OUT.wasm` compiles a multi-file program with no Rust involved (standard library at `$AIPL_STD`, else `share/aipl/std/` beside the executable's folder, else `aipl_src/std/` below the working directory: `aipl_src/stdpath.aipl`); `aipl compile --exe aipl_src/driver.aipl -o aiplc` makes the same command an executable (`./aiplc IN.aipl OUT.wasm`); exit status 0, 1 on a resolve, type, compile, or write error, 2 on bad usage. The driver type-checks before compiling (`front.aipl`), so it rejects every program `aipl verify` rejects, with the same message; it reports the error as `path: line:col: message` in the file the error is in, mapping the flat program back through the resolver's and the generics pass's re-printing (`origins.aipl`; an error inside a generic template is reported at the template). In the VM: `aipl eval aipl_src/driver.aipl -- IN.aipl OUT.wasm`. The byte-parity tests in `tests/test_selfhost.rs` still flatten with the Rust resolver and `src/printer.rs` (`tests/test_printer.rs` checks that round trip), which keeps them independent of the AIPL resolver. `aipl_src/aipl.aipl` adds the native backend to the driver: built natively, it is the stage-0 compiler, `aipl compile [--exe [--sandbox]] FILE.aipl -o OUT`, which builds the next version of itself with no Rust (bootstrap/README.md). A qualified call such as `(call util.f)` resolves only if a function with that exact name is defined in the source given.
- **Float literals get Rust's bits.** `compile_module` converts an `f64` literal with `std/float`'s `from_decimal` (section 12.6), which rounds to the nearest double, ties to even, as Rust's `parse::<f64>` does: one IEEE multiplication or division when the digits form an integer of at most 2^53 and the power of ten is at most 22 either way, and exact big-integer arithmetic (`std/bigint`) otherwise, subnormals, infinities, and halfway cases included. Both tokenizers read the same literals (a leading `+`, exponents such as `1.5e3`).

Buffers are sized from the input: tokens exactly (`compiler.count_tokens` first, `(sizeof compiler.Token)` each), AST `(sizeof compiler.Node) * (tokens + 2)`, and output, section scratch, and function scratch `4 * src_len + 64 KiB` each. Memory is grown with `mem.grow` as needed, so a compile works within the 32768-page limit shared by both backends. Compiling the whole self-hosted toolchain (driver, resolver, checker, codegen, and the standard library it uses), checking included, takes about 47 MB of process memory in the native `aiplc` (2026-10-06).

| Compile error (cell 4) | Meaning |
|---|---|
| 90 | `mem.grow` refused: the compile needs more than 32768 pages |
| 91 | output or a function body exceeded `4 * src_len + 64 KiB` |
| 92, 93 | not given since 2026-10-26: the tables of functions, parameters, and locals grow, as the Rust code generator's vectors do (they were 8192 functions, 16 parameters, 1024 locals) |
| 94 | more than 8 scalars in a value struct (which the checker refuses first); the tables of structs, unions, variants, fields, and `call_ref` signatures grow since 2026-10-26 |
| 95 | a struct field, array element, or `sizeof` type that cannot be stored in memory (`void`, a result) |
| 96 | unknown struct or field in `new`/`get`/`put`/`sizeof` |
| 98 | a type the backend cannot lower (an unknown type name, or `void` where a value is needed) |
| 99 | an operator applied to a type with no wasm instruction for it (e.g. `%` on `f64`, `+` on `str`) |
| 100 | no longer given (2026-10-19): the label stack grows as wasm.rs's does; before, past 255 nested blocks it crashed instead of reporting this |
| 101 | `break` or `continue` with no enclosing loop (the Rust checker rejects this first) |
| 768 | the string literals and the heap start do not fit in the first 1 MiB of memory, the limit both backends share |
| 971 | a symbol that is not a local or parameter |
| 974 | `Name.member` naming a member its enum does not have (`consts.aipl`; the Rust resolver reports it first) |
| 975 | `make` or a `match` arm naming a variant its union does not have (the Rust checker reports it first) |
| 987 | a form whose head is not a recognised keyword |
| 999 | an empty expression where one is required |
| 1452 | call to an undefined function; cells 44/48 hold the callee name's source offset and length |

### 6.5 Standalone executables and the `aipl-run` launcher

A compiled module runs under `aipl-run` (`src/bin/aipl_run.rs`), the toolchain's native launcher: wasm cannot start itself, so a small Rust program embeds wasmtime, loads the module (compiled by Cranelift at start-up, about 10 ms for a small program), and connects it to the operating system. It contains no compiler logic. On Linux x86-64, `aipl compile --exe` writes native executables instead (section 6.6), which need no launcher.

- **Entry point and exit status.** A module with a zero-argument `main` and no `_start` of its own gets an exported `_start` that calls it, so the module is a WASI command. `main` gives the exit status, the same in every backend (`aipl eval` exits with it too), and returns one of three types; the checker refuses any other:
  - `i32`: the status itself, 0 for success and 1-125 for a failure (`_start` calls `proc_exit` with a status that is not 0).
  - `(result i32 str)`: `(ok:str 0)` (or another status), or `(err:i32 "why")`, whose message goes to standard error, then a newline, and the status is 1 (`_start` reads the result itself; `aipl eval` does the same with main's value).
  - `void`: status 0.

  `sys.exit` ends the program with its status from anywhere. A status must be 0-125: as under wasmtime, any other value (including a negative one) is an error reported with status 134, since shells reserve 126 and up. A program's output is what it prints or writes; main's value is only the status, so a program reporting a number prints it (`(println "{n}")`) and returns 0. A trap (a failed contract or bounds check included) prints the program as invoked and the reason, then its call chain, the functions it happened in, innermost first, and exits with 134:

  ```
  ./prog: wasm trap: integer divide by zero
    at math.div (math.aipl:4:5)
    at main (prog.aipl:9:22)
  ```

  Each frame names its function and where in the source it was: the expression that trapped for the innermost frame (the operation, the `arr.get` of a failed bounds check, the condition of a failed `req` or `ens`), and the call for each frame below it. In an optimized build (section 6.7) a function inlined into its caller has no frame: a trap inside it is reported at the call, in the caller's frame (`at main (prog.aipl:9:22)` above, had `math.div` been inlined). A frame with no recorded position shows only its name, such as the innermost frame of a stack overflow, which traps on entry. The chain comes from the module's name section and the positions from its line table (section 6.2), and it leaves out the generated functions (`_start`, `wasi_thread_start`, and the `aipl.` check helpers) and any without a name; it shows at most 32 frames, then `  ... N more`. A trap in a spawned thread shows that thread's chain. The exit-status error above is not a trap: one line. `AIPL_BACKTRACE=1` makes `aipl-run` print wasmtime's full report instead. Native executables print the same lines, walking their own frame pointers. (`main`'s return value is not the exit status: many programs return data.)
- **What the program sees.** The real stdin, stdout, and stderr; the command line (argv[0] is the program); the environment; the working directory as preopened fd 3 and `/` as fd 4, so relative and absolute paths both work (section 4.C); and, for a threaded module, the wasi-threads `thread-spawn` import (section 4.D). `--sandbox` grants only the working directory.
- **`aipl compile --exe --target wasm FILE -o prog`** (the default on platforms without a native backend) writes the launcher followed by the module, the grants (one a line: `--listen`'s `ADDR:PORT`, or `run:PROGRAM` for each `--allow-run`), and a 20-byte trailer: flags (`u32` LE, bit 0 = sandbox, bit 1 = `--serve`, whose address is then the grants' first line), the grants' length and the module's (`u32` LE each), and the magic `AIPLEXE2`. The launcher finds a module appended to its own file and runs it; without one it runs a `.wasm` named on its command line (`aipl-run [--sandbox] [--listen ADDR:PORT]... [--allow-run PROGRAM]... prog.wasm ARGS...`). The result is one file with no other dependency, about 18 MB in a release build (the size is wasmtime), built for the host's OS and CPU. The `.wasm` remains the portable form.
- The launcher is found as `aipl-run` next to the `aipl` binary, or at `$AIPL_RUNNER`. Writing the bundle is done by the Rust CLI only because WASI cannot set a file's executable bit; the format is plain bytes.

### 6.6 Native executables (Linux x86-64)

`aipl compile --exe FILE -o prog` on Linux x86-64 (or `--target native`) writes a native executable: an ELF file of machine code, statically linked, with no libc, launcher, or runtime (`aipl_src/native/`, designed in `docs/design/NATIVE_BACKEND_PLAN.md`). It is made from the same wasm module (`aipl_src/native/native.aipl` translates wasm to x86-64; the CLI runs it in its embedded wasmtime and sets the file's execute bit), so its behaviour is the wasm module's, and every test program is checked to match `aipl-run`'s output, error output, exit status, and files byte for byte (`tests/test_native.rs`).

- **Size and speed.** `word_count` is about 94 KB (250 KB without the optimizer); the AIPL compiler itself, with its native backend, about 3.9 MB (the launcher build is 18 MB). Start-up is a few milliseconds (no engine to load or compile). The translator is a single-pass compiler that tracks where each value is (a register, a constant, a local) and emits code only when a value is used (docs/design/NATIVE_SPEED_PLAN.md): the natively built compiler compiles itself in 0.46 s against 0.53 s under wasmtime (which includes Cranelift's compile), and the eight benchmarks run in 0.8 to 1.9 times wasmtime's time, 1.9 on nbody, whose float-heavy loop has more values than registers (docs/BENCHMARKS.md). Memory accesses are checked by guard pages: an access outside the memory faults, and the fault becomes wasm's out-of-bounds trap with the same message and call chain. A function returns one result in `rax`; several (a value struct, section 4.E) go through 8 slots in the thread's block after its stack limit, stored by the function just before it returns and loaded by the caller right after the call.
- **Same behaviour as section 6.5:** `_start`/`main`, exit statuses (0-125, otherwise an error with 134), traps with their call chain and status 134 (`./prog: wasm trap: ...` and `  at f` lines; WASI functions given a bad pointer report wasmtime's `Pointer out of bounds: Region { start: S, len: L }`), the command line, environment, stdio, fd 3 = working directory, fd 4 = `/` (not with `--sandbox`), threads (one OS thread each), clocks, and randomness.
- **File access.** Paths resolve beneath their directory descriptor with `openat2(RESOLVE_BENEATH)`, so the kernel refuses `..` escapes, absolute paths under fd 3, and symlinks leading out, as wasmtime's preopens do. File descriptors are numbered as under wasmtime (the lowest free). `openat2` needs Linux 5.6 or newer, and Rosetta 2 and some container profiles lack it (the call fails with ENOSYS, or with EPERM from a seccomp filter). A program built without `--sandbox` then opens with plain `openat`, with the same flags and mode: it is not confined to its directory, which costs it nothing, since `/` is its fd 4 and any absolute path opens anyway (it follows `..` and symlinks out where `aipl-run` refuses). A program built with `--sandbox` probes `openat2` at start and, without it, writes `this program was built with --sandbox, which needs the openat2 system call (Linux 5.6 or newer, and not Rosetta 2)` to standard error and exits 1. Everything else a native executable does works on any kernel since 3.17. Tested by refusing the call with a seccomp filter (`tools/without_openat2.py`, `tests/test_openat_fallback.rs`, and `tests/test_native.rs` run that way by CI); not yet run under Rosetta 2 itself.
- **Processes.** `aipl_sys_v1`'s `spawn`, `stream`, `read`, `write`, `close`, `wait`, and `exec` become Linux system calls (`aipl_src/native/procs.aipl`), with src/sys.rs's behaviour: the programs a program may start are fixed in the executable when it is compiled (`--allow-run`, at most 256 bytes each), a pipe's handle is its descriptor and a process's a slot of 64 (anything else is refused with `-8`), and a program that cannot start is told to the parent by a pipe that closes on exec, so a missing program is `-44` as it is under aipl-run.
- **Sockets.** `aipl_net_v1` becomes Linux system calls (`aipl_src/native/sockets.aipl`), with src/net.rs's behaviour: the grants are fixed in the executable when it is compiled (`--listen`), a handle is the Linux descriptor of a socket the program opened (anything else is refused with `-1`), and `send` uses `MSG_NOSIGNAL`.
- **Serving a fresh copy per connection** (`--serve ADDR:PORT`, `aipl_src/native/fresh.aipl`). Start-up listens on the address once linear memory is ready, before the program's start function and `_start`; for each connection it forks (`fork`, after `pipe2`), and the child takes the connection as its standard input and the pipe as its standard output, closes the host's other descriptors, and goes on with start-up, so the program runs from the beginning in a copy of memory that only its writes change. The host reads the pipe to its end (64 MiB kept), waits for the child, and sends its output if it exited with status 0, else aipl-run's plain 500; then shuts the connection down. The child's standard error is the host's, so a trap is logged there. One connection at a time.
- **Platforms.** Linux x86-64 only; elsewhere `--exe` writes the launcher bundle (section 6.5), and `--target native` is an error. `--target wasm` asks for the bundle on Linux too.

### 6.7 The optimizer (`aipl_src/codegen_opt.aipl`)

`aipl compile` builds through the optimizer unless `--no-opt` is given, for every output (`.wasm`, `--exe`, `--tests`, `--max-memory`). It is a source-to-source pass over the checked program after formats are lowered (docs/design/OPT_PLAN.md): constant folding, algebraic identities, and power-of-two strength reduction (`i32` and `i64` wrap and shift counts are masked, so these are exact); inlining of small functions (an argument that is not an atom, or a pure expression used once, is first bound to a `let` with a fresh name, in the call's order, so each argument is evaluated once, where the call evaluated it); propagation of immutable locals; and removal of the functions `main` cannot reach. Contracts are left as written, since a failed one prints its text. `AIPL_OPT_DUMP=FILE aipl compile ...` writes the optimizer's output text to FILE, for reading what it did.

- **Same behaviour.** An optimized program prints the same output, writes the same files, and exits with the same status as the unoptimized one, and a trap gives the same message. tools/opt_diff.py checks this in CI on every program in the repository and on 500 generated ones (tools/run_fuzz.py's).
- **What differs.** A function inlined into its caller has no frame of its own: a trap inside it is reported at the call, in the caller's frame (section 9). The data is smaller, so the heap starts lower and printed addresses (`(ptr T) @ N`, a failed contract's pointer arguments) differ from the VM's and from an unoptimized build's. The code is smaller and faster: spectralnorm runs in about 0.6 of the time, knucleotide 0.85, nothing slower (docs/design/OPT_PLAN.md).
- **Its output is checked.** What the optimizer makes is type-checked before code generation; a program that does not check is a compile error naming the optimizer and `--no-opt`, never wrong code. A failure of the optimized build on a program the checks passed is a toolchain bug; `--no-opt` builds without it.
- **The two commands agree.** The Rust command checks the program with its own front end (so every error reads as without the optimizer), then runs `driver.compile_file_opt` (`aipl_src/driver.aipl`), compiled to wasm, under wasmtime; the self-hosted command (`aipl_src/aipl.aipl`) calls it directly. Both write the same bytes, optimized or not; the bootstrap's stage 2 is an optimized build of the compiler, and stage 3 is identical to it.
- **The library.** What `aipl_src/` holds (the standard library and the toolchain's sources: the driver, the native backend, `aipl doc`) is built into the `aipl` binary (`build.rs`, `src/library.rs`). Run from its own checkout's `target/`, `aipl` reads that checkout's `aipl_src/`, so an edit there is seen at once; anywhere else (copied, installed, unpacked from a kit) it writes its built-in copy once to the cache directory below, as `library-HASH/`, and reads that, so the binary needs nothing beside it. `$AIPL_STD` replaces the standard library for the programs it compiles and runs, not for its own modules. tests/test_portable.rs runs a copied `aipl`.
- **Module cache.** The toolchain modules the Rust command runs under wasmtime (the driver for the optimizer, `native/native` for `--exe`) are compiled to machine code once and kept in `$AIPL_CACHE`, else `$XDG_CACHE_HOME/aipl`, else `~/.cache/aipl`, keyed by the Rust sources and every file below `aipl_src/`: a compile takes about 0.08 s, the first after a change about 0.5 s. An empty `AIPL_CACHE` turns the cache off; an entry wasmtime refuses is compiled again.

---

## 7. Typing and Evaluation Rules by Example

The checker is `src/checker.rs`; the VM is `src/vm.rs`. Both agree on these rules.

### 7.1 Typing and Scoping Rules (P7 Specification)

The checker (`src/checker.rs`) enforces these six rules; the VM (`src/vm.rs`), the Rust wasm backend (`src/compiler/wasm.rs`), and the self-hosted compiler (`aipl_src/codegen.aipl`, which assumes checked input and emits the same bytes as the Rust backend) implement the same semantics:

1. `set!` has type void.
2. `if` whose two branches are both void is void; otherwise both branches must have the same non-void type — an if mixing void and non-void is a type error with a message suggesting `(block ... value)`. A branch that always leaves, a `return`, `break`, or `continue` (or a block ending in one, or an `if` whose branches both do), takes the other branch's type: `(if (gt n 0) n (return -1))` is an `i32`. The same holds for the arms of `cond`, `match`, and `match_result`. In compiled code such a branch leaves the wasm stack to the jump, or, ending in an `if` whose branches both leave, is followed by `unreachable`.
3. `let` has type void (it declares, it does not yield); a function body's last expression must therefore be a value expression when the return type is non-void.
4. A function's parameters have different names, and `let` is block-scoped: a `let` inside if/while/loop/block/match arms is visible only within that construct; shadowing an outer name is a type error. A name's scope is its block or arm: a sibling scope (another block, another `match` or `match_result` arm) may declare it again with any type, as `(block (let y:f64 1.5)) (block (let y:i32 3))` and two arms each binding `v`. The code generators give each name one local, so the generics pass renames such a name `'K.y` (its K-th declaration in the function) with its uses (src/scopes.rs, generics.aipl scope_fn); a message names it as written.
5. `set!` on an undeclared name is a type error (and a VM runtime error); there are no implicit globals.
6. `match_result` binds its `ok` variable to the result's ok type and its `err` variable to the error type. `(ok v)` takes its ok type from `v` and defaults its error type to `i32`; `(ok:E v)` names the error type, and `(err:T e)` names the ok type (section 7.7).

| Form | Type | Example |
|---|---|---|
| integer literal | `i32` | `42`, `-7` |
| suffixed integer literal | `i64` | `42i64`, `-7i64`, `4294967296i64` |
| float literal | `f64` | `3.5` |
| float literal with `f32` | `f32` | `3.5f32` |
| `true` / `false` | `bool` | |
| `"text"` | `str` | |
| `(let x:T v)` | `void` (declares `x` as `T`; `v` must be `T`) | `(let n:i32 (+ 1 2))` |
| `(set! x v)` | `void` (`v` must match `x`; `x` must be declared) | `(set! n 5)` |
| `(if c a b)` | `void` if both `a`,`b` are `void`; else `T` where `typeof(a)==typeof(b)==T`; neither branch an empty `(block)` | |
| `(when c body*)` | `void`; the body's expressions are statements (a value is dropped) | |
| `(for i s e body*)` | `void`; `s e` must be `i32`; `i` is block-scoped to the body | |
| `(while c body*)` | `void` | |
| `(block e1 ... en)` | type of `en` (or `void` if empty); introduces a block scope | |
| `(call f args)` | declared return type of `f`; arity and arg types must match | |
| `(ok v)` / `(ok:T_err v)` | `ResultType<typeof v, T_err>` (default `T_err` = `i32`) | `(ok 42)` |
| `(err e)` / `(err:T_ok e)` | `ResultType<T_ok, typeof e>` (default `T_ok` = `i32`) | `(err -1)` |
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
| binary arithmetic / bitwise | type of the operands, which must be equal | `+ - * /` on numbers (`i32 i64 f32 f64`); `% divu remu ^ shl shr shru bitand bitor` on `i32`/`i64` only |
| comparisons | `bool`; operands must have equal type | `eq neq` on anything but a union; `lt lte gt gte` on numbers; `ltu lteu gtu gteu` on `i32`/`i64` only |
| `checked.add` / `checked.sub` / `checked.mul` | type of the operands, which must be equal | `i32` or `i64` only |

A function body is a sequence of expressions. The **last** expression's type must equal the declared return type unless the return type is `void`, in which case the last value is discarded.

### 7.2 Valid: value-producing `if` as the final expression

```lisp
(module ex_if
  (fn sign [n:i32] -> i32
    (if (lt n 0) -1
        (if (eq n 0) 0 1))))
```
`aipl eval` on a driver calling `(call sign -5)` prints `[AIPL Result]: Int(-1)`. Compiles to `if (result i32) ... else ... end` in wasm.

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
`(ok x)` defaults its error type to `i32` and `(err e)` defaults its ok type to `i32`, so both branches of the `if` are `(result i32 i32)`. Write `(ok:T v)` / `(err:T e)` to name the other side explicitly, e.g. `(ok:bool 1)` is `(result i32 bool)`. Both arms of `match_result` must end in the same type. This compiles and runs identically in both backends.

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

There is one layout and one allocator, shared by the VM, compiled wasm, and AIPL code such as `memory.aipl` and `codegen.aipl`. Bytes `0..1024` are the **runtime block**; the heap begins at `1024` and grows upward through `mem.alloc`.

| Address | Width | Owner | Meaning |
|---|---|---|---|
| 0 | i32 | allocator | heap cursor: the next address `mem.alloc` will return. Initialised to `1024` by `VM::new` and by the wasm data segment. |
| 4 | i32 | codegen | the first compile error's code (`0` = none; section 6.4), read by the host |
| 8..12 | | unused | |
| 16 | i32 | codegen | pointer to the keyword map, built once by `codegen_init` and reused by later compiles |
| 20 | | unused | |
| 24 | i32 | std/testing | the failures in the test running (section 4.L) |
| 28 | i32 | std/os | pointer to the command line's table, read by the first `os.arg` or `os.arg_count` (0 until then) |
| 32 | i32 | std/os | pointer to the environment's table, read by the first `os.env` (0 until then) |
| 36..40 | | unused | (the rest of a compile's state is in the `Cg` struct `compile_module` creates) |
| 44, 48 | i32, i32 | codegen | source offset and length of the callee name, after compile error 1452, read by the host |
| 52 | i32 | std/alloc | pointer to the shared default allocator, made by the first `alloc.default` (0 until then) |
| 56 | i32 | std/gen | the counter fresh generations come from (GENERATIONS_PLAN.md) |
| 60 | i32 | codegen | pointer to the module `compile_module` emitted, read by the host |
| 64, 68 | i32, i32 | WASI runtime | iovec 0: buffer pointer, length (used by `sys.print`, `fs.read`, `fs.write`) |
| 72, 76 | i32, i32 | WASI runtime | iovec 1: the interned `"\n"` and length 1 (`sys.print`) |
| 80 | i32 | WASI runtime | `nwritten` / `nread` out-parameter |
| 84 | i32 | WASI runtime | `path_open`'s opened-fd out-parameter |
| 88 | i32 | threaded modules | 1 once the shared memory has been initialised (section 6.2) |
| 92, 96 | i32, i32 | compiled checks | address and length of a failed check's message (`Array index out of bounds: ...`, or a contract's `Pre-condition failed in ...`, written at the heap cursor), set just before the `unreachable` that ends the program; `aipl-run` and native executables print it instead of the trap (0 = none) |
| 128..256 | | compiled checks | where a failed bounds check writes its message |
| 88..1024 | | reserved | not handed out by `mem.alloc`; do not use |
| 1024..heap start | | string literals | interned string literals, `[len u32 LE][bytes]` each, in first-use order; a `str` value points at the bytes. Empty for a module without literals. Read-only |
| heap start.. | | heap | `mem.alloc` region. The heap start is the first 8-aligned address after the literals (1024 without literals); it is the initial value of the cursor at address 0 |

The runtime's own writes into 64-87 are emitted directly and bypass the store guard described below; user code still cannot store anywhere from 64 up to the heap start.

Rules that follow from this:

- **Never write to a literal address.** Take memory from `mem.alloc` and pass pointers around. Every module in `aipl_src/` and every test does this; the only fixed addresses in `codegen.aipl` are this layout's own (cells below 64, and the string data at 1024 that it compiles into modules).
- **The checker enforces the block for literal addresses.** Any `mem.*` or `atomic.*` op whose address is a literal is rejected at check time if it stores to or locks bytes 0-3 (`... bytes 0-3 are the heap cursor owned by mem.alloc ...`), touches bytes 64-1023 (`... bytes 64-1023 are the reserved runtime block ...`), or names a misaligned cell in 4-63. Reading the cursor with `(mem.load32 0)` and using the aligned cells 4-60 is allowed; that is what `memory.aipl` and `codegen.aipl` do. Literal heap addresses (1024 and up) are allowed but discouraged.
- **Both backends enforce the block for writes at runtime.** Every `mem.store*`, `put`, `arr.set`, and `atomic.*` op checks its address before writing: bytes 0-3, 64-1023, and the string literals (1024 up to the heap start) are refused however the address was computed. A store into a literal fails in the VM with `... bytes 1024-N are the program's string literals, which are read-only ...`. The VM fails with `mem.store32 at address 512: bytes 64-1023 are the reserved runtime block; take memory from (mem.alloc n) instead`; compiled wasm traps with `unreachable` on exactly the same addresses (the backend emits two compare-and-trap checks before each store, using the first of the two scratch `i32` locals each function has). This is a shared semantic, not a VM-only guard, so the differential test treats it as agreement. The self-hosted compiler (`codegen.aipl`, `emit_store_guard`) emits the identical bytes, and `tests/test_selfhost.rs` checks that equality directly. Reads are not checked: the block is zero and reading it is harmless.
- **The VM additionally enforces lock validity.** A lock word is only ever `0` (free) or `1` (held). `atomic.lock` on a word holding anything else fails immediately with `atomic.lock: word at ptr N holds V, which is not a lock state ...` instead of spinning forever, and `atomic.unlock` on a word that is not `1` fails with `... a held lock holds 1 ...`. This is what turns "I locked the heap cursor by accident" from a silent hang into an error, whichever way the address was produced.
- **Fresh instances agree.** A fresh VM and a fresh wasm instance both return the heap start from the first `mem.alloc` (1024 for a module without string literals), then that plus `size` rounded up to a multiple of 8, and so on: the VM lays out the first loaded module's literals exactly as the wasm backend does (`wasm::string_layout`). This is why memory-heavy programs can be compared across backends (section 10.4).
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

`return`, `break`, and `continue` are statements of type `void`. They go where a statement goes: in a body, a `block`, a `cond` clause, or an `if`. An early exit is written `(when (lt i 0) (return -1))`; where the `if` gives a value, a branch that always returns, breaks, or continues fits any type (section 7.1), so `(let x:i32 (if (lt i 0) (return -1) i))` is fine too (checked in the VM, wasm, and native, 2026-10-21). A function body may end in `(return v)` instead of a bare `v`. `break` and `continue` outside a loop body (including a `while` condition) and `return` inside a contract are checker errors.

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

Lowering (wasm and the self-hosted compiler): `return` is the `return` instruction, or in a function with an `ens` a branch to the end of the body's block, where the `ens` checks run (section 6.2). Loops are `block { loop { ... } }`; `break` branches to the outer block. A `while` body's `continue` branches to the loop header. A `loop` wraps its body in one more block, and `continue` branches to that block's end, which falls into the step. `(and a b)` is `a; if (result i32) b else i32.const 0 end` and `(or a b)` is `a; if (result i32) i32.const 1 else b end`. Branch depths count the enclosing `if`s (including those of `and`/`or`) and `match_result` arms.

A jump inside an operand (`(+ x (block (when c (break)) 1))`, legal because a `block` may end in a value) leaves the whole expression at once in both backends: operands after it are not evaluated.

### 7.11 Recursion depth

Calls nest as deep as the stack allows. Running out of stack stops the program with a message in every backend, never a crash:

| Backend | Room | Nested calls of a small function | Failure |
|---|---|---|---|
| `aipl-run` (wasm) | wasmtime's 512 KiB wasm stack | about 32,000 | `prog.wasm: wasm trap: call stack exhausted`, the call chain, exit 134 |
| native executable | 8 MiB per thread, the main thread's included (its own stack, so `ulimit -s` does not matter), below a guard page | about 170,000 | the same, with the program's name |
| VM | 192 MiB of the thread's stack | about 40,000 | `Call stack exhausted calling 'f': calls are nested too deeply (a recursion that does not end?)` |

A call's room is its frame: a function with many locals or deeply nested expressions gets fewer calls. Code meant to run under `aipl-run` should stay well below 30,000 nested calls; walk long lists and deep structures with `while`. In a native executable, each function's prologue compares the stack pointer, less the frame it is about to push, with a limit 256 KiB above the stack's low end and traps below it, which leaves room for the host routines and the trap's own output.

---

## 8. Integer Semantics

`i32` is a wrapping two's-complement 32-bit integer in **both** backends; the wasm instruction set defines the behaviour and the VM must match it bit for bit. Internally the VM stores integers in an `i64` but truncates every operand to `i32` before each operation, so no value wider than 32 bits is observable.

| Expression | Result | Why |
|---|---|---|
| `(+ 2147483647 1)` | `-2147483648` | wraps |
| `(* 65536 65536)` | `0` | wraps |
| `(- 0 -2147483648)` | `-2147483648` | wraps |
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

### 8.1 `i64`

`i64` is a first-class declarable type with the same wrapping two's-complement semantics at 64 bits. There is **no implicit widening**: an `i32` and an `i64` never meet in one operation, and the checker rejects `(+ 1 2i64)` with `Type mismatch in binary op: i32 vs i64`. Conversions are explicit ops that map one-to-one onto wasm instructions.

| Form | Meaning | wasm |
|---|---|---|
| `42i64`, `-7i64`, `4294967296i64` | 64-bit literal; the suffix is part of the token | `i64.const` |
| `(let x:i64 0i64)`, `[a:i64]`, `-> i64` | declarations | local/param/result `i64` |
| `+ - * / % divu remu ^ bitand bitor shl shr shru` on two `i64` | 64-bit wrapping; shift counts masked to 6 bits | `i64.add` ... `i64.shr_u` |
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
| `(+ 9223372036854775807i64 1i64)` | `-9223372036854775808` |
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

`+ - *` wrap. `checked.add`, `checked.sub`, and `checked.mul` compute the same result when it fits in the operands' type and stop the program when it does not: the VM fails with `Integer overflow in checked.add` (`.sub`, `.mul`), and compiled code traps with the host's integer-overflow trap. Use them where a wrapped result would be a wrong answer rather than an intended one (money, sizes, counters).

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

The wasm lowering (`compile_checked` in `src/compiler/wasm.rs`, `codegen.aipl` alike): for `i32`, both operands are widened to `i64`, the exact result is computed there, and it traps unless it lies in `[-2^31, 2^31)`; for `i64`, addition and subtraction test the sign bits (`(a ^ r) & (b ^ r) < 0` for `+`, `(a ^ b) & (a ^ r) < 0` for `-`) and multiplication checks `r / a == b` when `a` is not 0 (and the `-1 * MIN` case through the same division). The trap is an `i32.div_s` of `MIN` by `-1`, so every host reports it as integer overflow. `tests/test_differential.rs` runs every pair of 12 edge values per width through the VM and wasmtime and compares both with Rust's `checked_*`.

### 8.3 Type-directed code generation

The wasm backend selects instructions from the static operand type (`i32` / `i64` / `f32` / `f64`), and an `if` whose branches are `i64` or `f64` gets a matching block result type. `f64` arithmetic (`+ - * /`) and comparisons therefore compile and validate. `%`, `divu`, `remu`, shifts, and bitwise ops on floats are rejected at compile time with `Wasm Codegen: <op> is not supported for operands of type f64`.

**Status:** the wrapping rules above for both widths are implemented in `src/vm.rs`, covered by `tests/test_i64.rs` (VM result plus wasm validation), and proven equal across backends by `tests/test_differential.rs`, which executes every case in this section in both the VM and wasmtime and asserts identical results. When the two disagree, wasm is right and the VM is fixed (section 10.4).

---

## 9. Diagnostics

Every parser and checker error is a single line of the form

```
<line>:<col>: <message>
```

with 1-based line and column of the offending token or the opening `(` of the offending form. Every error found before run time is prefixed with the path of the file it is in (`examples/x.aipl: 3:5: Unknown op/keyword: badop`), whether the resolver, the parser, or the checker finds it, and in both toolchains (section 6.4). An error inside a generic instance names the template's file, or the file that wrote the type argument when the error is at that argument. A syntax error, or an error in a definition (struct, enum, union, constant, function signature), is reported alone, since everything after depends on it. Otherwise every function body is checked, and each failing function reports its first error, one per line in source order; both toolchains give the same lines. A runtime error prints its call chain with the source position of each frame (`  at f (math.aipl:4:5)`): a compiled trap under `aipl-run` and natively (section 6.5), and an error in the VM (`aipl eval`, `aipl test`), whose chain is the same, frame for frame; only the message's wording differs (`Division by zero` against wasmtime's `integer divide by zero`). A VM contract failure also gives the contract's position in its message (section 7.6). A module parsed from one text (not through the resolver) names no file: `  at f (3:5)`.

Representative messages, exactly as produced:

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
| wrong argument type | `4:5: Arg 1 of 'add' expects i32, got bool` |
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

## 10. Testing Conventions

There are three layers of tests. Each one must fail loudly on fabricated work; a test that only asserts "returned a number" or "produced more than N bytes" is not acceptable in this repository (see `docs/history/AUDIT_2026-10.md`, item U6).

### 10.1 AIPL-native self-tests (`aipl test`)

Each real module in `aipl_src/` exposes a zero-arg `run_<module>_tests` function that returns an `i32` count of passing sub-tests (or `1` for a single pass / `0` for fail). The suite entry point `aipl_src/test_suite.aipl` imports every verified module, calls each runner, compares against the expected count, prints one `[PASS]`/`[FAIL]` line per group with `sys.print`, and returns the number of failing groups. The Rust `test` subcommand turns that into the exit code.

```lisp
;; aipl_src/test_suite.aipl (abridged)
(module test_suite
  (import compiler)
  (import memory)
  (import io)

  (fn report [label:str result:i32 expected:i32] -> i32
    (if (eq result expected)
        (block (call io.write_str 1 "[PASS] ") (call io.println label) 0)
        (block (call io.write_str 1 "[FAIL] ") (call io.println label) 1)))

  (fn run_all [] -> i32
    (let failures:i32 0)
    (set! failures (+ failures (call report "compiler: tokenizer (2 tests)" (call compiler.run_tokenizer_tests) 2)))
    (set! failures (+ failures (call report "memory: allocator + arena"     (call memory.run_memory_tests)     1)))
    failures))
```

Pattern for a module's own runner: perform real work, then assert on the **content** of the result, not its shape.

```lisp
;; aipl_src/memory.aipl
(fn run_memory_tests [] -> i32
  (let p1:i32 (call aipl_heap_alloc 64))
  (let p2:i32 (call aipl_heap_alloc 128))
  (let arena:i32 (call arena_create 1024))
  (let a1:i32 (call arena_alloc arena 32))
  (let a2:i32 (call arena_alloc arena 64))
  (let _r:i32 (call arena_reset arena))
  (let a3:i32 (call arena_alloc arena 32))
  (if (and (gt p2 p1) (and (gt a2 a1) (eq a3 a1))) 1 0))
```

Expected terminal output (30 groups; abridged):
```
[AIPL Test] Running 'run_all' from 'aipl_src/test_suite.aipl'...

[PASS] compiler: tokenizer (2 tests)
[PASS] compiler: parser (2 tests)
[PASS] codegen: signatures + 3 real wasm modules (4 tests)
...
[PASS] std/bigint: arbitrary-precision integers (13 tests)
...
[PASS] consts: constants and enums erased for codegen (5 tests)
[PASS] thread_sync: 4 threads x 1000 atomic adds = 4000

[AIPL Test] All groups passed.
```

`aipl test` runs the suite in the VM; it also compiles to wasm.

Rules: import a module into `test_suite.aipl` only once its runner is verified to do real work. `thread_sync.aipl` is imported like every other module: `thread.spawn` takes a function reference, which survives the resolver's renaming (P10).

### 10.2 Rust integration tests (`cargo test`, `tests/*.rs`)

Rust tests drive the library crate `aipl_core` directly and are the only way to call AIPL functions with arguments. The canonical shape exercises **every** stage and asserts concrete values:

```rust
use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::parser::Parser;
use aipl_core::vm::{Value, VM};
use wasmparser::Validator;

#[test]
fn fib_runs_and_compiles() {
    let src = r#"
    (module compute
      (fn fib [n:i32] -> i32
        (if (lte n 1) n
            (+ (call fib (- n 1)) (call fib (- n 2))))))
    "#;
    let module = Parser::parse(src).expect("parse");
    TypeChecker::new().check_module(&module).expect("check");

    let mut vm = VM::new();
    vm.load_module(module.clone());
    assert_eq!(vm.invoke("fib", vec![Value::Int(7)]).unwrap(), Value::Int(13));

    let wasm = WasmCompiler::compile(&module).expect("compile");
    assert!(wasm.starts_with(&[0x00, 0x61, 0x73, 0x6D]));
    Validator::new().validate_all(&wasm).expect("wasm must validate");
}
```

To assert a diagnostic, match on the exact `L:C:` prefix so a regression in position tracking fails the test:

```rust
let err = Parser::parse("(module m\n  (fn f [] -> i32 42)").unwrap_err();
assert!(err.starts_with("2:21:"), "got {err}");
```

To assert that a backend **rejects** something rather than faking it:

```rust
let module = Parser::parse("(module m (fn f [] -> void (sys.print 5)))").unwrap();
let err = WasmCompiler::compile(&module).unwrap_err();
assert!(err.contains("sys.print supports str arguments only in the wasm backend"));
```

Files today (65 files, 485 tests as of 2026-10-29): `tests/test_all.rs` (pipeline smoke), `tests/test_v2.rs` (memory, atomics across real threads, real file I/O, results, imports), `tests/test_diagnostics.rs` (exact `L:C:` prefixes), `tests/test_i64.rs` (64-bit type, VM plus wasm validation), `tests/test_memory_layout.rs` (reserved-block enforcement in both backends), `tests/test_opcode_conformance.rs` (10.3), `tests/test_differential.rs` (10.4), `tests/test_wasi.rs` (10.5), `tests/test_selfhost.rs` (10.6), `tests/test_doc_examples.rs` (10.7), `tests/test_pointers.rs` (strict pointer/array typing, VM/wasm agreement, struct namespacing across imports), `tests/test_std.rs` (every eligible standard-library function in both backends under WASI, plus exact printed output), `tests/test_printer.rs` (source round trip of every repository program), `tests/test_refs.rs` (function references in both backends, signature checks, refs across imports), `tests/test_control_flow.rs` (return/break/continue/cond, short-circuit `and`/`or`, loop bounds evaluated once, in both backends; checker rejections), `tests/test_threads.rs` (threaded modules under a wasi-threads host against the VM), `tests/test_generics.rs` (template expansion in both backends, Rust/AIPL parity, errors), `tests/test_visibility.rs` (what a module gives and keeps, both resolvers, message for message), `tests/test_interfaces.rs` (interfaces, implementations, where clauses, and templates checked once, in both backends and both toolchains, message for message), `tests/test_resolver_aipl.rs` (the AIPL resolver against the Rust one, subdirectory imports), `tests/test_runner.rs` (the `aipl-run` launcher and `aipl compile --exe` executables), `tests/test_benchmarks.rs` (every program in `benchmarks/`, as wasm and natively, against its `expected.txt`), `tests/test_consts_enums.rs` (constants and enums in the VM, wasm, and natively, and every checker rule), `tests/test_sum_types.rs` (unions and `match` in every backend, and the checker's rules), `tests/test_values.rs` (value structs, and unions, results, and `Option` by value, in every backend and both toolchains, with every checker rule), `tests/test_checker_aipl.rs` (the AIPL tokenizer, parser, printer, and checker against the Rust ones, message for message, run compiled to wasm), `tests/test_heap.rs` (`std/heap`'s failure checks: the same message from the VM, `aipl-run`, and native executables), `tests/test_par.rs` (`examples/spectralnorm_par.aipl`, through `std/par`, prints the sequential benchmark's result on 1, 3, and 8 threads in the VM, under aipl-run, and natively), `tests/test_generations.rs` (use after free through a copy, after reuse, of an array, after an arena or fast arena reset, and through an allocator, and versioned loops failing exactly where unversioned ones would, with the same output and call chain in the VM, under aipl-run, and natively), `tests/test_language_gaps.rs` (`f32` values, conversions, and traps at wasm's bounds, `f32` literal rounding in both toolchains, 8- and 16-bit storage types, `inv` at entry and exit, and constant expressions, each in the VM, under aipl-run, and natively, with the self-hosted toolchain's bytes the Rust one's), `tests/test_formats.rs` (the formatting forms alike in the VM, under aipl-run, and natively, and in both toolchains' bytes; every formatting error, message for message; `std/show` against Python on generated values and formats), `tests/test_early_returns.rs` (branches that return, break, or continue in value position; `try`, `try_opt`, `try_or`, and `defer` in every backend and both toolchains, with every checker rule), `tests/test_text.rs` (`std/text` and `std/str`'s search, split, lines, replace, and trim against Python on generated inputs, in the VM, under aipl-run, and natively, and in both toolchains' bytes), `tests/test_stream.rs` (a program reading standard input a line at a time and writing through a buffered writer prints what Rust's `str::lines` gives, in the VM, under aipl-run, and natively, and in both toolchains' bytes; `format_to` on a writer that is not a variable, one message in both), `tests/test_doc.rs` (`aipl doc` on a module with every kind of item, one item, a search, and errors; every public item of the standard library documented), `tests/test_eval_expr.rs` (`aipl eval -e`: values with their types in the VM, under aipl-run, and natively, `--with` a file's private functions, standard modules imported, errors at `-e:LINE:COL`), `tests/test_dbg.rs` (`(dbg e)` for every kind of value, in an imported module, a generic, nested, and in a `try`, alike in the VM, under aipl-run, and natively, and in both toolchains' bytes; every mistake one message in both), `tests/test_testing.rs` (`aipl test` on a file of passing, failing, and trapping tests, alike under aipl-run, in the VM, natively, and with `--all-backends`; `-k`; ordinary builds leave tests out; the self-hosted `aipl compile --tests` gives the Rust toolchain's bytes and each mistake's message), `tests/test_watch.rs` (`aipl watch` running a file again when a file it imports changes, and a file's tests with `--test`), `tests/test_sugar.rs` (`when`, `for`, and variadic `and`/`or` alike in every backend and both toolchains' bytes, with each mistake one message in both and a test's body read only in a test build), `tests/test_csv.rs` (std/csv against Python's csv on generated texts and rows, and its API, in every backend), `tests/test_openat_fallback.rs` (native executables with the openat2 system call refused: the fallback to openat, and the --sandbox refusal), `tests/test_math.rs` (std/math against Python on generated cases, in every backend, and what stops the program), `tests/test_stats.rs` (std/stats against Python's statistics module to the bit on generated data, and its other functions, in every backend), `tests/test_libs.rs` (std/set, std/random and std/uuid, std/path against Python's posixpath, and std/cli with its usage and exit statuses, in every backend), `tests/test_encode.rs` (std/encode against Python's base64, binascii, and urllib.parse on generated bytes and near-miss texts, and its API, in every backend), `tests/test_regex.rs` (std/regex against Python's re on 1,500 generated patterns and texts, and its API, including a pattern exponential for backtracking, in every backend), `tests/test_memory.rs` (a view of a freed buffer, of a buffer that grew, and a reader's line after close each stop the program with "Use after free" in every backend; a memory reader's line outlives it; `(no_leak)`'s refusals with their chains, a `(no_leak)` handler on a request arena run 10,000 times with memory flat, and one reading and writing through four kinds of stream; `--max-memory` stopping a program at the same place in the VM, under aipl-run, natively, and in the bundle, both toolchains' module alike; os.arg and os.env reading their tables once; a `(no_leak)` handler over std/url, std/json, and std/text run 10,000 times with memory flat), `tests/test_web.rs` (std/url against Python's urlsplit, parse_qsl, and urlencode and RFC 3986's resolution, its examples and its algorithm on generated references (Python's urljoin differs from the RFC on 32 of 300: it keeps dot segments in a network-path reference, drops empty segments, and returns the base whole for an empty reference); std/html against Python's html module; std/http's reading, refusals, limits, and writing; json.get_path; std/ip against Rust's IpAddr and SocketAddr parsers on 8,000 generated texts; in every backend), `tests/test_exit.rs` (main's i32, void, and (result i32 str), and sys.exit, give the same status, output, and error output in the VM, under aipl-run, natively, and in the launcher bundle, a status outside 0-125 an error with 134 everywhere; both toolchains' bytes alike), `tests/test_serve.rs` (std/serve answering real requests, refusals included, with the same bytes from the VM, aipl-run, a native executable, and the launcher bundle; 100,000 requests natively and under aipl-run (a million with AIPL_MILLION=1), and 20,000 in the VM, with the heap where it was after the first thousand; a view kept past its request stopping the program with "Use after free" in every backend; serve.once answering saved requests piped through it with the same bytes in the VM, under aipl-run, and natively; `aipl serve --fresh`, a native executable built with `--serve`, and the bundle built with it each answering 500 for a handler that traps and for an answer over 64 MiB, and serving the next request; an address in use refused with the same message by each, and a threaded program refused when compiled; both toolchains' bytes alike), `tests/test_sys.rs` (`fs.chmod`: the same results and the same files' modes from the VM, aipl-run, a native executable, and the launcher bundle, with and without `--sandbox`, and from a native executable where `openat2` is refused; the process operations: a pipe each way through `cat`, exit statuses and a signal, output and error through one pipe, arguments and environment, every refusal, `exec` taking over the program with its status, and the grants of `--allow-run` (none, one, two, `*`), and `std/proc` over them (running a program, a conversation through pipes, the error on its own pipe, the PATH lookup, `exec`), `std/fs`'s `chmod`, `rename`, and `remove` (their errors as `io.Error`s, the modes and files they leave), and `fs.rename` (a file replaced, a directory and a link moved, the errors of each mismatch, what the working directory and `--sandbox` confine, and from a native executable where `openat2` is refused), and `fs.rmdir` (an empty directory removed, the errors for one that is full, a file, a link, or a missing path, the same confinement and fallback), and `fs.mkdir` (a directory made with the mode the umask leaves, the errors for a path that exists, a missing parent, or a file as parent, the same confinement and fallback), alike from the VM, aipl-run, a native executable, and the launcher bundle; both toolchains' bytes alike), `tests/test_net.rs` (a server whose `(no_leak)` handler serves four connections over loopback, IPv4 and IPv6, with the same bytes from the VM, aipl-run, a native executable, and the launcher bundle, its arena flat; a listen without a grant, or with another one, refused everywhere; both toolchains' bytes alike), `tests/test_deflate.rs` (std/deflate against Python's zlib and gzip both ways: Python's streams of every level and strategy, whole, with a bit flipped, and cut short, and ours checked by Python; and its API, in every backend), `tests/test_hash.rs` (std/hash against Python's hashlib, hmac, and zlib on every length up to a few blocks, NIST's and RFC 4231's cases, and its API, in every backend), `tests/test_io.rs` (a failed read or write gives an `io.Error` of the same kind in the VM, under aipl-run, and natively, from the one WASI errno table; both toolchains' bytes alike), `tests/test_scopes.rs` (a name declared again in a sibling block, arm, loop, or clause, with another type, and in a template, in every backend and both toolchains' bytes; shadowing a name still visible, one message in both), `tests/test_showcase.rs` (examples/jsonpp.aipl, the JSON pretty-printer: its own tests, forty generated documents laid out as Python's `json.dumps` lays them out, as wasm and as an executable, and every option and failure message), `tests/test_portable.rs` (a copied `aipl` running every command from the library built into it; `AIPL_STD` for programs only; the self-hosted `aipl` from a `bin/` and `share/aipl/std/` layout giving the Rust command's bytes), `tests/test_opt.rs` (a trap in an inlined function reported at its call under aipl-run and natively, `--no-opt` keeping its frame; the module cache surviving a damaged entry and turned off), `tests/test_bootstrap.rs` (the checked-in seed builds the compiler, and its build is byte for byte the Rust toolchain's; a compiler replaces the file it is running from and leaves no temporary file, a failed write included; the AIPL `aipl` command against the Rust one, on every example, benchmark, and program below tests/aipl), and the native backend's `tests/test_native_reader.rs` (the wasm reader against wasmparser), `tests/test_native_x64.rs` (the encoder against GNU as), `tests/test_native_elf.rs` (runs the first native executable), and `tests/test_native.rs` (the native backend's harness: every program built as wasm and natively, run under `aipl-run` and directly, with identical output, error output, exit status, and files; the AIPL compiler built natively and reproducing itself). `tests/test_differential.rs` also checks that allocation grows memory to the same page count in both backends.

### 10.3 Opcode conformance contract

`tests/test_opcode_conformance.rs` holds a minimal program for every `OpCode` variant. The `match` over `OpCode` has no wildcard arm, so adding a variant to `src/ast.rs` without a program is a compile error. For each variant the test asserts exactly one of:

- **(a)** the VM returns `Ok`, the wasm backend returns bytes, and `wasmparser` validates them; or
- **(b)** the checker, the VM, or the wasm backend returns `Err`.

An op that "succeeds" by returning a default value in one backend and a no-op in the other satisfies neither and fails the suite. New opcodes must be added to this file in the same commit that adds them to the AST.

### 10.4 Differential testing (`tests/test_differential.rs`)

This is what makes "wasm semantics are the spec" enforceable. `wasmtime` is a dev-dependency; the harness compiles a module with `WasmCompiler`, instantiates the bytes in wasmtime (no imports are needed), and calls the export. `differential(module, wasm, fn_name, args)` runs the same call in a fresh `VM` and a fresh wasmtime instance and asserts one of:

- both return the same value (`bool` is normalised to `Int(0|1)`, its wasm shape), or
- both fail (VM `Err` and wasmtime trap, e.g. division by zero).

Contract and bounds failures must happen in both: the compiled message (read from cells 92 and 96) must be the VM's, without the position and the values compiled code cannot show for a contract. Any other mismatch panics with `DIVERGENCE ... (fix src/vm.rs)`. The wasm side is never changed to match the interpreter.

Coverage:

- Every edge case in section 8 and 8.1 as a single-expression program.
- Control flow: `for` bounds (the end left out), negative-start loops, `while` with a stepped counter, `while` with `set!`, nested `if`/`block` values, recursion, memory round trips through the bump allocator.
- Typing and scoping (P7): one case per rule.
- Structs and arrays (P8): `i32`, `bool`, `i64`, `f32`, `f64` fields at their aligned offsets; `sizeof` with padding; `i32` and `i64` arrays including the length header and cursor advance; a size expression that itself allocates; the heap addresses left by `ok`/`err` cells and by an allocation inside a result payload; negative `arr.new` sizes and `put`/`arr.set` into the reserved block failing in both; out-of-range indexes failing in both with the same message; and the section 4.E example.
- Value structs held inline (docs/design/VALUES_PLAN.md): their sizes and offsets, raw words written through a field path, that results and unions holding one take no heap, array element spacing, and reads through a null pointer or past an array's end and writes into the reserved block failing in both.
- Every `examples/*.aipl`: resolved, checked, compiled; every function returning `i32`/`i64`/`bool` with all-`i32` params is called over nine fixed argument tuples in both backends. Files the wasm backend rejects are skipped with a printed reason, and files that do I/O (`word_count`, `word_freq`) are compared with captured output in `tests/test_wasi.rs` instead. Every example must parse (the stale-file list is empty). The test asserts at least 4 files and 40 calls were compared so it cannot silently go vacuous; today it compares `accounts`, `math_core`, `matrix_mult`, and `quicksort` over 116 calls. Every example is also compiled by the self-hosted compiler at byte parity (`tests/test_selfhost.rs`).
- `aipl_src/codegen.aipl`'s `test_signatures_and_locals` is compared under WASI (the module uses `fs.*` and so imports WASI).

Sample argument values are deliberately small. Example functions use parameters as loop bounds, and a tree-walking VM asked to iterate `i32::MAX` times is not a test, it is a hang. Wrap-around is covered by the explicit expression cases instead.

### 10.5 I/O under WASI (`tests/test_wasi.rs`)

Compiled modules that print or touch files import from `wasi_snapshot_preview1`, so they need a WASI host. The tests build one with `wasmtime-wasi`: a `WasiCtxBuilder` with stdout captured into a `MemoryOutputPipe`, stderr inherited, and a fresh temporary directory preopened as `.` with read-write permission, linked through `wasmtime_wasi::p1::add_to_linker_sync`. The VM side of each comparison runs with the process cwd switched to its own temporary directory (under a lock, since cwd is process-global) so both backends see an empty directory.

Covered: `aipl_src/file_io.aipl`'s `run_file_io_tests` returns 1 in both backends and leaves no file behind; `sys.print` output is exactly one line per argument; interned string literals report their length and compare by identity; 600 bytes of literals compile and run in both backends, and literals past the 1 MiB limit fail to compile with a message; opening a missing file returns -1 in both; a write-then-read round trip agrees byte for byte and the wasm side's file is visible on the host in the preopened directory; `sys.exit 7` surfaces as `I32Exit(7)` from wasmtime and as `sys.exit(7) requested` from the VM; and a module with no I/O has no import section and still instantiates with no imports.

To run compiled I/O outside the tests: `aipl run module.wasm` (the working directory is preopened), or build an executable with `aipl compile --exe`.

### 10.6 Self-hosted byte parity (`tests/test_selfhost.rs`)

`self_host(src)` runs `codegen.compile_module` (codegen.aipl compiled to wasm by the Rust toolchain, under wasmtime, a fresh instance per program; in the VM until 2026-10-20, a hundred times slower), validates the output with `wasmparser`, and returns the bytes or the compile error code. `assert_self_hosted_matches_rust` asserts the whole module equals `WasmCompiler::compile` for the same source and prints the first differing function and byte if not. Coverage: minimal, `add`, `i64` (literals at both extremes, unsigned ops, an `i64` struct field after a `bool`, `i64` arrays, `mem.load64`/`store64`), `f32`/`f64` parameters, arithmetic and struct fields, 524 float literals (200 random short ones, 300 random ones with up to 40 digits and exponents to ±330, and hard cases: halfway points, subnormals, infinities), `(ok:T v)` with compound `T`, an escaped quote inside a string, `compute` (loop + call), structs with `bool`/`str` fields, arrays, `sys.print`, results (including `match_result` as a statement), file I/O, repeated string literals with a `str` let plus `mem.alloc`/`mem.grow`/`sys.exit`, a mixed program, void `if` with block-scoped `let`s and an empty `(block)` else, a store, arrays plus results, `aipl_src/memory.aipl`, `aipl_src/compiler.aipl`, and codegen.aipl compiling itself (under wasmtime and, the longest of these tests, in the VM too: the one test that keeps the VM running the whole compiler). Behavioural checks run the self-hosted output in wasmtime: the store guard traps on bytes 0-3 and 64-1023 and nowhere else in a module without literals, and also on the literals in a module with 800 bytes of them, whose heap then starts at 1832 in both the VM and wasm; arrays and results compute the same values as the VM and a negative `arr.new` traps; a file write plus `sys.print` under WASI produces the file and the exact stdout. Compile errors 95, 96, and 768 are asserted for a result-typed field, unknown structs, and a 70 000-byte literal (over the self-hosted 64 KiB buffer).

**Bootstrap fixpoint.** `self_hosted_compiler_reproduces_itself_under_wasmtime` compiles the self-hosted compiler with the Rust backend (stage 1), runs that wasm module under wasmtime on its own source, and requires the output (stage 2) to be byte-identical to stage 1. The second compile involves neither the VM nor any Rust compiler code, and takes about 20 ms. Since P9, codegen.aipl itself uses `i64` and `f64` arithmetic (literal parsing, LEB128, float literal bits), so the fixpoint also covers those paths of the self-hosted compiler.

### 10.7 Documentation examples (`tests/test_doc_examples.rs`)

Every ```` ```lisp ```` block in `PROMPT_GUIDE_FOR_AIS.md`, `README.md`, and this specification is checked. A complete module must parse, check, and compile, and if it defines `main`, `main` must return the value listed in the test's `EXPECTED` table in both the VM and wasmtime (with WASI and a scratch directory); the few spec modules that cannot run that way are listed with a reason in `SPEC_SPECIAL`. A fragment of this specification (a few definitions without `(module`) is wrapped in a module and type-checked; the deliberately invalid example in 7.3 must fail with its documented message. Adding an example with a `main` means adding its expected value there. This test found that a `match_result` used as a statement compiled to invalid wasm.

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
- Resolution is implemented twice: `src/resolver.rs` (used by every `aipl` command) and `aipl_src/resolver.aipl` (used by `aipl compile --self` and by the self-hosted toolchain compiled to wasm). The AIPL resolver produces flat source text: every struct, then every function, imported modules first in resolution order, comments dropped. The host passes the standard library directory; the resolver reads `AIPL_PATH` itself with `std/os`. `tests/test_resolver_aipl.rs` requires both to produce the same compiled bytes for every program with imports in the repository, plus aliases, diamonds, cycles, and missing modules (`PATH: Circular import detected: ...`, `Cannot resolve import 'NAME': no 'NAME.aipl' found in ...` listing the places searched).
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
[AIPL VM] Executing function 'main' from 'gcd.aipl'...
21
[AIPL Result]: Int(0)
$ aipl compile gcd.aipl -o gcd.wasm
[AIPL Compiler] Successfully compiled 'gcd.aipl' -> 'gcd.wasm' (N bytes)
```
The `.wasm` exports `gcd`, `main`, `memory`, and `_start`, which runs `main` and exits with its status (section 6.5). Note the `let t` inside the `while` body: it becomes a wasm local of the function and is re-assigned each iteration, which is the intended semantics.

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
`aipl eval hello.aipl` prints `hello, aipl` then `[AIPL Result]: Int(0)`, and exits with status 0. `aipl compile hello.aipl` produces a module importing `wasi_snapshot_preview1::fd_write`; run it with `aipl run hello.wasm`, or any WASI host, and it prints the same line. `(+ "a" "b")` is a checker error: text is built with the formatting forms (section 4.K), `std/buf`, or `str.concat` (which makes a `str` that is never freed).

### 12.4 Threads and atomics, both backends

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

---

### 12.6 The standard library (`aipl_src/std/`)

Written in AIPL over `fs.*`, `mem.*`, `str.len`, and `str.ptr` (no Rust opcodes), so each function behaves identically in the VM and compiled under WASI. Import modules by name: `(import io)`, `(import vec)`, and so on.

| Module | Contents |
|---|---|
| `str` | `(struct Bytes [data:(arr u8) start:i32 len:i32] (value))`, a byte slice (`len` -1 marks a failed read): a checked view reads `data` from `start`, so a freed array stops the program when read; `bytes [addr len] -> Bytes` (unsafe to call) makes an unchecked view of raw memory (no array: for memory that is never freed, or that the caller vouches for); `of_array [a start len]` a checked one; `addr [b] -> i32` (its first byte's address, for `mem.*`, after checking the array), `checked [b] -> bool`, `absent [] -> Bytes` and `present [b] -> bool` (the `len -1` view where text may be missing); the fields are read-only outside `str`, so no other module can make a view; `from_str [s:str] -> Bytes` (a literal or run-time `str`, viewed in place), `is_at [b i s:str] -> bool` (whether b holds s at index i; allocates nothing), `byte_at`, `is_space [c] -> bool` (space and `\t \n \v \f \r`), `bytes_eq [a b] -> bool`, `find_byte [b c] -> i32` (first index or -1), `count_byte`, `count_lines` (newlines plus an unterminated last line), `count_words` (runs of non-space bytes), `parse_int [b] -> (result i32 i32)` (`(ok n)`, or `(err i)` with the index of the first bad byte; optional leading `-`), `parse_int_or [b default] -> i32` (the number, or `default`), `valid_utf8 [b] -> bool`; searching and trimming, allocating nothing (a slice is a value) (linear time: needles over 64 bytes by the Two-Way algorithm of Crochemore and Perrin, as musl's memmem): `starts_with [b p]`, `ends_with`, `contains`, `find [b p] -> i32` (first index or -1), `find_from [b p from]`, `rfind`, `count` (not overlapping), `trim`, `trim_start`, `trim_end` (ASCII whitespace), `split_once [b sep] -> (option.Option str.Halves)` (`before`, `after`), `eq_ignore_case` (ASCII), `cmp [a b] -> i32` (byte order), `parse_i64 [b] -> (result i64 i32)`, `parse_bool`; making a `str` (a copy, never freed; section 4.B): `same [a:str b:str] -> bool` (the same bytes; `eq` on `str` is an error), `concat [a:str b:str] -> str`, `from_bytes [b] -> str` (`b` must be UTF-8) |
| `fmt` | `uint_to_bytes [n out] -> i32` (n read as unsigned), `int_to_bytes` (leading `-`), `hex_to_bytes` (lowercase, no prefix), and for `i64`: `uint64_to_bytes`, `int64_to_bytes`: each writes ASCII at `out` and returns the count (at most 10, 11, 8, 20, and 21 bytes); `f64_fixed [x digits out]` writes `x` with `digits` decimals exactly as C's `printf("%.*f")` (the exact decimal value of the double, rounded half to even; `inf`, `nan`, a `-` for any negative sign bit; at most 312 + digits bytes) |
| `io` | `read_stdin [] -> (result str.Bytes io.Error)` (all of stdin), `write_str [fd s]`, `println [s]`, `eprintln [s]` (stderr), `read_file [path:str] -> (result str.Bytes io.Error)` (the whole file, in a buffer that is never freed), `read_fd [fd] -> (result str.Bytes io.Error)` (reads an open descriptor to its end, freeing each buffer it outgrows), `write_file [path:str b:str.Bytes] -> (result i32 io.Error)` (the bytes written), and `read_path` / `write_path`, the same for a path held as `str.Bytes`; `read_stdin` also gives a result (empty input is an empty view, not an error). A failure is an `io.Error`: `(enum Error [not_found denied is_directory other])`, with `message [e] -> str` for its words (`"no such file or directory"`, `"permission denied"`, `"is a directory"`) and `error_of [code] -> io.Error` for an `fs.*` op's failed result (section 4.C). The message leaves the path out: the program has it, `(eprintln "{path}: {(call io.message e)}")` |
| `vec` | generic growable list `(vec.Vec T)`: `(call (vec.make T) a capacity)` with `a` an `alloc.Allocator` (growing frees the old storage through it), `(call (vec.free T) v)`, `push`, `pop`, `at [v i]`, `set [v i x]`, `len`, `truncate [v n]`, `clear`, `index_of`, `(sort T) [v]` (ascending by `T`'s `cmp.Ord`, stable), `sort_by [v cmp:(fn [T T] -> i32 (no_leak))]` (stable merge sort; `cmp` negative puts the first argument first; any comparison that keeps no memory: section 4.G), `(sort_by_with T C) [v ctx:C cmp:(fn [C T T] -> i32 (no_leak))]` (the same, `cmp` given `ctx` first), each called as `(call (vec.push T) v x)`; plus `sort_i32 [v:(ptr (vec.Vec i32))]` and `cmp_i32` |
| `map` | generic hash map `(map.Map K V)` from keys of any type with a `cmp.Hash` implementation (`i32`, `i64`, `str`, and `str.Bytes` by its contents, from `std/cmp`; a module implements it for its own types): `(call (map.make K V) a capacity)` (an allocator, as for `vec`), `free`, `set [m k v]`, `get_or [m k default]`, `(map.add K) [m k d] -> i32` (for a `(Map K i32)`: adds `d` to k's count, 0 when absent, and returns the new count, in one lookup where `get_or` then `set` take two), `find [m k] -> (option.Option V)` (held by value when `V` fits, so a lookup allocates nothing), `has`, `remove -> bool`, `count`, each called as `(call (map.set K V) m k v)`; iterate with `(for i 0 (call (map.capacity K V) m) (if (call (map.slot_used K V) m i) ... (block)))` reading `slot_key` / `slot_val`. A `str.Bytes` key is kept as the pointer, so its bytes must not change while it is stored |
| `cmp` | the interfaces `(cmp.Eq T)` (`same`), `(cmp.Ord T)` (`compare`: -1, 0, or 1), and `(cmp.Hash T)` (`hash`, `same`), implemented for `i32`, `i64`, `bool` (Eq), `str` (bytewise), and `str.Bytes` (Hash, by contents); none for floats (section 4.H) |
| `os` | the command line and environment: `arg_count [] -> i32` (argv[0], the program, included), `arg [i] -> (option.Option str.Bytes)` (`none` past the end), `arg_int [i default] -> i32` (argument `i` as a number, or `default` if it is missing or not one: `(call os.arg_int 1 1000)`), `env [name:str] -> (option.Option str.Bytes)` (`none` if unset; an empty value is `some` of an empty view), each table read once and kept, so they allocate nothing after the first call and are `(no_leak)`; `random_i32 [] -> i32` from the OS generator |
| `buf` | string builder: `make [a:alloc.Allocator capacity] -> (ptr buf.Buf)`, `free [b]` (a view taken before then stops the program when read: "Use after free"; so does one taken before a push that moved the bytes), `push_byte`, `push_str [b s:str]`, `insert [b at n c]` (n copies of byte c at index at), `push_unsigned [b u:i64 base upper]` (base 2, 8, 10, or 16, read as unsigned), `push_bytes [b str.Bytes]`, `push_int`, `push_i64`, `push_f64 [b x digits]`, `len`, `byte_at [b i]` (checked; no view made), `clear`, `truncate [b n]` (keeps the first n bytes), `bytes [b] -> str.Bytes` (a view of the contents; take it after building), `room [b n] -> i32` and `grew [b n]` (unsafe to call: the address where n more bytes go, for a system call to write, then how many it wrote), `write [b fd] -> i32`, `print [b]` (the contents to standard output), `to_str [b] -> str` (a copy of the contents, which must be UTF-8; later pushes and `free` leave it alone) |
| `arena` | a region allocator (frees everything at once; for single objects see `heap`): `make [chunk_size] -> (ptr arena.Arena)`, `(call (arena.alloc T) a) -> (ptr T)` (zeroed, like `new`, no cast), `raw [a n] -> i32` (n zeroed bytes, 8-aligned), `reset [a]` (frees everything from `a`, marking every object freed, so a pointer used after it fails at once; its chunks are reused), `reserved [a]`; never fails while memory remains (a full chunk moves on to another). A fast arena, `make_fast [chunk_size] -> (ptr arena.Fast)` with `(call (arena.fast_alloc T) f)`, `fast_raw`, and `fast_reset`, keeps no per-block headers and marks nothing at reset: less memory and time, but a pointer used after its reset is caught only once the memory holds a new object (before that it reads the old one) |
| `heap` | a general-purpose allocator with per-object free (docs/design/HEAP_PLAN.md): `make [] -> (ptr heap.Heap)`, `(call (heap.create T) h) -> (ptr T)` and `(call (heap.destroy T) h p)`, `(call (heap.array T) h n) -> (arr T)` and `(call (heap.free_array T) h a)`, `raw [h n] -> i32` and `free_raw [h p n]` (n zeroed bytes, 8-aligned), `live [h]` (blocks not freed), `live_bytes [h]`, `reserved [h]`. A double free, a free of memory this heap did not allocate, a free with the wrong size or type, and a write after free (found when the block is reused) stop the program with a contract failure naming the check; freed memory reads as `0xDEADBEEF` words, and any pointer to a destroyed object fails with `Use after free` (generation checks) |
| `alloc` | any allocator as one value (docs/design/HEAP_PLAN.md): the union `alloc.Allocator` of a heap, an arena, a fast arena, or a fixed buffer; `of_heap [h]`, `of_arena [r]`, `of_fast_arena [f]`, `fixed [base len]` (unsafe to call: hands out those bytes; a free takes back only the most recent allocation), `default []` (one shared heap, made on first use); `raw [a n]`, `free_raw [a p n]` (unsafe to call), `(call (alloc.create T) a)`, `(call (alloc.destroy T) a p)`, `(call (alloc.array T) a n)`, `(call (alloc.free_array T) a xs)`. A free does what the allocator does: a heap reuses and checks, an arena ignores it until `reset` ; `(custom C) [state:C alloc_fn free_fn]` for a program's own allocator, unsafe to call (`alloc_fn [state:C n] -> i32`, 8-aligned; `free_fn [state:C p n]`; the state keeps its type, typically a pointer to the allocator's struct) |
| `gen` | generations for allocators (GENERATIONS_PLAN.md): `fresh [] -> i32`, `(adopt T) [base] -> (ptr T)` and `(adopt_array T) [base n] -> (arr T)` (a zeroed block of 8 + the object's bytes becomes a checked object with a fresh generation), `(retire T) [p] -> i32` and `(retire_array T) [a]` (fails if already freed; marks it freed; returns the block), `kill [base]`; all unsafe to call |
| `date` | dates and times in UTC (no time zones, no leap seconds), years 0000-9999: `(struct Date [year month day hour minute second nano] (value))` with read-only fields; `now [] -> Date`, `from_unix [secs:i64]`, `from_unix_ns [ns:i64]`, `to_unix [d] -> i64`, `to_unix_ns [d] -> i64`, `make [year month day hour minute second nano] -> Date` (checked by `valid [...] -> bool`), `weekday [d] -> i32` (1 Monday .. 7 Sunday), `day_of_year`, `is_leap [year]`, `days_in_month [year month]`, `add_seconds [d secs:i64]`, `add_days [d days]`; ISO 8601: `push_iso [b d]` and `to_str [d] -> str` (`2026-10-16T09:41:07.25Z`), `push_iso_date [b d]`, `parse [b:str.Bytes] -> (result Date i32)` (RFC 3339: a date, or a date and time with `Z` or a `+HH:MM` offset, which is applied; the error is the index of the first byte that does not fit) |
| `text` | text that builds something, from the allocator passed in: `split [a b sep] -> (ptr (vec.Vec str.Bytes))` (Python's `str.split(sep)`: `"a,,b"` gives three pieces), `fields [a b]` (runs of non-whitespace, Python's `str.split()`), `lines [a b]` (Rust's `str::lines`: `\r\n` or `\n`, no empty last line), `join [a parts sep] -> (ptr buf.Buf)`, `replace [a b from to]` (left to right, not overlapping), `repeat [a b n]`, `to_upper`, `to_lower` (ASCII), `parse_f64 [b] -> (result f64 i32)` (exactly rounded; `12`, `-0.5`, `1e-3`; the error is the index of the first byte that does not fit). Checked against Python on 800 generated cases (tests/aipl/text) |
| `utf8` | one character (scalar value) at a time: `decode [b i] -> utf8.Char` (`cp`, and `len` 1-4, or 0 where the bytes are not UTF-8), `step [ch] -> i32` (the bytes to move past it), `encode [buf cp]`, `count [b]` (characters), `scalar [cp]`, `is_space [cp]` (Unicode's White_Space property), `is_ascii_letter`, `is_ascii_digit` |
| `show` | values as text, as the formatting forms (section 4.K) print them; each function takes a buffer and returns it, so calls chain: `start [] -> (ptr buf.Buf)`, `lit [b s:str]`, `put_i32 [b v spec]`, `put_i64`, `put_f32`, `put_f64` (the fewest digits that read back, as Python's repr; or exact decimals), `put_bool`, `put_str`, `put_bytes`, then `done [b] -> str`, `print [b]`, `println [b]`, `eprintln [b]` (each frees b), or `end_to [b]`; `debug_addr [b label addr]` writes a pointer or array inside a value shown by a generated printer (`null`, or `label @ addr`). `spec [width decimals align zero base plus fill] -> i32` packs a format (the word's layout is in the module's header) |
| `testing` | what a test's `expect` calls (section 4.L): `fail [label:str]`, `fail_cmp [label left right]` (each counts one failure and writes to standard error), `failures [] -> i32`, `status [n] -> i32` (n, at most 125) |
| `stream` | reading and writing as you go (a big input a piece at a time), over a file, standard input and output, bytes in memory, a `buf.Buf`, or functions of your own, each from the allocator passed in. Readers: `open [a path:str] -> (result (ptr stream.Reader) io.Error)` (std/io's error: `not_found`, `denied`, ...), `open_path`, `stdin [a]`, `of_fd [a fd]`, `of_bytes [a b]` and `of_str [a s]` (read in place), `(reader_of C) [a state fill_fn]` with `fill_fn:(fn [C (ptr buf.Buf)] -> i32 (no_leak))` (a function that keeps no memory, section 4.G) appending some bytes and returning how many (0 at the end, negative on an error); `next_line [r] -> bool` then `line [r] -> str.Bytes`, or `read_line [r] -> (option.Option str.Bytes)` (a line ends at `\n` or `\r\n`, which it leaves out; a last line with no `\n` keeps a `\r`, as Rust's `str::lines`), `read_byte`, `peek_byte` (-1 at the end), `read [r into max]` and `read_all [r into]` (append to a buffer), `read_error [r] -> bool`, `close_reader [r]` (closes what `open` opened). What a reader hands out points into its window, good until the next read from it. Writers: `create [a path] -> (result (ptr stream.Writer) io.Error)`, `create_path`, `stdout [a]`, `stderr [a]`, `to_fd [a fd]`, `to_buf [a b]` (appends to b, holding nothing back), `(writer_of C) [a state write_fn]` with `write_fn:(fn [C str.Bytes] -> bool (no_leak))`; `write_byte`, `write_bytes`, `write_str`, `(format_to w ...)`, `buffer [w]` and `wrote [w]` (add to the held bytes directly), `flush [w] -> bool`, `write_error [w] -> bool`, `close_writer [w] -> bool` (flushes; closes what `create` opened). A writer holds what it is given until it has 8 KB, `flush`, or `close_writer`: a program that ends without either loses its last writes, and between a buffered `stdout` writer and `println` (which writes at once) output can reorder unless you flush. Every OS error is -1 (section 4.C), so a stream records only that one happened; after a read error a reader is at its end, and after a write error a writer drops what it is given |
| `csv` | comma-separated values, read as Python's `csv.reader` reads them with its default dialect (from its C source, non-strict: quoted fields with delimiters, doubled quotes, and line breaks; text after a closing quote kept; a line ending at `\n`, `\r\n`, or `\r`; an empty line a row of no fields; no field size limit), a row at a time over a `stream.Reader`: `reader [a r]`, `reader_with [a r delim]`, `open [a path] -> (result (ptr csv.Reader) io.Error)`, `of_bytes [a b]`, `close`; `next_row [cr] -> bool`, `count`, `field [cr i] -> str.Bytes` (a view, good until the next row), `row` (1 the first), `read_error`; `header [cr] -> bool` then `column [cr name] -> i32` (-1 if none) and `get [cr name] -> (option.Option str.Bytes)`; `f64_at` and `i64_at [cr i] -> (result T (ptr csv.Error))`, `Error` holding `row`, `column`, and `what` (`row 3, column 1: x2 is not an integer`); `parse [a b]` (every row at once, as vectors of views). Writing, as Python's `csv.writer` (quoting a field only if it holds the delimiter, a quote, or a line break; each quote doubled; rows ended by `\r\n`; a row of one empty field written `""`): `writer [a w]`, `writer_with [a w delim]`, `field_bytes`, `field_str`, `field_i64`, `field_f64`, `end_row`, `free`. Checked against Python on 3,600 generated cases (600 kept, tests/aipl/csv) |
| `stats` | statistics over `(arr f64)`, computed as Python's `statistics` module and `math.fsum` compute them, to the last bit: `sum` (`fsum`: the exact sum, rounded once), `fmean` (`fsum / n`), `mean` (the exact mean, rounded once), `median`, `quantiles [a xs n inclusive] -> (arr f64)` (the n - 1 cut points; Python's default `exclusive` method, or `inclusive`), `min`, `max`, `mode` (the most common; of equally common, the first to appear), `variance` and `pvariance` (the exact sample or population variance, rounded once), `stdev` and `pstdev` (the square root of the exact one, rounded once), `covariance`, `correlation`, and `linear_fit [xs ys] -> stats.Fit` (`slope`, `intercept`, as `statistics.linear_regression`, and `r2`), over `fsum` and `math.sumprod`'s triple-length accumulation; plus `zscore [x mean sd]` and `histogram [a xs lo hi bins] -> (arr i32)` (equal intervals, `hi` in the last). The exact values are held as expansions (Shewchuk's partials, Dekker's products) and the roundings found by exact sign tests, so a square beyond about 1e300 overflows where Python's fractions would not. Too little data fails the function's contract, where Python raises `StatisticsError`. Checked against Python on about 44,000 values (300 data sets kept, tests/aipl/stats) |
| `set` | a hash set `(set.Set K)` of any type with a `cmp.Hash` implementation, over std/map: `(call (set.make K) a capacity)`, `free`, `add -> bool` (whether it was new), `has`, `remove -> bool`, `count`, `items [a s] -> (ptr (vec.Vec K))` (in slot order, the same in every backend), `union`, `intersection`, `difference` (each a new set from a), `is_subset`; each named with its type, `(call (set.add str) s "x")` |
| `random` | random numbers from PCG-32 (pcg-c-basic's pcg32), so a seed gives the same numbers everywhere: `seeded [a seed:i64] -> (ptr random.Rng)` (stream 54, as pcg32's demo: seed 42 gives `a15c02b7 7b47f409 ba1d3330 ...`), `seeded_stream [a seed seq]`, `seeded_from_os [a]`, `free`; `next_u32 -> i32` (32 bits), `next_u64`, `below [r bound] -> i32` (0 to bound - 1, unbiased: pcg32_boundedrand), `range [r lo hi]` (hi left out), `f64` (in [0, 1), a multiple of 2^-53), `coin`, `normal [r mean sd]` (Marsaglia's polar method, over a port of fdlibm's log), `(shuffle T) [r xs]` (Fisher-Yates, in place), `(choice T) [r xs]`, `(sample T) [a r xs k]` (k without repeats). Not for secrets |
| `uuid` | version 4 UUIDs as 36 lowercase characters, 8-4-4-4-12: `v4 [] -> str` (the system's random bytes), `v4_from [r:(ptr random.Rng)]` (the same for the same seed), `valid [s] -> bool` |
| `math` | the small arithmetic the language does not spell: `min`, `max`, `abs`, `sign`, `clamp`, `floor_div` and `floor_mod` (rounded toward negative infinity, as Python's `//` and `%`; the operators `/` and `%` round toward zero), `gcd` (never negative), `pow [x n]` (n not negative; stops the program on overflow, through `checked.mul`), `isqrt` (the largest r with r*r <= x), for `i32`, and the same with an `_i64` suffix (`pow_i64 [x:i64 n:i32]`); for `f64`, `floor`, `ceil`, `trunc`, and `round` (halves away from zero, as C's round; a whole number, infinity, and not-a-number come back as they are), `min_f64`, `max_f64` (not a number when either is), `abs_f64`, `powi [x n:i32]` (by repeated multiplication, so within a few units of the last place of libm's pow, not always its last bit), `is_nan`; the constants `PI` and `TAU`. `abs` of the smallest integer, a zero divisor, a negative `isqrt` argument, and `clamp` with lo above hi fail their contracts. Not here: exp, ln, the trigonometric functions, a real-exponent pow. Checked against Python on generated cases in every backend (`tools/math_vectors.py`) |
| `path` | paths as text, as Python's `posixpath` treats them (no file system access): `join [a b]` (b if it is absolute), `parent` (`dirname`), `file_name` (`basename`), `stem` and `extension` (`splitext`: `".gz"` for `b.tar.gz`, none for `.bashrc`), `normalize` (`normpath`: `"."` and repeated slashes out, `".."` taking the part before it, `"."` for the empty path), `is_absolute`; each takes and gives `str`. Checked against Python on 1,600 generated paths (400 kept, tests/aipl/libs) |
| `cli` | command-line options: `parser [a name about] -> (ptr cli.Parser)`, then `flag [p name short help]`, `option [p name short default help]`, `positional [p name help]` (required, in order), `rest [p name help]` (any after); `parse [p] -> (result (ptr cli.Args) str)` over the program's arguments, `parse_list [p argv]`, or `parse_or_exit [p]` (usage on standard output and exit 0 for `--help` or `-h`; `NAME: ERROR (NAME --help shows the options)` on standard error and exit 2 for arguments that do not fit); `get_str [args name] -> str`, `get_bool`, `get_i64 -> (result i64 str)`, `rest_args`; `usage [p] -> str`. Accepted: `--name`, `--name value`, `--name=value`, `-n`, `-n value`, `-nvalue`, flags together (`-vq`), `-` as an argument, and `--` to end the options. Errors: `unknown option '--x'`, `option '--name' takes no value`, `option '--name' needs a value`, `missing argument 'file'`, `unexpected argument 'x'` |
| `encode` | bytes as text and back, decoding strictly (it accepts what Python's `binascii` with `strict_mode` and `urllib.parse` accept; the error is the index of the first byte it refuses, and nothing is appended then): `base64_to [o:(ptr buf.Buf) b:str.Bytes alphabet:encode.Alphabet padding:encode.Padding]` (`Alphabet.standard` or `url_safe`, `Padding.padded` or `unpadded`), `base64_decode_to [o b alphabet padding] -> (result i32 i32)` (bytes appended, or the index); `hex_to [o b upper]`, `hex_decode_to [o b]`, `hex_value [c] -> i32`; `percent_encode_to [o b keep:str plus:bool]` (keeps A-Z a-z 0-9 `_.-~` and keep's ASCII, `%XX` for the rest, with plus a space as `+`: Python's `quote` and `quote_plus`), `percent_decode_to [o b plus]` (a `%` without two hex digits is an error); and from str to str: `base64`, `base64_decode -> (result str i32)`, `hex`, `hex_decode`, `percent_encode [s keep]`, `percent_decode` |
| `hash` | checksums and hashes: `hash.Algorithm` is `md5`, `sha1`, `sha256`, or `sha512`; `make [a alg] -> (ptr hash.Hasher)`, `update [x b:str.Bytes]`, `update_reader [x r:(ptr stream.Reader)] -> i64` (bytes read, or -1), `finish_to [x o:(ptr buf.Buf)]` (appends the digest; no updates after), `free`; `digest_to [o alg b]`, `hex [alg s] -> str` (Python's `hexdigest()`), `digest_size [alg]`, `block_size [alg]`; `hmac_to [o alg key msg]` and `hmac_hex [alg key:str msg:str]` (RFC 2104; a key longer than a block is hashed first); `crc32 [b] -> i32` and `crc32_update [crc b]` (zlib's, signed), `adler32` and `adler32_update [adler b]`; `constant_time_eq [a b] -> bool` (time independent of where they differ). MD5 and SHA-1 are for checksums only (both broken for security); no encryption |
| `regex` | regular expressions of the guaranteed-linear kind (a Pike VM: time in proportion to pattern size times text length, whatever the pattern): `compile [a pattern:str] -> (result (ptr regex.Regex) (ptr regex.Error))` (`Error.at`, a byte offset, and `Error.what`; `free_error [a e]`), `free`, `groups`, `group_index [re name] -> i32` (-1 if none); over text as `str.Bytes`, spans `regex.Span` (`start`, `end`, byte offsets): `is_match`, `find -> (option.Option regex.Span)`, `find_at [re text start]`, `captures [a re text] -> (option.Option (ptr regex.Captures))` with `group [c i] -> (option.Option regex.Span)` and `free_captures`, `find_all [a re text] -> (ptr (vec.Vec regex.Span))` (Python's `finditer` rule for empty matches), `replace_all [re text template] -> (result str (ptr regex.Error))` (`$1`, `${1}`, `$name`, `${name}`, `$0`, `$$`; a group the pattern lacks is an error), `split [a re text]` (the text between matches; unlike Python's, without the groups). Syntax: literals, escapes, `.`, classes, `\d \w \s` and negations (ASCII), `^ $ \A \Z \b \B`, groups (capturing, `(?:...)`, `(?P<name>...)`), `|`, `* + ? {m} {m,} {,n} {m,n}` greedy and lazy, flags `(?i) (?m) (?s)` at the start. Matches are Python's `re` with `re.ASCII` (leftmost, as a backtracking matcher prefers; `$` also before a final newline); `.` and classes match UTF-8 characters. Lookaround, backreferences, possessive quantifiers, scoped flags, and counts over 1000 are errors. A Regex is for one thread at a time |
| `deflate` | DEFLATE (RFC 1951), raw and in zlib (RFC 1950) and gzip (RFC 1952) wrappers, both ways: `compress_to [a o:(ptr buf.Buf) b:str.Bytes level]`, `zlib_compress_to`, `gzip_compress_to` (levels 0, stored, to 9; LZ77 over a 32 KiB window with hash chains, lazy from level 4; each block stored, fixed, or dynamic, whichever is smallest; within 1% of zlib's size); `inflate_to [a o b limit] -> (result i32 (ptr deflate.Error))` and `zlib_decompress_to` (the input bytes the stream took; what follows is the caller's), `gzip_decompress_to` (every member to the end; zero padding allowed; an empty input is no members); `limit` caps the output; `Error.at` (input offset) and `Error.what`, `free_error [a e]`. Refuses what zlib refuses (puff.c's rules), checks the Adler-32, CRC-32, and length, and refuses gzip's reserved header flags as zlib does |
| `url` | URLs (RFC 3986): `parse [a text:str.Bytes] -> (result (ptr url.Url) (ptr url.Error))`, the parts as views in the text as written (`scheme` empty if none; `has_authority`, `authority`, `has_user`, `user`, `has_password`, `password`, `host` (an IPv6 address without its brackets), `port` (-1 if none), `path`, `has_query`, `query`, `has_fragment`, `fragment`), split as Python's `urlsplit` and its properties split them; a port that is not a number up to 65535 or an unpaired bracket is an error; `free [u]`; `write [o u]`, `to_str [u]`; `resolve [a base ref] -> (ptr url.Url)` (RFC 3986 5.2, strict: "http:g" stays; Python's `urljoin` differs from the RFC in three ways: it keeps dot segments in a network-path reference, drops empty segments, and gives an empty reference back as the whole base), `resolve_str [a base ref] -> (result str (ptr url.Error))`, `remove_dots [path]`; `query_pairs [a q] -> (result (ptr (vec.Vec str)) (ptr url.Error))` (names and values in turn, + as space, %XX decoded; a bad % or bytes that are not UTF-8 are an error, where Python passes them through), `query_get [a q name] -> (result (option.Option str) (ptr url.Error))`; these three give `str`s, never freed, and each has an `_into` form that writes into a buffer of yours and keeps no memory, for a request's path: `remove_dots_into [o path]`, `query_pairs_into [a o q] -> (result (ptr (vec.Vec str.Bytes)) (ptr url.Error))` and `query_get_into [a o q name] -> (result (option.Option str.Bytes) (ptr url.Error))` (views of `o`, good until it changes). `parse`, `resolve` (whose path the new Url owns: `free` frees it), `write`, and the `_into` forms are `(no_leak)`, as are std/json's and std/text's functions; `query_build [pairs] -> str` (Python's `urlencode`); `free_error [a e]` |
| `http` | HTTP/1.1 messages (RFC 9112) without the network: `parse_request [a text limits] -> (result (ptr http.Message) (ptr http.Error))` and `parse_response` (`method`, `target`, `status`, `reason`, `version`, `headers` (a `(vec.Vec http.Header)` of `name` and `value` views, in order), `body`, `length`: the bytes the message took, as another may follow); `header [m name] -> (option.Option str.Bytes)` (ignoring case); `free_message`, `free_error`; an `http.Error` has `at`, `what`, `status` (the answer the refusal calls for: 400, 413 for a body over the limit, 431 for a head or header count over it), and `incomplete` (only cut short: more bytes could make a whole message); `default_limits` (64 KiB of head, 100 headers, 8 MiB of body) and `limits [head headers body]`. Refused, as two readers could disagree about them: a space before a header's `:`, a folded header line, both Transfer-Encoding and Content-Length, two different Content-Lengths, a transfer coding other than a final `chunked`. Lines may end with CRLF or LF. A response with neither length header runs to the end of the text; 1xx, 204, and 304 have no body. Writing: `start_request [o method target]`, `start_response [o status]`, `add_header [o name value]` (name a token, value without CR, LF, or NUL: contracts), `end_head [o body]` (Content-Length, the empty line, the body); `reason [status] -> str` (Python's `http.HTTPStatus` phrases), `mime_type [name] -> str` (by extension, from a fixed table of 31), `token`, `clean` |
| `html` | `escape_to [o b quote:bool]` and `escape [s] -> str` (`& < >`, and with quote `" '` as `&quot;` and `&#x27;`: Python's `html.escape`); `unescape_to [o b]` and `unescape [s] -> str`: numeric references as Python maps them (the Windows-1252 range to its characters, surrogates and numbers past U+10FFFF to U+FFFD, controls and noncharacters to nothing), HTML 4's named ones with `&apos;` (259 names, each meaning what HTML5 gives it), and HTML5's legacy names without their `;`, the longest prefix winning (`&notit;` is `¬it;`); a name it does not know stays as it is (Python knows all 2,231 of HTML5's) |
| `time` | timing code with the monotonic clock: `now [] -> i64` (nanoseconds), `since [start] -> i64`, `push_duration [b ns]` ("850 ns", "12.345 us", "3.071 ms", "4.200 s": three decimals in the largest fitting unit, integer arithmetic), `report [label start]` ("label: 12.345 ms" on stderr, so a program's output stays clean) |
| `par` | parallel work with no `unsafe` (GENERATIONS_PLAN.md, "Data races"): `(call (par.map T U C) out f ctx xs threads)` sets `out[i]` to `(call_ref f ctx xs' i)` for every `i` of `out` (`f:(fn [C (arr T) i32] -> U)`), split into `threads` contiguous parts, one on the calling thread and each other on its own thread, each with its own copy `xs'` of `xs`; returns when all are done. `(call (par.reduce T U C) f join ctx xs n init threads) -> U` folds the indices `0 .. n-1` the same way: each part starts from `init` and sets `acc` to `(call_ref f ctx xs' i acc)` for each of its indices in order (`f:(fn [C (arr T) i32 U] -> U)`), and the parts' results are joined in order with `join:(fn [U U] -> U)`; with an associative `join` and `init` its unit, the answer is the sequential one (with floats it can depend on `threads`, never on timing). `T`, `U`, and `C` must be plain (`(where (plain T))`, section 4.H), so no worker can reach memory another writes. A program using it is a threaded module |
| `thread` | `(spawn_with C) [worker:(fn [C] -> i32) ctx:C] -> i32`: runs `worker` on `ctx` on a new thread (unsafe to call, as `thread.spawn` is); `(thread.join h)` gives the result. The worker's context keeps its type, where `thread.spawn` passes an `i32` |
| `fs` | directories, file information, and permissions over `fs.opendir`, `fs.readdir`, `fs.stat`, and `fs.chmod`: `list [a:alloc.Allocator path:str] -> (option.Option (ptr (vec.Vec str.Bytes)))` (the names in a directory sorted bytewise, without `.` and `..`, so a listing is the same in every backend; `none` if it cannot be read), `info [path:str] -> (option.Option fs.Info)` with `(struct Info [(read kind:fs.Kind) (read size:i64) (read modified:i64)] (value))`, a value struct (section 4.E) (`fs.Kind.file`, `directory`, or `other`, after following symbolic links; `modified` in nanoseconds since 1970), and `list_path` / `info_path` for a path held as `str.Bytes`; `chmod [path:str mode:i32] -> (result i32 io.Error)` (and `chmod_path`) sets the permission bits, 0 to 0o777, with `fs.FILE` (0o644) and `fs.PROGRAM` (0o755) for the two common modes, confined as the other file operations are, and needing no grant. Starting other programs is std/proc (below), over `proc.spawn` and its operations (section 4.C), granted by `--allow-run` |
| `ip` | IP and socket addresses from text, read as Rust's standard library reads them (tests/test_web.rs compares the two on generated texts), allocating nothing: `parse [t:str.Bytes] -> (option.Option ip.Addr)` (IPv4 as four numbers 0-255 without leading zeros; IPv6 with `::` and an IPv4 tail), `parse_endpoint [t] -> (option.Option ip.Endpoint)` (`a.b.c.d:port`, `[v6]:port`; a zone index such as `%eth0` refused), `len [a] -> i32` (4 or 16), `byte_at [a i] -> i32` (network order); `Endpoint.addr`, `Endpoint.port`, `Addr.v6` |
| `net` | TCP sockets over `aipl_net_v1`, in every backend (docs/design/NET_PLAN.md): `listen [a at:str.Bytes] -> (result net.Socket net.Error)` (ADDR:PORT; only where `--listen` granted, else `Error.not_granted`), `accept [l] -> (result net.Socket net.Error)` (waits), `recv [c into:(ptr buf.Buf) max] -> i32` (added to `into`; 0 at the end, -1 on an error), `send [c b:str.Bytes] -> bool` (all of it), `close [s]`, `reader [a c] -> (ptr stream.Reader)` and `writer [a c] -> (ptr stream.Writer)` (std/stream over the connection; closing them leaves it open), `message [e] -> str`. A `Socket`'s handle is private, so a socket never reaches std/fs. All of it keeps no memory (`(no_leak)`); one connection at a time for now |
| `proc` | starting other programs, over `aipl_sys_v1` (docs/design/SYS_PLAN.md), in every backend; only what `--allow-run` granted (else `Error.not_granted`). `command [a path:str] -> (ptr proc.Command)` (and `command_path` for bytes; the path is given to the system as written, and is the program's own name), `arg c "text"` and `arg_bytes` add arguments, `free_command`; `run [c input:str.Bytes out:(ptr buf.Buf)] -> (result i32 proc.Error)` runs it to the end (an empty input is the null device; everything it writes, its error too, is added to `out`; the result is its status); `spawn [c] -> (result proc.Child proc.Error)` starts it with a pipe to its input and one from its output and error together, `spawn_with c input output error` with each stream a `proc.Stream` (`inherit`, `pipe`, `null`, or for the error `merged`); on a `Child`: `write ch b -> bool`, `close_input ch`, `read ch into max -> i32` (like `net.recv`) and `read_error`, `wait ch -> i32`, `close ch`; `exec [c] -> proc.Error` replaces this program (it returns only to say why not). A status is the exit status 0-255, or 256 plus the signal (`exited s`, `signal s`). `find [name:str out:(ptr buf.Buf)] -> bool` looks a name up in the PATH. `message [e]` words an error; a program that writes more than a pipe holds before it reads can wait for this one while this one waits for it |
| `serve` | a long-running HTTP/1.1 server over `net` and `http` (docs/design/NET_PLAN.md, mode A): `run [l:net.Socket handler options] -> serve.Stats` with a handler `(fn [alloc.Allocator (ptr http.Message) (ptr buf.Buf)] -> void (no_leak))` given the request's arena, the request, and the buffer its whole response goes in (`http.start_response`, `add_header`, `end_head`); `(run_with C) [l ctx handler options]` for state kept between requests, the handler taking `ctx` first. One connection at a time, one request each; each request on an arena reset after it (memory stays flat: tests/test_serve.rs serves 100,000 requests natively and under aipl-run, a million with AIPL_MILLION=1, with the heap where it was after the first thousand); a request std/http refuses answered with the error's status and reason; a view kept past its request stops the program with "Use after free" on its next use. `options` (http's default limits, 64 KiB arena chunks, no end), `requests [o n]` (return after n answers), `limits [o lim]`; `Stats.served`, `Stats.arena_bytes`. A trap in a handler ends the server. Mode B, a fresh instance per request: `once [handler options] -> i32` reads one request from standard input and writes its answer to standard output (a refusal too), 0, or 1 if the input ended before a request began; `(once_with C) [ctx handler options]`. Run under `aipl serve --fresh` or as an executable built with `aipl compile --exe --serve ADDR:PORT`, where a trap answers 500 and the next request is served; an answer that cannot all be written gives 1; and a saved request is tried with `aipl eval prog.aipl < request.txt` in any backend |
| `json` | JSON (RFC 8259) to and from a `json.Value`, `(union Value [(null) (boolean b:bool) (number n:f64) (text s:str.Bytes) (list items:(ptr (vec.Vec Value))) (object fields:(ptr (vec.Vec (ptr json.Field))))])`. `parse [a:alloc.Allocator text:str.Bytes] -> (result json.Value (ptr json.Error))` (its error from a; `free_error [a e]`) is strict: valid UTF-8, no duplicate keys, nesting at most 512 deep, numbers that fit an `f64`, and an `Error` with the byte offset `at` and `what` went wrong. `get [v key:str] -> (option.Option json.Value)` (an object's member), `at [v i]` (a list's element), `count [v]` (elements or members, 0 for other values); `get_path [v path:str] -> (option.Option Value)` (`"a.b[2].c"`: members after `.`, elements in brackets; `""` is v; path must pass `valid_path [path] -> bool`, and a key cannot hold `.` or `[`); `field [a key value] -> (ptr json.Field)` makes a member for building an object (`(read key)`, `(read value)`). `write [b:(ptr buf.Buf) v]` writes the text JavaScript's `JSON.stringify` writes: no spaces, keys in their order, numbers as the shortest digits that read back as the same `f64` (`0.1`, `1e+21`, `5e-324`); writing an infinity or NaN fails its contract. `tests/test_std.rs` compares the numbers with Rust's shortest digits on 20,000 doubles |
| `option` | `(union (option.Option T) [(some value:T) (none)])`, a value that may be missing (section 4.J): `(make (option.Option i32) some 5)`, `(make (option.Option i32) none)`; `(call (option.is_some T) o)`, `is_none`, `(call (option.unwrap_or T) o fallback)` |
| `float` | `from_decimal [addr len] -> f64`: decimal text (an optional sign, digits with one `.`, optionally `e`/`E` and a signed exponent) to the nearest `f64`, ties to even, as Rust's `parse::<f64>`; too large is an infinity, too small a zero. The self-hosted compiler converts its float literals with it; `tests/test_std.rs` compares it with Rust on 400,000 literals |
| `bigint` | arbitrarily large signed integers, changed in place (reuse numbers rather than making new ones), each from the allocator it is made with, which it grows through: `(ptr bigint.Int)` from `from_i32 [a v]`, `with_capacity [a limbs]`; `free [n]` gives it back; `set_i32 [a v]`, `assign [dst src]`, `add [a b]`, `sub [a b]`, `add_mul_small [a b m]` (a += b·m), `sub_mul_small [a b m]`, `mul_small [a m]` (\|m\| < 2^31), `div_small [a d] -> i32` (a /= d toward zero, returns the remainder, 0 < d < 2^31), `div_small_quotient [a b] -> i32` (a ≥ 0, b > 0, quotient < 2^31: returns a / b, leaves the remainder in a), `negate [a]`, `compare [a b] -> i32`, `sign [a] -> i32`, `is_zero [a]`, `limb_count [a]`, `push_decimal [out a]` |

Containers are generic (section 4.H): a list of points is `(ptr (vec.Vec (ptr Point)))`, filled with `(call (vec.push (ptr Point)) v p)` and read with `(call (vec.at (ptr Point)) v i)`, with no casts.

From outside, the slice type is `str.Bytes`: `str.Bytes`, `(get b str.Bytes.len)`. Every module ends in a `run_<module>_tests` runner wired into `aipl_src/test_suite.aipl`. Allocation grows memory as needed (section 4.A), and memory from `mem.alloc` and `alloc.forever` is never freed: `read_file` allocates a buffer per file. Containers take an allocator (`std/alloc`; `(call alloc.default)` is a shared heap) and free their old storage when they grow, and `free` releases them.

### 12.7 A complete I/O program with the standard library, both backends

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
[AIPL Result]: Ok(Int(0))
$ aipl compile --exe examples/word_count.aipl -o word_count
$ cd examples && ../word_count
lines: 4
words: 15
bytes: 81
```
`tests/test_wasi.rs` runs this program in both backends against a generated input and against the shipped `examples/input.txt`, and asserts the exact stdout and exit status; `aipl compile --self` reports byte parity for it and for each `std` module.

## 13. Pitfalls for Code Generators

Each of these is a real failure mode observed when LLMs write AIPL. The fix is in the second column. The compiler's message for most of them names the fix too (CANONICAL_PLAN.md step 2), and a name close to a real one (an operator, a function, a module's item) gets a "did you mean".

| Mistake | Correct form |
|---|---|
| `(mem.load32 p)`, `(ptr.cast S a)`, or `(thread.spawn ...)` in an ordinary function | wrap it in `(unsafe ...)`, or mark the function `(unsafe)` if wrong arguments could break memory (section 3); usually `new`, arrays, or a std function avoid it |
| `(f x)` to call a user function | `(call f x)`; bare `(name ...)` is only for built-in ops |
| `(if c (set! x 1))` with no else | `if` always takes three arguments; for one branch write `(when c (set! x 1))`; both branches must be void or both the same value type |
| `(if c (set! x 1) (block))` or `(if c (block) (set! x 1))` | an empty `(block)` branch is an error naming `when`: `(when c (set! x 1))`, `(when (not c) (set! x 1))` |
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
| hand-writing digit formatting, file-reading loops, byte counting, growable arrays, hash tables, or string building | the standard library (section 12.6): `io`, `str`, `fmt`, `vec`, `map`, `buf`, `os` |
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
