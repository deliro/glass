use rstest::rstest;
use std::process::Command;

fn compile_glass(source: &str) -> String {
    let tmp =
        std::env::temp_dir().join(format!("glass_src_{:?}.glass", std::thread::current().id()));
    std::fs::write(&tmp, source).expect("write temp file");

    let output = Command::new(env!("CARGO_BIN_EXE_glass"))
        .arg(&tmp)
        .arg("--no-check")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to run glass");

    assert!(
        output.status.success(),
        "glass compilation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("invalid utf8")
}

fn pjass_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tools")
        .join("pjass")
}

fn common_stub_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("common_stub.j")
}

fn validate_jass_with_natives(jass_code: &str, use_common_stub: bool) {
    let pjass = pjass_path();
    if !pjass.exists() {
        eprintln!("pjass not found at {:?}, skipping validation", pjass);
        return;
    }
    let tmp = std::env::temp_dir().join(format!("glass_test_{:?}.j", std::thread::current().id()));
    std::fs::write(&tmp, jass_code).expect("write temp file");

    let mut cmd = Command::new(&pjass);
    if use_common_stub {
        cmd.arg(common_stub_path());
    }
    cmd.arg(&tmp);

    let output = cmd.output().expect("failed to run pjass");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "pjass validation failed!\n--- pjass output ---\n{}\n{}",
        stdout,
        stderr
    );
}

fn compile_and_validate(source: &str) {
    let jass = compile_glass(source);
    validate_jass_with_natives(&jass, false);
}

fn compile_and_validate_with_natives(source: &str) {
    let jass = compile_glass(source);
    validate_jass_with_natives(&jass, true);
}

/// Compile a .glass file by path (preserving directory context for imports).
fn compile_glass_file(path: &std::path::Path) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_glass"))
        .arg(path)
        .arg("--no-check")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to run glass");

    assert!(
        output.status.success(),
        "glass compilation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("invalid utf8")
}

// ============================================================
// Name mangling safety
// ============================================================

/// Stress test: function params use many single-letter names (a-z).
/// After mangling, no global should shadow these locals.
#[test]
fn mangle_no_conflict_with_single_letter_params() {
    compile_and_validate(
        r#"
pub struct Data { a: Int, b: Int, c: Int, d: Int, e: Int }

fn make(a: Int, b: Int, c: Int, d: Int, e: Int) -> Data {
    Data { a, b, c, d, e }
}

fn sum(f: Int, g: Int, h: Int) -> Int {
    f + g + h
}

fn use_data(p: Data) -> Int {
    p.a + p.b + p.c + p.d + p.e
}

pub fn test(j: Int, k: Int, l: Int, m: Int, n: Int) -> Int {
    let o = make(j, k, l, m, n)
    let q = use_data(o)
    let r = sum(j, k, l)
    q + r
}
"#,
    );
}

/// Verify that a function name is never mangled to the same name as one of
/// its own parameters (e.g. fn func(a:Int) must not become `function a takes integer a`).
#[test]
fn mangle_function_name_never_equals_own_param() {
    let jass = compile_glass(
        r#"
fn func(a: Int) -> Int { a + 1 }
fn other(b: Int) -> Int { func(b) }
"#,
    );
    // Parse JASS output: find every "function NAME takes TYPE PARAM" and check NAME != PARAM
    for line in jass.lines() {
        if let Some(rest) = line.strip_prefix("function ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            // parts[0] = function name, parts[1] = "takes"
            // then pairs of (type, param_name)
            if parts.len() >= 4 && parts[1] == "takes" && parts[2] != "nothing" {
                let fn_name = parts[0];
                // Collect param names: every odd index after index 2
                let mut i = 3;
                while i < parts.len() {
                    let param_name = parts[i].trim_end_matches(',');
                    assert_ne!(
                        fn_name, param_name,
                        "function '{}' has same name as its param '{}' in JASS output:\n{}",
                        fn_name, param_name, line
                    );
                    i += 2; // skip next type
                    if i < parts.len() && parts[i - 1] == "returns" {
                        break;
                    }
                }
            }
        }
    }
}

