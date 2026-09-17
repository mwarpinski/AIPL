# PROMPT GUIDE FOR AI AGENTS: Generating and Executing AIPL Code

This document provides system prompt instructions, syntax rules, and zero-shot examples for LLMs (Gemini, Claude, GPT) to generate valid **AIPL (AI Programming Language)** code.

---

## SYSTEM PROMPT MODULE (Include in Agent Context)

```sysprompt
You are an AI agent capable of writing, compiling, and executing AIPL (AI Programming Language).
AIPL is a strict, token-efficient S-Expression language designed for formal verification and WebAssembly execution.

RULES FOR GENERATING AIPL:
1. Wrap all code inside a top-level (module <name> ...).
2. Every function must be declared with (fn <name> [<param:type> ...] -> <RetType> (body...)).
3. All operations are prefix S-expressions: (+ a b), (if cond then else), (let x:i32 10).
4. Use formal contracts (req (<cond>)) and (ens (<cond>)) whenever input domain constraints exist.
5. Do NOT use human syntax sugar like curlies {}, semicolons ;, or indentation sensitivity.
```

---

## Zero-Shot Syntax Examples

### Example 1: Pure Mathematical Computation (Factorial with Invariants)
```lisp
(module math_demo
  (fn factorial [n:i32] -> i32
    (req (gte n 0))
    (ens (gt res 0))
    (if (lte n 1)
        1
        (* n (call factorial (- n 1))))))
```

### Example 2: Vector Matrix Multiplication (SIMD Optimized)
```lisp
(module tensor_ops
  (fn dot_product [a:(vec f64 4) b:(vec f64 4)] -> f64
    (vec.dot a b))

  (fn matrix_mult_2x2 [a:(arr f64 4) b:(arr f64 4)] -> (arr f64 4)
    (matmul a b)))
```

### Example 3: Formally Verified Binary Search
```lisp
(module verified_algo
  (fn binary_search [arr:(arr i32 100) target:i32] -> i32
    (let low:i32 0)
    (let high:i32 99)
    (let result:i32 -1)
    (while (and (lte low high) (eq result -1))
      (let mid:i32 (/ (+ low high) 2))
      (let val:i32 (arr.get arr mid))
      (if (eq val target)
          (set! result mid)
          (if (lt val target)
              (set! low (+ mid 1))
              (set! high (- mid 1)))))
    result))
```

### Example 4: Web Application DOM Rendering (Interactive Browser UI)
```lisp
(module web_app
  (fn render_app [] -> void
    (let root:i32 (web.create_element "div" "container" "AIPL Native Browser App"))
    (let btn:i32 (web.create_element "button" "btn-primary" "Click Me (AIPL)"))
    (web.append_child root btn)
    (web.mount "#app" root)
    (web.on_event btn "click" (fn [e:i32] -> void
      (web.alert "Executed from compiled WebAssembly in browser!")))))
```

---

## Diagnostic Checklist for AI Agents
Before returning AIPL code, verify:
- [ ] Balanced parentheses `(` and `)` across all sub-expressions.
- [ ] Top-level construct is `(module ...)`.
- [ ] Type signatures match returned values (`-> i32`, `-> f64`, `-> void`).
- [ ] Contracts specify `req` (pre-condition) and `ens` (post-condition) correctly.
