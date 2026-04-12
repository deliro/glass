use std::collections::{HashMap, HashSet};

use crate::ast::*;

/// Dead code elimination: keep only definitions reachable from entry points.
/// `imported_count` is the number of leading imported definitions.
/// Only user (non-imported) pub functions are entry points.
pub(crate) fn dead_code_eliminate(defs: &[Definition], imported_count: usize) -> Vec<Definition> {
    // Build fn name → index
    let mut fn_indices: HashMap<&str, usize> = HashMap::new();
    for (i, def) in defs.iter().enumerate() {
        match def {
            Definition::Function(f) => {
                fn_indices.insert(&f.name, i);
            }
            Definition::External(e) => {
                fn_indices.insert(&e.fn_name, i);
            }
            _ => {}
        }
    }

    // Build call graph
    let mut calls: HashMap<usize, HashSet<usize>> = HashMap::new();
    for (i, def) in defs.iter().enumerate() {
        if let Definition::Function(f) = def {
            let mut called = HashSet::new();
            collect_calls_in_expr(&f.body, &fn_indices, &mut called);
            calls.insert(i, called);
        }
    }

    // Check if there are any pub functions or Elm entry points
    let has_entry_points = defs.iter().any(|d| matches!(d,
        Definition::Function(f) if f.is_pub || matches!(f.name.as_str(), "init" | "update" | "subscriptions")
    ));

    // If no entry points, keep everything (e.g., test files, single-file scripts)
    if !has_entry_points {
        return defs.to_vec();
    }

    // Seed: entry points
    let mut reachable: HashSet<usize> = HashSet::new();
    for (i, def) in defs.iter().enumerate() {
        let is_entry = match def {
            // All type defs, imports, extends, consts are always kept
            Definition::Type(_)
            | Definition::Import(_)
            | Definition::Extend(_)
            | Definition::Const(_)
            | Definition::External(_) => true,
            // Only USER pub functions and Elm entry points are roots.
            // Imported pub functions are NOT entry points — only kept if reachable.
            Definition::Function(f) => {
                let is_user_def = i >= imported_count;
                (is_user_def && f.is_pub)
                    || matches!(f.name.as_str(), "init" | "update" | "subscriptions")
            }
        };
        if is_entry {
            reachable.insert(i);
        }
    }

    // BFS from entry points
    let mut queue: Vec<usize> = reachable.iter().copied().collect();
    while let Some(idx) = queue.pop() {
        if let Some(callees) = calls.get(&idx) {
            for &callee in callees {
                if reachable.insert(callee) {
                    queue.push(callee);
                }
            }
        }
    }

    // Filter
    defs.iter()
        .enumerate()
        .filter(|(i, _)| reachable.contains(i))
        .map(|(_, d)| d.clone())
        .collect()
}

/// Topological sort of definitions so callees appear before callers in JASS output.
pub(crate) fn topo_sort_definitions(defs: &[Definition]) -> Vec<&Definition> {
    // Build name → index mapping for functions
    let mut fn_indices: HashMap<&str, usize> = HashMap::new();
    for (i, def) in defs.iter().enumerate() {
        match def {
            Definition::Function(f) => {
                fn_indices.insert(&f.name, i);
            }
            Definition::External(e) => {
                fn_indices.insert(&e.fn_name, i);
            }
            _ => {}
        }
    }

    // Build adjacency: fn_index → set of fn_indices it calls
    let mut deps: HashMap<usize, HashSet<usize>> = HashMap::new();
    for (i, def) in defs.iter().enumerate() {
        if let Definition::Function(f) = def {
            let mut called = HashSet::new();
            collect_calls_in_expr(&f.body, &fn_indices, &mut called);
            called.remove(&i);
            deps.insert(i, called);
        }
    }

    // Kahn's algorithm: caller depends on callee. in_deg[caller] = number of callees.
    let n = defs.len();
    let mut in_deg2 = vec![0usize; n];
    for (&caller, callees) in &deps {
        if let Some(slot) = in_deg2.get_mut(caller) {
            *slot = callees.len();
        }
    }

    let mut queue: Vec<usize> = (0..n)
        .filter(|&i| in_deg2.get(i).copied() == Some(0))
        .collect();
    let mut sorted: Vec<usize> = Vec::new();

    // Build reverse map: callee → callers (who depend on callee)
    let mut callers_of: HashMap<usize, Vec<usize>> = HashMap::new();
    for (&caller, callees) in &deps {
        for &callee in callees {
            callers_of.entry(callee).or_default().push(caller);
        }
    }

    while let Some(node) = queue.pop() {
        sorted.push(node);
        if let Some(callers) = callers_of.get(&node) {
            for &caller in callers {
                if let Some(deg) = in_deg2.get_mut(caller) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push(caller);
                    }
                }
            }
        }
    }

    // Add any remaining (cycles) in original order
    let in_sorted: HashSet<usize> = sorted.iter().copied().collect();
    for i in 0..n {
        if !in_sorted.contains(&i) {
            sorted.push(i);
        }
    }

    sorted.iter().filter_map(|&i| defs.get(i)).collect()
}

