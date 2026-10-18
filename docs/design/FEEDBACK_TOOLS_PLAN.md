# Fast feedback without a REPL: doc, eval -e, dbg, test, watch

**Status: approved 2026-10-18, every decision as recommended; F1, F2, F3, and F3b built 2026-10-18; F4 next.** It replaces a REPL as the
next developer-experience work. docs/design/REPL_PLAN.md (a wasm
interpreter in AIPL) stays a separate, later question: it is what would let
the self-hosted `aipl` run code itself (section 7).

## Why not a REPL

A REPL serves six needs: quick experiments, trying a function while writing
it, exploring an API, inspecting values while debugging, exploring data,
and learning the language. It serves them all with one mechanism, a
long-lived session that holds state, and that state is the hard part in
AIPL: code lives only in functions, there are no globals (a design
decision: std/par's race-freedom and local reasoning depend on it, AIPL_SPEC
section 4), and a compiled module cannot add to itself.

It is also the wrong shape for AIPL's main users. An AI agent works in
separate tool calls; a session is state it cannot see, which drifts, and
what it tries there is gone afterwards. What helps an agent (and a person)
is a set of one-shot commands with deterministic output, and checks that
stay in the code.

| Need | Served by |
|---|---|
| quick experiments | `aipl eval -e` (F2) |
| trying a function while writing it | `aipl eval -e --with` (F2); the `test` form (F4), which keeps the experiment |
| exploring an API | `aipl doc` (F1) |
| inspecting values while debugging | `(dbg e)` (F3) |
| exploring data, learning | `aipl watch` (F5): a scratch file re-run on every save, the editor as the session |

Each piece is small and none changes the language's model. A REPL, if it is
still wanted afterwards, is `eval -e` in a loop over a growing session.

## F1. `aipl doc`: a module's interface, from its source

```
$ aipl doc text
std/text: the text functions that build something, over byte slices ...

  split [a:alloc.Allocator b:(ptr str.Bytes) sep:(ptr str.Bytes)] -> (ptr (vec.Vec (ptr str.Bytes)))
    The pieces of b between each sep: "a,,b" gives "a", "", "b"; "" gives
    one empty piece.
  ...
$ aipl doc text.split          # one item
$ aipl doc                     # every std module, with its first line
$ aipl doc --find split        # names and comments across std that match
$ aipl doc path/to/mod.aipl    # any module
```

- An item's documentation is the `;;` block directly above it, and a
  module's is the block above `(module`, the convention std already follows
  (308 public items). Only `(pub)` items are shown (functions, structs with
  their readable fields, unions, enums, constants, interfaces);
  `--private` shows the rest.
- Signatures print as written, with the module's own names qualified as
  a caller writes them (`(ptr str.Bytes)`, `(vec.Vec T)`).
- `aipl doc --check FILE` fails on a public item with no comment;
  `cargo test` runs it over std, so the standard library stays documented.
- Written in AIPL (`aipl_src/doc.aipl`): it needs no execution, only parsing,
  so it is a self-hosted subcommand. The Rust CLI's `aipl doc` compiles and
  runs that same program (decision D1).

As built (2026-10-18): `aipl_src/doc.aipl`, about 660 lines, in only what
the stage-0 seed compiles (no formatting forms, `try`, or `defer`); the
Rust CLI's `aipl doc` is `src/tools.rs` compiling and running it (about 50
ms a call). Generic items show as a caller writes them, `(option.unwrap_or
T)`, and contracts follow the comment. `--check` found 88 undocumented
public items in std, now documented (308 in 25 files).

Why first: it is the cheapest, and the most useful to an agent, which today
reads a 400-line file to learn one function's parameters. The prompt guide
can then say "run `aipl doc MODULE`" instead of listing APIs inline.

## F2. `aipl eval -e`: one expression, one answer

```
$ aipl eval -e '(call str.find (call str.from_str "a,b") (call str.from_str ","))'
1 : i32
$ aipl eval --with rows.aipl -e '(call parse_row "a,b,c")'
3 : i32
$ aipl eval --with rows.aipl -e '(let r:(ptr Row) (call make_row 2)) (get r Row.n)'
2 : i32
```

- The expressions become the body of a function added to the `--with`
  file's own module (so its private functions can be called); its `main`
  is not run. Without `--with`, an empty module.
