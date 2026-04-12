use std::collections::HashMap;

use crate::ast::*;
use crate::type_repr::Type;
use crate::types::{TypeInfo, VariantInfo};

use crate::types::TypeRegistry;

use super::safe_jass_name;

impl super::JassCodegen {
    pub(super) fn bare_ctor_name(name: &str) -> &str {
        name.rsplit("::").next().unwrap_or(name)
    }

    pub(super) fn full_bare_name(name: &str) -> &str {
        let after_colons = name.rsplit("::").next().unwrap_or(name);
        after_colons.rsplit('.').next().unwrap_or(after_colons)
    }

    pub(super) fn type_hint_from_ctor_name(name: &str) -> Option<&str> {
        if name.contains("::") {
            let before = name.split("::").next().unwrap_or("");
            let type_part = before.rsplit('.').next().unwrap_or(before);
            if type_part.is_empty() {
                None
            } else {
                Some(type_part)
            }
        } else {
            None
        }
    }

    pub(super) fn extract_tuple_field_types_from_subject(
        &self,
        subject: &Spanned<Expr>,
    ) -> Option<Vec<String>> {
        let ty = self.lookup_full_type(subject.span)?;
        match ty {
            Type::App(con, args) => {
                if let Type::Con(name) = *con
                    && name == "List"
                    && let Some(elem_ty) = args.into_iter().next()
                {
                    return self.tuple_field_types_from_type(&elem_ty);
                }
                None
            }
            Type::Tuple(elems) => Some(elems.iter().map(|e| e.to_jass().to_string()).collect()),
            _ => None,
        }
    }

    pub(super) fn tuple_field_types_from_type(&self, ty: &Type) -> Option<Vec<String>> {
        match ty {
            Type::Tuple(elems) => Some(elems.iter().map(|e| e.to_jass().to_string()).collect()),
            Type::App(con, args) => {
                if let Type::Con(name) = con.as_ref()
                    && name.starts_with("Tuple")
                {
                    return Some(args.iter().map(|a| a.to_jass().to_string()).collect());
                }
                None
            }
            _ => None,
        }
    }

    pub(super) fn lookup_tuple_field_types(&self, arity: usize) -> Vec<String> {
        if let Some(ref types) = self.current_tuple_field_types
            && types.len() == arity
        {
            return types.clone();
        }
        let prefix = format!("Tuple{}_", arity);
        let candidates: Vec<&TypeInfo> = self
            .types
            .types
            .values()
            .filter(|ti| {
                ti.name.starts_with(&prefix)
                    && ti.variants.first().is_some_and(|v| v.fields.len() == arity)
            })
            .collect();
        if let [candidate] = candidates.as_slice() {
            candidate
                .variants
                .first()
                .map(|v| v.fields.iter().map(|f| f.jass_type.clone()).collect())
                .unwrap_or_else(|| vec!["integer".to_string(); arity])
        } else {
            vec!["integer".to_string(); arity]
        }
    }

