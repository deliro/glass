use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::type_repr::{Substitution, Type};
use crate::types::TypeRegistry;

impl super::JassCodegen {
    // === Type mapping ===

    /// Extract the innermost type name from a type expression.
    /// E.g. `List(PudgeState)` → `"PudgeState"`, `Int` → `"Int"`, `fn(...) -> ...` → None
    pub(super) fn extract_inner_type_name(ty: &TypeExpr) -> Option<String> {
        match ty {
            TypeExpr::Named { name, args } => {
                if args.is_empty() {
                    Some(name.clone())
                } else {
                    // For List(X), Option(X), etc. — extract X
                    args.iter().find_map(Self::extract_inner_type_name)
                }
            }
            _ => None,
        }
    }

    pub(super) fn type_to_jass(&self, ty: &TypeExpr) -> String {
        let TypeExpr::Named { name, .. } = ty else {
            return "integer".to_string();
        };
        Self::type_name_to_jass(name)
    }

    pub(super) fn type_name_to_jass(name: &str) -> String {
        match name {
            "Float" => "real".to_string(),
            "Bool" => "boolean".to_string(),
            "String" => "string".to_string(),
            "Unit" => "unit".to_string(),
            "Player" => "player".to_string(),
            "Timer" => "timer".to_string(),
            "Group" => "group".to_string(),
            "Trigger" => "trigger".to_string(),
            "Sfx" => "effect".to_string(),
            // Int, user types, Effect, closures, tuples → all integer
            _ => "integer".to_string(),
        }
    }

    pub(super) fn dispatch_fn_name(param_types: &[String]) -> String {
        let arity = param_types.len();
        if arity == 0 {
            return "glass_dispatch_0".to_string();
        }
        format!("glass_dispatch_{}_{}", arity, param_types.join("_"))
    }

    pub(super) fn type_to_jass_from_type(&self, ty: &Type) -> String {
        match ty {
            Type::Con(name) => Self::type_name_to_jass(name),
            Type::App(con, _) => match con.as_ref() {
                Type::Con(name) => Self::type_name_to_jass(name),
                _ => "integer".to_string(),
            },
            _ => "integer".to_string(),
        }
    }

    pub(super) fn gen_mono_function(
        &mut self,
        orig_name: &str,
        mono_name: &str,
        subst: Substitution,
    ) {
        let fdef = match self.fn_defs.get(orig_name).cloned() {
            Some(f) => f,
            None => return,
        };

        // Save current state
        let prev_subst = std::mem::replace(&mut self.mono_subst, subst);
        let prev_param_types = std::mem::take(&mut self.mono_param_types);
        let prev_output = std::mem::take(&mut self.output);
        let prev_indent = std::mem::replace(&mut self.indent, 0);
        let prev_temp = std::mem::replace(&mut self.temp_counter, 0);
        let prev_temp_types = std::mem::take(&mut self.temp_types);

        // Build param name → concrete type mapping
        for p in &fdef.params {
            let concrete_type = self.resolve_type_expr_to_type(&p.type_expr);
            self.mono_param_types.insert(p.name.clone(), concrete_type);
        }

        // Generate function header with concrete types
        let params: Vec<String> = fdef
            .params
            .iter()
            .map(|p| {
                let jass_type = self.type_to_jass_with_subst(&p.type_expr);
                format!("{} {}", jass_type, p.name)
            })
            .collect();

        let ret_type = fdef
            .return_type
            .as_ref()
            .map(|t| self.type_to_jass_with_subst(t))
            .unwrap_or_else(|| "nothing".to_string());

        // Collect user locals
        let mut locals = Vec::new();
        self.collect_locals(&fdef.body.node, &mut locals);

        // Generate body into buffer with fresh temp counter
        self.indent = 1;
        let result = self.gen_spanned_expr(&fdef.body);
        if ret_type != "nothing" {
            self.emit(&format!("return {}", result));
        }
        let body_buf = std::mem::take(&mut self.output);
        let temp_types = std::mem::take(&mut self.temp_types);

        // Build full function
        self.indent = 0;
        self.emit(&format!(
            "function {} takes {} returns {}",
            mono_name,
            if params.is_empty() {
                "nothing".to_string()
            } else {
                params.join(", ")
            },
            ret_type
        ));
        self.indent = 1;
        {
            let mut seen = std::collections::HashSet::new();
            for (name, jass_type) in &locals {
                if seen.insert(name.clone()) {
                    self.emit(&format!("local {} {}", jass_type, name));
                }
            }
        }
        for (i, jass_type) in temp_types.iter().enumerate() {
            self.emit(&format!("local {} glass_tmp_{}", jass_type, i));
        }
        self.output.push_str(&body_buf);
        self.indent = 0;
        self.emit("endfunction");

        // Capture the generated function, restore state
        let mono_output = std::mem::replace(&mut self.output, prev_output);
        self.indent = prev_indent;
        self.mono_subst = prev_subst;
        self.mono_param_types = prev_param_types;
        self.temp_counter = prev_temp;
        self.temp_types = prev_temp_types;

        // Append the mono function to the main output (at top level, before current position)
        // We prepend it to the output so it's defined before it's called
        self.output = format!("{}\n{}", mono_output, self.output);
    }

