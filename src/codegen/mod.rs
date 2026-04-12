mod closure;
mod dce;
mod expr;
mod mono;
mod pattern;
mod soa;

use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::closures::LambdaInfo;
use crate::modules::ResolvedImport;
use crate::runtime::ElmEntryPoints;
use crate::type_repr::{Substitution, Type, TypeVarId};
use crate::types::TypeRegistry;

pub(super) fn is_jass_type_keyword(name: &str) -> bool {
    matches!(
        name,
        "integer"
            | "real"
            | "boolean"
            | "string"
            | "handle"
            | "code"
            | "unit"
            | "player"
            | "item"
            | "effect"
            | "trigger"
            | "timer"
            | "group"
            | "force"
            | "location"
            | "rect"
            | "widget"
            | "destructable"
            | "texttag"
            | "multiboard"
            | "event"
            | "dialog"
            | "button"
            | "quest"
            | "region"
            | "sound"
            | "image"
            | "ubersplat"
            | "hashtable"
            | "fogmodifier"
            | "agent"
            | "boolexpr"
            | "conditionfunc"
            | "filterfunc"
    )
}

pub(super) fn safe_jass_name(name: &str) -> String {
    if is_jass_type_keyword(name) {
        format!("v_{}", name)
    } else {
        name.to_string()
    }
}

pub(super) fn format_float(n: f64) -> String {
    let s = format!("{}", n);
    if s.contains('.') {
        s
    } else {
        format!("{}.0", s)
    }
}

pub struct JassCodegen {
    /// Accumulated global declarations (emitted as one globals block)
    pub(super) globals: Vec<String>,
    /// Function bodies and other non-global output
    pub(super) output: String,
    pub(super) indent: usize,
    pub(super) temp_counter: usize,
    pub(super) temp_types: Vec<String>,
    pub(super) lambda_counter: usize,
    pub(super) types: TypeRegistry,
    pub(super) lambdas: Vec<LambdaInfo>,
    /// Map from AST node span (start, end) to its resolved type.
    /// Populated by the type checker; used for type-directed codegen.
    pub(super) type_map: HashMap<(usize, usize), Type>,
    /// Map from Glass function name → external JASS native name.
    /// e.g. "save_integer" → "SaveInteger"
    pub(super) externals: HashMap<String, ExternalInfo>,
    /// Functions that contain intrinsic calls and need monomorphization.
    pub(super) mono_needed: HashSet<String>,
    /// Type param vars from inferencer: fn_name → {param_name → TypeVarId}
    pub(super) type_param_vars: HashMap<String, HashMap<String, TypeVarId>>,
    /// Active type substitution for monomorphization (VarId → concrete Type).
    /// Set when generating a specialized function copy.
    pub(super) mono_subst: Substitution,
    /// During mono function generation: maps parameter names to their concrete types.
    pub(super) mono_param_types: HashMap<String, Type>,
    /// Already-generated specializations: (fn_name, mangled_suffix) → mangled_name
    pub(super) mono_generated: HashSet<String>,
    /// Function definitions by name (for generating specialized copies)
    pub(super) fn_defs: HashMap<String, FnDef>,
    /// Glass type names from the current function's parameters (for fallback type resolution).
    /// E.g. for `fn find(xs: List(PudgeState), uid: Int)` → ["PudgeState", "Int"]
    pub(super) current_fn_param_type_names: Vec<String>,
    /// Known constants: name → inlined JASS value (e.g. "ROT_SPELL" → "'AUau'").
    /// Constants are fully inlined — no globals emitted.
    pub(super) const_values: HashMap<String, String>,
    pub(super) dispatch_sigs: HashSet<String>,
    pub(super) current_list_elem_type: Option<String>,
    pub(super) current_tuple_field_types: Option<Vec<String>>,
    pub(super) var_list_elem_types: HashMap<String, String>,
    pub(super) local_var_jass_types: HashMap<String, String>,
    pub(super) local_var_glass_types: HashMap<String, String>,
    pub(super) pattern_var_types: HashMap<String, String>,
    pub(super) extend_methods: HashMap<String, String>,
    pub(super) current_expr_span: Option<crate::token::Span>,
    pub(super) current_case_type_name: Option<String>,
}

pub(super) struct ClosureEmitInfo {
    pub(super) id: usize,
    pub(super) captures: Vec<(String, String)>,
    pub(super) param_names: Vec<String>,
    pub(super) param_types: Vec<String>,
    pub(super) has_captures: bool,
}

#[derive(Clone)]
pub(super) struct ExternalInfo {
    pub(super) jass_name: String,
    /// "jass" for native JASS functions, "glass" for compiler intrinsics
    pub(super) module: String,
}

