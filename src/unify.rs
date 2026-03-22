// Unification algorithm for the Glass type checker.
//
// unify(T1, T2) produces a Substitution that makes T1 == T2,
// or an error if the types are incompatible.

#![allow(dead_code)]

use crate::token::Span;
use crate::type_repr::{Substitution, Type, TypeVarId};

#[derive(Debug, Clone)]
pub struct UnifyError {
    pub message: String,
    pub expected: Type,
    pub actual: Type,
    pub span: Span,
}

impl UnifyError {
    fn mismatch(expected: &Type, actual: &Type, span: Span) -> Self {
        Self {
            message: format!("type mismatch: expected {expected}, got {actual}"),
            expected: expected.clone(),
            actual: actual.clone(),
            span,
        }
    }

    fn occurs(var: TypeVarId, ty: &Type, span: Span) -> Self {
        Self {
            message: format!("infinite type: ?{var} occurs in {ty}"),
            expected: Type::Var(var),
            actual: ty.clone(),
            span,
        }
    }

    fn arity(expected: usize, actual: usize, span: Span) -> Self {
        Self {
            message: format!("type argument count mismatch: expected {expected}, got {actual}"),
            expected: Type::Con(format!("<{expected} args>")),
            actual: Type::Con(format!("<{actual} args>")),
            span,
        }
    }
}

/// Unify two types, producing a substitution or an error.
pub fn unify(t1: &Type, t2: &Type, span: Span) -> Result<Substitution, UnifyError> {
    match (t1, t2) {
        // Same constructor
        (Type::Con(a), Type::Con(b)) if a == b => Ok(Substitution::new()),

        // Variable on either side: bind
        (Type::Var(id), t) | (t, Type::Var(id)) => bind_var(*id, t, span),

        // Type application: unify constructor + each arg
        (Type::App(c1, args1), Type::App(c2, args2)) => {
            if args1.len() != args2.len() {
                return Err(UnifyError::arity(args1.len(), args2.len(), span));
            }
            let mut subst = unify(c1, c2, span)?;
            for (a1, a2) in args1.iter().zip(args2.iter()) {
                let a1 = a1.apply(&subst);
                let a2 = a2.apply(&subst);
                let s = unify(&a1, &a2, span)?;
                subst = subst.compose(&s);
            }
            Ok(subst)
        }

        // Function types: unify params + return
        (Type::Fn(p1, r1), Type::Fn(p2, r2)) => {
            if p1.len() != p2.len() {
                return Err(UnifyError::arity(p1.len(), p2.len(), span));
            }
            let mut subst = Substitution::new();
            for (a, b) in p1.iter().zip(p2.iter()) {
                let a = a.apply(&subst);
                let b = b.apply(&subst);
                let s = unify(&a, &b, span)?;
                subst = subst.compose(&s);
            }
            let r1 = r1.apply(&subst);
            let r2 = r2.apply(&subst);
            let s = unify(&r1, &r2, span)?;
            Ok(subst.compose(&s))
        }

        // Tuple types: unify element-wise
        (Type::Tuple(e1), Type::Tuple(e2)) => {
            if e1.len() != e2.len() {
                return Err(UnifyError::arity(e1.len(), e2.len(), span));
            }
            let mut subst = Substitution::new();
            for (a, b) in e1.iter().zip(e2.iter()) {
                let a = a.apply(&subst);
                let b = b.apply(&subst);
                let s = unify(&a, &b, span)?;
                subst = subst.compose(&s);
            }
            Ok(subst)
        }

        // Mismatch
        _ => Err(UnifyError::mismatch(t1, t2, span)),
    }
}