    pub(super) fn resolve_type_expr_to_type(&self, ty: &TypeExpr) -> Type {
        match ty {
            TypeExpr::Named { name, args } => {
                if name.chars().next().is_some_and(|c| c.is_lowercase()) {
                    // Type variable — resolve via mono_subst
                    for tvars in self.type_param_vars.values() {
                        if let Some(&var_id) = tvars.get(name.as_str())
                            && let Some(concrete) = self.mono_subst.0.get(&var_id)
                        {
                            return concrete.clone();
                        }
                    }
                    Type::Var(0) // unresolved
                } else if args.is_empty() {
                    Type::Con(name.clone())
                } else {
                    let resolved_args: Vec<Type> = args
                        .iter()
                        .map(|a| self.resolve_type_expr_to_type(a))
                        .collect();
                    Type::App(Box::new(Type::Con(name.clone())), resolved_args)
                }
            }
            TypeExpr::Tuple(elems) => Type::Tuple(
                elems
                    .iter()
                    .map(|e| self.resolve_type_expr_to_type(e))
                    .collect(),
            ),
            TypeExpr::Fn { params, ret } => Type::Fn(
                params
                    .iter()
                    .map(|p| self.resolve_type_expr_to_type(p))
                    .collect(),
                Box::new(self.resolve_type_expr_to_type(ret)),
            ),
        }
    }

    pub(super) fn type_to_jass_with_subst(&self, ty: &TypeExpr) -> String {
        match ty {
            TypeExpr::Named { name, .. } => {
                // Lowercase = type variable. Look up in mono_subst via type_param_vars.
                if name.chars().next().is_some_and(|c| c.is_lowercase()) {
                    // Find the VarId for this name in any function's type_param_vars
                    for tvars in self.type_param_vars.values() {
                        if let Some(&var_id) = tvars.get(name.as_str())
                            && let Some(concrete) = self.mono_subst.0.get(&var_id)
                        {
                            return concrete.to_jass().to_string();
                        }
                    }
                    "integer".to_string() // fallback
                } else {
                    self.type_to_jass(ty)
                }
            }
            _ => self.type_to_jass(ty),
        }
    }

    pub(super) fn contains_intrinsic_call(expr: &Expr, intrinsics: &HashSet<String>) -> bool {
        match expr {
            Expr::Call { function, args } => {
                if let Expr::Var(name) = &function.node
                    && intrinsics.contains(name)
                {
                    return true;
                }
                args.iter()
                    .any(|a| Self::contains_intrinsic_call(&a.node, intrinsics))
            }
            Expr::Let { value, body, .. } => {
                Self::contains_intrinsic_call(&value.node, intrinsics)
                    || Self::contains_intrinsic_call(&body.node, intrinsics)
            }
            Expr::Case { subject, arms } => {
                Self::contains_intrinsic_call(&subject.node, intrinsics)
                    || arms
                        .iter()
                        .any(|arm| Self::contains_intrinsic_call(&arm.body.node, intrinsics))
            }
            Expr::Block(exprs) => exprs
                .iter()
                .any(|e| Self::contains_intrinsic_call(&e.node, intrinsics)),
            _ => false,
        }
    }

    pub(super) fn mangle_types(types: &[Type]) -> String {
        types
            .iter()
            .map(|t| t.to_jass().replace(' ', "_"))
            .collect::<Vec<_>>()
            .join("_")
    }

    pub(super) fn build_mono_subst(
        &self,
        fn_name: &str,
        concrete_arg_types: &[Type],
    ) -> Substitution {
        let mut subst = Substitution::new();
        if let Some(tvars) = self.type_param_vars.get(fn_name) {
            // tvars: {"k" → VarId(42), "v" → VarId(43)}
            // We need to figure out which concrete type each var maps to.
            // The concrete_arg_types correspond to the function's params.
            // We need the function's param type annotations to match.
            if let Some(fdef) = self.fn_defs.get(fn_name) {
                let mut name_to_type: HashMap<String, Type> = HashMap::new();
                for (param, concrete) in fdef.params.iter().zip(concrete_arg_types.iter()) {
                    Self::extract_type_bindings(&param.type_expr, concrete, &mut name_to_type);
                }
                // Convert name-based bindings to VarId-based substitution
                for (name, var_id) in tvars {
                    if let Some(concrete_type) = name_to_type.get(name) {
                        subst.0.insert(*var_id, concrete_type.clone());
                    }
                }
            }
        }
        subst
    }

