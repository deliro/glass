# Pudge Wars Phase 2: Hook Mechanics

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the core hook projectile system — launching, forward movement, chain link spawning, wall bouncing, max-range retraction, and cleanup. No enemy collision or headshot yet (Phase 3).

**Architecture:** Hooks are launched via SpellCast subscription, ticked at 40 Hz via HookTick. Model stores hook state (position, angle, velocity, chain links). Unit handles use `clone()` pattern from `hook_demo.glass` — clone for @external/effect calls, keep original in Model. Chain link WC3 units created via `CreateUnitCallback`, handles accumulated in Model. Direct `@external` calls in update() for high-frequency position updates (SetUnitX/SetUnitY) to avoid effect queue overhead at 40 Hz.

**Tech Stack:** Glass (map code), Rust (compiler fixes if needed)

---

## Key Design Decisions

### Unit Handle Management
- Tip unit created via `CreateUnitCallback` effect → handle stored in Model via clone
- Chain links created via `CreateUnitCallback` → handles accumulated in `List(Unit)`
- Position updates via direct `@external` calls: `unit.set_x(clone(u), x)` per tick
- Removal via `effect.remove_unit(u)` (consumes handle)

### Performance Strategy
- Direct @external calls for per-tick SetUnitX/SetUnitY (no Effect allocation overhead)
- Effects only for discrete events: CreateUnit, RemoveUnit, DamageUnit
- ~20 chain links per hook × 40 Hz = tolerable if using direct calls

### Hook State Machine
```
Idle → SpellCast → Launching (tip moves forward, links spawn) → MaxRange → Retracting (links retract toward caster) → Complete (all units removed) → Idle
```

---

## File Map

### Glass code
- **Modify:** `examples/pudge_wars/types.glass` — Expand HookData with unit handles, chain state
- **Create:** `examples/pudge_wars/hook.glass` — Hook physics: launch, tick, bounce, retract
- **Modify:** `examples/pudge_wars/main.glass` — Wire hook handlers into update()
- **Modify:** `examples/pudge_wars/arena.glass` — Add wall bounce math

### SDK
- **Modify:** `sdk/wc3/unit.glass` — Add missing natives if needed

### Compiler (if needed)
- **Modify:** `src/linearity.rs` — Fix if clone-in-Model pattern causes issues
- **Modify:** `src/codegen.rs` — Fix if direct @external calls in update() have issues

---

## Task 1: Expand HookData and Arena Math

**Files:**
- Modify: `examples/pudge_wars/types.glass`
- Modify: `examples/pudge_wars/arena.glass`

- [ ] **Step 1: Update HookData struct**

Add fields for WC3 unit handles and chain management:

```glass
pub struct ChainLink {
    unit: Unit,
}

pub struct HookData {
    state: HookState,
    tip_x: Float,
    tip_y: Float,
    angle: Float,
    cos_a: Float,
    sin_a: Float,
    velocity: Float,
    max_range: Float,
    distance: Float,
    link_accum: Float,
    damage: Float,
    radius: Float,
    owner_index: Int,
    tip_unit: Option(Unit),
    chain_links: List(ChainLink),
    bounce_count: Int,
}
```

Add new Msg variants to main.glass:

```glass
HookTipCreated { hook_index: Int, tip: Unit }
ChainLinkCreated { hook_index: Int, link: Unit }
```

- [ ] **Step 2: Add wall bounce functions to arena.glass**

```glass
pub fn bounce_horizontal(angle: Float) -> Float {
    case angle < 0.0 {
        True -> 0.0 - 3.14159265 - angle
        False -> 3.14159265 - angle
    }
}

pub fn bounce_vertical(angle: Float) -> Float {
    0.0 - angle
}

pub fn clamp_to_bounds(x: Float, y: Float) -> (Float, Float) {
    let cx = case x < min_x() { True -> min_x() _ -> case x > max_x() { True -> max_x() _ -> x } }
    let cy = case y < min_y() { True -> min_y() _ -> case y > max_y() { True -> max_y() _ -> y } }
    (cx, cy)
}
```

- [ ] **Step 3: Add hook creation helper to types.glass**

