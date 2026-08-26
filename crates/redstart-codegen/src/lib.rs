//! Code generation for Redstart.
//!
//! From a loaded [`ModuleTree`] and a validated [`Checked`] symbol table (from
//! `redstart-checker`), [`generate`] produces the artifacts a subgraph needs —
//! `schema.graphql`, `subgraph.yaml`, and the AssemblyScript mappings — from a
//! *single* unified source of truth spanning every module. Drift between them is
//! impossible because they are all projections of the same AST, and the resolved
//! types are computed once by the checker and shared here.

#![forbid(unsafe_code)]

mod lower;
mod manifest;
mod mappings;
mod names;
mod schema;

use lower::Env;
use manifest::ManifestInput;
use redstart_checker::Checked;
use redstart_checker::{resolve_type, RTy};
use redstart_loader::ModuleTree;
use redstart_parser::ast::{
    AggregationDecl, EntityDecl, EnumDecl, FnDecl, HandlerDecl, InterfaceDecl, SourceDecl,
    TemplateDecl,
};
use std::collections::HashMap;
use std::path::PathBuf;

/// The artifacts produced by a Redstart build.
#[derive(Debug)]
pub struct Generated {
    /// Contents of `schema.graphql`.
    pub schema: String,
    /// Contents of `subgraph.yaml`.
    pub manifest: String,
    /// Contents of `src/mappings.ts`.
    pub mappings: String,
    /// ABIs to copy into `abis/`, as `(file_name, source_path)`.
    pub abi_copies: Vec<(String, PathBuf)>,
    /// Non-fatal warnings raised during generation.
    pub warnings: Vec<String>,
    /// Informational optimisation notes (e.g. inferred `immutable`) — not problems.
    pub notes: Vec<String>,
}

