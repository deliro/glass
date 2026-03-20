use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::closures::LambdaInfo;
use crate::modules::ResolvedImport;
use crate::runtime::ElmEntryPoints;
use crate::type_repr::{Substitution, Type, TypeVarId};
use crate::types::TypeRegistry;

pub struct JassCodegen {
    /// Accumulated global declarations (emitted as one globals block)
    globals: Vec<String>,
    /// Function bodies and other non-global output
    output: String,
    indent: usize,
    temp_counter: usize,
    temp_types: Vec<String>,
    lambda_counter: usize,
    types: TypeRegistry,
    lambdas: Vec<LambdaInfo>,
    /// Map from AST node span (start, end) to its resolved type.
    /// Populated by the type checker; used for type-directed codegen.
    type_map: HashMap<(usize, usize), Type>,
    /// Map from Glass function name → external JASS native name.
    /// e.g. "save_integer" → "SaveInteger"
    externals: HashMap<String, ExternalInfo>,
    /// Functions that contain intrinsic calls and need monomorphization.
    mono_needed: HashSet<String>,
    /// Type param vars from inferencer: fn_name → {param_name → TypeVarId}
    type_param_vars: HashMap<String, HashMap<String, TypeVarId>>,
    /// Active type substitution for monomorphization (VarId → concrete Type).
    /// Set when generating a specialized function copy.
    mono_subst: Substitution,
    /// During mono function generation: maps parameter names to their concrete types.
    mono_param_types: HashMap<String, Type>,
    /// Already-generated specializations: (fn_name, mangled_suffix) → mangled_name
    mono_generated: HashSet<String>,
    /// Function definitions by name (for generating specialized copies)
    fn_defs: HashMap<String, FnDef>,
    /// Glass type names from the current function's parameters (for fallback type resolution).
    /// E.g. for `fn find(xs: List(PudgeState), uid: Int)` → ["PudgeState", "Int"]
    current_fn_param_type_names: Vec<String>,
    /// Known constants: name → inlined JASS value (e.g. "ROT_SPELL" → "'AUau'").
    /// Constants are fully inlined — no globals emitted.
    const_values: HashMap<String, String>,
}

struct ClosureEmitInfo {
    id: usize,
    captures: Vec<(String, String)>,
    param_names: Vec<String>,
    param_types: Vec<String>,
    has_captures: bool,
}

