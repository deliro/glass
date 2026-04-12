# Split codegen.rs and infer.rs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split the monolithic `codegen/mod.rs` (~3196 lines) into focused modules and `infer.rs` (~1647 lines of code + ~591 tests) into a module directory, per issues #23 and #24.

**Architecture:** Pure mechanical refactoring — extract logical groups of methods into sub-modules under `codegen/` and `infer/`. No behavioral changes. All existing tests must continue to pass at every step. Each extraction moves `impl JassCodegen` methods (or `impl Inferencer` methods) into a new file with `pub(crate)` visibility where needed, re-exporting nothing — callers already go through the struct.

**Tech Stack:** Rust, `cargo test`, `insta` snapshots

**Working directory:** `/Users/tochkamac/projects/own/glass/.claude/worktrees/agent-ab1cfbe8`

---

## File Structure

### codegen/ (current → target)

| File | Responsibility | Lines (approx) |
|------|---------------|-----------------|
| `mod.rs` | JassCodegen struct, `new()`, `generate()`, `gen_definition`, `gen_fn_def`, `gen_tco_body`, emit/temp helpers, free functions | ~550 |
| `dce.rs` | Dead code elimination + topological sort (already extracted) | 241 |
| `soa.rs` | SoA array generation, alloc/dealloc, constructors, list preamble (rename from `types.rs`) | 258 |
| `closure.rs` | Closure capture resolution, globals/alloc, dispatch function generation | ~490 |
| `expr.rs` | `gen_expr` + expression type helpers + `const_fold_binop` | ~900 |
| `pattern.rs` | Pattern condition codegen, pattern bindings, locals collection | ~500 |
| `mono.rs` | Monomorphization, intrinsic calls, type substitution | ~350 |
| `resolve.rs` | Type-to-JASS mapping, type lookup, list/tuple type resolution | ~150 |

### infer/ (current → target)

| File | Responsibility | Lines (approx) |
|------|---------------|-----------------|
| `mod.rs` | Inferencer struct, `new()`, `infer_module*`, type registration, module-level orchestration | ~500 |
| `expr.rs` | `infer_expr_inner` (the large match), `infer_binop` | ~750 |
| `pattern.rs` | `check_pattern`, `bind_pattern` | ~200 |
| `resolve.rs` | `resolve_type_expr*`, `fresh_subst_for` | ~100 |
| `tests.rs` | All `#[cfg(test)]` code (stays as-is, just moved) | ~591 |

---

## Part 1: codegen/ split

### Task 1: Rename `types.rs` → `soa.rs`

**Files:**
- Rename: `src/codegen/types.rs` → `src/codegen/soa.rs`
- Modify: `src/codegen/mod.rs:2` — update `mod types` → `mod soa`

- [ ] **Step 1: Rename the file**

```bash
cd /Users/tochkamac/projects/own/glass/.claude/worktrees/agent-ab1cfbe8
mv src/codegen/types.rs src/codegen/soa.rs
```

- [ ] **Step 2: Update mod declaration**

In `src/codegen/mod.rs`, change line 2:
```rust
// old
mod types;
// new
mod soa;
```

- [ ] **Step 3: Run tests**

```bash
cargo test
```
Expected: all 40 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/codegen/soa.rs src/codegen/mod.rs
git rm src/codegen/types.rs
git commit -m "refactor(codegen): rename types.rs → soa.rs for clarity"
```

---

### Task 2: Extract `closure.rs`

**Files:**
- Create: `src/codegen/closure.rs`
- Modify: `src/codegen/mod.rs` — remove closure methods, add `mod closure`

Functions to extract (lines 347–836 of mod.rs):
- `ClosureEmitInfo` struct (lines 118–124)
- `resolve_capture_type` (347–369)
- `find_capture_usage_type` (371–419)
- `find_capture_annotation` (421–438)
- `collect_closure_infos` (440–470)
- `pre_collect_pattern_var_types` (472–478)
- `scan_pattern_vars_in_expr` (480–519)
- `scan_pattern_var_bindings` (521–574)
- `gen_closure_globals_and_alloc` (577–608)
- `gen_closure_dispatch` (611–836)

- [ ] **Step 1: Create `closure.rs`**

Create `src/codegen/closure.rs` with the following structure:

```rust
use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::closures::CapturedVar;

use super::{safe_jass_name, JassCodegen};

pub(super) struct ClosureEmitInfo {
    pub(super) id: usize,
    pub(super) captures: Vec<(String, String)>,
    pub(super) param_names: Vec<String>,
    pub(super) param_types: Vec<String>,
    pub(super) has_captures: bool,
}

