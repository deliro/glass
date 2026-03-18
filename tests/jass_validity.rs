use std::process::Command;

fn compile_glass(source: &str) -> String {
    // Write source to a temp file (clap doesn't support /dev/stdin well)
    let tmp =
        std::env::temp_dir().join(format!("glass_src_{:?}.glass", std::thread::current().id()));
    std::fs::write(&tmp, source).expect("write temp file");

    let output = Command::new(env!("CARGO_BIN_EXE_glass"))
        .arg(&tmp)
        .arg("--no-check") // Skip type checking for JASS validity tests
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
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.join("tools").join("pjass")
}

fn common_stub_path() -> std::path::PathBuf {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.join("tests").join("common_stub.j")
}

fn validate_jass(jass_code: &str) {
    validate_jass_with_natives(jass_code, false);
}

fn validate_jass_with_natives(jass_code: &str, use_common_stub: bool) {
    let pjass = pjass_path();
    if !pjass.exists() {
        eprintln!("pjass not found at {:?}, skipping validation", pjass);
        return;
    }

    // Write jass to unique temp file (tests run in parallel)
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
        "pjass validation failed!\n--- JASS code ---\n{}\n--- pjass output ---\n{}\n{}",
        jass_code,
        stdout,
        stderr
    );
}

fn compile_and_validate(source: &str) {
    let jass = compile_glass(source);
    validate_jass(&jass);
}

fn compile_and_validate_with_natives(source: &str) {
    let jass = compile_glass(source);
    validate_jass_with_natives(&jass, true);
}

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
fn type_definitions() {
    compile_and_validate(
        r#"
pub type Phase {
    Lobby
    Playing { wave: Int }
    Victory { winner: Int }
}

pub type Model {
    Model { phase: Int, wave: Int, score: Int }
}

fn test() -> Model {
    Model(phase: 0, wave: 1, score: 100)
}
"#,
    );
}

#[test]
fn case_expression() {
    compile_and_validate(
        r#"
fn check(x: Bool) -> Int {
    case x {
        True -> 1
        False -> 0
    }
}
"#,
    );
}

#[test]
fn let_binding() {
    compile_and_validate(
        r#"
fn add(a: Int, b: Int) -> Int { a + b }

fn test() -> Int {
    let x: Int = 5
    let y: Int = 10
    add(x, y)
}
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
    compile_and_validate(
        r#"
fn pair(a: Int, b: Int) -> Int { #(a, b) }
"#,
    );
}

#[test]
fn record_update() {
    compile_and_validate(
        r#"
pub type Model { Model { wave: Int, score: Int } }
fn bump(m: Int) -> Int { Model(..m, wave: 5) }
"#,
    );
}

#[test]
fn list_literal() {
    compile_and_validate(
        r#"
fn nums() -> Int { [1, 2, 3] }
"#,
    );
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

pub type Msg { Increment Decrement Reset }
pub type Model { Model { count: Int } }

pub fn init() -> #(Model, List(effect.Effect(Msg))) {
    #(Model(count: 0), [])
}

pub fn update(model: Model, msg: Msg) -> #(Model, List(effect.Effect(Msg))) {
    case msg {
        Increment -> #(Model(count: model.count + 1), [])
        Decrement -> #(Model(count: model.count - 1), [])
        Reset -> #(Model(count: 0), [])
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

pub type Msg { Tick GameStart }
pub type Model { Model { count: Int } }

pub fn init() -> #(Model, List(effect.Effect(Msg))) {
    #(Model(count: 0), [
        effect.display_text(0, "Init!", 3.0)
    ])
}

pub fn update(model: Model, msg: Msg) -> #(Model, List(effect.Effect(Msg))) {
    case msg {
        Tick -> #(Model(count: model.count + 1), [
            effect.after(1.0, fn() { Tick })
        ])
        GameStart -> #(model, [])
    }
}
"#,
    );
}

#[test]
fn enum_constructors() {
    compile_and_validate(
        r#"
pub type Phase { Lobby Playing { wave: Int } Victory { winner: Int } }
fn start() -> Int { Playing(wave: 1) }
fn idle() -> Int { Lobby }
"#,
    );
}
