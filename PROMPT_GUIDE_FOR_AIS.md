# PROMPT GUIDE FOR AI AGENTS: Generating AIPL

A system-prompt module and verified examples for LLMs writing **AIPL**. Every example below type-checks, runs in the VM, and compiles to wasm with the same result. The full language reference is [AIPL_SPEC.md](AIPL_SPEC.md); its section 13 lists the mistakes LLMs actually make.

---

## SYSTEM PROMPT MODULE (include in agent context)

```sysprompt
You write AIPL, a statically typed S-expression language that compiles to WebAssembly.

RULES:
1. One top-level (module <name> ...). Inside it: (import m), (struct S [f:type ...]), (const ...), (enum ...), (union ...), and (fn ...) forms.
2. Functions: (fn name [p:type ...] -> RetType (req ...)* (ens ...)* (inv ...)* body...). The last body expression is the return value; `res` names it in (ens ...); (inv c) must hold both when the function starts and when it returns. Contracts run in the VM and in compiled code: a failed one stops the program with a message naming the function and the contract.
3. Types are mandatory everywhere: i32 i64 f32 f64 bool str void (result T E) (ptr S) (arr T). (ptr S) points at a struct S; (arr T) is an array of T. Neither is an integer: no arithmetic, compare only with eq/neq. u8 i8 u16 i16 are only for struct fields and array elements ((arr u8) for bytes); reading one gives an i32, writing one takes an i32.
4. Every operation is prefix: (+ a b), (lt a b), (and a b). Call user functions with (call f a b), never (f a b).
5. (let x:T v) declares and is void; (set! x v) assigns and is void. let is block-scoped; shadowing an outer name is an error. A name reused in a sibling scope must keep its type within the function.
6. (if c a b) always has three parts; both branches are void or both the same type. Use (block ...) to sequence. For an if with nothing to do otherwise, write (when c body...): it is (if c (block body...) (block)).
7. Loops: (for i 0 n body...) runs i from 0 up to n, n left out (n times; the end evaluated once), so (for i 0 (arr.len xs) ...) visits every element; (while cond body...); or (loop i start end step body...), where end is INCLUSIVE, for another step. (break) leaves the loop, (continue) goes to the next iteration, (return v) leaves the function. These are statements: write (if c (return v) (block)), never (if c (return v) x). For 3+ branches use (cond (test body...) ... (else body...)); else is required.
8. Literals: 42 is i32, 42i64 is i64, 1.5 is f64 (needs a dot), 1.5f32 is f32, "s" is str. Never mix number types without converting: (i64.extend_s x) / (i32.wrap x) between i32 and i64, (f64.promote x) / (f32.demote x) between f32 and f64, and f64.convert_i32_s, f32.convert_i32_s, i32.trunc_f64_s and the like between integers and floats.
9. Memory: every allocation names its allocator. (new S a) gives a zeroed (ptr S) from the alloc.Allocator a; (arr.new T a n) gives an (arr T) of n zeroed Ts. A function that allocates takes the allocator as a parameter, a:alloc.Allocator, and passes it on; the program picks one, usually in main: (call alloc.default) (a shared heap; free with (call (alloc.destroy S) a p)), (call alloc.forever) (never freed: what a program makes once and keeps), or an arena it resets. (new S) without one is an error. (ptr.null S) / (arr.null T) are typed nulls. Raw bytes come from (mem.alloc n), which is an i32; convert explicitly with (ptr.cast S addr) / (arr.cast T addr) (an unchecked pointer: no use-after-free check) and back with (ptr.addr p) / (arr.addr a). Never store to a literal address below 1024. mem.load*/mem.store* and the casts are unchecked: they need (unsafe ...) (rule 20), so prefer (new S a) and (arr.new T a n).
10. Structs: (get p S.f), (put p S.f v), (sizeof S), with p a (ptr S). Arrays: (arr.get T a i), (arr.set T a i v), (arr.len a), with a an (arr T). Every index is bounds-checked, compiled code included: an index out of range stops the program (`Array index out of bounds: ...`), so check it first where it may be out of range. Structs from an imported module m are m.S: (ptr m.S), (get p m.S.f).
11. Results: (ok:E v) / (err:T e) for a (result T E): each names the side it does not carry, always ((ok:str 5) for a (result i32 str)); consumed with (match_result r (ok v body...) (err e body...)); payloads may be any type. For a value that may be missing, use (import option): (option.Option T) with (make (option.Option i32) some 5) and (make (option.Option i32) none), never -1 or a null pointer. Pass a failure up instead of nesting match_result: (let v:i32 (try (call parse s))) gives the ok value or returns the error from a function that returns a (result R E) with the same E; (try_opt o) does the same for an Option in a function returning an Option; (try_or o err) turns a missing value into (err err). (defer (call buf.free b)), a statement of the function's body, runs when the function returns, however it returns. A branch that returns, breaks, or continues fits any type: (if ok v (return -1)).
12. Printing and formatting: (println "n = {n}, mean {mean:.2}") prints a line; (print ...) without the newline, (eprintln ...) to stderr; (format "...") makes a str and (format_to b "...") appends to a buf. In the string, {name} is a variable or constant, {(expression)} any expression (no string literal inside: let-bind it first), {{ and }} are braces; after a colon, a format: {n:>5} {n:05} {x:.3} {n:x} {label:<12}. Every value prints: numbers, bool, str, str.Bytes, and enums as themselves (these take formats like :>5), and a struct, array, union, or result as its contents, Row{n: 2, name: "a"}, [1, 2, 3], some{value: 5}, ok(4) (no format); not void or a function reference. To see a value while debugging, wrap it: (dbg (call f x)) prints file:line:col: (call f x) = 5 to stderr and gives the value, so it can sit anywhere an expression can (not inside a placeholder). Strings: there is no + and no eq on str. Join two with (call str.concat a b), compare with (call str.same a b); build text in a loop with (import buf) (buf.push_str, then buf.to_str once at the end). A str.Bytes is a view for reading and parsing (a value: making or slicing one allocates nothing, and reading one of a buffer that has been freed or has grown since stops the program, "Use after free", so take a view after building): (call str.from_str s) gives one, (call str.from_bytes b) copies one back into a str. On views: str.find, str.contains, str.starts_with, str.trim, str.split_once, str.parse_int, str.parse_i64; (import text) for text.split a b sep, text.fields, text.lines, text.join, text.replace, text.to_lower, text.parse_f64 (a is an alloc.Allocator); (import utf8) for one character at a time. sys.print takes str values only; print numbers with io.print_int and friends. Arithmetic is for numbers, % and bitwise ops for integers only, and lt/gt order numbers only (bool compares with eq/neq). (sys.time) and (sys.monotonic) are i64 nanoseconds; (sys.random ptr len) fills bytes. Threads: (import thread) and (call (thread.spawn_with (ptr Job)) (ref worker) job) with worker of type (fn [(ptr Job)] -> i32), so the worker's data keeps its type; (thread.join h) gives its result (both inside (unsafe ...)); atomic.add / atomic.cas / atomic.lock / atomic.unlock on 4-byte words from mem.alloc. Allocation is thread-safe. Threads, atomics, and sys.random need (unsafe ...) (rule 20); os.random_i32 does not.
13. Function values: (ref f) has type (fn [param types] -> ret); call one with (call_ref (fn [param types] -> ret) g args...). There are no closures, lambdas, or globals: a callback sees only its parameters, so data it needs is a context passed as its first parameter, as in (call (vec.sort_by_with i32 (arr i32)) order scores (ref by_score)) with by_score [scores:(arr i32) a:i32 b:i32] -> i32. Library functions that pass a context end in _with.
14. Use the standard library instead of hand-written loops: (import io) gives io.println, io.eprintln, io.print_int, io.println_int "label " n, io.read_file path -> str.Bytes (len -1 on failure), io.write_file, and io.read_path / io.write_path for a path held as str.Bytes; (import str) gives str.from_str, str.byte_at b i, str.set_byte b i c, str.slice b start len (a view, no copy), str.count_lines, str.count_words, str.find_byte, str.bytes_eq; every index is checked. (fmt.int_to_bytes and the other fmt writers, fmt.f64_fixed, str.bytes, and alloc.custom take raw addresses and are marked (unsafe); use buf instead.) Collections are generic (rule 16) and take an allocator when made, as in Zig (import alloc; (call alloc.default) is a shared heap): (import vec) gives (vec.Vec T) with (call (vec.make T) (call alloc.default) cap), (call (vec.free T) v) when done, (call (vec.push T) v x), (call (vec.at T) v i), vec.len, vec.pop, vec.set, (call (vec.sort T) v) by T's cmp.Ord (i32, i64, str), vec.sort_by with a comparator (ref f) of type (fn [T T] -> i32 (no_leak)) (f must not allocate memory it keeps), or vec.sort_by_with v ctx cmp when the comparison needs data of its own (cmp takes ctx first); (import map) gives (map.Map K V) keyed by any type with a cmp.Hash implementation (i32, i64, str, and str.Bytes by its contents come with std/cmp), with (call (map.make K V) a cap), free, set m k v, get_or m k default, find m k -> (option.Option V) (allocates nothing for a small V), has, remove, count, each named with both types, e.g. (call (map.get_or str.Bytes i32) m k 0); (import buf) string builder ((call buf.make a cap), buf.free, buf.push_str, buf.push_int, buf.bytes; buf.print writes it to stdout). str.parse_int parses decimal text (str.parse_int_or b default when a fallback will do). (import os) gives os.arg_int i default (argument i as a number, e.g. (call os.arg_int 1 1000)), os.arg_count, os.arg i (0 is the program; len -1 past the end), os.env "NAME" (len -1 if unset), and os.random_i32. io.read_stdin reads all of stdin. For a big input, or line by line, (import stream): (let r:(ptr stream.Reader) (try (call stream.open a "data.txt"))) (or stream.stdin a, stream.of_str a s), then (while (call stream.next_line r) (let line:str.Bytes (call stream.line r)) ...) and (call stream.close_reader r) (a (defer ...) is a good place for it); the line is good until the next read, so copy what you keep. Write with (let w:(ptr stream.Writer) (call stream.stdout a)) (or stream.create a path, stream.to_buf a b), then (format_to w "...") (w a variable), stream.write_str, stream.write_bytes, and end with (call stream.close_writer w), which flushes: a writer holds output until 8 KB, so without close_writer or flush the last of it is lost. (import fs) gives fs.list a "dir" -> (option.Option (ptr (vec.Vec str.Bytes))) (names sorted, no . or ..) and fs.info "path" -> (option.Option fs.Info) (a value struct) with (get i fs.Info.kind) (fs.Kind.file, directory, other), size and modified (i64, bytes and ns). (import json) gives (call json.parse a text) -> (result json.Value (ptr json.Error)) (strict; (get e json.Error.at) is the byte offset), json.get v "key" and json.at v i -> (option.Option json.Value), json.count, and json.write b v (compact text, as JSON.stringify); match a json.Value with (json.Value.number [n] ...), text [s], list [items], object [fields], boolean [b], null. There is no way to start another process (WASI has none). Numbers: io.print_i64 / io.println_i64, io.print_f64 x digits / io.println_f64 "label " x digits, buf.push_i64, buf.push_f64, fmt.f64_fixed. (import time) gives time.now (i64 ns), time.since start, time.report "label" start (to stderr). (import heap) frees single objects: (call (heap.create T) h), (call (heap.destroy T) h p), (call (heap.array T) h n), (call (heap.free_array T) h xs); a double free or a wrong size or type stops the program, and so does using any pointer to an object after it is destroyed ("Use after free"): do not keep one, and heap.live counts what is not freed. (import alloc) wraps any allocator as one value: alloc.of_heap, alloc.of_arena, alloc.fixed (unsafe to call), alloc.default, (call (alloc.custom (ptr Pool)) pool (ref my_alloc) (ref my_free)) with a typed state, with the same create/destroy/array/free_array. (import arena) is a region allocator: (call (arena.alloc T) a), arena.reset frees everything at once (a pointer used after it stops the program); arena.make_fast gives an arena.Fast (fast_alloc, fast_reset) that is faster but catches such a pointer only once its memory is reused. (import par) runs work in parallel with no unsafe: (call (par.map T U C) out (ref f) ctx xs threads) sets out[i] to (f ctx xs i), f a (fn [C (arr T) i32] -> U), each thread with its own copy of xs; T, U, and C must be plain (no pointers or arrays inside: numbers, str, enums, value structs). (import csv), (import stats) (Python's statistics, to the bit), (import set), (import random) (seeded, the same everywhere), (import uuid), (import path), (import cli), (import encode) (base64, hex, percent-encoding), (import hash) (CRC-32, SHA-256, HMAC, and more), (import regex) (linear-time regular expressions, matching as Python's re does), (import deflate) (DEFLATE, zlib, and gzip, both ways), (import url) (parse, resolve, query strings), (import http) (HTTP/1.1 messages, read and written), and (import html) (escape, unescape) cover data files, statistics, sets, random numbers, ids, paths, command-line options, encodings, hashes, pattern matching, compression, and the web's formats; json.get_path reads "a.b[2].c": `aipl doc MODULE` shows each. (import bigint) has arbitrary-precision integers changed in place, each from an allocator ((call bigint.from_i32 a n), add, sub, mul_small, div_small, compare, push_decimal, and bigint.free). Allocation grows memory by itself (up to 2 GiB); what (new S a) and (arr.new T a n) make is freed through the allocator a, except alloc.forever's, which, like mem.alloc's, is never freed.
14b. What alloc.forever hands out, mem.alloc, and run-time str values (format, str.concat, buf.to_str) are never freed. For code that runs for a long time (a request handler, a loop over a big input), take an alloc.Allocator, allocate through it, free or reset it, and mark the function (no_leak) after its return type: the checker then refuses anything in it, or in what it calls, that would make memory never freed, and names the call that does. A (no_leak) function may call through a reference only if the type says so: (fn [i32] -> i32 (no_leak)) takes only functions that keep no memory (marked, or proven by the checker), and call_ref through it with that same signature.
15. (and a b c ...) and (or a b c ...) short-circuit, left to right, and take two or more operands. (and (lt i n) (eq (arr.get i32 a i) x)) is a safe bounds guard.
16. Generics: (struct (Box T) [value:T]) and (fn (make T) [v:T] -> (ptr (Box T)) ...) are templates. Every use names the types: (ptr (Box i32)), (new (Box i32) a), (get b (Box i32) value), (put b (Box i32) value 5), (call (make i32) 5), (ref (make i32)). Type parameters start with an uppercase letter; generic names may not be built-in forms like get or put. A template is checked once with its parameters sealed: T supports only let, parameters, results, fields, (arr T), and sizeof. Anything more is asked for right after the signature: (where (cmp.Eq T)) then (call (cmp.Eq.same T) a b); (where (cmp.Ord T)) then (call (cmp.Ord.compare T) a b) (-1, 0, or 1); (where (cmp.Hash T)); (where (struct T)) to use (ptr T) or (new T a). Never (eq a b) or (lt a b) on a T. Pass a struct as (ptr S), never S, unless the parameter has (where (struct T)). Give your own type an interface with (impl (cmp.Eq (ptr Point)) (fn same [a:(ptr Point) b:(ptr Point)] -> bool ...)) in the module that defines Point: the interface's functions, with its parameter names and types.
17. Constants and enums: (const PAGE_SIZE:i32 65536) names a value (capitals; i32 i64 f64 bool str); use PAGE_SIZE anywhere. A constant may be computed from literals and other constants: (const POOL:i32 (* 4 PAGE_SIZE)). (enum Kind [red green (blue 10)]) is a new type with members Kind.red (0), Kind.green (1), Kind.blue (10); use Kind as a type ([k:Kind], (arr Kind), struct fields). Enums are not numbers: compare with eq/neq only, convert with (enum.ord k) -> i32 and (enum.cast Kind n). Imported: m.PAGE_SIZE, m.Kind, m.Kind.red. Use these instead of bare numbers or zero-argument functions for fixed values and codes.
18. Unions: (union Shape [(circle r:f64) (rect w:f64 h:f64) (dot)]) is a type whose value is one variant with that variant's fields. Build one with (make Shape.rect 2.0 3.0) (one value per field, in order); take it apart with (match s (Shape.circle [r] body...) (Shape.rect [w h] body...) (Shape.dot body...)): every variant needs an arm unless the last arm is (else body...), and an else that can never run is an error. Binders are new names for the variant's fields, in order; write _ for a field you do not need. All arms yield the same type. match also works on enums: (match k (Kind.red 1) (else 0)). Union values never compare (no eq) and are never null; use a variant like (none) for "no value". Every union is a value: make allocates nothing, so (option.Option T) is free to return. A union may not contain itself or need more than 8 numbers (a str and an enum are 1, a str.Bytes 3, a pointer 1): put a recursive or large variant's fields in a struct and make the variant a pointer to it, from an allocator you name, (union Tree [(leaf) (node n:(ptr Node))]) with (struct Node [l:Tree v:i32 r:Tree]); the checker names the field or variant to move. Use a union instead of a struct with a kind field whose other fields mean different things per kind. A generic union (union (Pair T) [(one a:T) (two a:T b:T)]) is used with its type arguments, then the variant: (make (Pair i32) one 5), and the arm ((Pair i32) two [a b] body...).
19. Integers: lt/lte/gt/gte are signed; ltu/lteu/gtu/gteu compare i32/i64 as unsigned (sizes, hashes). + - * wrap silently; checked.add / checked.sub / checked.mul stop the program on overflow instead, for values where wrapping would be a wrong answer.
20. Unchecked operations (mem.load*/mem.store*, ptr.cast, arr.cast, atomic.*, thread.*, fs.*, args.*, env.*, sys.random, and calls to functions marked (unsafe)) are rejected unless written inside (unsafe e...), a block in every other way, or in a function with the (unsafe) clause: (fn f [at:i32] -> void (unsafe) body...), which makes f itself unsafe to call. Wrap the operations in (unsafe ...) when the function is safe for any arguments; add the clause only when wrong arguments could break memory. Most programs need neither: use new, arrays, and the standard library.
21. Visibility: a module's items are private to it. Mark what other modules may use with (pub) right after the signature: (fn f [x:i32] -> i32 (pub) ...), (struct S [fields] (pub)), (enum E [members] (pub)), (union U [variants] (pub)), (const MAX:i32 9 (pub)). Fields are private unless written (read x:T) (others may get it) or (pub x:T) (get and put); others can (new S a) only if every field is (pub). A single-module program needs no marks.
22. Value structs: (struct Vec3 [x:f64 y:f64 z:f64] (value)) is a value, like an i32: no pointer, nothing allocated, copied where it goes, never changed. Make one with (make Vec3 1.0 2.0 3.0) (every field, in order); read with (get v Vec3.x); a changed copy is (with v Vec3.x 5.0). Its type is written Vec3, never (ptr Vec3), and there is no new, put, or eq for it. It may hold numbers, bool, enums, pointers, arrays, function references, and other value structs (not str, unions, or results), at most 8 numbers in all. Held in a pointer struct's field or an array it is inline, and a path reads or writes one field in place: (get p Body.vel Vec3.x), (put p Body.vel Vec3.x 0.0), (arr.get Body bs i Body.vel Vec3.x), (arr.set Body bs i Body.vel Vec3.x 0.0). Use a value struct for a small record of numbers passed around or returned (a point, a vector, a size and a time); use a pointer struct for anything larger, changed in place, or shared. (pub) goes after (value): (struct V [fields] (value) (pub)).
```

