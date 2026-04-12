# Pudge Wars Phase 1: JASS Subscriptions + PW Skeleton

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Get a minimal Pudge Wars skeleton compiling to both JASS and Lua with working subscriptions, proving the Elm architecture can drive a real WC3 map.

**Architecture:** Elm TEA pattern with init/update/subscriptions. Phase 1 covers: JASS subscription runtime (compiler fix), PW native SDK extensions, game data model, arena initialization, basic hook launching via spell subscription, and 40 Hz hook tick via timer subscription. No physics, no scoring, no items yet — just "cast hook → dummy projectile moves forward → retracts."

**Tech Stack:** Rust (compiler), Glass (map code), JASS/Lua (targets), pjass (validation)

---

## File Map

### Compiler changes
- **Modify:** `src/runtime.rs` — Add JASS subscription registration (port from lua_runtime.rs)
- **Modify:** `src/codegen.rs` — Wire subscription closure dispatch into JASS codegen

### SDK extensions
- **Create:** `sdk/wc3/player.glass` — Player natives not yet exposed
- **Create:** `sdk/wc3/group.glass` — Group enumeration natives
- **Create:** `sdk/wc3/hashtable.glass` — Hashtable natives for per-unit data
- **Modify:** `sdk/wc3/unit.glass` — Additional unit natives (scale, invulnerable, etc.)

### Pudge Wars Glass code
- **Create:** `examples/pudge_wars/main.glass` — Entry point: init, update, subscriptions
- **Create:** `examples/pudge_wars/types.glass` — All data types (Model, Msg, Hook, Player, etc.)
- **Create:** `examples/pudge_wars/arena.glass` — Arena constants, boundary helpers
- **Create:** `examples/pudge_wars/hook.glass` — Hook launching and tick logic

### Tests
- **Modify:** `tests/jass_validity.rs` — Add PW compilation test, subscription tests
- **Modify:** `tests/lua_validity.rs` — Add PW Lua compilation test

---

## Task 1: JASS Subscription Runtime — Globals & Callback Scaffolding

The JASS runtime currently has no subscription support. JASS can't use anonymous functions, so each subscription type needs a named callback function plus a global to store the handler closure ID.

**Files:**
- Modify: `src/runtime.rs`

- [ ] **Step 1: Write failing test — JASS subscriptions compile**

Add to `tests/jass_validity.rs`:

```rust
#[test]
fn jass_subscriptions_basic() {
    compile_and_validate_with_natives(
        r#"
import effect
import subscription

pub enum Msg {
    HeroKilled { victim: Unit, killer: Unit }
    Tick
}

pub struct Model {
    kills: Int,
}

pub fn init() -> (Model, List(effect.Effect(Msg))) {
    (Model { kills: 0 }, [])
}

pub fn update(model: Model, msg: Msg) -> (Model, List(effect.Effect(Msg))) {
    case msg {
        HeroKilled(victim, killer) -> (Model { kills: model.kills + 1 }, [])
        Tick -> (model, [])
    }
}

pub fn subscriptions(model: Model) -> List(subscription.Subscription(Msg)) {
    [
        subscription.on_death(fn(victim: Unit, killer: Unit) { Msg::HeroKilled { victim, killer } }),
        subscription.on_timer(1.0, fn() { Msg::Tick }),
    ]
}
"#,
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test jass_subscriptions_basic -- --nocapture`
Expected: FAIL — JASS runtime does not emit subscription registration code, so the subscriptions function is compiled but never called. The output should compile but subscriptions won't be wired.

Actually, the test may pass syntactically but subscriptions won't work at runtime. Let's verify compilation first, then add the runtime code.

- [ ] **Step 3: Add subscription handler globals to runtime**

In `src/runtime.rs`, modify `collect_runtime_globals` to add subscription handler storage:

```rust
pub fn collect_runtime_globals(globals: &mut Vec<String>) {
    globals.push("    // ========== Glass Elm Runtime ==========".into());
    globals.push("    integer glass_model = 0".into());
    globals.push("    integer glass_msg_tag = 0".into());
    for i in 0..4 {
        globals.push(format!("    integer glass_msg_p{} = 0", i));
    }
    globals.push("    hashtable glass_timer_ht = null".into());
    globals.push("    group glass_group_temp = null".into());
    globals.push("    multiboard glass_multiboard = null".into());
    globals.push("    string array glass_BoardRow_label".into());
    globals.push("    string array glass_BoardRow_value".into());
    // Subscription handler closure IDs
    globals.push("    integer glass_sub_on_attack = -1".into());
    globals.push("    integer glass_sub_on_death = -1".into());
    globals.push("    integer glass_sub_on_timer_handler = -1".into());
    globals.push("    real glass_sub_on_timer_interval = 0.0".into());
    globals.push("    integer glass_sub_on_spell_effect = -1".into());
    globals.push("    integer glass_sub_on_spell_cast = -1".into());
    globals.push("    integer glass_sub_on_spell_channel = -1".into());
    globals.push("    integer glass_sub_on_damage = -1".into());
    globals.push("    integer glass_sub_on_item_pickup = -1".into());
    globals.push("    integer glass_sub_on_item_use = -1".into());
    globals.push("    integer glass_sub_on_item_drop = -1".into());
    globals.push("    integer glass_sub_on_chat = -1".into());
    globals.push("    integer glass_sub_on_player_leave = -1".into());
    globals.push("    integer glass_sub_on_hero_level_up = -1".into());
    globals.push("    integer glass_sub_on_construction_finish = -1".into());
    globals.push("    integer glass_sub_on_spell_ground = -1".into());
    globals.push("    integer glass_sub_on_summon = -1".into());
    globals.push("    integer glass_sub_on_unit_sold = -1".into());
    globals.push("    integer glass_sub_on_item_sold = -1".into());
    globals.push("    integer glass_sub_on_unit_trained = -1".into());
    globals.push("    integer glass_sub_on_research_finish = -1".into());
    globals.push("    integer glass_sub_on_construction_start = -1".into());
    globals.push("    integer glass_sub_on_spell_finish = -1".into());
    globals.push("    integer glass_sub_on_order_issued = -1".into());
}
```

