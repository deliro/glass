use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::token::Span;

const INLINE_COST_THRESHOLD: usize = 12;

static INLINE_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn fresh_suffix() -> String {
    let n = INLINE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("_i{}", n)
}

pub fn apply_inlining(module: &mut Module) {
    INLINE_COUNTER.store(0, std::sync::atomic::Ordering::Relaxed);

    let external_names: HashSet<String> = module
        .definitions
        .iter()
        .filter_map(|d| {
            if let Definition::External(e) = d {
                Some(e.fn_name.clone())
            } else {
                None
            }
        })
        .collect();

    for _ in 0..3 {
        let info = analyze(module);
        let fn_map: HashMap<String, FnDef> = module
            .definitions
            .iter()
            .filter_map(|d| {
                if let Definition::Function(f) = d {
                    Some((f.name.clone(), f.clone()))
                } else {
                    None
                }
            })
            .collect();

        let mut defs = std::mem::take(&mut module.definitions);
        for def in &mut defs {
            if let Definition::Function(f) = def {
                f.body = inline_expr(f.body.clone(), &fn_map, &info, &external_names);
            }
        }
        module.definitions = defs;
    }
}

#[derive(Debug)]
struct InlineInfo {
    call_counts: HashMap<String, usize>,
    recursive: HashSet<String>,
}

fn analyze(module: &Module) -> InlineInfo {
    let mut call_counts: HashMap<String, usize> = HashMap::new();
    let mut recursive = HashSet::new();

    for def in &module.definitions {
        if let Definition::Function(f) = def {
            if contains_tco(&f.body.node) {
                recursive.insert(f.name.clone());
            }
            let mut calls = Vec::new();
            collect_calls(&f.body.node, &mut calls);
            for name in &calls {
                if *name == f.name {
                    recursive.insert(f.name.clone());
                }
                *call_counts.entry(name.clone()).or_default() += 1;
            }
        }
    }

    InlineInfo {
        call_counts,
        recursive,
    }
}

fn contains_tco(expr: &Expr) -> bool {
    matches!(expr, Expr::TcoLoop { .. } | Expr::TcoContinue { .. })
}

fn collect_calls(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Call { function, args } => {
            if let Expr::Var(name) = &function.node {
                out.push(name.clone());
            }
            collect_calls(&function.node, out);
            for a in args {
                collect_calls(&a.node, out);
            }
        }
        Expr::Let { value, body, .. } => {
            collect_calls(&value.node, out);
            collect_calls(&body.node, out);
        }
        Expr::Case { subject, arms } => {
            collect_calls(&subject.node, out);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_calls(&guard.node, out);
                }
                collect_calls(&arm.body.node, out);
            }
        }
        Expr::BinOp { left, right, .. } | Expr::Pipe { left, right } => {
            collect_calls(&left.node, out);
            collect_calls(&right.node, out);
        }
        Expr::UnaryOp { operand, .. } | Expr::Clone(operand) => {
            collect_calls(&operand.node, out);
        }
        Expr::FieldAccess { object, .. } => collect_calls(&object.node, out),
        Expr::MethodCall {
            object,
            method,
            args,
        } => {
            if matches!(&object.node, Expr::Var(_)) {
                out.push(method.clone());
            }
            collect_calls(&object.node, out);
            for a in args {
                collect_calls(&a.node, out);
            }
        }
        Expr::Block(exprs) => {
            for e in exprs {
                collect_calls(&e.node, out);
            }
        }
        Expr::Tuple(elems) | Expr::List(elems) => {
            for e in elems {
                collect_calls(&e.node, out);
            }
        }
        Expr::ListCons { head, tail } => {
            collect_calls(&head.node, out);
            collect_calls(&tail.node, out);
        }
        Expr::Lambda { body, .. } | Expr::TcoLoop { body } => collect_calls(&body.node, out),
        Expr::Constructor { args, .. } => {
            for a in args {
                match a {
                    ConstructorArg::Positional(e) | ConstructorArg::Named(_, e) => {
                        collect_calls(&e.node, out);
                    }
                }
            }
        }
        Expr::RecordUpdate { base, updates, .. } => {
            collect_calls(&base.node, out);
            for (_, e) in updates {
                collect_calls(&e.node, out);
            }
        }
        Expr::TcoContinue { args } => {
            for (_, e) in args {
                collect_calls(&e.node, out);
            }
        }
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::String(_)
        | Expr::Rawcode(_)
        | Expr::Bool(_)
        | Expr::Var(_)
        | Expr::Todo(_) => {}
    }
}

