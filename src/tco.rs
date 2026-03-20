use crate::ast::*;

/// Apply TCO to all eligible functions in the module.
/// A function is eligible if it has at least one self-recursive call
/// and ALL self-recursive calls are in tail position.
pub fn apply_tco(module: &mut Module) {
    for def in &mut module.definitions {
        if let Definition::Function(f) = def {
            // Skip functions with _ params (can't reassign _ in JASS)
            if f.params.iter().any(|p| p.name == "_") {
                continue;
            }
            if is_tail_recursive(&f.name, &f.body.node) {
                let new_body = wrap_in_tco_loop(&f.name, &f.params, f.body.clone());
                f.body = new_body;
            }
        }
    }
}

/// Check if the function body has at least one self-call and ALL self-calls are in tail position.
fn is_tail_recursive(fn_name: &str, body: &Expr) -> bool {
    let mut has_rec = false;
    let mut all_tail = true;
    check_calls(fn_name, body, true, &mut has_rec, &mut all_tail);
    has_rec && all_tail
}

fn check_calls(fn_name: &str, expr: &Expr, in_tail: bool, has_rec: &mut bool, all_tail: &mut bool) {
    match expr {
        Expr::Call { function, args } => {
            if let Expr::Var(name) = &function.node
                && name == fn_name
            {
                *has_rec = true;
                if !in_tail {
                    *all_tail = false;
                }
                // Args are never in tail position
                for arg in args {
                    check_calls(fn_name, &arg.node, false, has_rec, all_tail);
                }
                return;
            }
            check_calls(fn_name, &function.node, false, has_rec, all_tail);
            for arg in args {
                check_calls(fn_name, &arg.node, false, has_rec, all_tail);
            }
        }
        Expr::Let { value, body, .. } => {
            check_calls(fn_name, &value.node, false, has_rec, all_tail);
            check_calls(fn_name, &body.node, in_tail, has_rec, all_tail);
        }
        Expr::Case { subject, arms } => {
            check_calls(fn_name, &subject.node, false, has_rec, all_tail);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    check_calls(fn_name, &guard.node, false, has_rec, all_tail);
                }
                check_calls(fn_name, &arm.body.node, in_tail, has_rec, all_tail);
            }
        }
        Expr::Block(exprs) => {
            for (i, e) in exprs.iter().enumerate() {
                let is_last = i == exprs.len() - 1;
                check_calls(fn_name, &e.node, in_tail && is_last, has_rec, all_tail);
            }
        }
        Expr::BinOp { left, right, .. } | Expr::Pipe { left, right } => {
            check_calls(fn_name, &left.node, false, has_rec, all_tail);
            check_calls(fn_name, &right.node, false, has_rec, all_tail);
        }
        Expr::UnaryOp { operand, .. } => {
            check_calls(fn_name, &operand.node, false, has_rec, all_tail);
        }
        Expr::Lambda { body, .. } => {
            // Different function context — don't propagate tail position
            check_calls(fn_name, &body.node, false, has_rec, all_tail);
        }
        Expr::FieldAccess { object, .. } => {
            check_calls(fn_name, &object.node, false, has_rec, all_tail);
        }
        Expr::MethodCall { object, args, .. } => {
            check_calls(fn_name, &object.node, false, has_rec, all_tail);
            for arg in args {
                check_calls(fn_name, &arg.node, false, has_rec, all_tail);
            }
        }
        Expr::Constructor { args, .. } => {
            for arg in args {
                match arg {
                    ConstructorArg::Positional(e) | ConstructorArg::Named(_, e) => {
                        check_calls(fn_name, &e.node, false, has_rec, all_tail);
                    }
                }
            }
        }
        Expr::RecordUpdate { base, updates, .. } => {
            check_calls(fn_name, &base.node, false, has_rec, all_tail);
            for (_, e) in updates {
                check_calls(fn_name, &e.node, false, has_rec, all_tail);
            }
        }
        Expr::Tuple(elems) | Expr::List(elems) => {
            for e in elems {
                check_calls(fn_name, &e.node, false, has_rec, all_tail);
            }
        }
        Expr::ListCons { head, tail } => {
            check_calls(fn_name, &head.node, false, has_rec, all_tail);
            check_calls(fn_name, &tail.node, false, has_rec, all_tail);
        }
        Expr::Clone(e) => {
            check_calls(fn_name, &e.node, false, has_rec, all_tail);
        }
        // Leaves — no recursive structure (TcoLoop/TcoContinue should not appear before TCO)
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::String(_)
        | Expr::Rawcode(_)
        | Expr::Bool(_)
        | Expr::Var(_)
        | Expr::Todo(_)
        | Expr::TcoLoop { .. }
        | Expr::TcoContinue { .. } => {}
    }
}