impl JassCodegen {
    // paste: resolve_capture_type, find_capture_usage_type, find_capture_annotation,
    //        collect_closure_infos, pre_collect_pattern_var_types,
    //        scan_pattern_vars_in_expr, scan_pattern_var_bindings,
    //        gen_closure_globals_and_alloc, gen_closure_dispatch
    //
    // All methods keep their existing signatures.
    // Change visibility from private to pub(crate) where called from mod.rs.
    // Methods called only within closure.rs stay private.
}
```

Methods called from `mod.rs::generate()`:
- `pre_collect_pattern_var_types` → `pub(crate)`
- `gen_closure_globals_and_alloc` → `pub(crate)`
- `gen_closure_dispatch` → `pub(crate)`

All others remain private (called only from within closure methods).

- [ ] **Step 2: Remove moved code from `mod.rs`, add `mod closure`**

Add `mod closure;` after `mod soa;` in mod.rs.

Remove: `ClosureEmitInfo` struct and all 10 functions listed above from the `impl JassCodegen` block.

- [ ] **Step 3: Run tests**

```bash
cargo test
```
Expected: all 40 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/codegen/closure.rs src/codegen/mod.rs
git commit -m "refactor(codegen): extract closure.rs — capture resolution and dispatch"
```

---

### Task 3: Extract `expr.rs`

**Files:**
- Create: `src/codegen/expr.rs`
- Modify: `src/codegen/mod.rs` — remove expression methods, add `mod expr`

Functions to extract:
- `gen_expr` (1165–2021) — the largest single function
- `expr_has_float` (2022–2037)
- `expr_is_string` (2039–2057)
- `infer_case_jass_type` (2059–2075)
- `bare_ctor_name` (2077–2079)
- `full_bare_name` (2081–2084)
- `type_hint_from_ctor_name` (2086–2094)
- `extract_tuple_field_types_from_subject` (2096–2114)
- `tuple_field_types_from_type` (2116–2131)
- `lookup_tuple_field_types` (2133–2159)
- `resolve_variant` (2161–2168)
- `const_fold_binop` (standalone function, 3162–end)

- [ ] **Step 1: Create `expr.rs`**

```rust
use std::collections::HashSet;

use crate::ast::*;
use crate::type_repr::Type;
use crate::types::{TypeInfo, VariantInfo};

use super::{format_float, safe_jass_name, ExternalInfo, JassCodegen};

impl JassCodegen {
    // paste all functions listed above
    // gen_expr stays #[allow(clippy::indexing_slicing)] — it's inherited from original
    // resolve_variant needs pub(crate) — called from closure.rs (scan_pattern_var_bindings)
    //                                     and pattern.rs
}

pub(super) fn const_fold_binop(op: &BinOp, left: &Expr, right: &Expr) -> Option<String> {
    // paste standalone function
}
```

Visibility changes:
- `gen_expr` → `pub(crate)` (called from mod.rs gen_fn_def, gen_tco_body, closure.rs gen_closure_dispatch)
- `resolve_variant` → `pub(crate)` (called from closure.rs, pattern.rs)
- `extract_tuple_field_types_from_subject` → `pub(crate)` (called from mod.rs gen_tco_body)
- `lookup_tuple_field_types` → `pub(crate)` (called from closure.rs, pattern.rs)
- Rest remain private.

- [ ] **Step 2: Remove moved code from `mod.rs`, add `mod expr`**

