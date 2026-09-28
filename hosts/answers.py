"""Every conformance case's raw answer from the WebAssembly module under
wasmtime (compute's host), one per line, to compare byte for byte with
the native build and Node."""

import json
import pathlib
import sys

import wasmtime

ROOT = pathlib.Path(__file__).resolve().parent.parent
wasm = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "target/wasm32-unknown-unknown/release/mbexpr.wasm"
engine = wasmtime.Engine()
store = wasmtime.Store(engine)
instance = wasmtime.Instance(store, wasmtime.Module.from_file(engine, str(wasm)), [])
x = instance.exports(store)
memory = x["memory"]


def raw(text: str) -> str:
    data = text.encode()
    ptr = x["mbexpr_alloc"](store, len(data))
    memory.write(store, data, ptr)
    packed = x["mbexpr_call"](store, ptr, len(data))
    x["mbexpr_free"](store, ptr, len(data))
    p, n = (packed >> 32) & 0xFFFFFFFF, packed & 0xFFFFFFFF
    out = memory.read(store, p, p + n).decode()
    x["mbexpr_free"](store, p, n)
    return out


for f in sorted((ROOT / "conformance").glob("*.json")):
    doc = json.loads(f.read_text())
    for c in doc["cases"]:
        req = {k: c[k] for k in ("record", "params", "header") if k in c}
        if "check" in c:
            req.update(op="check", expr=c["check"])
        elif "template" in c:
            req.update(op="template", template=c["template"])
        else:
            req.update(op="eval", expr=c["expr"])
        print(raw(json.dumps(req, ensure_ascii=False)))
