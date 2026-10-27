# Using AIPL: build, run, test

How to write, build, run, and test a program with the `aipl` command, and
how to read what it tells you. The language's forms and the mistakes models
make most are in [PROMPT_GUIDE_FOR_AIS.md](../PROMPT_GUIDE_FOR_AIS.md); read that
first. This page is the workflow, the library map, and the spelling of the
operators (section 8). It leaves out how the compiler works.

## 1. The loop

```bash
aipl verify prog.aipl                       # type-check; every error, as FILE:LINE:COL: message
aipl compile prog.aipl -o prog.wasm         # build it
aipl-run prog.wasm < input.txt              # run it (standard input and output as usual)
aipl test prog.aipl                         # run the (test ...) items in it
aipl doc --find WORD                        # look for a library function before writing one
```

`aipl run prog.wasm -- ARG...` is the same runner with arguments. `aipl`
and `aipl-run` are two programs; keep them together.

## 2. A program's shape

One file is one `(module name ...)`. `main` is the entry point, and its
value is the exit status:

| `main` returns | Meaning |
|---|---|
| `-> i32` | the value is the exit status (0 is success) |
| `-> (result i32 str)` | `(ok:str 0)` exits 0; `(err:i32 "why")` prints `why` on standard error and exits 1 |
| `-> void` | exit status 0 |

A program's answer is what it prints: `(println "n = {n}")` writes a line
to standard output, `(eprintln ...)` to standard error. Never return an
answer as `main`'s value.

Input: `(call io.read_stdin)` reads all of standard input, as a result of a
`str.Bytes` (a view of the bytes); `(call os.arg 1)` is the first
command-line argument, an `(option.Option str.Bytes)`; `(call os.env "HOME")`
likewise; `(call io.read_file "path")` reads a file. A program sees the
directory it runs in, nothing else it was not given.

This program reads lines from standard input and prints each one upper-cased
and numbered. It shows the usual shape: an allocator made in `main` and
passed down, the input read once, a failure turned into `main`'s `err`.

```lisp
(module use_lines
  (import alloc)
  (import buf)
  (import io)
  (import str)
  (import text)
  (import vec)

  ;; prints each input line upper-cased, numbered from 1
  (fn main [] -> (result i32 str)
    (let a:alloc.Allocator (call alloc.default))
    (let input:str.Bytes
      (match_result (call io.read_stdin)
        (ok b b)
        (err e (return (err:i32 (format "stdin: {(call io.message e)}"))))))
    (let lines:(ptr (vec.Vec str.Bytes)) (call text.lines a input))
    (for i 0 (call (vec.len str.Bytes) lines)
      (let line:str.Bytes (call (vec.at str.Bytes) lines i))
      (println "{(+ i 1)}: {(call buf.to_str (call text.to_upper a line))}"))
    (ok:str 0)))
```

Run on `hello` and `world` (one per line) it prints `1: HELLO` and
`2: WORLD`. Note `text.to_upper` returns a `(ptr buf.Buf)`, a builder, not a
string: printing it as it is prints the struct. `buf.to_str` makes the `str`.

A second program, with a map: count the words of standard input and print
each, in alphabetical order, with its count.

```lisp
(module word_tally
  (import alloc)
  (import buf)
  (import io)
  (import map)
  (import str)
  (import text)
  (import vec)

  ;; byte order, for sorting the words
  (fn by_word [a:str.Bytes b:str.Bytes] -> i32 (no_leak)
    (call str.cmp a b))

  ;; counts the words of standard input (split at white space, lower-cased)
  ;; and prints each once with its count, in alphabetical order
  (fn main [] -> (result i32 str)
    (let a:alloc.Allocator (call alloc.default))
    (let input:str.Bytes
      (match_result (call io.read_stdin)
        (ok b b)
        (err e (return (err:i32 (format "stdin: {(call io.message e)}"))))))
    (let counts:(ptr (map.Map str.Bytes i32)) (call (map.make str.Bytes i32) a 16))
    (let words:(ptr (vec.Vec str.Bytes)) (call text.fields a input))
    (let order:(ptr (vec.Vec str.Bytes)) (call (vec.make str.Bytes) a 16))
    (for i 0 (call (vec.len str.Bytes) words)
      (let w:str.Bytes (call buf.bytes (call text.to_lower a (call (vec.at str.Bytes) words i))))
      (when (not (call (map.has str.Bytes i32) counts w))
        (call (vec.push str.Bytes) order w))
      (call (map.add str.Bytes) counts w 1))
    (call (vec.sort_by str.Bytes) order (ref by_word))
    (for i 0 (call (vec.len str.Bytes) order)
      (let w:str.Bytes (call (vec.at str.Bytes) order i))
      (println "{(call str.from_bytes w)} {(call (map.get_or str.Bytes i32) counts w 0)}"))
    (ok:str 0)))
```

On `the cat The dog` and `the end` it prints `cat 1`, `dog 1`, `end 1`,
`the 3`. A map keeps no order of its own, so keep the keys in a vector (here,
in order of first appearance) and sort that. `map.add` adds to an `i32` value
and gives a missing key the start of 0.

