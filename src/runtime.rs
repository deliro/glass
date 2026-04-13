// Elm Architecture runtime code generation for JASS.
//
// The runtime manages:
// - Global model state
// - Message dispatch (glass_send_msg)
// - Effect processing queue
// - Map initialization trigger

use std::collections::HashSet;

use crate::ast::{Definition, Module};
use crate::types::{FieldInfo, TypeRegistry};

#[derive(Debug, Clone)]
pub struct EffectVariantDef {
    pub name: String,
    #[allow(dead_code)]
    pub tag: i64,
    pub fields: Vec<FieldInfo>,
    pub has_exec_fn: bool,
}

impl EffectVariantDef {
    pub fn has_callback_fields(&self) -> bool {
        self.fields.iter().any(|f| f.is_callback)
    }

    pub fn non_callback_fields(&self) -> Vec<&FieldInfo> {
        self.fields.iter().filter(|f| !f.is_callback).collect()
    }

    pub fn callback_fields(&self) -> Vec<&FieldInfo> {
        self.fields.iter().filter(|f| f.is_callback).collect()
    }
}

pub fn to_snake_case(name: &str) -> String {
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

/// Detected Elm architecture entry points.
#[allow(dead_code)]
pub struct ElmEntryPoints {
    pub has_init: bool,
    pub has_update: bool,
    pub has_subscriptions: bool,
    pub msg_variants: Vec<(String, i64, usize)>,
    pub effect_variants: Vec<EffectVariantDef>,
}

impl ElmEntryPoints {
    pub fn detect(module: &Module, types: &TypeRegistry) -> Option<Self> {
        let mut has_init = false;
        let mut has_update = false;
        let mut has_subscriptions = false;
        let mut exec_fn_names: HashSet<String> = HashSet::new();
        for def in &module.definitions {
            if let Definition::Function(f) = def {
                match f.name.as_str() {
                    "init" if f.is_pub => has_init = true,
                    "update" if f.is_pub => has_update = true,
                    "subscriptions" if f.is_pub => has_subscriptions = true,
                    _ => {}
                }
                if f.name.starts_with("exec_") {
                    exec_fn_names.insert(f.name.clone());
                }
            }
        }

        if !has_init || !has_update {
            return None;
        }

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

        let effect_variants = types
            .types
            .get("Effect")
            .map(|info| {
                info.variants
                    .iter()
                    .map(|v| {
                        let snake = to_snake_case(&v.name);
                        let exec_name = format!("exec_{}", snake);
                        EffectVariantDef {
                            name: v.name.clone(),
                            tag: v.tag,
                            fields: v.fields.clone(),
                            has_exec_fn: exec_fn_names.contains(&exec_name),
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Some(ElmEntryPoints {
            has_init,
            has_update,
            has_subscriptions,
            msg_variants,
            effect_variants,
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
    to_snake_case(name)
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
    gen_timer_callback(entry, output);
    gen_exec_effect(entry, output);
    gen_process_effects(output);
    gen_send_msg(entry, output);
    if entry.has_subscriptions {
        gen_sub_timer_callback(output);
        gen_subscription_callbacks(dispatch_sigs, output);
        gen_sub_callbacks(dispatch_sigs, output);
        gen_unregister_one_sub(output);
        gen_register_one_sub(dispatch_sigs, output);
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
                "    call glass_send_msg({}({}))\n",
                sub.dispatch, global
            ));
        } else {
            output.push_str(&format!(
                "    call glass_send_msg({}({}, {}))\n",
                sub.dispatch, global, sub.event_args
            ));
        }

        output.push_str("endfunction\n\n");
    }
}

fn jass_field_access(variant_name: &str, field_name: &str) -> String {
    format!("glass_Effect_{}_{}", variant_name, field_name)
}

fn gen_jass_exec_call(variant: &EffectVariantDef, indent: &str, output: &mut String) {
    let snake = to_snake_case(&variant.name);
    let non_cb: Vec<&FieldInfo> = variant.non_callback_fields();
    let args: Vec<String> = non_cb
        .iter()
        .map(|f| format!("{}[fx_id]", jass_field_access(&variant.name, &f.name)))
        .collect();
    output.push_str(&format!(
        "{}call glass_exec_{}({})\n",
        indent,
        snake,
        args.join(", ")
    ));
}

fn gen_jass_after_effect(variant_name: &str, indent: &str, output: &mut String) {
    output.push_str(&format!("{}set t = CreateTimer()\n", indent));
    output.push_str(&format!(
        "{}call SaveInteger(glass_timer_ht, GetHandleId(t), 0, {}[fx_id])\n",
        indent,
        jass_field_access(variant_name, "callback")
    ));
    output.push_str(&format!(
        "{}call TimerStart(t, {}[fx_id], false, function glass_timer_callback)\n",
        indent,
        jass_field_access(variant_name, "duration")
    ));
    output.push_str(&format!("{}set t = null\n", indent));
}

fn gen_jass_callback_unit_effect(variant: &EffectVariantDef, indent: &str, output: &mut String) {
    let non_cb = variant.non_callback_fields();
    let cb_fields = variant.callback_fields();
    let cb_field = match cb_fields.first() {
        Some(f) => f,
        None => return,
    };

    output.push_str(&format!("{}set t = CreateTimer()\n", indent));

    let has_unit_result = non_cb.iter().any(|f| f.jass_type == "unit");
    if has_unit_result {
        let create_args: Vec<String> = non_cb
            .iter()
            .map(|f| {
                let access = format!("{}[fx_id]", jass_field_access(&variant.name, &f.name));
                if f.name == "owner" {
                    format!("Player({})", access)
                } else {
                    access
                }
            })
            .collect();
        output.push_str(&format!(
            "{}set u = CreateUnit({})\n",
            indent,
            create_args.join(", ")
        ));
    }

    output.push_str(&format!(
        "{}call SaveInteger(glass_timer_ht, GetHandleId(t), 0, {}[fx_id])\n",
        indent,
        jass_field_access(&variant.name, &cb_field.name)
    ));

    if has_unit_result {
        output.push_str(&format!(
            "{}call SaveUnitHandle(glass_timer_ht, GetHandleId(t), 1, u)\n",
            indent
        ));
        output.push_str(&format!(
            "{}call SaveInteger(glass_timer_ht, GetHandleId(t), 2, 1)\n",
            indent
        ));
    }

    output.push_str(&format!(
        "{}call TimerStart(t, 0.0, false, function glass_timer_callback)\n",
        indent
    ));

    if has_unit_result {
        output.push_str(&format!("{}set u = null\n", indent));
    }
    output.push_str(&format!("{}set t = null\n", indent));
}

fn gen_jass_find_nearest_enemy(variant_name: &str, indent: &str, output: &mut String) {
    output.push_str(&format!("{}set glass_group_temp = CreateGroup()\n", indent));
    output.push_str(&format!(
        "{}call GroupEnumUnitsInRange(glass_group_temp, {}[fx_id], {}[fx_id], {}[fx_id], null)\n",
        indent,
        jass_field_access(variant_name, "x"),
        jass_field_access(variant_name, "y"),
        jass_field_access(variant_name, "radius"),
    ));
    output.push_str(&format!(
        "{}set u = FirstOfGroup(glass_group_temp)\n",
        indent
    ));
    output.push_str(&format!("{}call DestroyGroup(glass_group_temp)\n", indent));
    output.push_str(&format!("{}set glass_group_temp = null\n", indent));
    output.push_str(&format!("{}if u != null then\n", indent));
    output.push_str(&format!("{}    set t = CreateTimer()\n", indent));
    output.push_str(&format!(
        "{}    call SaveInteger(glass_timer_ht, GetHandleId(t), 0, {}[fx_id])\n",
        indent,
        jass_field_access(variant_name, "callback"),
    ));
    output.push_str(&format!(
        "{}    call SaveUnitHandle(glass_timer_ht, GetHandleId(t), 1, u)\n",
        indent
    ));
    output.push_str(&format!(
        "{}    call SaveInteger(glass_timer_ht, GetHandleId(t), 2, 1)\n",
        indent
    ));
    output.push_str(&format!(
        "{}    call TimerStart(t, 0.0, false, function glass_timer_callback)\n",
        indent
    ));
    output.push_str(&format!("{}    set t = null\n", indent));
    output.push_str(&format!("{}endif\n", indent));
    output.push_str(&format!("{}set u = null\n", indent));
}

fn gen_jass_for_units_in_range(variant_name: &str, indent: &str, output: &mut String) {
    output.push_str(&format!("{}set glass_group_temp = CreateGroup()\n", indent));
    output.push_str(&format!(
        "{}call GroupEnumUnitsInRange(glass_group_temp, {}[fx_id], {}[fx_id], {}[fx_id], null)\n",
        indent,
        jass_field_access(variant_name, "x"),
        jass_field_access(variant_name, "y"),
        jass_field_access(variant_name, "radius"),
    ));
    output.push_str(&format!("{}loop\n", indent));
    output.push_str(&format!(
        "{}    set u = FirstOfGroup(glass_group_temp)\n",
        indent
    ));
    output.push_str(&format!("{}    exitwhen u == null\n", indent));
    output.push_str(&format!("{}    set t = CreateTimer()\n", indent));
    output.push_str(&format!(
        "{}    call SaveInteger(glass_timer_ht, GetHandleId(t), 0, {}[fx_id])\n",
        indent,
        jass_field_access(variant_name, "callback"),
    ));
    output.push_str(&format!(
        "{}    call SaveUnitHandle(glass_timer_ht, GetHandleId(t), 1, u)\n",
        indent
    ));
    output.push_str(&format!(
        "{}    call SaveInteger(glass_timer_ht, GetHandleId(t), 2, 1)\n",
        indent
    ));
    output.push_str(&format!(
        "{}    call TimerStart(t, 0.0, false, function glass_timer_callback)\n",
        indent
    ));
    output.push_str(&format!(
        "{}    call GroupRemoveUnit(glass_group_temp, u)\n",
        indent
    ));
    output.push_str(&format!("{}    set t = null\n", indent));
    output.push_str(&format!("{}endloop\n", indent));
    output.push_str(&format!("{}call DestroyGroup(glass_group_temp)\n", indent));
    output.push_str(&format!("{}set glass_group_temp = null\n", indent));
}

fn gen_jass_update_board(variant_name: &str, indent: &str, output: &mut String) {
    output.push_str(&format!("{}set row_count = 0\n", indent));
    output.push_str(&format!(
        "{}set row_cur = {}[fx_id]\n",
        indent,
        jass_field_access(variant_name, "rows")
    ));
    output.push_str(&format!("{}loop\n", indent));
    output.push_str(&format!("{}    exitwhen row_cur == -1\n", indent));
    output.push_str(&format!("{}    set row_count = row_count + 1\n", indent));
    output.push_str(&format!(
        "{}    set row_cur = glass_List_integer_tail[row_cur]\n",
        indent
    ));
    output.push_str(&format!("{}endloop\n", indent));
    output.push_str(&format!("{}if glass_multiboard == null then\n", indent));
    output.push_str(&format!(
        "{}    set glass_multiboard = CreateMultiboard()\n",
        indent
    ));
    output.push_str(&format!("{}endif\n", indent));
    output.push_str(&format!(
        "{}call MultiboardSetColumnCount(glass_multiboard, 2)\n",
        indent
    ));
    output.push_str(&format!(
        "{}call MultiboardSetRowCount(glass_multiboard, row_count)\n",
        indent
    ));
    output.push_str(&format!(
        "{}call MultiboardSetTitleText(glass_multiboard, {}[fx_id])\n",
        indent,
        jass_field_access(variant_name, "title")
    ));
    output.push_str(&format!(
        "{}call MultiboardDisplay(glass_multiboard, true)\n",
        indent
    ));
    output.push_str(&format!(
        "{}set row_cur = {}[fx_id]\n",
        indent,
        jass_field_access(variant_name, "rows")
    ));
    output.push_str(&format!("{}set ri = 0\n", indent));
    output.push_str(&format!("{}loop\n", indent));
    output.push_str(&format!("{}    exitwhen row_cur == -1\n", indent));
    output.push_str(&format!(
        "{}    set row_data = glass_List_integer_head[row_cur]\n",
        indent
    ));
    output.push_str(&format!(
        "{}    set mbi = MultiboardGetItem(glass_multiboard, ri, 0)\n",
        indent
    ));
    output.push_str(&format!(
        "{}    call MultiboardSetItemValue(mbi, glass_BoardRow_label[row_data])\n",
        indent
    ));
    output.push_str(&format!(
        "{}    call MultiboardSetItemWidth(mbi, 0.10)\n",
        indent
    ));
    output.push_str(&format!("{}    call MultiboardReleaseItem(mbi)\n", indent));
    output.push_str(&format!(
        "{}    set mbi = MultiboardGetItem(glass_multiboard, ri, 1)\n",
        indent
    ));
    output.push_str(&format!(
        "{}    call MultiboardSetItemValue(mbi, glass_BoardRow_value[row_data])\n",
        indent
    ));
    output.push_str(&format!(
        "{}    call MultiboardSetItemWidth(mbi, 0.08)\n",
        indent
    ));
    output.push_str(&format!("{}    call MultiboardReleaseItem(mbi)\n", indent));
    output.push_str(&format!("{}    set ri = ri + 1\n", indent));
    output.push_str(&format!(
        "{}    set row_cur = glass_List_integer_tail[row_cur]\n",
        indent
    ));
    output.push_str(&format!("{}endloop\n", indent));
    output.push_str(&format!("{}set mbi = null\n", indent));
}

fn gen_jass_create_unit_callback(variant_name: &str, indent: &str, output: &mut String) {
    output.push_str(&format!("{}set t = CreateTimer()\n", indent));
    output.push_str(&format!(
        "{}set u = CreateUnit(Player({}[fx_id]), {}[fx_id], {}[fx_id], {}[fx_id], {}[fx_id])\n",
        indent,
        jass_field_access(variant_name, "owner"),
        jass_field_access(variant_name, "type_id"),
        jass_field_access(variant_name, "x"),
        jass_field_access(variant_name, "y"),
        jass_field_access(variant_name, "facing"),
    ));
    output.push_str(&format!(
        "{}call SaveInteger(glass_timer_ht, GetHandleId(t), 0, {}[fx_id])\n",
        indent,
        jass_field_access(variant_name, "callback")
    ));
    output.push_str(&format!(
        "{}call SaveUnitHandle(glass_timer_ht, GetHandleId(t), 1, u)\n",
        indent
    ));
    output.push_str(&format!(
        "{}call SaveInteger(glass_timer_ht, GetHandleId(t), 2, 1)\n",
        indent
    ));
    output.push_str(&format!(
        "{}call TimerStart(t, 0.0, false, function glass_timer_callback)\n",
        indent
    ));
    output.push_str(&format!("{}set u = null\n", indent));
    output.push_str(&format!("{}set t = null\n", indent));
}

fn gen_jass_after_then_effect(variant_name: &str, indent: &str, output: &mut String) {
    output.push_str(&format!("{}set t = CreateTimer()\n", indent));
    output.push_str(&format!(
        "{}call SaveInteger(glass_timer_ht, GetHandleId(t), 0, {}[fx_id])\n",
        indent,
        jass_field_access(variant_name, "chain")
    ));
    output.push_str(&format!(
        "{}call SaveInteger(glass_timer_ht, GetHandleId(t), 2, 2)\n",
        indent
    ));
    output.push_str(&format!(
        "{}call TimerStart(t, {}[fx_id], false, function glass_timer_callback)\n",
        indent,
        jass_field_access(variant_name, "duration")
    ));
    output.push_str(&format!("{}set t = null\n", indent));
}

fn gen_jass_create_unit_then_effect(variant_name: &str, indent: &str, output: &mut String) {
    output.push_str(&format!("{}set t = CreateTimer()\n", indent));
    output.push_str(&format!(
        "{}set u = CreateUnit(Player({}[fx_id]), {}[fx_id], {}[fx_id], {}[fx_id], {}[fx_id])\n",
        indent,
        jass_field_access(variant_name, "owner"),
        jass_field_access(variant_name, "type_id"),
        jass_field_access(variant_name, "x"),
        jass_field_access(variant_name, "y"),
        jass_field_access(variant_name, "facing"),
    ));
    output.push_str(&format!(
        "{}call SaveInteger(glass_timer_ht, GetHandleId(t), 0, {}[fx_id])\n",
        indent,
        jass_field_access(variant_name, "chain")
    ));
    output.push_str(&format!(
        "{}call SaveUnitHandle(glass_timer_ht, GetHandleId(t), 1, u)\n",
        indent
    ));
    output.push_str(&format!(
        "{}call SaveInteger(glass_timer_ht, GetHandleId(t), 2, 3)\n",
        indent
    ));
    output.push_str(&format!(
        "{}call TimerStart(t, 0.0, false, function glass_timer_callback)\n",
        indent
    ));
    output.push_str(&format!("{}set u = null\n", indent));
    output.push_str(&format!("{}set t = null\n", indent));
}

fn gen_jass_find_nearest_enemy_then(variant_name: &str, indent: &str, output: &mut String) {
    output.push_str(&format!("{}set glass_group_temp = CreateGroup()\n", indent));
    output.push_str(&format!(
        "{}call GroupEnumUnitsInRange(glass_group_temp, {}[fx_id], {}[fx_id], {}[fx_id], null)\n",
        indent,
        jass_field_access(variant_name, "x"),
        jass_field_access(variant_name, "y"),
        jass_field_access(variant_name, "radius"),
    ));
    output.push_str(&format!(
        "{}set u = FirstOfGroup(glass_group_temp)\n",
        indent
    ));
    output.push_str(&format!("{}call DestroyGroup(glass_group_temp)\n", indent));
    output.push_str(&format!("{}set glass_group_temp = null\n", indent));
    output.push_str(&format!("{}if u != null then\n", indent));
    output.push_str(&format!("{}    set t = CreateTimer()\n", indent));
    output.push_str(&format!(
        "{}    call SaveInteger(glass_timer_ht, GetHandleId(t), 0, {}[fx_id])\n",
        indent,
        jass_field_access(variant_name, "chain"),
    ));
    output.push_str(&format!(
        "{}    call SaveUnitHandle(glass_timer_ht, GetHandleId(t), 1, u)\n",
        indent
    ));
    output.push_str(&format!(
        "{}    call SaveInteger(glass_timer_ht, GetHandleId(t), 2, 3)\n",
        indent
    ));
    output.push_str(&format!(
        "{}    call TimerStart(t, 0.0, false, function glass_timer_callback)\n",
        indent
    ));
    output.push_str(&format!("{}    set t = null\n", indent));
    output.push_str(&format!("{}endif\n", indent));
    output.push_str(&format!("{}set u = null\n", indent));
}

fn gen_jass_for_units_in_range_then_effect(variant_name: &str, indent: &str, output: &mut String) {
    output.push_str(&format!("{}set glass_group_temp = CreateGroup()\n", indent));
    output.push_str(&format!(
        "{}call GroupEnumUnitsInRange(glass_group_temp, {}[fx_id], {}[fx_id], {}[fx_id], null)\n",
        indent,
        jass_field_access(variant_name, "x"),
        jass_field_access(variant_name, "y"),
        jass_field_access(variant_name, "radius"),
    ));
    output.push_str(&format!("{}loop\n", indent));
    output.push_str(&format!(
        "{}    set u = FirstOfGroup(glass_group_temp)\n",
        indent
    ));
    output.push_str(&format!("{}    exitwhen u == null\n", indent));
    output.push_str(&format!("{}    set t = CreateTimer()\n", indent));
    output.push_str(&format!(
        "{}    call SaveInteger(glass_timer_ht, GetHandleId(t), 0, {}[fx_id])\n",
        indent,
        jass_field_access(variant_name, "chain"),
    ));
    output.push_str(&format!(
        "{}    call SaveUnitHandle(glass_timer_ht, GetHandleId(t), 1, u)\n",
        indent
    ));
    output.push_str(&format!(
        "{}    call SaveInteger(glass_timer_ht, GetHandleId(t), 2, 3)\n",
        indent
    ));
    output.push_str(&format!(
        "{}    call TimerStart(t, 0.0, false, function glass_timer_callback)\n",
        indent
    ));
    output.push_str(&format!(
        "{}    call GroupRemoveUnit(glass_group_temp, u)\n",
        indent
    ));
    output.push_str(&format!("{}    set t = null\n", indent));
    output.push_str(&format!("{}endloop\n", indent));
    output.push_str(&format!("{}call DestroyGroup(glass_group_temp)\n", indent));
    output.push_str(&format!("{}set glass_group_temp = null\n", indent));
}

fn gen_jass_effect_variant_body(variant: &EffectVariantDef, indent: &str, output: &mut String) {
    match variant.name.as_str() {
        "After" => gen_jass_after_effect(&variant.name, indent, output),
        "AfterThen" => gen_jass_after_then_effect(&variant.name, indent, output),
        "FindNearestEnemy" => gen_jass_find_nearest_enemy(&variant.name, indent, output),
        "FindNearestEnemyThen" => {
            gen_jass_find_nearest_enemy_then(&variant.name, indent, output);
        }
        "CreateUnitCallback" => gen_jass_create_unit_callback(&variant.name, indent, output),
        "CreateUnitThen" => gen_jass_create_unit_then_effect(&variant.name, indent, output),
        "ForUnitsInRange" => gen_jass_for_units_in_range(&variant.name, indent, output),
        "ForUnitsInRangeThen" => {
            gen_jass_for_units_in_range_then_effect(&variant.name, indent, output);
        }
        "UpdateBoard" => gen_jass_update_board(&variant.name, indent, output),
        _ if variant.has_exec_fn => gen_jass_exec_call(variant, indent, output),
        _ if variant.has_callback_fields() => {
            gen_jass_callback_unit_effect(variant, indent, output);
        }
        _ => {}
    }
}

fn gen_exec_effect(entry: &ElmEntryPoints, output: &mut String) {
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

    let mut first = true;
    for variant in &entry.effect_variants {
        let keyword = if first { "if" } else { "elseif" };
        first = false;
        output.push_str(&format!(
            "    {} fx_tag == glass_TAG_Effect_{} then\n",
            keyword, variant.name
        ));
        gen_jass_effect_variant_body(variant, "        ", output);
    }

    if !entry.effect_variants.is_empty() {
        output.push_str("    endif\n");
    }
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
fn gen_timer_callback(entry: &ElmEntryPoints, output: &mut String) {
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
    output.push_str("    if cb_type == 1 then\n");
    output.push_str("        set msg_result = glass_dispatch_1_unit(closure_id, cb_unit)\n");
    output.push_str("    elseif cb_type == 2 then\n");
    output.push_str("        set current = glass_dispatch_void(closure_id)\n");
    output.push_str("    elseif cb_type == 3 then\n");
    output.push_str("        set current = glass_dispatch_1_unit(closure_id, cb_unit)\n");
    output.push_str("    else\n");
    output.push_str("        set msg_result = glass_dispatch_void(closure_id)\n");
    output.push_str("    endif\n");
    output.push_str("    call FlushChildHashtable(glass_timer_ht, hid)\n");
    output.push_str("    call DestroyTimer(t)\n");
    output.push_str("    set t = null\n");
    output.push_str("    set cb_unit = null\n");
    output.push_str("    if cb_type < 2 then\n");
    output.push_str("        set glass_msg_tag = msg_result\n");
    output.push_str("        set glass_result = glass_dispatch_update()\n");
    output.push_str("        set glass_model = glass_rt_tuple_0(glass_result)\n");
    output.push_str("        set glass_effects = glass_rt_tuple_1(glass_result)\n");
    output.push_str("        set current = glass_effects\n");
    output.push_str("    endif\n");
    output.push_str("    loop\n");
    output.push_str("        exitwhen current == -1\n");
    output.push_str("        set fx_id = glass_List_integer_head[current]\n");
    output.push_str("        set fx_tag = glass_Effect_tag[fx_id]\n");

    let mut first = true;
    for variant in &entry.effect_variants {
        let keyword = if first { "if" } else { "elseif" };
        first = false;
        output.push_str(&format!(
            "        {} fx_tag == glass_TAG_Effect_{} then\n",
            keyword, variant.name
        ));
        gen_jass_effect_variant_body(variant, "            ", output);
    }

    if !entry.effect_variants.is_empty() {
        output.push_str("        endif\n");
    }
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
    output.push_str("function glass_send_msg takes integer msg returns nothing\n");
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
        output.push_str("    call ExecuteFunc(\"glass_reconcile_subs\")\n");
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

fn gen_sub_callbacks(dispatch_sigs: &HashSet<String>, output: &mut String) {
    for sub in SUB_TYPES {
        let dispatch_name = sub.dispatch_call.split('(').next().unwrap_or("");
        if !dispatch_sigs.contains(dispatch_name) {
            continue;
        }
        output.push_str(&format!(
            "function glass_sub_cb_{} takes nothing returns nothing\n",
            sub.tag
        ));
        output.push_str("    local integer closure_id = LoadInteger(glass_timer_ht, GetHandleId(GetTriggeringTrigger()), 0)\n");
        output.push_str(&format!("    call glass_send_msg({})\n", sub.dispatch_call));
        output.push_str("endfunction\n\n");
    }
}

fn gen_sub_timer_callback(output: &mut String) {
    output.push_str("function glass_sub_timer_cb takes nothing returns nothing\n");
    output.push_str("    local timer t = GetExpiredTimer()\n");
    output.push_str(
        "    local integer closure_id = LoadInteger(glass_timer_ht, GetHandleId(t), 0)\n",
    );
    output.push_str("    call glass_send_msg(glass_dispatch_void(closure_id))\n");
    output.push_str("    set t = null\n");
    output.push_str("endfunction\n\n");
}

fn gen_register_one_sub(dispatch_sigs: &HashSet<String>, output: &mut String) {
    output.push_str(
        "function glass_register_one_sub takes integer sub_id, integer idx returns nothing\n",
    );
    output.push_str("    local integer sub_tag = glass_Subscription_tag[sub_id]\n");
    output.push_str("    local trigger t = null\n");
    output.push_str("    local timer tm = null\n");
    output.push_str("    local integer i = 0\n");

    let mut first = true;
    for sub in SUB_TYPES {
        let dispatch_name = sub.dispatch_call.split('(').next().unwrap_or("");
        if !dispatch_sigs.contains(dispatch_name) {
            continue;
        }
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
        let module = {
            let _o = parser.parse_module();
            assert!(_o.errors.is_empty(), "parse errors: {:?}", _o.errors);
            _o.module
        };
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
