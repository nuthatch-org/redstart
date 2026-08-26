//! Abstract syntax tree for Redstart.
//!
//! A single [`Program`] holds every declaration kind — ABIs, entities, sources,
//! templates, handlers, functions — that today live in three drift-prone files
//! (`schema.graphql`, `subgraph.yaml`, `src/mapping.ts`). Unifying them here is
//! what makes manifest/schema/handler drift a *compile-time* impossibility.

use crate::span::{Ident, Span};

/// One parsed Redstart source file.
#[derive(Debug, Clone, Default)]
pub struct Program {
    /// `mod foo;` declarations (multi-file module tree).
    pub mods: Vec<ModDecl>,
    /// `use a::b::c;` imports.
    pub uses: Vec<UseDecl>,
    /// `abi Name from "path.json"` declarations.
    pub abis: Vec<AbiDecl>,
    /// `entity Name { ... }` declarations.
    pub entities: Vec<EntityDecl>,
    /// `enum Name { A, B }` declarations.
    pub enums: Vec<EnumDecl>,
    /// `interface Name { ... }` declarations.
    pub interfaces: Vec<InterfaceDecl>,
    /// `aggregation Name over Source every [..] { ... }` declarations.
    pub aggregations: Vec<AggregationDecl>,
    /// `source Name { ... }` data sources.
    pub sources: Vec<SourceDecl>,
    /// `template Name { ... }` dynamic data sources.
    pub templates: Vec<TemplateDecl>,
    /// `graft { base: "Qm…", block: N }` — inherit a base deployment's store.
    pub graft: Option<GraftDecl>,
    /// `handler on Source.Event(p) { ... }` event handlers.
    pub handlers: Vec<HandlerDecl>,
    /// Free `fn` declarations (helpers).
    pub functions: Vec<FnDecl>,
    /// `test "name" { ... }` blocks.
    pub tests: Vec<TestDecl>,
}

/// `mod name;`
#[derive(Debug, Clone)]
pub struct ModDecl {
    /// The child module name.
    pub name: Ident,
    /// Whether it is publicly re-exported (`pub mod`).
    pub is_pub: bool,
    /// Span of the whole declaration.
    pub span: Span,
}

/// `use a::b::c;`
#[derive(Debug, Clone)]
pub struct UseDecl {
    /// The dotted/`::`-separated path segments.
    pub path: Vec<Ident>,
    /// Span of the whole declaration.
    pub span: Span,
}

/// `abi UniswapV3Pool from "./abis/UniswapV3Pool.json"`
#[derive(Debug, Clone)]
pub struct AbiDecl {
    /// The in-language name bound to the ABI.
    pub name: Ident,
    /// The path to the ABI JSON file, relative to the source file.
    pub path: String,
    /// Span of the whole declaration.
    pub span: Span,
}

/// `entity Name [implements A & B] [modifiers] { fields }`
#[derive(Debug, Clone)]
pub struct EntityDecl {
    /// The entity name.
    pub name: Ident,
    /// Interfaces this entity implements (`implements A & B`).
    pub implements: Vec<Ident>,
    /// Modifiers such as `immutable`, `timeseries`.
    pub modifiers: Vec<Ident>,
    /// The declared fields.
    pub fields: Vec<FieldDecl>,
    /// Span of the whole declaration.
    pub span: Span,
}

/// `interface Name { fields }` — a GraphQL interface entities can implement.
#[derive(Debug, Clone)]
pub struct InterfaceDecl {
    /// The interface name.
    pub name: Ident,
    /// The declared fields.
    pub fields: Vec<FieldDecl>,
    /// Span of the whole declaration.
    pub span: Span,
}

/// `aggregation Name over Source every [hour, day] { field = fn(arg), … }`.
#[derive(Debug, Clone)]
pub struct AggregationDecl {
    /// The aggregation type name.
    pub name: Ident,
    /// The source timeseries entity.
    pub source: Ident,
    /// Intervals: `hour`, `day`.
    pub intervals: Vec<Ident>,
    /// The aggregated fields.
    pub fields: Vec<AggregateField>,
    /// Span of the whole declaration.
    pub span: Span,
}

