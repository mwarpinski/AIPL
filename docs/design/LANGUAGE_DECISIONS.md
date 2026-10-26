# Language decisions before release

**Status: proposed (2026-10-26), for the owner to decide item by item.**
Written by Claude (Opus 5.5) as step 2 of the release order (docs/HANDOFF.md):
the changes that would break existing programs, settled before anything is
measured or published, so the seed is refreshed once and the evidence for
the launch post (LAUNCH_CHECKLIST.md section 5) is gathered on the final
language.

Each item says what changes (with code), why (with evidence), what it
costs, the options, and a recommendation with how strongly it is held.
Counts are over the repository's 170 AIPL files (aipl_src/, examples/,
benchmarks/, tests/aipl/), measured 2026-10-26.

The rule the recommendations follow is the one the owner set: **what is
correct and least error-prone for a model writing AIPL wins, not human
ergonomics.** In practice that means: one way to write each thing, a
mistake caught by the compiler rather than at run time, and nothing a
model has to remember that the compiler could check.

## At a glance

| # | Change | Breaks programs? | Cost | Recommendation |
|---|---|---|---|---|
| 1 | Every module a file uses, it imports | Yes, barely (1 file) | Small | **Decided yes, done 2026-10-26** |
| 2 | A missing value is an Option, not a fake piece of text | Yes (~30 call sites) | Medium | **Decided yes, done 2026-10-26** |
| 3 | An array literal, `(arr.of T a x y z)` | No (new form) | Small | **Decided yes, done 2026-10-26** |
| 4 | A heap struct is made in one form, every field given | Yes (268 sites) | Large, mostly by a tool | **Decided yes: fields by name, zeros written with `(zero T)`**; in progress |
| 5 | A name's scope is its block or match arm | No (it allows more) | Medium | **Yes** (moderate) |
| 6 | One way to print: the format forms | Yes (~105 call sites) | Medium, by a tool | **Yes** (moderate) |
| 7 | An address-sized integer type | No, if not done | None now | **No: say why in the FAQ** (strong) |
| 8 | `(= x 1)` says to write `eq` (today it suggests `(call = ...)`) | No (a message) | Tiny | **Yes**, just do it |

Already done, so not proposed: `str.Bytes` as a value struct (the
checklist's list still names it; std/str.aipl:12 is `(value)` since
MEMORY_PLAN.md M1, 2026-10-19).

Not in this document: the review's syntax proposals (inferred `let`
types, `(f x)` without `call`, `p.f` for a field, a bytes literal). They
make code less explicit, and CANONICAL_PLAN.md step 7 decides them by
measurement, with the harness, after this.

---

## 1. Every module a file uses, it imports

**Today.** A file that imports `text` can name `vec.Vec` without
`(import vec)`, because text imports vec and imports are flattened into
one program (AIPL_SPEC.md 11). With no imports at all, `str.Bytes` is
"Unknown struct". The spec says neither that this is allowed nor that it
is not; it works by accident.

```
(module m
  (import text)            ;; no (import vec), yet:
  (fn f [a:alloc.Allocator] -> i32
    (call (vec.len str.Bytes) (call text.split a ...))))
```

**Proposed.** `m.x` (a call, a type, a field path, a `ref`) needs
`(import m)` in that file, checked by both resolvers, with the message
naming the line to add: `vec.Vec: this file does not import vec; add
(import vec)`. The modules the compiler imports for its own forms (`new`
brings std/alloc, the formatting forms std/show) are not the file's
business and stay invisible.

**Why.** A model reading one file should see every dependency it has,
and a program should not break because a library stopped importing
something. Today the rule a model would have to learn is "it works if
something you import happens to import it", which no model will guess
correctly.

**Cost.** Small. One repository file leans on it
(tests/aipl/memory/request_paths.aipl uses `vec.Vec` through `text`).
The check is in the two resolvers (src/resolver.rs,
aipl_src/resolver.aipl), with a message test in each.

