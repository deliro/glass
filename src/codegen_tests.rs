//! Unified codegen tests: same Glass source → snapshot both JASS and Lua output.

use crate::closures::LambdaCollector;
use crate::codegen::JassCodegen;
use crate::lua_codegen::LuaCodegen;
use crate::parser::Parser;
use crate::token::Lexer;
use crate::types::TypeRegistry;
use rstest::rstest;

fn compile_jass(source: &str) -> String {
    let tokens = Lexer::tokenize(source).expect("lex failed");
    let mut parser = Parser::new(tokens);
    let module = {
        let _o = parser.parse_module();
        assert!(_o.errors.is_empty(), "parse errors: {:?}", _o.errors);
        _o.module
    };
    let types = TypeRegistry::from_module(&module);
    let mut collector = LambdaCollector::new();
    collector.collect_module(&module);
    let mut inferencer = crate::infer::Inferencer::new();
    let infer_result = inferencer.infer_module(&module);
    JassCodegen::new(
        types,
        collector.lambdas,
        infer_result.type_map,
        inferencer.type_param_vars.clone(),
    )
    .generate(&module, &[])
}

fn compile_lua(source: &str) -> String {
    let tokens = Lexer::tokenize(source).expect("lex failed");
    let mut parser = Parser::new(tokens);
    let module = {
        let _o = parser.parse_module();
        assert!(_o.errors.is_empty(), "parse errors: {:?}", _o.errors);
        _o.module
    };
    let types = TypeRegistry::from_module(&module);
    let mut collector = LambdaCollector::new();
    collector.collect_module(&module);
    let mut inferencer = crate::infer::Inferencer::new();
    let infer_result = inferencer.infer_module(&module);
    LuaCodegen::new(
        types,
        collector.lambdas,
        infer_result.type_map,
        inferencer.type_param_vars.clone(),
    )
    .generate(&module, &[])
}

/// Compile to both targets and return a combined string for a single snapshot.
fn compile_both(source: &str) -> String {
    let jass = compile_jass(source);
    let lua = compile_lua(source);
    format!(
        "===== JASS =====\n{}\n===== Lua =====\n{}",
        jass.trim(),
        lua.trim()
    )
}

// ============================================================
// Parity tests: same Glass code → JASS + Lua snapshot
// ============================================================

#[rstest]
// --- Basic expressions ---
#[case::add("add", "fn add(a: Int, b: Int) -> Int { a + b }")]
#[case::bool_return("bool_return", "fn is_positive(x: Int) -> Bool { x > 0 }")]
#[case::string_concat(
    "string_concat",
    r#"fn greet(name: String) -> String { "Hello " <> name }"#
)]
#[case::function_call("function_call", "fn double(x: Int) -> Int { add(x, x) }")]
#[case::let_binding("let_binding", "fn test() -> Int { let x: Int = 5 x }")]
#[case::no_return("no_return", "fn side_effect(x: Int) { add(x, x) }")]
#[case::modulo("modulo", "fn rem(a: Int, b: Int) -> Int { a % b }")]
#[case::negation("negation", "fn neg(x: Int) -> Int { -x }")]
#[case::logical_not("logical_not", "fn invert(x: Bool) -> Bool { !x }")]
// --- Case / pattern matching ---
#[case::case_bool(
    "case_bool",
    "fn check(x: Bool) -> Int { case x { True -> 1 False -> 0 } }"
)]
#[case::case_enum(
    "case_enum",
    "
pub enum Color { Red Green Blue }
fn to_int(c: Color) -> Int { case c { Red -> 1 Green -> 2 Blue -> 3 } }
"
)]
#[case::case_with_fields(
    "case_with_fields",
    "
pub enum Shape { Circle { radius: Float } Rect { w: Float, h: Float } }
fn area(s: Shape) -> Float {
    case s {
        Circle(r) -> r * r * 3.14
        Rect(w, h) -> w * h
    }
}
"
)]
// --- Pipe ---
#[case::pipe_call("pipe_call", "fn test(x: Int) -> Int { x |> add(1) }")]
#[case::pipe_var("pipe_var", "fn test(x: Int) -> Int { x |> double }")]
#[case::pipe_placeholder("pipe_placeholder", "fn test(x: Int) -> Int { x |> sub(10, _) }")]
#[case::pipe_module_call("pipe_module_call", "fn test(x: Int) -> Int { x |> int.max(_, 0) }")]
#[case::pipe_module_no_args(
    "pipe_module_no_args",
    "fn test(x: Int) -> String { x |> int.to_string }"
)]
// --- Functions ---
#[case::multi_function(
    "multi_function",
    "
