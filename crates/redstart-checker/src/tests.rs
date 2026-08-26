//! Checker integration tests: each writes a tiny project and asserts whether
//! `check` accepts it or rejects it with the expected diagnostic.

use crate::check;
use std::fs;

const ABI: &str = r#"[
  {"type":"event","name":"Transfer","inputs":[
    {"name":"from","type":"address","indexed":true},
    {"name":"to","type":"address","indexed":true},
    {"name":"value","type":"uint256","indexed":false}]},
  {"type":"function","name":"balanceOf","stateMutability":"view",
    "inputs":[{"name":"account","type":"address"}],
    "outputs":[{"name":"","type":"uint256"}]}
]"#;

/// Build a one-file project from `src` and run the checker.
fn run(src: &str) -> Result<(), Vec<String>> {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/abis")).unwrap();
    fs::write(
        dir.path().join("redstart.toml"),
        "[project]\nname = \"t\"\nentry = \"src/main.red\"",
    )
    .unwrap();
    fs::write(dir.path().join("src/abis/ERC20.json"), ABI).unwrap();
    fs::write(dir.path().join("src/main.red"), src).unwrap();

    let tree = redstart_loader::load(dir.path()).unwrap();
    check(&tree).map(|_| ())
}

const PREAMBLE: &str = r#"
abi ERC20 from "./abis/ERC20.json"
entity Account { id: Id<Bytes> balance: BigInt }
source Token {
  abi: ERC20
  network: mainnet
  address: 0x1234567890abcdef1234567890abcdef12345678
  startBlock: 1
}
"#;

fn with_handler(body: &str) -> String {
    format!("{PREAMBLE}\nhandler on Token.Transfer(event) {{\n{body}\n}}\n")
}

fn assert_err_contains(result: Result<(), Vec<String>>, needle: &str) {
    let errs = result.expect_err("expected a check error");
    let joined = errs.join("\n");
    assert!(joined.contains(needle), "expected `{needle}` in:\n{joined}");
}

#[test]
fn valid_program_passes() {
    let ok = with_handler(
        "let acct = Account.loadOrCreate(event.params.to, { balance: BigInt.zero })\n\
         acct.balance = acct.balance + event.params.value",
    );
    assert!(run(&ok).is_ok());
}

#[test]
fn missing_required_field_is_rejected() {
    let src = with_handler("let acct = Account.loadOrCreate(event.params.to, {})");
    assert_err_contains(run(&src), "missing required field(s): balance");
}

#[test]
fn unknown_record_field_is_rejected() {
    let src = with_handler(
        "let acct = Account.loadOrCreate(event.params.to, { balance: BigInt.zero, bogus: BigInt.zero })",
    );
    assert_err_contains(run(&src), "has no field `bogus`");
}

#[test]
fn unknown_source_is_rejected() {
    let src = format!("{PREAMBLE}\nhandler on Nope.Transfer(event) {{ }}\n");
    assert_err_contains(run(&src), "unknown source `Nope`");
}

#[test]
fn unknown_event_is_rejected() {
    let src = format!("{PREAMBLE}\nhandler on Token.Nope(event) {{ }}\n");
    assert_err_contains(run(&src), "event `Nope` not found");
}

#[test]
fn assign_to_derived_field_is_rejected() {
    let src = format!(
        "{}\nentity Pool {{ id: Id<Bytes> accs: [Account] derived from owner }}\n\
         handler on Token.Transfer(event) {{\n\
           let p = Pool.loadOrCreate(event.address, {{}})\n\
           p.accs = event.params.value\n\
         }}\n",
        PREAMBLE
    );
    // Account needs an `owner` field for the derive to be valid; add it.
    let src = src.replace(
        "entity Account { id: Id<Bytes> balance: BigInt }",
        "entity Account { id: Id<Bytes> balance: BigInt owner: Account }",
    );
    assert_err_contains(run(&src), "cannot assign to derived field `accs`");
}

