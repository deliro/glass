# `glass patch` Command Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `glass patch` CLI subcommand that compiles Glass source and injects it into a .w3x (MPQ) map archive, replacing `war3map.j` (JASS) or `war3map.lua` (Lua).

**Architecture:** New `mpq` module handles MPQ archive read/write via `mpq-rs` crate (pure Rust, git dependency). The `patch` subcommand reuses the existing compilation pipeline from `cmd_compile`, then opens the original .w3x, copies all files (with the script file replaced), and writes a new .w3x. Pre-MPQ data (HM3W header at offset 0x000–0x1FF) is preserved byte-for-byte. If `(listfile)` is missing (protected maps), a fallback probes standard WC3 filenames.

**Tech Stack:** Rust, mpq-rs (git dep), clap (existing)

---

## File Structure

| File | Responsibility |
|------|----------------|
| `src/mpq.rs` (create) | MPQ archive operations: open w3x, read files, write patched w3x. Detailed logging via `eprintln!`. Fallback file list for protected maps. |
| `src/main.rs` (modify) | Add `Patch` variant to `Command` enum, `cmd_patch` function, wire up CLI args. |
| `Cargo.toml` (modify) | Add `mpq-rs` git dependency. |
| `tests/patch.rs` (create) | Integration test: patch pw.w3x with pudge_wars example, validate output is valid MPQ with correct war3map.j. |

---

### Task 1: Add mpq-rs dependency

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add mpq-rs git dependency to Cargo.toml**

Add under `[dependencies]`:

```toml
mpq-rs = { git = "https://github.com/WarRaft/mpq-rs", package = "mpq-rs" }
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: compiles successfully (mpq-rs fetched and available)

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "deps: add mpq-rs for MPQ archive support"
```

---

### Task 2: Create `src/mpq.rs` — MPQ read/write with detailed logging

**Files:**
- Create: `src/mpq.rs`
- Modify: `src/main.rs` (add `mod mpq;`)

This is the core module. It must log every significant decision to stderr so the user can diagnose issues with any map.

- [ ] **Step 1: Write the `src/mpq.rs` module with `patch_w3x` function**

