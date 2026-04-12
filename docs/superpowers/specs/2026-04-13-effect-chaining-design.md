# Effect Chaining — `_then` variants for sequential effects without update roundtrip

## Problem

Callback effects (`CreateUnitCallback`, `After`, etc.) require a message roundtrip through `update` to chain sequential operations. When the intermediate result (e.g., created unit handle) is only needed by the next effect — not the model — this forces unnecessary boilerplate: define a Msg variant, handle it in `update`, produce the next effects.

Example of current boilerplate for "create unit, then move it":
```glass
// In Msg enum:
HookTipCreated { unit: Unit }

// In update:
HookTipCreated { unit } -> (model, [effect.move_unit(unit, x, y)])

// At call site:
effect.create_unit_callback(owner, type_id, x, y, facing,
    fn(u: Unit) -> Msg { Msg::HookTipCreated { unit: u } })
```

Three separate locations for one sequential operation.

## Design

Add `_then` variants for each callback effect. Instead of `callback: fn(result) -> M` (produces a message), they take `then: fn(result) -> List(Effect(M))` (produces more effects). The runtime executes the returned effects immediately without going through `update`.

### New Effect variants

```glass
AfterThen { duration: Float, then: fn() -> List(Effect(M)) }
CreateUnitThen { owner: Int, type_id: Int, x: Float, y: Float, facing: Float,
                 then: fn(Unit) -> List(Effect(M)) }
FindNearestEnemyThen { x: Float, y: Float, radius: Float,
                       then: fn(Unit) -> List(Effect(M)) }
ForUnitsInRangeThen { x: Float, y: Float, radius: Float,
                      then: fn(Unit) -> List(Effect(M)) }
```

### Constructor functions

```glass
pub fn after_then(duration: Float, then: fn() -> List(Effect(m))) -> Effect(m)
pub fn create_unit_then(owner: Int, type_id: Int, x: Float, y: Float, facing: Float,
                        then: fn(Unit) -> List(Effect(m))) -> Effect(m)
pub fn find_nearest_enemy_then(x: Float, y: Float, radius: Float,
                               then: fn(Unit) -> List(Effect(m))) -> Effect(m)
pub fn for_units_in_range_then(x: Float, y: Float, radius: Float,
                               then: fn(Unit) -> List(Effect(m))) -> Effect(m)
```

### Usage

```glass
// Before (3 locations):
effect.create_unit_callback(owner, type_id, x, y, facing,
    fn(u: Unit) -> Msg { Msg::HookTipCreated { unit: u } })
// + Msg variant + update handler

// After (1 location):
effect.create_unit_then(owner, type_id, x, y, facing,
    fn(u: Unit) -> List(Effect(Msg)) {
        [effect.move_unit(clone(u), x, y), effect.add_sfx_target(u, "model.mdl")]
    })
```

### When to use which

- **`_callback`** — intermediate result needed in model (e.g., store created unit handle in state)
- **`_then`** — intermediate result only needed by the next effect (e.g., move a unit immediately after creating it)

The user always has the choice. `_then` is additive, not a replacement.

## Runtime execution

### Existing `_callback` flow
```
exec_effect(CreateUnitCallback) → CreateUnit → callback(unit) → Msg → glass_send_msg → update → effects
```

### New `_then` flow
```
exec_effect(CreateUnitThen) → CreateUnit → then(unit) → List(Effect) → glass_process_effects
```

No message, no update cycle. Effects returned by `then` are processed recursively — nested `_then` chains work automatically.

### JASS runtime

Current callback effects use 0-delay timers for forward reference avoidance. `_then` variants execute synchronously — call `then()`, get the effect list, process inline. No timer indirection.

```jass
elseif fx_tag == glass_TAG_Effect_CreateUnitThen then
    set u = CreateUnit(Player(owner), type_id, x, y, facing)
    // Call then(u) → returns list of effects
    set effect_list = glass_dispatch_1_unit(then_closure, u)
    // Process returned effects immediately
    call glass_process_effects(effect_list)
```

### Lua runtime

Already synchronous. Same pattern:

```lua
elseif fx.tag == glass_TAG_Effect_CreateUnitThen then
    local u = CreateUnit(Player(fx.owner), fx.type_id, fx.x, fx.y, fx.facing)
    local effects = fx.then_fn(u)
    glass_process_effects(effects)
```

## Effect.map support

Each `_then` variant gets a branch in `map` and `map_list`:

```glass
CreateUnitThen(owner, type_id, x, y, facing, then) ->
    Effect::CreateUnitThen { owner, type_id, x, y, facing,
        then: fn(u: Unit) { map_list(then(u), f) } }
```

## Changes required

| Component | Change | Lines (est.) |
|-----------|--------|-------------|
| sdk/effect.glass | 4 variants + 4 constructors + 4 map branches + 4 map_list branches | ~200 |
| src/runtime.rs | 4 branches in gen_exec_effect (JASS) | ~100 |
| src/lua_runtime.rs | 4 branches in gen_exec_effect (Lua) | ~80 |
| tests | Codegen snapshots for _then variants (JASS + Lua) | ~100 |

**No changes to:** parser, type checker, linearity checker, codegen expressions. `Effect` is a regular enum — `_then` variants are constructed and dispatched like any other.

## Relationship to TEA

This follows Elm's `Task.andThen` precedent — chaining side-effects without intermediate update cycles. The model is not updated between chain steps. If the intermediate result is needed in the model, use the existing `_callback` variant with a message.

## Closes

Issue #5 (Effect chaining: sequential effects with data dependencies)