/// Collect all function names called in an expression.
fn collect_calls_in_expr(
    expr: &Spanned<Expr>,
    fn_map: &HashMap<&str, usize>,
    out: &mut HashSet<usize>,
) {
    match &expr.node {
        Expr::Call { function, args } => {
            if let Expr::Var(name) = &function.node
                && let Some(&idx) = fn_map.get(name.as_str())
            {
                out.insert(idx);
            }
            collect_calls_in_expr(function, fn_map, out);
            for a in args {
                collect_calls_in_expr(a, fn_map, out);
            }
        }
        Expr::Var(name) => {
            if let Some(&idx) = fn_map.get(name.as_str()) {
                out.insert(idx);
            }
        }
        Expr::Let { value, body, .. } => {
            collect_calls_in_expr(value, fn_map, out);
            collect_calls_in_expr(body, fn_map, out);
        }
        Expr::Case { subject, arms } => {
            collect_calls_in_expr(subject, fn_map, out);
            for arm in arms {
                collect_calls_in_expr(&arm.body, fn_map, out);
                if let Some(g) = &arm.guard {
                    collect_calls_in_expr(g, fn_map, out);
                }
            }
        }
        Expr::BinOp { left, right, .. } | Expr::Pipe { left, right } => {
            collect_calls_in_expr(left, fn_map, out);
            collect_calls_in_expr(right, fn_map, out);
        }
        Expr::UnaryOp { operand, .. } | Expr::Clone(operand) => {
            collect_calls_in_expr(operand, fn_map, out);
        }
        Expr::Block(exprs) => {
            for e in exprs {
                collect_calls_in_expr(e, fn_map, out);
            }
        }
        Expr::Tuple(elems) | Expr::List(elems) => {
            for e in elems {
                collect_calls_in_expr(e, fn_map, out);
            }
        }
        Expr::ListCons { head, tail } => {
            collect_calls_in_expr(head, fn_map, out);
            collect_calls_in_expr(tail, fn_map, out);
        }
        Expr::Lambda { body, .. } => collect_calls_in_expr(body, fn_map, out),
        Expr::MethodCall {
            object,
            method,
            args,
        } => {
            // Register method name as a function call (for qualified module.func calls)
            if let Some(&idx) = fn_map.get(method.as_str()) {
                out.insert(idx);
            }
            collect_calls_in_expr(object, fn_map, out);
            for a in args {
                collect_calls_in_expr(a, fn_map, out);
            }
        }
        Expr::FieldAccess { object, field } => {
            if let Some(&idx) = fn_map.get(field.as_str()) {
                out.insert(idx);
            }
            collect_calls_in_expr(object, fn_map, out);
        }
        Expr::Constructor { args, .. } => {
            for a in args {
                let e = match a {
                    ConstructorArg::Positional(e) | ConstructorArg::Named(_, e) => e,
                };
                collect_calls_in_expr(e, fn_map, out);
            }
        }
        Expr::RecordUpdate { base, updates, .. } => {
            collect_calls_in_expr(base, fn_map, out);
            for (_, e) in updates {
                collect_calls_in_expr(e, fn_map, out);
            }
        }
        Expr::TcoLoop { body } => {
            collect_calls_in_expr(body, fn_map, out);
        }
        Expr::TcoContinue { args } => {
            for (_, val) in args {
                collect_calls_in_expr(val, fn_map, out);
            }
        }
        _ => {}
    }
}