**Recommendation: yes, strongly.** Cheap now, impossible after release
without breaking users.

**Decided (owner, 2026-10-26): yes. Done 2026-10-26** on branch
features/explicit-imports: both resolvers (`foreign` in src/resolver.rs
and aipl_src/resolver.aipl) refuse a module's name the file does not
import, with one message; the compiler's own writing (`alloc.create`
and `alloc.array` for `new` and `arr.new`, a test build's `_start`) is
exempt, and the formatting forms' std/show calls are written after the
check in both toolchains. One repository file needed its line, and so
did programs the scan of .aipl files did not see: two written inside
Rust tests (tests/test_selfhost.rs, tests/test_sugar.rs), tools/run_fuzz.py's
generated programs (alloc), and tools/make_kit.sh's test program. AIPL_SPEC.md 11
and 13, the guide's rule 1, tests/test_visibility.rs (three cases, both
resolvers message for message).

## 2. A missing value is an Option, not a fake piece of text

**Today.** Four std functions report "nothing" by returning a text view
whose length is -1, a value that looks like text but is not:
`io.read_file` (and `io.read_path`, `io.read_stdin` on a read error),
`os.arg` past the last argument, and `os.env` for an unset variable.

```
(let name:str.Bytes (call os.env "HOME"))
(call io.println name)            ;; compiles; nothing checked it was set
```

The prompt guide tells models to use `Option` for a missing value, and
std/stream's `open` already returns a result, so std contradicts its
own advice in its oldest corner.

**Proposed.**
- `os.arg [i] -> (option.Option str.Bytes)` and
  `os.env [name] -> (option.Option str.Bytes)`: absence is the only
  failure. `os.arg_int [i default]` stays as it is (it has a default).
- `io.read_file`, `io.read_path`, `io.read_stdin ->
  (result str.Bytes str)`: a file can fail for several reasons, so the
  error is a message (`"cannot open notes.txt"`), as std/stream does.

The compiler then makes every caller handle the missing case (`match`,
or `try` to pass it up); forgetting is a compile error, not a wrong
answer.

**Not proposed: integer `-1` returns** (`str.find`, `regex.group_index`,
a few others). "Index, or -1" is the convention models know from every
other language, a `-1` index cannot be mistaken for real data the way a
length -1 text can, and changing them touches far more code. They stay,
documented as they are.

**Cost.** Medium. About 30 call sites (`os.arg` 24, `os.env` 4,
`io.read_stdin` 1, the self-hosted toolchain's own std lookup
aipl_src/stdpath.aipl among them; `io.read_file` has none in the
repository), the guide's and spec's examples, and both toolchains alike.
These modules are compiled into the self-hosted compiler, so the change
must be one the current seed can compile; Option is older than the seed,
so it should be (to be confirmed by `sh bootstrap/build.sh` before
anything else is changed).

**Recommendation: yes, strongly.** A value that pretends to be text is
exactly the kind of silent failure AIPL exists to rule out.

