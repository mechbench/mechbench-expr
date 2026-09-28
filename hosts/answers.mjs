// Every conformance case's raw answer from the WebAssembly module in
// Node, one per line, to compare byte for byte with the native build and
// the other hosts. The request is built as examples/answers.rs builds it.
import { readFileSync, readdirSync } from "node:fs";

const wasm = readFileSync(new URL(process.argv[2] ?? "../target/wasm32-unknown-unknown/release/mbexpr.wasm", import.meta.url));
const { instance } = await WebAssembly.instantiate(wasm, {});
const x = instance.exports;
const enc = new TextEncoder();
const dec = new TextDecoder();
function raw(text) {
  const input = enc.encode(text);
  const ptr = x.mbexpr_alloc(input.length);
  new Uint8Array(x.memory.buffer, ptr, input.length).set(input);
  const packed = x.mbexpr_call(ptr, input.length);
  x.mbexpr_free(ptr, input.length);
  const p = Number(packed >> 32n), n = Number(packed & 0xffffffffn);
  const out = dec.decode(new Uint8Array(x.memory.buffer, p, n));
  x.mbexpr_free(p, n);
  return out;
}
const dir = new URL("../conformance/", import.meta.url);
for (const f of readdirSync(dir).filter((f) => f.endsWith(".json")).sort()) {
  // The case file's own text: each request is re-serialized by the
  // engine's reader, so key order and number spelling come from the file.
  const doc = JSON.parse(readFileSync(new URL(f, dir), "utf8"));
  for (const c of doc.cases) {
    const req = {};
    for (const k of ["record", "params", "header"]) if (k in c) req[k] = c[k];
    if ("check" in c) Object.assign(req, { op: "check", expr: c.check });
    else if ("template" in c) Object.assign(req, { op: "template", template: c.template });
    else Object.assign(req, { op: "eval", expr: c.expr });
    process.stdout.write(raw(JSON.stringify(req)) + "\n");
  }
}
