//! Semantic analysis for Redstart.
//!
//! [`check`] runs over a loaded [`ModuleTree`] and enforces the guarantees the
//! design report makes load-bearing — "impossible states unrepresentable" — as
//! actual compile errors, spanning every module:
//!
//! - handlers must bind to a real source and a real ABI event;
//! - sources must declare their required settings and a known ABI;
//! - entity field types must resolve; `derived from` must name a real back-ref;
//! - `loadOrCreate`/`create` must initialise every required field;
//! - you cannot assign to a `derived` field;
//! - you cannot read a contract call's `.value` without `match`ing it first
//!   (contract calls can revert);
//! - you cannot do arithmetic on an `Option` without unwrapping it (the
//!   nullable-arithmetic miscompile);
//! - you cannot call non-deterministic host functions (`Date.now`,
//!   `Math.random`, …) — they diverge Proof-of-Indexing and get indexers slashed.
//!
//! On success it returns a [`Checked`] symbol table that codegen consumes, so
//! the resolved types are computed once and shared.

#![forbid(unsafe_code)]

pub mod abi;
mod diag;
pub mod explain;
pub mod ty;

pub use abi::{resolve_abi_path, AbiIndex, EventParam};
pub use diag::Diag;
pub use explain::Explanation;
use redstart_loader::ModuleTree;
use redstart_parser::ast::{
    EntityDecl, Expr, FieldDecl, ForIter, HandlerDecl, HandlerKind, MatchArm, Pattern, Setting,
    SourceDecl, Stmt, TemplateDecl, TypeExpr,
};
use std::collections::{HashMap, HashSet};
pub use ty::{is_scalar, resolve_type, sol_to_rty, EntityInfo, RTy};

/// The validated symbol table produced by a successful [`check`].
pub struct Checked {
    /// Entity name -> resolved field types.
    pub entities: HashMap<String, EntityInfo>,
    /// Source/template name -> ABI name.
    pub source_abi: HashMap<String, String>,
    /// ABI access (resolved file paths, event/function lookups).
    pub abis: AbiIndex,
    /// Entities that are provably append-only — created but never loaded or
    /// mutated anywhere — so codegen can emit `@entity(immutable: true)`. Immutable
    /// entities index faster and use less disk (Edge & Node: up to 19% / 48%).
    pub immutable_inferred: HashSet<String>,
}

/// Run semantic analysis. On error, returns rendered diagnostics (one string
/// per problem), ready to print.
///
/// # Errors
/// Returns the rendered diagnostics if any check fails.
pub fn check(tree: &ModuleTree) -> Result<Checked, Vec<String>> {
    check_with_abis(tree, &HashMap::new())
}

/// Like [`check`], but with ABIs supplied in-memory (name -> JSON) rather than
/// read from disk. Used by the WASM playground, which has no filesystem.
///
/// # Errors
/// Returns the rendered diagnostics if any check fails.
pub fn check_with_abis(
    tree: &ModuleTree,
    abi_texts: &HashMap<String, String>,
) -> Result<Checked, Vec<String>> {
    let (checked, diags, _) = analyze(tree, abi_texts);
    // Warnings (lints) are reported elsewhere but never block the build.
    let errors: Vec<String> = diags
        .iter()
        .filter(|d| d.is_error())
        .map(Diag::render)
        .collect();
    if errors.is_empty() {
        Ok(checked)
    } else {
        Err(errors)
    }
}

/// Run semantic analysis, returning structured diagnostics (empty if clean).
/// Used by the language server to publish editor squiggles.
#[must_use]
pub fn check_diags(tree: &ModuleTree) -> Vec<Diag> {
    analyze(tree, &HashMap::new()).1
}

fn analyze(
    tree: &ModuleTree,
    abi_texts: &HashMap<String, String>,
) -> (Checked, Vec<Diag>, Vec<IdSite>) {
    let mut diags: Vec<Diag> = Vec::new();
    // Convertible/blocking id-construction sites, gathered as a side-channel for
    // `plan_id_rewrites`; ignored on the normal check paths.
    let mut sites: Vec<IdSite> = Vec::new();

    // ---- gather modules with their filenames ----
    let modules = tree.ordered();
    let files: Vec<String> = modules
        .iter()
        .map(|m| m.file_path.display().to_string())
        .collect();

    // ---- global symbol build ----
    let mut abis = AbiIndex::default();
    for m in &modules {
        let dir = m
            .file_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        for a in &m.program.abis {
            abis.insert(a.name.name.clone(), resolve_abi_path(dir, &a.path));
        }
    }
    // In-memory ABIs (the WASM playground) take precedence over disk paths.
    for (name, json) in abi_texts {
        abis.insert_text(name.clone(), json.clone());
    }

    // Entity names (first pass), checking duplicates.
    let mut entities: HashMap<String, EntityInfo> = HashMap::new();
    let mut entity_meta: HashMap<String, EntityMeta> = HashMap::new();
    let mut seen: HashMap<String, ()> = HashMap::new();
    for (m, file) in modules.iter().zip(&files) {
        for e in &m.program.entities {
            if seen.insert(e.name.name.clone(), ()).is_some() {
                diags.push(
                    Diag::new(
                        file,
                        &e.name.span,
                        "E010",
                        format!("duplicate entity `{}`", e.name.name),
                        "already declared",
                    )
                    .with_help("each entity must be declared exactly once across all modules"),
                );
            }
            entities.insert(e.name.name.clone(), EntityInfo::default());
            entity_meta.insert(e.name.name.clone(), EntityMeta::from_decl(e));
        }
    }
    // Second pass: resolve field types now that all names are known.
    for m in &modules {
        for e in &m.program.entities {
            let fields = e
                .fields
                .iter()
                .map(|f| (f.name.name.clone(), resolve_type(&f.ty, &entities)))
                .collect();
            entities.insert(e.name.name.clone(), EntityInfo { fields });
        }
    }

    // Source / template tables.
    let mut source_abi: HashMap<String, String> = HashMap::new();
    let mut source_network: HashMap<String, String> = HashMap::new();
    let mut data_source_names: HashMap<String, ()> = HashMap::new();
    for m in &modules {
        for s in &m.program.sources {
            data_source_names.insert(s.name.name.clone(), ());
            if let Some(a) = path_setting(&s.settings, "abi") {
                source_abi.insert(s.name.name.clone(), a);
            }
            if let Some(n) = string_setting(&s.settings, "network") {
                source_network.insert(s.name.name.clone(), n);
            }
        }
        for t in &m.program.templates {
            data_source_names.insert(t.name.name.clone(), ());
            if let Some(a) = path_setting(&t.settings, "abi") {
                source_abi.insert(t.name.name.clone(), a);
            }
        }
    }

    // Entities whose id is a `String` (`Id<String>` or bare `String`) — the only
    // ones `--rewrite-ids` (`plan_id_rewrites`) can steer toward a `Bytes` id.
    let id_string_entities: HashSet<String> = entities
        .iter()
        .filter(|(_, info)| matches!(info.fields.get("id"), Some(RTy::String)))
        .map(|(name, _)| name.clone())
        .collect();

    // ---- per-declaration validation ----
    let entity_names: Vec<String> = entities.keys().cloned().collect();
    let enum_names: Vec<String> = modules
        .iter()
        .flat_map(|m| m.program.enums.iter().map(|e| e.name.name.clone()))
        .collect();
    // Interface name -> its declared field names (for `implements` checking).
    let interfaces: HashMap<String, Vec<String>> = modules
        .iter()
        .flat_map(|m| &m.program.interfaces)
        .map(|i| {
            (
                i.name.name.clone(),
                i.fields.iter().map(|f| f.name.name.clone()).collect(),
            )
        })
        .collect();
    // Interface and enum names are both valid field types.
    let mut aux_types = enum_names.clone();
    aux_types.extend(interfaces.keys().cloned());

    // Free-function return types, for typing helper calls in bodies.
    let fn_returns: HashMap<String, RTy> = modules
        .iter()
        .flat_map(|m| &m.program.functions)
        .filter_map(|f| {
            f.ret
                .as_ref()
                .map(|t| (f.name.name.clone(), resolve_type(t, &entities)))
        })
        .collect();
    // Every declared free function, by name — including the ones with no return
    // type, which `fn_returns` drops. This is the resolution set for a bare call.
    let fn_names: HashSet<String> = modules
        .iter()
        .flat_map(|m| &m.program.functions)
        .map(|f| f.name.name.clone())
        .collect();

    for (m, file) in modules.iter().zip(&files) {
        for i in &m.program.interfaces {
            for f in &i.fields {
                validate_type(&f.ty, &entity_names, &aux_types, file, &mut diags);
            }
        }
        for e in &m.program.entities {
            check_entity(e, &entity_names, &aux_types, &entity_meta, file, &mut diags);
            check_implements(e, &interfaces, file, &mut diags);
        }
        for agg in &m.program.aggregations {
            if !entity_names.iter().any(|n| n == &agg.source.name) {
                diags.push(
                    Diag::new(
                        file,
                        &agg.source.span,
                        "E005",
                        format!(
                            "aggregation `{}` sources unknown entity `{}`",
                            agg.name.name, agg.source.name
                        ),
                        "no such entity",
                    )
                    .with_help("`over <Entity>` must name a `timeseries` entity"),
                );
            }
            for f in &agg.fields {
                validate_type(&f.ty, &entity_names, &aux_types, file, &mut diags);
            }
        }
        for s in &m.program.sources {
            check_source(s, &abis, file, &mut diags);
        }
        for t in &m.program.templates {
            check_template(t, &abis, file, &mut diags);
        }
        for h in &m.program.handlers {
            check_handler(
                h,
                &entities,
                &entity_meta,
                &source_abi,
                &source_network,
                &data_source_names,
                &fn_returns,
                &fn_names,
                &mut abis,
                file,
                &mut diags,
                &id_string_entities,
                &mut sites,
            );
        }
        for func in &m.program.functions {
            check_fn(
                func,
                &entities,
                &entity_meta,
                &fn_returns,
                &fn_names,
                &abis,
                file,
                &mut diags,
                &id_string_entities,
                &mut sites,
            );
        }
    }

    let immutable_inferred = collect_immutable_entities(tree);
    (
        Checked {
            entities,
            source_abi,
            abis,
            immutable_inferred,
        },
        diags,
        sites,
    )
}

// ---- entity metadata (from AST) ----

struct EntityMeta {
    fields: Vec<FieldMeta>,
}

struct FieldMeta {
    name: String,
    is_optional: bool,
    is_derived: bool,
}

impl EntityMeta {
    fn from_decl(e: &EntityDecl) -> Self {
        let fields = e
            .fields
            .iter()
            .map(|f| FieldMeta {
                name: f.name.name.clone(),
                is_optional: is_option_type(&f.ty),
                is_derived: f.derived_from.is_some(),
            })
            .collect();
        Self { fields }
    }

    /// Fields a constructor record must initialise: not the id, not derived,
    /// not optional.
    fn required(&self) -> Vec<&str> {
        self.fields
            .iter()
            .filter(|f| f.name != "id" && !f.is_derived && !f.is_optional)
            .map(|f| f.name.as_str())
            .collect()
    }

    fn has_field(&self, name: &str) -> bool {
        self.fields.iter().any(|f| f.name == name)
    }

    fn is_derived(&self, name: &str) -> bool {
        self.fields.iter().any(|f| f.name == name && f.is_derived)
    }