#[test]
fn reading_call_value_without_match_is_rejected() {
    let src = with_handler(
        "let r = ERC20.bind(event.address).balanceOf(event.params.to)\n\
         let b = r.value",
    );
    assert_err_contains(run(&src), "cannot read `.value` of a contract call");
}

#[test]
fn arithmetic_on_option_is_rejected() {
    let src = format!(
        "{}\nentity Acc {{ id: Id<Bytes> bal: Option<BigInt> }}\n\
         handler on Token.Transfer(event) {{\n\
           let a = Acc.loadOrCreate(event.params.to, {{}})\n\
           let x = a.bal + event.params.value\n\
         }}\n",
        PREAMBLE
    );
    assert_err_contains(run(&src), "cannot do arithmetic on an `Option`");
}

#[test]
fn deref_of_nullable_load_is_rejected() {
    // `load` returns `Option<Entity>` — touching a field without `match`ing first
    // is a null-deref, caught at compile time.
    let src = with_handler(
        "let a = Account.load(event.params.to)\n\
         let b = a.balance",
    );
    assert_err_contains(run(&src), "cannot access `.balance` on a nullable value");
}

#[test]
fn matched_load_is_accepted() {
    let ok = with_handler(
        "let a = Account.load(event.params.to)\n\
         match a {\n  Some(acct) => { let b = acct.balance }\n  None => {}\n}",
    );
    assert!(run(&ok).is_ok(), "matched load should type-check");
}

#[test]
fn unknown_type_is_rejected() {
    let src = format!("{PREAMBLE}\nentity Bad {{ id: Id<Bytes> thing: Wibble }}\n");
    assert_err_contains(run(&src), "unknown type `Wibble`");
}

#[test]
fn duplicate_entity_is_rejected() {
    let src = format!("{PREAMBLE}\nentity Account {{ id: Id<Bytes> balance: BigInt }}\n");
    assert_err_contains(run(&src), "duplicate entity `Account`");
}

#[test]
fn missing_source_setting_is_rejected() {
    let src = "abi ERC20 from \"./abis/ERC20.json\"\n\
               entity Account { id: Id<Bytes> }\n\
               source Token { abi: ERC20 network: mainnet }\n";
    assert_err_contains(run(src), "missing `address`");
}

#[test]
fn unknown_entity_field_is_rejected() {
    let src = with_handler(
        "let acct = Account.loadOrCreate(event.params.to, { balance: BigInt.zero })\n\
         acct.blance = event.params.value",
    );
    assert_err_contains(run(&src), "`Account` has no field `blance`");
}

#[test]
fn non_exhaustive_match_is_rejected() {
    let src = with_handler(
        "let r = ERC20.bind(event.address).balanceOf(event.params.to)\n\
         match r {\n  Ok(b) => {}\n}",
    );
    assert_err_contains(run(&src), "non-exhaustive `match`: missing Err");
}

#[test]
fn wildcard_match_is_exhaustive() {
    let ok = with_handler(
        "let r = ERC20.bind(event.address).balanceOf(event.params.to)\n\
         match r {\n  Ok(b) => {}\n  _ => {}\n}",
    );
    assert!(run(&ok).is_ok());
}

#[test]
fn unknown_contract_function_is_rejected() {
    let src = with_handler("let r = ERC20.bind(event.address).totalSupply(event.params.to)");
    assert_err_contains(run(&src), "contract `ERC20` has no function `totalSupply`");
}

#[test]
fn derived_backref_must_exist() {
    let src =
        format!("{PREAMBLE}\nentity Pool {{ id: Id<Bytes> accs: [Account] derived from nope }}\n");
    assert_err_contains(run(&src), "has no field `nope`");
}

#[test]
fn enum_typed_field_is_accepted() {
    let src = format!(
        "{PREAMBLE}\nenum Kind {{ Mint, Burn }}\nentity Tx {{ id: Id<Bytes> kind: Kind at: Timestamp }}\n"
    );
    assert!(run(&src).is_ok(), "enum/Timestamp field should type-check");
}

