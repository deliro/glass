use crate::ast::*;

pub fn apply_const_propagation(module: &mut Module) {
    let defs = std::mem::take(&mut module.definitions);
    module.definitions = defs
        .into_iter()
        .map(|def| match def {
            Definition::Function(mut f) => {
                f.body = propagate(f.body);
                Definition::Function(f)
            }
            other => other,
        })
        .collect();
}

fn is_cheap(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Var(_)
            | Expr::Int(_)
            | Expr::Float(_)
            | Expr::String(_)
            | Expr::Rawcode(_)
            | Expr::Bool(_)
    )
}

fn propagate(expr: Spanned<Expr>) -> Spanned<Expr> {
    let span = expr.span;
    match expr.node {
        Expr::Let {
            pattern,
            type_annotation,
            value,
            body,
        } => {
            let value = propagate(*value);
            if let Pattern::Var(ref name) = pattern.node
                && is_cheap(&value.node)
            {
                let body = substitute(&body, name, &value);
                return propagate(body);
            }
            Spanned {
                node: Expr::Let {
                    pattern,
                    type_annotation,
                    value: Box::new(value),
                    body: Box::new(propagate(*body)),
                },
                span,
            }
        }
        Expr::Case { subject, arms } => Spanned {
            node: Expr::Case {
                subject: Box::new(propagate(*subject)),
                arms: arms
                    .into_iter()
                    .map(|arm| CaseArm {
                        pattern: arm.pattern,
                        guard: arm.guard.map(propagate),
                        body: propagate(arm.body),
                        span: arm.span,
                    })
                    .collect(),
            },
            span,
        },
        Expr::BinOp { op, left, right } => Spanned {
            node: Expr::BinOp {
                op,
                left: Box::new(propagate(*left)),
                right: Box::new(propagate(*right)),
            },
            span,
        },
        Expr::UnaryOp { op, operand } => Spanned {
            node: Expr::UnaryOp {
                op,
                operand: Box::new(propagate(*operand)),
            },
            span,
        },
        Expr::Call { function, args } => Spanned {
            node: Expr::Call {
                function: Box::new(propagate(*function)),
                args: args.into_iter().map(propagate).collect(),
            },
            span,
        },
        Expr::Lambda {
            params,
            return_type,
            body,
        } => Spanned {
            node: Expr::Lambda {
                params,
                return_type,
                body: Box::new(propagate(*body)),
            },
            span,
        },
        Expr::Pipe { left, right } => Spanned {
            node: Expr::Pipe {
                left: Box::new(propagate(*left)),
                right: Box::new(propagate(*right)),
            },
            span,
        },
        Expr::Block(exprs) => Spanned {
            node: Expr::Block(exprs.into_iter().map(propagate).collect()),
            span,
        },
        Expr::Tuple(elems) => Spanned {
            node: Expr::Tuple(elems.into_iter().map(propagate).collect()),
            span,
        },
        Expr::List(elems) => Spanned {
            node: Expr::List(elems.into_iter().map(propagate).collect()),
            span,
        },
        Expr::ListCons { head, tail } => Spanned {
            node: Expr::ListCons {
                head: Box::new(propagate(*head)),
                tail: Box::new(propagate(*tail)),
            },
            span,
        },
        Expr::FieldAccess { object, field } => Spanned {
            node: Expr::FieldAccess {
                object: Box::new(propagate(*object)),
                field,
            },
            span,
        },
        Expr::MethodCall {
            object,
            method,
            args,
        } => Spanned {
            node: Expr::MethodCall {
                object: Box::new(propagate(*object)),
                method,
                args: args.into_iter().map(propagate).collect(),
            },
            span,
        },
        Expr::Constructor { name, args } => Spanned {
            node: Expr::Constructor {
                name,
                args: args
                    .into_iter()
                    .map(|a| match a {
                        ConstructorArg::Positional(e) => ConstructorArg::Positional(propagate(e)),
                        ConstructorArg::Named(n, e) => ConstructorArg::Named(n, propagate(e)),
                    })
                    .collect(),
            },
            span,
        },
        Expr::RecordUpdate {
            name,
            base,
            updates,
        } => Spanned {
            node: Expr::RecordUpdate {
                name,
                base: Box::new(propagate(*base)),
                updates: updates
                    .into_iter()
                    .map(|(n, e)| (n, propagate(e)))
                    .collect(),
            },
            span,
        },
        Expr::Clone(inner) => Spanned {
            node: Expr::Clone(Box::new(propagate(*inner))),
            span,
        },
        Expr::TcoLoop { body } => Spanned {
            node: Expr::TcoLoop {
                body: Box::new(propagate(*body)),
            },
            span,
        },
        Expr::TcoContinue { args } => Spanned {
            node: Expr::TcoContinue {
                args: args
                    .into_iter()
                    .map(|(n, e)| (n, Box::new(propagate(*e))))
                    .collect(),
            },
            span,
        },
        other @ (Expr::Var(_)
        | Expr::Int(_)
        | Expr::Float(_)
        | Expr::String(_)
        | Expr::Rawcode(_)
        | Expr::Bool(_)
        | Expr::Todo(_)) => Spanned { node: other, span },
    }
}