impl Generated {
    /// Write all artifacts to `out_dir`, mirroring a hand-written subgraph layout
    /// so the output is directly ejectable into `graph codegen` / `graph build`.
    ///
    /// # Errors
    /// Returns any IO error encountered while creating files.
    pub fn write_to(&self, out_dir: &std::path::Path) -> std::io::Result<()> {
        std::fs::create_dir_all(out_dir.join("src"))?;
        std::fs::create_dir_all(out_dir.join("abis"))?;

        std::fs::write(out_dir.join("schema.graphql"), &self.schema)?;
        std::fs::write(out_dir.join("subgraph.yaml"), &self.manifest)?;
        std::fs::write(out_dir.join("src/mappings.ts"), &self.mappings)?;

        for (file_name, src) in &self.abi_copies {
            if src.exists() {
                let dest = out_dir.join("abis").join(file_name);
                match std::fs::read_to_string(src) {
                    // Normalise the ABI so `graph deploy` accepts it: graph-node
                    // requires every event to carry `anonymous`, which many ABIs
                    // (and `graph build`) omit. Fall back to a verbatim copy if the
                    // ABI isn't the JSON we expect.
                    Ok(text) => match normalize_abi(&text) {
                        Some(fixed) => std::fs::write(dest, fixed)?,
                        None => {
                            std::fs::copy(src, dest)?;
                        }
                    },
                    Err(_) => {
                        std::fs::copy(src, dest)?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// Ensure every `event` entry in an ABI JSON array carries an `anonymous` field
/// (defaulting to `false`). graph-node rejects ABIs without it at deploy time,
/// even though `graph build` is happy. Returns `None` if the text isn't a JSON
/// array (leave it untouched).
fn normalize_abi(text: &str) -> Option<String> {
    let mut value: serde_json::Value = serde_json::from_str(text).ok()?;
    let items = value.as_array_mut()?;
    let mut changed = false;
    for item in items {
        if item.get("type").and_then(|t| t.as_str()) == Some("event") {
            if let Some(obj) = item.as_object_mut() {
                if !obj.contains_key("anonymous") {
                    obj.insert("anonymous".into(), serde_json::Value::Bool(false));
                    changed = true;
                }
            }
        }
    }
    changed.then(|| serde_json::to_string_pretty(&value).unwrap_or_else(|_| text.to_string()))
}

/// Generate all artifacts from a loaded module tree and its checked symbol table.
#[must_use]
pub fn generate(tree: &ModuleTree, checked: &mut Checked) -> Generated {
    // Aggregate declarations across every module, in deterministic order.
    let mut entities: Vec<&EntityDecl> = Vec::new();
    let mut enums: Vec<&EnumDecl> = Vec::new();
    let mut interfaces: Vec<&InterfaceDecl> = Vec::new();
    let mut aggregations: Vec<&AggregationDecl> = Vec::new();
    let mut sources: Vec<&SourceDecl> = Vec::new();
    let mut templates: Vec<&TemplateDecl> = Vec::new();
    let mut handlers: Vec<&HandlerDecl> = Vec::new();
    let mut functions: Vec<&FnDecl> = Vec::new();

    for module in tree.ordered() {
        entities.extend(module.program.entities.iter());
        enums.extend(module.program.enums.iter());
        interfaces.extend(module.program.interfaces.iter());
        aggregations.extend(module.program.aggregations.iter());
        sources.extend(module.program.sources.iter());
        templates.extend(module.program.templates.iter());
        handlers.extend(module.program.handlers.iter());
        functions.extend(module.program.functions.iter());
    }

    let entity_names: Vec<String> = entities.iter().map(|e| e.name.name.clone()).collect();

    // ---- schema (with inferred `immutable` — roadmap §4.3) ----
    let schema = schema::render(
        &entities,
        &enums,
        &interfaces,
        &aggregations,
        &checked.immutable_inferred,
    );
    // Surface the optimisation so it isn't silent: only entities the user didn't
    // already mark immutable.
    let inferred: Vec<&str> = entities
        .iter()
        .filter(|e| {
            checked.immutable_inferred.contains(&e.name.name)
                && !e.modifiers.iter().any(|m| m.name == "immutable")
        })
        .map(|e| e.name.name.as_str())
        .collect();

    // ---- manifest ----
    let uses_aggregations = !aggregations.is_empty()
        || entities
            .iter()
            .any(|e| e.modifiers.iter().any(|m| m.name == "timeseries"));
    // Project-wide names and bind targets: both need every declaration at once,
    // so they are resolved here and shared by the manifest and the mappings —
    // the manifest's `handler:` entry *is* the exported function name.
    let names = names::Names::new(&handlers);
    let bound_abis = names::bound_abis(&handlers, &functions, &checked.abis.paths);

    let input = ManifestInput {
        name: &tree.name,
        description: tree.description.as_deref(),
        sources: &sources,
        templates: &templates,
        handlers: &handlers,
        entity_names: &entity_names,
        uses_aggregations,
        names: &names,
        bound_abis: &bound_abis,
    };
    let (manifest_src, mut warnings) = manifest::render(&input, &mut checked.abis);

    // ---- mappings (lowering uses the checked type tables) ----
    let (mappings_src, map_warnings) = {
        // Resolve each helper's return type so calls to them are typed.
        let entities_map = checked.entities.clone();
        let fn_returns: HashMap<String, RTy> = functions
            .iter()
            .filter_map(|f| {
                f.ret
                    .as_ref()
                    .map(|t| (f.name.name.clone(), resolve_type(t, &entities_map)))
            })
            .collect();
        let mut env = Env {
            entities: entities_map,
            source_abi: checked.source_abi.clone(),
            templates: templates.iter().map(|t| t.name.name.clone()).collect(),
            fn_returns,
            abis: &mut checked.abis,
        };
        mappings::render(
            &handlers,
            &functions,
            &entity_names,
            &names,
            &bound_abis,
            &mut env,
        )
    };
    warnings.extend(map_warnings);
    let mut notes = Vec::new();
    if !inferred.is_empty() {
        notes.push(format!(
            "inferred @entity(immutable: true) for {} (append-only — indexes faster, less disk)",
            inferred.join(", ")
        ));
    }
    notes.push(
        "indexerHints: prune: auto — smaller DB & faster queries (keeps only reorg history)"
            .to_string(),
    );

    // ---- ABI copies ----
    let abi_copies: Vec<(String, PathBuf)> = checked
        .abis
        .paths
        .iter()
        .map(|(name, path)| (format!("{name}.json"), path.clone()))
        .collect();

    Generated {
        schema,
        manifest: manifest_src,
        mappings: mappings_src,
        abi_copies,
        warnings,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn build(main_red: &str, abi: &str) -> Generated {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/abis")).unwrap();
        fs::write(
            dir.path().join("redstart.toml"),
            "[project]\nname = \"erc20\"\nentry = \"src/main.red\"",
        )
        .unwrap();
        fs::write(dir.path().join("src/abis/ERC20.json"), abi).unwrap();
        fs::write(dir.path().join("src/main.red"), main_red).unwrap();

        let tree = redstart_loader::load(dir.path()).unwrap();
        let mut checked = redstart_checker::check(&tree).expect("check should pass");
        generate(&tree, &mut checked)
    }

    const TRANSFER_ABI: &str = r#"[{"type":"event","name":"Transfer","inputs":[
        {"name":"from","type":"address","indexed":true},
        {"name":"to","type":"address","indexed":true},
        {"name":"value","type":"uint256","indexed":false}]}]"#;

    fn build_demo() -> Generated {
        build(
            r#"
abi ERC20 from "./abis/ERC20.json"
entity Account { id: Id<Bytes> balance: BigInt }
source Token {
  abi: ERC20
  network: mainnet
  address: 0x1234567890abcdef1234567890abcdef12345678
  startBlock: 1
}
handler on Token.Transfer(event) {
  let acct = Account.loadOrCreate(event.params.to, { balance: BigInt.zero })
  acct.balance = acct.balance + event.params.value
}
"#,
            TRANSFER_ABI,
        )
    }

    #[test]
    fn schema_and_manifest_reflect_source() {
        let gen = build_demo();
        assert!(gen
            .schema
            .contains("type Account @entity(immutable: false) {"));
        assert!(gen.schema.contains("balance: BigInt!"));
        assert!(gen
            .manifest
            .contains("event: Transfer(indexed address,indexed address,uint256)"));
        assert!(gen.manifest.contains("handler: handleTransfer"));
        assert!(gen.manifest.contains("file: ./src/mappings.ts"));
    }

    #[test]
    fn lowering_produces_faithful_assemblyscript() {
        let gen = build_demo();
        let m = &gen.mappings;
        assert!(m.contains("let acct = Account.load(event.params.to)"));
        assert!(m.contains("if (acct == null) {"));
        assert!(m.contains("acct = new Account(event.params.to)"));
        assert!(m.contains("acct.balance = BigInt.zero()"));
        assert!(m.contains("acct.balance = acct.balance.plus(event.params.value)"));
        assert!(m.contains("acct.save()"));
        assert!(
            m.contains("import { Transfer as TransferEvent } from \"../generated/Token/ERC20\"")
        );
        assert!(m.contains("import { Account } from \"../generated/schema\""));
        assert!(m.contains("import { BigInt } from \"@graphprotocol/graph-ts\""));
        assert!(gen.warnings.is_empty(), "warnings: {:?}", gen.warnings);
    }

    const CALLS_ABI: &str = r#"[
      {"type":"event","name":"Approval","inputs":[
        {"name":"owner","type":"address","indexed":true},
        {"name":"spender","type":"address","indexed":true},
        {"name":"value","type":"uint256","indexed":false}]},
      {"type":"function","name":"balanceOf","stateMutability":"view",
        "inputs":[{"name":"account","type":"address"}],
        "outputs":[{"name":"","type":"uint256"}]}
    ]"#;

    #[test]
    fn contract_calls_and_match_lower_correctly() {
        let gen = build(
            r#"
abi ERC20 from "./abis/ERC20.json"
entity Account { id: Id<Bytes> balance: BigInt }
source Token {
  abi: ERC20
  network: mainnet
  address: 0x1234567890abcdef1234567890abcdef12345678
  startBlock: 1
}
handler on Token.Approval(event) {
  let result = ERC20.bind(event.address).balanceOf(event.params.owner)
  match result {
    Ok(bal) => {
      let owner = Account.loadOrCreate(event.params.owner, { balance: BigInt.zero })
      owner.balance = bal
    }
    Err(e) => {}
  }
}
"#,
            CALLS_ABI,
        );
        let m = &gen.mappings;
        assert!(
            m.contains("let result = ERC20.bind(event.address).try_balanceOf(event.params.owner)")
        );
        assert!(m.contains("if (!result.reverted) {"));
        assert!(m.contains("let bal = result.value"));
        assert!(m.contains("owner.save()"));
        assert!(!m.contains("else {"));
        assert!(m.contains("import { ERC20, Approval as ApprovalEvent }"));
        assert!(gen.warnings.is_empty(), "warnings: {:?}", gen.warnings);
    }

    #[test]
    fn control_flow_lowers_to_native_assemblyscript() {
        let gen = build(
            r#"
abi ERC20 from "./abis/ERC20.json"
entity Account { id: Id<Bytes> balance: BigInt }
source Token {
  abi: ERC20
  network: mainnet
  address: 0x1234567890abcdef1234567890abcdef12345678
  startBlock: 1
}
handler on Token.Transfer(event) {
  let acct = Account.loadOrCreate(event.params.to, { balance: BigInt.zero })
  if event.params.value > BigInt.zero {
    acct.balance = acct.balance + event.params.value
  } else {
    acct.balance = BigInt.zero
  }
  for i in 0..3 {
    acct.balance = acct.balance + event.params.value
  }
}
"#,
            TRANSFER_ABI,
        );
        let m = &gen.mappings;
        // BigInt comparison in the condition lowers to a method call, not `>`.
        assert!(
            m.contains("if (event.params.value.gt(BigInt.zero())) {"),
            "got:\n{m}"
        );
        assert!(m.contains("} else {"), "got:\n{m}");
        // Numeric range becomes a counted native `for`.
        assert!(m.contains("for (let i = 0; i < 3; i++) {"), "got:\n{m}");
        assert!(gen.warnings.is_empty(), "warnings: {:?}", gen.warnings);
    }

    const FN_ABI: &str = r#"[
      {"type":"function","name":"transfer","stateMutability":"nonpayable",
        "inputs":[{"name":"to","type":"address"},{"name":"amount","type":"uint256"}],
        "outputs":[{"name":"success","type":"bool"}]}
    ]"#;

    #[test]
    fn call_and_block_handlers_lower_and_manifest() {
        let gen = build(
            r#"
abi ERC20 from "./abis/ERC20.json"
entity Account { id: Id<Bytes> balance: BigInt }
entity Snapshot { id: Id<Bytes> total: BigInt }
source Token {
  abi: ERC20
  network: mainnet
  address: 0x1234567890abcdef1234567890abcdef12345678
  startBlock: 1
}
handler call Token.transfer(call) {
  let acct = Account.loadOrCreate(call.inputs.to, { balance: BigInt.zero })
  acct.balance = acct.balance + call.inputs.amount
}
handler block Token(block) every 100 {
  let snap = Snapshot.create(block.hash, { total: BigInt.zero })
  snap.total = block.number
}
"#,
            FN_ABI,
        );
        // Manifest: callHandlers + blockHandlers with filter.
        assert!(
            gen.manifest.contains("callHandlers:"),
            "manifest:\n{}",
            gen.manifest
        );
        assert!(gen.manifest.contains("function: transfer(address,uint256)"));
        assert!(gen.manifest.contains("handler: handleTransferCall"));
        assert!(gen.manifest.contains("blockHandlers:"));
        assert!(gen.manifest.contains("handler: handleTokenBlock"));
        assert!(gen.manifest.contains("kind: polling"));
        assert!(gen.manifest.contains("every: 100"));
        // Mappings: correct param types and member access.
        let m = &gen.mappings;
        assert!(
            m.contains("export function handleTransferCall(call: TransferCall): void"),
            "got:\n{m}"
        );
        assert!(m.contains("acct.balance.plus(call.inputs.amount)"));
        assert!(m.contains("export function handleTokenBlock(block: ethereum.Block): void"));
        assert!(m.contains("snap.total = block.number"));
        assert!(m.contains("import { TransferCall } from \"../generated/Token/ERC20\""));
        assert!(m.contains("ethereum"));
        assert!(gen.warnings.is_empty(), "warnings: {:?}", gen.warnings);
    }

    #[test]
    fn template_instantiation_lowers_and_imports() {
        let gen = build(
            r#"
abi ERC20 from "./abis/ERC20.json"
entity Pair { id: Id<Bytes> }
source Token {
  abi: ERC20
  network: mainnet
  address: 0x1234567890abcdef1234567890abcdef12345678
  startBlock: 1
}
template PairTemplate {
  abi: ERC20
  network: mainnet
}
handler on Token.Transfer(event) {
  PairTemplate.create(event.params.to)
}
"#,
            TRANSFER_ABI,
        );
        assert!(
            gen.manifest.contains("templates:"),
            "manifest:\n{}",
            gen.manifest
        );
        assert!(gen.manifest.contains("name: PairTemplate"));
        let m = &gen.mappings;
        assert!(
            m.contains("PairTemplate.create(event.params.to)"),
            "got:\n{m}"
        );
        assert!(
            m.contains("import { PairTemplate } from \"../generated/templates\""),
            "got:\n{m}"
        );
        assert!(gen.warnings.is_empty(), "warnings: {:?}", gen.warnings);
    }

    #[test]
    fn template_handler_imports_use_templates_path() {
        // Regression: `graph codegen` writes a template's ABI types under
        // `generated/templates/<Template>/<Abi>.ts`, not `generated/<Template>/…`.
        // A wrong path makes `graph build` fail with TS6054 (file not found).
        let gen = build(
            r#"
abi ERC20 from "./abis/ERC20.json"
entity Account { id: Id<Bytes> balance: BigInt }
source Token {
  abi: ERC20
  network: mainnet
  address: 0x1234567890abcdef1234567890abcdef12345678
  startBlock: 1
}
template TokenTemplate {
  abi: ERC20
  network: mainnet
}
handler on TokenTemplate.Transfer(event) {
  let a = Account.loadOrCreate(event.params.to, { balance: BigInt.zero })
  a.balance = a.balance + event.params.value
}
"#,
            TRANSFER_ABI,
        );
        let m = &gen.mappings;
        assert!(
            m.contains("from \"../generated/templates/TokenTemplate/ERC20\""),
            "template handler must import from generated/templates/…, got:\n{m}"
        );
        // A regular data source keeps the plain path.
        assert!(
            !m.contains("from \"../generated/TokenTemplate/ERC20\""),
            "got:\n{m}"
        );
        assert!(gen.warnings.is_empty(), "warnings: {:?}", gen.warnings);
    }

    #[test]
    fn graph_ts_namespaces_and_statics() {
        let gen = build(
            r#"
abi ERC20 from "./abis/ERC20.json"
entity Account { id: Id<Bytes> balance: BigInt score: BigInt }
source Token {
  abi: ERC20
  network: mainnet
  address: 0x1234567890abcdef1234567890abcdef12345678
  startBlock: 1
}
handler on Token.Transfer(event) {
  log.info("v {}", [event.params.value.toString()])
  let acct = Account.loadOrCreate(event.params.to, { balance: BigInt.zero, score: BigInt.fromI32(0) })
  let h = crypto.keccak256(event.params.from)
  acct.score = BigInt.fromI32(10).pow(2) + acct.score
}
"#,
            TRANSFER_ABI,
        );
        let m = &gen.mappings;
        // Whole-word import detection: log/crypto imported, logIndex is not a false positive.
        assert!(
            m.contains("import { BigInt, crypto, log } from \"@graphprotocol/graph-ts\""),
            "got:\n{m}"
        );
        // A BigInt static chained through `.pow()` is still BigInt, so `+` lowers to `.plus()`.
        assert!(
            m.contains("BigInt.fromI32(10).pow(2).plus(acct.score)"),
            "got:\n{m}"
        );
        assert!(m.contains("crypto.keccak256(event.params.from)"));
        assert!(gen.warnings.is_empty(), "warnings: {:?}", gen.warnings);
    }

    #[test]
    fn file_data_source_manifest_and_handler() {
        let gen = build(
            r#"
abi ERC20 from "./abis/ERC20.json"
entity Token { id: Id<Bytes> uri: String }
source Src {
  abi: ERC20
  network: mainnet
  address: 0x1234567890abcdef1234567890abcdef12345678
  startBlock: 1
}
template Meta {
  kind: file
}
handler on Src.Transfer(event) {
  Meta.create("QmCid")
}
handler file Meta(content) {
  let v = json.fromBytes(content)
  let cid = dataSource.stringParam()
  let t = Token.create(Bytes.fromUTF8(cid), { uri: cid })
}
"#,
            TRANSFER_ABI,
        );
        // file/ipfs template with a single `handler:` and no network/address.
        assert!(
            gen.manifest.contains("kind: file/ipfs"),
            "manifest:\n{}",
            gen.manifest
        );
        assert!(gen.manifest.contains("name: Meta"));
        assert!(gen.manifest.contains("handler: handleMeta"));
        let m = &gen.mappings;
        assert!(
            m.contains("export function handleMeta(content: Bytes): void"),
            "got:\n{m}"
        );
        assert!(m.contains("json.fromBytes(content)"));
        assert!(m.contains("dataSource.stringParam()"));
        assert!(gen.warnings.is_empty(), "warnings: {:?}", gen.warnings);
    }

    #[test]
    fn nullable_load_matches_and_autosaves() {
        let gen = build(
            r#"
abi ERC20 from "./abis/ERC20.json"
entity Account { id: Id<Bytes> balance: BigInt }
source Token {
  abi: ERC20
  network: mainnet
  address: 0x1234567890abcdef1234567890abcdef12345678
  startBlock: 1
}
handler on Token.Transfer(event) {
  let acct = Account.load(event.params.to)
  match acct {
    Some(a) => {
      a.balance = a.balance + event.params.value
    }
    None => {}
  }
}
"#,
            TRANSFER_ABI,
        );
        let m = &gen.mappings;
        // load() is nullable -> match lowers to a null-check; the bound entity
        // is auto-saved inside the arm.
        assert!(
            m.contains("let acct = Account.load(event.params.to)"),
            "got:\n{m}"
        );
        assert!(m.contains("if (acct !== null) {"), "got:\n{m}");
        assert!(m.contains("let a = acct!"), "got:\n{m}");
        assert!(
            m.contains("a.balance = a.balance.plus(event.params.value)"),
            "got:\n{m}"
        );
        assert!(
            m.contains("a.save()"),
            "matched entity must auto-save, got:\n{m}"
        );
        assert!(gen.warnings.is_empty(), "warnings: {:?}", gen.warnings);
    }

    /// Found by porting PancakeSwap Infinity CL (nightswatchhq/pancakeswap-
    /// infinity-cl-redstart). Three separate emissions that `check` accepted and
    /// only `graph build` rejected, which is exactly the bug class the porting
    /// guide asks to be reported.
    #[test]
    fn option_and_entity_ctor_emissions_compile() {
        let gen = build(
            r#"
abi ERC20 from "./abis/ERC20.json"
entity Note {
  id: Id<String>
  n: Option<BigInt>
}
entity Log {
  id: Id<String>
  n: BigInt
}
source Token {
  abi: ERC20
  network: mainnet
  address: 0x1234567890abcdef1234567890abcdef12345678
  startBlock: 1
}
fn mk(id: String) -> Note {
  return Note.loadOrCreate(id, { n: None })
}
handler on Token.Transfer(event) {
  let note = mk("1")
  note.n = Some(event.params.value)
  match note.n {
    Some(v) => {
      Log.create(v.toString(), { n: v })
    }
    None => {}
  }
}
"#,
            TRANSFER_ABI,
        );
        let m = &gen.mappings;

        // 1. `Some(x)` is sugar for `x`; emitting a call produced a reference to
        //    an undefined `Some` in the generated AssemblyScript.
        assert!(!m.contains("Some("), "`Some(` must not survive, got:\n{m}");
        assert!(
            m.contains("note.n = event.params.value"),
            "Some(x) must lower to x, got:\n{m}"
        );

        // 2. graph-ts gives BigInt an `@operator('!=')`, so a loose null test
        //    crashes the AssemblyScript compiler in compileBinaryOverload.
        assert!(
            !m.contains("!= null"),
            "null tests must be strict, got:\n{m}"
        );

        // 3. A create in statement position must expand, not emit a call to a
        //    non-existent static with a `/* record */` placeholder argument.
        assert!(
            !m.contains("/* record */"),
            "record literal must be expanded, got:\n{m}"
        );
        assert!(m.contains("new Log("), "got:\n{m}");

        // 4. …and so must one in return position.
        assert!(m.contains("new Note("), "got:\n{m}");

        assert!(gen.warnings.is_empty(), "warnings: {:?}", gen.warnings);
    }

    /// The guard on the fix above: `<Template>.create(addr)` is a data-source
    /// spawn, not an entity write, and in statement position the two shapes are
    /// textually identical.
    #[test]
    fn template_create_in_statement_position_is_not_an_entity_ctor() {
        let gen = build(
            r#"
abi ERC20 from "./abis/ERC20.json"
entity Thing {
  id: Id<String>
}
source Token {
  abi: ERC20
  network: mainnet
  address: 0x1234567890abcdef1234567890abcdef12345678
  startBlock: 1
}
template Pool {
  abi: ERC20
  network: mainnet
}
handler on Token.Transfer(event) {
  Pool.create(event.params.to)
}
"#,
            TRANSFER_ABI,
        );
        let m = &gen.mappings;
        assert!(
            m.contains("Pool.create(event.params.to)"),
            "template spawn must survive verbatim, got:\n{m}"
        );
        assert!(!m.contains("new Pool("), "got:\n{m}");
    }

    #[test]
    fn free_fn_lowers_with_return_flush_and_typed_calls() {
        let gen = build(
            r#"
abi ERC20 from "./abis/ERC20.json"
entity Account { id: Id<Bytes> balance: BigInt }
source Token {
  abi: ERC20
  network: mainnet
  address: 0x1234567890abcdef1234567890abcdef12345678
  startBlock: 1
}
fn getOrCreateAccount(addr: Bytes) -> Account {
  let acct = Account.loadOrCreate(addr, { balance: BigInt.zero })
  return acct
}
handler on Token.Transfer(event) {
  let acct = getOrCreateAccount(event.params.to)
  acct.balance = acct.balance + event.params.value
}
"#,
            TRANSFER_ABI,
        );
        let m = &gen.mappings;
        // The helper is emitted as a function; its save precedes the `return`.
        assert!(
            m.contains("function getOrCreateAccount(addr: Bytes): Account {"),
            "got:\n{m}"
        );
        let helper = &m[m.find("function getOrCreateAccount").unwrap()..];
        let save_pos = helper.find("acct.save()").expect("helper saves");
        let ret_pos = helper.find("return acct").expect("helper returns");
        assert!(
            save_pos < ret_pos,
            "save must precede return, got:\n{helper}"
        );
        // The helper's return type flows into the caller: `+` lowers to `.plus()`
        // and the result auto-saves.
        assert!(
            m.contains("acct.balance = acct.balance.plus(event.params.value)"),
            "got:\n{m}"
        );
        assert!(gen.warnings.is_empty(), "warnings: {:?}", gen.warnings);
    }

    // ---- porting-friction regressions (v0.16.0) ----

    /// Build a project with two ABI files, so cross-ABI binds can be exercised.
    fn build_two_abi(main_red: &str, abi_a: (&str, &str), abi_b: (&str, &str)) -> Generated {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/abis")).unwrap();
        fs::write(
            dir.path().join("redstart.toml"),
            "[project]\nname = \"two\"\nentry = \"src/main.red\"\ndescription = \"has: a colon\"",
        )
        .unwrap();
        fs::write(
            dir.path().join(format!("src/abis/{}.json", abi_a.0)),
            abi_a.1,
        )
        .unwrap();
        fs::write(
            dir.path().join(format!("src/abis/{}.json", abi_b.0)),
            abi_b.1,
        )
        .unwrap();
        fs::write(dir.path().join("src/main.red"), main_red).unwrap();

        let tree = redstart_loader::load(dir.path()).unwrap();
        let mut checked = redstart_checker::check(&tree).expect("check should pass");
        generate(&tree, &mut checked)
    }

    const READER_ABI: &str = r#"[{"type":"function","name":"balanceOf","stateMutability":"view",
        "inputs":[{"name":"account","type":"address"}],
        "outputs":[{"name":"","type":"uint256"}]}]"#;

