# Architectural Refactoring (P0 + P1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate DRY violations, fix silent correctness bugs, and remove KISS violations from the Glass compiler.

**Architecture:** Extract shared `free_vars` module from duplicated code in `lift.rs`/`closures.rs`. Add ambiguity diagnostics to the type inferencer. Fix `ARCH_TODO.md` item #14 — the unwrap/panic count was overcounted (almost all are in `#[cfg(test)]`; production code is clean).

**Tech Stack:** Rust (stable), `cargo test`, `cargo clippy`

---

## Task 1: Extract shared `free_vars` module (ARCH_TODO #1)

**Files:**
- Create: `src/free_vars.rs`
- Modify: `src/lift.rs:399-548` — remove `bind_pattern` + `find_free_vars`, import from `free_vars`
- Modify: `src/closures.rs:170-283` — remove `bind_pattern` + `find_free_vars`, import from `free_vars`
- Modify: `src/main.rs:8` — add `mod free_vars;` between `mod exhaustive;` and `mod infer;`

`lift.rs` has the more complete version (handles guards in case arms, handles `TcoLoop`/`TcoContinue`, handles `ConstructorNamed` with nested patterns and `Or` patterns). `closures.rs` is a subset that uses catch-all `_ => {}` in both `bind_pattern` and `find_free_vars` — meaning it silently ignores `TcoLoop`, `TcoContinue`, `ConstructorNamed` in `bind_pattern`, `Or` patterns in `bind_pattern`, `List` in `bind_pattern` (only handles `Tuple`), and guards in case arms for `find_free_vars`.

The extracted module should use `lift.rs`'s complete version.

- [ ] **Step 1: Create `src/free_vars.rs` with the shared implementations**

