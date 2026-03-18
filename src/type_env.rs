// Type environment for the Glass type checker.
//
// Tracks variable→type bindings in nested scopes.
// Preloaded with built-in types and JASS handle types.

#![allow(dead_code)]

use std::collections::{BTreeSet, HashMap};

use crate::type_repr::{Type, TypeScheme, TypeVarGen, TypeVarId};

/// A scoped type environment.
#[derive(Clone, Debug)]
pub struct TypeEnv {
    /// Stack of scopes. Last = innermost.
    scopes: Vec<HashMap<String, TypeScheme>>,
}

impl TypeEnv {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
        }
    }

    /// Create environment preloaded with Glass built-in types.
    pub fn with_builtins() -> Self {
        let mut env = Self::new();
        // Arithmetic: Int -> Int -> Int
        let ii_i = TypeScheme::mono(Type::Fn(
            vec![Type::int(), Type::int()],
            Box::new(Type::int()),
        ));
        env.bind("add".into(), ii_i.clone());
        env.bind("sub".into(), ii_i.clone());
        env.bind("mul".into(), ii_i.clone());
        env.bind("div".into(), ii_i);
        env
    }

    /// Push a new scope.
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// Pop the innermost scope.
    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    /// Bind a name in the current (innermost) scope.
    pub fn bind(&mut self, name: String, scheme: TypeScheme) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, scheme);
        }
    }

    /// Look up a name, searching from innermost to outermost scope.
    pub fn lookup(&self, name: &str) -> Option<&TypeScheme> {
        for scope in self.scopes.iter().rev() {
            if let Some(scheme) = scope.get(name) {
                return Some(scheme);
            }
        }
        None
    }

    /// Instantiate a type scheme with fresh type variables.
    /// ∀a,b. a -> b  becomes  ?3 -> ?4  (with fresh vars)
    pub fn instantiate(&self, scheme: &TypeScheme, var_gen: &mut TypeVarGen) -> Type {
        if scheme.vars.is_empty() {
            return scheme.ty.clone();
        }
        let mut subst = crate::type_repr::Substitution::new();
        for var in &scheme.vars {
            subst.bind(*var, var_gen.fresh());
        }
        scheme.ty.apply(&subst)
    }

    /// Generalize a type over variables not free in the environment.
    /// Used for let-polymorphism.
    pub fn generalize(&self, ty: &Type) -> TypeScheme {
        let env_fv = self.free_vars();
        let ty_fv = ty.free_vars();
        let vars: Vec<TypeVarId> = ty_fv.difference(&env_fv).copied().collect();
        TypeScheme {
            vars,
            ty: ty.clone(),
        }
    }

    /// Collect all free type variables in the environment.
    fn free_vars(&self) -> BTreeSet<TypeVarId> {
        let mut fv = BTreeSet::new();
        for scope in &self.scopes {
            for scheme in scope.values() {
                fv.extend(scheme.free_vars());
            }
        }
        fv
    }
}

/// Type information for a user-defined type constructor.
#[derive(Clone, Debug)]
pub struct ConstructorInfo {
    /// The type this constructor belongs to (e.g., "Phase")
    pub type_name: String,
    /// Field types for this variant (in order)
    pub field_types: Vec<(String, Type)>,
    /// The full type returned by this constructor
    pub result_type: Type,
    /// Type variable IDs for generic type params (empty for non-generic types)
    pub type_var_ids: Vec<u32>,
}

/// Registry of all known type constructors (from type definitions).
#[derive(Clone, Debug, Default)]
pub struct ConstructorRegistry {
    pub constructors: HashMap<String, ConstructorInfo>,
}