    #[test]
    fn two_sources_sharing_an_event_get_distinct_symbols() {
        // Same event name on two sources used to emit two `handleTransfer`s and
        // two `Transfer as TransferEvent` imports — `graph build` rejected both.
        let gen = build(
            r#"
abi ERC20 from "./abis/ERC20.json"
entity Account { id: Id<Bytes> balance: BigInt }
source Token { abi: ERC20 network: mainnet address: 0x1234567890abcdef1234567890abcdef12345678 startBlock: 1 }
source Other { abi: ERC20 network: mainnet address: 0xabcdefabcdefabcdefabcdefabcdefabcdefabcd startBlock: 1 }
handler on Token.Transfer(event) { let a = Account.loadOrCreate(event.params.to, { balance: BigInt.zero }) }
handler on Other.Transfer(event) { let a = Account.loadOrCreate(event.params.to, { balance: BigInt.zero }) }
"#,
            TRANSFER_ABI,
        );
        let m = &gen.mappings;
        assert!(m.contains("export function handleTokenTransfer("));
        assert!(m.contains("export function handleOtherTransfer("));
        assert!(!m.contains("export function handleTransfer("));
        assert!(m.contains("Transfer as TokenTransferEvent"));
        assert!(m.contains("Transfer as OtherTransferEvent"));
        // …and the manifest points each source at its own function.
        assert!(gen.manifest.contains("handler: handleTokenTransfer"));
        assert!(gen.manifest.contains("handler: handleOtherTransfer"));
    }

