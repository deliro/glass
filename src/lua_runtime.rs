// Elm Architecture runtime code generation for Lua.
//
// The Lua runtime is simpler than JASS because:
// - Functions are first-class (no closure dispatch tables)
// - Tables replace SoA arrays (no tuple extraction functions)
// - Local variables in any scope (no forward declaration issues)

use crate::runtime::ElmEntryPoints;

/// Generate the Elm runtime Lua code (after user functions).
pub fn gen_lua_elm_runtime(entry: &ElmEntryPoints, output: &mut String) {
    output.push_str("-- ========== Glass Elm Runtime ==========\n\n");

    // Global model state
    output.push_str("local glass_model = nil\n");
    output.push_str("local glass_timer_ht = nil\n\n");

    // Effect executor
    gen_exec_effect(output);

    // Process effects (walk a linked list of effects)
    gen_process_effects(output);

    // Send message
    gen_send_msg(output);

    // Subscription registration
    if entry.has_subscriptions {
        gen_register_subscriptions(output);
    }

    // Runtime init
    gen_runtime_init(entry, output);

    // Map init trigger
    gen_map_init(output);
}

fn gen_exec_effect(output: &mut String) {
    output.push_str("function glass_exec_effect(fx)\n");

    // After
    output.push_str("    if fx.tag == glass_TAG_After then\n");
    output.push_str("        local t = CreateTimer()\n");
    output.push_str("        local cb = fx.callback\n");
    output.push_str("        TimerStart(t, fx.duration, false, function()\n");
    output.push_str("            local expired = GetExpiredTimer()\n");
    output.push_str("            DestroyTimer(expired)\n");
    output.push_str("            local msg = cb()\n");
    output.push_str("            glass_send_msg(msg)\n");
    output.push_str("        end)\n");

    // DisplayText
    output.push_str("    elseif fx.tag == glass_TAG_DisplayText then\n");
    output.push_str(
        "        DisplayTimedTextToPlayer(Player(fx.player_id), 0, 0, fx.duration, fx.text)\n",
    );

    // DamageUnit
    output.push_str("    elseif fx.tag == glass_TAG_DamageUnit then\n");
    output.push_str("        UnitDamageTarget(glass_handle_lookup_unit(fx.source_id), glass_handle_lookup_unit(fx.target_id), fx.amount, true, false, fx.attack_type, fx.damage_type, 0)\n");

    // CreateUnit
    output.push_str("    elseif fx.tag == glass_TAG_CreateUnit then\n");
    output.push_str("        CreateUnit(Player(fx.owner), fx.type_id, fx.x, fx.y, fx.facing)\n");

    // RemoveUnit
    output.push_str("    elseif fx.tag == glass_TAG_RemoveUnit then\n");
    output.push_str("        RemoveUnit(glass_handle_lookup_unit(fx.unit_id))\n");

    // MoveUnit
    output.push_str("    elseif fx.tag == glass_TAG_MoveUnit then\n");
    output.push_str("        SetUnitPosition(glass_handle_lookup_unit(fx.unit_id), fx.x, fx.y)\n");

    // PlayAnimation
    output.push_str("    elseif fx.tag == glass_TAG_PlayAnimation then\n");
    output.push_str("        SetUnitAnimation(glass_handle_lookup_unit(fx.unit_id), fx.anim)\n");

    // AddAbility
    output.push_str("    elseif fx.tag == glass_TAG_AddAbility then\n");
    output
        .push_str("        UnitAddAbility(glass_handle_lookup_unit(fx.unit_id), fx.ability_id)\n");

    // AddSfx
    output.push_str("    elseif fx.tag == glass_TAG_AddSfx then\n");
    output.push_str("        DestroyEffect(AddSpecialEffect(fx.model, fx.x, fx.y))\n");

    // SetUnitHp
    output.push_str("    elseif fx.tag == glass_TAG_SetUnitHp then\n");
    output.push_str(
        "        SetUnitState(glass_handle_lookup_unit(fx.unit_id), UNIT_STATE_LIFE, fx.hp)\n",
    );

    // SetUnitMana
    output.push_str("    elseif fx.tag == glass_TAG_SetUnitMana then\n");
    output.push_str(
        "        SetUnitState(glass_handle_lookup_unit(fx.unit_id), UNIT_STATE_MANA, fx.mana)\n",
    );

    // PanCamera
    output.push_str("    elseif fx.tag == glass_TAG_PanCamera then\n");
    output.push_str("        if GetLocalPlayer() == Player(fx.player_id) then\n");
    output.push_str("            PanCameraTo(fx.x, fx.y)\n");
    output.push_str("        end\n");

    // ShowFloatingText
    output.push_str("    elseif fx.tag == glass_TAG_ShowFloatingText then\n");
    output.push_str("        local tt = CreateTextTag()\n");
    output.push_str("        SetTextTagText(tt, fx.text, fx.size)\n");
    output.push_str("        SetTextTagPos(tt, fx.x, fx.y, 0.0)\n");
    output.push_str("        SetTextTagLifespan(tt, 3.0)\n");
    output.push_str("        SetTextTagPermanent(tt, false)\n");
    output.push_str("        SetTextTagVelocity(tt, 0.0, 0.04)\n");

    // PlaySound
    output.push_str("    elseif fx.tag == glass_TAG_PlaySound then\n");
    output.push_str("        -- PlaySound not yet implemented\n");

    output.push_str("    end\n");
    output.push_str("end\n\n");
}