```rust
use mpq_rs::{Archive, Creator, FileOptions, MpqError};
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

/// Standard WC3 map files to probe when (listfile) is missing.
/// Order doesn't matter — we try each one and keep what exists.
const WC3_STANDARD_FILES: &[&str] = &[
    "war3map.j",
    "war3map.lua",
    "war3map.w3e",
    "war3map.w3i",
    "war3map.wtg",
    "war3map.wct",
    "war3map.wts",
    "war3map.w3r",
    "war3map.w3c",
    "war3map.w3s",
    "war3map.w3u",
    "war3map.w3t",
    "war3map.w3a",
    "war3map.w3b",
    "war3map.w3d",
    "war3map.w3q",
    "war3map.w3h",
    "war3map.doo",
    "war3map.shd",
    "war3mapMap.blp",
    "war3mapMap.b00",
    "war3mapMap.tga",
    "war3mapPath.tga",
    "war3mapPreview.tga",
    "war3mapPreview.blp",
    "war3map.mmp",
    "war3mapMisc.txt",
    "war3mapSkin.txt",
    "war3mapExtra.txt",
    "war3mapUnits.doo",
    "Scripts\\war3map.j",
    "(listfile)",
    "(attributes)",
    "(signature)",
];

/// The MPQ header boundary (512 bytes). W3X files typically have
/// an HM3W header occupying the first 0x200 bytes.
const HEADER_BOUNDARY: usize = 512;

/// HM3W magic bytes at offset 0 of a .w3x file.
const HM3W_MAGIC: &[u8; 4] = b"HM3W";

#[derive(Debug)]
pub enum PatchError {
    Io(std::io::Error),
    Mpq(MpqError),
    NoFiles,
    InputTooSmall,
}

impl std::fmt::Display for PatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PatchError::Io(e) => write!(f, "I/O error: {e}"),
            PatchError::Mpq(e) => write!(f, "MPQ error: {e}"),
            PatchError::NoFiles => write!(f, "no files found in archive (listfile missing and fallback found nothing)"),
            PatchError::InputTooSmall => write!(f, "input file is too small to be a valid .w3x"),
        }
    }
}

impl From<std::io::Error> for PatchError {
    fn from(e: std::io::Error) -> Self {
        PatchError::Io(e)
    }
}

impl From<MpqError> for PatchError {
    fn from(e: MpqError) -> Self {
        PatchError::Mpq(e)
    }
}

/// Patch a .w3x map: replace the script file with compiled Glass output.
///
/// `script_filename` is `"war3map.j"` for JASS or `"war3map.lua"` for Lua.
pub fn patch_w3x(
    input_path: &Path,
    output_path: &Path,
    script_filename: &str,
    compiled_script: &[u8],
) -> Result<(), PatchError> {
    let input_data = std::fs::read(input_path)?;
    eprintln!("[patch] input: {} ({} bytes)", input_path.display(), input_data.len());

    if input_data.len() < HEADER_BOUNDARY {
        return Err(PatchError::InputTooSmall);
    }

    // Detect and preserve pre-MPQ header (HM3W)
    let pre_mpq_data = detect_pre_mpq_header(&input_data);
    let mpq_offset = pre_mpq_data.len();

    // Open the MPQ portion
    let mpq_slice = input_data.get(mpq_offset..).ok_or(PatchError::InputTooSmall)?;
    eprintln!("[patch] opening MPQ archive at offset 0x{mpq_offset:X} ({mpq_offset} bytes)");
    let cursor = Cursor::new(mpq_slice);
    let mut archive = Archive::open(BufReader::new(cursor))?;
    eprintln!("[patch] MPQ archive opened successfully");

    // Enumerate files
    let file_list = build_file_list(&mut archive);

    // Read all files from original archive
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut replaced = false;

    for name in &file_list {
        if name == script_filename || (script_filename == "war3map.j" && name == "Scripts\\war3map.j") {
            eprintln!("[patch] replacing: {name} ({} bytes -> {} bytes)", 
                match archive.read_file(name) {
                    Ok(data) => data.len(),
                    Err(_) => 0,
                },
                compiled_script.len()
            );
            files.push((name.clone(), compiled_script.to_vec()));
            replaced = true;
        } else if name == "(listfile)" {
            // Creator generates its own (listfile), skip the original
            eprintln!("[patch] skipping: {name} (Creator auto-generates)");
        } else {
            match archive.read_file(name) {
                Ok(data) => {
                    eprintln!("[patch] copying: {name} ({} bytes)", data.len());
                    files.push((name.clone(), data));
                }
                Err(e) => {
                    eprintln!("[patch] WARNING: failed to read {name}: {e} — skipping");
                }
            }
        }
    }

    if !replaced {
        eprintln!("[patch] script file {script_filename} not found in archive, adding as new file");
        files.push((script_filename.to_string(), compiled_script.to_vec()));
    }

    if files.is_empty() {
        return Err(PatchError::NoFiles);
    }

    // Create new MPQ archive
    eprintln!("[patch] creating new MPQ archive with {} files", files.len());
    let mut creator = Creator::default();
    for (name, data) in &files {
        creator.add_file(
            name,
            data,
            FileOptions {
                encrypt: false,
                compress: true,
                adjust_key: false,
            },
        );
    }

    let mut mpq_buffer = Cursor::new(Vec::new());
    creator.write(&mut mpq_buffer)?;
    let mpq_bytes = mpq_buffer.into_inner();
    eprintln!("[patch] new MPQ archive: {} bytes", mpq_bytes.len());

    // Write output: pre-MPQ header + new MPQ
    let mut output = Vec::with_capacity(pre_mpq_data.len() + mpq_bytes.len());
    output.extend_from_slice(pre_mpq_data);
    output.extend_from_slice(&mpq_bytes);

    std::fs::write(output_path, &output)?;
    eprintln!(
        "[patch] written: {} ({} bytes = {} pre-MPQ + {} MPQ)",
        output_path.display(),
        output.len(),
        pre_mpq_data.len(),
        mpq_bytes.len()
    );

    Ok(())
}

/// Detect and return the pre-MPQ header bytes.
/// W3X files start with HM3W magic followed by map metadata,
/// with the MPQ archive at offset 0x200.
fn detect_pre_mpq_header(data: &[u8]) -> &[u8] {
    if data.len() >= 4 && data.get(..4) == Some(HM3W_MAGIC) {
        eprintln!("[patch] detected HM3W header (Warcraft III map)");

        // Scan for MPQ magic at HEADER_BOUNDARY intervals
        let mut offset = HEADER_BOUNDARY;
        while offset + 4 <= data.len() {
            if data.get(offset..offset + 4) == Some(&[0x4D, 0x50, 0x51, 0x1A]) {
                eprintln!("[patch] found MPQ signature at offset 0x{offset:X}");
                return data.get(..offset).unwrap_or(data);
            }
            offset += HEADER_BOUNDARY;
        }
        eprintln!("[patch] WARNING: HM3W header present but no MPQ signature found at any 0x200 boundary");
        // Return empty — let mpq-rs try from the start
        &[]
    } else {
        eprintln!("[patch] no HM3W header — treating entire file as MPQ archive");
        &[]
    }
}

/// Build a file list from the archive. Tries (listfile) first,
/// falls back to probing standard WC3 filenames.
fn build_file_list(archive: &mut Archive<BufReader<Cursor<&[u8]>>>) -> Vec<String> {
    // Try (listfile) first
    if let Some(files) = archive.files() {
        if !files.is_empty() {
            eprintln!("[patch] (listfile) found: {} entries", files.len());
            for f in &files {
                eprintln!("[patch]   - {f}");
            }
            return files;
        }
        eprintln!("[patch] (listfile) found but empty");
    } else {
        eprintln!("[patch] (listfile) not found — map may be protected/obfuscated");
    }

    // Fallback: probe standard WC3 file names
    eprintln!("[patch] falling back to standard WC3 filename probing...");
    let mut found = Vec::new();
    for name in WC3_STANDARD_FILES {
        match archive.read_file(name) {
            Ok(_) => {
                eprintln!("[patch]   probed: {name} — FOUND");
                found.push((*name).to_string());
            }
            Err(_) => {
                // not found, silently skip
            }
        }
    }
    eprintln!("[patch] fallback found {} files", found.len());
    found
}
```

