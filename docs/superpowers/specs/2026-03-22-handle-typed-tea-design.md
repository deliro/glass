# Handle-Typed TEA

## Problem

The TEA (The Elm Architecture) layer uses `Int` for all handle references — unit IDs, player IDs, ability IDs. This collapses distinct semantic domains into one type. A player ID can be passed as a unit ID, a gold amount as a target — the compiler accepts all of it silently. This violates Glass's core principle: make invalid states unrepresentable.

The current flow has four unnecessary conversions per cycle:

```
WC3 runtime (Unit handle)
  → GetHandleId() → Int
  → Subscription handler receives Int
  → Model stores Int
  → Effect accepts Int
  → glass_handle_lookup_unit(Int) → Unit handle
WC3 runtime
```

## Solution

Pass WC3 handle types (`Unit`, `Player`, `Timer`, etc.) through the entire TEA pipeline. Zero conversions.

```
WC3 runtime (Unit handle)
  → Subscription handler receives Unit
  → Model stores Unit
  → Effect accepts Unit
  → Runtime reads Unit from SoA directly
WC3 runtime
```

Read-only native calls (`get_x`, `get_hp`, `is_alive`) are allowed directly in `update`. Write operations remain Effect-only.

## Design

### Subscriptions

All handler signatures change from Int to typed handles:

| Subscription | Before | After |
|---|---|---|
| OnAttack | `fn(Int, Int) -> M` | `fn(Unit, Unit) -> M` |
| OnDeath | `fn(Int, Int) -> M` | `fn(Unit, Unit) -> M` |
| OnDamage | `fn(Int, Int, Float) -> M` | `fn(Unit, Unit, Float) -> M` |
| OnSpellEffect | `fn(Int, Int, Int) -> M` | `fn(Unit, Int, Unit) -> M` |
| OnSpellCast | `fn(Int, Int) -> M` | `fn(Unit, Int) -> M` |
| OnSpellChannel | `fn(Int, Int) -> M` | `fn(Unit, Int) -> M` |
| OnSpellGround | `fn(Int, Int, Float, Float) -> M` | `fn(Unit, Int, Float, Float) -> M` |
| OnSpellFinish | `fn(Int, Int) -> M` | `fn(Unit, Int) -> M` |
| OnItemPickup | `fn(Int, Int) -> M` | `fn(Unit, Int) -> M` |
| OnItemUse | `fn(Int, Int) -> M` | `fn(Unit, Int) -> M` |
| OnItemDrop | `fn(Int, Int) -> M` | `fn(Unit, Int) -> M` |
| OnUnitEntersRegion | `fn(Int) -> M` | `fn(Unit) -> M` |
| OnChat | `fn(Int, String) -> M` | `fn(Int, String) -> M` (player ID stays Int — no Player handle in TEA yet) |
| OnPlayerLeave | `fn(Int) -> M` | `fn(Int) -> M` (same) |
| OnHeroLevelUp | `fn(Int) -> M` | `fn(Unit) -> M` |
| OnConstructionFinish | `fn(Int) -> M` | `fn(Unit) -> M` |
| OnConstructionStart | `fn(Int) -> M` | `fn(Unit) -> M` |
| OnSummon | `fn(Int, Int) -> M` | `fn(Unit, Unit) -> M` |
| OnUnitSold | `fn(Int, Int) -> M` | `fn(Unit, Unit) -> M` |
| OnItemSold | `fn(Int, Int) -> M` | `fn(Unit, Int) -> M` |
| OnUnitTrained | `fn(Int, Int) -> M` | `fn(Unit, Unit) -> M` |
| OnResearchFinish | `fn(Int, Int) -> M` | `fn(Unit, Int) -> M` |
| OnOrderIssued | `fn(Int, Int) -> M` | `fn(Unit, Int) -> M` |

**Rule:** first param that represents a unit → `Unit`. Ability IDs, item type IDs, order IDs, research IDs stay `Int` (they are raw four-char codes, not handles). Player IDs stay `Int` for now.

### Effects

All `unit_id: Int` fields become `unit: Unit`:

| Field pattern | Before | After |
|---|---|---|
| `unit_id: Int` | `Effect::KillUnit { unit_id: 42 }` | `Effect::KillUnit { unit: my_unit }` |
| `source_id: Int, target_id: Int` | Two Ints | `source: Unit, target: Unit` |
| `player_id: Int` | Stays Int for now | Stays Int (no Player handle in TEA) |
| `owner: Int` (CreateUnit) | Stays Int | Stays Int (player index) |

Affected variants: DamageUnit, RemoveUnit, MoveUnit, PlayAnimation, AddAbility, RemoveAbility, AddSfx (no change — point-based), AddSfxTarget, SetUnitHp, SetUnitMana, SetUnitOwner, PauseUnit, ShowUnit, SetInvulnerable, IssueOrder, IssuePointOrder, IssueTargetOrder, ReviveHero, AddHeroXp, SetUnitFacing, KillUnit, SetUnitMoveSpeed.

CreateUnit keeps `owner: Int` (player index, not a handle).

### FindNearestEnemy callback

Before: `callback: fn(Int) -> M`. After: `callback: fn(Unit) -> M`. The runtime passes the found unit handle directly.

### Model

User stores handles directly:

```glass
pub struct Model {
    hero: Unit,
    time: Int,
}
```

Reading game state in update:

```glass
pub fn update(m: Model, msg: Msg) -> (Model, List(Effect(Msg))) {
    let x = unit.get_x(clone(m.hero))
    let hp = unit.get_hp(clone(m.hero))
    ...
}
```

`clone(m.hero)` creates a borrowed alias. WC3 handles are reference-counted — this is safe.

### SoA Codegen Changes

Currently all struct fields generate `integer array` in JASS. After this change, handle-typed fields generate their JASS handle type:

| Glass type | JASS array type |
|---|---|
| `Int` | `integer array` |
| `Float` | `real array` |
| `String` | `string array` |
| `Bool` | `boolean array` |
| `Unit` | `unit array` |
| `Timer` | `timer array` |
| `Group` | `group array` |
| `Sfx` | `effect array` |
| `Player` | `player array` |
| Any other handle | corresponding JASS handle array |
| Any ADT/struct | `integer array` (SoA pointer) |

The codegen must look up the JASS type for each field's Glass type. The mapping already exists in `type_name_to_jass` / `type_expr_to_jass`.

For Lua target: no array type distinction needed (Lua tables hold any type).

### Runtime Changes

**Subscription registration (Lua):**

Before:
```lua
glass_track_unit(GetAttacker())
glass_track_unit(GetTriggerUnit())
glass_send_msg(handler(GetHandleId(GetAttacker()), GetHandleId(GetTriggerUnit())))
```

After:
```lua
glass_send_msg(handler(GetAttacker(), GetTriggerUnit()))
```

No `glass_track_unit`, no `GetHandleId`. The handle passes through directly.

**Subscription registration (JASS):**

The JASS runtime currently uses `glass_send_msg(tag, p0, p1)` with integer params. This needs to change to pass handle values. Options:
- Store handles in global vars before calling send_msg
- Change send_msg to accept handles
- Use glass_msg_unit_0, glass_msg_unit_1 globals

**Effect execution (Lua):**

Before:
```lua
KillUnit(glass_handle_lookup_unit(fx.unit_id))
```

After:
```lua
KillUnit(fx.unit)
```

**Effect execution (JASS):**

Before:
```jass
call KillUnit(glass_handle_lookup_unit(glass_Effect_KillUnit_unit_id[fx_id]))
```

After:
```jass
call KillUnit(glass_Effect_KillUnit_unit[fx_id])
```

Where `glass_Effect_KillUnit_unit` is a `unit array` instead of `integer array`.

### Handle Table Elimination

`glass_handle_register_unit` / `glass_handle_lookup_unit` / `glass_unit_table` / `glass_handle_ht` become unnecessary for TEA apps. The handle registration system was a workaround for the Int↔Handle gap. With direct handle passing, it can be removed.

Exception: `CreateUnit` effect creates a new unit. The created handle needs to reach the model somehow. Options:
1. `CreateUnitCallback { ..., callback: fn(Unit) -> M }` — runtime creates unit, calls back with handle
2. Fire a synthetic `UnitCreated` message after creation
3. Store created unit in a global and read it next tick

Recommended: option 1, consistent with `FindNearestEnemy` pattern.