fn add(a: Int, b: Int) -> Int { a + b }
fn mul(a: Int, b: Int) -> Int { a * b }
fn combined(x: Int) -> Int { add(x, mul(x, 2)) }
"
)]
#[case::topo_sort("topo_sort", "fn b() -> Int { a() }\nfn a() -> Int { 42 }")]
// --- Types ---
#[case::struct_def("struct_def", "pub struct Model { phase: Int, wave: Int, score: Int }")]
#[case::enum_def(
    "enum_def",
    "pub enum Phase { Lobby Playing { wave: Int } Victory { winner: Int } }"
)]
#[case::constructor(
    "constructor",
    "
pub struct Model { wave: Int, score: Int }
fn make() -> Int { Model { wave: 1, score: 100 } }
"
)]
#[case::record_update(
    "record_update",
    "
pub struct Model { wave: Int, score: Int }
fn bump(m: Int) -> Int { Model { ..m, wave: 5 } }
"
)]
#[case::record_update_handle_fields(
    "record_update_handle_fields",
    "
pub struct HookData { tip_x: Float, tip_y: Float, owner: Unit, target: Option(Unit), count: Int }
fn update_tip(h: HookData, x: Float, y: Float) -> HookData { HookData { ..h, tip_x: x, tip_y: y } }
"
)]
// --- Field access ---
#[case::field_access(
    "field_access",
    "
pub struct Model { wave: Int }
fn get_wave(m: Model) -> Int { m.wave }
"
)]
#[case::method_call("method_call", "fn test(h: Unit) -> Bool { h.is_alive() }")]
// --- Tuples ---
#[case::tuple_literal("tuple_literal", "fn make() -> Int { (1, 2, 3) }")]
#[case::tuple_in_fn("tuple_in_fn", "fn pair(a: Int, b: Int) -> Int { (a, b) }")]
// --- Lists ---
#[case::list_literal("list_literal", "fn nums() -> Int { [1, 2, 3] }")]
#[case::empty_list("empty_list", "fn empty() -> Int { [] }")]
#[case::list_cons("list_cons", "fn prepend(x: Int, xs: Int) -> Int { [x | xs] }")]
#[case::list_with_type_def(
    "list_with_type_def",
    "
pub struct Model { wave: Int }
fn test() -> Int { [1, 2, 3] }
"
)]
// --- Lambdas ---
#[case::lambda_no_capture("lambda_no_capture", "fn test() -> Int { fn(x: Int) { x + 1 } }")]
#[case::lambda_with_capture(
    "lambda_with_capture",
    "fn test(y: Int) -> Int { fn(x: Int) { x + y } }"
)]
#[case::lambda_capture_unit(
    "lambda_capture_unit",
    "fn test(u: Unit) -> Int { fn(x: Int) -> Unit { u } }"
)]
#[case::lambda_capture_unit_clone(
    "lambda_capture_unit_clone",
    r#"
enum Msg { Hit { source: Unit, target: Unit, amount: Float } }
fn make_aoe(caster: Unit, dmg: Float) -> Int {
    fn(u: Unit) -> Msg { Msg::Hit { source: clone(caster), target: u, amount: dmg } }
}
"#
)]
#[case::lambda_void("lambda_void", "fn test() -> Int { fn() { 42 } }")]
// --- Constants ---
#[case::const_int("const_int", "const MAX_WAVE: Int = 10")]
#[case::const_string("const_string", r#"const GREETING: String = "hello""#)]
#[case::const_bool("const_bool", "const DEBUG: Bool = true")]
// --- Misc ---
#[case::block(
    "block",
    "
fn test(x: Int) -> Int {
    let a: Int = x + 1
    let b: Int = a * 2
    b
}
"
)]
#[case::clone("clone", "fn test(x: Int) -> Int { clone(x) }")]
#[case::todo_expr("todo_expr", "fn unimplemented() -> Int { todo }")]
#[case::todo_msg("todo_msg", r#"fn unimplemented() -> Int { todo "not yet" }"#)]
#[case::rawcode("rawcode", r#"fn footman() -> Int { 'hfoo' }"#)]
#[case::discard("discard", "fn ignore(x: Int) -> Int { let _: Int = x 0 }")]
#[case::tuple_destructure(
    "tuple_destructure",
    "fn first(t: Int) -> Int { let (a, _b): Int = t a }"
)]
// --- Extend blocks ---
#[case::extend_single_method(
    "extend_single_method",
    "
extend Int {
    fn doubled(self: Int) -> Int { self + self }
}
"
)]
#[case::extend_multiple_methods(
    "extend_multiple_methods",
    "
