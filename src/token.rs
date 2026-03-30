use logos::Logos;

#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r\n]+")]
#[logos(skip r"//[^\n]*")]
pub enum Token {
    // Keywords
    #[token("fn")]
    Fn,
    #[token("let")]
    Let,
    #[token("case")]
    Case,
    #[token("struct")]
    Struct,
    #[token("enum")]
    Enum,
    #[token("pub")]
    Pub,
    #[token("import")]
    Import,
    #[token("local")]
    Local,
    #[token("const")]
    Const,
    #[token("extend")]
    Extend,
    #[token("clone")]
    Clone,
    #[token("todo")]
    Todo,
    #[token("as")]
    As,
    #[token("True")]
    True,
    #[token("False")]
    False,

    // Operators (multi-char first to avoid ambiguity)
    #[token("|>")]
    Pipe,
    #[token("<>")]
    StringConcat,
    #[token("->")]
    Arrow,
    #[token("::")]
    ColonColon,
    #[token("..")]
    DotDot,
    #[token("==")]
    EqEq,
    #[token("!=")]
    NotEq,
    #[token("<=")]
    LessEq,
    #[token(">=")]
    GreaterEq,
    #[token("&&")]
    AndAnd,
    #[token("||")]
    OrOr,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("<")]
    Less,
    #[token(">")]
    Greater,
    #[token("!")]
    Bang,
    #[token("=")]
    Eq,

    // Delimiters
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token(".")]
    Dot,
    #[token("|")]
    Bar,
    #[token("@")]
    At,

    // Literals
    #[regex(r"0x[0-9a-fA-F]+", |lex| i64::from_str_radix(&lex.slice()[2..], 16).ok())]
    #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().ok(), priority = 2)]
    IntLiteral(i64),

    #[regex(r"[0-9]+\.[0-9]+", |lex| lex.slice().parse::<f64>().ok())]
    FloatLiteral(f64),

    #[regex(r#""([^"\\]|\\.)*""#, |lex| {
        let s = lex.slice();
        Some(s[1..s.len()-1].to_string())
    })]
    StringLiteral(String),

    #[regex(r"'[a-zA-Z0-9]{4}'", |lex| {
        let s = lex.slice();
        Some(s[1..s.len()-1].to_string())
    })]
    RawcodeLiteral(String),

    // Identifiers
    #[regex(r"[a-z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string(), priority = 1)]
    LowerIdent(String),

    #[regex(r"[A-Z][A-Z0-9]*_[A-Z0-9_]*", |lex| lex.slice().to_string(), priority = 2)]
    ConstIdent(String),

    #[regex(r"[A-Z][a-zA-Z0-9_]*", |lex| lex.slice().to_string(), priority = 1)]
    UpperIdent(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn merge(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LexError {
    pub span: Span,
    pub text: String,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unexpected character at {}..{}: {:?}",
            self.span.start, self.span.end, self.text
        )
    }
}

pub struct Lexer;