### SoA Dealloc: Null Handle Fields

When a SoA slot is deallocated, all handle-typed fields must be set to `null` to release the WC3 reference count. Without this, deallocated slots keep handles alive indefinitely — a memory leak.

The dealloc function generator must iterate variant fields and emit `set glass_Type_Variant_field[id] = null` for every handle-typed field. This requires the dealloc generator to know field types from TypeRegistry.

### JASS Message Passing

JASS `glass_send_msg` currently takes `integer tag, integer p0, integer p1`. With typed handles, unit values cannot pass through integer params.

Solution: typed globals. The subscription trigger action stores handles in globals before calling send_msg:

```jass
unit glass_msg_unit_0 = null
unit glass_msg_unit_1 = null

// In subscription trigger action:
set glass_msg_unit_0 = GetAttacker()
set glass_msg_unit_1 = GetTriggerUnit()
call glass_send_msg(tag)
```

The generated `glass_update` reads from these globals based on the Msg variant's field types. The timer callback (`gen_timer_callback`) follows the same pattern — the After closure returns a Msg SoA ID, and the timer callback reads handle fields from the Msg SoA into the globals before dispatching update.

### Linearity Strategy

The linearity checker must not warn about handles stored in TEA Model fields. The Model is returned from `update`, transferring ownership to the runtime. Handles in the Model are conceptually borrowed for the duration of `update`.

Concrete fix: in `check_function`, if a function is named `update` and returns `(Model, List(Effect(Msg)))`, suppress "unconsumed handle" warnings for handle variables bound from the model parameter. Alternatively, suppress warnings for any handle variable that appears in the function's return expression.

### Null Handle Safety

Some WC3 natives return null handles:
- `GetEventDamageSource()` — null for terrain/trigger damage
- `GetSpellTargetUnit()` — null for point-targeted spells

Subscription handlers that receive potentially-null handles must guard against this. Two options:
- Split subscriptions: `OnSpellEffectUnit` vs `OnSpellEffectPoint`
- Use `Option(Unit)` — but this requires runtime boxing

Recommended: keep `OnSpellEffect` handler signature as `fn(Unit, Int, Unit) -> M` and have the runtime pass a sentinel "null unit" (handle ID 0). Document that target may be null for point-cast spells. The `OnSpellGround` subscription already handles point-targeted spells cleanly with `fn(Unit, Int, Float, Float) -> M`.

### effect.map

The `map` function in `effect.glass` must be updated for all changed variants. `FindNearestEnemy` callback changes from `fn(Int) -> M` to `fn(Unit) -> M` — the map wrapper changes the callback argument type.

### Phases

1. **Codegen SoA**: handle-typed fields → handle-typed arrays + dealloc nulling
2. **Linearity checker**: suppress warnings for Model-contained handles in update
3. **JASS message passing**: add typed globals, update send_msg/timer_callback
4. **Effect + Subscription API**: change all Int → Unit (atomic with examples)
5. **Runtime Lua**: remove GetHandleId/lookup, pass handles directly
6. **Runtime JASS**: same + typed globals + handle-typed SoA access
7. **Handle table removal**: remove registration/lookup infrastructure
8. **CreateUnitCallback**: add callback variant for unit creation
9. **Examples**: update all examples to use typed handles

Phases 4-6 and 9 are atomic — no intermediate compilable state for examples.

## Non-Goals

- Player handle (`Player`) in TEA — deferred. Player indices (0-15) are safe enough as Int for now.
- Item handle in TEA — deferred until item system is more developed. CreateItem has the same callback gap as CreateUnit.
- Newtype optimization (single-field struct → inline) — separate compiler feature.
- `OnUnitEntersRegion.region_id` stays Int — it is a user-defined region index, not a WC3 region handle.

## Risks

- **SoA dealloc correctness**: handle fields must be nulled on dealloc. Failure = WC3 handle memory leak. Mitigated by Phase 1.
- **Timer callback duplication**: `gen_timer_callback` inlines all effect handlers (JASS forward-ref constraint). Every SoA field type change must be applied in both `gen_exec_effect` and `gen_timer_callback`. This is a maintenance burden but cannot be eliminated without JASS restructuring.
- **Backwards compatibility**: all existing examples break. This is acceptable — the examples are internal.