extend Int {
    fn doubled(self: Int) -> Int { self + self }
    fn is_positive(self: Int) -> Bool { self > 0 }
}
"
)]
#[case::extend_self_usage(
    "extend_self_usage",
    "
extend Int {
    fn add_to(self: Int, other: Int) -> Int { self + other }
}
"
)]
#[case::extend_method_call(
    "extend_method_call",
    "
extend Int {
    fn doubled(self: Int) -> Int { self + self }
}
fn test(x: Int) -> Int { x.doubled() }
"
)]
fn parity(#[case] name: &str, #[case] source: &str) {
    insta::assert_snapshot!(name, compile_both(source));
}

// ============================================================
// Topo sort assertion (both backends)
// ============================================================

#[test]
fn topo_sort_both_backends() {
    let source = "fn b() -> Int { a() }\nfn a() -> Int { 42 }";
    for (label, output) in [("jass", compile_jass(source)), ("lua", compile_lua(source))] {
        let a_pos = output
            .find("function glass_a")
            .unwrap_or_else(|| panic!("{}: glass_a not found", label));
        let b_pos = output
            .find("function glass_b")
            .unwrap_or_else(|| panic!("{}: glass_b not found", label));
        assert!(
            a_pos < b_pos,
            "{}: a should appear before b (a at {}, b at {})",
            label,
            a_pos,
            b_pos
        );
    }
}

// ============================================================
// Generic type monomorphization
// ============================================================

#[test]
fn option_multi_instantiation_jass() {
    let source = r#"
enum Option(T) {
    Some(T)
    None
}

pub fn make_unit_opt(u: Unit) -> Option(Unit) { Option::Some(u) }
pub fn make_float_opt() -> Option(Float) { Option::Some(3.14) }

pub fn test_unit(o: Option(Unit)) -> Bool {
    case o {
        Option::Some(u) -> True
        Option::None -> False
    }
}

pub fn test_float(o: Option(Float)) -> Float {
    case o {
        Option::Some(v) -> v
        Option::None -> 0.0
    }
}
"#;
    let jass = compile_jass(source);

    assert!(
        jass.contains("unit array glass_Option_unit_Some_"),
        "should have unit array for Option(Unit), got:\n{}",
        jass
    );
    assert!(
        jass.contains("real array glass_Option_real_Some_"),
        "should have real array for Option(Float), got:\n{}",
        jass
    );
    assert!(
        jass.contains("integer array glass_Option_unit_tag"),
        "should have separate tag array for Option_unit, got:\n{}",
        jass
    );
    assert!(
        jass.contains("integer array glass_Option_real_tag"),
        "should have separate tag array for Option_real, got:\n{}",
        jass
    );
    assert!(
        jass.contains("glass_Option_unit_alloc"),
        "should have separate allocator for Option_unit, got:\n{}",
        jass
    );
    assert!(
        jass.contains("glass_Option_real_alloc"),
        "should have separate allocator for Option_real, got:\n{}",
        jass
    );
    assert!(
        !jass.contains("glass_Option_Some_") || jass.contains("glass_Option_unit_Some_"),
        "should NOT have generic Option_Some arrays when multiple instantiations exist, got:\n{}",
        jass
    );
    assert!(
        jass.contains("glass_new_Option_unit_Some"),
        "constructor should use monomorphized name, got:\n{}",
        jass
    );
    assert!(
        jass.contains("glass_new_Option_real_Some"),
        "constructor should use monomorphized name, got:\n{}",
        jass
    );
    assert!(
        jass.contains("glass_Option_unit_Some_") && jass.contains("[o]"),
        "pattern match field access should use monomorphized arrays, got:\n{}",
        jass
    );
}

#[test]
fn option_multi_instantiation_lua() {
    let source = r#"
enum Option(T) {
    Some(T)
    None
}

pub fn make_unit_opt(u: Unit) -> Option(Unit) { Option::Some(u) }
pub fn make_float_opt() -> Option(Float) { Option::Some(3.14) }

pub fn test_unit(o: Option(Unit)) -> Bool {
    case o {
        Option::Some(u) -> True
        Option::None -> False
    }
}

pub fn test_float(o: Option(Float)) -> Float {
    case o {
        Option::Some(v) -> v
        Option::None -> 0.0
    }
}
"#;
    let lua = compile_lua(source);

    assert!(
        lua.contains("glass_TAG_Option_unit_Some") || lua.contains("glass_TAG_Option_Some"),
        "Lua should have tag constants for Option, got:\n{}",
        lua
    );
}

