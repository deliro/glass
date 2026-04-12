use crate::ast::*;
use crate::type_repr::Type;

use super::{format_float, safe_jass_name};

impl super::JassCodegen {
    pub(super) fn handle_destroy_fn(ty: &TypeExpr) -> Option<String> {
        let TypeExpr::Named { name, .. } = ty else {
            return None;
        };
        match name.as_str() {
            "Timer" => Some("DestroyTimer".into()),
            "Trigger" => Some("DestroyTrigger".into()),
            "Group" => Some("DestroyGroup".into()),
            "Force" => Some("DestroyForce".into()),
            "Region" => Some("RemoveRegion".into()),
            "Location" => Some("RemoveLocation".into()),
            "Sound" => Some("StopSound".into()),
            "Unit" => Some("RemoveUnit".into()),
            "Effect" => Some("DestroyEffect".into()),
            "Dialog" => Some("DialogDestroy".into()),
            "Quest" => Some("DestroyQuest".into()),
            "Multiboard" => Some("DestroyMultiboard".into()),
            "Leaderboard" => Some("DestroyLeaderboard".into()),
            _ => None,
        }
    }

    pub(super) fn gen_external_def(&mut self, _e: &ExternalDef) {
        // External functions map directly to JASS natives — no code generated.
        // The call sites will use the native name directly.
    }

    pub(super) fn gen_const_def(&mut self, _c: &ConstDef) {
        // Constants are fully inlined at use sites — no codegen needed.
    }

    // === Expressions ===

    pub(super) fn gen_spanned_expr(&mut self, expr: &Spanned<Expr>) -> String {
        let prev = self.current_expr_span.replace(expr.span);
        let result = self.gen_expr(&expr.node);
        self.current_expr_span = prev;
        result
    }