- Its result type comes from the checker (a new "type of the last
  expression" query), and the value prints as the formatting forms print
  it; a value they cannot print shows its type and address,
  `(ptr Row) @ 4120`, until F3b. `void` prints nothing.
- Standard modules named in the expression (`str.find`, `text.split`) are
  imported for it, so a one-liner needs no `(import ...)`.
- A program's own output comes first; the answer is the last line of
  standard output; errors are the usual `path: line:col: message`, the
  expression being `-e:1:col`. Exit status 0, or 1 on any error.
- Runs in the VM by default (fast to start, and the independent
  interpreter); `--wasm` or `--native` compiles and runs it instead (D2).
- In the Rust CLI: running code is what the self-hosted binary cannot do
  yet (WASI cannot start a process, and AIPL has no interpreter of its own,
  section 7), which is the reason this is Rust.

As built (2026-10-18): `src/evalexpr.rs`, about 300 lines. The
expressions are checked inside a `void` wrapper, which gives their type
(the checker records each function's body type, `body_type`); the second
step changes the flat, already-resolved module rather than re-parsing, since
the flat program names generic instances `option.Option<i32>`, which source
cannot. The printer is a `(println ...)` built in the tree and lowered by
`formats::lower` like any other. Exit status: 1 for an error in the VM,
134 for a trap compiled, as for any program.

## F3. `(dbg e)`: see a value, keep going

```lisp
(let d:i32 (dbg (call distance p q)))
```
prints to standard error, and gives `e`'s value:
```
rows.aipl:12:16: (call distance p q) = 5
```

- Every backend, both toolchains: lowered after checking, like the
  formatting forms, to a call that prints and returns its argument
  (`(call show.dbg_i32 "rows.aipl:12:16: (call distance p q)" e)`, one per
  printable type, so `e` is evaluated once). An enum prints its member's
  name through a function generated for that enum.
- The text is the expression as written (re-printed, so both toolchains
  give the same), and the position its file and line, as errors give them.
- It stays in every build, as Rust's `dbg!` does; `aipl verify --no-dbg`
  refuses it where a project wants that (D5).
- As built (2026-10-18): each resolver turns `(dbg e)` into `(format.dbg
  "FILE:L:C: e" e)` while it still has the file's text (both take `e`'s text
  from the source the same way, `formats.rs form_text`, `resolver.aipl
  dbg_text`), the checker gives it `e`'s type, and the lowering binds
  the value to `_dbg_N` (numbered per function, inner ones first, as
  tries are). One rule was added: a placeholder cannot hold a `dbg`, since
  its code has the string's position, not its own.
- **F3b**, after: a struct (behind its pointer) and a value struct print
  their fields, `Row{n: 2, name: "a"}`, through a printer generated per
  type, for `dbg`, `eval -e`, and `{p}` in formatting. One level deep, so a
  cycle cannot loop.

  As built (2026-10-18): both toolchains write the printers as source
  text from the same template (formats.rs debug_functions,
  aipl_src/fmtdebug.aipl), for each type printed and each value type
  inside one, sorted by name, so the order the lowering meets them in
  cannot differ; Rust parses the text, the AIPL toolchain appends it to the
  program, and both codegens give these functions no line-table positions
  (as `_start`). Arrays print too, up to 16 elements; unions as
  `variant{field: value}`, results as `ok(v)` / `err(e)`. Every field
  shows, private ones too. A value printed this way takes no format.

## F4. A test form: experiments that stay

```lisp
(module rows
  (fn parse_row [s:str] -> i32 ...)

  (test "an empty line has no fields"
    (expect (eq (call parse_row "") 0)))

  (test "three fields"
    (let n:i32 (call parse_row "a,b,c"))
    (expect (eq n 3))))
```
```
$ aipl test rows.aipl
rows.aipl
  ok    an empty line has no fields
  FAIL  three fields
        rows.aipl:9:5: (expect (eq n 3)): left 2, right 3
2 tests: 1 passed, 1 failed
```

- `(test "name" body...)` is an item of the module, so it may call the
  module's private functions; `(expect cond)` may appear only inside one.
  A failed `expect` reports its position and text and the test goes on
  (more to see per run, D3); with `eq`, `neq`, `lt`, `lte`, `gt`, `gte` on
  printable values it shows both sides. A trap ends that test only, with
  its call chain, as a failure.
- Each test runs in a fresh instance (a fresh VM, or a new instance of the
  module compiled once), so tests cannot see each other's state and a trap
  cannot stop the rest (D4).
- `aipl test FILE` runs the file's tests (not its imports'); `aipl test DIR`
  every file's; `-k TEXT` those whose names contain it; `--all-backends`
  runs each in the VM, under aipl-run, and natively and fails if they
  differ. A file with no tests but a `run_all` entry point runs that, as
  today, so aipl_src/test_suite.aipl keeps working.