/// Bind a type variable to a type, with occurs check.
fn bind_var(var: TypeVarId, ty: &Type, span: Span) -> Result<Substitution, UnifyError> {
    // Self-binding: ?a ~ ?a → empty substitution
    if let Type::Var(other) = ty
        && var == *other
    {
        return Ok(Substitution::new());
    }

    // Occurs check: prevent infinite types like ?a ~ List(?a)
    if ty.free_vars().contains(&var) {
        return Err(UnifyError::occurs(var, ty, span));
    }

    let mut subst = Substitution::new();
    subst.bind(var, ty.clone());
    Ok(subst)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span() -> Span {
        Span::new(0, 0)
    }

    // === Success cases ===

    #[test]
    fn unify_same_con() {
        let s = unify(&Type::int(), &Type::int(), span());
        assert!(s.is_ok());
        assert!(s.as_ref().is_ok_and(Substitution::is_empty));
    }

    #[test]
    fn unify_var_with_con() {
        let s = unify(&Type::Var(0), &Type::int(), span());
        let s = s.expect("should unify");
        assert_eq!(Type::Var(0).apply(&s), Type::int());
    }

    #[test]
    fn unify_con_with_var() {
        let s = unify(&Type::int(), &Type::Var(0), span());
        let s = s.expect("should unify");
        assert_eq!(Type::Var(0).apply(&s), Type::int());
    }

    #[test]
    fn unify_var_with_var() {
        let s = unify(&Type::Var(0), &Type::Var(1), span());
        let s = s.expect("should unify");
        let t0 = Type::Var(0).apply(&s);
        let t1 = Type::Var(1).apply(&s);
        assert_eq!(t0, t1);
    }

    #[test]
    fn unify_var_self() {
        let s = unify(&Type::Var(0), &Type::Var(0), span());
        assert!(s.is_ok());
        assert!(s.as_ref().is_ok_and(Substitution::is_empty));
    }

    #[test]
    fn unify_list_types() {
        let s = unify(&Type::list(Type::Var(0)), &Type::list(Type::int()), span());
        let s = s.expect("should unify");
        assert_eq!(Type::Var(0).apply(&s), Type::int());
    }

    #[test]
    fn unify_fn_types() {
        // fn(?0, ?1) -> ?2  ~  fn(Int, String) -> Bool
        let t1 = Type::Fn(vec![Type::Var(0), Type::Var(1)], Box::new(Type::Var(2)));
        let t2 = Type::Fn(vec![Type::int(), Type::string()], Box::new(Type::bool()));
        let s = unify(&t1, &t2, span()).expect("should unify");
        assert_eq!(Type::Var(0).apply(&s), Type::int());
        assert_eq!(Type::Var(1).apply(&s), Type::string());
        assert_eq!(Type::Var(2).apply(&s), Type::bool());
    }

    #[test]
    fn unify_tuple_types() {
        let t1 = Type::Tuple(vec![Type::Var(0), Type::Var(1)]);
        let t2 = Type::Tuple(vec![Type::int(), Type::string()]);
        let s = unify(&t1, &t2, span()).expect("should unify");
        assert_eq!(Type::Var(0).apply(&s), Type::int());
        assert_eq!(Type::Var(1).apply(&s), Type::string());
    }

    #[test]
    fn unify_nested() {
        // List(?0) ~ List((Int, ?1))
        let t1 = Type::list(Type::Var(0));
        let t2 = Type::list(Type::Tuple(vec![Type::int(), Type::Var(1)]));
        let s = unify(&t1, &t2, span()).expect("should unify");
        assert_eq!(
            Type::Var(0).apply(&s),
            Type::Tuple(vec![Type::int(), Type::Var(1)])
        );
    }

    #[test]
    fn unify_transitive() {
        // ?0 ~ ?1, then ?1 ~ Int  → ?0 = Int
        let s1 = unify(&Type::Var(0), &Type::Var(1), span()).expect("ok");
        let s2 = unify(&Type::Var(1).apply(&s1), &Type::int(), span()).expect("ok");
        let composed = s1.compose(&s2);
        assert_eq!(Type::Var(0).apply(&composed), Type::int());
        assert_eq!(Type::Var(1).apply(&composed), Type::int());
    }

    // === Error cases ===

    #[test]
    fn unify_different_cons() {
        let e = unify(&Type::int(), &Type::string(), span());
        assert!(e.is_err());
        let err = e.unwrap_err();
        assert!(err.message.contains("type mismatch"));
        assert!(err.message.contains("Int"));
        assert!(err.message.contains("String"));
    }

    #[test]
    fn unify_fn_arity_mismatch() {
        let t1 = Type::Fn(vec![Type::int()], Box::new(Type::int()));
        let t2 = Type::Fn(vec![Type::int(), Type::int()], Box::new(Type::int()));
        let e = unify(&t1, &t2, span());
        assert!(e.is_err());
    }

    #[test]
    fn unify_tuple_arity_mismatch() {
        let t1 = Type::Tuple(vec![Type::int()]);
        let t2 = Type::Tuple(vec![Type::int(), Type::int()]);
        let e = unify(&t1, &t2, span());
        assert!(e.is_err());
    }

    #[test]
    fn unify_occurs_check() {
        // ?0 ~ List(?0) → infinite type
        let e = unify(&Type::Var(0), &Type::list(Type::Var(0)), span());
        assert!(e.is_err());
        let err = e.unwrap_err();
        assert!(err.message.contains("infinite type"));
    }

    #[test]
    fn unify_fn_vs_con() {
        let e = unify(
            &Type::Fn(vec![Type::int()], Box::new(Type::int())),
            &Type::int(),
            span(),
        );
        assert!(e.is_err());
    }

    #[test]
    fn unify_deep_mismatch() {
        // List(Int) ~ List(String)
        let e = unify(
            &Type::list(Type::int()),
            &Type::list(Type::string()),
            span(),
        );
        assert!(e.is_err());
    }
}
