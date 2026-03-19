use std::collections::{HashMap, HashSet};

use crate::ast::{self, Constructor, Definition, Expr, Module, Spanned, TypeDef};

/// Collected type information for codegen.
#[derive(Debug)]
pub struct TypeInfo {
    pub name: String,
    /// All constructors (variants) of this type.
    pub variants: Vec<VariantInfo>,
    /// True if this type has multiple constructors (needs a tag).
    pub is_enum: bool,
}

#[derive(Debug)]
pub struct VariantInfo {
    pub name: String,
    pub tag: i64,
    pub fields: Vec<FieldInfo>,
}

#[derive(Debug, Clone)]
pub struct FieldInfo {
    pub name: String,
    pub jass_type: String,
}

/// Maps type names to their info.
#[derive(Debug)]
pub struct TypeRegistry {
    pub types: HashMap<String, TypeInfo>,
    /// Monomorphized list element JASS types (e.g. "integer", "real")
    pub list_types: HashSet<String>,
}

impl TypeRegistry {
    pub fn from_module(module: &Module) -> Self {
        let mut types = HashMap::new();

        for def in &module.definitions {
            if let Definition::Type(type_def) = def {
                let info = Self::collect_type(type_def);
                types.insert(info.name.clone(), info);
            }
        }

        // Discover and register tuple shapes used in the module
        let mut tuple_shapes: HashSet<Vec<String>> = HashSet::new();
        for def in &module.definitions {
            Self::discover_tuples(def, &mut tuple_shapes);
        }
        for shape in &tuple_shapes {
            let name = Self::tuple_type_name(shape);
            if !types.contains_key(&name) {
                let fields: Vec<FieldInfo> = shape
                    .iter()
                    .enumerate()
                    .map(|(i, jt)| FieldInfo {
                        name: format!("_{}", i),
                        jass_type: jt.clone(),
                    })
                    .collect();
                types.insert(
                    name.clone(),
                    TypeInfo {
                        name: name.clone(),
                        is_enum: false,
                        variants: vec![VariantInfo {
                            name,
                            tag: 0,
                            fields,
                        }],
                    },
                );
            }
        }

        // Discover list element types
        let mut list_elem_types: HashSet<String> = HashSet::new();
        for def in &module.definitions {
            Self::discover_lists(def, &mut list_elem_types);
        }

        TypeRegistry {
            types,
            list_types: list_elem_types,
        }
    }

    /// Generate a deterministic name for a tuple type based on its element types.
    pub fn tuple_type_name(jass_types: &[String]) -> String {
        format!("Tuple{}_{}", jass_types.len(), jass_types.join("_"))
    }

    /// Walk AST to find all tuple expressions and collect their shapes.
    fn discover_tuples(def: &Definition, shapes: &mut HashSet<Vec<String>>) {
        match def {
            Definition::Function(f) => Self::discover_tuples_expr(&f.body, shapes),
            Definition::Const(c) => Self::discover_tuples_expr(&c.value, shapes),
            _ => {}
        }
    }

    fn discover_tuples_expr(expr: &Spanned<Expr>, shapes: &mut HashSet<Vec<String>>) {
        match &expr.node {
            Expr::Tuple(elems) => {
                let shape: Vec<String> = elems
                    .iter()
                    .map(|e| Self::infer_jass_type_simple(&e.node))
                    .collect();
                shapes.insert(shape);
                for e in elems {
                    Self::discover_tuples_expr(e, shapes);
                }
            }
            Expr::Let { value, body, .. } => {
                Self::discover_tuples_expr(value, shapes);
                Self::discover_tuples_expr(body, shapes);
            }
            Expr::Case { subject, arms } => {
                Self::discover_tuples_expr(subject, shapes);
                for arm in arms {
                    Self::discover_tuples_expr(&arm.body, shapes);
                }
            }
            Expr::BinOp { left, right, .. } | Expr::Pipe { left, right } => {
                Self::discover_tuples_expr(left, shapes);
                Self::discover_tuples_expr(right, shapes);
            }
            Expr::UnaryOp { operand, .. } => Self::discover_tuples_expr(operand, shapes),
            Expr::Call { function, args } => {
                Self::discover_tuples_expr(function, shapes);
                for a in args {
                    Self::discover_tuples_expr(a, shapes);
                }
            }
            Expr::Block(exprs) => {
                for e in exprs {
                    Self::discover_tuples_expr(e, shapes);
                }
            }
            Expr::Lambda { body, .. } => Self::discover_tuples_expr(body, shapes),
            Expr::Clone(inner) => Self::discover_tuples_expr(inner, shapes),
            Expr::FieldAccess { object, .. } | Expr::MethodCall { object, .. } => {
                Self::discover_tuples_expr(object, shapes);
            }
            Expr::Constructor { args, .. } => {
                for a in args {
                    match a {
                        ast::ConstructorArg::Positional(e) | ast::ConstructorArg::Named(_, e) => {
                            Self::discover_tuples_expr(e, shapes);
                        }
                    }
                }
            }
            Expr::RecordUpdate { base, updates, .. } => {
                Self::discover_tuples_expr(base, shapes);
                for (_, e) in updates {
                    Self::discover_tuples_expr(e, shapes);
                }
            }
            Expr::List(elems) => {
                for e in elems {
                    Self::discover_tuples_expr(e, shapes);
                }
            }
            _ => {}
        }
    }