- [ ] **Step 2: Register the module in `main.rs`**

Add `mod mpq;` to the module declarations at the top of `src/main.rs` (after `mod modules;`):

```rust
mod mpq;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: compiles (module registered, no callers yet)

- [ ] **Step 4: Commit**

```bash
git add src/mpq.rs src/main.rs
git commit -m "feat: add mpq module for w3x archive read/write"
```

---

### Task 3: Add `Patch` CLI subcommand to `main.rs`

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add `Patch` variant to `Command` enum**

In the `Command` enum (after the `Lsp` variant), add:

```rust
    /// Patch a .w3x map: compile Glass source and replace the script file
    Patch {
        /// Input .w3x map file
        #[arg(value_name = "MAP")]
        map: String,

        /// Glass source file to compile
        #[arg(value_name = "INPUT")]
        input: String,

        /// Output .w3x file
        #[arg(value_name = "OUTPUT")]
        output: String,

        /// Compilation target (determines war3map.j vs war3map.lua)
        #[arg(long, value_enum, default_value_t = Target::Jass)]
        target: Target,

        /// Disable name mangling
        #[arg(long)]
        no_mangle: bool,

        /// Keep blank lines and comments
        #[arg(long)]
        no_strip: bool,

        /// Disable tail call optimization
        #[arg(long)]
        no_tco: bool,

        /// Disable lambda lifting
        #[arg(long)]
        no_lift: bool,

        /// Disable function inlining
        #[arg(long)]
        no_inline: bool,

        /// Disable beta reduction
        #[arg(long)]
        no_beta: bool,

        /// Disable constant propagation
        #[arg(long)]
        no_const_prop: bool,
    },
