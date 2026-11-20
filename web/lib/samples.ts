// Real, honest samples used across the landing page. The AssemblyScript is the
// independently hand-written conformance reference, not a strawman.

export const REDSTART_ERC20 = `abi ERC20 from "./abis/ERC20.json"

entity Account {
  id: Id<Bytes>
  balance: BigInt
}

entity Transfer immutable {
  id: Id<Bytes>
  from: Account
  to: Account
  value: BigInt
  timestamp: BigInt
}

source Token {
  abi: ERC20
  network: mainnet
  address: 0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48
  startBlock: 6082465
}

handler on Token.Transfer(event) {
  let sender = Account.loadOrCreate(event.params.from, { balance: BigInt.zero })
  let receiver = Account.loadOrCreate(event.params.to, { balance: BigInt.zero })

  sender.balance = sender.balance - event.params.value
  receiver.balance = receiver.balance + event.params.value
  // dirty-tracked, auto-saved: forgetting .save() can't happen

  Transfer.create(event.id, {
    from: event.params.from,
    to: event.params.to,
    value: event.params.value,
    timestamp: event.block.timestamp,
  })
}`;

export const AS_MAPPINGS = `import { BigInt } from "@graphprotocol/graph-ts";
import {
  Transfer as TransferEvent,
} from "../generated/Token/ERC20";
import { Account, Transfer } from "../generated/schema";

export function handleTransfer(event: TransferEvent): void {
  let sender = Account.load(event.params.from);
  if (sender == null) {
    sender = new Account(event.params.from);
    sender.balance = BigInt.zero();
  }

  let receiver = Account.load(event.params.to);
  if (receiver == null) {
    receiver = new Account(event.params.to);
    receiver.balance = BigInt.zero();
  }

  sender.balance = sender.balance.minus(event.params.value);
  receiver.balance = receiver.balance.plus(event.params.value);
  sender.save();      // forget this and balances silently desync
  receiver.save();

  let transfer = new Transfer(
    event.transaction.hash.concatI32(event.logIndex.toI32())
  );
  transfer.from = event.params.from;
  transfer.to = event.params.to;
  transfer.value = event.params.value;
  transfer.timestamp = event.block.timestamp;
  transfer.save();
}`;

export const AS_SCHEMA = `type Account @entity {
  id: Bytes!
  balance: BigInt!
}

type Transfer @entity(immutable: true) {
  id: Bytes!
  from: Account!     # name must match mappings.ts, by hand
  to: Account!
  value: BigInt!
  timestamp: BigInt!
}`;

export const AS_MANIFEST = `dataSources:
  - kind: ethereum
    name: Token
    network: mainnet
    source:
      address: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"
      abi: ERC20
      startBlock: 6082465
    mapping:
      eventHandlers:
        - event: Transfer(indexed address,indexed address,uint256)
          handler: handleTransfer   # string must match the export, by hand`;
