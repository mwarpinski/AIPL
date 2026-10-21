# One way to write each thing (canonical forms)

**Status: approved 2026-10-20** (the owner's answers to Sonnet's review of
2026-10-19: retire `loop`, `when` as the only one-branch `if`, measure
the syntax proposals before deciding, and do this batch before NET_PLAN.md
step 5). Step 1 built 2026-10-21.

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
| 1 | Docs: the guide's `and`/`or` contradiction, a version stamp, the top pitfalls first, `aipl doc --find` as step one | **Done** 2026-10-21 |
| 2 | Errors that name the right form, for every guess a model makes | Not started |
| 3 | Retire `loop`: `for` is the only counted loop | Not started |
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
  repository, so it was not used: when it is, revisit the list. Rows 2,
  4 and 8 change when steps 3 to 5 land (the table must follow them).
- `aipl doc --find` is the first thing under "Start here", the first
  line of the system prompt module, and the start of the guide's last
  paragraph.

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

The step: every row of the spec's pitfall table (section 13) that is a
compile error gets a message naming the fix, the same in both
toolchains (tests/test_checker_aipl.rs checks messages for parity), and
a test per row. An unknown op or function close to a real one names
the nearest (`text.splt`: `did you mean text.split?`).

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