fn expr_cost(expr: &Expr) -> usize {
    match expr {
        Expr::Var(_)
        | Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::Rawcode(_)
        | Expr::Todo(_) => 0,
        Expr::BinOp { left, right, .. } | Expr::Pipe { left, right } => {
            1 + expr_cost(&left.node) + expr_cost(&right.node)
        }
        Expr::UnaryOp { operand, .. } => 1 + expr_cost(&operand.node),
        Expr::Constructor { args, .. } => {
            1 + args
                .iter()
                .map(|a| match a {
                    ConstructorArg::Positional(e) | ConstructorArg::Named(_, e) => {
                        expr_cost(&e.node)
                    }
                })
                .sum::<usize>()
        }
        Expr::Let { value, body, .. } => 1 + expr_cost(&value.node) + expr_cost(&body.node),
        Expr::Call { function, args } => {
            2 + expr_cost(&function.node) + args.iter().map(|a| expr_cost(&a.node)).sum::<usize>()
        }
        Expr::Case { subject, arms } => {
            2 + expr_cost(&subject.node)
                + arms.iter().map(|a| expr_cost(&a.body.node)).sum::<usize>()
        }
        Expr::FieldAccess { object, .. } => 1 + expr_cost(&object.node),
        Expr::MethodCall { object, args, .. } => {
            2 + expr_cost(&object.node) + args.iter().map(|a| expr_cost(&a.node)).sum::<usize>()
        }
        Expr::Block(exprs) => exprs.iter().map(|e| expr_cost(&e.node)).sum(),
        Expr::Tuple(elems) | Expr::List(elems) => elems.iter().map(|e| expr_cost(&e.node)).sum(),
        Expr::ListCons { head, tail } => 1 + expr_cost(&head.node) + expr_cost(&tail.node),
        Expr::Lambda { body, .. } | Expr::TcoLoop { body } => expr_cost(&body.node),
        Expr::Clone(inner) => expr_cost(&inner.node),
        Expr::RecordUpdate { base, updates, .. } => {
            1 + expr_cost(&base.node)
                + updates
                    .iter()
                    .map(|(_, e)| expr_cost(&e.node))
                    .sum::<usize>()
        }
        Expr::TcoContinue { args } => args.iter().map(|(_, e)| expr_cost(&e.node)).sum(),
    }
}

fn should_inline(
    name: &str,
    fdef: &FnDef,
    info: &InlineInfo,
    _external_names: &HashSet<String>,
) -> bool {
    if info.recursive.contains(name) {
        return false;
    }
    if matches!(&fdef.body.node, Expr::TcoLoop { .. }) {
        return false;
    }
    if fdef.name.starts_with("glass_new_") {
        return false;
    }
    if fdef.name.starts_with("glass_get_") {
        return false;
    }

    let count = info.call_counts.get(name).copied().unwrap_or(0);
    if count <= 1 {
        return true;
    }

    let cost = expr_cost(&fdef.body.node);
    cost <= INLINE_COST_THRESHOLD
}