impl Lexer {
    pub fn tokenize(source: &str) -> Result<Vec<(Token, Span)>, LexError> {
        let mut lexer = Token::lexer(source);
        let mut tokens = Vec::new();
        while let Some(result) = lexer.next() {
            match result {
                Ok(token) => {
                    let span = lexer.span();
                    let token = match token {
                        Token::ConstIdent(s) => Token::LowerIdent(s),
                        other => other,
                    };
                    tokens.push((token, Span::new(span.start, span.end)));
                }
                Err(()) => {
                    let span = lexer.span();
                    return Err(LexError {
                        span: Span::new(span.start, span.end),
                        text: source[span.start..span.end].to_string(),
                    });
                }
            }
        }
        Ok(tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn token_kinds(source: &str) -> Vec<Token> {
        Lexer::tokenize(source)
            .expect("lex failed")
            .into_iter()
            .map(|(t, _)| t)
            .collect()
    }

    #[rstest]
    #[case("fn", Token::Fn)]
    #[case("let", Token::Let)]
    #[case("case", Token::Case)]
    #[case("struct", Token::Struct)]
    #[case("enum", Token::Enum)]
    #[case("pub", Token::Pub)]
    #[case("import", Token::Import)]
    #[case("local", Token::Local)]
    #[case("const", Token::Const)]
    #[case("extend", Token::Extend)]
    #[case("clone", Token::Clone)]
    #[case("todo", Token::Todo)]
    #[case("as", Token::As)]
    #[case("True", Token::True)]
    #[case("False", Token::False)]
    fn keyword(#[case] input: &str, #[case] expected: Token) {
        assert_eq!(token_kinds(input), vec![expected]);
    }

    #[rstest]
    #[case("|>", Token::Pipe)]
    #[case("<>", Token::StringConcat)]
    #[case("->", Token::Arrow)]
    #[case("..", Token::DotDot)]
    #[case("==", Token::EqEq)]
    #[case("!=", Token::NotEq)]
    #[case("<=", Token::LessEq)]
    #[case(">=", Token::GreaterEq)]
    #[case("&&", Token::AndAnd)]
    #[case("||", Token::OrOr)]
    #[case("+", Token::Plus)]
    #[case("-", Token::Minus)]
    #[case("*", Token::Star)]
    #[case("/", Token::Slash)]
    #[case("%", Token::Percent)]
    #[case("<", Token::Less)]
    #[case(">", Token::Greater)]
    #[case("!", Token::Bang)]
    #[case("=", Token::Eq)]
    fn operator(#[case] input: &str, #[case] expected: Token) {
        assert_eq!(token_kinds(input), vec![expected]);
    }

    #[rstest]
    #[case("42", Token::IntLiteral(42))]
    #[case("0xFF", Token::IntLiteral(255))]
    #[case("0", Token::IntLiteral(0))]
    fn int_literal(#[case] input: &str, #[case] expected: Token) {
        assert_eq!(token_kinds(input), vec![expected]);
    }

    #[rstest]
    #[case("3.14", Token::FloatLiteral(3.14))]
    #[case("0.0", Token::FloatLiteral(0.0))]
    fn float_literal(#[case] input: &str, #[case] expected: Token) {
        assert_eq!(token_kinds(input), vec![expected]);
    }

    #[rstest]
    #[case("\"hello\"", Token::StringLiteral("hello".into()))]
    #[case("\"\"", Token::StringLiteral("".into()))]
    #[case("\"with \\\"escape\\\"\"", Token::StringLiteral("with \\\"escape\\\"".into()))]
    fn string_literal(#[case] input: &str, #[case] expected: Token) {
        assert_eq!(token_kinds(input), vec![expected]);
    }

    #[rstest]
    #[case("'hfoo'", Token::RawcodeLiteral("hfoo".into()))]
    #[case("'A000'", Token::RawcodeLiteral("A000".into()))]
    fn rawcode_literal(#[case] input: &str, #[case] expected: Token) {
        assert_eq!(token_kinds(input), vec![expected]);
    }

    #[rstest]
    #[case("hello", Token::LowerIdent("hello".into()))]
    #[case("_private", Token::LowerIdent("_private".into()))]
    #[case("snake_case_42", Token::LowerIdent("snake_case_42".into()))]
    #[case("HOOK_ABILITY", Token::LowerIdent("HOOK_ABILITY".into()))]
    #[case("MAX_BOUNCES", Token::LowerIdent("MAX_BOUNCES".into()))]
    #[case("A_B", Token::LowerIdent("A_B".into()))]
    fn lower_ident(#[case] input: &str, #[case] expected: Token) {
        assert_eq!(token_kinds(input), vec![expected]);
    }

    #[rstest]
    #[case("World", Token::UpperIdent("World".into()))]
    #[case("MyType", Token::UpperIdent("MyType".into()))]
    fn upper_ident(#[case] input: &str, #[case] expected: Token) {
        assert_eq!(token_kinds(input), vec![expected]);
    }

    #[test]
    fn delimiters() {
        assert_eq!(
            token_kinds("( ) { } [ ] , : . | @"),
            vec![
                Token::LParen,
                Token::RParen,
                Token::LBrace,
                Token::RBrace,
                Token::LBracket,
                Token::RBracket,
                Token::Comma,
                Token::Colon,
                Token::Dot,
                Token::Bar,
                Token::At,
            ]
        );
    }

    #[test]
    fn comments_skipped() {
        let tokens = token_kinds("fn // this is a comment\nlet");
        assert_eq!(tokens, vec![Token::Fn, Token::Let]);
    }

    #[test]
    fn spans() {
        let tokens = Lexer::tokenize("fn add").expect("lex failed");
        assert_eq!(tokens[0].1, Span::new(0, 2));
        assert_eq!(tokens[1].1, Span::new(3, 6));
    }

    #[test]
    fn full_function() {
        insta::assert_debug_snapshot!(token_kinds("pub fn add(a: Int, b: Int) -> Int { a + b }"));
    }

    #[rstest]
    #[case("fn $ let", 3, 4, "$")]
    #[case("$", 0, 1, "$")]
    #[case("fn ~ let", 3, 4, "~")]
    #[case("let ` x", 4, 5, "`")]
    fn lex_error_stops_at_invalid_char(
        #[case] input: &str,
        #[case] start: usize,
        #[case] end: usize,
        #[case] text: &str,
    ) {
        let err = Lexer::tokenize(input).unwrap_err();
        assert_eq!(err.span, Span::new(start, end));
        assert_eq!(err.text, text);
    }

    #[test]
    fn valid_input_returns_ok() {
        assert!(Lexer::tokenize("fn add(x: Int) -> Int { x }").is_ok());
    }
}
