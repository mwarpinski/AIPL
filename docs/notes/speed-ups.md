# Optimization Potential and Benchmarking Strategy

An analysis of AIPL's current execution performance, architectural bottlenecks, and practical optimization pathways to approach or exceed C performance.

---

## 1. Current Benchmark Positioning

From [`docs/BENCHMARKS.md`](file:///home/matt/Projects/Repos/AIPL/docs/BENCHMARKS.md#L30-L39), comparing AIPL native x86-64 output against `gcc -O2`:

| Benchmark | AIPL vs `gcc -O2` | Primary Contributing Factor |
|---|---|---|
| `binarytrees`, `mandelbrot` | **1.2x** | Near parity; minimal runtime overhead |
| `spigot`, `fannkuch` | **1.6x – 1.8x** | Basic block codegen and addressing modes |
| `nbody` | **3.0x** | **Register Allocation Pressure:** Single-pass backend spills live floats to memory |
| `spectralnorm` | **4.0x** | GCC auto-vectorizes loop bodies with SIMD (AVX2); AIPL emits scalar code |
| `knucleotide` | **17.4x** | String hashing strategy and unhoisted per-byte generational checks |
| `pidigits` | **10.4x** | C implementation utilizes GMP assembly routines |

---

## 2. Compiler Architecture Evolution

The existing pipeline operates as:
$$\text{AST Optimizer } (\texttt{codegen\_opt}) \longrightarrow \text{WebAssembly} \longrightarrow \text{Single-Pass x86-64 } (\texttt{lower.aipl})$$

### Architectural Enhancements
1. **Tiered Compilation:**
   - **Baseline Tier (`-O0`):** Retain the single-pass Liftoff-style translator for rapid compilation and debug builds.
   - **Optimizing Tier (`-O2`):** Construct an SSA-form intermediate representation from Wasm or AST, enabling global value numbering, loop-invariant code motion (LICM), linear-scan register allocation, and x86 complex addressing mode generation (`[base + index*scale + disp]`).
2. **Language-Level AST Optimizations:**
   Expanding [`codegen_opt.aipl`](file:///home/matt/Projects/Repos/AIPL/aipl_src/codegen_opt.aipl) preserves high-level type and contract metadata that is stripped during lowering to Wasm bytecode.

---

## 3. Structural Advantages of AIPL Over C

While C benefits from decades of compiler tuning, AIPL possesses semantic properties that enable safe, aggressive optimizations:

1. **Strict Type Disjointness & Alias Immunity:**
   In C, non-`restrict` pointers require conservative reloads due to potential pointer aliasing. In AIPL, distinct types do not alias, value structs are immutable copies, and `(plain T)` guarantees isolation, allowing aggressive load-hoisting.
2. **Whole-Program Scope by Default:**
   All module dependencies are resolved into a unified module graph, providing cross-module function inlining, constant propagation, and dead-function stripping without complex external Link-Time Optimization (LTO).
3. **Contract-Guided Invariant Proving:**
   Preconditions (`req`) and postconditions (`ens`) act as formal invariants. Once proven or checked at function boundaries, redundant internal bounds checks can be eliminated safely without introducing undefined behavior.
4. **Arena and Custom Memory Lifecycles:**
   Bulk arena allocations avoid per-object heap allocation overhead and cache fragmentation inherent in standard C `malloc`/`free` patterns.

---

## 4. Key Engineering Priorities to Close the Performance Gap

1. **Loop Register Allocation (SSA / Linear Scan):**
   Ensuring loop accumulators remain in machine registers throughout loop execution directly addresses bottlenecks observed in float-heavy benchmarks like `nbody`.
2. **Hardware Addressing Modes (SIB):**
   Generating native x86 base-index-scale-displacement addressing turns multi-instruction pointer arithmetic into single memory-operand instructions.
3. **SIMD Vectorization Primitives:**
   Exposing explicit vector types and AVX2 operations will allow parallel math execution to match GCC auto-vectorized loop throughput.