    pub(super) fn resolve_variant<'a>(
        &'a self,
        name: &str,
    ) -> Option<(&'a TypeInfo, &'a VariantInfo)> {
        let bare = Self::full_bare_name(name);
        match Self::type_hint_from_ctor_name(name) {
            Some(tn) => self
                .types
                .get_variant_of_type(bare, tn)
                .or_else(|| self.types.get_variant(bare)),
            None => self.types.get_variant(bare),
        }
    }

    pub(super) fn resolve_variant_in_case<'a>(
        &'a self,
        name: &str,
    ) -> Option<(&'a TypeInfo, &'a VariantInfo)> {
        let bare = Self::full_bare_name(name);
        if let Some(tn) = &self.current_case_type_name
            && let Some(result) = self.types.get_variant_of_type(bare, tn)
        {
            return Some(result);
        }
        self.resolve_variant(name)
    }

    pub(super) fn gen_pattern_condition_typed(
        &self,
        pattern: &Pattern,
        subject: &str,
        type_name: Option<&str>,
    ) -> String {
        match pattern {
            Pattern::Bool(true) => format!("({} == true)", subject),
            Pattern::Bool(false) => format!("({} == false)", subject),
            Pattern::Int(n) => format!("({} == {})", subject, n),
            Pattern::Rawcode(s) => format!("({} == '{}')", subject, s),
            Pattern::String(s) => format!("({} == \"{}\")", subject, s),
            Pattern::Constructor { name, args } => {
                let bare = Self::bare_ctor_name(name);
                if args.is_empty() {
                    let const_key = bare.rsplit('.').next().unwrap_or(bare);
                    if let Some(value) = self.const_values.get(const_key) {
                        format!("({} == {})", subject, value)
                    } else {
                        let variant_info = match type_name {
                            Some(tn) => self
                                .types
                                .get_variant_of_type(bare, tn)
                                .or_else(|| self.resolve_variant(name)),
                            None => self.resolve_variant(name),
                        };
                        let qualified = variant_info
                            .map(|(ti, _)| format!("{}_{}", ti.name, bare))
                            .unwrap_or_else(|| bare.to_string());
                        format!("({} == glass_TAG_{})", subject, qualified)
                    }
                } else {
                    let tag_access = match type_name {
                        Some(tn) => format!("glass_{}_tag[{}]", tn, subject),
                        None => format!("glass_tag({})", subject),
                    };
                    let qualified = match type_name {
                        Some(tn) => self
                            .types
                            .get_variant_of_type(bare, tn)
                            .or_else(|| self.resolve_variant(name)),
                        None => self.resolve_variant(name),
                    }
                    .map(|(ti, _)| format!("{}_{}", ti.name, bare))
                    .unwrap_or_else(|| bare.to_string());
                    format!("({} == glass_TAG_{})", tag_access, qualified)
                }
            }
            Pattern::ConstructorNamed { name, .. } => {
                let bare = Self::bare_ctor_name(name);
                let tag_access = match type_name {
                    Some(tn) => format!("glass_{}_tag[{}]", tn, subject),
                    None => format!("glass_tag({})", subject),
                };
                let qualified = match type_name {
                    Some(tn) => self
                        .types
                        .get_variant_of_type(bare, tn)
                        .or_else(|| self.resolve_variant(name)),
                    None => self.resolve_variant(name),
                }
                .map(|(ti, _)| format!("{}_{}", ti.name, bare))
                .unwrap_or_else(|| bare.to_string());
                format!("({} == glass_TAG_{})", tag_access, qualified)
            }
            Pattern::Or(alternatives) => {
                let conditions: Vec<String> = alternatives
                    .iter()
                    .map(|alt| self.gen_pattern_condition_typed(&alt.node, subject, type_name))
                    .collect();
                format!("({})", conditions.join(" or "))
            }
            Pattern::As { pattern, .. } => {
                self.gen_pattern_condition_typed(&pattern.node, subject, type_name)
            }
            // Empty list pattern: [] → subject == -1
            Pattern::List(elems) if elems.is_empty() => {
                format!("({} == -1)", subject)
            }
            // List cons pattern: [h | t] → subject != -1
            Pattern::ListCons { .. } => {
                format!("({} != -1)", subject)
            }
            // Tuple, Discard, Var, and other patterns always match
            _ => "true".to_string(),
        }
    }

    pub(super) fn gen_let_pattern_binding(
        &mut self,
        pattern: &Pattern,
        val: &str,
        value_expr: &Expr,
    ) {
        match pattern {
            Pattern::Var(name) => {
                self.emit(&format!("set {} = {}", safe_jass_name(name), val));
            }
            Pattern::Discard => {
                if matches!(value_expr, Expr::Call { .. }) {
                    self.emit(&format!("call {}", val));
                }
            }
            Pattern::Tuple(elems) => {
                let field_types = self.lookup_tuple_field_types(elems.len());
                let shape: Vec<String> = if field_types.iter().any(|t| t != "integer") {
                    field_types
                } else {
                    elems.iter().map(|_| "integer".to_string()).collect()
                };
                let tuple_type = crate::types::TypeRegistry::tuple_type_name(&shape);

                let tmp = if val.starts_with("glass_") {
                    val.to_string()
                } else {
                    let t = self.fresh_temp();
                    self.emit(&format!("set {} = {}", t, val));
                    t
                };

                for (i, elem) in elems.iter().enumerate() {
                    let field_val = format!("glass_{}_{}__{} [{}]", tuple_type, tuple_type, i, tmp);
                    self.gen_let_pattern_binding(&elem.node, &field_val, value_expr);
                }
            }
            Pattern::Constructor { args, .. } => {
                for (i, arg) in args.iter().enumerate() {
                    let field = format!("glass_field_{}({})", i, val);
                    self.gen_let_pattern_binding(&arg.node, &field, value_expr);
                }
            }
            Pattern::ConstructorNamed { name, fields, .. } => {
                let bare = Self::bare_ctor_name(name);
                let type_name = self
                    .resolve_variant(name)
                    .map(|(ti, _)| ti.name.clone())
                    .unwrap_or_default();
                let prefix = if type_name.is_empty() {
                    bare.to_string()
                } else {
                    format!("{}_{}", type_name, bare)
                };
                let tmp = if val.starts_with("glass_") {
                    val.to_string()
                } else {
                    let t = self.fresh_temp();
                    self.emit(&format!("set {} = {}", t, val));
                    t
                };
                for fp in fields {
                    let field = format!("glass_{}_{}[{}]", prefix, fp.field_name, tmp);
                    if let Some(nested) = fp.pattern.as_ref().filter(|_| fp.has_nested_pattern()) {
                        self.gen_let_pattern_binding(&nested.node, &field, value_expr);
                    } else {
                        let var = safe_jass_name(fp.binding_name());
                        self.emit(&format!("set {} = {}", var, field));
                    }
                }
            }
            Pattern::As {
                pattern: inner,
                name,
            } => {
                let safe = safe_jass_name(name);
                self.emit(&format!("set {} = {}", safe, val));
                self.gen_let_pattern_binding(&inner.node, &safe, value_expr);
            }
            _ => {
                let tmp = self.fresh_temp();
                self.emit(&format!("set {} = {}", tmp, val));
            }
        }
    }

    pub(super) fn gen_pattern_bindings(&mut self, pattern: &Pattern, subject: &str) {
        match pattern {
            Pattern::Var(name) => {
                self.emit(&format!("set {} = {}", safe_jass_name(name), subject));
            }
            Pattern::Constructor { name, args } => {
                let bare = Self::bare_ctor_name(name);
                let variant_info = self.resolve_variant_in_case(name).map(|(ti, v)| {
                    let field_names: Vec<String> =
                        v.fields.iter().map(|f| f.name.clone()).collect();
                    (ti.name.clone(), field_names)
                });
                for (i, arg) in args.iter().enumerate() {
                    let field = match &variant_info {
                        Some((tn, field_names)) => {
                            let fname = field_names
                                .get(i)
                                .cloned()
                                .unwrap_or_else(|| format!("_{}", i));
                            format!("glass_{}_{}_{} [{}]", tn, bare, fname, subject)
                        }
                        None => format!("glass_field_{}({})", i, subject),
                    };
                    self.gen_pattern_bindings(&arg.node, &field);
                }
            }
            Pattern::ConstructorNamed { name, fields, .. } => {
                let bare = Self::bare_ctor_name(name);
                let type_name = self
                    .resolve_variant_in_case(name)
                    .map(|(ti, _)| ti.name.clone())
                    .unwrap_or_default();
                let prefix = if type_name.is_empty() {
                    bare.to_string()
                } else {
                    format!("{}_{}", type_name, bare)
                };
                for fp in fields {
                    let field = format!("glass_{}_{}[{}]", prefix, fp.field_name, subject);
                    if let Some(nested) = fp.pattern.as_ref().filter(|_| fp.has_nested_pattern()) {
                        let tmp = self.fresh_temp();
                        self.emit(&format!("set {} = {}", tmp, field));
                        self.gen_pattern_bindings(&nested.node, &tmp);
                    } else {
                        let var = safe_jass_name(fp.binding_name());
                        self.emit(&format!("set {} = {}", var, field));
                    }
                }
            }
            Pattern::Or(alternatives) => {
                // Bind from the first alternative (all must bind same vars)
                if let Some(first) = alternatives.first() {
                    self.gen_pattern_bindings(&first.node, subject);
                }
            }
            Pattern::ListCons { head, tail } => {
                let list_type = self
                    .current_list_elem_type
                    .as_ref()
                    .map(|et| TypeRegistry::list_type_name(et))
                    .unwrap_or_else(|| "List_integer".to_string());
                let head_expr = format!("glass_{}_head[{}]", list_type, subject);
                let tail_expr = format!("glass_{}_tail[{}]", list_type, subject);
                self.gen_pattern_bindings(&head.node, &head_expr);
                self.gen_pattern_bindings(&tail.node, &tail_expr);
            }
            Pattern::Tuple(elems) => {
                let field_types = self.lookup_tuple_field_types(elems.len());
                let tuple_type = crate::types::TypeRegistry::tuple_type_name(&field_types);
                for (i, elem) in elems.iter().enumerate() {
                    let field_val =
                        format!("glass_{}_{}__{} [{}]", tuple_type, tuple_type, i, subject);
                    self.gen_pattern_bindings(&elem.node, &field_val);
                }
            }
            Pattern::As { pattern, name } => {
                self.emit(&format!("set {} = {}", safe_jass_name(name), subject));
                self.gen_pattern_bindings(&pattern.node, subject);
            }
            _ => {}
        }
    }

    // === Locals collection ===

    pub(super) fn collect_locals(&mut self, expr: &Expr, locals: &mut Vec<(String, String)>) {
        match expr {
            Expr::Let {
                pattern,
                value,
                body,
                type_annotation,
                ..
            } => {
                match &pattern.node {
                    Pattern::Var(name) => {
                        let jass_type = match type_annotation {
                            Some(t) => {
                                let ann_type = self.type_to_jass(t);
                                if ann_type == "integer" {
                                    if let Expr::Var(ref vname) = value.node {
                                        self.local_var_jass_types
                                            .get(vname)
                                            .filter(|jt| *jt != "integer")
                                            .cloned()
                                            .unwrap_or(ann_type)
                                    } else {
                                        ann_type
                                    }
                                } else {
                                    ann_type
                                }
                            }
                            None => {
                                let lt = self.lookup_type(value.span);
                                if lt == "integer" && self.expr_has_float(&value.node) {
                                    "real".to_string()
                                } else if lt == "integer" {
                                    if let Expr::Var(ref vname) = value.node {
                                        self.local_var_jass_types
                                            .get(vname)
                                            .cloned()
                                            .unwrap_or_else(|| lt.to_string())
                                    } else {
                                        lt.to_string()
                                    }
                                } else {
                                    lt.to_string()
                                }
                            }
                        };
                        self.local_var_jass_types
                            .insert(name.clone(), jass_type.clone());
                        locals.push((name.clone(), jass_type));
                    }
                    Pattern::Tuple(elems) => {
                        self.collect_pattern_locals(&pattern.node, locals);
                        // Also recurse into each sub-element
                        for elem in elems {
                            self.collect_pattern_locals(&elem.node, locals);
                        }
                    }
                    _ => {
                        self.collect_pattern_locals(&pattern.node, locals);
                    }
                }
                self.collect_locals(&value.node, locals);
                self.collect_locals(&body.node, locals);
            }
            Expr::Case { subject, arms } => {
                self.collect_locals(&subject.node, locals);
                let new_list_type = self.extract_list_elem_type_from_subject(subject);
                let prev = match new_list_type {
                    Some(et) => self.current_list_elem_type.replace(et),
                    None => None,
                };
                let new_tuple_types = self.extract_tuple_field_types_from_subject(subject);
                let prev_tuple = match new_tuple_types {
                    Some(tt) => self.current_tuple_field_types.replace(tt),
                    None => None,
                };
                for arm in arms {
                    self.collect_pattern_locals(&arm.pattern.node, locals);
                    self.collect_locals(&arm.body.node, locals);
                }
                if let Some(prev_val) = prev {
                    self.current_list_elem_type = Some(prev_val);
                }
                if let Some(prev_val) = prev_tuple {
                    self.current_tuple_field_types = Some(prev_val);
                } else {
                    self.current_tuple_field_types = None;
                }
            }
            Expr::Block(exprs) => {
                for e in exprs {
                    self.collect_locals(&e.node, locals);
                }
            }
            Expr::RecordUpdate { base, updates, .. } => {
                self.collect_locals(&base.node, locals);
                for (_, val) in updates {
                    self.collect_locals(&val.node, locals);
                }
            }
            Expr::TcoLoop { body } | Expr::Lambda { body, .. } => {
                self.collect_locals(&body.node, locals);
            }
            Expr::TcoContinue { args } => {
                for (_, val) in args {
                    self.collect_locals(&val.node, locals);
                }
            }
            Expr::Call { function, args } => {
                self.collect_locals(&function.node, locals);
                for a in args {
                    self.collect_locals(&a.node, locals);
                }
            }
            Expr::BinOp { left, right, .. } | Expr::Pipe { left, right } => {
                self.collect_locals(&left.node, locals);
                self.collect_locals(&right.node, locals);
            }
            Expr::UnaryOp { operand, .. } | Expr::Clone(operand) => {
                self.collect_locals(&operand.node, locals);
            }
            Expr::ListCons { head, tail } => {
                self.collect_locals(&head.node, locals);
                self.collect_locals(&tail.node, locals);
            }
            Expr::Tuple(elems) | Expr::List(elems) => {
                for e in elems {
                    self.collect_locals(&e.node, locals);
                }
            }
            Expr::FieldAccess { object, .. } => {
                self.collect_locals(&object.node, locals);
            }
            Expr::MethodCall { object, args, .. } => {
                self.collect_locals(&object.node, locals);
                for a in args {
                    self.collect_locals(&a.node, locals);
                }
            }
            Expr::Constructor { args, .. } => {
                for a in args {
                    match a {
                        ConstructorArg::Positional(e) | ConstructorArg::Named(_, e) => {
                            self.collect_locals(&e.node, locals);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn collect_pattern_locals(
        &self,
        pattern: &Pattern,
        locals: &mut Vec<(String, String)>,
    ) {
        match pattern {
            Pattern::Var(name) if name != "_" => {
                locals.push((name.clone(), "integer".to_string()));
            }
            Pattern::Constructor { name, args } => {
                let field_types: Vec<String> = self
                    .resolve_variant(name)
                    .map(|(_, v)| v.fields.iter().map(|f| f.jass_type.clone()).collect())
                    .unwrap_or_default();
                for (i, arg) in args.iter().enumerate() {
                    if let Pattern::Var(vname) = &arg.node {
                        if vname != "_" {
                            let jass_type = field_types
                                .get(i)
                                .cloned()
                                .unwrap_or_else(|| "integer".to_string());
                            locals.push((vname.clone(), jass_type));
                        }
                    } else {
                        self.collect_pattern_locals(&arg.node, locals);
                    }
                }
            }
            Pattern::ConstructorNamed { name, fields, .. } => {
                let field_types: HashMap<String, String> = self
                    .resolve_variant(name)
                    .map(|(_, v)| {
                        v.fields
                            .iter()
                            .map(|f| (f.name.clone(), f.jass_type.clone()))
                            .collect()
                    })
                    .unwrap_or_default();
                for fp in fields {
                    if let Some(nested) = fp.pattern.as_ref().filter(|_| fp.has_nested_pattern()) {
                        self.collect_pattern_locals(&nested.node, locals);
                    } else {
                        let var = fp.binding_name();
                        let jass_type = field_types
                            .get(&fp.field_name)
                            .cloned()
                            .unwrap_or_else(|| "integer".to_string());
                        locals.push((var.to_string(), jass_type));
                    }
                }
            }
            Pattern::Or(alternatives) => {
                // All alternatives bind the same vars — use first
                if let Some(first) = alternatives.first() {
                    self.collect_pattern_locals(&first.node, locals);
                }
            }
            Pattern::As { pattern, name } => {
                locals.push((name.clone(), "integer".to_string()));
                self.collect_pattern_locals(&pattern.node, locals);
            }
            Pattern::Tuple(elems) => {
                let field_types = self.lookup_tuple_field_types(elems.len());
                for (i, e) in elems.iter().enumerate() {
                    if let Pattern::Var(name) = &e.node {
                        if name != "_" {
                            let jass_type = field_types
                                .get(i)
                                .cloned()
                                .unwrap_or_else(|| "integer".to_string());
                            locals.push((name.clone(), jass_type));
                        }
                    } else {
                        self.collect_pattern_locals(&e.node, locals);
                    }
                }
            }
            Pattern::ListCons { head, tail } => {
                if let Some(ref elem_type) = self.current_list_elem_type {
                    if let Pattern::Var(name) = &head.node {
                        if name != "_" {
                            locals.push((name.clone(), elem_type.clone()));
                        }
                    } else {
                        self.collect_pattern_locals(&head.node, locals);
                    }
                } else {
                    self.collect_pattern_locals(&head.node, locals);
                }
                self.collect_pattern_locals(&tail.node, locals);
            }
            _ => {}
        }
    }
}
