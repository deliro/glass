use crate::ast::*;

pub fn apply_beta_reduction(module: &mut Module) {
    let defs = std::mem::take(&mut module.definitions);
    module.definitions = defs
        .into_iter()
        .map(|def| match def {
            Definition::Function(mut f) => {
                f.body = reduce_expr(f.body);
                Definition::Function(f)
            }
            other => other,
        })
        .collect();
}

fn reduce_expr(expr: Spanned<Expr>) -> Spanned<Expr> {
    let span = expr.span;
    match expr.node {
        Expr::Pipe { left, right } => {
            let left = reduce_expr(*left);
            let right = reduce_expr(*right);
            match right.node {
                Expr::Lambda {
                    ref params,
                    ref body,
                    ..
                } if params.len() == 1 => {
                    let param_name = params.first().map(|p| p.name.clone()).unwrap_or_default();
                    let body = *body.clone();
                    Spanned {
                        node: Expr::Let {
                            pattern: Spanned {
                                node: Pattern::Var(param_name),
                                span,
                            },
                            type_annotation: None,
                            value: Box::new(left),
                            body: Box::new(body),
                        },
                        span,
                    }
                }
                _ => Spanned {
                    node: Expr::Pipe {
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    span,
                },
            }
        }
        Expr::Call { function, args } => {
            let function = reduce_expr(*function);
            let args: Vec<_> = args.into_iter().map(reduce_expr).collect();
            if let Expr::Lambda { params, body, .. } = &function.node
                && params.len() == args.len()
            {
                let mut result = *body.clone();
                for (p, a) in params.iter().zip(args.iter()).rev() {
                    result = Spanned {
                        node: Expr::Let {
                            pattern: Spanned {
                                node: Pattern::Var(p.name.clone()),
                                span,
                            },
                            type_annotation: None,
                            value: Box::new(a.clone()),
                            body: Box::new(result),
                        },
                        span,
                    };
                }
                return reduce_expr(result);
            }
            Spanned {
                node: Expr::Call {
                    function: Box::new(function),
                    args,
                },
                span,
            }
        }
        Expr::Let {
            pattern,
            type_annotation,
            value,
            body,
        } => Spanned {
            node: Expr::Let {
                pattern,
                type_annotation,
                value: Box::new(reduce_expr(*value)),
                body: Box::new(reduce_expr(*body)),
            },
            span,
        },
        Expr::Case { subject, arms } => Spanned {
            node: Expr::Case {
                subject: Box::new(reduce_expr(*subject)),
                arms: arms
                    .into_iter()
                    .map(|arm| CaseArm {
                        pattern: arm.pattern,
                        guard: arm.guard.map(reduce_expr),
                        body: reduce_expr(arm.body),
                        span: arm.span,
                    })
                    .collect(),
            },
            span,
        },
        Expr::BinOp { op, left, right } => Spanned {
            node: Expr::BinOp {
                op,
                left: Box::new(reduce_expr(*left)),
                right: Box::new(reduce_expr(*right)),
            },
            span,
        },
        Expr::UnaryOp { op, operand } => Spanned {
            node: Expr::UnaryOp {
                op,
                operand: Box::new(reduce_expr(*operand)),
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
                body: Box::new(reduce_expr(*body)),
            },
            span,
        },
        Expr::Block(exprs) => Spanned {
            node: Expr::Block(exprs.into_iter().map(reduce_expr).collect()),
            span,
        },
        Expr::Tuple(elems) => Spanned {
            node: Expr::Tuple(elems.into_iter().map(reduce_expr).collect()),
            span,
        },
        Expr::List(elems) => Spanned {
            node: Expr::List(elems.into_iter().map(reduce_expr).collect()),
            span,
        },
        Expr::ListCons { head, tail } => Spanned {
            node: Expr::ListCons {
                head: Box::new(reduce_expr(*head)),
                tail: Box::new(reduce_expr(*tail)),
            },
            span,
        },
        Expr::FieldAccess { object, field } => Spanned {
            node: Expr::FieldAccess {
                object: Box::new(reduce_expr(*object)),
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
                object: Box::new(reduce_expr(*object)),
                method,
                args: args.into_iter().map(reduce_expr).collect(),
            },
            span,
        },
        Expr::Constructor { name, args } => Spanned {
            node: Expr::Constructor {
                name,
                args: args
                    .into_iter()
                    .map(|a| match a {
                        ConstructorArg::Positional(e) => ConstructorArg::Positional(reduce_expr(e)),
                        ConstructorArg::Named(n, e) => ConstructorArg::Named(n, reduce_expr(e)),
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
                base: Box::new(reduce_expr(*base)),
                updates: updates
                    .into_iter()
                    .map(|(n, e)| (n, reduce_expr(e)))
                    .collect(),
            },
            span,
        },
        Expr::Clone(inner) => Spanned {
            node: Expr::Clone(Box::new(reduce_expr(*inner))),
            span,
        },
        Expr::TcoLoop { body } => Spanned {
            node: Expr::TcoLoop {
                body: Box::new(reduce_expr(*body)),
            },
            span,
        },
        Expr::TcoContinue { args } => Spanned {
            node: Expr::TcoContinue {
                args: args
                    .into_iter()
                    .map(|(n, e)| (n, Box::new(reduce_expr(*e))))
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
