# Effect Chaining Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `_then` variants for callback effects that chain effects without update roundtrip.

**Architecture:** 4 new Effect enum variants where `then` returns `List(Effect(M))` instead of `M`. Runtime processes returned effects inline via `glass_process_effects`. No compiler changes — just SDK + runtime codegen.

**Tech Stack:** Glass SDK (`.glass`), Rust runtime codegen (`runtime.rs`, `lua_runtime.rs`), insta snapshots.

---

### Task 1: Add `_then` variants to sdk/effect.glass

**Files:**
- Modify: `sdk/effect.glass`

- [ ] **Step 1: Add 4 new variants to the Effect enum**

In `sdk/effect.glass`, add after `ForUnitsInRange` (line 49), before the closing `}`:

```glass
    AfterThen { duration: Float, then: fn() -> List(Effect(M)) }
    CreateUnitThen { owner: Int, type_id: Int, x: Float, y: Float, facing: Float, then: fn(Unit) -> List(Effect(M)) }
    FindNearestEnemyThen { x: Float, y: Float, radius: Float, then: fn(Unit) -> List(Effect(M)) }
    ForUnitsInRangeThen { x: Float, y: Float, radius: Float, then: fn(Unit) -> List(Effect(M)) }
```

- [ ] **Step 2: Add 4 constructor functions**

After `for_units_in_range` (line 205), add:

```glass
pub fn after_then(duration: Float, then: fn() -> List(Effect(m))) -> Effect(m) {
    Effect::AfterThen { duration, then }
}

pub fn create_unit_then(owner: Int, type_id: Int, x: Float, y: Float, facing: Float, then: fn(Unit) -> List(Effect(m))) -> Effect(m) {
    Effect::CreateUnitThen { owner, type_id, x, y, facing, then }
}

pub fn find_nearest_enemy_then(x: Float, y: Float, radius: Float, then: fn(Unit) -> List(Effect(m))) -> Effect(m) {
    Effect::FindNearestEnemyThen { x, y, radius, then }
}

pub fn for_units_in_range_then(x: Float, y: Float, radius: Float, then: fn(Unit) -> List(Effect(m))) -> Effect(m) {
    Effect::ForUnitsInRangeThen { x, y, radius, then }
}
```

- [ ] **Step 3: Add 4 branches to `map` function**

In the `map` function (after the `ForUnitsInRange` branch, line 284), add:

```glass
        AfterThen(duration, then) ->
            Effect::AfterThen { duration, then: fn() { map_list(then(), f) } }
        CreateUnitThen(owner, type_id, x, y, facing, then) ->
            Effect::CreateUnitThen { owner, type_id, x, y, facing, then: fn(u: Unit) { map_list(then(u), f) } }
        FindNearestEnemyThen(x, y, radius, then) ->
            Effect::FindNearestEnemyThen { x, y, radius, then: fn(u: Unit) { map_list(then(u), f) } }
        ForUnitsInRangeThen(x, y, radius, then) ->
            Effect::ForUnitsInRangeThen { x, y, radius, then: fn(u: Unit) { map_list(then(u), f) } }
```

- [ ] **Step 4: Verify Glass compiles**

Run: `cargo run -- check sdk/effect.glass` (or compile any example that imports effect)
Expected: no parse or type errors

- [ ] **Step 5: Commit**

```bash
git add sdk/effect.glass
git commit -m "feat: add _then effect variants for chaining without update roundtrip"
```

---

### Task 2: JASS runtime — `_then` variant dispatch

**Files:**
- Modify: `src/runtime.rs`

The key difference from `_callback` variants: instead of saving the closure to a timer hashtable and calling `glass_send_msg(callback_result)`, we call the closure synchronously and pass its result (a `List(Effect)`) to `glass_process_effects`.

- [ ] **Step 1: Add `AfterThen` JASS codegen**

In `src/runtime.rs`, in `gen_jass_effect_variant_body` (line 651), add a new match arm:

```rust
        "AfterThen" => gen_jass_after_then_effect(&variant.name, indent, output),
```

Then add the function (near the existing `gen_jass_after_effect`):

```rust
fn gen_jass_after_then_effect(variant_name: &str, indent: &str, output: &mut String) {
    let field = |f| format!("glass_Effect_{}_{}", variant_name, f);
    output.push_str(&format!("{}set t = CreateTimer()\n", indent));
    output.push_str(&format!(
        "{}call SaveInteger(glass_timer_ht, GetHandleId(t), 0, {}[fx_id])\n",
        indent,
        field("then")
    ));
    // cb_type = 2 signals "then" mode: callback returns effect list, not message
    output.push_str(&format!(
        "{}call SaveInteger(glass_timer_ht, GetHandleId(t), 2, 2)\n",
        indent
    ));
    output.push_str(&format!(
        "{}call TimerStart(t, {}[fx_id], false, function glass_timer_callback)\n",
        indent,
        field("duration")
    ));
    output.push_str(&format!("{}set t = null\n", indent));
}
```