---

## Examples

### 1. Recursion with contracts
```lisp
(module math_demo
  (fn factorial [n:i32] -> i32
    (req (gte n 0))
    (ens (gt res 0))
    (if (lte n 1)
        1
        (* n (call factorial (- n 1))))))
```
`(call factorial 10)` is `3628800`.

### 2. Binary search over a heap array
```lisp
(module search
  (import alloc)
  ;; returns the index of target in the sorted array a, or -1
  (fn binary_search [a:(arr i32) target:i32] -> i32
    (let low:i32 0)
    (let high:i32 (- (arr.len a) 1))
    (while (lte low high)
      (let mid:i32 (/ (+ low high) 2))
      (let v:i32 (arr.get i32 a mid))
      (cond
        ((eq v target) (return mid))
        ((lt v target) (set! low (+ mid 1)))
        (else (set! high (- mid 1)))))
    -1)

  (fn main [] -> i32
    (let a:(arr i32) (arr.new i32 (call alloc.forever) 8))
    (loop i 0 7 1
      (arr.set i32 a i (* i 3)))                  ;; 0 3 6 ... 21
    (+ (* 10 (call binary_search a 15)) (call binary_search a 4))))
```
`main` returns `49`: 15 is found at index 5, and 4 is not found, giving `50 + -1`.

