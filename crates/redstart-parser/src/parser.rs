//! Recursive-descent parser for Redstart.
//!
//! Hand-rolled (per the design report's lean) rather than combinator-based: the
//! grammar is small enough that explicit control flow yields better error
//! recovery and clearer diagnostics. [`parse`] always returns a (possibly
//! partial) [`Program`] plus a list of [`ParseError`]s, so editor tooling can
//! work with incomplete input.

use crate::ast::*;
use crate::error::ParseError;
use crate::lexer::Spanned;
use crate::span::{Ident, Span};
use crate::token::Token;
use std::sync::Arc;

/// Parse a token slice into a [`Program`].
///
/// Returns the parsed program together with any recoverable errors. The program
/// is always returned (partial on error) to support tooling.
#[must_use]
pub fn parse(tokens: &[Spanned], source: Arc<str>) -> (Program, Vec<ParseError>) {
    let mut p = Parser::new(tokens, source);
    let program = p.parse_program();
    (program, p.errors)
}

struct Parser<'t> {
    tokens: &'t [Spanned],
    pos: usize,
    source: Arc<str>,
    errors: Vec<ParseError>,
}

type PResult<T> = Result<T, ParseError>;

impl<'t> Parser<'t> {
    fn new(tokens: &'t [Spanned], source: Arc<str>) -> Self {
        Self {
            tokens,
            pos: 0,
            source,
            errors: Vec::new(),
        }
    }

    // ---- cursor helpers ----

    fn peek(&self) -> Option<&Spanned> {
        self.tokens.get(self.pos)
    }

    fn peek_kind(&self) -> Option<Token> {
        self.peek().map(|s| s.token)
    }

    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn check(&self, kind: Token) -> bool {
        self.peek_kind() == Some(kind)
    }