    pub(super) fn extract_type_bindings(
        type_expr: &TypeExpr,
        concrete: &Type,
        bindings: &mut HashMap<String, Type>,
    ) {
        match type_expr {
            TypeExpr::Named { name, args } => {
                // Lowercase name = type variable
                if name.chars().next().is_some_and(|c| c.is_lowercase()) {
                    bindings.insert(name.clone(), concrete.clone());
                } else if let Type::App(_, concrete_args) = concrete {
                    for (te, ct) in args.iter().zip(concrete_args.iter()) {
                        Self::extract_type_bindings(te, ct, bindings);
                    }
                }
            }
            TypeExpr::Tuple(elems) => {
                if let Type::Tuple(concrete_elems) = concrete {
                    for (te, ct) in elems.iter().zip(concrete_elems.iter()) {
                        Self::extract_type_bindings(te, ct, bindings);
                    }
                }
            }
            TypeExpr::Fn { params, ret } => {
                if let Type::Fn(concrete_params, concrete_ret) = concrete {
                    for (te, ct) in params.iter().zip(concrete_params.iter()) {
                        Self::extract_type_bindings(te, ct, bindings);
                    }
                    Self::extract_type_bindings(ret, concrete_ret, bindings);
                }
            }
        }
    }

    pub(super) fn resolve_arg_jass_type(&self, arg: &Spanned<Expr>) -> &'static str {
        // First check if it's a variable with a known mono param type
        if let Expr::Var(name) = &arg.node
            && let Some(ty) = self.mono_param_types.get(name.as_str())
        {
            return ty.to_jass();
        }
        // Fall back to type_map
        self.lookup_type(arg.span)
    }

    pub(super) fn gen_intrinsic_call(
        &self,
        intrinsic: &str,
        args: &[Spanned<Expr>],
        args_str: &[String],
        call_span: Option<crate::token::Span>,
    ) -> String {
        match intrinsic {
            "dict_save" => {
                // dict_save(ht, parent_key, child_key, value)
                // The 4th arg (value) determines which Save* native to use
                let value_jass = if let Some(arg) = args.get(3) {
                    self.resolve_arg_jass_type(arg)
                } else {
                    "integer"
                };
                let jass_fn = match value_jass {
                    "real" => "SaveReal",
                    "string" => "SaveStr",
                    "boolean" => "SaveBoolean",
                    _ => "SaveInteger",
                };
                format!("{}({})", jass_fn, args_str.join(", "))
            }
            "dict_load" => {
                // dict_load(ht, parent_key, child_key) -> v
                // Return type determines which Load* native to use.
                // call_span points to the function var — extract return type from fn type.
                let ret_jass = call_span
                    .and_then(|s| self.type_map.get(&(s.start, s.end)))
                    .and_then(|ty| match ty {
                        Type::Fn(_, ret) => Some(ret.to_jass()),
                        _ => None,
                    })
                    .unwrap_or("integer");
                let jass_fn = match ret_jass {
                    "real" => "LoadReal",
                    "string" => "LoadStr",
                    "boolean" => "LoadBoolean",
                    _ => "LoadInteger",
                };
                format!("{}({})", jass_fn, args_str.join(", "))
            }
            "dict_has" => {
                // HaveSaved* depends on value type, but the key arg determines which slot.
                // JASS HasSaved functions are type-specific. We use the same heuristic:
                // look at what type is stored at this key (same as dict_save's value type).
                // For simplicity, use the key type to pick the right HaveSaved.
                // Actually, HaveSavedInteger checks if an integer was saved at that slot.
                // For now, use HaveSavedInteger as default (covers Int + user types).
                let key_jass = if let Some(arg) = args.get(2) {
                    self.lookup_type(arg.span)
                } else {
                    "integer"
                };
                let jass_fn = match key_jass {
                    "real" => "HaveSavedReal",
                    "string" => "HaveSavedString",
                    "boolean" => "HaveSavedBoolean",
                    _ => "HaveSavedInteger",
                };
                format!("{}({})", jass_fn, args_str.join(", "))
            }
            "dict_remove" => {
                let key_jass = if let Some(arg) = args.get(2) {
                    self.lookup_type(arg.span)
                } else {
                    "integer"
                };
                let jass_fn = match key_jass {
                    "real" => "RemoveSavedReal",
                    "string" => "RemoveSavedString",
                    "boolean" => "RemoveSavedBoolean",
                    _ => "RemoveSavedInteger",
                };
                format!("{}({})", jass_fn, args_str.join(", "))
            }
            _ => {
                format!("glass_{}({})", intrinsic, args_str.join(", "))
            }
        }
    }

    pub(super) fn lookup_type(&self, span: crate::token::Span) -> &'static str {
        if let Some(ty) = self.type_map.get(&(span.start, span.end)) {
            let resolved = ty.apply(&self.mono_subst);
            if !resolved.free_vars().is_empty() {
                // Still has type vars — might be in mono_param_types
                return "integer"; // TODO: better fallback
            }
            resolved.to_jass()
        } else {
            "integer" // fallback
        }
    }

    pub(super) fn lookup_full_type(&self, span: crate::token::Span) -> Option<Type> {
        self.type_map
            .get(&(span.start, span.end))
            .map(|ty| ty.apply(&self.mono_subst))
    }

    pub(super) fn resolve_type_name_from_app(&self, ty: &Type) -> Option<String> {
        match ty {
            Type::Con(name) => Some(name.clone()),
            Type::App(con, args) => {
                let Type::Con(base) = con.as_ref() else {
                    return None;
                };
                let jass_args: Vec<String> = args.iter().map(|a| a.to_jass().to_string()).collect();
                if let Some(mono_name) = self.types.resolve_mono_name(base, &jass_args) {
                    Some(mono_name.to_string())
                } else if self.types.types.contains_key(base) {
                    Some(base.clone())
                } else {
                    Some(base.clone())
                }
            }
            _ => None,
        }
    }

    pub(super) fn resolve_mono_ctor_type_from_span(&self, _ctor_name: &str) -> Option<String> {
        if self.types.mono_map.is_empty() {
            return None;
        }
        let span = self.current_expr_span?;
        let ty = self.lookup_full_type(span)?;
        self.resolve_type_name_from_app(&ty)
    }

    pub(super) fn resolve_mono_ctor_type(
        &self,
        ctor_name: &str,
        args: &[ConstructorArg],
    ) -> Option<String> {
        let type_hint = Self::type_hint_from_ctor_name(ctor_name)?;
        if self.types.mono_map.is_empty() {
            return None;
        }
        let jass_args: Vec<String> = args
            .iter()
            .map(|a| {
                let e = match a {
                    ConstructorArg::Positional(e) | ConstructorArg::Named(_, e) => e,
                };
                self.lookup_type(e.span).to_string()
            })
            .collect();
        if jass_args.is_empty() {
            return None;
        }
        self.types
            .resolve_mono_name(type_hint, &jass_args)
            .map(|s| s.to_string())
    }

    pub(super) fn var_to_list_elem_type(&self, name: &str) -> Option<String> {
        let jt = self.local_var_jass_types.get(name)?;
        if jt != "integer" && self.types.list_types.contains(jt.as_str()) {
            Some(jt.clone())
        } else {
            None
        }
    }

    pub(super) fn extract_list_elem_jass_type(ty: &TypeExpr) -> Option<String> {
        if let TypeExpr::Named { name, args } = ty {
            if name == "List" {
                if let Some(arg) = args.first() {
                    let jass = TypeRegistry::type_expr_to_jass_public(arg);
                    if jass != "integer" {
                        return Some(jass);
                    }
                }
            }
        }
        None
    }

    pub(super) fn extract_list_elem_type_from_subject(
        &self,
        subject: &Spanned<Expr>,
    ) -> Option<String> {
        if let Expr::Var(name) = &subject.node {
            if let Some(elem) = self.var_list_elem_types.get(name) {
                return Some(elem.clone());
            }
        }
        if let Some(ty) = self.lookup_full_type(subject.span) {
            if let Type::App(con, args) = ty {
                if let Type::Con(name) = *con {
                    if name == "List" {
                        if let Some(elem) = args.first() {
                            let jass = elem.to_jass();
                            if jass != "integer" && self.types.list_types.contains(jass) {
                                return Some(jass.to_string());
                            }
                        }
                    }
                }
            }
        }
        None
    }

    pub(super) fn infer_list_elem_from_tail(&self, tail: &Spanned<Expr>) -> Option<String> {
        let ty = self.lookup_full_type(tail.span)?;
        match ty {
            Type::App(con, args) => {
                if let Type::Con(name) = *con {
                    if name == "List" {
                        if let Some(elem) = args.first() {
                            let jass = elem.to_jass();
                            if self.types.list_types.contains(jass) {
                                return Some(jass.to_string());
                            }
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }
}