- [ ] **Step 4: Run `cargo check` to verify compilation**

Run: `cargo check`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/runtime.rs
git commit -m "feat: add JASS subscription handler globals to runtime"
```

---

## Task 2: JASS Subscription Callback Functions

Generate named JASS callback functions for each subscription type. Each callback reads event data, dispatches the handler closure, and calls `glass_send_msg`.

**Files:**
- Modify: `src/runtime.rs`

- [ ] **Step 1: Add `gen_subscription_callbacks` function**

In `src/runtime.rs`, add a function that emits named JASS callbacks. These are self-contained functions that JASS triggers can reference via `function glass_sub_cb_on_death`. Each one:
1. Reads WC3 event data via JASS natives
2. Calls the appropriate `glass_dispatch_N_*` function with the handler closure ID and event args
3. Passes the resulting Msg to `glass_send_msg`

```rust
fn gen_subscription_callbacks(entry: &ElmEntryPoints, output: &mut String) {
    // OnAttack: handler(attacker, attacked) -> Msg
    output.push_str("function glass_sub_cb_on_attack takes nothing returns nothing\n");
    output.push_str("    local unit u0 = GetAttacker()\n");
    output.push_str("    local unit u1 = GetTriggerUnit()\n");
    output.push_str("    local integer msg = glass_dispatch_2_unit_unit(glass_sub_on_attack, u0, u1)\n");
    output.push_str("    set u0 = null\n");
    output.push_str("    set u1 = null\n");
    output.push_str("    call glass_send_msg(msg, 0, 0)\n");
    output.push_str("endfunction\n\n");

    // OnDeath: handler(dying_unit, killing_unit) -> Msg
    output.push_str("function glass_sub_cb_on_death takes nothing returns nothing\n");
    output.push_str("    local unit u0 = GetTriggerUnit()\n");
    output.push_str("    local unit u1 = GetKillingUnit()\n");
    output.push_str("    local integer msg = glass_dispatch_2_unit_unit(glass_sub_on_death, u0, u1)\n");
    output.push_str("    set u0 = null\n");
    output.push_str("    set u1 = null\n");
    output.push_str("    call glass_send_msg(msg, 0, 0)\n");
    output.push_str("endfunction\n\n");

    // OnTimer: handler() -> Msg
    output.push_str("function glass_sub_cb_on_timer takes nothing returns nothing\n");
    output.push_str("    local integer msg = glass_dispatch_void(glass_sub_on_timer_handler)\n");
    output.push_str("    call glass_send_msg(msg, 0, 0)\n");
    output.push_str("endfunction\n\n");

    // OnSpellEffect: handler(caster, spell_id, target) -> Msg
    output.push_str("function glass_sub_cb_on_spell_effect takes nothing returns nothing\n");
    output.push_str("    local unit u0 = GetTriggerUnit()\n");
    output.push_str("    local integer i0 = GetSpellAbilityId()\n");
    output.push_str("    local unit u1 = GetSpellTargetUnit()\n");
    output.push_str("    local integer msg = glass_dispatch_3_unit_integer_unit(glass_sub_on_spell_effect, u0, i0, u1)\n");
    output.push_str("    set u0 = null\n");
    output.push_str("    set u1 = null\n");
    output.push_str("    call glass_send_msg(msg, 0, 0)\n");
    output.push_str("endfunction\n\n");

    // OnDamage: handler(source, target, amount) -> Msg
    output.push_str("function glass_sub_cb_on_damage takes nothing returns nothing\n");
    output.push_str("    local unit u0 = GetEventDamageSource()\n");
    output.push_str("    local unit u1 = GetTriggerUnit()\n");
    output.push_str("    local real r0 = GetEventDamage()\n");
    output.push_str("    local integer msg = glass_dispatch_3_unit_unit_real(glass_sub_on_damage, u0, u1, r0)\n");
    output.push_str("    set u0 = null\n");
    output.push_str("    set u1 = null\n");
    output.push_str("    call glass_send_msg(msg, 0, 0)\n");
    output.push_str("endfunction\n\n");

    // OnChat: handler(player_id, message) -> Msg
    output.push_str("function glass_sub_cb_on_chat takes nothing returns nothing\n");
    output.push_str("    local integer pid = GetPlayerId(GetTriggerPlayer())\n");
    output.push_str("    local string s = GetEventPlayerChatString()\n");
    output.push_str("    local integer msg = glass_dispatch_2_integer_string(glass_sub_on_chat, pid, s)\n");
    output.push_str("    call glass_send_msg(msg, 0, 0)\n");
    output.push_str("endfunction\n\n");

    // OnPlayerLeave: handler(player_id) -> Msg
    output.push_str("function glass_sub_cb_on_player_leave takes nothing returns nothing\n");
    output.push_str("    local integer pid = GetPlayerId(GetTriggerPlayer())\n");
    output.push_str("    local integer msg = glass_dispatch_1_integer(glass_sub_on_player_leave, pid)\n");
    output.push_str("    call glass_send_msg(msg, 0, 0)\n");
    output.push_str("endfunction\n\n");

    // OnHeroLevelUp: handler(unit) -> Msg
    output.push_str("function glass_sub_cb_on_hero_level_up takes nothing returns nothing\n");
    output.push_str("    local unit u0 = GetTriggerUnit()\n");
    output.push_str("    local integer msg = glass_dispatch_1_unit(glass_sub_on_hero_level_up, u0)\n");
    output.push_str("    set u0 = null\n");
    output.push_str("    call glass_send_msg(msg, 0, 0)\n");
    output.push_str("endfunction\n\n");

    // OnSpellGround: handler(caster, spell_id, x, y) -> Msg
    output.push_str("function glass_sub_cb_on_spell_ground takes nothing returns nothing\n");
    output.push_str("    local unit u0 = GetTriggerUnit()\n");
    output.push_str("    local integer i0 = GetSpellAbilityId()\n");
    output.push_str("    local real r0 = GetSpellTargetX()\n");
    output.push_str("    local real r1 = GetSpellTargetY()\n");
    output.push_str("    local integer msg = glass_dispatch_4_unit_integer_real_real(glass_sub_on_spell_ground, u0, i0, r0, r1)\n");
    output.push_str("    set u0 = null\n");
    output.push_str("    call glass_send_msg(msg, 0, 0)\n");
    output.push_str("endfunction\n\n");

    // Remaining subscription callbacks follow the same pattern.
    // OnSpellCast, OnSpellChannel, OnItemPickup, OnItemUse, OnItemDrop,
    // OnConstructionFinish, OnSummon, OnUnitSold, OnItemSold, OnUnitTrained,
    // OnResearchFinish, OnConstructionStart, OnSpellFinish, OnOrderIssued
    // will be added as needed when PW code uses them.
}
```

Note: The `glass_dispatch_N_*` functions are generated by the closure system in codegen.rs. The exact signatures depend on what closures the user's Glass code creates. We'll need to verify these signatures exist in the compiled output.

- [ ] **Step 2: Add `gen_register_subscriptions` for JASS**

This function walks the subscription list (a Glass List, represented as linked SoA nodes) and stores handler closure IDs in the globals, then creates appropriate JASS triggers:

```rust
fn gen_register_subscriptions_jass(output: &mut String) {
    output.push_str("function glass_register_subscriptions takes integer subs returns nothing\n");
    output.push_str("    local integer current = subs\n");
    output.push_str("    local integer sub_id\n");
    output.push_str("    local integer sub_tag\n");
    output.push_str("    local trigger t\n");
    output.push_str("    local integer i\n");
    output.push_str("    loop\n");
    output.push_str("        exitwhen current == -1\n");
    output.push_str("        set sub_id = glass_List_integer_head[current]\n");
    output.push_str("        set sub_tag = glass_Subscription_tag[sub_id]\n");

    // OnAttack
    output.push_str("        if sub_tag == glass_TAG_Subscription_OnAttack then\n");
    output.push_str("            set glass_sub_on_attack = glass_Subscription_OnAttack_handler[sub_id]\n");
    output.push_str("            set t = CreateTrigger()\n");
    output.push_str("            set i = 0\n");
    output.push_str("            loop\n");
    output.push_str("                exitwhen i >= bj_MAX_PLAYER_SLOTS\n");
    output.push_str("                call TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_ATTACKED, null)\n");
    output.push_str("                set i = i + 1\n");
    output.push_str("            endloop\n");
    output.push_str("            call TriggerAddAction(t, function glass_sub_cb_on_attack)\n");

    // OnDeath
    output.push_str("        elseif sub_tag == glass_TAG_Subscription_OnDeath then\n");
    output.push_str("            set glass_sub_on_death = glass_Subscription_OnDeath_handler[sub_id]\n");
    output.push_str("            set t = CreateTrigger()\n");
    output.push_str("            set i = 0\n");
    output.push_str("            loop\n");
    output.push_str("                exitwhen i >= bj_MAX_PLAYER_SLOTS\n");
    output.push_str("                call TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_DEATH, null)\n");
    output.push_str("                set i = i + 1\n");
    output.push_str("            endloop\n");
    output.push_str("            call TriggerAddAction(t, function glass_sub_cb_on_death)\n");

    // OnTimer
    output.push_str("        elseif sub_tag == glass_TAG_Subscription_OnTimer then\n");
    output.push_str("            set glass_sub_on_timer_handler = glass_Subscription_OnTimer_handler[sub_id]\n");
    output.push_str("            set t = CreateTimer()\n");
    output.push_str("            call TimerStart(t, glass_Subscription_OnTimer_interval[sub_id], true, function glass_sub_cb_on_timer)\n");

    // OnSpellEffect
    output.push_str("        elseif sub_tag == glass_TAG_Subscription_OnSpellEffect then\n");
    output.push_str("            set glass_sub_on_spell_effect = glass_Subscription_OnSpellEffect_handler[sub_id]\n");
    output.push_str("            set t = CreateTrigger()\n");
    output.push_str("            set i = 0\n");
    output.push_str("            loop\n");
    output.push_str("                exitwhen i >= bj_MAX_PLAYER_SLOTS\n");
    output.push_str("                call TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_SPELL_EFFECT, null)\n");
    output.push_str("                set i = i + 1\n");
    output.push_str("            endloop\n");
    output.push_str("            call TriggerAddAction(t, function glass_sub_cb_on_spell_effect)\n");

    // OnDamage
    output.push_str("        elseif sub_tag == glass_TAG_Subscription_OnDamage then\n");
    output.push_str("            set glass_sub_on_damage = glass_Subscription_OnDamage_handler[sub_id]\n");
    output.push_str("            set t = CreateTrigger()\n");
    output.push_str("            set i = 0\n");
    output.push_str("            loop\n");
    output.push_str("                exitwhen i >= bj_MAX_PLAYER_SLOTS\n");
    output.push_str("                call TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_DAMAGED, null)\n");
    output.push_str("                set i = i + 1\n");
    output.push_str("            endloop\n");
    output.push_str("            call TriggerAddAction(t, function glass_sub_cb_on_damage)\n");

    // OnChat
    output.push_str("        elseif sub_tag == glass_TAG_Subscription_OnChat then\n");
    output.push_str("            set glass_sub_on_chat = glass_Subscription_OnChat_handler[sub_id]\n");
    output.push_str("            set t = CreateTrigger()\n");
    output.push_str("            set i = 0\n");
    output.push_str("            loop\n");
    output.push_str("                exitwhen i >= bj_MAX_PLAYER_SLOTS\n");
    output.push_str("                call TriggerRegisterPlayerChatEvent(t, Player(i), \"\", false)\n");
    output.push_str("                set i = i + 1\n");
    output.push_str("            endloop\n");
    output.push_str("            call TriggerAddAction(t, function glass_sub_cb_on_chat)\n");

    // OnPlayerLeave
    output.push_str("        elseif sub_tag == glass_TAG_Subscription_OnPlayerLeave then\n");
    output.push_str("            set glass_sub_on_player_leave = glass_Subscription_OnPlayerLeave_handler[sub_id]\n");
    output.push_str("            set t = CreateTrigger()\n");
    output.push_str("            set i = 0\n");
    output.push_str("            loop\n");
    output.push_str("                exitwhen i >= bj_MAX_PLAYER_SLOTS\n");
    output.push_str("                call TriggerRegisterPlayerEvent(t, Player(i), EVENT_PLAYER_LEAVE)\n");
    output.push_str("                set i = i + 1\n");
    output.push_str("            endloop\n");
    output.push_str("            call TriggerAddAction(t, function glass_sub_cb_on_player_leave)\n");

    // OnHeroLevelUp
    output.push_str("        elseif sub_tag == glass_TAG_Subscription_OnHeroLevelUp then\n");
    output.push_str("            set glass_sub_on_hero_level_up = glass_Subscription_OnHeroLevelUp_handler[sub_id]\n");
    output.push_str("            set t = CreateTrigger()\n");
    output.push_str("            set i = 0\n");
    output.push_str("            loop\n");
    output.push_str("                exitwhen i >= bj_MAX_PLAYER_SLOTS\n");
    output.push_str("                call TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_HERO_LEVEL, null)\n");
    output.push_str("                set i = i + 1\n");
    output.push_str("            endloop\n");
    output.push_str("            call TriggerAddAction(t, function glass_sub_cb_on_hero_level_up)\n");

    // OnSpellGround
    output.push_str("        elseif sub_tag == glass_TAG_Subscription_OnSpellGround then\n");
    output.push_str("            set glass_sub_on_spell_ground = glass_Subscription_OnSpellGround_handler[sub_id]\n");
    output.push_str("            set t = CreateTrigger()\n");
    output.push_str("            set i = 0\n");
    output.push_str("            loop\n");
    output.push_str("                exitwhen i >= bj_MAX_PLAYER_SLOTS\n");
    output.push_str("                call TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_SPELL_EFFECT, null)\n");
    output.push_str("                set i = i + 1\n");
    output.push_str("            endloop\n");
    output.push_str("            call TriggerAddAction(t, function glass_sub_cb_on_spell_ground)\n");

    // More subscription types added as PW needs them

    output.push_str("        endif\n");
    output.push_str("        set current = glass_List_integer_tail[current]\n");
    output.push_str("    endloop\n");
    output.push_str("    set t = null\n");
    output.push_str("endfunction\n\n");
}
```

- [ ] **Step 3: Wire into runtime init**

Modify `gen_elm_runtime_functions` to call the new functions, and update `glass_runtime_init` to call `glass_register_subscriptions` when subscriptions are detected:

In `gen_elm_runtime_functions`, add calls:
```rust
pub fn gen_elm_runtime_functions(
    entry: &ElmEntryPoints,
    _lambdas: &[crate::closures::LambdaInfo],
    output: &mut String,
) {
    output.push_str("// ========== Glass Elm Runtime Functions ==========\n\n");
    gen_rt_tuple_helpers(output);
    gen_msg_dispatch(entry, output);
    if entry.has_subscriptions {
        gen_subscription_callbacks(entry, output);
    }
    gen_timer_callback(output);
    gen_exec_effect(output);
    gen_process_effects(output);
    gen_send_msg(output);
    if entry.has_subscriptions {
        gen_register_subscriptions_jass(output);
    }
    // runtime_init with subscription call
    gen_runtime_init_jass(entry, output);
}
```

Update `glass_runtime_init` to call subscriptions:
```jass
function glass_runtime_init takes nothing returns nothing
    local integer glass_result
    local integer glass_effects
    set glass_timer_ht = InitHashtable()
    set glass_result = glass_init()
    set glass_model = glass_rt_tuple_0(glass_result)
    set glass_effects = glass_rt_tuple_1(glass_result)
    call glass_process_effects(glass_effects)
    // NEW: register subscriptions
    call glass_register_subscriptions(glass_subscriptions(glass_model))