#[test]
fn option_single_instantiation_backward_compat() {
    let source = r#"
enum Option(T) {
    Some(T)
    None
}

pub fn make_float_opt() -> Option(Float) { Option::Some(3.14) }

pub fn test_float(o: Option(Float)) -> Float {
    case o {
        Option::Some(v) -> v
        Option::None -> 0.0
    }
}
"#;
    let jass = compile_jass(source);

    assert!(
        jass.contains("real array glass_Option_Some_"),
        "single instantiation should use base name, got:\n{}",
        jass
    );
    assert!(
        jass.contains("glass_Option_alloc"),
        "single instantiation should use base name allocator, got:\n{}",
        jass
    );
}

// ============================================================
// Elm runtime parity
// ============================================================

#[test]
fn elm_runtime_jass_snapshot() {
    let entry = crate::runtime::ElmEntryPoints {
        has_init: true,
        has_update: true,
        has_subscriptions: false,
        msg_variants: vec![("Tick".into(), 0, 0), ("UnitDied".into(), 1, 2)],
        effect_variants: vec![],
    };
    let mut output = String::new();
    crate::runtime::gen_elm_runtime_functions(
        &entry,
        &[],
        &std::collections::HashSet::new(),
        &mut output,
    );
    insta::assert_snapshot!(output);
}

#[test]
fn dealloc_nulls_handle_fields() {
    let source = r#"
pub struct HeroState { hero: Unit, level: Int }
pub fn init() -> (HeroState, List(Int)) { (HeroState { hero: todo(), level: 1 }, []) }
pub fn update(m: HeroState, msg: Int) -> (HeroState, List(Int)) { (m, []) }
"#;
    let jass = compile_jass(source);
    assert!(
        jass.contains("set glass_HeroState_HeroState_hero [id] = null"),
        "dealloc must null handle-typed fields, got:\n{}",
        jass
    );
}

#[test]
fn elm_runtime_lua_snapshot() {
    let entry = crate::runtime::ElmEntryPoints {
        has_init: true,
        has_update: true,
        has_subscriptions: false,
        msg_variants: vec![("Tick".into(), 0, 0), ("UnitDied".into(), 1, 2)],
        effect_variants: vec![],
    };
    let mut output = String::new();
    crate::lua_runtime::gen_lua_elm_runtime(&entry, &mut output);
    insta::assert_snapshot!(output);
}

#[test]
fn elm_runtime_jass_with_subs_snapshot() {
    let entry = crate::runtime::ElmEntryPoints {
        has_init: true,
        has_update: true,
        has_subscriptions: true,
        msg_variants: vec![("Tick".into(), 0, 0), ("UnitDied".into(), 1, 2)],
        effect_variants: vec![],
    };
    let mut output = String::new();
    crate::runtime::gen_elm_runtime_functions(
        &entry,
        &[],
        &std::collections::HashSet::new(),
        &mut output,
    );
    insta::assert_snapshot!(output);
}

#[test]
fn elm_runtime_lua_with_subs_snapshot() {
    let entry = crate::runtime::ElmEntryPoints {
        has_init: true,
        has_update: true,
        has_subscriptions: true,
        msg_variants: vec![("Tick".into(), 0, 0), ("UnitDied".into(), 1, 2)],
        effect_variants: vec![],
    };
    let mut output = String::new();
    crate::lua_runtime::gen_lua_elm_runtime(&entry, &mut output);
    insta::assert_snapshot!(output);
}

#[test]
fn lua_send_msg_reconciles_subs() {
    let entry = crate::runtime::ElmEntryPoints {
        has_init: true,
        has_update: true,
        has_subscriptions: true,
        msg_variants: vec![],
        effect_variants: vec![],
    };
    let mut output = String::new();
    crate::lua_runtime::gen_lua_elm_runtime(&entry, &mut output);
    assert!(
        output.contains("glass_reconcile_subs(glass_subscriptions(glass_model))"),
        "send_msg must reconcile subscriptions"
    );
}

#[test]
fn jass_send_msg_reconciles_subs() {
    let entry = crate::runtime::ElmEntryPoints {
        has_init: true,
        has_update: true,
        has_subscriptions: true,
        msg_variants: vec![],
        effect_variants: vec![],
    };
    let mut output = String::new();
    crate::runtime::gen_elm_runtime_functions(
        &entry,
        &[],
        &std::collections::HashSet::new(),
        &mut output,
    );
    assert!(
        output.contains("call glass_reconcile_subs()"),
        "send_msg must reconcile subscriptions"
    );
}

