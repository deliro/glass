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

        // Phase 1: Collect globals and emit functions for SoA types
        self.gen_soa_preamble();

        // Phase 1b: List SoA
        self.gen_list_preamble();

        // Phase 1c: Closure infrastructure
        self.gen_closure_preamble();

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

        // Phase 2: Emit user definitions (DCE + topologically sorted)
        let live_defs = dead_code_eliminate(&module.definitions);
        let sorted_defs = topo_sort_definitions(&live_defs);
        for def in &sorted_defs {
            self.gen_definition(def);
            self.output.push('\n');
        }

        // Phase 3: Emit Elm runtime functions (after user functions)
        if let Some(entry) = elm_entry {
            crate::runtime::gen_elm_runtime_functions(&entry, &self.lambdas, &mut self.output);
        }

        self.output
    }

    fn fresh_temp(&mut self) -> String {
        let n = self.temp_counter;
        self.temp_counter += 1;
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

        // Field getters
        for info in &type_infos {
            for variant in &info.variants {
                for (fname, ftype) in &variant.fields {
                    self.gen_field_getter_from(&info.name, &variant.name, fname, ftype);
                    self.output.push('\n');
                }
            }
        }
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

    fn gen_closure_preamble(&mut self) {
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

        // Collect info to avoid borrow issues
        struct ClosureEmitInfo {
            id: usize,
            captures: Vec<(String, String)>, // (name, jass_type)
            param_names: Vec<String>,
            param_types: Vec<String>,
            has_captures: bool,
        }

        let infos: Vec<ClosureEmitInfo> = self
            .lambdas
            .iter()
            .map(|l| ClosureEmitInfo {
                id: l.id,
                captures: l
                    .captures
                    .iter()
                    .map(|c| (c.name.clone(), c.jass_type.clone()))
                    .collect(),
                param_names: l.params.iter().enumerate().map(|(i, p)| {
                    if p.name == "_" { format!("glass_unused_{}", i) } else { p.name.clone() }
                }).collect(),
                param_types: l
                    .params
                    .iter()
                    .map(|p| self.type_to_jass(&p.type_expr))
                    .collect(),
                has_captures: !l.captures.is_empty(),
            })
            .collect();

        // Collect closure globals
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

            // Alloc/dealloc for capturing closures
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

        // Generate function for each lambda body
        // Use saved lambda bodies
        let lambda_bodies: Vec<Spanned<Expr>> =
            self.lambdas.iter().map(|l| l.body.clone()).collect();

        for (info, body) in infos.iter().zip(lambda_bodies.iter()) {
            // All lambda functions take (integer glass_clos_id) as first param
            // + their actual params. This unifies capturing and non-capturing lambdas.
            let mut takes_parts: Vec<String> = vec!["integer glass_clos_id".to_string()];
            for (pname, ptype) in info.param_names.iter().zip(info.param_types.iter()) {
                takes_parts.push(format!("{} {}", ptype, pname));
            }

            let takes = takes_parts.join(", ");

            self.emit(&format!(
                "function glass_lambda_{} takes {} returns integer",
                info.id, takes
            ));
            self.indent += 1;

            // Load captured variables from SoA (only for capturing closures)
            if info.has_captures {
                for (name, jass_type) in &info.captures {
                    self.emit(&format!(
                        "local {} {} = glass_clos{}_{}[glass_clos_id]",
                        jass_type, name, info.id, name
                    ));
                }
            }

            // Collect and declare locals
            let mut locals = Vec::new();
            self.collect_locals(&body.node, &mut locals);
            for (name, jass_type) in &locals {
                self.emit(&format!("local {} {}", jass_type, name));
            }

            let result = self.gen_expr(&body.node);
            self.emit(&format!("return {}", result));
            self.indent -= 1;
            self.emit("endfunction");
            self.output.push('\n');
        }

        // Generate dispatch functions by arity (0, 1, 2, ...).
        // All closure params are integer in JASS.
        let mut arity_groups: HashMap<usize, Vec<&ClosureEmitInfo>> = HashMap::new();
        for info in &infos {
            arity_groups
                .entry(info.param_names.len())
                .or_default()
                .push(info);
        }

        // Generate dispatchers for arities 0..=max (ensure common arities exist)
        let max_arity = arity_groups.keys().copied().max().unwrap_or(0).max(2);
        for arity in 0..=max_arity {
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

            if let Some(lambdas) = arity_groups.get(&arity) {
                for (i, info) in lambdas.iter().enumerate() {
                    let kw = if i == 0 { "if" } else { "elseif" };
                    self.emit(&format!("{} glass_tag == {} then", kw, info.id));
                    self.indent += 1;

                    let mut call_args = vec!["glass_cid".to_string()];
                    for j in 0..arity {
                        call_args.push(format!("glass_p{}", j));
                    }

                    self.emit(&format!(
                        "return glass_lambda_{}({})",
                        info.id,
                        call_args.join(", ")
                    ));
                    self.indent -= 1;
                }
                self.emit("endif");
            }

            self.emit("return 0");
            self.indent -= 1;
            self.emit("endfunction");
            self.output.push('\n');
        }

        // glass_dispatch_void = alias for glass_dispatch_0
        self.emit(
            "function glass_dispatch_void takes integer glass_closure returns integer",
        );
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

    fn gen_field_getter_from(
        &mut self,
        type_name: &str,
        variant_name: &str,
        field_name: &str,
        jass_type: &str,
    ) {
        self.emit(&format!(
            "function glass_get_{}_{}_{} takes integer id returns {}",
            type_name, variant_name, field_name, jass_type
        ));
        self.indent += 1;
        self.emit(&format!(
            "return glass_{}_{}_{} [id]",
            type_name, variant_name, field_name
        ));
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

        self.emit(&format!(
            "function glass_{} takes {} returns {}",
            f.name, takes, returns
        ));
        self.indent += 1;

        // Collect handle params for auto-null (prevents JASS handle leaks)
        let handle_params: Vec<String> = f
            .params
            .iter()
            .filter(|p| Self::handle_destroy_fn(&p.type_expr).is_some())
            .map(|p| p.name.clone())
            .collect();

        // Collect locals needed for the body
        let mut locals = Vec::new();
        self.collect_locals(&f.body.node, &mut locals);
        for (name, jass_type) in &locals {
            self.emit(&format!("local {} {}", jass_type, name));
        }

        // Generate body
        let result = self.gen_expr(&f.body.node);

        // Null handle locals to prevent reference leaks
        // (DestroyX is the user's responsibility — linearity checker enforces)
        for name in &handle_params {
            self.emit(&format!("set {} = null", name));
        }

        if f.return_type.is_some() {
            self.emit(&format!("return {}", result));
        }

        self.indent -= 1;
        self.emit("endfunction");
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

    fn gen_const_def(&mut self, c: &ConstDef) {
        let jass_type = match &c.type_expr {
            Some(t) => self.type_to_jass(t),
            None => "integer".to_string(),
        };
        let value = self.gen_expr(&c.value.node);
        self.emit(&format!(
            "globals\n    constant {} glass_{} = {}\nendglobals",
            jass_type, c.name, value
        ));
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
            Expr::Var(name) => name.clone(),

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
                let obj = self.gen_expr(&object.node);
                // Look up the object's type to generate the correct getter name
                let type_name = self
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

                if type_name.is_empty() {
                    format!("glass_get_{}({})", field, obj)
                } else {
                    // Try to find the variant that has this field
                    // For single-constructor types, variant name == type name
                    // Try type_variant_field pattern first
                    format!("glass_get_{}_{}_{} ({})", type_name, type_name, field, obj)
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
                let result_var = self.fresh_temp();

                // Look up subject type for enum tag access
                let subject_type_name = self
                    .lookup_full_type(subject.span)
                    .and_then(|ty| match &ty {
                        Type::Con(name) => Some(name.clone()),
                        Type::App(con, _) => match con.as_ref() {
                            Type::Con(name) => Some(name.clone()),
                            _ => None,
                        },
                        _ => None,
                    });

                for (i, arm) in arms.iter().enumerate() {
                    let condition = self.gen_pattern_condition_typed(&arm.pattern.node, &subj, subject_type_name.as_deref());
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
                    _ => {
                        let r = self.gen_expr(&right.node);
                        format!("{}({})", r, l)
                    }
                }
            }

            Expr::Constructor { name, args } => {
                // Look up variant name from types (clone to release borrow)
                let variant_name = self.types.get_variant(name).map(|(_, v)| v.name.clone());

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
        }
    }

    /// Generate pattern condition with type info for correct SoA tag access.
    fn gen_pattern_condition_typed(&self, pattern: &Pattern, subject: &str, type_name: Option<&str>) -> String {
        match pattern {
            Pattern::Bool(true) => format!("({} == true)", subject),
            Pattern::Bool(false) => format!("({} == false)", subject),
            Pattern::Int(n) => format!("({} == {})", subject, n),
            Pattern::Float(n) => format!("({} == {:.1})", subject, n),
            Pattern::String(s) => format!("({} == \"{}\")", subject, s),
            Pattern::Constructor { name, args } => {
                if args.is_empty() {
                    // Nullary constructor — subject IS the tag directly (for enums stored as tag only)
                    format!("({} == glass_TAG_{})", subject, name)
                } else {
                    // Constructor with fields — look up tag from SoA
                    let tag_access = match type_name {
                        Some(tn) => format!("glass_{}_tag[{}]", tn, subject),
                        None => format!("glass_tag({})", subject), // fallback
                    };
                    format!("({} == glass_TAG_{})", tag_access, name)
                }
            }
            Pattern::ConstructorNamed { name, .. } => {
                let tag_access = match type_name {
                    Some(tn) => format!("glass_{}_tag[{}]", tn, subject),
                    None => format!("glass_tag({})", subject),
                };
                format!("({} == glass_TAG_{})", tag_access, name)
            }
            Pattern::Or(alternatives) => {
                let conditions: Vec<String> = alternatives
                    .iter()
                    .map(|alt| self.gen_pattern_condition_typed(&alt.node, subject, type_name))
                    .collect();
                format!("({})", conditions.join(" or "))
            }
            Pattern::As { pattern, .. } => self.gen_pattern_condition_typed(&pattern.node, subject, type_name),
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
                    let field_val =
                        format!("glass_get_{}_{}__{} [{}]", tuple_type, tuple_type, i, tmp);
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
            Pattern::Constructor { args, .. } => {
                for (i, arg) in args.iter().enumerate() {
                    let field = format!("glass_field_{}({})", i, subject);
                    self.gen_pattern_bindings(&arg.node, &field);
                }
            }
            Pattern::ConstructorNamed { name, fields, .. } => {
                for fp in fields {
                    let var = fp.binding.as_ref().unwrap_or(&fp.field_name);
                    let field = format!("glass_get_{}_{}({})", name, fp.field_name, subject);
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
                        // Temp for the tuple ID + each element binding
                        locals.push((format!("glass_tmp_{}", locals.len()), "integer".to_string()));
                        Self::collect_pattern_locals(&pattern.node, locals);
                        // Also recurse into each sub-element
                        for elem in elems {
                            Self::collect_pattern_locals(&elem.node, locals);
                        }
                    }
                    _ => {
                        Self::collect_pattern_locals(&pattern.node, locals);
                    }
                }
                self.collect_locals(&value.node, locals);
                self.collect_locals(&body.node, locals);
            }
            Expr::Case { subject, arms } => {
                // Add result temp
                locals.push((format!("glass_tmp_{}", locals.len()), "integer".to_string()));
                self.collect_locals(&subject.node, locals);
                for arm in arms {
                    // Collect bindings from patterns
                    Self::collect_pattern_locals(&arm.pattern.node, locals);
                    self.collect_locals(&arm.body.node, locals);
                }
            }
            Expr::Block(exprs) => {
                for e in exprs {
                    self.collect_locals(&e.node, locals);
                }
            }
            Expr::RecordUpdate { base, updates, .. } => {
                // RecordUpdate uses a temp for the new ID
                locals.push((format!("glass_tmp_{}", locals.len()), "integer".to_string()));
                self.collect_locals(&base.node, locals);
                for (_, val) in updates {
                    self.collect_locals(&val.node, locals);
                }
            }
            Expr::Lambda { .. } => {
                // Capturing lambdas use a temp for the closure instance ID
                // Check if this lambda has captures by matching the lambda counter
                // For safety, always reserve a temp for any lambda
                locals.push((format!("glass_tmp_{}", locals.len()), "integer".to_string()));
            }
            _ => {}
        }
    }

    fn collect_pattern_locals(pattern: &Pattern, locals: &mut Vec<(String, String)>) {
        match pattern {
            Pattern::Var(name) if name != "_" => {
                locals.push((name.clone(), "integer".to_string()));
            }
            Pattern::Constructor { args, .. } => {
                for arg in args {
                    Self::collect_pattern_locals(&arg.node, locals);
                }
            }
            Pattern::ConstructorNamed { fields, .. } => {
                for fp in fields {
                    let var = fp.binding.as_ref().unwrap_or(&fp.field_name);
                    locals.push((var.clone(), "integer".to_string()));
                }
            }
            Pattern::Or(alternatives) => {
                // All alternatives bind the same vars — use first
                if let Some(first) = alternatives.first() {
                    Self::collect_pattern_locals(&first.node, locals);
                }
            }
            Pattern::As { pattern, name } => {
                locals.push((name.clone(), "integer".to_string()));
                Self::collect_pattern_locals(&pattern.node, locals);
            }
            Pattern::Tuple(elems) => {
                for e in elems {
                    Self::collect_pattern_locals(&e.node, locals);
                }
            }
            Pattern::ListCons { head, tail } => {
                Self::collect_pattern_locals(&head.node, locals);
                Self::collect_pattern_locals(&tail.node, locals);
            }
            _ => {}
        }
    }

    // === Type mapping ===

    fn type_to_jass(&self, ty: &TypeExpr) -> String {
        let TypeExpr::Named { name, .. } = ty else {
            return "integer".to_string();
        };
        match name.as_str() {
            "Float" => "real".to_string(),
            "Bool" => "boolean".to_string(),
            "String" => "string".to_string(),
            "Unit" => "unit".to_string(),
            "Player" => "player".to_string(),
            "Timer" => "timer".to_string(),
            "Group" => "group".to_string(),
            "Trigger" => "trigger".to_string(),
            // Int, user types, Effect, closures, tuples → all integer
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
        self.indent += 1;

        // Collect and emit locals
        let mut locals = Vec::new();
        self.collect_locals(&fdef.body.node, &mut locals);
        for (name, jass_type) in &locals {
            self.emit(&format!("local {} {}", jass_type, name));
        }

        // Generate body
        let result = self.gen_expr(&fdef.body.node);
        if ret_type != "nothing" {
            self.emit(&format!("return {}", result));
        }

        self.indent -= 1;
        self.emit("endfunction");

        // Capture the generated function, restore state
        let mono_output = std::mem::replace(&mut self.output, prev_output);
        self.indent = prev_indent;
        self.mono_subst = prev_subst;
        self.mono_param_types = prev_param_types;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use crate::token::Lexer;
    use rstest::rstest;

    fn compile(source: &str) -> String {
        let tokens = Lexer::tokenize(source);
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module().expect("parse failed");
        let types = crate::types::TypeRegistry::from_module(&module);
        let mut collector = crate::closures::LambdaCollector::new();
        collector.collect_module(&module);
        // Run inference to get type_map
        let mut inferencer = crate::infer::Inferencer::new();
        let infer_result = inferencer.infer_module(&module);
        JassCodegen::new(
            types,
            collector.lambdas,
            infer_result.type_map,
            inferencer.type_param_vars.clone(),
        )
        .generate(&module, &[])
    }

    #[rstest]
    #[case::simple_add("fn add(a: Int, b: Int) -> Int { a + b }")]
    #[case::bool_return("fn is_positive(x: Int) -> Bool { x > 0 }")]
    #[case::string_concat(r#"fn greet(name: String) -> String { "Hello " <> name }"#)]
    #[case::function_call("fn double(x: Int) -> Int { add(x, x) }")]
    #[case::let_binding("fn test() -> Int { let x: Int = 5 x }")]
    #[case::no_return("fn side_effect(x: Int) { add(x, x) }")]
    #[case::modulo("fn rem(a: Int, b: Int) -> Int { a % b }")]
    #[case::negation("fn neg(x: Int) -> Int { -x }")]
    #[case::logical_not("fn invert(x: Bool) -> Bool { !x }")]
    fn codegen_snapshot(#[case] source: &str) {
        insta::assert_snapshot!(source, compile(source));
    }

    #[test]
    fn case_bool() {
        insta::assert_snapshot!(compile(
            "fn check(x: Bool) -> Int { case x { True -> 1 False -> 0 } }"
        ));
    }

    #[test]
    fn pipe_codegen() {
        insta::assert_snapshot!(compile("fn test(x: Int) -> Int { x |> add(1) }"));
    }

    #[test]
    fn multi_function() {
        insta::assert_snapshot!(compile(
            r#"
fn add(a: Int, b: Int) -> Int { a + b }
fn mul(a: Int, b: Int) -> Int { a * b }
fn combined(x: Int) -> Int { add(x, mul(x, 2)) }
"#
        ));
    }

    #[test]
    fn field_access() {
        insta::assert_snapshot!(compile("fn get_wave(m: Int) -> Int { m.wave }"));
    }

    #[test]
    fn method_call() {
        insta::assert_snapshot!(compile("fn test(h: Unit) -> Bool { h.is_alive() }"));
    }

    #[test]
    fn soa_enum_type() {
        insta::assert_snapshot!(compile(
            "pub type Phase { Lobby Playing { wave: Int } Victory { winner: Int } }"
        ));
    }

    #[test]
    fn soa_record_type() {
        insta::assert_snapshot!(compile(
            "pub type Model { Model { phase: Int, wave: Int, score: Int } }"
        ));
    }

    #[test]
    fn constructor_call() {
        insta::assert_snapshot!(compile(
            r#"
pub type Model { Model { wave: Int, score: Int } }
fn make() -> Int { Model(wave: 1, score: 100) }
"#
        ));
    }

    #[test]
    fn record_update_codegen() {
        insta::assert_snapshot!(compile(
            r#"
pub type Model { Model { wave: Int, score: Int } }
fn bump(m: Int) -> Int { Model(..m, wave: 5) }
"#
        ));
    }

    #[test]
    fn tuple_creation() {
        insta::assert_snapshot!(compile("fn make() -> Int { #(1, 2, 3) }"));
    }

    #[test]
    fn tuple_in_function() {
        insta::assert_snapshot!(compile(
            r#"
fn pair(a: Int, b: Int) -> Int { #(a, b) }
"#
        ));
    }

    #[test]
    fn list_literal() {
        insta::assert_snapshot!(compile("fn nums() -> Int { [1, 2, 3] }"));
    }

    #[test]
    fn empty_list() {
        insta::assert_snapshot!(compile("fn empty() -> Int { [] }"));
    }

    #[test]
    fn list_with_type_def() {
        insta::assert_snapshot!(compile(
            r#"
pub type Model { Model { wave: Int } }
fn test() -> Int { [1, 2, 3] }
"#
        ));
    }

    #[test]
    fn lambda_no_capture() {
        insta::assert_snapshot!(compile("fn test() -> Int { fn(x: Int) { x + 1 } }"));
    }

    #[test]
    fn lambda_with_capture() {
        insta::assert_snapshot!(compile("fn test(y: Int) -> Int { fn(x: Int) { x + y } }"));
    }

    #[test]
    fn topo_sort_forward_reference() {
        // b calls a, but a is defined after b → topo sort should put a first
        let result = compile("fn b() -> Int { a() }\nfn a() -> Int { 42 }");
        let a_pos = result.find("function glass_a").unwrap();
        let b_pos = result.find("function glass_b").unwrap();
        assert!(
            a_pos < b_pos,
            "a should appear before b in output (a at {}, b at {})",
            a_pos,
            b_pos
        );
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
/// Entry points: pub functions, init/update/subscriptions (Elm), type definitions.
fn dead_code_eliminate(defs: &[Definition]) -> Vec<Definition> {
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
            // Pub functions and Elm entry points
            Definition::Function(f) => {
                f.is_pub || matches!(f.name.as_str(), "init" | "update" | "subscriptions")
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
        _ => {}
    }
}