Memory in short: every allocation names an allocator, passed in as a
parameter. A short program can take `(call alloc.default)` once in `main`
and never free what it allocates; the run stops only at the memory limit
(2 GiB, `--max-memory` sets another). Free things (`vec.free`, `map.free`,
`buf.free`) only when a loop would otherwise grow without bound.

## 3. The commands

**`aipl verify FILE`** parses and type-checks without running. A syntax
error is reported alone. Otherwise each function with a type error reports
its first one, one per line, so fix all that are listed and run it again.
Exit status 1 on any error.

**`aipl eval FILE`** runs `main` in the interpreter. It is for quick tries:
it adds two lines of its own to standard output (`[AIPL VM] Executing ...`
and `[AIPL Result]: ...`), so use `compile` and `aipl-run` when the output
has to be exact. Standard input reaches the program; arguments follow `--`
(`aipl eval prog.aipl -- one two`).

**`aipl eval -e 'EXPRESSIONS'`** evaluates expressions and prints the last
value with its type: `aipl eval -e '(call str.trim (call str.from_str "  x "))'`.
`--with FILE` evaluates inside that file's module, so its functions (private
ones too) can be called: `aipl eval --with prog.aipl -e '(call helper 3)'`.

**`aipl compile FILE -o OUT.wasm`** builds a WebAssembly module, optimized
by default (`--no-opt` turns that off; the program behaves the same, but a
trap inside a small function that was inlined is reported at the call).
`aipl compile --exe FILE -o OUT` writes a standalone executable instead:
native on Linux x86-64, a launcher bundle elsewhere. `--max-memory 512M`
sets the memory limit; `--sandbox` (with `--exe`) limits file access to the
working directory.

**`aipl-run OUT.wasm [ARG...]`** runs a built module; `aipl run OUT.wasm --
ARG...` is the same. Both give the program its standard input, its output,
its exit status, and the current directory.

**`aipl test FILE`** runs each `(test "name" ...)` item in a fresh process
and reports which passed (section 5). `-k TEXT` runs only tests whose name
contains TEXT; `--all-backends` runs each in the interpreter, as WebAssembly,
and natively, and passes only if all agree.

**`aipl doc`** is the library's reference, from the comments in its source:
`aipl doc --find split` searches every module; `aipl doc text` lists a
module's functions with their exact parameters and what they do; `aipl doc
text.fields` shows one; `aipl doc prog.aipl` does the same for your own file
(`--private` adds the private items). Look here before guessing a name or a
signature.

**`aipl watch FILE`** runs the file again each time it, or a file it
imports, changes (`--test` runs its tests instead).

## 4. When something goes wrong

A compile-time error is `FILE:LINE:COL: message`, and the message usually
says what to write. For example:

```text
$ aipl verify bad.aipl
Error: bad.aipl: 4:16: Arg 1 of 'add' expects i32, got bool
$ aipl verify syn.aipl
Error: syn.aipl: 1:42: Unexpected token parsing expression: ')'; if needs both branches: (if c then else); for one branch, write (when c then)
```

A run-time failure stops the program with exit status 134, a message on
standard error, and the chain of calls it was in, innermost first, with a
position for each:

```text
$ aipl-run trap.wasm
trap.wasm: Array index out of bounds: index 5 for array of length 3
  at main (trap.aipl:6:14)
```

The ones you will meet: `Array index out of bounds`, a failed contract
(`Pre-condition failed in 'f': ... with a = 10, b = 0`, naming the
function, the condition, and the values), `Use after free`, `integer
overflow` (from `checked.add` and its kin), and division by zero. A failure
that `main` returns as `err` is not a trap: its message, exit status 1.

| Exit status | Means |
|---|---|
| 0 | success |
| 1 | `main` returned `err`; or a compile error; or a failed test |
| 134 | a trap: the message and call chain are on standard error |

If the message does not say what to change, the guide's table of common
mistakes and its checklist are the next place to look.

## 5. Testing a program

A `(test "name" ...)` item is part of the module, may call private
functions, and is left out of ordinary builds. `(expect c)` checks a bool and,
for a comparison, shows both sides when it fails:

```lisp
(module doubling
  (fn double [x:i32] -> i32 (* x 2))

  (test "doubles a number"
    (expect (eq (call double 4) 8)))

  (test "doubles zero and a negative"
    (let n:i32 (call double -3))
    (expect (eq n -6))
    (expect (eq (call double 0) 0))))
```

```text
$ aipl test doubling.aipl
doubling.aipl
  ok    doubles a number
  ok    doubles zero and a negative
2 tests: 2 passed, 0 failed
```

A failing one says where and why:

```text
  FAIL  fails
        t.aipl:4:45: (expect (eq n 7)): left 6, right 7
