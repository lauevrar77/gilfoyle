// Launcher for the Astro CLI.
//
// Astro's Rust compiler ships as a native NAPI binding that assumes a modern
// x86-64 CPU (AVX2/BMI2). On a CPU without those instructions — a QEMU VM, for
// instance — loading the native binding crashes the process with SIGILL, which
// cannot be caught from JavaScript. The compiler ships a WebAssembly fallback;
// this launcher selects it when the CPU lacks AVX2, and leaves the native
// binding in place everywhere else.
//
// The fallback is a cpu=wasm32 package, so npm never installs it on x64; this
// launcher fetches it on demand (once) when it is actually needed.
//
// Force it explicitly with GILFOYLE_FORCE_WASM=1, or opt out with
// GILFOYLE_NATIVE_COMPILER=1.

import { execFileSync, spawn } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const astroBin = join(root, "node_modules", "astro", "bin", "astro.mjs");
const wasmDir = join(
  root,
  "node_modules",
  "@astrojs",
  "compiler-binding-wasm32-wasi",
);
const wasmEntry = join(wasmDir, "astro.wasi.cjs");
const WASM_VERSION = "0.5.3";

function cpuNeedsWasm() {
  if (process.env.GILFOYLE_FORCE_WASM === "1") return true;
  if (process.env.GILFOYLE_NATIVE_COMPILER === "1") return false;
  if (process.platform !== "linux" || process.arch !== "x64") return false;
  try {
    return !/\bavx2\b/.test(readFileSync("/proc/cpuinfo", "utf8"));
  } catch {
    return false;
  }
}

function fetchWasmFallback() {
  const scratch = mkdtempSync(join(tmpdir(), "gilfoyle-wasm-"));
  try {
    process.stderr.write(
      "[gilfoyle] fetching the WASM Astro compiler fallback (one time)…\n",
    );
    const name = execFileSync(
      "npm",
      [
        "pack",
        "--silent",
        `@astrojs/compiler-binding-wasm32-wasi@${WASM_VERSION}`,
      ],
      { cwd: scratch, encoding: "utf8" },
    )
      .trim()
      .split("\n")
      .pop();
    mkdirSync(wasmDir, { recursive: true });
    execFileSync("tar", [
      "-xzf",
      join(scratch, name),
      "-C",
      wasmDir,
      "--strip-components=1",
    ]);
    return existsSync(wasmEntry);
  } catch (error) {
    process.stderr.write(
      `[gilfoyle] could not fetch the fallback: ${error.message}\n` +
        "           Install it manually with:\n" +
        "             npm install --force @astrojs/compiler-binding-wasm32-wasi\n",
    );
    return false;
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

const env = { ...process.env };

if (cpuNeedsWasm()) {
  const ready = existsSync(wasmEntry) || fetchWasmFallback();
  if (ready) {
    // NODE_OPTIONS, not argv: Astro forks helper processes (e.g. the preview
    // server) that must inherit the shim too.
    const prefix = `--require ${join(root, "scripts", "astro-compiler-wasm.cjs")}`;
    env.NODE_OPTIONS = env.NODE_OPTIONS
      ? `${prefix} ${env.NODE_OPTIONS}`
      : prefix;
    process.stderr.write(
      "[gilfoyle] CPU without AVX2 — using the WASM Astro compiler.\n",
    );
  }
}

const child = spawn(process.execPath, [astroBin, ...process.argv.slice(2)], {
  stdio: "inherit",
  env,
});

child.on("exit", (code, signal) => {
  if (signal) process.kill(process.pid, signal);
  else process.exit(code ?? 0);
});