```

- [ ] **Step 2: Add `cmd_patch` function**

Add the function after `cmd_compile`:

```rust
fn cmd_patch(
    map: &str,
    input: &str,
    output: &str,
    target: Target,
    opt: &optimize::OptFlags,
) {
    // Compile Glass source
    eprintln!("[patch] compiling {} -> {:?}", input, target);
    let compiled = compile_to_string(input, target, opt);
    eprintln!("[patch] compilation successful ({} bytes)", compiled.len());

    // Determine script filename based on target
    let script_filename = match target {
        Target::Jass => "war3map.j",
        Target::Lua => "war3map.lua",
    };

    // Patch the map
    let map_path = std::path::Path::new(map);
    let output_path = std::path::Path::new(output);
    if let Err(e) = mpq::patch_w3x(map_path, output_path, script_filename, compiled.as_bytes()) {
        eprintln!("[patch] error: {e}");
        std::process::exit(1);
    }
    eprintln!("[patch] done!");
}
```

- [ ] **Step 3: Extract `compile_to_string` from `cmd_compile`**

Refactor `cmd_compile` to extract the compilation logic into a reusable function. Add this function before `cmd_compile`:

```rust
/// Compile a Glass source file to a JASS or Lua string.
fn compile_to_string(input: &str, target: Target, opt: &optimize::OptFlags) -> String {
    let source = read_file(input);
    let module = parse_source(input, &source);
    let (mut module, imports, imported_count, def_module_map) = resolve_imports(input, module);

    resolve_const_patterns::resolve_const_patterns(&mut module);

    let mut inferencer = infer::Inferencer::new();
    let infer_result = inferencer.infer_module_with_imports(&module, &imports, &def_module_map);

    let error_count = run_checks_with_result(
        input,
        &source,
        &module,
        &imports,
        &infer_result,
        &inferencer,
        imported_count,
    );
    if error_count > 0 {
        std::process::exit(1);
    }

    if opt.tco {
        tco::apply_tco(&mut module);
    }
    if opt.lift {
        lift::apply_lambda_lifting(&mut module);
    }
    if opt.beta {
        beta::apply_beta_reduction(&mut module);
    }
    if opt.const_prop {
        const_prop::apply_const_propagation(&mut module);
    }
    if opt.inline {
        inline::apply_inlining(&mut module);
    }

    let type_registry = TypeRegistry::from_module(&module);
    let mut lambda_collector = closures::LambdaCollector::new();
    lambda_collector.collect_module(&module);

    let name_table = if opt.mangle {
        Some(optimize::build_name_table(
            &module,
            &type_registry,
            &lambda_collector.lambdas,
        ))
    } else {
        None
    };

    let mut result = match target {
        Target::Jass => JassCodegen::new(
            type_registry,
            lambda_collector.lambdas,
            infer_result.type_map,
            inferencer.type_param_vars.clone(),
        )
        .generate(&module, &imports),
        Target::Lua => LuaCodegen::new(
            type_registry,
            lambda_collector.lambdas,
            infer_result.type_map,
            inferencer.type_param_vars.clone(),
        )
        .generate(&module, &imports),
    };

    if let Some(table) = &name_table {
        result = table.apply(&result);
    }
    if opt.strip {
        result = optimize::strip_whitespace_and_comments(&result);
    }
    result.replace('\n', "\r\n")
}
```

Then simplify `cmd_compile` to call `compile_to_string`:

```rust
fn cmd_compile(
    input: &str,
    output: Option<&str>,
    no_check: bool,
    target: Target,
    opt: &optimize::OptFlags,
) {
    // If no_check, use the original direct-compilation path (skips type checking)
    if no_check {
        // ... keep existing no_check path unchanged ...
    }

    let result = compile_to_string(input, target, opt);

    match output {
        Some(path) => {
            if let Err(e) = std::fs::write(path, &result) {
                eprintln!("Error writing {}: {}", path, e);
                std::process::exit(1);
            }
        }
        None => print!("{}", result),
    }
}
```

Note: `cmd_compile` has a `no_check` code path that skips type checking. `compile_to_string` always type-checks (the patch command should always type-check — we're producing a real map). Keep the `no_check` path in `cmd_compile` for backward compat, but `compile_to_string` does not offer `no_check`.

- [ ] **Step 4: Wire up the Patch command in `main()`**

In the `match cli.command` block, add the `Patch` arm:

```rust
        Some(Command::Patch {
            map,
            input,
            output,
            target,
            no_mangle,
            no_strip,
            no_tco,
            no_lift,
            no_inline,
            no_beta,
            no_const_prop,
        }) => {
            let opt = optimize::OptFlags {
                mangle: !no_mangle,
                strip: !no_strip,
                tco: !no_tco,
                lift: !no_lift,
                inline: !no_inline,
                beta: !no_beta,
                const_prop: !no_const_prop,
            };
            cmd_patch(&map, &input, &output, target, &opt);
        }
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo check`
Expected: compiles

- [ ] **Step 6: Manual smoke test with pw.w3x**

Run:
```bash
cargo run -- patch pw/pw.w3x examples/pudge_wars/main.glass /tmp/pw_patched.w3x
```

Expected: detailed log output to stderr showing each step. Output file created at `/tmp/pw_patched.w3x`.

- [ ] **Step 7: Commit**

```bash
git add src/main.rs
git commit -m "feat: add glass patch command for w3x map patching"
```

---

### Task 4: Validate the patched .w3x output

**Files:** (no new files — manual validation)

After the smoke test produces `/tmp/pw_patched.w3x`, we need to verify the output is a valid MPQ that can be reopened.

- [ ] **Step 1: Verify patched file can be opened by mpq-rs**

Write a quick validation by running a second `glass patch` on the output to confirm it's readable, or add a temporary test. The simplest check: the file starts with `HM3W` and contains a valid MPQ.

Run:
```bash
xxd -l 4 /tmp/pw_patched.w3x  # Should show HM3W
xxd -s 512 -l 4 /tmp/pw_patched.w3x  # Should show MPQ\x1a
```

- [ ] **Step 2: Verify war3map.j content in the patched archive**

We can verify by re-reading the patched archive in a test. For now, verify file size is reasonable (should be similar to original, possibly smaller due to recompression).

```bash
ls -la pw/pw.w3x /tmp/pw_patched.w3x
```

---

### Task 5: Integration test

**Files:**
- Create: `tests/patch.rs`

- [ ] **Step 1: Write the integration test**

```rust
use std::process::Command;

