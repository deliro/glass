// Internal type representation for the Glass type checker.
//
// Separate from AST's TypeExpr — this is what the inference engine works with.
// TypeExpr is surface syntax; Type is the resolved, canonical form.

#![allow(dead_code)]

use std::collections::{BTreeSet, HashMap};
use std::fmt;

/// Unique identifier for a type variable (unification variable).
pub type TypeVarId = u32;

/// A type in the Glass type system.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Type {
    /// Type constructor: Int, Float, Bool, String, Unit, Timer, etc.
    Con(String),

    /// Type variable (unification variable, introduced during inference).
    Var(TypeVarId),

    /// Type application: List(Int), Option(Unit), etc.
    /// App(constructor, arguments)
    App(Box<Type>, Vec<Type>),

    /// Function type: fn(A, B) -> C
    Fn(Vec<Type>, Box<Type>),

    /// Tuple type: (A, B, C)
    Tuple(Vec<Type>),
}

impl Type {
    // Convenience constructors

    pub fn int() -> Self {
        Type::Con("Int".into())
    }
    pub fn float() -> Self {
        Type::Con("Float".into())
    }
    pub fn bool() -> Self {
        Type::Con("Bool".into())
    }
    pub fn string() -> Self {
        Type::Con("String".into())
    }
    pub fn unit_type() -> Self {
        Type::Con("Unit".into())
    }
    pub fn timer() -> Self {
        Type::Con("Timer".into())
    }
    pub fn group() -> Self {
        Type::Con("Group".into())
    }
    pub fn player() -> Self {
        Type::Con("Player".into())
    }

    pub fn list(elem: Type) -> Self {
        Type::App(Box::new(Type::Con("List".into())), vec![elem])
    }

    pub fn option(inner: Type) -> Self {
        Type::App(Box::new(Type::Con("Option".into())), vec![inner])
    }

    /// Convert a resolved Type to its JASS type string.
    pub fn to_jass(&self) -> &'static str {
        match self {
            Type::Con(name) => match name.as_str() {
                "Float" => "real",
                "Bool" => "boolean",
                "String" => "string",
                // JASS handle types
                "Unit" => "unit",
                "Player" => "player",
                "Timer" => "timer",
                "Group" => "group",
                "Trigger" => "trigger",
                "Force" => "force",
                "Widget" => "widget",
                "Destructable" => "destructable",
                "Item" => "item",
                "Ability" => "ability",
                "Buff" => "buff",
                "Sfx" => "effect",
                "Quest" => "quest",
                "Dialog" => "dialog",
                "Sound" => "sound",
                "Region" => "region",
                "Rect" => "rect",
                "Location" => "location",
                "Fogmodifier" => "fogmodifier",
                "Hashtable" => "hashtable",
                "Image" => "image",
                "Texttag" => "texttag",
                "Lightning" => "lightning",
                "Multiboard" => "multiboard",
                "Leaderboard" => "leaderboard",
                "Trackable" => "trackable",
                "Ubersplat" => "ubersplat",
                // Int and everything else → integer
                _ => "integer",
            },
            // Tuples, Lists, user types, App, Function types, and unresolved type variables → integer
            Type::App(_, _) | Type::Tuple(_) | Type::Fn(_, _) | Type::Var(_) => "integer",
        }
    }

    /// Collect all free type variables in this type.
    pub fn free_vars(&self) -> BTreeSet<TypeVarId> {
        match self {
            Type::Con(_) => BTreeSet::new(),
            Type::Var(id) => {
                let mut s = BTreeSet::new();
                s.insert(*id);
                s
            }
            Type::App(con, args) => {
                let mut s = con.free_vars();
                for a in args {
                    s.extend(a.free_vars());
                }
                s
            }
            Type::Fn(params, ret) => {
                let mut s = BTreeSet::new();
                for p in params {
                    s.extend(p.free_vars());
                }
                s.extend(ret.free_vars());
                s
            }
            Type::Tuple(elems) => {
                let mut s = BTreeSet::new();
                for e in elems {
                    s.extend(e.free_vars());
                }
                s
            }
        }
    }

    /// Apply a substitution to this type.
    pub fn apply(&self, subst: &Substitution) -> Type {
        match self {
            Type::Con(_) => self.clone(),
            Type::Var(id) => {
                match subst.0.get(id) {
                    Some(t) => t.apply(subst), // follow chains
                    None => self.clone(),
                }
            }
            Type::App(con, args) => Type::App(
                Box::new(con.apply(subst)),
                args.iter().map(|a| a.apply(subst)).collect(),
            ),
            Type::Fn(params, ret) => Type::Fn(
                params.iter().map(|p| p.apply(subst)).collect(),
                Box::new(ret.apply(subst)),
            ),
            Type::Tuple(elems) => Type::Tuple(elems.iter().map(|e| e.apply(subst)).collect()),
        }
    }
}

