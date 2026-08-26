//! Token definitions for the Redstart lexer.
//!
//! Design notes:
//! - Type names (`BigInt`, `BigDecimal`, `Bytes`, `Address`, `Option`, `Result`,
//!   `List`, `Id`) and constructors (`Some`, `None`, `Ok`, `Err`) are NOT keywords.
//!   They are ordinary capitalised identifiers resolved by the checker. This keeps
//!   the surface grammar small and lets the "uppercase = type/constructor"
//!   convention do the work.
//! - There is exactly one equality operator (`==`). AssemblyScript's `===`
//!   identity-vs-value inversion is simply absent from the grammar by design.

use logos::Logos;
use std::fmt;

/// A lexical token. Spans are tracked separately by the lexer (see `lexer.rs`).
#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[logos(skip r"[ \t\r\n\f]+")] // whitespace
#[logos(skip r"//[^\n]*")] // line comments
#[logos(skip r"/\*([^*]|\*[^/])*\*/")] // block comments
pub enum Token {
    // ---- structural keywords ----
    #[token("abi")]
    KwAbi,
    #[token("from")]
    KwFrom,
    #[token("entity")]
    KwEntity,
    #[token("enum")]
    KwEnum,
    #[token("interface")]
    KwInterface,
    #[token("aggregation")]
    KwAggregation,
    #[token("source")]
    KwSource,
    #[token("template")]
    KwTemplate,
    #[token("graft")]
    KwGraft,
    #[token("handler")]
    KwHandler,
    #[token("on")]
    KwOn,
    #[token("derived")]
    KwDerived,
    #[token("match")]
    KwMatch,
    #[token("let")]
    KwLet,
    #[token("return")]
    KwReturn,
    #[token("if")]
    KwIf,
    #[token("else")]
    KwElse,
    #[token("while")]
    KwWhile,
    #[token("for")]
    KwFor,
    #[token("in")]
    KwIn,
    #[token("fn")]
    KwFn,
    #[token("mod")]
    KwMod,
    #[token("use")]
    KwUse,
    #[token("test")]
    KwTest,
    #[token("true")]
    KwTrue,
    #[token("false")]
    KwFalse,

    // ---- literals ----
    // Hex must out-rank Int (it does, by longest match) for `0x...` addresses/bytes.
    #[regex(r"0x[0-9a-fA-F]+")]
    HexLit,
    #[regex(r"[0-9][0-9_]*\.[0-9][0-9_]*")]
    DecimalLit,
    #[regex(r"[0-9][0-9_]*")]
    IntLit,
    #[regex(r#""([^"\\]|\\.)*""#)]
    StringLit,
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*")]
    Ident,

    // ---- punctuation & operators ----
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
    #[token("::")]
    PathSep,
    #[token(":")]
    Colon,
    #[token(";")]
    Semi,
    #[token(",")]
    Comma,
    #[token("=>")]
    FatArrow,
    #[token("->")]
    ThinArrow,
    #[token("..")]
    DotDot,
    #[token(".")]
    Dot,
    #[token("?")]
    Question,

    // comparison / equality — note: only `==`, never `===`
    #[token("==")]
    EqEq,
    #[token("!=")]
    NotEq,
    #[token("<=")]
    Le,
    #[token(">=")]
    Ge,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,

    // logical
    #[token("&&")]
    AndAnd,
    #[token("&")]
    Amp,
    #[token("||")]
    OrOr,
    #[token("!")]
    Bang,

    // arithmetic
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

    // assignment
    #[token("=")]
    Eq,
}

impl Token {
    /// A human-readable description of this token kind, for error messages.
    #[must_use]
    pub fn describe(self) -> &'static str {
        use Token::*;
        match self {
            KwAbi => "`abi`",
            KwFrom => "`from`",
            KwEntity => "`entity`",
            KwEnum => "`enum`",
            KwInterface => "`interface`",
            KwAggregation => "`aggregation`",
            KwSource => "`source`",
            KwTemplate => "`template`",
            KwGraft => "`graft`",
            KwHandler => "`handler`",
            KwOn => "`on`",
            KwDerived => "`derived`",
            KwMatch => "`match`",
            KwLet => "`let`",
            KwReturn => "`return`",
            KwIf => "`if`",
            KwElse => "`else`",
            KwWhile => "`while`",
            KwFor => "`for`",
            KwIn => "`in`",
            KwFn => "`fn`",
            KwMod => "`mod`",
            KwUse => "`use`",
            KwTest => "`test`",
            KwTrue => "`true`",
            KwFalse => "`false`",
            HexLit => "a hex literal",
            DecimalLit => "a decimal literal",
            IntLit => "an integer literal",
            StringLit => "a string literal",
            Ident => "an identifier",
            LParen => "`(`",
            RParen => "`)`",
            LBrace => "`{`",
            RBrace => "`}`",
            LBracket => "`[`",
            RBracket => "`]`",
            PathSep => "`::`",
            Colon => "`:`",
            Semi => "`;`",
            Comma => "`,`",
            FatArrow => "`=>`",
            ThinArrow => "`->`",
            DotDot => "`..`",
            Dot => "`.`",
            Question => "`?`",
            EqEq => "`==`",
            NotEq => "`!=`",
            Le => "`<=`",
            Ge => "`>=`",
            Lt => "`<`",
            Gt => "`>`",
            AndAnd => "`&&`",
            Amp => "`&`",
            OrOr => "`||`",
            Bang => "`!`",
            Plus => "`+`",
            Minus => "`-`",
            Star => "`*`",
            Slash => "`/`",
            Percent => "`%`",
            Eq => "`=`",
        }
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.describe())
    }
}
