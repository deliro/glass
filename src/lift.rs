use std::collections::{HashMap, HashSet};

use crate::ast::*;

fn default_type_expr() -> TypeExpr {
    TypeExpr::Named {
        name: "Int".to_string(),
        args: Vec::new(),
    }
}

pub fn apply_lambda_lifting(module: &mut Module) {
    let mut lifted = Vec::new();
    let mut counter = 0;
    let fn_names = collect_top_level_names(&module.definitions);

    let mut defs = std::mem::take(&mut module.definitions);
    for def in &mut defs {
        if let Definition::Function(f) = def {
            let mut scope: HashMap<String, TypeExpr> = f
                .params
                .iter()
                .map(|p| (p.name.clone(), p.type_expr.clone()))
                .collect();
            for name in &fn_names {
                scope.entry(name.clone()).or_insert_with(default_type_expr);
            }
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

fn bind_pattern_typed(pattern: &Pattern, scope: &mut HashMap<String, TypeExpr>) {
    match pattern {
        Pattern::Var(name) if name != "_" => {
            scope.entry(name.clone()).or_insert_with(default_type_expr);
        }
        Pattern::Constructor { args, .. } => {
            for arg in args {
                bind_pattern_typed(&arg.node, scope);
            }
        }
        Pattern::ConstructorNamed { fields, .. } => {
            for f in fields {
                if let Some(p) = &f.pattern {
                    bind_pattern_typed(&p.node, scope);
                } else {
                    scope
                        .entry(f.field_name.clone())
                        .or_insert_with(default_type_expr);
                }
            }
        }
        Pattern::Tuple(elems) | Pattern::List(elems) => {
            for e in elems {
                bind_pattern_typed(&e.node, scope);
            }
        }
        Pattern::ListCons { head, tail } => {
            bind_pattern_typed(&head.node, scope);
            bind_pattern_typed(&tail.node, scope);
        }
        Pattern::Or(alts) => {
            for alt in alts {
                bind_pattern_typed(&alt.node, scope);
            }
        }
        Pattern::As { pattern, name } => {
            bind_pattern_typed(&pattern.node, scope);
            scope.entry(name.clone()).or_insert_with(default_type_expr);
        }
        _ => {}
    }
}

fn lift_expr(
    expr: Spanned<Expr>,
    scope: &HashMap<String, TypeExpr>,
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
            crate::free_vars::find_free_vars(&body.node, &params_to_set(&params), &mut free_vars);
            free_vars.retain(|v| scope.contains_key(v) && !top_names.contains(v));
            free_vars.sort();
            free_vars.dedup();

            let lifted_name = format!("lifted_{}", *counter);
            *counter += 1;

            let mut lifted_params = Vec::new();
            for fv in &free_vars {
                let type_expr = scope.get(fv).cloned().unwrap_or_else(default_type_expr);
                lifted_params.push(Param {
                    name: fv.clone(),
                    type_expr,
                    span,
                });
            }
            lifted_params.extend(params.clone());

            let lifted_return_type = return_type.clone().or(Some(default_type_expr()));

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
            bind_pattern_typed(&pattern.node, &mut new_scope);
            if let Pattern::Var(ref name) = pattern.node
                && let Some(ann) = &type_annotation
            {
                new_scope.insert(name.clone(), ann.clone());
            }
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
                    bind_pattern_typed(&arm.pattern.node, &mut arm_scope);
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
                        bind_pattern_typed(&pattern.node, &mut block_scope);
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

fn extend_scope(scope: &HashMap<String, TypeExpr>, params: &[Param]) -> HashMap<String, TypeExpr> {
    let mut new_scope = scope.clone();
    for p in params {
        new_scope.insert(p.name.clone(), p.type_expr.clone());
    }
    new_scope
}

fn params_to_set(params: &[Param]) -> HashSet<String> {
    params.iter().map(|p| p.name.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use crate::token::Lexer;

    fn parse_and_lift(source: &str) -> Module {
        let tokens = Lexer::tokenize(source).expect("lex failed");
        let mut parser = Parser::new(tokens);
        let mut module = {
            let _o = parser.parse_module();
            assert!(_o.errors.is_empty(), "parse errors: {:?}", _o.errors);
            _o.module
        };
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
    fn lift_with_capture() {
        let module = parse_and_lift("fn test(y: Int) -> Int { fn(x: Int) { x + y } }");
        assert!(has_lifted_fn(&module, "lifted_"));
    }

    #[test]
    fn lift_preserves_non_lambda() {
        let module = parse_and_lift("fn test(x: Int) -> Int { x + 1 }");
        assert!(!has_lifted_fn(&module, "lifted_"));
    }

    #[test]
    fn nested_lambda_both_lifted() {
        let module = parse_and_lift("fn test() -> Int { fn(x: Int) { fn(y: Int) { x + y } } }");
        let count = module
            .definitions
            .iter()
            .filter(|d| {
                if let Definition::Function(f) = d {
                    f.name.starts_with("lifted_")
                } else {
                    false
                }
            })
            .count();
        assert_eq!(count, 2);
    }

    #[test]
    fn lifted_fn_has_capture_params_first() {
        let module = parse_and_lift("fn test(y: Int) -> Int { fn(x: Int) { x + y } }");
        let lifted = module
            .definitions
            .iter()
            .find_map(|d| {
                if let Definition::Function(f) = d {
                    if f.name.starts_with("lifted_") {
                        return Some(f);
                    }
                }
                None
            })
            .expect("no lifted fn");
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