impl ConstructorRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, name: String, info: ConstructorInfo) {
        self.constructors.insert(name, info);
    }

    pub fn lookup(&self, name: &str) -> Option<&ConstructorInfo> {
        self.constructors.get(name)
    }

    pub fn all(&self) -> impl Iterator<Item = &ConstructorInfo> {
        self.constructors.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_and_lookup() {
        let mut env = TypeEnv::new();
        env.bind("x".into(), TypeScheme::mono(Type::int()));
        assert_eq!(env.lookup("x").map(|s| &s.ty), Some(&Type::int()));
        assert!(env.lookup("y").is_none());
    }

    #[test]
    fn scoped_lookup() {
        let mut env = TypeEnv::new();
        env.bind("x".into(), TypeScheme::mono(Type::int()));

        env.push_scope();
        // Inner scope shadows outer
        env.bind("x".into(), TypeScheme::mono(Type::string()));
        assert_eq!(env.lookup("x").map(|s| &s.ty), Some(&Type::string()));

        // y only in inner
        env.bind("y".into(), TypeScheme::mono(Type::float()));
        assert_eq!(env.lookup("y").map(|s| &s.ty), Some(&Type::float()));

        env.pop_scope();
        // x reverts to outer
        assert_eq!(env.lookup("x").map(|s| &s.ty), Some(&Type::int()));
        // y gone
        assert!(env.lookup("y").is_none());
    }

    #[test]
    fn instantiate_mono() {
        let env = TypeEnv::new();
        let mut var_gen = TypeVarGen::new();
        let scheme = TypeScheme::mono(Type::int());
        let t = env.instantiate(&scheme, &mut var_gen);
        assert_eq!(t, Type::int());
    }

    #[test]
    fn instantiate_poly() {
        let env = TypeEnv::new();
        let mut var_gen = TypeVarGen::new();

        // ∀a. a -> a
        let scheme = TypeScheme {
            vars: vec![100], // use a high id to show renaming
            ty: Type::Fn(vec![Type::Var(100)], Box::new(Type::Var(100))),
        };

        let t = env.instantiate(&scheme, &mut var_gen);
        // Should be ?0 -> ?0 (fresh var)
        match &t {
            Type::Fn(params, ret) => {
                assert_eq!(params.len(), 1);
                assert_eq!(params[0], **ret); // same fresh var
                assert!(matches!(params[0], Type::Var(0)));
            }
            _ => panic!("expected Fn, got {t}"),
        }
    }

    #[test]
    fn generalize_no_env_vars() {
        let env = TypeEnv::new();
        // Type: ?0 -> ?0
        let ty = Type::Fn(vec![Type::Var(0)], Box::new(Type::Var(0)));
        let scheme = env.generalize(&ty);
        // Should generalize ?0
        assert_eq!(scheme.vars, vec![0]);
    }

    #[test]
    fn generalize_with_env_vars() {
        let mut env = TypeEnv::new();
        // Env has ?1 free (bound to some scheme)
        env.bind(
            "x".into(),
            TypeScheme {
                vars: vec![],
                ty: Type::Var(1),
            },
        );

        // Type: ?0 -> ?1
        let ty = Type::Fn(vec![Type::Var(0)], Box::new(Type::Var(1)));
        let scheme = env.generalize(&ty);
        // Should generalize ?0 but NOT ?1 (free in env)
        assert_eq!(scheme.vars, vec![0]);
    }

    #[test]
    fn constructor_registry() {
        let mut reg = ConstructorRegistry::new();
        reg.register(
            "Some".into(),
            ConstructorInfo {
                type_name: "Option".into(),
                field_types: vec![("value".into(), Type::Var(0))],
                result_type: Type::option(Type::Var(0)),
                type_var_ids: vec![0],
            },
        );
        reg.register(
            "None".into(),
            ConstructorInfo {
                type_name: "Option".into(),
                field_types: vec![],
                result_type: Type::option(Type::Var(0)),
                type_var_ids: vec![0],
            },
        );

        assert!(reg.lookup("Some").is_some());
        assert!(reg.lookup("None").is_some());
        assert!(reg.lookup("Unknown").is_none());
    }
}
