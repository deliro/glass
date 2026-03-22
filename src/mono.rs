// Monomorphization pass for Glass.
//
// Collects all concrete instantiations of generic types (List(Int), Option(Unit), etc.)
// from the inferred types, and produces a mapping for codegen.

#![allow(dead_code)]

use std::collections::BTreeSet;

use crate::ast::*;
use crate::infer::Inferencer;
use crate::type_repr::{Substitution, Type};

/// A concrete instantiation of a generic type.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MonoType {
    /// The generic type name (e.g., "List", "Option")
    pub base: String,
    /// Concrete type arguments (e.g., [Int], [Unit])
    pub args: Vec<Type>,
    /// Generated JASS name (e.g., "List_integer", "Option_unit")
    pub jass_name: String,
}

/// Collect all monomorphized type instances from an inferred module.
pub fn collect_mono_types(module: &Module, inferencer: &Inferencer) -> BTreeSet<MonoType> {
    let mut mono_types = BTreeSet::new();

    // Collect from ALL types in the substitution (covers all inferred types)
    for ty in inferencer.subst.values() {
        let resolved = ty.apply(&inferencer.subst);
        collect_from_type(&resolved, &inferencer.subst, &mut mono_types);
    }

    // Collect from inferred types at call sites
    for ty in &inferencer.inferred_types {
        let resolved = ty.apply(&inferencer.subst);
        collect_from_type(&resolved, &inferencer.subst, &mut mono_types);
    }

    // Collect from AST annotations (explicit type annotations with concrete types)
    collect_from_ast_types(module, &inferencer.subst, &mut mono_types);

    mono_types
}

/// Scan all type expressions in the AST (annotations, params, returns) for App types.
fn collect_from_ast_types(module: &Module, subst: &Substitution, mono: &mut BTreeSet<MonoType>) {
    for def in &module.definitions {
        match def {
            Definition::Function(f) => {
                for p in &f.params {
                    let ty = Inferencer::resolve_type_expr_static(&p.type_expr);
                    collect_from_type(&ty, subst, mono);
                }
                if let Some(ret) = &f.return_type {
                    let ty = Inferencer::resolve_type_expr_static(ret);
                    collect_from_type(&ty, subst, mono);
                }
                collect_type_exprs_in_expr(&f.body, subst, mono);
            }
            Definition::Const(c) => {
                if let Some(te) = &c.type_expr {
                    let ty = Inferencer::resolve_type_expr_static(te);
                    collect_from_type(&ty, subst, mono);
                }
            }
            _ => {}
        }
    }
}