fn gen_process_effects(output: &mut String) {
    output.push_str("function glass_process_effects(effect_list)\n");
    output.push_str("    local current = effect_list\n");
    output.push_str("    while current ~= nil do\n");
    output.push_str("        glass_exec_effect(current.head)\n");
    output.push_str("        current = current.tail\n");
    output.push_str("    end\n");
    output.push_str("end\n\n");
}

fn gen_send_msg(output: &mut String) {
    output.push_str("function glass_send_msg(msg)\n");
    output.push_str("    local result = glass_update(glass_model, msg)\n");
    output.push_str("    glass_model = result[1]\n");
    output.push_str("    glass_process_effects(result[2])\n");
    output.push_str("end\n\n");
}

/// Generate subscription registration: walk a List(Subscription) and register
/// WC3 triggers that call glass_send_msg with messages from handler closures.
fn gen_register_subscriptions(output: &mut String) {
    output.push_str("function glass_register_subscriptions(subs)\n");
    output.push_str("    local current = subs\n");
    output.push_str("    while current ~= nil do\n");
    output.push_str("        local sub = current.head\n");

    // OnAttack: register EVENT_PLAYER_UNIT_ATTACKED for all players
    output.push_str("        if sub.tag == glass_TAG_OnAttack then\n");
    output.push_str("            local handler = sub.handler\n");
    output.push_str("            local t = CreateTrigger()\n");
    output.push_str("            for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("                TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_ATTACKED, nil)\n");
    output.push_str("            end\n");
    output.push_str("            TriggerAddAction(t, function()\n");
    output.push_str("                glass_send_msg(handler(GetHandleId(GetAttacker()), GetHandleId(GetTriggerUnit())))\n");
    output.push_str("            end)\n");

    // OnDeath: register EVENT_PLAYER_UNIT_DEATH for all players
    output.push_str("        elseif sub.tag == glass_TAG_OnDeath then\n");
    output.push_str("            local handler = sub.handler\n");
    output.push_str("            local t = CreateTrigger()\n");
    output.push_str("            for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("                TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_DEATH, nil)\n");
    output.push_str("            end\n");
    output.push_str("            TriggerAddAction(t, function()\n");
    output.push_str("                glass_send_msg(handler(GetHandleId(GetTriggerUnit()), GetHandleId(GetKillingUnit())))\n");
    output.push_str("            end)\n");

    // OnTimer: create a repeating timer
    output.push_str("        elseif sub.tag == glass_TAG_OnTimer then\n");
    output.push_str("            local handler = sub.handler\n");
    output.push_str("            local t = CreateTimer()\n");
    output.push_str("            TimerStart(t, sub.interval, true, function()\n");
    output.push_str("                glass_send_msg(handler())\n");
    output.push_str("            end)\n");

    // OnSpellEffect: register EVENT_PLAYER_UNIT_SPELL_EFFECT for all players
    output.push_str("        elseif sub.tag == glass_TAG_OnSpellEffect then\n");
    output.push_str("            local handler = sub.handler\n");
    output.push_str("            local t = CreateTrigger()\n");
    output.push_str("            for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("                TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_SPELL_EFFECT, nil)\n");
    output.push_str("            end\n");
    output.push_str("            TriggerAddAction(t, function()\n");
    output.push_str("                local target = GetSpellTargetUnit()\n");
    output.push_str("                local target_id = 0\n");
    output.push_str("                if target ~= nil then target_id = GetHandleId(target) end\n");
    output.push_str("                glass_send_msg(handler(GetHandleId(GetTriggerUnit()), GetSpellAbilityId(), target_id))\n");
    output.push_str("            end)\n");

    // OnItemPickup: register EVENT_PLAYER_UNIT_PICKUP_ITEM for all players
    output.push_str("        elseif sub.tag == glass_TAG_OnItemPickup then\n");
    output.push_str("            local handler = sub.handler\n");
    output.push_str("            local t = CreateTrigger()\n");
    output.push_str("            for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("                TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_PICKUP_ITEM, nil)\n");
    output.push_str("            end\n");
    output.push_str("            TriggerAddAction(t, function()\n");
    output.push_str("                glass_send_msg(handler(GetHandleId(GetTriggerUnit()), GetItemTypeId(GetManipulatedItem())))\n");
    output.push_str("            end)\n");

    // OnSpellCast
    output.push_str("        elseif sub.tag == glass_TAG_OnSpellCast then\n");
    output.push_str("            local handler = sub.handler\n");
    output.push_str("            local t = CreateTrigger()\n");
    output.push_str("            for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("                TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_SPELL_CAST, nil)\n");
    output.push_str("            end\n");
    output.push_str("            TriggerAddAction(t, function()\n");
    output.push_str("                glass_send_msg(handler(GetHandleId(GetTriggerUnit()), GetSpellAbilityId()))\n");
    output.push_str("            end)\n");

    // OnSpellChannel
    output.push_str("        elseif sub.tag == glass_TAG_OnSpellChannel then\n");
    output.push_str("            local handler = sub.handler\n");
    output.push_str("            local t = CreateTrigger()\n");
    output.push_str("            for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("                TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_SPELL_CHANNEL, nil)\n");
    output.push_str("            end\n");
    output.push_str("            TriggerAddAction(t, function()\n");
    output.push_str("                glass_send_msg(handler(GetHandleId(GetTriggerUnit()), GetSpellAbilityId()))\n");
    output.push_str("            end)\n");

    // OnDamage (uses EVENT_PLAYER_UNIT_DAMAGED)
    output.push_str("        elseif sub.tag == glass_TAG_OnDamage then\n");
    output.push_str("            local handler = sub.handler\n");
    output.push_str("            local t = CreateTrigger()\n");
    output.push_str("            for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("                TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_DAMAGED, nil)\n");
    output.push_str("            end\n");
    output.push_str("            TriggerAddAction(t, function()\n");
    output.push_str("                glass_send_msg(handler(GetHandleId(GetEventDamageSource()), GetHandleId(GetTriggerUnit()), R2I(GetEventDamage())))\n");
    output.push_str("            end)\n");

    // OnItemUse
    output.push_str("        elseif sub.tag == glass_TAG_OnItemUse then\n");
    output.push_str("            local handler = sub.handler\n");
    output.push_str("            local t = CreateTrigger()\n");
    output.push_str("            for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("                TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_USE_ITEM, nil)\n");
    output.push_str("            end\n");
    output.push_str("            TriggerAddAction(t, function()\n");
    output.push_str("                glass_send_msg(handler(GetHandleId(GetTriggerUnit()), GetItemTypeId(GetManipulatedItem())))\n");
    output.push_str("            end)\n");

    // OnItemDrop
    output.push_str("        elseif sub.tag == glass_TAG_OnItemDrop then\n");
    output.push_str("            local handler = sub.handler\n");
    output.push_str("            local t = CreateTrigger()\n");
    output.push_str("            for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("                TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_DROP_ITEM, nil)\n");
    output.push_str("            end\n");
    output.push_str("            TriggerAddAction(t, function()\n");
    output.push_str("                glass_send_msg(handler(GetHandleId(GetTriggerUnit()), GetItemTypeId(GetManipulatedItem())))\n");
    output.push_str("            end)\n");

    // OnChat
    output.push_str("        elseif sub.tag == glass_TAG_OnChat then\n");
    output.push_str("            local handler = sub.handler\n");
    output.push_str("            local t = CreateTrigger()\n");
    output.push_str("            for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("                TriggerRegisterPlayerChatEvent(t, Player(i), \"\", false)\n");
    output.push_str("            end\n");
    output.push_str("            TriggerAddAction(t, function()\n");
    output.push_str("                glass_send_msg(handler(GetPlayerId(GetTriggerPlayer()), GetEventPlayerChatString()))\n");
    output.push_str("            end)\n");

    // OnPlayerLeave
    output.push_str("        elseif sub.tag == glass_TAG_OnPlayerLeave then\n");
    output.push_str("            local handler = sub.handler\n");
    output.push_str("            local t = CreateTrigger()\n");
    output.push_str("            for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output
        .push_str("                TriggerRegisterPlayerEvent(t, Player(i), EVENT_PLAYER_LEAVE)\n");
    output.push_str("            end\n");
    output.push_str("            TriggerAddAction(t, function()\n");
    output.push_str("                glass_send_msg(handler(GetPlayerId(GetTriggerPlayer())))\n");
    output.push_str("            end)\n");

    // OnHeroLevelUp
    output.push_str("        elseif sub.tag == glass_TAG_OnHeroLevelUp then\n");
    output.push_str("            local handler = sub.handler\n");
    output.push_str("            local t = CreateTrigger()\n");
    output.push_str("            for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("                TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_HERO_LEVEL, nil)\n");
    output.push_str("            end\n");
    output.push_str("            TriggerAddAction(t, function()\n");
    output.push_str("                glass_send_msg(handler(GetHandleId(GetTriggerUnit())))\n");
    output.push_str("            end)\n");

    // OnConstructionFinish
    output.push_str("        elseif sub.tag == glass_TAG_OnConstructionFinish then\n");
    output.push_str("            local handler = sub.handler\n");
    output.push_str("            local t = CreateTrigger()\n");
    output.push_str("            for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("                TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_CONSTRUCT_FINISH, nil)\n");
    output.push_str("            end\n");
    output.push_str("            TriggerAddAction(t, function()\n");
    output.push_str("                glass_send_msg(handler(GetHandleId(GetTriggerUnit())))\n");
    output.push_str("            end)\n");

    output.push_str("        end\n");
    output.push_str("        current = current.tail\n");
    output.push_str("    end\n");
    output.push_str("end\n\n");
}

fn gen_runtime_init(entry: &ElmEntryPoints, output: &mut String) {
    output.push_str("function glass_runtime_init()\n");
    output.push_str("    glass_timer_ht = InitHashtable()\n");
    output.push_str("    local result = glass_init()\n");
    output.push_str("    glass_model = result[1]\n");
    output.push_str("    glass_process_effects(result[2])\n");
    if entry.has_subscriptions {
        output.push_str("    local subs = glass_subscriptions(glass_model)\n");
        output.push_str("    glass_register_subscriptions(subs)\n");
    }
    output.push_str("end\n\n");
}

fn gen_map_init(output: &mut String) {
    output.push_str("do\n");
    output.push_str("    local t = CreateTrigger()\n");
    output.push_str("    TriggerRegisterTimerEvent(t, 0.00, false)\n");
    output.push_str("    TriggerAddAction(t, glass_runtime_init)\n");
    output.push_str("end\n");
}