/// One `name: Type = fn(arg)` field inside an [`AggregationDecl`].
#[derive(Debug, Clone)]
pub struct AggregateField {
    /// The field name.
    pub name: Ident,
    /// The field type.
    pub ty: TypeExpr,
    /// The aggregation function (`sum`, `count`, `min`, `max`, `first`, `last`).
    pub func: Ident,
    /// The source attribute the function reduces over (absent for `count()`).
    pub arg: Option<Ident>,
    /// Span of the whole field.
    pub span: Span,
}

/// `enum Name { Variant, Variant }` — a GraphQL enum type.
#[derive(Debug, Clone)]
pub struct EnumDecl {
    /// The enum name.
    pub name: Ident,
    /// The variant names, in declared order.
    pub variants: Vec<Ident>,
    /// Span of the whole declaration.
    pub span: Span,
}

/// A single field inside an `entity`.
#[derive(Debug, Clone)]
pub struct FieldDecl {
    /// The field name.
    pub name: Ident,
    /// The declared type.
    pub ty: TypeExpr,
    /// If `Some`, this is a `derived from <field>` virtual field. The compiler
    /// statically forbids assigning to it.
    pub derived_from: Option<Ident>,
    /// Span of the whole field.
    pub span: Span,
}

/// `source Name { key: value, ... }`
#[derive(Debug, Clone)]
pub struct SourceDecl {
    /// The data source name.
    pub name: Ident,
    /// Settings such as `abi`, `network`, `address`, `startBlock`.
    pub settings: Vec<Setting>,
    /// Span of the whole declaration.
    pub span: Span,
}

/// `graft { base: "Qm…", block: N }` — start from another deployment's entity
/// store instead of indexing from scratch. Subgraph-level, so there is at most
/// one per project, and it makes the manifest declare `features: [grafting]`.
#[derive(Debug, Clone)]
pub struct GraftDecl {
    /// Settings: `base` (the base deployment id) and `block`.
    pub settings: Vec<Setting>,
    /// Span of the whole declaration.
    pub span: Span,
}

/// `template Name { key: value, ... }` — a dynamic data source (no address).
#[derive(Debug, Clone)]
pub struct TemplateDecl {
    /// The template name.
    pub name: Ident,
    /// Settings such as `abi`, `network`.
    pub settings: Vec<Setting>,
    /// Span of the whole declaration.
    pub span: Span,
}

/// A `key: value` pair inside a `source`/`template` block.
#[derive(Debug, Clone)]
pub struct Setting {
    /// The setting key.
    pub key: Ident,
    /// The setting value expression.
    pub value: Expr,
    /// Span of the whole setting.
    pub span: Span,
}

/// `handler on Source.Event(param) { body }` and its call/block variants.
#[derive(Debug, Clone)]
pub struct HandlerDecl {
    /// What kind of trigger this handler responds to.
    pub kind: HandlerKind,
    /// The data source or template name the trigger belongs to.
    pub source: Ident,
    /// The event name (event handler) or function name (call handler). For
    /// block handlers this echoes the source name and is otherwise unused.
    pub event: Ident,
    /// The handler parameter binding (`event` / `call` / `block`).
    pub param: Ident,
    /// The handler body.
    pub body: Block,
    /// Span of the whole declaration.
    pub span: Span,
}

/// The trigger kind of a [`HandlerDecl`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandlerKind {
    /// `handler on Source.Event(event)` — an Ethereum log/event handler.
    Event,
    /// `handler call Source.function(call)` — a function-call handler.
    Call,
    /// `handler block Source(block) [every N | once]` — a block handler.
    Block(BlockFilter),
    /// `handler file Template(content)` — a file/IPFS data-source handler.
    File,
}

/// The block-handler trigger filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockFilter {
    /// Run on every block (no `filter:` in the manifest).
    Every,
    /// `filter: { kind: polling, every: N }`.
    Polling(u64),
    /// `filter: { kind: once }`.
    Once,
}