#[derive(Clone)]
struct ExternalInfo {
    jass_name: String,
    /// "jass" for native JASS functions, "glass" for compiler intrinsics
    module: String,
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
        }
    }

    fn add_global(&mut self, line: &str) {
        self.globals.push(format!("    {}", line));
    }

    pub fn generate(mut self, module: &Module, imports: &[ResolvedImport]) -> String {
        // Phase 0: Collect external bindings and identify functions needing mono
        // Register externals with both unqualified AND qualified (module.name) keys
        for def in &module.definitions {
            match def {
                Definition::Const(c) => {
                    let value = self.gen_expr(&c.value.node);
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
                _ => {}
            }
        }
        // Register qualified names for imported externals
        for imp in imports {
            if imp.qualified {
                for def in &imp.definitions {
                    if let Definition::External(e) = def {
                        let qualified = format!("{}.{}", imp.module_name, e.fn_name);
                        self.externals.insert(
                            qualified,
                            ExternalInfo {
                                jass_name: e.name_in_module.clone(),
                                module: e.module.clone(),
                            },
                        );
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
        let live_defs = dead_code_eliminate(&module.definitions, imported_count);
        let sorted_defs = topo_sort_definitions(&live_defs);

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
        if elm_entry.is_some() {
            crate::runtime::collect_runtime_globals(&mut self.globals);
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
        self.gen_closure_dispatch();

        // Phase 3: Emit Elm runtime functions (after user functions)
        if let Some(entry) = elm_entry {
            crate::runtime::gen_elm_runtime_functions(&entry, &self.lambdas, &mut self.output);
        }

        self.output
    }

    fn fresh_temp(&mut self) -> String {
        self.fresh_temp_typed("integer")
    }

    fn fresh_temp_typed(&mut self, jass_type: &str) -> String {
        let n = self.temp_counter;
        self.temp_counter += 1;
        self.temp_types.push(jass_type.to_string());
        format!("glass_tmp_{}", n)
    }

    fn emit(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
        self.output.push_str(s);
        self.output.push('\n');
    }

    // === SoA type compilation ===

    fn gen_soa_preamble(&mut self) {
        if self.types.types.is_empty() {
            return;
        }

        // Collect all info upfront to avoid borrow conflicts with self.emit
        struct TypeEmitInfo {
            name: String,
            is_enum: bool,
            variants: Vec<VariantEmitInfo>,
        }
        struct VariantEmitInfo {
            name: String,
            tag: i64,
            fields: Vec<(String, String)>, // (field_name, jass_type)
        }

        let type_infos: Vec<TypeEmitInfo> = self
            .types
            .types
            .values()
            .map(|info| TypeEmitInfo {
                name: info.name.clone(),
                is_enum: info.is_enum,
                variants: info
                    .variants
                    .iter()
                    .map(|v| VariantEmitInfo {
                        name: v.name.clone(),
                        tag: v.tag,
                        fields: v
                            .fields
                            .iter()
                            .map(|f| (f.name.clone(), f.jass_type.clone()))
                            .collect(),
                    })
                    .collect(),
            })
            .collect();

        // Collect SoA globals
        for info in &type_infos {
            self.add_global(&format!("// SoA arrays for type {}", info.name));

            if info.is_enum {
                self.add_global(&format!("integer array glass_{}_tag", info.name));
            }

            for variant in &info.variants {
                for (fname, ftype) in &variant.fields {
                    self.add_global(&format!(
                        "{} array glass_{}_{}_{}",
                        ftype, info.name, variant.name, fname
                    ));
                }
            }

            self.add_global(&format!("integer array glass_{}_free", info.name));
            self.add_global(&format!("integer glass_{}_free_top = 0", info.name));
            self.add_global(&format!("integer glass_{}_count = 0", info.name));
        }

        for info in &type_infos {
            if info.is_enum {
                for variant in &info.variants {
                    self.add_global(&format!(
                        "constant integer glass_TAG_{} = {}",
                        variant.name, variant.tag
                    ));
                }
            }
        }

        // Alloc/dealloc
        for info in &type_infos {
            self.gen_alloc_fn(&info.name);
            self.output.push('\n');
            self.gen_dealloc_fn(&info.name);
            self.output.push('\n');
        }

        // Constructors
        for info in &type_infos {
            for variant in &info.variants {
                self.gen_constructor_fn_from(
                    &info.name,
                    info.is_enum,
                    &variant.name,
                    variant.tag,
                    &variant.fields,
                );
                self.output.push('\n');
            }
        }

        // Field getters — inlined as direct array access at call sites
        for _info in &type_infos {}
    }

    fn gen_list_preamble(&mut self) {
        let list_types: Vec<String> = self.types.list_types.iter().cloned().collect();
        if list_types.is_empty() {
            return;
        }

        // Collect list globals
        for elem_type in &list_types {
            let lt = TypeRegistry::list_type_name(elem_type);
            self.add_global(&format!("// Linked list: {}", lt));
            self.add_global(&format!("{} array glass_{}_head", elem_type, lt));
            self.add_global(&format!("integer array glass_{}_tail", lt));
            self.add_global(&format!("integer array glass_{}_free", lt));
            self.add_global(&format!("integer glass_{}_free_top = 0", lt));
            self.add_global(&format!("integer glass_{}_count = 0", lt));
        }

        // Alloc/dealloc + cons for each list type
        for elem_type in &list_types {
            let lt = TypeRegistry::list_type_name(elem_type);
            self.gen_alloc_fn(&lt);
            self.output.push('\n');
            self.gen_dealloc_fn(&lt);
            self.output.push('\n');

            // cons: prepend element to list, return new node ID
            self.emit(&format!(
                "function glass_{}_cons takes {} head, integer tail returns integer",
                lt, elem_type
            ));
            self.indent += 1;
            self.emit(&format!("local integer id = glass_{}_alloc()", lt));
            self.emit(&format!("set glass_{}_head[id] = head", lt));
            self.emit(&format!("set glass_{}_tail[id] = tail", lt));
            self.emit("return id");
            self.indent -= 1;
            self.emit("endfunction");
            self.output.push('\n');
        }
    }

    fn collect_closure_infos(&self) -> Vec<ClosureEmitInfo> {
        self.lambdas
            .iter()
            .map(|l| ClosureEmitInfo {
                id: l.id,
                captures: l
                    .captures
                    .iter()
                    .map(|c| (c.name.clone(), c.jass_type.clone()))
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

    /// Emit closure globals and alloc/dealloc functions (safe before user code).
    fn gen_closure_globals_and_alloc(&mut self) {
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

    /// Emit dispatch functions with inlined lambda bodies (after user code).
    fn gen_closure_dispatch(&mut self) {
        if self.lambdas.is_empty() {
            // Always generate stub dispatch_void for runtime compatibility
            self.emit("function glass_dispatch_void takes integer glass_closure returns integer");
            self.indent += 1;
            self.emit("return 0");
            self.indent -= 1;
            self.emit("endfunction");
            self.output.push('\n');
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
                    local_decls.push(format!("local {} {}", jass_type, name));
                    assignments.push(format!(
                        "set {} = glass_clos{}_{}[glass_cid]",
                        name, info.id, name
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
                if *pname != dispatch_name {
                    local_decls.push(format!("local {} {}", ptype, pname));
                    assignments.push(format!("set {} = {}", pname, dispatch_name));
                }
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

            let result = self.gen_expr(&body.node);

            let body_output = std::mem::replace(&mut self.output, saved_output);
            let temp_types = std::mem::replace(&mut self.temp_types, saved_temp_types);
            self.temp_counter = saved_temp;
            self.indent = saved_indent;

            let mut locals_code = String::new();
            {
                let mut seen = std::collections::HashSet::new();
                for (name, jass_type) in &locals {
                    if seen.insert(name.clone()) {
                        locals_code.push_str(&format!("    local {} {}\n", jass_type, name));
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

        // Generate dispatch functions by arity (0, 1, 2, ...).
        // Lambda bodies are inlined into the dispatch to avoid JASS forward-reference issues
        // (lambda bodies may call glass_dispatch_N for nested closures).
        let mut arity_groups: HashMap<usize, Vec<&ClosureEmitInfo>> = HashMap::new();
        for info in &infos {
            arity_groups
                .entry(info.param_names.len())
                .or_default()
                .push(info);
        }

        let max_arity = arity_groups.keys().copied().max().unwrap_or(0).max(2);
        // Emit from highest to lowest arity: higher-arity dispatchers are called
        // by lower-arity lambda bodies (e.g. effect.map's 0-arity lambda calls dispatch_1).
        for arity in (0..=max_arity).rev() {
            let mut takes_parts = vec!["integer glass_closure".to_string()];
            for i in 0..arity {
                takes_parts.push(format!("integer glass_p{}", i));
            }

            self.emit(&format!(
                "function glass_dispatch_{} takes {} returns integer",
                arity,
                takes_parts.join(", ")
            ));
            self.indent += 1;
            self.emit("local integer glass_tag = glass_closure / 8192");
            self.emit("local integer glass_cid = glass_closure - glass_tag * 8192");

            // Hoist all locals from all lambda bodies in this arity group to the function top.
            // JASS requires all local declarations before the first statement.
            if let Some(lambdas) = arity_groups.get(&arity) {
                let mut seen_locals = std::collections::HashSet::new();
                for info in lambdas {
                    if let Some(code) = lambda_code.get(&info.id) {
                        // Hoist capture/param locals
                        for decl in &code.local_decls {
                            if seen_locals.insert(decl.clone()) {
                                self.emit(decl);
                            }
                        }
                        // Hoist body expression locals
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
                        // Emit capture/param assignments
                        self.indent += 1;
                        for assignment in &code.assignments {
                            self.emit(assignment);
                        }
                        self.indent -= 1;
                        // Emit body code + return
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

        // glass_dispatch_void = alias for glass_dispatch_0
        self.emit("function glass_dispatch_void takes integer glass_closure returns integer");
        self.indent += 1;
        self.emit("return glass_dispatch_0(glass_closure)");
        self.indent -= 1;
        self.emit("endfunction");
        self.output.push('\n');
    }

    fn gen_alloc_fn(&mut self, type_name: &str) {
        self.emit(&format!(
            "function glass_{}_alloc takes nothing returns integer",
            type_name
        ));
        self.indent += 1;
        self.emit("local integer id");
        self.emit(&format!("if glass_{}_free_top > 0 then", type_name));
        self.indent += 1;
        self.emit(&format!(
            "set glass_{}_free_top = glass_{}_free_top - 1",
            type_name, type_name
        ));
        self.emit(&format!(
            "set id = glass_{}_free[glass_{}_free_top]",
            type_name, type_name
        ));
        self.indent -= 1;
        self.emit("else");
        self.indent += 1;
        self.emit(&format!(
            "set glass_{}_count = glass_{}_count + 1",
            type_name, type_name
        ));
        self.emit(&format!("set id = glass_{}_count", type_name));
        self.indent -= 1;
        self.emit("endif");
        self.emit("return id");
        self.indent -= 1;
        self.emit("endfunction");
    }

    fn gen_dealloc_fn(&mut self, type_name: &str) {
        self.emit(&format!(
            "function glass_{}_dealloc takes integer id returns nothing",
            type_name
        ));
        self.indent += 1;
        self.emit(&format!(
            "set glass_{}_free[glass_{}_free_top] = id",
            type_name, type_name
        ));
        self.emit(&format!(
            "set glass_{}_free_top = glass_{}_free_top + 1",
            type_name, type_name
        ));
        self.indent -= 1;
        self.emit("endfunction");
    }

    fn gen_constructor_fn_from(
        &mut self,
        type_name: &str,
        is_enum: bool,
        variant_name: &str,
        variant_tag: i64,
        fields: &[(String, String)],
    ) {
        let params: Vec<String> = fields
            .iter()
            .map(|(fname, ftype)| format!("{} p_{}", ftype, fname))
            .collect();

        let takes = if params.is_empty() {
            "nothing".to_string()
        } else {
            params.join(", ")
        };

        self.emit(&format!(
            "function glass_new_{} takes {} returns integer",
            variant_name, takes
        ));
        self.indent += 1;
        self.emit(&format!("local integer id = glass_{}_alloc()", type_name));

        if is_enum {
            self.emit(&format!(
                "set glass_{}_tag[id] = {}",
                type_name, variant_tag
            ));
        }

        for (fname, _ftype) in fields {
            self.emit(&format!(
                "set glass_{}_{}_{} [id] = p_{}",
                type_name, variant_name, fname, fname
            ));
        }

        self.emit("return id");
        self.indent -= 1;
        self.emit("endfunction");
    }

    // === Definitions ===

    fn gen_definition(&mut self, def: &Definition) {
        match def {
            Definition::Function(f) => self.gen_fn_def(f),
            Definition::External(e) => self.gen_external_def(e),
            Definition::Const(c) => self.gen_const_def(c),
            Definition::Type(_) | Definition::Import(_) | Definition::Extend(_) => {}
        }
    }

    fn gen_fn_def(&mut self, f: &FnDef) {
        // Track parameter type names for field access disambiguation
        self.current_fn_param_type_names = f
            .params
            .iter()
            .filter_map(|p| Self::extract_inner_type_name(&p.type_expr))
            .collect();

        let params = f
            .params
            .iter()
            .map(|p| format!("{} {}", self.type_to_jass(&p.type_expr), p.name))
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

        // Collect handle params for auto-null (prevents JASS handle leaks)
        let handle_params: Vec<String> = f
            .params
            .iter()
            .filter(|p| Self::handle_destroy_fn(&p.type_expr).is_some())
            .map(|p| p.name.clone())
            .collect();

        // Collect user-defined locals (let bindings, pattern vars)
        let mut locals = Vec::new();
        self.collect_locals(&f.body.node, &mut locals);

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
            let result = self.gen_expr(&f.body.node);
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
                f.params.iter().map(|p| p.name.clone()).collect();
            for (name, jass_type) in &locals {
                if seen.insert(name.clone()) {
                    self.emit(&format!("local {} {}", jass_type, name));
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
                    let val = self.gen_expr(&value.node);
                    self.emit(&format!("set glass_tco_{} = {}", i, val));
                }
                // Assign TCO temps to params
                for (i, (param_name, _)) in args.iter().enumerate() {
                    self.emit(&format!("set {} = glass_tco_{}", param_name, i));
                }
                // No return — loop continues naturally
            }
            Expr::Case { subject, arms } => {
                let subj = self.gen_expr(&subject.node);

                let subject_type_name =
                    self.lookup_full_type(subject.span)
                        .and_then(|ty| match &ty {
                            Type::Con(name) => Some(name.clone()),
                            Type::App(con, _) => match con.as_ref() {
                                Type::Con(name) => Some(name.clone()),
                                _ => None,
                            },
                            _ => None,
                        });

                let subj = if subject_type_name.as_deref() == Some("Bool")
                    && subj.contains("glass_dispatch_")
                {
                    format!("glass_i2b({})", subj)
                } else {
                    subj
                };

                for (i, arm) in arms.iter().enumerate() {
                    let condition = self.gen_pattern_condition_typed(
                        &arm.pattern.node,
                        &subj,
                        subject_type_name.as_deref(),
                    );
                    let guard = arm
                        .guard
                        .as_ref()
                        .map(|g| format!(" and ({})", self.gen_expr(&g.node)))
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
            }
            Expr::Let {
                pattern,
                value,
                body,
                ..
            } => {
                let val = self.gen_expr(&value.node);
                self.gen_let_pattern_binding(&pattern.node, &val, &value.node);
                self.gen_tco_body(&body.node);
            }
            Expr::Block(exprs) => {
                for (i, e) in exprs.iter().enumerate() {
                    if i < exprs.len() - 1 {
                        self.gen_expr(&e.node);
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

    /// Returns the JASS destroy function for a handle type, if applicable.
    fn handle_destroy_fn(ty: &TypeExpr) -> Option<String> {
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

    fn gen_external_def(&mut self, _e: &ExternalDef) {
        // External functions map directly to JASS natives — no code generated.
        // The call sites will use the native name directly.
    }

    fn gen_const_def(&mut self, _c: &ConstDef) {
        // Constants are fully inlined at use sites — no codegen needed.
    }

    // === Expressions ===

    #[allow(clippy::indexing_slicing)]
    fn gen_expr(&mut self, expr: &Expr) -> String {
        match expr {
            Expr::Int(n) => n.to_string(),
            Expr::Float(n) => format!("{:.1}", n),
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
                // Inline constants at use site
                if let Some(value) = self.const_values.get(name.as_str()) {
                    return value.clone();
                }
                name.clone()
            }

            Expr::BinOp { op, left, right } => {
                // Constant folding: evaluate compile-time constants
                if let Some(result) = const_fold_binop(op, &left.node, &right.node) {
                    return result;
                }
                let l = self.gen_expr(&left.node);
                let r = self.gen_expr(&right.node);
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
                let o = self.gen_expr(&operand.node);
                match op {
                    UnaryOp::Negate => format!("-({})", o),
                    UnaryOp::Not => format!("not ({})", o),
                }
            }

            Expr::Call { function, args } => {
                // Check for external/intrinsic first (clone to avoid borrow issues)
                let ext_info = if let Expr::Var(name) = &function.node {
                    self.externals.get(name.as_str()).cloned()
                } else {
                    None
                };

                if let Some(ext) = ext_info {
                    let args_str: Vec<String> =
                        args.iter().map(|a| self.gen_expr(&a.node)).collect();
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
                            // Closure call: dispatch by arity
                            let args_str: Vec<String> =
                                args.iter().map(|a| self.gen_expr(&a.node)).collect();
                            let arity = args_str.len();
                            let mut dispatch_args = vec![name.clone()];
                            dispatch_args.extend(args_str);
                            return format!(
                                "glass_dispatch_{}({})",
                                arity,
                                dispatch_args.join(", ")
                            );
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
                                args.iter().map(|a| self.gen_expr(&a.node)).collect();
                            return format!("{}({})", mono_name, args_str.join(", "));
                        }
                        format!("glass_{}", name)
                    }
                    _ => self.gen_expr(&function.node),
                };
                let args_str: Vec<String> = args.iter().map(|a| self.gen_expr(&a.node)).collect();
                format!("{}({})", func_name, args_str.join(", "))
            }

            Expr::FieldAccess { object, field } => {
                // Check if this is a qualified const access: module.CONST_NAME
                if let Some(value) = self.const_values.get(field.as_str()) {
                    return value.clone();
                }

                let obj = self.gen_expr(&object.node);
                // Look up the object's type to generate the correct getter name
                let mut type_name = self
                    .lookup_full_type(object.span)
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
                    let mut candidates: Vec<String> = Vec::new();
                    for (tn, info) in &self.types.types {
                        for variant in &info.variants {
                            if variant.fields.iter().any(|f| f.name == *field) {
                                candidates.push(tn.clone());
                                break;
                            }
                        }
                    }
                    if candidates.len() == 1 {
                        type_name = candidates.into_iter().next().unwrap_or_default();
                    } else if candidates.len() > 1 {
                        // Disambiguate: prefer a type that matches a current function parameter
                        let param_match = candidates
                            .iter()
                            .find(|c| self.current_fn_param_type_names.contains(c));
                        if let Some(matched) = param_match {
                            type_name = matched.clone();
                        }
                    }
                }

                if type_name.is_empty() {
                    format!("glass_get_{}({})", field, obj)
                } else {
                    format!("glass_{}_{}_{} [{}]", type_name, type_name, field, obj)
                }
            }

            Expr::MethodCall {
                object,
                method,
                args,
            } => {
                // Check if this is a qualified module call (module.function)
                if let Expr::Var(module_name) = &object.node {
                    // Check for external (JASS native or Glass intrinsic)
                    let qualified_name = format!("{}.{}", module_name, method);
                    let ext_info = self
                        .externals
                        .get(&qualified_name)
                        .or_else(|| self.externals.get(method.as_str()))
                        .cloned();
                    if let Some(ext) = ext_info {
                        let args_str: Vec<String> =
                            args.iter().map(|a| self.gen_expr(&a.node)).collect();
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

                    // Regular module function call: module.func(args) → glass_func(args)
                    // Check if this function needs monomorphization
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
                            args.iter().map(|a| self.gen_expr(&a.node)).collect();
                        return format!("{}({})", mono_name, args_str.join(", "));
                    }

                    let args_str: Vec<String> =
                        args.iter().map(|a| self.gen_expr(&a.node)).collect();
                    return format!("glass_{}({})", method, args_str.join(", "));
                }

                let obj = self.gen_expr(&object.node);
                let mut all_args = vec![obj];
                for a in args {
                    all_args.push(self.gen_expr(&a.node));
                }
                format!("glass_{}({})", method, all_args.join(", "))
            }

            Expr::Let {
                value,
                body,
                pattern,
                ..
            } => {
                let val = self.gen_expr(&value.node);
                self.gen_let_pattern_binding(&pattern.node, &val, &value.node);
                self.gen_expr(&body.node)
            }

            Expr::Case { subject, arms } => {
                let subj = self.gen_expr(&subject.node);
                // Determine JASS type for the case result from first arm body
                let case_jass_type = arms
                    .first()
                    .and_then(|arm| self.lookup_full_type(arm.body.span))
                    .map(|ty| {
                        let jt = self.type_to_jass_from_type(&ty);
                        // Guard against stale type_map for imported functions.
                        // Infer the correct type from the arm body expression.
                        if let Some(arm) = arms.first() {
                            if jt != "real" && Self::expr_has_float(&arm.body.node) {
                                return "real".to_string();
                            }
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
                    .unwrap_or_else(|| "integer".to_string());
                let result_var = self.fresh_temp_typed(&case_jass_type);

                // Look up subject type for enum tag access
                let subject_type_name =
                    self.lookup_full_type(subject.span)
                        .and_then(|ty| match &ty {
                            Type::Con(name) => Some(name.clone()),
                            Type::App(con, _) => match con.as_ref() {
                                Type::Con(name) => Some(name.clone()),
                                _ => None,
                            },
                            _ => None,
                        });

                // If subject type is Bool but generated code is a dispatch call (returns integer),
                // wrap with glass_i2b to convert integer → boolean
                let subj = if subject_type_name.as_deref() == Some("Bool")
                    && subj.contains("glass_dispatch_")
                {
                    format!("glass_i2b({})", subj)
                } else {
                    subj
                };

                for (i, arm) in arms.iter().enumerate() {
                    let condition = self.gen_pattern_condition_typed(
                        &arm.pattern.node,
                        &subj,
                        subject_type_name.as_deref(),
                    );
                    let guard = arm
                        .guard
                        .as_ref()
                        .map(|g| format!(" and ({})", self.gen_expr(&g.node)))
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
                    let val = self.gen_expr(&arm.body.node);
                    self.emit(&format!("set {} = {}", result_var, val));
                    self.indent -= 1;
                }
                self.emit("endif");
                result_var
            }

            Expr::Block(exprs) => {
                let mut last = String::from("null");
                for expr in exprs {
                    last = self.gen_expr(&expr.node);
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

                let arg_strs: Vec<String> = elems.iter().map(|e| self.gen_expr(&e.node)).collect();

                format!("glass_new_{}({})", tuple_type, arg_strs.join(", "))
            }

            Expr::List(elems) => {
                if elems.is_empty() {
                    // nil = -1
                    "-1".to_string()
                } else {
                    // Look up element type from first element
                    let elem_type = self.lookup_type(elems[0].span).to_string();
                    let lt = crate::types::TypeRegistry::list_type_name(&elem_type);

                    // Build list right-to-left: [1, 2, 3] → cons(1, cons(2, cons(3, -1)))
                    let mut result = "-1".to_string();
                    for elem in elems.iter().rev() {
                        let val = self.gen_expr(&elem.node);
                        result = format!("glass_{}_cons({}, {})", lt, val, result);
                    }
                    result
                }
            }

            Expr::ListCons { head, tail } => {
                let h = self.gen_expr(&head.node);
                let t = self.gen_expr(&tail.node);
                // Look up element type from head
                let elem_type = self.lookup_type(head.span).to_string();
                let lt = crate::types::TypeRegistry::list_type_name(&elem_type);
                format!("glass_{}_cons({}, {})", lt, h, t)
            }

            Expr::Pipe { left, right } => {
                let l = self.gen_expr(&left.node);
                // Pipe: a |> f(b, _) → f(b, a), a |> f(b) → f(a, b), a |> f → f(a)
                match &right.node {
                    Expr::Call { function, args } => {
                        let func_name = match &function.node {
                            Expr::Var(name) => format!("glass_{}", name),
                            _ => self.gen_expr(&function.node),
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
                                        self.gen_expr(&a.node)
                                    }
                                })
                                .collect()
                        } else {
                            // No placeholder: insert as first arg
                            let mut all = vec![l];
                            for a in args {
                                all.push(self.gen_expr(&a.node));
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
                                            self.gen_expr(&a.node)
                                        }
                                    })
                                    .collect()
                            } else {
                                let mut all = vec![l];
                                for a in args {
                                    all.push(self.gen_expr(&a.node));
                                }
                                all
                            };
                            format!("{}({})", func_name, all_args.join(", "))
                        }
                    }
                    _ => {
                        let r = self.gen_expr(&right.node);
                        format!("{}({})", r, l)
                    }
                }
            }

            Expr::Constructor { name, args } => {
                // Check if this is a const reference (nullary, no :: qualifier)
                if args.is_empty()
                    && !name.contains("::")
                    && let Some(value) = self.const_values.get(name.as_str())
                {
                    return value.clone();
                }

                // Look up variant name from types (clone to release borrow)
                // Strip qualified prefix: "BashResult::Bashed" → "Bashed"
                let bare_name = name.rsplit("::").next().unwrap_or(name);
                let variant_name = self
                    .types
                    .get_variant(bare_name)
                    .map(|(_, v)| v.name.clone());

                match variant_name {
                    Some(vname) => {
                        let arg_strs: Vec<String> =
                            args.iter()
                                .map(|a| {
                                    let e = match a {
                                        ConstructorArg::Positional(e)
                                        | ConstructorArg::Named(_, e) => e,
                                    };
                                    self.gen_expr(&e.node)
                                })
                                .collect();
                        if arg_strs.is_empty() {
                            format!("glass_new_{}()", vname)
                        } else {
                            format!("glass_new_{}({})", vname, arg_strs.join(", "))
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
                // Clone type info to release borrow on self
                let record_info: Option<(String, Vec<(String, String)>)> =
                    self.types.types.get(name.as_str()).and_then(|info| {
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
                        let base_val = self.gen_expr(&base.node);
                        let tmp = self.fresh_temp();
                        self.emit(&format!("set {} = glass_{}_alloc()", tmp, name));
                        for (fname, _ftype) in &fields {
                            let updated = updates.iter().find(|(n, _)| n == fname);
                            match updated {
                                Some((_, val)) => {
                                    let v = self.gen_expr(&val.node);
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
                        // Allocate closure instance, store captured vars
                        let tmp = self.fresh_temp();
                        self.emit(&format!("set {} = glass_clos{}_alloc()", tmp, lambda_id));
                        for name in &capture_names {
                            self.emit(&format!(
                                "set glass_clos{}_{}[{}] = {}",
                                lambda_id, name, tmp, name
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

            Expr::Clone(inner) => self.gen_expr(&inner.node),

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

    /// Generate pattern condition with type info for correct SoA tag access.
    /// Strip qualified prefix: "BashResult::Bashed" → "Bashed"
    fn expr_has_float(expr: &Expr) -> bool {
        match expr {
            Expr::Float(_) => true,
            Expr::BinOp { left, right, .. } => {
                Self::expr_has_float(&left.node) || Self::expr_has_float(&right.node)
            }
            Expr::UnaryOp { operand, .. } => Self::expr_has_float(&operand.node),
            _ => false,
        }
    }

    fn bare_ctor_name(name: &str) -> &str {
        name.rsplit("::").next().unwrap_or(name)
    }

    fn gen_pattern_condition_typed(
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
                    // Check if this is a named constant (not a constructor).
                    // Handle qualified names: "setup.ROT_SPELL" → check "ROT_SPELL"
                    let const_key = bare.rsplit('.').next().unwrap_or(bare);
                    if let Some(value) = self.const_values.get(const_key) {
                        format!("({} == {})", subject, value)
                    } else {
                        format!("({} == glass_TAG_{})", subject, bare)
                    }
                } else {
                    let tag_access = match type_name {
                        Some(tn) => format!("glass_{}_tag[{}]", tn, subject),
                        None => format!("glass_tag({})", subject),
                    };
                    format!("({} == glass_TAG_{})", tag_access, bare)
                }
            }
            Pattern::ConstructorNamed { name, .. } => {
                let bare = Self::bare_ctor_name(name);
                let tag_access = match type_name {
                    Some(tn) => format!("glass_{}_tag[{}]", tn, subject),
                    None => format!("glass_tag({})", subject),
                };
                format!("({} == glass_TAG_{})", tag_access, bare)
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

    fn gen_let_pattern_binding(&mut self, pattern: &Pattern, val: &str, value_expr: &Expr) {
        match pattern {
            Pattern::Var(name) => {
                self.emit(&format!("set {} = {}", name, val));
            }
            Pattern::Discard => {
                // Side effects only — if it's a function call, emit `call`
                if matches!(value_expr, Expr::Call { .. }) {
                    self.emit(&format!("call {}", val));
                }
            }
            Pattern::Tuple(elems) => {
                // Destructure: let #(a, b) = tuple_expr
                // val is the tuple ID, read fields via getters
                let shape: Vec<String> = elems.iter().map(|_| "integer".to_string()).collect();
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
            _ => {
                let tmp = self.fresh_temp();
                self.emit(&format!("set {} = {}", tmp, val));
            }
        }
    }

    fn gen_pattern_bindings(&mut self, pattern: &Pattern, subject: &str) {
        match pattern {
            Pattern::Var(name) => {
                self.emit(&format!("set {} = {}", name, subject));
            }
            Pattern::Constructor { name, args } => {
                let bare = Self::bare_ctor_name(name);
                let variant_info = self.types.get_variant(bare).map(|(ti, v)| {
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
                    .types
                    .get_variant(bare)
                    .map(|(ti, _)| ti.name.clone())
                    .unwrap_or_default();
                let prefix = if type_name.is_empty() {
                    bare.to_string()
                } else {
                    format!("{}_{}", type_name, bare)
                };
                for fp in fields {
                    let var = fp.binding.as_ref().unwrap_or(&fp.field_name);
                    let field = format!("glass_{}_{}[{}]", prefix, fp.field_name, subject);
                    self.emit(&format!("set {} = {}", var, field));
                }
            }
            Pattern::Or(alternatives) => {
                // Bind from the first alternative (all must bind same vars)
                if let Some(first) = alternatives.first() {
                    self.gen_pattern_bindings(&first.node, subject);
                }
            }
            Pattern::ListCons { head, tail } => {
                // Extract head and tail from linked list SoA
                // TODO: determine list element type for correct SoA name
                // For now, use List_integer (most common case)
                let list_type = "List_integer";
                let head_expr = format!("glass_{}_head[{}]", list_type, subject);
                let tail_expr = format!("glass_{}_tail[{}]", list_type, subject);
                self.gen_pattern_bindings(&head.node, &head_expr);
                self.gen_pattern_bindings(&tail.node, &tail_expr);
            }
            Pattern::As { pattern, name } => {
                self.emit(&format!("set {} = {}", name, subject));
                self.gen_pattern_bindings(&pattern.node, subject);
            }
            _ => {}
        }
    }

    // === Locals collection ===

    fn collect_locals(&self, expr: &Expr, locals: &mut Vec<(String, String)>) {
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
                            Some(t) => self.type_to_jass(t),
                            None => self.lookup_type(value.span).to_string(),
                        };
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
                for arm in arms {
                    // Collect bindings from patterns
                    self.collect_pattern_locals(&arm.pattern.node, locals);
                    self.collect_locals(&arm.body.node, locals);
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

    fn collect_pattern_locals(&self, pattern: &Pattern, locals: &mut Vec<(String, String)>) {
        match pattern {
            Pattern::Var(name) if name != "_" => {
                locals.push((name.clone(), "integer".to_string()));
            }
            Pattern::Constructor { name, args } => {
                // Look up field types from TypeRegistry for correct JASS types
                let bare = Self::bare_ctor_name(name);
                let field_types: Vec<String> = self
                    .types
                    .get_variant(bare)
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
                // Look up field types from the type registry
                let bare = Self::bare_ctor_name(name);
                let field_types: HashMap<String, String> = self
                    .types
                    .get_variant(bare)
                    .map(|(_, v)| {
                        v.fields
                            .iter()
                            .map(|f| (f.name.clone(), f.jass_type.clone()))
                            .collect()
                    })
                    .unwrap_or_default();
                for fp in fields {
                    let var = fp.binding.as_ref().unwrap_or(&fp.field_name);
                    let jass_type = field_types
                        .get(&fp.field_name)
                        .cloned()
                        .unwrap_or_else(|| "integer".to_string());
                    locals.push((var.clone(), jass_type));
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
                for e in elems {
                    self.collect_pattern_locals(&e.node, locals);
                }
            }
            Pattern::ListCons { head, tail } => {
                self.collect_pattern_locals(&head.node, locals);
                self.collect_pattern_locals(&tail.node, locals);
            }
            _ => {}
        }
    }

    // === Type mapping ===

    /// Extract the innermost type name from a type expression.
    /// E.g. `List(PudgeState)` → `"PudgeState"`, `Int` → `"Int"`, `fn(...) -> ...` → None
    fn extract_inner_type_name(ty: &TypeExpr) -> Option<String> {
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

    fn type_to_jass(&self, ty: &TypeExpr) -> String {
        let TypeExpr::Named { name, .. } = ty else {
            return "integer".to_string();
        };
        Self::type_name_to_jass(name)
    }

    fn type_name_to_jass(name: &str) -> String {
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

    fn type_to_jass_from_type(&self, ty: &Type) -> String {
        match ty {
            Type::Con(name) => Self::type_name_to_jass(name),
            Type::App(con, _) => match con.as_ref() {
                Type::Con(name) => Self::type_name_to_jass(name),
                _ => "integer".to_string(),
            },
            _ => "integer".to_string(),
        }
    }

    /// Generate a monomorphized (specialized) copy of a function.
    /// Emits the function to a separate buffer, then appends to output after current function.
    fn gen_mono_function(&mut self, orig_name: &str, mono_name: &str, subst: Substitution) {
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
        let result = self.gen_expr(&fdef.body.node);
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

    /// Resolve a TypeExpr to a concrete Type using the current mono substitution.
    fn resolve_type_expr_to_type(&self, ty: &TypeExpr) -> Type {
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

    /// Convert a TypeExpr to JASS type, applying the current mono substitution
    /// for lowercase type variables.
    fn type_to_jass_with_subst(&self, ty: &TypeExpr) -> String {
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

    /// Check if an expression tree contains calls to intrinsic functions.
    fn contains_intrinsic_call(expr: &Expr, intrinsics: &HashSet<String>) -> bool {
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

    /// Compute a type mangling suffix from concrete types.
    fn mangle_types(types: &[Type]) -> String {
        types
            .iter()
            .map(|t| t.to_jass().replace(' ', "_"))
            .collect::<Vec<_>>()
            .join("_")
    }

    /// Build a mono_subst from function param type annotations and concrete call-site types.
    fn build_mono_subst(&self, fn_name: &str, concrete_arg_types: &[Type]) -> Substitution {
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

    /// Recursively match a type expression against a concrete type to extract bindings.
    /// e.g. TypeExpr::Named("Dict", [Named("k"), Named("v")]) matched with Type::App(Dict, [Int, String])
    /// → {"k" → Int, "v" → String}
    fn extract_type_bindings(
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

    /// Resolve the JASS type of an argument expression.
    /// Checks mono_param_types for variables, then falls back to type_map.
    fn resolve_arg_jass_type(&self, arg: &Spanned<Expr>) -> &'static str {
        // First check if it's a variable with a known mono param type
        if let Expr::Var(name) = &arg.node
            && let Some(ty) = self.mono_param_types.get(name.as_str())
        {
            return ty.to_jass();
        }
        // Fall back to type_map
        self.lookup_type(arg.span)
    }

    /// Resolve a compiler intrinsic call based on argument types.
    /// `call_span` is the span of the entire Call expression (for return type lookup).
    fn gen_intrinsic_call(
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

    /// Look up the resolved type for a spanned expression.
    /// Applies the active mono_subst to resolve type variables.
    /// Falls back to mono_param_types for variable references.
    fn lookup_type(&self, span: crate::token::Span) -> &'static str {
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

    /// Look up the full resolved Type for a spanned expression.
    fn lookup_full_type(&self, span: crate::token::Span) -> Option<Type> {
        self.type_map
            .get(&(span.start, span.end))
            .map(|ty| ty.apply(&self.mono_subst))
    }
}

/// Constant folding: evaluate binary operations on literals at compile time.
fn const_fold_binop(op: &BinOp, left: &Expr, right: &Expr) -> Option<String> {
    match (op, left, right) {
        // Int arithmetic
        (BinOp::Add, Expr::Int(a), Expr::Int(b)) => Some(format!("{}", a + b)),
        (BinOp::Sub, Expr::Int(a), Expr::Int(b)) => Some(format!("{}", a - b)),
        (BinOp::Mul, Expr::Int(a), Expr::Int(b)) => Some(format!("{}", a * b)),
        (BinOp::Div, Expr::Int(a), Expr::Int(b)) if *b != 0 => Some(format!("{}", a / b)),
        (BinOp::Mod, Expr::Int(a), Expr::Int(b)) if *b != 0 => Some(format!("{}", a % b)),

        // Float arithmetic
        (BinOp::Add, Expr::Float(a), Expr::Float(b)) => Some(format!("{:.1}", a + b)),
        (BinOp::Sub, Expr::Float(a), Expr::Float(b)) => Some(format!("{:.1}", a - b)),
        (BinOp::Mul, Expr::Float(a), Expr::Float(b)) => Some(format!("{:.1}", a * b)),

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

/// Dead code elimination: keep only definitions reachable from entry points.
/// `imported_count` is the number of leading imported definitions.
/// Only user (non-imported) pub functions are entry points.
fn dead_code_eliminate(defs: &[Definition], imported_count: usize) -> Vec<Definition> {
    // Build fn name → index
    let mut fn_indices: HashMap<&str, usize> = HashMap::new();
    for (i, def) in defs.iter().enumerate() {
        match def {
            Definition::Function(f) => {
                fn_indices.insert(&f.name, i);
            }
            Definition::External(e) => {
                fn_indices.insert(&e.fn_name, i);
            }
            _ => {}
        }
    }

    // Build call graph
    let mut calls: HashMap<usize, HashSet<usize>> = HashMap::new();
    for (i, def) in defs.iter().enumerate() {
        if let Definition::Function(f) = def {
            let mut called = HashSet::new();
            collect_calls_in_expr(&f.body, &fn_indices, &mut called);
            calls.insert(i, called);
        }
    }

    // Check if there are any pub functions or Elm entry points
    let has_entry_points = defs.iter().any(|d| matches!(d,
        Definition::Function(f) if f.is_pub || matches!(f.name.as_str(), "init" | "update" | "subscriptions")
    ));

    // If no entry points, keep everything (e.g., test files, single-file scripts)
    if !has_entry_points {
        return defs.to_vec();
    }

    // Seed: entry points
    let mut reachable: HashSet<usize> = HashSet::new();
    for (i, def) in defs.iter().enumerate() {
        let is_entry = match def {
            // All type defs, imports, extends, consts are always kept
            Definition::Type(_)
            | Definition::Import(_)
            | Definition::Extend(_)
            | Definition::Const(_)
            | Definition::External(_) => true,
            // Only USER pub functions and Elm entry points are roots.
            // Imported pub functions are NOT entry points — only kept if reachable.
            Definition::Function(f) => {
                let is_user_def = i >= imported_count;
                (is_user_def && f.is_pub)
                    || matches!(f.name.as_str(), "init" | "update" | "subscriptions")
            }
        };
        if is_entry {
            reachable.insert(i);
        }
    }

    // BFS from entry points
    let mut queue: Vec<usize> = reachable.iter().copied().collect();
    while let Some(idx) = queue.pop() {
        if let Some(callees) = calls.get(&idx) {
            for &callee in callees {
                if reachable.insert(callee) {
                    queue.push(callee);
                }
            }
        }
    }

    // Filter
    defs.iter()
        .enumerate()
        .filter(|(i, _)| reachable.contains(i))
        .map(|(_, d)| d.clone())
        .collect()
}

/// Topological sort of definitions so callees appear before callers in JASS output.
fn topo_sort_definitions(defs: &[Definition]) -> Vec<&Definition> {
    // Build name → index mapping for functions
    let mut fn_indices: HashMap<&str, usize> = HashMap::new();
    for (i, def) in defs.iter().enumerate() {
        match def {
            Definition::Function(f) => {
                fn_indices.insert(&f.name, i);
            }
            Definition::External(e) => {
                fn_indices.insert(&e.fn_name, i);
            }
            _ => {}
        }
    }

    // Build adjacency: fn_index → set of fn_indices it calls
    let mut deps: HashMap<usize, HashSet<usize>> = HashMap::new();
    for (i, def) in defs.iter().enumerate() {
        if let Definition::Function(f) = def {
            let mut called = HashSet::new();
            collect_calls_in_expr(&f.body, &fn_indices, &mut called);
            deps.insert(i, called);
        }
    }

    // Kahn's algorithm: caller depends on callee. in_deg[caller] = number of callees.
    let n = defs.len();
    let mut in_deg2 = vec![0usize; n];
    for (&caller, callees) in &deps {
        if let Some(slot) = in_deg2.get_mut(caller) {
            *slot = callees.len();
        }
    }

    let mut queue: Vec<usize> = (0..n)
        .filter(|&i| in_deg2.get(i).copied() == Some(0))
        .collect();
    let mut sorted: Vec<usize> = Vec::new();

    // Build reverse map: callee → callers (who depend on callee)
    let mut callers_of: HashMap<usize, Vec<usize>> = HashMap::new();
    for (&caller, callees) in &deps {
        for &callee in callees {
            callers_of.entry(callee).or_default().push(caller);
        }
    }

    while let Some(node) = queue.pop() {
        sorted.push(node);
        if let Some(callers) = callers_of.get(&node) {
            for &caller in callers {
                if let Some(deg) = in_deg2.get_mut(caller) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push(caller);
                    }
                }
            }
        }
    }

    // Add any remaining (cycles) in original order
    let in_sorted: HashSet<usize> = sorted.iter().copied().collect();
    for i in 0..n {
        if !in_sorted.contains(&i) {
            sorted.push(i);
        }
    }

    sorted.iter().filter_map(|&i| defs.get(i)).collect()
}

/// Collect all function names called in an expression.
fn collect_calls_in_expr(
    expr: &Spanned<Expr>,
    fn_map: &HashMap<&str, usize>,
    out: &mut HashSet<usize>,
) {
    match &expr.node {
        Expr::Call { function, args } => {
            if let Expr::Var(name) = &function.node
                && let Some(&idx) = fn_map.get(name.as_str())
            {
                out.insert(idx);
            }
            collect_calls_in_expr(function, fn_map, out);
            for a in args {
                collect_calls_in_expr(a, fn_map, out);
            }
        }
        Expr::Var(name) => {
            if let Some(&idx) = fn_map.get(name.as_str()) {
                out.insert(idx);
            }
        }
        Expr::Let { value, body, .. } => {
            collect_calls_in_expr(value, fn_map, out);
            collect_calls_in_expr(body, fn_map, out);
        }
        Expr::Case { subject, arms } => {
            collect_calls_in_expr(subject, fn_map, out);
            for arm in arms {
                collect_calls_in_expr(&arm.body, fn_map, out);
                if let Some(g) = &arm.guard {
                    collect_calls_in_expr(g, fn_map, out);
                }
            }
        }
        Expr::BinOp { left, right, .. } | Expr::Pipe { left, right } => {
            collect_calls_in_expr(left, fn_map, out);
            collect_calls_in_expr(right, fn_map, out);
        }
        Expr::UnaryOp { operand, .. } | Expr::Clone(operand) => {
            collect_calls_in_expr(operand, fn_map, out);
        }
        Expr::Block(exprs) => {
            for e in exprs {
                collect_calls_in_expr(e, fn_map, out);
            }
        }
        Expr::Tuple(elems) | Expr::List(elems) => {
            for e in elems {
                collect_calls_in_expr(e, fn_map, out);
            }
        }
        Expr::ListCons { head, tail } => {
            collect_calls_in_expr(head, fn_map, out);
            collect_calls_in_expr(tail, fn_map, out);
        }
        Expr::Lambda { body, .. } => collect_calls_in_expr(body, fn_map, out),
        Expr::MethodCall {
            object,
            method,
            args,
        } => {
            // Register method name as a function call (for qualified module.func calls)
            if let Some(&idx) = fn_map.get(method.as_str()) {
                out.insert(idx);
            }
            collect_calls_in_expr(object, fn_map, out);
            for a in args {
                collect_calls_in_expr(a, fn_map, out);
            }
        }
        Expr::FieldAccess { object, .. } => collect_calls_in_expr(object, fn_map, out),
        Expr::Constructor { args, .. } => {
            for a in args {
                let e = match a {
                    ConstructorArg::Positional(e) | ConstructorArg::Named(_, e) => e,
                };
                collect_calls_in_expr(e, fn_map, out);
            }
        }
        Expr::RecordUpdate { base, updates, .. } => {
            collect_calls_in_expr(base, fn_map, out);
            for (_, e) in updates {
                collect_calls_in_expr(e, fn_map, out);
            }
        }
        Expr::TcoLoop { body } => {
            collect_calls_in_expr(body, fn_map, out);
        }
        Expr::TcoContinue { args } => {
            for (_, val) in args {
                collect_calls_in_expr(val, fn_map, out);
            }
        }
        _ => {}
    }
}