```glass
pub fn new_hook(owner_idx: Int, angle: Float, speed: Float, max_range: Float, damage: Float, radius: Float) -> HookData {
    HookData {
        state: HookState::Launching,
        tip_x: 0.0,
        tip_y: 0.0,
        angle: angle,
        cos_a: 0.0,
        sin_a: 0.0,
        velocity: speed * 0.025,
        max_range: max_range,
        distance: 0.0,
        link_accum: 0.0,
        damage: damage,
        radius: radius,
        owner_index: owner_idx,
        tip_unit: Option::None,
        chain_links: [],
        bounce_count: 0,
    }
}
```

Note: cos_a/sin_a will be set after creation using wc3/math.cos and wc3/math.sin.

- [ ] **Step 4: Compile and test**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 5: Commit**

```bash
git add examples/pudge_wars/
git commit -m "feat(pw): expand hook data types with unit handles and arena bounce math"
```

---

## Task 2: Hook Launching

When a player casts Meat Hook (ability 'A000'), create a HookData and request tip unit creation.

**Files:**
- Create: `examples/pudge_wars/hook.glass`
- Modify: `examples/pudge_wars/main.glass`

- [ ] **Step 1: Create hook.glass with launch function**

```glass
import wc3/math
import wc3/unit
import wc3/player
import effect
import option { Option }
import types
import arena

pub fn launch(caster: Unit, target_x: Float, target_y: Float, player_data: types.PlayerData, hook_index: Int) -> (types.HookData, List(effect.Effect(m))) {
    let cx = unit.get_x(clone(caster))
    let cy = unit.get_y(clone(caster))
    let angle = math.atan2(target_y - cy, target_x - cx)
    let hook = types.HookData {
        state: types.HookState::Launching,
        tip_x: cx,
        tip_y: cy,
        angle: angle,
        cos_a: math.cos(angle),
        sin_a: math.sin(angle),
        velocity: player_data.hook_speed * arena.hook_tick_interval(),
        max_range: player_data.hook_range,
        distance: 0.0,
        link_accum: 0.0,
        damage: player_data.hook_damage,
        radius: player_data.hook_radius,
        owner_index: player_data.index,
        tip_unit: Option::None,
        chain_links: [],
        bounce_count: 0,
    }
    let tip_x = cx + hook.velocity * hook.cos_a
    let tip_y = cy + hook.velocity * hook.sin_a
    let owner_id = player.get_id(unit.get_owner(clone(caster)))
    let effects = [
        effect.create_unit_callback(owner_id, 'hOOK', tip_x, tip_y, math.rad2deg(angle), fn(tip: Unit) {
            types.Msg::HookTipCreated { hook_index: hook_index, tip: tip }
        }),
    ]
    (hook, effects)
}
```

- [ ] **Step 2: Wire SpellCast into main.glass update()**

Handle the `SpellCast` message — check if it's a hook ability ('A000'), look up the player data, call `hook.launch`:

```glass
SpellCast(caster, spell_id, target) -> {
    case spell_id == 'A000' {
        True -> {
            let pd = find_player_by_unit(model.players, caster)
            case pd {
                Option::Some(p) -> {
                    let idx = list_length(model.hooks)
                    let result = hook.launch(clone(caster), unit.get_spell_target_x(), unit.get_spell_target_y(), p, idx)
                    // result is (HookData, effects)
                    // Add hook to model
                    ...
                }
                Option::None -> (model, [])
            }
        }
        False -> (model, [])
    }
}
```

Also handle `HookTipCreated` to store the tip unit handle:

```glass
HookTipCreated(hook_index, tip) -> {
    let new_hooks = set_hook_tip(model.hooks, hook_index, tip)
    (Model { ..model, hooks: new_hooks }, [])
}
```

- [ ] **Step 3: Add helper functions**

`find_player_by_unit` — finds player data matching a unit's owner. Since we don't have handle-to-player-data mapping yet, use player ID:

```glass
fn find_player_by_owner(players: List(types.PlayerData), owner_id: Int) -> Option(types.PlayerData) {
    case players {
        [] -> Option::None
        [p | rest] -> case p.index == owner_id {
            True -> Option::Some(p)
            False -> find_player_by_owner(rest, owner_id)
        }
    }
}
```

- [ ] **Step 4: Compile and test**

Run: `cargo test`
Fix any compiler errors (linearity, imports, type resolution).

- [ ] **Step 5: Commit**

```bash
git add examples/pudge_wars/
git commit -m "feat(pw): hook launching — create hook on spell cast, request tip unit"
```

---

## Task 3: Hook Tick — Forward Movement

Each HookTick, advance all active hooks: move tip forward, accumulate distance, check max range.