Add `mod expr;`. Remove all listed functions. Keep `use expr::const_fold_binop;` if needed (it likely isn't — it was only called from gen_expr which is now in expr.rs).

- [ ] **Step 3: Run tests**

```bash
cargo test
```
Expected: all 40 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/codegen/expr.rs src/codegen/mod.rs
git commit -m "refactor(codegen): extract expr.rs — expression and case codegen"
```

---

### Task 4: Extract `pattern.rs`

**Files:**
- Create: `src/codegen/pattern.rs`
- Modify: `src/codegen/mod.rs` — remove pattern methods, add `mod pattern`

Functions to extract:
- `gen_pattern_condition_typed` (2170–2250)
- `gen_let_pattern_binding` (2252–2331)
- `gen_pattern_bindings` (2333–2414)
- `collect_locals` (2418–2565)
- `collect_pattern_locals` (2567–2656)

- [ ] **Step 1: Create `pattern.rs`**

```rust
use std::collections::HashMap;

use crate::ast::*;

use super::{safe_jass_name, JassCodegen};

impl JassCodegen {
    // paste all 5 functions
}
```

Visibility changes:
- `gen_pattern_condition_typed` → `pub(crate)` (called from mod.rs gen_tco_body, expr.rs gen_expr)
- `gen_let_pattern_binding` → `pub(crate)` (called from mod.rs gen_tco_body, expr.rs gen_expr)
- `gen_pattern_bindings` → `pub(crate)` (called from mod.rs gen_tco_body, expr.rs gen_expr)
- `collect_locals` → `pub(crate)` (called from mod.rs gen_fn_def, closure.rs gen_closure_dispatch)
- `collect_pattern_locals` stays private (only called from collect_locals).

- [ ] **Step 2: Remove moved code from `mod.rs`, add `mod pattern`**

- [ ] **Step 3: Run tests**

```bash
cargo test
```
Expected: all 40 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/codegen/pattern.rs src/codegen/mod.rs
git commit -m "refactor(codegen): extract pattern.rs — pattern matching and locals collection"
```

---

### Task 5: Extract `mono.rs`

**Files:**
- Create: `src/codegen/mono.rs`
- Modify: `src/codegen/mod.rs` — remove mono methods, add `mod mono`

Functions to extract:
- `gen_mono_function` (2720–2809)
- `resolve_type_expr_to_type` (2811–2850)
- `type_to_jass_with_subst` (2852–2873)
- `contains_intrinsic_call` (2875–2902)
- `mangle_types` (2904–2911)
- `build_mono_subst` (2913–2937)
- `extract_type_bindings` (2939–2973)
- `resolve_arg_jass_type` (2975–2986)
- `gen_intrinsic_call` (2988–3072)

- [ ] **Step 1: Create `mono.rs`**

```rust
use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::type_repr::{Substitution, Type};

use super::{safe_jass_name, JassCodegen};

impl JassCodegen {
    // paste all 9 functions
}
```

Visibility changes:
- `gen_mono_function` → `pub(crate)` (called from expr.rs gen_expr)
- `contains_intrinsic_call` → `pub(crate)` (called from mod.rs generate)
- `gen_intrinsic_call` → `pub(crate)` (called from expr.rs gen_expr)
- `build_mono_subst` → `pub(crate)` (called from expr.rs gen_expr)
- `mangle_types` → `pub(crate)` (called from expr.rs gen_expr)
- Rest stay private.

- [ ] **Step 2: Remove moved code from `mod.rs`, add `mod mono`**

- [ ] **Step 3: Run tests**

```bash
cargo test
```
Expected: all 40 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/codegen/mono.rs src/codegen/mod.rs
git commit -m "refactor(codegen): extract mono.rs — monomorphization and intrinsics"
```

---

### Task 6: Extract `resolve.rs`

**Files:**
- Create: `src/codegen/resolve.rs`
- Modify: `src/codegen/mod.rs` — remove type resolution methods, add `mod resolve`

Functions to extract:
- `extract_inner_type_name` (2662–2674)
- `type_to_jass` (2676–2681)
- `type_name_to_jass` (2683–2697)
- `dispatch_fn_name` (2699–2705)
- `type_to_jass_from_type` (2707–2718)
- `lookup_type` (3074–3086)
- `lookup_full_type` (3088–3092)
- `var_to_list_elem_type` (3094–3101)
- `extract_list_elem_jass_type` (3103–3115)
- `extract_list_elem_type_from_subject` (3117–3138)
- `infer_list_elem_from_tail` (3140–3160)

- [ ] **Step 1: Create `resolve.rs`**

```rust
use crate::ast::*;
use crate::type_repr::Type;

use super::JassCodegen;

impl JassCodegen {
    // paste all 11 functions
}
```

Visibility: nearly all are `pub(crate)` since they're called from expr.rs, closure.rs, pattern.rs, mono.rs, and mod.rs.

- [ ] **Step 2: Remove moved code from `mod.rs`, add `mod resolve`**

- [ ] **Step 3: Run tests**

```bash
cargo test
```
Expected: all 40 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/codegen/resolve.rs src/codegen/mod.rs
git commit -m "refactor(codegen): extract resolve.rs — type-to-JASS mapping and lookups"
```

---

### Task 7: Verify final codegen/ state

- [ ] **Step 1: Check line counts**

```bash
wc -l src/codegen/*.rs
```

Expected: `mod.rs` should be ~500-600 lines. Total line count should be approximately equal to the original 3196 + overhead from imports.

- [ ] **Step 2: Run full test suite**

```bash
cargo test
```
Expected: all 40 tests pass with no warnings.

- [ ] **Step 3: Run clippy**

```bash
cargo clippy -- -D warnings
```
Expected: no warnings.

---

## Part 2: infer/ split

### Task 8: Convert `infer.rs` to `infer/mod.rs`

**Files:**
- Rename: `src/infer.rs` → `src/infer/mod.rs`

- [ ] **Step 1: Create directory and move**

```bash
mkdir -p src/infer
mv src/infer.rs src/infer/mod.rs
```

- [ ] **Step 2: Run tests**

```bash
cargo test
```
Expected: all tests pass (Rust resolves `mod infer` to either `infer.rs` or `infer/mod.rs`).

- [ ] **Step 3: Commit**

```bash
git add src/infer/mod.rs
git rm src/infer.rs
git commit -m "refactor(infer): convert infer.rs to infer/mod.rs"
```

---

### Task 9: Extract `infer/expr.rs`

**Files:**
- Create: `src/infer/expr.rs`
- Modify: `src/infer/mod.rs`

Functions to extract:
- `infer_expr` (559–563) — thin wrapper, stays in mod.rs
- `infer_expr_inner` (565–1291) — the massive match expression
- `infer_binop` (1468–1512)

- [ ] **Step 1: Create `infer/expr.rs`**

```rust
use crate::ast::*;
use crate::token::Span;
use crate::type_repr::Type;
use crate::type_env::TypeEnv;

use super::Inferencer;

impl Inferencer {
    // paste infer_expr_inner and infer_binop
    // infer_expr_inner: pub(crate) — called from mod.rs infer_expr
    // infer_binop: stays private (only called from infer_expr_inner)
}
```

- [ ] **Step 2: Remove from mod.rs, add `mod expr`**

Keep `infer_expr` in mod.rs (it's a thin public wrapper calling `self.infer_expr_inner`).

- [ ] **Step 3: Run tests**

```bash
cargo test
```

- [ ] **Step 4: Commit**

```bash
git add src/infer/expr.rs src/infer/mod.rs
git commit -m "refactor(infer): extract expr.rs — expression type inference"
```

---

### Task 10: Extract `infer/pattern.rs`

**Files:**
- Create: `src/infer/pattern.rs`
- Modify: `src/infer/mod.rs`

Functions to extract:
- `check_pattern` (1292–1462)
- `bind_pattern` (1463–1466)

- [ ] **Step 1: Create `infer/pattern.rs`**

```rust
use crate::ast::*;
use crate::token::Span;
use crate::type_repr::Type;
use crate::type_env::TypeEnv;

use super::Inferencer;

impl Inferencer {
    // paste check_pattern (pub(crate)) and bind_pattern (pub(crate))
}
```

- [ ] **Step 2: Remove from mod.rs, add `mod pattern`**

- [ ] **Step 3: Run tests**

```bash
cargo test
```

- [ ] **Step 4: Commit**

```bash
git add src/infer/pattern.rs src/infer/mod.rs
git commit -m "refactor(infer): extract pattern.rs — pattern type checking"
```

---

### Task 11: Extract `infer/resolve.rs`

**Files:**
- Create: `src/infer/resolve.rs`
- Modify: `src/infer/mod.rs`

Functions to extract:
- `resolve_type_expr` (1514–1517)
- `resolve_type_expr_with_tvars` (1519–1525)
- `resolve_type_expr_inner` (1527–1603)
- `resolve_type_expr_static` (1604–1632)
- `resolve_type_expr_to_type` (1633–1636)
- `fresh_subst_for` (1638–1645)

- [ ] **Step 1: Create `infer/resolve.rs`**

```rust
use crate::ast::*;
use crate::type_repr::{Substitution, Type};

use super::Inferencer;

impl Inferencer {
    // paste all 6 functions
    // resolve_type_expr: pub (already public)
    // resolve_type_expr_with_tvars: pub (already public)
    // resolve_type_expr_static: pub (already public)
    // resolve_type_expr_inner: private
    // resolve_type_expr_to_type: private
    // fresh_subst_for: private
}
```

- [ ] **Step 2: Remove from mod.rs, add `mod resolve`**

- [ ] **Step 3: Run tests**

```bash
cargo test
```

- [ ] **Step 4: Commit**

```bash
git add src/infer/resolve.rs src/infer/mod.rs
git commit -m "refactor(infer): extract resolve.rs — type expression resolution"
```

---

### Task 12: Verify final infer/ state

- [ ] **Step 1: Check line counts**

```bash
wc -l src/infer/*.rs
```

Expected: `mod.rs` ~1100 lines (including ~591 lines of tests). Non-test code in mod.rs ~500 lines.

- [ ] **Step 2: Run full test suite + clippy**

```bash
cargo test && cargo clippy -- -D warnings
```

Expected: all tests pass, no warnings.

- [ ] **Step 3: Commit final state or squash if needed**

Review git log for the branch — ensure all commits are clean and descriptive.