    /// Advance, returning the token we moved past.
    fn bump(&mut self) -> Option<&Spanned> {
        let t = self.tokens.get(self.pos);
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn text(&self, sp: &Spanned) -> &str {
        &self.source[sp.start..sp.end]
    }

    fn src_len(&self) -> usize {
        self.source.len()
    }

    fn prev_end(&self) -> usize {
        if self.pos == 0 {
            0
        } else {
            self.tokens[self.pos - 1].end
        }
    }

    fn cur_start(&self) -> usize {
        self.peek().map_or_else(|| self.src_len(), |s| s.start)
    }

    fn span(&self, start: usize, end: usize) -> Span {
        Span::new(start, end, Arc::clone(&self.source))
    }

    fn span_from(&self, start: usize) -> Span {
        self.span(start, self.prev_end())
    }

    fn here_span(&self) -> Span {
        match self.peek() {
            Some(s) => self.span(s.start, s.end),
            None => self.span(self.src_len(), self.src_len()),
        }
    }

    fn err(&self, message: impl Into<String>, label: impl Into<String>) -> ParseError {
        ParseError::new(message, label, self.here_span())
    }

    /// Consume a token of the expected kind, or produce an error.
    fn expect(&mut self, kind: Token, context: &str) -> PResult<Spanned> {
        if self.check(kind) {
            Ok(self.bump().unwrap().clone())
        } else {
            let found = self
                .peek_kind()
                .map_or_else(|| "end of input".to_string(), |k| k.describe().to_string());
            Err(self.err(
                format!("expected {} {context}, found {found}", kind.describe()),
                format!("expected {}", kind.describe()),
            ))
        }
    }

    /// Consume an identifier and return it as an `Ident`.
    fn expect_ident(&mut self, context: &str) -> PResult<Ident> {
        if self.check(Token::Ident) {
            let sp = self.bump().unwrap().clone();
            Ok(Ident::new(
                self.text(&sp).to_string(),
                self.span(sp.start, sp.end),
            ))
        } else {
            let found = self
                .peek_kind()
                .map_or_else(|| "end of input".to_string(), |k| k.describe().to_string());
            Err(self.err(
                format!("expected an identifier {context}, found {found}"),
                "expected an identifier",
            ))
        }
    }

    /// Consume an identifier *or* a keyword used as a name, returning its text.
    ///
    /// Subgraph events freely use Solidity parameter names like `from`, `to`, and
    /// `value` — and `from` is one of our keywords. In data-shaped positions
    /// (field access, record keys, entity field names, setting keys) we therefore
    /// accept any keyword token as if it were a plain identifier.
    fn expect_ident_like(&mut self, context: &str) -> PResult<Ident> {
        match self.peek_kind() {
            Some(t) if t == Token::Ident || is_keyword(t) => {
                let sp = self.bump().unwrap().clone();
                Ok(Ident::new(
                    self.text(&sp).to_string(),
                    self.span(sp.start, sp.end),
                ))
            }
            _ => {
                let found = self
                    .peek_kind()
                    .map_or_else(|| "end of input".to_string(), |k| k.describe().to_string());
                Err(self.err(
                    format!("expected a name {context}, found {found}"),
                    "expected a name",
                ))
            }
        }
    }

    /// Optionally consume `pub` (lexed as an ordinary identifier).
    fn eat_pub(&mut self) -> bool {
        if let Some(sp) = self.peek() {
            if sp.token == Token::Ident && self.text(sp) == "pub" {
                self.bump();
                return true;
            }
        }
        false
    }

    // ---- top level ----

    fn parse_program(&mut self) -> Program {
        let mut program = Program::default();

        while !self.at_end() {
            // Snapshot position so we can guarantee forward progress on error.
            let before = self.pos;
            match self.parse_item(&mut program) {
                Ok(()) => {}
                Err(e) => {
                    self.errors.push(e);
                    self.synchronize();
                }
            }
            if self.pos == before {
                // No progress (defensive): skip one token to avoid an infinite loop.
                self.bump();
            }
        }

        program
    }

    fn parse_item(&mut self, program: &mut Program) -> PResult<()> {
        let is_pub = self.eat_pub();
        match self.peek_kind() {
            Some(Token::KwMod) => program.mods.push(self.parse_mod(is_pub)?),
            Some(Token::KwUse) => program.uses.push(self.parse_use()?),
            Some(Token::KwAbi) => program.abis.push(self.parse_abi()?),
            Some(Token::KwEntity) => program.entities.push(self.parse_entity()?),
            Some(Token::KwEnum) => program.enums.push(self.parse_enum()?),
            Some(Token::KwInterface) => program.interfaces.push(self.parse_interface()?),
            Some(Token::KwAggregation) => program.aggregations.push(self.parse_aggregation()?),
            Some(Token::KwSource) => program.sources.push(self.parse_source()?),
            Some(Token::KwTemplate) => program.templates.push(self.parse_template()?),
            Some(Token::KwGraft) => {
                let graft = self.parse_graft()?;
                if program.graft.is_some() {
                    // Point at the duplicate itself; `self.err` would use the
                    // cursor, which by now has moved past the whole block and
                    // lands on whatever declaration follows it.
                    return Err(ParseError::new(
                        "duplicate `graft` block",
                        "only one graft per subgraph",
                        graft.span.clone(),
                    )
                    .with_help(
                        "a manifest carries at most one `graft`; merge or remove the extra block",
                    ));
                }
                program.graft = Some(graft);
            }
            Some(Token::KwHandler) => program.handlers.push(self.parse_handler()?),
            Some(Token::KwFn) => program.functions.push(self.parse_fn(is_pub)?),
            Some(Token::KwTest) => program.tests.push(self.parse_test()?),
            _ => {
                return Err(self
                    .err("expected a top-level declaration", "unexpected token")
                    .with_help(
                        "top-level items are `abi`, `entity`, `source`, `template`, \
                         `handler`, `fn`, `test`, `graft`, `mod`, or `use`",
                    ));
            }
        }
        Ok(())
    }

    /// Skip tokens until the next likely declaration boundary.
    fn synchronize(&mut self) {
        while let Some(kind) = self.peek_kind() {
            if matches!(
                kind,
                Token::KwAbi
                    | Token::KwEntity
                    | Token::KwEnum
                    | Token::KwInterface
                    | Token::KwAggregation
                    | Token::KwSource
                    | Token::KwTemplate
                    | Token::KwGraft
                    | Token::KwHandler
                    | Token::KwFn
                    | Token::KwTest
                    | Token::KwMod
                    | Token::KwUse
            ) {
                return;
            }
            self.bump();
        }
    }

    fn parse_mod(&mut self, is_pub: bool) -> PResult<ModDecl> {
        let start = self.cur_start();
        self.expect(Token::KwMod, "to begin a module declaration")?;
        let name = self.expect_ident("for the module name")?;
        self.eat_semi();
        Ok(ModDecl {
            name,
            is_pub,
            span: self.span_from(start),
        })
    }

    fn parse_use(&mut self) -> PResult<UseDecl> {
        let start = self.cur_start();
        self.expect(Token::KwUse, "to begin an import")?;
        let mut path = vec![self.expect_ident("in the import path")?];
        while self.check(Token::PathSep) {
            self.bump();
            path.push(self.expect_ident("in the import path")?);
        }
        self.eat_semi();
        Ok(UseDecl {
            path,
            span: self.span_from(start),
        })
    }

    fn parse_abi(&mut self) -> PResult<AbiDecl> {
        let start = self.cur_start();
        self.expect(Token::KwAbi, "to begin an ABI import")?;
        let name = self.expect_ident("for the ABI name")?;
        self.expect(Token::KwFrom, "after the ABI name")
            .map_err(|e| e.with_help("ABI imports look like `abi Name from \"./path.json\"`"))?;
        let str_tok = self.expect(Token::StringLit, "for the ABI file path")?;
        let path = unescape_string(self.text(&str_tok));
        self.eat_semi();
        Ok(AbiDecl {
            name,
            path,
            span: self.span_from(start),
        })
    }

    fn parse_entity(&mut self) -> PResult<EntityDecl> {
        let start = self.cur_start();
        self.expect(Token::KwEntity, "to begin an entity")?;
        let name = self.expect_ident("for the entity name")?;

        // Optional `implements A & B` clause (before bare-ident modifiers, since
        // `implements` itself lexes as an identifier).
        let mut implements = Vec::new();
        if self.peek_text_is("implements") {
            self.bump();
            implements.push(self.expect_ident("for the implemented interface")?);
            while self.check(Token::Amp) {
                self.bump();
                implements.push(self.expect_ident("for an implemented interface")?);
            }
        }

        // Bare-identifier modifiers, e.g. `entity Swap immutable {`.
        let mut modifiers = Vec::new();
        while self.check(Token::Ident) {
            let sp = self.bump().unwrap().clone();
            modifiers.push(Ident::new(
                self.text(&sp).to_string(),
                self.span(sp.start, sp.end),
            ));
        }

        self.expect(Token::LBrace, "to open the entity body")?;
        let mut fields = Vec::new();
        while !self.check(Token::RBrace) && !self.at_end() {
            fields.push(self.parse_field()?);
            self.eat_comma();
        }
        self.expect(Token::RBrace, "to close the entity body")?;

        Ok(EntityDecl {
            name,
            implements,
            modifiers,
            fields,
            span: self.span_from(start),
        })
    }

    fn parse_interface(&mut self) -> PResult<InterfaceDecl> {
        let start = self.cur_start();
        self.expect(Token::KwInterface, "to begin an interface")?;
        let name = self.expect_ident("for the interface name")?;
        self.expect(Token::LBrace, "to open the interface body")?;
        let mut fields = Vec::new();
        while !self.check(Token::RBrace) && !self.at_end() {
            fields.push(self.parse_field()?);
            self.eat_comma();
        }
        self.expect(Token::RBrace, "to close the interface body")?;
        Ok(InterfaceDecl {
            name,
            fields,
            span: self.span_from(start),
        })
    }

    fn parse_aggregation(&mut self) -> PResult<AggregationDecl> {
        let start = self.cur_start();
        self.expect(Token::KwAggregation, "to begin an aggregation")?;
        let name = self.expect_ident("for the aggregation name")?;
        self.expect_contextual("over", "after the aggregation name")
            .map_err(|e| {
                e.with_help(
                    "aggregations look like `aggregation Stats over Data every [hour, day] { … }`",
                )
            })?;
        let source = self.expect_ident("for the source timeseries entity")?;
        self.expect_contextual("every", "before the interval list")?;
        self.expect(Token::LBracket, "to open the interval list")?;
        let mut intervals = Vec::new();
        while !self.check(Token::RBracket) && !self.at_end() {
            intervals.push(self.expect_ident("for an interval")?);
            self.eat_comma();
        }
        self.expect(Token::RBracket, "to close the interval list")?;

        self.expect(Token::LBrace, "to open the aggregation body")?;
        let mut fields = Vec::new();
        while !self.check(Token::RBrace) && !self.at_end() {
            fields.push(self.parse_aggregate_field()?);
            self.eat_comma();
        }
        self.expect(Token::RBrace, "to close the aggregation body")?;
        Ok(AggregationDecl {
            name,
            source,
            intervals,
            fields,
            span: self.span_from(start),
        })
    }

    fn parse_aggregate_field(&mut self) -> PResult<AggregateField> {
        let start = self.cur_start();
        let name = self.expect_ident_like("for the field name")?;
        self.expect(Token::Colon, "after the field name")?;
        let ty = self.parse_type()?;
        self.expect(Token::Eq, "before the aggregation function")
            .map_err(|e| {
                e.with_help("aggregated fields look like `total: BigDecimal = sum(price)`")
            })?;
        let func = self.expect_ident("for the aggregation function")?;
        self.expect(Token::LParen, "before the aggregation argument")?;
        let arg = if self.check(Token::RParen) {
            None
        } else {
            Some(self.expect_ident_like("for the aggregation argument")?)
        };
        self.expect(Token::RParen, "after the aggregation argument")?;
        Ok(AggregateField {
            name,
            ty,
            func,
            arg,
            span: self.span_from(start),
        })
    }

    /// Consume an identifier that must equal `word` (a contextual keyword).
    fn expect_contextual(&mut self, word: &str, context: &str) -> PResult<()> {
        if self.peek_text_is(word) {
            self.bump();
            Ok(())
        } else {
            let found = self
                .peek_kind()
                .map_or_else(|| "end of input".to_string(), |k| k.describe().to_string());
            Err(self.err(
                format!("expected `{word}` {context}, found {found}"),
                format!("expected `{word}`"),
            ))
        }
    }

    fn parse_enum(&mut self) -> PResult<EnumDecl> {
        let start = self.cur_start();
        self.expect(Token::KwEnum, "to begin an enum")?;
        let name = self.expect_ident("for the enum name")?;
        self.expect(Token::LBrace, "to open the enum body")?;
        let mut variants = Vec::new();
        while !self.check(Token::RBrace) && !self.at_end() {
            variants.push(self.expect_ident("for an enum variant")?);
            self.eat_comma();
        }
        self.expect(Token::RBrace, "to close the enum body")?;
        Ok(EnumDecl {
            name,
            variants,
            span: self.span_from(start),
        })
    }

    fn parse_field(&mut self) -> PResult<FieldDecl> {
        let start = self.cur_start();
        let name = self.expect_ident_like("for the field name")?;
        self.expect(Token::Colon, "after the field name")?;
        let ty = self.parse_type()?;

        let derived_from = if self.check(Token::KwDerived) {
            self.bump();
            self.expect(Token::KwFrom, "after `derived`").map_err(|e| {
                e.with_help("derived fields look like `swaps: [Swap] derived from pool`")
            })?;
            Some(self.expect_ident_like("for the back-reference field")?)
        } else {
            None
        };

        Ok(FieldDecl {
            name,
            ty,
            derived_from,
            span: self.span_from(start),
        })
    }

    fn parse_source(&mut self) -> PResult<SourceDecl> {
        let start = self.cur_start();
        self.expect(Token::KwSource, "to begin a source")?;
        let name = self.expect_ident("for the source name")?;
        let settings = self.parse_settings_block()?;
        Ok(SourceDecl {
            name,
            settings,
            span: self.span_from(start),
        })
    }

    /// `graft { base: "Qm…", block: 113581821 }`
    fn parse_graft(&mut self) -> PResult<GraftDecl> {
        let start = self.cur_start();
        self.expect(Token::KwGraft, "to begin a graft")?;
        let settings = self.parse_settings_block()?;
        Ok(GraftDecl {
            settings,
            span: self.span_from(start),
        })
    }

    fn parse_template(&mut self) -> PResult<TemplateDecl> {
        let start = self.cur_start();
        self.expect(Token::KwTemplate, "to begin a template")?;
        let name = self.expect_ident("for the template name")?;
        let settings = self.parse_settings_block()?;
        Ok(TemplateDecl {
            name,
            settings,
            span: self.span_from(start),
        })
    }

    fn parse_settings_block(&mut self) -> PResult<Vec<Setting>> {
        self.expect(Token::LBrace, "to open the block")?;
        let mut settings = Vec::new();
        while !self.check(Token::RBrace) && !self.at_end() {
            let start = self.cur_start();
            // Keys may be an identifier or a keyword (e.g. `abi:`, `from:`).
            let key = self.expect_ident_like("for the setting key")?;
            self.expect(Token::Colon, "after the setting key")?;
            let value = self.parse_expr()?;
            settings.push(Setting {
                key,
                value,
                span: self.span_from(start),
            });
            self.eat_comma();
        }
        self.expect(Token::RBrace, "to close the block")?;
        Ok(settings)
    }

    fn parse_handler(&mut self) -> PResult<HandlerDecl> {
        let start = self.cur_start();
        self.expect(Token::KwHandler, "to begin a handler")?;

        // The trigger kind is selected by a contextual keyword after `handler`:
        //   `handler on Source.Event(event)`        — event handler
        //   `handler call Source.function(call)`    — call handler
        //   `handler block Source(block) [filter]`  — block handler
        match self.peek_kind() {
            Some(Token::KwOn) => {
                self.bump();
                let source = self.expect_ident("for the source name")?;
                self.expect(Token::Dot, "between the source and event")?;
                let event = self.expect_ident("for the event name")?;
                let param = self.parse_handler_param()?;
                let body = self.parse_block()?;
                Ok(HandlerDecl {
                    kind: HandlerKind::Event,
                    source,
                    event,
                    param,
                    body,
                    span: self.span_from(start),
                })
            }
            Some(Token::Ident) if self.peek_text_is("call") => {
                self.bump();
                let source = self.expect_ident("for the source name")?;
                self.expect(Token::Dot, "between the source and function")?;
                let event = self.expect_ident("for the function name")?;
                let param = self.parse_handler_param()?;
                let body = self.parse_block()?;
                Ok(HandlerDecl {
                    kind: HandlerKind::Call,
                    source,
                    event,
                    param,
                    body,
                    span: self.span_from(start),
                })
            }
            Some(Token::Ident) if self.peek_text_is("file") => {
                self.bump();
                let source = self.expect_ident("for the file template name")?;
                let param = self.parse_handler_param()?;
                let body = self.parse_block()?;
                Ok(HandlerDecl {
                    kind: HandlerKind::File,
                    event: source.clone(),
                    source,
                    param,
                    body,
                    span: self.span_from(start),
                })
            }
            Some(Token::Ident) if self.peek_text_is("block") => {
                self.bump();
                let source = self.expect_ident("for the source name")?;
                let param = self.parse_handler_param()?;
                let filter = self.parse_block_filter()?;
                let body = self.parse_block()?;
                Ok(HandlerDecl {
                    kind: HandlerKind::Block(filter),
                    event: source.clone(),
                    source,
                    param,
                    body,
                    span: self.span_from(start),
                })
            }
            _ => Err(self
                .err("expected a handler trigger", "unexpected token")
                .with_help(
                    "handlers look like `handler on Source.Event(event) { … }`, \
                     `handler call Source.fn(call) { … }`, or `handler block Source(block) { … }`",
                )),
        }
    }

    /// Parse the `(param)` binding shared by all handler kinds.
    fn parse_handler_param(&mut self) -> PResult<Ident> {
        self.expect(Token::LParen, "before the handler parameter")?;
        let param = self.expect_ident("for the handler parameter")?;
        self.expect(Token::RParen, "after the handler parameter")?;
        Ok(param)
    }

    /// Parse an optional block-handler filter: `every N` or `once`.
    fn parse_block_filter(&mut self) -> PResult<BlockFilter> {
        if self.peek_text_is("every") {
            self.bump();
            let n = self.expect(Token::IntLit, "for the polling interval")?;
            let raw = self.text(&n).replace('_', "");
            let every = raw.parse::<u64>().map_err(|_| {
                self.err(
                    "polling interval must be a positive integer",
                    "invalid interval",
                )
            })?;
            Ok(BlockFilter::Polling(every))
        } else if self.peek_text_is("once") {
            self.bump();
            Ok(BlockFilter::Once)
        } else {
            Ok(BlockFilter::Every)
        }
    }

    /// Whether the current token is an identifier with the given text.
    fn peek_text_is(&self, text: &str) -> bool {
        self.peek()
            .is_some_and(|sp| sp.token == Token::Ident && self.text(sp) == text)
    }

    fn parse_fn(&mut self, is_pub: bool) -> PResult<FnDecl> {
        let start = self.cur_start();
        self.expect(Token::KwFn, "to begin a function")?;
        let name = self.expect_ident("for the function name")?;
        self.expect(Token::LParen, "before the parameter list")?;
        let mut params = Vec::new();
        while !self.check(Token::RParen) && !self.at_end() {
            let pstart = self.cur_start();
            let pname = self.expect_ident("for the parameter name")?;
            self.expect(Token::Colon, "after the parameter name")?;
            let pty = self.parse_type()?;
            params.push(Param {
                name: pname,
                ty: pty,
                span: self.span_from(pstart),
            });
            if !self.check(Token::RParen) {
                self.expect(Token::Comma, "between parameters")?;
            }
        }
        self.expect(Token::RParen, "to close the parameter list")?;

        let ret = if self.check(Token::ThinArrow) {
            self.bump();
            Some(self.parse_type()?)
        } else {
            None
        };

        let body = self.parse_block()?;
        Ok(FnDecl {
            name,
            is_pub,
            params,
            ret,
            body,
            span: self.span_from(start),
        })
    }

    fn parse_test(&mut self) -> PResult<TestDecl> {
        let start = self.cur_start();
        self.expect(Token::KwTest, "to begin a test")?;
        let str_tok = self.expect(Token::StringLit, "for the test description")?;
        let name = unescape_string(self.text(&str_tok));
        let body = self.parse_block()?;
        Ok(TestDecl {
            name,
            body,
            span: self.span_from(start),
        })
    }

    // ---- types ----

    fn parse_type(&mut self) -> PResult<TypeExpr> {
        let start = self.cur_start();
        if self.check(Token::LBracket) {
            self.bump();
            let elem = self.parse_type()?;
            self.expect(Token::RBracket, "to close a list type")?;
            return Ok(TypeExpr::List {
                elem: Box::new(elem),
                span: self.span_from(start),
            });
        }

        let mut segments = vec![self.expect_ident("for a type name")?];
        while self.check(Token::PathSep) {
            self.bump();
            segments.push(self.expect_ident("in a type path")?);
        }
        let mut ty = TypeExpr::Path {
            segments,
            span: self.span_from(start),
        };

        if self.check(Token::Lt) {
            self.bump();
            let mut args = Vec::new();
            while !self.check(Token::Gt) && !self.at_end() {
                args.push(self.parse_type()?);
                if !self.check(Token::Gt) {
                    self.expect(Token::Comma, "between type arguments")?;
                }
            }
            self.expect(Token::Gt, "to close type arguments")?;
            ty = TypeExpr::Generic {
                base: Box::new(ty),
                args,
                span: self.span_from(start),
            };
        }

        Ok(ty)
    }

    // ---- blocks & statements ----

    fn parse_block(&mut self) -> PResult<Block> {
        let start = self.cur_start();
        self.expect(Token::LBrace, "to open a block")?;
        let mut stmts = Vec::new();
        while !self.check(Token::RBrace) && !self.at_end() {
            let before = self.pos;
            stmts.push(self.parse_stmt()?);
            self.eat_semi();
            if self.pos == before {
                self.bump();
            }
        }
        self.expect(Token::RBrace, "to close a block")?;
        Ok(Block {
            stmts,
            span: self.span_from(start),
        })
    }

    fn parse_stmt(&mut self) -> PResult<Stmt> {
        let start = self.cur_start();
        match self.peek_kind() {
            Some(Token::KwIf) => self.parse_if(start),
            Some(Token::KwWhile) => self.parse_while(start),
            Some(Token::KwFor) => self.parse_for(start),
            Some(Token::KwLet) => {
                self.bump();
                let name = self.expect_ident("after `let`")?;
                let ty = if self.check(Token::Colon) {
                    self.bump();
                    Some(self.parse_type()?)
                } else {
                    None
                };
                self.expect(Token::Eq, "in a `let` binding")?;
                let value = self.parse_expr()?;
                Ok(Stmt::Let {
                    name,
                    ty,
                    value,
                    span: self.span_from(start),
                })
            }
            Some(Token::KwReturn) => {
                self.bump();
                let value = if self.check(Token::Semi) || self.check(Token::RBrace) {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                Ok(Stmt::Return {
                    value,
                    span: self.span_from(start),
                })
            }
            _ => {
                let expr = self.parse_expr()?;
                if self.check(Token::Eq) {
                    self.bump();
                    let value = self.parse_expr()?;
                    Ok(Stmt::Assign {
                        target: expr,
                        value,
                        span: self.span_from(start),
                    })
                } else {
                    Ok(Stmt::Expr(expr))
                }
            }
        }
    }

    fn parse_if(&mut self, start: usize) -> PResult<Stmt> {
        self.expect(Token::KwIf, "to begin an `if`")?;
        let cond = self.parse_expr()?;
        let then_block = self.parse_block()?;

        let mut else_ifs = Vec::new();
        let mut else_block = None;
        while self.check(Token::KwElse) {
            self.bump();
            if self.check(Token::KwIf) {
                self.bump();
                let c = self.parse_expr()?;
                let b = self.parse_block()?;
                else_ifs.push((c, b));
            } else {
                else_block = Some(self.parse_block()?);
                break;
            }
        }

        Ok(Stmt::If {
            cond,
            then_block,
            else_ifs,
            else_block,
            span: self.span_from(start),
        })
    }

    fn parse_while(&mut self, start: usize) -> PResult<Stmt> {
        self.expect(Token::KwWhile, "to begin a `while`")?;
        let cond = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt::While {
            cond,
            body,
            span: self.span_from(start),
        })
    }

    fn parse_for(&mut self, start: usize) -> PResult<Stmt> {
        self.expect(Token::KwFor, "to begin a `for`")?;
        let var = self.expect_ident("for the loop variable")?;
        self.expect(Token::KwIn, "after the loop variable")
            .map_err(|e| {
                e.with_help("loops look like `for i in 0..n { … }` or `for x in items { … }`")
            })?;
        let first = self.parse_expr()?;
        let iter = if self.check(Token::DotDot) {
            self.bump();
            let end = self.parse_expr()?;
            ForIter::Range { start: first, end }
        } else {
            ForIter::Each(first)
        };
        let body = self.parse_block()?;
        Ok(Stmt::For {
            var,
            iter,
            body,
            span: self.span_from(start),
        })
    }

    // ---- expressions (precedence climbing) ----

    fn parse_expr(&mut self) -> PResult<Expr> {
        self.parse_binary(0)
    }

    fn parse_binary(&mut self, min_bp: u8) -> PResult<Expr> {
        let mut lhs = self.parse_unary()?;

        while let Some(kind) = self.peek_kind() {
            let Some((op, bp)) = binop_of(kind) else {
                break;
            };
            if bp < min_bp {
                break;
            }
            self.bump();
            // Left-associative: parse the rhs with a higher minimum.
            let rhs = self.parse_binary(bp + 1)?;
            let span = lhs.span().merge(rhs.span());
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span,
            };
        }

        Ok(lhs)
    }