    pub(super) fn gen_expr(&mut self, expr: &Expr) -> String {
        match expr {
            Expr::Int(n) => n.to_string(),
            Expr::Float(n) => format_float(*n),
            Expr::String(s) => format!("\"{}\"", s),
            Expr::Rawcode(s) => format!("'{}'", s),
            Expr::Bool(b) => {
                if *b {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            Expr::Var(name) => {
                if let Some(value) = self.const_values.get(name.as_str()) {
                    return value.clone();
                }
                safe_jass_name(name)
            }

            Expr::BinOp { op, left, right } => {
                // Constant folding: evaluate compile-time constants
                if let Some(result) = const_fold_binop(op, &left.node, &right.node) {
                    return result;
                }
                let l = self.gen_spanned_expr(&left);
                let r = self.gen_spanned_expr(&right);
                let op_str = match op {
                    BinOp::Add | BinOp::StringConcat => "+",
                    BinOp::Sub => "-",
                    BinOp::Mul => "*",
                    BinOp::Div => "/",
                    BinOp::Mod => {
                        // JASS has no modulo, use: a - (a / b) * b
                        return format!("({} - ({} / {}) * {})", l, l, r, r);
                    }
                    BinOp::Eq => "==",
                    BinOp::NotEq => "!=",
                    BinOp::Less => "<",
                    BinOp::Greater => ">",
                    BinOp::LessEq => "<=",
                    BinOp::GreaterEq => ">=",
                    BinOp::And => "and",
                    BinOp::Or => "or",
                };
                format!("({} {} {})", l, op_str, r)
            }

            Expr::UnaryOp { op, operand } => {
                let o = self.gen_spanned_expr(&operand);
                match op {
                    UnaryOp::Negate => format!("-({})", o),
                    UnaryOp::Not => format!("not ({})", o),
                }
            }

            Expr::Call { function, args } => {
                let ext_info = if let Expr::Var(name) = &function.node {
                    if self.fn_defs.contains_key(name.as_str()) {
                        None
                    } else {
                        self.externals.get(name.as_str()).cloned()
                    }
                } else {
                    None
                };

                if let Some(ext) = ext_info {
                    let args_str: Vec<String> =
                        args.iter().map(|a| self.gen_spanned_expr(&a)).collect();
                    if ext.module == "glass" {
                        // For dict_load: we need the return type of this call.
                        // Pass the function var span as a hint — the type checker
                        // records the full call expression type at the function span.
                        let call_span = Some(function.span);
                        return self.gen_intrinsic_call(&ext.jass_name, args, &args_str, call_span);
                    }
                    return format!("{}({})", ext.jass_name, args_str.join(", "));
                }

                let func_name = match &function.node {
                    Expr::Var(name) => {
                        // Check if the variable holds a closure value (Type::Fn in type_map)
                        // BUT exclude top-level function names (they're also Fn type but called directly)
                        let is_known_function = self.fn_defs.contains_key(name.as_str())
                            || self.externals.contains_key(name.as_str());
                        let is_closure_var = !is_known_function
                            && self
                                .lookup_full_type(function.span)
                                .is_some_and(|ty| matches!(ty, Type::Fn(_, _)));

                        if is_closure_var {
                            let args_str: Vec<String> =
                                args.iter().map(|a| self.gen_spanned_expr(&a)).collect();
                            let param_jass_types: Vec<String> = args
                                .iter()
                                .map(|a| {
                                    self.lookup_full_type(a.span)
                                        .map(|ty| self.type_to_jass_from_type(&ty))
                                        .unwrap_or_else(|| "integer".to_string())
                                })
                                .collect();
                            let dispatch_name = Self::dispatch_fn_name(&param_jass_types);
                            let mut dispatch_args = vec![name.clone()];
                            dispatch_args.extend(args_str);
                            return format!("{}({})", dispatch_name, dispatch_args.join(", "));
                        }

                        // Check if this function needs monomorphization
                        if self.mono_needed.contains(name.as_str()) {
                            let concrete_types: Vec<Type> = args
                                .iter()
                                .map(|a| self.lookup_full_type(a.span).unwrap_or(Type::int()))
                                .collect();
                            let mangled = Self::mangle_types(&concrete_types);
                            let mono_name = format!("glass_{}_{}", name, mangled);
                            if !self.mono_generated.contains(&mono_name) {
                                self.mono_generated.insert(mono_name.clone());
                                let subst = self.build_mono_subst(name, &concrete_types);
                                self.gen_mono_function(name, &mono_name, subst);
                            }
                            let args_str: Vec<String> =
                                args.iter().map(|a| self.gen_spanned_expr(&a)).collect();
                            return format!("{}({})", mono_name, args_str.join(", "));
                        }
                        format!("glass_{}", name)
                    }
                    Expr::FieldAccess { object, field } => {
                        if let Expr::Var(module_name) = &object.node {
                            let qualified = format!("{}.{}", module_name, field);
                            if let Some(ext) = self
                                .externals
                                .get(&qualified)
                                .or_else(|| {
                                    if self.fn_defs.contains_key(field.as_str())
                                        || self.fn_defs.contains_key(qualified.as_str())
                                    {
                                        None
                                    } else {
                                        self.externals.get(field.as_str())
                                    }
                                })
                                .cloned()
                            {
                                let args_str: Vec<String> =
                                    args.iter().map(|a| self.gen_spanned_expr(&a)).collect();
                                if ext.module == "glass" {
                                    return self.gen_intrinsic_call(
                                        &ext.jass_name,
                                        args,
                                        &args_str,
                                        Some(function.span),
                                    );
                                }
                                return format!("{}({})", ext.jass_name, args_str.join(", "));
                            }
                            if self.fn_defs.contains_key(field.as_str()) {
                                let args_str: Vec<String> =
                                    args.iter().map(|a| self.gen_spanned_expr(&a)).collect();
                                return format!("glass_{}({})", field, args_str.join(", "));
                            }
                        }
                        self.gen_spanned_expr(&function)
                    }
                    _ => self.gen_spanned_expr(&function),
                };
                let args_str: Vec<String> =
                    args.iter().map(|a| self.gen_spanned_expr(&a)).collect();
                format!("{}({})", func_name, args_str.join(", "))
            }

            Expr::FieldAccess { object, field } => {
                if let Expr::Var(module_name) = &object.node {
                    let qualified = format!("{}.{}", module_name, field);
                    if let Some(value) = self.const_values.get(&qualified) {
                        return value.clone();
                    }
                }
                if let Some(value) = self.const_values.get(field.as_str()) {
                    return value.clone();
                }

                let obj = self.gen_spanned_expr(&object);
                let mut type_name = self
                    .lookup_full_type(object.span)
                    .or_else(|| {
                        if let Expr::Var(name) = &object.node {
                            self.lookup_full_type(crate::token::Span {
                                start: object.span.start,
                                end: object.span.start + name.len(),
                            })
                        } else {
                            None
                        }
                    })
                    .map(|ty| match &ty {
                        Type::Con(name) => name.clone(),
                        Type::App(con, _) => match con.as_ref() {
                            Type::Con(name) => name.clone(),
                            _ => String::new(),
                        },
                        _ => String::new(),
                    })
                    .unwrap_or_default();

                // Fallback: if type_map gave a primitive or empty name, search TypeRegistry
                // for a user-defined type that has this field.
                if type_name.is_empty() || !self.types.types.contains_key(&type_name) {
                    let mut candidates: Vec<String> = self
                        .types
                        .types
                        .iter()
                        .filter(|(_, info)| {
                            info.variants
                                .iter()
                                .any(|v| v.fields.iter().any(|f| f.name == *field))
                        })
                        .map(|(tn, _)| tn.clone())
                        .collect();
                    candidates.sort();
                    if candidates.len() == 1 {
                        type_name = candidates.into_iter().next().unwrap_or_default();
                    } else if candidates.len() > 1 {
                        let obj_glass_type = if let Expr::Var(name) = &object.node {
                            self.local_var_glass_types.get(name).cloned()
                        } else {
                            None
                        };
                        let local_match = obj_glass_type
                            .and_then(|gt| candidates.iter().find(|c| **c == gt).cloned());
                        let struct_match = || {
                            candidates
                                .iter()
                                .find(|c| self.types.types.get(*c).is_some_and(|ti| !ti.is_enum))
                                .cloned()
                        };
                        let param_match = || {
                            candidates
                                .iter()
                                .find(|c| self.current_fn_param_type_names.contains(c))
                                .cloned()
                        };
                        type_name = local_match
                            .or_else(struct_match)
                            .or_else(param_match)
                            .or_else(|| candidates.into_iter().next())
                            .unwrap_or_default();
                    }
                }

                if type_name.is_empty() {
                    format!("glass_get_{}({})", field, obj)
                } else {
                    let variant_name = self
                        .types
                        .types
                        .get(&type_name)
                        .and_then(|info| {
                            info.variants
                                .iter()
                                .find(|v| v.fields.iter().any(|f| f.name == *field))
                                .map(|v| v.name.clone())
                        })
                        .unwrap_or_else(|| type_name.clone());
                    format!("glass_{}_{}_{} [{}]", type_name, variant_name, field, obj)
                }
            }

            Expr::MethodCall {
                object,
                method,
                args,
            } => {
                // Check if this is a qualified module call (module.function)
                if let Expr::Var(module_name) = &object.node {
                    let qualified_name = format!("{}.{}", module_name, method);
                    let ext_info = self
                        .externals
                        .get(&qualified_name)
                        .or_else(|| {
                            if self.fn_defs.contains_key(method.as_str())
                                || self.fn_defs.contains_key(qualified_name.as_str())
                            {
                                None
                            } else {
                                self.externals.get(method.as_str())
                            }
                        })
                        .cloned();
                    if let Some(ext) = ext_info {
                        let args_str: Vec<String> =
                            args.iter().map(|a| self.gen_spanned_expr(&a)).collect();
                        if ext.module == "glass" {
                            return self.gen_intrinsic_call(
                                &ext.jass_name,
                                args,
                                &args_str,
                                Some(object.span),
                            );
                        }
                        return format!("{}({})", ext.jass_name, args_str.join(", "));
                    }

                    if let Some(prefixed) = self.extend_methods.get(method.as_str()).cloned() {
                        let obj = self.gen_spanned_expr(&object);
                        let mut all_args = vec![obj];
                        for a in args {
                            all_args.push(self.gen_spanned_expr(&a));
                        }
                        return format!("glass_{}({})", prefixed, all_args.join(", "));
                    }

                    if self.fn_defs.contains_key(method.as_str()) {
                        let args_str: Vec<String> =
                            args.iter().map(|a| self.gen_spanned_expr(&a)).collect();
                        return format!("glass_{}({})", method, args_str.join(", "));
                    }

                    if self.mono_needed.contains(method.as_str()) {
                        // Collect concrete argument types from type_map
                        let concrete_types: Vec<Type> = args
                            .iter()
                            .map(|a| self.lookup_full_type(a.span).unwrap_or(Type::int()))
                            .collect();
                        let mangled = Self::mangle_types(&concrete_types);
                        let mono_name = format!("glass_{}_{}", method, mangled);

                        // Generate specialized copy if not already done
                        if !self.mono_generated.contains(&mono_name) {
                            self.mono_generated.insert(mono_name.clone());
                            let subst = self.build_mono_subst(method, &concrete_types);
                            self.gen_mono_function(method, &mono_name, subst);
                        }

                        let args_str: Vec<String> =
                            args.iter().map(|a| self.gen_spanned_expr(&a)).collect();
                        return format!("{}({})", mono_name, args_str.join(", "));
                    }

                    let args_str: Vec<String> =
                        args.iter().map(|a| self.gen_spanned_expr(&a)).collect();
                    return format!("glass_{}({})", method, args_str.join(", "));
                }

                let resolved = self.extend_methods.get(method.as_str()).cloned();
                let name = resolved.as_deref().unwrap_or(method.as_str());
                let obj = self.gen_spanned_expr(&object);
                let mut all_args = vec![obj];
                for a in args {
                    all_args.push(self.gen_spanned_expr(&a));
                }
                format!("glass_{}({})", name, all_args.join(", "))
            }

            Expr::Let {
                value,
                body,
                pattern,
                ..
            } => {
                let val = self.gen_spanned_expr(&value);
                self.gen_let_pattern_binding(&pattern.node, &val, &value.node);
                self.gen_spanned_expr(&body)
            }

            Expr::Case { subject, arms } => {
                let subj = self.gen_spanned_expr(&subject);
                let has_compound_arm = arms.iter().any(|a| {
                    matches!(
                        &a.body.node,
                        Expr::List(_)
                            | Expr::ListCons { .. }
                            | Expr::Constructor { .. }
                            | Expr::Tuple(_)
                    )
                });
                let has_integer_var_arm = arms.iter().any(|a| {
                    if let Expr::Var(ref vname) = a.body.node {
                        self.local_var_jass_types
                            .get(vname)
                            .is_some_and(|jt| jt == "integer")
                    } else {
                        false
                    }
                });
                let has_str_arm = arms.iter().any(|a| self.expr_is_string(&a.body.node));
                let case_jass_type = arms
                    .first()
                    .and_then(|arm| self.lookup_full_type(arm.body.span))
                    .map(|ty| {
                        let jt = self.type_to_jass_from_type(&ty);
                        if has_str_arm {
                            return "string".to_string();
                        }
                        if has_compound_arm || has_integer_var_arm {
                            return "integer".to_string();
                        }
                        if jt == "integer" && arms.iter().any(|a| self.expr_has_float(&a.body.node))
                        {
                            return "real".to_string();
                        }
                        if let Some(arm) = arms.first() {
                            if jt == "string"
                                && matches!(
                                    &arm.body.node,
                                    Expr::Call { .. }
                                        | Expr::Constructor { .. }
                                        | Expr::List(_)
                                        | Expr::ListCons { .. }
                                )
                            {
                                return "integer".to_string();
                            }
                        }
                        jt
                    })
                    .unwrap_or_else(|| self.infer_case_jass_type(arms));
                let result_var = self.fresh_temp_typed(&case_jass_type);

                let subject_type_name = self
                    .lookup_full_type(subject.span)
                    .and_then(|ty| self.resolve_type_name_from_app(&ty))
                    .or_else(|| {
                        for arm in arms.iter() {
                            if let Pattern::Constructor { name: pname, .. }
                            | Pattern::ConstructorNamed { name: pname, .. } = &arm.pattern.node
                            {
                                if let Some((ti, _)) = self.resolve_variant(pname) {
                                    return Some(ti.name.clone());
                                }
                            }
                        }
                        None
                    });

                let subj = if subject_type_name.as_deref() == Some("Bool")
                    && subj.contains("glass_dispatch_")
                {
                    format!("glass_i2b({})", subj)
                } else {
                    subj
                };

                let new_list_type = self.extract_list_elem_type_from_subject(subject);
                let prev_list_type = match new_list_type {
                    Some(et) => self.current_list_elem_type.replace(et),
                    None => None,
                };
                let new_tuple_types = self.extract_tuple_field_types_from_subject(subject);
                let prev_tuple_types = match new_tuple_types {
                    Some(tt) => self.current_tuple_field_types.replace(tt),
                    None => None,
                };

                let prev_case_type = self.current_case_type_name.take();
                self.current_case_type_name = subject_type_name.clone();

                for (i, arm) in arms.iter().enumerate() {
                    let condition = self.gen_pattern_condition_typed(
                        &arm.pattern.node,
                        &subj,
                        subject_type_name.as_deref(),
                    );
                    let guard = arm
                        .guard
                        .as_ref()
                        .map(|g| format!(" and ({})", self.gen_spanned_expr(&g)))
                        .unwrap_or_default();

                    if i == 0 {
                        self.emit(&format!("if ({}{}) then", condition, guard));
                    } else if condition == "true" {
                        self.emit("else");
                    } else {
                        self.emit(&format!("elseif ({}{}) then", condition, guard));
                    }

                    self.indent += 1;
                    self.gen_pattern_bindings(&arm.pattern.node, &subj);
                    let val = self.gen_spanned_expr(&arm.body);
                    self.emit(&format!("set {} = {}", result_var, val));
                    self.indent -= 1;
                }
                self.emit("endif");
                self.current_case_type_name = prev_case_type;
                if let Some(prev) = prev_list_type {
                    self.current_list_elem_type = Some(prev);
                }
                if let Some(prev) = prev_tuple_types {
                    self.current_tuple_field_types = Some(prev);
                } else {
                    self.current_tuple_field_types = None;
                }
                result_var
            }

            Expr::Block(exprs) => {
                let mut last = String::from("null");
                for expr in exprs {
                    last = self.gen_spanned_expr(&expr);
                }
                last
            }

            Expr::Tuple(elems) => {
                // Tuples compile to SoA-allocated records with positional fields _0, _1, ...
                let shape: Vec<String> = elems
                    .iter()
                    .map(|e| self.lookup_type(e.span).to_string())
                    .collect();
                let tuple_type = crate::types::TypeRegistry::tuple_type_name(&shape);

                let arg_strs: Vec<String> =
                    elems.iter().map(|e| self.gen_spanned_expr(&e)).collect();

                format!(
                    "glass_new_{}_{}({})",
                    tuple_type,
                    tuple_type,
                    arg_strs.join(", ")
                )
            }

            Expr::List(elems) => {
                if elems.is_empty() {
                    // nil = -1
                    "-1".to_string()
                } else {
                    let raw_type = self.lookup_full_type(elems[0].span);
                    let elem_type = raw_type
                        .as_ref()
                        .filter(|ty| !matches!(ty, Type::Var(_)))
                        .map(|ty| ty.to_jass().to_string())
                        .filter(|jt| self.types.list_types.contains(jt.as_str()))
                        .or_else(|| self.current_list_elem_type.clone())
                        .or_else(|| {
                            if let Expr::Var(name) = &elems[0].node {
                                self.var_list_elem_types
                                    .get(name)
                                    .cloned()
                                    .or_else(|| self.var_to_list_elem_type(name))
                            } else {
                                None
                            }
                        })
                        .unwrap_or_else(|| self.lookup_type(elems[0].span).to_string());
                    let lt = crate::types::TypeRegistry::list_type_name(&elem_type);

                    let mut result = "-1".to_string();
                    for elem in elems.iter().rev() {
                        let val = self.gen_spanned_expr(&elem);
                        result = format!("glass_{}_cons({}, {})", lt, val, result);
                    }
                    result
                }
            }

            Expr::ListCons { head, tail } => {
                let h = self.gen_spanned_expr(&head);
                let t = self.gen_spanned_expr(&tail);
                let head_type = self.lookup_full_type(head.span);
                let elem_type = head_type
                    .as_ref()
                    .filter(|ty| !matches!(ty, Type::Var(_)))
                    .map(|ty| ty.to_jass().to_string())
                    .filter(|jt| self.types.list_types.contains(jt.as_str()))
                    .or_else(|| self.current_list_elem_type.clone())
                    .or_else(|| {
                        if let Expr::Var(name) = &head.node {
                            self.var_list_elem_types
                                .get(name)
                                .cloned()
                                .or_else(|| self.var_to_list_elem_type(name))
                        } else {
                            None
                        }
                    })
                    .or_else(|| self.infer_list_elem_from_tail(tail))
                    .unwrap_or_else(|| "integer".to_string());
                let lt = crate::types::TypeRegistry::list_type_name(&elem_type);
                format!("glass_{}_cons({}, {})", lt, h, t)
            }

            Expr::Pipe { left, right } => {
                let l = self.gen_spanned_expr(&left);
                // Pipe: a |> f(b, _) → f(b, a), a |> f(b) → f(a, b), a |> f → f(a)
                match &right.node {
                    Expr::Call { function, args } => {
                        let func_name = match &function.node {
                            Expr::Var(name) => format!("glass_{}", name),
                            _ => self.gen_spanned_expr(&function),
                        };
                        // Check if any arg is `_` (capture/placeholder)
                        let has_placeholder = args
                            .iter()
                            .any(|a| matches!(&a.node, Expr::Var(n) if n == "_"));
                        let all_args: Vec<String> = if has_placeholder {
                            // Replace _ with piped value
                            args.iter()
                                .map(|a| {
                                    if matches!(&a.node, Expr::Var(n) if n == "_") {
                                        l.clone()
                                    } else {
                                        self.gen_spanned_expr(&a)
                                    }
                                })
                                .collect()
                        } else {
                            // No placeholder: insert as first arg
                            let mut all = vec![l];
                            for a in args {
                                all.push(self.gen_spanned_expr(&a));
                            }
                            all
                        };
                        format!("{}({})", func_name, all_args.join(", "))
                    }
                    Expr::Var(name) => {
                        format!("glass_{}({})", name, l)
                    }
                    // x |> module.func → glass_func(x)
                    // x |> module.func(a, b) → glass_func(x, a, b)
                    // x |> module.func(a, _) → glass_func(a, x)
                    Expr::MethodCall {
                        object,
                        method,
                        args,
                    } => {
                        // Check for external
                        let ext_info = if let Expr::Var(module_name) = &object.node {
                            let qualified = format!("{}.{}", module_name, method);
                            self.externals
                                .get(&qualified)
                                .or_else(|| self.externals.get(method.as_str()))
                                .cloned()
                        } else {
                            None
                        };
                        let func_name = match &ext_info {
                            Some(ext) => ext.jass_name.clone(),
                            None => format!("glass_{}", method),
                        };
                        if args.is_empty() {
                            format!("{}({})", func_name, l)
                        } else {
                            let has_placeholder = args
                                .iter()
                                .any(|a| matches!(&a.node, Expr::Var(n) if n == "_"));
                            let all_args: Vec<String> = if has_placeholder {
                                args.iter()
                                    .map(|a| {
                                        if matches!(&a.node, Expr::Var(n) if n == "_") {
                                            l.clone()
                                        } else {
                                            self.gen_spanned_expr(&a)
                                        }
                                    })
                                    .collect()
                            } else {
                                let mut all = vec![l];
                                for a in args {
                                    all.push(self.gen_spanned_expr(&a));
                                }
                                all
                            };
                            format!("{}({})", func_name, all_args.join(", "))
                        }
                    }
                    Expr::FieldAccess { object, field } => {
                        if let Expr::Var(module_name) = &object.node {
                            let qualified = format!("{}.{}", module_name, field);
                            if let Some(ext) = self
                                .externals
                                .get(&qualified)
                                .or_else(|| {
                                    if self.fn_defs.contains_key(field.as_str())
                                        || self.fn_defs.contains_key(qualified.as_str())
                                    {
                                        None
                                    } else {
                                        self.externals.get(field.as_str())
                                    }
                                })
                                .cloned()
                            {
                                return format!("{}({})", ext.jass_name, l);
                            }
                        }
                        let func_name = if self.fn_defs.contains_key(field.as_str()) {
                            format!("glass_{}", field)
                        } else {
                            self.gen_spanned_expr(&right)
                        };
                        format!("{}({})", func_name, l)
                    }
                    _ => {
                        let r = self.gen_spanned_expr(&right);
                        format!("{}({})", r, l)
                    }
                }
            }

            Expr::Constructor { name, args } => {
                if args.is_empty()
                    && !name.contains("::")
                    && let Some(value) = self.const_values.get(name.as_str())
                {
                    return value.clone();
                }

                let mono_tname = self
                    .resolve_mono_ctor_type_from_span(name)
                    .or_else(|| self.resolve_mono_ctor_type(name, args));

                let variant_info = match &mono_tname {
                    Some(mn) => self
                        .types
                        .get_variant_of_type(Self::full_bare_name(name), mn)
                        .map(|(ti, v)| (ti.name.clone(), v.name.clone())),
                    None => self
                        .resolve_variant(name)
                        .map(|(ti, v)| (ti.name.clone(), v.name.clone())),
                };

                match variant_info {
                    Some((tname, vname)) => {
                        let arg_strs: Vec<String> =
                            args.iter()
                                .map(|a| {
                                    let e = match a {
                                        ConstructorArg::Positional(e)
                                        | ConstructorArg::Named(_, e) => e,
                                    };
                                    self.gen_spanned_expr(&e)
                                })
                                .collect();
                        if arg_strs.is_empty() {
                            format!("glass_new_{}_{}()", tname, vname)
                        } else {
                            format!("glass_new_{}_{}({})", tname, vname, arg_strs.join(", "))
                        }
                    }
                    None => {
                        format!("0 /* unknown constructor {} */", name)
                    }
                }
            }

            Expr::RecordUpdate {
                name,
                base,
                updates,
            } => {
                let bare_record_name = name.rsplit('.').next().unwrap_or(name);
                // Clone type info to release borrow on self
                let record_info: Option<(String, Vec<(String, String)>)> =
                    self.types.types.get(bare_record_name).and_then(|info| {
                        if info.is_enum {
                            return None;
                        }
                        let v = info.variants.first()?;
                        let fields: Vec<(String, String)> = v
                            .fields
                            .iter()
                            .map(|f| (f.name.clone(), f.jass_type.clone()))
                            .collect();
                        Some((v.name.clone(), fields))
                    });

                match record_info {
                    Some((variant_name, fields)) => {
                        let base_val = self.gen_spanned_expr(&base);
                        let tmp = self.fresh_temp();
                        self.emit(&format!("set {} = glass_{}_alloc()", tmp, name));
                        for (fname, _ftype) in &fields {
                            let updated = updates.iter().find(|(n, _)| n == fname);
                            match updated {
                                Some((_, val)) => {
                                    let v = self.gen_spanned_expr(&val);
                                    self.emit(&format!(
                                        "set glass_{}_{}_{} [{}] = {}",
                                        name, variant_name, fname, tmp, v
                                    ));
                                }
                                None => {
                                    self.emit(&format!(
                                        "set glass_{}_{}_{} [{}] = glass_{}_{}_{} [{}]",
                                        name,
                                        variant_name,
                                        fname,
                                        tmp,
                                        name,
                                        variant_name,
                                        fname,
                                        base_val
                                    ));
                                }
                            }
                        }
                        tmp
                    }
                    None => {
                        format!("0 /* TODO: update {} */", name)
                    }
                }
            }

            Expr::Lambda { .. } => {
                // Get pre-collected lambda info by counter
                let lambda_id = self.lambda_counter;
                self.lambda_counter += 1;

                // Check if this lambda has captures
                let capture_info: Option<Vec<String>> = self
                    .lambdas
                    .get(lambda_id)
                    .filter(|l| !l.captures.is_empty())
                    .map(|l| l.captures.iter().map(|c| c.name.clone()).collect());

                match capture_info {
                    Some(capture_names) => {
                        let tmp = self.fresh_temp();
                        self.emit(&format!("set {} = glass_clos{}_alloc()", tmp, lambda_id));
                        for name in &capture_names {
                            self.emit(&format!(
                                "set glass_clos{}_{}[{}] = {}",
                                lambda_id,
                                name,
                                tmp,
                                safe_jass_name(name)
                            ));
                        }
                        // Encode as (type_tag * 8192 + instance_id)
                        format!("({} * 8192 + {})", lambda_id, tmp)
                    }
                    None => {
                        // Non-capturing: just encode the lambda ID (instance_id = 0)
                        format!("({} * 8192)", lambda_id)
                    }
                }
            }

            Expr::Clone(inner) => self.gen_spanned_expr(&inner),

            Expr::Todo(msg) => {
                let msg_str = msg
                    .as_ref()
                    .map(|s| format!("\"{}\"", s))
                    .unwrap_or_else(|| "\"todo\"".to_string());
                format!("glass_panic({})", msg_str)
            }

            // TCO nodes should not appear in gen_expr — they are handled by gen_tco_body
            Expr::TcoLoop { .. } | Expr::TcoContinue { .. } => "0".to_string(),
        }
    }

    pub(super) fn expr_has_float(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Float(_) => true,
            Expr::BinOp { left, right, .. } => {
                self.expr_has_float(&left.node) || self.expr_has_float(&right.node)
            }
            Expr::UnaryOp { operand, .. } => self.expr_has_float(&operand.node),
            Expr::Let { body, .. } => self.expr_has_float(&body.node),
            Expr::Block(exprs) => exprs.last().is_some_and(|e| self.expr_has_float(&e.node)),
            Expr::Case { arms, .. } => arms.iter().any(|a| self.expr_has_float(&a.body.node)),
            Expr::Var(name) => self
                .local_var_jass_types
                .get(name)
                .is_some_and(|jt| jt == "real"),
            _ => false,
        }
    }

    pub(super) fn expr_is_string(&self, expr: &Expr) -> bool {
        match expr {
            Expr::String(_) => true,
            Expr::BinOp { op, .. } if *op == BinOp::StringConcat => true,
            Expr::Let { body, .. } => self.expr_is_string(&body.node),
            Expr::Block(exprs) => exprs.last().is_some_and(|e| self.expr_is_string(&e.node)),
            Expr::Case { arms, .. } => arms.iter().any(|a| self.expr_is_string(&a.body.node)),
            Expr::Var(name) => {
                self.const_values
                    .get(name.as_str())
                    .is_some_and(|v| v.starts_with('"'))
                    || self
                        .local_var_jass_types
                        .get(name)
                        .is_some_and(|jt| jt == "string")
            }
            _ => false,
        }
    }

    pub(super) fn infer_case_jass_type(&self, arms: &[CaseArm]) -> String {
        if arms.iter().any(|a| self.expr_is_string(&a.body.node)) {
            return "string".to_string();
        }
        if arms.iter().any(|a| self.expr_has_float(&a.body.node)) {
            return "real".to_string();
        }
        for arm in arms {
            if let Some(ty) = self.lookup_full_type(arm.body.span) {
                let jt = self.type_to_jass_from_type(&ty);
                if jt != "integer" {
                    return jt;
                }
            }
        }
        "integer".to_string()
    }
}

pub(super) fn const_fold_binop(op: &BinOp, left: &Expr, right: &Expr) -> Option<String> {
    match (op, left, right) {
        // Int arithmetic
        (BinOp::Add, Expr::Int(a), Expr::Int(b)) => Some(format!("{}", a + b)),
        (BinOp::Sub, Expr::Int(a), Expr::Int(b)) => Some(format!("{}", a - b)),
        (BinOp::Mul, Expr::Int(a), Expr::Int(b)) => Some(format!("{}", a * b)),
        (BinOp::Div, Expr::Int(a), Expr::Int(b)) if *b != 0 => Some(format!("{}", a / b)),
        (BinOp::Mod, Expr::Int(a), Expr::Int(b)) if *b != 0 => Some(format!("{}", a % b)),

        // Float arithmetic
        (BinOp::Add, Expr::Float(a), Expr::Float(b)) => Some(format_float(a + b)),
        (BinOp::Sub, Expr::Float(a), Expr::Float(b)) => Some(format_float(a - b)),
        (BinOp::Mul, Expr::Float(a), Expr::Float(b)) => Some(format_float(a * b)),

        // String concatenation
        (BinOp::StringConcat, Expr::String(a), Expr::String(b)) => Some(format!("\"{}{}\"", a, b)),

        // Bool comparisons on ints
        (BinOp::Eq, Expr::Int(a), Expr::Int(b)) => {
            Some(if a == b { "true" } else { "false" }.into())
        }
        (BinOp::NotEq, Expr::Int(a), Expr::Int(b)) => {
            Some(if a == b { "false" } else { "true" }.into())
        }
        (BinOp::Less, Expr::Int(a), Expr::Int(b)) => {
            Some(if a < b { "true" } else { "false" }.into())
        }
        (BinOp::Greater, Expr::Int(a), Expr::Int(b)) => {
            Some(if a > b { "true" } else { "false" }.into())
        }

        _ => None,
    }
}
