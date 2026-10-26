# Helpers & modules

## Modules

A Redstart project is a tree of modules, exactly like Rust. Declare a child
module with `mod`, and refer across modules with `::`.

```redstart
// src/main.red
mod accounts;

handler on Token.Transfer(event) {
  let receiver = accounts::Account.loadOrCreate(event.params.to, { balance: BigInt.zero })
  // ...
}
```

```redstart
// src/accounts.red
entity Account {
  id: Id<Bytes>
  balance: BigInt
}
```

Entities can live in one module and the handlers that write them in another; the
compiler resolves and checks across all of them. `mod name;` resolves to a
sibling `name.red` or a nested `name/mod.red`, with cycle detection.

## Helper functions

Free `fn` declarations factor out shared logic. They lower to AssemblyScript
functions, work across modules, and return typed values.

A helper is visible from every module, so the bare `normalize(x)` works
anywhere. `math::normalize(x)` also works and is checked against the module it
names. Names must be unique across the project, since every helper ends up in
the same generated file.

```redstart
fn normalize(amount: BigInt, decimals: i32) -> BigDecimal {
  return amount.toBigDecimal() / exponent(decimals)
}
```

Entities touched inside a helper are dirty-tracked too: their saves are flushed
at every `return` (and at the end of the calling handler), so the auto-save
guarantee holds across function boundaries.