- [ ] **Step 2: Add `CreateUnitThen` JASS codegen**

Add match arm in `gen_jass_effect_variant_body`:

```rust
        "CreateUnitThen" => gen_jass_create_unit_then(&variant.name, indent, output),
```

Add the function:

```rust
fn gen_jass_create_unit_then(variant_name: &str, indent: &str, output: &mut String) {
    let field = |f| format!("glass_Effect_{}_{}", variant_name, f);
    // Create the unit
    output.push_str(&format!(
        "{}set u = CreateUnit(Player({}[fx_id]), {}[fx_id], {}[fx_id], {}[fx_id], {}[fx_id])\n",
        indent,
        field("owner"),
        field("type_id"),
        field("x"),
        field("y"),
        field("facing")
    ));
    // Call then(unit) synchronously — returns List(Effect)
    output.push_str(&format!(
        "{}call glass_process_effects(glass_dispatch_1_unit({}[fx_id], u))\n",
        indent,
        field("then")
    ));
    output.push_str(&format!("{}set u = null\n", indent));
}
```

- [ ] **Step 3: Add `FindNearestEnemyThen` JASS codegen**

Add match arm:

```rust
        "FindNearestEnemyThen" => gen_jass_find_nearest_enemy_then(&variant.name, indent, output),
```

Add the function:

```rust
fn gen_jass_find_nearest_enemy_then(variant_name: &str, indent: &str, output: &mut String) {
    let field = |f| format!("glass_Effect_{}_{}", variant_name, f);
    output.push_str(&format!("{}set glass_group_temp = CreateGroup()\n", indent));
    output.push_str(&format!(
        "{}call GroupEnumUnitsInRange(glass_group_temp, {}[fx_id], {}[fx_id], {}[fx_id], null)\n",
        indent,
        field("x"),
        field("y"),
        field("radius")
    ));
    output.push_str(&format!(
        "{}set u = FirstOfGroup(glass_group_temp)\n",
        indent
    ));
    output.push_str(&format!(
        "{}call DestroyGroup(glass_group_temp)\n",
        indent
    ));
    output.push_str(&format!("{}set glass_group_temp = null\n", indent));
    output.push_str(&format!("{}if u != null then\n", indent));
    output.push_str(&format!(
        "{}    call glass_process_effects(glass_dispatch_1_unit({}[fx_id], u))\n",
        indent,
        field("then")
    ));
    output.push_str(&format!("{}endif\n", indent));
    output.push_str(&format!("{}set u = null\n", indent));
}
```

- [ ] **Step 4: Add `ForUnitsInRangeThen` JASS codegen**

Add match arm:

```rust
        "ForUnitsInRangeThen" => gen_jass_for_units_in_range_then(&variant.name, indent, output),
```

Add the function:

```rust
fn gen_jass_for_units_in_range_then(variant_name: &str, indent: &str, output: &mut String) {
    let field = |f| format!("glass_Effect_{}_{}", variant_name, f);
    output.push_str(&format!("{}set glass_group_temp = CreateGroup()\n", indent));
    output.push_str(&format!(
        "{}call GroupEnumUnitsInRange(glass_group_temp, {}[fx_id], {}[fx_id], {}[fx_id], null)\n",
        indent,
        field("x"),
        field("y"),
        field("radius")
    ));
    output.push_str(&format!("{}loop\n", indent));
    output.push_str(&format!(
        "{}    set u = FirstOfGroup(glass_group_temp)\n",
        indent
    ));
    output.push_str(&format!("{}    exitwhen u == null\n", indent));
    output.push_str(&format!(
        "{}    call glass_process_effects(glass_dispatch_1_unit({}[fx_id], u))\n",
        indent,
        field("then")
    ));
    output.push_str(&format!(
        "{}    call GroupRemoveUnit(glass_group_temp, u)\n",
        indent
    ));
    output.push_str(&format!("{}endloop\n", indent));
    output.push_str(&format!(
        "{}call DestroyGroup(glass_group_temp)\n",
        indent
    ));
    output.push_str(&format!("{}set glass_group_temp = null\n", indent));
}
```

- [ ] **Step 5: Update `glass_timer_callback` for `AfterThen`**

In the timer callback codegen (where `cb_type` is checked), add handling for `cb_type == 2`:

Find the section in `gen_timer_callback` that checks `cb_type`. After the existing `cb_type == 1` branch, add:

```jass
elseif cb_type == 2 then
    // _then mode: callback returns List(Effect), not a message
    call glass_process_effects(glass_dispatch_void(closure_id))
```

This replaces the `glass_send_msg` path — instead of producing a message and calling update, it produces effects and processes them directly.

- [ ] **Step 6: Verify compilation**

Run: `cargo test --bin glass`
Expected: all existing tests pass (new variants don't affect existing codegen)

- [ ] **Step 7: Commit**

```bash
git add src/runtime.rs
git commit -m "feat: JASS runtime dispatch for _then effect variants"
```

---

### Task 3: Lua runtime — `_then` variant dispatch

**Files:**
- Modify: `src/lua_runtime.rs`

Lua is simpler — no timer indirection needed. Call `then()` synchronously, pass result to `glass_process_effects`.

- [ ] **Step 1: Add all 4 `_then` variant bodies**

In `gen_lua_effect_variant_body` (line 171), add match arms:

```rust
        "AfterThen" => gen_lua_after_then_effect(indent, output),
        "CreateUnitThen" => gen_lua_create_unit_then(indent, output),
        "FindNearestEnemyThen" => gen_lua_find_nearest_enemy_then(indent, output),
        "ForUnitsInRangeThen" => gen_lua_for_units_in_range_then(indent, output),
```

- [ ] **Step 2: Implement `gen_lua_after_then_effect`**

```rust
fn gen_lua_after_then_effect(indent: &str, output: &mut String) {
    output.push_str(&format!("{}local trig = CreateTrigger()\n", indent));
    output.push_str(&format!("{}local cb = fx.then_fn\n", indent));
    output.push_str(&format!(
        "{}TriggerRegisterTimerEvent(trig, fx.duration, false)\n",
        indent
    ));
    output.push_str(&format!("{}TriggerAddAction(trig, function()\n", indent));
    output.push_str(&format!(
        "{}    glass_process_effects(cb())\n",
        indent
    ));
    output.push_str(&format!("{}end)\n", indent));
}
```

- [ ] **Step 3: Implement `gen_lua_create_unit_then`**

```rust
fn gen_lua_create_unit_then(indent: &str, output: &mut String) {
    output.push_str(&format!(
        "{}local u = CreateUnit(Player(fx.owner), fx.type_id, fx.x, fx.y, fx.facing)\n",
        indent
    ));
    output.push_str(&format!(
        "{}glass_process_effects(fx.then_fn(u))\n",
        indent
    ));
}
```

- [ ] **Step 4: Implement `gen_lua_find_nearest_enemy_then`**

```rust
fn gen_lua_find_nearest_enemy_then(indent: &str, output: &mut String) {
    output.push_str(&format!("{}local g = CreateGroup()\n", indent));
    output.push_str(&format!(
        "{}GroupEnumUnitsInRange(g, fx.x, fx.y, fx.radius, nil)\n",
        indent
    ));
    output.push_str(&format!("{}local best = FirstOfGroup(g)\n", indent));
    output.push_str(&format!("{}DestroyGroup(g)\n", indent));
    output.push_str(&format!("{}if best ~= nil then\n", indent));
    output.push_str(&format!(
        "{}    glass_process_effects(fx.then_fn(best))\n",
        indent
    ));
    output.push_str(&format!("{}end\n", indent));
}
```

- [ ] **Step 5: Implement `gen_lua_for_units_in_range_then`**

```rust
fn gen_lua_for_units_in_range_then(indent: &str, output: &mut String) {
    output.push_str(&format!("{}local g = CreateGroup()\n", indent));
    output.push_str(&format!(
        "{}GroupEnumUnitsInRange(g, fx.x, fx.y, fx.radius, nil)\n",
        indent
    ));
    output.push_str(&format!("{}local u = FirstOfGroup(g)\n", indent));
    output.push_str(&format!("{}while u ~= nil do\n", indent));
    output.push_str(&format!(
        "{}    glass_process_effects(fx.then_fn(u))\n",
        indent
    ));
    output.push_str(&format!("{}    GroupRemoveUnit(g, u)\n", indent));
    output.push_str(&format!("{}    u = FirstOfGroup(g)\n", indent));
    output.push_str(&format!("{}end\n", indent));
    output.push_str(&format!("{}DestroyGroup(g)\n", indent));
}
```

- [ ] **Step 6: Handle `then` field name in Lua codegen**

The Lua codegen accesses fields as `fx.field_name`. For `_then` variants, the callback field is named `then` in the Glass enum. Check if `then` is a Lua reserved word — it IS (`if...then...end`). The Lua codegen must use a safe name.

In the Lua SoA table generation, `then` fields should be emitted as `then_fn` to avoid Lua keyword collision. Check `src/lua_codegen.rs` for how field names are emitted in table constructors — if field names are emitted verbatim, add a `then` → `then_fn` mapping in the Lua codegen's field name sanitizer (similar to JASS's `safe_jass_name`).