/// Walk expressions looking for type annotations (let bindings, lambdas).
fn collect_type_exprs_in_expr(
    expr: &Spanned<Expr>,
    subst: &Substitution,
    mono: &mut BTreeSet<MonoType>,
) {
    match &expr.node {
        Expr::Let {
            type_annotation,
            value,
            body,
            ..
        } => {
            if let Some(te) = type_annotation {
                let ty = Inferencer::resolve_type_expr_static(te);
                collect_from_type(&ty, subst, mono);
            }
            collect_type_exprs_in_expr(value, subst, mono);
            collect_type_exprs_in_expr(body, subst, mono);
        }
        Expr::Lambda {
            params,
            return_type,
            body,
        } => {
            for p in params {
                let ty = Inferencer::resolve_type_expr_static(&p.type_expr);
                collect_from_type(&ty, subst, mono);
            }
            if let Some(ret) = return_type {
                let ty = Inferencer::resolve_type_expr_static(ret);
                collect_from_type(&ty, subst, mono);
            }
            collect_type_exprs_in_expr(body, subst, mono);
        }
        Expr::Case { subject, arms } => {
            collect_type_exprs_in_expr(subject, subst, mono);
            for arm in arms {
                collect_type_exprs_in_expr(&arm.body, subst, mono);
            }
        }
        Expr::BinOp { left, right, .. } | Expr::Pipe { left, right } => {
            collect_type_exprs_in_expr(left, subst, mono);
            collect_type_exprs_in_expr(right, subst, mono);
        }
        Expr::UnaryOp { operand, .. } | Expr::Clone(operand) => {
            collect_type_exprs_in_expr(operand, subst, mono);
        }
        Expr::Call { function, args } => {
            collect_type_exprs_in_expr(function, subst, mono);
            for a in args {
                collect_type_exprs_in_expr(a, subst, mono);
            }
        }
        Expr::MethodCall { object, args, .. } => {
            collect_type_exprs_in_expr(object, subst, mono);
            for a in args {
                collect_type_exprs_in_expr(a, subst, mono);
            }
        }
        Expr::Block(exprs) => {
            for e in exprs {
                collect_type_exprs_in_expr(e, subst, mono);
            }
        }
        Expr::Tuple(elems) | Expr::List(elems) => {
            for e in elems {
                collect_type_exprs_in_expr(e, subst, mono);
            }
        }
        Expr::Constructor { args, .. } => {
            for a in args {
                let e = match a {
                    ConstructorArg::Positional(e) | ConstructorArg::Named(_, e) => e,
                };
                collect_type_exprs_in_expr(e, subst, mono);
            }
        }
        Expr::RecordUpdate { base, updates, .. } => {
            collect_type_exprs_in_expr(base, subst, mono);
            for (_, e) in updates {
                collect_type_exprs_in_expr(e, subst, mono);
            }
        }
        Expr::FieldAccess { object, .. } => collect_type_exprs_in_expr(object, subst, mono),
        _ => {}
    }
}

/// Collect MonoType instances from a resolved type.
fn collect_from_type(ty: &Type, subst: &Substitution, mono: &mut BTreeSet<MonoType>) {
    let resolved = ty.apply(subst);
    match &resolved {
        Type::App(base, args) => {
            let all_concrete = args.iter().all(|a| a.apply(subst).free_vars().is_empty());
            if all_concrete && let Type::Con(name) = base.as_ref() {
                let concrete_args: Vec<Type> = args.iter().map(|a| a.apply(subst)).collect();
                let jass_name = make_jass_name(name, &concrete_args);
                mono.insert(MonoType {
                    base: name.clone(),
                    args: concrete_args.clone(),
                    jass_name,
                });
                for a in &concrete_args {
                    collect_from_type(a, subst, mono);
                }
            }
        }
        Type::Fn(params, ret) => {
            for p in params {
                collect_from_type(p, subst, mono);
            }
            collect_from_type(ret, subst, mono);
        }
        Type::Tuple(elems) => {
            for e in elems {
                collect_from_type(e, subst, mono);
            }
        }
        _ => {}
    }
}

/// Generate a JASS-compatible name for a monomorphized type.
fn make_jass_name(base: &str, args: &[Type]) -> String {
    let arg_parts: Vec<String> = args.iter().map(type_to_jass_suffix).collect();
    format!("{}_{}", base, arg_parts.join("_"))
}

