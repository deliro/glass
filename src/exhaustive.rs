// Exhaustiveness checking for case expressions.
//
// Warns when a case expression doesn't cover all variants of a type.
// Not an error — user can add `_ -> ...` to silence.

#![allow(dead_code)]

use std::collections::HashSet;

use crate::ast::*;
use crate::token::Span;
use crate::type_env::ConstructorRegistry;

#[derive(Debug)]
pub struct ExhaustivenessWarning {
    pub message: String,
    pub span: Span,
}

/// Check all case expressions in a module for exhaustiveness.
/// `skip_imported` is the number of leading definitions that came from imports
/// and should not be checked (their spans don't correspond to the user source).
pub fn check_exhaustiveness(
    module: &Module,
    constructors: &ConstructorRegistry,
    skip_imported: usize,
) -> Vec<ExhaustivenessWarning> {
    let mut warnings = Vec::new();
    for def in module.definitions.iter().skip(skip_imported) {
        if let Definition::Function(f) = def {
            check_expr(&f.body, constructors, &mut warnings);
        }
    }
    warnings
}

fn check_expr(
    expr: &Spanned<Expr>,
    constructors: &ConstructorRegistry,
    warnings: &mut Vec<ExhaustivenessWarning>,
) {
    match &expr.node {
        Expr::Case { subject, arms } => {
            // Recurse into sub-expressions first
            check_expr(subject, constructors, warnings);
            for arm in arms {
                check_expr(&arm.body, constructors, warnings);
            }

            // Check exhaustiveness of this case
            check_case_arms(arms, constructors, expr.span, warnings);
        }
        Expr::Let { value, body, .. } => {
            check_expr(value, constructors, warnings);
            check_expr(body, constructors, warnings);
        }
        Expr::BinOp { left, right, .. } | Expr::Pipe { left, right } => {
            check_expr(left, constructors, warnings);
            check_expr(right, constructors, warnings);
        }
        Expr::UnaryOp { operand, .. } | Expr::Clone(operand) => {
            check_expr(operand, constructors, warnings);
        }
        Expr::Call { function, args } => {
            check_expr(function, constructors, warnings);
            for a in args {
                check_expr(a, constructors, warnings);
            }
        }
        Expr::MethodCall { object, args, .. } => {
            check_expr(object, constructors, warnings);
            for a in args {
                check_expr(a, constructors, warnings);
            }
        }
        Expr::Block(exprs) => {
            for e in exprs {
                check_expr(e, constructors, warnings);
            }
        }
        Expr::Lambda { body, .. } => check_expr(body, constructors, warnings),
        Expr::Tuple(elems) | Expr::List(elems) => {
            for e in elems {
                check_expr(e, constructors, warnings);
            }
        }
        Expr::Constructor { args, .. } => {
            for a in args {
                let e = match a {
                    ConstructorArg::Positional(e) | ConstructorArg::Named(_, e) => e,
                };
                check_expr(e, constructors, warnings);
            }
        }
        Expr::RecordUpdate { base, updates, .. } => {
            check_expr(base, constructors, warnings);
            for (_, e) in updates {
                check_expr(e, constructors, warnings);
            }
        }
        Expr::FieldAccess { object, .. } => check_expr(object, constructors, warnings),
        _ => {}
    }
}

fn check_case_arms(
    arms: &[CaseArm],
    constructors: &ConstructorRegistry,
    case_span: Span,
    warnings: &mut Vec<ExhaustivenessWarning>,
) {
    // If any arm has a wildcard or variable pattern, it's exhaustive
    if arms
        .iter()
        .any(|arm| is_catch_all(&arm.pattern.node) && arm.guard.is_none())
    {
        return;
    }

    // Collect constructor names from patterns
    let mut covered_constructors: HashSet<String> = HashSet::new();
    let mut has_bool_true = false;
    let mut has_bool_false = false;

    for arm in arms {
        collect_covered(
            &arm.pattern.node,
            &mut covered_constructors,
            &mut has_bool_true,
            &mut has_bool_false,
        );
    }

    // Check Bool exhaustiveness
    if has_bool_true || has_bool_false {
        if !has_bool_true {
            warnings.push(ExhaustivenessWarning {
                message: "non-exhaustive case: missing True".into(),
                span: case_span,
            });
        }
        if !has_bool_false {
            warnings.push(ExhaustivenessWarning {
                message: "non-exhaustive case: missing False".into(),
                span: case_span,
            });
        }
        return;
    }

    // Check constructor exhaustiveness
    if covered_constructors.is_empty() {
        return; // No constructor patterns — might be matching on literals, skip
    }

    // Find which type these constructors belong to
    let type_name = covered_constructors.iter().find_map(|ctor_name| {
        constructors
            .lookup(ctor_name)
            .map(|info| info.type_name.clone())
    });

    let Some(type_name) = type_name else { return };

    // Get all constructors of this type (bare names only, no qualified)
    let all_ctors: HashSet<String> = constructors
        .constructors
        .iter()
        .filter(|(name, info)| info.type_name == type_name && !name.contains("::"))
        .map(|(name, _)| name.clone())
        .collect();

    let missing: Vec<&String> = all_ctors.difference(&covered_constructors).collect();

    if !missing.is_empty() {
        let mut missing_sorted: Vec<&str> = missing.iter().map(|s| s.as_str()).collect();
        missing_sorted.sort();
        warnings.push(ExhaustivenessWarning {
            message: format!("non-exhaustive case: missing {}", missing_sorted.join(", ")),
            span: case_span,
        });
    }
}

/// Check if a pattern catches everything (variable or wildcard).
fn is_catch_all(pat: &Pattern) -> bool {
    matches!(pat, Pattern::Var(_) | Pattern::Discard)
}

