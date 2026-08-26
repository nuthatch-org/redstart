//! Lowering Redstart handler bodies to AssemblyScript.
//!
//! This is the core of the whole bet: emit the AssemblyScript a careful human
//! would write, so the canonical `graph build` toolchain consumes it unmodified
//! (the eject path). A lightweight type environment — entity field types, ABI
//! event-parameter types, and ABI function return types — is enough to make the
//! footgun-prone lowerings correct:
//!
//! - `BigInt`/`BigDecimal` operators (`+ - * /`) lower to `.plus()`/`.minus()`/
//!   `.times()`/`.div()`, never silent native arithmetic.
//! - `loadOrCreate` lowers to the load → null-check → `new` → init dance, so the
//!   nullable-arithmetic miscompile and the forgotten-init crash cannot occur.
//! - Contract calls return `Result`, lowered to graph-ts `try_*` + `.reverted`.
//!   You cannot touch a reverted call's value because you must `match` it first.
//! - `match` on `Result`/`Option` lowers to the corresponding `.reverted` /
//!   null-check `if`/`else`.
//! - Entities created or mutated are auto-saved (dirty-tracked) at the end of the
//!   scope where they were declared — including inside `match` arms.
//!
//! The environment spans *all* modules, so a handler in one `.red` file can
//! reference an entity declared in another — multi-file is first-class here.

use redstart_checker::{resolve_type, sol_to_rty, AbiIndex, EntityInfo, RTy};
use redstart_parser::ast::{
    BinOp, Block, Expr, FnDecl, ForIter, HandlerDecl, HandlerKind, MatchArm, Pattern, Stmt, UnOp,
};
use redstart_parser::Ident;
use std::collections::HashMap;

/// The static environment shared across all handlers.
pub struct Env<'a> {
    /// Entity name -> field info (aggregated across every module).
    pub entities: HashMap<String, EntityInfo>,
    /// Source/template name -> ABI name.
    pub source_abi: HashMap<String, String>,
    /// Declared template names (dynamic data sources).
    pub templates: Vec<String>,
    /// Free-function name -> resolved return type (for typing helper calls).
    pub fn_returns: HashMap<String, RTy>,
    /// ABI access for event-parameter and function-return types.
    pub abis: &'a mut AbiIndex,
}

/// One lexical scope's save bookkeeping. Entities are saved at the end of the
/// scope in which they were *declared*, so a `match`-arm entity is saved inside
/// the arm (where it is in scope), and an outer entity mutated in an arm is
/// saved at the outer scope's end.
#[derive(Default)]
struct Frame {
    /// Entity locals declared in this frame.
    declared: Vec<String>,
    /// Of those, the ones that became dirty (created or mutated).
    dirty: Vec<String>,
}

/// Per-handler mutable scope.
struct Scope {
    /// Local variable -> resolved type (flat; shadowing is rare and tolerated).
    locals: HashMap<String, RTy>,
    /// The handler parameter name (the event/call/block binding).
    event_param: String,
    /// The resolved type of the handler parameter (Event / Call / Block).
    param_ty: RTy,
    /// The current handler's ABI name (for param/function lookup).
    abi: String,
    /// The event name (event handler) or function name (call handler).
    member: String,
    /// Stack of save frames, one per lexical block.
    frames: Vec<Frame>,
    /// Counter for synthetic temporaries.
    tmp: usize,
    /// Warnings raised during lowering.
    warnings: Vec<String>,
}

impl Scope {
    /// Declare an entity local in the current frame.
    fn declare_entity(&mut self, name: &str, entity: String) {
        self.locals.insert(name.to_string(), RTy::Entity(entity));
        if let Some(f) = self.frames.last_mut() {
            f.declared.push(name.to_string());
        }
    }

    /// Declare a non-entity local (no save tracking).
    fn declare_local(&mut self, name: &str, ty: RTy) {
        self.locals.insert(name.to_string(), ty);
    }

    /// Mark an entity local dirty, attributing it to its declaring frame.
    fn mark_dirty(&mut self, name: &str) {
        for f in self.frames.iter_mut().rev() {
            if f.declared.iter().any(|d| d == name) {
                if !f.dirty.iter().any(|d| d == name) {
                    f.dirty.push(name.to_string());
                }
                return;
            }
        }
    }

    fn fresh(&mut self) -> String {
        self.fresh_with("_call")
    }

    fn fresh_with(&mut self, prefix: &str) -> String {
        let n = self.tmp;
        self.tmp += 1;
        format!("{prefix}{n}")
    }
}

/// Lower a single handler to an AssemblyScript function body (statements only,
/// without the surrounding `export function` line). Returns warnings too.
pub fn lower_handler(handler: &HandlerDecl, env: &mut Env) -> (String, Vec<String>) {
    let abi = env
        .source_abi
        .get(&handler.source.name)
        .cloned()
        .unwrap_or_default();

    let param_ty = match handler.kind {
        HandlerKind::Event => RTy::Event,
        HandlerKind::Call => RTy::Call,
        HandlerKind::Block(_) => RTy::Block,
        HandlerKind::File => RTy::Bytes,
    };

    let mut scope = Scope {
        locals: HashMap::new(),
        event_param: handler.param.name.clone(),
        param_ty,
        abi,
        member: handler.event.name.clone(),
        frames: Vec::new(),
        tmp: 0,
        warnings: Vec::new(),
    };

    let mut body = String::new();
    lower_block(&handler.body, env, &mut scope, &mut body, 1);
    (body, scope.warnings)
}

