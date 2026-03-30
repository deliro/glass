// Elm Architecture runtime code generation for Lua.
//
// The Lua runtime is simpler than JASS because:
// - Functions are first-class (no closure dispatch tables)
// - Tables replace SoA arrays (no tuple extraction functions)
// - Local variables in any scope (no forward declaration issues)

use crate::runtime::{to_snake_case, EffectVariantDef, ElmEntryPoints};
use crate::types::FieldInfo;

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

    gen_exec_effect(entry, output);

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

fn gen_lua_exec_call(variant: &EffectVariantDef, indent: &str, output: &mut String) {
    let snake = to_snake_case(&variant.name);
    let non_cb: Vec<&FieldInfo> = variant.non_callback_fields();
    let args: Vec<String> = non_cb.iter().map(|f| format!("fx.{}", f.name)).collect();
    output.push_str(&format!(
        "{}glass_exec_{}({})\n",
        indent, snake, args.join(", ")
    ));
}

fn gen_lua_after_effect(indent: &str, output: &mut String) {
    output.push_str(&format!("{}local trig = CreateTrigger()\n", indent));
    output.push_str(&format!("{}local cb = fx.callback\n", indent));
    output.push_str(&format!(
        "{}TriggerRegisterTimerEvent(trig, fx.duration, false)\n",
        indent
    ));
    output.push_str(&format!("{}TriggerAddAction(trig, function()\n", indent));
    output.push_str(&format!("{}    local msg = cb()\n", indent));
    output.push_str(&format!("{}    glass_send_msg(msg)\n", indent));
    output.push_str(&format!("{}end)\n", indent));
}

fn gen_lua_find_nearest_enemy(indent: &str, output: &mut String) {
    output.push_str(&format!("{}local g = CreateGroup()\n", indent));
    output.push_str(&format!(
        "{}GroupEnumUnitsInRange(g, fx.x, fx.y, fx.radius, nil)\n",
        indent
    ));
    output.push_str(&format!("{}local best = FirstOfGroup(g)\n", indent));
    output.push_str(&format!("{}DestroyGroup(g)\n", indent));
    output.push_str(&format!("{}if best ~= nil then\n", indent));
    output.push_str(&format!("{}    local msg = fx.callback(best)\n", indent));
    output.push_str(&format!("{}    glass_send_msg(msg)\n", indent));
    output.push_str(&format!("{}end\n", indent));
}

fn gen_lua_create_unit_callback(indent: &str, output: &mut String) {
    output.push_str(&format!(
        "{}local u = CreateUnit(Player(fx.owner), fx.type_id, fx.x, fx.y, fx.facing)\n",
        indent
    ));
    output.push_str(&format!("{}local msg = fx.callback(u)\n", indent));
    output.push_str(&format!("{}glass_send_msg(msg)\n", indent));
}

fn gen_lua_for_units_in_range(indent: &str, output: &mut String) {
    output.push_str(&format!("{}local g = CreateGroup()\n", indent));
    output.push_str(&format!(
        "{}GroupEnumUnitsInRange(g, fx.x, fx.y, fx.radius, nil)\n",
        indent
    ));
    output.push_str(&format!("{}local u = FirstOfGroup(g)\n", indent));
    output.push_str(&format!("{}while u ~= nil do\n", indent));
    output.push_str(&format!("{}    local msg = fx.callback(u)\n", indent));
    output.push_str(&format!("{}    glass_send_msg(msg)\n", indent));
    output.push_str(&format!("{}    GroupRemoveUnit(g, u)\n", indent));
    output.push_str(&format!("{}    u = FirstOfGroup(g)\n", indent));
    output.push_str(&format!("{}end\n", indent));
    output.push_str(&format!("{}DestroyGroup(g)\n", indent));
}

fn gen_lua_update_board(indent: &str, output: &mut String) {
    output.push_str(&format!("{}local row_count = 0\n", indent));
    output.push_str(&format!("{}local cur = fx.rows\n", indent));
    output.push_str(&format!(
        "{}while cur ~= nil do row_count = row_count + 1; cur = cur.tail end\n",
        indent
    ));
    output.push_str(&format!("{}if glass_multiboard == nil then\n", indent));
    output.push_str(&format!(
        "{}    glass_multiboard = CreateMultiboard()\n",
        indent
    ));
    output.push_str(&format!("{}end\n", indent));
    output.push_str(&format!(
        "{}MultiboardSetColumnCount(glass_multiboard, 2)\n",
        indent
    ));
    output.push_str(&format!(
        "{}MultiboardSetRowCount(glass_multiboard, row_count)\n",
        indent
    ));
    output.push_str(&format!(
        "{}MultiboardSetTitleText(glass_multiboard, fx.title)\n",
        indent
    ));
    output.push_str(&format!(
        "{}MultiboardDisplay(glass_multiboard, true)\n",
        indent
    ));
    output.push_str(&format!("{}cur = fx.rows\n", indent));
    output.push_str(&format!("{}local ri = 0\n", indent));
    output.push_str(&format!("{}while cur ~= nil do\n", indent));
    output.push_str(&format!("{}    local r = cur.head\n", indent));
    output.push_str(&format!(
        "{}    local mbi0 = MultiboardGetItem(glass_multiboard, ri, 0)\n",
        indent
    ));
    output.push_str(&format!(
        "{}    MultiboardSetItemValue(mbi0, r.label)\n",
        indent
    ));
    output.push_str(&format!(
        "{}    MultiboardSetItemWidth(mbi0, 0.10)\n",
        indent
    ));
    output.push_str(&format!("{}    MultiboardReleaseItem(mbi0)\n", indent));
    output.push_str(&format!(
        "{}    local mbi1 = MultiboardGetItem(glass_multiboard, ri, 1)\n",
        indent
    ));
    output.push_str(&format!(
        "{}    MultiboardSetItemValue(mbi1, r.value)\n",
        indent
    ));
    output.push_str(&format!(
        "{}    MultiboardSetItemWidth(mbi1, 0.08)\n",
        indent
    ));
    output.push_str(&format!("{}    MultiboardReleaseItem(mbi1)\n", indent));
    output.push_str(&format!("{}    ri = ri + 1\n", indent));
    output.push_str(&format!("{}    cur = cur.tail\n", indent));
    output.push_str(&format!("{}end\n", indent));
}

fn gen_lua_effect_variant_body(
    variant: &EffectVariantDef,
    indent: &str,
    output: &mut String,
) {
    match variant.name.as_str() {
        "After" => gen_lua_after_effect(indent, output),
        "FindNearestEnemy" => gen_lua_find_nearest_enemy(indent, output),
        "CreateUnitCallback" => gen_lua_create_unit_callback(indent, output),
        "ForUnitsInRange" => gen_lua_for_units_in_range(indent, output),
        "UpdateBoard" => gen_lua_update_board(indent, output),
        _ if variant.has_exec_fn => gen_lua_exec_call(variant, indent, output),
        _ => {}
    }
}

fn gen_exec_effect(entry: &ElmEntryPoints, output: &mut String) {
    output.push_str("function glass_exec_effect(fx)\n");

    let mut first = true;
    for variant in &entry.effect_variants {
        let keyword = if first { "if" } else { "elseif" };
        first = false;
        output.push_str(&format!(
            "    {} fx.tag == glass_TAG_Effect_{} then\n",
            keyword, variant.name
        ));
        gen_lua_effect_variant_body(variant, "        ", output);
    }

    if !entry.effect_variants.is_empty() {
        output.push_str("    end\n");
    }
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
