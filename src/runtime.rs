// Elm Architecture runtime code generation for JASS.
//
// The runtime manages:
// - Global model state
// - Message dispatch (glass_send_msg)
// - Effect processing queue
// - Map initialization trigger

use std::collections::HashSet;

use crate::ast::{Definition, Module};
use crate::types::TypeRegistry;

/// Detected Elm architecture entry points.
#[allow(dead_code)] // Fields used progressively across milestones + tests
pub struct ElmEntryPoints {
    pub has_init: bool,
    pub has_update: bool,
    pub has_subscriptions: bool,
    pub msg_variants: Vec<(String, i64, usize)>,
}

impl ElmEntryPoints {
    pub fn detect(module: &Module, types: &TypeRegistry) -> Option<Self> {
        let mut has_init = false;
        let mut has_update = false;
        let mut has_subscriptions = false;
        for def in &module.definitions {
            if let Definition::Function(f) = def {
                match f.name.as_str() {
                    "init" if f.is_pub => has_init = true,
                    "update" if f.is_pub => has_update = true,
                    "subscriptions" if f.is_pub => has_subscriptions = true,
                    _ => {}
                }
            }
        }

        if !has_init || !has_update {
            return None;
        }

        // Find the Msg type to build dispatch table
        let msg_variants = types
            .types
            .get("Msg")
            .map(|info| {
                info.variants
                    .iter()
                    .map(|v| (v.name.clone(), v.tag, v.fields.len()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Some(ElmEntryPoints {
            has_init,
            has_update,
            has_subscriptions,
            msg_variants,
        })
    }
}

struct SubDef {
    name: &'static str,
    dispatch: &'static str,
    event_args: &'static str,
}

const SUB_DEFS: &[SubDef] = &[
    SubDef {
        name: "OnAttack",
        dispatch: "glass_dispatch_2_unit_unit",
        event_args: "GetAttacker(), GetTriggerUnit()",

    },
    SubDef {
        name: "OnDeath",
        dispatch: "glass_dispatch_2_unit_unit",
        event_args: "GetTriggerUnit(), GetKillingUnit()",

    },
    SubDef {
        name: "OnTimer",
        dispatch: "glass_dispatch_void",
        event_args: "",

    },
    SubDef {
        name: "OnSpellEffect",
        dispatch: "glass_dispatch_3_unit_integer_unit",
        event_args: "GetTriggerUnit(), GetSpellAbilityId(), GetSpellTargetUnit()",

    },
    SubDef {
        name: "OnSpellCast",
        dispatch: "glass_dispatch_2_unit_integer",
        event_args: "GetTriggerUnit(), GetSpellAbilityId()",

    },
    SubDef {
        name: "OnSpellChannel",
        dispatch: "glass_dispatch_2_unit_integer",
        event_args: "GetTriggerUnit(), GetSpellAbilityId()",

    },
    SubDef {
        name: "OnDamage",
        dispatch: "glass_dispatch_3_unit_unit_real",
        event_args: "GetEventDamageSource(), GetTriggerUnit(), GetEventDamage()",

    },
    SubDef {
        name: "OnItemPickup",
        dispatch: "glass_dispatch_2_unit_integer",
        event_args: "GetTriggerUnit(), GetItemTypeId(GetManipulatedItem())",

    },
    SubDef {
        name: "OnItemUse",
        dispatch: "glass_dispatch_2_unit_integer",
        event_args: "GetTriggerUnit(), GetItemTypeId(GetManipulatedItem())",

    },
    SubDef {
        name: "OnItemDrop",
        dispatch: "glass_dispatch_2_unit_integer",
        event_args: "GetTriggerUnit(), GetItemTypeId(GetManipulatedItem())",

    },
    SubDef {
        name: "OnChat",
        dispatch: "glass_dispatch_2_integer_string",
        event_args: "GetPlayerId(GetTriggerPlayer()), GetEventPlayerChatString()",

    },
    SubDef {
        name: "OnPlayerLeave",
        dispatch: "glass_dispatch_1_integer",
        event_args: "GetPlayerId(GetTriggerPlayer())",

    },
    SubDef {
        name: "OnHeroLevelUp",
        dispatch: "glass_dispatch_1_unit",
        event_args: "GetTriggerUnit()",

    },
    SubDef {
        name: "OnConstructionFinish",
        dispatch: "glass_dispatch_1_unit",
        event_args: "GetTriggerUnit()",

    },
    SubDef {
        name: "OnSpellGround",
        dispatch: "glass_dispatch_4_unit_integer_real_real",
        event_args: "GetTriggerUnit(), GetSpellAbilityId(), GetSpellTargetX(), GetSpellTargetY()",

    },
    SubDef {
        name: "OnSummon",
        dispatch: "glass_dispatch_2_unit_unit",
        event_args: "GetTriggerUnit(), GetSummonedUnit()",

    },
    SubDef {
        name: "OnUnitSold",
        dispatch: "glass_dispatch_2_unit_unit",
        event_args: "GetTriggerUnit(), GetSoldUnit()",

    },
    SubDef {
        name: "OnItemSold",
        dispatch: "glass_dispatch_2_unit_integer",
        event_args: "GetTriggerUnit(), GetItemTypeId(GetSoldItem())",

    },
    SubDef {
        name: "OnUnitTrained",
        dispatch: "glass_dispatch_2_unit_unit",
        event_args: "GetTriggerUnit(), GetTrainedUnit()",

    },
    SubDef {
        name: "OnResearchFinish",
        dispatch: "glass_dispatch_2_unit_integer",
        event_args: "GetTriggerUnit(), GetResearched()",

    },
    SubDef {
        name: "OnConstructionStart",
        dispatch: "glass_dispatch_1_unit",
        event_args: "GetTriggerUnit()",

    },
    SubDef {
        name: "OnSpellFinish",
        dispatch: "glass_dispatch_2_unit_integer",
        event_args: "GetTriggerUnit(), GetSpellAbilityId()",

    },
    SubDef {
        name: "OnOrderIssued",
        dispatch: "glass_dispatch_2_unit_integer",
        event_args: "GetTriggerUnit(), GetIssuedOrderId()",

    },
];

fn sub_snake_name(name: &str) -> String {
    name.chars()
        .enumerate()
        .fold(String::new(), |mut acc, (i, c)| {
            if c.is_uppercase() && i > 0 {
                acc.push('_');
            }
            acc.push(c.to_ascii_lowercase());
            acc
        })
}

fn sub_global_name(name: &str) -> String {
    format!("glass_sub_{}", sub_snake_name(name))
}

/// Collect runtime globals (merged into the single globals block).
pub fn collect_runtime_globals(entry: &ElmEntryPoints, globals: &mut Vec<String>) {
    globals.push("    // ========== Glass Elm Runtime ==========".into());
    globals.push("    integer glass_model = 0".into());
    globals.push("    integer glass_msg_tag = 0".into());
    // Timer data hashtable for closure dispatch
    globals.push("    hashtable glass_timer_ht = null".into());
    globals.push("    group glass_group_temp = null".into());
    globals.push("    multiboard glass_multiboard = null".into());
    globals.push("    string array glass_BoardRow_label".into());
    globals.push("    string array glass_BoardRow_value".into());
    if entry.has_subscriptions {
        for sub in SUB_DEFS {
            globals.push(format!("    integer {} = -1", sub_global_name(sub.name)));
        }
        globals.push("    integer array glass_sub_tags".into());
        globals.push("    trigger array glass_sub_triggers".into());
        globals.push("    timer array glass_sub_timers".into());
        globals.push("    integer glass_sub_count = 0".into());
    }
}

/// Generate the Elm runtime JASS functions (after user code).
pub fn gen_elm_runtime_functions(
    entry: &ElmEntryPoints,
    _lambdas: &[crate::closures::LambdaInfo],
    dispatch_sigs: &HashSet<String>,
    output: &mut String,
) {
    output.push_str("// ========== Glass Elm Runtime Functions ==========\n\n");

    // Order matters in JASS: callees must be defined before callers.
    // timer_callback is self-contained (inlines effect processing).
    // exec_effect references `function glass_timer_callback`, so must come after.
    // Order: rt_tuple → dispatch_update → handle_lookup → timer_callback → exec_effect → process_effects → send_msg
    gen_rt_tuple_helpers(output);
    gen_msg_dispatch(entry, output);
    gen_timer_callback(output);
    gen_exec_effect(output);
    gen_process_effects(output);
    gen_send_msg(entry, output);
    if entry.has_subscriptions {
        gen_subscription_callbacks(dispatch_sigs, output);
        gen_sub_callbacks(output);
        gen_register_one_sub(output);
        gen_unregister_one_sub(output);
        gen_reconcile_subs(output);
    }

    gen_runtime_init(entry, output);

    output.push_str("function InitTrig_GlassInit takes nothing returns nothing\n");
    output.push_str("    local trigger t = CreateTrigger()\n");
    output.push_str("    call TriggerRegisterTimerEvent(t, 0.00, false)\n");
    output.push_str("    call TriggerAddAction(t, function glass_runtime_init)\n");
    output.push_str("endfunction\n\n");
}

fn gen_runtime_init(entry: &ElmEntryPoints, output: &mut String) {
    output.push_str("function glass_runtime_init takes nothing returns nothing\n");
    output.push_str("    local integer glass_result\n");
    output.push_str("    local integer glass_effects\n");
    output.push_str("    set glass_timer_ht = InitHashtable()\n");
    output.push_str("    set glass_result = glass_init()\n");
    output.push_str("    set glass_model = glass_rt_tuple_0(glass_result)\n");
    output.push_str("    set glass_effects = glass_rt_tuple_1(glass_result)\n");
    output.push_str("    call glass_process_effects(glass_effects)\n");
    if entry.has_subscriptions {
        output.push_str("    call glass_reconcile_subs()\n");
    }
    output.push_str("endfunction\n\n");
}

fn gen_subscription_callbacks(dispatch_sigs: &HashSet<String>, output: &mut String) {
    for sub in SUB_DEFS {
        if !dispatch_sigs.contains(sub.dispatch) {
            continue;
        }
        let cb_name = format!("glass_sub_cb_{}", sub_snake_name(sub.name));
        let global = sub_global_name(sub.name);

        output.push_str(&format!(
            "function {} takes nothing returns nothing\n",
            cb_name
        ));

        if sub.event_args.is_empty() {
            output.push_str(&format!(
                "    call glass_send_msg({}({}), 0, 0)\n",
                sub.dispatch, global
            ));
        } else {
            output.push_str(&format!(
                "    call glass_send_msg({}({}, {}), 0, 0)\n",
                sub.dispatch, global, sub.event_args
            ));
        }

        output.push_str("endfunction\n\n");
    }
}

/// Execute a single effect by reading its tag and fields from the Effect SoA.
fn gen_exec_effect(output: &mut String) {
    output.push_str("function glass_exec_effect takes integer fx_id returns nothing\n");
    output.push_str("    local integer fx_tag = glass_Effect_tag[fx_id]\n");
    output.push_str("    local timer t\n");
    output.push_str("    local unit u\n");
    output.push_str("    local effect sfx\n");
    output.push_str("    local texttag tt\n");
    output.push_str("    local sound snd\n");
    output.push_str("    local integer row_count\n");
    output.push_str("    local integer row_cur\n");
    output.push_str("    local integer row_data\n");
    output.push_str("    local multiboarditem mbi\n");
    output.push_str("    local integer ri\n");

    // After — timer-based delayed callback
    output.push_str("    if fx_tag == glass_TAG_Effect_After then\n");
    output.push_str("        set t = CreateTimer()\n");
    output.push_str("        call SaveInteger(glass_timer_ht, GetHandleId(t), 0, glass_Effect_After_callback[fx_id])\n");
    output.push_str("        call TimerStart(t, glass_Effect_After_duration[fx_id], false, function glass_timer_callback)\n");
    output.push_str("        set t = null\n");

    // DisplayText
    output.push_str("    elseif fx_tag == glass_TAG_Effect_DisplayText then\n");
    output.push_str("        call DisplayTimedTextToPlayer(Player(glass_Effect_DisplayText_player_id[fx_id]), 0, 0, glass_Effect_DisplayText_duration[fx_id], glass_Effect_DisplayText_text[fx_id])\n");

    // DamageUnit
    output.push_str("    elseif fx_tag == glass_TAG_Effect_DamageUnit then\n");
    output.push_str("        call UnitDamageTarget(glass_Effect_DamageUnit_source[fx_id], glass_Effect_DamageUnit_target[fx_id], glass_Effect_DamageUnit_amount[fx_id], true, false, glass_Effect_DamageUnit_attack_type[fx_id], glass_Effect_DamageUnit_damage_type[fx_id], 0)\n");

    // CreateUnit — creates and registers in handle table
    output.push_str("    elseif fx_tag == glass_TAG_Effect_CreateUnit then\n");
    output.push_str("        set u = CreateUnit(Player(glass_Effect_CreateUnit_owner[fx_id]), glass_Effect_CreateUnit_type_id[fx_id], glass_Effect_CreateUnit_x[fx_id], glass_Effect_CreateUnit_y[fx_id], glass_Effect_CreateUnit_facing[fx_id])\n");
    output.push_str("        set u = null\n");

    // RemoveUnit
    output.push_str("    elseif fx_tag == glass_TAG_Effect_RemoveUnit then\n");
    output.push_str("        call RemoveUnit(glass_Effect_RemoveUnit_unit[fx_id])\n");

    // MoveUnit
    output.push_str("    elseif fx_tag == glass_TAG_Effect_MoveUnit then\n");
    output.push_str("        call SetUnitPosition(glass_Effect_MoveUnit_unit[fx_id], glass_Effect_MoveUnit_x[fx_id], glass_Effect_MoveUnit_y[fx_id])\n");

    // PlayAnimation
    output.push_str("    elseif fx_tag == glass_TAG_Effect_PlayAnimation then\n");
    output.push_str("        call SetUnitAnimation(glass_Effect_PlayAnimation_unit[fx_id], glass_Effect_PlayAnimation_anim[fx_id])\n");

    // AddAbility
    output.push_str("    elseif fx_tag == glass_TAG_Effect_AddAbility then\n");
    output.push_str("        call UnitAddAbility(glass_Effect_AddAbility_unit[fx_id], glass_Effect_AddAbility_ability_id[fx_id])\n");

    // AddSfx
    output.push_str("    elseif fx_tag == glass_TAG_Effect_AddSfx then\n");
    output.push_str("        set sfx = AddSpecialEffect(glass_Effect_AddSfx_model[fx_id], glass_Effect_AddSfx_x[fx_id], glass_Effect_AddSfx_y[fx_id])\n");
    output.push_str("        call DestroyEffect(sfx)\n");
    output.push_str("        set sfx = null\n");

    // SetUnitHp
    output.push_str("    elseif fx_tag == glass_TAG_Effect_SetUnitHp then\n");
    output.push_str("        call SetUnitState(glass_Effect_SetUnitHp_unit[fx_id], UNIT_STATE_LIFE, glass_Effect_SetUnitHp_hp[fx_id])\n");

    // SetUnitMana
    output.push_str("    elseif fx_tag == glass_TAG_Effect_SetUnitMana then\n");
    output.push_str("        call SetUnitState(glass_Effect_SetUnitMana_unit[fx_id], UNIT_STATE_MANA, glass_Effect_SetUnitMana_mana[fx_id])\n");

    // PanCamera
    output.push_str("    elseif fx_tag == glass_TAG_Effect_PanCamera then\n");
    output.push_str(
        "        if GetLocalPlayer() == Player(glass_Effect_PanCamera_player_id[fx_id]) then\n",
    );
    output.push_str("            call PanCameraTo(glass_Effect_PanCamera_x[fx_id], glass_Effect_PanCamera_y[fx_id])\n");
    output.push_str("        endif\n");

    // ShowFloatingText
    output.push_str("    elseif fx_tag == glass_TAG_Effect_ShowFloatingText then\n");
    output.push_str("        set tt = CreateTextTag()\n");
    output.push_str("        call SetTextTagText(tt, glass_Effect_ShowFloatingText_text[fx_id], glass_Effect_ShowFloatingText_size[fx_id])\n");
    output.push_str("        call SetTextTagPos(tt, glass_Effect_ShowFloatingText_x[fx_id], glass_Effect_ShowFloatingText_y[fx_id], 0.0)\n");
    output.push_str("        call SetTextTagLifespan(tt, 3.0)\n");
    output.push_str("        call SetTextTagPermanent(tt, false)\n");
    output.push_str("        call SetTextTagVelocity(tt, 0.0, 0.04)\n");
    output.push_str("        set tt = null\n");

    // GiveGold
    output.push_str("    elseif fx_tag == glass_TAG_Effect_GiveGold then\n");
    output.push_str("        call SetPlayerState(Player(glass_Effect_GiveGold_player_id[fx_id]), PLAYER_STATE_RESOURCE_GOLD, GetPlayerState(Player(glass_Effect_GiveGold_player_id[fx_id]), PLAYER_STATE_RESOURCE_GOLD) + glass_Effect_GiveGold_gold_amount[fx_id])\n");

    // GiveLumber
    output.push_str("    elseif fx_tag == glass_TAG_Effect_GiveLumber then\n");
    output.push_str("        call SetPlayerState(Player(glass_Effect_GiveLumber_player_id[fx_id]), PLAYER_STATE_RESOURCE_LUMBER, GetPlayerState(Player(glass_Effect_GiveLumber_player_id[fx_id]), PLAYER_STATE_RESOURCE_LUMBER) + glass_Effect_GiveLumber_lumber_amount[fx_id])\n");

    // KillUnit
    output.push_str("    elseif fx_tag == glass_TAG_Effect_KillUnit then\n");
    output.push_str("        call KillUnit(glass_Effect_KillUnit_unit[fx_id])\n");

    // RemoveAbility
    output.push_str("    elseif fx_tag == glass_TAG_Effect_RemoveAbility then\n");
    output.push_str("        call UnitRemoveAbility(glass_Effect_RemoveAbility_unit[fx_id], glass_Effect_RemoveAbility_ability_id[fx_id])\n");

    // SetUnitOwner
    output.push_str("    elseif fx_tag == glass_TAG_Effect_SetUnitOwner then\n");
    output.push_str("        call SetUnitOwner(glass_Effect_SetUnitOwner_unit[fx_id], Player(glass_Effect_SetUnitOwner_player_id[fx_id]), true)\n");

    // PauseUnit
    output.push_str("    elseif fx_tag == glass_TAG_Effect_PauseUnit then\n");
    output.push_str("        call PauseUnit(glass_Effect_PauseUnit_unit[fx_id], glass_Effect_PauseUnit_paused[fx_id])\n");

    // ShowUnit
    output.push_str("    elseif fx_tag == glass_TAG_Effect_ShowUnit then\n");
    output.push_str("        call ShowUnit(glass_Effect_ShowUnit_unit[fx_id], glass_Effect_ShowUnit_shown[fx_id])\n");

    // SetInvulnerable
    output.push_str("    elseif fx_tag == glass_TAG_Effect_SetInvulnerable then\n");
    output.push_str("        call SetUnitInvulnerable(glass_Effect_SetInvulnerable_unit[fx_id], glass_Effect_SetInvulnerable_invulnerable[fx_id])\n");

    // IssueOrder
    output.push_str("    elseif fx_tag == glass_TAG_Effect_IssueOrder then\n");
    output.push_str("        call IssueImmediateOrder(glass_Effect_IssueOrder_unit[fx_id], glass_Effect_IssueOrder_order[fx_id])\n");

    // IssuePointOrder
    output.push_str("    elseif fx_tag == glass_TAG_Effect_IssuePointOrder then\n");
    output.push_str("        call IssuePointOrder(glass_Effect_IssuePointOrder_unit[fx_id], glass_Effect_IssuePointOrder_order[fx_id], glass_Effect_IssuePointOrder_x[fx_id], glass_Effect_IssuePointOrder_y[fx_id])\n");

    // IssueTargetOrder
    output.push_str("    elseif fx_tag == glass_TAG_Effect_IssueTargetOrder then\n");
    output.push_str("        call IssueTargetOrder(glass_Effect_IssueTargetOrder_unit[fx_id], glass_Effect_IssueTargetOrder_order[fx_id], glass_Effect_IssueTargetOrder_target[fx_id])\n");

    // AddSfxTarget
    output.push_str("    elseif fx_tag == glass_TAG_Effect_AddSfxTarget then\n");
    output.push_str("        set sfx = AddSpecialEffectTarget(glass_Effect_AddSfxTarget_model[fx_id], glass_Effect_AddSfxTarget_unit[fx_id], glass_Effect_AddSfxTarget_attach_point[fx_id])\n");
    output.push_str("        call DestroyEffect(sfx)\n");
    output.push_str("        set sfx = null\n");

    // ReviveHero
    output.push_str("    elseif fx_tag == glass_TAG_Effect_ReviveHero then\n");
    output.push_str("        call ReviveHero(glass_Effect_ReviveHero_unit[fx_id], glass_Effect_ReviveHero_x[fx_id], glass_Effect_ReviveHero_y[fx_id], true)\n");

    // AddHeroXp
    output.push_str("    elseif fx_tag == glass_TAG_Effect_AddHeroXp then\n");
    output.push_str("        call AddHeroXP(glass_Effect_AddHeroXp_unit[fx_id], glass_Effect_AddHeroXp_xp[fx_id], true)\n");

    // SetUnitFacing
    output.push_str("    elseif fx_tag == glass_TAG_Effect_SetUnitFacing then\n");
    output.push_str("        call SetUnitFacing(glass_Effect_SetUnitFacing_unit[fx_id], glass_Effect_SetUnitFacing_facing[fx_id])\n");

    // PingMinimap
    output.push_str("    elseif fx_tag == glass_TAG_Effect_PingMinimap then\n");
    output.push_str("        call PingMinimap(glass_Effect_PingMinimap_x[fx_id], glass_Effect_PingMinimap_y[fx_id], glass_Effect_PingMinimap_duration[fx_id])\n");

    // CreateItem
    output.push_str("    elseif fx_tag == glass_TAG_Effect_CreateItem then\n");
    output.push_str("        call CreateItem(glass_Effect_CreateItem_item_type_id[fx_id], glass_Effect_CreateItem_x[fx_id], glass_Effect_CreateItem_y[fx_id])\n");

    // SetUnitMoveSpeed
    output.push_str("    elseif fx_tag == glass_TAG_Effect_SetUnitMoveSpeed then\n");
    output.push_str("        call SetUnitMoveSpeed(glass_Effect_SetUnitMoveSpeed_unit[fx_id], glass_Effect_SetUnitMoveSpeed_speed[fx_id])\n");

    // CreateUnitCallback — deferred via 0-duration timer, cb_type=1 for unit dispatch
    output.push_str("    elseif fx_tag == glass_TAG_Effect_CreateUnitCallback then\n");
    output.push_str("        set t = CreateTimer()\n");
    output.push_str("        set u = CreateUnit(Player(glass_Effect_CreateUnitCallback_owner[fx_id]), glass_Effect_CreateUnitCallback_type_id[fx_id], glass_Effect_CreateUnitCallback_x[fx_id], glass_Effect_CreateUnitCallback_y[fx_id], glass_Effect_CreateUnitCallback_facing[fx_id])\n");
    output.push_str("        call SaveInteger(glass_timer_ht, GetHandleId(t), 0, glass_Effect_CreateUnitCallback_callback[fx_id])\n");
    output.push_str("        call SaveUnitHandle(glass_timer_ht, GetHandleId(t), 1, u)\n");
    output.push_str("        call SaveInteger(glass_timer_ht, GetHandleId(t), 2, 1)\n");
    output.push_str("        call TimerStart(t, 0.0, false, function glass_timer_callback)\n");
    output.push_str("        set u = null\n");
    output.push_str("        set t = null\n");

    // ForUnitsInRange — iterate group, defer each unit callback via 0-duration timer
    output.push_str("    elseif fx_tag == glass_TAG_Effect_ForUnitsInRange then\n");
    output.push_str("        set glass_group_temp = CreateGroup()\n");
    output.push_str("        call GroupEnumUnitsInRange(glass_group_temp, glass_Effect_ForUnitsInRange_x[fx_id], glass_Effect_ForUnitsInRange_y[fx_id], glass_Effect_ForUnitsInRange_radius[fx_id], null)\n");
    output.push_str("        loop\n");
    output.push_str("            set u = FirstOfGroup(glass_group_temp)\n");
    output.push_str("            exitwhen u == null\n");
    output.push_str("            set t = CreateTimer()\n");
    output.push_str("            call SaveInteger(glass_timer_ht, GetHandleId(t), 0, glass_Effect_ForUnitsInRange_callback[fx_id])\n");
    output.push_str("            call SaveUnitHandle(glass_timer_ht, GetHandleId(t), 1, u)\n");
    output.push_str("            call SaveInteger(glass_timer_ht, GetHandleId(t), 2, 1)\n");
    output.push_str("            call TimerStart(t, 0.0, false, function glass_timer_callback)\n");
    output.push_str("            call GroupRemoveUnit(glass_group_temp, u)\n");
    output.push_str("            set t = null\n");
    output.push_str("        endloop\n");
    output.push_str("        call DestroyGroup(glass_group_temp)\n");
    output.push_str("        set glass_group_temp = null\n");

    // PlaySound
    output.push_str("    elseif fx_tag == glass_TAG_Effect_PlaySound then\n");
    output.push_str("        set snd = CreateSound(glass_Effect_PlaySound_path[fx_id], false, false, false, 10, 10, \"DefaultEAXON\")\n");
    output.push_str("        call SetSoundVolume(snd, 127)\n");
    output.push_str("        call StartSound(snd)\n");
    output.push_str("        call KillSoundWhenDone(snd)\n");
    output.push_str("        set snd = null\n");

    // UpdateBoard
    output.push_str("    elseif fx_tag == glass_TAG_Effect_UpdateBoard then\n");
    output.push_str("        set row_count = 0\n");
    output.push_str("        set row_cur = glass_Effect_UpdateBoard_rows[fx_id]\n");
    output.push_str("        loop\n");
    output.push_str("            exitwhen row_cur == -1\n");
    output.push_str("            set row_count = row_count + 1\n");
    output.push_str("            set row_cur = glass_List_integer_tail[row_cur]\n");
    output.push_str("        endloop\n");
    output.push_str("        if glass_multiboard == null then\n");
    output.push_str("            set glass_multiboard = CreateMultiboard()\n");
    output.push_str("        endif\n");
    output.push_str("        call MultiboardSetColumnCount(glass_multiboard, 2)\n");
    output.push_str("        call MultiboardSetRowCount(glass_multiboard, row_count)\n");
    output.push_str("        call MultiboardSetTitleText(glass_multiboard, glass_Effect_UpdateBoard_title[fx_id])\n");
    output.push_str("        call MultiboardDisplay(glass_multiboard, true)\n");
    output.push_str("        set row_cur = glass_Effect_UpdateBoard_rows[fx_id]\n");
    output.push_str("        set ri = 0\n");
    output.push_str("        loop\n");
    output.push_str("            exitwhen row_cur == -1\n");
    output.push_str("            set row_data = glass_List_integer_head[row_cur]\n");
    output.push_str("            set mbi = MultiboardGetItem(glass_multiboard, ri, 0)\n");
    output
        .push_str("            call MultiboardSetItemValue(mbi, glass_BoardRow_label[row_data])\n");
    output.push_str("            call MultiboardSetItemWidth(mbi, 0.10)\n");
    output.push_str("            call MultiboardReleaseItem(mbi)\n");
    output.push_str("            set mbi = MultiboardGetItem(glass_multiboard, ri, 1)\n");
    output
        .push_str("            call MultiboardSetItemValue(mbi, glass_BoardRow_value[row_data])\n");
    output.push_str("            call MultiboardSetItemWidth(mbi, 0.08)\n");
    output.push_str("            call MultiboardReleaseItem(mbi)\n");
    output.push_str("            set ri = ri + 1\n");
    output.push_str("            set row_cur = glass_List_integer_tail[row_cur]\n");
    output.push_str("        endloop\n");
    output.push_str("        set mbi = null\n");

    output.push_str("    endif\n");
    output.push_str("    call glass_Effect_dealloc(fx_id)\n");
    output.push_str("endfunction\n\n");
}

/// Walk a List(Effect) and execute each effect.
/// Batch effects are handled here by pushing their sub-list onto a stack.
fn gen_process_effects(output: &mut String) {
    output.push_str("function glass_process_effects takes integer effect_list returns nothing\n");
    output.push_str("    local integer current = effect_list\n");
    output.push_str("    loop\n");
    output.push_str("        exitwhen current == -1\n");
    output.push_str("        call glass_exec_effect(glass_List_integer_head[current])\n");
    output.push_str("        set current = glass_List_integer_tail[current]\n");
    output.push_str("    endloop\n");
    output.push_str("endfunction\n\n");
}

/// Timer callback: fully self-contained to avoid JASS forward reference cycle.
/// Inlines effect processing because `glass_exec_effect` references
/// `function glass_timer_callback` for After effects (circular dependency).
fn gen_timer_callback(output: &mut String) {
    output.push_str("function glass_timer_callback takes nothing returns nothing\n");
    output.push_str("    local timer t = GetExpiredTimer()\n");
    output.push_str("    local integer hid = GetHandleId(t)\n");
    output.push_str("    local integer closure_id = LoadInteger(glass_timer_ht, hid, 0)\n");
    output.push_str("    local integer cb_type = LoadInteger(glass_timer_ht, hid, 2)\n");
    output.push_str("    local unit cb_unit = LoadUnitHandle(glass_timer_ht, hid, 1)\n");
    output.push_str("    local integer msg_result = 0\n");
    output.push_str("    local integer glass_result\n");
    output.push_str("    local integer glass_effects\n");
    output.push_str("    local integer current\n");
    output.push_str("    local integer fx_id\n");
    output.push_str("    local integer fx_tag\n");
    output.push_str("    local timer t2\n");
    output.push_str("    local unit u\n");
    output.push_str("    local effect sfx\n");
    output.push_str("    local texttag tt\n");
    output.push_str("    local sound snd\n");
    output.push_str("    local integer row_count\n");
    output.push_str("    local integer row_cur\n");
    output.push_str("    local integer row_data\n");
    output.push_str("    local multiboarditem mbi\n");
    output.push_str("    local integer ri\n");
    // Dispatch closure → get Msg (void or unit callback)
    output.push_str("    if cb_type == 1 then\n");
    output.push_str("        set msg_result = glass_dispatch_1_unit(closure_id, cb_unit)\n");
    output.push_str("    else\n");
    output.push_str("        set msg_result = glass_dispatch_void(closure_id)\n");
    output.push_str("    endif\n");
    // Cleanup expired timer
    output.push_str("    call FlushChildHashtable(glass_timer_ht, hid)\n");
    output.push_str("    call DestroyTimer(t)\n");
    output.push_str("    set t = null\n");
    output.push_str("    set cb_unit = null\n");
    // Call update (inlined send_msg)
    output.push_str("    set glass_msg_tag = msg_result\n");
    output.push_str("    set glass_result = glass_dispatch_update()\n");
    output.push_str("    set glass_model = glass_rt_tuple_0(glass_result)\n");
    output.push_str("    set glass_effects = glass_rt_tuple_1(glass_result)\n");
    // Walk effect list (inlined — cannot call glass_exec_effect due to forward ref)
    output.push_str("    set current = glass_effects\n");
    output.push_str("    loop\n");
    output.push_str("        exitwhen current == -1\n");
    output.push_str("        set fx_id = glass_List_integer_head[current]\n");
    output.push_str("        set fx_tag = glass_Effect_tag[fx_id]\n");
    // After — self-referencing timer
    output.push_str("        if fx_tag == glass_TAG_Effect_After then\n");
    output.push_str("            set t2 = CreateTimer()\n");
    output.push_str("            call SaveInteger(glass_timer_ht, GetHandleId(t2), 0, glass_Effect_After_callback[fx_id])\n");
    output.push_str("            call TimerStart(t2, glass_Effect_After_duration[fx_id], false, function glass_timer_callback)\n");
    output.push_str("            set t2 = null\n");
    // DisplayText
    output.push_str("        elseif fx_tag == glass_TAG_Effect_DisplayText then\n");
    output.push_str("            call DisplayTimedTextToPlayer(Player(glass_Effect_DisplayText_player_id[fx_id]), 0, 0, glass_Effect_DisplayText_duration[fx_id], glass_Effect_DisplayText_text[fx_id])\n");
    // CreateUnit
    output.push_str("        elseif fx_tag == glass_TAG_Effect_CreateUnit then\n");
    output.push_str("            set u = CreateUnit(Player(glass_Effect_CreateUnit_owner[fx_id]), glass_Effect_CreateUnit_type_id[fx_id], glass_Effect_CreateUnit_x[fx_id], glass_Effect_CreateUnit_y[fx_id], glass_Effect_CreateUnit_facing[fx_id])\n");
    output.push_str("            set u = null\n");
    // DamageUnit
    output.push_str("        elseif fx_tag == glass_TAG_Effect_DamageUnit then\n");
    output.push_str("            call UnitDamageTarget(glass_Effect_DamageUnit_source[fx_id], glass_Effect_DamageUnit_target[fx_id], glass_Effect_DamageUnit_amount[fx_id], true, false, glass_Effect_DamageUnit_attack_type[fx_id], glass_Effect_DamageUnit_damage_type[fx_id], 0)\n");
    // AddSfx
    output.push_str("        elseif fx_tag == glass_TAG_Effect_AddSfx then\n");
    output.push_str("            set sfx = AddSpecialEffect(glass_Effect_AddSfx_model[fx_id], glass_Effect_AddSfx_x[fx_id], glass_Effect_AddSfx_y[fx_id])\n");
    output.push_str("            call DestroyEffect(sfx)\n");
    output.push_str("            set sfx = null\n");
    // RemoveUnit
    output.push_str("        elseif fx_tag == glass_TAG_Effect_RemoveUnit then\n");
    output.push_str("            call RemoveUnit(glass_Effect_RemoveUnit_unit[fx_id])\n");
    // MoveUnit
    output.push_str("        elseif fx_tag == glass_TAG_Effect_MoveUnit then\n");
    output.push_str("            call SetUnitPosition(glass_Effect_MoveUnit_unit[fx_id], glass_Effect_MoveUnit_x[fx_id], glass_Effect_MoveUnit_y[fx_id])\n");
    // PlayAnimation
    output.push_str("        elseif fx_tag == glass_TAG_Effect_PlayAnimation then\n");
    output.push_str("            call SetUnitAnimation(glass_Effect_PlayAnimation_unit[fx_id], glass_Effect_PlayAnimation_anim[fx_id])\n");
    // AddAbility
    output.push_str("        elseif fx_tag == glass_TAG_Effect_AddAbility then\n");
    output.push_str("            call UnitAddAbility(glass_Effect_AddAbility_unit[fx_id], glass_Effect_AddAbility_ability_id[fx_id])\n");
    // SetUnitHp
    output.push_str("        elseif fx_tag == glass_TAG_Effect_SetUnitHp then\n");
    output.push_str("            call SetUnitState(glass_Effect_SetUnitHp_unit[fx_id], UNIT_STATE_LIFE, glass_Effect_SetUnitHp_hp[fx_id])\n");
    // SetUnitMana
    output.push_str("        elseif fx_tag == glass_TAG_Effect_SetUnitMana then\n");
    output.push_str("            call SetUnitState(glass_Effect_SetUnitMana_unit[fx_id], UNIT_STATE_MANA, glass_Effect_SetUnitMana_mana[fx_id])\n");
    // PanCamera
    output.push_str("        elseif fx_tag == glass_TAG_Effect_PanCamera then\n");
    output.push_str(
        "            if GetLocalPlayer() == Player(glass_Effect_PanCamera_player_id[fx_id]) then\n",
    );
    output.push_str("                call PanCameraTo(glass_Effect_PanCamera_x[fx_id], glass_Effect_PanCamera_y[fx_id])\n");
    output.push_str("            endif\n");
    // ShowFloatingText
    output.push_str("        elseif fx_tag == glass_TAG_Effect_ShowFloatingText then\n");
    output.push_str("            set tt = CreateTextTag()\n");
    output.push_str("            call SetTextTagText(tt, glass_Effect_ShowFloatingText_text[fx_id], glass_Effect_ShowFloatingText_size[fx_id])\n");
    output.push_str("            call SetTextTagPos(tt, glass_Effect_ShowFloatingText_x[fx_id], glass_Effect_ShowFloatingText_y[fx_id], 0.0)\n");
    output.push_str("            call SetTextTagLifespan(tt, 3.0)\n");
    output.push_str("            call SetTextTagPermanent(tt, false)\n");
    output.push_str("            call SetTextTagVelocity(tt, 0.0, 0.04)\n");
    output.push_str("            set tt = null\n");
    // GiveGold
    output.push_str("        elseif fx_tag == glass_TAG_Effect_GiveGold then\n");
    output.push_str("            call SetPlayerState(Player(glass_Effect_GiveGold_player_id[fx_id]), PLAYER_STATE_RESOURCE_GOLD, GetPlayerState(Player(glass_Effect_GiveGold_player_id[fx_id]), PLAYER_STATE_RESOURCE_GOLD) + glass_Effect_GiveGold_gold_amount[fx_id])\n");
    // GiveLumber
    output.push_str("        elseif fx_tag == glass_TAG_Effect_GiveLumber then\n");
    output.push_str("            call SetPlayerState(Player(glass_Effect_GiveLumber_player_id[fx_id]), PLAYER_STATE_RESOURCE_LUMBER, GetPlayerState(Player(glass_Effect_GiveLumber_player_id[fx_id]), PLAYER_STATE_RESOURCE_LUMBER) + glass_Effect_GiveLumber_lumber_amount[fx_id])\n");
    // KillUnit
    output.push_str("        elseif fx_tag == glass_TAG_Effect_KillUnit then\n");
    output.push_str("            call KillUnit(glass_Effect_KillUnit_unit[fx_id])\n");
    // RemoveAbility
    output.push_str("        elseif fx_tag == glass_TAG_Effect_RemoveAbility then\n");
    output.push_str("            call UnitRemoveAbility(glass_Effect_RemoveAbility_unit[fx_id], glass_Effect_RemoveAbility_ability_id[fx_id])\n");
    // SetUnitOwner
    output.push_str("        elseif fx_tag == glass_TAG_Effect_SetUnitOwner then\n");
    output.push_str("            call SetUnitOwner(glass_Effect_SetUnitOwner_unit[fx_id], Player(glass_Effect_SetUnitOwner_player_id[fx_id]), true)\n");
    // PauseUnit
    output.push_str("        elseif fx_tag == glass_TAG_Effect_PauseUnit then\n");
    output.push_str("            call PauseUnit(glass_Effect_PauseUnit_unit[fx_id], glass_Effect_PauseUnit_paused[fx_id])\n");
    // ShowUnit
    output.push_str("        elseif fx_tag == glass_TAG_Effect_ShowUnit then\n");
    output.push_str("            call ShowUnit(glass_Effect_ShowUnit_unit[fx_id], glass_Effect_ShowUnit_shown[fx_id])\n");
    // SetInvulnerable
    output.push_str("        elseif fx_tag == glass_TAG_Effect_SetInvulnerable then\n");
    output.push_str("            call SetUnitInvulnerable(glass_Effect_SetInvulnerable_unit[fx_id], glass_Effect_SetInvulnerable_invulnerable[fx_id])\n");
    // IssueOrder
    output.push_str("        elseif fx_tag == glass_TAG_Effect_IssueOrder then\n");
    output.push_str("            call IssueImmediateOrder(glass_Effect_IssueOrder_unit[fx_id], glass_Effect_IssueOrder_order[fx_id])\n");
    // IssuePointOrder
    output.push_str("        elseif fx_tag == glass_TAG_Effect_IssuePointOrder then\n");
    output.push_str("            call IssuePointOrder(glass_Effect_IssuePointOrder_unit[fx_id], glass_Effect_IssuePointOrder_order[fx_id], glass_Effect_IssuePointOrder_x[fx_id], glass_Effect_IssuePointOrder_y[fx_id])\n");
    // IssueTargetOrder
    output.push_str("        elseif fx_tag == glass_TAG_Effect_IssueTargetOrder then\n");
    output.push_str("            call IssueTargetOrder(glass_Effect_IssueTargetOrder_unit[fx_id], glass_Effect_IssueTargetOrder_order[fx_id], glass_Effect_IssueTargetOrder_target[fx_id])\n");
    // AddSfxTarget
    output.push_str("        elseif fx_tag == glass_TAG_Effect_AddSfxTarget then\n");
    output.push_str("            set sfx = AddSpecialEffectTarget(glass_Effect_AddSfxTarget_model[fx_id], glass_Effect_AddSfxTarget_unit[fx_id], glass_Effect_AddSfxTarget_attach_point[fx_id])\n");
    output.push_str("            call DestroyEffect(sfx)\n");
    output.push_str("            set sfx = null\n");
    // ReviveHero
    output.push_str("        elseif fx_tag == glass_TAG_Effect_ReviveHero then\n");
    output.push_str("            call ReviveHero(glass_Effect_ReviveHero_unit[fx_id], glass_Effect_ReviveHero_x[fx_id], glass_Effect_ReviveHero_y[fx_id], true)\n");
    // AddHeroXp
    output.push_str("        elseif fx_tag == glass_TAG_Effect_AddHeroXp then\n");
    output.push_str("            call AddHeroXP(glass_Effect_AddHeroXp_unit[fx_id], glass_Effect_AddHeroXp_xp[fx_id], true)\n");
    // SetUnitFacing
    output.push_str("        elseif fx_tag == glass_TAG_Effect_SetUnitFacing then\n");
    output.push_str("            call SetUnitFacing(glass_Effect_SetUnitFacing_unit[fx_id], glass_Effect_SetUnitFacing_facing[fx_id])\n");
    // PingMinimap
    output.push_str("        elseif fx_tag == glass_TAG_Effect_PingMinimap then\n");
    output.push_str("            call PingMinimap(glass_Effect_PingMinimap_x[fx_id], glass_Effect_PingMinimap_y[fx_id], glass_Effect_PingMinimap_duration[fx_id])\n");
    // CreateItem
    output.push_str("        elseif fx_tag == glass_TAG_Effect_CreateItem then\n");
    output.push_str("            call CreateItem(glass_Effect_CreateItem_item_type_id[fx_id], glass_Effect_CreateItem_x[fx_id], glass_Effect_CreateItem_y[fx_id])\n");
    // SetUnitMoveSpeed
    output.push_str("        elseif fx_tag == glass_TAG_Effect_SetUnitMoveSpeed then\n");
    output.push_str("            call SetUnitMoveSpeed(glass_Effect_SetUnitMoveSpeed_unit[fx_id], glass_Effect_SetUnitMoveSpeed_speed[fx_id])\n");
    // CreateUnitCallback — deferred via 0-duration timer, cb_type=1 for unit dispatch
    output.push_str("        elseif fx_tag == glass_TAG_Effect_CreateUnitCallback then\n");
    output.push_str("            set t2 = CreateTimer()\n");
    output.push_str("            set u = CreateUnit(Player(glass_Effect_CreateUnitCallback_owner[fx_id]), glass_Effect_CreateUnitCallback_type_id[fx_id], glass_Effect_CreateUnitCallback_x[fx_id], glass_Effect_CreateUnitCallback_y[fx_id], glass_Effect_CreateUnitCallback_facing[fx_id])\n");
    output.push_str("            call SaveInteger(glass_timer_ht, GetHandleId(t2), 0, glass_Effect_CreateUnitCallback_callback[fx_id])\n");
    output.push_str("            call SaveUnitHandle(glass_timer_ht, GetHandleId(t2), 1, u)\n");
    output.push_str("            call SaveInteger(glass_timer_ht, GetHandleId(t2), 2, 1)\n");
    output.push_str("            call TimerStart(t2, 0.0, false, function glass_timer_callback)\n");
    output.push_str("            set u = null\n");
    output.push_str("            set t2 = null\n");
    // ForUnitsInRange — iterate group, defer each unit callback via 0-duration timer
    output.push_str("        elseif fx_tag == glass_TAG_Effect_ForUnitsInRange then\n");
    output.push_str("            set glass_group_temp = CreateGroup()\n");
    output.push_str("            call GroupEnumUnitsInRange(glass_group_temp, glass_Effect_ForUnitsInRange_x[fx_id], glass_Effect_ForUnitsInRange_y[fx_id], glass_Effect_ForUnitsInRange_radius[fx_id], null)\n");
    output.push_str("            loop\n");
    output.push_str("                set u = FirstOfGroup(glass_group_temp)\n");
    output.push_str("                exitwhen u == null\n");
    output.push_str("                set t2 = CreateTimer()\n");
    output.push_str("                call SaveInteger(glass_timer_ht, GetHandleId(t2), 0, glass_Effect_ForUnitsInRange_callback[fx_id])\n");
    output.push_str("                call SaveUnitHandle(glass_timer_ht, GetHandleId(t2), 1, u)\n");
    output.push_str("                call SaveInteger(glass_timer_ht, GetHandleId(t2), 2, 1)\n");
    output.push_str(
        "                call TimerStart(t2, 0.0, false, function glass_timer_callback)\n",
    );
    output.push_str("                call GroupRemoveUnit(glass_group_temp, u)\n");
    output.push_str("                set t2 = null\n");
    output.push_str("            endloop\n");
    output.push_str("            call DestroyGroup(glass_group_temp)\n");
    output.push_str("            set glass_group_temp = null\n");
    // PlaySound
    output.push_str("        elseif fx_tag == glass_TAG_Effect_PlaySound then\n");
    output.push_str("            set snd = CreateSound(glass_Effect_PlaySound_path[fx_id], false, false, false, 10, 10, \"DefaultEAXON\")\n");
    output.push_str("            call SetSoundVolume(snd, 127)\n");
    output.push_str("            call StartSound(snd)\n");
    output.push_str("            call KillSoundWhenDone(snd)\n");
    output.push_str("            set snd = null\n");
    // UpdateBoard
    output.push_str("        elseif fx_tag == glass_TAG_Effect_UpdateBoard then\n");
    output.push_str("            set row_count = 0\n");
    output.push_str("            set row_cur = glass_Effect_UpdateBoard_rows[fx_id]\n");
    output.push_str("            loop\n");
    output.push_str("                exitwhen row_cur == -1\n");
    output.push_str("                set row_count = row_count + 1\n");
    output.push_str("                set row_cur = glass_List_integer_tail[row_cur]\n");
    output.push_str("            endloop\n");
    output.push_str("            if glass_multiboard == null then\n");
    output.push_str("                set glass_multiboard = CreateMultiboard()\n");
    output.push_str("            endif\n");
    output.push_str("            call MultiboardSetColumnCount(glass_multiboard, 2)\n");
    output.push_str("            call MultiboardSetRowCount(glass_multiboard, row_count)\n");
    output.push_str("            call MultiboardSetTitleText(glass_multiboard, glass_Effect_UpdateBoard_title[fx_id])\n");
    output.push_str("            call MultiboardDisplay(glass_multiboard, true)\n");
    output.push_str("            set row_cur = glass_Effect_UpdateBoard_rows[fx_id]\n");
    output.push_str("            set ri = 0\n");
    output.push_str("            loop\n");
    output.push_str("                exitwhen row_cur == -1\n");
    output.push_str("                set row_data = glass_List_integer_head[row_cur]\n");
    output.push_str("                set mbi = MultiboardGetItem(glass_multiboard, ri, 0)\n");
    output.push_str(
        "                call MultiboardSetItemValue(mbi, glass_BoardRow_label[row_data])\n",
    );
    output.push_str("                call MultiboardSetItemWidth(mbi, 0.10)\n");
    output.push_str("                call MultiboardReleaseItem(mbi)\n");
    output.push_str("                set mbi = MultiboardGetItem(glass_multiboard, ri, 1)\n");
    output.push_str(
        "                call MultiboardSetItemValue(mbi, glass_BoardRow_value[row_data])\n",
    );
    output.push_str("                call MultiboardSetItemWidth(mbi, 0.08)\n");
    output.push_str("                call MultiboardReleaseItem(mbi)\n");
    output.push_str("                set ri = ri + 1\n");
    output.push_str("                set row_cur = glass_List_integer_tail[row_cur]\n");
    output.push_str("            endloop\n");
    output.push_str("            set mbi = null\n");
    output.push_str("        endif\n");
    output.push_str("        call glass_Effect_dealloc(fx_id)\n");
    output.push_str("        set current = glass_List_integer_tail[current]\n");
    output.push_str("    endloop\n");
    output.push_str("endfunction\n\n");
}

fn gen_msg_dispatch(_entry: &ElmEntryPoints, output: &mut String) {
    // update returns (Model, List(Effect)) — a tuple
    output.push_str("function glass_dispatch_update takes nothing returns integer\n");
    output.push_str("    return glass_update(glass_model, glass_msg_tag)\n");
    output.push_str("endfunction\n\n");
}

fn gen_send_msg(entry: &ElmEntryPoints, output: &mut String) {
    output.push_str(
        "function glass_send_msg takes integer msg returns nothing\n",
    );
    output.push_str("    local integer glass_result\n");
    output.push_str("    local integer glass_new_model\n");
    output.push_str("    local integer glass_effects\n");
    output.push_str("    set glass_msg_tag = msg\n");
    output.push_str("    set glass_result = glass_dispatch_update()\n");
    output.push_str("    set glass_new_model = glass_rt_tuple_0(glass_result)\n");
    output.push_str("    set glass_effects = glass_rt_tuple_1(glass_result)\n");
    output.push_str("    set glass_model = glass_new_model\n");
    output.push_str("    call glass_process_effects(glass_effects)\n");
    if entry.has_subscriptions {
        output.push_str("    call glass_reconcile_subs()\n");
    }
    output.push_str("endfunction\n\n");
}

/// Runtime tuple field extractors. These read from the SoA tuple arrays directly.
/// The naming must match what codegen generates for Tuple2_integer_integer.
fn gen_rt_tuple_helpers(output: &mut String) {
    // glass_rt_tuple_0/1 — extract first/second field from a 2-tuple
    // The tuple SoA is Tuple2_integer_integer (model ID + effect list ID, both integer)
    output.push_str("function glass_rt_tuple_0 takes integer tid returns integer\n");
    output.push_str("    return glass_Tuple2_integer_integer_Tuple2_integer_integer__0[tid]\n");
    output.push_str("endfunction\n\n");

    output.push_str("function glass_rt_tuple_1 takes integer tid returns integer\n");
    output.push_str("    return glass_Tuple2_integer_integer_Tuple2_integer_integer__1[tid]\n");
    output.push_str("endfunction\n\n");
}

struct SubType {
    tag: &'static str,
    event: SubEvent,
    dispatch_call: &'static str,
}

enum SubEvent {
    PlayerUnit(&'static str),
    Player(&'static str),
    Chat,
    Timer,
}

const SUB_TYPES: &[SubType] = &[
    SubType {
        tag: "OnAttack",
        event: SubEvent::PlayerUnit("EVENT_PLAYER_UNIT_ATTACKED"),
        dispatch_call: "glass_dispatch_2_unit_unit(closure_id, GetAttacker(), GetTriggerUnit())",
    },
    SubType {
        tag: "OnDeath",
        event: SubEvent::PlayerUnit("EVENT_PLAYER_UNIT_DEATH"),
        dispatch_call: "glass_dispatch_2_unit_unit(closure_id, GetTriggerUnit(), GetKillingUnit())",
    },
    SubType {
        tag: "OnTimer",
        event: SubEvent::Timer,
        dispatch_call: "glass_dispatch_void(closure_id)",
    },
    SubType {
        tag: "OnSpellEffect",
        event: SubEvent::PlayerUnit("EVENT_PLAYER_UNIT_SPELL_EFFECT"),
        dispatch_call: "glass_dispatch_3_unit_integer_unit(closure_id, GetTriggerUnit(), GetSpellAbilityId(), GetSpellTargetUnit())",
    },
    SubType {
        tag: "OnItemPickup",
        event: SubEvent::PlayerUnit("EVENT_PLAYER_UNIT_PICKUP_ITEM"),
        dispatch_call: "glass_dispatch_2_unit_integer(closure_id, GetTriggerUnit(), GetItemTypeId(GetManipulatedItem()))",
    },
    SubType {
        tag: "OnSpellCast",
        event: SubEvent::PlayerUnit("EVENT_PLAYER_UNIT_SPELL_CAST"),
        dispatch_call: "glass_dispatch_2_unit_integer(closure_id, GetTriggerUnit(), GetSpellAbilityId())",
    },
    SubType {
        tag: "OnSpellChannel",
        event: SubEvent::PlayerUnit("EVENT_PLAYER_UNIT_SPELL_CHANNEL"),
        dispatch_call: "glass_dispatch_2_unit_integer(closure_id, GetTriggerUnit(), GetSpellAbilityId())",
    },
    SubType {
        tag: "OnDamage",
        event: SubEvent::PlayerUnit("EVENT_PLAYER_UNIT_DAMAGED"),
        dispatch_call: "glass_dispatch_3_unit_unit_real(closure_id, GetEventDamageSource(), GetTriggerUnit(), GetEventDamage())",
    },
    SubType {
        tag: "OnItemUse",
        event: SubEvent::PlayerUnit("EVENT_PLAYER_UNIT_USE_ITEM"),
        dispatch_call: "glass_dispatch_2_unit_integer(closure_id, GetTriggerUnit(), GetItemTypeId(GetManipulatedItem()))",
    },
    SubType {
        tag: "OnItemDrop",
        event: SubEvent::PlayerUnit("EVENT_PLAYER_UNIT_DROP_ITEM"),
        dispatch_call: "glass_dispatch_2_unit_integer(closure_id, GetTriggerUnit(), GetItemTypeId(GetManipulatedItem()))",
    },
    SubType {
        tag: "OnChat",
        event: SubEvent::Chat,
        dispatch_call: "glass_dispatch_2_integer_string(closure_id, GetPlayerId(GetTriggerPlayer()), GetEventPlayerChatString())",
    },
    SubType {
        tag: "OnPlayerLeave",
        event: SubEvent::Player("EVENT_PLAYER_LEAVE"),
        dispatch_call: "glass_dispatch_1_integer(closure_id, GetPlayerId(GetTriggerPlayer()))",
    },
    SubType {
        tag: "OnHeroLevelUp",
        event: SubEvent::PlayerUnit("EVENT_PLAYER_HERO_LEVEL"),
        dispatch_call: "glass_dispatch_1_unit(closure_id, GetTriggerUnit())",
    },
    SubType {
        tag: "OnConstructionFinish",
        event: SubEvent::PlayerUnit("EVENT_PLAYER_UNIT_CONSTRUCT_FINISH"),
        dispatch_call: "glass_dispatch_1_unit(closure_id, GetTriggerUnit())",
    },
    SubType {
        tag: "OnSpellGround",
        event: SubEvent::PlayerUnit("EVENT_PLAYER_UNIT_SPELL_EFFECT"),
        dispatch_call: "glass_dispatch_4_unit_integer_real_real(closure_id, GetTriggerUnit(), GetSpellAbilityId(), GetSpellTargetX(), GetSpellTargetY())",
    },
    SubType {
        tag: "OnSummon",
        event: SubEvent::PlayerUnit("EVENT_PLAYER_UNIT_SUMMON"),
        dispatch_call: "glass_dispatch_2_unit_unit(closure_id, GetTriggerUnit(), GetSummonedUnit())",
    },
    SubType {
        tag: "OnUnitSold",
        event: SubEvent::PlayerUnit("EVENT_PLAYER_UNIT_SELL"),
        dispatch_call: "glass_dispatch_2_unit_unit(closure_id, GetTriggerUnit(), GetSoldUnit())",
    },
    SubType {
        tag: "OnItemSold",
        event: SubEvent::PlayerUnit("EVENT_PLAYER_UNIT_SELL_ITEM"),
        dispatch_call: "glass_dispatch_2_unit_integer(closure_id, GetTriggerUnit(), GetItemTypeId(GetSoldItem()))",
    },
    SubType {
        tag: "OnUnitTrained",
        event: SubEvent::PlayerUnit("EVENT_PLAYER_UNIT_TRAIN_FINISH"),
        dispatch_call: "glass_dispatch_2_unit_unit(closure_id, GetTriggerUnit(), GetTrainedUnit())",
    },
    SubType {
        tag: "OnResearchFinish",
        event: SubEvent::PlayerUnit("EVENT_PLAYER_UNIT_RESEARCH_FINISH"),
        dispatch_call: "glass_dispatch_2_unit_integer(closure_id, GetTriggerUnit(), GetResearched())",
    },
    SubType {
        tag: "OnConstructionStart",
        event: SubEvent::PlayerUnit("EVENT_PLAYER_UNIT_CONSTRUCT_START"),
        dispatch_call: "glass_dispatch_1_unit(closure_id, GetTriggerUnit())",
    },
    SubType {
        tag: "OnSpellFinish",
        event: SubEvent::PlayerUnit("EVENT_PLAYER_UNIT_SPELL_FINISH"),
        dispatch_call: "glass_dispatch_2_unit_integer(closure_id, GetTriggerUnit(), GetSpellAbilityId())",
    },
    SubType {
        tag: "OnOrderIssued",
        event: SubEvent::PlayerUnit("EVENT_PLAYER_UNIT_ISSUED_ORDER"),
        dispatch_call: "glass_dispatch_2_unit_integer(closure_id, GetTriggerUnit(), GetIssuedOrderId())",
    },
];

fn gen_sub_callbacks(output: &mut String) {
    for sub in SUB_TYPES {
        output.push_str(&format!(
            "function glass_sub_cb_{} takes nothing returns nothing\n",
            sub.tag
        ));
        output.push_str("    local integer closure_id = LoadInteger(glass_timer_ht, GetHandleId(GetTriggeringTrigger()), 0)\n");
        output.push_str(&format!(
            "    call glass_send_msg({}, 0, 0)\n",
            sub.dispatch_call
        ));
        output.push_str("endfunction\n\n");
    }
}

fn gen_sub_timer_callback(output: &mut String) {
    output.push_str("function glass_sub_timer_cb takes nothing returns nothing\n");
    output.push_str("    local timer t = GetExpiredTimer()\n");
    output.push_str(
        "    local integer closure_id = LoadInteger(glass_timer_ht, GetHandleId(t), 0)\n",
    );
    output.push_str("    call glass_send_msg(glass_dispatch_void(closure_id), 0, 0)\n");
    output.push_str("    set t = null\n");
    output.push_str("endfunction\n\n");
}

fn gen_register_one_sub(output: &mut String) {
    gen_sub_timer_callback(output);

    output.push_str(
        "function glass_register_one_sub takes integer sub_id, integer idx returns nothing\n",
    );
    output.push_str("    local integer sub_tag = glass_Subscription_tag[sub_id]\n");
    output.push_str("    local trigger t = null\n");
    output.push_str("    local timer tm = null\n");
    output.push_str("    local integer i = 0\n");

    let mut first = true;
    for sub in SUB_TYPES {
        let keyword = if first { "if" } else { "elseif" };
        first = false;
        output.push_str(&format!(
            "    {} sub_tag == glass_TAG_Subscription_{} then\n",
            keyword, sub.tag
        ));

        match sub.event {
            SubEvent::Timer => {
                output.push_str("        set tm = CreateTimer()\n");
                output.push_str(&format!(
                    "        call SaveInteger(glass_timer_ht, GetHandleId(tm), 0, glass_Subscription_{}_handler[sub_id])\n",
                    sub.tag
                ));
                output.push_str(&format!(
                    "        call TimerStart(tm, glass_Subscription_{}_interval[sub_id], true, function glass_sub_timer_cb)\n",
                    sub.tag
                ));
                output.push_str("        set glass_sub_timers[idx] = tm\n");
                output.push_str("        set tm = null\n");
            }
            SubEvent::PlayerUnit(event) => {
                output.push_str("        set t = CreateTrigger()\n");
                output.push_str("        set i = 0\n");
                output.push_str("        loop\n");
                output.push_str("            exitwhen i >= bj_MAX_PLAYER_SLOTS\n");
                output.push_str(&format!(
                    "            call TriggerRegisterPlayerUnitEvent(t, Player(i), {}, null)\n",
                    event
                ));
                output.push_str("            set i = i + 1\n");
                output.push_str("        endloop\n");
                output.push_str(&format!(
                    "        call SaveInteger(glass_timer_ht, GetHandleId(t), 0, glass_Subscription_{}_handler[sub_id])\n",
                    sub.tag
                ));
                output.push_str(&format!(
                    "        call TriggerAddAction(t, function glass_sub_cb_{})\n",
                    sub.tag
                ));
                output.push_str("        set glass_sub_triggers[idx] = t\n");
                output.push_str("        set t = null\n");
            }
            SubEvent::Player(event) => {
                output.push_str("        set t = CreateTrigger()\n");
                output.push_str("        set i = 0\n");
                output.push_str("        loop\n");
                output.push_str("            exitwhen i >= bj_MAX_PLAYER_SLOTS\n");
                output.push_str(&format!(
                    "            call TriggerRegisterPlayerEvent(t, Player(i), {})\n",
                    event
                ));
                output.push_str("            set i = i + 1\n");
                output.push_str("        endloop\n");
                output.push_str(&format!(
                    "        call SaveInteger(glass_timer_ht, GetHandleId(t), 0, glass_Subscription_{}_handler[sub_id])\n",
                    sub.tag
                ));
                output.push_str(&format!(
                    "        call TriggerAddAction(t, function glass_sub_cb_{})\n",
                    sub.tag
                ));
                output.push_str("        set glass_sub_triggers[idx] = t\n");
                output.push_str("        set t = null\n");
            }
            SubEvent::Chat => {
                output.push_str("        set t = CreateTrigger()\n");
                output.push_str("        set i = 0\n");
                output.push_str("        loop\n");
                output.push_str("            exitwhen i >= bj_MAX_PLAYER_SLOTS\n");
                output.push_str(
                    "            call TriggerRegisterPlayerChatEvent(t, Player(i), \"\", false)\n",
                );
                output.push_str("            set i = i + 1\n");
                output.push_str("        endloop\n");
                output.push_str(&format!(
                    "        call SaveInteger(glass_timer_ht, GetHandleId(t), 0, glass_Subscription_{}_handler[sub_id])\n",
                    sub.tag
                ));
                output.push_str(&format!(
                    "        call TriggerAddAction(t, function glass_sub_cb_{})\n",
                    sub.tag
                ));
                output.push_str("        set glass_sub_triggers[idx] = t\n");
                output.push_str("        set t = null\n");
            }
        }
    }

    output.push_str("    endif\n");
    output.push_str("    set glass_sub_tags[idx] = sub_tag\n");
    output.push_str("endfunction\n\n");
}

fn gen_unregister_one_sub(output: &mut String) {
    output.push_str("function glass_unregister_one_sub takes integer idx returns nothing\n");
    output.push_str("    if glass_sub_timers[idx] != null then\n");
    output.push_str("        call PauseTimer(glass_sub_timers[idx])\n");
    output.push_str(
        "        call FlushChildHashtable(glass_timer_ht, GetHandleId(glass_sub_timers[idx]))\n",
    );
    output.push_str("        call DestroyTimer(glass_sub_timers[idx])\n");
    output.push_str("        set glass_sub_timers[idx] = null\n");
    output.push_str("    endif\n");
    output.push_str("    if glass_sub_triggers[idx] != null then\n");
    output.push_str("        call DisableTrigger(glass_sub_triggers[idx])\n");
    output.push_str(
        "        call FlushChildHashtable(glass_timer_ht, GetHandleId(glass_sub_triggers[idx]))\n",
    );
    output.push_str("        call DestroyTrigger(glass_sub_triggers[idx])\n");
    output.push_str("        set glass_sub_triggers[idx] = null\n");
    output.push_str("    endif\n");
    output.push_str("    set glass_sub_tags[idx] = 0\n");
    output.push_str("endfunction\n\n");
}

fn gen_reconcile_subs(output: &mut String) {
    output.push_str("function glass_reconcile_subs takes nothing returns nothing\n");
    output.push_str("    local integer new_list = glass_subscriptions(glass_model)\n");
    output.push_str("    local integer current = new_list\n");
    output.push_str("    local integer new_count = 0\n");
    output.push_str("    local integer idx = 0\n");
    output.push_str("    local integer new_tag = 0\n");
    output.push_str("    local integer new_sub_id = 0\n");
    output.push_str("    loop\n");
    output.push_str("        exitwhen current == -1\n");
    output.push_str("        set new_count = new_count + 1\n");
    output.push_str("        set current = glass_List_integer_tail[current]\n");
    output.push_str("    endloop\n");
    output.push_str("    set idx = new_count\n");
    output.push_str("    loop\n");
    output.push_str("        exitwhen idx >= glass_sub_count\n");
    output.push_str("        call glass_unregister_one_sub(idx)\n");
    output.push_str("        set idx = idx + 1\n");
    output.push_str("    endloop\n");
    output.push_str("    set current = new_list\n");
    output.push_str("    set idx = 0\n");
    output.push_str("    loop\n");
    output.push_str("        exitwhen current == -1\n");
    output.push_str("        set new_sub_id = glass_List_integer_head[current]\n");
    output.push_str("        set new_tag = glass_Subscription_tag[new_sub_id]\n");
    output.push_str("        if idx >= glass_sub_count then\n");
    output.push_str("            call glass_register_one_sub(new_sub_id, idx)\n");
    output.push_str("        elseif glass_sub_tags[idx] != new_tag then\n");
    output.push_str("            call glass_unregister_one_sub(idx)\n");
    output.push_str("            call glass_register_one_sub(new_sub_id, idx)\n");
    output.push_str("        endif\n");
    output.push_str("        set idx = idx + 1\n");
    output.push_str("        set current = glass_List_integer_tail[current]\n");
    output.push_str("    endloop\n");
    output.push_str("    set glass_sub_count = new_count\n");
    output.push_str("endfunction\n\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use crate::token::Lexer;

    fn detect_entry_points(source: &str) -> Option<ElmEntryPoints> {
        let tokens = Lexer::tokenize(source).expect("lex failed");
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module().expect("parse failed");
        let types = TypeRegistry::from_module(&module);
        ElmEntryPoints::detect(&module, &types)
    }

    #[test]
    fn detects_elm_app() {
        let entry = detect_entry_points(
            r#"
pub enum Msg { Tick GameStart }
pub fn init() -> Int { 0 }
pub fn update(model: Int, msg: Msg) -> Int { model }
"#,
        );
        let entry = entry.expect("should detect entry points");
        assert!(entry.has_init);
        assert!(entry.has_update);
        assert!(!entry.has_subscriptions);
        assert_eq!(entry.msg_variants.len(), 2);
        assert_eq!(entry.msg_variants[0].0, "Tick");
        assert_eq!(entry.msg_variants[1].0, "GameStart");
    }

    #[test]
    fn no_elm_without_init() {
        let entry = detect_entry_points("fn update(model: Int) -> Int { model }");
        assert!(entry.is_none());
    }

    #[test]
    fn no_elm_without_pub() {
        let entry = detect_entry_points(
            r#"
fn init() -> Int { 0 }
fn update(model: Int) -> Int { model }
"#,
        );
        assert!(entry.is_none());
    }
}
