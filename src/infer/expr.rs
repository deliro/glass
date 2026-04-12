use crate::ast::*;
use crate::suggest::closest_match;
use crate::token::Span;
use crate::type_env::TypeEnv;
use crate::type_repr::{Type, TypeScheme};
use crate::unify;

use super::TypeError;

impl super::Inferencer {
    pub(super) fn infer_expr_inner(&mut self, expr: &Spanned<Expr>, env: &mut TypeEnv) -> Type {
        match &expr.node {
            // Literals
            Expr::Int(_) | Expr::Rawcode(_) => Type::int(),
            Expr::Float(_) => Type::float(),
            Expr::String(_) => Type::string(),
            Expr::Bool(_) => Type::bool(),

            // Variable
            Expr::Var(name) => match env.lookup(name) {
                Some(scheme) => env.instantiate(scheme, &mut self.var_gen),
                None => {
                    if self.module_names.contains(name) {
                        self.var_gen.fresh()
                    } else if let Some(modules) = self.ambiguous_names.get(name) {
                        let qualified: Vec<String> =
                            modules.iter().map(|m| format!("{m}.{name}")).collect();
                        self.errors.push(TypeError {
                            message: format!(
                                "ambiguous name `{name}` — defined in modules: {}. Use qualified syntax: {}",
                                modules.join(", "),
                                qualified.join(" or "),
                            ),
                            span: expr.span,
                        });
                        self.var_gen.fresh()
                    } else {
                        let suggestion = closest_match(name, env.all_names().into_iter());
                        let message = match suggestion {
                            Some(s) => format!("undefined variable '{name}', did you mean '{s}'?"),
                            None => format!("undefined variable '{name}'"),
                        };
                        self.errors.push(TypeError {
                            message,
                            span: expr.span,
                        });
                        self.var_gen.fresh()
                    }
                }
            },

            // Let binding
            Expr::Let {
                pattern,
                value,
                body,
                type_annotation,
                ..
            } => {
                let val_type = self.infer_expr(value, env);

                if let Some(ann) = type_annotation {
                    let ann_type = self.resolve_type_expr(ann);
                    if let Err(e) = unify::unify(&val_type, &ann_type, value.span) {
                        self.errors.push(e.into());
                    }
                }

                env.push_scope();
                self.bind_pattern(&pattern.node, &val_type, env, expr.span);
                let body_type = self.infer_expr(body, env);
                env.pop_scope();

                body_type
            }

            // Binary operations
            Expr::BinOp { op, left, right } => {
                let lt = self.infer_expr(left, env);
                let rt = self.infer_expr(right, env);
                self.infer_binop(*op, &lt, &rt, expr.span)
            }

            // Unary operations
            Expr::UnaryOp { op, operand } => {
                let t = self.infer_expr(operand, env);
                match op {
                    UnaryOp::Negate => {
                        // Works on Int or Float
                        if let Err(e) = unify::unify(&t, &Type::int(), operand.span) {
                            // Try Float
                            if unify::unify(&t, &Type::float(), operand.span).is_err() {
                                self.errors.push(e.into());
                            }
                        }
                        t
                    }
                    UnaryOp::Not => {
                        if let Err(e) = unify::unify(&t, &Type::bool(), operand.span) {
                            self.errors.push(e.into());
                        }
                        Type::bool()
                    }
                }
            }

            // Function call
            Expr::Call { function, args } => {
                let func_type = self.infer_expr(function, env);
                let arg_types: Vec<Type> = args.iter().map(|a| self.infer_expr(a, env)).collect();
                let ret_type = self.var_gen.fresh();

                let resolved_func = func_type.apply(&self.subst);
                if let Type::Fn(ref params, _) = resolved_func
                    && params.len() != arg_types.len()
                {
                    let fn_name = match &function.node {
                        Expr::Var(n) => n.clone(),
                        _ => "function".to_string(),
                    };
                    self.errors.push(TypeError {
                        message: format!(
                            "function '{}' expects {} arguments, got {}",
                            fn_name,
                            params.len(),
                            arg_types.len()
                        ),
                        span: expr.span,
                    });
                    return ret_type;
                }

                let expected_fn = Type::Fn(arg_types, Box::new(ret_type.clone()));
                match unify::unify(&resolved_func, &expected_fn.apply(&self.subst), expr.span) {
                    Ok(s) => {
                        self.subst = self.subst.compose(&s);
                        let resolved = ret_type.apply(&self.subst);
                        self.inferred_types.push(resolved.clone());
                        resolved
                    }
                    Err(e) => {
                        self.errors.push(e.into());
                        ret_type
                    }
                }
            }

            // Method call: obj.method(args) → method(obj, args)
            Expr::MethodCall {
                object,
                method,
                args,
            } => {
                if let Expr::Var(module_name) = &object.node {
                    let qualified = format!("{}.{}", module_name, method);
                    if let Some(scheme) = env.lookup(&qualified).cloned() {
                        let arg_types: Vec<Type> =
                            args.iter().map(|a| self.infer_expr(a, env)).collect();
                        let ret_type = self.var_gen.fresh();
                        let fn_type = env.instantiate(&scheme, &mut self.var_gen);
                        let resolved_fn = fn_type.apply(&self.subst);
                        if let Type::Fn(ref params, _) = resolved_fn
                            && params.len() != arg_types.len()
                        {
                            self.errors.push(TypeError {
                                message: format!(
                                    "function '{}' expects {} arguments, got {}",
                                    qualified,
                                    params.len(),
                                    arg_types.len()
                                ),
                                span: expr.span,
                            });
                            return ret_type;
                        }
                        let expected = Type::Fn(arg_types, Box::new(ret_type.clone()));
                        match unify::unify(&resolved_fn, &expected.apply(&self.subst), expr.span) {
                            Ok(s) => {
                                self.subst = self.subst.compose(&s);
                                let resolved = ret_type.apply(&self.subst);
                                self.inferred_types.push(resolved.clone());
                                return resolved;
                            }
                            Err(e) => {
                                self.errors.push(e.into());
                                return ret_type;
                            }
                        }
                    }
                    if let Some(scheme) = env.lookup(method).cloned() {
                        let arg_types: Vec<Type> =
                            args.iter().map(|a| self.infer_expr(a, env)).collect();
                        let ret_type = self.var_gen.fresh();
                        let fn_type = env.instantiate(&scheme, &mut self.var_gen);
                        let resolved_fn = fn_type.apply(&self.subst);
                        if let Type::Fn(ref params, _) = resolved_fn
                            && params.len() != arg_types.len()
                        {
                            self.errors.push(TypeError {
                                message: format!(
                                    "function '{}' expects {} arguments, got {}",
                                    method,
                                    params.len(),
                                    arg_types.len()
                                ),
                                span: expr.span,
                            });
                            return ret_type;
                        }
                        let expected = Type::Fn(arg_types, Box::new(ret_type.clone()));
                        match unify::unify(&resolved_fn, &expected.apply(&self.subst), expr.span) {
                            Ok(s) => {
                                self.subst = self.subst.compose(&s);
                                let resolved = ret_type.apply(&self.subst);
                                self.inferred_types.push(resolved.clone());
                                return resolved;
                            }
                            Err(e) => {
                                self.errors.push(e.into());
                                return ret_type;
                            }
                        }
                    }
                }

                let obj_type = self.infer_expr(object, env);
                let mut all_arg_types = vec![obj_type];
                for a in args {
                    all_arg_types.push(self.infer_expr(a, env));
                }

                let ret_type = self.var_gen.fresh();
                match env.lookup(method) {
                    Some(scheme) => {
                        let fn_type = env.instantiate(scheme, &mut self.var_gen);
                        let resolved_fn = fn_type.apply(&self.subst);
                        if let Type::Fn(ref params, _) = resolved_fn
                            && params.len() != all_arg_types.len()
                        {
                            self.errors.push(TypeError {
                                message: format!(
                                    "function '{}' expects {} arguments, got {}",
                                    method,
                                    params.len(),
                                    all_arg_types.len()
                                ),
                                span: expr.span,
                            });
                            return ret_type;
                        }
                        let expected = Type::Fn(all_arg_types, Box::new(ret_type.clone()));
                        match unify::unify(&resolved_fn, &expected.apply(&self.subst), expr.span) {
                            Ok(s) => {
                                self.subst = self.subst.compose(&s);
                                ret_type.apply(&self.subst)
                            }
                            Err(e) => {
                                self.errors.push(e.into());
                                ret_type
                            }
                        }
                    }
                    None => {
                        // Unknown method — return fresh var
                        ret_type
                    }
                }
            }

            Expr::FieldAccess { object, field } => {
                if let Expr::Var(module_name) = &object.node
                    && self.module_names.contains(module_name)
                {
                    let qualified = format!("{}.{}", module_name, field);
                    if let Some(const_type) = self.const_types.get(&qualified) {
                        return const_type.clone();
                    }
                    if let Some(const_type) = self.const_types.get(field.as_str()) {
                        return const_type.clone();
                    }
                    if let Some(scheme) = env.lookup(&qualified).cloned() {
                        let t = env.instantiate(&scheme, &mut self.var_gen);
                        self.inferred_types.push(t.clone());
                        return t;
                    }
                    if let Some(scheme) = env.lookup(field).cloned() {
                        let t = env.instantiate(&scheme, &mut self.var_gen);
                        self.inferred_types.push(t.clone());
                        return t;
                    }
                }
                let obj_type = self.infer_expr(object, env);
                let obj_resolved = obj_type.apply(&self.subst);
                // Look up the field type from the constructor registry
                let type_name = match &obj_resolved {
                    Type::Con(name) => Some(name.as_str()),
                    Type::App(con, _) => match con.as_ref() {
                        Type::Con(name) => Some(name.as_str()),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(tn) = type_name {
                    for info in self.constructors.all() {
                        if info.type_name == tn
                            && let Some((_, ft)) = info.field_types.iter().find(|(n, _)| n == field)
                        {
                            return ft.apply(&self.subst);
                        }
                    }
                    let known_fields = self
                        .constructors
                        .all()
                        .filter(|info| info.type_name == tn)
                        .flat_map(|info| info.field_types.iter().map(|(n, _)| n.as_str()));
                    let suggestion = closest_match(field, known_fields);
                    let message = match suggestion {
                        Some(s) => {
                            format!(
                                "type '{}' has no field '{}', did you mean '{}'?",
                                tn, field, s
                            )
                        }
                        None => format!("type '{}' has no field '{}'", tn, field),
                    };
                    self.errors.push(TypeError {
                        message,
                        span: object.span,
                    });
                }
                self.var_gen.fresh()
            }

            // Constructor: Name(args)
            Expr::Constructor { name, args } => {
                match env.lookup(name).cloned() {
                    Some(scheme) => {
                        let ctor_type = env.instantiate(&scheme, &mut self.var_gen);
                        if args.is_empty() {
                            // Nullary constructor: type is the result type directly
                            ctor_type
                        } else {
                            // Apply constructor as function
                            let arg_types: Vec<Type> = args
                                .iter()
                                .map(|a| {
                                    let e = match a {
                                        ConstructorArg::Positional(e)
                                        | ConstructorArg::Named(_, e) => e,
                                    };
                                    self.infer_expr(e, env)
                                })
                                .collect();
                            let ret = self.var_gen.fresh();
                            let expected = Type::Fn(arg_types, Box::new(ret.clone()));
                            match unify::unify(
                                &ctor_type.apply(&self.subst),
                                &expected.apply(&self.subst),
                                expr.span,
                            ) {
                                Ok(s) => {
                                    self.subst = self.subst.compose(&s);
                                    ret.apply(&self.subst)
                                }
                                Err(e) => {
                                    self.errors.push(e.into());
                                    ret
                                }
                            }
                        }
                    }
                    None => {
                        let suggestion = closest_match(
                            name,
                            self.constructors.constructors.keys().map(|s| s.as_str()),
                        );
                        let message = match suggestion {
                            Some(s) => {
                                format!("unknown constructor '{name}', did you mean '{s}'?")
                            }
                            None => format!("unknown constructor '{name}'"),
                        };
                        self.errors.push(TypeError {
                            message,
                            span: expr.span,
                        });
                        self.var_gen.fresh()
                    }
                }
            }

            // Record update
            Expr::RecordUpdate {
                name,
                base,
                updates,
            } => {
                let base_type = self.infer_expr(base, env);
                let expected = Type::Con(name.clone());
                if let Err(e) = unify::unify(&base_type.apply(&self.subst), &expected, expr.span) {
                    self.errors.push(e.into());
                }
                for (field_name, val) in updates {
                    let val_type = self.infer_expr(val, env);
                    let mut found = false;
                    for info in self.constructors.all() {
                        if info.type_name == *name
                            && let Some((_, ft)) =
                                info.field_types.iter().find(|(n, _)| n == field_name)
                        {
                            found = true;
                            if let Err(e) = unify::unify(
                                &val_type.apply(&self.subst),
                                &ft.apply(&self.subst),
                                val.span,
                            ) {
                                self.errors.push(e.into());
                            }
                            break;
                        }
                    }
                    if !found {
                        let known_fields = self
                            .constructors
                            .all()
                            .filter(|info| info.type_name == *name)
                            .flat_map(|info| info.field_types.iter().map(|(n, _)| n.as_str()));
                        let suggestion = closest_match(field_name, known_fields);
                        let message = match suggestion {
                            Some(s) => format!(
                                "type '{}' has no field '{}', did you mean '{}'?",
                                name, field_name, s
                            ),
                            None => format!("type '{}' has no field '{}'", name, field_name),
                        };
                        self.errors.push(TypeError {
                            message,
                            span: val.span,
                        });
                    }
                }
                expected
            }

            // Tuple
            Expr::Tuple(elems) => {
                let types: Vec<Type> = elems.iter().map(|e| self.infer_expr(e, env)).collect();
                Type::Tuple(types)
            }

            // List
            Expr::List(elems) => {
                if elems.is_empty() {
                    Type::list(self.var_gen.fresh())
                } else {
                    let Some((first, rest)) = elems.split_first() else {
                        // Unreachable: we checked !is_empty above
                        return Type::list(self.var_gen.fresh());
                    };
                    let elem_type = self.infer_expr(first, env);
                    for e in rest {
                        let t = self.infer_expr(e, env);
                        if let Err(err) = unify::unify(
                            &elem_type.apply(&self.subst),
                            &t.apply(&self.subst),
                            e.span,
                        ) {
                            self.errors.push(err.into());
                        }
                    }
                    Type::list(elem_type.apply(&self.subst))
                }
            }

            // Lambda
            Expr::Lambda {
                params,
                return_type,
                body,
            } => {
                env.push_scope();
                let param_types: Vec<Type> = params
                    .iter()
                    .map(|p| {
                        let ty = self.resolve_type_expr(&p.type_expr);
                        env.bind(p.name.clone(), TypeScheme::mono(ty.clone()));
                        ty
                    })
                    .collect();

                let body_type = self.infer_expr(body, env);

                if let Some(ret_ann) = return_type {
                    let ret = self.resolve_type_expr(ret_ann);
                    if let Err(e) = unify::unify(&body_type.apply(&self.subst), &ret, body.span) {
                        self.errors.push(e.into());
                    }
                }

                env.pop_scope();
                Type::Fn(param_types, Box::new(body_type.apply(&self.subst)))
            }

            // Pipe: a |> f → f(a), a |> f(b) → f(a, b), a |> f(b, _) → f(b, a)
            Expr::Pipe { left, right } => {
                let left_type = self.infer_expr(left, env);

                match &right.node {
                    // a |> f(b, c) or a |> f(b, _, c)
                    Expr::Call { function, args } => {
                        let func_type = self.infer_expr(function, env);

                        // Check if any arg is `_` (placeholder for piped value)
                        let has_placeholder = args
                            .iter()
                            .any(|a| matches!(&a.node, Expr::Var(n) if n == "_"));

                        let all_arg_types: Vec<Type> = if has_placeholder {
                            // Replace _ with left_type, infer others normally
                            args.iter()
                                .map(|arg| {
                                    if matches!(&arg.node, Expr::Var(n) if n == "_") {
                                        left_type.apply(&self.subst)
                                    } else {
                                        self.infer_expr(arg, env)
                                    }
                                })
                                .collect()
                        } else {
                            // No placeholder: insert left as first arg
                            let mut all = vec![left_type.apply(&self.subst)];
                            for arg in args {
                                all.push(self.infer_expr(arg, env));
                            }
                            all
                        };

                        let ret_type = self.var_gen.fresh();
                        let expected_fn = Type::Fn(all_arg_types, Box::new(ret_type.clone()));

                        match unify::unify(
                            &func_type.apply(&self.subst),
                            &expected_fn.apply(&self.subst),
                            expr.span,
                        ) {
                            Ok(s) => {
                                self.subst = self.subst.compose(&s);
                                ret_type.apply(&self.subst)
                            }
                            Err(e) => {
                                self.errors.push(e.into());
                                ret_type
                            }
                        }
                    }
                    Expr::MethodCall {
                        object,
                        method,
                        args,
                    } => {
                        let _obj_type = self.infer_expr(object, env);
                        let func_type = if let Some(scheme) = env.lookup(method).cloned() {
                            env.instantiate(&scheme, &mut self.var_gen)
                        } else {
                            self.var_gen.fresh()
                        };

                        let has_placeholder = args
                            .iter()
                            .any(|a| matches!(&a.node, Expr::Var(n) if n == "_"));

                        let all_arg_types: Vec<Type> = if has_placeholder {
                            args.iter()
                                .map(|arg| {
                                    if matches!(&arg.node, Expr::Var(n) if n == "_") {
                                        left_type.apply(&self.subst)
                                    } else {
                                        self.infer_expr(arg, env)
                                    }
                                })
                                .collect()
                        } else {
                            let mut all = vec![left_type.apply(&self.subst)];
                            for arg in args {
                                all.push(self.infer_expr(arg, env));
                            }
                            all
                        };

                        let ret_type = self.var_gen.fresh();
                        let expected_fn = Type::Fn(all_arg_types, Box::new(ret_type.clone()));

                        match unify::unify(
                            &func_type.apply(&self.subst),
                            &expected_fn.apply(&self.subst),
                            expr.span,
                        ) {
                            Ok(s) => {
                                self.subst = self.subst.compose(&s);
                                ret_type.apply(&self.subst)
                            }
                            Err(e) => {
                                self.errors.push(e.into());
                                ret_type
                            }
                        }
                    }
                    Expr::FieldAccess { object, field } if matches!(&object.node, Expr::Var(_)) => {
                        let module_name = match &object.node {
                            Expr::Var(n) => n,
                            _ => "",
                        };
                        let qualified = format!("{}.{}", module_name, field);
                        let func_type = if let Some(scheme) = env.lookup(&qualified).cloned() {
                            env.instantiate(&scheme, &mut self.var_gen)
                        } else if let Some(scheme) = env.lookup(field).cloned() {
                            env.instantiate(&scheme, &mut self.var_gen)
                        } else {
                            self.errors.push(TypeError {
                                message: format!("undefined function '{}'", qualified),
                                span: right.span,
                            });
                            self.var_gen.fresh()
                        };
                        let ret_type = self.var_gen.fresh();
                        let expected = Type::Fn(
                            vec![left_type.apply(&self.subst)],
                            Box::new(ret_type.clone()),
                        );
                        match unify::unify(
                            &func_type.apply(&self.subst),
                            &expected.apply(&self.subst),
                            expr.span,
                        ) {
                            Ok(s) => {
                                self.subst = self.subst.compose(&s);
                                ret_type.apply(&self.subst)
                            }
                            Err(e) => {
                                self.errors.push(e.into());
                                ret_type
                            }
                        }
                    }
                    _ => {
                        let right_type = self.infer_expr(right, env);
                        let ret_type = self.var_gen.fresh();
                        let expected = Type::Fn(
                            vec![left_type.apply(&self.subst)],
                            Box::new(ret_type.clone()),
                        );

                        match unify::unify(
                            &right_type.apply(&self.subst),
                            &expected.apply(&self.subst),
                            expr.span,
                        ) {
                            Ok(s) => {
                                self.subst = self.subst.compose(&s);
                                ret_type.apply(&self.subst)
                            }
                            Err(e) => {
                                self.errors.push(e.into());
                                ret_type
                            }
                        }
                    }
                }
            }

            // Block
            Expr::Block(exprs) => {
                let mut result = self.var_gen.fresh();
                for e in exprs {
                    result = self.infer_expr(e, env);
                }
                result
            }

            // Case
            Expr::Case { subject, arms } => {
                let subject_type = self.infer_expr(subject, env);
                let result_type = self.var_gen.fresh();

                for arm in arms {
                    env.push_scope();
                    self.check_pattern(&arm.pattern.node, &subject_type, env, arm.pattern.span);

                    if let Some(guard) = &arm.guard {
                        let gt = self.infer_expr(guard, env);
                        if let Err(e) = unify::unify(&gt, &Type::bool(), guard.span) {
                            self.errors.push(e.into());
                        }
                    }

                    let arm_type = self.infer_expr(&arm.body, env);
                    match unify::unify(
                        &arm_type.apply(&self.subst),
                        &result_type.apply(&self.subst),
                        arm.body.span,
                    ) {
                        Ok(s) => self.subst = self.subst.compose(&s),
                        Err(e) => self.errors.push(e.into()),
                    }
                    env.pop_scope();
                }

                result_type.apply(&self.subst)
            }

            // List cons: [head | tail]
            Expr::ListCons { head, tail } => {
                let elem_type = self.infer_expr(head, env);
                let list_type = Type::list(elem_type);
                let tail_type = self.infer_expr(tail, env);
                if let Err(e) = unify::unify(
                    &tail_type.apply(&self.subst),
                    &list_type.apply(&self.subst),
                    tail.span,
                ) {
                    self.errors.push(e.into());
                }
                list_type.apply(&self.subst)
            }

            // Clone
            Expr::Clone(inner) => self.infer_expr(inner, env),

            // TCO nodes (appear after TCO pass, which runs after inference)
            Expr::TcoLoop { body } => self.infer_expr(body, env),

            // Todo and TcoContinue are opaque — produce a fresh type variable
            Expr::Todo(_) | Expr::TcoContinue { .. } => self.var_gen.fresh(),
        }
    }

    pub(super) fn infer_binop(&mut self, op: BinOp, lt: &Type, rt: &Type, span: Span) -> Type {
        match op {
            // Arithmetic: Int -> Int -> Int (or Float -> Float -> Float)
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                if let Err(e) = unify::unify(&lt.apply(&self.subst), &rt.apply(&self.subst), span) {
                    self.errors.push(e.into());
                }
                lt.apply(&self.subst)
            }
            // Comparison: a -> a -> Bool
            BinOp::Eq
            | BinOp::NotEq
            | BinOp::Less
            | BinOp::Greater
            | BinOp::LessEq
            | BinOp::GreaterEq => {
                if let Err(e) = unify::unify(&lt.apply(&self.subst), &rt.apply(&self.subst), span) {
                    self.errors.push(e.into());
                }
                Type::bool()
            }
            // Logical: Bool -> Bool -> Bool
            BinOp::And | BinOp::Or => {
                if let Err(e) = unify::unify(lt, &Type::bool(), span) {
                    self.errors.push(e.into());
                }
                if let Err(e) = unify::unify(rt, &Type::bool(), span) {
                    self.errors.push(e.into());
                }
                Type::bool()
            }
            // String concat: String -> String -> String
            BinOp::StringConcat => {
                if let Err(e) = unify::unify(lt, &Type::string(), span) {
                    self.errors.push(e.into());
                }
                if let Err(e) = unify::unify(rt, &Type::string(), span) {
                    self.errors.push(e.into());
                }
                Type::string()
            }
        }
    }
}