If no sanitizer exists, rename the field in the Glass enum from `then` to `chain` to avoid the collision entirely. This is the simpler fix.

**Decision:** Rename the field from `then` to `chain` in all 4 variants in `sdk/effect.glass` (Task 1). Update the constructor functions and map branches to use `chain` instead of `then`. This avoids Lua keyword issues without any codegen changes.

- [ ] **Step 7: Go back to Task 1 and rename `then` → `chain` everywhere**

In `sdk/effect.glass`, replace all `then:` fields and `then` parameters with `chain:` / `chain`. This affects:
- 4 enum variant field names
- 4 constructor function parameter names
- 4 map branches

- [ ] **Step 8: Verify compilation**

Run: `cargo test --bin glass`
Expected: all tests pass

- [ ] **Step 9: Commit**

```bash
git add src/lua_runtime.rs
git commit -m "feat: Lua runtime dispatch for _then effect variants"
```

---

### Task 4: Codegen snapshot tests

**Files:**
- Modify: `src/codegen_tests.rs`

- [ ] **Step 1: Add a test for `create_unit_then`**

In `src/codegen_tests.rs`, add a new `#[case]` to the parity test:

```rust
#[case::create_unit_then(
    "create_unit_then",
    "
pub struct Msg { tag: Int }
pub fn test() -> Int {
    effect.create_unit_then(0, 1148481101, 0.0, 0.0, 0.0, fn(u: Unit) -> List(effect.Effect(Msg)) {
        [effect.move_unit(u, 100.0, 200.0)]
    })
}
"
)]
```

Note: This test may need adjustment depending on how the compiler handles `effect.Effect(Msg)` references. If imports are needed, use the pattern from existing tests.

- [ ] **Step 2: Run the test, accept the snapshot**

Run: `cargo test create_unit_then`
Expected: snapshot mismatch (new snapshot). Review the generated JASS and Lua:
- JASS should contain `call glass_process_effects(glass_dispatch_1_unit(...))`
- Lua should contain `glass_process_effects(fx.chain(u))`

Accept: `cargo insta test --accept -- create_unit_then`

- [ ] **Step 3: Add a test for `after_then`**

```rust
#[case::after_then(
    "after_then",
    "
pub struct Msg { tag: Int }
pub fn test() -> Int {
    effect.after_then(2.0, fn() -> List(effect.Effect(Msg)) {
        []
    })
}
"
)]
```

- [ ] **Step 4: Run and accept snapshot**

Run: `cargo insta test --accept -- after_then`

- [ ] **Step 5: Run full test suite**

Run: `cargo test --bin glass`
Expected: all tests pass, including new snapshots

- [ ] **Step 6: Run clippy and fmt**

Run: `cargo clippy && cargo fmt -- --check`
Expected: zero errors

- [ ] **Step 7: Commit**

```bash
git add src/codegen_tests.rs src/snapshots/
git commit -m "test: codegen snapshots for _then effect variants (JASS + Lua)"
```

---

### Task 5: Verify end-to-end and close issue

**Files:** none (verification only)

- [ ] **Step 1: Run full test suite**

Run: `cargo test --bin glass`
Expected: all tests pass

- [ ] **Step 2: Verify clippy clean**

Run: `cargo clippy`
Expected: zero errors

- [ ] **Step 3: Verify fmt clean**

Run: `cargo fmt -- --check`
Expected: no diff

- [ ] **Step 4: Final commit and push**

```bash
git push
```

- [ ] **Step 5: Close issue #5**

```bash
gh issue close 5 --comment "Effect chaining implemented via _then variants. CreateUnitThen, AfterThen, FindNearestEnemyThen, ForUnitsInRangeThen allow chaining effects without update roundtrip."
```

---

## Verification checklist

- [ ] 4 new Effect variants in `sdk/effect.glass`
- [ ] 4 constructor functions
- [ ] 4 map branches + 4 map_list branches (via map_list in map branches)
- [ ] JASS runtime dispatches all 4 `_then` variants
- [ ] Lua runtime dispatches all 4 `_then` variants
- [ ] `then` field renamed to `chain` to avoid Lua keyword collision
- [ ] `AfterThen` uses timer with `cb_type == 2` in JASS
- [ ] `CreateUnitThen`, `FindNearestEnemyThen`, `ForUnitsInRangeThen` call `glass_process_effects` synchronously
- [ ] Codegen snapshot tests for at least 2 variants
- [ ] All existing tests still pass
- [ ] clippy + fmt clean
