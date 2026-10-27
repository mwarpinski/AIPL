# The A/B harness

Does a language designed for AI agents make them write correct programs
more often? This harness measures it the plain way: the same models write
the same 26 small programs in AIPL, Python, and Rust, and every program is
compiled and tested the same way (docs/LAUNCH_CHECKLIST.md section 5,
docs/design/CANONICAL_PLAN.md step 7).

## What is here

- `tasks/NN_name/task.md`: a task in plain language. It is all a model
  sees of the task. Every task reads standard input and writes standard
  output in an exactly specified form, so one set of tests serves every
  language.
- `tasks/NN_name/ref.py`: the reference solution, in Python. It defines
  the expected outputs and is never shown to a model.
- `tasks/NN_name/tests/`: the tests, `NN.in` and `NN.out`, written from
  `ref.py` by `harness.py gen`: the task's example first, then edge
  cases, then generated inputs, some large.
- `harness.py`: builds prompts, compiles and tests programs, runs the
  feedback rounds, records results, and reports them.
- `results/MODEL.jsonl`: one line per program scored.

The tasks cover text processing (word counts, wrapping, an INI reader,
Base64, a hex dump), parsing (RPN, expressions with precedence, Roman
numerals, dates), data structures (a binary search tree, an LRU cache, a
priority queue, anagram groups), algorithms (shortest paths, a
topological order, edit distance, interval merging, primes, Collatz), and
small simulations (a bank, the Game of Life). None is solved in AIPL in
this repository, on purpose.

## The protocol

Fixed, so runs are comparable:

1. **The prompt.** For AIPL: the whole of PROMPT_GUIDE_FOR_AIS.md, then
   the task, then the request. For Python and Rust: the task and the
   request only (the models know those languages). `harness.py prompt
   TASK LANG` prints it.
2. **A fresh context per task**: no earlier conversation, no other files
   or documentation, no tools.
3. **Up to 3 rounds.** If the program fails, the model is sent the
   feedback below in the same conversation and asked for the whole
   corrected program. The feedback is the same for every language:
   - it did not compile: the compiler's message;
   - it stopped with an error, gave a wrong answer, or took more than 10
     seconds on a test: that test's input, the expected and actual
     output, and its standard error.
4. **The reply is used as given**: its first fenced code block (or the
   whole reply if there is none). Do not fix a reply by hand.
5. **Record the model**: its name and version, and settings such as
   temperature, in the `--model` name (for example
   `qwen2.5-coder-32b-q4-t0.2`).

## Running it by hand (any chat, a fresh one per task)

    cargo build --release                       # aipl and aipl-run, for AIPL
    python3 tools/harness/harness.py prompts aipl runs/mymodel-aipl

This writes `TASK.prompt.txt` for every task. Paste each into a fresh
chat, and save the model's reply, unedited, as `TASK.reply.txt` in the
same folder. Then:

    python3 tools/harness/harness.py score aipl runs/mymodel-aipl --model mymodel

For each failure it writes `TASK.feedback.1.txt`: paste that into the
same chat, save the answer as `TASK.reply.2.txt`, and score again with
`--round 2` (then a third round the same way). Do the same with `python`
and `rust` in their own folders.

## Running it with a command (local models, CLIs)

Any command that reads a prompt on standard input and prints the reply
can be the model:

    python3 tools/harness/harness.py run aipl --model qwen2.5-coder-32b \
        --model-cmd "ollama run qwen2.5-coder:32b"
    python3 tools/harness/harness.py run python --model gpt-x --model-cmd "llm -m gpt-x"
    python3 tools/harness/harness.py run rust --model claude-y --model-cmd "claude -p"

Each round sends the whole conversation so far (the prompt, the model's
earlier replies, and the feedback), so the command needs no memory of its
own. `--rounds` (default 3), `--attempts` (repeats of each task, default
1), and `--tasks 01_word_freq,02_column_stats` narrow or widen a run.