#[test]
fn lua_reconcile_destroys_old_subs() {
    let entry = crate::runtime::ElmEntryPoints {
        has_init: true,
        has_update: true,
        has_subscriptions: true,
        msg_variants: vec![],
        effect_variants: vec![],
    };
    let mut output = String::new();
    crate::lua_runtime::gen_lua_elm_runtime(&entry, &mut output);
    assert!(output.contains("glass_unregister_one_sub(key)"));
    assert!(output.contains("DestroyTrigger(entry.handle)"));
    assert!(output.contains("DestroyTimer(entry.handle)"));
}

#[test]
fn jass_reconcile_destroys_old_subs() {
    let entry = crate::runtime::ElmEntryPoints {
        has_init: true,
        has_update: true,
        has_subscriptions: true,
        msg_variants: vec![],
        effect_variants: vec![],
    };
    let mut output = String::new();
    crate::runtime::gen_elm_runtime_functions(
        &entry,
        &[],
        &std::collections::HashSet::new(),
        &mut output,
    );
    assert!(output.contains("call glass_unregister_one_sub(idx)"));
    assert!(output.contains("call DestroyTrigger(glass_sub_triggers[idx])"));
    assert!(output.contains("call DestroyTimer(glass_sub_timers[idx])"));
}

#[test]
fn no_reconciliation_without_subs() {
    let entry = crate::runtime::ElmEntryPoints {
        has_init: true,
        has_update: true,
        has_subscriptions: false,
        msg_variants: vec![],
        effect_variants: vec![],
    };
    let mut jass_output = String::new();
    crate::runtime::gen_elm_runtime_functions(
        &entry,
        &[],
        &std::collections::HashSet::new(),
        &mut jass_output,
    );
    assert!(!jass_output.contains("glass_reconcile_subs"));

    let mut lua_output = String::new();
    crate::lua_runtime::gen_lua_elm_runtime(&entry, &mut lua_output);
    assert!(!lua_output.contains("glass_reconcile_subs"));
}

#[test]
fn closure_captures_unit_handle_simple() {
    let source = "fn test(u: Unit) -> Int { fn(x: Int) -> Unit { u } }";
    let jass = compile_jass(source);
    assert!(
        jass.contains("unit array glass_clos0_u"),
        "capture array should be unit type, got:\n{}",
        jass
    );
    assert!(
        jass.contains("local unit u"),
        "local in dispatch should be unit type, got:\n{}",
        jass
    );
}

#[test]
fn data_driven_effect_dispatch_with_exec_fn() {
    use crate::runtime::EffectVariantDef;
    use crate::types::FieldInfo;

    let entry = crate::runtime::ElmEntryPoints {
        has_init: true,
        has_update: true,
        has_subscriptions: false,
        msg_variants: vec![],
        effect_variants: vec![
            EffectVariantDef {
                name: "DisplayText".into(),
                tag: 0,
                fields: vec![
                    FieldInfo {
                        name: "player_id".into(),
                        jass_type: "integer".into(),
                        is_callback: false,
                        callback_param_jass_types: vec![],
                    },
                    FieldInfo {
                        name: "text".into(),
                        jass_type: "string".into(),
                        is_callback: false,
                        callback_param_jass_types: vec![],
                    },
                    FieldInfo {
                        name: "duration".into(),
                        jass_type: "real".into(),
                        is_callback: false,
                        callback_param_jass_types: vec![],
                    },
                ],
                has_exec_fn: true,
            },
            EffectVariantDef {
                name: "KillUnit".into(),
                tag: 1,
                fields: vec![FieldInfo {
                    name: "unit".into(),
                    jass_type: "unit".into(),
                    is_callback: false,
                    callback_param_jass_types: vec![],
                }],
                has_exec_fn: true,
            },
        ],
    };

    let mut jass = String::new();
    crate::runtime::gen_elm_runtime_functions(
        &entry,
        &[],
        &std::collections::HashSet::new(),
        &mut jass,
    );
    assert!(
        jass.contains("call glass_exec_display_text(glass_Effect_DisplayText_player_id[fx_id], glass_Effect_DisplayText_text[fx_id], glass_Effect_DisplayText_duration[fx_id])"),
        "JASS must dispatch to exec function with SoA field args"
    );
    assert!(
        jass.contains("call glass_exec_kill_unit(glass_Effect_KillUnit_unit[fx_id])"),
        "JASS must dispatch to exec function for KillUnit"
    );

    let mut lua = String::new();
    crate::lua_runtime::gen_lua_elm_runtime(&entry, &mut lua);
    assert!(
        lua.contains("glass_exec_display_text(fx.player_id, fx.text, fx.duration)"),
        "Lua must dispatch to exec function with table field args"
    );
    assert!(
        lua.contains("glass_exec_kill_unit(fx.unit)"),
        "Lua must dispatch to exec function for KillUnit"
    );
}