    /// Whether the named field is declared `Option<T>` — the only kind that can
    /// be cleared with `None`.
    fn is_optional(&self, name: &str) -> bool {
        self.fields.iter().any(|f| f.name == name && f.is_optional)
    }
}

fn is_option_type(ty: &TypeExpr) -> bool {
    matches!(ty, TypeExpr::Generic { base, .. } if base.simple_name() == Some("Option"))
}

// ---- declaration checks ----

fn check_entity(
    e: &EntityDecl,
    entity_names: &[String],
    aux_types: &[String],
    meta: &HashMap<String, EntityMeta>,
    file: &str,
    diags: &mut Vec<Diag>,
) {
    for f in &e.fields {
        validate_type(&f.ty, entity_names, aux_types, file, diags);
        if let Some(back) = &f.derived_from {
            check_derived(f, back, meta, entity_names, file, diags);
        } else {
            warn_stored_entity_array(e, f, entity_names, file, diags);
        }
    }
}

/// Warn (W050, §4.4) on a *stored* array of entity references — a field typed
/// `[Child]` where `Child` is an entity and the field is not `derived from`.
/// graph-node stores such arrays inline and rewrites the whole array into a new
/// versioned row on every append, so growth is O(n²) on disk. The one-to-many
/// should be modelled with `@derivedFrom` (a computed reverse lookup) instead.
/// Scalar and enum arrays (`[String]`, `[BigInt]`, `[TokenStandard]`) are
/// genuinely stored values and never fire.
fn warn_stored_entity_array(
    e: &EntityDecl,
    f: &FieldDecl,
    entity_names: &[String],
    file: &str,
    diags: &mut Vec<Diag>,
) {
    let TypeExpr::List { elem, .. } = &f.ty else {
        return;
    };
    let Some(child) = elem.simple_name() else {
        return;
    };
    if !entity_names.iter().any(|n| n == child) {
        return;
    }
    diags.push(
        Diag::new(
            file,
            f.ty.span(),
            "W050",
            format!("`{}` stores an array of `{child}` entities", f.name.name),
            "graph-node copies the whole array on every append — O(n²) disk as it grows",
        )
        .warning()
        .with_help(format!(
            "model this one-to-many with `@derivedFrom`: add a back-ref field on `{child}` (e.g. `{back}: {parent}`) and declare `{field}: [{child}] derived from {back}`",
            back = lower_first(&e.name.name),
            parent = e.name.name,
            field = f.name.name,
        )),
    );
}

/// Lowercase the first character of a type name for a suggested field name
/// (`Indexer` → `indexer`).
fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(first) => first.to_lowercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Validate an entity's `implements` clause: each interface must exist, and the
/// entity must redeclare every field the interface defines (The Graph requires
/// it — catching it here turns a `graph build` failure into a teaching error).
fn check_implements(
    e: &EntityDecl,
    interfaces: &HashMap<String, Vec<String>>,
    file: &str,
    diags: &mut Vec<Diag>,
) {
    for iface in &e.implements {
        let Some(iface_fields) = interfaces.get(&iface.name) else {
            diags.push(
                Diag::new(
                    file,
                    &iface.span,
                    "E003",
                    format!("unknown interface `{}`", iface.name),
                    "not a declared interface",
                )
                .with_help("declare it with `interface Name { … }`"),
            );
            continue;
        };
        for field in iface_fields {
            if !e.fields.iter().any(|f| &f.name.name == field) {
                diags.push(
                    Diag::new(
                        file,
                        &e.name.span,
                        "E004",
                        format!(
                            "`{}` implements `{}` but is missing field `{field}`",
                            e.name.name, iface.name
                        ),
                        "missing interface field",
                    )
                    .with_help(
                        "an entity must redeclare every field of the interfaces it implements",
                    ),
                );
            }
        }
    }
}

fn check_derived(
    f: &FieldDecl,
    back: &redstart_parser::Ident,
    meta: &HashMap<String, EntityMeta>,
    entity_names: &[String],
    file: &str,
    diags: &mut Vec<Diag>,
) {
    let Some(target) = entity_name_of(&f.ty) else {
        diags.push(
            Diag::new(
                file,
                f.ty.span(),
                "E020",
                "a `derived from` field must reference an entity",
                "not an entity type",
            )
            .with_help("derived fields look like `swaps: [Swap] derived from pool`"),
        );
        return;
    };
    if !entity_names.iter().any(|n| n == &target) {
        return; // already reported by validate_type
    }
    match meta.get(&target) {
        Some(tm) if tm.has_field(&back.name) => {}
        Some(_) => diags.push(
            Diag::new(
                file,
                &back.span,
                "E021",
                format!("`{target}` has no field `{}` to derive from", back.name),
                "no such field",
            )
            .with_help(format!(
                "add a `{}: {}` field to `{target}`",
                back.name, "…"
            )),
        ),
        None => {}
    }
}

fn check_source(s: &SourceDecl, abis: &AbiIndex, file: &str, diags: &mut Vec<Diag>) {
    for key in ["abi", "network", "address", "startBlock"] {
        if get_setting(&s.settings, key).is_none() {
            diags.push(
                Diag::new(
                    file,
                    &s.name.span,
                    "E030",
                    format!("source `{}` is missing `{key}`", s.name.name),
                    format!("add `{key}: …`"),
                )
                .with_help("a source needs `abi`, `network`, `address`, and `startBlock`"),
            );
        }
    }
    check_abi_ref(&s.settings, abis, file, diags);
}

fn check_template(t: &TemplateDecl, abis: &AbiIndex, file: &str, diags: &mut Vec<Diag>) {
    // A `kind: file` (file/IPFS) template has no contract — no `abi`/`network`.
    if is_file_template(t) {
        return;
    }
    for key in ["abi", "network"] {
        if get_setting(&t.settings, key).is_none() {
            diags.push(Diag::new(
                file,
                &t.name.span,
                "E031",
                format!("template `{}` is missing `{key}`", t.name.name),
                format!("add `{key}: …`"),
            ));
        }
    }
    check_abi_ref(&t.settings, abis, file, diags);
}

/// Whether a template declares `kind: file` (a file/IPFS data source).
fn is_file_template(t: &TemplateDecl) -> bool {
    matches!(
        get_setting(&t.settings, "kind")
            .and_then(|s| path_name(&s.value))
            .as_deref(),
        Some("file" | "ipfs")
    )
}

