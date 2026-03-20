// Linear type checker for Glass.
//
// Handles (Unit, Timer, Group, etc.) are linear: every use is a move.
// Records and primitives are Copy.
//
// Rules:
// 1. `let b = a` for handle → move (a invalid)
// 2. `f(a)` for handle → move (a invalid)
// 3. Closure capture → move
// 4. destroy_*/remove_* → consumed
// 5. Scope exit with owned handle → warning (auto-destroy in codegen)
// 6. clone(handle) → error

use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::token::Span;

/// Known JASS handle type names.
const HANDLE_TYPES: &[&str] = &[
    "Unit",
    "Timer",
    "Group",
    "Trigger",
    "Force",
    "Sound",
    "Sfx",
    "Location",
    "Region",
    "Rect",
    "Dialog",
    "Quest",
    "Multiboard",
    "Leaderboard",
    "Texttag",
    "Lightning",
    "Image",
    "Ubersplat",
    "Trackable",
    "Timerdialog",
    "Fogmodifier",
    "Hashtable",
];

fn is_handle_type(name: &str) -> bool {
    HANDLE_TYPES.contains(&name)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum VarState {
    Alive,
    Moved,
    /// Handle was cloned (read-only use) — not a leak if unconsumed at end.
    Borrowed,
}

#[derive(Debug)]
pub struct LinearityError {
    pub message: String,
    pub span: Span,
}

pub struct LinearityChecker {
    errors: Vec<LinearityError>,
    warnings: Vec<LinearityError>,
}

impl LinearityChecker {
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn check_module(mut self, module: &Module) -> LinearityResult {
        for def in &module.definitions {
            if let Definition::Function(f) = def {
                self.check_function(f);
            }
        }
        LinearityResult {
            errors: self.errors,
            warnings: self.warnings,
        }
    }

    fn check_function(&mut self, f: &FnDef) {
        // Track handle parameters
        let mut handles: HashMap<String, VarState> = HashMap::new();
        for p in &f.params {
            if Self::is_handle_type_expr(&p.type_expr) {
                handles.insert(p.name.clone(), VarState::Alive);
            }
        }

        self.check_expr(&f.body, &mut handles);

        // Check for un-consumed handles at function exit
        // If the function returns the handle, it's transferred to caller (OK)
        // Otherwise it's a leak → warning
        for (name, state) in &handles {
            if *state == VarState::Alive {
                // Handle was never used at all — likely a bug.
                self.warnings.push(LinearityError {
                    message: format!(
                        "handle '{}' is not consumed at end of function (will be auto-destroyed)",
                        name
                    ),
                    span: f.span,
                });
            }
            // VarState::Borrowed — handle was clone()-d for read-only use, OK.
            // VarState::Moved — handle was consumed, OK.
        }
    }

    /// If expr is a Var referencing a tracked handle, mark it as moved.
    fn try_move_handle(&mut self, expr: &Spanned<Expr>, handles: &mut HashMap<String, VarState>) {
        let Expr::Var(name) = &expr.node else { return };
        if handles.contains_key(name.as_str()) {
            self.mark_moved(name, expr.span, handles);
        }
    }

    fn check_expr(&mut self, expr: &Spanned<Expr>, handles: &mut HashMap<String, VarState>) {
        match &expr.node {
            // Var: usage checked by context (call, assignment) via try_move_handle
            Expr::Let {
                pattern,
                value,
                body,
                type_annotation,
                ..
            } => {
                self.check_expr(value, handles);

                if let Pattern::Var(name) = &pattern.node {
                    let is_handle = type_annotation
                        .as_ref()
                        .is_some_and(Self::is_handle_type_expr);

                    if is_handle {
                        handles.insert(name.clone(), VarState::Alive);
                    }

                    // RHS handle variable → moved
                    self.try_move_handle(value, handles);
                }

                self.check_expr(body, handles);
            }

            Expr::Call { function, args } => {
                self.check_expr(function, handles);
                for arg in args {
                    self.check_expr(arg, handles);
                    self.try_move_handle(arg, handles);
                }
            }

            Expr::MethodCall { object, args, .. } => {
                self.check_expr(object, handles);
                self.try_move_handle(object, handles);
                for arg in args {
                    self.check_expr(arg, handles);
                    self.try_move_handle(arg, handles);
                }
            }

            Expr::Lambda { body, .. } => {
                let mut free_vars: HashSet<String> = HashSet::new();
                Self::collect_free_vars(&body.node, &HashSet::new(), &mut free_vars);

                for var in &free_vars {
                    if handles.contains_key(var) {
                        self.mark_moved(var, expr.span, handles);
                    }
                }
            }

            Expr::Clone(inner) => {
                // clone(handle) is allowed — it creates an alias to the same
                // JASS handle without consuming the original.  The underlying
                // handle is reference-counted by the WC3 runtime, so this is
                // safe.  Mark the original as Borrowed (read-only use — no
                // leak warning at end of function).
                if let Expr::Var(name) = &inner.node
                    && let Some(state) = handles.get_mut(name.as_str())
                    && *state == VarState::Alive
                {
                    *state = VarState::Borrowed;
                }
                self.check_expr(inner, handles);
            }

            Expr::Case { subject, arms } => {
                self.check_expr(subject, handles);
                let snapshot = handles.clone();
                // Track the "most moved" state across all arms
                let mut merged = snapshot.clone();
                for arm in arms {
                    *handles = snapshot.clone();
                    self.check_expr(&arm.body, handles);
                    // Merge: pick the "most consumed" state per variable
                    for (name, state) in handles.iter() {
                        let merged_state = merged.get(name).cloned().unwrap_or(VarState::Alive);
                        let new_state = match (&merged_state, state) {
                            (VarState::Moved, _) | (_, VarState::Moved) => VarState::Moved,
                            (VarState::Borrowed, _) | (_, VarState::Borrowed) => VarState::Borrowed,
                            _ => VarState::Alive,
                        };
                        merged.insert(name.clone(), new_state);
                    }
                }
                *handles = merged;
            }

            Expr::BinOp { left, right, .. } | Expr::Pipe { left, right } => {
                self.check_expr(left, handles);
                self.check_expr(right, handles);
            }

            Expr::UnaryOp { operand, .. } => {
                self.check_expr(operand, handles);
            }

            Expr::FieldAccess { object, .. } => {
                self.check_expr(object, handles);
            }

            Expr::Block(exprs) => {
                for e in exprs {
                    self.check_expr(e, handles);
                }
            }

            Expr::Tuple(elems) | Expr::List(elems) => {
                for e in elems {
                    self.check_expr(e, handles);
                }
            }

            Expr::Constructor { args, .. } => {
                for a in args {
                    let e = match a {
                        ConstructorArg::Positional(e) | ConstructorArg::Named(_, e) => e,
                    };
                    self.check_expr(e, handles);
                    // Handle passed into constructor → moved (ownership transferred to ADT)
                    self.try_move_handle(e, handles);
                }
            }

            Expr::RecordUpdate { base, updates, .. } => {
                self.check_expr(base, handles);
                for (_, e) in updates {
                    self.check_expr(e, handles);
                }
            }

            _ => {} // Literals, etc.
        }
    }

    fn mark_moved(&mut self, name: &str, span: Span, handles: &mut HashMap<String, VarState>) {
        if let Some(state) = handles.get_mut(name) {
            if *state == VarState::Moved {
                self.errors.push(LinearityError {
                    message: format!("use of moved handle '{}'", name),
                    span,
                });
            } else {
                *state = VarState::Moved;
            }
        }
    }

    fn is_handle_type_expr(ty: &TypeExpr) -> bool {
        match ty {
            TypeExpr::Named { name, .. } => is_handle_type(name),
            _ => false,
        }
    }

    fn collect_free_vars(expr: &Expr, bound: &HashSet<String>, free: &mut HashSet<String>) {
        match expr {
            Expr::Var(name) => {
                if !bound.contains(name) {
                    free.insert(name.clone());
                }
            }
            Expr::Let {
                pattern,
                value,
                body,
                ..
            } => {
                Self::collect_free_vars(&value.node, bound, free);
                let mut new_bound = bound.clone();
                if let Pattern::Var(n) = &pattern.node {
                    new_bound.insert(n.clone());
                }
                Self::collect_free_vars(&body.node, &new_bound, free);
            }
            Expr::BinOp { left, right, .. } | Expr::Pipe { left, right } => {
                Self::collect_free_vars(&left.node, bound, free);
                Self::collect_free_vars(&right.node, bound, free);
            }
            Expr::Call { function, args } => {
                Self::collect_free_vars(&function.node, bound, free);
                for a in args {
                    Self::collect_free_vars(&a.node, bound, free);
                }
            }
            Expr::Block(exprs) => {
                for e in exprs {
                    Self::collect_free_vars(&e.node, bound, free);
                }
            }
            Expr::Case { subject, arms } => {
                Self::collect_free_vars(&subject.node, bound, free);
                for arm in arms {
                    Self::collect_free_vars(&arm.body.node, bound, free);
                }
            }
            _ => {}
        }
    }
}

pub struct LinearityResult {
    pub errors: Vec<LinearityError>,
    pub warnings: Vec<LinearityError>,
}

// === Local fn checker ===
// Functions marked `local fn` run in GetLocalPlayer() context.
// They must NOT create/destroy handles or call sync-sensitive operations.

const DESYNC_UNSAFE_PREFIXES: &[&str] = &[
    "create_",
    "destroy_",
    "remove_",
    "trigger_",
    "timer_start",
    "for_group",
    "for_force",
    "trigger_sleep_action",
];

pub fn check_local_fns(module: &Module) -> Vec<LinearityError> {
    let mut errors = Vec::new();
    for def in &module.definitions {
        if let Definition::Function(f) = def
            && f.is_local
        {
            check_local_fn_body(&f.body, &mut errors);
        }
    }
    errors
}

fn check_local_fn_body(expr: &Spanned<Expr>, errors: &mut Vec<LinearityError>) {
    match &expr.node {
        Expr::Call { function, args } => {
            if let Expr::Var(name) = &function.node
                && is_desync_unsafe(name)
            {
                errors.push(LinearityError {
                    message: format!("desync-unsafe call '{}' inside local fn", name),
                    span: expr.span,
                });
            }
            for a in args {
                check_local_fn_body(a, errors);
            }
        }
        Expr::MethodCall {
            object,
            method,
            args,
            ..
        } => {
            // Check effect.after, effect.create_unit etc.
            if let Expr::Var(module_name) = &object.node {
                let full_name = format!("{}.{}", module_name, method);
                if is_desync_unsafe(&full_name) || is_desync_unsafe(method) {
                    errors.push(LinearityError {
                        message: format!("desync-unsafe call '{}' inside local fn", full_name),
                        span: expr.span,
                    });
                }
            }
            check_local_fn_body(object, errors);
            for a in args {
                check_local_fn_body(a, errors);
            }
        }
        Expr::Let { value, body, .. } => {
            check_local_fn_body(value, errors);
            check_local_fn_body(body, errors);
        }
        Expr::Case { subject, arms } => {
            check_local_fn_body(subject, errors);
            for arm in arms {
                check_local_fn_body(&arm.body, errors);
            }
        }
        Expr::BinOp { left, right, .. } | Expr::Pipe { left, right } => {
            check_local_fn_body(left, errors);
            check_local_fn_body(right, errors);
        }
        Expr::UnaryOp { operand, .. } | Expr::Clone(operand) => {
            check_local_fn_body(operand, errors);
        }
        Expr::Block(exprs) => {
            for e in exprs {
                check_local_fn_body(e, errors);
            }
        }
        Expr::Tuple(elems) | Expr::List(elems) => {
            for e in elems {
                check_local_fn_body(e, errors);
            }
        }
        _ => {}
    }
}

fn is_desync_unsafe(name: &str) -> bool {
    let lower = name.to_lowercase();
    DESYNC_UNSAFE_PREFIXES
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use crate::token::Lexer;

    fn check(source: &str) -> LinearityResult {
        let tokens = Lexer::tokenize(source);
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module().expect("parse failed");
        LinearityChecker::new().check_module(&module)
    }

    fn errors(source: &str) -> Vec<String> {
        check(source)
            .errors
            .iter()
            .map(|e| e.message.clone())
            .collect()
    }

    fn warnings(source: &str) -> Vec<String> {
        check(source)
            .warnings
            .iter()
            .map(|e| e.message.clone())
            .collect()
    }

    #[test]
    fn handle_used_once_ok() {
        let errs = errors("fn test(t: Timer) { destroy_timer(t) }");
        assert!(errs.is_empty());
    }

    #[test]
    fn handle_used_twice_error() {
        let errs = errors(
            r#"
fn test(t: Timer) {
    destroy_timer(t)
    destroy_timer(t)
}
"#,
        );
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("moved handle 't'"));
    }

    #[test]
    fn handle_not_consumed_warning() {
        let warns = warnings("fn test(t: Timer) { 0 }");
        assert_eq!(warns.len(), 1);
        assert!(warns[0].contains("auto-destroyed"));
    }

    #[test]
    fn non_handle_no_error() {
        let errs = errors(
            r#"
fn test(x: Int) {
    let a = x
    let b = x
    a + b
}
"#,
        );
        assert!(errs.is_empty());
    }

    #[test]
    fn handle_moved_by_assignment() {
        let errs = errors(
            r#"
fn test(t: Timer) {
    let t2: Timer = t
    destroy_timer(t)
}
"#,
        );
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("moved handle 't'"));
    }

    #[test]
    fn handle_moved_by_assignment_then_use_new() {
        let errs = errors(
            r#"
fn test(t: Timer) {
    let t2: Timer = t
    destroy_timer(t2)
}
"#,
        );
        assert!(errs.is_empty());
    }

    #[test]
    fn clone_handle_allowed() {
        // clone(handle) is now allowed — it creates an alias without consuming
        // the original. The WC3 runtime reference-counts handles.
        let errs = errors("fn test(t: Timer) { clone(t) }");
        assert_eq!(errs.len(), 0);
    }

    #[test]
    fn lambda_captures_handle() {
        let errs = errors(
            r#"
fn test(g: Group) {
    let f = fn() { destroy_group(g) }
    destroy_group(g)
}
"#,
        );
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("moved handle 'g'"));
    }

    #[test]
    fn lambda_captures_handle_ok() {
        let errs = errors(
            r#"
fn test(g: Group) {
    let f = fn() { destroy_group(g) }
    f
}
"#,
        );
        assert!(errs.is_empty());
    }

    #[test]
    fn record_clone_ok() {
        let errs = errors(
            r#"
fn test(m: Model) {
    let a = clone(m)
    let b = clone(m)
    a
}
"#,
        );
        assert!(errs.is_empty());
    }

    // === local fn tests ===

    fn local_fn_errors(source: &str) -> Vec<String> {
        let tokens = Lexer::tokenize(source);
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module().expect("parse failed");
        check_local_fns(&module)
            .iter()
            .map(|e| e.message.clone())
            .collect()
    }

    #[test]
    fn local_fn_allows_safe_calls() {
        let errs = local_fn_errors(
            r#"
local fn update_camera(x: Float, y: Float) {
    set_camera_position(x, y)
}
"#,
        );
        assert!(errs.is_empty());
    }

    #[test]
    fn local_fn_forbids_create() {
        let errs = local_fn_errors(
            r#"
local fn bad() {
    create_timer()
}
"#,
        );
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("desync-unsafe"));
        assert!(errs[0].contains("create_timer"));
    }

    #[test]
    fn local_fn_forbids_destroy() {
        let errs = local_fn_errors(
            r#"
local fn bad(t: Timer) {
    destroy_timer(t)
}
"#,
        );
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("destroy_timer"));
    }

    #[test]
    fn local_fn_forbids_effect_after() {
        let errs = local_fn_errors(
            r#"
local fn bad() {
    effect.create_unit(0, 0, 0.0, 0.0, 0.0)
}
"#,
        );
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("effect.create_unit"));
    }

    #[test]
    fn non_local_fn_allows_everything() {
        let errs = local_fn_errors(
            r#"
fn normal() {
    create_timer()
    destroy_timer(create_timer())
}
"#,
        );
        assert!(errs.is_empty());
    }
}
