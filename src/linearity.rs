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

fn is_handle_type(name: &str, handle_types: &HashSet<String>) -> bool {
    handle_types.contains(name)
}

fn type_expr_has_handle(ty: &TypeExpr, handle_types: &HashSet<String>) -> bool {
    match ty {
        TypeExpr::Named { name, args } => {
            is_handle_type(name, handle_types)
                || args.iter().any(|a| type_expr_has_handle(a, handle_types))
        }
        TypeExpr::Tuple(elems) => elems.iter().any(|e| type_expr_has_handle(e, handle_types)),
        TypeExpr::Fn { params, ret } => {
            params.iter().any(|p| type_expr_has_handle(p, handle_types))
                || type_expr_has_handle(ret, handle_types)
        }
    }
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
    handle_types: HashSet<String>,
    types_with_handles: HashSet<String>,
    constructor_to_type: HashMap<String, String>,
}

impl LinearityChecker {
    pub fn new(handle_types: HashSet<String>) -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
            handle_types,
            types_with_handles: HashSet::new(),
            constructor_to_type: HashMap::new(),
        }
    }

    pub fn check_module(mut self, module: &Module) -> LinearityResult {
        self.collect_types_with_handles(module);
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

    fn collect_types_with_handles(&mut self, module: &Module) {
        for def in &module.definitions {
            if let Definition::Type(t) = def {
                for ctor in &t.constructors {
                    self.constructor_to_type
                        .insert(ctor.name.clone(), t.name.clone());
                    for field in &ctor.fields {
                        if type_expr_has_handle(&field.type_expr, &self.handle_types) {
                            self.types_with_handles.insert(t.name.clone());
                        }
                    }
                }
            }
        }
    }

    fn check_function(&mut self, f: &FnDef) {
        // Track handle parameters
        let mut handles: HashMap<String, VarState> = HashMap::new();
        for p in &f.params {
            if self.is_handle_type_expr(&p.type_expr) {
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

                self.check_as_pattern_linearity(&pattern.node, type_annotation.as_ref(), expr.span);

                if let Pattern::Var(name) = &pattern.node {
                    let is_handle = type_annotation
                        .as_ref()
                        .is_some_and(|ty| self.is_handle_type_expr(ty));

                    if is_handle {
                        handles.insert(name.clone(), VarState::Alive);
                    }

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
                if let Expr::Var(name) = &inner.node
                    && let Some(state) = handles.get_mut(name.as_str())
                {
                    match state {
                        VarState::Alive => *state = VarState::Borrowed,
                        VarState::Moved => {
                            self.errors.push(LinearityError {
                                message: format!("clone of moved handle '{}'", name),
                                span: expr.span,
                            });
                        }
                        VarState::Borrowed => {}
                    }
                }
                self.check_expr(inner, handles);
            }

            Expr::Case { subject, arms } => {
                self.check_expr(subject, handles);
                self.try_move_handle(subject, handles);
                let snapshot = handles.clone();
                let mut merged = snapshot.clone();
                for arm in arms {
                    self.check_as_pattern_linearity(&arm.pattern.node, None, arm.span);
                    *handles = snapshot.clone();
                    if let Some(guard) = &arm.guard {
                        self.check_expr(guard, handles);
                    }
                    self.check_expr(&arm.body, handles);
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

            Expr::Pipe { left, right } => {
                self.check_expr(left, handles);
                self.try_move_handle(left, handles);
                self.check_expr(right, handles);
            }

            Expr::BinOp { left, right, .. } => {
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
                    self.try_move_handle(e, handles);
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
                self.try_move_handle(base, handles);
                for (_, e) in updates {
                    self.check_expr(e, handles);
                    self.try_move_handle(e, handles);
                }
            }

            Expr::ListCons { head, tail } => {
                self.check_expr(head, handles);
                self.try_move_handle(head, handles);
                self.check_expr(tail, handles);
                self.try_move_handle(tail, handles);
            }

            _ => {} // Literals, etc.
        }
    }

    fn check_as_pattern_linearity(
        &mut self,
        pattern: &Pattern,
        type_ann: Option<&TypeExpr>,
        span: Span,
    ) {
        match pattern {
            Pattern::As { pattern: inner, .. } => {
                let has_handle = match &inner.node {
                    Pattern::Constructor { name, .. } | Pattern::ConstructorNamed { name, .. } => {
                        if let Some(ty) = type_ann {
                            self.type_annotation_has_handle(ty)
                        } else {
                            self.constructor_to_type
                                .get(name.as_str())
                                .is_some_and(|type_name| {
                                    self.types_with_handles.contains(type_name.as_str())
                                })
                        }
                    }
                    Pattern::Tuple(_) => {
                        type_ann.is_some_and(|ty| self.type_annotation_has_handle(ty))
                    }
                    _ => false,
                };
                if has_handle {
                    self.errors.push(LinearityError {
                        message: "'as' binding on destructured type with handle fields creates implicit clone — use explicit let bindings instead".to_string(),
                        span,
                    });
                }
                self.check_as_pattern_linearity(&inner.node, type_ann, span);
            }
            Pattern::Constructor { args, .. } => {
                for arg in args {
                    self.check_as_pattern_linearity(&arg.node, None, span);
                }
            }
            Pattern::ConstructorNamed { fields, .. } => {
                for field in fields {
                    if let Some(p) = &field.pattern {
                        self.check_as_pattern_linearity(&p.node, None, span);
                    }
                }
            }
            Pattern::Tuple(elems) | Pattern::List(elems) => {
                for elem in elems {
                    self.check_as_pattern_linearity(&elem.node, None, span);
                }
            }
            Pattern::ListCons { head, tail } => {
                self.check_as_pattern_linearity(&head.node, None, span);
                self.check_as_pattern_linearity(&tail.node, None, span);
            }
            Pattern::Or(alts) => {
                for alt in alts {
                    self.check_as_pattern_linearity(&alt.node, None, span);
                }
            }
            _ => {}
        }
    }

    fn type_annotation_has_handle(&self, ty: &TypeExpr) -> bool {
        match ty {
            TypeExpr::Named { name, args } => {
                is_handle_type(name, &self.handle_types)
                    || self.types_with_handles.contains(name.as_str())
                    || args.iter().any(|a| self.type_annotation_has_handle(a))
            }
            TypeExpr::Tuple(elems) => elems.iter().any(|e| self.type_annotation_has_handle(e)),
            TypeExpr::Fn { params, ret } => {
                params.iter().any(|p| self.type_annotation_has_handle(p))
                    || self.type_annotation_has_handle(ret)
            }
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

    fn is_handle_type_expr(&self, ty: &TypeExpr) -> bool {
        match ty {
            TypeExpr::Named { name, .. } => is_handle_type(name, &self.handle_types),
            _ => false,
        }
    }

    fn collect_pattern_bindings(pat: &Pattern, bound: &mut HashSet<String>) {
        match pat {
            Pattern::Var(n) => {
                bound.insert(n.clone());
            }
            Pattern::Constructor { args, .. } => {
                for a in args {
                    Self::collect_pattern_bindings(&a.node, bound);
                }
            }
            Pattern::ConstructorNamed { fields, .. } => {
                for f in fields {
                    match &f.pattern {
                        Some(p) => Self::collect_pattern_bindings(&p.node, bound),
                        None => {
                            bound.insert(f.field_name.clone());
                        }
                    }
                }
            }
            Pattern::Tuple(elems) | Pattern::List(elems) => {
                for e in elems {
                    Self::collect_pattern_bindings(&e.node, bound);
                }
            }
            Pattern::ListCons { head, tail } => {
                Self::collect_pattern_bindings(&head.node, bound);
                Self::collect_pattern_bindings(&tail.node, bound);
            }
            Pattern::As {
                pattern: inner,
                name,
            } => {
                bound.insert(name.clone());
                Self::collect_pattern_bindings(&inner.node, bound);
            }
            Pattern::Or(alts) => {
                for a in alts {
                    Self::collect_pattern_bindings(&a.node, bound);
                }
            }
            Pattern::Discard
            | Pattern::Int(_)
            | Pattern::String(_)
            | Pattern::Bool(_)
            | Pattern::Rawcode(_) => {}
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
                Self::collect_pattern_bindings(&pattern.node, &mut new_bound);
                Self::collect_free_vars(&body.node, &new_bound, free);
            }
            Expr::BinOp { left, right, .. }
            | Expr::Pipe { left, right }
            | Expr::ListCons {
                head: left,
                tail: right,
            } => {
                Self::collect_free_vars(&left.node, bound, free);
                Self::collect_free_vars(&right.node, bound, free);
            }
            Expr::Call { function, args }
            | Expr::MethodCall {
                object: function,
                args,
                ..
            } => {
                Self::collect_free_vars(&function.node, bound, free);
                for a in args {
                    Self::collect_free_vars(&a.node, bound, free);
                }
            }
            Expr::Block(exprs) | Expr::Tuple(exprs) | Expr::List(exprs) => {
                for e in exprs {
                    Self::collect_free_vars(&e.node, bound, free);
                }
            }
            Expr::Case { subject, arms } => {
                Self::collect_free_vars(&subject.node, bound, free);
                for arm in arms {
                    let mut arm_bound = bound.clone();
                    Self::collect_pattern_bindings(&arm.pattern.node, &mut arm_bound);
                    if let Some(guard) = &arm.guard {
                        Self::collect_free_vars(&guard.node, &arm_bound, free);
                    }
                    Self::collect_free_vars(&arm.body.node, &arm_bound, free);
                }
            }
            Expr::Lambda { params, body, .. } => {
                let mut new_bound = bound.clone();
                for p in params {
                    new_bound.insert(p.name.clone());
                }
                Self::collect_free_vars(&body.node, &new_bound, free);
            }
            Expr::Constructor { args, .. } => {
                for a in args {
                    match a {
                        ConstructorArg::Positional(e) | ConstructorArg::Named(_, e) => {
                            Self::collect_free_vars(&e.node, bound, free);
                        }
                    }
                }
            }
            Expr::RecordUpdate { base, updates, .. } => {
                Self::collect_free_vars(&base.node, bound, free);
                for (_, e) in updates {
                    Self::collect_free_vars(&e.node, bound, free);
                }
            }
            Expr::FieldAccess { object, .. } => {
                Self::collect_free_vars(&object.node, bound, free);
            }
            Expr::UnaryOp { operand, .. } | Expr::Clone(operand) => {
                Self::collect_free_vars(&operand.node, bound, free);
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
        Expr::Constructor { args, .. } => {
            for a in args {
                match a {
                    ConstructorArg::Positional(e) | ConstructorArg::Named(_, e) => {
                        check_local_fn_body(e, errors);
                    }
                }
            }
        }
        Expr::RecordUpdate { base, updates, .. } => {
            check_local_fn_body(base, errors);
            for (_, e) in updates {
                check_local_fn_body(e, errors);
            }
        }
        Expr::FieldAccess { object, .. } => {
            check_local_fn_body(object, errors);
        }
        Expr::ListCons { head, tail } => {
            check_local_fn_body(head, errors);
            check_local_fn_body(tail, errors);
        }
        Expr::Lambda { body, .. } => {
            check_local_fn_body(body, errors);
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
    use crate::jass_parser::JassSdk;
    use crate::parser::Parser;
    use crate::token::Lexer;
    use rstest::rstest;

    fn test_handle_types() -> HashSet<String> {
        let stub = include_str!("../tests/common_stub.j");
        JassSdk::parse(stub).handle_type_names()
    }

    fn check(source: &str) -> LinearityResult {
        let tokens = Lexer::tokenize(source).expect("lex failed");
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module().expect("parse failed");
        LinearityChecker::new(test_handle_types()).check_module(&module)
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

    fn local_fn_errs(source: &str) -> Vec<String> {
        let tokens = Lexer::tokenize(source).expect("lex failed");
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module().expect("parse failed");
        check_local_fns(&module)
            .iter()
            .map(|e| e.message.clone())
            .collect()
    }

    #[rstest]
    #[case::use_once("fn test(t: Timer) { destroy_timer(t) }")]
    #[case::assign_then_use_new("fn test(t: Timer) { let t2: Timer = t\n destroy_timer(t2) }")]
    #[case::clone_allowed("fn test(t: Timer) { clone(t) }")]
    #[case::lambda_capture_no_reuse("fn test(g: Group) { let f = fn() { destroy_group(g) }\n f }")]
    #[case::clone_twice("fn test(m: Model) { let a = clone(m)\n let b = clone(m)\n a }")]
    #[case::non_handle("fn test(x: Int) { let a = x\n let b = x\n a + b }")]
    #[case::as_pattern_no_handle(
        "pub struct Model { time: Int, kills: Int }\nfn test(Model { time, .. } as m: Model) -> Int { time }"
    )]
    #[case::as_in_case_no_handle(
        "pub struct Model { time: Int, kills: Int }\npub enum Action { Update(Model) }\nfn test(a: Action) -> Int { case a { Update(Model { time, .. } as m) -> time } }"
    )]
    #[case::record_update(
        "pub struct State { timer: Timer, count: Int }\nfn test(s: State) -> Int { let s2 = State(..s, count: 5)\n s2.count }"
    )]
    fn no_errors(#[case] source: &str) {
        let errs = errors(source);
        assert!(errs.is_empty(), "expected no errors, got: {:?}", errs);
    }

    #[rstest]
    #[case::double_destroy("fn test(t: Timer) { destroy_timer(t)\n destroy_timer(t) }")]
    #[case::moved_by_assignment("fn test(t: Timer) { let t2: Timer = t\n destroy_timer(t) }")]
    #[case::lambda_capture_reuse(
        "fn test(g: Group) { let f = fn() { destroy_group(g) }\n destroy_group(g) }"
    )]
    #[case::pipe("fn test(t: Timer) { t |> destroy_timer\n t |> destroy_timer }")]
    #[case::tuple("fn test(t: Timer) { let pair = (t, 5)\n destroy_timer(t) }")]
    #[case::list("fn test(t: Timer) { let xs = [t]\n destroy_timer(t) }")]
    #[case::list_cons(
        "fn test(t: Timer, xs: List(Timer)) { let ys = [t | xs]\n destroy_timer(t) }"
    )]
    #[case::constructor(
        "pub enum Wrap { Hold(Timer) }\nfn test(t: Timer) { let w = Wrap::Hold(t)\n destroy_timer(t) }"
    )]
    #[case::case_subject("fn test(t: Timer) { case t { _ -> 0 }\n destroy_timer(t) }")]
    fn handle_move_error(#[case] source: &str) {
        let errs = errors(source);
        assert_eq!(errs.len(), 1, "expected 1 error, got: {:?}", errs);
        assert!(
            errs[0].contains("moved handle") || errs[0].contains("clone of moved"),
            "unexpected error: {}",
            errs[0]
        );
    }

    #[rstest]
    #[case::fn_param(
        "pub struct State { timer: Timer, count: Int }\nfn test(State { count, .. } as s: State) -> Int { count }"
    )]
    #[case::case_arm(
        "pub struct State { timer: Timer, count: Int }\npub enum Action { Update(State) }\nfn test(a: Action) -> Int { case a { Update(State { count, .. } as s) -> count } }"
    )]
    #[case::enum_variant(
        "pub struct Spell { caster: Unit, target: Unit }\nfn test(s: Spell) -> Int { case s { Spell { caster, .. } as sp -> 0 } }"
    )]
    fn as_pattern_with_handle_error(#[case] source: &str) {
        let errs = errors(source);
        assert_eq!(errs.len(), 1, "expected 1 error, got: {:?}", errs);
        assert!(
            errs[0].contains("'as' binding"),
            "unexpected error: {}",
            errs[0]
        );
    }

    #[test]
    fn clone_moved_handle() {
        let errs = errors("fn test(t: Timer) { destroy_timer(t)\n clone(t) }");
        assert_eq!(errs.len(), 1);
        insta::assert_snapshot!(errs[0]);
    }

    #[test]
    fn unconsumed_handle_warning() {
        let warns = warnings("fn test(t: Timer) { 0 }");
        assert_eq!(warns.len(), 1);
        insta::assert_snapshot!(warns[0]);
    }

    #[rstest]
    #[case::create("local fn bad() { create_timer() }")]
    #[case::destroy("local fn bad(t: Timer) { destroy_timer(t) }")]
    #[case::effect_method("local fn bad() { effect.create_unit(0, 0, 0.0, 0.0, 0.0) }")]
    fn local_fn_desync_error(#[case] source: &str) {
        let errs = local_fn_errs(source);
        assert_eq!(errs.len(), 1, "expected 1 error, got: {:?}", errs);
        assert!(
            errs[0].contains("desync-unsafe"),
            "unexpected error: {}",
            errs[0]
        );
    }

    #[rstest]
    #[case::safe_local("local fn update_camera(x: Float, y: Float) { set_camera_position(x, y) }")]
    #[case::non_local("fn normal() { create_timer()\n destroy_timer(create_timer()) }")]
    fn local_fn_no_error(#[case] source: &str) {
        let errs = local_fn_errs(source);
        assert!(errs.is_empty(), "expected no errors, got: {:?}", errs);
    }

    #[test]
    fn handle_in_returned_struct_no_warning() {
        let warns = warnings(
            r#"
pub struct State { hero: Unit, time: Int }
fn test(s: State) -> State { State(..s, time: 5) }
"#,
        );
        assert!(
            warns.is_empty(),
            "no warning for handle returned in struct: {:?}",
            warns
        );
    }

    #[test]
    fn destructured_handle_returned_in_struct_no_warning() {
        let warns = warnings(
            r#"
pub struct State { hero: Unit, time: Int }
fn test(State { hero, time } as s: State) -> State { State { hero, time: time + 1 } }
"#,
        );
        assert!(
            warns.is_empty(),
            "no warning when destructured handle is returned: {:?}",
            warns
        );
    }
}
