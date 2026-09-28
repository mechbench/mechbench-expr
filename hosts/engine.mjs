// The engine in a JavaScript host (Node or a browser): load the module,
// then `call(request)` with a request object, answered as the Rust side
// answers (see src/api.rs).
export async function load(bytes) {
  const { instance } = await WebAssembly.instantiate(bytes, {});
  const x = instance.exports;
  const enc = new TextEncoder();
  const dec = new TextDecoder();
  return {
    call(request) {
      const input = enc.encode(JSON.stringify(request));
      const ptr = x.mbexpr_alloc(input.length);
      new Uint8Array(x.memory.buffer, ptr, input.length).set(input);
      const packed = x.mbexpr_call(ptr, input.length);
      x.mbexpr_free(ptr, input.length);
      const outPtr = Number(packed >> 32n);
      const outLen = Number(packed & 0xffffffffn);
      const text = dec.decode(new Uint8Array(x.memory.buffer, outPtr, outLen));
      x.mbexpr_free(outPtr, outLen);
      return JSON.parse(text);
    },
  };
}