endfunction
```

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: All existing tests pass. The new `jass_subscriptions_basic` test also passes.

- [ ] **Step 5: Commit**

```bash
git add src/runtime.rs tests/jass_validity.rs
git commit -m "feat: implement JASS subscription runtime (port from Lua)"
```

---

## Task 3: Closure Dispatch Signatures for Subscriptions

The subscription callbacks call `glass_dispatch_2_unit_unit`, `glass_dispatch_3_unit_integer_unit`, etc. These dispatch functions are auto-generated by the closure system based on what lambda signatures exist in user code. We need to verify the codegen generates the right dispatch signatures, and fix if not.

**Files:**
- Modify: `src/codegen.rs` (if dispatch signatures don't match)
- Test: `tests/jass_validity.rs`

- [ ] **Step 1: Write a test with subscriptions that use various signatures**

```rust
#[test]
fn jass_subscription_dispatch_signatures() {
    let jass = compile_glass(
        r#"
import effect
import subscription

pub enum Msg {
    Death { victim: Unit, killer: Unit }
    Tick
    Spell { caster: Unit, id: Int, target: Unit }
    Damage { source: Unit, target: Unit, amount: Float }
    Chat { player: Int, text: String }
}

pub struct Model { x: Int }

pub fn init() -> (Model, List(effect.Effect(Msg))) {
    (Model { x: 0 }, [])
}

pub fn update(model: Model, msg: Msg) -> (Model, List(effect.Effect(Msg))) {
    (model, [])
}

pub fn subscriptions(model: Model) -> List(subscription.Subscription(Msg)) {
    [
        subscription.on_death(fn(v: Unit, k: Unit) { Msg::Death { victim: v, killer: k } }),
        subscription.on_timer(0.025, fn() { Msg::Tick }),
        subscription.on_spell_effect(fn(c: Unit, id: Int, t: Unit) { Msg::Spell { caster: c, id, target: t } }),
        subscription.on_damage(fn(s: Unit, t: Unit, a: Float) { Msg::Damage { source: s, target: t, amount: a } }),
        subscription.on_chat(fn(p: Int, t: String) { Msg::Chat { player: p, text: t } }),
    ]
}
"#,
    );
    // Verify dispatch functions exist in output
    assert!(jass.contains("glass_dispatch_2_unit_unit"), "missing dispatch for (unit, unit)");
    assert!(jass.contains("glass_dispatch_void"), "missing dispatch for void");
    assert!(jass.contains("glass_dispatch_3_unit_integer_unit"), "missing dispatch for (unit, int, unit)");
    validate_jass_with_natives(&jass, true);
}
```

- [ ] **Step 2: Run test, fix dispatch generation if needed**

Run: `cargo test jass_subscription_dispatch_signatures -- --nocapture`

The closure system generates dispatch functions based on lambda types it sees. The subscription handler lambdas should trigger generation of the correct signatures. If they don't, we'll need to modify the closure dispatch generation in `codegen.rs` to recognize subscription handler types.

- [ ] **Step 3: Commit**

```bash
git add src/codegen.rs tests/jass_validity.rs
git commit -m "feat: ensure closure dispatch signatures for subscription handlers"
```

---

## Task 4: PW Native SDK Extensions

Pudge Wars needs JASS natives not yet in the SDK. Since @external already works in user code, we can add them to the SDK for reuse.

**Files:**
- Modify: `sdk/wc3/unit.glass`
- Create: `sdk/wc3/player.glass`
- Create: `sdk/wc3/group.glass`
- Create: `sdk/wc3/hashtable.glass`
- Create: `sdk/wc3/sfx.glass` (if not exists)
- Create: `sdk/wc3/display.glass`

- [ ] **Step 1: Extend `sdk/wc3/unit.glass` with additional natives**

Add these missing unit natives:

```glass
@external("jass", "SetUnitScale")
pub fn set_scale(u: Unit, x: Float, y: Float, z: Float) -> Int

