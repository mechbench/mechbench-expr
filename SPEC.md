# The mechbench expression language

A strict subset of Python's expressions, with semantics of our own that
are total, pure and the same on every machine. One engine,
written in Rust and compiled to WebAssembly, evaluates them in compute
(wasmtime), the API (Node) and the page (the browser); the same bytes,
the same answers.

## Where an expression appears

A JSON value in a node's params may be an expression:

```json
{"$expr": "top[0].token.text != tracked.truth.token"}
```

beside the references that exist today, `{"$param": "layer"}` and
`{"$ref": "benji/lab/prompts"}`. A string may interpolate expressions:

```json
"Write a two-sentence story about {animal}."
"{coords.fact}: {round(p, 3)}"
```

`{{` and `}}` are literal braces. An interpolated string is a template
only where an operation declares the param as one (fill's templates, a
derive's string fields, a chart's labels); elsewhere braces are text.

## Two scopes

- **Protocol scope** (evaluated when the run is bound): the protocol's
  params, by name, as `params.name` (and bare `name` where no record is in
  scope). `{"$param": "layer"}` is the expression `params.layer`.
- **Record scope** (evaluated per record, inside an operation that reads
  records): the record's own fields, plus `params.*` and `header.*` (the
  input collection's header: `header.arch.global_layers`).

**A bare name reads the record's own top-level field**, and nothing else:
a coordinate is `coords.prompt`, a parameter `params.layer`. One rule, and
no field can shadow another; the renames that moved fields into `coords`
so an operation's `by` could find them go away because `by` takes
expressions. `record` names the record itself, for a field whose name is
a reserved word or shadows a function (`record["in"]`).

## Grammar

A strict subset of Python 3 expressions (Python's grammar, fewer
productions):

| form | example |
|---|---|
| literals | `3`, `-2`, `0.5`, `1e-3`, `"flash"`, `'flash'`, `True`/`true`, `False`/`false`, `None`/`null` (both spellings, the same values) |
| lists and dicts | `[1, 2, 3]`, `{"true": "global", "false": "local"}` |
| field access | `tracked.truth.token`, `top[0]`, `metadata["call"]`, `xs[-1]`, `xs[1:3]` |
| arithmetic | `+ - * / // % **`, unary `-` |
| comparison | `== != < <= > >=`, chained `0 <= p <= 1`, `in`, `not in`, `is None`, `is not None` |
| boolean | `and`, `or`, `not` |
| conditional | `"honest" if delta > 0 else "lie"` |
| calls | `round(p, 3)`, `len(tokens)`, `s.startswith("The")` (the fixed library below) |
| comprehension | `[v.winner for v in votes if v.parsed]`, `sum(x.p for x in top)` (one `for`, one optional `if`) |

Not in the language: assignment (including `:=`), `lambda`, `def`,
loops, `import`, attribute access on anything but data, starred
arguments, f-strings (a template is the interpolated string above),
sets, tuples as values. Every expression terminates: the only iteration
is over a finite list, and there is no recursion.

## Values and semantics

Values are JSON's: null, booleans, numbers, strings, lists, objects.

- **Integers and floats** stay distinct: an integer is a 64-bit signed
  integer; an overflow is an error, never a wrap. `/` always gives a
  float (`7 / 2 == 3.5`); `//` floors (`-7 // 2 == -4`); `%` takes the
  divisor's sign, as Python's does. `int(x)` truncates toward zero.
- **Rounding** is half to even, as Python's `round`: `round(2.5) == 2`,
  `round(0.125, 2) == 0.12`. Decimal places round the float's exact
  value, as Python does.
- **Floats** are IEEE-754 doubles. `exp`, `log`, `sqrt` and the rest come
  from the engine's own library (musl's, through Rust's `libm`), so they
  agree to the bit on every host. `sqrt` is exact; the transcendental
  functions are within one unit in the last place of the true value, and
  may differ from a particular platform's Python in that last place
  (`exp(1)` is `2.7182818284590455`, Python on macOS `2.718281828459045`).
  Correctly rounded functions are a possible upgrade.
  **An undefined number is null**, never NaN or infinity (JSON, which
  results are stored and served as, has neither): `x / 0`, `x // 0`,
  `x % 0`, `log(0)`, `sqrt(-1)` and any result that would be NaN or
  infinite are `null`. The operation counts them and says why in its
  header and its reading ("12 divisions by zero"), so a null that came
  from arithmetic is never mistaken for a missing field.
- **Missing is null, never zero.** A field that is absent reads `null`.
  Arithmetic, ordering and functions of `null` give `null`; `null == null`
  is true and `null == 0` is false. `and`, `or` and `not` are Kleene's:
  `null and False` is false, `null or True` is true, otherwise `null`.
  A **filter keeps a record only when its condition is `True`**: false
  and null both drop it, and the operation counts the nulls it met.
- **Truth**: a condition must be a boolean or null; `1` and `0` are not
  booleans, but `x == 1` is, and a numeric field compared to a boolean
  compares `1 == True` as Python does (the inventory found booleans
  stored as 0/1 in 26 counts).
- **Strings** are sequences of Unicode code points: `len`, indexing and
  slicing count code points, and `<` compares code point by code point.
- **Equality** is structural: lists and objects are equal when their
  elements are. Ordering is defined for two numbers, two strings or two
  lists; anything else is an error.
- **Errors**: a type error, an integer overflow or an index out of range
  is an error naming the expression, the record's id and
  the value that failed. An operation fails on the first one unless its
  `on_error` says `null` (the value becomes null and is counted).