#[test]
fn closure_captures_unit_via_clone() {
    let source = r#"
enum Msg {
    Hit { source: Unit, target: Unit, amount: Float }
}
fn make_aoe(caster: Unit, dmg: Float) -> Int {
    fn(u: Unit) -> Msg {
        Msg::Hit { source: clone(caster), target: u, amount: dmg }
    }
}
"#;
    let jass = compile_jass(source);
    assert!(
        jass.contains("unit array glass_clos0_caster"),
        "capture array for caster should be unit type, got:\n{}",
        jass
    );
    assert!(
        jass.contains("local unit caster"),
        "local caster in dispatch should be unit type, got:\n{}",
        jass
    );
}

#[test]
fn data_driven_effect_callback_variants() {
    use crate::runtime::EffectVariantDef;
    use crate::types::FieldInfo;

    let entry = crate::runtime::ElmEntryPoints {
        has_init: true,
        has_update: true,
        has_subscriptions: false,
        msg_variants: vec![],
        effect_variants: vec![
            EffectVariantDef {
                name: "After".into(),
                tag: 0,
                fields: vec![
                    FieldInfo {
                        name: "duration".into(),
                        jass_type: "real".into(),
                        is_callback: false,
                        callback_param_jass_types: vec![],
                    },
                    FieldInfo {
                        name: "callback".into(),
                        jass_type: "integer".into(),
                        is_callback: true,
                        callback_param_jass_types: vec![],
                    },
                ],
                has_exec_fn: false,
            },
            EffectVariantDef {
                name: "ForUnitsInRange".into(),
                tag: 1,
                fields: vec![
                    FieldInfo {
                        name: "x".into(),
                        jass_type: "real".into(),
                        is_callback: false,
                        callback_param_jass_types: vec![],
                    },
                    FieldInfo {
                        name: "y".into(),
                        jass_type: "real".into(),
                        is_callback: false,
                        callback_param_jass_types: vec![],
                    },
                    FieldInfo {
                        name: "radius".into(),
                        jass_type: "real".into(),
                        is_callback: false,
                        callback_param_jass_types: vec![],
                    },
                    FieldInfo {
                        name: "callback".into(),
                        jass_type: "integer".into(),
                        is_callback: true,
                        callback_param_jass_types: vec!["unit".into()],
                    },
                ],
                has_exec_fn: false,
            },
        ],
    };

    let mut jass = String::new();
    crate::runtime::gen_elm_runtime_functions(
        &entry,
        &[],
        &std::collections::HashSet::new(),
        &mut jass,
    );
    assert!(
        jass.contains("glass_TAG_Effect_After"),
        "JASS must handle After variant"
    );
    assert!(
        jass.contains("glass_timer_callback"),
        "JASS After must use timer callback"
    );
    assert!(
        jass.contains("glass_TAG_Effect_ForUnitsInRange"),
        "JASS must handle ForUnitsInRange variant"
    );
    assert!(
        jass.contains("GroupEnumUnitsInRange"),
        "JASS ForUnitsInRange must iterate group"
    );

    let mut lua = String::new();
    crate::lua_runtime::gen_lua_elm_runtime(&entry, &mut lua);
    assert!(
        lua.contains("glass_TAG_Effect_After"),
        "Lua must handle After variant"
    );
    assert!(
        lua.contains("glass_send_msg(msg)"),
        "Lua After must send msg via callback"
    );
    assert!(
        lua.contains("glass_TAG_Effect_ForUnitsInRange"),
        "Lua must handle ForUnitsInRange variant"
    );
}

#[test]
fn closure_captures_unit_elm_context() {
    let source = r#"
import effect

pub enum Msg {
    Created { hero: Unit, spawned: Unit }
}
pub struct Model { hero: Unit }
pub fn init() -> (Model, List(effect.Effect(Msg))) {
    (Model { hero: todo }, [])
}
pub fn update(model: Model, msg: Msg) -> (Model, List(effect.Effect(Msg))) {
    let h = clone(model.hero)
    let fx = effect.create_unit_callback(0, 'hfoo', 0.0, 0.0, 0.0, fn(u: Unit) -> Msg {
        Msg::Created { hero: h, spawned: u }
    })
    (model, [fx])
}
"#;
    let jass = compile_jass(source);
    assert!(
        jass.contains("unit array glass_clos") && jass.contains("_h"),
        "capture array for h should be unit type, got:\n{}",
        jass
    );
    assert!(
        jass.contains("local unit h"),
        "local h in dispatch should be unit type, got:\n{}",
        jass
    );
}