@external("jass", "GetOwningPlayer")
pub fn get_owner(u: Unit) -> Player

@external("jass", "IsUnitType")
pub fn is_type(u: Unit, unit_type: Int) -> Bool

@external("jass", "IsUnitInRangeXY")
pub fn in_range_xy(u: Unit, x: Float, y: Float, range: Float) -> Bool

@external("jass", "GetWidgetLife")
pub fn get_life(u: Unit) -> Float

@external("jass", "SetWidgetLife")
pub fn set_life(u: Unit, hp: Float) -> Int

@external("jass", "UnitDamageTarget")
pub fn damage_target(source: Unit, target: Unit, amount: Float, attack: Bool, ranged: Bool, attack_type: Int, damage_type: Int, weapon_type: Int) -> Bool

@external("jass", "SetUnitFacing")
pub fn set_facing(u: Unit, angle: Float) -> Int

@external("jass", "ReviveHero")
pub fn revive_hero(u: Unit, x: Float, y: Float, eye_candy: Bool) -> Bool

@external("jass", "GetHeroLevel")
pub fn get_hero_level(u: Unit) -> Int

@external("jass", "SetHeroLevel")
pub fn set_hero_level(u: Unit, level: Int, show: Bool) -> Int

@external("jass", "UnitApplyTimedLife")
pub fn apply_timed_life(u: Unit, buff_id: Int, duration: Float) -> Int

