use std::collections::HashSet;

use crate::ast::*;

pub fn apply_lambda_lifting(module: &mut Module) {
    let mut lifted = Vec::new();
    let mut counter = 0;
    let fn_names = collect_top_level_names(&module.definitions);

    let mut defs = std::mem::take(&mut module.definitions);
    for def in &mut defs {
        if let Definition::Function(f) = def {
            let mut scope: HashSet<String> = f.params.iter().map(|p| p.name.clone()).collect();
            scope.extend(fn_names.iter().cloned());
            f.body = lift_expr(f.body.clone(), &scope, &fn_names, &mut lifted, &mut counter);
        }
    }
    for lf in lifted {
        defs.push(Definition::Function(lf));
    }
    module.definitions = defs;
}

fn collect_top_level_names(defs: &[Definition]) -> HashSet<String> {
    let mut names = HashSet::new();
    for def in defs {
        match def {
            Definition::Function(f) => {
                names.insert(f.name.clone());
            }
            Definition::External(e) => {
                names.insert(e.fn_name.clone());
            }
            Definition::Const(c) => {
                names.insert(c.name.clone());
            }
            _ => {}
        }
    }
    names
}

fn lift_expr(
    expr: Spanned<Expr>,
    scope: &HashSet<String>,
    top_names: &HashSet<String>,
    lifted: &mut Vec<FnDef>,
    counter: &mut usize,
) -> Spanned<Expr> {
    let span = expr.span;
    match expr.node {
        Expr::Lambda {
            params,
            return_type,
            body,
        } => {
            let inner_scope = extend_scope(scope, &params);
            let body = lift_expr(*body, &inner_scope, top_names, lifted, counter);

            let mut free_vars = Vec::new();
            find_free_vars(&body.node, &params_to_set(&params), &mut free_vars);
            free_vars.retain(|v| scope.contains(v) && !top_names.contains(v));
            free_vars.sort();
            free_vars.dedup();

            let lifted_name = format!("lifted_{}", *counter);
            *counter += 1;

            let mut lifted_params = Vec::new();
            for fv in &free_vars {
                lifted_params.push(Param {
                    name: fv.clone(),
                    type_expr: TypeExpr::Named {
                        name: "Int".to_string(),
                        args: Vec::new(),
                    },
                    span,
                });
            }
            lifted_params.extend(params.clone());

            let lifted_return_type = return_type.clone().or(Some(TypeExpr::Named {
                name: "Int".to_string(),
                args: Vec::new(),
            }));

            lifted.push(FnDef {
                is_pub: false,
                is_local: false,
                name: lifted_name.clone(),
                params: lifted_params,
                return_type: lifted_return_type,
                body,
                span,
            });

            let call_args: Vec<Spanned<Expr>> = free_vars
                .iter()
                .map(|v| Spanned {
                    node: Expr::Var(v.clone()),
                    span,
                })
                .chain(params.iter().map(|p| Spanned {
                    node: Expr::Var(p.name.clone()),
                    span,
                }))
                .collect();

            Spanned {
                node: Expr::Lambda {
                    params,
                    return_type,
                    body: Box::new(Spanned {
                        node: Expr::Call {
                            function: Box::new(Spanned {
                                node: Expr::Var(lifted_name),
                                span,
                            }),
                            args: call_args,
                        },
                        span,
                    }),
                },
                span,
            }
        }

        Expr::Let {
            pattern,
            type_annotation,
            value,
            body,
        } => {
            let value = lift_expr(*value, scope, top_names, lifted, counter);
            let mut new_scope = scope.clone();
            bind_pattern(&pattern.node, &mut new_scope);
            let body = lift_expr(*body, &new_scope, top_names, lifted, counter);
            Spanned {
                node: Expr::Let {
                    pattern,
                    type_annotation,
                    value: Box::new(value),
                    body: Box::new(body),
                },
                span,
            }
        }

        Expr::Case { subject, arms } => {
            let subject = lift_expr(*subject, scope, top_names, lifted, counter);
            let arms = arms
                .into_iter()
                .map(|arm| {
                    let mut arm_scope = scope.clone();
                    bind_pattern(&arm.pattern.node, &mut arm_scope);
                    let guard = arm
                        .guard
                        .map(|g| lift_expr(g, &arm_scope, top_names, lifted, counter));
                    let body = lift_expr(arm.body, &arm_scope, top_names, lifted, counter);
                    CaseArm {
                        pattern: arm.pattern,
                        guard,
                        body,
                        span: arm.span,
                    }
                })
                .collect();
            Spanned {
                node: Expr::Case {
                    subject: Box::new(subject),
                    arms,
                },
                span,
            }
        }

        Expr::Call { function, args } => {
            let function = lift_expr(*function, scope, top_names, lifted, counter);
            let args = args
                .into_iter()
                .map(|a| lift_expr(a, scope, top_names, lifted, counter))
                .collect();
            Spanned {
                node: Expr::Call {
                    function: Box::new(function),
                    args,
                },
                span,
            }
        }

        Expr::BinOp { op, left, right } => {
            let left = lift_expr(*left, scope, top_names, lifted, counter);
            let right = lift_expr(*right, scope, top_names, lifted, counter);
            Spanned {
                node: Expr::BinOp {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            }
        }

        Expr::UnaryOp { op, operand } => {
            let operand = lift_expr(*operand, scope, top_names, lifted, counter);
            Spanned {
                node: Expr::UnaryOp {
                    op,
                    operand: Box::new(operand),
                },
                span,
            }
        }

        Expr::Pipe { left, right } => {
            let left = lift_expr(*left, scope, top_names, lifted, counter);
            let right = lift_expr(*right, scope, top_names, lifted, counter);
            Spanned {
                node: Expr::Pipe {
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            }
        }

        Expr::Block(exprs) => {
            let mut block_scope = scope.clone();
            let exprs = exprs
                .into_iter()
                .map(|e| {
                    let result = lift_expr(e, &block_scope, top_names, lifted, counter);
                    if let Expr::Let { ref pattern, .. } = result.node {
                        bind_pattern(&pattern.node, &mut block_scope);
                    }
                    result
                })
                .collect();
            Spanned {
                node: Expr::Block(exprs),
                span,
            }
        }

        Expr::Tuple(elems) => {
            let elems = elems
                .into_iter()
                .map(|e| lift_expr(e, scope, top_names, lifted, counter))
                .collect();
            Spanned {
                node: Expr::Tuple(elems),
                span,
            }
        }

        Expr::List(elems) => {
            let elems = elems
                .into_iter()
                .map(|e| lift_expr(e, scope, top_names, lifted, counter))
                .collect();
            Spanned {
                node: Expr::List(elems),
                span,
            }
        }

        Expr::ListCons { head, tail } => {
            let head = lift_expr(*head, scope, top_names, lifted, counter);
            let tail = lift_expr(*tail, scope, top_names, lifted, counter);
            Spanned {
                node: Expr::ListCons {
                    head: Box::new(head),
                    tail: Box::new(tail),
                },
                span,
            }
        }

        Expr::FieldAccess { object, field } => {
            let object = lift_expr(*object, scope, top_names, lifted, counter);
            Spanned {
                node: Expr::FieldAccess {
                    object: Box::new(object),
                    field,
                },
                span,
            }
        }

        Expr::MethodCall {
            object,
            method,
            args,
        } => {
            let object = lift_expr(*object, scope, top_names, lifted, counter);
            let args = args
                .into_iter()
                .map(|a| lift_expr(a, scope, top_names, lifted, counter))
                .collect();
            Spanned {
                node: Expr::MethodCall {
                    object: Box::new(object),
                    method,
                    args,
                },
                span,
            }
        }

        Expr::Constructor { name, args } => {
            let args = args
                .into_iter()
                .map(|a| match a {
                    ConstructorArg::Positional(e) => {
                        ConstructorArg::Positional(lift_expr(e, scope, top_names, lifted, counter))
                    }
                    ConstructorArg::Named(n, e) => {
                        ConstructorArg::Named(n, lift_expr(e, scope, top_names, lifted, counter))
                    }
                })
                .collect();
            Spanned {
                node: Expr::Constructor { name, args },
                span,
            }
        }

        Expr::RecordUpdate {
            name,
            base,
            updates,
        } => {
            let base = lift_expr(*base, scope, top_names, lifted, counter);
            let updates = updates
                .into_iter()
                .map(|(n, e)| (n, lift_expr(e, scope, top_names, lifted, counter)))
                .collect();
            Spanned {
                node: Expr::RecordUpdate {
                    name,
                    base: Box::new(base),
                    updates,
                },
                span,
            }
        }

        Expr::Clone(inner) => {
            let inner = lift_expr(*inner, scope, top_names, lifted, counter);
            Spanned {
                node: Expr::Clone(Box::new(inner)),
                span,
            }
        }

        Expr::TcoLoop { body } => {
            let body = lift_expr(*body, scope, top_names, lifted, counter);
            Spanned {
                node: Expr::TcoLoop {
                    body: Box::new(body),
                },
                span,
            }
        }

        Expr::TcoContinue { args } => {
            let args = args
                .into_iter()
                .map(|(name, e)| {
                    (
                        name,
                        Box::new(lift_expr(*e, scope, top_names, lifted, counter)),
                    )
                })
                .collect();
            Spanned {
                node: Expr::TcoContinue { args },
                span,
            }
        }

        other => Spanned { node: other, span },
    }
}

fn extend_scope(scope: &HashSet<String>, params: &[Param]) -> HashSet<String> {
    let mut new_scope = scope.clone();
    for p in params {
        new_scope.insert(p.name.clone());
    }
    new_scope
}

fn params_to_set(params: &[Param]) -> HashSet<String> {
    params.iter().map(|p| p.name.clone()).collect()
}

fn bind_pattern(pattern: &Pattern, scope: &mut HashSet<String>) {
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
                scope.insert(f.binding.as_ref().unwrap_or(&f.field_name).clone());
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

fn find_free_vars(expr: &Expr, scope: &HashSet<String>, free: &mut Vec<String>) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use crate::token::Lexer;

    fn parse_and_lift(source: &str) -> Module {
        let tokens = Lexer::tokenize(source);
        let mut parser = Parser::new(tokens);
        let mut module = parser.parse_module().expect("parse failed");
        apply_lambda_lifting(&mut module);
        module
    }

    fn has_lifted_fn(module: &Module, prefix: &str) -> bool {
        module.definitions.iter().any(|d| {
            if let Definition::Function(f) = d {
                f.name.starts_with(prefix)
            } else {
                false
            }
        })
    }

    #[test]
    fn lift_no_capture_lambda() {
        let module = parse_and_lift("fn test() -> Int { fn(x: Int) { x + 1 } }");
        assert!(has_lifted_fn(&module, "lifted_"));
    }

    #[test]
    fn lift_capturing_lambda_has_extra_params() {
        let module = parse_and_lift("fn test(y: Int) -> Int { fn(x: Int) { x + y } }");
        let lifted = module.definitions.iter().find_map(|d| {
            if let Definition::Function(f) = d {
                if f.name.starts_with("lifted_") {
                    return Some(f);
                }
            }
            None
        });
        let lifted = lifted.expect("lifted fn not found");
        assert_eq!(lifted.params.len(), 2);
        assert_eq!(lifted.params[0].name, "y");
        assert_eq!(lifted.params[1].name, "x");
    }

    #[test]
    fn lambda_body_becomes_forwarding_call() {
        let module = parse_and_lift("fn test(y: Int) -> Int { fn(x: Int) { x + y } }");
        if let Definition::Function(f) = &module.definitions[0] {
            if let Expr::Lambda { body, .. } = &f.body.node {
                assert!(
                    matches!(&body.node, Expr::Call { function, .. } if matches!(&function.node, Expr::Var(n) if n.starts_with("lifted_"))),
                    "lambda body should be a call to the lifted function"
                );
            } else {
                panic!("expected Lambda");
            }
        }
    }
}