A local model's context must hold the AIPL guide: the AIPL prompt is about
38,000 characters (roughly 10,000 tokens), against about 1,000 for Python
and Rust.

## Running it with an agent that has a shell (the workspace)

The two ways above give a model nothing but a prompt. To test an agent that
works in a folder (a coding agent, a local model with tools) on the limited
instructions it would have in the field, build it a workspace:

    harness.py workspace /path/to/ws                      # the guide and the usage page
    harness.py workspace /path/to/ws --docs guide         # the guide alone
    harness.py workspace /path/to/ws --docs guide,spec    # the guide and the full spec

The documents are PROMPT_GUIDE_FOR_AIS.md (the language, by rule and by
example), docs/USING_AIPL.md (how to build, run, and test a program, and
where to look in the library; under 2,000 words), and AIPL_SPEC.md (the
full reference, over 40,000). Running the same model with each set shows how
much instruction the language needs.

The folder holds the documents named (`docs/`), `bin/aipl` and
`bin/aipl-run`, the task texts (`tasks/NN_name.md`), a `README.md` telling
the agent what to do, and an empty `solutions/`. It holds no reference
solution, no tests, no example programs, and no git history, and `selftest`
checks that. Point the agent at that folder and nothing else; it writes
`solutions/NN_name.aipl`, and it can compile and run what it writes on the
example in each task, as an agent would. When it is done:

    harness.py score-workspace /path/to/ws --model NAME-workspace

scores every solution on the hidden tests (a task with no solution file
counts as failed) and records the results like the other ways do. Put the
mode and the documents in the model name (`qwen-32b-guide-only`), so the
report keeps the runs apart. There are no feedback rounds here: the agent's
own compile errors are its only feedback, and the hidden tests tell it
nothing.

What the workspace cannot keep from an agent that can read the whole disk:
the `aipl` binary carries the standard library and the toolchain's sources
and writes them to its module cache (`$AIPL_CACHE`, else
`~/.cache/aipl`) when it first runs, and `aipl doc` prints the library's
documentation. The second is part of the language's tools, and a real user
has it; the first is a leak if the agent goes looking. Run the agent in a
sandbox that sees only the workspace (a container, or a user with no access
to the cache), and say in the write-up which you did.

## Results

    python3 tools/harness/harness.py report

prints a table per model and language:

- **compiles first try**, **passes first try**: of each task's first
  round;
- **passes within its rounds**, **mean rounds to pass**;
- **failures, by kind**: `compile_error` (for AIPL, the checker and the
  parser), `runtime_error` (a trap, a failed contract, an exception, a
  panic), `wrong_answer`, `timeout`. A runtime error found before a wrong
  answer was printed is a bug the language caught;
- **mean lines** of the passing programs (non-blank lines).

Each line of `results/MODEL.jsonl` also has the program's size and the
prompt's and reply's lengths in characters, and for AIPL the
`aipl --version` it was compiled with. To publish a run, commit its
results file and the folder of prompts, replies, and feedback.

## Honest limits

- Python and Rust are the models' best-known languages; AIPL is new to
  every model, with only the guide to go on. That is the comparison the
  claim invites, not a level field.
- Tasks like these are common in training data in Python and Rust, less
  so in AIPL's form; a model may remember a Python solution.
- 26 tasks and a few attempts each make a small sample. Report the
  numbers with their counts, and say where AIPL loses.

## Checking the harness

    python3 tools/harness/harness.py selftest

checks that every `ref.py` passes its own tests (run as a Python
submission) and that the AIPL, Python, and Rust paths compile and run a
small program. tools/ci.sh and CI run it.

## Adding a task

Make `tasks/NN_name/` with a `task.md` (an exact input and output form,
and an example) and a `ref.py` with `solve(input) -> output` and
`cases(rng) -> [input, ...]` (the example first), then run
`harness.py gen` and `harness.py selftest`, and commit the tests.