impl JassCodegen {
    pub fn new(
        types: TypeRegistry,
        lambdas: Vec<LambdaInfo>,
        type_map: HashMap<(usize, usize), Type>,
        type_param_vars: HashMap<String, HashMap<String, TypeVarId>>,
    ) -> Self {
        Self {
            globals: Vec::new(),
            output: String::new(),
            indent: 0,
            temp_counter: 0,
            temp_types: Vec::new(),
            lambda_counter: 0,
            types,
            lambdas,
            type_map,
            externals: HashMap::new(),
            mono_needed: HashSet::new(),
            type_param_vars,
            mono_subst: Substitution::new(),
            mono_param_types: HashMap::new(),
            mono_generated: HashSet::new(),
            fn_defs: HashMap::new(),
            current_fn_param_type_names: Vec::new(),
            const_values: HashMap::new(),
            dispatch_sigs: HashSet::new(),
            current_list_elem_type: None,
            current_tuple_field_types: None,
            var_list_elem_types: HashMap::new(),
            local_var_jass_types: HashMap::new(),
            local_var_glass_types: HashMap::new(),
            pattern_var_types: HashMap::new(),
            extend_methods: HashMap::new(),
            current_expr_span: None,
            current_case_type_name: None,
        }
    }

    pub(super) fn add_global(&mut self, line: &str) {
        self.globals.push(format!("    {}", line));
    }

    pub fn generate(mut self, module: &Module, imports: &[ResolvedImport]) -> String {
        // Phase 0: Collect external bindings and identify functions needing mono
        // Register externals with both unqualified AND qualified (module.name) keys
        for def in &module.definitions {
            if let Definition::External(e) = def {
                if let Some(ref src) = e.source_module {
                    let qualified = format!("{}.{}", src, e.fn_name);
                    self.externals
                        .entry(qualified)
                        .or_insert_with(|| ExternalInfo {
                            jass_name: e.name_in_module.clone(),
                            module: e.module.clone(),
                        });
                }
            }
        }
        for def in &module.definitions {
            match def {
                Definition::Const(c) => {
                    let value = self.gen_spanned_expr(&c.value);
                    self.const_values.insert(c.name.clone(), value);
                }
                Definition::External(e) => {
                    let info = ExternalInfo {
                        jass_name: e.name_in_module.clone(),
                        module: e.module.clone(),
                    };
                    self.externals.entry(e.fn_name.clone()).or_insert(info);
                }
                Definition::Function(f) => {
                    self.fn_defs.insert(f.name.clone(), f.clone());
                }
                Definition::Extend(ext) => {
                    for method in &ext.methods {
                        let prefixed = format!("{}_{}", ext.type_name, method.name);
                        self.extend_methods
                            .insert(method.name.clone(), prefixed.clone());
                        self.fn_defs.insert(prefixed, method.clone());
                    }
                }
                _ => {}
            }
        }
        for imp in imports {
            if imp.qualified {
                for def in &imp.definitions {
                    match def {
                        Definition::External(e) => {
                            let qualified = format!("{}.{}", imp.module_name, e.fn_name);
                            self.externals.insert(
                                qualified,
                                ExternalInfo {
                                    jass_name: e.name_in_module.clone(),
                                    module: e.module.clone(),
                                },
                            );
                        }
                        Definition::Const(c) => {
                            let qualified = format!("{}.{}", imp.module_name, c.name);
                            let value = self.gen_spanned_expr(&c.value);
                            self.const_values.insert(qualified, value);
                        }
                        _ => {}
                    }
                }
            }
        }
        // Mark functions that call intrinsics as needing monomorphization
        let intrinsic_names: HashSet<String> = self
            .externals
            .iter()
            .filter(|(_, ext)| ext.module == "glass")
            .map(|(name, _)| name.clone())
            .collect();
        for (name, fdef) in &self.fn_defs {
            if Self::contains_intrinsic_call(&fdef.body.node, &intrinsic_names) {
                self.mono_needed.insert(name.clone());
            }
        }

        // Phase 0b: DCE + topo sort — determine live definitions and their order.
        // Lambda collection must use the SAME order as codegen (sorted_defs) to keep
        // lambda_counter in sync between collection and codegen visitation.
        let imported_count: usize = imports.iter().map(|i| i.definitions.len()).sum();
        let live_defs = dce::dead_code_eliminate(&module.definitions, imported_count);
        let sorted_defs = dce::topo_sort_definitions(&live_defs);

        // Re-collect lambdas from sorted definitions (same order as codegen)
        {
            let mut fresh_collector = crate::closures::LambdaCollector::new();
            let sorted_module = crate::ast::Module {
                definitions: sorted_defs.iter().map(|d| (*d).clone()).collect(),
            };
            fresh_collector.collect_module(&sorted_module);
            self.lambdas = fresh_collector.lambdas;
        }

        // Phase 1: Collect globals and emit functions for SoA types
        self.gen_soa_preamble();

        // Phase 1b: List SoA
        self.gen_list_preamble();

        self.pre_collect_pattern_var_types(module);

        // Phase 1c: Closure globals + alloc/dealloc (before user functions)
        self.gen_closure_globals_and_alloc();

        // Phase 1c2: Type conversion helpers for dispatch
        self.emit("function glass_i2b takes integer i returns boolean");
        self.indent += 1;
        self.emit("return i != 0");
        self.indent -= 1;
        self.emit("endfunction");
        self.output.push('\n');

        // Phase 1d: Elm runtime globals
        let elm_entry = ElmEntryPoints::detect(module, &self.types);
        if let Some(ref entry) = elm_entry {
            crate::runtime::collect_runtime_globals(entry, &mut self.globals);
        }

        // Emit the single globals block
        if !self.globals.is_empty() {
            let mut result = String::from("globals\n");
            for g in &self.globals {
                result.push_str(g);
                result.push('\n');
            }
            result.push_str("endglobals\n\n");
            result.push_str(&self.output);
            self.output = result;
        }
        for def in &sorted_defs {
            self.gen_definition(def);
            self.output.push('\n');
        }

        // Phase 2.5: Closure dispatch functions with inlined lambda bodies.
        // Must be after user functions so lambda bodies can call them without forward references.
        self.gen_closure_dispatch(elm_entry.is_some());

        // Phase 3: Emit Elm runtime functions (after user functions)
        if let Some(entry) = elm_entry {
            crate::runtime::gen_elm_runtime_functions(
                &entry,
                &self.lambdas,
                &self.dispatch_sigs,
                &mut self.output,
            );
        }

        self.output
    }