/// Cross-function scenario: function `foo` is called from `bar`,
/// and `bar` has a parameter with a name that could collide.
#[test]
fn mangle_cross_function_no_shadow() {
    let jass = compile_glass(
        r#"
pub struct Pair { x: Int, y: Int }

fn make(x: Int, y: Int) -> Pair { Pair { x, y } }
fn sum_pair(p: Pair) -> Int { p.x + p.y }

fn go(c: Int, d: Int, e: Int, f: Int, g: Int, h: Int) -> Int {
    let p = make(c, d)
    sum_pair(p) + e + f + g + h
}
"#,
    );
    validate_jass_with_natives(&jass, false);

    // Additionally: parse all globals and locals, verify no overlap within any function
    let mut globals: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut in_globals = false;
    for line in jass.lines() {
        let trimmed = line.trim();
        if trimmed == "globals" {
            in_globals = true;
            continue;
        }
        if trimmed == "endglobals" {
            in_globals = false;
            continue;
        }
        if in_globals {
            // Extract global name: "integer array NAME" or "integer NAME = 0"
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 2 {
                let name_part = if parts.len() >= 3 && parts[1] == "array" {
                    parts[2]
                } else {
                    parts[1]
                };
                globals.insert(name_part.to_string());
            }
        }
    }

    // Check that no local variable in any function has the same name as a global
    let mut current_locals: Vec<String> = Vec::new();
    for line in jass.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("function ") {
            current_locals.clear();
            // Extract param names
            if let Some(takes_part) = trimmed.split(" takes ").nth(1) {
                let before_returns = takes_part.split(" returns ").next().unwrap_or("");
                if before_returns != "nothing" {
                    for chunk in before_returns.split(',') {
                        let parts: Vec<&str> = chunk.trim().split_whitespace().collect();
                        if parts.len() >= 2 {
                            current_locals.push(parts[1].to_string());
                        }
                    }
                }
            }
        }
        if trimmed.starts_with("local ") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 3 {
                let var_name = parts[2].trim_end_matches(|c| c == '=' || c == ' ');
                current_locals.push(var_name.to_string());
            }
        }
        if trimmed == "endfunction" {
            for local in &current_locals {
                assert!(
                    !globals.contains(local),
                    "local '{}' shadows global '{}' — mangling conflict!",
                    local,
                    local
                );
            }
            current_locals.clear();
        }
    }
}

// ============================================================
// Example files — full compilation cycle + pjass validation
// ============================================================

#[rstest]
#[case("add.glass")]
#[case("types.glass")]
#[case("elm_counter.glass")]
#[case("elm_timer.glass")]
#[case("tower_defense.glass")]
#[case("axes_rexxar.glass")]
#[case("greater_bash.glass")]
#[case("invoker.glass")]
#[case("buff_system.glass")]
#[case("rune_system.glass")]
#[case("chain_lightning.glass")]
#[case("item_combine.glass")]
#[case("game/main.glass")]
#[case("sdk_smoke.glass")]
#[case("stdlib_smoke.glass")]
fn example_compiles(#[case] filename: &str) {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = manifest.join("examples").join(filename);
    let jass = compile_glass_file(&path);
    validate_jass_with_natives(&jass, true);
}

// ============================================================
// Game example — full compilation WITH type checking (no --no-check)
// ============================================================

#[test]
fn game_compiles_with_type_checking() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = manifest.join("examples").join("game/main.glass");
    // Compile WITHOUT --no-check to ensure type checker passes
    let output = Command::new(env!("CARGO_BIN_EXE_glass"))
        .arg(&path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to run glass");

    assert!(
        output.status.success(),
        "game/main.glass failed type checking:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let jass = String::from_utf8(output.stdout).expect("invalid utf8");
    validate_jass_with_natives(&jass, true);
}

// ============================================================
// Inline language feature tests
// ============================================================

#[test]
fn simple_functions() {
    compile_and_validate(
        r#"
fn add(a: Int, b: Int) -> Int { a + b }
fn is_positive(x: Int) -> Bool { x > 0 }
fn greet(name: String) -> String { "Hello " <> name }
"#,
    );
}

#[test]
fn struct_and_enum() {
    compile_and_validate(
        r#"
pub enum Phase { Lobby Playing { wave: Int } Victory { winner: Int } }
pub struct Model { phase: Int, wave: Int, score: Int }
fn test() -> Model { Model { phase: 0, wave: 1, score: 100 } }
"#,
    );
}

#[test]
fn case_expression() {
    compile_and_validate("fn check(x: Bool) -> Int { case x { True -> 1  False -> 0 } }");
}

#[test]
fn let_binding() {
    compile_and_validate(
        r#"
fn add(a: Int, b: Int) -> Int { a + b }
fn test() -> Int { let x: Int = 5  let y: Int = 10  add(x, y) }
"#,
    );
}

#[test]
fn pipe_operator() {
    compile_and_validate(
        r#"
fn double(x: Int) -> Int { x + x }
fn test(x: Int) -> Int { x |> double }
"#,
    );
}

#[test]
fn tuple_creation() {
    compile_and_validate("fn pair(a: Int, b: Int) -> Int { #(a, b) }");
}

#[test]
fn record_update() {
    compile_and_validate(
        r#"
pub struct Model { wave: Int, score: Int }
fn bump(m: Int) -> Int { Model { ..m, wave: 5 } }
"#,
    );
}

#[test]
fn list_literal() {
    compile_and_validate("fn nums() -> Int { [1, 2, 3] }");
}

#[test]
fn empty_list() {
    compile_and_validate("fn empty() -> Int { [] }");
}

#[test]
fn lambda_no_capture() {
    compile_and_validate("fn test() -> Int { fn(x: Int) { x + 1 } }");
}

#[test]
fn lambda_with_capture() {
    compile_and_validate("fn test(y: Int) -> Int { fn(x: Int) { x + y } }");
}

#[test]
fn elm_counter_app() {
    compile_and_validate_with_natives(
        r#"
import effect

pub enum Msg { Increment Decrement Reset }
pub struct Model { count: Int }

pub fn init() -> #(Model, List(effect.Effect(Msg))) {
    #(Model { count: 0 }, [])
}

pub fn update(model: Model, msg: Msg) -> #(Model, List(effect.Effect(Msg))) {
    case msg {
        Msg::Increment -> #(Model { count: model.count + 1 }, [])
        Msg::Decrement -> #(Model { count: model.count - 1 }, [])
        Msg::Reset -> #(Model { count: 0 }, [])
    }
}
"#,
    );
}