#[test]
fn unknown_field_type_still_rejected() {
    let src = format!("{PREAMBLE}\nentity Tx {{ id: Id<Bytes> kind: Nonsense }}\n");
    assert_err_contains(run(&src), "unknown type `Nonsense`");
}

#[test]
fn entity_implementing_interface_passes() {
    let src = format!(
        "{PREAMBLE}\ninterface Named {{ id: Id<Bytes> name: String }}\n\
         entity Thing implements Named {{ id: Id<Bytes> name: String extra: BigInt }}\n"
    );
    assert!(run(&src).is_ok(), "valid implements should pass");
}

#[test]
fn missing_interface_field_is_rejected() {
    let src = format!(
        "{PREAMBLE}\ninterface Named {{ id: Id<Bytes> name: String }}\n\
         entity Thing implements Named {{ id: Id<Bytes> }}\n"
    );
    assert_err_contains(run(&src), "missing field `name`");
}

#[test]
fn implementing_unknown_interface_is_rejected() {
    let src = format!("{PREAMBLE}\nentity Thing implements Ghost {{ id: Id<Bytes> }}\n");
    assert_err_contains(run(&src), "unknown interface `Ghost`");
}

#[test]
fn rejects_nondeterministic_date_now() {
    let body = "  let t = Date.now()\n  let a = Account.loadOrCreate(event.params.to, { balance: BigInt.zero })";
    assert_err_contains(run(&with_handler(body)), "E080");
}

#[test]
fn rejects_math_random() {
    let body = "  let r = Math.random()\n  let a = Account.loadOrCreate(event.params.to, { balance: BigInt.zero })";
    assert_err_contains(run(&with_handler(body)), "non-deterministic");
}

/// Build a one-file project and return ALL diagnostics (including warnings).
fn diags_of(src: &str) -> Vec<crate::Diag> {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/abis")).unwrap();
    fs::write(
        dir.path().join("redstart.toml"),
        "[project]\nname = \"t\"\nentry = \"src/main.red\"",
    )
    .unwrap();
    fs::write(dir.path().join("src/abis/ERC20.json"), ABI).unwrap();
    fs::write(dir.path().join("src/main.red"), src).unwrap();
    let tree = redstart_loader::load(dir.path()).unwrap();
    crate::check_diags(&tree)
}

fn assert_warns(src: &str, code: &str) {
    let diags = diags_of(src);
    assert!(
        diags
            .iter()
            .any(|d| d.code_short() == code && !d.is_error()),
        "expected warning {code}; got: {:?}",
        diags.iter().map(|d| d.code_short()).collect::<Vec<_>>()
    );
}

#[test]
fn warns_on_unfiltered_block_handler() {
    let src = format!("{PREAMBLE}\nhandler block Token(block) {{\n  let a = Account.loadOrCreate(0x01, {{ balance: BigInt.zero }})\n}}\n");
    assert_warns(&src, "W011");
}

/// A project whose `Holder` entity is keyed on a `String` id, with a handler body
/// we vary to exercise W040 (stringified single-value id vs genuine composite id).
fn with_string_id_holder(body: &str) -> String {
    format!(
        "{}\nentity Holder {{ id: Id<String> balance: BigInt }}\nhandler on Token.Transfer(event) {{\n{body}\n}}\n",
        PREAMBLE
    )
}

fn assert_no_warn(src: &str, code: &str) {
    let diags = diags_of(src);
    assert!(
        !diags.iter().any(|d| d.code_short() == code),
        "unexpected {code}; got: {:?}",
        diags.iter().map(|d| d.code_short()).collect::<Vec<_>>()
    );
}

#[test]
fn warns_on_stringified_id_via_local() {
    // `let id = addr.toHexString(); Holder.create(id, …)` — a Bytes id would be
    // ~28% faster / ~48% smaller.
    let src = with_string_id_holder(
        "let id = event.params.to.toHexString()\n\
         let h = Holder.create(id, { balance: BigInt.zero })",
    );
    assert_warns(&src, "W040");
}

