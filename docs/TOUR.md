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
    (+ (call square 3) (call square 4))))
```

It prints `7 squared is 49` and returns `25`.

Everything is a parenthesized form with its operator first: `(* x x)`, not
`x * x`. There is no operator precedence to remember, because there are no
infix operators. Calling your own function is always `(call f args...)`,
which keeps a user function apart from a built-in operation like `+`. A
function's last expression is its value.

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
    (i32.wrap (i64.trunc_f64_s (* avg 2.0)))))
```

It returns `43`.

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
    (loop i 1 n 1                     ;; i = 1, 2, ..., n (the end is included)
      (if (eq (% i 2) 0)
          (set! total (+ total i))
          (block)))
    total)

  (fn sign [x:i32] -> i32
    (cond
      ((lt x 0) -1)
      ((eq x 0) 0)
      (else 1)))

  (fn main [] -> i32
    (+ (call sum_even 10) (call sign -5))))
```

It returns `29`: the even numbers up to 10 add to 30, and the sign of -5 is
-1.

`if` always has both branches; `(block)` is an empty one. `loop` counts
from its start to its end, both included, by its step; `while` repeats
while a condition holds. `cond` picks the first clause whose test is true,
and must end with `else`. Comparisons are words: `eq`, `neq`, `lt`, `lte`,
`gt`, `gte`.

## 4. Structs, pointers, and arrays

```lisp
(module tour_structs
  (struct Point [x:i32 y:i32])

  (fn make_point [x:i32 y:i32] -> (ptr Point)
    (let p:(ptr Point) (new Point))
    (put p Point.x x)
    (put p Point.y y)
    p)

  (fn total [ps:(arr (ptr Point))] -> i32
    (let sum:i32 0)
    (loop i 0 (- (arr.len ps) 1) 1
      (let p:(ptr Point) (arr.get (ptr Point) ps i))
      (set! sum (+ sum (+ (get p Point.x) (get p Point.y)))))
    sum)

  (fn main [] -> i32
    (let ps:(arr (ptr Point)) (arr.new (ptr Point) 3))
    (loop i 0 2 1
      (arr.set (ptr Point) ps i (call make_point i (* i 10))))
    (call total ps)))
```

It returns `33`: the points are (0, 0), (1, 10), and (2, 20).

`(new Point)` makes a struct and gives a `(ptr Point)`. A pointer to one
struct can never be used as another, or as a number. `get` and `put` name
the field as `Struct.field`. An `(arr T)` holds values of type `T`, and
`arr.get` and `arr.set` repeat the element type. Every index is checked: an
index past the end stops the program with a message, compiled code
included.

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
    (if (and (gte c 48) (lte c 57)) (ok (- c 48)) (err c)))

  (fn main [] -> i32
    (let a:f64 (+ (call area (make Shape.rect 2.0 3.0)) (call area (make Shape.circle 1.0))))
    (let d:i32 (match_result (call digit 55)
                 (ok v v)
                 (err c -1)))
    (+ (i32.wrap (i64.trunc_f64_s a)) (* 10 d))))
```

It returns `79`: the areas add to 9, and the character 55 (`'7'`) is the
digit 7.

A union is a value that is exactly one of several variants, each with its
own fields. `make` builds one; `match` takes it apart, and must handle every
variant (or end with `else`). A `(result T E)` is either `(ok value)` or
`(err error)`, read with `match_result`. There are no exceptions: a
function that can fail says so in its type.

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
    (+ (call isqrt 99) (call isqrt 100))))
```

It returns `19` (9 + 10).

`req` states what must be true when the function is called, and `ens` what
must be true when it returns, with `res` naming the result. They are checked
on every call, in the interpreter and in compiled code. A call that breaks
one stops the program with the contract, the values, and where it happened.
Changing the second call to `(call isqrt -4)` and running the compiled
program gives:

```
./tour: Pre-condition failed in 'isqrt': (req (gte n 0)) with n = -4
  at isqrt (tour.aipl:4:10)
  at main (tour.aipl:12:24)
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
    (+ first n)))
```

It returns `13`: after sorting, the first element is 10, and there are 3.

`vec.Vec` is a generic growable list. Every use names its type, as in
`(vec.push i32)`: wordy, but nothing is inferred, so nothing is guessed.
Collections take an allocator when they are made, as in Zig, and are freed
explicitly; `(call alloc.default)` is a shared general-purpose heap.
`(ref by_value)` passes a function as a value, here the comparison for
sorting. The standard library also has hash maps, a string builder, file
I/O, number formatting, big integers, and timing.

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
