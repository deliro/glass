// Type inference engine for Glass (Algorithm W).
//
// Walks the AST, assigns types to expressions, collects constraints,
// and unifies to produce a fully-typed program.

#![allow(dead_code)]

mod expr;
mod pattern;
mod resolve;

use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::token::Span;
use crate::type_env::{ConstructorInfo, ConstructorRegistry, TypeEnv};
use crate::type_repr::TypeVarId;
use crate::type_repr::{Substitution, Type, TypeScheme, TypeVarGen};
use crate::unify::{self, UnifyError};

/// Result of type inference for a module.
pub struct InferResult {
    pub errors: Vec<TypeError>,
    /// Map from AST node span (start, end) to its resolved type.
    pub type_map: HashMap<(usize, usize), Type>,
}

#[derive(Debug)]
pub struct TypeError {
    pub message: String,
    pub span: Span,
}

impl From<UnifyError> for TypeError {
    fn from(e: UnifyError) -> Self {
        TypeError {
            message: e.message,
            span: e.span,
        }
    }
}

pub struct Inferencer {
    pub var_gen: TypeVarGen,
    pub subst: Substitution,
    pub errors: Vec<TypeError>,
    pub constructors: ConstructorRegistry,
    /// Inferred types collected during inference (for monomorphization).
    /// Stores concrete App types seen at call/construction sites.
    pub inferred_types: Vec<Type>,
    /// Map from AST node span (start, end) to its inferred type.
    /// Types are stored pre-substitution; finalized in `build_type_map`.
    pub(super) type_map_raw: HashMap<(usize, usize), Type>,
    /// For each generic function, maps type param names to their TypeVarIds.
    /// e.g. "insert" → {"k" → 42, "v" → 43}
    pub type_param_vars: HashMap<String, HashMap<String, TypeVarId>>,
    pub(super) const_types: HashMap<String, Type>,
    pub ambiguous_names: HashMap<String, Vec<String>>,
    pub(super) module_names: HashSet<String>,
}

impl Inferencer {
    pub fn new() -> Self {
        Self {
            var_gen: TypeVarGen::new(),
            subst: Substitution::new(),
            errors: Vec::new(),
            constructors: ConstructorRegistry::new(),
            inferred_types: Vec::new(),
            type_map_raw: HashMap::new(),
            type_param_vars: HashMap::new(),
            const_types: HashMap::new(),
            module_names: HashSet::new(),
            ambiguous_names: HashMap::new(),
        }
    }

    /// Record the inferred type for an AST node at the given span.
    fn record_type(&mut self, span: Span, ty: &Type) {
        self.type_map_raw.insert((span.start, span.end), ty.clone());
    }

    /// After inference, apply the final substitution to all recorded types.
    pub fn build_type_map(&self) -> HashMap<(usize, usize), Type> {
        self.type_map_raw
            .iter()
            .map(|(&span, ty)| (span, ty.apply(&self.subst)))
            .collect()
    }

    /// Run inference on a full module.
    pub fn infer_module(&mut self, module: &Module) -> InferResult {
        self.infer_module_with_imports(module, &[], &HashMap::new())
    }