/// Lower a free `fn` to a complete AssemblyScript function (signature + body).
/// Helper functions let real subgraphs stay DRY (`getOrCreateIndexer`, …).
pub fn lower_fn(func: &FnDecl, env: &mut Env) -> (String, Vec<String>) {
    let mut scope = Scope {
        locals: HashMap::new(),
        event_param: String::new(),
        param_ty: RTy::Unknown,
        abi: String::new(),
        member: String::new(),
        frames: Vec::new(),
        tmp: 0,
        warnings: Vec::new(),
    };

    let mut sig_params = Vec::new();
    for p in &func.params {
        let rty = resolve_type(&p.ty, &env.entities);
        sig_params.push(format!("{}: {}", p.name.name, rty_to_as(&rty)));
        scope.declare_local(&p.name.name, rty);
    }
    let ret_as = func.ret.as_ref().map_or_else(
        || "void".to_string(),
        |t| rty_to_as(&resolve_type(t, &env.entities)),
    );

    let mut body = String::new();
    lower_block(&func.body, env, &mut scope, &mut body, 1);

    let keyword = if func.is_pub {
        "export function"
    } else {
        "function"
    };
    let text = format!(
        "{keyword} {}({}): {ret_as} {{\n{body}}}\n\n",
        func.name.name,
        sig_params.join(", ")
    );
    (text, scope.warnings)
}

/// Map a resolved Redstart type to its AssemblyScript spelling.
fn rty_to_as(ty: &RTy) -> String {
    match ty {
        RTy::BigInt => "BigInt".to_string(),
        RTy::BigDecimal => "BigDecimal".to_string(),
        RTy::Bytes => "Bytes".to_string(),
        RTy::Address => "Address".to_string(),
        RTy::String => "string".to_string(),
        RTy::Boolean => "bool".to_string(),
        RTy::Int => "i32".to_string(),
        RTy::Entity(name) => name.clone(),
        RTy::Option(inner) => format!("{} | null", rty_to_as(inner)),
        RTy::List(inner) => format!("Array<{}>", rty_to_as(inner)),
        _ => "void".to_string(),
    }
}

fn indent(level: usize) -> String {
    "  ".repeat(level)
}

/// Lower a block: statements followed by auto-saves for entities declared here.
fn lower_block(block: &Block, env: &mut Env, scope: &mut Scope, out: &mut String, level: usize) {
    scope.frames.push(Frame::default());
    for stmt in &block.stmts {
        lower_stmt(stmt, env, scope, out, level);
    }
    let frame = scope.frames.pop().expect("frame pushed above");
    if !frame.dirty.is_empty() {
        let pad = indent(level);
        out.push('\n');
        for name in &frame.dirty {
            out.push_str(&format!("{pad}{name}.save()\n"));
        }
    }
}

/// Emit `.save()` for every entity currently dirty across all open frames, and
/// clear them — used before a `return` so saves precede the exit.
fn flush_all_dirty(scope: &mut Scope, out: &mut String, pad: &str) {
    let mut names = Vec::new();
    for frame in &mut scope.frames {
        names.append(&mut frame.dirty);
    }
    for name in names {
        out.push_str(&format!("{pad}{name}.save()\n"));
    }
}