fn check_abi_ref(settings: &[Setting], abis: &AbiIndex, file: &str, diags: &mut Vec<Diag>) {
    if let Some(setting) = get_setting(settings, "abi") {
        if let Some(name) = path_name(&setting.value) {
            if !abis.paths.contains_key(&name) {
                diags.push(
                    Diag::new(
                        file,
                        setting.value.span(),
                        "E032",
                        format!("unknown ABI `{name}`"),
                        "not imported",
                    )
                    .with_help(format!(
                        "import it with `abi {name} from \"./abis/{name}.json\"`"
                    )),
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn check_handler(
    h: &HandlerDecl,
    entities: &HashMap<String, EntityInfo>,
    meta: &HashMap<String, EntityMeta>,
    source_abi: &HashMap<String, String>,
    source_network: &HashMap<String, String>,
    data_sources: &HashMap<String, ()>,
    fn_returns: &HashMap<String, RTy>,
    fn_names: &HashSet<String>,
    abis: &mut AbiIndex,
    file: &str,
    diags: &mut Vec<Diag>,
    id_string_entities: &HashSet<String>,
    sites: &mut Vec<IdSite>,
) {
    // The source must exist.
    if !data_sources.contains_key(&h.source.name) {
        diags.push(
            Diag::new(
                file,
                &h.source.span,
                "E040",
                format!("unknown source `{}`", h.source.name),
                "no such source or template",
            )
            .with_help("declare it with a `source` (or `template`) block"),
        );
        return;
    }

    let abi_name = source_abi.get(&h.source.name).cloned().unwrap_or_default();

    // Resolve the handler's trigger members and the type of its parameter.
    // Whether the ABI gave us an authoritative parameter list — only then can an
    // unknown `event.params.x` be called a mistake rather than a blind spot.
    let mut params_known = false;
    let (param_ty, params, outputs) = match &h.kind {
        HandlerKind::Event => {
            if abis.readable(&abi_name) && abis.event_params(&abi_name, &h.event.name).is_none() {
                diags.push(
                    Diag::new(
                        file,
                        &h.event.span,
                        "E041",
                        format!("event `{}` not found in ABI `{abi_name}`", h.event.name),
                        "no such event",
                    )
                    .with_help("check the event name and casing against the ABI"),
                );
            }
            let resolved = abis.event_params(&abi_name, &h.event.name);
            params_known = resolved.is_some();
            (RTy::Event, rty_map(resolved), HashMap::new())
        }
        HandlerKind::Call => {
            if abis.readable(&abi_name) && abis.function_inputs(&abi_name, &h.event.name).is_none()
            {
                diags.push(
                    Diag::new(
                        file,
                        &h.event.span,
                        "E042",
                        format!("function `{}` not found in ABI `{abi_name}`", h.event.name),
                        "no such function",
                    )
                    .with_help(
                        "call handlers bind a contract function by name — check it against the ABI",
                    ),
                );
            }
            // Call handlers need Parity-style tracing, which several major
            // networks don't support — there the handler silently never fires.
            if let Some(net) = source_network.get(&h.source.name) {
                if network_lacks_call_tracing(net) {
                    diags.push(
                        Diag::new(
                            file,
                            &h.event.span,
                            "W010",
                            format!("call handler on `{net}`, which has no call tracing"),
                            "this handler will never fire here",
                        )
                        .warning()
                        .with_help("call handlers need Parity tracing (mainly Ethereum mainnet) — prefer an event handler"),
                    );
                }
            }
            let resolved = abis.function_inputs(&abi_name, &h.event.name);
            params_known = resolved.is_some();
            (
                RTy::Call,
                rty_map(resolved),
                rty_map(abis.function_output_params(&abi_name, &h.event.name)),
            )
        }
        HandlerKind::Block(filter) => {
            // An unfiltered block handler runs on *every block of the whole
            // chain* — documented as "very, very slow".
            if matches!(filter, redstart_parser::ast::BlockFilter::Every) {
                diags.push(
                    Diag::new(
                        file,
                        &h.source.span,
                        "W011",
                        "unfiltered block handler runs on every block of the entire chain",
                        "very slow",
                    )
                    .warning()
                    .with_help("add `every N` to poll, or `once` to run a single time"),
                );
            }
            (RTy::Block, HashMap::new(), HashMap::new())
        }
        // A file/IPFS handler receives the file contents as `Bytes`.
        HandlerKind::File => (RTy::Bytes, HashMap::new(), HashMap::new()),
    };

    let ctx = BodyCtx {
        entities,
        meta,
        event_param: h.param.name.clone(),
        param_ty,
        event_params: params,
        params_known,
        call_outputs: outputs,
        fn_returns,
        fn_names,
        abis,
    };
    let mut locals: HashMap<String, RTy> = HashMap::new();
    check_block(&h.body.stmts, &ctx, &mut locals, file, diags);
    warn_stringified_ids(
        &h.body.stmts,
        &ctx,
        &mut HashMap::new(),
        &mut HashSet::new(),
        file,
        diags,
    );
    let mut ident_freq = HashMap::new();
    count_path_uses(&h.body.stmts, &mut ident_freq);
    collect_id_sites(
        &h.body.stmts,
        &ctx,
        &mut HashMap::new(),
        &mut HashMap::new(),
        &ident_freq,
        id_string_entities,
        file,
        sites,
    );
}

/// Check a free `fn` body — same footgun analysis as a handler, with the
/// function's parameters seeded as typed locals.
#[allow(clippy::too_many_arguments)]
fn check_fn(
    func: &redstart_parser::ast::FnDecl,
    entities: &HashMap<String, EntityInfo>,
    meta: &HashMap<String, EntityMeta>,
    fn_returns: &HashMap<String, RTy>,
    fn_names: &HashSet<String>,
    abis: &AbiIndex,
    file: &str,
    diags: &mut Vec<Diag>,
    id_string_entities: &HashSet<String>,
    sites: &mut Vec<IdSite>,
) {
    let ctx = BodyCtx {
        entities,
        meta,
        event_param: String::new(),
        param_ty: RTy::Unknown,
        event_params: HashMap::new(),
        params_known: false,
        call_outputs: HashMap::new(),
        fn_returns,
        fn_names,
        abis,
    };
    let mut locals: HashMap<String, RTy> = func
        .params
        .iter()
        .map(|p| (p.name.name.clone(), resolve_type(&p.ty, entities)))
        .collect();
    check_block(&func.body.stmts, &ctx, &mut locals, file, diags);
    warn_stringified_ids(
        &func.body.stmts,
        &ctx,
        &mut locals.clone(),
        &mut HashSet::new(),
        file,
        diags,
    );
    let mut ident_freq = HashMap::new();
    count_path_uses(&func.body.stmts, &mut ident_freq);
    collect_id_sites(
        &func.body.stmts,
        &ctx,
        &mut locals.clone(),
        &mut HashMap::new(),
        &ident_freq,
        id_string_entities,
        file,
        sites,
    );
}

/// Convert an optional ABI parameter list into a name → resolved-type map.
fn rty_map(params: Option<Vec<EventParam>>) -> HashMap<String, RTy> {
    params
        .unwrap_or_default()
        .into_iter()
        .map(|p| (p.name, sol_to_rty(&p.sol_type)))
        .collect()
}

// ---- handler body checks ----

struct BodyCtx<'a> {
    entities: &'a HashMap<String, EntityInfo>,
    meta: &'a HashMap<String, EntityMeta>,
    event_param: String,
    /// The resolved type of the handler parameter (Event / Call / Block).
    param_ty: RTy,
    /// Event params (event handler) or function inputs (call handler).
    event_params: HashMap<String, RTy>,
    /// Whether `event_params` came from a resolved ABI entry (so an absent name
    /// is a real error) rather than an unreadable or missing ABI.
    params_known: bool,
    /// Function outputs (call handler only).
    call_outputs: HashMap<String, RTy>,
    /// Free-function name -> resolved return type.
    fn_returns: &'a HashMap<String, RTy>,
    /// Every declared free function name, for resolving bare calls. Wider than
    /// `fn_returns`, which only holds the ones that declare a return type.
    fn_names: &'a HashSet<String>,
    abis: &'a AbiIndex,
}

fn check_block(
    stmts: &[Stmt],
    ctx: &BodyCtx,
    locals: &mut HashMap<String, RTy>,
    file: &str,
    diags: &mut Vec<Diag>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Let { name, value, .. } => {
                if let Some((entity, record, nullable)) = entity_ctor(value) {
                    check_ctor_record(&entity, record, ctx, file, value, diags);
                    check_expr(value, ctx, locals, file, diags);
                    // `loadOrCreate`/`create` yield the entity; `load`/`loadInBlock`
                    // yield `Option<Entity>` — it must be matched before use.
                    let ty = if nullable {
                        RTy::Option(Box::new(RTy::Entity(entity)))
                    } else {
                        RTy::Entity(entity)
                    };
                    locals.insert(name.name.clone(), ty);
                } else {
                    check_expr(value, ctx, locals, file, diags);
                    locals.insert(name.name.clone(), infer(value, ctx, locals));
                }
            }
            Stmt::Assign { target, value, .. } => {
                check_assign_target(target, ctx, locals, file, diags);
                check_none_assignment(target, value, ctx, locals, file, diags);
                warn_bigint_division_precision(target, value, ctx, locals, file, diags);
                check_expr(target, ctx, locals, file, diags);
                check_expr(value, ctx, locals, file, diags);
            }
            Stmt::Return { value: Some(v), .. } => check_expr(v, ctx, locals, file, diags),
            Stmt::Return { .. } => {}
            Stmt::If {
                cond,
                then_block,
                else_ifs,
                else_block,
                ..
            } => {
                check_expr(cond, ctx, locals, file, diags);
                let mut b = locals.clone();
                check_block(&then_block.stmts, ctx, &mut b, file, diags);
                for (c, block) in else_ifs {
                    check_expr(c, ctx, locals, file, diags);
                    let mut bb = locals.clone();
                    check_block(&block.stmts, ctx, &mut bb, file, diags);
                }
                if let Some(block) = else_block {
                    let mut bb = locals.clone();
                    check_block(&block.stmts, ctx, &mut bb, file, diags);
                }
            }
            Stmt::While { cond, body, .. } => {
                check_expr(cond, ctx, locals, file, diags);
                let mut b = locals.clone();
                warn_calls_in_loop(&body.stmts, ctx, &b, file, diags);
                check_block(&body.stmts, ctx, &mut b, file, diags);
            }
            Stmt::For {
                var, iter, body, ..
            } => {
                let elem = check_for_iter(iter, ctx, locals, file, diags);
                let mut b = locals.clone();
                b.insert(var.name.clone(), elem);
                warn_calls_in_loop(&body.stmts, ctx, &b, file, diags);
                check_block(&body.stmts, ctx, &mut b, file, diags);
            }
            Stmt::Expr(e) => {
                if let Expr::Match {
                    scrutinee, arms, ..
                } = e
                {
                    check_match(scrutinee, arms, ctx, locals, file, diags);
                } else {
                    check_expr(e, ctx, locals, file, diags);
                }
            }
        }
    }
}

/// Warn (W020) on a contract `eth_call` inside a loop body — the documented
/// "stuck at 3%" sync killer (each call is a 100 ms+ blocking RPC, run serially
/// while the handler is paused). Skips nested loops, which warn via their own arm.
fn warn_calls_in_loop(
    stmts: &[Stmt],
    ctx: &BodyCtx,
    locals: &HashMap<String, RTy>,
    file: &str,
    diags: &mut Vec<Diag>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Let { value, .. }
            | Stmt::Return {
                value: Some(value), ..
            } => {
                warn_calls_in_expr(value, ctx, locals, file, diags);
            }
            Stmt::Assign { target, value, .. } => {
                warn_calls_in_expr(target, ctx, locals, file, diags);
                warn_calls_in_expr(value, ctx, locals, file, diags);
            }
            Stmt::Expr(e) => warn_calls_in_expr(e, ctx, locals, file, diags),
            Stmt::If {
                cond,
                then_block,
                else_ifs,
                else_block,
                ..
            } => {
                warn_calls_in_expr(cond, ctx, locals, file, diags);
                warn_calls_in_loop(&then_block.stmts, ctx, locals, file, diags);
                for (c, block) in else_ifs {
                    warn_calls_in_expr(c, ctx, locals, file, diags);
                    warn_calls_in_loop(&block.stmts, ctx, locals, file, diags);
                }
                if let Some(block) = else_block {
                    warn_calls_in_loop(&block.stmts, ctx, locals, file, diags);
                }
            }
            // Nested loops handle their own bodies; don't double-report.
            Stmt::While { .. } | Stmt::For { .. } | Stmt::Return { .. } => {}
        }
    }
}

/// Recurse through an expression, flagging any contract call (a call inferring
/// to `Result`) with W020.
fn warn_calls_in_expr(
    expr: &Expr,
    ctx: &BodyCtx,
    locals: &HashMap<String, RTy>,
    file: &str,
    diags: &mut Vec<Diag>,
) {
    match expr {
        Expr::Call { callee, args, .. } => {
            if matches!(infer(expr, ctx, locals), RTy::Result(_)) {
                diags.push(
                    Diag::new(
                        file,
                        expr.span(),
                        "W020",
                        "contract call (`eth_call`) inside a loop",
                        "this blocks the handler on an RPC every iteration",
                    )
                    .warning()
                    .with_help("hoist the call out of the loop, or cache it — declared/looped eth_calls are a top sync killer"),
                );
            }
            warn_calls_in_expr(callee, ctx, locals, file, diags);
            for a in args {
                warn_calls_in_expr(a, ctx, locals, file, diags);
            }
        }
        Expr::Field { base, .. } => warn_calls_in_expr(base, ctx, locals, file, diags),
        Expr::Binary { lhs, rhs, .. } => {
            warn_calls_in_expr(lhs, ctx, locals, file, diags);
            warn_calls_in_expr(rhs, ctx, locals, file, diags);
        }
        Expr::Unary { expr, .. } => warn_calls_in_expr(expr, ctx, locals, file, diags),
        Expr::Record { fields, .. } => {
            for (_, v) in fields {
                warn_calls_in_expr(v, ctx, locals, file, diags);
            }
        }
        Expr::Match {
            scrutinee, arms, ..
        } => {
            warn_calls_in_expr(scrutinee, ctx, locals, file, diags);
            for arm in arms {
                warn_calls_in_loop(&arm.body.stmts, ctx, locals, file, diags);
            }
        }
        _ => {}
    }
}

/// Warn (W040) when an entity id is a single `Bytes`/`Address` value stringified
/// via `.toHexString()` / `.toHex()`. A `Bytes` id indexes ~28% faster and uses
/// ~48% less disk than the equivalent hex-string id (Edge & Node benchmark), so
/// the entity should declare `id: Id<Bytes>` and pass the raw value. Composite ids
/// (`a.toHexString() + "-" + b…`, a `Binary`) and literal-string ids are genuinely
/// strings and never fire.
///
/// This is a *warning*, not an auto-rewrite: converting a String id to `Bytes`
/// changes the stored id value (hex-string → raw bytes), so it is a data change the
/// author must opt into — unlike immutability inference, which is store-identical.
fn warn_stringified_ids(
    stmts: &[Stmt],
    ctx: &BodyCtx,
    locals: &mut HashMap<String, RTy>,
    hex_locals: &mut HashSet<String>,
    file: &str,
    diags: &mut Vec<Diag>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Let { name, value, .. } => {
                warn_stringified_ctor_id(value, ctx, locals, hex_locals, file, diags);
                if stringified_single_bytes(value, ctx, locals) {
                    hex_locals.insert(name.name.clone());
                } else {
                    hex_locals.remove(&name.name);
                }
                locals.insert(name.name.clone(), infer(value, ctx, locals));
            }
            Stmt::Assign { value, .. } => {
                warn_stringified_ctor_id(value, ctx, locals, hex_locals, file, diags);
            }
            Stmt::Return { value: Some(v), .. } => {
                warn_stringified_ctor_id(v, ctx, locals, hex_locals, file, diags);
            }
            Stmt::Return { .. } => {}
            Stmt::If {
                then_block,
                else_ifs,
                else_block,
                ..
            } => {
                warn_stringified_ids(
                    &then_block.stmts,
                    ctx,
                    &mut locals.clone(),
                    &mut hex_locals.clone(),
                    file,
                    diags,
                );
                for (_, b) in else_ifs {
                    warn_stringified_ids(
                        &b.stmts,
                        ctx,
                        &mut locals.clone(),
                        &mut hex_locals.clone(),
                        file,
                        diags,
                    );
                }
                if let Some(b) = else_block {
                    warn_stringified_ids(
                        &b.stmts,
                        ctx,
                        &mut locals.clone(),
                        &mut hex_locals.clone(),
                        file,
                        diags,
                    );
                }
            }
            Stmt::While { body, .. } => warn_stringified_ids(
                &body.stmts,
                ctx,
                &mut locals.clone(),
                &mut hex_locals.clone(),
                file,
                diags,
            ),
            Stmt::For { var, body, .. } => {
                let mut l = locals.clone();
                l.insert(var.name.clone(), RTy::Unknown);
                warn_stringified_ids(
                    &body.stmts,
                    ctx,
                    &mut l,
                    &mut hex_locals.clone(),
                    file,
                    diags,
                );
            }
            Stmt::Expr(e) => {
                if let Expr::Match { arms, .. } = e {
                    for arm in arms {
                        warn_stringified_ids(
                            &arm.body.stmts,
                            ctx,
                            &mut locals.clone(),
                            &mut hex_locals.clone(),
                            file,
                            diags,
                        );
                    }
                } else {
                    warn_stringified_ctor_id(e, ctx, locals, hex_locals, file, diags);
                }
            }
        }
    }
}

