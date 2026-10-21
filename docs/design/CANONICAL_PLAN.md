# One way to write each thing (canonical forms)

**Status: approved 2026-10-20** (the owner's answers to Sonnet's review of
2026-10-19: retire `loop`, `when` as the only one-branch `if`, measure
the syntax proposals before deciding, and do this batch before NET_PLAN.md
step 5). Steps 1 to 3 built 2026-10-21; what each still has outstanding is
listed at the end of its section.

AIPL is written by models, and a model does best with one obvious way to
do each thing: every second way is a choice it can get wrong, and a
stale prompt or an old copy of the docs steers it to the wrong one.
Sonnet's review (docs/NOTES_REVIEW.md, "sonnet", with a verdict on each
claim) found models choosing the second way: in AI-SQL's 4,100 lines,
96 `loop`s and no `for`, 156 empty `(block)` elses beside 32 `when`s.
This batch removes the second ways it found, makes the errors for what
models guess name the right form, fills the gap that caused both of its
first-try failures (an array literal), and builds the harness that
decides the larger syntax questions with numbers.

## At a glance

| Step | What | Status |
|---|---|---|
| 1 | Docs: the guide's `and`/`or` contradiction, a version stamp, the top pitfalls first, `aipl doc --find` as step one | **Done** 2026-10-21 (two small items outstanding, in section 1) |
| 2 | Errors that name the right form, for every guess a model makes | **Done** 2026-10-21 (outstanding items in section 2) |
| 3 | Retire `loop`: `for` is the only counted loop | **Done** 2026-10-21 (outstanding items in section 3: the internal Loop node is kept) |
| 4 | `when` is the only `if` with one branch | Not started |
| 5 | An array literal, `arr.of` | Not started |
| 6 | The other duplicates: a sweep of the spec, a proposal for each | Not started |
| 7 | The A/B harness, then the syntax proposals one at a time | Not started |

Each step: both toolchains (src/ and aipl_src/), byte for byte alike;
every backend; the spec, the prompt guide, and the tour; the full CI.

## 1. Docs

- PROMPT_GUIDE_FOR_AIS.md's checklist says `and`/`or` take exactly two
  operands; rule 15 (and the language) takes two or more. The checklist
  is wrong.
- A version stamp: `aipl --version` gives the commit it was built from
  (`aipl 0.1.0 (5e29a53)`), and the kit's copies of the spec, guide, and
  tour start with the same line, so a stale copy shows itself.
- The ten pitfalls models hit most (from AGENT_FRICTION.md, AI-SQL's
  mistake log, and the review) at the top of the guide; the spec's
  62-row table stays as the reference.
- `aipl doc --find WORD` before writing any helper: the review found
  AI-SQL hand-writing a float parser std/text already has.

Built 2026-10-21 (PROMPT_GUIDE_FOR_AIS.md "Start here"; build.rs and
tools/make_kit.sh):

- The checklist now says two or more operands.
- `aipl --version` prints `aipl 0.1.0 (d7a02a9)` from the Rust command
  (build.rs records the commit). tools/make_kit.sh puts the same line at
  the top of the kit's three docs and checks it against the kit's
  `aipl --version`. The self-hosted command still prints `aipl 0.1.0`
  (tests/test_bootstrap.rs compares the two): a compiled program cannot
  know its commit without a build step, and the kit's `aipl` is the Rust
  command anyway. Revisit when the Rust command is retired.
- The ten pitfalls are, in order: bare `(f x)`, one-branch `if`, `let`
  without a type, `loop`'s inclusive end, a string literal in a
  placeholder, a name with two types, a missing allocator, no array
  literal, `(ok v)` and `match_result` binders, and `+`/`eq` on strings.
  Chosen from AGENT_FRICTION.md rows 1, 2, 18, 19, 25 and 33, the
  review's two first-try failures, and the spec's table; each checked
  against the compiler on 2026-10-21. AI-SQL's mistake log is not in this
  repository, so it was not used: when it is, revisit the list. Row 4
  followed step 3 (`loop` is gone); rows 2 and 8 must follow steps 4 and 5.
- `aipl doc --find` is the first thing under "Start here", the first
  line of the system prompt module, and the start of the guide's last
  paragraph.

**Outstanding in step 1:**