impl HandlerDecl {
    /// The AssemblyScript export name Redstart derives for this handler.
    #[must_use]
    pub fn fn_name(&self) -> String {
        match self.kind {
            HandlerKind::Event => format!("handle{}", self.event.name),
            HandlerKind::Call => format!("handle{}Call", capitalize(&self.event.name)),
            // The param binding (conventionally `block`) keeps sibling block
            // handlers on one source uniquely named — `handleTokenBlock`.
            HandlerKind::Block(_) => {
                format!("handle{}{}", self.source.name, capitalize(&self.param.name))
            }
            HandlerKind::File => format!("handle{}", capitalize(&self.source.name)),
        }
    }

    /// Whether this is a file/IPFS data-source handler.
    #[must_use]
    pub fn is_file(&self) -> bool {
        matches!(self.kind, HandlerKind::File)
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map_or_else(String::new, |c| {
        c.to_uppercase().collect::<String>() + chars.as_str()
    })
}

/// `fn name(params) -> ret { body }`
#[derive(Debug, Clone)]
pub struct FnDecl {
    /// The function name.
    pub name: Ident,
    /// Whether it is publicly exported.
    pub is_pub: bool,
    /// The parameters.
    pub params: Vec<Param>,
    /// The optional return type.
    pub ret: Option<TypeExpr>,
    /// The function body.
    pub body: Block,
    /// Span of the whole declaration.
    pub span: Span,
}

/// A function parameter.
#[derive(Debug, Clone)]
pub struct Param {
    /// The parameter name.
    pub name: Ident,
    /// The parameter type.
    pub ty: TypeExpr,
    /// Span of the parameter.
    pub span: Span,
}

/// `test "description" { body }`
#[derive(Debug, Clone)]
pub struct TestDecl {
    /// The test description string.
    pub name: String,
    /// The test body.
    pub body: Block,
    /// Span of the whole declaration.
    pub span: Span,
}

/// A type expression.
#[derive(Debug, Clone)]
pub enum TypeExpr {
    /// A named type, possibly module-qualified: `Pool`, `BigInt`, `foo::Bar`.
    Path { segments: Vec<Ident>, span: Span },
    /// A generic application: `Id<Bytes>`, `Option<T>`, `Result<T, E>`.
    Generic {
        base: Box<TypeExpr>,
        args: Vec<TypeExpr>,
        span: Span,
    },
    /// A list type written `[T]`.
    List { elem: Box<TypeExpr>, span: Span },
}

impl TypeExpr {
    /// The span of this type expression.
    #[must_use]
    pub fn span(&self) -> &Span {
        match self {
            TypeExpr::Path { span, .. }
            | TypeExpr::Generic { span, .. }
            | TypeExpr::List { span, .. } => span,
        }
    }

    /// For a simple single-segment path type, its name. Useful for the checker.
    #[must_use]
    pub fn simple_name(&self) -> Option<&str> {
        match self {
            TypeExpr::Path { segments, .. } if segments.len() == 1 => {
                Some(segments[0].name.as_str())
            }
            _ => None,
        }
    }
}

/// A `{ ... }` block of statements.
#[derive(Debug, Clone)]
pub struct Block {
    /// The statements in order.
    pub stmts: Vec<Stmt>,
    /// Span of the whole block.
    pub span: Span,
}

/// A statement.
#[derive(Debug, Clone)]
pub enum Stmt {
    /// `let name [: ty] = value;`
    Let {
        name: Ident,
        ty: Option<TypeExpr>,
        value: Expr,
        span: Span,
    },
    /// `target = value;` — e.g. `pool.liquidity = event.params.liquidity`.
    Assign {
        target: Expr,
        value: Expr,
        span: Span,
    },
    /// `return [value];`
    Return { value: Option<Expr>, span: Span },
    /// `if cond { … } else if cond { … } else { … }`.
    If {
        /// The leading condition.
        cond: Expr,
        /// The `then` block.
        then_block: Block,
        /// Zero or more `else if (cond) { … }` branches, in order.
        else_ifs: Vec<(Expr, Block)>,
        /// The trailing `else { … }`, if any.
        else_block: Option<Block>,
        span: Span,
    },
    /// `while cond { … }`.
    While { cond: Expr, body: Block, span: Span },
    /// `for x in <iter> { … }`.
    For {
        /// The loop binding.
        var: Ident,
        /// What is being iterated.
        iter: ForIter,
        body: Block,
        span: Span,
    },
    /// An expression used as a statement (e.g. a method call).
    Expr(Expr),
}

/// The iterable of a `for` loop.
#[derive(Debug, Clone)]
pub enum ForIter {
    /// A half-open numeric range `start..end`.
    Range { start: Expr, end: Expr },
    /// Each element of a list expression.
    Each(Expr),
}

impl Stmt {
    /// The span of this statement.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Stmt::Let { span, .. }
            | Stmt::Assign { span, .. }
            | Stmt::Return { span, .. }
            | Stmt::If { span, .. }
            | Stmt::While { span, .. }
            | Stmt::For { span, .. } => span.clone(),
            Stmt::Expr(e) => e.span().clone(),
        }
    }
}

