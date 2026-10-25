# A ten-minute tour of AIPL

This page walks through the language with small programs you can run. It
is for people; the [prompt guide](../PROMPT_GUIDE_FOR_AIS.md) is the
version written for models, and the [spec](../AIPL_SPEC.md) has every rule.
Every example here is checked by the test suite: each `main` returns the
number given below it, in the interpreter and compiled.

To follow along, build AIPL (see the [README](../README.md#install)), save
an example as `tour.aipl`, and run it with `aipl eval tour.aipl`.

## 1. A first program

```lisp
(module tour_hello
  (import io)

  (fn square [x:i32] -> i32
    (* x x))

  (fn main [] -> i32
    (call io.println_int "7 squared is " (call square 7))
    (call io.println_int "3 squared plus 4 squared is " (+ (call square 3) (call square 4)))
    0))
```

It prints `7 squared is 49` and `3 squared plus 4 squared is 25`.

Everything is a parenthesized form with its operator first: `(* x x)`, not
`x * x`. There is no operator precedence to remember, because there are no
infix operators. Calling your own function is always `(call f args...)`,
which keeps a user function apart from a built-in operation like `+`. A
function's last expression is its value. `main`'s value is the program's
exit status, 0 for success, so a program prints its answers and returns 0.

A file is one `(module name ...)`. `(import io)` brings in a module from the
standard library; its functions are then `io.something`.

## 2. Types are written out

```lisp
(module tour_types
  (fn average [a:i64 b:i64] -> f64
    (/ (f64.convert_i64_s (+ a b)) 2.0))

  (fn main [] -> i32
    (let total:i32 40)
    (let big:i64 (i64.extend_s total))
    (let avg:f64 (call average big 3i64))
    (println "the average of {total} and 3 is {avg}")
    0))
```

It prints `the average of 40 and 3 is 21.5`.

Every parameter, return value, and local has its type: `i32`, `i64`, `f32`,
`f64`, `bool`, `str`. Literals say their type too: `40` is an `i32`, `3i64`
an `i64`, `2.0` an `f64`. Nothing converts on its own, so `(+ a b)` with an
`i32` and an `i64` is an error, and each conversion is named
(`i64.extend_s`, `f64.convert_i64_s`, `i32.wrap`). `let` declares a local;
`set!` assigns to one.

## 3. Control flow

```lisp
(module tour_control
  ;; the sum of the even numbers from 1 to n
  (fn sum_even [n:i32] -> i32
    (let total:i32 0)
    (for i 1 (+ n 1)                     ;; i = 1, 2, ..., n (the end, n + 1, is left out)
      (when (eq (% i 2) 0)
          (set! total (+ total i))))
    total)

  (fn sign [x:i32] -> i32
    (cond
      ((lt x 0) -1)
      ((eq x 0) 0)
      (else 1)))

  (fn main [] -> i32
    (println "evens up to 10 add to {(call sum_even 10)}; the sign of -5 is {(call sign -5)}")
    0))
```

It prints `evens up to 10 add to 30; the sign of -5 is -1`.

`if` always has both branches; `(block)` is an empty one. `for` counts
from its start up to its end, the end left out; `while` repeats
while a condition holds (and is the loop for any other step). `cond` picks the first clause whose test is true,
and must end with `else`. Comparisons are words: `eq`, `neq`, `lt`, `lte`,
`gt`, `gte`.

## 4. Structs, pointers, and arrays

```lisp
(module tour_structs
  (import alloc)
  (struct Point [x:i32 y:i32])

  (fn make_point [x:i32 y:i32] -> (ptr Point)
    (let p:(ptr Point) (new Point (call alloc.forever)))
    (put p Point.x x)
    (put p Point.y y)
    p)

  (fn total [ps:(arr (ptr Point))] -> i32
    (let sum:i32 0)
    (for i 0 (arr.len ps)
      (let p:(ptr Point) (arr.get (ptr Point) ps i))
      (set! sum (+ sum (+ (get p Point.x) (get p Point.y)))))
    sum)

  (fn main [] -> i32
    (let ps:(arr (ptr Point)) (arr.new (ptr Point) (call alloc.forever) 3))
    (for i 0 3
      (arr.set (ptr Point) ps i (call make_point i (* i 10))))
    (println "total: {(call total ps)}")
    0))
```

It prints `total: 33`: the points are (0, 0), (1, 10), and (2, 20).

`(new Point a)` makes a struct from the allocator `a` and gives a `(ptr
Point)`; every allocation names where its memory comes from, and
`(call alloc.forever)` is memory kept until the program exits (a function
that allocates usually takes the allocator as a parameter, so its caller
decides). A pointer to one
struct can never be used as another, or as a number. `get` and `put` name
the field as `Struct.field`. An `(arr T)` holds values of type `T`, and
`arr.get` and `arr.set` repeat the element type. Every index is checked: an
index past the end stops the program with a message, compiled code
included.

A struct declared `(value)` is a value instead, like a number: no pointer,
nothing allocated, copied where it goes, never changed in place.

```lisp
(module tour_values
  (import alloc)
  (struct Point [x:i32 y:i32] (value))

  (fn shift [p:Point dx:i32] -> Point
    (with p Point.x (+ (get p Point.x) dx)))

  (fn main [] -> i32
    (let ps:(arr Point) (arr.new Point (call alloc.forever) 3))
    (for i 0 3 (arr.set Point ps i (make Point i (* i 10))))
    (let q:Point (call shift (arr.get Point ps 2) 5))
    (println "shifted: ({(get q Point.x)}, {(get q Point.y)}); second y: {(arr.get Point ps 1 Point.y)}")
    0))
```

It prints `shifted: (7, 20); second y: 10`: the third point moved 5 to the
right, and the second point's `y`. `make` gives every field in order, `with` makes a copy with one
field changed, and the array holds the points themselves, not pointers to
them. A value struct holds at most eight numbers; larger records, and
anything changed in place, stay behind pointers.

## 5. Unions and results

```lisp
(module tour_results
  (union Shape [(circle r:f64) (rect w:f64 h:f64) (dot)])

  (fn area [s:Shape] -> f64
    (match s
      (Shape.circle [r] (* 3.0 (* r r)))
      (Shape.rect [w h] (* w h))
      (Shape.dot 0.0)))

  ;; a digit's value, or the character that was not a digit
  (fn digit [c:i32] -> (result i32 i32)
    (if (and (gte c 48) (lte c 57)) (ok:i32 (- c 48)) (err:i32 c)))

  (fn main [] -> i32
    (let a:f64 (+ (call area (make Shape.rect 2.0 3.0)) (call area (make Shape.circle 1.0))))
    (let d:i32 (match_result (call digit 55)
                 (ok v v)
                 (err c -1)))
    (println "areas: {a}, digit: {d}")
    0))
```

It prints `areas: 9.0, digit: 7`: the areas add to 9, and the character 55
(`'7'`) is the digit 7.

A union is a value that is exactly one of several variants, each with its
own fields. `make` builds one; `match` takes it apart, and must handle every
variant (or end with `else`). A `(result T E)` is either `(ok value)` or
`(err error)`, read with `match_result`. There are no exceptions: a
function that can fail says so in its type. A union can be generic, like
`std/option`'s `(option.Option T)`, for a value that may be missing:
`(make (option.Option i32) some 5)`, matched with
`((option.Option i32) some [v] ...)`.

## 6. Contracts

```lisp
(module tour_contracts
  ;; integer square root: the largest r with r * r <= n
  (fn isqrt [n:i32] -> i32
    (req (gte n 0))
    (ens (and (lte (* res res) n) (gt (* (+ res 1) (+ res 1)) n)))
    (let r:i32 0)
    (while (lte (* (+ r 1) (+ r 1)) n)
      (set! r (+ r 1)))
    r)

  (fn main [] -> i32
    (let a:i32 (call isqrt 99))
    (let b:i32 (call isqrt 100))
    (println "isqrt 99 = {a}, isqrt 100 = {b}")
    0))
```

It prints `isqrt 99 = 9, isqrt 100 = 10`.

`req` states what must be true when the function is called, and `ens` what
must be true when it returns, with `res` naming the result. They are checked
on every call, in the interpreter and in compiled code. A call that breaks
one stops the program with the contract, the values, and where it happened.
Changing the second call to `(call isqrt -4)` and running the compiled
program gives:

```
./tour: Pre-condition failed in 'isqrt': (req (gte n 0)) with n = -4
  at isqrt (tour.aipl:4:10)
  at main (tour.aipl:13:16)
```

## 7. Generics and the standard library

```lisp
(module tour_generics
  (import vec)
  (import alloc)

  (fn by_value [a:i32 b:i32] -> i32 (- a b))

  (fn main [] -> i32
    (let v:(ptr (vec.Vec i32)) (call (vec.make i32) (call alloc.default) 4))
    (call (vec.push i32) v 30)
    (call (vec.push i32) v 10)
    (call (vec.push i32) v 20)
    (call (vec.sort_by i32) v (ref by_value))
    (let first:i32 (call (vec.at i32) v 0))
    (let n:i32 (call (vec.len i32) v))
    (call (vec.free i32) v)
    (println "first {first}, {n} elements")
    0))
```

It prints `first 10, 3 elements`: the smallest after sorting.

`vec.Vec` is a generic growable list. Every use names its type, as in
`(vec.push i32)`: wordy, but nothing is inferred, so nothing is guessed.
Collections take an allocator when they are made, as in Zig, and are freed
explicitly; `(call alloc.default)` is a shared general-purpose heap.
`(ref by_value)` passes a function as a value, here the comparison for
sorting. A generic function that compares its `T` says so, with
`(where (cmp.Ord T))` after its signature, and calls
`(call (cmp.Ord.compare T) a b)`: a template is checked once, on its own,
so it may use only what its `where` clauses grant. The standard library
also has hash maps (`(map.Map K V)`, for any key with `cmp.Hash`), a string
builder, file I/O, number formatting, big integers, and timing.

## 8. Compiling, and what errors look like

The same program runs three ways:

```bash
aipl eval tour.aipl                     # in the interpreter
aipl compile tour.aipl -o tour.wasm     # to WebAssembly...
aipl run tour.wasm                      # ...run by the launcher
aipl compile --exe tour.aipl -o tour    # to a native executable (Linux x86-64)
./tour
```

`aipl verify` checks a file without running it. Mistakes are reported with
the file, line, and column, one line for each function that has one. Here
one function adds an `i32` to an `i64`, and another misspells a variable:

```
Error: bad.aipl: 3:5: Type mismatch in binary op: i32 vs i64
bad.aipl: 6:5: Undefined variable 'totl' in set!
```

A syntax error is reported on its own, since nothing after it can be read
reliably. The most common one is an `if` with no else branch, where the
parser finds `)` where the else should be:

```
Error: bad.aipl: 3:19: Unexpected token parsing expression: ')'
```

## Where next

- [examples/](../examples/): small complete programs, from quicksort to a
  word counter that reads files.
- [AIPL_SPEC.md](../AIPL_SPEC.md): the full language; its section 13 lists
  the mistakes models make most often, with the fix for each.
- [FAQ.md](FAQ.md): why the language looks the way it does.