fn lower_stmt(stmt: &Stmt, env: &mut Env, scope: &mut Scope, out: &mut String, level: usize) {
    let pad = indent(level);
    match stmt {
        Stmt::Let { name, value, .. } => {
            if let Some(ctor) = entity_ctor(value) {
                lower_entity_ctor(&name.name, &ctor, env, scope, out, level);
            } else if matches!(value, Expr::Match { .. }) {
                scope
                    .warnings
                    .push("`match` in `let` position is not supported yet".into());
                out.push_str(&format!(
                    "{pad}// TODO: `let {name} = match …` unsupported\n"
                ));
            } else {
                let ty = infer(value, env, scope);
                let rhs = lower_expr(value, env, scope);
                out.push_str(&format!("{pad}let {name} = {rhs}\n"));
                // An entity returned from a helper is mutation-tracked so its
                // writes auto-save, exactly like a `loadOrCreate` local.
                if let RTy::Entity(e) = &ty {
                    scope.declare_entity(&name.name, e.clone());
                } else {
                    scope.declare_local(&name.name, ty);
                }
            }
        }
        Stmt::Assign { target, value, .. } => lower_assign(target, value, env, scope, out, level),
        Stmt::Return { value, .. } => {
            // `return Entity.create(…)` has to be bound before it is returned:
            // the constructor lowers to several statements (a `new`, then one
            // assignment per record field), which cannot sit inside a `return`
            // expression. Without this it fell through to the generic call
            // path and emitted `Entity.create(id, /* record */)` — a method
            // graph-ts has no such thing as, caught only by `graph build`.
            if let Some(v) = value {
                if let Some(ctor) = entity_ctor(v).filter(|c| env.entities.contains_key(&c.entity))
                {
                    let tmp = scope.fresh_with("_entity");
                    lower_entity_ctor(&tmp, &ctor, env, scope, out, level);
                    flush_all_dirty(scope, out, &pad);
                    out.push_str(&format!("{pad}return {tmp}\n"));
                    return;
                }
            }
            // Flush every pending entity save before leaving — the block-end
            // auto-save would otherwise be emitted *after* the `return` (dead
            // code) and the write would be silently lost.
            let r = value.as_ref().map(|v| lower_expr(v, env, scope));
            flush_all_dirty(scope, out, &pad);
            match r {
                Some(r) => out.push_str(&format!("{pad}return {r}\n")),
                None => out.push_str(&format!("{pad}return\n")),
            }
        }
        Stmt::If {
            cond,
            then_block,
            else_ifs,
            else_block,
            ..
        } => lower_if(
            cond,
            then_block,
            else_ifs,
            else_block.as_ref(),
            env,
            scope,
            out,
            level,
        ),
        Stmt::While { cond, body, .. } => {
            let c = lower_cond(cond, env, scope);
            out.push_str(&format!("{pad}while ({c}) {{\n"));
            lower_block(body, env, scope, out, level + 1);
            out.push_str(&format!("{pad}}}\n"));
        }
        Stmt::For {
            var, iter, body, ..
        } => lower_for(var, iter, body, env, scope, out, level),
        Stmt::Expr(e) => {
            // A create whose result nobody binds. Same expansion as the `let`
            // form, into a temporary nothing reads.
            //
            // The `entities.contains_key` guard is load-bearing: `entity_ctor`
            // matches any `X.create(…)`, and `<Template>.create(addr)` spawns a
            // dynamic data source rather than writing an entity. In `let`
            // position that never collided, because nobody binds a template
            // spawn; in statement position both shapes look identical.
            let entity_stmt = entity_ctor(e)
                .filter(|c| env.entities.contains_key(&c.entity))
                .map(|c| (c.entity.clone(), c));
            if let Some((_, ctor)) = entity_stmt {
                let tmp = scope.fresh_with("_entity");
                lower_entity_ctor(&tmp, &ctor, env, scope, out, level);
            } else if let Expr::Match {
                scrutinee, arms, ..
            } = e
            {
                lower_match(scrutinee, arms, env, scope, out, level);
            } else {
                let s = lower_expr(e, env, scope);
                out.push_str(&format!("{pad}{s}\n"));
            }
        }
    }
}

/// Lower an `if`/`else if`/`else` chain.
#[allow(clippy::too_many_arguments)]
fn lower_if(
    cond: &Expr,
    then_block: &Block,
    else_ifs: &[(Expr, Block)],
    else_block: Option<&Block>,
    env: &mut Env,
    scope: &mut Scope,
    out: &mut String,
    level: usize,
) {
    let pad = indent(level);
    let c = lower_cond(cond, env, scope);
    out.push_str(&format!("{pad}if ({c}) {{\n"));
    lower_block(then_block, env, scope, out, level + 1);
    out.push_str(&format!("{pad}}}"));
    for (c, b) in else_ifs {
        let cs = lower_cond(c, env, scope);
        out.push_str(&format!(" else if ({cs}) {{\n"));
        lower_block(b, env, scope, out, level + 1);
        out.push_str(&format!("{pad}}}"));
    }
    if let Some(b) = else_block {
        out.push_str(" else {\n");
        lower_block(b, env, scope, out, level + 1);
        out.push_str(&format!("{pad}}}"));
    }
    out.push('\n');
}

/// Lower a `for` loop to an index-based AssemblyScript `for` (AS has no
/// `for…of`): numeric ranges become a counted loop; list iteration loops over
/// indices and binds each element.
fn lower_for(
    var: &Ident,
    iter: &ForIter,
    body: &Block,
    env: &mut Env,
    scope: &mut Scope,
    out: &mut String,
    level: usize,
) {
    let pad = indent(level);
    match iter {
        ForIter::Range { start, end } => {
            let s = lower_expr(start, env, scope);
            let e = lower_expr(end, env, scope);
            let v = &var.name;
            scope.declare_local(&var.name, RTy::Int);
            out.push_str(&format!("{pad}for (let {v} = {s}; {v} < {e}; {v}++) {{\n"));
            lower_block(body, env, scope, out, level + 1);
            out.push_str(&format!("{pad}}}\n"));
        }
        ForIter::Each(list) => {
            let elem_ty = match infer(list, env, scope) {
                RTy::List(inner) => *inner,
                _ => RTy::Unknown,
            };
            let ls = lower_expr(list, env, scope);
            // Bind the list to a temp so a method/call expression is evaluated once.
            let arr = scope.fresh_with("_arr");
            let idx = scope.fresh_with("_i");
            let v = &var.name;
            out.push_str(&format!("{pad}let {arr} = {ls}\n"));
            out.push_str(&format!(
                "{pad}for (let {idx} = 0; {idx} < {arr}.length; {idx}++) {{\n"
            ));
            out.push_str(&format!("{}let {v} = {arr}[{idx}]\n", indent(level + 1)));
            scope.declare_local(&var.name, elem_ty);
            lower_block(body, env, scope, out, level + 1);
            out.push_str(&format!("{pad}}}\n"));
        }
    }
}

