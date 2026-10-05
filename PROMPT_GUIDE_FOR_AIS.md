# PROMPT GUIDE FOR AI AGENTS: Generating AIPL

A system-prompt module and verified examples for LLMs writing **AIPL**. Every example below type-checks, runs in the VM, and compiles to wasm with the same result. The full language reference is [AIPL_SPEC.md](AIPL_SPEC.md); its section 13 lists the mistakes LLMs actually make.

---

## SYSTEM PROMPT MODULE (include in agent context)

```sysprompt
You write AIPL, a statically typed S-expression language that compiles to WebAssembly.

RULES:
1. One top-level (module <name> ...). Inside it: (import m), (struct S [f:type ...]), and (fn ...) forms.
2. Functions: (fn name [p:type ...] -> RetType (req ...)* (ens ...)* body...). The last body expression is the return value; `res` names it in (ens ...). Contracts run only in the VM (aipl eval / aipl test); compiled code does not check them, so validate inputs explicitly where a compiled program must not misbehave.
3. Types are mandatory everywhere: i32 i64 f32 f64 bool str void (result T E) (ptr S) (arr T). (ptr S) points at a struct S; (arr T) is an array of T. Neither is an integer: no arithmetic, compare only with eq/neq.
4. Every operation is prefix: (+ a b), (lt a b), (and a b). Call user functions with (call f a b), never (f a b).
5. (let x:T v) declares and is void; (set! x v) assigns and is void. let is block-scoped; shadowing an outer name is an error.
6. (if c a b) always has three parts; both branches are void or both the same type. Use (block ...) to sequence.
7. Loops: (while cond body...) or (loop i start end step body...), where end is INCLUSIVE. (break) leaves the loop, (continue) goes to the next iteration, (return v) leaves the function. These are statements: write (if c (return v) (block)), never (if c (return v) x). For 3+ branches use (cond (test body...) ... (else body...)); else is required.
8. Literals: 42 is i32, 42i64 is i64, 1.5 is f64 (needs a dot), "s" is str. Never mix i32 and i64 without (i64.extend_s x) / (i32.wrap x).
9. Memory: (new S) gives a (ptr S); (arr.new T n) gives an (arr T); (ptr.null S) / (arr.null T) are typed nulls. Raw bytes come from (mem.alloc n), which is an i32; convert explicitly with (ptr.cast S addr) / (arr.cast T addr) and back with (ptr.addr p) / (arr.addr a). Never store to a literal address below 1024.
10. Structs: (get p S.f), (put p S.f v), (sizeof S), with p a (ptr S). Arrays: (arr.get T a i), (arr.set T a i v), (arr.len a), with a an (arr T). Compiled code does not bounds-check arrays (only the VM does): an index out of range silently reads or overwrites other data. Structs from an imported module m are m.S: (ptr m.S), (get p m.S.f).
11. Results: (ok v) / (err e), consumed with (match_result r (ok v body...) (err e body...)). Keep payloads 32-bit.
12. Code meant for `aipl compile` must not use (+ str str); sys.print takes str only. (sys.time) and (sys.monotonic) are i64 nanoseconds; (sys.random ptr len) fills bytes. Threads: (thread.spawn (ref worker) arg) with worker of type (fn [i32] -> i32), (thread.join h) gives its result; atomic.add / atomic.cas / atomic.lock / atomic.unlock on 4-byte words from mem.alloc. Allocation is thread-safe.
13. Function values: (ref f) has type (fn [param types] -> ret); call one with (call_ref (fn [param types] -> ret) g args...). There are no closures.
14. Use the standard library instead of hand-written loops: (import io) gives io.println, io.eprintln, io.print_int, io.println_int "label " n, io.read_file path -> (ptr str.Bytes) (len -1 on failure), io.write_file, and io.read_path / io.write_path for a path held as (ptr str.Bytes); (import str) gives str.from_str, str.count_lines, str.count_words, str.find_byte, str.bytes_eq; (import fmt) gives fmt.int_to_bytes, fmt.uint_to_bytes, fmt.hex_to_bytes. Collections are generic (rule 16): (import vec) gives (vec.Vec T) with (call (vec.make T) cap), (call (vec.push T) v x), (call (vec.at T) v i), vec.len, vec.pop, vec.set, vec.sort_by with a (fn [T T] -> i32) comparator; (import map) gives (map.Map V) keyed by i32 and (import strmap) gives (strmap.StrMap V) keyed by (ptr str.Bytes), both with make, set m k v, get_or m k default, has, remove, count; (import buf) string builder (buf.push_str, buf.push_int, buf.bytes). str.parse_int parses decimal text. (import os) gives os.arg_count, os.arg i (0 is the program; len -1 past the end), os.env "NAME" (len -1 if unset), and os.random_i32. io.read_stdin reads all of stdin. Numbers: io.print_i64 / io.println_i64, io.print_f64 x digits / io.println_f64 "label " x digits, buf.push_i64, buf.push_f64, fmt.f64_fixed. (import time) gives time.now (i64 ns), time.since start, time.report "label" start (to stderr). (import arena) is a region allocator: (call (arena.alloc T) a), arena.reset frees everything at once. (import bigint) has arbitrary-precision integers changed in place (bigint.from_i32, add, sub, mul_small, div_small, compare, push_decimal). Allocation grows memory by itself (up to 64 MiB); nothing else is ever freed.
15. (and a b) and (or a b) short-circuit and take exactly two operands; nest for more: (and a (and b c)). (and (lt i n) (eq (arr.get i32 a i) x)) is a safe bounds guard.
16. Generics: (struct (Box T) [value:T]) and (fn (make T) [v:T] -> (ptr (Box T)) ...) are templates. Every use names the types: (ptr (Box i32)), (new (Box i32)), (get b (Box i32) value), (put b (Box i32) value 5), (call (make i32) 5), (ref (make i32)). Type parameters start with an uppercase letter; generic names may not be built-in forms like get or put.
17. Constants and enums: (const PAGE_SIZE:i32 65536) names a literal (capitals; i32 i64 f64 bool str); use PAGE_SIZE anywhere. (enum Kind [red green (blue 10)]) is a new type with members Kind.red (0), Kind.green (1), Kind.blue (10); use Kind as a type ([k:Kind], (arr Kind), struct fields). Enums are not numbers: compare with eq/neq only, convert with (enum.ord k) -> i32 and (enum.cast Kind n). Imported: m.PAGE_SIZE, m.Kind, m.Kind.red. Use these instead of bare numbers or zero-argument functions for fixed values and codes.
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
    (let a:(arr i32) (arr.new i32 8))
    (loop i 0 7 1
      (arr.set i32 a i (* i 3)))                  ;; 0 3 6 ... 21
    (+ (* 10 (call binary_search a 15)) (call binary_search a 4))))
```
`main` returns `49`: 15 is found at index 5, and 4 is not found, giving `50 + -1`.

