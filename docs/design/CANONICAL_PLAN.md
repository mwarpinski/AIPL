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
| 4 | `when` is the only `if` with one branch | **Done** 2026-10-25 on features/when (outstanding items in section 4) |
| 5 | An array literal, `arr.of` | Done 2026-10-26 (LANGUAGE_DECISIONS.md item 3) |
| 6 | The other duplicates: a sweep of the spec, a proposal for each | Not started |
| 7 | The A/B harness, then the syntax proposals one at a time | Harness built 2026-10-26 (tools/harness/); runs to come |

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
- (Built 2026-10-21, found in the verification below.) A generic's
  application that names no generic function, `(call (vec.psh i32) v 1)`,
  used to say `Expected func name in call, got '('`; both generics passes
  now say `'vec.psh' is not a generic function; did you mean vec.push?`.
  Still open beside it: a typo in a generic struct's name in a type,
  `(ptr (vec.Vecc i32))`, and a program with no generics at all, where the
  generics pass does not run and the parser's message stands.

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
  plus one: `(- X 1)` became `X`, `(- X k)` became `(- X k-1)`, a number
  became the next, anything else `(+ E 1)`. Counted by the tool's own rule
  over the sources before the change (corrected 2026-10-21; an earlier
  count here was wrong): of the 661 loops in aipl_src, 471 had a `(- X 1)`
  end and compile exactly as before (`for` gives back `(- X 1)`); the other
  190 compile with more arithmetic on entry: 95 with a number end now
  compute `(- 8 1)` where they had `7`, 80 compute `(- (+ E 1) 1)` where
  they had `E`, and 15 `(- (- X k-1) 1)` where they had `(- X k)`. Across
  all programs (aipl_src, tests/aipl, benchmarks, examples): 727 loops, 506,
  113, 92, and 15 of those kinds, plus the one with a step of 7. The four
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
- Built later the same day: `(for i 0 n 1 ...)`, a step carried over from
  `loop`, whose `1` would have been taken as the first statement of the body
  and dropped, is an error from both bare parsers and both resolvers: `a for
  has no step: ... for another step, use while`, at the number. Only an
  integer literal right after the end is refused; a step written as a
  variable or an expression (`(for i 0 n step ...)`) is still taken as a
  statement and dropped, a smaller trap left open.
- Not measured: the benchmarks' times. Their outputs are checked by the
  suite; times are as before apart from the extra arithmetic above, which
  runs once per loop, on entry, not per pass.

**Outstanding in step 3:**

- `for` as a node of its own (the decision above, not built): an exclusive
  compare in the checker (src/checker.rs, aipl_src/checker.aipl), the VM
  (src/vm.rs), the wasm backend (src/compiler/wasm.rs), the self-hosted code
  generator (aipl_src/codegen.aipl, codegen_opt.aipl), the leaks pass, and the
  printer, in both toolchains with byte-equal output. It fixes `end` = i32's
  least value, and drops a subtract per loop entry. Do it if the benchmarks
  below show the subtract matters, or when the owner asks for it.
- Measure the benchmarks again (docs/BENCHMARKS.md): the plan asked for it
  after the loop changes, and 190 loops in the compiler (227 across all
  programs, the benchmarks' among them) now do more arithmetic on entry.
  Compare against the table dated 2026-10-16. Folding a number end in the
  resolver's `for` rewrite (`(for i 0 8)` to `(counted_loop i 0 7 1)`, in
  both toolchains) would make 95 of them exact again.
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

