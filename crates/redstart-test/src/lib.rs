//! Native test interpreter for Redstart.
//!
//! Runs `test "..." { ... }` blocks by evaluating handler ASTs directly against
//! an in-memory mock store — no WASM, no Matchstick binary, no Docker. Because
//! Redstart owns the language and ABI metadata, tests need no `matchstick-as`
//! and event fixtures are synthesised from a record literal:
//!
//! ```text
//! test "transfer moves balance" {
//!   mockCall(ERC20.balanceOf(bob), 100)              // mock a contract read
//!   Token.Transfer({ from: alice, to: bob, value: 100 })  // fire the event
//!   assertEq(Account.at(bob).balance, 100)           // assert on the store
//! }
//! ```
//!
//! Fidelity vs the real WASM is backstopped by the conformance store-diff gate;
//! this layer is the fast inner loop for *handler logic*.

#![forbid(unsafe_code)]

mod value;

pub use value::{CallVal, EventVal, Value};

use bigdecimal::BigDecimal;
use bigdecimal::Zero;
use num_bigint::BigInt;
use redstart_checker::Checked;
use redstart_loader::ModuleTree;
use redstart_parser::ast::{BinOp, Block, Expr, ForIter, HandlerDecl, HandlerKind, Stmt, UnOp};
use redstart_parser::Span;
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;
use value::value_eq;

/// The outcome of one test.
pub enum Outcome {
    /// The test passed.
    Pass,
    /// The test failed, with a message and optional `file:line:col`.
    Fail {
        /// The failure message.
        message: String,
        /// Where it failed, if known.
        location: Option<String>,
    },
}

/// A single test result.
pub struct TestResult {
    /// The test description.
    pub name: String,
    /// The outcome.
    pub outcome: Outcome,
}

/// The full report for a run.
pub struct TestReport {
    /// One entry per test.
    pub results: Vec<TestResult>,
}

impl TestReport {
    /// Whether every test passed.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.results
            .iter()
            .all(|r| matches!(r.outcome, Outcome::Pass))
    }

    /// Number of passing tests.
    #[must_use]
    pub fn passed(&self) -> usize {
        self.results
            .iter()
            .filter(|r| matches!(r.outcome, Outcome::Pass))
            .count()
    }
}

/// Run every `test` block in the project.
#[must_use]
pub fn run_tests(tree: &ModuleTree, _checked: &Checked) -> TestReport {
    let interp = Interp::build(tree);
    let mut results = Vec::new();
    for module in tree.ordered() {
        let file = module.file_path.display().to_string();
        for t in &module.program.tests {
            let outcome = interp.run_test(&t.body, &file);
            results.push(TestResult {
                name: t.name.clone(),
                outcome,
            });
        }
    }
    TestReport { results }
}

// ---- interpreter ----

struct Interp<'t> {
    /// (source, event-or-function) -> event/call handler.
    handlers: HashMap<(String, String), &'t HandlerDecl>,
    /// source -> its block handlers.
    block_handlers: HashMap<String, Vec<&'t HandlerDecl>>,
    /// file-template name -> its file handler.
    file_handlers: HashMap<String, &'t HandlerDecl>,
    /// source/template name -> address bytes.
    source_addr: HashMap<String, Vec<u8>>,
    /// known ABI names (for `Abi.bind`).
    abis: Vec<String>,
    /// declared template names (dynamic data sources).
    templates: std::collections::HashSet<String>,
    /// free helper functions, by name.
    fns: HashMap<String, &'t redstart_parser::ast::FnDecl>,
}

/// A mocked contract-call outcome.
enum Mock {
    Return(Value),
    Revert,
}

/// Per-test mutable world.
struct World {
    store: HashMap<(String, Vec<u8>), BTreeMap<String, Value>>,
    mocks: HashMap<String, Mock>,
    /// Dynamic data sources spawned via `<Template>.create(addr)`.
    created: Vec<(String, Vec<u8>)>,
    /// Entities touched during the *current* handler invocation, shared across
    /// every helper-fn frame so `getOrCreateX` helpers see each other's writes
    /// (graph-node's in-block semantics). Flushed to `store` when the handler
    /// returns, then cleared.
    working: Vec<WorkingEntity>,
}

/// A lexical frame (test body, a handler invocation, or a helper-fn call).
struct Frame {
    locals: HashMap<String, Value>,
    returned: bool,
    /// The value of the `return` that exited this frame (for helper fns).
    return_value: Option<Value>,
}

struct WorkingEntity {
    entity: String,
    id: Vec<u8>,
    fields: BTreeMap<String, Value>,
    dirty: bool,
}

/// An interpreter error == a failed assertion or a misuse (unmocked call, etc.).
struct TError {
    message: String,
    span: Option<Span>,
}

fn err<T>(message: impl Into<String>, span: Option<Span>) -> Result<T, TError> {
    Err(TError {
        message: message.into(),
        span,
    })
}

type R<T> = Result<T, TError>;

impl<'t> Interp<'t> {
    fn build(tree: &'t ModuleTree) -> Self {
        let mut handlers = HashMap::new();
        let mut block_handlers: HashMap<String, Vec<&HandlerDecl>> = HashMap::new();
        let mut file_handlers: HashMap<String, &HandlerDecl> = HashMap::new();
        let mut source_addr = HashMap::new();
        let mut abis = Vec::new();
        let mut templates = std::collections::HashSet::new();
        let mut fns = HashMap::new();

        for m in tree.ordered() {
            for a in &m.program.abis {
                abis.push(a.name.name.clone());
            }
            for t in &m.program.templates {
                templates.insert(t.name.name.clone());
            }
            for f in &m.program.functions {
                fns.insert(f.name.name.clone(), f);
            }
            for h in &m.program.handlers {
                match h.kind {
                    HandlerKind::Block(_) => {
                        block_handlers
                            .entry(h.source.name.clone())
                            .or_default()
                            .push(h);
                    }
                    HandlerKind::File => {
                        file_handlers.insert(h.source.name.clone(), h);
                    }
                    HandlerKind::Event | HandlerKind::Call => {
                        handlers.insert((h.source.name.clone(), h.event.name.clone()), h);
                    }
                }
            }
            for s in &m.program.sources {
                if let Some(addr) = setting_addr(&s.settings) {
                    source_addr.insert(s.name.name.clone(), addr);
                }
            }
        }
        Self {
            handlers,
            block_handlers,
            file_handlers,
            source_addr,
            abis,
            templates,
            fns,
        }
    }

    fn run_test(&self, body: &Block, file: &str) -> Outcome {
        let mut world = World {
            store: HashMap::new(),
            mocks: HashMap::new(),
            created: Vec::new(),
            working: Vec::new(),
        };
        let mut frame = Frame {
            locals: HashMap::new(),
            returned: false,
            return_value: None,
        };
        match self.exec_block(body, &mut world, &mut frame) {
            Ok(()) => Outcome::Pass,
            Err(e) => Outcome::Fail {
                message: e.message,
                location: e.span.map(|s| {
                    let (line, col) = s.line_col();
                    format!("{file}:{line}:{col}")
                }),
            },
        }
    }

    // ---- statements ----