/// Collect which constructors and booleans a pattern covers.
fn collect_covered(
    pat: &Pattern,
    constructors: &mut HashSet<String>,
    has_true: &mut bool,
    has_false: &mut bool,
) {
    match pat {
        Pattern::Constructor { name, .. } | Pattern::ConstructorNamed { name, .. } => {
            // Strip qualified prefix: "BashResult::Bashed" → "Bashed"
            let bare = name.rsplit("::").next().unwrap_or(name);
            constructors.insert(bare.to_string());
        }
        Pattern::Bool(true) => *has_true = true,
        Pattern::Bool(false) => *has_false = true,
        Pattern::As { pattern, .. } => {
            collect_covered(&pattern.node, constructors, has_true, has_false);
        }
        Pattern::Or(alternatives) => {
            for alt in alternatives {
                collect_covered(&alt.node, constructors, has_true, has_false);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infer::Inferencer;
    use crate::parser::Parser;
    use crate::token::Lexer;

    fn check(source: &str) -> Vec<String> {
        let tokens = Lexer::tokenize(source);
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module().expect("parse failed");

        // Run inference first to populate constructor registry
        let mut inferencer = Inferencer::new();
        inferencer.infer_module(&module);

        check_exhaustiveness(&module, &inferencer.constructors, 0)
            .iter()
            .map(|w| w.message.clone())
            .collect()
    }

    #[test]
    fn exhaustive_bool() {
        let w = check(
            r#"
fn test(x: Bool) -> Int {
    case x {
        True -> 1
        False -> 0
    }
}
"#,
        );
        assert!(w.is_empty());
    }

    #[test]
    fn missing_false() {
        let w = check(
            r#"
fn test(x: Bool) -> Int {
    case x {
        True -> 1
    }
}
"#,
        );
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("missing False"));
    }

    #[test]
    fn missing_true() {
        let w = check(
            r#"
fn test(x: Bool) -> Int {
    case x {
        False -> 0
    }
}
"#,
        );
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("missing True"));
    }

    #[test]
    fn exhaustive_enum() {
        let w = check(
            r#"
pub enum Color { Red Green Blue }
fn test(c: Color) -> Int {
    case c {
        Color::Red -> 1
        Color::Green -> 2
        Color::Blue -> 3
    }
}
"#,
        );
        assert!(w.is_empty());
    }

    #[test]
    fn missing_enum_variant() {
        let w = check(
            r#"
pub enum Color { Red Green Blue }
fn test(c: Color) -> Int {
    case c {
        Color::Red -> 1
        Color::Green -> 2
    }
}
"#,
        );
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("Blue"));
    }

    #[test]
    fn missing_multiple_variants() {
        let w = check(
            r#"
pub enum Phase { Lobby Playing Victory Draw }
fn test(p: Phase) -> Int {
    case p {
        Phase::Lobby -> 0
    }
}
"#,
        );
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("Draw"));
        assert!(w[0].contains("Playing"));
        assert!(w[0].contains("Victory"));
    }

    #[test]
    fn wildcard_makes_exhaustive() {
        let w = check(
            r#"
pub enum Color { Red Green Blue }
fn test(c: Color) -> Int {
    case c {
        Color::Red -> 1
        _ -> 0
    }
}
"#,
        );
        assert!(w.is_empty());
    }

    #[test]
    fn variable_makes_exhaustive() {
        let w = check(
            r#"
pub enum Color { Red Green Blue }
fn test(c: Color) -> Int {
    case c {
        Color::Red -> 1
        other -> 0
    }
}
"#,
        );
        assert!(w.is_empty());
    }

    #[test]
    fn guard_doesnt_count_as_exhaustive() {
        // A wildcard with a guard is NOT catch-all
        let w = check(
            r#"
pub enum Color { Red Green Blue }
fn test(c: Color) -> Int {
    case c {
        Color::Red -> 1
        x if True -> 0
    }
}
"#,
        );
        // The guarded wildcard is not exhaustive
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("Blue"));
        assert!(w[0].contains("Green"));
    }

    #[test]
    fn nested_case_checked() {
        let w = check(
            r#"
pub enum AB { A B }
fn test(x: AB, y: AB) -> Int {
    case x {
        AB::A -> case y {
            AB::A -> 1
        }
        AB::B -> 0
    }
}
"#,
        );
        // Inner case missing B
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("B"));
    }

    #[test]
    fn or_pattern_covers_union() {
        let w = check(
            r#"
pub enum Event { Chat { from: Int } Quit { player: Int } GameStarted }
fn test(e: Event) -> Int {
    case e {
        Event::Chat(p) | Event::Quit(p) -> p
        Event::GameStarted -> 0
    }
}
"#,
        );
        assert!(w.is_empty());
    }

    #[test]
    fn or_pattern_still_missing() {
        let w = check(
            r#"
pub enum Event { Chat { from: Int } Quit { player: Int } GameStarted }
fn test(e: Event) -> Int {
    case e {
        Event::Chat(p) | Event::Quit(p) -> p
    }
}
"#,
        );
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("GameStarted"));
    }

    #[test]
    fn named_field_pattern_exhaustive() {
        let w = check(
            r#"
pub enum Event { Chat { from: Int, text: String } Quit { player: Int } }
fn test(e: Event) -> Int {
    case e {
        Event::Chat { from, .. } -> from
        Event::Quit(p) -> p
    }
}
"#,
        );
        assert!(w.is_empty());
    }

    #[test]
    fn no_warning_for_int_patterns() {
        // Int patterns can't be exhaustive — no warning expected
        let w = check(
            r#"
fn test(x: Int) -> Int {
    case x {
        0 -> 1
        1 -> 2
    }
}
"#,
        );
        assert!(w.is_empty());
    }
}