    /// Run inference with import namespace information.
    pub fn infer_module_with_imports(
        &mut self,
        module: &Module,
        imports: &[crate::modules::ResolvedImport],
        def_module_map: &HashMap<usize, String>,
    ) -> InferResult {
        let mut env = TypeEnv::with_builtins();

        // Build a map: definition name → which modules it came from
        // A name can appear in multiple modules (e.g., int.to_string vs float.to_string)
        let mut name_to_modules: HashMap<String, Vec<&crate::modules::ResolvedImport>> =
            HashMap::new();
        for imp in imports {
            if imp.qualified {
                self.module_names.insert(imp.module_name.clone());
            }
            for def in &imp.definitions {
                if let Some(name) = crate::modules::def_name_pub(def) {
                    name_to_modules
                        .entry(name.to_string())
                        .or_default()
                        .push(imp);
                }
            }
        }

        // def_module_map: (def_index in merged module) → source module name
        // Built during module resolution where we know exactly which import each def comes from.

        // Phase 1: Register all type definitions and their constructors
        for def in &module.definitions {
            if let Definition::Type(td) = def {
                self.register_type_def(td, &mut env);
                // Register qualified names if module provides qualified access
                if let Some(imps) = name_to_modules.get(td.name.as_str()) {
                    for imp in imps {
                        self.register_qualified_type(td, imp, &mut env);
                    }
                }
            }
        }

        // Phase 2: Register function signatures (forward declarations)
        for (def_idx, def) in module.definitions.iter().enumerate() {
            if let Definition::Function(f) = def {
                let (fn_type, tvars) = self.fn_def_type(f);
                let type_var_ids: Vec<u32> = tvars
                    .values()
                    .filter_map(|t| {
                        if let Type::Var(id) = t {
                            Some(*id)
                        } else {
                            None
                        }
                    })
                    .collect();
                // Save type param mapping for monomorphization
                let var_id_map: HashMap<String, TypeVarId> = tvars
                    .iter()
                    .filter_map(|(name, ty)| {
                        if let Type::Var(id) = ty {
                            Some((name.clone(), *id))
                        } else {
                            None
                        }
                    })
                    .collect();
                if !var_id_map.is_empty() {
                    self.type_param_vars.insert(f.name.clone(), var_id_map);
                }
                let scheme = TypeScheme {
                    vars: type_var_ids,
                    ty: fn_type,
                };
                let is_imported = def_module_map.contains_key(&def_idx);
                let collides = is_imported
                    && name_to_modules.get(f.name.as_str()).is_some_and(|imps| {
                        let unique_modules: HashSet<&str> =
                            imps.iter().map(|i| i.module_name.as_str()).collect();
                        unique_modules.len() > 1
                    });
                if !collides {
                    env.bind(f.name.clone(), scheme.clone());
                }
                if collides && let Some(imps) = name_to_modules.get(f.name.as_str()) {
                    let modules: Vec<String> = imps.iter().map(|i| i.module_name.clone()).collect();
                    self.ambiguous_names.insert(f.name.clone(), modules);
                }
                if let Some(src_mod) = def_module_map.get(&def_idx) {
                    let qname = format!("{}.{}", src_mod, f.name);
                    env.bind(qname, scheme);
                }
            }
        }

        // Phase 2b: Register external function signatures
        for (def_idx, def) in module.definitions.iter().enumerate() {
            if let Definition::External(e) = def {
                let mut tvars = HashMap::new();
                let param_types: Vec<Type> = e
                    .params
                    .iter()
                    .map(|p| self.resolve_type_expr_with_tvars(&p.type_expr, &mut tvars))
                    .collect();
                let ret_type = e
                    .return_type
                    .as_ref()
                    .map(|t| self.resolve_type_expr_with_tvars(t, &mut tvars))
                    .unwrap_or_else(|| self.var_gen.fresh());
                let fn_type = Type::Fn(param_types, Box::new(ret_type));
                let type_var_ids: Vec<u32> = tvars
                    .values()
                    .filter_map(|t| {
                        if let Type::Var(id) = t {
                            Some(*id)
                        } else {
                            None
                        }
                    })
                    .collect();
                let scheme = TypeScheme {
                    vars: type_var_ids,
                    ty: fn_type,
                };
                let collides = name_to_modules
                    .get(e.fn_name.as_str())
                    .is_some_and(|imps| imps.len() > 1);
                if !collides {
                    env.bind(e.fn_name.clone(), scheme.clone());
                }
                if collides && let Some(imps) = name_to_modules.get(e.fn_name.as_str()) {
                    let modules: Vec<String> = imps.iter().map(|i| i.module_name.clone()).collect();
                    self.ambiguous_names.insert(e.fn_name.clone(), modules);
                }
                if let Some(src_mod) = def_module_map.get(&def_idx) {
                    let qname = format!("{}.{}", src_mod, e.fn_name);
                    env.bind(qname, scheme);
                }
            }
        }

        // Build set of imported function names (to skip re-checking their bodies)
        // Imported bodies are type-checked separately per module to avoid type var conflicts.
        let imported_fns: std::collections::HashSet<String> = imports
            .iter()
            .flat_map(|imp| imp.definitions.iter())
            .filter_map(|d| match d {
                Definition::Function(f) => Some(f.name.clone()),
                _ => None,
            })
            .collect();

        // Phase 2c: Register const types and bind them in env
        for def in &module.definitions {
            if let Definition::Const(c) = def {
                let const_type = match &c.type_expr {
                    Some(t) => self.resolve_type_expr(t),
                    None => self.infer_expr(&c.value, &mut env),
                };
                self.const_types.insert(c.name.clone(), const_type.clone());
                env.bind(
                    c.name.clone(),
                    crate::type_repr::TypeScheme {
                        vars: vec![],
                        ty: const_type,
                    },
                );
            }
        }

        // Phase 2d: Register extend method signatures
        for def in &module.definitions {
            if let Definition::Extend(ext) = def {
                for method in &ext.methods {
                    let (fn_type, tvars) = self.fn_def_type(method);
                    let type_var_ids: Vec<u32> = tvars
                        .values()
                        .filter_map(|t| {
                            if let Type::Var(id) = t {
                                Some(*id)
                            } else {
                                None
                            }
                        })
                        .collect();
                    let prefixed = format!("{}_{}", ext.type_name, method.name);
                    let scheme = TypeScheme {
                        vars: type_var_ids,
                        ty: fn_type,
                    };
                    env.bind(prefixed.clone(), scheme.clone());
                    env.bind(method.name.clone(), scheme);
                }
            }
        }

        // Phase 3: Infer user function bodies only
        for def in &module.definitions {
            if let Definition::Function(f) = def
                && !imported_fns.contains(&f.name)
            {
                self.check_function(f, &mut env);
            }
        }

        // Phase 3: Infer extend method bodies
        for def in &module.definitions {
            if let Definition::Extend(ext) = def {
                for method in &ext.methods {
                    self.check_function(method, &mut env);
                }
            }
        }

        // Phase 3b: Infer imported function bodies in isolated contexts
        // Each import gets its own substitution to avoid type var contamination.
        // After checking each body, apply the per-import substitution to recorded
        // type_map entries so field access expressions resolve correctly.
        let saved_subst = self.subst.clone();
        let saved_errors_len = self.errors.len();
        for imp in imports {
            for def in &imp.definitions {
                if let Definition::Function(f) = def {
                    self.subst = Substitution::new();
                    self.check_function(f, &mut env);
                }
            }
        }
        // Restore: discard any errors from re-checking imported bodies
        // (they were already validated when their module was compiled)
        self.errors.truncate(saved_errors_len);
        self.subst = saved_subst;

        InferResult {
            errors: std::mem::take(&mut self.errors),
            type_map: self.build_type_map(),
        }
    }

