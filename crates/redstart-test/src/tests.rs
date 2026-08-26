//! Interpreter tests: write a project with handlers + `test` blocks, run them,
//! and assert which pass and which fail.

use crate::{run_tests, Outcome};
use std::fs;

const ABI: &str = r#"[
  {"type":"event","name":"Transfer","inputs":[
    {"name":"from","type":"address","indexed":true},
    {"name":"to","type":"address","indexed":true},
    {"name":"value","type":"uint256","indexed":false}]},
  {"type":"event","name":"Approval","inputs":[
    {"name":"owner","type":"address","indexed":true},
    {"name":"spender","type":"address","indexed":true},
    {"name":"value","type":"uint256","indexed":false}]},
  {"type":"function","name":"balanceOf","stateMutability":"view",
    "inputs":[{"name":"account","type":"address"}],
    "outputs":[{"name":"","type":"uint256"}]},
  {"type":"function","name":"transfer","stateMutability":"nonpayable",
    "inputs":[{"name":"to","type":"address"},{"name":"amount","type":"uint256"}],
    "outputs":[{"name":"success","type":"bool"}]}
]"#;

const PROGRAM: &str = r#"
abi ERC20 from "./abis/ERC20.json"

entity Account { id: Id<Bytes> balance: BigInt }

source Token {
  abi: ERC20
  network: mainnet
  address: 0x00
  startBlock: 1
}

handler on Token.Transfer(event) {
  let sender = Account.loadOrCreate(event.params.from, { balance: BigInt.zero })
  let receiver = Account.loadOrCreate(event.params.to, { balance: BigInt.zero })
  sender.balance = sender.balance - event.params.value
  receiver.balance = receiver.balance + event.params.value
}

handler on Token.Approval(event) {
  TxInfo.loadOrCreate(event.transaction.hash, {
    gasPrice: event.transaction.gasPrice,
    sender: event.transaction.from,
    recipient: event.transaction.to,
    amount: event.transaction.value,
  })
  let r = ERC20.bind(event.address).balanceOf(event.params.owner)
  match r {
    Ok(b) => {
      let o = Account.loadOrCreate(event.params.owner, { balance: BigInt.zero })
      o.balance = b
    }
    Err(e) => {}
  }
}

entity Snapshot { id: Id<Bytes> total: BigInt }

entity TxInfo { id: Id<Bytes> gasPrice: BigInt sender: Bytes recipient: Bytes amount: BigInt }

template PoolTemplate {
  abi: ERC20
  network: mainnet
}

entity Doc { id: Id<Bytes> body: BigInt }

template DocFile {
  kind: file
}

handler file DocFile(content) {
  let d = Doc.create(content, { body: 7 })
}

handler call Token.transfer(call) {
  let acct = Account.loadOrCreate(call.inputs.to, { balance: BigInt.zero })
  acct.balance = acct.balance + call.inputs.amount
  PoolTemplate.create(call.inputs.to)
}

handler block Token(block) {
  let snap = Snapshot.create(block.hash, { total: BigInt.zero })
  snap.total = block.number
}
"#;

/// Run the program plus `tests_src`, returning the per-test outcomes by name.
fn outcomes(tests_src: &str) -> Vec<(String, bool, String)> {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/abis")).unwrap();
    fs::write(
        dir.path().join("redstart.toml"),
        "[project]\nname=\"t\"\nentry=\"src/main.red\"",
    )
    .unwrap();
    fs::write(dir.path().join("src/abis/ERC20.json"), ABI).unwrap();
    fs::write(
        dir.path().join("src/main.red"),
        format!("{PROGRAM}\n{tests_src}"),
    )
    .unwrap();

    let tree = redstart_loader::load(dir.path()).unwrap();
    let checked = redstart_checker::check(&tree).expect("program should pass the checker");
    let report = run_tests(&tree, &checked);
    report
        .results
        .into_iter()
        .map(|r| match r.outcome {
            Outcome::Pass => (r.name, true, String::new()),
            Outcome::Fail { message, .. } => (r.name, false, message),
        })
        .collect()
}

#[test]
fn transfer_credits_and_debits() {
    let out = outcomes(
        r#"
test "transfer moves balance" {
  Token.Transfer({ from: 0x01, to: 0x02, value: 100 })
  assertEq(Account.at(0x02).balance, 100)
}
"#,
    );
    assert!(out[0].1, "expected pass, got: {}", out[0].2);
}