```

A program that reads standard input is tested from outside: build it, then
give it an input and compare the output with what you worked out by hand:

```bash
aipl compile prog.aipl -o prog.wasm
printf 'hello\nworld\n' | aipl-run prog.wasm | diff - expected.txt
```

Test the edges the task implies, not only its example: empty input, one
line, a line without a final newline, the largest sizes it names.

## 6. Where to look in the library

Run `aipl doc MODULE` for any of these; each lists exact parameters. Import a
module before naming anything in it.

| To do this | Use |
|---|---|
| read standard input whole / line by line | `io.read_stdin`; `stream.stdin` with `stream.next_line` and `stream.line` |
| split text | `text.lines`, `text.fields` (white space), `text.split`, `str.split_once` |
| search, trim, compare views | `str.find`, `str.contains`, `str.starts_with`, `str.trim`, `str.cmp`, `str.same` |
| text to a number | `str.parse_int`, `str.parse_i64`, `text.parse_f64` (each gives a result) |
| build text | `(format "...")`; `buf.make`, `buf.push_str`, `buf.push_int`, `buf.to_str` |
| change case, replace, join | `text.to_lower`, `text.to_upper`, `text.replace`, `text.join` |
| a growable list; sort it | `vec.make`, `vec.push`, `vec.at`, `vec.len`, `vec.sort`, `vec.sort_by` |
| a map or a set | `map.make`, `map.set`, `map.get_or`, `map.add`, `map.has`; `set.make`, `set.add`, `set.has` |
| optional values | `option.Option`; `os.arg` and `os.env` return them |
| big integers; statistics; random | `bigint`, `stats`, `random` |
| JSON, CSV | `json.parse`, `csv` |
| command-line flags | `cli` |
| write output through a buffer | `stream.stdout`, then `format_to`, and `stream.close_writer` at the end |

To walk a map, visit its slots: `(for i 0 (call (map.capacity K V) m) ...)`
with `map.slot_used`, `map.slot_key`, and `map.slot_val`; the order is
unspecified, so sort what you print.

## 7. More than one file

`(import name)` finds `name.aipl` next to the importing file, then in the
standard library. A module's items are private unless marked `(pub)` after the
signature. Name an imported item with its module: `(call util.double 21)`,
`(ptr util.Node)`. Every module whose name you write needs its own `(import
...)`, even when an import already imports it. A task of this size is one
file.

## 8. Syntax and operators at a glance

The spelling of what the guide's rules use, in one place.

**Text.** A comment is `;;` to the end of the line. Tokens are separated by
spaces, tabs, and newlines. An integer is decimal: `42`, `-7` (an `i32`,
which must fit in 32 bits), `42i64` (an `i64`). There are no hex, binary, or
underscore forms: write 255 for 0xFF, or `(shl 1 8)`. A float needs a dot and
a digit: `1.0`, `.5`, `1.5e3` (an `f64`), `1.5f32` (an `f32`). A string is
double-quoted with the escapes `\n \t \r \0 \\ \"` and no others; inside
it `{x}` and `{(expression)}` print a value, and `{{` and `}}` are braces.
Booleans are `true` and `false`.

**A file** is `(module name (import m)... item...)`, where an item is
`(struct ...)`, `(enum ...)`, `(union ...)`, `(const ...)`, `(fn ...)`, or
`(test ...)` (also `(interface ...)` and `(impl ...)`, for generics). Types
are `i32 i64 f32 f64 bool str void`, `(result T E)`, `(ptr S)`, `(arr T)`,
a struct, enum, or union name, and `(fn [T ...] -> R)` for a function value.

**Operators**, all written first, `(op a b)`:

| Kind | Operators |
|---|---|
| arithmetic | `+ - * / %`; `/` and `%` round toward zero (`(/ -7 2)` is -3, `(% -7 2)` is -1); `divu remu` divide as unsigned; `checked.add checked.sub checked.mul` stop on overflow |
| bits (`i32` and `i64`) | `bitand bitor ^` (xor) `shl shr shru`; `shr` keeps the sign, `shru` fills with zeros (`(shru -16 28)` is 15); both operands have one type, so an `i64` shifts by an `i64`. There is no `&`, `\|`, `<<`, or `~` |
| compare (give `bool`) | `eq neq lt lte gt gte`, signed; `ltu lteu gtu gteu`, unsigned |
| logic | `and or not`; `and` and `or` take two or more and stop early |
| numbers of another type | `i64.extend_s i64.extend_u i32.wrap f64.convert_i32_s f64.convert_i64_s i32.trunc_f64_s i64.trunc_f64_s f64.promote f32.demote`, and `f64.sqrt` |

There is no `min`, `max`, `abs`, `floor`, or `pow`, as an operator or in the
library: write what you need with `if` and a loop.

```lisp
(module bits_demo
  (fn main [] -> i32
    (println "{(bitand 12 10)} {(bitor 12 10)} {(^ 12 10)} {(shl 1 4)} {(shr -16 2)} {(shru -16 28)} {(/ -7 2)} {(% -7 2)}")
    0))
```

It prints `8 14 6 16 -4 15 -3 -1`.