#[test]
fn warns_on_stringified_id_inline() {
    let src = with_string_id_holder(
        "let h = Holder.create(event.params.from.toHexString(), { balance: BigInt.zero })",
    );
    assert_warns(&src, "W040");
}

#[test]
fn no_warning_for_composite_string_id() {
    // A genuine composite key (two values joined) is really a String — never flag.
    let src = with_string_id_holder(
        "let id = event.params.from.toHexString() + \"-\" + event.params.to.toHexString()\n\
         let h = Holder.create(id, { balance: BigInt.zero })",
    );
    assert_no_warn(&src, "W040");
}

#[test]
fn no_warning_for_raw_bytes_id() {
    // The good case: raw Bytes/Address id, no stringification.
    let src =
        with_handler("let acct = Account.loadOrCreate(event.params.to, { balance: BigInt.zero })");
    assert_no_warn(&src, "W040");
}

#[test]
fn warns_on_call_handler_on_non_tracing_network() {
    let src = r#"
abi ERC20 from "./abis/ERC20.json"
entity Account { id: Id<Bytes> balance: BigInt }
source Token {
  abi: ERC20
  network: "arbitrum-one"
  address: 0x1234567890abcdef1234567890abcdef12345678
  startBlock: 1
}
handler call Token.balanceOf(call) {
  let a = Account.loadOrCreate(0x01, { balance: BigInt.zero })
}
"#;
    assert_warns(src, "W010");
}

#[test]
fn warnings_do_not_fail_the_build() {
    let src = format!("{PREAMBLE}\nhandler block Token(block) {{\n  let a = Account.loadOrCreate(0x01, {{ balance: BigInt.zero }})\n}}\n");
    // `check` is errors-only — a warning must not turn into a failure.
    assert!(run(&src).is_ok());
}

#[test]
fn warns_on_eth_call_in_loop() {
    let body = "  let xs = [event.params.from, event.params.to]\n  for h in xs {\n    match ERC20.bind(event.address).balanceOf(h) { Ok(b) => { let a = Account.loadOrCreate(h, { balance: b }) } Err(e) => {} }\n  }";
    assert_warns(&with_handler(body), "W020");
}

#[test]
fn no_warning_for_eth_call_outside_loop() {
    let body = "  match ERC20.bind(event.address).balanceOf(event.params.to) { Ok(b) => { let a = Account.loadOrCreate(event.params.to, { balance: b }) } Err(e) => {} }";
    let diags = diags_of(&with_handler(body));
    assert!(
        !diags.iter().any(|d| d.code_short() == "W020"),
        "unexpected W020 outside a loop"
    );
}

#[test]
fn rejects_division_by_zero_literal() {
    assert_err_contains(
        run(&with_handler("  let x = event.params.value / 0")),
        "E090",
    );
}

#[test]
fn rejects_division_by_bigint_zero() {
    assert_err_contains(
        run(&with_handler(
            "  let x = event.params.value / BigInt.zero()",
        )),
        "E090",
    );
}

const PAIR_PREAMBLE: &str = r#"
abi ERC20 from "./abis/ERC20.json"
entity Pair { id: Id<Bytes> r0: BigInt r1: BigInt whole: BigInt price: BigDecimal }
source Token {
  abi: ERC20
  network: mainnet
  address: 0x1234567890abcdef1234567890abcdef12345678
  startBlock: 1
}
"#;

#[test]
fn warns_bigint_division_into_bigdecimal() {
    let src = format!(
        "{PAIR_PREAMBLE}\nhandler on Token.Transfer(event) {{\n  let p = Pair.loadOrCreate(event.address, {{ r0: BigInt.zero, r1: BigInt.zero, whole: BigInt.zero, price: BigDecimal.zero }})\n  p.price = p.r0 / p.r1\n}}\n"
    );
    assert_warns(&src, "W030");
}