    /// Generate the JASS name prefix for a list of a given element type.
    pub fn list_type_name(elem_jass_type: &str) -> String {
        format!("List_{}", elem_jass_type)
    }

    fn discover_lists(def: &Definition, elem_types: &mut HashSet<String>) {
        match def {
            Definition::Function(f) => Self::discover_lists_expr(&f.body, elem_types),
            Definition::Const(c) => Self::discover_lists_expr(&c.value, elem_types),
            _ => {}
        }
    }

    fn discover_lists_expr(expr: &Spanned<Expr>, elem_types: &mut HashSet<String>) {
        match &expr.node {
            Expr::List(elems) => {
                // Infer element type from first element, default to integer
                let elem_type = elems
                    .first()
                    .map(|e| Self::infer_jass_type_simple(&e.node))
                    .unwrap_or_else(|| "integer".to_string());
                elem_types.insert(elem_type);
                for e in elems {
                    Self::discover_lists_expr(e, elem_types);
                }
            }
            Expr::Let { value, body, .. } => {
                Self::discover_lists_expr(value, elem_types);
                Self::discover_lists_expr(body, elem_types);
            }
            Expr::Case { subject, arms } => {
                Self::discover_lists_expr(subject, elem_types);
                for arm in arms {
                    Self::discover_lists_expr(&arm.body, elem_types);
                }
            }
            Expr::BinOp { left, right, .. } | Expr::Pipe { left, right } => {
                Self::discover_lists_expr(left, elem_types);
                Self::discover_lists_expr(right, elem_types);
            }
            Expr::UnaryOp { operand, .. } => Self::discover_lists_expr(operand, elem_types),
            Expr::Call { function, args } => {
                Self::discover_lists_expr(function, elem_types);
                for a in args {
                    Self::discover_lists_expr(a, elem_types);
                }
            }
            Expr::Block(exprs) => {
                for e in exprs {
                    Self::discover_lists_expr(e, elem_types);
                }
            }
            Expr::Lambda { body, .. } | Expr::Clone(body) => {
                Self::discover_lists_expr(body, elem_types);
            }
            Expr::Tuple(elems) => {
                for e in elems {
                    Self::discover_lists_expr(e, elem_types);
                }
            }
            Expr::Constructor { args, .. } => {
                for a in args {
                    match a {
                        ast::ConstructorArg::Positional(e) | ast::ConstructorArg::Named(_, e) => {
                            Self::discover_lists_expr(e, elem_types);
                        }
                    }
                }
            }
            Expr::RecordUpdate { base, updates, .. } => {
                Self::discover_lists_expr(base, elem_types);
                for (_, e) in updates {
                    Self::discover_lists_expr(e, elem_types);
                }
            }
            _ => {}
        }
    }

    fn infer_jass_type_simple(expr: &Expr) -> String {
        match expr {
            Expr::Float(_) => "real".to_string(),
            Expr::Bool(_) => "boolean".to_string(),
            Expr::String(_) => "string".to_string(),
            _ => "integer".to_string(),
        }
    }

    fn collect_type(def: &TypeDef) -> TypeInfo {
        let is_enum = !def.is_struct && def.constructors.len() > 1;
        let variants: Vec<VariantInfo> = def
            .constructors
            .iter()
            .enumerate()
            .map(|(tag, ctor)| Self::collect_variant(ctor, i64::try_from(tag).unwrap_or(0)))
            .collect();

        TypeInfo {
            name: def.name.clone(),
            variants,
            is_enum,
        }
    }

    fn collect_variant(ctor: &Constructor, tag: i64) -> VariantInfo {
        let fields: Vec<FieldInfo> = ctor
            .fields
            .iter()
            .map(|f| FieldInfo {
                name: f.name.clone(),
                jass_type: Self::type_expr_to_jass(&f.type_expr),
            })
            .collect();

        VariantInfo {
            name: ctor.name.clone(),
            tag,
            fields,
        }
    }

    fn type_expr_to_jass(ty: &crate::ast::TypeExpr) -> String {
        let crate::ast::TypeExpr::Named { name, .. } = ty else {
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
            // Int, user types → integer SoA IDs
            _ => "integer".to_string(),
        }
    }

    /// Get variant info by constructor name.
    pub fn get_variant(&self, constructor_name: &str) -> Option<(&TypeInfo, &VariantInfo)> {
        for info in self.types.values() {
            for variant in &info.variants {
                if variant.name == constructor_name {
                    return Some((info, variant));
                }
            }
        }
        None
    }
}