    pub(super) fn fresh_temp(&mut self) -> String {
        self.fresh_temp_typed("integer")
    }

    pub(super) fn fresh_temp_typed(&mut self, jass_type: &str) -> String {
        let n = self.temp_counter;
        self.temp_counter += 1;
        self.temp_types.push(jass_type.to_string());
        format!("glass_tmp_{}", n)
    }

    pub(super) fn emit(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
        self.output.push_str(s);
        self.output.push('\n');
    }

    // === Definitions ===

    fn gen_definition(&mut self, def: &Definition) {
        match def {
            Definition::Function(f) => self.gen_fn_def(f),
            Definition::External(e) => self.gen_external_def(e),
            Definition::Const(c) => self.gen_const_def(c),
            Definition::Extend(ext) => {
                for method in &ext.methods {
                    let mut prefixed = method.clone();
                    prefixed.name = format!("{}_{}", ext.type_name, method.name);
                    self.gen_fn_def(&prefixed);
                }
            }
            Definition::Type(_) | Definition::Import(_) => {}
        }
    }

    fn gen_fn_def(&mut self, f: &FnDef) {
        // Track parameter type names for field access disambiguation
        self.current_fn_param_type_names = f
            .params
            .iter()
            .filter_map(|p| Self::extract_inner_type_name(&p.type_expr))
            .collect();

        self.current_list_elem_type = None;
        self.var_list_elem_types.clear();
        for p in &f.params {
            if let Some(elem) = Self::extract_list_elem_jass_type(&p.type_expr) {
                if self.types.list_types.contains(&elem) {
                    self.var_list_elem_types.insert(p.name.clone(), elem);
                }
            }
        }

        let params = f
            .params
            .iter()
            .map(|p| {
                format!(
                    "{} {}",
                    self.type_to_jass(&p.type_expr),
                    safe_jass_name(&p.name)
                )
            })
            .collect::<Vec<_>>();

        let takes = if params.is_empty() {
            "nothing".to_string()
        } else {
            params.join(", ")
        };

        let returns = match &f.return_type {
            Some(t) => self.type_to_jass(t),
            None => "nothing".to_string(),
        };

        let handle_params: Vec<String> = f
            .params
            .iter()
            .filter(|p| Self::handle_destroy_fn(&p.type_expr).is_some())
            .map(|p| safe_jass_name(&p.name))
            .collect();

        // Collect user-defined locals (let bindings, pattern vars)
        let mut locals = Vec::new();
        self.collect_locals(&f.body.node, &mut locals);

        self.local_var_jass_types.clear();
        self.local_var_glass_types.clear();
        for (name, jass_type) in &locals {
            self.local_var_jass_types
                .insert(name.clone(), jass_type.clone());
        }
        for p in &f.params {
            self.local_var_jass_types
                .insert(p.name.clone(), self.type_to_jass(&p.type_expr));
            if let Some(glass_type) = Self::extract_inner_type_name(&p.type_expr) {
                self.local_var_glass_types
                    .insert(p.name.clone(), glass_type);
            }
        }

        // Reset temp counter for this function, generate body into buffer
        let saved_temp = self.temp_counter;
        self.temp_counter = 0;
        let saved_temp_types = std::mem::take(&mut self.temp_types);
        let saved_output = std::mem::take(&mut self.output);
        let saved_indent = self.indent;
        self.indent = 1;

        let is_tco = matches!(&f.body.node, Expr::TcoLoop { .. });

        if is_tco {
            if let Expr::TcoLoop { body } = &f.body.node {
                self.emit("loop");
                self.indent += 1;
                self.gen_tco_body(&body.node);
                self.indent -= 1;
                self.emit("endloop");
                // Unreachable: pjass requires a return after endloop
                if let Some(ret_type) = &f.return_type {
                    let default = match self.type_to_jass(ret_type).as_str() {
                        "real" => "0.0",
                        "boolean" => "false",
                        "string" => "\"\"",
                        _ => "0",
                    };
                    self.emit(&format!("return {}", default));
                }
            }
        } else {
            let result = self.gen_spanned_expr(&f.body);
            for name in &handle_params {
                self.emit(&format!("set {} = null", name));
            }
            if f.return_type.is_some() {
                self.emit(&format!("return {}", result));
            }
        }

        let body_output = std::mem::replace(&mut self.output, saved_output);
        let temp_types = std::mem::replace(&mut self.temp_types, saved_temp_types);
        self.temp_counter = saved_temp;
        self.indent = saved_indent;

        // Now emit: header, locals, temps, body
        self.emit(&format!(
            "function glass_{} takes {} returns {}",
            f.name, takes, returns
        ));
        self.indent += 1;

        {
            let mut seen: std::collections::HashSet<String> =
                f.params.iter().map(|p| safe_jass_name(&p.name)).collect();
            for (name, jass_type) in &locals {
                let safe = safe_jass_name(name);
                if seen.insert(safe.clone()) {
                    self.emit(&format!("local {} {}", jass_type, safe));
                }
            }
        }
        for (i, jass_type) in temp_types.iter().enumerate() {
            self.emit(&format!("local {} glass_tmp_{}", jass_type, i));
        }
        // TCO temp locals for safe parameter reassignment
        if is_tco {
            for (i, p) in f.params.iter().enumerate() {
                self.emit(&format!(
                    "local {} glass_tco_{}",
                    self.type_to_jass(&p.type_expr),
                    i
                ));
            }
        }

        self.output.push_str(&body_output);
        self.indent -= 1;
        self.emit("endfunction");
    }