/// An expression.
#[derive(Debug, Clone)]
pub enum Expr {
    /// An integer literal, kept as text so `BigInt` literals never overflow.
    Int { raw: String, span: Span },
    /// A `0x...` hex literal (address or bytes).
    Hex { raw: String, span: Span },
    /// A decimal literal, kept as text for `BigDecimal` fidelity.
    Decimal { raw: String, span: Span },
    /// A string literal (already unescaped).
    Str { value: String, span: Span },
    /// A boolean literal.
    Bool { value: bool, span: Span },
    /// A bare identifier, possibly `::`-qualified (`event`, `foo::bar`).
    Path { segments: Vec<Ident>, span: Span },
    /// Field/member access: `base.field`.
    Field {
        base: Box<Expr>,
        field: Ident,
        span: Span,
    },
    /// A call: `callee(args)`. Method calls are `Field` then `Call`.
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },
    /// A record literal: `{ field: value, ... }` (used by `loadOrCreate`/`create`).
    Record {
        fields: Vec<(Ident, Expr)>,
        span: Span,
    },
    /// An array literal: `[a, b, c]`.
    Array { elems: Vec<Expr>, span: Span },
    /// An index access: `base[index]`.
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    /// A binary operation.
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    /// A unary operation (`!x`, `-x`).
    Unary {
        op: UnOp,
        expr: Box<Expr>,
        span: Span,
    },
    /// A `match scrutinee { pattern => body, ... }` expression.
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
        span: Span,
    },
}

impl Expr {
    /// The span of this expression.
    #[must_use]
    pub fn span(&self) -> &Span {
        match self {
            Expr::Int { span, .. }
            | Expr::Hex { span, .. }
            | Expr::Decimal { span, .. }
            | Expr::Str { span, .. }
            | Expr::Bool { span, .. }
            | Expr::Path { span, .. }
            | Expr::Field { span, .. }
            | Expr::Call { span, .. }
            | Expr::Record { span, .. }
            | Expr::Array { span, .. }
            | Expr::Index { span, .. }
            | Expr::Binary { span, .. }
            | Expr::Unary { span, .. }
            | Expr::Match { span, .. } => span,
        }
    }
}

/// One arm of a `match` expression.
#[derive(Debug, Clone)]
pub struct MatchArm {
    /// The pattern to match.
    pub pattern: Pattern,
    /// The arm body.
    pub body: Block,
    /// Span of the whole arm.
    pub span: Span,
}

/// A `match` pattern. Deliberately small for v0.1.
#[derive(Debug, Clone)]
pub enum Pattern {
    /// A constructor pattern with bindings: `Some(x)`, `Ok(v)`, `Err(e)`.
    Ctor {
        name: Ident,
        bindings: Vec<Ident>,
        span: Span,
    },
    /// A wildcard `_`.
    Wildcard { span: Span },
    /// A bare binding identifier.
    Binding { name: Ident, span: Span },
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%`
    Rem,
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `&&`
    And,
    /// `||`
    Or,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    /// `!`
    Not,
    /// `-`
    Neg,
}