### 3. A struct-based linked list
```lisp
(module list_demo
  (struct Node [val:i32 next:(ptr Node)])

  (fn push [head:(ptr Node) v:i32] -> (ptr Node)
    (let n:(ptr Node) (new Node))
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
  (fn digit [c:i32] -> (result i32 i32)
    (if (and (gte c 48) (lte c 57))
        (ok (- c 48))
        (err c)))

  ;; parses the decimal digits of s; -1 on the first non-digit
  (fn parse_uint [s:str] -> i32
    (let p:i32 (str.ptr s))
    (let n:i32 0)
    (let bad:bool false)
    (loop i 0 (- (str.len s) 1) 1
      (match_result (call digit (mem.load8 (+ p i)))
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
  (fn main [] -> i32
    (let path:str "note.txt")
    (let msg:str "hello from AIPL")
    (let fd:i32 (fs.open (str.ptr path) (str.len path) 1))
    (if (lt fd 0)
        (block (sys.print "open failed") -1)
        (block
          (let n:i32 (fs.write fd (str.ptr msg) (str.len msg)))
          (fs.close fd)
          (sys.print "wrote note.txt")
          n))))
```
Returns `15`. To run it compiled: `aipl compile io_demo.aipl -o io.wasm && aipl run io.wasm`, or `aipl compile --exe io_demo.aipl -o io_demo && ./io_demo`.