@external("jass", "PauseUnit")
pub fn pause(u: Unit, flag: Bool) -> Int

@external("jass", "ShowUnit")
pub fn show(u: Unit, flag: Bool) -> Int

@external("jass", "SetUnitInvulnerable")
pub fn set_invulnerable(u: Unit, flag: Bool) -> Int

@external("jass", "GetUnitState")
pub fn get_state(u: Unit, state: Int) -> Float

@external("jass", "SetUnitState")
pub fn set_state(u: Unit, state: Int, value: Float) -> Int

@external("jass", "IssueImmediateOrder")
pub fn order_immediate(u: Unit, order: String) -> Bool

@external("jass", "IssuePointOrder")
pub fn order_point(u: Unit, order: String, x: Float, y: Float) -> Bool

@external("jass", "GetUnitAbilityLevel")
pub fn get_ability_level(u: Unit, ability_id: Int) -> Int

@external("jass", "SetUnitAbilityLevel")
pub fn set_ability_level(u: Unit, ability_id: Int, level: Int) -> Int
```

- [ ] **Step 2: Create `sdk/wc3/player.glass`**

```glass
@external("jass", "Player")
pub fn player(index: Int) -> Player

@external("jass", "GetPlayerId")
pub fn get_id(p: Player) -> Int

@external("jass", "GetLocalPlayer")
pub fn local_player() -> Player

