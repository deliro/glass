//! Unified codegen tests: same Glass source → snapshot both JASS and Lua output.

use crate::closures::LambdaCollector;
use crate::codegen::JassCodegen;
use crate::lua_codegen::LuaCodegen;
use crate::parser::Parser;
use crate::token::Lexer;
use crate::types::TypeRegistry;
use rstest::rstest;

fn compile_jass(source: &str) -> String {
    let tokens = Lexer::tokenize(source);
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module().expect("parse failed");
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
    let tokens = Lexer::tokenize(source);
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module().expect("parse failed");
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
// Elm runtime parity
// ============================================================

#[test]
fn elm_runtime_jass_snapshot() {
    let entry = crate::runtime::ElmEntryPoints {
        has_init: true,
        has_update: true,
        has_subscriptions: false,
        msg_variants: vec![("Tick".into(), 0, 0), ("UnitDied".into(), 1, 2)],
    };
    let mut output = String::new();
    crate::runtime::gen_elm_runtime_functions(&entry, &[], &mut output);
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
    };
    let mut output = String::new();
    crate::lua_runtime::gen_lua_elm_runtime(&entry, &mut output);
    insta::assert_snapshot!(output);
}
