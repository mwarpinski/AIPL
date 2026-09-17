# AISQL Formal Specification (v2.0 Systems & Vector Edition)
## AI-Native Hybrid Relational & Vector Embedding Database Specification

AISQL (AI Structured Query Language) is an ultra-dense, non-ambiguous, formally verifiable query grammar, systems Intermediate Representation (IR), and binary page storage architecture engineered specifically for AI agents, LLM tool calls, and high-performance WebAssembly engines.

---

## 1. Why AISQL S-Expressions vs. Human SQL

Traditional SQL (`SELECT id, name, karma FROM users WHERE karma > 100 ORDER BY karma DESC;`) was designed for human readability in the 1970s. For AI agents, text-based SQL presents serious flaws:
- **Syntax & Parsing Errors**: High risk of LLM syntax errors, forgotten curlies, commas, or semicolons.
- **Context Waste**: Up to 70% of tokens are wasted on keywords (`SELECT`, `FROM`, `WHERE`, `ORDER BY`, `GROUP BY`).
- **Security & Injection**: SQL injection attacks require constant sanitization layers.

**AISQL S-Expression Queries** operate directly on unambiguous Abstract Syntax Trees:
```lisp
(db.query "users"
  (where (gt karma 100))
  (order-by karma desc)
  (select [id name karma]))
```

---

## 2. Updated Formal EBNF Grammar (v2.0)

```ebnf
program        ::= "(" "module" identifier statement* ")" ;
statement      ::= fn_def | let_stmt | set_stmt | expr ;

fn_def         ::= "(" "fn" identifier "[" param* "]" "->" type contract* expr* ")" ;
param          ::= identifier ":" type ;
contract       ::= "(" ("req" | "ens" | "inv") expr ")" ;

type           ::= "i32" | "i64" | "f32" | "f64" | "str" | "(" "vec" "f32" integer ")" ;

expr           ::= literal
                 | identifier
                 | "(" "if" expr expr expr ")"
                 | "(" "call" expr expr* ")"
                 | "(" "ok" expr ")"
                 | "(" "err" expr ")"
                 | "(" "match_result" expr "(" "ok" identifier expr* ")" "(" "err" identifier expr* ")" ")"
                 | "(" op expr expr* ")" ;

op             ::= mem_op | bit_op | atomic_op | comp_op | vector_op ;

mem_op         ::= "mem.load32" | "mem.load64" | "mem.store32" | "mem.store64" | "mem.alloc" | "mem.free" ;
bit_op         ::= "^" | "shl" | "shr" | "bitand" | "bitor" ;
atomic_op      ::= "atomic.add" | "atomic.cas" | "atomic.lock" | "atomic.unlock" ;
comp_op        ::= "eq" | "neq" | "lt" | "lte" | "gt" | "gte" | "and" | "or" | "not" ;
vector_op      ::= "vec.dot" | "db.vector_search" | "db.query" | "db.insert" ;
```

---

## 3. Native Hybrid Relational + AI Vector Similarity Search

AISQL bakes vector similarity search directly into the query engine alongside relational columns without needing external plugins (like `pgvector`).

### SIMD Vector Similarity Search Query
`db.vector_search` executes SIMD dot product and cosine similarity calculations natively across all stored vector embeddings:
```lisp
;; Find Top-5 nearest semantic vector embedding matches for an AI RAG query
(db.vector_search "ai_documents" "embedding" [0.12 0.85 0.43 0.91] 5)
```

---

## 4. Binary Memory Page Storage Format (`.aisql` / `.aipl-db`)

Every AISQL storage file is formatted into binary memory pages loaded into Wasm Linear Memory:
- **Bytes 0-3**: Magic Signature `0x41495351` ("AISQ").
- **Bytes 4-7**: Record Count (`i32`).
- **Bytes 8-11**: Mutex Lock State (`i32`).
- **Bytes 12-15**: Next Free Byte Offset (`i32`).
- **Bytes 32+**: Packed 32-Byte Binary Record Slots + Hash Index Table.