### 3. A struct-based linked list
```lisp
(module list_demo
  (import alloc)
  (struct Node [val:i32 next:(ptr Node)])

  (fn push [head:(ptr Node) v:i32] -> (ptr Node)
    (let n:(ptr Node) (new Node (call alloc.forever)))
    (put n Node.val v)
    (put n Node.next head)
    n)

  (fn sum [head:(ptr Node)] -> i32
    (let total:i32 0)
    (let cur:(ptr Node) head)
    (while (neq cur (ptr.null Node))
      (set! total (+ total (get cur Node.val)))
      (set! cur (get cur Node.next)))
    total)

  (fn main [] -> i32
    (let h:(ptr Node) (ptr.null Node))
    (loop i 1 10 1
      (set! h (call push h i)))
    (call sum h)))
```
`main` returns `55`.

### 4. Fallible parsing with results
```lisp
(module parse_demo
  (import str)
  (fn digit [c:i32] -> (result i32 i32)
    (if (and (gte c 48) (lte c 57))
        (ok:i32 (- c 48))
        (err:i32 c)))

  ;; parses the decimal digits of s; -1 on the first non-digit
  (fn parse_uint [s:str] -> i32
    (let b:str.Bytes (call str.from_str s))
    (let n:i32 0)
    (let bad:bool false)
    (loop i 0 (- (str.len s) 1) 1
      (match_result (call digit (call str.byte_at b i))
        (ok d (set! n (+ (* n 10) d)))
        (err e (set! bad true))))
    (if bad -1 n))

  (fn main [] -> i32
    (+ (call parse_uint "1234") (call parse_uint "12x"))))
```
`main` returns `1233`, which is `1234 + -1`.

