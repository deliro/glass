use crate::ast::*;
use crate::token::{Span, Token};

pub struct Parser {
    tokens: Vec<(Token, Span)>,
    pos: usize,
}

type ParseResult<T> = Result<T, ParseError>;

pub struct ParseOutput {
    pub module: Module,
    pub errors: Vec<ParseError>,
}

#[derive(Debug)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl ParseError {
    fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }
}

impl Parser {
    pub fn new(tokens: Vec<(Token, Span)>) -> Self {
        Self { tokens, pos: 0 }
    }

    // === Utilities ===

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|(t, _)| t)
    }

    fn peek_span(&self) -> Span {
        self.tokens
            .get(self.pos)
            .map(|(_, s)| *s)
            .unwrap_or(Span::new(0, 0))
    }

    fn advance(&mut self) -> (Token, Span) {
        let tok = self
            .tokens
            .get(self.pos)
            .cloned()
            .unwrap_or_else(|| (Token::Fn, Span::new(0, 0))); // fallback; errors caught by expect()
        self.pos += 1;
        tok
    }

    fn expect(&mut self, expected: &Token) -> ParseResult<Span> {
        match self.peek() {
            Some(t) if t == expected => {
                let (_, span) = self.advance();
                Ok(span)
            }
            Some(t) => Err(ParseError::new(
                format!("expected {:?}, got {:?}", expected, t),
                self.peek_span(),
            )),
            None => Err(ParseError::new(
                format!("expected {:?}, got EOF", expected),
                self.peek_span(),
            )),
        }
    }

    fn at(&self, token: &Token) -> bool {
        self.peek() == Some(token)
    }

    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn expect_lower_ident(&mut self) -> ParseResult<(String, Span)> {
        match self.peek().cloned() {
            Some(Token::LowerIdent(name)) => {
                let (_, span) = self.advance();
                Ok((name, span))
            }
            other => Err(ParseError::new(
                format!("expected identifier, got {:?}", other),
                self.peek_span(),
            )),
        }
    }

    fn expect_upper_ident(&mut self) -> ParseResult<(String, Span)> {
        match self.peek().cloned() {
            Some(Token::UpperIdent(name)) => {
                let (_, span) = self.advance();
                Ok((name, span))
            }
            other => Err(ParseError::new(
                format!("expected type name, got {:?}", other),
                self.peek_span(),
            )),
        }
    }

    // === Module ===

    pub fn parse_module(&mut self) -> ParseOutput {
        let mut definitions = Vec::new();
        let mut errors = Vec::new();
        while !self.at_end() {
            match self.parse_definition() {
                Ok(defs) => definitions.extend(defs),
                Err(e) => {
                    errors.push(e);
                    self.synchronize();
                }
            }
        }
        ParseOutput {
            module: Module { definitions },
            errors,
        }
    }

    fn synchronize(&mut self) {
        loop {
            match self.peek() {
                None
                | Some(
                    Token::Fn
                    | Token::Pub
                    | Token::Enum
                    | Token::Struct
                    | Token::Import
                    | Token::Const
                    | Token::Extend
                    | Token::At,
                ) => return,
                _ => {
                    self.advance();
                }
            }
        }
    }

    // === Definitions ===

    fn parse_definition(&mut self) -> ParseResult<Vec<Definition>> {
        match self.peek() {
            Some(Token::At) => self.parse_external_def().map(|d| vec![d]),
            Some(Token::Import) => self.parse_import_defs(),
            Some(Token::Extend) => self.parse_extend_def().map(|d| vec![d]),
            _ => {
                let is_pub = self.eat_pub();

                match self.peek() {
                    Some(Token::Enum) => self.parse_type_def(is_pub, false).map(|d| vec![d]),
                    Some(Token::Struct) => self.parse_type_def(is_pub, true).map(|d| vec![d]),
                    Some(Token::Const) => self.parse_const_def(is_pub).map(|d| vec![d]),
                    Some(Token::Fn) | Some(Token::Local) => {
                        self.parse_fn_def(is_pub).map(|d| vec![d])
                    }
                    other => Err(ParseError::new(
                        format!("expected definition, got {:?}", other),
                        self.peek_span(),
                    )),
                }
            }
        }
    }

    fn eat_pub(&mut self) -> bool {
        if self.at(&Token::Pub) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn parse_fn_def(&mut self, is_pub: bool) -> ParseResult<Definition> {
        let start = self.peek_span();
        let is_local = if self.at(&Token::Local) {
            self.advance();
            true
        } else {
            false
        };

        self.expect(&Token::Fn)?;
        let (name, _) = self.expect_lower_ident()?;
        self.expect(&Token::LParen)?;
        let (params, pattern_bindings) = self.parse_params_with_patterns()?;
        self.expect(&Token::RParen)?;

        let return_type = if self.at(&Token::Arrow) {
            self.advance();
            Some(self.parse_type_expr()?)
        } else {
            None
        };

        let mut body = self.parse_block()?;

        for dp in pattern_bindings.into_iter().rev() {
            body = Spanned::new(
                Expr::Let {
                    pattern: dp.pattern,
                    type_annotation: Some(dp.type_annotation),
                    value: Box::new(Spanned::new(Expr::Var(dp.param_name), dp.span)),
                    body: Box::new(body),
                },
                dp.span,
            );
        }

        let span = start.merge(body.span);

        Ok(Definition::Function(FnDef {
            is_pub,
            is_local,
            name,
            params,
            return_type,
            body,
            span,
        }))
    }

    fn parse_params_with_patterns(&mut self) -> ParseResult<(Vec<Param>, Vec<DestructuredParam>)> {
        let mut params = Vec::new();
        let mut pattern_bindings = Vec::new();
        let mut destr_counter = 0;

        if self.at(&Token::RParen) {
            return Ok((params, pattern_bindings));
        }

        let (param, binding) = self.parse_param_or_pattern(&mut destr_counter)?;
        params.push(param);
        if let Some(b) = binding {
            pattern_bindings.push(b);
        }

        while self.at(&Token::Comma) {
            self.advance();
            if self.at(&Token::RParen) {
                break;
            }
            let (param, binding) = self.parse_param_or_pattern(&mut destr_counter)?;
            params.push(param);
            if let Some(b) = binding {
                pattern_bindings.push(b);
            }
        }
        Ok((params, pattern_bindings))
    }

    fn parse_param_or_pattern(
        &mut self,
        destr_counter: &mut usize,
    ) -> ParseResult<(Param, Option<DestructuredParam>)> {
        if matches!(
            self.peek(),
            Some(Token::UpperIdent(_)) | Some(Token::LParen)
        ) {
            let start = self.peek_span();
            let pattern = self.parse_pattern()?;
            self.expect(&Token::Colon)?;
            let type_expr = self.parse_type_expr()?;
            let span = start.merge(self.prev_span());
            let param_name = match &pattern.node {
                Pattern::As { name, .. } => name.clone(),
                _ => {
                    let name = format!("glass_dp{}", destr_counter);
                    *destr_counter += 1;
                    name
                }
            };
            let param = Param {
                name: param_name.clone(),
                type_expr: type_expr.clone(),
                span,
            };
            Ok((
                param,
                Some(DestructuredParam {
                    pattern,
                    type_annotation: type_expr,
                    param_name,
                    span,
                }),
            ))
        } else {
            let param = self.parse_param()?;
            Ok((param, None))
        }
    }

    fn parse_params(&mut self) -> ParseResult<Vec<Param>> {
        let mut params = Vec::new();
        if self.at(&Token::RParen) {
            return Ok(params);
        }
        params.push(self.parse_param()?);
        while self.at(&Token::Comma) {
            self.advance();
            if self.at(&Token::RParen) {
                break;
            }
            params.push(self.parse_param()?);
        }
        Ok(params)
    }

    fn parse_param(&mut self) -> ParseResult<Param> {
        let (name, start) = self.expect_lower_ident()?;
        self.expect(&Token::Colon)?;
        let type_expr = self.parse_type_expr()?;
        let span = start.merge(self.prev_span());
        Ok(Param {
            name,
            type_expr,
            span,
        })
    }

    fn parse_type_def(&mut self, is_pub: bool, is_struct: bool) -> ParseResult<Definition> {
        let start = self.peek_span();
        if self.at(&Token::Enum) || self.at(&Token::Struct) {
            self.advance();
        } else {
            return Err(ParseError::new(
                "expected type, enum, or struct",
                self.peek_span(),
            ));
        }
        let (name, _) = self.expect_upper_ident()?;

        let type_params = if self.at(&Token::LParen) {
            self.advance();
            let mut params = Vec::new();
            if !self.at(&Token::RParen) {
                let (p, _) = self.expect_upper_ident()?;
                params.push(p);
                while self.at(&Token::Comma) {
                    self.advance();
                    let (p, _) = self.expect_upper_ident()?;
                    params.push(p);
                }
            }
            self.expect(&Token::RParen)?;
            params
        } else {
            Vec::new()
        };

        if is_struct {
            // `struct Name { field: Type, ... }`
            // Desugars to a single constructor with the same name as the type.
            self.expect(&Token::LBrace)?;
            let mut fields = Vec::new();
            if !self.at(&Token::RBrace) {
                fields.push(self.parse_named_field()?);
                while self.at(&Token::Comma) {
                    self.advance();
                    if self.at(&Token::RBrace) {
                        break;
                    }
                    fields.push(self.parse_named_field()?);
                }
            }
            let end = self.expect(&Token::RBrace)?;
            let span = start.merge(end);
            let ctor = Constructor {
                name: name.clone(),
                fields,
                span,
            };
            Ok(Definition::Type(TypeDef {
                is_pub,
                name,
                type_params,
                constructors: vec![ctor],
                is_struct: true,
                span,
            }))
        } else {
            // `enum Name { Variant1, Variant2 { ... } }` or `type Name { ... }`
            self.expect(&Token::LBrace)?;
            let mut constructors = Vec::new();
            while !self.at(&Token::RBrace) && !self.at_end() {
                constructors.push(self.parse_constructor()?);
            }
            let end = self.expect(&Token::RBrace)?;
            let span = start.merge(end);
            // Auto-detect: single constructor with same name = struct-like
            let auto_struct =
                constructors.len() == 1 && constructors.first().is_some_and(|c| c.name == name);
            Ok(Definition::Type(TypeDef {
                is_pub,
                name,
                type_params,
                constructors,
                is_struct: auto_struct,
                span,
            }))
        }
    }

    fn parse_constructor(&mut self) -> ParseResult<Constructor> {
        let (name, start) = self.expect_upper_ident()?;
        let fields = if self.at(&Token::LParen) {
            // Tuple-like: Z(String, Int) — unnamed positional fields
            self.advance();
            let mut fields = Vec::new();
            let mut unnamed_idx: usize = 0;
            if !self.at(&Token::RParen) {
                fields.push(self.parse_unnamed_field(&mut unnamed_idx)?);
                while self.at(&Token::Comma) {
                    self.advance();
                    if self.at(&Token::RParen) {
                        break;
                    }
                    fields.push(self.parse_unnamed_field(&mut unnamed_idx)?);
                }
            }
            self.expect(&Token::RParen)?;
            fields
        } else if self.at(&Token::LBrace) {
            // Struct-like: Y { val: Int, name: String } — named fields
            self.advance();
            let mut fields = Vec::new();
            if !self.at(&Token::RBrace) {
                fields.push(self.parse_named_field()?);
                while self.at(&Token::Comma) {
                    self.advance();
                    if self.at(&Token::RBrace) {
                        break;
                    }
                    fields.push(self.parse_named_field()?);
                }
            }
            self.expect(&Token::RBrace)?;
            fields
        } else {
            Vec::new()
        };
        let span = start.merge(self.prev_span());
        Ok(Constructor { name, fields, span })
    }

    /// Named field in struct-like constructor: `name: Type`
    fn parse_named_field(&mut self) -> ParseResult<Field> {
        let (name, start) = self.expect_lower_ident()?;
        self.expect(&Token::Colon)?;
        let type_expr = self.parse_type_expr()?;
        let span = start.merge(self.prev_span());
        Ok(Field {
            name,
            type_expr,
            span,
        })
    }

    /// Unnamed field in tuple-like constructor: just `Type`
    fn parse_unnamed_field(&mut self, unnamed_idx: &mut usize) -> ParseResult<Field> {
        let start = self.peek_span();
        let type_expr = self.parse_type_expr()?;
        let name = format!("_{}", unnamed_idx);
        *unnamed_idx += 1;
        let span = start.merge(self.prev_span());
        Ok(Field {
            name,
            type_expr,
            span,
        })
    }

    fn parse_const_def(&mut self, is_pub: bool) -> ParseResult<Definition> {
        let start = self.peek_span();
        self.expect(&Token::Const)?;
        let (name, _) = match self.peek() {
            Some(Token::LowerIdent(_)) => self.expect_lower_ident()?,
            _ => self.expect_upper_ident()?,
        };

        let type_expr = if self.at(&Token::Colon) {
            self.advance();
            Some(self.parse_type_expr()?)
        } else {
            None
        };

        self.expect(&Token::Eq)?;
        let value = self.parse_expr()?;
        let span = start.merge(value.span);

        Ok(Definition::Const(ConstDef {
            is_pub,
            name,
            type_expr,
            value,
            span,
        }))
    }

    fn parse_extend_def(&mut self) -> ParseResult<Definition> {
        let start = self.peek_span();
        self.expect(&Token::Extend)?;
        let (type_name, _) = self.expect_upper_ident()?;

        let type_params = if self.at(&Token::LParen) {
            self.advance();
            let mut params = Vec::new();
            let (p, _) = self.expect_upper_ident()?;
            params.push(p);
            while self.at(&Token::Comma) {
                self.advance();
                let (p, _) = self.expect_upper_ident()?;
                params.push(p);
            }
            self.expect(&Token::RParen)?;
            params
        } else {
            Vec::new()
        };

        self.expect(&Token::LBrace)?;
        let mut methods = Vec::new();
        while !self.at(&Token::RBrace) && !self.at_end() {
            let is_pub = self.eat_pub();
            match self.parse_fn_def(is_pub)? {
                Definition::Function(f) => methods.push(f),
                _ => unreachable!(),
            }
        }
        let end = self.expect(&Token::RBrace)?;

        Ok(Definition::Extend(ExtendDef {
            type_name,
            type_params,
            methods,
            span: start.merge(end),
        }))
    }

    fn parse_external_def(&mut self) -> ParseResult<Definition> {
        let start = self.peek_span();
        self.expect(&Token::At)?;

        // expect "external" as a lower ident
        let (kw, _) = self.expect_lower_ident()?;
        if kw != "external" {
            return Err(ParseError::new(
                format!("expected 'external', got '{}'", kw),
                self.peek_span(),
            ));
        }

        self.expect(&Token::LParen)?;
        let module = match self.advance() {
            (Token::StringLiteral(s), _) => s,
            (t, span) => {
                return Err(ParseError::new(
                    format!("expected string, got {:?}", t),
                    span,
                ));
            }
        };
        self.expect(&Token::Comma)?;
        let name_in_module = match self.advance() {
            (Token::StringLiteral(s), _) => s,
            (t, span) => {
                return Err(ParseError::new(
                    format!("expected string, got {:?}", t),
                    span,
                ));
            }
        };
        self.expect(&Token::RParen)?;

        let is_pub = self.eat_pub();
        self.expect(&Token::Fn)?;
        let (fn_name, _) = self.expect_lower_ident()?;
        self.expect(&Token::LParen)?;
        let params = self.parse_params()?;
        self.expect(&Token::RParen)?;

        let return_type = if self.at(&Token::Arrow) {
            self.advance();
            Some(self.parse_type_expr()?)
        } else {
            None
        };

        let span = start.merge(self.prev_span());

        Ok(Definition::External(ExternalDef {
            module,
            name_in_module,
            is_pub,
            fn_name,
            params,
            return_type,
            span,
            source_module: None,
        }))
    }

    /// Parse import definitions. Supports:
    ///   import foo                                       → 1 ImportDef
    ///   import foo/bar                                   → 1 ImportDef
    ///   import foo/bar { Baz, quux }                     → 1 ImportDef (selective)
    ///   import jass { math { cos, sin }, sfx, unit }     → 3 ImportDefs (grouped)
    fn parse_import_defs(&mut self) -> ParseResult<Vec<Definition>> {
        let start = self.peek_span();
        self.expect(&Token::Import)?;

        let mut path = Vec::new();
        let (first, _) = self.expect_lower_ident()?;
        path.push(first);
        while self.at(&Token::Slash) {
            self.advance();
            if self.at(&Token::LBrace) {
                break;
            }
            let (seg, _) = self.expect_lower_ident()?;
            path.push(seg);
        }

        if self.at(&Token::LBrace) {
            // Peek ahead: is this a grouped import (sub-modules) or selective import (items)?
            // Grouped: `{ math { ... }, unit }` — lower ident followed by { or ,
            // Selective: `{ Option, Some, None }` — upper ident, or lower ident not followed by {
            if self.is_grouped_import_brace() {
                return self.parse_grouped_import(&path, start);
            }

            // Selective import: `import foo/bar { Item1, Item2 }`
            self.advance(); // consume {
            let mut items = Vec::new();
            if !self.at(&Token::RBrace) {
                items.push(self.parse_import_item()?);
                while self.at(&Token::Comma) {
                    self.advance();
                    if self.at(&Token::RBrace) {
                        break;
                    }
                    items.push(self.parse_import_item()?);
                }
            }
            self.expect(&Token::RBrace)?;
            let span = start.merge(self.prev_span());
            return Ok(vec![Definition::Import(ImportDef {
                path,
                items: Some(items),
                alias: None,
                span,
            })]);
        }

        // Simple import: `import foo` or `import foo/bar`
        let alias = if self.at(&Token::As) {
            self.advance();
            let (name, _) = self.expect_lower_ident()?;
            Some(name)
        } else {
            None
        };

        let span = start.merge(self.prev_span());
        Ok(vec![Definition::Import(ImportDef {
            path,
            items: None,
            alias,
            span,
        })])
    }

    /// Check if the upcoming `{` starts a grouped import (sub-modules)
    /// vs a selective import (items).
    /// Grouped: next tokens after `{` are `lower_ident {` or `lower_ident ,` or `lower_ident }`
    /// Selective: next tokens after `{` are `UpperIdent` or `self`
    fn is_grouped_import_brace(&self) -> bool {
        // Look at token after `{`
        let after_brace = self.tokens.get(self.pos + 1).map(|(t, _)| t.clone());
        let after_after = self.tokens.get(self.pos + 2).map(|(t, _)| t.clone());
        match after_brace {
            Some(Token::LowerIdent(_)) => {
                // lower ident followed by { → grouped sub-module with items
                // lower ident followed by , or } → grouped sub-module (plain)
                matches!(
                    after_after,
                    Some(Token::LBrace) | Some(Token::Comma) | Some(Token::RBrace)
                )
            }
            _ => false,
        }
    }

    /// Parse grouped import body: `{ math { cos, sin }, sfx, unit }`
    /// Each entry becomes a separate ImportDef with `base_path/entry` as path.
    fn parse_grouped_import(
        &mut self,
        base_path: &[String],
        start: Span,
    ) -> ParseResult<Vec<Definition>> {
        self.advance(); // consume {
        let mut results = Vec::new();

        loop {
            if self.at(&Token::RBrace) {
                break;
            }
            let (sub_name, _) = self.expect_lower_ident()?;
            let mut sub_path = base_path.to_vec();
            sub_path.push(sub_name);

            let items = if self.at(&Token::LBrace) {
                // Sub-module with selective items: `math { cos, sin, self }`
                self.advance();
                let mut items = Vec::new();
                if !self.at(&Token::RBrace) {
                    items.push(self.parse_import_item()?);
                    while self.at(&Token::Comma) {
                        self.advance();
                        if self.at(&Token::RBrace) {
                            break;
                        }
                        items.push(self.parse_import_item()?);
                    }
                }
                self.expect(&Token::RBrace)?;
                Some(items)
            } else {
                None
            };

            let span = start.merge(self.prev_span());
            results.push(Definition::Import(ImportDef {
                path: sub_path,
                items,
                alias: None,
                span,
            }));

            if !self.at(&Token::Comma) {
                break;
            }
            self.advance(); // consume ,
        }
        self.expect(&Token::RBrace)?;
        Ok(results)
    }

    fn parse_import_item(&mut self) -> ParseResult<ImportItem> {
        let name = match self.peek().cloned() {
            Some(Token::UpperIdent(n) | Token::LowerIdent(n)) => {
                self.advance();
                n
            }
            other => {
                return Err(ParseError::new(
                    format!("expected import item, got {:?}", other),
                    self.peek_span(),
                ));
            }
        };
        let alias = if self.at(&Token::As) {
            self.advance();
            let a = match self.peek().cloned() {
                Some(Token::UpperIdent(n)) | Some(Token::LowerIdent(n)) => {
                    self.advance();
                    n
                }
                _ => {
                    return Err(ParseError::new("expected alias name", self.peek_span()));
                }
            };
            Some(a)
        } else {
            None
        };
        Ok(ImportItem { name, alias })
    }

    // === Expressions ===

    fn parse_block(&mut self) -> ParseResult<Spanned<Expr>> {
        let start = self.expect(&Token::LBrace)?;
        let mut exprs = Vec::new();
        while !self.at(&Token::RBrace) && !self.at_end() {
            exprs.push(self.parse_expr()?);
        }
        let end = self.expect(&Token::RBrace)?;
        let span = start.merge(end);

        if exprs.len() == 1 {
            let mut e = exprs.remove(0);
            e.span = span;
            Ok(e)
        } else {
            Ok(Spanned::new(Expr::Block(exprs), span))
        }
    }

    pub fn parse_expr(&mut self) -> ParseResult<Spanned<Expr>> {
        match self.peek() {
            Some(Token::Let) => self.parse_let_expr(),
            Some(Token::Case) => self.parse_case_expr(),
            _ => self.parse_pipe_expr(),
        }
    }

    fn parse_let_expr(&mut self) -> ParseResult<Spanned<Expr>> {
        let start = self.peek_span();
        self.expect(&Token::Let)?;
        let pattern = self.parse_pattern()?;

        let type_annotation = if self.at(&Token::Colon) {
            self.advance();
            Some(self.parse_type_expr()?)
        } else {
            None
        };

        self.expect(&Token::Eq)?;
        let value = self.parse_expr()?;
        let body = self.parse_expr()?;
        let span = start.merge(body.span);

        Ok(Spanned::new(
            Expr::Let {
                pattern,
                type_annotation,
                value: Box::new(value),
                body: Box::new(body),
            },
            span,
        ))
    }

    fn parse_case_expr(&mut self) -> ParseResult<Spanned<Expr>> {
        let start = self.peek_span();
        self.expect(&Token::Case)?;
        let subject = self.parse_expr()?;
        self.expect(&Token::LBrace)?;

        let mut arms = Vec::new();
        while !self.at(&Token::RBrace) && !self.at_end() {
            arms.push(self.parse_case_arm()?);
        }
        let end = self.expect(&Token::RBrace)?;

        Ok(Spanned::new(
            Expr::Case {
                subject: Box::new(subject),
                arms,
            },
            start.merge(end),
        ))
    }

    fn parse_case_arm(&mut self) -> ParseResult<CaseArm> {
        let start = self.peek_span();

        let or_pattern = self.parse_or_pattern()?;
        let pattern = if self.at(&Token::As) {
            self.advance();
            let (name, name_span) = self.expect_lower_ident()?;
            let span = or_pattern.span.merge(name_span);
            Spanned::new(
                Pattern::As {
                    pattern: Box::new(or_pattern),
                    name,
                },
                span,
            )
        } else {
            or_pattern
        };

        // Guard clause: `if expr`
        let guard = if matches!(self.peek(), Some(Token::LowerIdent(s)) if s == "if") {
            self.advance();
            Some(self.parse_pipe_expr()?)
        } else {
            None
        };

        self.expect(&Token::Arrow)?;

        let body = if self.at(&Token::LBrace) {
            self.parse_block()?
        } else {
            self.parse_expr()?
        };

        let span = start.merge(body.span);
        Ok(CaseArm {
            pattern,
            guard,
            body,
            span,
        })
    }

    // === Binary operators by precedence ===

    fn parse_pipe_expr(&mut self) -> ParseResult<Spanned<Expr>> {
        let mut left = self.parse_or_expr()?;
        while self.at(&Token::Pipe) {
            self.advance();
            let right = self.parse_unary_expr()?;
            let span = left.span.merge(right.span);
            left = Spanned::new(
                Expr::Pipe {
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
        }
        Ok(left)
    }

    fn parse_or_expr(&mut self) -> ParseResult<Spanned<Expr>> {
        let mut left = self.parse_and_expr()?;
        while self.at(&Token::OrOr) {
            self.advance();
            let right = self.parse_and_expr()?;
            let span = left.span.merge(right.span);
            left = Spanned::new(
                Expr::BinOp {
                    op: BinOp::Or,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
        }
        Ok(left)
    }

    fn parse_and_expr(&mut self) -> ParseResult<Spanned<Expr>> {
        let mut left = self.parse_cmp_expr()?;
        while self.at(&Token::AndAnd) {
            self.advance();
            let right = self.parse_cmp_expr()?;
            let span = left.span.merge(right.span);
            left = Spanned::new(
                Expr::BinOp {
                    op: BinOp::And,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
        }
        Ok(left)
    }

    fn parse_cmp_expr(&mut self) -> ParseResult<Spanned<Expr>> {
        let left = self.parse_add_expr()?;
        let op = match self.peek() {
            Some(Token::EqEq) => BinOp::Eq,
            Some(Token::NotEq) => BinOp::NotEq,
            Some(Token::Less) => BinOp::Less,
            Some(Token::Greater) => BinOp::Greater,
            Some(Token::LessEq) => BinOp::LessEq,
            Some(Token::GreaterEq) => BinOp::GreaterEq,
            _ => return Ok(left),
        };
        self.advance();
        let right = self.parse_add_expr()?;
        let span = left.span.merge(right.span);
        Ok(Spanned::new(
            Expr::BinOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            },
            span,
        ))
    }

    fn parse_add_expr(&mut self) -> ParseResult<Spanned<Expr>> {
        let mut left = self.parse_mul_expr()?;
        loop {
            let op = match self.peek() {
                Some(Token::Plus) => BinOp::Add,
                Some(Token::Minus) => BinOp::Sub,
                Some(Token::StringConcat) => BinOp::StringConcat,
                _ => break,
            };
            self.advance();
            let right = self.parse_mul_expr()?;
            let span = left.span.merge(right.span);
            left = Spanned::new(
                Expr::BinOp {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
        }
        Ok(left)
    }

    fn parse_mul_expr(&mut self) -> ParseResult<Spanned<Expr>> {
        let mut left = self.parse_unary_expr()?;
        loop {
            let op = match self.peek() {
                Some(Token::Star) => BinOp::Mul,
                Some(Token::Slash) => BinOp::Div,
                Some(Token::Percent) => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary_expr()?;
            let span = left.span.merge(right.span);
            left = Spanned::new(
                Expr::BinOp {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
        }
        Ok(left)
    }

    fn parse_unary_expr(&mut self) -> ParseResult<Spanned<Expr>> {
        match self.peek() {
            Some(Token::Minus) => {
                let start = self.peek_span();
                self.advance();
                let operand = self.parse_call_expr()?;
                let span = start.merge(operand.span);
                Ok(Spanned::new(
                    Expr::UnaryOp {
                        op: UnaryOp::Negate,
                        operand: Box::new(operand),
                    },
                    span,
                ))
            }
            Some(Token::Bang) => {
                let start = self.peek_span();
                self.advance();
                let operand = self.parse_call_expr()?;
                let span = start.merge(operand.span);
                Ok(Spanned::new(
                    Expr::UnaryOp {
                        op: UnaryOp::Not,
                        operand: Box::new(operand),
                    },
                    span,
                ))
            }
            _ => self.parse_call_expr(),
        }
    }

    fn is_callable(expr: &Expr) -> bool {
        matches!(
            expr,
            Expr::Var(_) | Expr::FieldAccess { .. } | Expr::Lambda { .. }
        )
    }

    fn parse_call_expr(&mut self) -> ParseResult<Spanned<Expr>> {
        let mut expr = self.parse_primary()?;

        loop {
            if self.at(&Token::LParen) && Self::is_callable(&expr.node) {
                // Function call
                self.advance();
                let args = self.parse_args()?;
                let end = self.expect(&Token::RParen)?;
                let span = expr.span.merge(end);
                expr = Spanned::new(
                    Expr::Call {
                        function: Box::new(expr),
                        args,
                    },
                    span,
                );
            } else if self.at(&Token::Dot) {
                self.advance();
                let is_upper = matches!(self.peek(), Some(Token::UpperIdent(_)));
                let (field, field_span) = match self.peek() {
                    Some(Token::UpperIdent(_)) => self.expect_upper_ident()?,
                    _ => self.expect_lower_ident()?,
                };
                if is_upper && (self.at(&Token::LBrace) || self.at(&Token::ColonColon)) {
                    let qualified = match &expr.node {
                        Expr::Var(module) => format!("{}.{}", module, field),
                        _ => field.clone(),
                    };
                    let name = if self.at(&Token::ColonColon) {
                        self.advance();
                        let (variant, _) = self.expect_upper_ident()?;
                        format!("{}::{}", qualified, variant)
                    } else {
                        qualified
                    };
                    if self.at(&Token::LBrace) {
                        expr = self.parse_brace_constructor_or_update(name, expr.span)?;
                    } else if self.at(&Token::LParen) {
                        expr = self.parse_constructor_or_update(name, expr.span)?;
                    } else {
                        expr = Spanned::new(
                            Expr::Constructor {
                                name,
                                args: Vec::new(),
                            },
                            expr.span.merge(field_span),
                        );
                    }
                } else if self.at(&Token::LParen) {
                    self.advance();
                    let args = self.parse_args()?;
                    let end = self.expect(&Token::RParen)?;
                    let span = expr.span.merge(end);
                    expr = Spanned::new(
                        Expr::MethodCall {
                            object: Box::new(expr),
                            method: field,
                            args,
                        },
                        span,
                    );
                } else {
                    let span = expr.span.merge(field_span);
                    expr = Spanned::new(
                        Expr::FieldAccess {
                            object: Box::new(expr),
                            field,
                        },
                        span,
                    );
                }
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_args(&mut self) -> ParseResult<Vec<Spanned<Expr>>> {
        let mut args = Vec::new();
        if self.at(&Token::RParen) {
            return Ok(args);
        }
        args.push(self.parse_expr()?);
        while self.at(&Token::Comma) {
            self.advance();
            if self.at(&Token::RParen) {
                break;
            }
            args.push(self.parse_expr()?);
        }
        Ok(args)
    }

    fn parse_primary(&mut self) -> ParseResult<Spanned<Expr>> {
        match self.peek().cloned() {
            // Int literal
            Some(Token::IntLiteral(n)) => {
                let (_, span) = self.advance();
                Ok(Spanned::new(Expr::Int(n), span))
            }
            // Float literal
            Some(Token::FloatLiteral(n)) => {
                let (_, span) = self.advance();
                Ok(Spanned::new(Expr::Float(n), span))
            }
            // String literal
            Some(Token::StringLiteral(s)) => {
                let (_, span) = self.advance();
                Ok(Spanned::new(Expr::String(s), span))
            }
            // Rawcode literal
            Some(Token::RawcodeLiteral(s)) => {
                let (_, span) = self.advance();
                Ok(Spanned::new(Expr::Rawcode(s), span))
            }
            // Bool
            Some(Token::True) => {
                let (_, span) = self.advance();
                Ok(Spanned::new(Expr::Bool(true), span))
            }
            Some(Token::False) => {
                let (_, span) = self.advance();
                Ok(Spanned::new(Expr::Bool(false), span))
            }
            // Clone
            Some(Token::Clone) => {
                let start = self.peek_span();
                self.advance();
                self.expect(&Token::LParen)?;
                let inner = self.parse_expr()?;
                let end = self.expect(&Token::RParen)?;
                Ok(Spanned::new(Expr::Clone(Box::new(inner)), start.merge(end)))
            }
            // Todo
            Some(Token::Todo) => {
                let start = self.peek_span();
                self.advance();
                let msg = if self.at(&Token::LParen) {
                    self.advance();
                    let m = match self.peek().cloned() {
                        Some(Token::StringLiteral(s)) => {
                            self.advance();
                            Some(s)
                        }
                        _ => None,
                    };
                    self.expect(&Token::RParen)?;
                    m
                } else {
                    None
                };
                Ok(Spanned::new(Expr::Todo(msg), start.merge(self.prev_span())))
            }
            // (removed — tuples now use LParen below)
            // List: [a, b, c] or [head | tail]
            Some(Token::LBracket) => {
                let start = self.peek_span();
                self.advance();
                if self.at(&Token::RBracket) {
                    let end = self.expect(&Token::RBracket)?;
                    return Ok(Spanned::new(Expr::List(Vec::new()), start.merge(end)));
                }
                let first = self.parse_expr()?;
                if self.at(&Token::Bar) {
                    // [head | tail] cons expression
                    self.advance();
                    let tail = self.parse_expr()?;
                    let end = self.expect(&Token::RBracket)?;
                    Ok(Spanned::new(
                        Expr::ListCons {
                            head: Box::new(first),
                            tail: Box::new(tail),
                        },
                        start.merge(end),
                    ))
                } else {
                    // [a, b, c] regular list
                    let mut elems = vec![first];
                    while self.at(&Token::Comma) {
                        self.advance();
                        if self.at(&Token::RBracket) {
                            break;
                        }
                        elems.push(self.parse_expr()?);
                    }
                    let end = self.expect(&Token::RBracket)?;
                    Ok(Spanned::new(Expr::List(elems), start.merge(end)))
                }
            }
            // Grouping: (expr) or lambda: fn(params) { body }
            Some(Token::Fn) => self.parse_lambda(),
            Some(Token::LParen) => {
                let start = self.peek_span();
                self.advance();
                if self.at(&Token::RParen) {
                    let end = self.expect(&Token::RParen)?;
                    return Ok(Spanned::new(Expr::Tuple(Vec::new()), start.merge(end)));
                }
                let first = self.parse_expr()?;
                if self.at(&Token::Comma) {
                    let mut elems = vec![first];
                    while self.at(&Token::Comma) {
                        self.advance();
                        if self.at(&Token::RParen) {
                            break;
                        }
                        elems.push(self.parse_expr()?);
                    }
                    let end = self.expect(&Token::RParen)?;
                    Ok(Spanned::new(Expr::Tuple(elems), start.merge(end)))
                } else {
                    let end = self.expect(&Token::RParen)?;
                    Ok(Spanned::new(first.node, start.merge(end)))
                }
            }
            // Block: { ... }
            Some(Token::LBrace) => self.parse_block(),
            // Upper ident: Constructor or RecordUpdate
            Some(Token::UpperIdent(type_or_ctor)) => {
                let (_, start) = self.advance();
                // Check for qualified variant: Type::Variant
                let name = if self.at(&Token::ColonColon) {
                    self.advance();
                    let (variant, _) = self.expect_upper_ident()?;
                    format!("{}::{}", type_or_ctor, variant)
                } else {
                    type_or_ctor
                };
                if self.at(&Token::LBrace) {
                    self.parse_brace_constructor_or_update(name, start)
                } else if self.at(&Token::LParen) {
                    self.parse_constructor_or_update(name, start)
                } else {
                    // Bare constructor with no args
                    Ok(Spanned::new(
                        Expr::Constructor {
                            name,
                            args: Vec::new(),
                        },
                        start,
                    ))
                }
            }
            // Lower ident: variable
            Some(Token::LowerIdent(name)) => {
                let (_, span) = self.advance();
                Ok(Spanned::new(Expr::Var(name), span))
            }
            other => Err(ParseError::new(
                format!("expected expression, got {:?}", other),
                self.peek_span(),
            )),
        }
    }

    fn parse_lambda(&mut self) -> ParseResult<Spanned<Expr>> {
        let start = self.peek_span();
        self.expect(&Token::Fn)?;
        self.expect(&Token::LParen)?;
        let params = self.parse_params()?;
        self.expect(&Token::RParen)?;

        let return_type = if self.at(&Token::Arrow) {
            self.advance();
            Some(self.parse_type_expr()?)
        } else {
            None
        };

        let body = self.parse_block()?;
        let span = start.merge(body.span);

        Ok(Spanned::new(
            Expr::Lambda {
                params,
                return_type,
                body: Box::new(body),
            },
            span,
        ))
    }

    fn parse_constructor_or_update(
        &mut self,
        name: String,
        start: Span,
    ) -> ParseResult<Spanned<Expr>> {
        self.expect(&Token::LParen)?;

        // Regular constructor: Name(arg1, arg2) or Name(field: val)
        let mut args = Vec::new();
        if !self.at(&Token::RParen) {
            args.push(self.parse_constructor_arg()?);
            while self.at(&Token::Comma) {
                self.advance();
                if self.at(&Token::RParen) {
                    break;
                }
                args.push(self.parse_constructor_arg()?);
            }
        }
        let end = self.expect(&Token::RParen)?;

        Ok(Spanned::new(
            Expr::Constructor { name, args },
            start.merge(end),
        ))
    }

    /// Parse `Name { field: val, short, ..base, field: val }`
    fn parse_brace_constructor_or_update(
        &mut self,
        name: String,
        start: Span,
    ) -> ParseResult<Spanned<Expr>> {
        self.expect(&Token::LBrace)?;

        if self.at(&Token::DotDot) {
            self.advance();
            let base = self.parse_expr()?;
            let mut updates = Vec::new();
            while self.at(&Token::Comma) {
                self.advance();
                if self.at(&Token::RBrace) {
                    break;
                }
                let (field_name, field_span) = self.expect_lower_ident()?;
                if self.at(&Token::Colon) {
                    self.advance();
                    let value = self.parse_expr()?;
                    updates.push((field_name, value));
                } else {
                    let var_expr = Spanned::new(Expr::Var(field_name.clone()), field_span);
                    updates.push((field_name, var_expr));
                }
            }
            let end = self.expect(&Token::RBrace)?;
            return Ok(Spanned::new(
                Expr::RecordUpdate {
                    name,
                    base: Box::new(base),
                    updates,
                },
                start.merge(end),
            ));
        }

        // Named constructor: Name { field: val, shorthand, ... }
        let mut args = Vec::new();
        if !self.at(&Token::RBrace) {
            args.push(self.parse_brace_constructor_arg()?);
            while self.at(&Token::Comma) {
                self.advance();
                if self.at(&Token::RBrace) {
                    break;
                }
                args.push(self.parse_brace_constructor_arg()?);
            }
        }
        let end = self.expect(&Token::RBrace)?;

        Ok(Spanned::new(
            Expr::Constructor { name, args },
            start.merge(end),
        ))
    }

    /// Parse one field in `Name { field: val }` or shorthand `Name { field }`.
    fn parse_brace_constructor_arg(&mut self) -> ParseResult<ConstructorArg> {
        let (field_name, field_span) = self.expect_lower_ident()?;
        if self.at(&Token::Colon) {
            // field: value
            self.advance();
            let value = self.parse_expr()?;
            Ok(ConstructorArg::Named(field_name, value))
        } else {
            // shorthand: `field` means `field: field`
            let var_expr = Spanned::new(Expr::Var(field_name.clone()), field_span);
            Ok(ConstructorArg::Named(field_name, var_expr))
        }
    }

    fn parse_constructor_arg(&mut self) -> ParseResult<ConstructorArg> {
        // Try named: ident ':'
        if let Some(Token::LowerIdent(_)) = self.peek() {
            let saved = self.pos;
            // Safe: we just checked peek() is LowerIdent
            let (name, _) = self.expect_lower_ident()?;
            if self.at(&Token::Colon) {
                self.advance();
                let value = self.parse_expr()?;
                return Ok(ConstructorArg::Named(name, value));
            }
            // Not named, backtrack
            self.pos = saved;
        }
        let expr = self.parse_expr()?;
        Ok(ConstructorArg::Positional(expr))
    }

    // === Patterns ===

    /// Parse OR pattern: single { "|" single }
    fn parse_or_pattern(&mut self) -> ParseResult<Spanned<Pattern>> {
        let first = self.parse_single_pattern()?;

        if !self.at(&Token::Bar) {
            return Ok(first);
        }

        let mut alternatives = vec![first];
        while self.at(&Token::Bar) {
            self.advance();
            alternatives.push(self.parse_single_pattern()?);
        }

        let start = alternatives
            .first()
            .map(|p| p.span)
            .unwrap_or(Span::new(0, 0));
        let end = alternatives.last().map(|p| p.span).unwrap_or(start);
        Ok(Spanned::new(Pattern::Or(alternatives), start.merge(end)))
    }

    /// Parse a single pattern used in let bindings and inside OR.
    fn parse_pattern(&mut self) -> ParseResult<Spanned<Pattern>> {
        let pattern = self.parse_single_pattern()?;
        if self.at(&Token::As) {
            self.advance();
            let (name, name_span) = self.expect_lower_ident()?;
            let span = pattern.span.merge(name_span);
            Ok(Spanned::new(
                Pattern::As {
                    pattern: Box::new(pattern),
                    name,
                },
                span,
            ))
        } else {
            Ok(pattern)
        }
    }

    fn parse_single_pattern(&mut self) -> ParseResult<Spanned<Pattern>> {
        match self.peek().cloned() {
            // Discard
            Some(Token::LowerIdent(ref s)) if s == "_" => {
                let (_, span) = self.advance();
                Ok(Spanned::new(Pattern::Discard, span))
            }
            // Variable or qualified constant (module.CONST_NAME)
            Some(Token::LowerIdent(name)) => {
                let (_, start) = self.advance();
                // Check for qualified constant: module.CONST_NAME
                let is_qualified_const = self.at(&Token::Dot)
                    && matches!(
                        self.tokens.get(self.pos + 1).map(|(t, _)| t.clone()),
                        Some(Token::UpperIdent(_))
                    );
                if is_qualified_const {
                    self.advance(); // consume .
                    let (const_name, end) = self.expect_upper_ident()?;
                    let qualified = format!("{}.{}", name, const_name);
                    return Ok(Spanned::new(
                        Pattern::Constructor {
                            name: qualified,
                            args: Vec::new(),
                        },
                        start.merge(end),
                    ));
                }
                Ok(Spanned::new(Pattern::Var(name), start))
            }
            // Constructor: positional () or named {}
            Some(Token::UpperIdent(type_or_ctor)) => {
                let (_, start) = self.advance();
                // Check for qualified variant: Type::Variant
                let name = if self.at(&Token::ColonColon) {
                    self.advance();
                    let (variant, _) = self.expect_upper_ident()?;
                    format!("{}::{}", type_or_ctor, variant)
                } else {
                    type_or_ctor
                };
                if self.at(&Token::LParen) {
                    // Positional: Constructor(pat, pat)
                    self.advance();
                    let mut args = Vec::new();
                    if !self.at(&Token::RParen) {
                        args.push(self.parse_pattern()?);
                        while self.at(&Token::Comma) {
                            self.advance();
                            if self.at(&Token::RParen) {
                                break;
                            }
                            args.push(self.parse_pattern()?);
                        }
                    }
                    let end = self.expect(&Token::RParen)?;
                    Ok(Spanned::new(
                        Pattern::Constructor { name, args },
                        start.merge(end),
                    ))
                } else if self.at(&Token::LBrace) {
                    // Named: Constructor { field as var, .. }
                    self.advance();
                    let mut fields = Vec::new();
                    let mut rest = false;
                    if !self.at(&Token::RBrace) {
                        loop {
                            if self.at(&Token::DotDot) {
                                self.advance();
                                rest = true;
                                // Allow trailing comma after ..
                                if self.at(&Token::Comma) {
                                    self.advance();
                                }
                                break;
                            }
                            fields.push(self.parse_field_pattern()?);
                            if !self.at(&Token::Comma) {
                                break;
                            }
                            self.advance();
                            if self.at(&Token::RBrace) {
                                break;
                            }
                        }
                    }
                    let end = self.expect(&Token::RBrace)?;
                    Ok(Spanned::new(
                        Pattern::ConstructorNamed { name, fields, rest },
                        start.merge(end),
                    ))
                } else {
                    // Nullary constructor
                    Ok(Spanned::new(
                        Pattern::Constructor {
                            name,
                            args: Vec::new(),
                        },
                        start,
                    ))
                }
            }
            // Int literal
            Some(Token::IntLiteral(n)) => {
                let (_, span) = self.advance();
                Ok(Spanned::new(Pattern::Int(n), span))
            }
            // Rawcode literal
            Some(Token::RawcodeLiteral(s)) => {
                let (_, span) = self.advance();
                Ok(Spanned::new(Pattern::Rawcode(s), span))
            }
            // Float literal — disallowed in patterns (floating point equality is unreliable)
            Some(Token::FloatLiteral(_)) => {
                let span = self.peek_span();
                Err(ParseError::new(
                    "cannot match on Float literals (floating point equality is unreliable)"
                        .to_string(),
                    span,
                ))
            }
            // String literal
            Some(Token::StringLiteral(s)) => {
                let (_, span) = self.advance();
                Ok(Spanned::new(Pattern::String(s), span))
            }
            // Bool
            Some(Token::True) => {
                let (_, span) = self.advance();
                Ok(Spanned::new(Pattern::Bool(true), span))
            }
            Some(Token::False) => {
                let (_, span) = self.advance();
                Ok(Spanned::new(Pattern::Bool(false), span))
            }
            // Tuple pattern: (a, b)
            Some(Token::LParen) => {
                let start = self.peek_span();
                self.advance();
                if self.at(&Token::RParen) {
                    let end = self.expect(&Token::RParen)?;
                    return Ok(Spanned::new(Pattern::Tuple(Vec::new()), start.merge(end)));
                }
                let first = self.parse_pattern()?;
                if self.at(&Token::Comma) {
                    let mut elems = vec![first];
                    while self.at(&Token::Comma) {
                        self.advance();
                        if self.at(&Token::RParen) {
                            break;
                        }
                        elems.push(self.parse_pattern()?);
                    }
                    let end = self.expect(&Token::RParen)?;
                    Ok(Spanned::new(Pattern::Tuple(elems), start.merge(end)))
                } else {
                    let end = self.expect(&Token::RParen)?;
                    Ok(Spanned::new(first.node, start.merge(end)))
                }
            }
            // List pattern: [a, b | tail]
            Some(Token::LBracket) => {
                let start = self.peek_span();
                self.advance();
                if self.at(&Token::RBracket) {
                    let end = self.expect(&Token::RBracket)?;
                    return Ok(Spanned::new(Pattern::List(Vec::new()), start.merge(end)));
                }

                let mut elems = Vec::new();
                elems.push(self.parse_pattern()?);
                while self.at(&Token::Comma) {
                    self.advance();
                    if self.at(&Token::RBracket) || self.at(&Token::Bar) {
                        break;
                    }
                    elems.push(self.parse_pattern()?);
                }

                if self.at(&Token::Bar) {
                    // [head | tail] pattern
                    self.advance();
                    let tail = self.parse_pattern()?;
                    let end = self.expect(&Token::RBracket)?;
                    // Build nested ListCons from elems
                    let mut result = tail;
                    for elem in elems.into_iter().rev() {
                        let span = elem.span.merge(result.span);
                        result = Spanned::new(
                            Pattern::ListCons {
                                head: Box::new(elem),
                                tail: Box::new(result),
                            },
                            span,
                        );
                    }
                    result.span = start.merge(end);
                    Ok(result)
                } else {
                    let end = self.expect(&Token::RBracket)?;
                    Ok(Spanned::new(Pattern::List(elems), start.merge(end)))
                }
            }
            other => Err(ParseError::new(
                format!("expected pattern, got {:?}", other),
                self.peek_span(),
            )),
        }
    }

    /// Parse a named field pattern:
    /// - `field_name` — shorthand, binds to field_name
    /// - `field_name as var_name` — binds to var_name
    /// - `field_name: pattern` — nested pattern destructuring
    fn parse_field_pattern(&mut self) -> ParseResult<FieldPattern> {
        let (field_name, _span) = self.expect_lower_ident()?;
        let pattern = if self.at(&Token::As) {
            self.advance();
            let (var_name, var_span) = self.expect_lower_ident()?;
            Some(Spanned::new(Pattern::Var(var_name), var_span))
        } else if self.at(&Token::Colon) {
            self.advance();
            Some(self.parse_pattern()?)
        } else {
            None
        };
        Ok(FieldPattern {
            field_name,
            pattern,
        })
    }

    // === Type expressions ===

    pub fn parse_type_expr(&mut self) -> ParseResult<TypeExpr> {
        match self.peek() {
            Some(Token::Fn) => {
                self.advance();
                self.expect(&Token::LParen)?;
                let mut params = Vec::new();
                if !self.at(&Token::RParen) {
                    params.push(self.parse_type_expr()?);
                    while self.at(&Token::Comma) {
                        self.advance();
                        params.push(self.parse_type_expr()?);
                    }
                }
                self.expect(&Token::RParen)?;
                self.expect(&Token::Arrow)?;
                let ret = self.parse_type_expr()?;
                Ok(TypeExpr::Fn {
                    params,
                    ret: Box::new(ret),
                })
            }
            Some(Token::LParen) => {
                self.advance();
                if self.at(&Token::RParen) {
                    self.advance();
                    return Ok(TypeExpr::Tuple(Vec::new()));
                }
                let first = self.parse_type_expr()?;
                if self.at(&Token::Comma) {
                    let mut elems = vec![first];
                    while self.at(&Token::Comma) {
                        self.advance();
                        if self.at(&Token::RParen) {
                            break;
                        }
                        elems.push(self.parse_type_expr()?);
                    }
                    self.expect(&Token::RParen)?;
                    Ok(TypeExpr::Tuple(elems))
                } else {
                    self.expect(&Token::RParen)?;
                    Ok(first)
                }
            }
            Some(Token::UpperIdent(_)) | Some(Token::LowerIdent(_)) => {
                // UpperIdent = concrete type (Int, String, Option, ...)
                // LowerIdent = type variable (a, b, ...) OR module prefix (option.Option)
                let (first, _) = if matches!(self.peek(), Some(Token::UpperIdent(_))) {
                    self.expect_upper_ident()?
                } else {
                    self.expect_lower_ident()?
                };

                // Check for qualified type: module.Type
                let name =
                    if first.chars().next().unwrap_or('A').is_lowercase() && self.at(&Token::Dot) {
                        // Could be module.Type — peek ahead
                        let saved = self.pos;
                        self.advance(); // consume '.'
                        if matches!(self.peek(), Some(Token::UpperIdent(_))) {
                            let (type_name, _) = self.expect_upper_ident()?;
                            format!("{}.{}", first, type_name)
                        } else {
                            // Not a qualified type, backtrack — it's a type variable
                            self.pos = saved;
                            first
                        }
                    } else {
                        first
                    };

                let args = if self.at(&Token::LParen) {
                    self.advance();
                    let mut args = Vec::new();
                    if !self.at(&Token::RParen) {
                        args.push(self.parse_type_expr()?);
                        while self.at(&Token::Comma) {
                            self.advance();
                            args.push(self.parse_type_expr()?);
                        }
                    }
                    self.expect(&Token::RParen)?;
                    args
                } else {
                    Vec::new()
                };
                Ok(TypeExpr::Named { name, args })
            }
            other => Err(ParseError::new(
                format!("expected type, got {:?}", other),
                self.peek_span(),
            )),
        }
    }

    fn prev_span(&self) -> Span {
        self.pos
            .checked_sub(1)
            .and_then(|i| self.tokens.get(i))
            .map(|(_, s)| *s)
            .unwrap_or(Span::new(0, 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::Lexer;
    use rstest::rstest;

    fn parse(source: &str) -> Module {
        let tokens = Lexer::tokenize(source).expect("lex failed");
        let mut parser = Parser::new(tokens);
        let output = parser.parse_module();
        assert!(
            output.errors.is_empty(),
            "parse errors: {:?}",
            output.errors
        );
        output.module
    }

    fn parse_expr_str(source: &str) -> Spanned<Expr> {
        let tokens = Lexer::tokenize(source).expect("lex failed");
        let mut parser = Parser::new(tokens);
        parser.parse_expr().unwrap()
    }

    // --- Definition snapshots ---

    #[test]
    fn simple_function() {
        insta::assert_debug_snapshot!(parse("fn add(a: Int, b: Int) -> Int { a + b }"));
    }

    #[test]
    fn pub_local_function() {
        insta::assert_debug_snapshot!(parse("pub local fn cam() { True }"));
    }

    #[test]
    fn type_def() {
        insta::assert_debug_snapshot!(parse(
            "pub enum Phase { Lobby Playing { wave: Int } Victory { winner: Player } }"
        ));
    }

    #[test]
    fn type_def_with_params() {
        insta::assert_debug_snapshot!(parse(
            "enum Result(A, B) { Ok { value: A } Err { error: B } }"
        ));
    }

    #[test]
    fn import_simple() {
        insta::assert_debug_snapshot!(parse("import jass/unit"));
    }

    #[test]
    fn import_with_alias() {
        insta::assert_debug_snapshot!(parse("import jass/effect as fx"));
    }

    #[test]
    fn external_def() {
        insta::assert_debug_snapshot!(parse(
            r#"@external("jass", "GetUnitX") pub fn get_unit_x(u: Unit) -> Float"#
        ));
    }

    // --- Expression snapshots ---

    #[rstest]
    #[case::int("42")]
    #[case::float("3.14")]
    #[case::string(r#""hello""#)]
    #[case::rawcode("'hfoo'")]
    #[case::bool_true("True")]
    #[case::bool_false("False")]
    #[case::var("x")]
    #[case::negation("-x")]
    #[case::not("!done")]
    fn literal_and_unary(#[case] input: &str) {
        insta::assert_debug_snapshot!(input, parse_expr_str(input));
    }

    #[rstest]
    #[case::add("a + b")]
    #[case::sub("a - b")]
    #[case::mul("a * b")]
    #[case::div("a / b")]
    #[case::modulo("a % b")]
    #[case::eq("a == b")]
    #[case::neq("a != b")]
    #[case::lt("a < b")]
    #[case::gt("a > b")]
    #[case::le("a <= b")]
    #[case::ge("a >= b")]
    #[case::and("a && b")]
    #[case::or("a || b")]
    #[case::concat(r#""a" <> "b""#)]
    fn binop(#[case] input: &str) {
        insta::assert_debug_snapshot!(input, parse_expr_str(input));
    }

    #[test]
    fn precedence() {
        // a + b * c should parse as a + (b * c)
        insta::assert_debug_snapshot!(parse_expr_str("a + b * c"));
    }

    #[test]
    fn case_expr() {
        insta::assert_debug_snapshot!(parse_expr_str("case x { True -> 1 False -> 0 }"));
    }

    #[test]
    fn case_with_guard() {
        insta::assert_debug_snapshot!(parse_expr_str("case d { n if n > 100 -> 1 _ -> 0 }"));
    }

    #[test]
    fn case_with_constructor_pattern() {
        insta::assert_debug_snapshot!(parse_expr_str(
            "case msg { Tick -> 1 UnitDied(killer, bounty) -> 2 _ -> 0 }"
        ));
    }

    #[test]
    fn pipe() {
        insta::assert_debug_snapshot!(parse_expr_str("a |> f |> g(x)"));
    }

    #[test]
    fn lambda() {
        insta::assert_debug_snapshot!(parse_expr_str("fn(x: Int) { x + 1 }"));
    }

    #[test]
    fn tuple() {
        insta::assert_debug_snapshot!(parse_expr_str("(1, 2, 3)"));
    }

    #[test]
    fn list() {
        insta::assert_debug_snapshot!(parse_expr_str("[1, 2, 3]"));
    }

    #[test]
    fn record_update() {
        insta::assert_debug_snapshot!(parse_expr_str("Model { ..old, wave: 5 }"));
    }

    #[test]
    fn record_update_paren_rejected() {
        let tokens = crate::token::Lexer::tokenize("Model(..old, wave: 5)").unwrap();
        let mut parser = super::Parser::new(tokens);
        let result = parser.parse_module();
        assert!(
            !result.errors.is_empty(),
            "parenthesized record update should be rejected"
        );
    }

    #[test]
    fn constructor_named_args() {
        insta::assert_debug_snapshot!(parse_expr_str("Model(phase: Lobby, wave: 0)"));
    }

    #[test]
    fn method_call() {
        insta::assert_debug_snapshot!(parse_expr_str("hero.is_alive()"));
    }

    #[test]
    fn field_access() {
        insta::assert_debug_snapshot!(parse_expr_str("model.wave"));
    }

    #[test]
    fn clone_expr() {
        insta::assert_debug_snapshot!(parse_expr_str("clone(model)"));
    }

    #[test]
    fn todo_expr() {
        insta::assert_debug_snapshot!(parse_expr_str("todo(\"not implemented\")"));
    }

    #[test]
    fn list_cons_pattern() {
        insta::assert_debug_snapshot!(parse_expr_str("case xs { [h | t] -> h _ -> 0 }"));
    }

    #[test]
    fn let_binding() {
        insta::assert_debug_snapshot!(parse_expr_str("let x: Int = 5 x"));
    }

    #[test]
    fn unnamed_fields() {
        insta::assert_debug_snapshot!(parse("pub enum X { Y(Int) Z(String, Bool) }"));
    }

    #[test]
    fn struct_like_variant() {
        insta::assert_debug_snapshot!(parse("pub enum X { Y { val: Int, name: String } }"));
    }

    #[test]
    fn mixed_tuple_and_struct_variants() {
        insta::assert_debug_snapshot!(parse("pub enum X { Y { val: Int } Z(String) W }"));
    }

    // --- Advanced pattern tests ---

    #[test]
    fn or_pattern() {
        insta::assert_debug_snapshot!(parse_expr_str("case c { Red | Green -> 1 Blue -> 2 }"));
    }

    #[test]
    fn or_pattern_with_bindings() {
        insta::assert_debug_snapshot!(parse_expr_str(
            "case e { Chat(p, _) | Quit(p) -> p _ -> 0 }"
        ));
    }

    #[test]
    fn named_field_pattern() {
        insta::assert_debug_snapshot!(parse_expr_str(
            "case e { Chat { from, text } -> from _ -> 0 }"
        ));
    }

    #[test]
    fn named_field_pattern_with_as() {
        insta::assert_debug_snapshot!(parse_expr_str(
            "case e { Chat { from as p, .. } -> p _ -> 0 }"
        ));
    }

    #[test]
    fn pattern_as_binding() {
        insta::assert_debug_snapshot!(parse_expr_str(
            "case e { Chat(p, _) as event -> event _ -> 0 }"
        ));
    }

    #[test]
    fn or_pattern_with_as_and_guard() {
        insta::assert_debug_snapshot!(parse_expr_str(
            "case c { Chat { from as p, .. } | Quit(p) as event if check(p) -> event _ -> 0 }"
        ));
    }

    #[test]
    fn named_field_rest_only() {
        insta::assert_debug_snapshot!(parse_expr_str("case e { Chat { .. } -> 1 _ -> 0 }"));
    }

    // --- Full program snapshot ---

    #[test]
    fn full_program() {
        let source = r#"
pub enum Msg {
    Tick
    UnitDied { killer: Player, bounty: Int }
}

pub fn update(model: Model, msg: Msg) -> (Model, List(Effect(Msg))) {
    case msg {
        Msg::Tick -> (model, [])
        Msg::UnitDied(killer, bounty) -> (model, [])
    }
}
"#;
        insta::assert_debug_snapshot!(parse(source));
    }

    #[test]
    fn recovers_after_bad_definition() {
        let tokens = Lexer::tokenize("fn foo( fn bar(x: Int) -> Int { x }").expect("lex failed");
        let mut parser = Parser::new(tokens);
        let output = parser.parse_module();
        assert_eq!(output.errors.len(), 1);
        assert_eq!(output.module.definitions.len(), 1);
    }

    #[test]
    fn recovers_multiple_errors() {
        let tokens = Lexer::tokenize("fn a( fn b( fn c(x: Int) -> Int { x }").expect("lex failed");
        let mut parser = Parser::new(tokens);
        let output = parser.parse_module();
        assert_eq!(output.errors.len(), 2);
        assert_eq!(output.module.definitions.len(), 1);
    }

    #[test]
    fn recovers_at_enum_sync_point() {
        let tokens = Lexer::tokenize("fn bad( enum Color { Red Green Blue }").expect("lex failed");
        let mut parser = Parser::new(tokens);
        let output = parser.parse_module();
        assert_eq!(output.errors.len(), 1);
        assert_eq!(output.module.definitions.len(), 1);
    }

    #[test]
    fn recovers_at_struct_sync_point() {
        let tokens =
            Lexer::tokenize("fn bad( struct Point { x: Int, y: Int }").expect("lex failed");
        let mut parser = Parser::new(tokens);
        let output = parser.parse_module();
        assert_eq!(output.errors.len(), 1);
        assert_eq!(output.module.definitions.len(), 1);
    }

    #[test]
    fn recovers_at_import_sync_point() {
        let tokens = Lexer::tokenize("fn bad( import option").expect("lex failed");
        let mut parser = Parser::new(tokens);
        let output = parser.parse_module();
        assert_eq!(output.errors.len(), 1);
        assert_eq!(output.module.definitions.len(), 1);
    }

    #[test]
    fn recovers_at_const_sync_point() {
        let tokens = Lexer::tokenize("fn bad( const x: Int = 5").expect("lex failed");
        let mut parser = Parser::new(tokens);
        let output = parser.parse_module();
        assert_eq!(output.errors.len(), 1);
        assert_eq!(output.module.definitions.len(), 1);
    }

    #[test]
    fn recovers_at_pub_sync_point() {
        let tokens =
            Lexer::tokenize("fn bad( pub fn good(x: Int) -> Int { x }").expect("lex failed");
        let mut parser = Parser::new(tokens);
        let output = parser.parse_module();
        assert_eq!(output.errors.len(), 1);
        assert_eq!(output.module.definitions.len(), 1);
    }

    #[test]
    fn no_errors_on_valid_input() {
        let tokens = Lexer::tokenize("fn foo(x: Int) -> Int { x }").expect("lex failed");
        let mut parser = Parser::new(tokens);
        let output = parser.parse_module();
        assert!(output.errors.is_empty());
        assert_eq!(output.module.definitions.len(), 1);
    }
}