/// Wrap function body in TcoLoop, replacing tail self-calls with TcoContinue.
fn wrap_in_tco_loop(fn_name: &str, params: &[Param], body: Spanned<Expr>) -> Spanned<Expr> {
    let span = body.span;
    let transformed = replace_tail_calls(fn_name, params, body);
    Spanned {
        node: Expr::TcoLoop {
            body: Box::new(transformed),
        },
        span,
    }
}

/// Walk the expression and replace tail-position self-calls with TcoContinue.
fn replace_tail_calls(fn_name: &str, params: &[Param], expr: Spanned<Expr>) -> Spanned<Expr> {
    let span = expr.span;
    match expr.node {
        Expr::Call { function, args } => {
            if let Expr::Var(ref name) = function.node
                && name == fn_name
            {
                let assignments: Vec<_> = params
                    .iter()
                    .zip(args)
                    .map(|(p, a)| (p.name.clone(), Box::new(a)))
                    .collect();
                return Spanned {
                    node: Expr::TcoContinue { args: assignments },
                    span,
                };
            }
            Spanned {
                node: Expr::Call { function, args },
                span,
            }
        }
        Expr::Let {
            pattern,
            type_annotation,
            value,
            body,
        } => {
            let new_body = replace_tail_calls(fn_name, params, *body);
            Spanned {
                node: Expr::Let {
                    pattern,
                    type_annotation,
                    value,
                    body: Box::new(new_body),
                },
                span,
            }
        }
        Expr::Case { subject, arms } => {
            let new_arms: Vec<_> = arms
                .into_iter()
                .map(|arm| {
                    let new_body = replace_tail_calls(fn_name, params, arm.body);
                    CaseArm {
                        pattern: arm.pattern,
                        guard: arm.guard,
                        body: new_body,
                        span: arm.span,
                    }
                })
                .collect();
            Spanned {
                node: Expr::Case {
                    subject,
                    arms: new_arms,
                },
                span,
            }
        }
        Expr::Block(exprs) => {
            let mut new_exprs: Vec<_> = exprs.into_iter().collect();
            if let Some(last) = new_exprs.pop() {
                let new_last = replace_tail_calls(fn_name, params, last);
                new_exprs.push(new_last);
            }
            Spanned {
                node: Expr::Block(new_exprs),
                span,
            }
        }
        other => Spanned { node: other, span },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::Span;

    fn s() -> Span {
        Span::new(0, 0)
    }

    fn sp<T>(node: T) -> Spanned<T> {
        Spanned { node, span: s() }
    }

    fn var(name: &str) -> Spanned<Expr> {
        sp(Expr::Var(name.to_string()))
    }

    fn call(fn_name: &str, args: Vec<Spanned<Expr>>) -> Expr {
        Expr::Call {
            function: Box::new(var(fn_name)),
            args,
        }
    }

    fn int(n: i64) -> Spanned<Expr> {
        sp(Expr::Int(n))
    }

    fn arm(pattern: Pattern, body: Spanned<Expr>) -> CaseArm {
        CaseArm {
            pattern: sp(pattern),
            guard: None,
            body,
            span: s(),
        }
    }

    #[test]
    fn detect_simple_tail_recursion() {
        let body = Expr::Case {
            subject: Box::new(int(0)),
            arms: vec![
                arm(Pattern::Bool(true), int(0)),
                arm(Pattern::Bool(false), sp(call("foo", vec![int(1)]))),
            ],
        };
        assert!(is_tail_recursive("foo", &body));
    }

    #[test]
    fn detect_non_tail_recursion() {
        // 1 + foo(n-1) — NOT tail recursive
        let body = Expr::BinOp {
            op: BinOp::Add,
            left: Box::new(int(1)),
            right: Box::new(sp(call("foo", vec![int(1)]))),
        };
        assert!(!is_tail_recursive("foo", &body));
    }

    #[test]
    fn detect_no_recursion() {
        let body = Expr::BinOp {
            op: BinOp::Add,
            left: Box::new(var("n")),
            right: Box::new(int(1)),
        };
        assert!(!is_tail_recursive("foo", &body));
    }

    #[test]
    fn detect_let_tail_recursion() {
        let body = Expr::Let {
            pattern: sp(Pattern::Var("x".to_string())),
            type_annotation: None,
            value: Box::new(int(1)),
            body: Box::new(sp(call("foo", vec![var("x")]))),
        };
        assert!(is_tail_recursive("foo", &body));
    }

    #[test]
    fn mixed_tail_and_non_tail_rejected() {
        let body = Expr::Case {
            subject: Box::new(var("n")),
            arms: vec![
                arm(Pattern::Int(0), sp(call("foo", vec![int(1)]))),
                arm(
                    Pattern::Discard,
                    sp(Expr::BinOp {
                        op: BinOp::Add,
                        left: Box::new(int(1)),
                        right: Box::new(sp(call("foo", vec![var("n")]))),
                    }),
                ),
            ],
        };
        assert!(!is_tail_recursive("foo", &body));
    }
}