fn inline_expr(
    expr: Spanned<Expr>,
    fn_map: &HashMap<String, FnDef>,
    info: &InlineInfo,
    external_names: &HashSet<String>,
) -> Spanned<Expr> {
    let span = expr.span;
    match expr.node {
        Expr::Call { function, args } => {
            let args: Vec<Spanned<Expr>> = args
                .into_iter()
                .map(|a| inline_expr(a, fn_map, info, external_names))
                .collect();

            if let Expr::Var(ref name) = function.node
                && let Some(fdef) = fn_map.get(name)
                && should_inline(name, fdef, info, external_names)
                && fdef.params.len() == args.len()
            {
                return inline_call(fdef, args, span);
            }

            let function = inline_expr(*function, fn_map, info, external_names);
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
        } => {
            let value = inline_expr(*value, fn_map, info, external_names);
            let body = inline_expr(*body, fn_map, info, external_names);
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
            let subject = inline_expr(*subject, fn_map, info, external_names);
            let arms = arms
                .into_iter()
                .map(|arm| {
                    let guard = arm
                        .guard
                        .map(|g| inline_expr(g, fn_map, info, external_names));
                    let body = inline_expr(arm.body, fn_map, info, external_names);
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

        Expr::BinOp { op, left, right } => {
            let left = inline_expr(*left, fn_map, info, external_names);
            let right = inline_expr(*right, fn_map, info, external_names);
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
            let operand = inline_expr(*operand, fn_map, info, external_names);
            Spanned {
                node: Expr::UnaryOp {
                    op,
                    operand: Box::new(operand),
                },
                span,
            }
        }

        Expr::Pipe { left, right } => {
            let left = inline_expr(*left, fn_map, info, external_names);
            let right = inline_expr(*right, fn_map, info, external_names);
            Spanned {
                node: Expr::Pipe {
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            }
        }

        Expr::Block(exprs) => {
            let exprs = exprs
                .into_iter()
                .map(|e| inline_expr(e, fn_map, info, external_names))
                .collect();
            Spanned {
                node: Expr::Block(exprs),
                span,
            }
        }

        Expr::Tuple(elems) => {
            let elems = elems
                .into_iter()
                .map(|e| inline_expr(e, fn_map, info, external_names))
                .collect();
            Spanned {
                node: Expr::Tuple(elems),
                span,
            }
        }

        Expr::List(elems) => {
            let elems = elems
                .into_iter()
                .map(|e| inline_expr(e, fn_map, info, external_names))
                .collect();
            Spanned {
                node: Expr::List(elems),
                span,
            }
        }

        Expr::ListCons { head, tail } => {
            let head = inline_expr(*head, fn_map, info, external_names);
            let tail = inline_expr(*tail, fn_map, info, external_names);
            Spanned {
                node: Expr::ListCons {
                    head: Box::new(head),
                    tail: Box::new(tail),
                },
                span,
            }
        }

        Expr::FieldAccess { object, field } => {
            let object = inline_expr(*object, fn_map, info, external_names);
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
            let args: Vec<Spanned<Expr>> = args
                .into_iter()
                .map(|a| inline_expr(a, fn_map, info, external_names))
                .collect();

            if matches!(&object.node, Expr::Var(_))
                && let Some(fdef) = fn_map.get(&method)
                && should_inline(&method, fdef, info, external_names)
                && fdef.params.len() == args.len()
            {
                return inline_call(fdef, args, span);
            }

            let object = inline_expr(*object, fn_map, info, external_names);
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
                        ConstructorArg::Positional(inline_expr(e, fn_map, info, external_names))
                    }
                    ConstructorArg::Named(n, e) => {
                        ConstructorArg::Named(n, inline_expr(e, fn_map, info, external_names))
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
            let base = inline_expr(*base, fn_map, info, external_names);
            let updates = updates
                .into_iter()
                .map(|(n, e)| (n, inline_expr(e, fn_map, info, external_names)))
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

        Expr::Lambda {
            params,
            return_type,
            body,
        } => {
            let body = inline_expr(*body, fn_map, info, external_names);
            Spanned {
                node: Expr::Lambda {
                    params,
                    return_type,
                    body: Box::new(body),
                },
                span,
            }
        }

        Expr::Clone(inner) => {
            let inner = inline_expr(*inner, fn_map, info, external_names);
            Spanned {
                node: Expr::Clone(Box::new(inner)),
                span,
            }
        }

        Expr::TcoLoop { body } => {
            let body = inline_expr(*body, fn_map, info, external_names);
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
                        Box::new(inline_expr(*e, fn_map, info, external_names)),
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

fn inline_call(fdef: &FnDef, args: Vec<Spanned<Expr>>, span: Span) -> Spanned<Expr> {
    let suffix = fresh_suffix();
    let mut rename_map: HashMap<String, String> = HashMap::new();
    for p in &fdef.params {
        if p.name != "_" {
            rename_map
                .entry(p.name.clone())
                .or_insert_with(|| format!("{}{}", p.name, &suffix));
        }
    }
    collect_body_bindings(&fdef.body.node, &mut rename_map, &suffix);

    let mut result = rename_expr(fdef.body.clone(), &rename_map);

    for (param, arg) in fdef.params.iter().zip(args.into_iter()).rev() {
        if param.name == "_" {
            continue;
        }
        let renamed_param = rename_map
            .get(&param.name)
            .cloned()
            .unwrap_or(param.name.clone());
        if let Expr::Var(ref name) = arg.node
            && *name == renamed_param
        {
            continue;
        }
        result = Spanned {
            node: Expr::Let {
                pattern: Spanned {
                    node: Pattern::Var(renamed_param),
                    span,
                },
                type_annotation: Some(param.type_expr.clone()),
                value: Box::new(arg),
                body: Box::new(result),
            },
            span,
        };
    }
    result
}

fn collect_body_bindings(expr: &Expr, map: &mut HashMap<String, String>, suffix: &str) {
    match expr {
        Expr::Let {
            pattern,
            value,
            body,
            ..
        } => {
            collect_pattern_names(&pattern.node, map, suffix);
            collect_body_bindings(&value.node, map, suffix);
            collect_body_bindings(&body.node, map, suffix);
        }
        Expr::Case { subject, arms } => {
            collect_body_bindings(&subject.node, map, suffix);
            for arm in arms {
                collect_pattern_names(&arm.pattern.node, map, suffix);
                collect_body_bindings(&arm.body.node, map, suffix);
            }
        }
        Expr::Block(exprs) => {
            for e in exprs {
                collect_body_bindings(&e.node, map, suffix);
            }
        }
        Expr::Call { function, args } => {
            collect_body_bindings(&function.node, map, suffix);
            for a in args {
                collect_body_bindings(&a.node, map, suffix);
            }
        }
        Expr::BinOp { left, right, .. } | Expr::Pipe { left, right } => {
            collect_body_bindings(&left.node, map, suffix);
            collect_body_bindings(&right.node, map, suffix);
        }
        Expr::UnaryOp { operand, .. } | Expr::Clone(operand) => {
            collect_body_bindings(&operand.node, map, suffix);
        }
        Expr::ListCons { head, tail } => {
            collect_body_bindings(&head.node, map, suffix);
            collect_body_bindings(&tail.node, map, suffix);
        }
        Expr::Lambda { body, .. } => {
            collect_body_bindings(&body.node, map, suffix);
        }
        Expr::Constructor { args, .. } => {
            for a in args {
                match a {
                    ConstructorArg::Positional(e) | ConstructorArg::Named(_, e) => {
                        collect_body_bindings(&e.node, map, suffix);
                    }
                }
            }
        }
        Expr::RecordUpdate { base, updates, .. } => {
            collect_body_bindings(&base.node, map, suffix);
            for (_, e) in updates {
                collect_body_bindings(&e.node, map, suffix);
            }
        }
        Expr::TcoLoop { body } => collect_body_bindings(&body.node, map, suffix),
        Expr::TcoContinue { args } => {
            for (_, e) in args {
                collect_body_bindings(&e.node, map, suffix);
            }
        }
        _ => {}
    }
}

fn collect_pattern_names(pattern: &Pattern, map: &mut HashMap<String, String>, suffix: &str) {
    match pattern {
        Pattern::Var(name) => {
            if name != "_" {
                map.entry(name.clone())
                    .or_insert_with(|| format!("{}{}", name, suffix));
            }
        }
        Pattern::Constructor { args, .. } => {
            for arg in args {
                collect_pattern_names(&arg.node, map, suffix);
            }
        }
        Pattern::ConstructorNamed { fields, .. } => {
            for f in fields {
                if let Some(p) = &f.pattern {
                    collect_pattern_names(&p.node, map, suffix);
                } else {
                    map.entry(f.field_name.clone())
                        .or_insert_with(|| format!("{}{}", f.field_name, suffix));
                }
            }
        }
        Pattern::Tuple(elems) | Pattern::List(elems) => {
            for e in elems {
                collect_pattern_names(&e.node, map, suffix);
            }
        }
        Pattern::ListCons { head, tail } => {
            collect_pattern_names(&head.node, map, suffix);
            collect_pattern_names(&tail.node, map, suffix);
        }
        Pattern::As { pattern, name } => {
            collect_pattern_names(&pattern.node, map, suffix);
            map.entry(name.clone())
                .or_insert_with(|| format!("{}{}", name, suffix));
        }
        Pattern::Or(alts) => {
            for a in alts {
                collect_pattern_names(&a.node, map, suffix);
            }
        }
        Pattern::Discard
        | Pattern::Int(_)
        | Pattern::String(_)
        | Pattern::Bool(_)
        | Pattern::Rawcode(_) => {}
    }
}

fn rename_expr(expr: Spanned<Expr>, map: &HashMap<String, String>) -> Spanned<Expr> {
    let span = expr.span;
    match expr.node {
        Expr::Var(name) => Spanned {
            node: Expr::Var(map.get(&name).cloned().unwrap_or(name)),
            span,
        },
        Expr::Let {
            pattern,
            type_annotation,
            value,
            body,
        } => Spanned {
            node: Expr::Let {
                pattern: rename_pattern(pattern, map),
                type_annotation,
                value: Box::new(rename_expr(*value, map)),
                body: Box::new(rename_expr(*body, map)),
            },
            span,
        },
        Expr::Case { subject, arms } => Spanned {
            node: Expr::Case {
                subject: Box::new(rename_expr(*subject, map)),
                arms: arms
                    .into_iter()
                    .map(|arm| CaseArm {
                        pattern: rename_pattern(arm.pattern, map),
                        guard: arm.guard.map(|g| rename_expr(g, map)),
                        body: rename_expr(arm.body, map),
                        span: arm.span,
                    })
                    .collect(),
            },
            span,
        },
        Expr::Call { function, args } => Spanned {
            node: Expr::Call {
                function: Box::new(rename_expr(*function, map)),
                args: args.into_iter().map(|a| rename_expr(a, map)).collect(),
            },
            span,
        },
        Expr::BinOp { op, left, right } => Spanned {
            node: Expr::BinOp {
                op,
                left: Box::new(rename_expr(*left, map)),
                right: Box::new(rename_expr(*right, map)),
            },
            span,
        },
        Expr::UnaryOp { op, operand } => Spanned {
            node: Expr::UnaryOp {
                op,
                operand: Box::new(rename_expr(*operand, map)),
            },
            span,
        },
        Expr::Pipe { left, right } => Spanned {
            node: Expr::Pipe {
                left: Box::new(rename_expr(*left, map)),
                right: Box::new(rename_expr(*right, map)),
            },
            span,
        },
        Expr::Block(exprs) => Spanned {
            node: Expr::Block(exprs.into_iter().map(|e| rename_expr(e, map)).collect()),
            span,
        },
        Expr::Tuple(elems) => Spanned {
            node: Expr::Tuple(elems.into_iter().map(|e| rename_expr(e, map)).collect()),
            span,
        },
        Expr::List(elems) => Spanned {
            node: Expr::List(elems.into_iter().map(|e| rename_expr(e, map)).collect()),
            span,
        },
        Expr::ListCons { head, tail } => Spanned {
            node: Expr::ListCons {
                head: Box::new(rename_expr(*head, map)),
                tail: Box::new(rename_expr(*tail, map)),
            },
            span,
        },
        Expr::FieldAccess { object, field } => Spanned {
            node: Expr::FieldAccess {
                object: Box::new(rename_expr(*object, map)),
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
                object: Box::new(rename_expr(*object, map)),
                method,
                args: args.into_iter().map(|a| rename_expr(a, map)).collect(),
            },
            span,
        },
        Expr::Constructor { name, args } => Spanned {
            node: Expr::Constructor {
                name,
                args: args
                    .into_iter()
                    .map(|a| match a {
                        ConstructorArg::Positional(e) => {
                            ConstructorArg::Positional(rename_expr(e, map))
                        }
                        ConstructorArg::Named(n, e) => {
                            ConstructorArg::Named(n, rename_expr(e, map))
                        }
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
                base: Box::new(rename_expr(*base, map)),
                updates: updates
                    .into_iter()
                    .map(|(n, e)| (n, rename_expr(e, map)))
                    .collect(),
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
                body: Box::new(rename_expr(*body, map)),
            },
            span,
        },
        Expr::Clone(inner) => Spanned {
            node: Expr::Clone(Box::new(rename_expr(*inner, map))),
            span,
        },
        Expr::TcoLoop { body } => Spanned {
            node: Expr::TcoLoop {
                body: Box::new(rename_expr(*body, map)),
            },
            span,
        },
        Expr::TcoContinue { args } => Spanned {
            node: Expr::TcoContinue {
                args: args
                    .into_iter()
                    .map(|(name, e)| {
                        (
                            map.get(&name).cloned().unwrap_or(name),
                            Box::new(rename_expr(*e, map)),
                        )
                    })
                    .collect(),
            },
            span,
        },
        other => Spanned { node: other, span },
    }
}

fn rename_pattern(pattern: Spanned<Pattern>, map: &HashMap<String, String>) -> Spanned<Pattern> {
    let span = pattern.span;
    match pattern.node {
        Pattern::Var(name) => Spanned {
            node: Pattern::Var(map.get(&name).cloned().unwrap_or(name)),
            span,
        },
        Pattern::Constructor { name, args } => Spanned {
            node: Pattern::Constructor {
                name,
                args: args.into_iter().map(|a| rename_pattern(a, map)).collect(),
            },
            span,
        },
        Pattern::ConstructorNamed { name, fields, rest } => Spanned {
            node: Pattern::ConstructorNamed {
                name,
                fields: fields
                    .into_iter()
                    .map(|f| {
                        let FieldPattern {
                            field_name,
                            pattern,
                        } = f;
                        let pattern = pattern.map(|p| rename_pattern(p, map)).or_else(|| {
                            map.get(&field_name).map(|new_name| Spanned {
                                node: Pattern::Var(new_name.clone()),
                                span,
                            })
                        });
                        FieldPattern {
                            field_name,
                            pattern,
                        }
                    })
                    .collect(),
                rest,
            },
            span,
        },
        Pattern::Tuple(elems) => Spanned {
            node: Pattern::Tuple(elems.into_iter().map(|e| rename_pattern(e, map)).collect()),
            span,
        },
        Pattern::List(elems) => Spanned {
            node: Pattern::List(elems.into_iter().map(|e| rename_pattern(e, map)).collect()),
            span,
        },
        Pattern::ListCons { head, tail } => Spanned {
            node: Pattern::ListCons {
                head: Box::new(rename_pattern(*head, map)),
                tail: Box::new(rename_pattern(*tail, map)),
            },
            span,
        },
        Pattern::As { pattern, name } => Spanned {
            node: Pattern::As {
                pattern: Box::new(rename_pattern(*pattern, map)),
                name: map.get(&name).cloned().unwrap_or(name),
            },
            span,
        },
        Pattern::Or(alts) => Spanned {
            node: Pattern::Or(alts.into_iter().map(|a| rename_pattern(a, map)).collect()),
            span,
        },
        other => Spanned { node: other, span },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use crate::token::Lexer;

    fn parse_and_inline(source: &str) -> Module {
        let tokens = Lexer::tokenize(source).expect("lex failed");
        let mut parser = Parser::new(tokens);
        let mut module = {
            let _o = parser.parse_module();
            assert!(_o.errors.is_empty(), "parse errors: {:?}", _o.errors);
            _o.module
        };
        apply_inlining(&mut module);
        module
    }

    fn calls_in_fn(module: &Module, fn_name: &str) -> Vec<String> {
        let f = module.definitions.iter().find_map(|d| {
            if let Definition::Function(f) = d {
                if f.name == fn_name {
                    return Some(f);
                }
            }
            None
        });
        let mut calls = Vec::new();
        if let Some(f) = f {
            collect_calls(&f.body.node, &mut calls);
        }
        calls
    }

    #[test]
    fn inline_single_use_fn() {
        let module = parse_and_inline(
            r#"
fn helper(x: Int) -> Int { x + 1 }
fn main() -> Int { helper(5) }
"#,
        );
        let calls = calls_in_fn(&module, "main");
        assert!(!calls.contains(&"helper".to_string()));
    }

    #[test]
    fn inline_small_multi_use() {
        let module = parse_and_inline(
            r#"
fn add1(x: Int) -> Int { x + 1 }
fn main() -> Int { add1(add1(5)) }
"#,
        );
        let calls = calls_in_fn(&module, "main");
        assert!(!calls.contains(&"add1".to_string()));
    }

    #[test]
    fn no_inline_recursive() {
        let module = parse_and_inline(
            r#"
fn fact(n: Int) -> Int {
    case n == 0 {
        True -> 1
        False -> n * fact(n - 1)
    }
}
fn main() -> Int { fact(5) }
"#,
        );
        let calls = calls_in_fn(&module, "main");
        assert!(calls.contains(&"fact".to_string()));
    }

    #[test]
    fn cost_trivial() {
        assert_eq!(expr_cost(&Expr::Var("x".to_string())), 0);
        assert_eq!(expr_cost(&Expr::Int(42)), 0);
    }
}
