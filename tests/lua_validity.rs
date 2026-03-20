use rstest::rstest;
use std::process::Command;

fn compile_glass_lua(source: &str) -> String {
    let tmp = std::env::temp_dir().join(format!(
        "glass_lua_src_{:?}.glass",
        std::thread::current().id()
    ));
    std::fs::write(&tmp, source).expect("write temp file");

    let output = Command::new(env!("CARGO_BIN_EXE_glass"))
        .arg(&tmp)
        .arg("--target")
        .arg("lua")
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

fn luac_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tools")
        .join("luac")
}

/// Validate generated Lua code with luac -p (parse-only mode).
/// WC3 globals (CreateTimer, etc.) are not defined, but luac -p only checks syntax.
fn validate_lua(lua_code: &str) {
    let luac = luac_path();
    if !luac.exists() {
        eprintln!("luac not found at {:?}, skipping validation", luac);
        return;
    }
    let tmp = std::env::temp_dir().join(format!(
        "glass_lua_test_{:?}.lua",
        std::thread::current().id()
    ));
    std::fs::write(&tmp, lua_code).expect("write temp file");

    let output = Command::new(&luac)
        .arg("-p")
        .arg(&tmp)
        .output()
        .expect("failed to run luac");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "luac validation failed!\n--- lua code ---\n{}\n--- luac output ---\n{}\n{}",
        lua_code,
        stdout,
        stderr
    );
}

fn compile_and_validate(source: &str) {
    let lua = compile_glass_lua(source);
    validate_lua(&lua);
}

/// Compile a .glass file by path (preserving directory context for imports).
fn compile_glass_lua_file(path: &std::path::Path) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_glass"))
        .arg(path)
        .arg("--target")
        .arg("lua")
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
// Example files — full compilation cycle + luac validation
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
    let lua = compile_glass_lua_file(&path);
    validate_lua(&lua);
}

// ============================================================
// Game example — full compilation WITH type checking
// ============================================================

#[test]
fn game_compiles_with_type_checking() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = manifest.join("examples").join("game/main.glass");
    let output = Command::new(env!("CARGO_BIN_EXE_glass"))
        .arg(&path)
        .arg("--target")
        .arg("lua")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to run glass");

    assert!(
        output.status.success(),
        "game/main.glass failed type checking (Lua):\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let lua = String::from_utf8(output.stdout).expect("invalid utf8");
    validate_lua(&lua);
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
    compile_and_validate(
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
    compile_and_validate(
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