/// Lower a boolean condition. `BigInt`/`BigDecimal` comparisons inside become
/// method calls via [`lower_expr`]; this is just the entry point for clarity.
fn lower_cond(cond: &Expr, env: &mut Env, scope: &mut Scope) -> String {
    lower_expr(cond, env, scope)
}

/// A recognised entity constructor call.
struct EntityCtor<'a> {
    entity: String,
    kind: CtorKind,
    id: &'a Expr,
    record: Option<&'a [(Ident, Expr)]>,
}

enum CtorKind {
    LoadOrCreate,
    Create,
    /// `load(id)` / `loadInBlock(id)` — both return `Option<Entity>`.
    Load {
        in_block: bool,
    },
}

fn entity_ctor(value: &Expr) -> Option<EntityCtor<'_>> {
    let Expr::Call { callee, args, .. } = value else {
        return None;
    };
    let Expr::Field { base, field, .. } = callee.as_ref() else {
        return None;
    };
    let Expr::Path { segments, .. } = base.as_ref() else {
        return None;
    };
    let entity = segments.last()?.name.clone();
    let kind = match field.name.as_str() {
        "loadOrCreate" => CtorKind::LoadOrCreate,
        "create" => CtorKind::Create,
        "load" => CtorKind::Load { in_block: false },
        "loadInBlock" => CtorKind::Load { in_block: true },
        _ => return None,
    };
    let id = args.first()?;
    let record = match args.get(1) {
        Some(Expr::Record { fields, .. }) => Some(fields.as_slice()),
        _ => None,
    };
    Some(EntityCtor {
        entity,
        kind,
        id,
        record,
    })
}

fn lower_entity_ctor(
    var: &str,
    ctor: &EntityCtor,
    env: &mut Env,
    scope: &mut Scope,
    out: &mut String,
    level: usize,
) {
    let pad = indent(level);
    let entity = &ctor.entity;
    let id = lower_expr(ctor.id, env, scope);

    match ctor.kind {
        CtorKind::LoadOrCreate => {
            scope.declare_entity(var, entity.clone());
            out.push_str(&format!("{pad}let {var} = {entity}.load({id})\n"));
            out.push_str(&format!("{pad}if ({var} == null) {{\n"));
            out.push_str(&format!("{pad}  {var} = new {entity}({id})\n"));
            if let Some(fields) = ctor.record {
                lower_record_init(var, entity, fields, env, scope, out, level + 1);
            }
            out.push_str(&format!("{pad}}}\n"));
            scope.mark_dirty(var);
        }
        CtorKind::Create => {
            scope.declare_entity(var, entity.clone());
            out.push_str(&format!("{pad}let {var} = new {entity}({id})\n"));
            if let Some(fields) = ctor.record {
                lower_record_init(var, entity, fields, env, scope, out, level);
            }
            scope.mark_dirty(var);
        }
        CtorKind::Load { in_block } => {
            // `load`/`loadInBlock` return `Option<Entity>`: the local is nullable
            // and must be `match`ed before use, so null-deref is unrepresentable.
            scope.declare_local(var, RTy::Option(Box::new(RTy::Entity(entity.clone()))));
            let method = if in_block { "loadInBlock" } else { "load" };
            out.push_str(&format!("{pad}let {var} = {entity}.{method}({id})\n"));
        }
    }
}

fn lower_record_init(
    var: &str,
    entity: &str,
    fields: &[(Ident, Expr)],
    env: &mut Env,
    scope: &mut Scope,
    out: &mut String,
    level: usize,
) {
    let pad = indent(level);
    for (key, value) in fields {
        let rhs = lower_field_value(entity, &key.name, value, env, scope);
        out.push_str(&format!("{pad}{var}.{} = {rhs}\n", key.name));
    }
}

/// Lower a value being assigned to `entity.field`, coercing an entity-typed
/// value to its `.id` when the target field is an entity reference.
fn lower_field_value(
    entity: &str,
    field: &str,
    value: &Expr,
    env: &mut Env,
    scope: &mut Scope,
) -> String {
    let target_ty = env
        .entities
        .get(entity)
        .and_then(|e| e.fields.get(field))
        .cloned();
    let mut rhs = lower_expr(value, env, scope);
    if let Some(RTy::Entity(_)) = target_ty {
        if matches!(infer(value, env, scope), RTy::Entity(_)) {
            rhs.push_str(".id");
        }
    }
    rhs
}