    /// Generate expression in tail position for TCO functions.
    /// Instead of returning a value, emits either:
    /// - `return VALUE` for base cases
    /// - parameter reassignment for tail calls (TcoContinue)
    fn gen_tco_body(&mut self, expr: &Expr) {
        match expr {
            Expr::TcoContinue { args } => {
                // Evaluate all new values into TCO temps (must happen before any assignment)
                for (i, (_, value)) in args.iter().enumerate() {
                    let val = self.gen_spanned_expr(&value);
                    self.emit(&format!("set glass_tco_{} = {}", i, val));
                }
                for (i, (param_name, _)) in args.iter().enumerate() {
                    self.emit(&format!(
                        "set {} = glass_tco_{}",
                        safe_jass_name(param_name),
                        i
                    ));
                }
                // No return — loop continues naturally
            }
            Expr::Case { subject, arms } => {
                let subj = self.gen_spanned_expr(&subject);

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

                let subj = if subject_type_name.as_deref() == Some("Bool")
                    && subj.contains("glass_dispatch_")
                {
                    format!("glass_i2b({})", subj)
                } else {
                    subj
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
                    self.gen_tco_body(&arm.body.node);
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
            }
            Expr::Let {
                pattern,
                value,
                body,
                ..
            } => {
                let val = self.gen_spanned_expr(&value);
                self.gen_let_pattern_binding(&pattern.node, &val, &value.node);
                self.gen_tco_body(&body.node);
            }
            Expr::Block(exprs) => {
                for (i, e) in exprs.iter().enumerate() {
                    if i < exprs.len() - 1 {
                        self.gen_spanned_expr(&e);
                    } else {
                        self.gen_tco_body(&e.node);
                    }
                }
            }
            _ => {
                // Non-tail expression — evaluate and return
                let val = self.gen_expr(expr);
                self.emit(&format!("return {}", val));
            }
        }
    }
}