fn type_to_jass_suffix(ty: &Type) -> String {
    match ty {
        Type::Con(name) => match name.as_str() {
            "Int" => "integer".into(),
            "Float" => "real".into(),
            "Bool" => "boolean".into(),
            "String" => "string".into(),
            other => other.to_lowercase(),
        },
        Type::App(base, args) => {
            let base_name = type_to_jass_suffix(base);
            let arg_names: Vec<String> = args.iter().map(type_to_jass_suffix).collect();
            format!("{}_{}", base_name, arg_names.join("_"))
        }
        Type::Tuple(elems) => {
            let parts: Vec<String> = elems.iter().map(type_to_jass_suffix).collect();
            format!("tuple_{}", parts.join("_"))
        }
        _ => "unknown".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use crate::token::Lexer;

    fn collect(source: &str) -> BTreeSet<MonoType> {
        let tokens = Lexer::tokenize(source);
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module().expect("parse failed");
        let mut inferencer = Inferencer::new();
        inferencer.infer_module(&module);
        collect_mono_types(&module, &inferencer)
    }

    fn mono_names(source: &str) -> Vec<String> {
        let mut names: Vec<String> = collect(source)
            .iter()
            .map(|m| m.jass_name.clone())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn list_of_int() {
        let names = mono_names("fn test() -> List(Int) { [1, 2, 3] }");
        assert!(names.contains(&"List_integer".to_string()));
    }

    #[test]
    fn list_of_string() {
        let names = mono_names(r#"fn test() -> List(String) { ["a", "b"] }"#);
        assert!(names.contains(&"List_string".to_string()));
    }

    #[test]
    fn multiple_list_types() {
        let names = mono_names(
            r#"
fn ints() -> List(Int) { [1] }
fn strs() -> List(String) { ["a"] }
"#,
        );
        assert!(names.contains(&"List_integer".to_string()));
        assert!(names.contains(&"List_string".to_string()));
    }

    #[test]
    fn no_generics() {
        let names = mono_names("fn test() -> Int { 42 }");
        assert!(names.is_empty());
    }

    #[test]
    fn tuple_in_return() {
        let names = mono_names("fn test() -> (Int, String) { (1, \"hello\") }");
        // Tuples are handled separately, not through App
        assert!(names.is_empty());
    }

    #[test]
    fn nested_generic() {
        let names = mono_names("fn test() -> List(List(Int)) { [[1]] }");
        // Should have both List(Int) and List(List(Int))
        assert!(names.iter().any(|n| n.contains("List_integer")));
    }

    #[test]
    fn snapshot_mono_types() {
        let types = collect(
            r#"
fn ints() -> List(Int) { [1, 2] }
fn strs() -> List(String) { ["a"] }
fn pair() -> (Int, String) { (1, "x") }
"#,
        );
        insta::assert_debug_snapshot!(types);
    }

    #[test]
    fn generic_adt_option() {
        let names = mono_names(
            r#"
enum Option(T) {
    Some(T)
    None
}
fn test_int() -> Option(Int) { Option::Some(42) }
fn test_str() -> Option(String) { Option::Some("x") }
"#,
        );
        assert!(names.contains(&"Option_integer".to_string()));
        assert!(names.contains(&"Option_string".to_string()));
    }

    #[test]
    fn generic_adt_result() {
        let names = mono_names(
            r#"
enum Result(T, E) {
    Ok(T)
    Err(E)
}
fn test() -> Result(Int, String) { Result::Ok(42) }
"#,
        );
        assert!(names.contains(&"Result_integer_string".to_string()));
    }

    #[test]
    fn generic_function_call_inferred() {
        // unwrap_or is generic (a), called with Int — should discover Option(Int)
        let names = mono_names(
            r#"
enum Option(T) {
    Some(T)
    None
}
fn unwrap_or(opt: Option(a), default: a) -> a {
    case opt {
        Option::Some(val) -> val
        Option::None -> default
    }
}
fn test() -> Int {
    unwrap_or(Option::Some(42), 0)
}
"#,
        );
        assert!(
            names.contains(&"Option_integer".to_string()),
            "got: {:?}",
            names
        );
    }

    #[test]
    fn generic_function_multiple_calls() {
        // Same generic function called with different types
        let names = mono_names(
            r#"
enum Option(T) {
    Some(T)
    None
}
fn unwrap_or(opt: Option(a), default: a) -> a {
    case opt {
        Option::Some(val) -> val
        Option::None -> default
    }
}
fn test_int() -> Int { unwrap_or(Option::Some(42), 0) }
fn test_str() -> String { unwrap_or(Option::Some("hi"), "default") }
"#,
        );
        assert!(
            names.contains(&"Option_integer".to_string()),
            "got: {:?}",
            names
        );
        assert!(
            names.contains(&"Option_string".to_string()),
            "got: {:?}",
            names
        );
    }
}