    fn parse_unary(&mut self) -> PResult<Expr> {
        let start = self.cur_start();
        match self.peek_kind() {
            Some(Token::Bang) => {
                self.bump();
                let expr = self.parse_unary()?;
                Ok(Expr::Unary {
                    op: UnOp::Not,
                    expr: Box::new(expr),
                    span: self.span_from(start),
                })
            }
            Some(Token::Minus) => {
                self.bump();
                let expr = self.parse_unary()?;
                Ok(Expr::Unary {
                    op: UnOp::Neg,
                    expr: Box::new(expr),
                    span: self.span_from(start),
                })
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> PResult<Expr> {
        let start = self.cur_start();
        let mut expr = self.parse_primary()?;

        loop {
            match self.peek_kind() {
                Some(Token::Dot) => {
                    self.bump();
                    let field = self.expect_ident_like("after `.`")?;
                    expr = Expr::Field {
                        base: Box::new(expr),
                        field,
                        span: self.span_from(start),
                    };
                }
                Some(Token::LParen) => {
                    self.bump();
                    let mut args = Vec::new();
                    while !self.check(Token::RParen) && !self.at_end() {
                        args.push(self.parse_expr()?);
                        if !self.check(Token::RParen) {
                            self.expect(Token::Comma, "between call arguments")?;
                        }
                    }
                    self.expect(Token::RParen, "to close a call")?;
                    expr = Expr::Call {
                        callee: Box::new(expr),
                        args,
                        span: self.span_from(start),
                    };
                }
                Some(Token::LBracket) => {
                    self.bump();
                    let index = self.parse_expr()?;
                    self.expect(Token::RBracket, "to close an index")?;
                    expr = Expr::Index {
                        base: Box::new(expr),
                        index: Box::new(index),
                        span: self.span_from(start),
                    };
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    fn parse_primary(&mut self) -> PResult<Expr> {
        let start = self.cur_start();
        match self.peek_kind() {
            Some(Token::IntLit) => {
                let sp = self.bump().unwrap().clone();
                Ok(Expr::Int {
                    raw: self.text(&sp).replace('_', ""),
                    span: self.span(sp.start, sp.end),
                })
            }
            Some(Token::HexLit) => {
                let sp = self.bump().unwrap().clone();
                Ok(Expr::Hex {
                    raw: self.text(&sp).to_string(),
                    span: self.span(sp.start, sp.end),
                })
            }
            Some(Token::DecimalLit) => {
                let sp = self.bump().unwrap().clone();
                Ok(Expr::Decimal {
                    raw: self.text(&sp).replace('_', ""),
                    span: self.span(sp.start, sp.end),
                })
            }
            Some(Token::StringLit) => {
                let sp = self.bump().unwrap().clone();
                Ok(Expr::Str {
                    value: unescape_string(self.text(&sp)),
                    span: self.span(sp.start, sp.end),
                })
            }
            Some(Token::KwTrue) => {
                let sp = self.bump().unwrap().clone();
                Ok(Expr::Bool {
                    value: true,
                    span: self.span(sp.start, sp.end),
                })
            }
            Some(Token::KwFalse) => {
                let sp = self.bump().unwrap().clone();
                Ok(Expr::Bool {
                    value: false,
                    span: self.span(sp.start, sp.end),
                })
            }
            Some(Token::KwMatch) => self.parse_match(),
            Some(Token::LParen) => {
                self.bump();
                let inner = self.parse_expr()?;
                self.expect(Token::RParen, "to close a parenthesised expression")?;
                Ok(inner)
            }
            Some(Token::LBrace) => self.parse_record(),
            Some(Token::LBracket) => {
                self.bump();
                let mut elems = Vec::new();
                while !self.check(Token::RBracket) && !self.at_end() {
                    elems.push(self.parse_expr()?);
                    if !self.check(Token::RBracket) {
                        self.expect(Token::Comma, "between array elements")?;
                    }
                }
                self.expect(Token::RBracket, "to close an array literal")?;
                Ok(Expr::Array {
                    elems,
                    span: self.span_from(start),
                })
            }
            Some(Token::Ident) => {
                let mut segments = vec![{
                    let sp = self.bump().unwrap().clone();
                    Ident::new(self.text(&sp).to_string(), self.span(sp.start, sp.end))
                }];
                while self.check(Token::PathSep) {
                    self.bump();
                    segments.push(self.expect_ident("in a path")?);
                }
                Ok(Expr::Path {
                    segments,
                    span: self.span_from(start),
                })
            }
            _ => Err(self.err("expected an expression", "expected an expression")),
        }
    }

    fn parse_record(&mut self) -> PResult<Expr> {
        let start = self.cur_start();
        self.expect(Token::LBrace, "to open a record literal")?;
        let mut fields = Vec::new();
        while !self.check(Token::RBrace) && !self.at_end() {
            let key = self.expect_ident_like("for a record field")?;
            self.expect(Token::Colon, "after a record field name")?;
            let value = self.parse_expr()?;
            fields.push((key, value));
            if !self.check(Token::RBrace) {
                self.expect(Token::Comma, "between record fields")?;
            }
        }
        self.expect(Token::RBrace, "to close a record literal")?;
        Ok(Expr::Record {
            fields,
            span: self.span_from(start),
        })
    }

    fn parse_match(&mut self) -> PResult<Expr> {
        let start = self.cur_start();
        self.expect(Token::KwMatch, "to begin a match")?;
        let scrutinee = self.parse_expr()?;
        self.expect(Token::LBrace, "to open the match arms")?;
        let mut arms = Vec::new();
        while !self.check(Token::RBrace) && !self.at_end() {
            let arm_start = self.cur_start();
            let pattern = self.parse_pattern()?;
            self.expect(Token::FatArrow, "after a match pattern")?;
            let body = self.parse_block()?;
            arms.push(MatchArm {
                pattern,
                body,
                span: self.span_from(arm_start),
            });
            self.eat_comma();
        }
        self.expect(Token::RBrace, "to close the match")?;
        Ok(Expr::Match {
            scrutinee: Box::new(scrutinee),
            arms,
            span: self.span_from(start),
        })
    }

    fn parse_pattern(&mut self) -> PResult<Pattern> {
        let start = self.cur_start();
        let name = self.expect_ident("for a match pattern")?;

        if name.name == "_" {
            return Ok(Pattern::Wildcard {
                span: self.span_from(start),
            });
        }

        if self.check(Token::LParen) {
            self.bump();
            let mut bindings = Vec::new();
            while !self.check(Token::RParen) && !self.at_end() {
                bindings.push(self.expect_ident("for a pattern binding")?);
                if !self.check(Token::RParen) {
                    self.expect(Token::Comma, "between pattern bindings")?;
                }
            }
            self.expect(Token::RParen, "to close a constructor pattern")?;
            return Ok(Pattern::Ctor {
                name,
                bindings,
                span: self.span_from(start),
            });
        }

        // Uppercase-initial bare name => nullary constructor (e.g. `None`).
        let is_ctor = name.name.chars().next().is_some_and(char::is_uppercase);
        if is_ctor {
            Ok(Pattern::Ctor {
                name,
                bindings: Vec::new(),
                span: self.span_from(start),
            })
        } else {
            Ok(Pattern::Binding {
                name,
                span: self.span_from(start),
            })
        }
    }

    // ---- trivia ----

    fn eat_semi(&mut self) {
        if self.check(Token::Semi) {
            self.bump();
        }
    }

    fn eat_comma(&mut self) {
        if self.check(Token::Comma) {
            self.bump();
        }
    }
}

/// Whether a token is one of Redstart's keywords.
fn is_keyword(kind: Token) -> bool {
    matches!(
        kind,
        Token::KwAbi
            | Token::KwFrom
            | Token::KwEntity
            | Token::KwEnum
            | Token::KwInterface
            | Token::KwAggregation
            | Token::KwSource
            | Token::KwTemplate
            | Token::KwHandler
            | Token::KwOn
            | Token::KwDerived
            | Token::KwMatch
            | Token::KwLet
            | Token::KwReturn
            | Token::KwIf
            | Token::KwElse
            | Token::KwWhile
            | Token::KwFor
            | Token::KwIn
            | Token::KwFn
            | Token::KwMod
            | Token::KwUse
            | Token::KwTest
            | Token::KwTrue
            | Token::KwFalse
    )
}

/// Binding power and operator for a binary token, if it is one.
fn binop_of(kind: Token) -> Option<(BinOp, u8)> {
    Some(match kind {
        Token::OrOr => (BinOp::Or, 1),
        Token::AndAnd => (BinOp::And, 2),
        Token::EqEq => (BinOp::Eq, 3),
        Token::NotEq => (BinOp::Ne, 3),
        Token::Lt => (BinOp::Lt, 4),
        Token::Le => (BinOp::Le, 4),
        Token::Gt => (BinOp::Gt, 4),
        Token::Ge => (BinOp::Ge, 4),
        Token::Plus => (BinOp::Add, 5),
        Token::Minus => (BinOp::Sub, 5),
        Token::Star => (BinOp::Mul, 6),
        Token::Slash => (BinOp::Div, 6),
        Token::Percent => (BinOp::Rem, 6),
        _ => return None,
    })
}

/// Strip the surrounding quotes from a string literal and process escapes.
fn unescape_string(raw: &str) -> String {
    let inner = raw
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(raw);
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;

    fn parse_ok(src: &str) -> Program {
        let lexed = lex(src).expect("lex");
        let (program, errors) = parse(lexed.tokens(), Arc::from(src));
        assert!(errors.is_empty(), "unexpected parse errors: {errors:#?}");
        program
    }

    #[test]
    fn parses_abi_import() {
        let p = parse_ok(r#"abi ERC20 from "./abis/ERC20.json""#);
        assert_eq!(p.abis.len(), 1);
        assert_eq!(p.abis[0].name.name, "ERC20");
        assert_eq!(p.abis[0].path, "./abis/ERC20.json");
    }

    #[test]
    fn parses_entity_with_fields_and_derived() {
        let p = parse_ok(
            r#"
entity Pool {
  id: Id<Bytes>
  liquidity: BigInt
  totalVolumeUSD: BigDecimal
  swaps: [Swap] derived from pool
}
"#,
        );
        assert_eq!(p.entities.len(), 1);
        let e = &p.entities[0];
        assert_eq!(e.fields.len(), 4);
        assert_eq!(e.fields[0].name.name, "id");
        assert!(e.fields[3].derived_from.is_some());
        assert_eq!(e.fields[3].derived_from.as_ref().unwrap().name, "pool");
    }

    #[test]
    fn parses_source_block() {
        let p = parse_ok(
            r#"
source PoolContract {
  abi: UniswapV3Pool
  network: mainnet
  address: 0x8ad599c3A0ff1De082011EFDDc58f1908eb6e6D8
  startBlock: 12369621
}
"#,
        );
        assert_eq!(p.sources.len(), 1);
        let s = &p.sources[0];
        assert_eq!(s.settings.len(), 4);
        assert_eq!(s.settings[0].key.name, "abi");
        assert_eq!(s.settings[3].key.name, "startBlock");
    }

    #[test]
    fn parses_handler_with_body() {
        let p = parse_ok(
            r#"
handler on PoolContract.Swap(event) {
  let pool = Pool.loadOrCreate(event.address, {
    liquidity: BigInt.zero,
    totalVolumeUSD: BigDecimal.zero,
  })
  let amountUSD = event.params.amount0.toDecimal().abs() * token0PriceUSD
  pool.liquidity = event.params.liquidity
  pool.totalVolumeUSD = pool.totalVolumeUSD + amountUSD
}
"#,
        );
        assert_eq!(p.handlers.len(), 1);
        let h = &p.handlers[0];
        assert_eq!(h.source.name, "PoolContract");
        assert_eq!(h.event.name, "Swap");
        assert_eq!(h.param.name, "event");
        assert_eq!(h.body.stmts.len(), 4);
    }

    #[test]
    fn parses_match_expression() {
        let p = parse_ok(
            r#"
handler on C.E(event) {
  let v = match C.balanceOf(event.address) {
    Ok(balance) => { return balance }
    Err(e) => { return BigInt.zero }
  }
}
"#,
        );
        let h = &p.handlers[0];
        if let Stmt::Let {
            value: Expr::Match { arms, .. },
            ..
        } = &h.body.stmts[0]
        {
            assert_eq!(arms.len(), 2);
        } else {
            panic!("expected a match expression");
        }
    }

    #[test]
    fn parses_multifile_decls() {
        let p = parse_ok("mod tokens; pub mod helpers; use tokens::Token;");
        assert_eq!(p.mods.len(), 2);
        assert!(p.mods[1].is_pub);
        assert_eq!(p.uses.len(), 1);
        assert_eq!(p.uses[0].path.len(), 2);
    }

    #[test]
    fn keywords_usable_as_field_and_record_names() {
        // `from` is a keyword but also a ubiquitous event field/param name.
        let p = parse_ok(
            r#"
entity Transfer { id: Id<Bytes> from: Account }
handler on C.Transfer(event) {
  let t = Transfer.create(event.id, { from: event.params.from })
}
"#,
        );
        assert_eq!(p.entities[0].fields[1].name.name, "from");
        assert_eq!(p.handlers.len(), 1);
    }

    #[test]
    fn precedence_is_correct() {
        // a + b * c  =>  a + (b * c)
        let p = parse_ok("fn f() { let x = a + b * c }");
        let body = &p.functions[0].body;
        let Stmt::Let {
            value: Expr::Binary { op, rhs, .. },
            ..
        } = &body.stmts[0]
        else {
            panic!("expected binary");
        };
        assert_eq!(*op, BinOp::Add);
        assert!(matches!(rhs.as_ref(), Expr::Binary { op: BinOp::Mul, .. }));
    }

    #[test]
    fn parses_aggregation() {
        let p = parse_ok(
            "aggregation Stats over Data every [hour, day] { total: BigDecimal = sum(price) n: Int8 = count() }",
        );
        assert_eq!(p.aggregations.len(), 1);
        let a = &p.aggregations[0];
        assert_eq!(a.name.name, "Stats");
        assert_eq!(a.source.name, "Data");
        assert_eq!(a.intervals.len(), 2);
        assert_eq!(a.fields.len(), 2);
        assert_eq!(a.fields[0].func.name, "sum");
        assert_eq!(a.fields[0].arg.as_ref().unwrap().name, "price");
        assert_eq!(a.fields[1].func.name, "count");
        assert!(a.fields[1].arg.is_none());
    }

    #[test]
    fn parses_interface_and_implements() {
        let p = parse_ok(
            "interface Token { id: Id<Bytes> symbol: String } \
             entity FT implements Token & Other immutable { id: Id<Bytes> symbol: String }",
        );
        assert_eq!(p.interfaces.len(), 1);
        assert_eq!(p.interfaces[0].name.name, "Token");
        assert_eq!(p.interfaces[0].fields.len(), 2);
        let e = &p.entities[0];
        assert_eq!(e.implements.len(), 2);
        assert_eq!(e.implements[0].name, "Token");
        assert_eq!(e.implements[1].name, "Other");
        assert!(e.modifiers.iter().any(|m| m.name == "immutable"));
    }

    #[test]
    fn parses_enum_declaration() {
        let p = parse_ok("enum Status { Active, Closed, Pending }");
        assert_eq!(p.enums.len(), 1);
        assert_eq!(p.enums[0].name.name, "Status");
        assert_eq!(p.enums[0].variants.len(), 3);
        assert_eq!(p.enums[0].variants[1].name, "Closed");
    }

    #[test]
    fn parses_event_call_and_block_handlers() {
        let p = parse_ok(
            r#"
handler on Token.Transfer(event) { }
handler call Token.transfer(call) { }
handler block Token(block) every 50 { }
handler block Token(b2) once { }
handler block Token(b3) { }
"#,
        );
        assert_eq!(p.handlers.len(), 5);
        assert_eq!(p.handlers[0].kind, HandlerKind::Event);
        assert_eq!(p.handlers[1].kind, HandlerKind::Call);
        assert_eq!(p.handlers[1].event.name, "transfer");
        assert_eq!(
            p.handlers[2].kind,
            HandlerKind::Block(BlockFilter::Polling(50))
        );
        assert_eq!(p.handlers[3].kind, HandlerKind::Block(BlockFilter::Once));
        assert_eq!(p.handlers[4].kind, HandlerKind::Block(BlockFilter::Every));
    }

    #[test]
    fn parses_file_handler() {
        let p = parse_ok("handler file TokenMetadata(content) { let v = json.fromBytes(content) }");
        assert_eq!(p.handlers.len(), 1);
        assert_eq!(p.handlers[0].kind, HandlerKind::File);
        assert_eq!(p.handlers[0].source.name, "TokenMetadata");
        assert_eq!(p.handlers[0].param.name, "content");
    }

    #[test]
    fn parses_control_flow_and_arrays() {
        let p = parse_ok(
            r#"
handler on C.E(event) {
  let total = 0
  if event.params.value > 0 {
    total = 1
  } else if event.params.value < 0 {
    total = 2
  } else {
    total = 3
  }
  for i in 0..10 {
    total = total + i
  }
  for x in [1, 2, 3] {
    total = total + x[0]
  }
  while total < 100 {
    total = total + 1
  }
}
"#,
        );
        let body = &p.handlers[0].body;
        assert!(matches!(body.stmts[1], Stmt::If { .. }));
        let Stmt::If {
            else_ifs,
            else_block,
            ..
        } = &body.stmts[1]
        else {
            panic!("expected if");
        };
        assert_eq!(else_ifs.len(), 1);
        assert!(else_block.is_some());
        assert!(matches!(
            body.stmts[2],
            Stmt::For {
                iter: ForIter::Range { .. },
                ..
            }
        ));
        assert!(matches!(
            body.stmts[3],
            Stmt::For {
                iter: ForIter::Each(_),
                ..
            }
        ));
        assert!(matches!(body.stmts[4], Stmt::While { .. }));
    }

    #[test]
    fn reports_error_but_recovers() {
        let src = "entity { } entity Good { id: Id<Bytes> }";
        let lexed = lex(src).expect("lex");
        let (program, errors) = parse(lexed.tokens(), Arc::from(src));
        assert!(!errors.is_empty());
        // Recovery should still find the second, well-formed entity.
        assert!(program.entities.iter().any(|e| e.name.name == "Good"));
    }
}