- **Tests are left out of every other build.** Today std's `run_x_tests`
  functions are compiled into every program that imports std (part of
  `word_count` growing from 83 to 104 KB, STDLIB_PLAN.md); moved into test
  forms (F6), they cost nothing outside `aipl test`.
- Both toolchains parse the form, drop it from ordinary builds (so their
  bytes stay the same), and compile a test build (`compile --tests`), byte
  for byte alike: each test a function returning its failure count, and
  `expect` lowered to calls into a small `std/testing` (as `format` lowers
  into `std/show`). The runner, which instantiates and reports, is in the
  Rust CLI for the reason F2 gives.

## F5. `aipl watch`: the editor as the session

```
$ aipl watch scratch.aipl          # re-run main on every save
$ aipl watch --test rows.aipl      # re-run its tests
```

It checks the file and every file it imports (the resolver knows them) for
changes four times a second; on a change it clears the screen and shows
`verify`'s errors, or the run's output, and how long it took. What a REPL
would hold in a session is in a file you can see, save, and diff. Rust, for
the reason F2 gives, and small (polling, no file-notification library).

## F6. Afterwards: std's tests become test forms

Module by module, `run_x_tests` (which return pass counts, checked against
numbers in test_suite.aipl) become `test` forms, so a failure names what
failed and binaries lose their test code. test_suite.aipl shrinks as
groups move; `cargo test` runs `aipl test aipl_src/std` in every backend.

## 7. What stays for later

- **A self-hosted `eval`, `test`, and `watch`.** They need to run code
  inside the self-hosted binary: REPL_PLAN.md's wasm interpreter in AIPL is
  the way, and when it exists these commands move to it and their Rust
  goes. That interpreter would not replace the Rust VM, which interprets
  the source rather than the generated code and so catches code generator
  bugs (it was the backend that was right about the result-cell overlap,
  ed12333).
- **A REPL**: `eval -e` over a growing session, replayed each time
  (recompiling a small session takes 40 ms), if wanted.

## Rust this adds

The `eval -e`, `test`, and `watch` front ends and the "type of an
expression" query in the Rust checker: a few hundred lines, each because
the self-hosted binary cannot yet run code. `doc` is AIPL; `dbg`, `test`,
and `expect` are compiler features in both toolchains, as every language
feature is.

## Batches

Each is one branch, tested (both toolchains for F3 and F4, every backend
for anything that runs), documented (README, spec, prompt guide), and
merged to development before the next.

| Batch | Contents |
|---|---|
| F1 | `aipl doc` (AIPL), `--find`, `--check` over std in `cargo test` |
| F2 | `aipl eval -e [--with FILE] [--wasm\|--native]`; the checker's expression-type query |
| F3 | `(dbg e)` in both toolchains, every backend |
| F3b | struct and value-struct printing, for `dbg`, `eval -e`, and formatting |
| F4 | `(test ...)`, `(expect ...)`, `std/testing`, `aipl test FILE\|DIR`, `-k`, `--all-backends` |
| F5 | `aipl watch [--test]` |
| F6 | std's tests as test forms |

## Decisions (approved as recommended, 2026-10-18)

- **D1, where `aipl doc` lives:** in AIPL, with the Rust CLI compiling and
  running it (recommended), or written twice, or Rust only.
- **D2, which backend `eval -e` uses:** the VM by default, `--wasm` and
  `--native` on request (recommended: fastest to start), or wasm by
  default (the specification's own semantics).
- **D3, a failed `expect`:** the test goes on and reports every failure
  (recommended, as Go's `t.Error`), or it stops there (as Rust's
  `assert!`).
- **D4, test isolation:** a fresh instance per test (recommended: no shared
  state, a trap fails one test), or one instance for the file (faster;
  state leaks between tests).
- **D5, `dbg` in release builds:** allowed, with `verify --no-dbg` to
  refuse it (recommended), or always an error outside tests.