    fn exec_block(&self, block: &Block, world: &mut World, frame: &mut Frame) -> R<()> {
        for stmt in &block.stmts {
            self.exec_stmt(stmt, world, frame)?;
            if frame.returned {
                break;
            }
        }
        Ok(())
    }

    fn exec_stmt(&self, stmt: &Stmt, world: &mut World, frame: &mut Frame) -> R<()> {
        match stmt {
            Stmt::Let { name, value, .. } => {
                let v = self.eval(value, world, frame)?;
                frame.locals.insert(name.name.clone(), v);
            }
            Stmt::Assign { target, value, .. } => {
                let v = self.eval(value, world, frame)?;
                self.assign(target, v, world, frame)?;
            }
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    frame.return_value = Some(self.eval(v, world, frame)?);
                }
                frame.returned = true;
            }
            Stmt::If {
                cond,
                then_block,
                else_ifs,
                else_block,
                ..
            } => {
                if self.eval(cond, world, frame)?.as_bool().unwrap_or(false) {
                    self.exec_block(then_block, world, frame)?;
                    return Ok(());
                }
                for (c, block) in else_ifs {
                    if self.eval(c, world, frame)?.as_bool().unwrap_or(false) {
                        self.exec_block(block, world, frame)?;
                        return Ok(());
                    }
                }
                if let Some(block) = else_block {
                    self.exec_block(block, world, frame)?;
                }
            }
            Stmt::While { cond, body, span } => {
                let mut guard = 0u64;
                while self.eval(cond, world, frame)?.as_bool().unwrap_or(false) {
                    self.exec_block(body, world, frame)?;
                    if frame.returned {
                        break;
                    }
                    guard += 1;
                    if guard > 10_000_000 {
                        return err(
                            "`while` loop exceeded 10M iterations (likely non-terminating)",
                            Some(span.clone()),
                        );
                    }
                }
            }
            Stmt::For {
                var,
                iter,
                body,
                span,
            } => {
                self.exec_for(var, iter, body, world, frame, span)?;
            }
            Stmt::Expr(e) => {
                self.exec_expr_stmt(e, world, frame)?;
            }
        }
        Ok(())
    }

    fn exec_for(
        &self,
        var: &redstart_parser::Ident,
        iter: &ForIter,
        body: &Block,
        world: &mut World,
        frame: &mut Frame,
        span: &Span,
    ) -> R<()> {
        match iter {
            ForIter::Range { start, end } => {
                let lo = self
                    .eval(start, world, frame)?
                    .to_bigint()
                    .ok_or_else(|| TError {
                        message: "range start must be numeric".into(),
                        span: Some(start.span().clone()),
                    })?;
                let hi = self
                    .eval(end, world, frame)?
                    .to_bigint()
                    .ok_or_else(|| TError {
                        message: "range end must be numeric".into(),
                        span: Some(end.span().clone()),
                    })?;
                let mut i = lo;
                while i < hi {
                    frame.locals.insert(var.name.clone(), Value::Big(i.clone()));
                    self.exec_block(body, world, frame)?;
                    if frame.returned {
                        break;
                    }
                    i += 1;
                }
            }
            ForIter::Each(list) => {
                let items = match self.eval(list, world, frame)? {
                    Value::Array(items) => items,
                    other => {
                        return err(
                            format!("`for … in` needs an array, got `{}`", other.canonical()),
                            Some(span.clone()),
                        )
                    }
                };
                for item in items {
                    frame.locals.insert(var.name.clone(), item);
                    self.exec_block(body, world, frame)?;
                    if frame.returned {
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    /// Expression statements: `match`, the test intrinsics, event firing, or a
    /// plain side-effecting expression.
    fn exec_expr_stmt(&self, e: &Expr, world: &mut World, frame: &mut Frame) -> R<()> {
        if let Expr::Match {
            scrutinee, arms, ..
        } = e
        {
            return self.exec_match(scrutinee, arms, world, frame);
        }
        if let Expr::Call { callee, args, span } = e {
            // Intrinsics and event firing are recognised by callee shape.
            if let Some(name) = path_name(callee) {
                match name.as_str() {
                    "mockCall" => return self.do_mock(args, world, frame, false, span),
                    "mockRevert" => return self.do_mock(args, world, frame, true, span),
                    "assert" => return self.do_assert(args, world, frame, span),
                    "assertEq" => return self.do_assert_eq(args, world, frame, span),
                    "assertCreated" => return self.do_assert_created(args, world, frame, span),
                    // `assertMissing(Entity, id)` — the assertion a "this
                    // handler bails cleanly" test needs. Without it such a test
                    // asserts nothing at all and passes even if the handler
                    // silently wrote a half-built entity.
                    "assertMissing" => return self.do_assert_missing(args, world, frame, span),
                    _ => {}
                }
            }
            if let Expr::Field { base, field, .. } = callee.as_ref() {
                if let Some(src) = single_path(base) {
                    // `Source.block({ … })` fires that source's block handlers.
                    if field.name == "block" && self.block_handlers.contains_key(&src) {
                        return self.fire_block(&src, args, world, frame, span);
                    }
                    // `Template.file(bytes)` fires a file/IPFS data-source handler.
                    if field.name == "file" && self.file_handlers.contains_key(&src) {
                        return self.fire_file(&src, args, world, frame, span);
                    }
                    if let Some(h) = self
                        .handlers
                        .get(&(src.clone(), field.name.clone()))
                        .copied()
                    {
                        return match h.kind {
                            HandlerKind::Call => self.fire_call(h, args, world, frame, span),
                            _ => self.fire(&src, &field.name, args, world, frame, span),
                        };
                    }
                }
            }
        }
        // Otherwise evaluate for side effects.
        self.eval(e, world, frame)?;
        Ok(())
    }

    fn exec_match(
        &self,
        scrutinee: &Expr,
        arms: &[redstart_parser::ast::MatchArm],
        world: &mut World,
        frame: &mut Frame,
    ) -> R<()> {
        use redstart_parser::ast::Pattern;
        let v = self.eval(scrutinee, world, frame)?;
        let (ctor, inner) = match &v {
            Value::Result { reverted, value } => {
                (if *reverted { "Err" } else { "Ok" }, (**value).clone())
            }
            Value::Null => ("None", Value::Null),
            other => ("Some", other.clone()),
        };
        for arm in arms {
            let matches = match &arm.pattern {
                Pattern::Wildcard { .. } | Pattern::Binding { .. } => true,
                Pattern::Ctor { name, bindings, .. } => {
                    if name.name == ctor {
                        if let Some(b) = bindings.first() {
                            frame.locals.insert(b.name.clone(), inner.clone());
                        }
                        true
                    } else {
                        false
                    }
                }
            };
            if matches {
                return self.exec_block(&arm.body, world, frame);
            }
        }
        Ok(())
    }

    fn assign(&self, target: &Expr, v: Value, world: &mut World, frame: &mut Frame) -> R<()> {
        // Plain variable reassignment (`i = i + 1`, accumulator updates).
        if let Some(name) = single_path(target) {
            frame.locals.insert(name, v);
            return Ok(());
        }
        if let Expr::Field { base, field, span } = target {
            let bv = self.eval(base, world, frame)?;
            if let Value::Handle(h) = bv {
                world.working[h].fields.insert(field.name.clone(), v);
                world.working[h].dirty = true;
                return Ok(());
            }
            return err("can only assign to entity fields", Some(span.clone()));
        }
        let _ = world;
        err("invalid assignment target", Some(target.span().clone()))
    }

    // ---- intrinsics ----

    fn do_mock(
        &self,
        args: &[Expr],
        world: &mut World,
        frame: &mut Frame,
        revert: bool,
        span: &Span,
    ) -> R<()> {
        let call = args.first().ok_or_else(|| TError {
            message: "mock needs a contract call as its first argument".into(),
            span: Some(span.clone()),
        })?;
        let key = self.mock_key(call, world, frame)?;
        let mock = if revert {
            Mock::Revert
        } else {
            let v = self.eval(
                args.get(1).ok_or_else(|| TError {
                    message: "mockCall needs a return value as its second argument".into(),
                    span: Some(span.clone()),
                })?,
                world,
                frame,
            )?;
            Mock::Return(v)
        };
        world.mocks.insert(key, mock);
        Ok(())
    }

    /// Build the `func(arg,arg)` key from an unevaluated `Abi.func(args)` call.
    fn mock_key(&self, call: &Expr, world: &mut World, frame: &mut Frame) -> R<String> {
        let Expr::Call { callee, args, .. } = call else {
            return err(
                "expected a contract call like `ERC20.balanceOf(addr)`",
                Some(call.span().clone()),
            );
        };
        let Expr::Field { field, .. } = callee.as_ref() else {
            return err(
                "expected a contract call like `ERC20.balanceOf(addr)`",
                Some(call.span().clone()),
            );
        };
        let mut parts = Vec::new();
        for a in args {
            parts.push(self.eval(a, world, frame)?.canonical());
        }
        Ok(format!("{}({})", field.name, parts.join(",")))
    }

    fn do_assert(&self, args: &[Expr], world: &mut World, frame: &mut Frame, span: &Span) -> R<()> {
        let cond = self.eval(
            args.first().ok_or_else(|| TError {
                message: "assert needs a condition".into(),
                span: Some(span.clone()),
            })?,
            world,
            frame,
        )?;
        match cond.as_bool() {
            Some(true) => Ok(()),
            Some(false) => err("assertion failed", Some(span.clone())),
            None => err("assert expects a boolean", Some(span.clone())),
        }
    }

    fn do_assert_eq(
        &self,
        args: &[Expr],
        world: &mut World,
        frame: &mut Frame,
        span: &Span,
    ) -> R<()> {
        let a = self.eval(
            args.first().ok_or_else(|| TError {
                message: "assertEq needs two arguments".into(),
                span: Some(span.clone()),
            })?,
            world,
            frame,
        )?;
        let b = self.eval(
            args.get(1).ok_or_else(|| TError {
                message: "assertEq needs two arguments".into(),
                span: Some(span.clone()),
            })?,
            world,
            frame,
        )?;
        if value_eq(&a, &b) {
            Ok(())
        } else {
            err(
                format!(
                    "assertEq failed: left = {}, right = {}",
                    a.canonical(),
                    b.canonical()
                ),
                Some(span.clone()),
            )
        }
    }

    /// `assertCreated(Template, addr)` — assert a dynamic data source was spawned.
    fn do_assert_created(
        &self,
        args: &[Expr],
        world: &mut World,
        frame: &mut Frame,
        span: &Span,
    ) -> R<()> {
        let tmpl = args.first().and_then(single_path).ok_or_else(|| TError {
            message: "assertCreated needs a template name and an address".into(),
            span: Some(span.clone()),
        })?;
        let addr = self
            .eval(
                args.get(1).ok_or_else(|| TError {
                    message: "assertCreated needs an address".into(),
                    span: Some(span.clone()),
                })?,
                world,
                frame,
            )?
            .as_bytes()
            .ok_or_else(|| TError {
                message: "assertCreated address must be Bytes/Address".into(),
                span: Some(span.clone()),
            })?;
        if world.created.iter().any(|(t, a)| t == &tmpl && a == &addr) {
            Ok(())
        } else {
            err(
                format!(
                    "assertCreated failed: no `{tmpl}` data source created at 0x{}",
                    hex(&addr)
                ),
                Some(span.clone()),
            )
        }
    }

    // ---- event firing ----

    fn fire(
        &self,
        source: &str,
        event: &str,
        args: &[Expr],
        world: &mut World,
        frame: &mut Frame,
        span: &Span,
    ) -> R<()> {
        let handler = self
            .handlers
            .get(&(source.to_string(), event.to_string()))
            .copied()
            .ok_or_else(|| TError {
                message: format!("no handler for `{source}.{event}`"),
                span: Some(span.clone()),
            })?;

        // Build the event from the record literal argument.
        let mut ev = EventVal {
            params: BTreeMap::new(),
            address: self.source_addr.get(source).cloned().unwrap_or_default(),
            block_number: BigInt::from(0),
            block_timestamp: BigInt::from(0),
            tx_hash: vec![0u8; 32],
            log_index: 0,
        };
        if let Some(Expr::Record { fields, .. }) = args.first() {
            for (k, vexpr) in fields {
                let v = self.eval(vexpr, world, frame)?;
                match k.name.as_str() {
                    // `_`-prefixed keys override event metadata, not params.
                    "_timestamp" => ev.block_timestamp = v.to_bigint().unwrap_or_default(),
                    "_block" => ev.block_number = v.to_bigint().unwrap_or_default(),
                    "_logIndex" => {
                        ev.log_index = v
                            .to_bigint()
                            .and_then(|b| i64::try_from(b).ok())
                            .unwrap_or(0)
                    }
                    "_address" => ev.address = v.as_bytes().unwrap_or_default(),
                    "_txHash" => ev.tx_hash = v.as_bytes().unwrap_or_else(|| vec![0u8; 32]),
                    _ => {
                        ev.params.insert(k.name.clone(), v);
                    }
                }
            }
        }

        let mut hframe = Frame {
            locals: HashMap::from([(handler.param.name.clone(), Value::Event(Box::new(ev)))]),
            returned: false,
            return_value: None,
        };
        self.exec_block(&handler.body, world, &mut hframe)?;
        flush(world);
        world.working.clear();
        Ok(())
    }

    /// Fire a source's block handlers with a synthesised `ethereum.Block`.
    fn fire_block(
        &self,
        source: &str,
        args: &[Expr],
        world: &mut World,
        frame: &mut Frame,
        span: &Span,
    ) -> R<()> {
        let mut ev = EventVal {
            params: BTreeMap::new(),
            address: self.source_addr.get(source).cloned().unwrap_or_default(),
            block_number: BigInt::from(0),
            block_timestamp: BigInt::from(0),
            tx_hash: vec![0u8; 32],
            log_index: 0,
        };
        if let Some(Expr::Record { fields, .. }) = args.first() {
            for (k, vexpr) in fields {
                let v = self.eval(vexpr, world, frame)?;
                match k.name.as_str() {
                    "_block" | "number" => ev.block_number = v.to_bigint().unwrap_or_default(),
                    "_timestamp" | "timestamp" => {
                        ev.block_timestamp = v.to_bigint().unwrap_or_default()
                    }
                    "_address" => ev.address = v.as_bytes().unwrap_or_default(),
                    _ => {}
                }
            }
        }
        let handlers = self.block_handlers.get(source).cloned().unwrap_or_default();
        if handlers.is_empty() {
            return err(
                format!("no block handler for `{source}`"),
                Some(span.clone()),
            );
        }
        for handler in handlers {
            let mut hframe = Frame {
                locals: HashMap::from([(
                    handler.param.name.clone(),
                    Value::EventBlock(Box::new(ev.clone())),
                )]),
                returned: false,
                return_value: None,
            };
            self.exec_block(&handler.body, world, &mut hframe)?;
            flush(world);
            world.working.clear();
        }
        Ok(())
    }

    /// Fire a file/IPFS handler with the file contents as its `Bytes` param.
    fn fire_file(
        &self,
        template: &str,
        args: &[Expr],
        world: &mut World,
        frame: &mut Frame,
        span: &Span,
    ) -> R<()> {
        let handler = self
            .file_handlers
            .get(template)
            .copied()
            .ok_or_else(|| TError {
                message: format!("no file handler for `{template}`"),
                span: Some(span.clone()),
            })?;
        let content = match args.first() {
            Some(e) => self.eval(e, world, frame)?,
            None => Value::Bytes(Vec::new()),
        };
        let mut hframe = Frame {
            locals: HashMap::from([(handler.param.name.clone(), content)]),
            returned: false,
            return_value: None,
        };
        self.exec_block(&handler.body, world, &mut hframe)?;
        flush(world);
        world.working.clear();
        Ok(())
    }

    /// Fire a call handler with a synthesised call object. Record keys populate
    /// `call.inputs`; `_out_<name>` keys populate `call.outputs`; `_block` /
    /// `_timestamp` / `_address` / `_txHash` set chain metadata.
    fn fire_call(
        &self,
        handler: &HandlerDecl,
        args: &[Expr],
        world: &mut World,
        frame: &mut Frame,
        span: &Span,
    ) -> R<()> {
        let mut call = CallVal {
            inputs: BTreeMap::new(),
            outputs: BTreeMap::new(),
            address: self
                .source_addr
                .get(&handler.source.name)
                .cloned()
                .unwrap_or_default(),
            block_number: BigInt::from(0),
            block_timestamp: BigInt::from(0),
            tx_hash: vec![0u8; 32],
        };
        if let Some(Expr::Record { fields, .. }) = args.first() {
            for (k, vexpr) in fields {
                let v = self.eval(vexpr, world, frame)?;
                match k.name.as_str() {
                    "_block" => call.block_number = v.to_bigint().unwrap_or_default(),
                    "_timestamp" => call.block_timestamp = v.to_bigint().unwrap_or_default(),
                    "_address" => call.address = v.as_bytes().unwrap_or_default(),
                    "_txHash" => call.tx_hash = v.as_bytes().unwrap_or_else(|| vec![0u8; 32]),
                    other => {
                        if let Some(out) = other.strip_prefix("_out_") {
                            call.outputs.insert(out.to_string(), v);
                        } else {
                            call.inputs.insert(other.to_string(), v);
                        }
                    }
                }
            }
        }
        let mut hframe = Frame {
            locals: HashMap::from([(handler.param.name.clone(), Value::Call(Box::new(call)))]),
            returned: false,
            return_value: None,
        };
        let _ = span;
        self.exec_block(&handler.body, world, &mut hframe)?;
        flush(world);
        world.working.clear();
        Ok(())
    }

    // ---- expression evaluation ----

    fn eval(&self, e: &Expr, world: &mut World, frame: &mut Frame) -> R<Value> {
        match e {
            Expr::Int { raw, .. } => Ok(raw
                .parse::<i64>()
                .map(Value::Int)
                .unwrap_or_else(|_| Value::Big(BigInt::from_str(raw).unwrap_or_default()))),
            Expr::Hex { raw, .. } => Ok(Value::Bytes(hex_to_bytes(raw).unwrap_or_default())),
            Expr::Decimal { raw, .. } => {
                Ok(Value::Dec(BigDecimal::from_str(raw).unwrap_or_default()))
            }
            Expr::Str { value, .. } => Ok(Value::Str(value.clone())),
            Expr::Bool { value, .. } => Ok(Value::Bool(*value)),
            Expr::Path { segments, span } => {
                if segments.len() == 1 {
                    if let Some(v) = frame.locals.get(&segments[0].name) {
                        return Ok(v.clone());
                    }
                }
                err(
                    format!("unknown identifier `{}`", path_str(segments)),
                    Some(span.clone()),
                )
            }
            Expr::Field { base, field, span } => {
                self.eval_field(base, &field.name, world, frame, span)
            }
            Expr::Call { callee, args, span } => self.eval_call(callee, args, world, frame, span),
            Expr::Unary { op, expr, .. } => {
                let v = self.eval(expr, world, frame)?;
                match op {
                    UnOp::Not => Ok(Value::Bool(!v.as_bool().unwrap_or(false))),
                    UnOp::Neg => match v.to_bigint() {
                        Some(b) => Ok(Value::Big(-b)),
                        None => err("cannot negate a non-number", Some(expr.span().clone())),
                    },
                }
            }
            Expr::Binary { op, lhs, rhs, span } => {
                self.eval_binary(*op, lhs, rhs, world, frame, span)
            }
            Expr::Array { elems, .. } => {
                let mut items = Vec::with_capacity(elems.len());
                for e in elems {
                    items.push(self.eval(e, world, frame)?);
                }
                Ok(Value::Array(items))
            }
            Expr::Index { base, index, span } => {
                let bv = self.eval(base, world, frame)?;
                let iv = self.eval(index, world, frame)?;
                let Value::Array(items) = bv else {
                    return err("can only index an array", Some(base.span().clone()));
                };
                let i = iv
                    .to_bigint()
                    .and_then(|b| usize::try_from(b).ok())
                    .ok_or_else(|| TError {
                        message: "array index must be a non-negative integer".into(),
                        span: Some(index.span().clone()),
                    })?;
                items.get(i).cloned().ok_or_else(|| TError {
                    message: format!("array index {i} out of bounds (len {})", items.len()),
                    span: Some(span.clone()),
                })
            }
            Expr::Record { span, .. } => err(
                "a record is only valid as a constructor or event argument",
                Some(span.clone()),
            ),
            Expr::Match { span, .. } => err(
                "`match` is only supported as a statement",
                Some(span.clone()),
            ),
        }
    }

    fn eval_field(
        &self,
        base: &Expr,
        field: &str,
        world: &mut World,
        frame: &mut Frame,
        span: &Span,
    ) -> R<Value> {
        // `BigInt.zero` / `BigDecimal.zero`.
        if field == "zero" {
            if let Some(name) = single_path(base) {
                if name == "BigInt" {
                    return Ok(Value::Big(BigInt::from(0)));
                }
                if name == "BigDecimal" {
                    return Ok(Value::Dec(BigDecimal::from(0)));
                }
            }
        }

        let bv = self.eval(base, world, frame)?;
        match bv {
            Value::Event(ev) => match field {
                "params" => Ok(Value::EventParams(ev)),
                "block" => Ok(Value::EventBlock(ev)),
                "transaction" => Ok(Value::EventTx(ev)),
                "address" => Ok(Value::Bytes(ev.address)),
                "logIndex" => Ok(Value::Int(ev.log_index)),
                "id" => Ok(Value::Bytes(make_id(&ev))),
                _ => err(format!("event has no field `{field}`"), Some(span.clone())),
            },
            Value::EventParams(ev) => ev.params.get(field).cloned().ok_or_else(|| TError {
                message: format!("event has no parameter `{field}`"),
                span: Some(span.clone()),
            }),
            Value::EventBlock(ev) => match field {
                "timestamp" => Ok(Value::Big(ev.block_timestamp)),
                "number" => Ok(Value::Big(ev.block_number)),
                "hash" => Ok(Value::Bytes(vec![0u8; 32])),
                _ => err(format!("block has no field `{field}`"), Some(span.clone())),
            },
            Value::EventTx(ev) => match field {
                "hash" => Ok(Value::Bytes(ev.tx_hash)),
                // The porting guide documents `.from`, `.to`, `.value` and
                // `.gasPrice` as available to a handler, and codegen lowers them,
                // so a handler that reads one has to be testable. They are
                // deterministic zeros here rather than derived from anything: a
                // test that cares about the sender should assert on an event
                // parameter, not on the mock transaction.
                "from" | "to" => Ok(Value::Bytes(vec![0u8; 20])),
                "value" | "gasPrice" | "gasLimit" | "gasUsed" | "nonce" => {
                    Ok(Value::Big(BigInt::from(0)))
                }
                "index" => Ok(Value::Int(0)),
                _ => err(
                    format!("transaction has no field `{field}`"),
                    Some(span.clone()),
                ),
            },
            Value::Call(c) => match field {
                "inputs" => Ok(Value::CallInputs(c)),
                "outputs" => Ok(Value::CallOutputs(c)),
                "block" => Ok(Value::EventBlock(Box::new(c.meta()))),
                "transaction" => Ok(Value::EventTx(Box::new(c.meta()))),
                "to" | "from" | "address" => Ok(Value::Bytes(c.address)),
                _ => err(format!("call has no field `{field}`"), Some(span.clone())),
            },
            Value::CallInputs(c) => c.inputs.get(field).cloned().ok_or_else(|| TError {
                message: format!("call has no input `{field}`"),
                span: Some(span.clone()),
            }),
            Value::CallOutputs(c) => c.outputs.get(field).cloned().ok_or_else(|| TError {
                message: format!("call has no output `{field}`"),
                span: Some(span.clone()),
            }),
            // `.length` on a string or a byte string, the same field graph-ts
            // exposes. Without it a hex parser cannot even be evaluated.
            Value::Str(ref t) if field == "length" => Ok(Value::Int(t.chars().count() as i64)),
            Value::Bytes(ref b) if field == "length" => Ok(Value::Int(b.len() as i64)),
            Value::Array(ref items) => match field {
                "length" => Ok(Value::Int(items.len() as i64)),
                _ => err(format!("array has no field `{field}`"), Some(span.clone())),
            },
            Value::Handle(h) => Ok(world.working[h]
                .fields
                .get(field)
                .cloned()
                .unwrap_or(Value::Null)),
            Value::Stored(_, fields) => Ok(fields.get(field).cloned().unwrap_or(Value::Null)),
            Value::Result { reverted, value } => match field {
                "reverted" => Ok(Value::Bool(reverted)),
                "value" => Ok(*value),
                _ => err(format!("result has no field `{field}`"), Some(span.clone())),
            },
            other => err(
                format!("value `{}` has no field `{field}`", other.canonical()),
                Some(span.clone()),
            ),
        }
    }

    fn eval_call(
        &self,
        callee: &Expr,
        args: &[Expr],
        world: &mut World,
        frame: &mut Frame,
        span: &Span,
    ) -> R<Value> {
        // A bare call to a user helper function.
        if let Some(name) = single_path(callee) {
            if let Some(func) = self.fns.get(&name).copied() {
                return self.invoke_fn(func, args, world, frame, span);
            }
        }
        if let Expr::Field { base, field, .. } = callee {
            // `log.debug(...)` and friends. graph-node's log namespace has no
            // return value and no observable effect on the store, so the
            // interpreter evaluates the arguments (they can still fail) and
            // yields Unit. `log.critical` is the exception: in graph-node it
            // terminates the handler, so it is modelled as a test failure rather
            // than quietly ignored.
            if single_path(base).as_deref() == Some("log") {
                let level = field.name.clone();
                if matches!(
                    level.as_str(),
                    "debug" | "info" | "warning" | "error" | "critical"
                ) {
                    for a in args {
                        self.eval(a, world, frame)?;
                    }
                    if level == "critical" {
                        return err(
                            "handler aborted: log.critical".to_string(),
                            Some(span.clone()),
                        );
                    }
                    return Ok(Value::Unit);
                }
            }

            // Static constructors. Without these the interpreter can evaluate a
            // handler but not a helper that builds a number, which is most of
            // them - every subgraph writes `BigInt.fromI32(0)` somewhere.
            if let Some(ns) = single_path(base) {
                let ctor = field.name.as_str();
                if ns == "BigInt" && ctor.starts_with("from") {
                    let a = args
                        .first()
                        .map(|a| self.eval(a, world, frame))
                        .transpose()?;
                    return match ctor {
                        "fromString" => a
                            .as_ref()
                            .map(|v| v.canonical())
                            .and_then(|t| BigInt::from_str(t.trim()).ok())
                            .map(Value::Big)
                            .ok_or_else(|| TError {
                                message: "BigInt.fromString: not an integer".into(),
                                span: Some(span.clone()),
                            }),
                        "fromByteArray" | "fromUnsignedBytes" | "fromSignedBytes" => a
                            .as_ref()
                            .and_then(|v| v.as_bytes())
                            .map(|b| Value::Big(BigInt::from_bytes_be(num_bigint::Sign::Plus, &b)))
                            .ok_or_else(|| TError {
                                message: format!("BigInt.{ctor}: not bytes"),
                                span: Some(span.clone()),
                            }),
                        _ => a
                            .as_ref()
                            .and_then(|v| v.to_bigint())
                            .map(Value::Big)
                            .ok_or_else(|| TError {
                                message: format!("BigInt.{ctor}: not a number"),
                                span: Some(span.clone()),
                            }),
                    };
                }
                if ns == "BigDecimal" && ctor.starts_with("from") {
                    let a = args
                        .first()
                        .map(|a| self.eval(a, world, frame))
                        .transpose()?;
                    return a
                        .as_ref()
                        .and_then(|v| {
                            BigDecimal::from_str(v.canonical().trim())
                                .ok()
                                .or_else(|| v.to_bigdecimal())
                        })
                        .map(Value::Dec)
                        .ok_or_else(|| TError {
                            message: "BigDecimal.fromString: not a decimal".into(),
                            span: Some(span.clone()),
                        });
                }
                if (ns == "Bytes" || ns == "Address") && ctor.starts_with("from") {
                    let a = args
                        .first()
                        .map(|a| self.eval(a, world, frame))
                        .transpose()?;
                    if let Some(v) = a {
                        if let Some(b) = v.as_bytes() {
                            return Ok(Value::Bytes(b));
                        }
                        return Ok(v);
                    }
                }
            }

            // `Abi.bind(addr)` -> a bound contract. (base is a namespace path)
            if field.name == "bind" {
                if let Some(name) = single_path(base) {
                    if self.abis.contains(&name) {
                        return Ok(Value::Contract(name));
                    }
                }
            }

            // `DataSourceContext.new()` -> an opaque context bag (test stub).
            if field.name == "new" && single_path(base).as_deref() == Some("DataSourceContext") {
                return Ok(Value::Unit);
            }

            // `<Template>.create(addr)` / `.createWithContext(addr, ctx)` spawns a
            // dynamic data source — record it; it is NOT an entity write.
            if let Some(name) = single_path(base) {
                if self.templates.contains(&name)
                    && matches!(field.name.as_str(), "create" | "createWithContext")
                {
                    let id = self.eval_id(args, world, frame, span)?;
                    world.created.push((name, id));
                    return Ok(Value::Unit);
                }
            }

            // Entity constructors / accessors. The base (`Account`,
            // `accounts::Account`) is a namespace, NOT a value — match it before
            // evaluating, or `Account` looks like an unknown identifier.
            if let Some(entity) = entity_of(base) {
                match field.name.as_str() {
                    "loadOrCreate" => {
                        return self.load_or_create(&entity, args, world, frame, true, span)
                    }
                    "create" => return self.create_entity(&entity, args, world, frame, span),
                    "load" | "loadInBlock" => {
                        return self.load_entity(&entity, args, world, frame, span)
                    }
                    "at" => return self.at_entity(&entity, args, world, frame, span),
                    _ => {}
                }
            }

            // Otherwise the base is a value.
            let bv = self.eval(base, world, frame)?;

            // Contract method call -> look up the mock.
            if let Value::Contract(_abi) = &bv {
                let mut parts = Vec::new();
                for a in args {
                    parts.push(self.eval(a, world, frame)?.canonical());
                }
                let key = format!("{}({})", field.name, parts.join(","));
                return match world.mocks.get(&key) {
                    Some(Mock::Return(v)) => Ok(Value::Result {
                        reverted: false,
                        value: Box::new(v.clone()),
                    }),
                    Some(Mock::Revert) => Ok(Value::Result {
                        reverted: true,
                        value: Box::new(Value::Null),
                    }),
                    None => err(
                        format!("unmocked contract call `{key}` — add `mockCall({key}, …)`"),
                        Some(span.clone()),
                    ),
                };
            }

            // `DataSourceContext` setters on the opaque context stub are no-ops.
            if matches!(bv, Value::Unit) && field.name.starts_with("set") {
                return Ok(Value::Unit);
            }

            // Value methods.
            match field.name.as_str() {
                "toDecimal" | "toBigDecimal" => {
                    return bv.to_bigdecimal().map(Value::Dec).ok_or_else(|| TError {
                        message: "cannot convert to BigDecimal".into(),
                        span: Some(span.clone()),
                    });
                }
                "toI64" | "toI32" | "toU64" | "toU32" | "toBigInt" => {
                    if let Some(b) = bv.to_bigint() {
                        return Ok(Value::Big(b));
                    }
                }
                "toHexString" | "toHex" => {
                    if let Value::Bytes(b) = &bv {
                        return Ok(Value::Str(format!("0x{}", hex(b))));
                    }
                    return Ok(Value::Str(bv.canonical()));
                }
                "toString" => return Ok(Value::Str(bv.canonical())),
                "abs" => {
                    if let Some(b) = bv.to_bigint() {
                        return Ok(Value::Big(b.magnitude().clone().into()));
                    }
                }
                // String methods. `startsWith`/`slice`/`charAt` are what a hex
                // parser needs, and every ported subgraph has one.
                "startsWith" | "endsWith" | "includes" | "slice" | "charAt" | "concat"
                | "split" | "toLowerCase" | "toUpperCase" => {
                    if let Value::Str(hay) = &bv {
                        let a0 = args
                            .first()
                            .map(|a| self.eval(a, world, frame))
                            .transpose()?;
                        let a1 = args
                            .get(1)
                            .map(|a| self.eval(a, world, frame))
                            .transpose()?;
                        let s0 = a0.as_ref().map(|v| v.canonical()).unwrap_or_default();
                        return Ok(match field.name.as_str() {
                            "startsWith" => Value::Bool(hay.starts_with(&s0)),
                            "endsWith" => Value::Bool(hay.ends_with(&s0)),
                            "includes" => Value::Bool(hay.contains(&s0)),
                            "toLowerCase" => Value::Str(hay.to_lowercase()),
                            "toUpperCase" => Value::Str(hay.to_uppercase()),
                            "concat" => Value::Str(format!("{hay}{s0}")),
                            "split" => Value::Array(
                                hay.split(&s0 as &str)
                                    .map(|p| Value::Str(p.to_string()))
                                    .collect(),
                            ),
                            "charAt" => {
                                let i = a0
                                    .and_then(|v| v.to_bigint())
                                    .and_then(|b| usize::try_from(b).ok())
                                    .unwrap_or(0);
                                Value::Str(hay.chars().nth(i).map(String::from).unwrap_or_default())
                            }
                            _ => {
                                let chars: Vec<char> = hay.chars().collect();
                                let from = a0
                                    .and_then(|v| v.to_bigint())
                                    .and_then(|b| usize::try_from(b).ok())
                                    .unwrap_or(0)
                                    .min(chars.len());
                                let to = a1
                                    .and_then(|v| v.to_bigint())
                                    .and_then(|b| usize::try_from(b).ok())
                                    .unwrap_or(chars.len())
                                    .clamp(from, chars.len());
                                Value::Str(chars[from..to].iter().collect())
                            }
                        });
                    }
                }
                "push" => {
                    let a0 = args
                        .first()
                        .map(|a| self.eval(a, world, frame))
                        .transpose()?;
                    if let (Value::Array(mut items), Some(v)) = (bv.clone(), a0) {
                        items.push(v);
                        return Ok(Value::Array(items));
                    }
                }
                "pow" => {
                    let e = args
                        .first()
                        .map(|a| self.eval(a, world, frame))
                        .transpose()?
                        .and_then(|v| v.to_bigint())
                        .and_then(|b| u32::try_from(b).ok())
                        .unwrap_or(0);
                    if let Some(a) = bv.to_bigint() {
                        return Ok(Value::Big(a.pow(e)));
                    }
                }
                "neg" => {
                    if let Value::Dec(d) = &bv {
                        return Ok(Value::Dec(-d.clone()));
                    }
                    if let Some(a) = bv.to_bigint() {
                        return Ok(Value::Big(-a));
                    }
                }
                "equals" | "notEqual" | "lt" | "gt" | "le" | "ge" => {
                    let rhs = args
                        .first()
                        .map(|a| self.eval(a, world, frame))
                        .transpose()?;
                    if let Some(r) = rhs {
                        // Compare as decimals when either side is one, so a
                        // BigInt/BigDecimal mix does not silently truncate.
                        let ord = if matches!(bv, Value::Dec(_)) || matches!(r, Value::Dec(_)) {
                            match (bv.to_bigdecimal(), r.to_bigdecimal()) {
                                (Some(a), Some(b)) => a.cmp(&b),
                                _ => return err("cannot compare", Some(span.clone())),
                            }
                        } else {
                            match (bv.to_bigint(), r.to_bigint()) {
                                (Some(a), Some(b)) => a.cmp(&b),
                                _ => return err("cannot compare", Some(span.clone())),
                            }
                        };
                        return Ok(Value::Bool(match field.name.as_str() {
                            "equals" => ord == std::cmp::Ordering::Equal,
                            "notEqual" => ord != std::cmp::Ordering::Equal,
                            "lt" => ord == std::cmp::Ordering::Less,
                            "gt" => ord == std::cmp::Ordering::Greater,
                            "le" => ord != std::cmp::Ordering::Greater,
                            _ => ord != std::cmp::Ordering::Less,
                        }));
                    }
                }
                "leftShift" | "rightShift" => {
                    let n = args
                        .first()
                        .map(|a| self.eval(a, world, frame))
                        .transpose()?
                        .and_then(|v| v.to_bigint())
                        .and_then(|b| u32::try_from(b).ok())
                        .unwrap_or(0);
                    if let Some(a) = bv.to_bigint() {
                        return Ok(Value::Big(if field.name == "leftShift" {
                            a << n
                        } else {
                            a >> n
                        }));
                    }
                }
                "plus" | "minus" | "times" | "div" | "mod" | "divDecimal" => {
                    let rhs = args
                        .first()
                        .map(|a| self.eval(a, world, frame))
                        .transpose()?;
                    // BigDecimal arithmetic went missing here, so any helper
                    // doing decimal maths was untestable.
                    let decimal = field.name == "divDecimal"
                        || matches!(bv, Value::Dec(_))
                        || matches!(rhs, Some(Value::Dec(_)));
                    if decimal {
                        if let (Some(a), Some(b)) = (
                            bv.to_bigdecimal(),
                            rhs.as_ref().and_then(|r| r.to_bigdecimal()),
                        ) {
                            return Ok(Value::Dec(match field.name.as_str() {
                                "plus" => a + b,
                                "minus" => a - b,
                                "times" => a * b,
                                _ => {
                                    if b.is_zero() {
                                        return err("division by zero", Some(span.clone()));
                                    }
                                    a / b
                                }
                            }));
                        }
                    }
                    if let (Some(a), Some(b)) = (bv.to_bigint(), rhs.and_then(|r| r.to_bigint())) {
                        return Ok(Value::Big(match field.name.as_str() {
                            "plus" => a + b,
                            "minus" => a - b,
                            "times" => a * b,
                            "mod" => {
                                if b == BigInt::from(0) {
                                    return err("modulo by zero", Some(span.clone()));
                                }
                                a % b
                            }
                            _ => {
                                if b == BigInt::from(0) {
                                    return err("division by zero", Some(span.clone()));
                                } else {
                                    a / b
                                }
                            }
                        }));
                    }
                }
                _ => {}
            }
            return err(
                format!("unsupported method `{}`", field.name),
                Some(span.clone()),
            );
        }
        err("unsupported call", Some(span.clone()))
    }

    /// Invoke a helper `fn`: bind params, run the body in its own frame. Working
    /// entities live in the `World`, so a returned handle stays valid in the
    /// caller and nested helpers share each other's in-block writes.
    fn invoke_fn(
        &self,
        func: &redstart_parser::ast::FnDecl,
        args: &[Expr],
        world: &mut World,
        frame: &mut Frame,
        span: &Span,
    ) -> R<Value> {
        if func.params.len() != args.len() {
            return err(
                format!(
                    "`{}` expects {} argument(s), got {}",
                    func.name.name,
                    func.params.len(),
                    args.len()
                ),
                Some(span.clone()),
            );
        }
        let mut locals = HashMap::new();
        for (p, a) in func.params.iter().zip(args) {
            let v = self.eval(a, world, frame)?;
            locals.insert(p.name.name.clone(), v);
        }
        let mut fnframe = Frame {
            locals,
            returned: false,
            return_value: None,
        };
        self.exec_block(&func.body, world, &mut fnframe)?;
        Ok(fnframe.return_value.unwrap_or(Value::Unit))
    }

    fn handle_for(&self, entity: &str, id: &[u8], world: &World) -> Option<usize> {
        world
            .working
            .iter()
            .position(|w| w.entity == entity && w.id == id)
    }

    fn load_or_create(
        &self,
        entity: &str,
        args: &[Expr],
        world: &mut World,
        frame: &mut Frame,
        _create: bool,
        span: &Span,
    ) -> R<Value> {
        let id = self.eval_id(args, world, frame, span)?;
        if let Some(h) = self.handle_for(entity, &id, world) {
            return Ok(Value::Handle(h));
        }
        if let Some(fields) = world.store.get(&(entity.to_string(), id.clone())) {
            world.working.push(WorkingEntity {
                entity: entity.to_string(),
                id,
                fields: fields.clone(),
                dirty: false,
            });
            return Ok(Value::Handle(world.working.len() - 1));
        }
        // Create with the provided initializers.
        let fields = self.record_fields(args.get(1), world, frame, &id)?;
        world.working.push(WorkingEntity {
            entity: entity.to_string(),
            id,
            fields,
            dirty: true,
        });
        Ok(Value::Handle(world.working.len() - 1))
    }

    fn create_entity(
        &self,
        entity: &str,
        args: &[Expr],
        world: &mut World,
        frame: &mut Frame,
        span: &Span,
    ) -> R<Value> {
        let id = self.eval_id(args, world, frame, span)?;
        let fields = self.record_fields(args.get(1), world, frame, &id)?;
        world.working.push(WorkingEntity {
            entity: entity.to_string(),
            id,
            fields,
            dirty: true,
        });
        Ok(Value::Handle(world.working.len() - 1))
    }

    fn load_entity(
        &self,
        entity: &str,
        args: &[Expr],
        world: &mut World,
        frame: &mut Frame,
        span: &Span,
    ) -> R<Value> {
        let id = self.eval_id(args, world, frame, span)?;
        if let Some(h) = self.handle_for(entity, &id, world) {
            return Ok(Value::Handle(h));
        }
        if let Some(fields) = world.store.get(&(entity.to_string(), id.clone())) {
            world.working.push(WorkingEntity {
                entity: entity.to_string(),
                id,
                fields: fields.clone(),
                dirty: false,
            });
            Ok(Value::Handle(world.working.len() - 1))
        } else {
            Ok(Value::Null)
        }
    }

    fn do_assert_missing(
        &self,
        args: &[Expr],
        world: &mut World,
        frame: &mut Frame,
        span: &Span,
    ) -> R<()> {
        let entity = args.first().and_then(single_path).ok_or_else(|| TError {
            message: "assertMissing needs an entity name and an id".into(),
            span: Some(span.clone()),
        })?;
        let id = self.eval_id(&args[1..], world, frame, span)?;
        if world.store.contains_key(&(entity.clone(), id.clone())) {
            return err(
                format!(
                    "assertMissing failed: `{entity}` with id 0x{} exists",
                    hex(&id)
                ),
                Some(span.clone()),
            );
        }
        Ok(())
    }

    fn at_entity(
        &self,
        entity: &str,
        args: &[Expr],
        world: &mut World,
        frame: &mut Frame,
        span: &Span,
    ) -> R<Value> {
        let id = self.eval_id(args, world, frame, span)?;
        match world.store.get(&(entity.to_string(), id.clone())) {
            Some(fields) => Ok(Value::Stored(entity.to_string(), fields.clone())),
            None => err(
                format!("no `{entity}` with id 0x{}", hex(&id)),
                Some(span.clone()),
            ),
        }
    }

    fn eval_id(
        &self,
        args: &[Expr],
        world: &mut World,
        frame: &mut Frame,
        span: &Span,
    ) -> R<Vec<u8>> {
        let id_expr = args.first().ok_or_else(|| TError {
            message: "missing id argument".into(),
            span: Some(span.clone()),
        })?;
        let v = self.eval(id_expr, world, frame)?;
        // Entity ids may be Bytes/Address, a String (composite ids), or an
        // integer (Int8 timeseries ids) — all reduce to a stable byte key.
        match &v {
            Value::Bytes(b) => Ok(b.clone()),
            Value::Str(s) => Ok(s.as_bytes().to_vec()),
            _ => v
                .to_bigint()
                .map(|b| b.to_string().into_bytes())
                .ok_or_else(|| TError {
                    message: "entity id must be Bytes/Address, String, or an integer".into(),
                    span: Some(id_expr.span().clone()),
                }),
        }
    }

    fn record_fields(
        &self,
        arg: Option<&Expr>,
        world: &mut World,
        frame: &mut Frame,
        id: &[u8],
    ) -> R<BTreeMap<String, Value>> {
        let mut fields = BTreeMap::new();
        fields.insert("id".to_string(), Value::Bytes(id.to_vec()));
        if let Some(Expr::Record { fields: rec, .. }) = arg {
            for (k, vexpr) in rec {
                let v = self.eval(vexpr, world, frame)?;
                // Entity-reference fields store the referenced id.
                let stored = match v {
                    Value::Handle(h) => Value::Bytes(world.working[h].id.clone()),
                    other => other,
                };
                fields.insert(k.name.clone(), stored);
            }
        }
        Ok(fields)
    }

    fn eval_binary(
        &self,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
        world: &mut World,
        frame: &mut Frame,
        span: &Span,
    ) -> R<Value> {
        // Short-circuit logical operators.
        if matches!(op, BinOp::And | BinOp::Or) {
            let l = self.eval(lhs, world, frame)?.as_bool().unwrap_or(false);
            if op == BinOp::And && !l {
                return Ok(Value::Bool(false));
            }
            if op == BinOp::Or && l {
                return Ok(Value::Bool(true));
            }
            return Ok(Value::Bool(
                self.eval(rhs, world, frame)?.as_bool().unwrap_or(false),
            ));
        }

        let l = self.eval(lhs, world, frame)?;
        let r = self.eval(rhs, world, frame)?;

        // Equality.
        if op == BinOp::Eq {
            return Ok(Value::Bool(value_eq(&l, &r)));
        }
        if op == BinOp::Ne {
            return Ok(Value::Bool(!value_eq(&l, &r)));
        }

        // String concatenation (`id = a.toHexString() + "-" + b…`).
        if op == BinOp::Add && (matches!(l, Value::Str(_)) || matches!(r, Value::Str(_))) {
            return Ok(Value::Str(format!("{}{}", l.canonical(), r.canonical())));
        }

        // Numeric: prefer BigDecimal if either side is decimal, else BigInt.
        let use_dec = matches!(l, Value::Dec(_)) || matches!(r, Value::Dec(_));
        if use_dec {
            let (a, b) = (l.to_bigdecimal(), r.to_bigdecimal());
            if let (Some(a), Some(b)) = (a, b) {
                return dec_op(op, a, b, span);
            }
        } else if let (Some(a), Some(b)) = (l.to_bigint(), r.to_bigint()) {
            return int_op(op, a, b, span);
        }

        err("operator needs numeric operands", Some(span.clone()))
    }
}

// ---- numeric helpers ----

fn int_op(op: BinOp, a: BigInt, b: BigInt, span: &Span) -> R<Value> {
    Ok(match op {
        BinOp::Add => Value::Big(a + b),
        BinOp::Sub => Value::Big(a - b),
        BinOp::Mul => Value::Big(a * b),
        BinOp::Div => {
            if b == BigInt::from(0) {
                return err("division by zero", Some(span.clone()));
            }
            Value::Big(a / b)
        }
        BinOp::Rem => Value::Big(a % b),
        BinOp::Lt => Value::Bool(a < b),
        BinOp::Le => Value::Bool(a <= b),
        BinOp::Gt => Value::Bool(a > b),
        BinOp::Ge => Value::Bool(a >= b),
        _ => return err("unsupported integer operator", Some(span.clone())),
    })
}

fn dec_op(op: BinOp, a: BigDecimal, b: BigDecimal, span: &Span) -> R<Value> {
    Ok(match op {
        BinOp::Add => Value::Dec(a + b),
        BinOp::Sub => Value::Dec(a - b),
        BinOp::Mul => Value::Dec(a * b),
        BinOp::Div => Value::Dec(a / b),
        BinOp::Lt => Value::Bool(a < b),
        BinOp::Le => Value::Bool(a <= b),
        BinOp::Gt => Value::Bool(a > b),
        BinOp::Ge => Value::Bool(a >= b),
        _ => return err("unsupported decimal operator", Some(span.clone())),
    })
}

// ---- misc helpers ----

fn flush(world: &mut World) {
    for w in &world.working {
        if w.dirty {
            world
                .store
                .insert((w.entity.clone(), w.id.clone()), w.fields.clone());
        }
    }
}

fn make_id(ev: &EventVal) -> Vec<u8> {
    let mut id = ev.tx_hash.clone();
    id.extend_from_slice(&(ev.log_index as i32).to_le_bytes());
    id
}

/// The last segment of a path used as an entity name (`accounts::Account` -> `Account`).
fn entity_of(base: &Expr) -> Option<String> {
    if let Expr::Path { segments, .. } = base {
        segments.last().map(|s| s.name.clone())
    } else {
        None
    }
}

fn single_path(e: &Expr) -> Option<String> {
    if let Expr::Path { segments, .. } = e {
        if segments.len() == 1 {
            return Some(segments[0].name.clone());
        }
    }
    None
}

fn path_name(e: &Expr) -> Option<String> {
    single_path(e)
}

fn path_str(segments: &[redstart_parser::Ident]) -> String {
    segments
        .iter()
        .map(|s| s.name.clone())
        .collect::<Vec<_>>()
        .join("::")
}

fn setting_addr(settings: &[redstart_parser::ast::Setting]) -> Option<Vec<u8>> {
    settings
        .iter()
        .find(|s| s.key.name == "address")
        .and_then(|s| {
            if let Expr::Hex { raw, .. } = &s.value {
                hex_to_bytes(raw)
            } else {
                None
            }
        })
}

fn hex_to_bytes(s: &str) -> Option<Vec<u8>> {
    let s = s.strip_prefix("0x")?;
    let s = if s.len() % 2 == 1 {
        format!("0{s}")
    } else {
        s.to_string()
    };
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests;