```rust
use std::collections::HashSet;

use crate::ast::*;

pub fn bind_pattern(pattern: &Pattern, scope: &mut HashSet<String>) {
    match pattern {
        Pattern::Var(name) => {
            scope.insert(name.clone());
        }
        Pattern::Constructor { args, .. } => {
            for arg in args {
                bind_pattern(&arg.node, scope);
            }
        }
        Pattern::ConstructorNamed { fields, .. } => {
            for f in fields {
                if let Some(p) = &f.pattern {
                    bind_pattern(&p.node, scope);
                } else {
                    scope.insert(f.field_name.clone());
                }
            }
        }
        Pattern::Tuple(elems) | Pattern::List(elems) => {
            for e in elems {
                bind_pattern(&e.node, scope);
            }
        }
        Pattern::ListCons { head, tail } => {
            bind_pattern(&head.node, scope);
            bind_pattern(&tail.node, scope);
        }
        Pattern::As { pattern, name } => {
            bind_pattern(&pattern.node, scope);
            scope.insert(name.clone());
        }
        Pattern::Or(alts) => {
            for a in alts {
                bind_pattern(&a.node, scope);
            }
        }
        Pattern::Discard
        | Pattern::Int(_)
        | Pattern::String(_)
        | Pattern::Bool(_)
        | Pattern::Rawcode(_) => {}
    }
}

pub fn find_free_vars(expr: &Expr, scope: &HashSet<String>, free: &mut Vec<String>) {
    match expr {
        Expr::Var(name) => {
            if !scope.contains(name) {
                free.push(name.clone());
            }
        }
        Expr::Let {
            value,
            body,
            pattern,
            ..
        } => {
            find_free_vars(&value.node, scope, free);
            let mut new_scope = scope.clone();
            bind_pattern(&pattern.node, &mut new_scope);
            find_free_vars(&body.node, &new_scope, free);
        }
        Expr::Case { subject, arms } => {
            find_free_vars(&subject.node, scope, free);
            for arm in arms {
                let mut arm_scope = scope.clone();
                bind_pattern(&arm.pattern.node, &mut arm_scope);
                if let Some(guard) = &arm.guard {
                    find_free_vars(&guard.node, &arm_scope, free);
                }
                find_free_vars(&arm.body.node, &arm_scope, free);
            }
        }
        Expr::BinOp { left, right, .. } | Expr::Pipe { left, right } => {
            find_free_vars(&left.node, scope, free);
            find_free_vars(&right.node, scope, free);
        }
        Expr::UnaryOp { operand, .. } | Expr::Clone(operand) => {
            find_free_vars(&operand.node, scope, free);
        }
        Expr::Call { function, args } => {
            find_free_vars(&function.node, scope, free);
            for a in args {
                find_free_vars(&a.node, scope, free);
            }
        }
        Expr::FieldAccess { object, .. } => {
            find_free_vars(&object.node, scope, free);
        }
        Expr::MethodCall { object, args, .. } => {
            find_free_vars(&object.node, scope, free);
            for a in args {
                find_free_vars(&a.node, scope, free);
            }
        }
        Expr::Block(exprs) => {
            let mut block_scope = scope.clone();
            for e in exprs {
                find_free_vars(&e.node, &block_scope, free);
                if let Expr::Let { pattern, .. } = &e.node {
                    bind_pattern(&pattern.node, &mut block_scope);
                }
            }
        }
        Expr::Tuple(elems) | Expr::List(elems) => {
            for e in elems {
                find_free_vars(&e.node, scope, free);
            }
        }
        Expr::ListCons { head, tail } => {
            find_free_vars(&head.node, scope, free);
            find_free_vars(&tail.node, scope, free);
        }
        Expr::Lambda { params, body, .. } => {
            let mut inner = scope.clone();
            for p in params {
                inner.insert(p.name.clone());
            }
            find_free_vars(&body.node, &inner, free);
        }
        Expr::Constructor { args, .. } => {
            for a in args {
                match a {
                    ConstructorArg::Positional(e) | ConstructorArg::Named(_, e) => {
                        find_free_vars(&e.node, scope, free);
                    }
                }
            }
        }
        Expr::RecordUpdate { base, updates, .. } => {
            find_free_vars(&base.node, scope, free);
            for (_, e) in updates {
                find_free_vars(&e.node, scope, free);
            }
        }
        Expr::TcoLoop { body } => find_free_vars(&body.node, scope, free),
        Expr::TcoContinue { args } => {
            for (_, e) in args {
                find_free_vars(&e.node, scope, free);
            }
        }
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::String(_)
        | Expr::Rawcode(_)
        | Expr::Bool(_)
        | Expr::Todo(_) => {}
    }
}
```

- [ ] **Step 2: Add `mod free_vars;` to `src/main.rs`**

Add between `mod exhaustive;` (line 8) and `mod infer;` (line 9) to maintain alphabetical order:
```rust
mod free_vars;
```

- [ ] **Step 3: Update `src/lift.rs` — remove local `bind_pattern` + `find_free_vars`, use shared module**

Remove lines 399-548 (the `bind_pattern` and `find_free_vars` functions).

Replace all calls:
- `bind_pattern(` → `crate::free_vars::bind_pattern(`
- `find_free_vars(` → `crate::free_vars::find_free_vars(`

Keep `use crate::ast::*;` — it's still needed for `lift_expr` and other functions.

- [ ] **Step 4: Update `src/closures.rs` — remove local `bind_pattern` + `find_free_vars`, use shared module**

Remove lines 170-283 (the `bind_pattern` and `find_free_vars` methods on `LambdaCollector`).

In `collect_expr` method where `Self::find_free_vars(...)` and `Self::bind_pattern(...)` are called, replace with:
- `Self::find_free_vars(` → `crate::free_vars::find_free_vars(`
- `Self::bind_pattern(` → `crate::free_vars::bind_pattern(`

- [ ] **Step 5: Run tests**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests pass

- [ ] **Step 6: Run clippy + fmt**

Run: `cargo fmt && cargo clippy -- -D warnings 2>&1 | tail -10`
Expected: clean