impl fmt::Debug for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Con(name) => write!(f, "{name}"),
            Type::Var(id) => write!(f, "?{id}"),
            Type::App(con, args) => {
                write!(f, "{con}(")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{a}")?;
                }
                write!(f, ")")
            }
            Type::Fn(params, ret) => {
                write!(f, "fn(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{p}")?;
                }
                write!(f, ") -> {ret}")
            }
            Type::Tuple(elems) => {
                write!(f, "(")?;
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{e}")?;
                }
                write!(f, ")")
            }
        }
    }
}

/// A substitution mapping type variables to types.
#[derive(Clone, Debug, Default)]
pub struct Substitution(pub HashMap<TypeVarId, Type>);

impl Substitution {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    /// Bind a type variable to a type.
    pub fn bind(&mut self, var: TypeVarId, ty: Type) {
        self.0.insert(var, ty);
    }

    /// Compose two substitutions: apply self first, then other.
    /// Result: for any type T, T.apply(result) == T.apply(self).apply(other)
    pub fn compose(&self, other: &Substitution) -> Substitution {
        let mut result: HashMap<TypeVarId, Type> =
            self.0.iter().map(|(k, v)| (*k, v.apply(other))).collect();
        // Add bindings from other that aren't in self
        for (k, v) in &other.0 {
            result.entry(*k).or_insert_with(|| v.clone());
        }
        Substitution(result)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn values(&self) -> impl Iterator<Item = &Type> {
        self.0.values()
    }
}

/// A type scheme: ∀ vars. type
/// Used for let-polymorphism: `let id = fn(x) { x }` has scheme ∀a. a -> a
#[derive(Clone, Debug)]
pub struct TypeScheme {
    pub vars: Vec<TypeVarId>,
    pub ty: Type,
}

impl TypeScheme {
    /// Monomorphic scheme (no quantified variables).
    pub fn mono(ty: Type) -> Self {
        TypeScheme {
            vars: Vec::new(),
            ty,
        }
    }

    pub fn free_vars(&self) -> BTreeSet<TypeVarId> {
        let mut fv = self.ty.free_vars();
        for v in &self.vars {
            fv.remove(v);
        }
        fv
    }
}

/// Counter for generating fresh type variables.
#[derive(Debug, Default)]
pub struct TypeVarGen {
    next_id: TypeVarId,
}

impl TypeVarGen {
    pub fn new() -> Self {
        Self { next_id: 0 }
    }

    pub fn fresh(&mut self) -> Type {
        let id = self.next_id;
        self.next_id += 1;
        Type::Var(id)
    }

    pub fn fresh_id(&mut self) -> TypeVarId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_display() {
        assert_eq!(Type::int().to_string(), "Int");
        assert_eq!(Type::Var(0).to_string(), "?0");
        assert_eq!(Type::list(Type::int()).to_string(), "List(Int)");
        assert_eq!(
            Type::Fn(vec![Type::int(), Type::int()], Box::new(Type::bool())).to_string(),
            "fn(Int, Int) -> Bool"
        );
        assert_eq!(
            Type::Tuple(vec![Type::int(), Type::string()]).to_string(),
            "(Int, String)"
        );
    }

