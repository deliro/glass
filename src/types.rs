use std::collections::{HashMap, HashSet};

use crate::ast::{self, Constructor, Definition, Expr, Module, Spanned, TypeDef, TypeExpr};

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
        let mut generic_defs: HashMap<String, &TypeDef> = HashMap::new();

        for def in &module.definitions {
            if let Definition::Type(type_def) = def {
                if type_def.type_params.is_empty() {
                    let info = Self::collect_type(type_def);
                    types.insert(info.name.clone(), info);
                } else {
                    generic_defs.insert(type_def.name.clone(), type_def);
                }
            }
        }

        let mut instantiations: HashSet<(String, Vec<String>)> = HashSet::new();
        for def in &module.definitions {
            Self::discover_generic_instantiations(def, &generic_defs, &mut instantiations);
        }

        for (type_name, gdef) in &generic_defs {
            let concrete: Vec<&Vec<String>> = instantiations
                .iter()
                .filter(|(n, _)| n == type_name)
                .map(|(_, args)| args)
                .collect();

            if let [jass_args] = concrete.as_slice() {
                let subst: HashMap<&str, &str> = gdef
                    .type_params
                    .iter()
                    .zip(jass_args.iter())
                    .map(|(p, j)| (p.as_str(), j.as_str()))
                    .collect();
                let info = Self::collect_type_with_subst(gdef, type_name, &subst);
                types.insert(type_name.clone(), info);
            } else {
                let info = Self::collect_type(gdef);
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
            Self::discover_lists_from_annotations(def, &mut list_elem_types);
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

    fn discover_lists_from_annotations(def: &Definition, elem_types: &mut HashSet<String>) {
        match def {
            Definition::Type(td) => {
                for ctor in &td.constructors {
                    for field in &ctor.fields {
                        Self::discover_list_in_type_expr(&field.type_expr, elem_types);
                    }
                }
            }
            Definition::Function(f) => {
                for p in &f.params {
                    Self::discover_list_in_type_expr(&p.type_expr, elem_types);
                }
                if let Some(rt) = &f.return_type {
                    Self::discover_list_in_type_expr(rt, elem_types);
                }
            }
            _ => {}
        }
    }

    fn discover_list_in_type_expr(ty: &TypeExpr, elem_types: &mut HashSet<String>) {
        if let TypeExpr::Named { name, args } = ty {
            if name == "List" {
                if let Some(arg) = args.first() {
                    let jass_type = Self::type_expr_to_jass(arg);
                    if jass_type != "integer" {
                        elem_types.insert(jass_type);
                    }
                }
            }
            for arg in args {
                Self::discover_list_in_type_expr(arg, elem_types);
            }
        } else if let TypeExpr::Tuple(elems) = ty {
            for elem in elems {
                Self::discover_list_in_type_expr(elem, elem_types);
            }
        }
    }

    fn discover_generic_instantiations(
        def: &Definition,
        generic_defs: &HashMap<String, &TypeDef>,
        out: &mut HashSet<(String, Vec<String>)>,
    ) {
        match def {
            Definition::Type(td) => {
                for ctor in &td.constructors {
                    for field in &ctor.fields {
                        Self::discover_generic_in_type_expr(&field.type_expr, generic_defs, out);
                    }
                }
            }
            Definition::Function(f) => {
                for p in &f.params {
                    Self::discover_generic_in_type_expr(&p.type_expr, generic_defs, out);
                }
                if let Some(rt) = &f.return_type {
                    Self::discover_generic_in_type_expr(rt, generic_defs, out);
                }
                Self::discover_generic_in_expr(&f.body, generic_defs, out);
            }
            _ => {}
        }
    }

    fn discover_generic_in_type_expr(
        ty: &TypeExpr,
        generic_defs: &HashMap<String, &TypeDef>,
        out: &mut HashSet<(String, Vec<String>)>,
    ) {
        if let TypeExpr::Named { name, args } = ty {
            let bare = name.rsplit('.').next().unwrap_or(name);
            if generic_defs.contains_key(bare) && !args.is_empty() {
                let jass_args: Vec<String> = args.iter().map(Self::type_expr_to_jass).collect();
                if jass_args.iter().any(|j| j != "integer") {
                    out.insert((bare.to_string(), jass_args));
                }
            }
            for a in args {
                Self::discover_generic_in_type_expr(a, generic_defs, out);
            }
        }
    }

    fn discover_generic_in_expr(
        expr: &Spanned<Expr>,
        generic_defs: &HashMap<String, &TypeDef>,
        out: &mut HashSet<(String, Vec<String>)>,
    ) {
        match &expr.node {
            Expr::Let {
                value,
                body,
                type_annotation,
                ..
            } => {
                if let Some(ann) = type_annotation {
                    Self::discover_generic_in_type_expr(ann, generic_defs, out);
                }
                Self::discover_generic_in_expr(value, generic_defs, out);
                Self::discover_generic_in_expr(body, generic_defs, out);
            }
            Expr::Case { subject, arms } => {
                Self::discover_generic_in_expr(subject, generic_defs, out);
                for arm in arms {
                    Self::discover_generic_in_expr(&arm.body, generic_defs, out);
                    if let Some(g) = &arm.guard {
                        Self::discover_generic_in_expr(g, generic_defs, out);
                    }
                }
            }
            Expr::BinOp { left, right, .. } | Expr::Pipe { left, right } => {
                Self::discover_generic_in_expr(left, generic_defs, out);
                Self::discover_generic_in_expr(right, generic_defs, out);
            }
            Expr::UnaryOp { operand, .. } | Expr::Clone(operand) => {
                Self::discover_generic_in_expr(operand, generic_defs, out);
            }
            Expr::Call { function, args } => {
                Self::discover_generic_in_expr(function, generic_defs, out);
                for a in args {
                    Self::discover_generic_in_expr(a, generic_defs, out);
                }
            }
            Expr::Block(exprs) => {
                for e in exprs {
                    Self::discover_generic_in_expr(e, generic_defs, out);
                }
            }
            Expr::Lambda { body, params, .. } => {
                for p in params {
                    Self::discover_generic_in_type_expr(&p.type_expr, generic_defs, out);
                }
                Self::discover_generic_in_expr(body, generic_defs, out);
            }
            Expr::Tuple(elems) | Expr::List(elems) => {
                for e in elems {
                    Self::discover_generic_in_expr(e, generic_defs, out);
                }
            }
            Expr::ListCons { head, tail } => {
                Self::discover_generic_in_expr(head, generic_defs, out);
                Self::discover_generic_in_expr(tail, generic_defs, out);
            }
            Expr::FieldAccess { object, .. } => {
                Self::discover_generic_in_expr(object, generic_defs, out);
            }
            Expr::MethodCall { object, args, .. } => {
                Self::discover_generic_in_expr(object, generic_defs, out);
                for a in args {
                    Self::discover_generic_in_expr(a, generic_defs, out);
                }
            }
            Expr::Constructor { args, .. } => {
                for a in args {
                    match a {
                        ast::ConstructorArg::Positional(e) | ast::ConstructorArg::Named(_, e) => {
                            Self::discover_generic_in_expr(e, generic_defs, out);
                        }
                    }
                }
            }
            Expr::RecordUpdate { base, updates, .. } => {
                Self::discover_generic_in_expr(base, generic_defs, out);
                for (_, e) in updates {
                    Self::discover_generic_in_expr(e, generic_defs, out);
                }
            }
            Expr::TcoLoop { body } => {
                Self::discover_generic_in_expr(body, generic_defs, out);
            }
            Expr::TcoContinue { args } => {
                for (_, e) in args {
                    Self::discover_generic_in_expr(e, generic_defs, out);
                }
            }
            _ => {}
        }
    }

    fn collect_type_with_subst(
        def: &TypeDef,
        mono_name: &str,
        subst: &HashMap<&str, &str>,
    ) -> TypeInfo {
        let is_enum = !def.is_struct && def.constructors.len() > 1;
        let variants: Vec<VariantInfo> = def
            .constructors
            .iter()
            .enumerate()
            .map(|(tag, ctor)| {
                let fields: Vec<FieldInfo> = ctor
                    .fields
                    .iter()
                    .map(|f| {
                        let jass_type = Self::type_expr_to_jass_with_subst(&f.type_expr, subst);
                        FieldInfo {
                            name: f.name.clone(),
                            jass_type,
                        }
                    })
                    .collect();
                VariantInfo {
                    name: ctor.name.clone(),
                    tag: i64::try_from(tag).unwrap_or(0),
                    fields,
                }
            })
            .collect();

        TypeInfo {
            name: mono_name.to_string(),
            variants,
            is_enum,
        }
    }

    fn type_expr_to_jass_with_subst(ty: &TypeExpr, subst: &HashMap<&str, &str>) -> String {
        let TypeExpr::Named { name, .. } = ty else {
            return "integer".to_string();
        };
        if let Some(jass) = subst.get(name.as_str()) {
            return jass.to_string();
        }
        Self::type_expr_to_jass(ty)
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

    pub fn type_expr_to_jass_public(ty: &crate::ast::TypeExpr) -> String {
        Self::type_expr_to_jass(ty)
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

    pub fn get_variant_of_type(
        &self,
        constructor_name: &str,
        type_name: &str,
    ) -> Option<(&TypeInfo, &VariantInfo)> {
        if let Some(info) = self.types.get(type_name) {
            for variant in &info.variants {
                if variant.name == constructor_name {
                    return Some((info, variant));
                }
            }
        }
        self.get_variant(constructor_name)
    }
}