fn lower_assign(
    target: &Expr,
    value: &Expr,
    env: &mut Env,
    scope: &mut Scope,
    out: &mut String,
    level: usize,
) {
    let pad = indent(level);
    if let Expr::Field { base, field, .. } = target {
        if let Expr::Path { segments, .. } = base.as_ref() {
            if segments.len() == 1 {
                let var = &segments[0].name;
                if let Some(RTy::Entity(entity)) = scope.locals.get(var).cloned() {
                    let rhs = lower_field_value(&entity, &field.name, value, env, scope);
                    out.push_str(&format!("{pad}{var}.{} = {rhs}\n", field.name));
                    scope.mark_dirty(var);
                    return;
                }
            }
        }
    }
    out.push_str(&format!(
        "{pad}{} = {}\n",
        lower_expr(target, env, scope),
        lower_expr(value, env, scope)
    ));
}

/// Lower a `match` statement on a `Result` or `Option` scrutinee.
fn lower_match(
    scrutinee: &Expr,
    arms: &[MatchArm],
    env: &mut Env,
    scope: &mut Scope,
    out: &mut String,
    level: usize,
) {
    let pad = indent(level);
    let scrut_ty = infer(scrutinee, env, scope);

    // Reference the scrutinee by a stable name; bind a temp if it's not a var.
    let var = if let Expr::Path { segments, .. } = scrutinee {
        if segments.len() == 1 {
            segments[0].name.clone()
        } else {
            bind_temp(scrutinee, env, scope, out, &pad)
        }
    } else {
        bind_temp(scrutinee, env, scope, out, &pad)
    };

    match scrut_ty {
        RTy::Result(inner) => {
            let (ok_bind, ok_body) = find_arm(arms, "Ok");
            let (_err_bind, err_body) = find_arm(arms, "Err");
            out.push_str(&format!("{pad}if (!{var}.reverted) {{\n"));
            lower_arm(
                ok_bind,
                &format!("{var}.value"),
                &inner,
                ok_body,
                env,
                scope,
                out,
                level,
            );
            out.push_str(&format!("{pad}}}"));
            if let Some(body) = err_body.filter(|b| !b.stmts.is_empty()) {
                out.push_str(" else {\n");
                lower_block(body, env, scope, out, level + 1);
                out.push_str(&format!("{pad}}}"));
            }
            out.push('\n');
        }
        RTy::Option(inner) => {
            let (some_bind, some_body) = find_arm(arms, "Some");
            let (_none_bind, none_body) = find_arm(arms, "None");
            // `!==` rather than `!=`: graph-ts gives BigInt, BigDecimal, Bytes
            // and Address an `@operator('!=')` taking two of themselves, so a
            // loose comparison against `null` sends the AssemblyScript compiler
            // into compileBinaryOverload, where it fails an internal assertion
            // and crashes the whole build. Strict identity skips overload
            // resolution entirely and is the correct test for a nullable
            // reference anyway.
            out.push_str(&format!("{pad}if ({var} !== null) {{\n"));
            lower_arm(
                some_bind,
                &format!("{var}!"),
                &inner,
                some_body,
                env,
                scope,
                out,
                level,
            );
            out.push_str(&format!("{pad}}}"));
            if let Some(body) = none_body.filter(|b| !b.stmts.is_empty()) {
                out.push_str(" else {\n");
                lower_block(body, env, scope, out, level + 1);
                out.push_str(&format!("{pad}}}"));
            }
            out.push('\n');
        }
        _ => {
            scope.warnings.push(format!(
                "`match` on a {scrut_ty:?} scrutinee is not supported yet; emitted a comment"
            ));
            out.push_str(&format!("{pad}// TODO: unsupported match\n"));
        }
    }
}

/// Lower one `match` arm: bind the unwrapped value (if the pattern binds one) in
/// a fresh frame, lower the body, then auto-save any entity it dirtied. An
/// entity binding is save-tracked here, so `Some(token) => { token.x = … }`
/// can't silently drop the write.
#[allow(clippy::too_many_arguments)]
fn lower_arm(
    bind: Option<&Ident>,
    bind_rhs: &str,
    inner: &RTy,
    body: Option<&Block>,
    env: &mut Env,
    scope: &mut Scope,
    out: &mut String,
    level: usize,
) {
    scope.frames.push(Frame::default());
    let pad = indent(level + 1);
    if let Some(b) = bind {
        out.push_str(&format!("{pad}let {} = {bind_rhs}\n", b.name));
        if let RTy::Entity(name) = inner {
            scope.declare_entity(&b.name, name.clone());
        } else {
            scope.declare_local(&b.name, inner.clone());
        }
    }
    if let Some(body) = body {
        for stmt in &body.stmts {
            lower_stmt(stmt, env, scope, out, level + 1);
        }
    }
    let frame = scope.frames.pop().expect("frame pushed above");
    if !frame.dirty.is_empty() {
        out.push('\n');
        for name in &frame.dirty {
            out.push_str(&format!("{pad}{name}.save()\n"));
        }
    }
}

fn bind_temp(expr: &Expr, env: &mut Env, scope: &mut Scope, out: &mut String, pad: &str) -> String {
    let name = scope.fresh();
    let rhs = lower_expr(expr, env, scope);
    out.push_str(&format!("{pad}let {name} = {rhs}\n"));
    name
}