/// Is `expr` exactly `<base>.toHexString()` / `<base>.toHex()` where `<base>` is a
/// single `Bytes`/`Address` value? (A concatenation is a `Binary`, not a `Call`.)
fn stringified_single_bytes(expr: &Expr, ctx: &BodyCtx, locals: &HashMap<String, RTy>) -> bool {
    let Expr::Call { callee, args, .. } = expr else {
        return false;
    };
    if !args.is_empty() {
        return false;
    }
    let Expr::Field { base, field, .. } = callee.as_ref() else {
        return false;
    };
    matches!(field.name.as_str(), "toHexString" | "toHex")
        && matches!(infer(base, ctx, locals), RTy::Bytes | RTy::Address)
}

/// The id argument of an `Entity.create(id, …)` / `Entity.loadOrCreate(id, …)`.
fn ctor_id_arg(value: &Expr) -> Option<&Expr> {
    let Expr::Call { callee, args, .. } = value else {
        return None;
    };
    let Expr::Field { base, field, .. } = callee.as_ref() else {
        return None;
    };
    if !matches!(field.name.as_str(), "create" | "loadOrCreate") {
        return None;
    }
    let Expr::Path { .. } = base.as_ref() else {
        return None;
    };
    args.first()
}

/// Emit W040 if a create/loadOrCreate keys the entity on a stringified single
/// address/bytes — directly (`E.create(addr.toHexString(), …)`) or via a local
/// (`let id = addr.toHexString(); E.create(id, …)`).
fn warn_stringified_ctor_id(
    value: &Expr,
    ctx: &BodyCtx,
    locals: &HashMap<String, RTy>,
    hex_locals: &HashSet<String>,
    file: &str,
    diags: &mut Vec<Diag>,
) {
    let Some(id_arg) = ctor_id_arg(value) else {
        return;
    };
    let via_local = matches!(id_arg, Expr::Path { segments, .. }
        if segments.len() == 1 && hex_locals.contains(&segments[0].name));
    if stringified_single_bytes(id_arg, ctx, locals) || via_local {
        diags.push(
            Diag::new(
                file,
                id_arg.span(),
                "W040",
                "entity id is a single address/bytes value stringified via `.toHexString()`",
                "a Bytes id indexes ~28% faster and uses ~48% less disk",
            )
            .warning()
            .with_help(
                "declare the entity `id: Id<Bytes>` and pass the raw `Bytes`/`Address`, dropping the `.toHexString()` (Edge & Node benchmark)",
            ),
        );
    }
}

// ---- id rewrite (`redstart fix --ids`, roadmap §4.3) ----------------------
//
// `plan_id_rewrites` turns the W040 lint into an opt-in auto-rewrite: it steers a
// `String`-keyed entity to a `Bytes` id (`Id<Bytes>`, ~28% faster / ~48% smaller
// on the Edge & Node benchmark) by flipping the schema declaration *and* dropping
// the `.toHexString()` at every construction site. Because that changes the stored
// id representation (hex-string → raw bytes) it is a deliberate data change, hence
// opt-in. It is applied only when *every* id site of the entity is provably a single
// stringified `Bytes`/`Address` — one literal-string or composite site and the whole
// entity is left untouched (and reported), so we never emit code that fails to check.

/// A single byte-range replacement in one `.red` file.
pub struct Edit {
    /// The file the offsets index into (display path).
    pub file: String,
    /// Byte offset of the start of the range to replace.
    pub start: usize,
    /// Byte offset of the end of the range (exclusive).
    pub end: usize,
    /// The replacement text (empty for a deletion).
    pub replacement: String,
}

/// An entity whose `String` id can be rewritten to `Id<Bytes>`, with every edit
/// (the schema declaration plus each construction site) needed to do it.
pub struct EntityIdRewrite {
    /// The entity name.
    pub entity: String,
    /// How many construction sites are rewritten.
    pub sites: usize,
    /// The declaration edit followed by one edit per site.
    pub edits: Vec<Edit>,
}

/// An entity with a convertible id site that also has a site we can't safely
/// convert, so it is left entirely untouched.
pub struct SkippedIdRewrite {
    /// The entity name.
    pub entity: String,
    /// Why the entity was skipped (the blocking site).
    pub reason: String,
    /// The file of the blocking site.
    pub file: String,
    /// The 1-based line of the blocking site.
    pub line: usize,
}

/// The result of [`plan_id_rewrites`].
pub struct IdRewritePlan {
    /// Entities that will be converted, each with its edits.
    pub rewrites: Vec<EntityIdRewrite>,
    /// Entities with a convertible site but a blocking site elsewhere.
    pub skipped: Vec<SkippedIdRewrite>,
}

/// One id-construction site found in a handler/fn/test body.
struct IdSite {
    entity: String,
    file: String,
    kind: SiteKind,
}

enum SiteKind {
    /// A `base.toHexString()` id — convert by deleting `[start, end)`.
    Convertible { start: usize, end: usize },
    /// An id we can't safely convert; blocks the whole entity. `w040` marks the
    /// stringified-via-a-local case — it still fires W040 (so the entity is a
    /// conversion *candidate*) but v1 won't rewrite through the intermediate local.
    Blocker {
        reason: String,
        line: usize,
        w040: bool,
    },
}

/// Plan the `String` → `Bytes` id rewrite across a whole project. Pure analysis:
/// it produces edits but touches nothing on disk (the CLI applies them).
#[must_use]
pub fn plan_id_rewrites(tree: &ModuleTree) -> IdRewritePlan {
    // Handler/fn sites, gathered by the checker with full type inference.
    let (_checked, _diags, mut sites) = analyze(tree, &HashMap::new());
    let modules = tree.ordered();

    // `String`-id entities, each mapped to the declaration edit (its id type span).
    let mut decl: HashMap<String, (String, usize, usize)> = HashMap::new();
    let mut id_entities: HashSet<String> = HashSet::new();
    for m in &modules {
        let file = m.file_path.display().to_string();
        for e in &m.program.entities {
            if let Some(idf) = e.fields.iter().find(|f| f.name.name == "id") {
                if is_string_id_type(&idf.ty) {
                    let sp = idf.ty.span();
                    decl.insert(e.name.name.clone(), (file.clone(), sp.start, sp.end));
                    id_entities.insert(e.name.name.clone());
                }
            }
        }
    }

    // Test bodies aren't type-checked through a `BodyCtx`, so scan them by shape.
    for m in &modules {
        let file = m.file_path.display().to_string();
        for t in &m.program.tests {
            collect_test_id_sites(&t.body.stmts, &id_entities, &file, &mut sites);
        }
    }

    // Fold sites into per-entity convertible edits, (first) blockers, and the set
    // of conversion candidates — entities for which W040 fired at least once.
    let mut convertible: HashMap<String, Vec<Edit>> = HashMap::new();
    let mut blockers: HashMap<String, SkippedIdRewrite> = HashMap::new();
    let mut candidates: HashSet<String> = HashSet::new();
    for site in sites {
        match site.kind {
            SiteKind::Convertible { start, end } => {
                candidates.insert(site.entity.clone());
                convertible.entry(site.entity).or_default().push(Edit {
                    file: site.file,
                    start,
                    end,
                    replacement: String::new(),
                });
            }
            SiteKind::Blocker { reason, line, w040 } => {
                if w040 {
                    candidates.insert(site.entity.clone());
                }
                blockers
                    .entry(site.entity.clone())
                    .or_insert(SkippedIdRewrite {
                        entity: site.entity,
                        reason,
                        file: site.file,
                        line,
                    });
            }
        }
    }

    // A candidate is only converted if it has no blocking site at all.
    let mut candidates: Vec<String> = candidates.into_iter().collect();
    candidates.sort();
    let mut rewrites = Vec::new();
    let mut skipped = Vec::new();
    for entity in candidates {
        if let Some(skip) = blockers.remove(&entity) {
            skipped.push(skip);
            continue;
        }
        let Some((dfile, dstart, dend)) = decl.get(&entity).cloned() else {
            continue;
        };
        let mut site_edits = convertible.remove(&entity).unwrap_or_default();
        // Deepest-first so earlier edits never shift later offsets.
        site_edits.sort_by_key(|e| std::cmp::Reverse(e.start));
        let sites = site_edits.len();
        let mut edits = vec![Edit {
            file: dfile,
            start: dstart,
            end: dend,
            replacement: "Id<Bytes>".to_string(),
        }];
        edits.extend(site_edits);
        rewrites.push(EntityIdRewrite {
            entity,
            sites,
            edits,
        });
    }
    skipped.sort_by(|a, b| a.entity.cmp(&b.entity));
    IdRewritePlan { rewrites, skipped }
}

/// Is this an `Id<String>` (or bare `String`) id type?
fn is_string_id_type(ty: &TypeExpr) -> bool {
    match ty {
        TypeExpr::Path { .. } => ty.simple_name() == Some("String"),
        TypeExpr::Generic { base, args, .. } => {
            base.simple_name() == Some("Id")
                && args.first().and_then(TypeExpr::simple_name) == Some("String")
        }
        TypeExpr::List { .. } => false,
    }
}

/// An `Entity.<method>(id, …)` call keyed on an entity id → `(entity, id_arg)`.
fn entity_id_call<'a>(expr: &'a Expr, methods: &[&str]) -> Option<(&'a str, &'a Expr)> {
    let Expr::Call { callee, args, .. } = expr else {
        return None;
    };
    let Expr::Field { base, field, .. } = callee.as_ref() else {
        return None;
    };
    if !methods.contains(&field.name.as_str()) {
        return None;
    }
    // `Entity` or a module-qualified `mod::Entity` — the entity name is the last
    // path segment (entity names are globally unique, so the qualifier is noise).
    let Expr::Path { segments, .. } = base.as_ref() else {
        return None;
    };
    Some((segments.last()?.name.as_str(), args.first()?))
}

/// The byte range of the `.toHexString()` / `.toHex()` suffix of `base.toHex…()`,
/// regardless of `base`'s type. Deleting it turns the call back into `base`.
fn to_hex_drop_range(arg: &Expr) -> Option<(usize, usize)> {
    let Expr::Call { callee, args, span } = arg else {
        return None;
    };
    if !args.is_empty() {
        return None;
    }
    let Expr::Field { base, field, .. } = callee.as_ref() else {
        return None;
    };
    if !matches!(field.name.as_str(), "toHexString" | "toHex") {
        return None;
    }
    Some((base.span().end, span.end))
}