#[test]
fn closure_captures_unit_let_chain() {
    let source = r#"
enum Msg {
    Hit { source: Unit, target: Unit }
}
fn test(caster: Unit) -> Int {
    let c = clone(caster)
    let d = c
    fn(u: Unit) -> Msg {
        Msg::Hit { source: d, target: u }
    }
}
"#;
    let jass = compile_jass(source);
    assert!(
        jass.contains("unit array glass_clos0_d"),
        "capture array for d should be unit type, got:\n{}",
        jass
    );
}

#[test]
fn closure_captures_unit_indirect_use() {
    let source = r#"
enum Msg {
    Hit { source: Unit, target: Unit }
}
fn test(caster: Unit) -> Int {
    fn(u: Unit) -> Msg {
        let c = caster
        Msg::Hit { source: c, target: u }
    }
}
"#;
    let jass = compile_jass(source);
    assert!(
        jass.contains("unit array glass_clos0_caster"),
        "capture array for caster should be unit type, got:\n{}",
        jass
    );
}

#[test]
fn closure_captures_player_handle() {
    let source = r#"
fn test(p: Player) -> Int {
    fn() -> Player { p }
}
"#;
    let jass = compile_jass(source);
    assert!(
        jass.contains("player array glass_clos0_p"),
        "capture array for p should be player type, got:\n{}",
        jass
    );
}

#[test]
fn closure_captures_timer_handle() {
    let source = r#"
fn test(t: Timer) -> Int {
    fn() -> Timer { t }
}
"#;
    let jass = compile_jass(source);
    assert!(
        jass.contains("timer array glass_clos0_t"),
        "capture array for t should be timer type, got:\n{}",
        jass
    );
}

#[test]
fn closure_captures_unit_from_case() {
    let source = r#"
enum Wrapper {
    HasUnit { u: Unit }
}
fn test(w: Wrapper) -> Int {
    case w {
        Wrapper::HasUnit { u } -> fn() -> Unit { u }
    }
}
"#;
    let jass = compile_jass(source);
    assert!(
        jass.contains("unit array glass_clos0_u"),
        "capture array for u should be unit type, got:\n{}",
        jass
    );
}

#[test]
fn data_driven_custom_effect_variant() {
    use crate::runtime::EffectVariantDef;
    use crate::types::FieldInfo;

    let entry = crate::runtime::ElmEntryPoints {
        has_init: true,
        has_update: true,
        has_subscriptions: false,
        msg_variants: vec![],
        effect_variants: vec![EffectVariantDef {
            name: "CustomBlast".into(),
            tag: 99,
            fields: vec![
                FieldInfo {
                    name: "x".into(),
                    jass_type: "real".into(),
                    is_callback: false,
                    callback_param_jass_types: vec![],
                },
                FieldInfo {
                    name: "y".into(),
                    jass_type: "real".into(),
                    is_callback: false,
                    callback_param_jass_types: vec![],
                },
                FieldInfo {
                    name: "power".into(),
                    jass_type: "integer".into(),
                    is_callback: false,
                    callback_param_jass_types: vec![],
                },
            ],
            has_exec_fn: true,
        }],
    };

    let mut jass = String::new();
    crate::runtime::gen_elm_runtime_functions(
        &entry,
        &[],
        &std::collections::HashSet::new(),
        &mut jass,
    );
    assert!(
        jass.contains("glass_TAG_Effect_CustomBlast"),
        "JASS must handle custom user-defined effect variant"
    );
    assert!(
        jass.contains("call glass_exec_custom_blast(glass_Effect_CustomBlast_x[fx_id], glass_Effect_CustomBlast_y[fx_id], glass_Effect_CustomBlast_power[fx_id])"),
        "JASS must dispatch custom effect to exec function"
    );

    let mut lua = String::new();
    crate::lua_runtime::gen_lua_elm_runtime(&entry, &mut lua);
    assert!(
        lua.contains("glass_TAG_Effect_CustomBlast"),
        "Lua must handle custom user-defined effect variant"
    );
    assert!(
        lua.contains("glass_exec_custom_blast(fx.x, fx.y, fx.power)"),
        "Lua must dispatch custom effect to exec function"
    );
}