### 5. Printing and files (compiled with WASI)
```lisp
(module io_demo
  (import io)
  (import str)
  (fn main [] -> i32
    (let n:i32 (call io.write_file "note.txt" (call str.from_str "hello from AIPL")))
    (if (lt n 0)
        (block (call io.eprintln "could not write note.txt") -1)
        (block (call io.println "wrote note.txt") n))))
```
Returns `15`. To run it compiled: `aipl compile io_demo.aipl -o io.wasm && aipl run io.wasm`, or `aipl compile --exe io_demo.aipl -o io_demo && ./io_demo`.

### 6. The standard library
```lisp
(module std_demo
  (import io)
  (import str)
  (fn main [] -> i32
    (let b:str.Bytes (call str.from_str "one two\nthree\n"))
    (call io.println_int "words: " (call str.count_words b))
    (call io.println_int "lines: " (call str.count_lines b))
    (+ (* 10 (call str.count_words b)) (call str.count_lines b))))
```
Prints `words: 3` and `lines: 2` and returns `32`. `(call io.read_file "input.txt")` gives the same `str.Bytes` for a file (see `examples/word_count.aipl`).

### 7. Generics: a typed list of structs
```lisp
(module generics_demo
  (import vec)
  (import alloc)
  (struct Point [x:i32 y:i32])
  (struct (Pair A B) [first:A second:B])

  (fn (swap A B) [p:(ptr (Pair A B))] -> (ptr (Pair B A))
    (let q:(ptr (Pair B A)) (new (Pair B A) (call alloc.forever)))
    (put q (Pair B A) first (get p (Pair A B) second))
    (put q (Pair B A) second (get p (Pair A B) first))
    q)

  (fn by_x [a:(ptr Point) b:(ptr Point)] -> i32 (- (get a Point.x) (get b Point.x)))

  (fn main [] -> i32
    (let pts:(ptr (vec.Vec (ptr Point))) (call (vec.make (ptr Point)) (call alloc.default) 0))
    (loop i 1 3 1
      (let p:(ptr Point) (new Point (call alloc.forever)))
      (put p Point.x (* i 10))
      (call (vec.push (ptr Point)) pts p))
    (call (vec.sort_by (ptr Point)) pts (ref by_x))
    (let pair:(ptr (Pair i32 bool)) (new (Pair i32 bool) (call alloc.forever)))
    (put pair (Pair i32 bool) first 7)
    (put pair (Pair i32 bool) second true)
    (let flipped:(ptr (Pair bool i32)) (call (swap i32 bool) pair))
    (+ (get (call (vec.at (ptr Point)) pts 0) Point.x) (get flipped (Pair bool i32) second))))
```
`main` returns `17`: the smallest `x` is `10`, and the swapped pair's `second` is `7`. Every instantiation (`vec.Vec<ptr<Point>>`, `Pair<i32,bool>`, `swap<i32,bool>`, ...) is an ordinary concrete struct or function after expansion.

