use crate::ast::*;
use crate::token::Span;
use std::collections::HashSet;

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

pub fn find_var_span(target: &str, expr: &Spanned<Expr>) -> Option<Span> {
    match &expr.node {
        Expr::Var(name) if name == target => Some(expr.span),
        Expr::Let {
            value,
            body,
            pattern,
            ..
        } => find_var_span(target, value).or_else(|| {
            let mut scope = HashSet::new();
            bind_pattern(&pattern.node, &mut scope);
            if scope.contains(target) {
                None
            } else {
                find_var_span(target, body)
            }
        }),
        Expr::Case { subject, arms } => find_var_span(target, subject).or_else(|| {
            arms.iter().find_map(|arm| {
                let mut scope = HashSet::new();
                bind_pattern(&arm.pattern.node, &mut scope);
                if scope.contains(target) {
                    None
                } else {
                    arm.guard
                        .as_ref()
                        .and_then(|g| find_var_span(target, g))
                        .or_else(|| find_var_span(target, &arm.body))
                }
            })
        }),
        Expr::BinOp { left, right, .. } | Expr::Pipe { left, right } => {
            find_var_span(target, left).or_else(|| find_var_span(target, right))
        }
        Expr::UnaryOp { operand, .. } | Expr::Clone(operand) => find_var_span(target, operand),
        Expr::Call { function, args } => find_var_span(target, function)
            .or_else(|| args.iter().find_map(|a| find_var_span(target, a))),
        Expr::FieldAccess { object, .. } => find_var_span(target, object),
        Expr::MethodCall { object, args, .. } => find_var_span(target, object)
            .or_else(|| args.iter().find_map(|a| find_var_span(target, a))),
        Expr::Block(exprs) => {
            let mut shadowed = false;
            for e in exprs {
                if shadowed {
                    continue;
                }
                if let Some(span) = find_var_span(target, e) {
                    return Some(span);
                }
                if let Expr::Let { pattern, .. } = &e.node {
                    let mut scope = HashSet::new();
                    bind_pattern(&pattern.node, &mut scope);
                    if scope.contains(target) {
                        shadowed = true;
                    }
                }
            }
            None
        }
        Expr::Tuple(elems) | Expr::List(elems) => {
            elems.iter().find_map(|e| find_var_span(target, e))
        }
        Expr::ListCons { head, tail } => {
            find_var_span(target, head).or_else(|| find_var_span(target, tail))
        }
        Expr::Lambda { params, body, .. } => {
            if params.iter().any(|p| p.name == target) {
                None
            } else {
                find_var_span(target, body)
            }
        }
        Expr::Constructor { args, .. } => args.iter().find_map(|a| match a {
            ConstructorArg::Positional(e) | ConstructorArg::Named(_, e) => find_var_span(target, e),
        }),
        Expr::RecordUpdate { base, updates, .. } => find_var_span(target, base)
            .or_else(|| updates.iter().find_map(|(_, e)| find_var_span(target, e))),
        Expr::TcoLoop { body } => find_var_span(target, body),
        Expr::TcoContinue { args } => args.iter().find_map(|(_, e)| find_var_span(target, e)),
        _ => None,
    }
}
