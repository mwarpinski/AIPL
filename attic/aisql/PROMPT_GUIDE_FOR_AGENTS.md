# PROMPT GUIDE FOR AI AGENTS: AISQL & AIPL Database Queries

This guide provides instructions, grammar rules, and zero-shot examples for LLMs to generate 100% valid **AISQL** database queries.

---

## SYSTEM PROMPT MODULE (Include in AI Agent Context)

```sysprompt
You are an AI Agent operating an AISQL / AIPL Database System.
AISQL is a machine-native S-Expression database language supporting relational queries and SIMD vector embedding similarity search.

RULES FOR GENERATING AISQL QUERIES:
1. Always wrap queries in S-expression parenthesis: (db.query ...), (db.insert ...), (db.vector_search ...).
2. Use prefix operators for conditions: (gt karma 100), (eq status 1), (and (gte age 18) (lt age 65)).
3. Specify formal verification contracts (req (<cond>)) and (ens (<cond>)) on table operations where applicable.
4. For AI RAG / Semantic Search, use (db.vector_search <table> <column> <query_vec> <k>).
```

---

## Zero-Shot Query Examples

### Example 1: Creating a Table with Vector Embeddings
```lisp
(module create_table_demo
  (fn init_schema [] -> void
    (db.create_table "ai_documents"
      [doc_id:i32 author:str text_content:str embedding:(vec f32 128)])))
```

### Example 2: Inserting a Binary Record
```lisp
(module insert_demo
  (fn insert_document [doc_id:i32 author:str text:str] -> i32
    (req (gt doc_id 0))
    (ens (gt res 0))
    (db.insert "ai_documents" [doc_id author text])))
```

### Example 3: Relational Filtering & Selection Query
```lisp
(module query_demo
  (fn get_active_users [min_karma:i32] -> i32
    (req (gte min_karma 0))
    (db.query "users"
      (where (gt karma min_karma))
      (order-by karma desc)
      (select [id name karma]))))
```

### Example 4: AI Vector Embedding Similarity Search (RAG Semantic Match)
```lisp
(module vector_search_demo
  (fn search_similar_docs [query_vec:(vec f32 4)] -> i32
    (db.vector_search "ai_documents" "embedding" query_vec 5)))
```