- [ ] **Step 7: Commit**

```bash
git add src/free_vars.rs src/lift.rs src/closures.rs src/main.rs
git commit -m "refactor: extract shared free_vars module from lift.rs and closures.rs"
```

---

## Task 2: Add ambiguity diagnostic for module collisions (ARCH_TODO #18)

**Files:**
- Modify: `src/infer.rs:39-68` — add `ambiguous_names` field to `Inferencer` struct and `new()`
- Modify: `src/infer.rs:162-167` — record collision when unqualified name is dropped
- Modify: `src/infer.rs:205-210` — same for externals
- Modify: `src/infer.rs:522-531` — emit better error message for ambiguous names

Currently when two imported modules export the same name (e.g., `int.min` and `float.min`), the inferencer silently drops the unqualified binding. The user then sees "undefined variable `min`" with no hint to use `int.min` or `float.min`.

- [ ] **Step 1: Add `ambiguous_names` field to `Inferencer` struct (`infer.rs:39-55`)**

Add field after `const_types` (line 54):
```rust
pub ambiguous_names: HashMap<String, Vec<String>>,
```

Initialize in `Inferencer::new()` (line 58-68), add after `const_types: HashMap::new(),`:
```rust
ambiguous_names: HashMap::new(),
```

- [ ] **Step 2: Record collisions during function registration (`infer.rs:162-167`)**

After the existing `if !collides { env.bind(...) }` block (line 165-167), add:
```rust
if collides {
    if let Some(imps) = name_to_modules.get(f.name.as_str()) {
        let modules: Vec<String> = imps.iter().map(|i| i.module_name.clone()).collect();
        self.ambiguous_names.insert(f.name.clone(), modules);
    }
}
```

Do the same for externals at line ~205-210.

- [ ] **Step 3: Use collision set in variable lookup (`infer.rs:522-531`)**

Replace the `Expr::Var` handler:

```rust
Expr::Var(name) => match env.lookup(name) {
    Some(scheme) => env.instantiate(scheme, &mut self.var_gen),
    None => {
        if let Some(modules) = self.ambiguous_names.get(name) {
            let qualified: Vec<String> = modules.iter().map(|m| format!("{m}.{name}")).collect();
            self.errors.push(TypeError {
                message: format!(
                    "ambiguous name `{name}` — defined in modules: {}. Use qualified syntax: {}",
                    modules.join(", "),
                    qualified.join(" or "),
                ),
                span: expr.span,
            });
        } else {
            self.errors.push(TypeError {
                message: format!("undefined variable '{name}'"),
                span: expr.span,
            });
        }
        self.var_gen.fresh()
    }
},
```

- [ ] **Step 4: Run tests**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests pass

- [ ] **Step 5: Run clippy + fmt**

Run: `cargo fmt && cargo clippy -- -D warnings 2>&1 | tail -10`
Expected: clean

- [ ] **Step 6: Commit**

```bash
git add src/infer.rs
git commit -m "fix: emit ambiguity diagnostic when colliding import names are used unqualified"
```

---

## Task 3: Remove `#[allow(dead_code)]` from `closures.rs` (ARCH_TODO #8)

**Files:**
- Modify: `src/closures.rs:3` — remove `#![allow(dead_code)]`

This is an exploratory task. The `#![allow(dead_code)]` is an inner attribute (applies to entire module). After removing it, the compiler will report which items are unused. The fix depends on what the compiler reports — either delete truly unused code or prefix unused fields with `_` if they are intentionally reserved for future milestones.

- [ ] **Step 1: Remove the allow and check what's actually dead**

Remove line 3: `#![allow(dead_code)]`

Run: `cargo check 2>&1 | grep "dead_code"`

- [ ] **Step 2: Fix each warning based on compiler output**

For each dead code warning:
- If the item is unused and has no future purpose: delete it
- If the item is used only from within the module: keep it (the warning is about pub visibility)
- If the item is a field used by external code: it should already be pub and not warned about