/// Find the arm whose pattern is a constructor named `ctor`, returning its first
/// binding (if any) and its body block.
fn find_arm<'a>(arms: &'a [MatchArm], ctor: &str) -> (Option<&'a Ident>, Option<&'a Block>) {
    for arm in arms {
        if let Pattern::Ctor { name, bindings, .. } = &arm.pattern {
            if name.name == ctor {
                return (bindings.first(), Some(&arm.body));
            }
        }
    }
    (None, None)
}

/// Lower an expression to AssemblyScript text.
fn lower_expr(expr: &Expr, env: &mut Env, scope: &mut Scope) -> String {
    match expr {
        Expr::Int { raw, .. } => raw.clone(),
        Expr::Hex { raw, .. } => format!("Bytes.fromHexString(\"{raw}\")"),
        Expr::Decimal { raw, .. } => format!("BigDecimal.fromString(\"{raw}\")"),
        Expr::Str { value, .. } => format!("\"{}\"", value.replace('"', "\\\"")),
        Expr::Bool { value, .. } => value.to_string(),
        // `None` is Redstart's absent value. Assigned to an `Option<T>` field it
        // becomes graph-ts's `null`, whose generated setter calls `unset(field)`
        // — the clear-a-relation move, without reaching for the raw store.
        Expr::Path { segments, .. } if segments.len() == 1 && segments[0].name == "None" => {
            "null".to_string()
        }
        Expr::Path { segments, .. } => segments
            .iter()
            .map(|s| s.name.clone())
            .collect::<Vec<_>>()
            .join("."),
        Expr::Field { base, field, .. } => lower_field(base, &field.name, env, scope),
        // `Some(x)` is Redstart sugar, not a function. An `Option<T>` lowers to
        // graph-ts's `T | null`, so the constructor is the identity: emitting a
        // call here produced a reference to an undefined `Some` that only
        // `graph build` would catch (and `None` is already lowered above).
        Expr::Call { callee, args, .. }
            if args.len() == 1
                && matches!(
                    callee.as_ref(),
                    Expr::Path { segments, .. } if segments.len() == 1 && segments[0].name == "Some"
                ) =>
        {
            lower_expr(&args[0], env, scope)
        }
        Expr::Call { callee, args, .. } => lower_call(callee, args, env, scope),
        Expr::Record { .. } => "/* record */".to_string(),
        Expr::Array { elems, .. } => {
            let items = elems
                .iter()
                .map(|e| lower_expr(e, env, scope))
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{items}]")
        }
        Expr::Index { base, index, .. } => {
            let b = lower_expr(base, env, scope);
            let i = lower_expr(index, env, scope);
            format!("{b}[{i}]")
        }
        Expr::Unary { op, expr, .. } => {
            let inner = lower_expr(expr, env, scope);
            match op {
                UnOp::Not => format!("!{inner}"),
                UnOp::Neg => format!("-{inner}"),
            }
        }
        Expr::Binary { op, lhs, rhs, .. } => lower_binary(*op, lhs, rhs, env, scope),
        Expr::Match { .. } => {
            scope
                .warnings
                .push("`match` used as a value is not supported yet".into());
            "/* TODO: match */".to_string()
        }
    }
}

fn lower_field(base: &Expr, field: &str, env: &mut Env, scope: &mut Scope) -> String {
    // Synthetic `event.id` -> a unique composite id (event handlers only).
    if field == "id" && scope.param_ty == RTy::Event {
        if let Expr::Path { segments, .. } = base {
            if segments.len() == 1 && segments[0].name == scope.event_param {
                return format!(
                    "{ev}.transaction.hash.concatI32({ev}.logIndex.toI32())",
                    ev = scope.event_param
                );
            }
        }
    }
    // Nullary static accessors read as properties in Redstart and are called in
    // AssemblyScript: `BigInt.zero` -> `BigInt.zero()`, `Bytes.empty` ->
    // `Bytes.empty()`. Written without the call, they'd lower to a function
    // reference and fail to compile on eject.
    if let Expr::Path { segments, .. } = base {
        if segments.len() == 1 {
            let ty = segments[0].name.as_str();
            let nullary = matches!(
                (ty, field),
                ("BigInt" | "BigDecimal", "zero")
                    | ("Bytes" | "ByteArray", "empty")
                    | ("Address", "zero")
            );
            if nullary {
                return format!("{ty}.{field}()");
            }
        }
    }
    format!("{}.{field}", lower_expr(base, env, scope))
}