#[test]
fn wrong_assertion_fails() {
    let out = outcomes(
        r#"
test "wrong expectation" {
  Token.Transfer({ from: 0x01, to: 0x02, value: 100 })
  assertEq(Account.at(0x02).balance, 999)
}
"#,
    );
    assert!(!out[0].1);
    assert!(out[0].2.contains("assertEq failed"), "msg: {}", out[0].2);
}

#[test]
fn debit_goes_negative() {
    let out = outcomes(
        r#"
test "sender is debited" {
  Token.Transfer({ from: 0x01, to: 0x02, value: 100 })
  assert(Account.at(0x01).balance < 0)
}
"#,
    );
    assert!(out[0].1, "expected pass, got: {}", out[0].2);
}

#[test]
fn mocked_contract_call_and_match() {
    let out = outcomes(
        r#"
test "approval reads balance via call" {
  mockCall(ERC20.balanceOf(0x05), 777)
  Token.Approval({ owner: 0x05, spender: 0x06, value: 1 })
  assertEq(Account.at(0x05).balance, 777)
}
"#,
    );
    assert!(out[0].1, "expected pass, got: {}", out[0].2);
}

#[test]
fn unmocked_call_fails_loudly() {
    let out = outcomes(
        r#"
test "forgot to mock" {
  Token.Approval({ owner: 0x05, spender: 0x06, value: 1 })
  assert(true)
}
"#,
    );
    assert!(!out[0].1);
    assert!(
        out[0].2.contains("unmocked contract call"),
        "msg: {}",
        out[0].2
    );
}

#[test]
fn for_range_loop_accumulates() {
    let out = outcomes(
        r#"
test "range sum" {
  let total = 0
  for i in 1..5 {
    total = total + i
  }
  assertEq(total, 10)
}
"#,
    );
    assert!(out[0].1, "expected pass, got: {}", out[0].2);
}

#[test]
fn for_each_and_index_and_length() {
    let out = outcomes(
        r#"
test "array each" {
  let xs = [10, 20, 30]
  assertEq(xs.length, 3)
  assertEq(xs[1], 20)
  let total = 0
  for v in xs {
    total = total + v
  }
  assertEq(total, 60)
}
"#,
    );
    assert!(out[0].1, "expected pass, got: {}", out[0].2);
}

#[test]
fn if_else_and_while() {
    let out = outcomes(
        r#"
test "branch and loop" {
  let x = 0
  if 3 > 5 {
    x = 1
  } else if 3 > 2 {
    x = 2
  } else {
    x = 3
  }
  assertEq(x, 2)
  let n = 0
  while n < 4 {
    n = n + 1
  }
  assertEq(n, 4)
}
"#,
    );
    assert!(out[0].1, "expected pass, got: {}", out[0].2);
}

#[test]
fn loop_inside_handler_writes_store() {
    let out = outcomes(
        r#"
test "handler with conditional logic" {
  Token.Transfer({ from: 0x01, to: 0x02, value: 50 })
  assert(Account.at(0x01).balance < 0)
  assertEq(Account.at(0x02).balance, 50)
}
"#,
    );
    assert!(out[0].1, "expected pass, got: {}", out[0].2);
}

#[test]
fn call_handler_reads_inputs() {
    let out = outcomes(
        r#"
test "call handler credits via inputs" {
  Token.transfer({ to: 0x07, amount: 250 })
  assertEq(Account.at(0x07).balance, 250)
}
"#,
    );
    assert!(out[0].1, "expected pass, got: {}", out[0].2);
}

#[test]
fn template_create_is_recorded() {
    let out = outcomes(
        r#"
test "call handler spawns a pool data source" {
  Token.transfer({ to: 0x09, amount: 1 })
  assertCreated(PoolTemplate, 0x09)
}
"#,
    );
    assert!(out[0].1, "expected pass, got: {}", out[0].2);
}

#[test]
fn file_handler_writes_entity() {
    let out = outcomes(
        r#"
test "file handler indexes content" {
  DocFile.file(0xAB)
  assertEq(Doc.at(0xAB).body, 7)
}
"#,
    );
    assert!(out[0].1, "expected pass, got: {}", out[0].2);
}

#[test]
fn block_handler_snapshots() {
    let out = outcomes(
        r#"
test "block handler records number" {
  Token.block({ _block: 1234 })
  assertEq(Snapshot.at(0x0000000000000000000000000000000000000000000000000000000000000000).total, 1234)
}
"#,
    );
    assert!(out[0].1, "expected pass, got: {}", out[0].2);
}