**Files:**
- Modify: `examples/pudge_wars/hook.glass`
- Modify: `examples/pudge_wars/main.glass`

- [ ] **Step 1: Implement tick_hook for a single hook**

```glass
pub fn tick_launching(hook: types.HookData, caster_x: Float, caster_y: Float) -> (types.HookData, List(effect.Effect(m))) {
    let new_x = hook.tip_x + hook.velocity * hook.cos_a
    let new_y = hook.tip_y + hook.velocity * hook.sin_a
    let new_dist = hook.distance + hook.velocity
    let new_accum = hook.link_accum + hook.velocity

    // Move tip unit (direct call via clone)
    case hook.tip_unit {
        Option::Some(tip) -> {
            let _ = unit.set_x(clone(tip), new_x)
            let _ = unit.set_y(clone(tip), new_y)
            let _ = unit.set_facing(clone(tip), math.rad2deg(hook.angle))
        }
        Option::None -> ()
    }

    // Check wall bounce
    let (final_x, final_y, final_angle, final_cos, final_sin, bounced) =
        check_wall_bounce(new_x, new_y, hook.angle, hook.cos_a, hook.sin_a)

    // Check max range
    let new_state = case new_dist >= hook.max_range {
        True -> types.HookState::Retracting
        False -> types.HookState::Launching
    }

    let new_hook = types.HookData {
        ..hook,
        tip_x: final_x,
        tip_y: final_y,
        angle: final_angle,
        cos_a: final_cos,
        sin_a: final_sin,
        distance: new_dist,
        link_accum: new_accum,
        state: new_state,
        bounce_count: case bounced { True -> hook.bounce_count + 1 False -> hook.bounce_count },
    }
    (new_hook, [])
}
```

Note: The actual implementation will differ as Glass doesn't have tuple destructuring for 6-tuples. Use a BounceResult struct instead.

- [ ] **Step 2: Implement wall bounce check**

```glass
pub struct BounceResult {
    x: Float,
    y: Float,
    angle: Float,
    cos_a: Float,
    sin_a: Float,
    bounced: Bool,
}

pub fn check_wall_bounce(x: Float, y: Float, angle: Float, cos_a: Float, sin_a: Float) -> BounceResult {
    case x < arena.min_x() || x > arena.max_x() {
        True -> {
            let new_angle = arena.bounce_horizontal(angle)
            BounceResult {
                x: case x < arena.min_x() { True -> arena.min_x() _ -> arena.max_x() },
                y: y,
                angle: new_angle,
                cos_a: math.cos(new_angle),
                sin_a: math.sin(new_angle),
                bounced: True,
            }
        }
        False -> case y < arena.min_y() || y > arena.max_y() {
            True -> {
                let new_angle = arena.bounce_vertical(angle)
                BounceResult {
                    x: x,
                    y: case y < arena.min_y() { True -> arena.min_y() _ -> arena.max_y() },
                    angle: new_angle,
                    cos_a: math.cos(new_angle),
                    sin_a: math.sin(new_angle),
                    bounced: True,
                }
            }
            False -> BounceResult { x: x, y: y, angle: angle, cos_a: cos_a, sin_a: sin_a, bounced: False }
        }
    }
}
```

- [ ] **Step 3: Wire HookTick in main.glass**

```glass
HookTick -> {
    let result = tick_all_hooks(model.hooks)
    (Model { ..model, hooks: result.hooks, tick_count: model.tick_count + 1 }, result.effects)
}
```

Where `tick_all_hooks` iterates the hook list and calls the appropriate tick function based on state.

- [ ] **Step 4: Compile, test, fix**

Run: `cargo test`
Expected: Passes. Fix linearity or type errors.

- [ ] **Step 5: Commit**

```bash
git add examples/pudge_wars/
git commit -m "feat(pw): hook tick — forward movement with wall bouncing"
```

---

## Task 4: Chain Link Spawning

Every 27 distance units, create a chain link WC3 unit at the caster's current position. Track link handles in HookData.chain_links.

**Files:**
- Modify: `examples/pudge_wars/hook.glass`
- Modify: `examples/pudge_wars/main.glass`

- [ ] **Step 1: Add chain link spawning to tick_launching**

After accumulating distance, check if link_accum >= 27.0. If so, emit CreateUnitCallback for a chain link at caster position:

