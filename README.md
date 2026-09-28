# mechbench-expr

The mechbench expression language: a strict subset of Python's
expressions with total, deterministic semantics. One engine, written in
Rust and compiled to WebAssembly, evaluates expressions for
mechbench-compute (under wasmtime), the mechbench API (Node) and the
browser: the same bytes give the same answers everywhere.

```
top[0].token.text != tracked.truth.token
depth >= params.allowance and coords.condition == "lie"
[v.winner for v in votes if v.parsed]
"{coords.fact}: {round(p, 3)}"
```

The language is specified in
[`mechbench/docs/EXPRESSIONS.md`](https://github.com/mechbench/mechbench/blob/main/docs/EXPRESSIONS.md)
(private for now; the semantics are also pinned by the conformance
suite here). In short: missing fields are null and null spreads;
integers stay integers and overflow is an error; `/` is always float and
`//` floors; rounding is half to even; an undefined number (a division
by zero, a log of zero) is null and counted; conditions are Kleene's.

## Layout

- `src/`: the engine. `api.rs` is its one entry point: a JSON request
  (`check`, `eval`, `filter`, `template`) and a JSON answer, exported to
  WebAssembly as `mbexpr_alloc`, `mbexpr_call`, `mbexpr_free`.
- `conformance/*.json`: the cases every host must answer alike.
- `hosts/`: the module driven from Node (`engine.mjs`, `conformance.mjs`)
  and from Python under wasmtime (`answers.py`); `answers.*` print every
  case's raw answer so hosts can be compared byte for byte.
- `build.sh`: the reproducible release build (toolchain pinned in
  `rust-toolchain.toml`, dependencies in `Cargo.lock`, local paths
  remapped); prints the sha256 compute pins.

## Checks

```
cargo test                   # the engine and the conformance suite, native
./build.sh                   # the module
node hosts/conformance.mjs   # the suite through WebAssembly in Node
```

Licensed MIT.