### 8. Constants and enums
```lisp
(module tokens_demo
  (import alloc)
  (const MAX_TOKENS:i32 4)
  (enum Tok [num plus (minus 10)])

  (fn value [t:Tok] -> i32
    (cond
      ((eq t Tok.plus) 1)
      ((eq t Tok.minus) -1)
      (else 0)))

  (fn main [] -> i32
    (let ts:(arr Tok) (arr.new Tok (call alloc.forever) MAX_TOKENS))
    (arr.set Tok ts 0 Tok.plus)
    (arr.set Tok ts 1 Tok.plus)
    (arr.set Tok ts 2 Tok.minus)
    (arr.set Tok ts 3 Tok.num)
    (let sum:i32 0)
    (loop i 0 (- MAX_TOKENS 1) 1
      (set! sum (+ sum (call value (arr.get Tok ts i)))))
    (+ (* 100 sum) (enum.ord Tok.minus))))
```
`main` returns `110`: the values sum to `1`, and `Tok.minus` is `10`. `(+ t 1)` or `(lt t Tok.plus)` on a `Tok` is a type error.

### 9. Unions and match
```lisp
(module expr_demo
  (import alloc)
  ;; an arithmetic expression: a number, or an operation on expressions.
  ;; A union is a value, so a variant that holds expressions holds them
  ;; through a pointer to a struct, made from an allocator.
  (union Expr [(num v:i32) (add p:(ptr Pair)) (mul p:(ptr Pair)) (neg s:(ptr Sub))])
  (struct Pair [a:Expr b:Expr])
  (struct Sub [e:Expr])

  (fn pair [al:alloc.Allocator a:Expr b:Expr] -> (ptr Pair)
    (let p:(ptr Pair) (new Pair al))
    (put p Pair.a a)
    (put p Pair.b b)
    p)

  (fn sub [al:alloc.Allocator e:Expr] -> (ptr Sub)
    (let s:(ptr Sub) (new Sub al))
    (put s Sub.e e)
    s)

  (fn eval [e:Expr] -> i32
    (match e
      (Expr.num [v] v)
      (Expr.add [p] (checked.add (call eval (get p Pair.a)) (call eval (get p Pair.b))))
      (Expr.mul [p] (checked.mul (call eval (get p Pair.a)) (call eval (get p Pair.b))))
      (Expr.neg [s] (- 0 (call eval (get s Sub.e))))))

  (fn is_leaf [e:Expr] -> bool
    (match e (Expr.num [v] true) (else false)))

  (fn main [] -> i32
    (let al:alloc.Allocator (call alloc.default))
    ;; (2 + 3) * -(4)
    (let e:Expr (make Expr.mul (call pair al (make Expr.add (call pair al (make Expr.num 2) (make Expr.num 3)))
                                             (make Expr.neg (call sub al (make Expr.num 4))))))
    (+ (call eval e) (if (call is_leaf (make Expr.num 7)) 100 0))))
```
`main` returns `80`: `(2 + 3) * -4` is `-20`, plus `100` because a `num` is a leaf. A union that held `Expr` directly would contain itself, and the checker refuses it, naming the field; the pointer is the program's own allocation, from the allocator it names (`alloc.default` here; a server building one tree per request passes its request arena). The binder `p` is reused by two arms with the same type, which is allowed; binding `p` as an `f64` in one arm and a `(ptr Pair)` in another is not.