@external("jass", "GetPlayerSlotState")
pub fn get_slot_state(p: Player) -> Int

@external("jass", "SetPlayerState")
pub fn set_state(p: Player, state: Int, value: Int) -> Int

@external("jass", "GetPlayerState")
pub fn get_state(p: Player, state: Int) -> Int

@external("jass", "SetPlayerHandicapXP")
pub fn set_handicap_xp(p: Player, handicap: Float) -> Int

@external("jass", "ForceAddPlayer")
pub fn force_add(f: Force, p: Player) -> Int

@external("jass", "IsPlayerInForce")
pub fn is_in_force(p: Player, f: Force) -> Bool
```

- [ ] **Step 3: Create `sdk/wc3/group.glass`**

```glass
@external("jass", "CreateGroup")
pub fn create() -> Group

@external("jass", "DestroyGroup")
pub fn destroy(g: Group) -> Int

@external("jass", "GroupEnumUnitsInRange")
pub fn enum_in_range(g: Group, x: Float, y: Float, radius: Float, filter: Int) -> Int

@external("jass", "FirstOfGroup")
pub fn first(g: Group) -> Unit

@external("jass", "GroupRemoveUnit")
pub fn remove_unit(g: Group, u: Unit) -> Int

@external("jass", "GroupAddUnit")
pub fn add_unit(g: Group, u: Unit) -> Int

@external("jass", "IsUnitInGroup")
pub fn has_unit(u: Unit, g: Group) -> Bool
```

- [ ] **Step 4: Create `sdk/wc3/display.glass`**

```glass
@external("jass", "DisplayTimedTextToPlayer")
pub fn timed_text(p: Player, x: Float, y: Float, duration: Float, message: String) -> Int

@external("jass", "CreateTextTag")
pub fn create_text_tag() -> TextTag

@external("jass", "SetTextTagText")
pub fn set_text_tag_text(tt: TextTag, text: String, size: Float) -> Int

@external("jass", "SetTextTagPos")
pub fn set_text_tag_pos(tt: TextTag, x: Float, y: Float, z: Float) -> Int

@external("jass", "SetTextTagColor")
pub fn set_text_tag_color(tt: TextTag, r: Int, g: Int, b: Int, a: Int) -> Int

@external("jass", "SetTextTagVelocity")
pub fn set_text_tag_velocity(tt: TextTag, xvel: Float, yvel: Float) -> Int

@external("jass", "SetTextTagLifespan")
pub fn set_text_tag_lifespan(tt: TextTag, lifespan: Float) -> Int

@external("jass", "SetTextTagPermanent")
pub fn set_text_tag_permanent(tt: TextTag, flag: Bool) -> Int