```glass
let link_effects = case new_accum >= arena.chain_link_spacing() {
    True -> {
        let owner_id = hook.owner_index
        [effect.create_unit_callback(owner_id, 'hCHN', caster_x, caster_y, math.rad2deg(hook.angle), fn(link: Unit) {
            types.Msg::ChainLinkCreated { hook_index: hook_idx, link: link }
        })]
    }
    False -> []
}

// Reset accumulator
let final_accum = case new_accum >= arena.chain_link_spacing() {
    True -> 0.0
    False -> new_accum
}
```

- [ ] **Step 2: Handle ChainLinkCreated message in main.glass**

```glass
ChainLinkCreated(hook_index, link) -> {
    let new_hooks = add_chain_link(model.hooks, hook_index, link)
    (Model { ..model, hooks: new_hooks }, [])
}
```

Where `add_chain_link` prepends the new Unit to the hook's chain_links list.

- [ ] **Step 3: Move existing chain links each tick**

In tick_launching, after moving the tip, iterate chain_links and move each toward the next link (toward tip):

```glass
fn move_chain_links(links: List(types.ChainLink), tip_x: Float, tip_y: Float, velocity: Float) -> List(types.ChainLink) {
    case links {
        [] -> []
        [link | rest] -> {
            let target_x = case rest { [] -> tip_x [next | _] -> unit.get_x(clone(next.unit)) }
            let target_y = case rest { [] -> tip_y [next | _] -> unit.get_y(clone(next.unit)) }
            let lx = unit.get_x(clone(link.unit))
            let ly = unit.get_y(clone(link.unit))
            let angle = math.atan2(target_y - ly, target_x - lx)
            let nx = lx + velocity * math.cos(angle)
            let ny = ly + velocity * math.sin(angle)
            let _ = unit.set_x(clone(link.unit), nx)
            let _ = unit.set_y(clone(link.unit), ny)
            let _ = unit.set_facing(clone(link.unit), math.rad2deg(angle))
            [link | move_chain_links(rest, tip_x, tip_y, velocity)]
        }
    }
}
```

- [ ] **Step 4: Compile, test, fix**

Run: `cargo test`

- [ ] **Step 5: Commit**

```bash
git add examples/pudge_wars/
git commit -m "feat(pw): chain link spawning and movement"
```

---

## Task 5: Hook Retraction

When hook reaches max range, switch to Retracting state. Chain links move toward caster. Links removed when they reach caster. Tip removed last → hook complete.

**Files:**
- Modify: `examples/pudge_wars/hook.glass`
- Modify: `examples/pudge_wars/main.glass`

- [ ] **Step 1: Implement tick_retracting**

```glass
pub fn tick_retracting(hook: types.HookData, caster_x: Float, caster_y: Float) -> (types.HookData, List(effect.Effect(m))) {
    // Move tip toward caster
    let angle_to_caster = math.atan2(caster_y - hook.tip_y, caster_x - hook.tip_x)
    let new_x = hook.tip_x + hook.velocity * math.cos(angle_to_caster)
    let new_y = hook.tip_y + hook.velocity * math.sin(angle_to_caster)

    case hook.tip_unit {
        Option::Some(tip) -> {
            let _ = unit.set_x(clone(tip), new_x)
            let _ = unit.set_y(clone(tip), new_y)
        }
        Option::None -> ()
    }

    // Move chain links toward caster, remove those that reached caster
    let vel_sq = hook.velocity * hook.velocity
    let result = retract_chain_links(hook.chain_links, caster_x, caster_y, hook.velocity, vel_sq)

    // Check if tip reached caster
    let dx = new_x - caster_x
    let dy = new_y - caster_y
    let dist_sq = dx * dx + dy * dy

    case dist_sq <= vel_sq {
        True -> {
            // Hook complete — remove tip, mark Idle
            let remove_effects = case hook.tip_unit {
                Option::Some(tip) -> [effect.remove_unit(tip)]
                Option::None -> []
            }
            let remove_links = remove_all_links(result.remaining)
            let all_effects = append_effects(remove_effects, remove_links)
            let done_hook = types.HookData { ..hook, state: types.HookState::Idle, tip_unit: Option::None, chain_links: [], tip_x: new_x, tip_y: new_y }
            (done_hook, all_effects)
        }
        False -> {
            let updated = types.HookData { ..hook, tip_x: new_x, tip_y: new_y, chain_links: result.remaining }
            (updated, result.remove_effects)
        }
    }
}
```

- [ ] **Step 2: Implement chain link retraction**

