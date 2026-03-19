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

    // Runtime init
    gen_runtime_init(entry, output);

    // Map init trigger
    gen_map_init(output);
}

fn gen_exec_effect(output: &mut String) {
    output.push_str("function glass_exec_effect(fx)\n");
    output.push_str("    if fx.tag == glass_TAG_After then\n");
    output.push_str("        local t = CreateTimer()\n");
    output.push_str("        SaveInteger(glass_timer_ht, GetHandleId(t), 0, fx.callback)\n");
    output.push_str("        TimerStart(t, fx.duration, false, function()\n");
    output.push_str("            local expired = GetExpiredTimer()\n");
    output.push_str("            local cb = LoadInteger(glass_timer_ht, GetHandleId(expired), 0)\n");
    output.push_str("            FlushChildHashtable(glass_timer_ht, GetHandleId(expired))\n");
    output.push_str("            DestroyTimer(expired)\n");
    output.push_str("            local msg = cb()\n");
    output.push_str("            glass_send_msg(msg)\n");
    output.push_str("        end)\n");
    output.push_str("    elseif fx.tag == glass_TAG_DisplayText then\n");
    output.push_str("        DisplayTimedTextToPlayer(Player(fx.player_id), 0, 0, fx.duration, fx.text)\n");
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

fn gen_runtime_init(_entry: &ElmEntryPoints, output: &mut String) {
    output.push_str("function glass_runtime_init()\n");
    output.push_str("    glass_timer_ht = InitHashtable()\n");
    output.push_str("    local result = glass_init()\n");
    output.push_str("    glass_model = result[1]\n");
    output.push_str("    glass_process_effects(result[2])\n");
    output.push_str("end\n\n");
}

fn gen_map_init(output: &mut String) {
    output.push_str("do\n");
    output.push_str("    local t = CreateTrigger()\n");
    output.push_str("    TriggerRegisterTimerEvent(t, 0.00, false)\n");
    output.push_str("    TriggerAddAction(t, glass_runtime_init)\n");
    output.push_str("end\n");
}

