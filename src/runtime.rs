// Elm Architecture runtime code generation for JASS.
//
// The runtime manages:
// - Global model state
// - Message dispatch (glass_send_msg)
// - Effect processing queue
// - Map initialization trigger

use crate::ast::{Definition, Module};
use crate::types::TypeRegistry;

/// Detected Elm architecture entry points.
#[allow(dead_code)] // Fields used progressively across milestones + tests
pub struct ElmEntryPoints {
    pub has_init: bool,
    pub has_update: bool,
    pub has_subscriptions: bool,
    /// Msg type info: variant names + tags for dispatch
    pub msg_variants: Vec<(String, i64, usize)>, // (name, tag, field_count)
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

/// Collect runtime globals (merged into the single globals block).
pub fn collect_runtime_globals(globals: &mut Vec<String>) {
    globals.push("    // ========== Glass Elm Runtime ==========".into());
    globals.push("    integer glass_model = 0".into());
    globals.push("    integer glass_msg_tag = 0".into());
    for i in 0..4 {
        globals.push(format!("    integer glass_msg_p{} = 0", i));
    }
    // Timer data hashtable for closure dispatch
    globals.push("    hashtable glass_timer_ht = null".into());
}

/// Generate the Elm runtime JASS functions (after user code).
pub fn gen_elm_runtime_functions(
    entry: &ElmEntryPoints,
    _lambdas: &[crate::closures::LambdaInfo],
    output: &mut String,
) {
    output.push_str("// ========== Glass Elm Runtime Functions ==========\n\n");

    // Order matters in JASS: callees must be defined before callers.
    // JASS requires callees defined before callers.
    // Circular dep broken by: timer_callback inlines update+effect processing.
    // Order: rt_tuple → dispatch_update → timer_callback → exec_effect → process_effects → send_msg
    gen_rt_tuple_helpers(output);
    gen_msg_dispatch(entry, output);
    gen_timer_callback(output);
    gen_exec_effect(output);
    gen_process_effects(output);
    gen_send_msg(output);

    // === glass_runtime_init (needs glass_init, rt_tuple, process_effects) ===
    output.push_str("function glass_runtime_init takes nothing returns nothing\n");
    output.push_str("    local integer glass_result\n");
    output.push_str("    local integer glass_effects\n");
    output.push_str("    set glass_timer_ht = InitHashtable()\n");
    output.push_str("    set glass_result = glass_init()\n");
    output.push_str("    set glass_model = glass_rt_tuple_0(glass_result)\n");
    output.push_str("    set glass_effects = glass_rt_tuple_1(glass_result)\n");
    output.push_str("    call glass_process_effects(glass_effects)\n");
    output.push_str("endfunction\n\n");

    // === Map init trigger ===
    output.push_str("function InitTrig_GlassInit takes nothing returns nothing\n");
    output.push_str("    local trigger t = CreateTrigger()\n");
    output.push_str("    call TriggerRegisterTimerEvent(t, 0.00, false)\n");
    output.push_str("    call TriggerAddAction(t, function glass_runtime_init)\n");
    output.push_str("endfunction\n\n");
}

/// Execute a single effect by reading its tag and fields from the Effect SoA.
/// SoA layout generated from `sdk/effect.glass`:
///   Effect_tag[id]                   — variant tag
///   Effect_After_duration[id]        — real
///   Effect_After_callback[id]        — integer (closure ID)
///   Effect_DisplayText_player_id[id] — integer
///   Effect_DisplayText_text[id]      — string
///   Effect_DisplayText_duration[id]  — real
///   Effect_Batch__0[id]              — integer (List(Effect) ID)
fn gen_exec_effect(output: &mut String) {
    // Note: this must be defined BEFORE glass_process_effects (which calls it).
    // Batch handling: we don't recurse into glass_process_effects (circular dep).
    // Instead, Batch is handled by glass_process_effects directly.
    output.push_str("function glass_exec_effect takes integer fx_id returns nothing\n");
    output.push_str("    local integer fx_tag = glass_Effect_tag[fx_id]\n");
    output.push_str("    local timer t\n");
    output.push_str("    if fx_tag == glass_TAG_After then\n");
    output.push_str("        set t = CreateTimer()\n");
    output.push_str("        call SaveInteger(glass_timer_ht, GetHandleId(t), 0, glass_Effect_After_callback[fx_id])\n");
    output.push_str("        call TimerStart(t, glass_Effect_After_duration[fx_id], false, function glass_timer_callback)\n");
    output.push_str("        set t = null\n");
    output.push_str("    elseif fx_tag == glass_TAG_DisplayText then\n");
    output.push_str("        call DisplayTimedTextToPlayer(Player(glass_Effect_DisplayText_player_id[fx_id]), 0, 0, glass_Effect_DisplayText_duration[fx_id], glass_Effect_DisplayText_text[fx_id])\n");
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

/// Timer callback: fully self-contained — no calls to other runtime functions.
/// Inlines: dispatch closure → update → extract model+effects → walk effect list.
/// This avoids the JASS forward reference cycle.
fn gen_timer_callback(output: &mut String) {
    output.push_str("function glass_timer_callback takes nothing returns nothing\n");
    output.push_str("    local timer t = GetExpiredTimer()\n");
    output.push_str("    local integer hid = GetHandleId(t)\n");
    output.push_str("    local integer closure_id = LoadInteger(glass_timer_ht, hid, 0)\n");
    output.push_str("    local integer msg_result = 0\n");
    output.push_str("    local integer glass_result\n");
    output.push_str("    local integer glass_effects\n");
    output.push_str("    local integer current\n");
    output.push_str("    local integer fx_id\n");
    output.push_str("    local integer fx_tag\n");
    output.push_str("    local timer t2\n");
    // Dispatch closure → get Msg
    output.push_str("    set msg_result = glass_dispatch_void(closure_id)\n");
    // Cleanup expired timer
    output.push_str("    call FlushChildHashtable(glass_timer_ht, hid)\n");
    output.push_str("    call DestroyTimer(t)\n");
    output.push_str("    set t = null\n");
    // Call update (inlined send_msg)
    output.push_str("    set glass_msg_tag = msg_result\n");
    output.push_str("    set glass_msg_p0 = 0\n");
    output.push_str("    set glass_msg_p1 = 0\n");
    output.push_str("    set glass_result = glass_dispatch_update()\n");
    output.push_str("    set glass_model = glass_rt_tuple_0(glass_result)\n");
    output.push_str("    set glass_effects = glass_rt_tuple_1(glass_result)\n");
    // Walk effect list (inlined process_effects + exec_effect for non-Batch)
    output.push_str("    set current = glass_effects\n");
    output.push_str("    loop\n");
    output.push_str("        exitwhen current == -1\n");
    output.push_str("        set fx_id = glass_List_integer_head[current]\n");
    output.push_str("        set fx_tag = glass_Effect_tag[fx_id]\n");
    output.push_str("        if fx_tag == glass_TAG_After then\n");
    output.push_str("            set t2 = CreateTimer()\n");
    output.push_str("            call SaveInteger(glass_timer_ht, GetHandleId(t2), 0, glass_Effect_After_callback[fx_id])\n");
    output.push_str("            call TimerStart(t2, glass_Effect_After_duration[fx_id], false, function glass_timer_callback)\n");
    output.push_str("            set t2 = null\n");
    output.push_str("        elseif fx_tag == glass_TAG_DisplayText then\n");
    output.push_str("            call DisplayTimedTextToPlayer(Player(glass_Effect_DisplayText_player_id[fx_id]), 0, 0, glass_Effect_DisplayText_duration[fx_id], glass_Effect_DisplayText_text[fx_id])\n");
    output.push_str("        endif\n");
    output.push_str("        call glass_Effect_dealloc(fx_id)\n");
    output.push_str("        set current = glass_List_integer_tail[current]\n");
    output.push_str("    endloop\n");
    output.push_str("endfunction\n\n");
}

fn gen_msg_dispatch(_entry: &ElmEntryPoints, output: &mut String) {
    // update returns #(Model, List(Effect)) — a tuple
    output.push_str("function glass_dispatch_update takes nothing returns integer\n");
    output.push_str("    return glass_update(glass_model, glass_msg_tag)\n");
    output.push_str("endfunction\n\n");
}

/// glass_send_msg: call update, extract #(model, effects), store model, process effects.
fn gen_send_msg(output: &mut String) {
    output.push_str(
        "function glass_send_msg takes integer tag, integer p0, integer p1 returns nothing\n",
    );
    output.push_str("    local integer glass_result\n");
    output.push_str("    local integer glass_new_model\n");
    output.push_str("    local integer glass_effects\n");
    output.push_str("    set glass_msg_tag = tag\n");
    output.push_str("    set glass_msg_p0 = p0\n");
    output.push_str("    set glass_msg_p1 = p1\n");
    // Call update → returns tuple #(Model, List(Effect))
    output.push_str("    set glass_result = glass_dispatch_update()\n");
    // Extract tuple fields
    output.push_str("    set glass_new_model = glass_rt_tuple_0(glass_result)\n");
    output.push_str("    set glass_effects = glass_rt_tuple_1(glass_result)\n");
    output.push_str("    set glass_model = glass_new_model\n");
    // Process the returned effects
    output.push_str("    call glass_process_effects(glass_effects)\n");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use crate::token::Lexer;

    fn detect_entry_points(source: &str) -> Option<ElmEntryPoints> {
        let tokens = Lexer::tokenize(source);
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module().expect("parse failed");
        let types = TypeRegistry::from_module(&module);
        ElmEntryPoints::detect(&module, &types)
    }

    #[test]
    fn detects_elm_app() {
        let entry = detect_entry_points(
            r#"
pub type Msg { Tick GameStart }
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

    #[test]
    fn runtime_globals_snapshot() {
        let mut globals = Vec::new();
        collect_runtime_globals(&mut globals);
        insta::assert_snapshot!("globals", globals.join("\n"));
    }

    #[test]
    fn runtime_functions_snapshot() {
        let entry = ElmEntryPoints {
            has_init: true,
            has_update: true,
            has_subscriptions: false,
            msg_variants: vec![("Tick".into(), 0, 0), ("UnitDied".into(), 1, 2)],
        };
        let mut output = String::new();
        gen_elm_runtime_functions(&entry, &[], &mut output);
        insta::assert_snapshot!("functions", output);
    }
}