```glass
pub struct RetractResult {
    remaining: List(types.ChainLink),
    remove_effects: List(effect.Effect(m)),
}

fn retract_chain_links(links: List(types.ChainLink), cx: Float, cy: Float, velocity: Float, vel_sq: Float) -> RetractResult {
    case links {
        [] -> RetractResult { remaining: [], remove_effects: [] }
        [link | rest] -> {
            let lx = unit.get_x(clone(link.unit))
            let ly = unit.get_y(clone(link.unit))
            let angle = math.atan2(cy - ly, cx - lx)
            let nx = lx + velocity * math.cos(angle)
            let ny = ly + velocity * math.sin(angle)
            let dx = nx - cx
            let dy = ny - cy
            let dist_sq = dx * dx + dy * dy
            let rest_result = retract_chain_links(rest, cx, cy, velocity, vel_sq)
            case dist_sq <= vel_sq {
                True -> {
                    // Link reached caster, remove it
                    RetractResult {
                        remaining: rest_result.remaining,
                        remove_effects: [effect.remove_unit(link.unit) | rest_result.remove_effects],
                    }
                }
                False -> {
                    let _ = unit.set_x(clone(link.unit), nx)
                    let _ = unit.set_y(clone(link.unit), ny)
                    let _ = unit.set_facing(clone(link.unit), math.rad2deg(angle) + 180.0)
                    RetractResult {
                        remaining: [link | rest_result.remaining],
                        remove_effects: rest_result.remove_effects,
                    }
                }
            }
        }
    }
}
```

- [ ] **Step 3: Clean up completed hooks from model**

In `tick_all_hooks`, filter out hooks with state == Idle after ticking:

```glass
fn is_active(hook: types.HookData) -> Bool {
    case hook.state {
        Idle -> False
        _ -> True
    }
}
```

- [ ] **Step 4: Compile, test, fix**

Run: `cargo test`

- [ ] **Step 5: Commit**

```bash
git add examples/pudge_wars/
git commit -m "feat(pw): hook retraction — links retract to caster, cleanup on complete"
```

---

## Task 6: Integration Testing and Compiler Fixes

Compile the full Pudge Wars to both JASS and Lua. Fix all compiler issues discovered.

**Files:**
- Various compiler files if fixes needed
- `tests/jass_validity.rs`, `tests/lua_validity.rs`

- [ ] **Step 1: Compile to JASS and manually inspect output**

```bash
cargo run -- examples/pudge_wars/main.glass --no-mangle --no-strip > /tmp/pw_phase2.j 2>/tmp/pw_err.txt
```

Inspect for:
- Subscription callbacks present
- Hook functions compiled
- Direct @external calls in update() generate correct JASS
- Chain link list management generates correct SoA code

- [ ] **Step 2: Compile to Lua**

```bash
cargo run -- examples/pudge_wars/main.glass --target lua --no-mangle --no-strip > /tmp/pw_phase2.lua 2>/tmp/pw_err_lua.txt
```

- [ ] **Step 3: Fix all compiler errors**

Common expected issues:
1. **Linearity with clone() in recursive list functions** — chain link iteration clones extensively
2. **Record update syntax with Option(Unit) fields** — `..hook` may not work if linearity flags the old hook's tip_unit
3. **Monomorphization of generic effects** — CreateUnitCallback callback types
4. **@external calls returning Unit in expressions** — `unit.get_x(clone(u))` as let binding

- [ ] **Step 4: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 5: Commit fixes**

```bash
git add -A
git commit -m "fix: compiler fixes for hook physics patterns (linearity, codegen)"
```

---

## Verification

After all tasks:
- [ ] `cargo test` passes all tests
- [ ] JASS output is valid (pjass)
- [ ] Lua output is valid (luac -p)
- [ ] Hook launch generates CreateUnitCallback for tip
- [ ] Each tick moves tip via direct SetUnitX/SetUnitY
- [ ] Chain links created every 27 units at caster position
- [ ] Wall bouncing reflects angle correctly
- [ ] Retraction moves links toward caster and removes them
- [ ] Completed hooks cleaned up from model

## What's Next (Phase 3)

- Enemy collision detection (GroupEnumUnitsInRange)
- Hook grabbing (catch target, drag during retraction)
- Headshot system (two hooks on same target)
- Structure deflection (center 'n006' curves hook)
- Kill/death scoring with gold rewards
- Multiboard updates