/// Why an id argument can't be auto-converted to `Bytes`.
fn id_blocker_reason(arg: &Expr) -> String {
    match arg {
        Expr::Str { .. } => "keyed on a literal string id".to_string(),
        Expr::Binary { .. } => "keyed on a composite id (string concatenation)".to_string(),
        Expr::Path { .. } => "id built via an intermediate local (convert it by hand)".to_string(),
        _ => "id expression is not a single stringified Bytes/Address value".to_string(),
    }
}

fn line_of(src: &str, off: usize) -> usize {
    src.get(..off)
        .map_or(1, |s| s.bytes().filter(|&b| b == b'\n').count() + 1)
}

/// The `let`-RHS `.toHexString()` drop range for an id passed *via a local* —
/// convertible only when the local holds a stringified single `Bytes`/`Address`
/// (in `hex_locals`) *and* is referenced exactly once in the whole body (this
/// site), so re-typing it to `Bytes` can't affect any other use.
fn via_local_range(
    id_arg: &Expr,
    hex_locals: &HashMap<String, (usize, usize)>,
    ident_freq: &HashMap<String, usize>,
) -> Option<(usize, usize)> {
    let Expr::Path { segments, .. } = id_arg else {
        return None;
    };
    if segments.len() != 1 {
        return None;
    }
    let name = &segments[0].name;
    let range = *hex_locals.get(name)?;
    (ident_freq.get(name).copied().unwrap_or(0) == 1).then_some(range)
}

/// Record a create/load id site (handler/fn body): convertible iff the id is a
/// single stringified `Bytes`/`Address` — inline, or via a use-once local — else
/// a blocker.
#[allow(clippy::too_many_arguments)]
fn record_id_site(
    value: &Expr,
    ctx: &BodyCtx,
    locals: &HashMap<String, RTy>,
    hex_locals: &HashMap<String, (usize, usize)>,
    ident_freq: &HashMap<String, usize>,
    id_entities: &HashSet<String>,
    file: &str,
    out: &mut Vec<IdSite>,
) {
    let methods = ["create", "loadOrCreate", "load", "loadInBlock"];
    let Some((entity, id_arg)) = entity_id_call(value, &methods) else {
        return;
    };
    if !id_entities.contains(entity) {
        return;
    }
    let kind = if stringified_single_bytes(id_arg, ctx, locals) {
        // Inline: `E.create(x.toHexString(), …)` — drop the suffix at the site.
        let (start, end) = to_hex_drop_range(id_arg).expect("stringified id is a toHex call");
        SiteKind::Convertible { start, end }
    } else if let Some((start, end)) = via_local_range(id_arg, hex_locals, ident_freq) {
        // `let id = x.toHexString(); E.create(id, …)`, `id` used only here — drop
        // the suffix on the `let` and flip the entity; the site itself is untouched.
        SiteKind::Convertible { start, end }
    } else {
        // A local holding `x.toHexString()` still fired W040 (candidate entity),
        // but it's reused elsewhere, so converting it isn't safe here.
        let via_local = matches!(id_arg, Expr::Path { segments, .. }
            if segments.len() == 1 && hex_locals.contains_key(&segments[0].name));
        let sp = id_arg.span();
        SiteKind::Blocker {
            reason: if via_local {
                "id built via a local that is used more than once (convert it by hand)".to_string()
            } else {
                id_blocker_reason(id_arg)
            },
            line: line_of(&sp.source, sp.start),
            w040: via_local,
        }
    };
    out.push(IdSite {
        entity: entity.to_string(),
        file: file.to_string(),
        kind,
    });
}

/// Collect id-construction sites in a handler/fn body (mirrors the W040 walk so
/// local types stay in scope for the `Bytes`/`Address` inference).
#[allow(clippy::too_many_arguments)]
fn collect_id_sites(
    stmts: &[Stmt],
    ctx: &BodyCtx,
    locals: &mut HashMap<String, RTy>,
    hex_locals: &mut HashMap<String, (usize, usize)>,
    ident_freq: &HashMap<String, usize>,
    id_entities: &HashSet<String>,
    file: &str,
    out: &mut Vec<IdSite>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Let { name, value, .. } => {
                record_id_site(
                    value,
                    ctx,
                    locals,
                    hex_locals,
                    ident_freq,
                    id_entities,
                    file,
                    out,
                );
                if stringified_single_bytes(value, ctx, locals) {
                    if let Some(range) = to_hex_drop_range(value) {
                        hex_locals.insert(name.name.clone(), range);
                    }
                } else {
                    hex_locals.remove(&name.name);
                }
                locals.insert(name.name.clone(), infer(value, ctx, locals));
            }
            Stmt::Assign { value, .. } => {
                record_id_site(
                    value,
                    ctx,
                    locals,
                    hex_locals,
                    ident_freq,
                    id_entities,
                    file,
                    out,
                );
            }
            Stmt::Return { value: Some(v), .. } => {
                record_id_site(
                    v,
                    ctx,
                    locals,
                    hex_locals,
                    ident_freq,
                    id_entities,
                    file,
                    out,
                );
            }
            Stmt::Return { .. } => {}
            Stmt::If {
                then_block,
                else_ifs,
                else_block,
                ..
            } => {
                collect_id_sites(
                    &then_block.stmts,
                    ctx,
                    &mut locals.clone(),
                    &mut hex_locals.clone(),
                    ident_freq,
                    id_entities,
                    file,
                    out,
                );
                for (_, b) in else_ifs {
                    collect_id_sites(
                        &b.stmts,
                        ctx,
                        &mut locals.clone(),
                        &mut hex_locals.clone(),
                        ident_freq,
                        id_entities,
                        file,
                        out,
                    );
                }
                if let Some(b) = else_block {
                    collect_id_sites(
                        &b.stmts,
                        ctx,
                        &mut locals.clone(),
                        &mut hex_locals.clone(),
                        ident_freq,
                        id_entities,
                        file,
                        out,
                    );
                }
            }
            Stmt::While { body, .. } => {
                collect_id_sites(
                    &body.stmts,
                    ctx,
                    &mut locals.clone(),
                    &mut hex_locals.clone(),
                    ident_freq,
                    id_entities,
                    file,
                    out,
                );
            }
            Stmt::For { var, body, .. } => {
                let mut l = locals.clone();
                l.insert(var.name.clone(), RTy::Unknown);
                collect_id_sites(
                    &body.stmts,
                    ctx,
                    &mut l,
                    &mut hex_locals.clone(),
                    ident_freq,
                    id_entities,
                    file,
                    out,
                );
            }
            Stmt::Expr(e) => {
                if let Expr::Match { arms, .. } = e {
                    for arm in arms {
                        collect_id_sites(
                            &arm.body.stmts,
                            ctx,
                            &mut locals.clone(),
                            &mut hex_locals.clone(),
                            ident_freq,
                            id_entities,
                            file,
                            out,
                        );
                    }
                } else {
                    record_id_site(
                        e,
                        ctx,
                        locals,
                        hex_locals,
                        ident_freq,
                        id_entities,
                        file,
                        out,
                    );
                }
            }
        }
    }
}

/// Count single-segment path identifier *uses* across a body (not `let` bindings,
/// which are `Ident`s, not `Path`s) — so `freq[name] == 1` means "used exactly
/// once". Powers the use-once safety check for converting a via-a-local id.
fn count_path_uses(stmts: &[Stmt], freq: &mut HashMap<String, usize>) {
    for stmt in stmts {
        match stmt {
            Stmt::Let { value, .. } => count_paths_expr(value, freq),
            Stmt::Assign { target, value, .. } => {
                count_paths_expr(target, freq);
                count_paths_expr(value, freq);
            }
            Stmt::Return { value: Some(v), .. } => count_paths_expr(v, freq),
            Stmt::Return { .. } => {}
            Stmt::If {
                cond,
                then_block,
                else_ifs,
                else_block,
                ..
            } => {
                count_paths_expr(cond, freq);
                count_path_uses(&then_block.stmts, freq);
                for (c, b) in else_ifs {
                    count_paths_expr(c, freq);
                    count_path_uses(&b.stmts, freq);
                }
                if let Some(b) = else_block {
                    count_path_uses(&b.stmts, freq);
                }
            }
            Stmt::While { cond, body, .. } => {
                count_paths_expr(cond, freq);
                count_path_uses(&body.stmts, freq);
            }
            Stmt::For { iter, body, .. } => {
                match iter {
                    ForIter::Range { start, end } => {
                        count_paths_expr(start, freq);
                        count_paths_expr(end, freq);
                    }
                    ForIter::Each(e) => count_paths_expr(e, freq),
                }
                count_path_uses(&body.stmts, freq);
            }
            Stmt::Expr(e) => count_paths_expr(e, freq),
        }
    }
}

fn count_paths_expr(e: &Expr, freq: &mut HashMap<String, usize>) {
    match e {
        Expr::Path { segments, .. } if segments.len() == 1 => {
            *freq.entry(segments[0].name.clone()).or_default() += 1;
        }
        Expr::Field { base, .. } => count_paths_expr(base, freq),
        Expr::Call { callee, args, .. } => {
            count_paths_expr(callee, freq);
            for a in args {
                count_paths_expr(a, freq);
            }
        }
        Expr::Record { fields, .. } => {
            for (_, v) in fields {
                count_paths_expr(v, freq);
            }
        }
        Expr::Array { elems, .. } => {
            for x in elems {
                count_paths_expr(x, freq);
            }
        }
        Expr::Index { base, index, .. } => {
            count_paths_expr(base, freq);
            count_paths_expr(index, freq);
        }
        Expr::Binary { lhs, rhs, .. } => {
            count_paths_expr(lhs, freq);
            count_paths_expr(rhs, freq);
        }
        Expr::Unary { expr, .. } => count_paths_expr(expr, freq),
        Expr::Match {
            scrutinee, arms, ..
        } => {
            count_paths_expr(scrutinee, freq);
            for arm in arms {
                count_path_uses(&arm.body.stmts, freq);
            }
        }
        _ => {}
    }
}

/// Visit every `Call` subexpression of `e` (used by the shape-only test scan).
fn visit_calls<F: FnMut(&Expr)>(e: &Expr, f: &mut F) {
    if let Expr::Call { .. } = e {
        f(e);
    }
    match e {
        Expr::Field { base, .. } => visit_calls(base, f),
        Expr::Call { callee, args, .. } => {
            visit_calls(callee, f);
            for a in args {
                visit_calls(a, f);
            }
        }
        Expr::Record { fields, .. } => {
            for (_, v) in fields {
                visit_calls(v, f);
            }
        }
        Expr::Array { elems, .. } => {
            for x in elems {
                visit_calls(x, f);
            }
        }
        Expr::Index { base, index, .. } => {
            visit_calls(base, f);
            visit_calls(index, f);
        }
        Expr::Binary { lhs, rhs, .. } => {
            visit_calls(lhs, f);
            visit_calls(rhs, f);
        }
        Expr::Unary { expr, .. } => visit_calls(expr, f),
        Expr::Match { scrutinee, .. } => visit_calls(scrutinee, f),
        _ => {}
    }
}

