# Glass Compiler — Architectural TODO

18,175 lines across 26 `.rs` files. 309 `.clone()` calls.

---

## P0: DRY violations (exact duplication)

- [x] **1. `find_free_vars` duplicated** — `lift.rs:444` and `closures.rs:197` are near-identical implementations (~150 lines each). The only difference is `Self::` method dispatch vs free function, and `closures.rs` lacks guard handling. Extract to a shared `free_vars` module.

- [ ] **2. `gen_expr` parallel implementations** — `codegen.rs:985` (JASS, 2759 lines) and `lua_codegen.rs:292` (Lua, 1452 lines) both implement the same AST walk with target-specific emission. Pattern condition/binding generation is also duplicated (`gen_pattern_condition`, `gen_pattern_bindings`). Consider a codegen trait or visitor pattern.

- [ ] **3. `runtime.rs` / `lua_runtime.rs` duplication** — both emit Elm architecture runtime (model globals, effect dispatch, subscription setup) for different targets. Shared structure, duplicated logic.

---

## P1: KISS violations (unnecessary complexity)

- [ ] **4. Name mangling is a post-processing hack** — `optimize.rs` does string-level find-and-replace on generated output, byte-by-byte with quote-skip logic. Should mangle at AST/IR level before codegen, not regex on output strings.

- [ ] **5. DCE and topo-sort live inside codegen** — `codegen.rs` lines ~152-153 do dead code elimination and topological sorting during emission. These are optimization passes that belong before codegen, operating on the AST/IR.

- [ ] **6. `current_fn_param_type_names` fallback** — `codegen.rs` lines ~41-42 maintain a fragile heuristic map of param names to types as a fallback when `type_map` lookup fails. Symptom of incomplete type propagation through optimization passes.

- [ ] **7. `#[allow(clippy::indexing_slicing)]`** in both `codegen.rs:984` and `lua_codegen.rs:291` — suppressed clippy warnings on the two largest functions in the project. These are the hot spots that most need bounds safety.

- [x] **8. `#[allow(dead_code)]` on `closures.rs:3`** — removed, all items were actually used. `runtime.rs:13` still has it.

---

## P2: Architectural improvements

- [ ] **9. `codegen.rs` is 2759 lines** — monolithic file with SoA generation, closure dispatch, expr codegen, pattern matching, Elm runtime, DCE, topo-sort. Split into:
  - `codegen/soa.rs` — SoA array generation, alloc/dealloc, constructors
  - `codegen/closure.rs` — capture arrays, dispatch functions
  - `codegen/expr.rs` — expression/pattern codegen
  - `codegen/elm.rs` — runtime infrastructure

- [ ] **10. `infer.rs` is 1920 lines** — type inference, unification orchestration, module collision handling, constructor registry. Could split constructor/type registration from inference logic.

- [ ] **11. Handle types are hard-coded** — `linearity.rs:20-43` has a `const HANDLE_TYPES: &[&str]` with 22 entries. Meanwhile `jass_parser.rs:153` has `is_handle_type()` that walks the JASS type hierarchy dynamically. The linearity checker should use the parsed JASS SDK type hierarchy instead of a manual list.

- [ ] **12. Effect types are hard-coded** in both `runtime.rs` and `lua_runtime.rs` — each new effect requires changes to compiler code. Should be data-driven from `sdk/effect.glass`.

- [ ] **13. Msg parameter slots capped at 4** — `glass_msg_p0..p3` in runtime. Msg variants with >4 fields will silently break.

---

## P3: Bug risks / correctness

- [x] **14. ~~131 `unwrap`/`expect`/`panic!` calls~~** — false positive. The 75 `parser.rs` hits were `Parser::expect(Token)` method calls (returns `Result`), not `Option::expect()`. Actual count: 14 `.unwrap()` + 4 `panic!`, all in `#[cfg(test)]` blocks. Production code is clean.

- [ ] **15. Captured var types default to `"integer"`** — `CapturedVar.jass_type` is a `String` set to `"integer"` when the actual type can't be resolved. Wrong type → JASS pjass validation failure or runtime corruption for non-integer captures.

- [ ] **16. Lexer continues after lex errors** — `token.rs` tokenize doesn't short-circuit on `Err(())` from logos. Garbage tokens can propagate to the parser.

- [ ] **17. No error recovery in parser** — first parse error terminates. User gets one error per compilation.

- [x] **18. Module collision silently requires qualified syntax** — fixed: inferencer now records ambiguous names and emits "ambiguous name `X` — defined in modules: A, B. Use qualified syntax: A.X or B.X" instead of generic "undefined variable".

---

## P4: Code quality

- [ ] **19. 309 `.clone()` calls** — many on `String`, `Vec<Type>`, `HashSet<String>` inside hot loops (inference, codegen). Audit for unnecessary allocations, especially in `infer.rs` (59 clones) and `codegen.rs` (62 clones).

- [x] **20. `parser.rs` type complexity** — replaced tuple `(Spanned<Pattern>, TypeExpr, String, Span)` with `DestructuredParam` struct, removed both clippy suppressions.

- [ ] **21. Optimization pass ordering is implicit** — `main.rs` calls TCO → lift → beta → const_prop → inline in sequence. No framework to declare pass dependencies or compose passes. Adding a pass means manually editing `main.rs` control flow.

- [ ] **22. Global atomic counter for inline alpha-renaming** — `inline.rs` uses `AtomicUsize` for suffix generation. Single-threaded compiler doesn't need atomics.

- [ ] **23. Inlining cost threshold is magic number** — `INLINE_COST_THRESHOLD = 12` with no explanation of why 12 and no way to tune it.

---

## P5: Missing features flagged by architecture

- [ ] **24. No intermediate representation (IR)** — all optimization passes operate directly on the AST. Every pass must handle every `Expr` variant, and adding a new variant requires updating every pass. An ANF or SSA IR would simplify optimization passes significantly.

- [ ] **25. No incremental compilation** — re-parses and re-checks every imported module on every compile. Module caching would help.

- [ ] **26. No source maps** — generated JASS/Lua has no way to trace back to Glass source locations for debugging.

---

## Suggested priority order

| Priority | Items | Rationale |
|----------|-------|-----------|
| **Now** | ~~#1~~, ~~#14~~, #15, ~~#18~~ | DRY violation, crash bugs, silent correctness issues |
| **Soon** | #4, #5, #9, #11 | Architectural debt that compounds with every new feature |
| **Next** | #2, #3, #6, #12, #20 | DRY/KISS that slows velocity |
| **Later** | #19, #21, #22, #23, #24 | Quality and optimization framework |
| **Someday** | #7, #8, #10, #13, #16, #17, #25, #26 | Polish and advanced features |
