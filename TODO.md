# Glass Compiler — TODO

## Milestone 1–9: Completed (summary)

- [x] M1: Lexer, parser, AST, basic codegen, 114 tests
- [x] M2: Types → SoA, pattern matching, record update, tuples, lists
- [x] M3: Closures (alloc + capture), pipe operator
- [x] M4 (partial): Elm runtime preamble, init/update, one-shot effects
- [x] M5: Move checker, auto-cleanup, local fn checker
- [x] M6: common.j parser (1409 natives), auto-bindings, miette errors
- [x] M7: HM type checker, exhaustiveness, advanced patterns, monomorphization, typed AST, function mono, CLI (clap)
- [x] M8: stdlib (Option, Result, List, Int/Float/String, Dict, Set)
- [x] M9 (partial): Module system, DCE, constant folding, 302 tests

---

## Milestone 10: Runtime

- [x] **10.1 Closure dispatch** — all lambdas take glass_clos_id, dispatch by signature group, runtime uses glass_dispatch_void.
- [x] **10.2 Рекурсия + list pattern matching** — factorial, `[h | t]`, head/tail extraction.
- [x] **10.3 @external end-to-end** — proper JASS handle types, native names resolved, pjass validates.
- [x] **10.4 Pure Effects** — `sdk/effect.glass` defines `Effect(M)` ADT (After, DisplayText). `update` returns `#(Model, List(Effect(Msg)))`. Runtime walks effect list and executes. Timer callback self-contained (avoids JASS forward ref cycle).
- [ ] **10.5 Subscriptions + reconciliation** — deferred (not needed for MVP).

## Milestone 11: Codegen correctness (exposed by tower_defense.glass)

**CRITICAL (blocks tower_defense.glass):**

- [ ] **11.1 Closure CALL dispatch** — when a function receives a closure parameter (`fn map(xs, f)`), calling `f(x)` generates `glass_f(x)` (treating `f` as a named function). Should generate `glass_dispatch_integer(f, x)` (dispatch by closure value). Without this, ALL stdlib higher-order functions (list.map, list.filter, option.map, list.fold) produce invalid JASS. **Highest priority.**

- [x] **11.2 Enum tag access** — `glass_tag(x)` → `glass_{TypeName}_tag[x]`. Partially fixed: Case subject type looked up from type_map. Still broken for imported function bodies where type_map has generic types.

- [ ] **11.3 Field access for variant types** — `glass_get_Playing_wave(x)` → `glass_get_Phase_Playing_wave(x)`. Type name missing from getter. Fixed for single-variant types via type_map; broken for vars bound in pattern arms of multi-variant case.

- [ ] **11.4 Positional field access** — `glass_field_0(x)` generated for `Constructor(val)` patterns. Should be `glass_get_{Type}_{Variant}_{field}[x]` like named fields.

- [ ] **11.5 @external resolution for qualified module calls** — `int.to_string(x)` → should be `I2S(x)`, but generates `glass_to_string(x)`. Qualified external names now registered in codegen; need to verify and test.

- [ ] **11.6 Module name collision** — `import int` + `import float` breaks because both define `to_string`. Module resolver deduplicates by function name, losing one. Need: either separate function namespaces per module, or qualified-only access for colliding names.

- [ ] **11.7 Lambda `_` parameter** — `fn(_: a)` generates `_` as JASS param name (invalid). Fixed: generates `glass_unused_N`.

- [ ] **11.8 Temp vars for imported function bodies** — `glass_tmp_N` undeclared in imported functions. Imported bodies now re-inferred for type_map; locals collection should work but verify.

- [ ] **11.9 SoA primitive field types** — `After { duration: Float }` generates `integer array` for Float fields. Need `real array`, `string array` based on field types from type_map or type annotations.

**Lower priority:**

- [ ] **11.10 `todo()` expression** — compile to runtime crash.
- [ ] **11.11 `extend` blocks codegen** — not implemented.

## Milestone 12: Юзабилити

- [ ] **12.1 Multiline expressions** — verify pipe chains, case arms parse across line breaks.
- [ ] **12.2 Better error messages** — "did you mean?", arg count mismatches, unknown fields.
- [ ] **12.3 Watch mode** — `glass watch file.glass`.
- [ ] **12.4 LSP / editor integration** — tree-sitter grammar or minimal language server.

## Milestone 13: Демо

- [ ] **13.1 Minimal playable example** — simple hero + effects. Prove Glass → JASS → WC3 works.
- [ ] **13.2 Tower Defense** — full game on Glass. Blocked by M11.