---

## Checklist before returning AIPL

- [ ] Parentheses balance, and nothing follows the module's closing `)`.
- [ ] Every `let`, parameter, return, and struct field has a type.
- [ ] Each function body ends in a value of its return type (not a `let`, `set!`, or loop), unless the return type is `void`.
- [ ] Both `if` branches are void, or both are the same type.
- [ ] No name is `let` twice in nested scopes.
- [ ] `for` leaves out its end: `(for i 0 n ...)` runs `n` times; `loop`'s end is inclusive: `(loop i 0 (- n 1) 1 ...)` runs `n` times.
- [ ] `return`/`break`/`continue` sit in statement positions (`(if c (return v) (block))`), and every `cond` ends with `(else ...)`.
- [ ] No mixed `i32`/`i64` operands; conversions are explicit.
- [ ] `and`/`or` have exactly two operands (nest for more).
- [ ] Fixed values and codes are `const`s and `enum`s, not bare numbers; enums are compared with `eq`/`neq`, never with arithmetic or `lt`.
- [ ] Data that is one of several shapes is a `union`, built with `make` and read with `match`; each `match` covers every variant or ends in `else`, and every arm yields the same type.
- [ ] A value struct (declared `(value)`) is written `V`, made with `(make V ...)`, changed by `(with v V.f x)`; a pointer struct is `(ptr S)`, made with `new`, changed by `put`.
- [ ] Pointers are `(ptr S)` and arrays `(arr T)`, never `i32`. `get`/`put` match the pointer's struct, `arr.get`/`arr.set` match the array's element type, and nulls are `(ptr.null S)` / `(arr.null T)`.
- [ ] Raw memory, casts, threads, and atomics sit inside `(unsafe ...)` or an `(unsafe)` function, and only where typed pointers, arrays, or the standard library cannot do the job.
- [ ] Every generic function or struct that compares, orders, hashes, or points to its `T` says so with a `where` clause right after its signature, and calls `(cmp.Eq.same T)` / `(cmp.Ord.compare T)` instead of `eq` / `lt`.
- [ ] In a program of several modules, everything another module uses is `(pub)`, and fields others read are `(read x:T)` (or `(pub x:T)` if they write them).
- [ ] Contracts are S-expressions such as `(req (gt n 0))`, and postconditions use `res`.

Look up a standard function before guessing it: `aipl doc text` lists a module's functions with their exact parameters and what they do, `aipl doc text.split` shows one, and `aipl doc --find split` searches every module. Write tests next to the code: (test "three fields" (let n:i32 (call parse_row "a,b,c")) (expect (eq n 3))) is an item of the module (it may call private functions), (expect c) checks a bool and shows both sides of a failed comparison, and `aipl test file.aipl` runs each test alone (-k NAME picks some); tests are left out of ordinary builds. Try an expression before building on it: `aipl eval -e '(call str.trim (call str.from_str "  x "))'` prints its value and type, and `aipl eval --with file.aipl -e '(call my_fn 3)'` runs it with that file's functions (private ones too). Validate with `aipl verify file.aipl`. Every error is `path: line:col: message`. A syntax error or an error in a definition is reported alone; otherwise each function with an error reports its first one, one per line, so fix them all and verify again. An `if` missing its else branch is reported as `Unexpected token parsing expression: ')'`.
