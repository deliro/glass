use std::collections::HashSet;

use crate::ast::{self, Expr, Param, Spanned};
use crate::token::Span;

/// Information about a lambda found in the AST.
#[derive(Debug)]
pub struct LambdaInfo {
    pub id: usize,
    pub params: Vec<Param>,
    pub body: Spanned<Expr>,
    pub captures: Vec<CapturedVar>,
}

#[derive(Debug, Clone)]
pub struct CapturedVar {
    pub name: String,
    pub span: Span,
}

/// Collects all lambdas from the module and analyzes captures.
pub struct LambdaCollector {
    pub lambdas: Vec<LambdaInfo>,
    next_id: usize,
}

impl LambdaCollector {
    pub fn new() -> Self {
        Self {
            lambdas: Vec::new(),
            next_id: 0,
        }
    }

    pub fn collect_module(&mut self, module: &crate::ast::Module) {
        for def in &module.definitions {
            if let crate::ast::Definition::Function(f) = def {
                let mut scope: HashSet<String> = HashSet::new();
                for p in &f.params {
                    scope.insert(p.name.clone());
                }
                self.collect_expr(&f.body, &scope);
            }
        }
    }

    fn collect_expr(&mut self, expr: &Spanned<Expr>, scope: &HashSet<String>) {
        match &expr.node {
            Expr::Lambda { params, body, .. } => {
                // Find free variables: things used in body that are NOT lambda params
                // but ARE in the enclosing scope (i.e., captured from outside)
                let mut lambda_scope: HashSet<String> = HashSet::new();
                for p in params {
                    lambda_scope.insert(p.name.clone());
                }

                let mut free_vars: Vec<String> = Vec::new();
                crate::free_vars::find_free_vars(&body.node, &lambda_scope, &mut free_vars);
                // Only keep vars that are actually in the outer scope (not globals/builtins)
                free_vars.retain(|v| scope.contains(v));
                free_vars.sort();
                free_vars.dedup();

                // Build inner scope for recursive lambda scanning
                let mut inner_scope: HashSet<String> = scope.clone();
                for p in params {
                    inner_scope.insert(p.name.clone());
                }

                let captures: Vec<CapturedVar> = free_vars
                    .into_iter()
                    .filter_map(|name| {
                        crate::free_vars::find_var_span(&name, body)
                            .map(|span| CapturedVar { name, span })
                    })
                    .collect();

                let id = self.next_id;
                self.next_id += 1;

                self.lambdas.push(LambdaInfo {
                    id,
                    params: params.clone(),
                    body: body.as_ref().clone(),
                    captures,
                });

                // Also scan inside the lambda body for nested lambdas
                self.collect_expr(body, &inner_scope);
            }
            Expr::Let {
                value,
                body,
                pattern,
                ..
            } => {
                self.collect_expr(value, scope);
                let mut new_scope = scope.clone();
                crate::free_vars::bind_pattern(&pattern.node, &mut new_scope);
                self.collect_expr(body, &new_scope);
            }
            Expr::Case { subject, arms } => {
                self.collect_expr(subject, scope);
                for arm in arms {
                    let mut arm_scope = scope.clone();
                    crate::free_vars::bind_pattern(&arm.pattern.node, &mut arm_scope);
                    self.collect_expr(&arm.body, &arm_scope);
                }
            }
            Expr::BinOp { left, right, .. } | Expr::Pipe { left, right } => {
                self.collect_expr(left, scope);
                self.collect_expr(right, scope);
            }
            Expr::UnaryOp { operand, .. } | Expr::Clone(operand) => {
                self.collect_expr(operand, scope);
            }
            Expr::Call { function, args } => {
                self.collect_expr(function, scope);
                for a in args {
                    self.collect_expr(a, scope);
                }
            }
            Expr::FieldAccess { object, .. } => {
                self.collect_expr(object, scope);
            }
            Expr::MethodCall { object, args, .. } => {
                self.collect_expr(object, scope);
                for a in args {
                    self.collect_expr(a, scope);
                }
            }
            Expr::Block(exprs) => {
                let mut block_scope = scope.clone();
                for e in exprs {
                    self.collect_expr(e, &block_scope);
                    // Let bindings in blocks extend scope for subsequent exprs
                    if let Expr::Let { pattern, .. } = &e.node {
                        crate::free_vars::bind_pattern(&pattern.node, &mut block_scope);
                    }
                }
            }
            Expr::Tuple(elems) | Expr::List(elems) => {
                for e in elems {
                    self.collect_expr(e, scope);
                }
            }
            Expr::Constructor { args, .. } => {
                for a in args {
                    match a {
                        ast::ConstructorArg::Positional(e) | ast::ConstructorArg::Named(_, e) => {
                            self.collect_expr(e, scope);
                        }
                    }
                }
            }
            Expr::RecordUpdate { base, updates, .. } => {
                self.collect_expr(base, scope);
                for (_, e) in updates {
                    self.collect_expr(e, scope);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use crate::token::Lexer;

    fn collect(source: &str) -> Vec<LambdaInfo> {
        let tokens = Lexer::tokenize(source).expect("lex failed");
        let mut parser = Parser::new(tokens);
        let module = {
            let _o = parser.parse_module();
            assert!(_o.errors.is_empty(), "parse errors: {:?}", _o.errors);
            _o.module
        };
        let mut collector = LambdaCollector::new();
        collector.collect_module(&module);
        collector.lambdas
    }

    #[test]
    fn no_capture() {
        let lambdas = collect("fn test() -> Int { fn(x: Int) { x + 1 } }");
        assert_eq!(lambdas.len(), 1);
        assert!(lambdas[0].captures.is_empty());
        assert_eq!(lambdas[0].params.len(), 1);
    }

    #[test]
    fn captures_outer_var() {
        let lambdas = collect("fn test(y: Int) -> Int { fn(x: Int) { x + y } }");
        assert_eq!(lambdas.len(), 1);
        assert_eq!(lambdas[0].captures.len(), 1);
        assert_eq!(lambdas[0].captures[0].name, "y");
    }

    #[test]
    fn captures_let_binding() {
        let lambdas = collect("fn test() -> Int { let y: Int = 5 fn() { y } }");
        assert_eq!(lambdas.len(), 1);
        assert_eq!(lambdas[0].captures.len(), 1);
        assert_eq!(lambdas[0].captures[0].name, "y");
    }

    #[test]
    fn no_capture_when_in_scope() {
        let lambdas = collect("fn test() -> Int { fn(y: Int) { y } }");
        assert_eq!(lambdas.len(), 1);
        assert!(lambdas[0].captures.is_empty());
    }

    #[test]
    fn multiple_lambdas() {
        let lambdas = collect(
            r#"
fn test(a: Int, b: Int) -> Int {
    let f = fn(x: Int) { x + a }
    let g = fn(x: Int) { x + b }
    f
}
"#,
        );
        assert_eq!(lambdas.len(), 2);
        assert_eq!(lambdas[0].captures[0].name, "a");
        assert_eq!(lambdas[1].captures[0].name, "b");
    }
}