#[test]
fn event_meta_override() {
    let out = outcomes(
        r#"
test "timestamp override flows into entity" {
  Token.Transfer({ from: 0x01, to: 0x02, value: 5, _timestamp: 42 })
  assertEq(Account.at(0x02).balance, 5)
}
"#,
    );
    assert!(out[0].1, "expected pass, got: {}", out[0].2);
}

// ---------------------------------------------------------------------------
// Gaps found by porting PancakeSwap Infinity CL. Each of these is code that
// `redstart verify` compiles to WASM but that the native runner could not
// evaluate, which made the "no Matchstick needed" claim false for any subgraph
// doing real arithmetic.
// ---------------------------------------------------------------------------

#[test]
fn log_namespace_is_a_no_op_and_critical_aborts() {
    let out = outcomes(
        r#"
test "log.debug does not stop a handler" {
  Token.Transfer({ from: 0x01, to: 0x02, value: 100 })
  log.debug("moved {}", ["100"])
  log.info("ok", [])
  log.warning("careful", [])
  log.error("bad", [])
  assertEq(Account.at(0x02).balance, 100)
}
test "log.critical aborts" {
  log.critical("stop", [])
}
"#,
    );
    assert!(out[0].1, "expected pass, got: {}", out[0].2);
    assert!(!out[1].1, "log.critical must abort the handler");
}

#[test]
fn transaction_exposes_the_documented_fields() {
    let out = outcomes(
        r#"
test "tx fields the porting guide documents are all readable" {
  mockCall(ERC20.balanceOf(0x01), 1)
  Token.Approval({ _txHash: 0xfeed, owner: 0x01, spender: 0x02, value: 5 })
  assertEq(TxInfo.at(0xfeed).gasPrice, BigInt.zero)
  assertEq(TxInfo.at(0xfeed).amount, BigInt.zero)
}
"#,
    );
    assert!(out[0].1, "expected pass, got: {}", out[0].2);
}

#[test]
fn numeric_statics_and_methods_are_available() {
    let out = outcomes(
        r#"
test "BigInt and BigDecimal statics" {
  assertEq(BigInt.fromI32(255), BigInt.fromString("255"))
  assertEq(BigDecimal.fromString("1.5") + BigDecimal.fromString("1.5"), BigDecimal.fromString("3"))
}
test "pow, mod, shifts and comparisons" {
  assertEq(BigInt.fromI32(2).pow(8), BigInt.fromI32(256))
  assertEq(BigInt.fromI32(7).mod(BigInt.fromI32(4)), BigInt.fromI32(3))
  assertEq(BigInt.fromI32(1).leftShift(4), BigInt.fromI32(16))
  assertEq(BigInt.fromI32(16).rightShift(4), BigInt.fromI32(1))
  assert(BigInt.fromI32(3).lt(BigInt.fromI32(4)))
  assert(BigInt.fromI32(4).notEqual(BigInt.fromI32(3)))
}
test "BigDecimal arithmetic, which used to fall through to BigInt" {
  assertEq(BigDecimal.fromString("1") / BigDecimal.fromString("4"), BigDecimal.fromString("0.25"))
}
"#,
    );
    for o in &out {
        assert!(o.1, "{} failed: {}", o.0, o.2);
    }
}

#[test]
fn string_methods_a_hex_parser_needs() {
    let out = outcomes(
        r#"
test "startsWith, slice, charAt and length" {
  assert("0xff".startsWith("0x"))
  assert(!"ff".startsWith("0x"))
  assertEq("0xff".slice(2), "ff")
  assertEq("abc".charAt(1), "b")
  assertEq("abc".length, 3)
}
"#,
    );
    assert!(out[0].1, "expected pass, got: {}", out[0].2);
}

#[test]
fn assert_missing_is_not_a_rubber_stamp() {
    let out = outcomes(
        r#"
test "missing entity passes" {
  assertMissing(Account, 0x09)
}
test "existing entity fails" {
  Token.Transfer({ from: 0x01, to: 0x02, value: 100 })
  assertMissing(Account, 0x02)
}
"#,
    );
    assert!(out[0].1, "expected pass, got: {}", out[0].2);
    assert!(
        !out[1].1,
        "assertMissing must fail on an entity that exists"
    );
}