@external("jass", "DestroyTextTag")
pub fn destroy_text_tag(tt: TextTag) -> Int
```

- [ ] **Step 5: Compile a smoke test to verify all new externals**

Create quick inline test:
```rust
#[test]
fn pw_sdk_extensions_compile() {
    compile_and_validate_with_natives(
        r#"
import wc3/unit
import wc3/player
import wc3/math

fn test_player() -> Int {
    let p = player.player(0)
    player.get_id(p)
}
"#,
    );
}
```

- [ ] **Step 6: Commit**

```bash
git add sdk/wc3/unit.glass sdk/wc3/player.glass sdk/wc3/group.glass sdk/wc3/display.glass tests/jass_validity.rs
git commit -m "feat: add PW SDK extensions — player, group, display, unit extras"
```

---

## Task 5: PW Data Types

Define the core Pudge Wars data model in Glass.

**Files:**
- Create: `examples/pudge_wars/types.glass`
- Create: `examples/pudge_wars/arena.glass`

- [ ] **Step 1: Create `examples/pudge_wars/types.glass`**

```glass
import option

pub enum Team {
    West
    East
}

pub enum GameMode {
    KillMode { target: Int }
    RoundMode { target: Int }
    TimedMode { minutes: Int }
}

pub enum Phase {
    WaitingForMode
    Playing
    Victory { winner: Team }
}

pub enum HookState {
    Idle
    Launching
    Retracting
    RetractingWithTarget
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
    damage: Float,
    radius: Float,
    owner_index: Int,
}

pub struct PlayerData {
    index: Int,
    team: Team,
    alive: Bool,
    kills: Int,
    deaths: Int,
    spree: Int,
    hook_speed: Int,
    hook_damage: Int,
    hook_range: Int,
    hook_radius: Int,
    respawn_timer: Int,
    gold: Int,
}

pub enum Msg {
    SpellCast { caster: Unit, spell_id: Int, target: Unit }
    SpellGround { caster: Unit, spell_id: Int, x: Float, y: Float }
    HookTick
    UnitDied { victim: Unit, killer: Unit }
    ChatCommand { player_id: Int, text: String }
    PlayerLeft { player_id: Int }
    HeroLevelUp { hero: Unit }
    DamageTaken { source: Unit, target: Unit, amount: Float }
}

pub struct Model {
    phase: Phase,
    mode: option.Option(GameMode),
    players: List(PlayerData),
    hooks: List(HookData),
    tick_count: Int,
}
```

- [ ] **Step 2: Create `examples/pudge_wars/arena.glass`**

```glass
fn arena_min_x() -> Float { 736.0 }
fn arena_max_x() -> Float { 3360.0 }
fn arena_min_y() -> Float { 352.0 }
fn arena_max_y() -> Float { 3744.0 }

fn river_min_x() -> Float { 1792.0 }
fn river_max_x() -> Float { 2304.0 }
fn center_x() -> Float { 2048.0 }
fn center_y() -> Float { 2048.0 }
fn center_deflect_radius() -> Float { 266.0 }

fn chain_link_spacing() -> Float { 27.0 }
fn hook_tick_interval() -> Float { 0.025 }

fn west_spawn_x() -> Float { 1024.0 }
fn west_spawn_y() -> Float { 2048.0 }
fn east_spawn_x() -> Float { 3072.0 }
fn east_spawn_y() -> Float { 2048.0 }

pub fn is_in_bounds(x: Float, y: Float) -> Bool {
    x > arena_min_x() && x < arena_max_x() && y > arena_min_y() && y < arena_max_y()
}

pub fn is_west_side(x: Float) -> Bool {
    x < river_min_x()
}

pub fn is_east_side(x: Float) -> Bool {
    x > river_max_x()
}
```

- [ ] **Step 3: Verify types compile**

Run: `cargo run -- examples/pudge_wars/types.glass --no-check > /dev/null`
Expected: PASS (just functions/types, no entry point needed)

- [ ] **Step 4: Commit**

```bash
git add examples/pudge_wars/types.glass examples/pudge_wars/arena.glass
git commit -m "feat: add Pudge Wars data types and arena constants"
```

---

## Task 6: PW Main — Init, Update, Subscriptions

Wire up the Elm architecture for Pudge Wars.

**Files:**
- Create: `examples/pudge_wars/main.glass`

- [ ] **Step 1: Create `examples/pudge_wars/main.glass`**

```glass
import effect
import subscription
import option
import pudge_wars/types
import pudge_wars/arena

fn initial_player(index: Int, team: types.Team) -> types.PlayerData {
    types.PlayerData {
        index,
        team,
        alive: True,
        kills: 0,
        deaths: 0,
        spree: 0,
        hook_speed: 250,
        hook_damage: 100,
        hook_range: 1500,
        hook_radius: 100,
        respawn_timer: 0,
        gold: 100,
    }
}

fn make_players() -> List(types.PlayerData) {
    [
        initial_player(0, types.Team::West),
        initial_player(1, types.Team::West),
        initial_player(2, types.Team::West),
        initial_player(3, types.Team::West),
        initial_player(4, types.Team::West),
        initial_player(6, types.Team::East),
        initial_player(7, types.Team::East),
        initial_player(8, types.Team::East),
        initial_player(9, types.Team::East),
        initial_player(10, types.Team::East),
    ]
}

pub fn init() -> (types.Model, List(effect.Effect(types.Msg))) {
    let model = types.Model {
        phase: types.Phase::WaitingForMode,
        mode: option.Option::None,
        players: make_players(),
        hooks: [],
        tick_count: 0,
    }
    (model, [])
}

