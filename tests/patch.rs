#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_ID: AtomicU64 = AtomicU64::new(0);

fn unique_id() -> u64 {
    TEST_ID.fetch_add(1, Ordering::Relaxed)
}

/// Locate pw.w3x relative to CARGO_MANIFEST_DIR.
/// Returns None if the file doesn't exist (e.g. in a worktree without pw/).
fn pw_w3x_path() -> Option<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = manifest.join("pw").join("pw.w3x");
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

fn pudge_wars_main() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("pudge_wars")
        .join("main.glass")
}

/// Run `glass patch` and return the output file path.
fn run_patch(map: &Path, input: &Path, target: &str) -> PathBuf {
    let output = std::env::temp_dir().join(format!("glass_patch_test_{}.w3x", unique_id()));

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_glass"));
    cmd.arg("patch")
        .arg(map)
        .arg(input)
        .arg(&output)
        .arg("--target")
        .arg(target)
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());

    let result = cmd.output().expect("failed to run glass patch");

    assert!(
        result.status.success(),
        "glass patch failed (target={target}):\nstderr: {}\nstdout: {}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout),
    );

    output
}

/// Verify the output file has the expected HM3W header at offset 0
/// and MPQ signature at offset 0x200.
fn assert_w3x_structure(path: &Path) {
    let data = std::fs::read(path).expect("failed to read output w3x");

    // Must be large enough to contain both headers
    assert!(
        data.len() > 0x204,
        "output file too small: {} bytes",
        data.len()
    );

    // HM3W magic at offset 0
    let hm3w = data.get(0..4).expect("file shorter than 4 bytes");
    assert_eq!(hm3w, b"HM3W", "expected HM3W header at offset 0");

    // MPQ signature at offset 0x200
    let mpq_sig = data.get(0x200..0x204).expect("file shorter than 0x204 bytes");
    assert_eq!(
        mpq_sig, b"MPQ\x1a",
        "expected MPQ signature at offset 0x200"
    );
}

#[test]
fn patch_pw_w3x_with_pudge_wars() {
    let Some(map) = pw_w3x_path() else {
        eprintln!("pw.w3x not found, skipping patch test (JASS)");
        return;
    };
    let input = pudge_wars_main();
    let output = run_patch(&map, &input, "jass");
    assert_w3x_structure(&output);
    std::fs::remove_file(&output).ok();
}

#[test]
fn patch_pw_w3x_lua_target() {
    let Some(map) = pw_w3x_path() else {
        eprintln!("pw.w3x not found, skipping patch test (Lua)");
        return;
    };
    let input = pudge_wars_main();
    let output = run_patch(&map, &input, "lua");
    assert_w3x_structure(&output);
    std::fs::remove_file(&output).ok();
}
