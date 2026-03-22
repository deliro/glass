# Handle-Typed TEA Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Int handle IDs with typed WC3 handles (Unit, Timer, etc.) throughout the TEA Effect/Subscription/Model pipeline.

**Architecture:** SoA codegen already maps `Unit` → `unit array`. The changes are: (1) dealloc nulls handle fields, (2) linearity checker tolerates handles in returned structs, (3) Effect/Subscription/Runtime/Examples all change atomically from Int to Unit.

**Tech Stack:** Rust (compiler), Glass (SDK), JASS/Lua (targets)

**Spec:** `docs/superpowers/specs/2026-03-22-handle-typed-tea-design.md`

---

### Task 1: SoA Dealloc Nulls Handle Fields

**Files:**
- Modify: `src/codegen.rs:683-699` (gen_dealloc_fn)

Handle-typed SoA slots must be nulled on dealloc to release WC3 reference counts. Without this, stale handle references in freed slots prevent garbage collection.

- [ ] **Step 1: Write a test**

Add to `src/codegen_tests.rs`:
```rust
#[test]
fn dealloc_nulls_handle_fields() {
    let source = r#"
pub struct HeroState { hero: Unit, level: Int }
pub fn init() -> (HeroState, List(Int)) { (HeroState { hero: todo(), level: 1 }, []) }
pub fn update(m: HeroState, msg: Int) -> (HeroState, List(Int)) { (m, []) }
"#;
    let jass = compile_to_jass(source);
    assert!(jass.contains("glass_HeroState_HeroState_hero [id] = null"),
        "dealloc must null handle-typed fields, got:\n{}", jass);
}
```

- [ ] **Step 2: Run test — expect FAIL**

Run: `cargo test --bin glass dealloc_nulls`

- [ ] **Step 3: Implement dealloc nulling in JASS codegen**

In `src/codegen.rs`, `gen_dealloc_fn` (line ~683). Add field nulling before the free-list push. `FieldInfo` has `jass_type: String` — check it directly:

```rust
if let Some(info) = self.types.types.get(type_name) {
    for variant in &info.variants {
        for field in &variant.fields {
            match field.jass_type.as_str() {
                "unit" | "player" | "timer" | "group" | "trigger" | "effect"
                | "force" | "sound" | "location" | "rect" | "region"
                | "dialog" | "quest" | "multiboard" | "leaderboard"
                | "texttag" | "lightning" | "image" | "ubersplat"
                | "trackable" | "timerdialog" | "fogmodifier" | "hashtable" => {
                    self.emit(&format!(
                        "set glass_{}_{}_{} [id] = null",
                        type_name, variant.name, field.name
                    ));
                }
                _ => {}
            }
        }
    }
}
```