#[test]
fn no_precision_warning_for_bigint_field() {
    // BigInt / BigInt into a BigInt field is fine — no W030.
    let src = format!(
        "{PAIR_PREAMBLE}\nhandler on Token.Transfer(event) {{\n  let p = Pair.loadOrCreate(event.address, {{ r0: BigInt.zero, r1: BigInt.zero, whole: BigInt.zero, price: BigDecimal.zero }})\n  p.whole = p.r0 / p.r1\n}}\n"
    );
    let diags = diags_of(&src);
    assert!(
        !diags.iter().any(|d| d.code_short() == "W030"),
        "unexpected W030"
    );
}

/// Build a one-file project and return the checked symbol table.
fn check_project(src: &str) -> crate::Checked {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/abis")).unwrap();
    fs::write(
        dir.path().join("redstart.toml"),
        "[project]\nname = \"t\"\nentry = \"src/main.red\"",
    )
    .unwrap();
    fs::write(dir.path().join("src/abis/ERC20.json"), ABI).unwrap();
    fs::write(dir.path().join("src/main.red"), src).unwrap();
    let tree = redstart_loader::load(dir.path()).unwrap();
    crate::check(&tree).expect("should check")
}

#[test]
fn warns_on_stored_entity_array() {
    // A stored `[Account]` (not `derived from`) is the O(n²) anti-pattern.
    let src = format!("{PREAMBLE}\nentity Pool {{ id: Id<Bytes> accounts: [Account] }}\n");
    assert_warns(&src, "W050");
}

#[test]
fn no_w050_for_scalar_arrays() {
    // Scalar/enum arrays are genuinely stored values — never flagged.
    let src =
        format!("{PREAMBLE}\nentity Pool {{ id: Id<Bytes> tags: [String] nums: [BigInt] }}\n");
    assert_no_warn(&src, "W050");
}

#[test]
fn no_w050_for_derived_relation() {
    // The recommended form must stay clean.
    let src = format!(
        "{PREAMBLE}\nentity Pool {{ id: Id<Bytes> accs: [Account] derived from owner }}\n\
         entity Owned {{ id: Id<Bytes> }}\n"
    );
    // (Account has no `owner` back-ref, so E-code may fire, but never W050.)
    assert_no_warn(&src, "W050");
}

// ---- id rewrite (`redstart fix --ids`) ------------------------------------

/// Build a one-file project, plan the id rewrite, and apply its edits to the
/// source in-memory. Returns `(plan, rewritten_main.red)`.
fn rewrite_of(src: &str) -> (crate::IdRewritePlan, String) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/abis")).unwrap();
    fs::write(
        dir.path().join("redstart.toml"),
        "[project]\nname = \"t\"\nentry = \"src/main.red\"",
    )
    .unwrap();
    fs::write(dir.path().join("src/abis/ERC20.json"), ABI).unwrap();
    let main = dir.path().join("src/main.red");
    fs::write(&main, src).unwrap();
    let tree = redstart_loader::load(dir.path()).unwrap();
    let plan = crate::plan_id_rewrites(&tree);

    let mut edits: Vec<(usize, usize, String)> = plan
        .rewrites
        .iter()
        .flat_map(|r| &r.edits)
        .map(|e| (e.start, e.end, e.replacement.clone()))
        .collect();
    edits.sort_by_key(|e| std::cmp::Reverse(e.0));
    let mut out = fs::read_to_string(&main).unwrap();
    for (start, end, repl) in edits {
        out.replace_range(start..end, &repl);
    }
    (plan, out)
}

#[test]
fn rewrite_ids_converts_inline_stringified_id() {
    let src = with_string_id_holder(
        "let h = Holder.create(event.params.from.toHexString(), { balance: BigInt.zero })",
    );
    let (plan, out) = rewrite_of(&src);
    assert_eq!(plan.rewrites.len(), 1);
    assert_eq!(plan.rewrites[0].entity, "Holder");
    assert_eq!(plan.rewrites[0].sites, 1);
    assert!(plan.skipped.is_empty());
    // Declaration flipped and the `.toHexString()` dropped.
    assert!(out.contains("id: Id<Bytes>"), "decl not flipped:\n{out}");
    assert!(!out.contains("toHexString"), "suffix not dropped:\n{out}");
    // And the rewritten program still checks clean.
    assert!(run(&out).is_ok(), "rewritten source must check:\n{out}");
}