- [ ] **Step 3: Run tests**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests pass

- [ ] **Step 4: Commit**

```bash
git add src/closures.rs
git commit -m "refactor: remove #[allow(dead_code)] from closures.rs, clean up unused code"
```

---

## Task 4: Update ARCH_TODO.md — correct #14 assessment

**Files:**
- Modify: `ARCH_TODO.md`

- [ ] **Step 1: Update item #14**

The original assessment of "131 unwrap/expect/panic! calls" was inflated by counting `Parser::expect()` method calls (75 in parser.rs) which is the parser's own `fn expect(&mut self, expected: &Token) -> ParseResult<Span>` method, not `Option::expect()`.

Actual count: 14 `.unwrap()` calls (all in `#[cfg(test)]` blocks across `parser.rs`, `modules.rs`, `optimize.rs`), 4 `panic!` calls (all in test code). Production code is clean.

Mark #14 as resolved (false positive). Also update the file header line count stats.

- [ ] **Step 2: Commit**

```bash
git add ARCH_TODO.md
git commit -m "docs: correct ARCH_TODO #14 — unwrap/panic count was test-only, production code is clean"
```

---

## Task 5: Fix `#[allow(clippy::type_complexity)]` in parser.rs (ARCH_TODO #20)

**Files:**
- Modify: `src/ast.rs` — add `DestructuredParam` struct
- Modify: `src/parser.rs:199-263` — use `DestructuredParam` instead of tuple

The complex type is `(Spanned<Pattern>, TypeExpr, String, Span)` — used as the return type of destructuring parameter parsing. Two functions suppress clippy:
- `parse_params_with_patterns` (line 200): returns `(Vec<Param>, Vec<(Spanned<Pattern>, TypeExpr, String, Span)>)`
- `parse_param_or_pattern` (line 232): returns `(Param, Option<(Spanned<Pattern>, TypeExpr, String, Span)>)`

- [ ] **Step 1: Define `DestructuredParam` in `src/ast.rs`**

Add at the end of the structs section:
```rust
pub struct DestructuredParam {
    pub pattern: Spanned<Pattern>,
    pub type_annotation: TypeExpr,
    pub param_name: String,
    pub span: Span,
}
```

- [ ] **Step 2: Update `parse_params_with_patterns` return type**

Change line 202 from:
```rust
) -> ParseResult<(Vec<Param>, Vec<(Spanned<Pattern>, TypeExpr, String, Span)>)> {
```
to:
```rust
) -> ParseResult<(Vec<Param>, Vec<DestructuredParam>)> {
```

Remove `#[allow(clippy::type_complexity)]` on line 199.

- [ ] **Step 3: Update `parse_param_or_pattern` return type**

Change line 235 from:
```rust
) -> ParseResult<(Param, Option<(Spanned<Pattern>, TypeExpr, String, Span)>)> {
```
to:
```rust
) -> ParseResult<(Param, Option<DestructuredParam>)> {
```

Remove `#[allow(clippy::type_complexity)]` on line 231.

Update the tuple construction at line 258 from:
```rust
Ok((param, Some((pattern, type_expr, param_name, span))))
```
to:
```rust
Ok((param, Some(DestructuredParam { pattern, type_annotation: type_expr, param_name, span })))
```

- [ ] **Step 4: Update all destructuring sites**

Find all places where the tuple `(pattern, type_expr, param_name, span)` is destructured and update to use struct fields. Search for `pattern_bindings` usage in `parse_fn_def`.

- [ ] **Step 5: Run tests + clippy**

Run: `cargo test && cargo clippy -- -D warnings 2>&1 | tail -10`
Expected: all pass, no warnings

- [ ] **Step 6: Commit**

```bash
git add src/ast.rs src/parser.rs
git commit -m "refactor: replace complex tuple type with DestructuredParam struct in parser"
```
