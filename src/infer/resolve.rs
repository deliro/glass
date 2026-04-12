use std::collections::HashMap;

use crate::ast::*;
use crate::type_repr::{Substitution, Type};

impl super::Inferencer {
    pub(super) fn resolve_type_expr(&mut self, ty: &TypeExpr) -> Type {
        self.resolve_type_expr_inner(ty, &mut HashMap::new())
    }

    pub(super) fn resolve_type_expr_with_tvars(
        &mut self,
        ty: &TypeExpr,
        tvars: &mut HashMap<String, Type>,
    ) -> Type {
        self.resolve_type_expr_inner(ty, tvars)
    }

    pub(super) fn resolve_type_expr_inner(
        &mut self,
        ty: &TypeExpr,
        tvars: &mut HashMap<String, Type>,
    ) -> Type {
        match ty {
            TypeExpr::Named { name, args } => {
                // Known concrete types
                let base = match name.as_str() {
                    "Int" => Some(Type::int()),
                    "Float" => Some(Type::float()),
                    "Bool" => Some(Type::bool()),
                    "String" => Some(Type::string()),
                    _ => None,
                };
                if let Some(concrete) = base {
                    if args.is_empty() {
                        return concrete;
                    } else {
                        let resolved_args: Vec<Type> = args
                            .iter()
                            .map(|a| self.resolve_type_expr_inner(a, tvars))
                            .collect();
                        return Type::App(Box::new(concrete), resolved_args);
                    }
                }
                // Qualified name (module.Type) → resolve to base type name
                if let Some(dot_pos) = name.find('.') {
                    let type_name = &name[dot_pos + 1..];
                    let con = Type::Con(type_name.to_string());
                    if args.is_empty() {
                        return con;
                    } else {
                        let resolved_args: Vec<Type> = args
                            .iter()
                            .map(|a| self.resolve_type_expr_inner(a, tvars))
                            .collect();
                        return Type::App(Box::new(con), resolved_args);
                    }
                }
                // Lowercase name without args → type variable (Gleam-style)
                let first_char = name.chars().next().unwrap_or('A');
                if first_char.is_lowercase() && args.is_empty() {
                    return tvars
                        .entry(name.clone())
                        .or_insert_with(|| self.var_gen.fresh())
                        .clone();
                }
                // Uppercase name → concrete type constructor
                let con = Type::Con(name.clone());
                if args.is_empty() {
                    con
                } else {
                    let resolved_args: Vec<Type> = args
                        .iter()
                        .map(|a| self.resolve_type_expr_inner(a, tvars))
                        .collect();
                    Type::App(Box::new(con), resolved_args)
                }
            }
            TypeExpr::Fn { params, ret } => {
                let p: Vec<Type> = params
                    .iter()
                    .map(|a| self.resolve_type_expr_inner(a, tvars))
                    .collect();
                Type::Fn(p, Box::new(self.resolve_type_expr_inner(ret, tvars)))
            }
            TypeExpr::Tuple(elems) => Type::Tuple(
                elems
                    .iter()
                    .map(|a| self.resolve_type_expr_inner(a, tvars))
                    .collect(),
            ),
        }
    }

    pub fn resolve_type_expr_static(ty: &TypeExpr) -> Type {
        match ty {
            TypeExpr::Named { name, args } => {
                let base = match name.as_str() {
                    "Int" => Type::int(),
                    "Float" => Type::float(),
                    "Bool" => Type::bool(),
                    "String" => Type::string(),
                    _ => Type::Con(name.clone()),
                };
                if args.is_empty() {
                    base
                } else {
                    let resolved_args: Vec<Type> =
                        args.iter().map(Self::resolve_type_expr_static).collect();
                    Type::App(Box::new(base), resolved_args)
                }
            }
            TypeExpr::Fn { params, ret } => {
                let p: Vec<Type> = params.iter().map(Self::resolve_type_expr_static).collect();
                Type::Fn(p, Box::new(Self::resolve_type_expr_static(ret)))
            }
            TypeExpr::Tuple(elems) => {
                Type::Tuple(elems.iter().map(Self::resolve_type_expr_static).collect())
            }
        }
    }

    pub(super) fn resolve_type_expr_to_type(&self, ty: &Type) -> Type {
        ty.clone()
    }

    pub(super) fn fresh_subst_for(&mut self, var_ids: &[u32]) -> Substitution {
        let mut subst = Substitution::new();
        for &id in var_ids {
            subst.bind(id, self.var_gen.fresh());
        }
        subst
    }
}
