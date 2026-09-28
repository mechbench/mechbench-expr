// Every conformance case through the WebAssembly module in Node. Values
// compare as JSON text, so an integer is not a float: `2` is not `2.0`.
import { readFileSync, readdirSync } from "node:fs";
import { load } from "./engine.mjs";

const wasm = process.argv[2] ?? "../target/wasm32-unknown-unknown/release/mbexpr.wasm";
const engine = await load(readFileSync(new URL(wasm, import.meta.url)));
// JSON.parse reads 2.0 as 2; compare the engine's own text for numbers.
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

let n = 0;
const failures = [];
const dir = new URL("../conformance/", import.meta.url);
for (const f of readdirSync(dir).filter((f) => f.endsWith(".json")).sort()) {
  const doc = JSON.parse(readFileSync(new URL(f, dir), "utf8"));
  for (const c of doc.cases) {
    n += 1;
    const req = {};
    for (const k of ["record", "params", "header"]) if (k in c) req[k] = c[k];
    if ("check" in c) Object.assign(req, { op: "check", expr: c.check });
    else if ("template" in c) Object.assign(req, { op: "template", template: c.template });
    else Object.assign(req, { op: "eval", expr: c.expr });
    const a = engine.call(req);
    const what = c.expr ?? c.template ?? c.check;
    if ("expect_error" in c) {
      if (!(a.ok === false && a.error.kind === c.expect_error)) failures.push(`${f}: ${what}: expected a ${c.expect_error} error, got ${JSON.stringify(a)}`);
      continue;
    }
    if (!a.ok) { failures.push(`${f}: ${what}: ${JSON.stringify(a.error)}`); continue; }
    if ("check" in c) {
      for (const k of ["canonical", "reads"]) if (k in c && !same(a[k], c[k])) failures.push(`${f}: ${what}: ${k} ${JSON.stringify(a[k])}`);
      continue;
    }
    if (!same(a.values[0], c.expect)) failures.push(`${f}: ${what}: expected ${JSON.stringify(c.expect)}, got ${JSON.stringify(a.values[0])}`);
    if (!same(a.undefined ?? {}, c.undefined ?? {})) failures.push(`${f}: ${what}: undefined ${JSON.stringify(a.undefined)}`);
  }
}
console.log(`${n - failures.length} of ${n} cases pass in ${process.version}`);
if (failures.length) { console.log(failures.join("\n")); process.exit(1); }
