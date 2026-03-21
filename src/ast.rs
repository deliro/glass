// AST types are data structures built incrementally — fields used progressively across milestones.
#![allow(dead_code)]

use crate::token::Span;

#[derive(Debug, Clone)]
pub struct Module {
    pub definitions: Vec<Definition>,
}

#[derive(Debug, Clone)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: Span) -> Self {
        Self { node, span }
    }
}

// === Top-level definitions ===

#[derive(Debug, Clone)]
pub enum Definition {
    Function(FnDef),
    Type(TypeDef),
    Const(ConstDef),
    Extend(ExtendDef),
    External(ExternalDef),
    Import(ImportDef),
}

#[derive(Debug, Clone)]
pub struct FnDef {
    pub is_pub: bool,
    pub is_local: bool,
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<TypeExpr>,
    pub body: Spanned<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub type_expr: TypeExpr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct DestructuredParam {
    pub pattern: Spanned<Pattern>,
    pub type_annotation: TypeExpr,
    pub param_name: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TypeDef {
    pub is_pub: bool,
    pub name: String,
    pub type_params: Vec<String>,
    pub constructors: Vec<Constructor>,
    /// True for `struct Name { ... }` — single variant, no tag needed.
    pub is_struct: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Constructor {
    pub name: String,
    pub fields: Vec<Field>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub type_expr: TypeExpr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ConstDef {
    pub is_pub: bool,
    pub name: String,
    pub type_expr: Option<TypeExpr>,
    pub value: Spanned<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ExtendDef {
    pub type_name: String,
    pub type_params: Vec<String>,
    pub methods: Vec<FnDef>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ExternalDef {
    pub module: String,
    pub name_in_module: String,
    pub is_pub: bool,
    pub fn_name: String,
    pub params: Vec<Param>,
    pub return_type: Option<TypeExpr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ImportDef {
    pub path: Vec<String>,
    pub items: Option<Vec<ImportItem>>,
    pub alias: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ImportItem {
    pub name: String,
    pub alias: Option<String>,
}

// === Expressions ===

#[derive(Debug, Clone)]
pub enum Expr {
    // Literals
    Int(i64),
    Float(f64),
    String(String),
    Rawcode(String),
    Bool(bool),

    // Variable reference
    Var(String),

    // Let binding: let pattern = value; body
    Let {
        pattern: Spanned<Pattern>,
        type_annotation: Option<TypeExpr>,
        value: Box<Spanned<Expr>>,
        body: Box<Spanned<Expr>>,
    },

    // Case expression
    Case {
        subject: Box<Spanned<Expr>>,
        arms: Vec<CaseArm>,
    },

    // Binary operation
    BinOp {
        op: BinOp,
        left: Box<Spanned<Expr>>,
        right: Box<Spanned<Expr>>,
    },

    // Unary operation
    UnaryOp {
        op: UnaryOp,
        operand: Box<Spanned<Expr>>,
    },

    // Function call: f(a, b)
    Call {
        function: Box<Spanned<Expr>>,
        args: Vec<Spanned<Expr>>,
    },

    // Field access: expr.field
    FieldAccess {
        object: Box<Spanned<Expr>>,
        field: String,
    },

    // Method call: expr.method(args)
    MethodCall {
        object: Box<Spanned<Expr>>,
        method: String,
        args: Vec<Spanned<Expr>>,
    },

    // Constructor: Name(args) or Name(field: val)
    Constructor {
        name: String,
        args: Vec<ConstructorArg>,
    },

    // Record update: Name(..expr, field: val)
    RecordUpdate {
        name: String,
        base: Box<Spanned<Expr>>,
        updates: Vec<(String, Spanned<Expr>)>,
    },

    // Tuple: (a, b, c)
    Tuple(Vec<Spanned<Expr>>),

    // List: [a, b, c]
    List(Vec<Spanned<Expr>>),

    // List cons: [head | tail]
    ListCons {
        head: Box<Spanned<Expr>>,
        tail: Box<Spanned<Expr>>,
    },

    // Lambda: fn(params) { body }
    Lambda {
        params: Vec<Param>,
        return_type: Option<TypeExpr>,
        body: Box<Spanned<Expr>>,
    },

    // Pipe: left |> right
    Pipe {
        left: Box<Spanned<Expr>>,
        right: Box<Spanned<Expr>>,
    },

    // Block: { expr1; expr2 }
    Block(Vec<Spanned<Expr>>),

    // Clone
    Clone(Box<Spanned<Expr>>),

    // Todo placeholder
    Todo(Option<String>),

    // TCO: loop wrapper for tail-recursive function body
    TcoLoop {
        body: Box<Spanned<Expr>>,
    },

    // TCO: tail call replaced by parameter reassignment
    // args: (param_name, new_value) pairs
    TcoContinue {
        args: Vec<(String, Box<Spanned<Expr>>)>,
    },
}

#[derive(Debug, Clone)]
pub enum ConstructorArg {
    Positional(Spanned<Expr>),
    Named(String, Spanned<Expr>),
}

#[derive(Debug, Clone)]
pub struct CaseArm {
    pub pattern: Spanned<Pattern>,
    pub guard: Option<Spanned<Expr>>,
    pub body: Spanned<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,          // +
    Sub,          // -
    Mul,          // *
    Div,          // /
    Mod,          // %
    Eq,           // ==
    NotEq,        // !=
    Less,         // <
    Greater,      // >
    LessEq,       // <=
    GreaterEq,    // >=
    And,          // &&
    Or,           // ||
    StringConcat, // <>
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Negate, // -
    Not,    // !
}

// === Patterns ===

#[derive(Debug, Clone)]
pub enum Pattern {
    Var(String),
    Discard,
    Int(i64),
    String(String),
    Bool(bool),
    Rawcode(String),
    /// Positional constructor: Quit(player)
    Constructor {
        name: String,
        args: Vec<Spanned<Pattern>>,
    },
    /// Named field constructor: Chat { from as p, .. }
    ConstructorNamed {
        name: String,
        fields: Vec<FieldPattern>,
        rest: bool, // true if `..` present (ignore remaining fields)
    },
    /// OR pattern: Pat1 | Pat2
    Or(Vec<Spanned<Pattern>>),
    Tuple(Vec<Spanned<Pattern>>),
    List(Vec<Spanned<Pattern>>),
    ListCons {
        head: Box<Spanned<Pattern>>,
        tail: Box<Spanned<Pattern>>,
    },
    /// Whole-pattern binding: pattern as name
    As {
        pattern: Box<Spanned<Pattern>>,
        name: String,
    },
}

/// Named field in a constructor pattern:
/// - `field` — binds field value to `field`
/// - `field as name` — binds field value to `name` (sugar for `field: name`)
/// - `field: pattern` — nested pattern destructuring
#[derive(Debug, Clone)]
pub struct FieldPattern {
    pub field_name: String,
    pub pattern: Option<Spanned<Pattern>>,
}

impl FieldPattern {
    pub fn binding_name(&self) -> &str {
        match &self.pattern {
            None => &self.field_name,
            Some(p) => match &p.node {
                Pattern::Var(name) => name,
                _ => &self.field_name,
            },
        }
    }

    pub fn has_nested_pattern(&self) -> bool {
        match &self.pattern {
            None => false,
            Some(p) => !matches!(p.node, Pattern::Var(_)),
        }
    }
}

// === Type expressions ===

#[derive(Debug, Clone)]
pub enum TypeExpr {
    Named {
        name: String,
        args: Vec<TypeExpr>,
    },
    Fn {
        params: Vec<TypeExpr>,
        ret: Box<TypeExpr>,
    },
    Tuple(Vec<TypeExpr>),
}
