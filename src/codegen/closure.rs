use std::collections::HashMap;

use crate::ast::*;
use crate::closures::CapturedVar;

use super::{safe_jass_name, ClosureEmitInfo};

impl super::JassCodegen {
    pub(super) fn resolve_capture_type(&self, capture: &CapturedVar, body: &Spanned<Expr>) -> String {
        let ty = self.lookup_type(capture.span);
        if ty != "integer" {
            return ty.to_string();
        }
        if let Some(usage_ty) = self.find_capture_usage_type(&capture.name, &body.node) {
            if usage_ty != "integer" {
                return usage_ty;
            }
        }
        if let Some(ann) = Self::find_capture_annotation(&capture.name, &body.node) {
            let resolved = self.type_to_jass(&ann);
            if resolved != "integer" {
                return resolved;
            }
        }
        if let Some(jt) = self.pattern_var_types.get(&capture.name) {
            if jt != "integer" {
                return jt.clone();
            }
        }
        ty.to_string()
    }

    pub(super) fn find_capture_usage_type(&self, name: &str, expr: &Expr) -> Option<String> {
        match expr {
            Expr::Let {
                value,
                body,
                pattern,
                ..
            } => {
                if let Expr::Var(ref v) = value.node {
                    if v == name {
                        if let Pattern::Var(ref bound) = pattern.node {
                            return self.find_capture_usage_type(bound, &body.node);
                        }
                    }
                }
                self.find_capture_usage_type(name, &body.node)
            }
            Expr::Constructor { args, .. } => {
                for arg in args {
                    let e = match arg {
                        ConstructorArg::Positional(e) | ConstructorArg::Named(_, e) => e,
                    };
                    if let Expr::Var(ref v) = e.node {
                        if v == name {
                            let ty = self.lookup_type(e.span);
                            if ty != "integer" {
                                return Some(ty.to_string());
                            }
                        }
                    }
                }
                None
            }
            Expr::Call { args, .. } => {
                for a in args {
                    if let Expr::Var(ref v) = a.node {
                        if v == name {
                            let ty = self.lookup_type(a.span);
                            if ty != "integer" {
                                return Some(ty.to_string());
                            }
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    pub(super) fn find_capture_annotation(name: &str, expr: &Expr) -> Option<TypeExpr> {
        if let Expr::Let {
            value,
            type_annotation,
            body,
            ..
        } = expr
        {
            if let Expr::Var(ref vname) = value.node
                && vname == name
                && let Some(ann) = type_annotation
            {
                return Some(ann.clone());
            }
            return Self::find_capture_annotation(name, &body.node);
        }
        None
    }

    pub(super) fn collect_closure_infos(&self) -> Vec<ClosureEmitInfo> {
        self.lambdas
            .iter()
            .map(|l| ClosureEmitInfo {
                id: l.id,
                captures: l
                    .captures
                    .iter()
                    .map(|c| (c.name.clone(), self.resolve_capture_type(c, &l.body)))
                    .collect(),
                param_names: l
                    .params
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        if p.name == "_" {
                            format!("glass_unused_{}", i)
                        } else {
                            p.name.clone()
                        }
                    })
                    .collect(),
                param_types: l
                    .params
                    .iter()
                    .map(|p| self.type_to_jass(&p.type_expr))
                    .collect(),
                has_captures: !l.captures.is_empty(),
            })
            .collect()
    }

    pub(super) fn pre_collect_pattern_var_types(&mut self, module: &Module) {
        for def in &module.definitions {
            if let Definition::Function(f) = def {
                self.scan_pattern_vars_in_expr(&f.body.node);
            }
        }
    }

    pub(super) fn scan_pattern_vars_in_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Case { subject, arms } => {
                self.scan_pattern_vars_in_expr(&subject.node);
                for arm in arms {
                    self.scan_pattern_var_bindings(&arm.pattern.node);
                    self.scan_pattern_vars_in_expr(&arm.body.node);
                }
            }
            Expr::Let { value, body, .. } => {
                self.scan_pattern_vars_in_expr(&value.node);
                self.scan_pattern_vars_in_expr(&body.node);
            }
            Expr::Block(exprs) => {
                for e in exprs {
                    self.scan_pattern_vars_in_expr(&e.node);
                }
            }
            Expr::Lambda { body, .. } => {
                self.scan_pattern_vars_in_expr(&body.node);
            }
            Expr::Call { function, args } => {
                self.scan_pattern_vars_in_expr(&function.node);
                for a in args {
                    self.scan_pattern_vars_in_expr(&a.node);
                }
            }
            Expr::BinOp { left, right, .. } | Expr::Pipe { left, right } => {
                self.scan_pattern_vars_in_expr(&left.node);
                self.scan_pattern_vars_in_expr(&right.node);
            }
            Expr::UnaryOp { operand, .. } | Expr::Clone(operand) => {
                self.scan_pattern_vars_in_expr(&operand.node);
            }
            Expr::TcoLoop { body } => {
                self.scan_pattern_vars_in_expr(&body.node);
            }
            _ => {}
        }
    }

    pub(super) fn scan_pattern_var_bindings(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Constructor { name, args } => {
                let field_types: Vec<String> = self
                    .resolve_variant(name)
                    .map(|(_, v)| v.fields.iter().map(|f| f.jass_type.clone()).collect())
                    .unwrap_or_default();
                for (i, arg) in args.iter().enumerate() {
                    if let Pattern::Var(vname) = &arg.node {
                        if vname != "_" {
                            if let Some(jt) = field_types.get(i) {
                                self.pattern_var_types.insert(vname.clone(), jt.clone());
                            }
                        }
                    } else {
                        self.scan_pattern_var_bindings(&arg.node);
                    }
                }
            }
            Pattern::ConstructorNamed { name, fields, .. } => {
                let field_type_map: HashMap<String, String> = self
                    .resolve_variant(name)
                    .map(|(_, v)| v.fields.iter().map(|f| (f.name.clone(), f.jass_type.clone())).collect())
                    .unwrap_or_default();
                for fp in fields {
                    if let Some(jt) = field_type_map.get(&fp.field_name) {
                        let binding = fp.binding_name();
                        if binding != "_" {
                            self.pattern_var_types.insert(binding.to_string(), jt.clone());
                        }
                    }
                }
            }
            Pattern::Tuple(elems) => {
                let field_types = self.lookup_tuple_field_types(elems.len());
                for (i, elem) in elems.iter().enumerate() {
                    if let Pattern::Var(vname) = &elem.node {
                        if vname != "_" {
                            if let Some(jt) = field_types.get(i) {
                                self.pattern_var_types.insert(vname.clone(), jt.clone());
                            }
                        }
                    } else {
                        self.scan_pattern_var_bindings(&elem.node);
                    }
                }
            }
            Pattern::ListCons { head, tail } => {
                self.scan_pattern_var_bindings(&head.node);
                self.scan_pattern_var_bindings(&tail.node);
            }
            _ => {}
        }
    }

    pub(super) fn gen_closure_globals_and_alloc(&mut self) {
        if self.lambdas.is_empty() {
            return;
        }
        let infos = self.collect_closure_infos();
        let has_any_captures = infos.iter().any(|i| i.has_captures);
        if has_any_captures {
            for info in &infos {
                if info.has_captures {
                    self.add_global(&format!("// Captures for closure {}", info.id));
                    for (name, jass_type) in &info.captures {
                        self.add_global(&format!(
                            "{} array glass_clos{}_{}",
                            jass_type, info.id, name
                        ));
                    }
                    self.add_global(&format!("integer array glass_clos{}_free", info.id));
                    self.add_global(&format!("integer glass_clos{}_free_top = 0", info.id));
                    self.add_global(&format!("integer glass_clos{}_count = 0", info.id));
                }
            }
            for info in &infos {
                if info.has_captures {
                    let name = format!("clos{}", info.id);
                    self.gen_alloc_fn(&name);
                    self.output.push('\n');
                    self.gen_dealloc_fn(&name);
                    self.output.push('\n');
                }
            }
        }
    }

    pub(super) fn gen_closure_dispatch(&mut self, has_elm_entry: bool) {
        if self.lambdas.is_empty() {
            self.dispatch_sigs.insert("glass_dispatch_void".into());
            self.emit("function glass_dispatch_void takes integer glass_closure returns integer");
            self.indent += 1;
            self.emit("return 0");
            self.indent -= 1;
            self.emit("endfunction");
            self.output.push('\n');
            if has_elm_entry {
                self.dispatch_sigs.insert("glass_dispatch_1_unit".into());
                self.emit(
                    "function glass_dispatch_1_unit takes integer glass_closure, unit glass_p0 returns integer",
                );
                self.indent += 1;
                self.emit("return 0");
                self.indent -= 1;
                self.emit("endfunction");
                self.output.push('\n');
            }
            return;
        }

        let infos = self.collect_closure_infos();

        // Generate lambda bodies into a map (id → (locals_code, body_code, result_expr))
        // These will be inlined into dispatch functions to avoid JASS forward-reference issues.
        let lambda_bodies: Vec<Spanned<Expr>> =
            self.lambdas.iter().map(|l| l.body.clone()).collect();

        struct LambdaBodyCode {
            local_decls: Vec<String>, // "local integer callback"
            assignments: Vec<String>, // "set callback = glass_clos0_callback[glass_cid]"
            locals_code: String,      // locals from body expressions
            body_code: String,
            result_expr: String,
        }

        let mut lambda_code: HashMap<usize, LambdaBodyCode> = HashMap::new();

        for (info, body) in infos.iter().zip(lambda_bodies.iter()) {
            // Generate capture/param declarations and assignments separately.
            // JASS requires all locals at function top, so declarations are hoisted.
            // Assignments are emitted inside the if-branch.
            let mut local_decls = Vec::new(); // "local integer callback"
            let mut assignments = Vec::new(); // "set callback = glass_clos0_callback[glass_cid]"

            if info.has_captures {
                for (name, jass_type) in &info.captures {
                    let safe = safe_jass_name(name);
                    local_decls.push(format!("local {} {}", jass_type, safe));
                    assignments.push(format!(
                        "set {} = glass_clos{}_{}[glass_cid]",
                        safe, info.id, name
                    ));
                }
            }

            for (j, (pname, ptype)) in info
                .param_names
                .iter()
                .zip(info.param_types.iter())
                .enumerate()
            {
                let dispatch_name = format!("glass_p{}", j);
                let safe = safe_jass_name(pname);
                if safe != dispatch_name {
                    local_decls.push(format!("local {} {}", ptype, safe));
                    assignments.push(format!("set {} = {}", safe, dispatch_name));
                }
            }

            // Pre-populate capture types so collect_locals can resolve value types
            let saved_local_types = self.local_var_jass_types.clone();
            for (cname, ctype) in &info.captures {
                self.local_var_jass_types.insert(cname.clone(), ctype.clone());
            }

            // Collect user locals, then generate body with fresh temp counter
            let mut locals = Vec::new();
            self.collect_locals(&body.node, &mut locals);

            let saved_temp = self.temp_counter;
            self.temp_counter = 0;
            let saved_temp_types = std::mem::take(&mut self.temp_types);
            let saved_output = std::mem::take(&mut self.output);
            let saved_indent = self.indent;
            self.indent = 1;

            let result = self.gen_spanned_expr(&body);
            self.local_var_jass_types = saved_local_types;

            let body_output = std::mem::replace(&mut self.output, saved_output);
            let temp_types = std::mem::replace(&mut self.temp_types, saved_temp_types);
            self.temp_counter = saved_temp;
            self.indent = saved_indent;

            let mut locals_code = String::new();
            {
                let mut seen = std::collections::HashSet::new();
                for (name, jass_type) in &locals {
                    let safe = safe_jass_name(name);
                    if seen.insert(safe.clone()) {
                        locals_code.push_str(&format!("    local {} {}\n", jass_type, safe));
                    }
                }
            }
            for (i, jass_type) in temp_types.iter().enumerate() {
                locals_code.push_str(&format!("    local {} glass_tmp_{}\n", jass_type, i));
            }

            lambda_code.insert(
                info.id,
                LambdaBodyCode {
                    local_decls,
                    assignments,
                    locals_code,
                    body_code: body_output,
                    result_expr: result,
                },
            );
        }

        let mut sig_groups: HashMap<Vec<String>, Vec<&ClosureEmitInfo>> = HashMap::new();
        for info in &infos {
            sig_groups
                .entry(info.param_types.clone())
                .or_default()
                .push(info);
        }

        let mut sorted_sigs: Vec<Vec<String>> = sig_groups.keys().cloned().collect();
        sorted_sigs.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));