**Progress (2026-10-21, branch features/when).** The owner chose the full
approach (A) over a partial one (B: refuse only `(if c x (block))` with a
single-form then, leaving `(if c (block a b) (block))` legal as `when`'s
expansion), because a rule with an exception is an edge case for a model,
the toolchain's own printed output would keep showing the refused form,
and the planned corpus must hold no counterexamples. The difficulty, as in
step 3: the toolchain reads its own output again (the resolver's flat text,
the printers' text), and the resolver expands `when` into exactly
`(if c (block body...) (block))`. So `when` becomes a form of its own,
in five stages, each checked by the full CI before the next:

1. **Done** (commit fe305bd): both bare parsers read `(when c body...)`
   into the tree the resolver's rewrite makes, every part at the when's
   position; `when` is in HEAD_NAMES (so `whn` gets a suggestion); a bare
   `(when)` says `when takes a condition and a body`. Full CI: every test
   passed except two in tests/test_serve.rs, the intermittent failures
   being investigated (docs/HANDOFF.md, Open problems), not this change.
2. **Done** (2026-10-25; full CI before the commit): both printers print `(if c (block b...) (block))` as `(when c b...)`
   (only when the then is a block and the else an empty one), and the
   self-hosted code generator learns `when`. What was found reading
   aipl_src/codegen.aipl: `cond` is the precedent (compiled from its own
   text to the bytes Rust's desugared tree gives), so each place that
   handles `cond` needs `when`. `when` is always void, which leaves four:
   `is_void_node` (about line 1609: true), `node_type` (about 2816: void),
   `groups_walk` (about 3047: walk the condition, then `groups_body` of
   the body with into false), and the compile dispatch (about 3420: a new
   `compile_when` that does what `compile_if` does for a block then and an
   empty else: the condition, OP_IF with block type 64, `label_push
   if_else`, `compile_stmt_seq` of the body under `at_push` of the when's
   position, OP_ELSE, nothing, `label_pop`, OP_END; `seal_node` with
   valued false emits nothing). `diverges_node` needs nothing (a when never
   always leaves: its else is empty). codegen_opt.aipl is not wired into
   any pipeline (`--opt` is not built), so it is left, and listed below.
   Check: tests/test_selfhost.rs byte parity, whose flat() gives the code
   generator the Rust printer's text. Built as described: `compile_when`
   emits what `compile_if` emits for that tree, the position marks
   included; tests/test_selfhost.rs `self_hosted_bytes_match_when` (when
   nested, in a loop with break and continue, with a return, with no body,
   beside a valued if, in a cond) fails if `compile_when` drops its else
   (tried), and tests/test_printer.rs `when_prints_as_when` checks the
   printing and that an `if` whose then is not a block keeps its `if`.
   Outstanding: codegen_opt.aipl does not know `when` (not wired in).
3. **Done** (2026-10-25; full CI before the commit): the resolver (src/sugar.rs, resolver.aipl emit_sugar kind 1) stops
   expanding `when`; the parsers read it (stage 1), and its flat output
   then holds `when`, which stage 2 lets the code generator read.
4. **Done** (2026-10-25; full CI before the commit): both parsers refuse an `if` whose then or else is an empty `(block)`:
   `an if has two real branches: for one, write (when c then...)`, and for
   an empty then `(when (not c) else...)`. Nothing internal writes that
   form after stages 2 and 3 (checked: no generated text in either
   toolchain contains an if with an empty block; the only producers were
   the `when` rewrite and the printers). Decide here the rule for `when`
   around a value: its body is statements, a value is dropped, its type is
   void, as for `while` and `for` (today `(when c (call f x))` with f
   giving an i32 fails with "If branch type mismatch: then is i32, else is
   void", naming an `if` the program does not have). That needs the
   checkers and both code generators to drop a then-branch's value when
   the else is the empty block; or the parsers can append an empty block
   to a when's body, which needs no change behind them, and the printers
   leave it out.
5. **Done** (2026-10-25, commit 2506725; full CI before it): rewrite the repository's uses (counted 2026-10-21 over .aipl and .rs
   files: 2,367 in 83 files; 1,505 `(if c X (block))` with one form as
   then, 798 with a `(block ...)` then, 64 `(if c (block) X)`, and one
   `(if c (block) (block))` to look at by hand), with a tool like
   tools/retire_loop.py; then the spec, the guide (pitfall row 2), the
   tour, and the tests that feed the bare parser or the code generator a
   program. `(if c (block a b) (block))` becomes `(when c a b)` exactly
   (the same tree); `(if c X (block))` becomes `(when c X)`, whose then is
   `(block X)`: check the bytes; `(if c (block) X)` becomes
   `(when (not c) X)`.

Order note: stage 5's rewrite must come before stage 4's refusal, or the
repository stops building; do 5 then 4.

How stage 4 was built: the value rule is the parsers ending a when's body
with an empty block (no change to the checkers, the VM, or wasm.rs; the
printers leave the block out; `compile_when` compiles the body's
expressions as statements, then that block's position marks), so
`(when c (call f x))` with an i32 `f` checks and runs (tests/
test_control_flow.rs `a_whens_body_drops_its_value`), and every existing
`when`, whose last expression was void, compiles to the same bytes. One
internal producer of the refused form was found only in this stage:
`expect`'s lowering, `(if c (block) (call testing.fail ...))`, which
src/formats.rs builds as a tree and aipl_src/fmtlower.aipl writes as text
for the parser; both now make `(when (not c) ...)`. The repository's
first search for producers had looked for the form in one string, and
fmtlower writes it in pieces. Tests: the refusals (both messages, both
toolchains) in tests/test_checker_aipl.rs BAD_PARSES; a when with a value
in FORMS and in tests/test_selfhost.rs `self_hosted_bytes_match_when`.

**Outstanding in step 4:**

- codegen_opt.aipl does not know `when` (it is not wired into any
  pipeline; when `--opt` is built, it must).
- `aipl serve --fresh` should exec aipl-run instead of starting it as a
  child (below). Done 2026-10-26.
- tools/when_rewrite.py rewrote prose inside the guide's fenced
  system-prompt block (it reads every fenced block as code); rules 6 and 7
  were fixed by hand. A later rewrite tool should skip fences that are
  not ```lisp.

Found on the way, not part of the step: `aipl serve --fresh` starts
`aipl-run --serve-fresh` as a child, so killing `aipl serve` by its
process id leaves the server running and listening (Ctrl-C reaches both,
as a process group). It should exec aipl-run instead. Tests that fail
mid-loop leave such servers behind (two were found still listening, hours
later, on 2026-10-21). Done 2026-10-26: it execs aipl-run, which removes
its temporary module once read (`--temporary`).

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


## Verification findings (Opus, 2026-10-21)

The owner's list of what a second review of steps 1 to 3 found, each
marked as it is fixed.

1. aipl doc teaches the retired form. aipl doc map prints "Iterate with
   (loop i 0 ...)", and aipl doc bigint prints (loop k 1 30 1 ...). These are
   module header comments the rewrite tool didn't touch. The guide tells
   models to run aipl doc first, so they would copy an error.
   **Done** 2026-10-21: both comments now show for
   (aipl_src/std/bigint.aipl, map.aipl), and tests/test_doc_examples.rs
   no_program_or_doc_comment_writes_loop fails if any .aipl file in
   aipl_src, examples, benchmarks, or tests/aipl writes a loop form,
   comments included.
2. A wrong suggestion. (set x 1) says "did you mean get?". get, let and set!
   are each one edit away, and the tie goes to whichever sorts first.
   Breaking ties by shared prefix would pick set!.
   **Done** 2026-10-21: of equal distance, the candidate that begins with
   more of the name as written wins, then sort order, in src/parser.rs
   nearest_name and aipl_src/suggest.aipl alike: (set x 1) now says did
   you mean set!?. The case is in tests/test_checker_aipl.rs BAD_PARSES,
   compared word for word.
3. The placeholder diagnosis can blame the wrong thing. A file with an extra
   ) and an innocent (let s:str "{") gets "this string ends inside a
   placeholder". It should only fire when the string is directly followed by
   more code, which is the pattern of the real mistake.
   **Done** 2026-10-21: the diagnosis now needs more code after the string
   (a next token that is not a closing bracket), in src/sexpr.rs read_one
   and aipl_src/resolver.aipl bracket_error alike. (let s:str "{") before
   an extra ) now gets the bracket error itself; the real mistake, {(call
   f "x")}, is still named. tests/test_resolver_aipl.rs
   brace_string_then_extra_paren compares both toolchains. Left: a string
   ending in { that is followed by a symbol, then a bracket error
   elsewhere, is still blamed; telling them apart needs the gap between
   tokens, which the Rust tokens do not keep.
4. Two stale lines in the spec. Line 853 still names the internal form
   (loop ...) rather than counted_loop. Section 13 line 1886 still recommends
   (if c (set! x 1) (block)), while the guide and the new error message say
   when.
   **Done** 2026-10-21: AIPL_SPEC.md 4.M's for row names the internal form
   counted_loop; section 13's one-branch if row says (when c (set! x 1));
   and the two lowering rows (6.x Loops, Locals) that called it a loop say
   counted loop and for.
5. Inaccurate numbers in the plan. It says "89 loops in the compiler" got a
   (+ E 1) end. The real figure for aipl_src is 77 (89 was across all program
   directories). It also says the compiled compiler differs "only by those
   adds", but 95 more loops with a literal end now compile (- 8 1) instead of
   the constant 7.
   **Done** 2026-10-21: the figures were recounted with
   tools/retire_loop.py's own rule over the sources before step 3, and
   both earlier numbers were wrong: 80 loops in aipl_src got an (+ E 1)
   end, not 89 or 77. Of aipl_src's 661 loops, 471 compile exactly as
   before and 190 do more arithmetic on entry (95 number ends, 80 (+ E 1),
   15 (- X k)); across all programs 727: 506, 113, 92, and 15. Section 3
   and its outstanding list now say so.
6. A false entry in AGENT_FRICTION.md. Row 43 says "aipl verify said where"
   for the stray ], but it was caught by re-reading before any verify. Step
   3's real process mistakes aren't logged at all: refusing loop in the
   resolver broke 71 tests; the step-7 loop the tool skipped wasn't converted
   before the CI run; the flat() change broke byte parity; a second constant
   landed inside the impl block.
   **Done** 2026-10-21: row 43 now says the stray ] was caught by re-
   reading, with the correction dated; rows 46 to 50 log step 3's
   mistakes: the resolver refusal that broke 71 tests, the skipped step-7
   loop run through CI, the flat() change that broke byte parity, the
   second const inside impl Parser, and the rewrite tool misreading Rust
   raw strings and a four-backtick Markdown fence.
7. An unhelpful message missing from the plan. A typo in a generic name,
   (call (vec.psh i32) v 1), still says "Expected func name in call, got
   '('". The plan's step 2 outstanding list doesn't mention it.
   **Done** 2026-10-21: fixed rather than only listed: both generics
   passes (src/generics.rs rewrite, aipl_src/generics.aipl
   check_not_generic) now say 'vec.psh' is not a generic function; did you
   mean vec.push?, or how to call one when no name is close;
   tests/test_resolver_aipl.rs generic_name_typo and generic_name_unknown
   compare both toolchains. Step 2's outstanding list records it and what
   is still open beside it (a typo in a generic struct's name in a type; a
   program with no generics). AGENT_FRICTION.md row 17 is marked fixed.