    /// Register a type definition's constructors in the environment.
    fn register_type_def(&mut self, td: &TypeDef, env: &mut TypeEnv) {
        // Map type param names (e.g. "T", "A") to fresh type variables
        let type_param_map: Vec<(String, Type)> = td
            .type_params
            .iter()
            .map(|name| (name.clone(), self.var_gen.fresh()))
            .collect();

        let result_type = if type_param_map.is_empty() {
            Type::Con(td.name.clone())
        } else {
            let params: Vec<Type> = type_param_map.iter().map(|(_, tv)| tv.clone()).collect();
            Type::App(Box::new(Type::Con(td.name.clone())), params)
        };

        // Collect all type var IDs used in this type def (for generalization)
        let type_var_ids: Vec<u32> = type_param_map
            .iter()
            .filter_map(|(_, tv)| {
                if let Type::Var(id) = tv {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect();

        for ctor in &td.constructors {
            let field_types: Vec<(String, Type)> = ctor
                .fields
                .iter()
                .map(|f| {
                    (
                        f.name.clone(),
                        Self::resolve_type_expr_with_params(&f.type_expr, &type_param_map),
                    )
                })
                .collect();

            let ctor_type = if field_types.is_empty() {
                result_type.clone()
            } else {
                let param_types: Vec<Type> = field_types.iter().map(|(_, t)| t.clone()).collect();
                Type::Fn(param_types, Box::new(result_type.clone()))
            };

            // Generalize over type params so each use gets fresh vars
            let scheme = TypeScheme {
                vars: type_var_ids.clone(),
                ty: ctor_type,
            };
            // For structs: bind bare name (struct name == constructor name).
            // For enums: bind qualified Type::Variant only (no bare name leaking).
            if td.is_struct {
                env.bind(ctor.name.clone(), scheme.clone());
            }
            let qualified_name = format!("{}::{}", td.name, ctor.name);
            env.bind(qualified_name.clone(), scheme);
            self.constructors.register(
                ctor.name.clone(),
                ConstructorInfo {
                    type_name: td.name.clone(),
                    field_types: field_types.clone(),
                    result_type: result_type.clone(),
                    type_var_ids: type_var_ids.clone(),
                },
            );
            // Also register qualified name in constructor registry
            self.constructors.register(
                qualified_name,
                ConstructorInfo {
                    type_name: td.name.clone(),
                    field_types,
                    result_type: result_type.clone(),
                    type_var_ids: type_var_ids.clone(),
                },
            );
        }
    }

    /// Register qualified name aliases for a type's constructors (e.g. option.Some, option.None).
    fn register_qualified_type(
        &mut self,
        td: &TypeDef,
        imp: &crate::modules::ResolvedImport,
        env: &mut TypeEnv,
    ) {
        if !imp.qualified {
            return;
        }
        let prefix = &imp.module_name;
        for ctor in &td.constructors {
            if let Some(scheme) = env.lookup(&ctor.name) {
                let qname = format!("{}.{}", prefix, ctor.name);
                env.bind(qname, scheme.clone());
            }
        }
        // Also register qualified constructor in registry
        for ctor in &td.constructors {
            if let Some(info) = self.constructors.lookup(&ctor.name) {
                let qname = format!("{}.{}", prefix, ctor.name);
                self.constructors.register(qname, info.clone());
            }
        }
    }

    /// Resolve a type expression, substituting type param names with their type vars.
    fn resolve_type_expr_with_params(ty: &TypeExpr, params: &[(String, Type)]) -> Type {
        match ty {
            TypeExpr::Named { name, args } => {
                // Check if it's a type parameter
                if let Some((_, tv)) = params.iter().find(|(n, _)| n == name)
                    && args.is_empty()
                {
                    return tv.clone();
                }
                // Strip qualified prefix (module.Type → Type)
                let bare_name = name
                    .find('.')
                    .map(|pos| &name[pos + 1..])
                    .unwrap_or(name.as_str());
                let base = match bare_name {
                    "Int" => Type::Con("Int".to_string()),
                    "Float" => Type::Con("Float".to_string()),
                    "Bool" => Type::Con("Bool".to_string()),
                    "String" => Type::Con("String".to_string()),
                    _ => Type::Con(bare_name.to_string()),
                };
                if args.is_empty() {
                    base
                } else {
                    let resolved_args: Vec<Type> = args
                        .iter()
                        .map(|a| Self::resolve_type_expr_with_params(a, params))
                        .collect();
                    Type::App(Box::new(base), resolved_args)
                }
            }
            TypeExpr::Fn {
                params: fn_params,
                ret,
            } => {
                let p: Vec<Type> = fn_params
                    .iter()
                    .map(|a| Self::resolve_type_expr_with_params(a, params))
                    .collect();
                Type::Fn(
                    p,
                    Box::new(Self::resolve_type_expr_with_params(ret, params)),
                )
            }
            TypeExpr::Tuple(elems) => Type::Tuple(
                elems
                    .iter()
                    .map(|a| Self::resolve_type_expr_with_params(a, params))
                    .collect(),
            ),
        }
    }

    /// Build the type of a function definition from its signature.
    /// Returns (fn_type, tvars_map) so check_function can reuse the same type vars.
    fn fn_def_type(&mut self, f: &FnDef) -> (Type, HashMap<String, Type>) {
        let mut tvars = HashMap::new();
        let param_types: Vec<Type> = f
            .params
            .iter()
            .map(|p| self.resolve_type_expr_with_tvars(&p.type_expr, &mut tvars))
            .collect();
        let ret_type = f
            .return_type
            .as_ref()
            .map(|t| self.resolve_type_expr_with_tvars(t, &mut tvars))
            .unwrap_or_else(|| self.var_gen.fresh());

        let fn_type = Type::Fn(param_types, Box::new(ret_type));
        (fn_type, tvars)
    }

    /// Check a function body against its declared signature.
    fn check_function(&mut self, f: &FnDef, env: &mut TypeEnv) {
        let mut tvars = HashMap::new();
        env.push_scope();

        for p in &f.params {
            let ty = self.resolve_type_expr_with_tvars(&p.type_expr, &mut tvars);
            env.bind(p.name.clone(), TypeScheme::mono(ty));
        }

        let body_type = self.infer_expr(&f.body, env);
        let declared_ret = f
            .return_type
            .as_ref()
            .map(|t| self.resolve_type_expr_with_tvars(t, &mut tvars));

        if let Some(ret) = declared_ret {
            let body_resolved = body_type.apply(&self.subst);
            let ret_resolved = ret.apply(&self.subst);
            if let Err(e) = unify::unify(&body_resolved, &ret_resolved, f.body.span) {
                self.errors.push(e.into());
            }
        }

        // Save type param mapping for monomorphization
        // Extract TypeVarIds from the Type::Var entries in tvars
        if !tvars.is_empty() {
            let var_ids: HashMap<String, TypeVarId> = tvars
                .iter()
                .filter_map(|(name, ty)| {
                    if let Type::Var(id) = ty {
                        Some((name.clone(), *id))
                    } else {
                        None
                    }
                })
                .collect();
            if !var_ids.is_empty() {
                self.type_param_vars.insert(f.name.clone(), var_ids);
            }
        }

        env.pop_scope();
    }

    /// Infer the type of an expression, updating substitution.
    pub fn infer_expr(&mut self, expr: &Spanned<Expr>, env: &mut TypeEnv) -> Type {
        let ty = self.infer_expr_inner(expr, env);
        self.record_type(expr.span, &ty);
        ty
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use crate::token::Lexer;
    use rstest::rstest;

    fn infer(source: &str) -> InferResult {
        let tokens = Lexer::tokenize(source).expect("lex failed");
        let mut parser = Parser::new(tokens);
        let module = {
            let _o = parser.parse_module();
            assert!(_o.errors.is_empty(), "parse errors: {:?}", _o.errors);
            _o.module
        };
        let mut inferencer = Inferencer::new();
        inferencer.infer_module(&module)
    }

    fn infer_ok(source: &str) {
        let result = infer(source);
        assert!(
            result.errors.is_empty(),
            "expected no errors, got: {:?}",
            result.errors
        );
    }

    fn infer_errors(source: &str) -> Vec<String> {
        infer(source)
            .errors
            .iter()
            .map(|e| e.message.clone())
            .collect()
    }

    // === Literals ===

    #[test]
    fn literal_types() {
        infer_ok("fn test() -> Int { 42 }");
        infer_ok("fn test() -> Float { 3.14 }");
        infer_ok("fn test() -> Bool { True }");
        infer_ok(r#"fn test() -> String { "hello" }"#);
    }

    // === Return type mismatch ===

    #[test]
    fn return_type_mismatch() {
        let errs = infer_errors("fn test() -> Int { True }");
        assert!(!errs.is_empty());
        assert!(errs[0].contains("type mismatch"));
    }

    #[test]
    fn return_type_mismatch_string_int() {
        let errs = infer_errors(r#"fn test() -> Int { "hello" }"#);
        assert!(!errs.is_empty());
    }

    // === Variables ===

    #[test]
    fn variable_type() {
        infer_ok("fn test(x: Int) -> Int { x }");
    }

    #[test]
    fn variable_type_mismatch() {
        let errs = infer_errors("fn test(x: Int) -> String { x }");
        assert!(!errs.is_empty());
    }

    #[test]
    fn undefined_variable() {
        let errs = infer_errors("fn test() -> Int { y }");
        assert!(errs.iter().any(|e| e.contains("undefined variable")));
    }

    // === Let binding ===

    #[test]
    fn let_binding_inferred() {
        infer_ok("fn test() -> Int { let x = 5 x }");
    }

    #[test]
    fn let_binding_annotated() {
        infer_ok("fn test() -> Int { let x: Int = 5 x }");
    }

    #[test]
    fn let_binding_annotation_mismatch() {
        let errs = infer_errors(r#"fn test() -> Int { let x: Int = "hello" x }"#);
        assert!(!errs.is_empty());
    }

    // === Binary operations ===

    #[rstest]
    #[case::add_int("fn t() -> Int { 1 + 2 }")]
    #[case::sub_int("fn t() -> Int { 5 - 3 }")]
    #[case::mul_int("fn t() -> Int { 2 * 3 }")]
    #[case::div_int("fn t() -> Int { 10 / 2 }")]
    #[case::comparison("fn t() -> Bool { 1 < 2 }")]
    #[case::equality("fn t() -> Bool { 1 == 2 }")]
    #[case::logical_and("fn t() -> Bool { True && False }")]
    #[case::logical_or("fn t() -> Bool { True || False }")]
    #[case::string_concat(r#"fn t() -> String { "a" <> "b" }"#)]
    fn binop_ok(#[case] source: &str) {
        infer_ok(source);
    }

    #[test]
    fn binop_type_mismatch() {
        let errs = infer_errors(r#"fn t() -> Int { 1 + "hello" }"#);
        assert!(!errs.is_empty());
    }

    #[test]
    fn concat_non_string() {
        let errs = infer_errors("fn t() -> String { 1 <> 2 }");
        assert!(!errs.is_empty());
    }

    // === Function calls ===

    #[test]
    fn function_call() {
        infer_ok(
            r#"
fn add(a: Int, b: Int) -> Int { a + b }
fn test() -> Int { add(1, 2) }
"#,
        );
    }

    #[test]
    fn function_call_wrong_arg_type() {
        let errs = infer_errors(
            r#"
fn add(a: Int, b: Int) -> Int { a + b }
fn test() -> Int { add(1, "two") }
"#,
        );
        assert!(!errs.is_empty());
    }

    // === Tuples ===

    #[test]
    fn tuple_type() {
        infer_ok("fn test() -> (Int, String) { (1, \"hello\") }");
    }

    #[test]
    fn tuple_mismatch() {
        let errs = infer_errors("fn test() -> (Int, Int) { (1, \"hello\") }");
        assert!(!errs.is_empty());
    }

    // === Lists ===

    #[test]
    fn list_homogeneous() {
        infer_ok("fn test() -> List(Int) { [1, 2, 3] }");
    }

    #[test]
    fn list_heterogeneous_error() {
        let errs = infer_errors(r#"fn test() -> List(Int) { [1, "two", 3] }"#);
        assert!(!errs.is_empty());
    }

    #[test]
    fn empty_list() {
        // Empty list has polymorphic element type — should unify with any List(T)
        infer_ok("fn test() -> List(Int) { [] }");
    }

    // === Case expressions ===

    #[test]
    fn case_bool() {
        infer_ok(
            r#"
fn test(x: Bool) -> Int {
    case x {
        True -> 1
        False -> 0
    }
}
"#,
        );
    }

    #[test]
    fn case_arms_type_mismatch() {
        let errs = infer_errors(
            r#"
fn test(x: Bool) -> Int {
    case x {
        True -> 1
        False -> "no"
    }
}
"#,
        );
        assert!(!errs.is_empty());
    }

    // === Custom types / constructors ===

    #[test]
    fn constructor_nullary() {
        infer_ok(
            r#"
pub enum Color { Red Green Blue }
fn test() -> Color { Color::Red }
"#,
        );
    }

    #[test]
    fn constructor_with_fields() {
        infer_ok(
            r#"
pub struct Pair { x: Int, y: Int }
fn test() -> Pair { Pair { x: 1, y: 2 } }
"#,
        );
    }

    #[test]
    fn constructor_wrong_field_type() {
        let errs = infer_errors(
            r#"
pub struct Pair { x: Int, y: Int }
fn test() -> Pair { Pair { x: 1, y: "two" } }
"#,
        );
        assert!(!errs.is_empty());
    }

    // === Lambda ===

    #[test]
    fn lambda_type() {
        infer_ok("fn test() -> fn(Int) -> Int { fn(x: Int) { x + 1 } }");
    }

    #[test]
    fn lambda_return_mismatch() {
        let errs = infer_errors("fn test() -> fn(Int) -> String { fn(x: Int) { x + 1 } }");
        assert!(!errs.is_empty());
    }

    // === Case with constructors ===

    #[test]
    fn case_with_constructors() {
        infer_ok(
            r#"
pub enum Shape { Circle { radius: Float } Square { side: Float } }
fn area(s: Shape) -> Float {
    case s {
        Shape::Circle(r) -> r * r * 3.14
        Shape::Square(s) -> s * s
    }
}
"#,
        );
    }

    // === Advanced patterns ===

    #[test]
    fn named_field_pattern() {
        infer_ok(
            r#"
pub enum Event { Chat { from: Int, text: String } Quit { player: Int } }
fn test(e: Event) -> Int {
    case e {
        Event::Chat { from, .. } -> from
        Event::Quit(p) -> p
    }
}
"#,
        );
    }

    #[test]
    fn named_field_pattern_with_as() {
        infer_ok(
            r#"
pub enum Event { Chat { from: Int, text: String } Quit { player: Int } }
fn test(e: Event) -> Int {
    case e {
        Event::Chat { from as p, .. } -> p
        _ -> 0
    }
}
"#,
        );
    }

    #[test]
    fn or_pattern_basic() {
        infer_ok(
            r#"
pub enum Color { Red Green Blue }
fn test(c: Color) -> Int {
    case c {
        Color::Red | Color::Green -> 1
        Color::Blue -> 2
    }
}
"#,
        );
    }

    #[test]
    fn as_binding_whole_pattern() {
        infer_ok(
            r#"
pub enum Event { Chat { from: Int, text: String } Quit { player: Int } GameStarted }
fn test(e: Event) -> Event {
    case e {
        Event::Chat(p, _) | Event::Quit(p) as event -> event
        _ -> e
    }
}
"#,
        );
    }

    #[test]
    fn named_field_missing_without_rest() {
        let errs = infer_errors(
            r#"
pub enum X { Y { val: Int, name: String } }
fn test(x: X) -> Int {
    case x {
        X::Y { val } -> val
        _ -> 0
    }
}
"#,
        );
        assert!(errs.iter().any(|e| e.contains("missing fields")));
        assert!(errs.iter().any(|e| e.contains("name")));
    }

    #[test]
    fn named_field_with_rest_ok() {
        infer_ok(
            r#"
pub enum X { Y { val: Int, name: String } }
fn test(x: X) -> Int {
    case x {
        X::Y { val, .. } -> val
        _ -> 0
    }
}
"#,
        );
    }

    #[test]
    fn named_field_all_listed_ok() {
        infer_ok(
            r#"
pub enum X { Y { val: Int, name: String } }
fn test(x: X) -> Int {
    case x {
        X::Y { val, name } -> val
        _ -> 0
    }
}
"#,
        );
    }

    #[test]
    fn named_field_unknown_field_error() {
        let errs = infer_errors(
            r#"
pub enum Event { Chat { from: Int, text: String } }
fn test(e: Event) -> Int {
    case e {
        Event::Chat { unknown, .. } -> 0
        _ -> 0
    }
}
"#,
        );
        assert!(errs.iter().any(|e| e.contains("unknown field")));
    }

    // === Multi-function program ===

    #[test]
    fn multi_function_program() {
        infer_ok(
            r#"
pub enum Msg { Tick Reset }
pub struct Model { count: Int }

fn init() -> Model { Model { count: 0 } }

fn update(model: Model, msg: Msg) -> Model {
    case msg {
        Msg::Tick -> Model { count: 1 }
        Msg::Reset -> Model { count: 0 }
    }
}
"#,
        );
    }

    #[test]
    fn type_map_populated() {
        let source = "fn add(a: Int, b: Int) -> Int { a + b }";
        let result = infer(source);
        assert!(result.errors.is_empty());
        // Type map should contain entries for expressions
        assert!(!result.type_map.is_empty(), "type_map should not be empty");
        // All resolved types should be concrete (no type variables)
        for (span, ty) in &result.type_map {
            assert!(
                ty.free_vars().is_empty(),
                "type at {:?} has unresolved vars: {}",
                span,
                ty
            );
        }
    }

    #[test]
    fn type_map_resolves_generics() {
        let source = r#"
enum Option(T) {
    Some(T)
    None
}
fn test() -> Option(Int) {
    Option::Some(42)
}
"#;
        let result = infer(source);
        assert!(result.errors.is_empty());
        // Find the Some(42) expression — its type should be Option(Int)
        let option_int = result
            .type_map
            .values()
            .find(|ty| matches!(ty, Type::App(_, _)))
            .expect("should have an App type in type_map");
        assert_eq!(option_int.to_string(), "Option(Int)");
    }

    #[test]
    fn type_to_jass() {
        assert_eq!(Type::int().to_jass(), "integer");
        assert_eq!(Type::float().to_jass(), "real");
        assert_eq!(Type::bool().to_jass(), "boolean");
        assert_eq!(Type::string().to_jass(), "string");
        assert_eq!(Type::Con("Unit".into()).to_jass(), "unit");
        assert_eq!(Type::Con("Timer".into()).to_jass(), "timer");
        assert_eq!(Type::list(Type::int()).to_jass(), "integer");
        assert_eq!(Type::option(Type::int()).to_jass(), "integer");
        assert_eq!(
            Type::Tuple(vec![Type::int(), Type::float()]).to_jass(),
            "integer"
        );
    }

    #[test]
    fn undefined_variable_suggestion() {
        let errs = infer_errors("fn test(length: Int) -> Int { lenght }");
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("undefined variable 'lenght'"));
        assert!(errs[0].contains("did you mean 'length'?"));
    }

    #[test]
    fn undefined_variable_no_suggestion_when_distant() {
        let errs = infer_errors("fn test(x: Int) -> Int { completely_unknown }");
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("undefined variable 'completely_unknown'"));
        assert!(!errs[0].contains("did you mean"));
    }

    #[test]
    fn unknown_field_suggestion_in_pattern() {
        let source = r#"
pub struct Model { wave: Int, score: Int }
fn test(m: Model) -> Int {
    case m {
        Model { wafe, .. } -> wafe
    }
}
"#;
        let errs = infer_errors(source);
        assert!(
            errs.iter()
                .any(|e| e.contains("unknown field 'wafe'") && e.contains("did you mean 'wave'?")),
            "expected suggestion for 'wafe', got: {:?}",
            errs
        );
    }

    #[test]
    fn unknown_field_no_suggestion_when_distant() {
        let source = r#"
pub struct Model { wave: Int }
fn test(m: Model) -> Int {
    case m {
        Model { zzz, .. } -> zzz
    }
}
"#;
        let errs = infer_errors(source);
        assert!(
            errs.iter()
                .any(|e| e.contains("unknown field 'zzz'") && !e.contains("did you mean")),
            "expected no suggestion for 'zzz', got: {:?}",
            errs
        );
    }

    #[test]
    fn arity_mismatch_too_few_args() {
        let source = r#"
fn add(a: Int, b: Int) -> Int { a + b }
fn test() -> Int { add(1) }
"#;
        let errs = infer_errors(source);
        assert!(
            errs.iter()
                .any(|e| e.contains("expects 2 arguments, got 1")),
            "expected arity mismatch error, got: {:?}",
            errs
        );
    }

    #[test]
    fn arity_mismatch_too_many_args() {
        let source = r#"
fn inc(a: Int) -> Int { a + 1 }
fn test() -> Int { inc(1, 2) }
"#;
        let errs = infer_errors(source);
        assert!(
            errs.iter()
                .any(|e| e.contains("expects 1 arguments, got 2")),
            "expected arity mismatch error, got: {:?}",
            errs
        );
    }

    #[test]
    fn unknown_constructor_suggestion() {
        let source = r#"
pub enum Phase { Lobby Playing Ended }
fn test() -> Phase { Lobbu }
"#;
        let errs = infer_errors(source);
        assert!(
            errs.iter()
                .any(|e| e.contains("unknown constructor 'Lobbu'")
                    && e.contains("did you mean 'Lobby'?")),
            "expected suggestion for 'Lobbu', got: {:?}",
            errs
        );
    }

    #[test]
    fn unknown_constructor_no_suggestion_when_distant() {
        let source = r#"
pub enum Phase { Lobby Playing Ended }
fn test() -> Phase { Xyzzy }
"#;
        let errs = infer_errors(source);
        assert!(
            errs.iter()
                .any(|e| e.contains("unknown constructor 'Xyzzy'") && !e.contains("did you mean")),
            "expected no suggestion for 'Xyzzy', got: {:?}",
            errs
        );
    }
}