#[test]
fn rewrite_ids_skips_entity_with_a_literal_id_site() {
    // One convertible site + one literal-string site → skip the whole entity.
    let src = with_string_id_holder(
        "let a = Holder.create(event.params.from.toHexString(), { balance: BigInt.zero })\n\
         let b = Holder.loadOrCreate(\"singleton\", { balance: BigInt.zero })",
    );
    let (plan, out) = rewrite_of(&src);
    assert!(plan.rewrites.is_empty(), "must not rewrite a mixed entity");
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.skipped[0].entity, "Holder");
    // Source untouched — still a String id, still stringified.
    assert!(out.contains("id: Id<String>"));
    assert!(out.contains("toHexString"));
}

#[test]
fn rewrite_ids_converts_via_use_once_local() {
    // `let id = x.toHexString(); E.create(id, …)` with `id` used only here — the
    // `.toHexString()` is dropped on the `let`, the site is untouched.
    let src = with_string_id_holder(
        "let id = event.params.to.toHexString()\n\
         let h = Holder.create(id, { balance: BigInt.zero })",
    );
    let (plan, out) = rewrite_of(&src);
    assert_eq!(plan.rewrites.len(), 1);
    assert_eq!(plan.rewrites[0].entity, "Holder");
    assert!(plan.skipped.is_empty());
    assert!(out.contains("id: Id<Bytes>"));
    assert!(
        out.contains("let id = event.params.to\n"),
        "let not rewritten:\n{out}"
    );
    assert!(!out.contains("toHexString"));
    assert!(run(&out).is_ok(), "rewritten source must check:\n{out}");
}

#[test]
fn rewrite_ids_skips_reused_local() {
    // The local is read again (here as another create id), so re-typing it to
    // `Bytes` isn't safe — report it, don't convert.
    let src = with_string_id_holder(
        "let id = event.params.to.toHexString()\n\
         let h = Holder.create(id, { balance: BigInt.zero })\n\
         let g = Holder.loadOrCreate(id, { balance: BigInt.zero })",
    );
    let (plan, out) = rewrite_of(&src);
    assert!(plan.rewrites.is_empty(), "reused local must not convert");
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.skipped[0].entity, "Holder");
    assert!(out.contains("id: Id<String>"));
    assert!(out.contains("toHexString"));
}

#[test]
fn rewrite_ids_leaves_composite_and_bytes_ids_alone() {
    // A genuine composite key is really a String — not a candidate, nothing to do.
    let composite = with_string_id_holder(
        "let id = event.params.from.toHexString() + \"-\" + event.params.to.toHexString()\n\
         let h = Holder.create(id, { balance: BigInt.zero })",
    );
    let (plan, _) = rewrite_of(&composite);
    assert!(plan.rewrites.is_empty() && plan.skipped.is_empty());

    // An already-Bytes id is never touched.
    let bytes =
        with_handler("let a = Account.loadOrCreate(event.params.to, { balance: BigInt.zero })");
    let (plan, _) = rewrite_of(&bytes);
    assert!(plan.rewrites.is_empty() && plan.skipped.is_empty());
}

#[test]
fn rewrite_ids_is_idempotent() {
    let src = with_string_id_holder(
        "let h = Holder.create(event.params.from.toHexString(), { balance: BigInt.zero })",
    );
    let (_, once) = rewrite_of(&src);
    // Feeding the converted source back in yields no further edits.
    let (plan, twice) = rewrite_of(&once);
    assert!(plan.rewrites.is_empty() && plan.skipped.is_empty());
    assert_eq!(once, twice);
}

