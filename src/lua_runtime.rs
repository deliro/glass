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

    output.push_str("local glass_model = nil\n");
    output.push_str("local glass_timer_ht = nil\n");
    output.push_str("local glass_multiboard = nil\n");
    if entry.has_subscriptions {
        output.push_str("local glass_active_subs = {}\n");
    }
    output.push('\n');

    // Effect executor
    gen_exec_effect(output);

    // Process effects (walk a linked list of effects)
    gen_process_effects(output);

    gen_send_msg(entry, output);

    if entry.has_subscriptions {
        gen_register_one_sub(output);
        gen_unregister_one_sub(output);
        gen_reconcile_subscriptions(output);
    }

    // Runtime init
    gen_runtime_init(entry, output);

    // Map init trigger
    gen_map_init(output);
}

fn gen_exec_effect(output: &mut String) {
    output.push_str("function glass_exec_effect(fx)\n");

    // After
    output.push_str("    if fx.tag == glass_TAG_Effect_After then\n");
    output.push_str("        local trig = CreateTrigger()\n");
    output.push_str("        local cb = fx.callback\n");
    output.push_str("        TriggerRegisterTimerEvent(trig, fx.duration, false)\n");
    output.push_str("        TriggerAddAction(trig, function()\n");
    output.push_str("            local msg = cb()\n");
    output.push_str("            glass_send_msg(msg)\n");
    output.push_str("        end)\n");

    // DisplayText
    output.push_str("    elseif fx.tag == glass_TAG_Effect_DisplayText then\n");
    output.push_str(
        "        DisplayTimedTextToPlayer(Player(fx.player_id), 0, 0, fx.duration, fx.text)\n",
    );

    // DamageUnit
    output.push_str("    elseif fx.tag == glass_TAG_Effect_DamageUnit then\n");
    output.push_str("        UnitDamageTarget(fx.source, fx.target, fx.amount, true, false, ConvertAttackType(fx.attack_type), ConvertDamageType(fx.damage_type), WEAPON_TYPE_WHOKNOWS)\n");

    output.push_str("    elseif fx.tag == glass_TAG_Effect_CreateUnit then\n");
    output.push_str(
        "        local u = CreateUnit(Player(fx.owner), fx.type_id, fx.x, fx.y, fx.facing)\n",
    );
    output.push_str("        u = nil\n");

    // RemoveUnit
    output.push_str("    elseif fx.tag == glass_TAG_Effect_RemoveUnit then\n");
    output.push_str("        RemoveUnit(fx.unit)\n");

    // MoveUnit
    output.push_str("    elseif fx.tag == glass_TAG_Effect_MoveUnit then\n");
    output.push_str("        SetUnitPosition(fx.unit, fx.x, fx.y)\n");

    // PlayAnimation
    output.push_str("    elseif fx.tag == glass_TAG_Effect_PlayAnimation then\n");
    output.push_str("        SetUnitAnimation(fx.unit, fx.anim)\n");

    // AddAbility
    output.push_str("    elseif fx.tag == glass_TAG_Effect_AddAbility then\n");
    output.push_str("        UnitAddAbility(fx.unit, fx.ability_id)\n");

    // AddSfx
    output.push_str("    elseif fx.tag == glass_TAG_Effect_AddSfx then\n");
    output.push_str("        DestroyEffect(AddSpecialEffect(fx.model, fx.x, fx.y))\n");

    // SetUnitHp
    output.push_str("    elseif fx.tag == glass_TAG_Effect_SetUnitHp then\n");
    output.push_str("        SetUnitState(fx.unit, UNIT_STATE_LIFE, fx.hp)\n");

    // SetUnitMana
    output.push_str("    elseif fx.tag == glass_TAG_Effect_SetUnitMana then\n");
    output.push_str("        SetUnitState(fx.unit, UNIT_STATE_MANA, fx.mana)\n");

    // PanCamera
    output.push_str("    elseif fx.tag == glass_TAG_Effect_PanCamera then\n");
    output.push_str("        if GetLocalPlayer() == Player(fx.player_id) then\n");
    output.push_str("            PanCameraTo(fx.x, fx.y)\n");
    output.push_str("        end\n");

    // ShowFloatingText
    output.push_str("    elseif fx.tag == glass_TAG_Effect_ShowFloatingText then\n");
    output.push_str("        local tt = CreateTextTag()\n");
    output.push_str("        SetTextTagText(tt, fx.text, fx.size)\n");
    output.push_str("        SetTextTagPos(tt, fx.x, fx.y, 0.0)\n");
    output.push_str("        SetTextTagLifespan(tt, 3.0)\n");
    output.push_str("        SetTextTagPermanent(tt, false)\n");
    output.push_str("        SetTextTagVelocity(tt, 0.0, 0.04)\n");

    output.push_str("    elseif fx.tag == glass_TAG_Effect_PlaySound then\n");
    output.push_str(
        "        local snd = CreateSound(fx.path, false, false, false, 10, 10, \"DefaultEAXON\")\n",
    );
    output.push_str("        SetSoundVolume(snd, 127)\n");
    output.push_str("        StartSound(snd)\n");
    output.push_str("        KillSoundWhenDone(snd)\n");

    output.push_str("    elseif fx.tag == glass_TAG_Effect_FindNearestEnemy then\n");
    output.push_str("        local g = CreateGroup()\n");
    output.push_str("        GroupEnumUnitsInRange(g, fx.x, fx.y, fx.radius, nil)\n");
    output.push_str("        local best = FirstOfGroup(g)\n");
    output.push_str("        DestroyGroup(g)\n");
    output.push_str("        if best ~= nil then\n");
    output.push_str("            local msg = fx.callback(best)\n");
    output.push_str("            glass_send_msg(msg)\n");
    output.push_str("        end\n");

    output.push_str("    elseif fx.tag == glass_TAG_Effect_CreateUnitCallback then\n");
    output.push_str(
        "        local u = CreateUnit(Player(fx.owner), fx.type_id, fx.x, fx.y, fx.facing)\n",
    );
    output.push_str("        local msg = fx.callback(u)\n");
    output.push_str("        glass_send_msg(msg)\n");

    output.push_str("    elseif fx.tag == glass_TAG_Effect_GiveGold then\n");
    output.push_str("        SetPlayerState(Player(fx.player_id), PLAYER_STATE_RESOURCE_GOLD, GetPlayerState(Player(fx.player_id), PLAYER_STATE_RESOURCE_GOLD) + fx.gold_amount)\n");

    output.push_str("    elseif fx.tag == glass_TAG_Effect_GiveLumber then\n");
    output.push_str("        SetPlayerState(Player(fx.player_id), PLAYER_STATE_RESOURCE_LUMBER, GetPlayerState(Player(fx.player_id), PLAYER_STATE_RESOURCE_LUMBER) + fx.lumber_amount)\n");

    output.push_str("    elseif fx.tag == glass_TAG_Effect_KillUnit then\n");
    output.push_str("        KillUnit(fx.unit)\n");

    output.push_str("    elseif fx.tag == glass_TAG_Effect_RemoveAbility then\n");
    output.push_str("        UnitRemoveAbility(fx.unit, fx.ability_id)\n");

    output.push_str("    elseif fx.tag == glass_TAG_Effect_SetUnitOwner then\n");
    output.push_str("        SetUnitOwner(fx.unit, Player(fx.player_id), true)\n");

    output.push_str("    elseif fx.tag == glass_TAG_Effect_PauseUnit then\n");
    output.push_str("        PauseUnit(fx.unit, fx.paused)\n");

    output.push_str("    elseif fx.tag == glass_TAG_Effect_ShowUnit then\n");
    output.push_str("        ShowUnit(fx.unit, fx.shown)\n");

    output.push_str("    elseif fx.tag == glass_TAG_Effect_SetInvulnerable then\n");
    output.push_str("        SetUnitInvulnerable(fx.unit, fx.invulnerable)\n");

    output.push_str("    elseif fx.tag == glass_TAG_Effect_IssueOrder then\n");
    output.push_str("        IssueImmediateOrder(fx.unit, fx.order)\n");

    output.push_str("    elseif fx.tag == glass_TAG_Effect_IssuePointOrder then\n");
    output.push_str("        IssuePointOrder(fx.unit, fx.order, fx.x, fx.y)\n");

    output.push_str("    elseif fx.tag == glass_TAG_Effect_IssueTargetOrder then\n");
    output.push_str("        IssueTargetOrder(fx.unit, fx.order, fx.target)\n");

    output.push_str("    elseif fx.tag == glass_TAG_Effect_AddSfxTarget then\n");
    output.push_str(
        "        DestroyEffect(AddSpecialEffectTarget(fx.model, fx.unit, fx.attach_point))\n",
    );

    output.push_str("    elseif fx.tag == glass_TAG_Effect_ReviveHero then\n");
    output.push_str("        ReviveHero(fx.unit, fx.x, fx.y, true)\n");

    output.push_str("    elseif fx.tag == glass_TAG_Effect_AddHeroXp then\n");
    output.push_str("        AddHeroXP(fx.unit, fx.xp, true)\n");

    output.push_str("    elseif fx.tag == glass_TAG_Effect_SetUnitFacing then\n");
    output.push_str("        SetUnitFacing(fx.unit, fx.facing)\n");

    output.push_str("    elseif fx.tag == glass_TAG_Effect_PingMinimap then\n");
    output.push_str("        PingMinimap(fx.x, fx.y, fx.duration)\n");

    output.push_str("    elseif fx.tag == glass_TAG_Effect_CreateItem then\n");
    output.push_str("        CreateItem(fx.item_type_id, fx.x, fx.y)\n");

    output.push_str("    elseif fx.tag == glass_TAG_Effect_SetUnitMoveSpeed then\n");
    output.push_str("        SetUnitMoveSpeed(fx.unit, fx.speed)\n");

    output.push_str("    elseif fx.tag == glass_TAG_Effect_ForUnitsInRange then\n");
    output.push_str("        local g = CreateGroup()\n");
    output.push_str("        GroupEnumUnitsInRange(g, fx.x, fx.y, fx.radius, nil)\n");
    output.push_str("        local u = FirstOfGroup(g)\n");
    output.push_str("        while u ~= nil do\n");
    output.push_str("            local msg = fx.callback(u)\n");
    output.push_str("            glass_send_msg(msg)\n");
    output.push_str("            GroupRemoveUnit(g, u)\n");
    output.push_str("            u = FirstOfGroup(g)\n");
    output.push_str("        end\n");
    output.push_str("        DestroyGroup(g)\n");

    output.push_str("    elseif fx.tag == glass_TAG_Effect_UpdateBoard then\n");
    output.push_str("        local row_count = 0\n");
    output.push_str("        local cur = fx.rows\n");
    output.push_str("        while cur ~= nil do row_count = row_count + 1; cur = cur.tail end\n");
    output.push_str("        if glass_multiboard == nil then\n");
    output.push_str("            glass_multiboard = CreateMultiboard()\n");
    output.push_str("        end\n");
    output.push_str("        MultiboardSetColumnCount(glass_multiboard, 2)\n");
    output.push_str("        MultiboardSetRowCount(glass_multiboard, row_count)\n");
    output.push_str("        MultiboardSetTitleText(glass_multiboard, fx.title)\n");
    output.push_str("        MultiboardDisplay(glass_multiboard, true)\n");
    output.push_str("        cur = fx.rows\n");
    output.push_str("        local ri = 0\n");
    output.push_str("        while cur ~= nil do\n");
    output.push_str("            local r = cur.head\n");
    output.push_str("            local mbi0 = MultiboardGetItem(glass_multiboard, ri, 0)\n");
    output.push_str("            MultiboardSetItemValue(mbi0, r.label)\n");
    output.push_str("            MultiboardSetItemWidth(mbi0, 0.10)\n");
    output.push_str("            MultiboardReleaseItem(mbi0)\n");
    output.push_str("            local mbi1 = MultiboardGetItem(glass_multiboard, ri, 1)\n");
    output.push_str("            MultiboardSetItemValue(mbi1, r.value)\n");
    output.push_str("            MultiboardSetItemWidth(mbi1, 0.08)\n");
    output.push_str("            MultiboardReleaseItem(mbi1)\n");
    output.push_str("            ri = ri + 1\n");
    output.push_str("            cur = cur.tail\n");
    output.push_str("        end\n");

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

fn gen_send_msg(entry: &ElmEntryPoints, output: &mut String) {
    output.push_str("function glass_send_msg(msg)\n");
    output.push_str("    local result = glass_update(glass_model, msg)\n");
    output.push_str("    glass_model = result[1]\n");
    output.push_str("    glass_process_effects(result[2])\n");
    if entry.has_subscriptions {
        output.push_str("    glass_reconcile_subs(glass_subscriptions(glass_model))\n");
    }
    output.push_str("end\n\n");
}

fn gen_register_one_sub(output: &mut String) {
    output.push_str("function glass_register_one_sub(sub, key)\n");
    output.push_str("    local handler = sub.handler\n");

    output.push_str("    if sub.tag == glass_TAG_Subscription_OnAttack then\n");
    output.push_str("        local t = CreateTrigger()\n");
    output.push_str("        for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("            TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_ATTACKED, nil)\n");
    output.push_str("        end\n");
    output.push_str("        TriggerAddAction(t, function()\n");
    output.push_str("            glass_send_msg(handler(GetAttacker(), GetTriggerUnit()))\n");
    output.push_str("        end)\n");
    output.push_str("        glass_active_subs[key] = { handle = t, kind = \"trigger\" }\n");

    output.push_str("    elseif sub.tag == glass_TAG_Subscription_OnDeath then\n");
    output.push_str("        local t = CreateTrigger()\n");
    output.push_str("        for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str(
        "            TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_DEATH, nil)\n",
    );
    output.push_str("        end\n");
    output.push_str("        TriggerAddAction(t, function()\n");
    output.push_str("            glass_send_msg(handler(GetTriggerUnit(), GetKillingUnit()))\n");
    output.push_str("        end)\n");
    output.push_str("        glass_active_subs[key] = { handle = t, kind = \"trigger\" }\n");

    output.push_str("    elseif sub.tag == glass_TAG_Subscription_OnTimer then\n");
    output.push_str("        local t = CreateTimer()\n");
    output.push_str("        TimerStart(t, sub.interval, true, function()\n");
    output.push_str("            glass_send_msg(handler())\n");
    output.push_str("        end)\n");
    output.push_str("        glass_active_subs[key] = { handle = t, kind = \"timer\" }\n");

    output.push_str("    elseif sub.tag == glass_TAG_Subscription_OnSpellEffect then\n");
    output.push_str("        local t = CreateTrigger()\n");
    output.push_str("        for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("            TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_SPELL_EFFECT, nil)\n");
    output.push_str("        end\n");
    output.push_str("        TriggerAddAction(t, function()\n");
    output.push_str("            glass_send_msg(handler(GetTriggerUnit(), GetSpellAbilityId(), GetSpellTargetUnit()))\n");
    output.push_str("        end)\n");
    output.push_str("        glass_active_subs[key] = { handle = t, kind = \"trigger\" }\n");

    output.push_str("    elseif sub.tag == glass_TAG_Subscription_OnItemPickup then\n");
    output.push_str("        local t = CreateTrigger()\n");
    output.push_str("        for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("            TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_PICKUP_ITEM, nil)\n");
    output.push_str("        end\n");
    output.push_str("        TriggerAddAction(t, function()\n");
    output.push_str("            glass_send_msg(handler(GetTriggerUnit(), GetItemTypeId(GetManipulatedItem())))\n");
    output.push_str("        end)\n");
    output.push_str("        glass_active_subs[key] = { handle = t, kind = \"trigger\" }\n");

    output.push_str("    elseif sub.tag == glass_TAG_Subscription_OnSpellCast then\n");
    output.push_str("        local t = CreateTrigger()\n");
    output.push_str("        for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("            TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_SPELL_CAST, nil)\n");
    output.push_str("        end\n");
    output.push_str("        TriggerAddAction(t, function()\n");
    output.push_str("            glass_send_msg(handler(GetTriggerUnit(), GetSpellAbilityId()))\n");
    output.push_str("        end)\n");
    output.push_str("        glass_active_subs[key] = { handle = t, kind = \"trigger\" }\n");

    output.push_str("    elseif sub.tag == glass_TAG_Subscription_OnSpellChannel then\n");
    output.push_str("        local t = CreateTrigger()\n");
    output.push_str("        for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("            TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_SPELL_CHANNEL, nil)\n");
    output.push_str("        end\n");
    output.push_str("        TriggerAddAction(t, function()\n");
    output.push_str("            glass_send_msg(handler(GetTriggerUnit(), GetSpellAbilityId()))\n");
    output.push_str("        end)\n");
    output.push_str("        glass_active_subs[key] = { handle = t, kind = \"trigger\" }\n");

    output.push_str("    elseif sub.tag == glass_TAG_Subscription_OnDamage then\n");
    output.push_str("        local t = CreateTrigger()\n");
    output.push_str("        for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("            TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_DAMAGED, nil)\n");
    output.push_str("        end\n");
    output.push_str("        TriggerAddAction(t, function()\n");
    output.push_str("            glass_send_msg(handler(GetEventDamageSource(), GetTriggerUnit(), GetEventDamage()))\n");
    output.push_str("        end)\n");
    output.push_str("        glass_active_subs[key] = { handle = t, kind = \"trigger\" }\n");

    output.push_str("    elseif sub.tag == glass_TAG_Subscription_OnItemUse then\n");
    output.push_str("        local t = CreateTrigger()\n");
    output.push_str("        for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("            TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_USE_ITEM, nil)\n");
    output.push_str("        end\n");
    output.push_str("        TriggerAddAction(t, function()\n");
    output.push_str("            glass_send_msg(handler(GetTriggerUnit(), GetItemTypeId(GetManipulatedItem())))\n");
    output.push_str("        end)\n");
    output.push_str("        glass_active_subs[key] = { handle = t, kind = \"trigger\" }\n");

    output.push_str("    elseif sub.tag == glass_TAG_Subscription_OnItemDrop then\n");
    output.push_str("        local t = CreateTrigger()\n");
    output.push_str("        for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("            TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_DROP_ITEM, nil)\n");
    output.push_str("        end\n");
    output.push_str("        TriggerAddAction(t, function()\n");
    output.push_str("            glass_send_msg(handler(GetTriggerUnit(), GetItemTypeId(GetManipulatedItem())))\n");
    output.push_str("        end)\n");
    output.push_str("        glass_active_subs[key] = { handle = t, kind = \"trigger\" }\n");

    output.push_str("    elseif sub.tag == glass_TAG_Subscription_OnChat then\n");
    output.push_str("        local t = CreateTrigger()\n");
    output.push_str("        for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("            TriggerRegisterPlayerChatEvent(t, Player(i), \"\", false)\n");
    output.push_str("        end\n");
    output.push_str("        TriggerAddAction(t, function()\n");
    output.push_str("            glass_send_msg(handler(GetPlayerId(GetTriggerPlayer()), GetEventPlayerChatString()))\n");
    output.push_str("        end)\n");
    output.push_str("        glass_active_subs[key] = { handle = t, kind = \"trigger\" }\n");

    output.push_str("    elseif sub.tag == glass_TAG_Subscription_OnPlayerLeave then\n");
    output.push_str("        local t = CreateTrigger()\n");
    output.push_str("        for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("            TriggerRegisterPlayerEvent(t, Player(i), EVENT_PLAYER_LEAVE)\n");
    output.push_str("        end\n");
    output.push_str("        TriggerAddAction(t, function()\n");
    output.push_str("            glass_send_msg(handler(GetPlayerId(GetTriggerPlayer())))\n");
    output.push_str("        end)\n");
    output.push_str("        glass_active_subs[key] = { handle = t, kind = \"trigger\" }\n");

    output.push_str("    elseif sub.tag == glass_TAG_Subscription_OnHeroLevelUp then\n");
    output.push_str("        local t = CreateTrigger()\n");
    output.push_str("        for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str(
        "            TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_HERO_LEVEL, nil)\n",
    );
    output.push_str("        end\n");
    output.push_str("        TriggerAddAction(t, function()\n");
    output.push_str("            glass_send_msg(handler(GetTriggerUnit()))\n");
    output.push_str("        end)\n");
    output.push_str("        glass_active_subs[key] = { handle = t, kind = \"trigger\" }\n");

    output.push_str("    elseif sub.tag == glass_TAG_Subscription_OnConstructionFinish then\n");
    output.push_str("        local t = CreateTrigger()\n");
    output.push_str("        for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("            TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_CONSTRUCT_FINISH, nil)\n");
    output.push_str("        end\n");
    output.push_str("        TriggerAddAction(t, function()\n");
    output.push_str("            glass_send_msg(handler(GetTriggerUnit()))\n");
    output.push_str("        end)\n");
    output.push_str("        glass_active_subs[key] = { handle = t, kind = \"trigger\" }\n");

    output.push_str("    elseif sub.tag == glass_TAG_Subscription_OnSpellGround then\n");
    output.push_str("        local t = CreateTrigger()\n");
    output.push_str("        for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("            TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_SPELL_EFFECT, nil)\n");
    output.push_str("        end\n");
    output.push_str("        TriggerAddAction(t, function()\n");
    output.push_str("            glass_send_msg(handler(GetTriggerUnit(), GetSpellAbilityId(), GetSpellTargetX(), GetSpellTargetY()))\n");
    output.push_str("        end)\n");
    output.push_str("        glass_active_subs[key] = { handle = t, kind = \"trigger\" }\n");

    output.push_str("    elseif sub.tag == glass_TAG_Subscription_OnSummon then\n");
    output.push_str("        local t = CreateTrigger()\n");
    output.push_str("        for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str(
        "            TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_SUMMON, nil)\n",
    );
    output.push_str("        end\n");
    output.push_str("        TriggerAddAction(t, function()\n");
    output.push_str("            glass_send_msg(handler(GetTriggerUnit(), GetSummonedUnit()))\n");
    output.push_str("        end)\n");
    output.push_str("        glass_active_subs[key] = { handle = t, kind = \"trigger\" }\n");

    output.push_str("    elseif sub.tag == glass_TAG_Subscription_OnUnitSold then\n");
    output.push_str("        local t = CreateTrigger()\n");
    output.push_str("        for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str(
        "            TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_SELL, nil)\n",
    );
    output.push_str("        end\n");
    output.push_str("        TriggerAddAction(t, function()\n");
    output.push_str("            glass_send_msg(handler(GetTriggerUnit(), GetSoldUnit()))\n");
    output.push_str("        end)\n");
    output.push_str("        glass_active_subs[key] = { handle = t, kind = \"trigger\" }\n");

    output.push_str("    elseif sub.tag == glass_TAG_Subscription_OnItemSold then\n");
    output.push_str("        local t = CreateTrigger()\n");
    output.push_str("        for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("            TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_SELL_ITEM, nil)\n");
    output.push_str("        end\n");
    output.push_str("        TriggerAddAction(t, function()\n");
    output.push_str(
        "            glass_send_msg(handler(GetTriggerUnit(), GetItemTypeId(GetSoldItem())))\n",
    );
    output.push_str("        end)\n");
    output.push_str("        glass_active_subs[key] = { handle = t, kind = \"trigger\" }\n");

    output.push_str("    elseif sub.tag == glass_TAG_Subscription_OnUnitTrained then\n");
    output.push_str("        local t = CreateTrigger()\n");
    output.push_str("        for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("            TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_TRAIN_FINISH, nil)\n");
    output.push_str("        end\n");
    output.push_str("        TriggerAddAction(t, function()\n");
    output.push_str("            glass_send_msg(handler(GetTriggerUnit(), GetTrainedUnit()))\n");
    output.push_str("        end)\n");
    output.push_str("        glass_active_subs[key] = { handle = t, kind = \"trigger\" }\n");

    output.push_str("    elseif sub.tag == glass_TAG_Subscription_OnResearchFinish then\n");
    output.push_str("        local t = CreateTrigger()\n");
    output.push_str("        for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("            TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_RESEARCH_FINISH, nil)\n");
    output.push_str("        end\n");
    output.push_str("        TriggerAddAction(t, function()\n");
    output.push_str("            glass_send_msg(handler(GetTriggerUnit(), GetResearched()))\n");
    output.push_str("        end)\n");
    output.push_str("        glass_active_subs[key] = { handle = t, kind = \"trigger\" }\n");

    output.push_str("    elseif sub.tag == glass_TAG_Subscription_OnConstructionStart then\n");
    output.push_str("        local t = CreateTrigger()\n");
    output.push_str("        for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("            TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_CONSTRUCT_START, nil)\n");
    output.push_str("        end\n");
    output.push_str("        TriggerAddAction(t, function()\n");
    output.push_str("            glass_send_msg(handler(GetTriggerUnit()))\n");
    output.push_str("        end)\n");
    output.push_str("        glass_active_subs[key] = { handle = t, kind = \"trigger\" }\n");

    output.push_str("    elseif sub.tag == glass_TAG_Subscription_OnSpellFinish then\n");
    output.push_str("        local t = CreateTrigger()\n");
    output.push_str("        for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("            TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_SPELL_FINISH, nil)\n");
    output.push_str("        end\n");
    output.push_str("        TriggerAddAction(t, function()\n");
    output.push_str("            glass_send_msg(handler(GetTriggerUnit(), GetSpellAbilityId()))\n");
    output.push_str("        end)\n");
    output.push_str("        glass_active_subs[key] = { handle = t, kind = \"trigger\" }\n");

    output.push_str("    elseif sub.tag == glass_TAG_Subscription_OnOrderIssued then\n");
    output.push_str("        local t = CreateTrigger()\n");
    output.push_str("        for i = 0, bj_MAX_PLAYER_SLOTS - 1 do\n");
    output.push_str("            TriggerRegisterPlayerUnitEvent(t, Player(i), EVENT_PLAYER_UNIT_ISSUED_ORDER, nil)\n");
    output.push_str("        end\n");
    output.push_str("        TriggerAddAction(t, function()\n");
    output.push_str("            glass_send_msg(handler(GetTriggerUnit(), GetIssuedOrderId()))\n");
    output.push_str("        end)\n");
    output.push_str("        glass_active_subs[key] = { handle = t, kind = \"trigger\" }\n");

    output.push_str("    end\n");
    output.push_str("end\n\n");
}

fn gen_unregister_one_sub(output: &mut String) {
    output.push_str("function glass_unregister_one_sub(key)\n");
    output.push_str("    local entry = glass_active_subs[key]\n");
    output.push_str("    if entry ~= nil then\n");
    output.push_str("        if entry.kind == \"timer\" then\n");
    output.push_str("            PauseTimer(entry.handle)\n");
    output.push_str("            DestroyTimer(entry.handle)\n");
    output.push_str("        else\n");
    output.push_str("            DisableTrigger(entry.handle)\n");
    output.push_str("            DestroyTrigger(entry.handle)\n");
    output.push_str("        end\n");
    output.push_str("        glass_active_subs[key] = nil\n");
    output.push_str("    end\n");
    output.push_str("end\n\n");
}

fn gen_reconcile_subscriptions(output: &mut String) {
    output.push_str("function glass_reconcile_subs(new_subs)\n");
    output.push_str("    local new_keys = {}\n");
    output.push_str("    local current = new_subs\n");
    output.push_str("    local idx = 0\n");
    output.push_str("    while current ~= nil do\n");
    output.push_str("        local sub = current.head\n");
    output.push_str("        local key = tostring(sub.tag) .. \"_\" .. tostring(idx)\n");
    output.push_str("        new_keys[key] = sub\n");
    output.push_str("        idx = idx + 1\n");
    output.push_str("        current = current.tail\n");
    output.push_str("    end\n");
    output.push_str("    for key, _ in pairs(glass_active_subs) do\n");
    output.push_str("        if new_keys[key] == nil then\n");
    output.push_str("            glass_unregister_one_sub(key)\n");
    output.push_str("        end\n");
    output.push_str("    end\n");
    output.push_str("    for key, sub in pairs(new_keys) do\n");
    output.push_str("        if glass_active_subs[key] == nil then\n");
    output.push_str("            glass_register_one_sub(sub, key)\n");
    output.push_str("        end\n");
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
        output.push_str("    glass_reconcile_subs(glass_subscriptions(glass_model))\n");
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
