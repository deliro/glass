use crate::ast::*;
use crate::suggest::closest_match;
use crate::token::Span;
use crate::type_env::TypeEnv;
use crate::type_repr::{Type, TypeScheme};
use crate::unify;

use super::TypeError;

impl super::Inferencer {
    pub(super) fn check_pattern(&mut self, pat: &Pattern, expected: &Type, env: &mut TypeEnv, span: Span) {
        match pat {
            Pattern::Var(name) => {
                env.bind(name.clone(), TypeScheme::mono(expected.apply(&self.subst)));
            }
            Pattern::Discard => {}
            Pattern::Bool(_) => {
                if let Err(e) = unify::unify(expected, &Type::bool(), span) {
                    self.errors.push(e.into());
                }
            }
            Pattern::Int(_) | Pattern::Rawcode(_) => {
                if let Err(e) = unify::unify(expected, &Type::int(), span) {
                    self.errors.push(e.into());
                }
            }
            // Pattern::Float removed — floating point equality is unreliable
            Pattern::String(_) => {
                if let Err(e) = unify::unify(expected, &Type::string(), span) {
                    self.errors.push(e.into());
                }
            }
            Pattern::Constructor { name, args } => {
                if let Some(info) = self.constructors.lookup(name).cloned() {
                    // Instantiate type vars with fresh vars for this usage
                    let fresh_subst = self.fresh_subst_for(&info.type_var_ids);
                    let result = info.result_type.apply(&fresh_subst);
                    if let Err(e) = unify::unify(
                        &expected.apply(&self.subst),
                        &result.apply(&self.subst),
                        span,
                    ) {
                        self.errors.push(e.into());
                    }
                    for (arg_pat, (_fname, ftype)) in args.iter().zip(info.field_types.iter()) {
                        let ft = ftype.apply(&fresh_subst);
                        self.check_pattern(&arg_pat.node, &ft, env, arg_pat.span);
                    }
                }
            }
            Pattern::Tuple(elems) => {
                let elem_types: Vec<Type> = elems.iter().map(|_| self.var_gen.fresh()).collect();
                let tuple_type = Type::Tuple(elem_types.clone());
                if let Err(e) = unify::unify(&expected.apply(&self.subst), &tuple_type, span) {
                    self.errors.push(e.into());
                }
                for (pat, ty) in elems.iter().zip(elem_types.iter()) {
                    self.check_pattern(&pat.node, ty, env, pat.span);
                }
            }
            Pattern::List(_) => {
                // Empty list pattern
                let elem = self.var_gen.fresh();
                if let Err(e) = unify::unify(expected, &Type::list(elem), span) {
                    self.errors.push(e.into());
                }
            }
            Pattern::ListCons { head, tail } => {
                let elem = self.var_gen.fresh();
                let list_type = Type::list(elem.clone());
                if let Err(e) = unify::unify(
                    &expected.apply(&self.subst),
                    &list_type.apply(&self.subst),
                    span,
                ) {
                    self.errors.push(e.into());
                }
                self.check_pattern(&head.node, &elem, env, head.span);
                self.check_pattern(&tail.node, &list_type, env, tail.span);
            }
            Pattern::ConstructorNamed { name, fields, rest } => {
                if let Some(info) = self.constructors.lookup(name).cloned() {
                    // Instantiate type vars with fresh vars for this usage
                    let fresh_subst = self.fresh_subst_for(&info.type_var_ids);
                    let result = info.result_type.apply(&fresh_subst);
                    if let Err(e) = unify::unify(
                        &expected.apply(&self.subst),
                        &result.apply(&self.subst),
                        span,
                    ) {
                        self.errors.push(e.into());
                    }
                    for fp in fields {
                        if let Some((_, ftype)) =
                            info.field_types.iter().find(|(n, _)| *n == fp.field_name)
                        {
                            let ft = ftype.apply(&fresh_subst);
                            if let Some(nested_pat) = &fp.pattern {
                                match &nested_pat.node {
                                    Pattern::Var(var_name) => {
                                        env.bind(
                                            var_name.clone(),
                                            TypeScheme::mono(ft.apply(&self.subst)),
                                        );
                                    }
                                    _ => {
                                        self.check_pattern(
                                            &nested_pat.node,
                                            &ft.apply(&self.subst),
                                            env,
                                            nested_pat.span,
                                        );
                                    }
                                }
                            } else {
                                env.bind(
                                    fp.field_name.clone(),
                                    TypeScheme::mono(ft.apply(&self.subst)),
                                );
                            }
                        } else {
                            let known_fields = info.field_types.iter().map(|(n, _)| n.as_str());
                            let suggestion = closest_match(&fp.field_name, known_fields);
                            let message = match suggestion {
                                Some(s) => format!(
                                    "unknown field '{}' in constructor '{}', did you mean '{}'?",
                                    fp.field_name, name, s
                                ),
                                None => format!(
                                    "unknown field '{}' in constructor '{}'",
                                    fp.field_name, name
                                ),
                            };
                            self.errors.push(TypeError { message, span });
                        }
                    }
                    // If no `..`, all fields must be mentioned
                    if !rest {
                        let mentioned: std::collections::HashSet<&str> =
                            fields.iter().map(|fp| fp.field_name.as_str()).collect();
                        let missing: Vec<&str> = info
                            .field_types
                            .iter()
                            .filter(|(n, _)| !mentioned.contains(n.as_str()))
                            .map(|(n, _)| n.as_str())
                            .collect();
                        if !missing.is_empty() {
                            self.errors.push(TypeError {
                                message: format!(
                                    "missing fields in pattern for '{}': {}. Use `..` to ignore remaining fields",
                                    name,
                                    missing.join(", ")
                                ),
                                span,
                            });
                        }
                    }
                }
            }
            Pattern::Or(alternatives) => {
                // Check each alternative against expected type
                // All alternatives must bind the same variables with the same types
                for alt in alternatives {
                    env.push_scope();
                    self.check_pattern(&alt.node, expected, env, alt.span);
                    env.pop_scope();
                }
                // For bindings: use the first alternative's bindings
                // (type checker verified they match)
                if let Some(first) = alternatives.first() {
                    self.check_pattern(&first.node, expected, env, first.span);
                }
            }
            Pattern::As { pattern, name } => {
                self.check_pattern(&pattern.node, expected, env, span);
                env.bind(name.clone(), TypeScheme::mono(expected.apply(&self.subst)));
            }
        }
    }

    pub(super) fn bind_pattern(&mut self, pat: &Pattern, ty: &Type, env: &mut TypeEnv, span: Span) {
        self.check_pattern(pat, ty, env, span);
    }

}