#[test]
fn infers_immutable_for_append_only_entities() {
    let src = format!(
        "{PREAMBLE}\nentity Log {{ id: Id<Bytes> n: BigInt }}\nhandler on Token.Transfer(event) {{\n  let a = Account.loadOrCreate(event.params.from, {{ balance: BigInt.zero }})\n  a.balance = a.balance + event.params.value\n  let l = Log.create(event.id, {{ n: event.params.value }})\n}}\n"
    );
    let checked = check_project(&src);
    // Log is only ever created -> append-only -> inferred immutable.
    assert!(
        checked.immutable_inferred.contains("Log"),
        "Log should be immutable"
    );
    // Account is loadOrCreate'd and field-mutated -> stays mutable.
    assert!(
        !checked.immutable_inferred.contains("Account"),
        "Account must not be inferred immutable"
    );
}

// ---- porting-friction regressions (v0.16.0) ----

#[test]
fn boolean_is_accepted_as_a_scalar_spelling() {
    // Every schema.graphql being ported says `Boolean`, not `Bool`.
    let src = format!(
        "{PREAMBLE}\nentity Flagged {{ id: Id<Bytes> ok: Boolean paused: Bool }}\n\
         handler on Token.Transfer(event) {{\n  \
           let f = Flagged.create(event.id, {{ ok: true, paused: false }})\n}}\n"
    );
    assert!(run(&src).is_ok(), "`Boolean` must be a valid scalar");
}

#[test]
fn narrow_solidity_ints_are_native_ints() {
    use crate::ty::{sol_to_rty, RTy};
    // graph-cli's table: int8..int32 and uint8..uint24 are i32, the rest BigInt.
    for t in [
        "uint8", "uint16", "uint24", "int8", "int16", "int24", "int32",
    ] {
        assert_eq!(sol_to_rty(t), RTy::Int, "{t} should be a native Int");
    }
    for t in [
        "uint32", "uint64", "uint256", "int64", "int256", "uint", "int",
    ] {
        assert_eq!(sol_to_rty(t), RTy::BigInt, "{t} should be a BigInt");
    }
    assert_eq!(sol_to_rty("uint8[]"), RTy::List(Box::new(RTy::Int)));
    assert_eq!(sol_to_rty("address[3]"), RTy::List(Box::new(RTy::Address)));
}

#[test]
fn save_teaches_that_entities_save_themselves() {
    let src = with_handler(
        "let a = Account.loadOrCreate(event.params.to, { balance: BigInt.zero })\n\
         a.save()",
    );
    assert_err_contains(run(&src), "entities save themselves");
}

#[test]
fn none_clears_an_optional_field_but_not_a_required_one() {
    let ok = format!(
        "{PREAMBLE}\nentity Trove {{ id: Id<Bytes> owner: Bytes batch: Option<String> }}\n\
         handler on Token.Transfer(event) {{\n  \
           let t = Trove.create(event.id, {{ owner: event.params.from }})\n  \
           t.batch = None\n}}\n"
    );
    assert!(run(&ok).is_ok(), "clearing an Option field is allowed");

    let bad = format!(
        "{PREAMBLE}\nentity Trove {{ id: Id<Bytes> owner: Bytes }}\n\
         handler on Token.Transfer(event) {{\n  \
           let t = Trove.create(event.id, {{ owner: event.params.from }})\n  \
           t.owner = None\n}}\n"
    );
    assert_err_contains(run(&bad), "cannot clear required field `owner`");
}

#[test]
fn an_event_parameter_the_abi_lacks_is_an_error() {
    let src =
        with_handler("let a = Account.create(event.id, { balance: event.params.notARealParam })");
    assert_err_contains(run(&src), "no parameter `notARealParam`");
    // …and the real ones still pass.
    let ok = with_handler("let a = Account.create(event.id, { balance: event.params.value })");
    assert!(run(&ok).is_ok());
}

