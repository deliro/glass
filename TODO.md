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

- [x] **11.1 Closure CALL dispatch** — closure parameter calls now generate `glass_dispatch_N(f, args)` instead of `glass_f(args)`. All stdlib higher-order functions (list.map, list.filter, option.map, list.fold) produce valid JASS.

- [x] **11.2 Enum tag access** — `glass_tag(x)` → `glass_{TypeName}_tag[x]`. Case subject type looked up from type_map. Bool dispatch subjects wrapped with `glass_i2b()` when coming from dispatch calls.

- [ ] **11.3 Field access for variant types** — `glass_get_Playing_wave(x)` → `glass_get_Phase_Playing_wave(x)`. Type name missing from getter. Fixed for single-variant types via type_map; broken for vars bound in pattern arms of multi-variant case.

- [ ] **11.4 Positional field access** — `glass_field_0(x)` generated for `Constructor(val)` patterns. Should be `glass_get_{Type}_{Variant}_{field}[x]` like named fields.

- [x] **11.5 @external resolution for qualified module calls** — `int.to_string(x)` → `I2S(x)`, `float.to_string(y)` → `R2S(y)`. Qualified external names resolve correctly.

- [x] **11.6 Module name collision** — `import int` + `import float` no longer breaks. Fix: module resolver deduplicates by qualified name (module.fn), inferencer maps each definition to its source module, colliding unqualified names are not bound (only qualified access works). Remaining issue: DCE keeps both versions of colliding imported pub functions → duplicate JASS function definitions when both modules imported.

- [ ] **11.6b Duplicate imported functions in codegen** — when two modules export same-named pub functions (e.g. `int.min` and `float.min`), both end up in JASS output as `glass_min` causing a redefinition error. Fix: either qualify JASS names (`glass_int_min`) or improve DCE to only keep imported functions reachable from user code.

- [x] **11.7 Lambda `_` parameter** — `fn(_: a)` generates `glass_unused_N`.

- [x] **11.8 Temp vars for imported function bodies** — fixed: temp_counter reset per function, body buffered, temps declared after generation. Dedup of locals via HashSet.

- [x] **11.9 SoA primitive field types** — `After { duration: Float }` now generates `real array`. Float/String/Bool/Unit/Sfx fields all get correct JASS array types. Typed pattern locals from ConstructorNamed patterns.

**Lower priority:**

- [ ] **11.10 `todo()` expression** — compile to runtime crash.
- [ ] **11.11 `extend` blocks codegen** — not implemented.

## Milestone 11b: Codegen correctness (fixes applied)

- [x] **11b.1 Temp variable per-function reset** — `fresh_temp()` global counter reset to 0 per function. Body buffered, locals declared after.
- [x] **11b.2 Typed temp variables** — case expression result temps get correct JASS type (boolean, real, etc.) from type_map instead of always integer.
- [x] **11b.3 Boolean dispatch conversion** — `glass_i2b(integer) → boolean` helper. Case subjects from dispatch wrapped automatically.
- [x] **11b.4 Duplicate local dedup** — case arms binding same variable name no longer produce duplicate JASS local declarations (HashSet dedup).
- [x] **11b.5 Typed ConstructorNamed pattern locals** — field JASS types looked up from TypeRegistry, so Unit/Sfx/Float fields declare correct local type.
- [x] **11b.6 `Sfx` handle type** — Glass type mapping to JASS `effect` handle (avoids collision with `Effect(M)` ADT).
- [x] **11b.7 `clone(handle)` allowed** — linearity checker now permits clone for handle types (WC3 runtime is ref-counted). New `Borrowed` state.
- [x] **11b.8 Constructor consumes handles** — handle passed as ADT constructor argument marked as Moved.
- [x] **11b.9 Case arm handle state merge** — after case, handle states merged across arms (Moved > Borrowed > Alive).
- [x] **11b.10 Exhaustiveness skips imports** — exhaustiveness checker skips imported definitions (wrong spans from merged modules).

## Milestone 11c: SDK (new modules)

- [x] **11c.1 `sdk/jass/math.glass`** — sin, cos, atan2, sqrt, random_int, random_real, deg2rad, rad2deg.
- [x] **11c.2 `sdk/jass/unit.glass`** — get_x, get_y, set_x, set_y, set_pos, get_facing, create, remove, handle_id.
- [x] **11c.3 `sdk/jass/sfx.glass`** — at_point, on_unit, destroy.

## Milestone 12: Юзабилити

- [ ] **12.1 Multiline expressions** — verify pipe chains, case arms parse across line breaks.
- [ ] **12.2 Better error messages** — "did you mean?", arg count mismatches, unknown fields.
- [ ] **12.3 Watch mode** — `glass watch file.glass`.
- [ ] **12.4 LSP / editor integration** — tree-sitter grammar or minimal language server.

## Milestone 13: Демо

- [x] **13.1 Spell examples** — Greater Bash (PRD + knockback + dust trail) and Axes of Rexxar (bouncing damage + cooldowns). 3 examples total, all compile to valid JASS (pjass-validated), 306 tests.
- [ ] **13.2 Tower Defense** — full game on Glass.