### 6. The standard library
```lisp
(module std_demo
  (import io)
  (import str)
  (fn main [] -> i32
    (let b:(ptr str.Bytes) (call str.from_str "one two\nthree\n"))
    (call io.println_int "words: " (call str.count_words b))
    (call io.println_int "lines: " (call str.count_lines b))
    (+ (* 10 (call str.count_words b)) (call str.count_lines b))))
```
Prints `words: 3` and `lines: 2` and returns `32`. `(call io.read_file "input.txt")` gives the same `(ptr str.Bytes)` for a file (see `examples/word_count.aipl`).

### 7. Generics: a typed list of structs
```lisp
(module generics_demo
  (import vec)
  (struct Point [x:i32 y:i32])
  (struct (Pair A B) [first:A second:B])

  (fn (swap A B) [p:(ptr (Pair A B))] -> (ptr (Pair B A))
    (let q:(ptr (Pair B A)) (new (Pair B A)))
    (put q (Pair B A) first (get p (Pair A B) second))
    (put q (Pair B A) second (get p (Pair A B) first))
    q)

  (fn by_x [a:(ptr Point) b:(ptr Point)] -> i32 (- (get a Point.x) (get b Point.x)))

  (fn main [] -> i32
    (let pts:(ptr (vec.Vec (ptr Point))) (call (vec.make (ptr Point)) 0))
    (loop i 1 3 1
      (let p:(ptr Point) (new Point))
      (put p Point.x (* i 10))
      (call (vec.push (ptr Point)) pts p))
    (call (vec.sort_by (ptr Point)) pts (ref by_x))
    (let pair:(ptr (Pair i32 bool)) (new (Pair i32 bool)))
    (put pair (Pair i32 bool) first 7)
    (put pair (Pair i32 bool) second true)
    (let flipped:(ptr (Pair bool i32)) (call (swap i32 bool) pair))
    (+ (get (call (vec.at (ptr Point)) pts 0) Point.x) (get flipped (Pair bool i32) second))))
```
`main` returns `17`: the smallest `x` is `10`, and the swapped pair's `second` is `7`. Every instantiation (`vec.Vec<ptr<Point>>`, `Pair<i32,bool>`, `swap<i32,bool>`, ...) is an ordinary concrete struct or function after expansion.

### 8. Constants and enums
```lisp
(module tokens_demo
  (const MAX_TOKENS:i32 4)
  (enum Tok [num plus (minus 10)])

  (fn value [t:Tok] -> i32
    (cond
      ((eq t Tok.plus) 1)
      ((eq t Tok.minus) -1)
      (else 0)))

  (fn main [] -> i32
    (let ts:(arr Tok) (arr.new Tok MAX_TOKENS))
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

---

## Checklist before returning AIPL

- [ ] Parentheses balance, and nothing follows the module's closing `)`.
- [ ] Every `let`, parameter, return, and struct field has a type.
- [ ] Each function body ends in a value of its return type (not a `let`, `set!`, or loop), unless the return type is `void`.
- [ ] Both `if` branches are void, or both are the same type.
- [ ] No name is `let` twice in nested scopes.
- [ ] `loop` end bounds are inclusive: `(loop i 0 (- n 1) 1 ...)` runs `n` times.
- [ ] `return`/`break`/`continue` sit in statement positions (`(if c (return v) (block))`), and every `cond` ends with `(else ...)`.
- [ ] No mixed `i32`/`i64` operands; conversions are explicit.
- [ ] `and`/`or` have exactly two operands (nest for more).
- [ ] Fixed values and codes are `const`s and `enum`s, not bare numbers; enums are compared with `eq`/`neq`, never with arithmetic or `lt`.
- [ ] Pointers are `(ptr S)` and arrays `(arr T)`, never `i32`. `get`/`put` match the pointer's struct, `arr.get`/`arr.set` match the array's element type, and nulls are `(ptr.null S)` / `(arr.null T)`.
- [ ] Contracts are S-expressions such as `(req (gt n 0))`, and postconditions use `res`.
- [ ] Code meant to compile avoids VM-only ops (AIPL_SPEC.md 6.3).

Validate with `aipl verify file.aipl`. Every error carries `line:col:` (syntax errors are also prefixed with the file path), and only the first error is reported, so fix it and verify again. An `if` missing its else branch is reported as `Unexpected token parsing expression: RParen`.