- The guide's pitfall table: row 2 (`if` with one branch) says `when` is the
  way, which step 4 makes an error rather than a habit, and row 8 says there
  is no array literal, which step 5 ends. Edit both rows, and the intro
  line's "a form that no longer exists", when those steps land.
- AI-SQL's mistake log (AI-SQL/mistake_log.md) was meant to be a source for
  the ten and is not in this repository. When it is, check the ten against
  it.
- The self-hosted `aipl --version` prints no commit (see above); the Rust
  command, which the kit ships, does.

## 2. Errors that name the right form

The errors for the forms models guess say what is wrong, not what to
write. Today:

| A model writes | It is told | It should be told |
|---|---|---|
| `(f 1)`, f a function | `Unknown op/keyword: f` | `f is a function: call it with (call f 1)` |
| `(let x 5)` | `Expected ':', got '5'` | `let names its type: (let x:i32 5)` |
| `(arr.of i32 1 2 3)` | `Unknown op/keyword: arr.of` | (step 5 makes it the literal, with its allocator) |
| `(if c x)` | `Unexpected token parsing expression: ')'` | `if needs both branches; for one, (when c x)` |

Checked 2026-10-21: some rows of section 13 already have a message that
names the fix, so the step is the rows that do not. Already good: `(new S)`
("new takes the allocator to make it from: ..."), `(ok 5)` ("write (ok:E
v)"), `eq` and `+` on `str` (name `str.same` and `str.concat`), and a name
with two types ("rename one"). Still bad: the four rows above, `(ok
[v] ...)` in `match_result` ("Expected ok var, got '['"), and a string
literal inside a placeholder ("unexpected input after the module's
closing ')'", far from the cause; AGENT_FRICTION.md rows 25 and 33).

Built 2026-10-21, in both toolchains with the same text (the BAD_PARSES
list and FORMS in tests/test_checker_aipl.rs, tests/test_visibility.rs,
and tests/test_resolver_aipl.rs compare them word for word):

- Parser: an unknown head says `to call a function, write (call f
  args...)`, or `did you mean ltu?` for `lt_u` (underscores ignored, then
  1 edit for 3 to 5 bytes, 2 for longer; `HEAD_NAMES` in src/parser.rs and
  `suggest.aipl` are the one rule), or names `cond` for `else if`, `elif`,
  `switch`, `case`, or says there is no array literal (`arr.of`, until step
  5). A `let` without its type, an `if` without its else (names `when`),
  a function type with parentheses or no `->`, `call_ref` without its
  type, `(get p Point x)`, `arr.get` without its element type, `arr.set`
  without a value, `(ok [v] ...)`, and a contract that is not one
  expression each say what to write.
- Checker: an unknown function names the nearest (`did you mean`), or says
  to use `call_ref` when it is a function reference in scope; a body that
  gives void for a value says to end in the value; a pointer or array given
  where an i32 goes, an i32 where a pointer or array goes (for null), i32
  against i64 or a float, and a comparison of a pointer with 0 say how to
  convert; `(ptr U)` for a union says unions are values and never null;
  `result` in a contract says it is `res`; an argument count lists the
  parameter types.
- Resolver: `text.splt` says `'text.splt' is not an item of module text:
  did you mean text.split?` (it used to say private); a string literal
  inside a placeholder says where the string ends inside it (it used to say
  `unexpected input after the module's closing ')'`). `aipl run x.aipl`
  says run takes a .wasm.
- Docs: AIPL_SPEC.md 4.x and section 13 and the guide's rule 7 and checklist
  said `(if c (return v) x)` is an error; it is accepted (section 7.1
  rule 2: a branch that always leaves fits any type) and runs the same in
  the VM, wasm, and native. Corrected.

Also built later the same day: a generic written without its type
arguments (`(call vec.push v x)`, `(ref vec.len)`, `(get b Box.value)`,
`(put b Box.value 1)`, `(make option.Option.some 5)`) says `'vec.push' is
generic: name its types, as in (call (vec.push T) ...)`, from both generics
passes (the checker never sees a template, so it is said there); and a
name that is not a parameter inside a contract (`(req (gt k 0))` after a
`let`) says a contract sees only the parameters, and res in (ens ...).

Not built: an unknown module in `(import nosuch)` does not get a "did you
mean" (it needs a directory listing in both resolvers, and the parity test
deliberately leaves the searched paths out of the comparison; `aipl doc
--find` and the guide list the modules). `(for i 0 3 1 ...)` taking the 1
as a statement belongs to step 3, which retires `loop`.

