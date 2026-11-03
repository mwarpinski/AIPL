# Documentation

Everything written about AIPL lives here or at the top of the repository, and this page lists it.

## Start here (top of the repository)

| Document | What it is |
|---|---|
| [README.md](../README.md) | What AIPL is, how to install it, what works today |
| [AIPL_SPEC.md](../AIPL_SPEC.md) | The language and toolchain reference: every form, library module, and command |
| [PROMPT_GUIDE_FOR_AIS.md](../PROMPT_GUIDE_FOR_AIS.md) | What an AI agent needs to write AIPL well |
| [DEVELOPING.md](../DEVELOPING.md) | Working on AIPL itself: layout, tests, rules, decisions already made |
| [ROADMAP.md](../ROADMAP.md) | What is done and what is next |

## Guides (here, in docs/)

| Document | What it is |
|---|---|
| [TOUR.md](TOUR.md) | A ten-minute tour of the language |
| [USING_AIPL.md](USING_AIPL.md) | Build, run, test |
| [FAQ.md](FAQ.md) | Frequently asked questions, platform support included |
| [BENCHMARKS.md](BENCHMARKS.md) | Speed measurements and how they are taken |
| [LAUNCH_CHECKLIST.md](LAUNCH_CHECKLIST.md) | What is left before showing AIPL publicly, and who has each item |

## Design (docs/design/)

Plans written before, and kept after, each piece of work: why it is the way it is.

| Document | What it covers |
|---|---|
| [LANGUAGE_DECISIONS.md](design/LANGUAGE_DECISIONS.md) | Language decisions before release |
| [CANONICAL_PLAN.md](design/CANONICAL_PLAN.md) | One way to write each thing (canonical forms) |
| [AGENT_FRICTION.md](design/AGENT_FRICTION.md) | Where an agent writing AIPL goes wrong, and what would prevent it |
| [aipl_standalone.md](design/aipl_standalone.md) | Nothing Rust can do that AIPL cannot |
| [SYS_PLAN.md](design/SYS_PLAN.md) | System host imports: `chmod` and processes |
| [NET_PLAN.md](design/NET_PLAN.md) | Networking: serving and fetching |
| [STDLIB_PLAN.md](design/STDLIB_PLAN.md) | A broader standard library, and string formatting |
| [MEMORY_PLAN.md](design/MEMORY_PLAN.md) | Memory for programs that run for days |
| [HEAP_PLAN.md](design/HEAP_PLAN.md) | Freeing memory: allocators in the style of Zig |
| [GENERATIONS_PLAN.md](design/GENERATIONS_PLAN.md) | Generation checks: stopping use after free |
| [VALUES_PLAN.md](design/VALUES_PLAN.md) | Small structs by value |
| [CONSTRAINTS_PLAN.md](design/CONSTRAINTS_PLAN.md) | Interfaces, constraints, and checking templates once |
| [CHECKS_PLAN.md](design/CHECKS_PLAN.md) | Contracts and array bounds checks in compiled code |
| [CHECKER_PLAN.md](design/CHECKER_PLAN.md) | The type checker in AIPL |
| [OPT_PLAN.md](design/OPT_PLAN.md) | The optimizer: faster programs by default |
| [CODEGEN_REWRITE.md](design/CODEGEN_REWRITE.md) | Rewriting the self-hosted code generator on typed data |
| [LINES_PLAN.md](design/LINES_PLAN.md) | Source lines in compiled crashes |
| [FEEDBACK_TOOLS_PLAN.md](design/FEEDBACK_TOOLS_PLAN.md) | Fast feedback without a REPL: doc, eval -e, dbg, test, watch |
| [REPL_PLAN.md](design/REPL_PLAN.md) | A self-hosted REPL and WebAssembly interpreter |
| [NATIVE_BACKEND_PLAN.md](design/NATIVE_BACKEND_PLAN.md) | Linux x86-64 native backend |
| [NATIVE_SPEED_PLAN.md](design/NATIVE_SPEED_PLAN.md) | Faster native code |
| [ARM64_PLAN.md](design/ARM64_PLAN.md) | Native ARM64 backend |
| [ARM64_OPT_PLAN.md](design/ARM64_OPT_PLAN.md) | The AArch64 backend's optimizer |

## History (docs/history/)

Records of what happened, kept as written. What they call "next" or "not yet" was true when written; [ROADMAP.md](../ROADMAP.md) is current.

| Document | What it is |
|---|---|
| [WORK_LOG.md](history/WORK_LOG.md) | What was done and found, in order |
| [AUDIT_2026-10.md](history/AUDIT_2026-10.md) | The structural audit of October 2026 |
| [HANDOFF.md](history/HANDOFF.md) | Where AIPL stood on 2026-10-27, for an agent picking the work up elsewhere |
| [APPLE_SILICON_HANDOFF.md](history/APPLE_SILICON_HANDOFF.md) | Instructions for the agent on the Mac (rounds 1 to 4) |
| [APPLE_SILICON_REPORT.md](history/APPLE_SILICON_REPORT.md) | The Apple Silicon port and its verification |

## Notes (docs/notes/)

Outside reviews of AIPL, from other models, with what turned out to be true. [REVIEW.md](notes/REVIEW.md) checks the first set claim by claim; each `.txt` is the original and each `.md` the cleaned-up copy.

| Document | What it is |
|---|---|
| [REVIEW.md](notes/REVIEW.md) | The notes below, checked claim by claim |
| [assessment.md](notes/assessment.md) | AI generation at scale: what it means that the whole code base was AI-written |
| [final_thoughts.md](notes/final_thoughts.md) | Evaluating AIPL for AI-driven code generation |
| [painpoints.md](notes/painpoints.md) | Strengths and friction points seen while developing |
| [pitfalls.md](notes/pitfalls.md) | Architectural assessment and language pitfalls |
| [speed-ups.md](notes/speed-ups.md) | Optimization potential and benchmarking strategy |
| [freedom.md](notes/freedom.md) | Decoupling the toolchain from Cargo |
| [kernel_notes.md](notes/kernel_notes.md) | Bare-metal OS and microkernel exploration |
| [arm64_build.md](notes/arm64_build.md) | Building for ARM64 and macOS |
| [review_and_next_steps.txt](notes/review_and_next_steps.txt) | A review and prioritised next steps, late October 2026 (as found, unedited) |

## Beside the code

| Document | What it is |
|---|---|
| [bootstrap/README.md](../bootstrap/README.md) | The stage-0 compiler (the checked-in seeds) |
| [tools/harness/README.md](../tools/harness/README.md) | The A/B harness: measuring how well agents write AIPL |
| [tools/flake/README.md](../tools/flake/README.md) | Stress scripts for the intermittent test failures |

`python3 tools/docs_check.py` checks that the documents agree with the repository.