Note: Lua codegen has no dealloc functions (Lua uses GC'd tables). This step is JASS-only.

- [ ] **Step 4: Run test — expect PASS**

Run: `cargo test --bin glass dealloc_nulls`

- [ ] **Step 5: Full validation**

Run: `cargo test --bin glass && cargo clippy`
Run pjass on all examples: `for f in examples/*.glass examples/game/main.glass; do cargo run -- "$f" --no-check -o /tmp/t.j 2>/dev/null && tools/pjass tests/common_stub.j /tmp/t.j 2>&1 | tail -1; done`
Expected: All pass

- [ ] **Step 6: Commit**

```bash
git add src/codegen.rs src/codegen_tests.rs
git commit -m "fix: null handle-typed SoA fields on dealloc to prevent WC3 handle leaks"
```

---

### Task 2: Linearity Checker — Handle in Returned Structs

**Files:**
- Modify: `src/linearity.rs:112-140` (check_function) — only if test fails

- [ ] **Step 1: Write test**

```rust
#[test]
fn handle_in_returned_struct_no_warning() {
    let warns = warnings(r#"
pub struct State { hero: Unit, time: Int }
fn test(s: State) -> State { State(..s, time: 5) }
"#);
    assert!(warns.is_empty(), "no warning for handle returned in struct: {:?}", warns);
}
```

- [ ] **Step 2: Run test**

Run: `cargo test --bin glass handle_in_returned_struct`

If PASS: the checker already doesn't register struct params as handles. Skip to Step 4.
If FAIL: implement suppression in Step 3.

- [ ] **Step 3: Implement (only if Step 2 fails)**

In `check_function`, suppress "unconsumed handle" warnings for handle variables that are part of the return value's struct.

- [ ] **Step 4: Write additional test for destructured handles**

```rust
#[test]
fn destructured_handle_returned_in_struct_no_warning() {
    let warns = warnings(r#"
pub struct State { hero: Unit, time: Int }
fn test(State { hero, time } as s: State) -> State { State { hero, time: time + 1 } }
"#);
    assert!(warns.is_empty(), "no warning when destructured handle is returned: {:?}", warns);
}
```

- [ ] **Step 5: Run and fix if needed, then commit**

```bash
git add src/linearity.rs
git commit -m "test: verify linearity checker handles returned structs with handles"
```

---

### Task 3: Atomic SDK + Runtime + Examples Migration (Int → Unit)

**Files (all changed in one commit):**
- Modify: `sdk/effect.glass` — all `unit_id: Int` → `unit: Unit`
- Modify: `sdk/subscription.glass` — handler `fn(Int, ...) → fn(Unit, ...)`
- Modify: `src/lua_runtime.rs` — remove GetHandleId/lookup, pass handles directly
- Modify: `src/runtime.rs` — remove glass_handle_lookup_unit, read from handle-typed SoA
- Modify: all `examples/*.glass`, `examples/game/**/*.glass`
- Snapshots: accept updated snapshots

This task is atomic — all these files must change together. The codebase is not compilable between sub-steps. Do all sub-steps before committing.

#### Sub-step 3a: Effect SDK

In `sdk/effect.glass`, change every `unit_id: Int` to `unit: Unit`, `source_id: Int` to `source: Unit`, `target_id: Int` to `target: Unit`. Leave `player_id: Int`, `owner: Int`, `ability_id: Int`, `item_type_id: Int` as Int.

22 variants affected (see spec lines 79). Update all constructor functions. Update the `map` function — all phantom variants just reconstruct with new field names. `FindNearestEnemy` callback changes from `fn(uid: Int)` to `fn(u: Unit)`.

#### Sub-step 3b: Subscription SDK

In `sdk/subscription.glass`, change handler signatures per spec table (lines 40-64). Unit-representing params become `Unit`, ability/item/order/research IDs stay `Int`, player IDs stay `Int`. Update all constructor functions.

#### Sub-step 3c: Lua Runtime

In `src/lua_runtime.rs`:

**Subscriptions:** Remove all `glass_track_unit()` and `GetHandleId()`. Pass handles directly:
- Before: `handler(GetHandleId(GetAttacker()), GetHandleId(GetTriggerUnit()))`
- After: `handler(GetAttacker(), GetTriggerUnit())`

**Effects:** Remove all `glass_handle_lookup_unit(fx.unit_id)`. Read handle field directly:
- Before: `KillUnit(glass_handle_lookup_unit(fx.unit_id))`
- After: `KillUnit(fx.unit)`

**FindNearestEnemy:** `handler(GetHandleId(best))` → `handler(best)`

**Remove:** `glass_track_unit` function, `glass_unit_table` global, `glass_handle_lookup_unit` function.

#### Sub-step 3d: JASS Runtime

In `src/runtime.rs`:

**Effects (gen_exec_effect + gen_timer_callback — BOTH must be updated in sync):**
Remove all `glass_handle_lookup_unit(glass_Effect_*_unit_id[fx_id])` wrappers. Replace with direct SoA access:
- Before: `call KillUnit(glass_handle_lookup_unit(glass_Effect_KillUnit_unit_id[fx_id]))`
- After: `call KillUnit(glass_Effect_KillUnit_unit[fx_id])`

This applies to every effect variant in both `gen_exec_effect` AND `gen_timer_callback` (they duplicate the effect handling due to JASS forward-reference constraints).

**Remove:** `glass_handle_register_unit`, `glass_handle_lookup_unit` function generation. Remove `glass_handle_ht` global. Remove handle registration in CreateUnit effect handler.

**Note:** JASS runtime has NO subscription registration code. Subscriptions are JASS-side are handled via codegen of triggers in `codegen.rs`, not runtime.rs. The `glass_send_msg` function stays as-is for now — it receives integer tag + packed params. JASS subscription trigger actions still use `GetHandleId()` to pass unit info as integers through msg params. Full typed globals for JASS subscriptions is deferred (the JASS subscription codegen is in `codegen.rs`, not `runtime.rs`, and is complex to change).

#### Sub-step 3e: Examples

Update all examples that use Effect/Subscription with unit IDs.

**Examples that use unit handles and NEED changes:**
- `hook_demo.glass` — SpellGround handler, damage_unit, move_unit, find_nearest_enemy
- `game/main.glass` — OnAttack, OnDeath, OnSpellEffect, damage_unit, create_unit
- `game/heroes/pudge.glass` — damage_unit, display_text with unit IDs
- `game/heroes/sniper.glass` — damage_unit
- `game/heroes/paladin.glass` — damage_unit, set_unit_hp
- `game/systems/damage.glass` — if exists, damage functions
- `arena_test.glass` — create_unit, damage_unit
- `tower_defense.glass` — create_unit, damage_unit, remove_unit
- `greater_bash.glass` — uses wc3/unit directly, may not use Effect
- `axes_rexxar.glass` — damage_unit effects
- `buff_system.glass` — damage_unit, set_unit_hp
- `chain_lightning.glass` — damage_unit, find_nearest_enemy
- `rune_system.glass` — create_unit, set_unit_hp
- `item_combine.glass` — item effects

**Examples that likely DON'T need changes:**
- `elm_counter.glass` — counter only, no units
- `elm_timer.glass` — timer only, no units
- `add.glass` — arithmetic only
- `types.glass` — type demo only
- `sdk_smoke.glass` — may need changes if it tests effects
- `stdlib_smoke.glass` — stdlib only, no effects

For each example: update Msg enum fields (`unit_id: Int` → `unit: Unit`), update handler lambdas in `subscriptions`, update Effect constructor calls, add `clone()` where needed for read-only native calls on model handles.

#### Sub-step 3f: Build and validate

- [ ] Run: `cargo build`
- [ ] Run: `cargo insta accept` (snapshot updates expected)
- [ ] Run: `cargo test --bin glass`
- [ ] Run: `cargo clippy`
- [ ] Run pjass validation on all 17 examples
- [ ] All must pass

#### Sub-step 3g: Commit

```bash
git add sdk/ src/ examples/ src/snapshots/
git commit -m "refactor: TEA uses typed Unit handles instead of Int IDs

Effect, Subscription, and all examples now use Unit handles directly.
Runtime passes handles through without GetHandleId/lookup roundtrip.
Handle table infrastructure removed from Lua runtime."
```

---

### Task 4: CreateUnitCallback Effect

**Files:**
- Modify: `sdk/effect.glass`
- Modify: `src/lua_runtime.rs`
- Modify: `src/runtime.rs`

- [ ] **Step 1: Add variant + constructor + map arm**

```glass
CreateUnitCallback { owner: Int, type_id: Int, x: Float, y: Float, facing: Float, callback: fn(Unit) -> M }
```

Constructor and map arm follow `FindNearestEnemy` pattern (callback wrapping).

- [ ] **Step 2: Wire Lua runtime**

```lua
elseif fx.tag == glass_TAG_Effect_CreateUnitCallback then
    local u = CreateUnit(Player(fx.owner), fx.type_id, fx.x, fx.y, fx.facing)
    local msg = fx.callback(u)
    glass_send_msg(msg)
```

- [ ] **Step 3: Wire JASS runtime (gen_exec_effect + gen_timer_callback)**

Create unit, store in a temporary `unit` local, dispatch callback closure with the unit handle.

- [ ] **Step 4: Write test**

Codegen test that verifies `CreateUnitCallback` generates correct JASS with unit callback dispatch.

- [ ] **Step 5: Full validation + commit**

```bash
git add sdk/effect.glass src/lua_runtime.rs src/runtime.rs src/codegen_tests.rs
git commit -m "feat: CreateUnitCallback effect returns typed Unit handle via callback"
```

---

### Task 5: Final Cleanup

- [ ] **Step 1: Verify no stale handle infrastructure remains**

Grep for `glass_handle_lookup_unit`, `glass_track_unit`, `glass_unit_table`, `glass_handle_register_unit`, `glass_handle_ht` in all generated output. None should appear.

- [ ] **Step 2: Full test + validation**

```bash
cargo test && cargo clippy
```
All 17 examples pjass-clean. All examples compile to Lua.

- [ ] **Step 3: Commit if any cleanup needed**

```bash
git commit -m "chore: final handle-typed TEA cleanup"
```
