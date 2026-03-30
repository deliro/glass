use std::collections::HashMap;

use crate::ast::*;

pub fn resolve_const_patterns(module: &mut Module) {
    let consts = collect_constants(&module.definitions);
    if consts.is_empty() {
        return;
    }
    let defs = std::mem::take(&mut module.definitions);
    module.definitions = defs
        .into_iter()
        .map(|def| match def {
            Definition::Function(mut f) => {
                f.body = resolve_expr(f.body, &consts);
                Definition::Function(f)
            }
            Definition::Const(mut c) => {
                c.value = resolve_expr(c.value, &consts);
                Definition::Const(c)
            }
            other => other,
        })
        .collect();
}

#[derive(Clone)]
enum ConstValue {
    Int(i64),
    Rawcode(String),
    String(String),
    Bool(bool),
}

fn collect_constants(definitions: &[Definition]) -> HashMap<String, ConstValue> {
    let mut consts = HashMap::new();
    for def in definitions {
        if let Definition::Const(c) = def {
            if let Some(val) = expr_to_const_value(&c.value.node) {
                consts.insert(c.name.clone(), val);
            }
        }
    }
    consts
}

fn expr_to_const_value(expr: &Expr) -> Option<ConstValue> {
    match expr {
        Expr::Int(n) => Some(ConstValue::Int(*n)),
        Expr::Rawcode(s) => Some(ConstValue::Rawcode(s.clone())),
        Expr::String(s) => Some(ConstValue::String(s.clone())),
        Expr::Bool(b) => Some(ConstValue::Bool(*b)),
        _ => None,
    }
}

fn const_to_pattern(val: &ConstValue) -> Pattern {
    match val {
        ConstValue::Int(n) => Pattern::Int(*n),
        ConstValue::Rawcode(s) => Pattern::Rawcode(s.clone()),
        ConstValue::String(s) => Pattern::String(s.clone()),
        ConstValue::Bool(b) => Pattern::Bool(*b),
    }
}

fn resolve_pattern(pat: Spanned<Pattern>, consts: &HashMap<String, ConstValue>) -> Spanned<Pattern> {
    let span = pat.span;
    match pat.node {
        Pattern::Var(ref name) if consts.contains_key(name) => {
            Spanned::new(const_to_pattern(&consts[name]), span)
        }
        Pattern::Constructor { ref name, ref args } if args.is_empty() => {
            if let Some(val) = consts.get(name) {
                Spanned::new(const_to_pattern(val), span)
            } else {
                pat
            }
        }
        Pattern::Tuple(elems) => Spanned::new(
            Pattern::Tuple(elems.into_iter().map(|p| resolve_pattern(p, consts)).collect()),
            span,
        ),
        Pattern::List(elems) => Spanned::new(
            Pattern::List(elems.into_iter().map(|p| resolve_pattern(p, consts)).collect()),
            span,
        ),
        Pattern::ListCons { head, tail } => Spanned::new(
            Pattern::ListCons {
                head: Box::new(resolve_pattern(*head, consts)),
                tail: Box::new(resolve_pattern(*tail, consts)),
            },
            span,
        ),
        Pattern::Or(alts) => Spanned::new(
            Pattern::Or(alts.into_iter().map(|p| resolve_pattern(p, consts)).collect()),
            span,
        ),
        Pattern::As { pattern, name } => Spanned::new(
            Pattern::As {
                pattern: Box::new(resolve_pattern(*pattern, consts)),
                name,
            },
            span,
        ),
        Pattern::Constructor { name, args } => Spanned::new(
            Pattern::Constructor {
                name,
                args: args.into_iter().map(|p| resolve_pattern(p, consts)).collect(),
            },
            span,
        ),
        Pattern::ConstructorNamed { name, fields, rest } => Spanned::new(
            Pattern::ConstructorNamed {
                name,
                fields: fields
                    .into_iter()
                    .map(|f| FieldPattern {
                        field_name: f.field_name,
                        pattern: f.pattern.map(|p| resolve_pattern(p, consts)),
                    })
                    .collect(),
                rest,
            },
            span,
        ),
        other @ (Pattern::Var(_)
        | Pattern::Discard
        | Pattern::Int(_)
        | Pattern::String(_)
        | Pattern::Bool(_)
        | Pattern::Rawcode(_)) => Spanned::new(other, span),
    }
}

