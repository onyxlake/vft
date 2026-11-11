// No npm dependencies. Run after build-web.py and building the native verify-frames example.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { resolve, join } from "node:path";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { createContext, Script } from "node:vm";
import { instantiateSender } from "../apps/vft-send-web/web/bridge.js";

const root = resolve(import.meta.dirname, "..");
const html = readFileSync(join(root, "output/web-sender/index.html"), "utf8");
const wasmBase64 = html.match(/id="vft-wasm"[^>]*>(.*?)<\/script>/s)[1];
const wasm = Buffer.from(wasmBase64, "base64");
const workerSource = JSON.parse(html.match(/id="vft-worker"[^>]*>(.*?)<\/script>/s)[1]);
// Compile the generated scripts as classic JavaScript: stray imports/exports must fail.
new Script(html.match(/<script>\s*([\s\S]*?)<\/script>/)[1], { filename: "bundled-app.js" });
const workerScript = new Script(workerSource, { filename: "bundled-worker.js" });
assert.doesNotMatch(html, /<(?:script|link)\b[^>]*\b(?:src|href)\s*=/i);
const notices = html.match(/<pre id="license-notices"[^>]*>([\s\S]*?)<\/pre>/)[1];
const projectLicense = readFileSync(join(root, "LICENSE"), "utf8").trim()
  .replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
assert(notices.includes(projectLicense), "standalone HTML must retain the full VFT license");
assert.match(notices, /generic-array [\d.]+ — MIT/);
assert.match(notices, /Copyright \(c\) 2015 Bartłomiej Kamiński/);
assert.match(notices, /Rust runtime — rustc/);
assert.doesNotMatch(notices, /<[^>]+>/, "license text must not become HTML markup");

function bundledWorker() {
  let messages;
  const self = { postMessage: message => messages.push(message) };
  // FileReaderSync is browser-only; model its bounded slice contract for this Node check.
  // No fetch/importScripts is supplied: recovery must use only the embedded payload.
  const context = createContext({ self, atob, crypto, performance, WebAssembly, Uint8Array,
    TextEncoder, TextDecoder, FileReaderSync: class {
      readAsArrayBuffer(slice) { assert(slice.byteLength <= 65536); return slice; }
    } });
  workerScript.runInContext(context);
  return async data => {
    messages = [];
    await self.onmessage({ data });
    return messages;
  };
}
const verifier = process.argv[2] ?? join(root, "target/debug/examples", process.platform === "win32" ? "verify-frames.exe" : "verify-frames");
const run = join(root, "output", `web-e2e-${Date.now()}`);
mkdirSync(run, { recursive: true });
const results = [];
for (const [profile, size, repairOnly] of [[0, 0, false], [1, 1024, false], [0, 1048576, false], [1, 1048576, false], [1, 1048576, true]]) {
  const data = Uint8Array.from({ length: size }, (_, i) => (i * 17 + (i >> 8)) & 255);
  const send = bundledWorker();
  const startup = await send({ type: "start", profile, wasmBase64,
    file: { name: "测试.bin", size, slice: (start, end) => data.slice(start, end).buffer } });
  const info = startup.at(-1);
  assert.equal(info.type, "ready", info.message);
  assert.equal(info.sha256, createHash("sha256").update(data).digest("hex"));
  const progress = startup.filter(message => message.type === "hash").at(-1);
  assert.deepEqual([progress.done, progress.total], [size, size]);
  assert(info.maxRead <= 65536);
  const frames = [];
  const generations = Math.ceil(info.totalSources / 64);
  const maxFrames = Math.ceil(info.totalSources * 30 / 29) + generations * 64 * 4 + 60;
  let diagnostics = info;
  for (let index = 0; index < maxFrames; index++) {
    const frame = (await send({ type: "next" })).at(-1);
    assert.equal(frame.type, "frame", frame.message);
    diagnostics = frame;
    // Late join discards the entire systematic pass, including early manifests.
    if (repairOnly && frame.repairs === 0) continue;
    if (index % 7 === 3) continue; // deterministic erasure, independent of the packet type.
    frames.push(Buffer.from(frame.cells));
  }
  // Reorder in bounded batches and include duplicates as a real decoder would encounter.
  const delivered = [];
  for (let i = 0; i < frames.length; i += 9) {
    const batch = frames.slice(i, i + 9).reverse();
    delivered.push(...batch, batch[0]);
  }
  const header = Buffer.alloc(16);
  header.write("VWT0");
  header.writeUInt32LE(info.columns, 4);
  header.writeUInt32LE(info.rows, 8);
  header.writeUInt32LE(delivered.length, 12);
  const label = `${profile}-${size}-${repairOnly ? "repair" : "source"}`;
  const inputPath = join(run, `${label}.bin`), framesPath = join(run, `${label}.frames`);
  writeFileSync(inputPath, data, { flag: "wx" });
  writeFileSync(framesPath, Buffer.concat([header, ...delivered]), { flag: "wx" });
  const result = spawnSync(verifier, [framesPath, inputPath], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr || result.error?.message);
  assert.match(result.stdout, /COMPLETE sha256=verified/);
  assert(diagnostics.maxRead <= 65536);
  assert(diagnostics.memoryBytes < 8 * 1024 * 1024, "sender heap unexpectedly scales with input size");
  const { cells, ...stats } = diagnostics;
  results.push({ profile, size, repairOnly, ...stats, recovery: result.stdout.trim() });
  console.log(`${label}: ${result.stdout.trim()} memory=${diagnostics.memoryBytes} maxRead=${diagnostics.maxRead}`);
}
const unready = bundledWorker();
assert.match((await unready({ type: "next" })).at(-1).message, /not ready/);
const badFile = bundledWorker();
assert.match((await badFile({ type: "start", profile: 1, wasmBase64,
  file: { name: "bad.bin", size: 1, slice() { throw new Error("unreadable file"); } } })).at(-1).message,
  /unreadable file/);
// Validate scalar ABI rejection and read failures, not just the Rust-host adapter.
const api = await WebAssembly.instantiate(wasm, { vft: { read_at: () => 0, hash_progress: () => {} } });
const raw = api.instance.exports;
assert.equal(raw.vft_init(1, -1, 1), 0);
assert.equal(raw.vft_init(1, 1.5, 1), 0);
assert.equal(raw.vft_init(256, 1, 1), 0);
assert.equal(raw.vft_init(1, 1, 256), 0);
assert.equal(raw.vft_next(), 0);
const failing = await instantiateSender(wasm, () => { throw new Error("injected read failure"); });
assert.throws(() => failing.init({ name: "x", size: 1, profile: 1, session: new Uint8Array(16) }), /injected read failure/);
assert.throws(() => failing.next(), /not initialized/);
let unreadable = false;
const changing = await instantiateSender(wasm, (_, length) => {
  if (unreadable) throw new Error("file changed during transmission");
  return new Uint8Array(length);
});
changing.init({ name: "x", size: 1, profile: 0, session: new Uint8Array(16) });
unreadable = true;
changing.next(); // Manifest needs no source read.
assert.throws(() => changing.next(), /file changed during transmission/);
assert.equal(changing.info().generated, 1, "failed source read advanced the sender");
writeFileSync(join(run, "results.json"), JSON.stringify(results, null, 2), { flag: "wx" });
console.log(`WASM/native E2E passed; evidence: ${run}`);
