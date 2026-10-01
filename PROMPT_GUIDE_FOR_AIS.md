# PROMPT GUIDE FOR AI AGENTS: Generating AIPL

A system-prompt module and verified examples for LLMs writing **AIPL**. Every example below type-checks, runs in the VM, and compiles to wasm with the same result. The full language reference is [AIPL_SPEC.md](AIPL_SPEC.md); its section 13 lists the mistakes LLMs actually make.

---

## SYSTEM PROMPT MODULE (include in agent context)

```sysprompt
You write AIPL, a statically typed S-expression language that compiles to WebAssembly.

RULES:
1. One top-level (module <name> ...). Inside it: (import m), (struct S [f:type ...]), and (fn ...) forms.
2. Functions: (fn name [p:type ...] -> RetType (req ...)* (ens ...)* body...). The last body expression is the return value; `res` names it in (ens ...).
3. Types are mandatory everywhere: i32 i64 f32 f64 bool str void (result T E). Pointers, struct references, and array references are i32.
4. Every operation is prefix: (+ a b), (lt a b), (and a b). Call user functions with (call f a b), never (f a b).
5. (let x:T v) declares and is void; (set! x v) assigns and is void. let is block-scoped; shadowing an outer name is an error.
6. (if c a b) always has three parts; both branches are void or both the same type. Use (block ...) to sequence.
7. Loops: (while cond body...) or (loop i start end step body...), where end is INCLUSIVE. There is no return, break, or continue.
8. Literals: 42 is i32, 42i64 is i64, 1.5 is f64 (needs a dot), "s" is str. Never mix i32 and i64 without (i64.extend_s x) / (i32.wrap x).
9. Memory: get it from (mem.alloc n) or (new S) or (arr.new T n); never store to a literal address below 1024.
10. Structs: (get p S.f), (put p S.f v), (sizeof S). Arrays: (arr.get T p i), (arr.set T p i v); the element count is at p - 4.
11. Results: (ok v) / (err e), consumed with (match_result r (ok v body...) (err e body...)). Keep payloads 32-bit.
12. Code meant for `aipl compile` must not use thread.*, atomic.*, sys.time, or (+ str str); sys.print takes str only.
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
  (fn binary_search [a:i32 target:i32] -> i32
    (let low:i32 0)
    (let high:i32 (- (mem.load32 (- a 4)) 1))   ;; element count lives at a - 4
    (let found:i32 -1)
    (while (and (lte low high) (eq found -1))
      (let mid:i32 (/ (+ low high) 2))
      (let v:i32 (arr.get i32 a mid))
      (if (eq v target)
          (set! found mid)
          (if (lt v target)
              (set! low (+ mid 1))
              (set! high (- mid 1)))))
    found)

  (fn main [] -> i32
    (let a:i32 (arr.new i32 8))
    (loop i 0 7 1
      (arr.set i32 a i (* i 3)))                  ;; 0 3 6 ... 21
    (+ (* 10 (call binary_search a 15)) (call binary_search a 4))))
```
`main` returns `49`: 15 is found at index 5, and 4 is not found, giving `50 + -1`.

### 3. A struct-based linked list
```lisp
(module list_demo
  (struct Node [val:i32 next:i32])

  (fn push [head:i32 v:i32] -> i32
    (let n:i32 (new Node))
    (put n Node.val v)
    (put n Node.next head)
    n)

  (fn sum [head:i32] -> i32
    (let total:i32 0)
    (let cur:i32 head)
    (while (neq cur 0)
      (set! total (+ total (get cur Node.val)))
      (set! cur (get cur Node.next)))
    total)

  (fn main [] -> i32
    (let h:i32 0)
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
Returns `15`. To run it compiled: `aipl compile io_demo.aipl -o io.wasm && wasmtime run --dir=. io.wasm --invoke main`.

---

## Checklist before returning AIPL

- [ ] Parentheses balance, and nothing follows the module's closing `)`.
- [ ] Every `let`, parameter, return, and struct field has a type.
- [ ] Each function body ends in a value of its return type (not a `let`, `set!`, or loop), unless the return type is `void`.
- [ ] Both `if` branches are void, or both are the same type.
- [ ] No name is `let` twice in nested scopes.
- [ ] `loop` end bounds are inclusive: `(loop i 0 (- n 1) 1 ...)` runs `n` times.
- [ ] No mixed `i32`/`i64` operands; conversions are explicit.
- [ ] Contracts are S-expressions such as `(req (gt n 0))`, and postconditions use `res`.
- [ ] Code meant to compile avoids VM-only ops (AIPL_SPEC.md 6.3).

Validate with `aipl verify file.aipl`. Every error starts with `line:col:`.