**Outstanding in step 2:**

- The "did you mean" for `(import nosuch)`, above: a directory listing in
  both resolvers, compared in a test that today ignores the searched paths.
- Messages that still say `loop` or `step` to a program that wrote `for`:
  `Loop bounds and step must be i32`, `Cannot shadow existing variable 'i' in
  loop`, and `break is only allowed inside a while or loop body` (both
  checkers, and the tests that pin them: tests/test_control_flow.rs,
  tests/test_checker_aipl.rs, tests/test_diagnostics.rs). They should say
  `for`.
- The survey covered the rows of section 13 that are compile errors and a
  few more (about 80 programs, 2026-10-21); rows that are runtime traps or
  behaviours (a stack that is exhausted, `%` on a negative, a program run
  with the stock wasmtime) have no message to improve. A row added to
  section 13 later needs its message and test the same way.
- Step 5 replaces the `arr.of` message ("there is no array literal") with
  the literal itself, and step 4 adds the message for an empty `else`.

## 3. Retire `loop`

`(loop i start end step body...)` includes its end, unlike every
mainstream language, and is the pitfall table's most repeated row. `for`
(2026-10-18) leaves its end out and is what models expect. After this
step:

- `for` is the language's counted loop, read by both parsers directly
  (today it is a rewrite into `loop`, which the bare parsers do not know).
- `loop` written in a program is an error: `loop is retired: (for i
  start end body...) counts from start up to end, end left out; for
  another step, use while`.
- Every `loop` in the repository (736: the compiler, std, tests,
  examples, benchmarks) becomes a `for`, or a `while` where its step is
  not 1. Mechanical, by a tool that rewrites only that form and is
  checked by the suite, the benchmarks' outputs, and the seed.
- The seed knows `for`, so it still builds the compiler; refreshed at
  the end so the next seed knows the new rule.

What a loop compiles to may change (an exclusive bound is one compare,
not a subtract and a compare); both toolchains' bytes stay equal to each
other, and the benchmarks are measured again.

Built 2026-10-21. Two choices differ from the text above, and are the
owner's to overrule:

- `loop` written in a program is an error from both parsers: `Unknown
  op/keyword: loop; loop is retired: (for i start end body...) counts from
  start up to end, end left out; for another step, use while`. It could not
  be refused where source is read (src/sugar.rs, resolver.aipl), because the
  toolchain's own output is read again as source (the printer's text, the
  text `dbg` and `show` lower to, the flat program the code generator
  parses), and that output has a counted loop in it. So the internal form,
  which counts to its end inclusive and has a step, now has its own head,
  `counted_loop`: the resolver's `for` rewrite, the printer, the lowering of
  `dbg`, and the code generators' tables write and read it. A program that
  writes `counted_loop` gets the old semantics; it is not documented.