fn lower_call(callee: &Expr, args: &[Expr], env: &mut Env, scope: &mut Scope) -> String {
    if let Expr::Field { base, field, .. } = callee {
        // `DataSourceContext.new()` -> `new DataSourceContext()` (AS construction).
        if field.name == "new" {
            if let Expr::Path { segments, .. } = base.as_ref() {
                if segments.len() == 1 && segments[0].name == "DataSourceContext" {
                    return "new DataSourceContext()".to_string();
                }
            }
        }
        // Contract call: `<contract>.method(args)` -> `<contract>.try_method(args)`.
        if let RTy::Contract(abi) = infer(base, env, scope) {
            if env.abis.is_function(&abi, &field.name) {
                let base_s = lower_expr(base, env, scope);
                let arg_s = lower_args(args, env, scope);
                return format!("{base_s}.try_{}({arg_s})", field.name);
            }
        }
        // Remap known method names: `.toDecimal()` -> `.toBigDecimal()`.
        let method = match field.name.as_str() {
            "toDecimal" => "toBigDecimal",
            other => other,
        };
        let base_s = lower_expr(base, env, scope);
        let arg_s = lower_args(args, env, scope);
        return format!("{base_s}.{method}({arg_s})");
    }
    let callee_s = lower_expr(callee, env, scope);
    let arg_s = lower_args(args, env, scope);
    format!("{callee_s}({arg_s})")
}

fn lower_args(args: &[Expr], env: &mut Env, scope: &mut Scope) -> String {
    args.iter()
        .map(|a| lower_expr(a, env, scope))
        .collect::<Vec<_>>()
        .join(", ")
}

fn lower_binary(op: BinOp, lhs: &Expr, rhs: &Expr, env: &mut Env, scope: &mut Scope) -> String {
    let lt = infer(lhs, env, scope);
    let rt = infer(rhs, env, scope);
    let ls = lower_expr(lhs, env, scope);
    let rs = lower_expr(rhs, env, scope);

    if (lt.is_bigint() || rt.is_bigint() || lt.is_bigdecimal() || rt.is_bigdecimal())
        && bigmath_method(op).is_some()
    {
        return format!("{ls}.{}({rs})", bigmath_method(op).unwrap());
    }
    format!("{ls} {} {rs}", binop_symbol(op))
}

/// The graph-ts `BigInt`/`BigDecimal` method for an operator.
fn bigmath_method(op: BinOp) -> Option<&'static str> {
    Some(match op {
        BinOp::Add => "plus",
        BinOp::Sub => "minus",
        BinOp::Mul => "times",
        BinOp::Div => "div",
        BinOp::Eq => "equals",
        BinOp::Ne => "notEqual",
        BinOp::Lt => "lt",
        BinOp::Le => "le",
        BinOp::Gt => "gt",
        BinOp::Ge => "ge",
        _ => return None,
    })
}

fn binop_symbol(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
    }
}

/// Infer the resolved type of an expression — enough to choose lowerings.
fn infer(expr: &Expr, env: &mut Env, scope: &mut Scope) -> RTy {
    match expr {
        Expr::Int { .. } => RTy::Int,
        Expr::Decimal { .. } => RTy::BigDecimal,
        Expr::Hex { .. } => RTy::Bytes,
        Expr::Str { .. } => RTy::String,
        Expr::Bool { .. } => RTy::Boolean,
        Expr::Path { segments, .. } => {
            if segments.len() == 1 {
                if segments[0].name == scope.event_param {
                    return scope.param_ty.clone();
                }
                if let Some(t) = scope.locals.get(&segments[0].name) {
                    return t.clone();
                }
            }
            RTy::Unknown
        }
        Expr::Field { base, field, .. } => infer_field(base, &field.name, env, scope),
        Expr::Call { callee, .. } => infer_call(callee, env, scope),
        Expr::Binary { op, lhs, rhs, .. } => {
            if matches!(
                op,
                BinOp::Eq
                    | BinOp::Ne
                    | BinOp::Lt
                    | BinOp::Le
                    | BinOp::Gt
                    | BinOp::Ge
                    | BinOp::And
                    | BinOp::Or
            ) {
                RTy::Boolean
            } else {
                let lt = infer(lhs, env, scope);
                if lt == RTy::Unknown {
                    infer(rhs, env, scope)
                } else {
                    lt
                }
            }
        }
        Expr::Unary { expr, .. } => infer(expr, env, scope),
        Expr::Array { elems, .. } => {
            let elem = elems.first().map_or(RTy::Unknown, |e| infer(e, env, scope));
            RTy::List(Box::new(elem))
        }
        Expr::Index { base, .. } => match infer(base, env, scope) {
            RTy::List(inner) => *inner,
            _ => RTy::Unknown,
        },
        _ => RTy::Unknown,
    }
}