/// Test that `glass patch` can patch pw.w3x with pudge_wars example code.
/// Verifies:
/// 1. The command succeeds (exit code 0)
/// 2. Output file is created
/// 3. Output starts with HM3W header
/// 4. Output contains MPQ signature at expected offset
#[test]
fn patch_pw_w3x_with_pudge_wars() {
    let glass = env!("CARGO_BIN_EXE_glass");
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let map_path = manifest_dir.join("pw").join("pw.w3x");
    let input_path = manifest_dir.join("examples").join("pudge_wars").join("main.glass");

    // Skip if pw.w3x doesn't exist (it's a large binary, may not be in CI)
    if !map_path.exists() {
        eprintln!("pw.w3x not found at {:?}, skipping patch test", map_path);
        return;
    }

    let output_path = std::env::temp_dir().join("glass_patch_test.w3x");

    let output = Command::new(glass)
        .arg("patch")
        .arg(&map_path)
        .arg(&input_path)
        .arg(&output_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to run glass patch");

    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("--- glass patch stderr ---\n{stderr}");

    assert!(
        output.status.success(),
        "glass patch failed with exit code {:?}\nstderr: {stderr}",
        output.status.code()
    );

    // Verify output file exists and has content
    let patched = std::fs::read(&output_path).expect("read patched file");
    assert!(patched.len() > 512, "patched file too small: {} bytes", patched.len());

    // Verify HM3W header preserved
    assert_eq!(
        &patched[..4],
        b"HM3W",
        "patched file should start with HM3W header"
    );

    // Verify MPQ signature at offset 0x200
    assert_eq!(
        &patched[0x200..0x204],
        &[0x4D, 0x50, 0x51, 0x1A],
        "patched file should have MPQ signature at offset 0x200"
    );

    // Cleanup
    let _ = std::fs::remove_file(&output_path);
}

/// Test that `glass patch` works with --target lua.
#[test]
fn patch_pw_w3x_lua_target() {
    let glass = env!("CARGO_BIN_EXE_glass");
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let map_path = manifest_dir.join("pw").join("pw.w3x");
    let input_path = manifest_dir.join("examples").join("pudge_wars").join("main.glass");

    if !map_path.exists() {
        eprintln!("pw.w3x not found, skipping");
        return;
    }

    let output_path = std::env::temp_dir().join("glass_patch_lua_test.w3x");

    let output = Command::new(glass)
        .arg("patch")
        .arg(&map_path)
        .arg(&input_path)
        .arg(&output_path)
        .arg("--target")
        .arg("lua")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to run glass patch");

    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("--- glass patch --target lua stderr ---\n{stderr}");

    assert!(
        output.status.success(),
        "glass patch --target lua failed: {stderr}"
    );

    let patched = std::fs::read(&output_path).expect("read patched file");
    assert!(patched.len() > 512);
    assert_eq!(&patched[..4], b"HM3W");

    let _ = std::fs::remove_file(&output_path);
}
```

- [ ] **Step 2: Run the integration test**

Run: `cargo test --test patch -- --nocapture`
Expected: both tests pass, stderr shows detailed patch logs

- [ ] **Step 3: Commit**

```bash
git add tests/patch.rs
git commit -m "test: integration tests for glass patch command"
```

---

### Task 6: Fix issues found during testing

**Files:** Varies based on findings

- [ ] **Step 1: Address any clippy warnings**

Run: `cargo clippy --all-targets`
Fix any issues. The project has strict clippy lints (deny all, no unwrap/expect/indexing).

- [ ] **Step 2: Run full test suite to check for regressions**

Run: `cargo test`
Expected: all existing tests still pass, new patch tests pass.

- [ ] **Step 3: Final commit**

```bash
git add -A
git commit -m "fix: address clippy warnings and test issues"
```