## The library

Fixed, total, and the same everywhere:

- **numbers**: `abs`, `round(x, n=0)`, `floor`, `ceil`, `int`, `float`,
  `min`, `max`, `sqrt`, `exp`, `log(x, base=e)`, `log2`, `log10`, `pow`.
  There is no `isnan`: an undefined number is already `null`.
- **strings**: `str(x)`, `len`, `.lower()`, `.upper()`, `.strip()`,
  `.startswith(s)`, `.endswith(s)`, `.replace(a, b)`, `.split(sep)`,
  `.join(xs)`, `contains(s, part)`, `format(x, spec)` (Python's format
  mini-language for numbers: `format(p, ".3f")`).
- **lists**: `len`, `sum`, `min`, `max`, `any`, `all`, `sorted(xs,
  reverse=False)`, `xs.index(v)`, `count(xs, v)`, `first(xs)`,
  `last(xs)`.
- **objects**: `d.get(k, default=None)`, `keys(d)`, `values(d)`, `has(d, k)`.
- **nulls**: `coalesce(a, b, ...)`, `x is None`.

## Aggregates (in a group)

Inside a group, a call to an aggregate takes an expression evaluated per
record of the group:

- **plain**: `count()`, `count(cond)`, `sum(x)`, `mean(x)`, `median(x)`,
  `min(x)`, `max(x)`, `share(cond)`, `any(cond)`, `all(cond)`,
  `first(x)`, `last(x)`, `collect(x)`.
- **named methods** (computed by compute, cited by name in the reading
  and the docs, their numerics unchanged from today's operations):
  `wilson(cond, level=0.95)` → `{k, n, rate, lo, hi}`;
  `bootstrap_mean(x, level=0.95, resamples=2000, seed=0)` → `{mean, lo, hi}`;
  `spearman(x, y, level=None)` → `{n, rho, lo, hi}`;
  `paired_difference(x, on, a, b, paired, level=0.95, resamples=2000,
  seed=0)` → `{n, diff, lo, hi, share_positive, ...}`.

Nulls are skipped by the plain aggregates and counted (`n_missing`), as
`on_missing: skip` does today; `on_missing: fail` stays available.

## Checked before it runs

Every field path an expression reads is checked against the upstream
output's kind (its declared fields) when the protocol is pushed, and a
path the kind does not declare is a finding, as a wrong port is today.
Types are checked where the kind declares them: `len(p)` on a declared
number is an error at push, not at run.

## Canonical form and the hash

An expression is stored as written. It enters the protocol's hash as its
canonical form (parsed and printed back with one spacing and quoting
rule), so `a==1` and `a == 1` are the same protocol.

## Compatibility with mechbench's earlier forms

Before expressions, each records operation in mechbench-compute wrote its
conditions, paths and templates in a form of its own. Each maps to one
expression. Those operations read a field from `coords` first and then
from the top level, so a bare name in an old param becomes
`coords.x if has(coords, "x") else x`, or the simpler `coords.x` or `x`
where the upstream kind or the stored results show which one holds it.
The table writes the simple form:

| today | expression |
|---|---|
| `where: {"prompt": "flash"}` | `coords.prompt == "flash"` |
| `where: {"face": ["1", "2"]}` | `face in ["1", "2"]` |
| `where: [{"op": ">=", "path": "depth", "value": {"$param": "allowance"}}]` | `depth >= params.allowance` |
| `where: ["delta<0"]` | `delta < 0` |
| `where: ["bold=0", "heading=0", "lex_words>0"]` | `bold == 0 and heading == 0 and lex_words > 0` |
| `field: "theme", equals: 1` | `theme == 1` |
| `rename: {"metadata.sampling.ended": "ended"}` | derive `ended: metadata.sampling.ended`, drop `metadata.sampling.ended` |
| `relabel: {"true": "global attention", "false": "local attention"}` | `"global attention" if attention else "local attention"` |
| `relabel` of many values, `others: "error"` | `{"attn": "its attention only", "mlp": "its MLP only"}.get(removed, "error")` |
| `fill: "Write a story about {animal}."` | the same string, now a template |
| single-input `union` with `batch_axis: "model"` | derive `coords.model: params.model` (or a literal) |

## The engine

One entry point takes a JSON request and answers JSON (`src/api.rs`),
exported to WebAssembly as `mbexpr_alloc`, `mbexpr_call` and
`mbexpr_free`, so every host drives it the same way and a whole
collection crosses the boundary once:

- `check`: the canonical form of an expression and the field paths it
  reads, or its syntax error;
- `eval`: one expression, or several by name, over a record or a list of
  records, with `params` and `header`; the undefined numbers met are
  counted by reason;
- `filter`: the indexes of the records whose condition is `True`, and how
  many were null;
- `template`: a template rendered per record;
- `split`: an aggregate call taken apart into its function, its
  positional arguments (canonical expressions, read per record) and its
  named arguments (evaluated once, in protocol scope).

An error names its kind (`syntax`, `type`, `name`, `range`, `limit`), its
place in the expression and the record it met.

## The conformance suite

`conformance/*.json`: cases of `{expr, record, params, header, expect}`,
`{template, ...}`, `{check, canonical, reads}` or `{expect_error}`. They
run natively (`cargo test`), in Node and under wasmtime, and CI checks
that the three hosts' raw answers are identical byte for byte. They cover
every rule above, including the edges: half-to-even rounding, floor
division of negatives, null spreading, Kleene logic, code-point strings,
integer overflow, and the engine's own `exp` and `log`.