    #[test]
    fn free_vars_concrete() {
        assert!(Type::int().free_vars().is_empty());
        assert!(Type::list(Type::int()).free_vars().is_empty());
    }

    #[test]
    fn free_vars_with_tvars() {
        let t = Type::Fn(vec![Type::Var(0)], Box::new(Type::Var(1)));
        let fv = t.free_vars();
        assert!(fv.contains(&0));
        assert!(fv.contains(&1));
        assert_eq!(fv.len(), 2);
    }

    #[test]
    fn substitution_apply() {
        let mut s = Substitution::new();
        s.bind(0, Type::int());

        assert_eq!(Type::Var(0).apply(&s), Type::int());
        assert_eq!(Type::Var(1).apply(&s), Type::Var(1));
        assert_eq!(Type::int().apply(&s), Type::int());
    }

    #[test]
    fn substitution_apply_nested() {
        let mut s = Substitution::new();
        s.bind(0, Type::int());

        let t = Type::list(Type::Var(0));
        assert_eq!(t.apply(&s), Type::list(Type::int()));
    }

    #[test]
    fn substitution_apply_fn() {
        let mut s = Substitution::new();
        s.bind(0, Type::int());
        s.bind(1, Type::string());

        let t = Type::Fn(vec![Type::Var(0)], Box::new(Type::Var(1)));
        assert_eq!(
            t.apply(&s),
            Type::Fn(vec![Type::int()], Box::new(Type::string()))
        );
    }

    #[test]
    fn substitution_chain() {
        // ?0 → ?1, ?1 → Int  ⇒  ?0 should resolve to Int
        let mut s = Substitution::new();
        s.bind(0, Type::Var(1));
        s.bind(1, Type::int());

        assert_eq!(Type::Var(0).apply(&s), Type::int());
    }

    #[test]
    fn substitution_compose() {
        let mut s1 = Substitution::new();
        s1.bind(0, Type::Var(1));

        let mut s2 = Substitution::new();
        s2.bind(1, Type::int());

        let composed = s1.compose(&s2);
        assert_eq!(Type::Var(0).apply(&composed), Type::int());
        assert_eq!(Type::Var(1).apply(&composed), Type::int());
    }

    #[test]
    fn fresh_vars() {
        let mut var_gen = TypeVarGen::new();
        let t0 = var_gen.fresh();
        let t1 = var_gen.fresh();
        assert_eq!(t0, Type::Var(0));
        assert_eq!(t1, Type::Var(1));
    }

    #[test]
    fn type_scheme_free_vars() {
        // ∀a. a -> Int  ⇒  no free vars
        let scheme = TypeScheme {
            vars: vec![0],
            ty: Type::Fn(vec![Type::Var(0)], Box::new(Type::int())),
        };
        assert!(scheme.free_vars().is_empty());
    }

    #[test]
    fn type_scheme_free_vars_partial() {
        // ∀a. a -> b  ⇒  free var: b (id 1)
        let scheme = TypeScheme {
            vars: vec![0],
            ty: Type::Fn(vec![Type::Var(0)], Box::new(Type::Var(1))),
        };
        let fv = scheme.free_vars();
        assert_eq!(fv.len(), 1);
        assert!(fv.contains(&1));
    }

    #[test]
    fn type_equality() {
        assert_eq!(Type::int(), Type::int());
        assert_ne!(Type::int(), Type::float());
        assert_eq!(Type::list(Type::int()), Type::list(Type::int()));
        assert_ne!(Type::list(Type::int()), Type::list(Type::float()));
        assert_eq!(Type::Var(0), Type::Var(0));
        assert_ne!(Type::Var(0), Type::Var(1));
    }

    #[test]
    fn tuple_type() {
        let t = Type::Tuple(vec![Type::int(), Type::string(), Type::bool()]);
        assert_eq!(t.to_string(), "(Int, String, Bool)");
        assert!(t.free_vars().is_empty());
    }

    #[test]
    fn option_type() {
        let t = Type::option(Type::unit_type());
        assert_eq!(t.to_string(), "Option(Unit)");
    }
}