/// Build a two-module project (`main.red` + a sibling `helpers.red`) and check it.
fn run_two_module(main: &str, helpers: &str) -> Result<(), Vec<String>> {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/abis")).unwrap();
    fs::write(
        dir.path().join("redstart.toml"),
        "[project]\nname = \"t\"\nentry = \"src/main.red\"",
    )
    .unwrap();
    fs::write(dir.path().join("src/abis/ERC20.json"), ABI).unwrap();
    fs::write(dir.path().join("src/main.red"), main).unwrap();
    fs::write(dir.path().join("src/helpers.red"), helpers).unwrap();
    let tree = redstart_loader::load(dir.path()).unwrap();
    check(&tree).map(|_| ())
}

#[test]
fn a_call_to_an_undefined_function_is_rejected() {
    let src = format!(
        "{PREAMBLE}\nfn probe() -> BigInt {{\n  return definitelyNotAFunctionAnywhere(42)\n}}\n"
    );
    assert_err_contains(
        run(&src),
        "call to undefined function `definitelyNotAFunctionAnywhere`",
    );
}

#[test]
fn an_undefined_call_is_caught_in_every_position() {
    // Statement, argument, array element and `if` condition all reach `check_expr`.
    for body in [
        "  stmtPos(1)",
        "  let x = BigInt.fromI32(argPos(1))",
        "  let xs = [arrayPos(1)]",
        "  if condPos(1) { let y = 1 }",
    ] {
        let src = with_handler(body);
        assert_err_contains(run(&src), "call to undefined function");
    }
}

#[test]
fn a_helper_declared_in_another_module_resolves() {
    // Cross-module, and with no declared return type — `fn_returns` drops those,
    // so resolving against it alone would call this undefined.
    let main = format!(
        "{PREAMBLE}\nmod helpers;\nhandler on Token.Transfer(event) {{\n  note(event.params.value)\n  let d = double(event.params.value)\n}}\n"
    );
    let helpers = "fn note(x: BigInt) {\n  let y = x\n}\nfn double(x: BigInt) -> BigInt {\n  return x + x\n}\n";
    assert!(
        run_two_module(&main, helpers).is_ok(),
        "a helper from a sibling module must resolve"
    );
}

#[test]
fn some_is_option_sugar_and_not_an_undefined_call() {
    // `Some(x)` lowers to the identity, so it must never be reported as a call.
    let src =
        format!("{PREAMBLE}\nfn wrap(x: BigInt) -> Option<BigInt> {{\n  return Some(x)\n}}\n");
    assert!(run(&src).is_ok());
}

#[test]
fn a_module_qualified_call_is_left_alone() {
    // The check is deliberately narrow: only single-segment callees are judged,
    // so a `mod::fn(…)` shape is never a false positive here.
    let main = format!(
        "{PREAMBLE}\nmod helpers;\nhandler on Token.Transfer(event) {{\n  let d = helpers::double(event.params.value)\n}}\n"
    );
    let helpers = "fn double(x: BigInt) -> BigInt {\n  return x + x\n}\n";
    assert!(run_two_module(&main, helpers).is_ok());
}

#[test]
fn an_entity_called_as_a_function_points_at_the_constructor() {
    let src = with_handler("  let a = Account(event.params.to)");
    assert_err_contains(run(&src), "call to undefined function `Account`");
    let errs = run(&src).expect_err("expected a check error");
    let joined = errs.join("\n");
    assert!(
        joined.contains("Account.create(id, { … })"),
        "expected the constructor fix-it in:\n{joined}"
    );
}

#[test]
fn a_casing_typo_suggests_the_declared_name() {
    let src = format!(
        "{PREAMBLE}\nfn scaleUp(x: BigInt) -> BigInt {{\n  return x\n}}\n\
         fn caller(x: BigInt) -> BigInt {{\n  return scaleup(x)\n}}\n"
    );
    let errs = run(&src).expect_err("expected a check error");
    let joined = errs.join("\n");
    assert!(
        joined.contains("did you mean `scaleUp`?"),
        "expected a casing suggestion in:\n{joined}"
    );
}