    #[test]
    fn a_bound_abi_is_imported_and_listed_in_the_manifest() {
        // Reading another contract used to emit `Reader.bind(...)` with no import
        // and no manifest entry: `Cannot find name 'Reader'` on eject.
        let gen = build_two_abi(
            r#"
abi ERC20 from "./abis/ERC20.json"
abi Reader from "./abis/Reader.json"
entity Account { id: Id<Bytes> balance: BigInt }
source Token { abi: ERC20 network: mainnet address: 0x1234567890abcdef1234567890abcdef12345678 startBlock: 1 }
handler on Token.Transfer(event) {
  match Reader.bind(event.address).balanceOf(event.params.to) {
    Ok(v) => { let a = Account.loadOrCreate(event.params.to, { balance: v }) }
    Err(e) => { }
  }
}
"#,
            ("ERC20", TRANSFER_ABI),
            ("Reader", READER_ABI),
        );
        assert!(gen
            .mappings
            .contains("import { Reader } from \"../generated/Token/Reader\""));
        assert!(gen.manifest.contains("- name: Reader"));
        assert!(gen.manifest.contains("file: ./abis/Reader.json"));
        // A description containing a colon must still be valid YAML.
        assert!(gen.manifest.contains("description: \"has: a colon\""));
    }

    #[test]
    fn none_lowers_to_null_so_the_generated_setter_unsets() {
        let gen = build(
            r#"
abi ERC20 from "./abis/ERC20.json"
entity Account { id: Id<Bytes> balance: BigInt label: Option<String> }
source Token { abi: ERC20 network: mainnet address: 0x1234567890abcdef1234567890abcdef12345678 startBlock: 1 }
handler on Token.Transfer(event) {
  let a = Account.loadOrCreate(event.params.to, { balance: BigInt.zero })
  a.label = None
}
"#,
            TRANSFER_ABI,
        );
        assert!(gen.mappings.contains("a.label = null"));
    }
}

#[cfg(test)]
mod abi_norm_tests {
    use super::normalize_abi;

    #[test]
    fn adds_anonymous_to_events_only() {
        let abi = r#"[{"type":"event","name":"Transfer","inputs":[]},{"type":"function","name":"balanceOf","inputs":[],"outputs":[]}]"#;
        let out = normalize_abi(abi).expect("should change");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr[0]["anonymous"], serde_json::Value::Bool(false));
        assert!(
            arr[1].get("anonymous").is_none(),
            "functions don't get anonymous"
        );
    }

    #[test]
    fn leaves_complete_abi_untouched() {
        let abi = r#"[{"type":"event","name":"T","anonymous":false,"inputs":[]}]"#;
        assert!(normalize_abi(abi).is_none(), "no change -> None");
    }

    #[test]
    fn non_json_is_left_alone() {
        assert!(normalize_abi("not json").is_none());
    }
}