fn compile_jass_with_sdk(source: &str) -> String {
    let tokens = Lexer::tokenize(source).expect("lex failed");
    let mut parser = Parser::new(tokens);
    let module = {
        let _o = parser.parse_module();
        assert!(_o.errors.is_empty(), "parse errors: {:?}", _o.errors);
        _o.module
    };
    // Use a dummy path — ModuleResolver falls back to cwd which has sdk/
    let input_path = std::path::Path::new("dummy.glass");
    let mut resolver = crate::modules::ModuleResolver::new(input_path);
    let (merged, imports, _, _) = resolver
        .resolve_module(&module)
        .expect("SDK import resolution failed");
    let types = TypeRegistry::from_module(&merged);
    let mut collector = LambdaCollector::new();
    collector.collect_module(&merged);
    let mut inferencer = crate::infer::Inferencer::new();
    let infer_result = inferencer.infer_module(&merged);
    JassCodegen::new(
        types,
        collector.lambdas,
        infer_result.type_map,
        inferencer.type_param_vars.clone(),
    )
    .generate(&merged, &imports)
}

fn compile_lua_with_sdk(source: &str) -> String {
    let tokens = Lexer::tokenize(source).expect("lex failed");
    let mut parser = Parser::new(tokens);
    let module = {
        let _o = parser.parse_module();
        assert!(_o.errors.is_empty(), "parse errors: {:?}", _o.errors);
        _o.module
    };
    let input_path = std::path::Path::new("dummy.glass");
    let mut resolver = crate::modules::ModuleResolver::new(input_path);
    let (merged, imports, _, _) = resolver
        .resolve_module(&module)
        .expect("SDK import resolution failed");
    let types = TypeRegistry::from_module(&merged);
    let mut collector = LambdaCollector::new();
    collector.collect_module(&merged);
    let mut inferencer = crate::infer::Inferencer::new();
    let infer_result = inferencer.infer_module(&merged);
    LuaCodegen::new(
        types,
        collector.lambdas,
        infer_result.type_map,
        inferencer.type_param_vars.clone(),
    )
    .generate(&merged, &imports)
}

#[test]
fn create_unit_then_generates_chain_dispatch() {
    let source = r#"
import effect

pub enum Msg { Spawned }
pub struct Model { count: Int }
pub fn init() -> (Model, List(effect.Effect(Msg))) {
    (Model { count: 0 }, [])
}
pub fn update(model: Model, msg: Msg) -> (Model, List(effect.Effect(Msg))) {
    let fx = effect.create_unit_then(0, 1148481101, 0.0, 0.0, 0.0, fn(u: Unit) -> List(effect.Effect(Msg)) {
        [effect.kill_unit(u)]
    })
    (model, [fx])
}
"#;
    let jass = compile_jass_with_sdk(source);
    // CreateUnitThen should use cb_type = 3 (unit chain callback)
    assert!(
        jass.contains("SaveInteger(glass_timer_ht, GetHandleId(t), 2, 3)"),
        "CreateUnitThen should use cb_type=3, got:\n{}",
        jass
    );
    // Should reference the chain field, not callback
    assert!(
        jass.contains("glass_Effect_CreateUnitThen_chain"),
        "should access chain field, got:\n{}",
        jass
    );

    let lua = compile_lua_with_sdk(source);
    // Lua should call glass_process_effects directly
    assert!(
        lua.contains("glass_process_effects(fx.chain(u))"),
        "Lua CreateUnitThen should call glass_process_effects with chain result, got:\n{}",
        lua
    );
}

#[test]
fn after_then_generates_chain_dispatch() {
    let source = r#"
import effect

pub enum Msg { Tick }
pub struct Model { count: Int }
pub fn init() -> (Model, List(effect.Effect(Msg))) {
    (Model { count: 0 }, [])
}
pub fn update(model: Model, msg: Msg) -> (Model, List(effect.Effect(Msg))) {
    let fx = effect.after_then(2.0, fn() -> List(effect.Effect(Msg)) {
        []
    })
    (model, [fx])
}
"#;
    let jass = compile_jass_with_sdk(source);
    // AfterThen should use cb_type = 2 (void chain callback)
    assert!(
        jass.contains("SaveInteger(glass_timer_ht, GetHandleId(t), 2, 2)"),
        "AfterThen should use cb_type=2, got:\n{}",
        jass
    );
    assert!(
        jass.contains("glass_Effect_AfterThen_chain"),
        "should access chain field, got:\n{}",
        jass
    );

    let lua = compile_lua_with_sdk(source);
    assert!(
        lua.contains("glass_process_effects(cb())"),
        "Lua AfterThen should call glass_process_effects in timer callback, got:\n{}",
        lua
    );
}