fn resolve_expr(expr: Spanned<Expr>, consts: &HashMap<String, ConstValue>) -> Spanned<Expr> {
    let span = expr.span;
    match expr.node {
        Expr::Case { subject, arms } => Spanned::new(
            Expr::Case {
                subject: Box::new(resolve_expr(*subject, consts)),
                arms: arms
                    .into_iter()
                    .map(|arm| CaseArm {
                        pattern: resolve_pattern(arm.pattern, consts),
                        guard: arm.guard.map(|g| resolve_expr(g, consts)),
                        body: resolve_expr(arm.body, consts),
                        span: arm.span,
                    })
                    .collect(),
            },
            span,
        ),
        Expr::Let {
            pattern,
            type_annotation,
            value,
            body,
        } => Spanned::new(
            Expr::Let {
                pattern,
                type_annotation,
                value: Box::new(resolve_expr(*value, consts)),
                body: Box::new(resolve_expr(*body, consts)),
            },
            span,
        ),
        Expr::BinOp { op, left, right } => Spanned::new(
            Expr::BinOp {
                op,
                left: Box::new(resolve_expr(*left, consts)),
                right: Box::new(resolve_expr(*right, consts)),
            },
            span,
        ),
        Expr::UnaryOp { op, operand } => Spanned::new(
            Expr::UnaryOp {
                op,
                operand: Box::new(resolve_expr(*operand, consts)),
            },
            span,
        ),
        Expr::Call { function, args } => Spanned::new(
            Expr::Call {
                function: Box::new(resolve_expr(*function, consts)),
                args: args.into_iter().map(|a| resolve_expr(a, consts)).collect(),
            },
            span,
        ),
        Expr::Lambda {
            params,
            return_type,
            body,
        } => Spanned::new(
            Expr::Lambda {
                params,
                return_type,
                body: Box::new(resolve_expr(*body, consts)),
            },
            span,
        ),
        Expr::Pipe { left, right } => Spanned::new(
            Expr::Pipe {
                left: Box::new(resolve_expr(*left, consts)),
                right: Box::new(resolve_expr(*right, consts)),
            },
            span,
        ),
        Expr::Block(exprs) => Spanned::new(
            Expr::Block(exprs.into_iter().map(|e| resolve_expr(e, consts)).collect()),
            span,
        ),
        Expr::Tuple(elems) => Spanned::new(
            Expr::Tuple(elems.into_iter().map(|e| resolve_expr(e, consts)).collect()),
            span,
        ),
        Expr::List(elems) => Spanned::new(
            Expr::List(elems.into_iter().map(|e| resolve_expr(e, consts)).collect()),
            span,
        ),
        Expr::ListCons { head, tail } => Spanned::new(
            Expr::ListCons {
                head: Box::new(resolve_expr(*head, consts)),
                tail: Box::new(resolve_expr(*tail, consts)),
            },
            span,
        ),
        Expr::FieldAccess { object, field } => Spanned::new(
            Expr::FieldAccess {
                object: Box::new(resolve_expr(*object, consts)),
                field,
            },
            span,
        ),
        Expr::MethodCall {
            object,
            method,
            args,
        } => Spanned::new(
            Expr::MethodCall {
                object: Box::new(resolve_expr(*object, consts)),
                method,
                args: args.into_iter().map(|a| resolve_expr(a, consts)).collect(),
            },
            span,
        ),
        Expr::Constructor { name, args } => Spanned::new(
            Expr::Constructor {
                name,
                args: args
                    .into_iter()
                    .map(|a| match a {
                        ConstructorArg::Positional(e) => {
                            ConstructorArg::Positional(resolve_expr(e, consts))
                        }
                        ConstructorArg::Named(n, e) => {
                            ConstructorArg::Named(n, resolve_expr(e, consts))
                        }
                    })
                    .collect(),
            },
            span,
        ),
        Expr::RecordUpdate {
            name,
            base,
            updates,
        } => Spanned::new(
            Expr::RecordUpdate {
                name,
                base: Box::new(resolve_expr(*base, consts)),
                updates: updates
                    .into_iter()
                    .map(|(n, e)| (n, resolve_expr(e, consts)))
                    .collect(),
            },
            span,
        ),
        Expr::Clone(inner) => Spanned::new(
            Expr::Clone(Box::new(resolve_expr(*inner, consts))),
            span,
        ),
        Expr::TcoLoop { body } => Spanned::new(
            Expr::TcoLoop {
                body: Box::new(resolve_expr(*body, consts)),
            },
            span,
        ),
        Expr::TcoContinue { args } => Spanned::new(
            Expr::TcoContinue {
                args: args
                    .into_iter()
                    .map(|(n, e)| (n, Box::new(resolve_expr(*e, consts))))
                    .collect(),
            },
            span,
        ),
        other @ (Expr::Var(_)
        | Expr::Int(_)
        | Expr::Float(_)
        | Expr::String(_)
        | Expr::Rawcode(_)
        | Expr::Bool(_)
        | Expr::Todo(_)) => Spanned::new(other, span),
    }
}