/// Record id sites inside a `test` body by shape (no `BodyCtx`). A raw `0x…`
/// literal is already valid as a `Bytes` id (neutral); a `.toHexString()` id is
/// convertible; anything else blocks the entity.
fn collect_test_id_sites(
    stmts: &[Stmt],
    id_entities: &HashSet<String>,
    file: &str,
    out: &mut Vec<IdSite>,
) {
    let methods = ["create", "loadOrCreate", "load", "loadInBlock", "at"];
    let record = |e: &Expr, out: &mut Vec<IdSite>| {
        visit_calls(e, &mut |call| {
            let Some((entity, id_arg)) = entity_id_call(call, &methods) else {
                return;
            };
            if !id_entities.contains(entity) {
                return;
            }
            match id_arg {
                // A raw address/bytes literal is already a valid `Bytes` id.
                Expr::Hex { .. } => {}
                _ if to_hex_drop_range(id_arg).is_some() => {
                    let (start, end) = to_hex_drop_range(id_arg).unwrap();
                    out.push(IdSite {
                        entity: entity.to_string(),
                        file: file.to_string(),
                        kind: SiteKind::Convertible { start, end },
                    });
                }
                _ => {
                    // A test never makes an entity a candidate — it only blocks one
                    // already flagged in a handler/fn body — so `w040` is false.
                    let sp = id_arg.span();
                    out.push(IdSite {
                        entity: entity.to_string(),
                        file: file.to_string(),
                        kind: SiteKind::Blocker {
                            reason: id_blocker_reason(id_arg),
                            line: line_of(&sp.source, sp.start),
                            w040: false,
                        },
                    });
                }
            }
        });
    };
    for stmt in stmts {
        match stmt {
            Stmt::Let { value, .. } => record(value, out),
            Stmt::Assign { target, value, .. } => {
                record(target, out);
                record(value, out);
            }
            Stmt::Return { value: Some(v), .. } => record(v, out),
            Stmt::Return { .. } => {}
            Stmt::If {
                cond,
                then_block,
                else_ifs,
                else_block,
                ..
            } => {
                record(cond, out);
                collect_test_id_sites(&then_block.stmts, id_entities, file, out);
                for (c, b) in else_ifs {
                    record(c, out);
                    collect_test_id_sites(&b.stmts, id_entities, file, out);
                }
                if let Some(b) = else_block {
                    collect_test_id_sites(&b.stmts, id_entities, file, out);
                }
            }
            Stmt::While { cond, body, .. } => {
                record(cond, out);
                collect_test_id_sites(&body.stmts, id_entities, file, out);
            }
            Stmt::For { body, .. } => collect_test_id_sites(&body.stmts, id_entities, file, out),
            Stmt::Expr(e) => {
                record(e, out);
                if let Expr::Match { arms, .. } = e {
                    for arm in arms {
                        collect_test_id_sites(&arm.body.stmts, id_entities, file, out);
                    }
                }
            }
        }
    }
}

/// Check a `for` iterable, returning the loop variable's element type.
fn check_for_iter(
    iter: &ForIter,
    ctx: &BodyCtx,
    locals: &HashMap<String, RTy>,
    file: &str,
    diags: &mut Vec<Diag>,
) -> RTy {
    match iter {
        ForIter::Range { start, end } => {
            check_expr(start, ctx, locals, file, diags);
            check_expr(end, ctx, locals, file, diags);
            RTy::Int
        }
        ForIter::Each(list) => {
            check_expr(list, ctx, locals, file, diags);
            match infer(list, ctx, locals) {
                RTy::List(inner) => *inner,
                _ => RTy::Unknown,
            }
        }
    }
}

fn check_match(
    scrutinee: &Expr,
    arms: &[MatchArm],
    ctx: &BodyCtx,
    locals: &mut HashMap<String, RTy>,
    file: &str,
    diags: &mut Vec<Diag>,
) {
    check_expr(scrutinee, ctx, locals, file, diags);
    let scrut_ty = infer(scrutinee, ctx, locals);
    for arm in arms {
        let mut arm_locals = locals.clone();
        if let Pattern::Ctor { name, bindings, .. } = &arm.pattern {
            if let Some(b) = bindings.first() {
                let bound = match (&scrut_ty, name.name.as_str()) {
                    (RTy::Result(inner), "Ok") | (RTy::Option(inner), "Some") => (**inner).clone(),
                    _ => RTy::Unknown,
                };
                arm_locals.insert(b.name.clone(), bound);
            }
        }
        check_block(&arm.body.stmts, ctx, &mut arm_locals, file, diags);
    }

    check_exhaustive(scrutinee, arms, &scrut_ty, file, diags);
}

/// Require a `match` to cover every variant (or carry a wildcard).
fn check_exhaustive(
    scrutinee: &Expr,
    arms: &[MatchArm],
    scrut_ty: &RTy,
    file: &str,
    diags: &mut Vec<Diag>,
) {
    let required: &[&str] = match scrut_ty {
        RTy::Result(_) => &["Ok", "Err"],
        RTy::Option(_) => &["Some", "None"],
        _ => return, // unknown scrutinee — can't judge
    };

    let has_wildcard = arms.iter().any(|a| {
        matches!(
            a.pattern,
            Pattern::Wildcard { .. } | Pattern::Binding { .. }
        )
    });
    if has_wildcard {
        return;
    }

    let present: Vec<&str> = arms
        .iter()
        .filter_map(|a| match &a.pattern {
            Pattern::Ctor { name, .. } => Some(name.name.as_str()),
            _ => None,
        })
        .collect();
    let missing: Vec<&str> = required
        .iter()
        .copied()
        .filter(|r| !present.contains(r))
        .collect();
    if !missing.is_empty() {
        diags.push(
            Diag::new(
                file,
                scrutinee.span(),
                "E070",
                format!("non-exhaustive `match`: missing {}", missing.join(", ")),
                "add the missing arm(s)",
            )
            .with_help("every variant must be handled — or add a `_ => { … }` wildcard arm"),
        );
    }
}

/// Check that a `loadOrCreate`/`create` record initialises every required field.
fn check_ctor_record(
    entity: &str,
    record: Option<&[(redstart_parser::Ident, Expr)]>,
    ctx: &BodyCtx,
    file: &str,
    value: &Expr,
    diags: &mut Vec<Diag>,
) {
    let Some(meta) = ctx.meta.get(entity) else {
        diags.push(Diag::new(
            file,
            value.span(),
            "E050",
            format!("unknown entity `{entity}`"),
            "no such entity",
        ));
        return;
    };
    let Some(record) = record else { return }; // load() has no record
    let present: Vec<&str> = record.iter().map(|(k, _)| k.name.as_str()).collect();

    let missing: Vec<&str> = meta
        .required()
        .into_iter()
        .filter(|r| !present.contains(r))
        .collect();
    if !missing.is_empty() {
        diags.push(
            Diag::new(
                file,
                value.span(),
                "E051",
                format!(
                    "`{entity}` is missing required field(s): {}",
                    missing.join(", ")
                ),
                "incomplete initializer",
            )
            .with_help("every non-optional field must be set when creating an entity"),
        );
    }

    for (k, _) in record {
        if !meta.has_field(&k.name) {
            diags.push(Diag::new(
                file,
                &k.span,
                "E052",
                format!("`{entity}` has no field `{}`", k.name),
                "unknown field",
            ));
        }
    }
}

/// Forbid assigning to a `derived` field.
fn check_assign_target(
    target: &Expr,
    ctx: &BodyCtx,
    locals: &HashMap<String, RTy>,
    file: &str,
    diags: &mut Vec<Diag>,
) {
    if let Expr::Field { base, field, .. } = target {
        if let RTy::Entity(entity) = infer(base, ctx, locals) {
            if ctx
                .meta
                .get(&entity)
                .is_some_and(|m| m.is_derived(&field.name))
            {
                diags.push(
                    Diag::new(
                        file,
                        &field.span,
                        "E053",
                        format!("cannot assign to derived field `{}`", field.name),
                        "derived fields are read-only",
                    )
                    .with_help(
                        "`derived from` fields are computed from the other side of the relation",
                    ),
                );
            }
        }
    }
}

/// Clearing a field — `trove.interestBatch = None` — only makes sense when the
/// field is nullable. Assigning `None` to a required field would store a null in
/// a non-nullable column, which graph-node rejects at write time.
fn check_none_assignment(
    target: &Expr,
    value: &Expr,
    ctx: &BodyCtx,
    locals: &HashMap<String, RTy>,
    file: &str,
    diags: &mut Vec<Diag>,
) {
    if !is_none_literal(value) {
        return;
    }
    let Expr::Field { base, field, .. } = target else {
        return;
    };
    let RTy::Entity(entity) = infer(base, ctx, locals) else {
        return;
    };
    let Some(meta) = ctx.meta.get(&entity) else {
        return;
    };
    if meta.has_field(&field.name) && !meta.is_optional(&field.name) {
        diags.push(
            Diag::new(
                file,
                value.span(),
                "E056",
                format!("cannot clear required field `{}` on `{entity}`", field.name),
                "this field is not optional",
            )
            .with_help(
                "only an `Option<T>` field can be cleared — declare it `Option<T>`, or assign a value",
            ),
        );
    }
}

/// Whether an expression is the bare `None` literal.
fn is_none_literal(expr: &Expr) -> bool {
    matches!(expr, Expr::Path { segments, .. }
        if segments.len() == 1 && segments[0].name == "None")
}