        let has_arity_0 = sorted_sigs.iter().any(|s| s.is_empty());
        if !has_arity_0 {
            sorted_sigs.insert(0, vec![]);
        }

        for sig in &sorted_sigs {
            let dispatch_name = Self::dispatch_fn_name(sig);
            let public_name = if sig.is_empty() {
                "glass_dispatch_void".to_string()
            } else {
                format!("glass_dispatch_{}_{}", sig.len(), sig.join("_"))
            };
            self.dispatch_sigs.insert(public_name);

            let mut takes_parts = vec!["integer glass_closure".to_string()];
            for (i, jass_type) in sig.iter().enumerate() {
                takes_parts.push(format!("{} glass_p{}", jass_type, i));
            }

            self.emit(&format!(
                "function {} takes {} returns integer",
                dispatch_name,
                takes_parts.join(", ")
            ));
            self.indent += 1;
            self.emit("local integer glass_tag = glass_closure / 8192");
            self.emit("local integer glass_cid = glass_closure - glass_tag * 8192");

            if let Some(lambdas) = sig_groups.get(sig) {
                let mut seen_locals = std::collections::HashSet::new();
                for info in lambdas {
                    if let Some(code) = lambda_code.get(&info.id) {
                        for decl in &code.local_decls {
                            if seen_locals.insert(decl.clone()) {
                                self.emit(decl);
                            }
                        }
                        for line in code.locals_code.lines() {
                            let trimmed = line.trim();
                            if trimmed.starts_with("local ")
                                && seen_locals.insert(trimmed.to_string())
                            {
                                self.emit(trimmed);
                            }
                        }
                    }
                }

                for (i, info) in lambdas.iter().enumerate() {
                    let kw = if i == 0 { "if" } else { "elseif" };
                    self.emit(&format!("{} glass_tag == {} then", kw, info.id));

                    if let Some(code) = lambda_code.get(&info.id) {
                        self.indent += 1;
                        for assignment in &code.assignments {
                            self.emit(assignment);
                        }
                        self.indent -= 1;
                        self.output.push_str(&code.body_code);
                        self.indent += 1;
                        self.emit(&format!("return {}", code.result_expr));
                        self.indent -= 1;
                    }
                }
                self.emit("endif");
            }

            self.emit("return 0");
            self.indent -= 1;
            self.emit("endfunction");
            self.output.push('\n');
        }

        self.emit("function glass_dispatch_void takes integer glass_closure returns integer");
        self.indent += 1;
        self.emit("return glass_dispatch_0(glass_closure)");
        self.indent -= 1;
        self.emit("endfunction");
        self.output.push('\n');

        if has_elm_entry && !sorted_sigs.iter().any(|s| s == &["unit".to_string()]) {
            self.dispatch_sigs.insert("glass_dispatch_1_unit".into());
            self.emit(
                "function glass_dispatch_1_unit takes integer glass_closure, unit glass_p0 returns integer",
            );
            self.indent += 1;
            self.emit("return 0");
            self.indent -= 1;
            self.emit("endfunction");
            self.output.push('\n');
        }
    }

}
