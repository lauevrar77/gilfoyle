// Redirect only the Astro compiler's native binding to its WebAssembly
// fallback. Used on CPUs without AVX2 (e.g. some QEMU VMs) where loading the
// native binding raises SIGILL — an unrecoverable signal, not a JS error.
//
// Loaded via `node --require` by scripts/run-astro.mjs. It deliberately does
// NOT set NAPI_RS_NATIVE_LIBRARY_PATH: that variable is global and would also
// hijack rolldown's binding.

const Module = require("node:module");

const WASM_BINDING = require.resolve("@astrojs/compiler-binding-wasm32-wasi");
const originalNodeLoader = Module._extensions[".node"];
const jsLoader = Module._extensions[".js"];

Module._extensions[".node"] = function (module, filename) {
  if (filename.includes("@astrojs/compiler-binding-linux-x64-gnu")) {
    return jsLoader(module, WASM_BINDING);
  }
  return originalNodeLoader(module, filename);
};