pub fn update(model: types.Model, msg: types.Msg) -> (types.Model, List(effect.Effect(types.Msg))) {
    case msg {
        SpellCast(caster, spell_id, target) -> (model, [])
        SpellGround(caster, spell_id, x, y) -> (model, [])
        HookTick -> (model, [])
        UnitDied(victim, killer) -> (model, [])
        ChatCommand(player_id, text) -> (model, [])
        PlayerLeft(player_id) -> (model, [])
        HeroLevelUp(hero) -> (model, [])
        DamageTaken(source, target, amount) -> (model, [])
    }
}

pub fn subscriptions(model: types.Model) -> List(subscription.Subscription(types.Msg)) {
    [
        subscription.on_spell_effect(fn(caster: Unit, spell_id: Int, target: Unit) {
            types.Msg::SpellCast { caster, spell_id, target }
        }),
        subscription.on_death(fn(victim: Unit, killer: Unit) {
            types.Msg::UnitDied { victim, killer }
        }),
        subscription.on_timer(arena.hook_tick_interval(), fn() {
            types.Msg::HookTick
        }),
        subscription.on_chat(fn(player_id: Int, text: String) {
            types.Msg::ChatCommand { player_id, text }
        }),
        subscription.on_player_leave(fn(player_id: Int) {
            types.Msg::PlayerLeft { player_id }
        }),
        subscription.on_hero_level_up(fn(hero: Unit) {
            types.Msg::HeroLevelUp { hero }
        }),
        subscription.on_damage(fn(source: Unit, target: Unit, amount: Float) {
            types.Msg::DamageTaken { source, target, amount }
        }),
    ]
}
```

- [ ] **Step 2: Compile to JASS and Lua**

Run:
```bash
cargo run -- examples/pudge_wars/main.glass --no-mangle --no-strip > /tmp/pw_jass.j 2>/tmp/pw_err.txt
cargo run -- examples/pudge_wars/main.glass --target lua --no-mangle --no-strip > /tmp/pw_lua.lua 2>/tmp/pw_err_lua.txt
```

Expected: Both compile successfully. Fix any compiler errors encountered.

- [ ] **Step 3: Validate JASS output with pjass**

Run: `tools/pjass tests/common_stub.j /tmp/pw_jass.j`
Expected: PASS

- [ ] **Step 4: Validate Lua output with luac**

Run: `luac -p /tmp/pw_lua.lua`
Expected: PASS

- [ ] **Step 5: Add to test suite**

In `tests/jass_validity.rs`, add:
```rust
#[case("pudge_wars/main.glass")]
```
to the `example_compiles` rstest list.

Similarly in `tests/lua_validity.rs`.

- [ ] **Step 6: Commit**

```bash
git add examples/pudge_wars/ tests/jass_validity.rs tests/lua_validity.rs
git commit -m "feat: Pudge Wars Phase 1 skeleton — init, update, subscriptions"
```

---

## Task 7: Fix Compiler Issues

This task is a catch-all for compiler bugs discovered while implementing Tasks 1-6. Common expected issues:

1. **Subscription SoA field names** — The runtime assumes names like `glass_Subscription_OnDeath_handler` but codegen might generate different names. Fix SoA naming to match.

2. **Closure dispatch signature mismatch** — Callbacks need `glass_dispatch_2_unit_unit` etc. If the closure system doesn't generate these, modify closure dispatch generation in `codegen.rs`.

3. **Qualified import issues** — PW uses `types.Msg::SpellCast` style cross-module constructors. Verify this works and fix parser/resolver if needed.

4. **Linearity warnings** — Subscription handlers receive Unit handles that may not be consumed. May need to add `clone` or handle cleanup.

5. **`hook_tick_interval()` as subscription argument** — The subscription's `interval` field expects a Float literal or expression. Verify function calls work in subscription construction.

**Files:**
- Potentially: `src/codegen.rs`, `src/infer.rs`, `src/runtime.rs`, `src/linearity.rs`

- [ ] **Step 1: Collect all compilation errors from Task 6**
- [ ] **Step 2: For each error, write a minimal reproducing test**
- [ ] **Step 3: Fix each error**
- [ ] **Step 4: Run full test suite**

Run: `cargo test`
Expected: ALL tests pass

- [ ] **Step 5: Commit each fix separately**

---

## Verification

After all tasks complete:

- [ ] `cargo test` passes all tests
- [ ] `cargo run -- examples/pudge_wars/main.glass --no-mangle` produces valid JASS
- [ ] `cargo run -- examples/pudge_wars/main.glass --target lua --no-mangle` produces valid Lua
- [ ] JASS output contains `glass_register_subscriptions` function
- [ ] JASS output contains named callback functions (`glass_sub_cb_on_death`, etc.)
- [ ] JASS output contains `glass_runtime_init` that calls subscriptions
- [ ] Both targets produce feature-equivalent output

## What's Next (Phase 2)

Phase 2 will implement hook physics:
- Hook launching (create tip unit, set trajectory)
- Hook tick (move tip, spawn chain links, wall bouncing, structure deflection)
- Hook collision (enemy detection, headshot)
- Hook retraction (drag target or return empty)
- Effects for unit creation/movement at 40 Hz

This will likely require:
- New Effect variants (or confirming existing ones suffice)
- `CreateUnitCallback` for creating hook tip units
- Investigating performance of SoA-based hook state vs current approach