- The AST keeps its Loop node and no backend changed: `for` still becomes
  `(counted_loop i start (- end 1) 1 body...)`, so `(for i 0 end)` is still
  a subtract and an inclusive test, and `end` = i32's least value wraps and
  runs about 2^32 times (the spec's `for` row says so). Making `for` a node
  of its own (an exclusive compare, no subtract) is a change to the checker,
  the VM, wasm.rs, codegen.aipl, the leaks pass, and the printer in both
  toolchains; do it when a benchmark shows it matters. What was done as the
  plan says: both bare parsers (Parser::parse, parser.aipl) read `for`
  directly now, into that same node.
- Every `loop` in the repository was rewritten by tools/retire_loop.py:
  727 in aipl_src, tests/aipl, benchmarks and examples; 69 in tests/*.rs;
  18 in the spec, guide and tour. A step of 1 became `for` with the end
  plus one (`(- X 1)` became `X`, `(- X k)` became `(- X k-1)`, a number
  became the next, anything else `(+ E 1)`: 89 loops in the compiler, so
  the compiled compiler differs from before only by those adds). The four
  with another step were done by hand as `while` (tests/aipl/
  results_by_value.aipl is the one in a program; three more in tests). The
  tests that feed the bare parser or the self-hosted code generator a
  program keep the inclusive form as `counted_loop` (tests/test_selfhost.rs,
  the corpus in tests/test_checker_aipl.rs); test_selfhost's helpers now
  resolve a program that has a `for`. The generators tools/fuzz.py,
  run_fuzz.py (its loops had steps 1 or 2, now only 1; its `while` loops
  stay) and text_vectors.py write `for`.
- The seed (bootstrap/) did not need a refresh: it reads `for`, and the
  compiler's source has no `loop` now. A new seed would only make the
  checked-in binary the one that refuses `loop`; ask the owner when one is
  wanted for another reason.
- Not measured: the benchmarks' times. Their outputs are checked by the
  suite; times are as before apart from the adds above, which run once per
  loop, not per pass.

**Outstanding in step 3:**

- `(for i 0 n 1 ...)`, a step carried over from `loop`: the `1` is taken as
  the first statement of the body and dropped, so the loop runs and does
  the wrong thing without a message. A body that begins with a bare literal
  is almost certainly this; refuse it in both parsers and the resolver's
  `for` shape check with a message naming `while` for another step.
- `for` as a node of its own (the decision above, not built): an exclusive
  compare in the checker (src/checker.rs, aipl_src/checker.aipl), the VM
  (src/vm.rs), the wasm backend (src/compiler/wasm.rs), the self-hosted code
  generator (aipl_src/codegen.aipl, codegen_opt.aipl), the leaks pass, and the
  printer, in both toolchains with byte-equal output. It fixes `end` = i32's
  least value, and drops a subtract per loop entry. Do it if the benchmarks
  below show the subtract matters, or when the owner asks for it.
- Measure the benchmarks again (docs/BENCHMARKS.md): the plan asked for it
  after the loop changes, and 89 loops in the compiler now add 1 to their
  end. Compare against the table dated 2026-10-16.
- `counted_loop` is readable by a program and undocumented. If a model finds
  it, it gets the old inclusive, stepped loop. To close it, the parser
  would have to know whether its text came from a program or from the
  resolver; today it cannot. Revisit if it turns up in generated code.
- A seed refresh is optional (above); do it with the next change that needs
  one, and with the owner's approval.
- Prose that still describes `loop` as a live form, left as history: docs/
  notes/, docs/history/, docs/NOTES_REVIEW.md, and the other plans in
  docs/design/. The spec's and the guide's live text was updated.

## 4. `when` is the only one-branch `if`

`(if c x (block))` and `(when c x)` mean the same. After this step an
`if` whose then or else is an empty `(block)` is an error naming `when`
(`(when (not c) x)` for an empty then), and the repository's uses are
rewritten. `if` always has two real branches; `when` has one.

Open within the step: the launch checklist's friction of `when` around
a value (section 2 there) is looked at here, since `when` becomes the
only form.

## 5. An array literal

Both of the review's first-try failures were a guessed array literal.
`(arr.of T a x y z ...)`: an `(arr T)` from the allocator `a` holding the
values, as `arr.new` takes one (MEMORY_PLAN.md F). Written without the
allocator, the error names it. Every backend, both toolchains.

## 6. The other duplicates

A sweep of the spec for any two forms that do the same thing, with a
proposal for each in this document for the owner. Not duplicates, by
the review's own list: `try` (pass an error up) and `match_result`
(handle it here); `defer` (no other form); nested two-operand
`and`/`or` (composition, not a second form). For those the guide says
when to use which.

## 7. The harness, then the syntax proposals

The review's section 5 proposals (inferred `let` types, inferred generic
arguments, `(f x)` without `call`, `p.f` for a field, a bytes literal)
each make code less explicit, against the owner's preference; the
token savings are the review's estimates. They are decided by
measurement:

- A fixed suite of tasks with expected outputs (the review's 11 to
  start), in tools/harness/.
- Each run gives a model the guide only, and records first-compile
  pass rate, iterations to a correct output, and tokens.
- Several model families, not only Claude: Claude models run through
  subagents here; others (Gemini, GPT) by the owner, or with API keys.
- One change per run, so each language change gets a number.

**Open for the owner:** whether to give the harness API keys for other
families, or to run those by hand.