/// Walk an expression, flagging the contract-call and nullable-arithmetic
/// footguns.
fn check_expr(
    expr: &Expr,
    ctx: &BodyCtx,
    locals: &HashMap<String, RTy>,
    file: &str,
    diags: &mut Vec<Diag>,
) {
    match expr {
        Expr::Field { base, field, .. } => {
            match infer(base, ctx, locals) {
                RTy::Result(_) if field.name == "value" => diags.push(
                    Diag::new(file, &field.span, "E060", "cannot read `.value` of a contract call directly", "this call may have reverted")
                        .with_help("`match` on the result: `match call { Ok(v) => { … } Err(e) => { … } }`"),
                ),
                // `.save()` is the reflex every subgraph author brings with them,
                // and "has no field `save`" teaches nothing. Say why it's absent.
                RTy::Entity(_) if field.name == "save" => diags.push(
                    Diag::new(
                        file,
                        &field.span,
                        "E055",
                        "entities save themselves — remove this `.save()`",
                        "not needed",
                    )
                    .with_help(
                        "Redstart dirty-tracks every entity it creates or mutates and saves it at the end of the scope, including on an early `return` — a forgotten `.save()` is unrepresentable",
                    ),
                ),
                RTy::Entity(name) if field.name != "id" => {
                    if ctx.meta.get(&name).is_some_and(|m| !m.has_field(&field.name)) {
                        diags.push(Diag::new(
                            file,
                            &field.span,
                            "E054",
                            format!("`{name}` has no field `{}`", field.name),
                            "unknown field",
                        ));
                    }
                }
                RTy::Option(_) => diags.push(
                    Diag::new(file, &field.span, "E062", format!("cannot access `.{}` on a nullable value", field.name), "this may be null")
                        .with_help("`match` it first: `match x { Some(v) => { … } None => { … } }` — `load`/`loadInBlock`/`ipfs.cat` can return nothing"),
                ),
                // The ABI is the source of truth for what a trigger carries, so a
                // name it doesn't declare is a typo or a renamed parameter — the
                // manifest/handler drift this language exists to make impossible.
                RTy::Params | RTy::CallInputs
                    if ctx.params_known && !ctx.event_params.contains_key(&field.name) =>
                {
                    let mut known: Vec<&str> =
                        ctx.event_params.keys().map(String::as_str).collect();
                    known.sort_unstable();
                    let help = if known.is_empty() {
                        "this trigger declares no parameters".to_string()
                    } else {
                        format!("declared parameters: {}", known.join(", "))
                    };
                    diags.push(
                        Diag::new(
                            file,
                            &field.span,
                            "E057",
                            format!("no parameter `{}` on this trigger", field.name),
                            "not in the ABI",
                        )
                        .with_help(help),
                    );
                }
                _ => {}
            }
            check_expr(base, ctx, locals, file, diags);
        }
        Expr::Binary { op, lhs, rhs, .. } => {
            if is_arithmetic(*op) {
                for side in [lhs.as_ref(), rhs.as_ref()] {
                    if infer(side, ctx, locals).is_option() {
                        diags.push(
                            Diag::new(
                                file,
                                side.span(),
                                "E061",
                                "cannot do arithmetic on an `Option`",
                                "unwrap this first",
                            )
                            .with_help("use `match` or `.unwrapOr(default)` before arithmetic"),
                        );
                    }
                }
            }
            // Division by a literal zero is a fatal, deterministic sync halt
            // (`attempted to divide … by zero`).
            if is_division(*op) && is_zero_literal(rhs) {
                diags.push(
                    Diag::new(file, rhs.span(), "E090", "division by zero", "this is zero")
                        .with_help(
                            "a divide-by-zero halts the entire sync — guard the denominator (`if d != BigInt.zero { … }`) or use a value you know is non-zero",
                        ),
                );
            }
            check_expr(lhs, ctx, locals, file, diags);
            check_expr(rhs, ctx, locals, file, diags);
        }
        Expr::Call { callee, args, .. } => {
            if let Expr::Field { base, field, .. } = callee.as_ref() {
                // Non-determinism breaks Proof-of-Indexing across indexers (and
                // gets them slashed). Forbid the known wall-clock / RNG host calls
                // at compile time — graph-node only blocks some of these at runtime.
                if let Expr::Path { segments, .. } = base.as_ref() {
                    if segments.len() == 1 {
                        if let Some(help) = nondeterministic_call(&segments[0].name, &field.name) {
                            diags.push(
                                Diag::new(
                                    file,
                                    &field.span,
                                    "E080",
                                    format!(
                                        "`{}.{}` is non-deterministic and not allowed in a subgraph",
                                        segments[0].name, field.name
                                    ),
                                    "non-deterministic call",
                                )
                                .with_help(help),
                            );
                        }
                    }
                }
                // Calling a function that the contract's ABI doesn't have.
                if let RTy::Contract(abi) = infer(base, ctx, locals) {
                    if ctx.abis.readable(&abi) && !ctx.abis.is_function(&abi, &field.name) {
                        diags.push(
                            Diag::new(file, &field.span, "E071", format!("contract `{abi}` has no function `{}`", field.name), "no such function")
                                .with_help("check the function name against the ABI (only view/pure calls are supported)"),
                        );
                    }
                }
            }
            check_unresolved_call(callee, ctx, locals, file, diags);
            check_expr(callee, ctx, locals, file, diags);
            for a in args {
                check_expr(a, ctx, locals, file, diags);
            }
        }
        Expr::Unary { expr, .. } => check_expr(expr, ctx, locals, file, diags),
        Expr::Record { fields, .. } => {
            for (_, v) in fields {
                check_expr(v, ctx, locals, file, diags);
            }
        }
        Expr::Array { elems, .. } => {
            for e in elems {
                check_expr(e, ctx, locals, file, diags);
            }
        }
        Expr::Index { base, index, .. } => {
            check_expr(base, ctx, locals, file, diags);
            check_expr(index, ctx, locals, file, diags);
        }
        _ => {}
    }
}

/// E072: a bare `name(…)` call that resolves to nothing.
///
/// `check` is the fast inner loop — it is what `redstart dev` re-runs on save and
/// what `check --json` hands an agent — and an unresolved call is the single most
/// common thing a half-finished port has. Until this existed only `verify` caught
/// it, as an AssemblyScript `Cannot find name` from deep inside `graph build`.
///
/// Deliberately narrow, because a false positive here would be worse than the
/// gap: only single-segment callees are judged. A module-qualified `helpers::f(…)`
/// is left alone, and every other call shape (`Entity.create`, `<Contract>.bind`,
/// `<Template>.create`, `log.info`, `BigInt.fromI32`) is an `Expr::Field` callee
/// and never reaches here.
fn check_unresolved_call(
    callee: &Expr,
    ctx: &BodyCtx,
    locals: &HashMap<String, RTy>,
    file: &str,
    diags: &mut Vec<Diag>,
) {
    let Expr::Path { segments, span } = callee else {
        return;
    };
    let [seg] = segments.as_slice() else {
        return;
    };
    let name = seg.name.as_str();

    // `Some(x)` is `Option` sugar, not a function — it lowers to the identity.
    if name == "Some" {
        return;
    }
    if ctx.fn_names.contains(name) {
        return;
    }
    // A local or the handler parameter being called is a different mistake
    // (there are no function values in Redstart), and not one to mislabel.
    if locals.contains_key(name) || name == ctx.event_param {
        return;
    }

    // An entity name reached as a call is nearly always a reach for a
    // constructor, so say where the constructor actually lives.
    let help = if ctx.entities.contains_key(name) {
        format!(
            "`{name}` is an entity, not a function — construct it with `{name}.create(id, {{ … }})` or `{name}.loadOrCreate(id, {{ … }})`"
        )
    // `min` rather than `find`: hash-set order is not stable, and a compiler's
    // diagnostics should not vary between runs of the same source.
    } else if let Some(near) = ctx
        .fn_names
        .iter()
        .filter(|f| f.eq_ignore_ascii_case(name))
        .min()
    {
        format!("did you mean `{near}`? — names are case-sensitive")
    } else {
        format!(
            "declare it with `fn {name}(…) -> … {{ … }}` — free functions are visible from every module in the project, so there is nothing to import"
        )
    };

    diags.push(
        Diag::new(
            file,
            span,
            "E072",
            format!("call to undefined function `{name}`"),
            "no such function",
        )
        .with_help(help),
    );
}

/// Known non-deterministic host calls (`namespace.method`). Returns the fix-it
/// help when the call is forbidden. Subgraphs must index identically on every
/// indexer; wall-clock time and randomness break that.
fn nondeterministic_call(ns: &str, method: &str) -> Option<&'static str> {
    match (ns, method) {
        ("Date", "now" | "UTC" | "parse") => {
            Some("subgraphs must be deterministic — use `event.block.timestamp` for time")
        }
        ("Math", "random") => Some(
            "there is no randomness in a deterministic subgraph — derive values from on-chain data (e.g. block hash)",
        ),
        _ => None,
    }
}

/// Networks known not to support the Parity tracing that call/block-`call`
/// handlers require — there a call handler silently never fires. Conservative:
/// only networks documented as lacking it, to avoid false positives.
fn network_lacks_call_tracing(network: &str) -> bool {
    let n = network.to_ascii_lowercase();
    matches!(
        n.as_str(),
        "arbitrum-one"
            | "arbitrum"
            | "arbitrum-nova"
            | "optimism"
            | "base"
            | "matic"
            | "polygon"
            | "polygon-zkevm"
            | "bsc"
            | "bnb"
            | "chapel"
    )
}

fn is_arithmetic(op: redstart_parser::ast::BinOp) -> bool {
    use redstart_parser::ast::BinOp;
    matches!(
        op,
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem
    )
}

fn is_division(op: redstart_parser::ast::BinOp) -> bool {
    use redstart_parser::ast::BinOp;
    matches!(op, BinOp::Div | BinOp::Rem)
}

/// Whether `expr` is a statically-zero value: a `0` / `0.0` literal, or a
/// `BigInt.zero()` / `BigDecimal.zero()` constant.
fn is_zero_literal(expr: &Expr) -> bool {
    match expr {
        Expr::Int { raw, .. } | Expr::Decimal { raw, .. } => {
            let digits = raw.replace('_', "");
            !digits.is_empty() && digits.chars().all(|c| c == '0' || c == '.')
        }
        Expr::Call { callee, .. } => {
            if let Expr::Field { base, field, .. } = callee.as_ref() {
                field.name == "zero"
                    && matches!(
                        base.as_ref(),
                        Expr::Path { segments, .. }
                            if matches!(
                                segments.last().map(|s| s.name.as_str()),
                                Some("BigInt" | "BigDecimal")
                            )
                    )
            } else {
                false
            }
        }
        _ => false,
    }
}

/// W030: `BigInt / BigInt` truncates the fraction to an integer; assigning that
/// to a `BigDecimal` field is the canonical Uniswap "price returns 0" bug.
fn warn_bigint_division_precision(
    target: &Expr,
    value: &Expr,
    ctx: &BodyCtx,
    locals: &HashMap<String, RTy>,
    file: &str,
    diags: &mut Vec<Diag>,
) {
    if let Expr::Binary { op, lhs, rhs, .. } = value {
        if is_division(*op)
            && infer(target, ctx, locals).is_bigdecimal()
            && infer(lhs, ctx, locals).is_bigint()
            && infer(rhs, ctx, locals).is_bigint()
        {
            diags.push(
                Diag::new(
                    file,
                    value.span(),
                    "W030",
                    "BigInt division truncates before becoming a BigDecimal",
                    "integer division drops the fraction",
                )
                .warning()
                .with_help(
                    "use `.divDecimal()` (or `.toBigDecimal()` the operands first) so the ratio keeps its fraction — otherwise e.g. a price computes to 0",
                ),
            );
        }
    }
}

// ---- inference (read-only; mirrors codegen's lowering view) ----

fn infer(expr: &Expr, ctx: &BodyCtx, locals: &HashMap<String, RTy>) -> RTy {
    match expr {
        Expr::Int { .. } => RTy::Int,
        Expr::Decimal { .. } => RTy::BigDecimal,
        Expr::Hex { .. } => RTy::Bytes,
        Expr::Str { .. } => RTy::String,
        Expr::Bool { .. } => RTy::Boolean,
        Expr::Path { segments, .. } => {
            if segments.len() == 1 {
                if segments[0].name == ctx.event_param {
                    return ctx.param_ty.clone();
                }
                if let Some(t) = locals.get(&segments[0].name) {
                    return t.clone();
                }
            }
            RTy::Unknown
        }
        Expr::Field { base, field, .. } => infer_field(base, &field.name, ctx, locals),
        Expr::Call { callee, .. } => infer_call(callee, ctx, locals),
        Expr::Binary { op, lhs, rhs, .. } => {
            use redstart_parser::ast::BinOp;
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
                let lt = infer(lhs, ctx, locals);
                if lt == RTy::Unknown {
                    infer(rhs, ctx, locals)
                } else {
                    lt
                }
            }
        }
        Expr::Unary { expr, .. } => infer(expr, ctx, locals),
        _ => RTy::Unknown,
    }
}