**Decided (owner, 2026-10-26):** yes, in place (the same names, new
types); a failed read or write gives **both an error type to branch on
and its words**, as std/net does: `(enum io.Error [not_found denied
is_directory other])` (chosen now: a member added later would break an
exhaustive match) and `io.message`, which leaves the path out (the
program has it, and its own file and line say where); `read_stdin` and
the writes (`write_file`, `write_path`) take the same shape; and
`stream.open` and `stream.create` (and `csv.open`, which passes
`stream.open`'s error up) give `io.Error` too, one type for one kind of
failure.

**Done 2026-10-26** on branch features/missing-values:
- `os.arg` and `os.env` return `(option.Option str.Bytes)`.
- Every WASI-backed op (`fs.*`, `sys.random`, `args.*`, `env.*`) fails
  with minus its WASI errno instead of -1, in every backend: both code
  generators emit the negated errno (the same bytes), the VM translates
  its OS errors through the native backend's table, and native
  executables already did. Reading a directory was the one place the
  backends differed (EISDIR natively and in the VM, EBADF in wasmtime,
  whose directory handles cannot be read); the VM and native now give
  EBADF, as the spec's rule that wasm is the reference says. AIPL_SPEC.md
  4.C.
- std/io: `io.Error`, `io.message`, `io.error_of` (an fs result to an
  Error); `read_file`, `read_path`, `read_stdin`, `read_fd`, `write_file`,
  `write_path` return results. std/stream and std/csv as decided.
- The callers: about 40, in the compiler, std, examples, a benchmark,
  test programs, and programs inside Rust tests; tests that check
  printed numbers keep them (a failure still prints -1 there).
- tests/test_io.rs: each kind (a missing file, a directory, a missing
  directory on the path, a file used as one, no permission, a write into
  a missing directory and onto a directory, and a branch on the kind) the
  same in the VM, under aipl-run, and natively, both toolchains' bytes
  alike (tests/aipl/io/errors.aipl).

## 3. An array literal

**Today.** There is none. An array is made with `arr.new` and one
`arr.set` per element. Both of the outside review's first-try failures
were a guessed array literal (CANONICAL_PLAN.md step 5); today
`(arr.of i32 1 2 3)` gives "Unknown op/keyword: arr.of; there is no
array literal: make the array with (arr.new T a n) and fill it with
(arr.set T xs i v)" (checked 2026-10-26): clear, but it costs a round.

**Proposed.** `(arr.of T a x y z ...)`: an `(arr T)` from the
allocator `a`, holding the values in order, as `arr.new` takes an
allocator (MEMORY_PLAN.md). Written without the allocator, the error
says so: `arr.of needs an allocator first: (arr.of T a x ...)`. Every
backend, both toolchains, the same bytes.

**Why.** It is what a model reaches for first, and the long form gets
the length or an index wrong.

**Cost.** Small. A new form, so nothing breaks. One open detail: whether
`(arr.of T a)` with no values is allowed (an empty array); proposed yes,
as `arr.new T a 0` is.

**Recommendation: yes, strongly.**

**Decided (owner, 2026-10-26):** yes; the allocator required; an empty
`(arr.of T a)` allowed, as an initializer to replace with longer arrays
(a list that grows is `vec.Vec`; no `vec.of` for now). **Done** on branch
features/arr-of: both resolvers rewrite it as a block that makes the
array and sets each value (sugar.rs, resolver.aipl emit_arr_of), the
temporary named by the form's line and column, so it stays flat however
many values it holds; a missing allocator, or a literal where it goes, is
one message in both. tests/test_sugar.rs: alike in every backend, both
toolchains' bytes, five message cases. Found on the way, open: sugar
inside a formatting placeholder is rewritten by the self-hosted resolver
but refused by the Rust one (docs/HANDOFF.md, Open problems).

## 4. A heap struct is made in one form

**Today.** `(new S a)` makes a zeroed `S` from allocator `a`, and a
`put` per field fills it. Nothing notices a field left out, and adding a
field to a struct means finding every place that makes one by hand. 218
of the repository's 224 `new` sites in the scan are followed by `put`s;
std/regex's Regex takes 20.

```
(let p:(ptr Point) (new Point a))
(put p Point.x 1)                 ;; forgot Point.y: it is 0, silently
```

Value structs and union variants already do it the safe way:
`(make Vec2 1.0 2.0)` must give every field, in order, or it does not
compile.

**Proposed.** `(new S a v1 v2 ...)`, every field in declaration order,
type-checked, as `make` does; a missing or extra field is a compile
error naming the field. The zeroed form `(new S a)` goes away, so there
is one way: a struct you want zeroed is written with its zeros.

**The question for the owner: positional or named fields?**
- *Positional* (`(new Point a 1 2)`), as `make` and every function call
  already are. Consistent: one rule for every place values are given in
  order. A swap of two fields of the same type (`x` and `y`) is not
  caught, the same risk every call with two `i32` parameters has today.
- *Named* (`(new Point a [x 1 y 2])`). Catches swaps, and reads better
  for a 20-field struct. But it is a second way to give values, unlike
  `make`, unless `make` changes too, which would break every value struct
  and union in the repository (several hundred sites more).

I lean **positional**, for consistency: "values are given in order,
everywhere" is one rule a model cannot get wrong, and the checker
already reports a type mismatch by field name. If the owner wants the
swap protection, it should come to `make` and `new` together, as a
separate decision.

**Cost.** Large in sites, small in thought. 268 `new` sites; most are a
`new` followed directly by a `put` of every field, which a rewriting tool
can fold (as tools/retire_loop.py and tools/when_rewrite.py did for `for`
and `when`, with a dry run read first). The rest are rewritten by hand.
The order matters because the compiler is compiled by the seed: (a) add
the new form to both toolchains, keeping the old; (b) refresh the seed
(owner approval); (c) rewrite the repository; (d) remove the old form.

**Recommendation: yes, moderately.** The protection is real and the cost
is mostly mechanical; it is the largest item here, so if time is short it
is the one to drop.

**Decided (2026-10-26).** A struct's fields are given by name, a
variant's payload by position: `(new S a [x:1 y:2])` and
`(make V [x:1 y:2])`, every field once, in declaration order (each value
evaluated in the order written). The owner asked whether positional with
optional names was possible (as some languages' calls are); it is, but it
would be two ways to write the same thing, so names only.

The zeroed `(new S a)` goes, as proposed. A scan before the rewrite found
that about 150 of some 370 sites leave fields zero (about 70 make a struct
empty and fill it later), and that two kinds of field have no zero that
can be written: a `T` inside a template, and a union. Keeping the zeroed
form for "fill it in later" was weighed and refused: a zero written out is
the same value as one implied, and only the written one makes a new field
an error at every site that does not say what it is. So there is a form
for the zero of a type, `(zero T)`, written out by the generics pass
(generics.rs zero_of, generics.aipl print_zero), where every type is
known, a template's `T` included: `0`, `0i64`, `0.0`, `0.0f32`, `false`,
`""`, `(ptr.null S)`, `(arr.null T)`, an enum's first member, a value
struct made of its fields' zeros. A union, a function reference, a
result, and a struct used through a pointer have none: a union's zero
would be whichever variant happens to be declared first.

Order: (a) the named forms in both toolchains, keeping the old (done);
(b) `(zero T)` (done); (c) the seed refreshed (owner approved,
2026-10-26); (d) the repository rewritten by a tool, its dry run read
first; (e) the old forms removed, with messages that show the new ones.

## 5. A name's scope is its block or match arm

**Today.** A name keeps one type for the whole function. Two match arms
cannot each bind `v` with different types, nor can two sibling blocks
each have a `let x` of different types:

```
(match s
  (Shape.circle [v] ...)          ;; v is f64
  (Shape.box [v h] ...))          ;; error: 'v' is i32 here but f64 elsewhere
```

The message is clear ("a name keeps one type per function (rename
one)"), but agents hit it 7 times in one day (AGENT_FRICTION.md row 2),
mostly with match binders, where the natural names repeat (`e` for two
different error types in two `match_result` arms).

**Proposed.** A name declared in a block or a match arm is visible only
there, and a sibling block or arm may declare it again with another
type. Shadowing a name that is still visible stays an error, as today.

**Why.** It removes a rule that exists for the code generators'
convenience (one local per name), not for the program's correctness;
nothing a model writes today stops compiling.

**Cost.** Medium. The VM and both code generators give each scope's
names their own locals (renaming inside the compiler), with the same
bytes in both toolchains.

**Recommendation: yes, moderately.** It does not break anything, so it
could also wait until after launch; it is here because the guide's
pitfall list should be final before the evidence is gathered.

## 6. One way to print

**Today.** Two ways: the format forms, `(println "total {n}")` (189
uses), and std/io's labelled helpers, `(call io.println_int "total " n)`
and its siblings `println_i64`, `println_f64`, `print_int` (about 105
uses). The format forms do everything the helpers do (`{x:.3}` for
decimals), work in `(no_leak)` functions alike (checked), and are what
the guide teaches; the helpers predate them.

**Proposed.** The format forms are the way to print. The labelled
helpers are removed; `io.write_str` (bytes to a file descriptor) and
`io.println` of plain text stay as the plain library underneath, and
`sys.print` stays the bare builtin for code that cannot import std.

**Why.** Two spellings for one thing make a model guess which this
codebase uses, and the guide's examples and the repository's code
disagree today.

**Cost.** Medium, mostly mechanical: about 105 call sites, rewritten by
a tool (`(call io.println_int "label " x)` is `(println "label {x}")`
exactly), plus the docs.

**Recommendation: yes, moderately.** Less important than 1-4; it is in
this list because it is a duplicate (CANONICAL_PLAN.md step 6 asks for
the sweep), and this is the only real duplicate the sweep found. The
others it checked are not duplicates: `new` and `arr.new` are the
spelling of `alloc.create` and `alloc.array`, which generic code needs;
`cond` and `if`, `for` and `while`, `try` and `match_result`, `str.concat`
and `buf` and the format forms each do something the other cannot (the
guide says when to use which).

## 7. An address-sized integer type

**Today.** Memory addresses (`ptr.addr`, `mem.*`, `mem.alloc`) are
`i32`, because AIPL compiles to 32-bit wasm, whose addresses are 32
bits. A future 64-bit target (ROADMAP.md, after launch) would want an
address-sized type, like Rust's `usize`.

**Proposed: not now; one FAQ entry.** Only `unsafe` code touches raw
addresses, and safe code never sees one, so adding the type later breaks
only unsafe code, which is a small, marked part of any program.
Adding it now would make every address computation in std and the
compiler change type for a target that does not exist yet, and add a
type models must learn and convert to and from.

**Recommendation: no, strongly**, with the FAQ saying exactly this.

## 8. `(= x 1)` says to write `eq`

**Today.** `(= 1 1)` gives "Unknown op/keyword: =; to call a function,
write (call = args...)" (checked 2026-10-26): the generic hint for an
unknown head, which here points the wrong way. The outside review quoted
a better message that never existed (NOTES_REVIEW.md).

**Proposed.** `'=' is not an operator; equality is (eq a b)` (and the
same for `==` and `!=`, which is `neq`), in both toolchains. Not a
language change; listed so it is not forgotten.

**Recommendation: yes; just do it.**

---

## Order of work, once decided

1. The items that need no seed refresh, each on its own branch with the
   full CI: 8, 1, 3, 2 (if the seed compiles it), 5.
2. Item 4's new form added alongside the old (both toolchains), and item
   6's rewrite, if approved.
3. **One seed refresh** (owner approval), from the commit where every
   approved form exists.
4. The rewrites that need the new seed (item 4's `new` sites in the
   compiler), then the old forms removed.
5. The guide's pitfall table and the spec updated last, once, for the
   harness (CANONICAL_PLAN.md step 7) to measure.

## Questions for the owner

1. Items 1-8: yes or no each (or "later" for 5 and 6, which do not have
   to be before launch).
2. Item 4: positional fields (consistent with `make`; my lean) or named
   (catches swaps; would also change `make`)?
3. Item 2: is a message enough for a failed file read, or do you want an
   error type a program can branch on (not found, permission, other)?
   A message is what std/stream does; a type is more work and more to
   learn.