#[test]
fn elm_timer_effects() {
    compile_and_validate_with_natives(
        r#"
import effect

pub enum Msg { Tick GameStart }
pub struct Model { count: Int }

pub fn init() -> #(Model, List(effect.Effect(Msg))) {
    #(Model { count: 0 }, [
        effect.display_text(0, "Init!", 3.0)
    ])
}

pub fn update(model: Model, msg: Msg) -> #(Model, List(effect.Effect(Msg))) {
    case msg {
        Msg::Tick -> #(Model { count: model.count + 1 }, [
            effect.after(1.0, fn() { Msg::Tick })
        ])
        Msg::GameStart -> #(model, [])
    }
}
"#,
    );
}

#[test]
fn qualified_enum_constructors() {
    compile_and_validate(
        r#"
pub enum Phase { Lobby Playing { wave: Int } GameOver { final_wave: Int } }
fn start() -> Phase { Phase::Playing { wave: 1 } }
fn idle() -> Phase { Phase::Lobby }
fn over(w: Int) -> Phase { Phase::GameOver { final_wave: w } }
"#,
    );
}

#[test]
fn brace_constructor_shorthand() {
    compile_and_validate(
        r#"
pub struct Point { x: Int, y: Int }
fn make(x: Int, y: Int) -> Point { Point { x, y } }
fn origin() -> Point { Point { x: 0, y: 0 } }
"#,
    );
}

#[test]
fn assoc_list_per_unit_state() {
    compile_and_validate(
        r#"
pub struct UnitState { uid: Int, counter: Int }

fn upsert(xs: List(UnitState), s: UnitState) -> List(UnitState) {
    case xs {
        [] -> [s]
        [h | t] -> case h.uid == s.uid {
            True -> [s | t]
            False -> [h | upsert(t, s)]
        }
    }
}

fn lookup(xs: List(UnitState), uid: Int) -> UnitState {
    case xs {
        [] -> UnitState { uid, counter: 0 }
        [h | t] -> case h.uid == uid {
            True -> h
            False -> lookup(t, uid)
        }
    }
}

pub fn test_upsert() -> Int {
    let empty: List(UnitState) = []
    let s1 = UnitState { uid: 1, counter: 10 }
    let list1 = upsert(empty, s1)
    let found = lookup(list1, 1)
    found.counter
}
"#,
    );
}

#[test]
fn prd_algorithm() {
    compile_and_validate(
        r#"
import int

pub struct PrdState { streak: Int, c_pct: Int }

pub enum RollResult {
    Procced { new_state: PrdState }
    Missed { new_state: PrdState }
}

fn eff_chance(s: PrdState) -> Int {
    int.min((s.streak + 1) * s.c_pct / 100, 100)
}

pub fn roll(s: PrdState, rng: Int) -> RollResult {
    let chance = eff_chance(s)
    case rng <= chance {
        True -> RollResult::Procced { new_state: PrdState { streak: 0, c_pct: s.c_pct } }
        False -> RollResult::Missed { new_state: PrdState { streak: s.streak + 1, c_pct: s.c_pct } }
    }
}
"#,
    );
}

#[test]
fn recursive_bounce_damage() {
    compile_and_validate(
        r#"
pub struct BounceResult { total_dmg: Int, hits: Int }

fn compute(dmg: Int, remaining: Int, decay: Int) -> BounceResult {
    case remaining <= 0 {
        True -> BounceResult { total_dmg: 0, hits: 0 }
        False -> {
            let rest = compute(dmg * decay / 100, remaining - 1, decay)
            BounceResult {
                total_dmg: dmg + rest.total_dmg,
                hits: 1 + rest.hits,
            }
        }
    }
}

pub fn test_bounces() -> BounceResult {
    compute(100, 5, 80)
}
"#,
    );
}