fn infer_field(base: &Expr, field: &str, env: &mut Env, scope: &mut Scope) -> RTy {
    let base_ty = infer(base, env, scope);
    match base_ty {
        RTy::Event => match field {
            "params" => RTy::Params,
            "block" => RTy::Block,
            "transaction" => RTy::Transaction,
            "address" => RTy::Address,
            "id" => RTy::Bytes,
            _ => RTy::Unknown,
        },
        RTy::Params => {
            let (abi, event) = (scope.abi.clone(), scope.member.clone());
            env.abis
                .event_params(&abi, &event)
                .and_then(|params| {
                    params
                        .iter()
                        .find(|p| p.name == field)
                        .map(|p| sol_to_rty(&p.sol_type))
                })
                .unwrap_or(RTy::Unknown)
        }
        RTy::Call => match field {
            "inputs" => RTy::CallInputs,
            "outputs" => RTy::CallOutputs,
            "block" => RTy::Block,
            "transaction" => RTy::Transaction,
            "from" | "to" => RTy::Address,
            _ => RTy::Unknown,
        },
        RTy::CallInputs => {
            let (abi, func) = (scope.abi.clone(), scope.member.clone());
            env.abis
                .function_inputs(&abi, &func)
                .and_then(|params| {
                    params
                        .iter()
                        .find(|p| p.name == field)
                        .map(|p| sol_to_rty(&p.sol_type))
                })
                .unwrap_or(RTy::Unknown)
        }
        RTy::CallOutputs => {
            let (abi, func) = (scope.abi.clone(), scope.member.clone());
            env.abis
                .function_output_params(&abi, &func)
                .and_then(|params| {
                    params
                        .iter()
                        .find(|p| p.name == field)
                        .map(|p| sol_to_rty(&p.sol_type))
                })
                .unwrap_or(RTy::Unknown)
        }
        RTy::Block => match field {
            "timestamp" | "number" => RTy::BigInt,
            "hash" => RTy::Bytes,
            _ => RTy::Unknown,
        },
        RTy::Transaction => match field {
            "hash" | "from" | "to" => RTy::Bytes,
            "value" | "gasPrice" => RTy::BigInt,
            _ => RTy::Unknown,
        },
        RTy::Result(inner) => match field {
            "value" => *inner,
            "reverted" => RTy::Boolean,
            _ => RTy::Unknown,
        },
        RTy::Entity(name) => env
            .entities
            .get(&name)
            .and_then(|e| e.fields.get(field))
            .cloned()
            .unwrap_or(RTy::Unknown),
        _ => RTy::Unknown,
    }
}

fn infer_call(callee: &Expr, env: &mut Env, scope: &mut Scope) -> RTy {
    // A call to a user-declared free function -> its declared return type.
    if let Expr::Path { segments, .. } = callee {
        if segments.len() == 1 {
            if let Some(ret) = env.fn_returns.get(&segments[0].name) {
                return ret.clone();
            }
        }
        return RTy::Unknown;
    }

    let Expr::Field { base, field, .. } = callee else {
        return RTy::Unknown;
    };

    if let Expr::Path { segments, .. } = base.as_ref() {
        if segments.len() == 1 {
            let base_name = &segments[0].name;
            // `Abi.bind(addr)` -> a bound contract instance.
            if field.name == "bind" && env.abis.paths.contains_key(base_name) {
                return RTy::Contract(base_name.clone());
            }
            // `Entity.load(id)` / `loadInBlock(id)` -> Option<Entity> (nullable).
            if matches!(field.name.as_str(), "load" | "loadInBlock")
                && env.entities.contains_key(base_name)
            {
                return RTy::Option(Box::new(RTy::Entity(base_name.clone())));
            }
            // `ipfs.cat(hash)` -> Option<Bytes> (the fetch may fail).
            if base_name == "ipfs" && field.name == "cat" {
                return RTy::Option(Box::new(RTy::Bytes));
            }
            // graph-ts static constructors: `BigInt.fromI32(x)`, `Address.zero()`, …
            if let Some(ty) = static_ctor_type(base_name, &field.name) {
                return ty;
            }
        }
    }
    // `<contract>.method(args)` -> Result<ret, CallRevert>.
    if let RTy::Contract(abi) = infer(base, env, scope) {
        if let Some(outputs) = env.abis.function_outputs(&abi, &field.name) {
            let ret = outputs.first().map_or(RTy::Unknown, |s| sol_to_rty(s));
            return RTy::Result(Box::new(ret));
        }
    }
    // graph-ts instance methods whose return type we know.
    match field.name.as_str() {
        "toDecimal" | "toBigDecimal" | "divDecimal" => RTy::BigDecimal,
        "toBigInt" => RTy::BigInt,
        "toHex" | "toHexString" | "toString" | "toBase58" => RTy::String,
        "toI32" | "toU32" => RTy::Int,
        // Numeric ops preserve the receiver's type.
        "abs" | "neg" | "plus" | "minus" | "times" | "div" | "mod" | "pow" | "sqrt" | "bitAnd"
        | "bitOr" | "leftShift" | "rightShift" => infer(base, env, scope),
        "concat" | "concatI32" => RTy::Bytes,
        _ => RTy::Unknown,
    }
}

/// The return type of a known graph-ts static constructor `Type.method(...)`.
fn static_ctor_type(ty: &str, method: &str) -> Option<RTy> {
    Some(match (ty, method) {
        ("BigInt", m) if m.starts_with("from") || m == "zero" => RTy::BigInt,
        ("BigDecimal", m) if m.starts_with("from") || m == "zero" => RTy::BigDecimal,
        ("ByteArray", _) => RTy::Bytes,
        ("Bytes", _) => RTy::Bytes,
        ("Address", _) => RTy::Address,
        ("crypto", "keccak256") => RTy::Bytes,
        ("dataSource", "address") => RTy::Address,
        ("dataSource", "network") => RTy::String,
        _ => return None,
    })
}