fn substitute(expr: &Spanned<Expr>, name: &str, replacement: &Spanned<Expr>) -> Spanned<Expr> {
    let span = expr.span;
    match &expr.node {
        Expr::Var(v) => {
            if v == name {
                replacement.clone()
            } else {
                expr.clone()
            }
        }
        Expr::Let {
            pattern,
            type_annotation,
            value,
            body,
        } => {
            let value = substitute(value, name, replacement);
            let shadows = pattern_binds(pattern, name);
            let body = if shadows {
                *body.clone()
            } else {
                substitute(body, name, replacement)
            };
            Spanned {
                node: Expr::Let {
                    pattern: pattern.clone(),
                    type_annotation: type_annotation.clone(),
                    value: Box::new(value),
                    body: Box::new(body),
                },
                span,
            }
        }
        Expr::Case { subject, arms } => Spanned {
            node: Expr::Case {
                subject: Box::new(substitute(subject, name, replacement)),
                arms: arms
                    .iter()
                    .map(|arm| {
                        let shadows = pattern_binds(&arm.pattern, name);
                        CaseArm {
                            pattern: arm.pattern.clone(),
                            guard: if shadows {
                                arm.guard.clone()
                            } else {
                                arm.guard.as_ref().map(|g| substitute(g, name, replacement))
                            },
                            body: if shadows {
                                arm.body.clone()
                            } else {
                                substitute(&arm.body, name, replacement)
                            },
                            span: arm.span,
                        }
                    })
                    .collect(),
            },
            span,
        },
        Expr::BinOp { op, left, right } => Spanned {
            node: Expr::BinOp {
                op: *op,
                left: Box::new(substitute(left, name, replacement)),
                right: Box::new(substitute(right, name, replacement)),
            },
            span,
        },
        Expr::UnaryOp { op, operand } => Spanned {
            node: Expr::UnaryOp {
                op: *op,
                operand: Box::new(substitute(operand, name, replacement)),
            },
            span,
        },
        Expr::Call { function, args } => Spanned {
            node: Expr::Call {
                function: Box::new(substitute(function, name, replacement)),
                args: args
                    .iter()
                    .map(|a| substitute(a, name, replacement))
                    .collect(),
            },
            span,
        },
        Expr::Lambda {
            params,
            return_type,
            body,
        } => {
            let shadows = params.iter().any(|p| p.name == name);
            Spanned {
                node: Expr::Lambda {
                    params: params.clone(),
                    return_type: return_type.clone(),
                    body: if shadows {
                        body.clone()
                    } else {
                        Box::new(substitute(body, name, replacement))
                    },
                },
                span,
            }
        }
        Expr::Pipe { left, right } => Spanned {
            node: Expr::Pipe {
                left: Box::new(substitute(left, name, replacement)),
                right: Box::new(substitute(right, name, replacement)),
            },
            span,
        },
        Expr::Block(exprs) => {
            let mut result = Vec::new();
            let mut shadowed = false;
            for e in exprs {
                if shadowed {
                    result.push(e.clone());
                } else {
                    result.push(substitute(e, name, replacement));
                    if let Expr::Let { pattern, .. } = &e.node
                        && pattern_binds(pattern, name)
                    {
                        shadowed = true;
                    }
                }
            }
            Spanned {
                node: Expr::Block(result),
                span,
            }
        }
        Expr::Tuple(elems) => Spanned {
            node: Expr::Tuple(
                elems
                    .iter()
                    .map(|e| substitute(e, name, replacement))
                    .collect(),
            ),
            span,
        },
        Expr::List(elems) => Spanned {
            node: Expr::List(
                elems
                    .iter()
                    .map(|e| substitute(e, name, replacement))
                    .collect(),
            ),
            span,
        },
        Expr::ListCons { head, tail } => Spanned {
            node: Expr::ListCons {
                head: Box::new(substitute(head, name, replacement)),
                tail: Box::new(substitute(tail, name, replacement)),
            },
            span,
        },
        Expr::FieldAccess { object, field } => Spanned {
            node: Expr::FieldAccess {
                object: Box::new(substitute(object, name, replacement)),
                field: field.clone(),
            },
            span,
        },
        Expr::MethodCall {
            object,
            method,
            args,
        } => Spanned {
            node: Expr::MethodCall {
                object: Box::new(substitute(object, name, replacement)),
                method: method.clone(),
                args: args
                    .iter()
                    .map(|a| substitute(a, name, replacement))
                    .collect(),
            },
            span,
        },
        Expr::Constructor { name: cname, args } => Spanned {
            node: Expr::Constructor {
                name: cname.clone(),
                args: args
                    .iter()
                    .map(|a| match a {
                        ConstructorArg::Positional(e) => {
                            ConstructorArg::Positional(substitute(e, name, replacement))
                        }
                        ConstructorArg::Named(n, e) => {
                            ConstructorArg::Named(n.clone(), substitute(e, name, replacement))
                        }
                    })
                    .collect(),
            },
            span,
        },
        Expr::RecordUpdate {
            name: rname,
            base,
            updates,
        } => Spanned {
            node: Expr::RecordUpdate {
                name: rname.clone(),
                base: Box::new(substitute(base, name, replacement)),
                updates: updates
                    .iter()
                    .map(|(n, e)| (n.clone(), substitute(e, name, replacement)))
                    .collect(),
            },
            span,
        },
        Expr::Clone(inner) => Spanned {
            node: Expr::Clone(Box::new(substitute(inner, name, replacement))),
            span,
        },
        Expr::TcoLoop { body } => Spanned {
            node: Expr::TcoLoop {
                body: Box::new(substitute(body, name, replacement)),
            },
            span,
        },
        Expr::TcoContinue { args } => Spanned {
            node: Expr::TcoContinue {
                args: args
                    .iter()
                    .map(|(n, e)| (n.clone(), Box::new(substitute(e, name, replacement))))
                    .collect(),
            },
            span,
        },
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::String(_)
        | Expr::Rawcode(_)
        | Expr::Bool(_)
        | Expr::Todo(_) => expr.clone(),
    }
}

fn pattern_binds(pat: &Spanned<Pattern>, name: &str) -> bool {
    match &pat.node {
        Pattern::Var(v) => v == name,
        Pattern::Constructor { args, .. } => args.iter().any(|a| pattern_binds(a, name)),
        Pattern::ConstructorNamed { fields, .. } => fields.iter().any(|f| {
            f.pattern
                .as_ref()
                .map_or(f.field_name == name, |p| pattern_binds(p, name))
        }),
        Pattern::Tuple(elems) | Pattern::List(elems) | Pattern::Or(elems) => {
            elems.iter().any(|e| pattern_binds(e, name))
        }
        Pattern::ListCons { head, tail } => pattern_binds(head, name) || pattern_binds(tail, name),
        Pattern::As { pattern, name: n } => n == name || pattern_binds(pattern, name),
        Pattern::Discard
        | Pattern::Int(_)
        | Pattern::String(_)
        | Pattern::Bool(_)
        | Pattern::Rawcode(_) => false,
    }
}