fn infer_field(base: &Expr, field: &str, ctx: &BodyCtx, locals: &HashMap<String, RTy>) -> RTy {
    match infer(base, ctx, locals) {
        RTy::Event => match field {
            "params" => RTy::Params,
            "block" => RTy::Block,
            "transaction" => RTy::Transaction,
            "address" => RTy::Address,
            "id" => RTy::Bytes,
            _ => RTy::Unknown,
        },
        RTy::Params => ctx.event_params.get(field).cloned().unwrap_or(RTy::Unknown),
        RTy::Call => match field {
            "inputs" => RTy::CallInputs,
            "outputs" => RTy::CallOutputs,
            "block" => RTy::Block,
            "transaction" => RTy::Transaction,
            "from" | "to" => RTy::Address,
            _ => RTy::Unknown,
        },
        RTy::CallInputs => ctx.event_params.get(field).cloned().unwrap_or(RTy::Unknown),
        RTy::CallOutputs => ctx.call_outputs.get(field).cloned().unwrap_or(RTy::Unknown),
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
        RTy::Entity(name) => ctx
            .entities
            .get(&name)
            .and_then(|e| e.fields.get(field))
            .cloned()
            .unwrap_or(RTy::Unknown),
        _ => RTy::Unknown,
    }
}

fn infer_call(callee: &Expr, ctx: &BodyCtx, locals: &HashMap<String, RTy>) -> RTy {
    if let Expr::Path { segments, .. } = callee {
        if segments.len() == 1 {
            if let Some(ret) = ctx.fn_returns.get(&segments[0].name) {
                return ret.clone();
            }
        }
    }
    if let Expr::Field { base, field, .. } = callee {
        if let Expr::Path { segments, .. } = base.as_ref() {
            if segments.len() == 1 {
                let base_name = &segments[0].name;
                if field.name == "bind" && ctx.abis.paths.contains_key(base_name) {
                    return RTy::Contract(base_name.clone());
                }
                // `Entity.load(id)` / `loadInBlock(id)` -> Option<Entity> (nullable).
                if matches!(field.name.as_str(), "load" | "loadInBlock")
                    && ctx.entities.contains_key(base_name)
                {
                    return RTy::Option(Box::new(RTy::Entity(base_name.clone())));
                }
                if base_name == "ipfs" && field.name == "cat" {
                    return RTy::Option(Box::new(RTy::Bytes));
                }
            }
        }
        if let RTy::Contract(abi) = infer(base, ctx, locals) {
            if let Some(outputs) = ctx.abis.function_outputs(&abi, &field.name) {
                let ret = outputs.first().map_or(RTy::Unknown, |s| sol_to_rty(s));
                return RTy::Result(Box::new(ret));
            }
        }
        match field.name.as_str() {
            "toDecimal" | "toBigDecimal" => return RTy::BigDecimal,
            "toBigInt" => return RTy::BigInt,
            "abs" | "plus" | "minus" | "times" | "div" => return infer(base, ctx, locals),
            _ => {}
        }
    }
    RTy::Unknown
}

// ---- small AST helpers ----

/// A record literal's fields, as found in a constructor call's second argument.
type CtorRecord<'a> = Option<&'a [(redstart_parser::Ident, Expr)]>;

/// Whole-program analysis for `immutable` inference (roadmap §4.3): an entity is
/// **append-only** if it's `create`d somewhere and *never* loaded (`load` /
/// `loadInBlock` / `loadOrCreate`) and never has a field assigned. Such entities
/// can be marked `@entity(immutable: true)` — graph-node stores them far more
/// cheaply. Conservative by construction: anything we can't prove append-only is
/// left mutable, and the store-diff gate is the backstop.
fn collect_immutable_entities(tree: &ModuleTree) -> HashSet<String> {
    let mut created: HashSet<String> = HashSet::new();
    let mut mutated: HashSet<String> = HashSet::new();
    for m in tree.ordered() {
        for h in &m.program.handlers {
            walk_mut(
                &h.body.stmts,
                &mut HashMap::new(),
                &mut created,
                &mut mutated,
            );
        }
        for f in &m.program.functions {
            walk_mut(
                &f.body.stmts,
                &mut HashMap::new(),
                &mut created,
                &mut mutated,
            );
        }
    }
    created.difference(&mutated).cloned().collect()
}

/// `Entity.<method>(...)` call → `(entity, method)` for the create/load family.
fn entity_call_method(value: &Expr) -> Option<(String, &str)> {
    let Expr::Call { callee, .. } = value else {
        return None;
    };
    let Expr::Field { base, field, .. } = callee.as_ref() else {
        return None;
    };
    let method = match field.name.as_str() {
        m @ ("create" | "loadOrCreate" | "load" | "loadInBlock") => m,
        _ => return None,
    };
    let Expr::Path { segments, .. } = base.as_ref() else {
        return None;
    };
    Some((segments.last()?.name.clone(), method))
}

fn note_entity_call(
    value: &Expr,
    locals: &mut HashMap<String, String>,
    bind: Option<&str>,
    created: &mut HashSet<String>,
    mutated: &mut HashSet<String>,
) {
    if let Some((entity, method)) = entity_call_method(value) {
        match method {
            "create" => {
                created.insert(entity.clone());
            }
            _ => {
                mutated.insert(entity.clone());
            }
        }
        if let Some(var) = bind {
            locals.insert(var.to_string(), entity);
        }
    }
}

fn walk_mut(
    stmts: &[Stmt],
    locals: &mut HashMap<String, String>,
    created: &mut HashSet<String>,
    mutated: &mut HashSet<String>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Let { name, value, .. } => {
                note_entity_call(value, locals, Some(&name.name), created, mutated);
                walk_expr_mut(value, locals, created, mutated);
            }
            Stmt::Assign { target, value, .. } => {
                // `entityVar.field = …` mutates that entity.
                if let Expr::Field { base, .. } = target {
                    if let Some(var) = path_name(base) {
                        if let Some(entity) = locals.get(&var) {
                            mutated.insert(entity.clone());
                        }
                    }
                }
                walk_expr_mut(value, locals, created, mutated);
            }
            Stmt::Return { value: Some(v), .. } => walk_expr_mut(v, locals, created, mutated),
            Stmt::Return { .. } => {}
            Stmt::If {
                cond,
                then_block,
                else_ifs,
                else_block,
                ..
            } => {
                walk_expr_mut(cond, locals, created, mutated);
                walk_mut(&then_block.stmts, &mut locals.clone(), created, mutated);
                for (c, b) in else_ifs {
                    walk_expr_mut(c, locals, created, mutated);
                    walk_mut(&b.stmts, &mut locals.clone(), created, mutated);
                }
                if let Some(b) = else_block {
                    walk_mut(&b.stmts, &mut locals.clone(), created, mutated);
                }
            }
            Stmt::While { cond, body, .. } => {
                walk_expr_mut(cond, locals, created, mutated);
                walk_mut(&body.stmts, &mut locals.clone(), created, mutated);
            }
            Stmt::For { body, .. } => {
                walk_mut(&body.stmts, &mut locals.clone(), created, mutated);
            }
            Stmt::Expr(e) => walk_expr_mut(e, locals, created, mutated),
        }
    }
}

fn walk_expr_mut(
    expr: &Expr,
    locals: &HashMap<String, String>,
    created: &mut HashSet<String>,
    mutated: &mut HashSet<String>,
) {
    // A create/load used as a bare expression (not let-bound) still counts.
    if let Some((entity, method)) = entity_call_method(expr) {
        if method == "create" {
            created.insert(entity);
        } else {
            mutated.insert(entity);
        }
    }
    match expr {
        Expr::Call { callee, args, .. } => {
            walk_expr_mut(callee, locals, created, mutated);
            for a in args {
                walk_expr_mut(a, locals, created, mutated);
            }
        }
        Expr::Field { base, .. } => walk_expr_mut(base, locals, created, mutated),
        Expr::Binary { lhs, rhs, .. } => {
            walk_expr_mut(lhs, locals, created, mutated);
            walk_expr_mut(rhs, locals, created, mutated);
        }
        Expr::Unary { expr, .. } => walk_expr_mut(expr, locals, created, mutated),
        Expr::Record { fields, .. } => {
            for (_, v) in fields {
                walk_expr_mut(v, locals, created, mutated);
            }
        }
        Expr::Match {
            scrutinee, arms, ..
        } => {
            walk_expr_mut(scrutinee, locals, created, mutated);
            for arm in arms {
                walk_mut(&arm.body.stmts, &mut locals.clone(), created, mutated);
            }
        }
        _ => {}
    }
}

/// Detect an `Entity.loadOrCreate(id, {..})` / `Entity.create(id, {..})` /
/// `Entity.load(id)` / `Entity.loadInBlock(id)` call, returning the entity name,
/// any record literal, and whether the result is nullable (`load`/`loadInBlock`).
fn entity_ctor(value: &Expr) -> Option<(String, CtorRecord<'_>, bool)> {
    let Expr::Call { callee, args, .. } = value else {
        return None;
    };
    let Expr::Field { base, field, .. } = callee.as_ref() else {
        return None;
    };
    let nullable = match field.name.as_str() {
        "loadOrCreate" | "create" => false,
        "load" | "loadInBlock" => true,
        _ => return None,
    };
    let Expr::Path { segments, .. } = base.as_ref() else {
        return None;
    };
    let entity = segments.last()?.name.clone();
    let record = match args.get(1) {
        Some(Expr::Record { fields, .. }) => Some(fields.as_slice()),
        _ => None,
    };
    Some((entity, record, nullable))
}

fn validate_type(
    ty: &TypeExpr,
    entity_names: &[String],
    aux_types: &[String],
    file: &str,
    diags: &mut Vec<Diag>,
) {
    match ty {
        TypeExpr::List { elem, .. } => validate_type(elem, entity_names, aux_types, file, diags),
        TypeExpr::Generic { base, args, .. } => {
            let name = base.simple_name().unwrap_or("");
            if !matches!(name, "Option" | "Id" | "List" | "Result") {
                diags.push(Diag::new(
                    file,
                    base.span(),
                    "E001",
                    format!("unknown generic type `{name}`"),
                    "not a known generic",
                ));
            }
            for a in args {
                validate_type(a, entity_names, aux_types, file, diags);
            }
        }
        TypeExpr::Path { .. } => {
            let name = ty.simple_name().unwrap_or("");
            if !is_scalar(name)
                && !entity_names.iter().any(|n| n == name)
                && !aux_types.iter().any(|n| n == name)
            {
                diags.push(
                    Diag::new(file, ty.span(), "E002", format!("unknown type `{name}`"), "not a scalar, entity, enum, or interface")
                        .with_help("did you forget to declare this entity/enum/interface, or misspell a scalar?"),
                );
            }
        }
    }
}

fn entity_name_of(ty: &TypeExpr) -> Option<String> {
    match ty {
        TypeExpr::List { elem, .. } => entity_name_of(elem),
        TypeExpr::Path { .. } => ty.simple_name().map(str::to_string),
        TypeExpr::Generic { .. } => None,
    }
}

fn get_setting<'a>(settings: &'a [Setting], key: &str) -> Option<&'a Setting> {
    settings.iter().find(|s| s.key.name == key)
}

fn path_setting(settings: &[Setting], key: &str) -> Option<String> {
    get_setting(settings, key).and_then(|s| path_name(&s.value))
}

/// Read a setting whose value may be a string literal (e.g. `network:
/// "arbitrum-one"`) or a bare identifier (e.g. `network: mainnet`).
fn string_setting(settings: &[Setting], key: &str) -> Option<String> {
    let s = get_setting(settings, key)?;
    match &s.value {
        Expr::Str { value, .. } => Some(value.clone()),
        _ => path_name(&s.value),
    }
}

fn path_name(expr: &Expr) -> Option<String> {
    if let Expr::Path { segments, .. } = expr {
        segments.last().map(|s| s.name.clone())
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
