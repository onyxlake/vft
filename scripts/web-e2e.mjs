// No npm dependencies. Run after build-web.ps1 and building the native verify-frames example.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { resolve, join } from "node:path";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { instantiateSender } from "../apps/vft-send-web/web/bridge.js";

const root = resolve(import.meta.dirname, "..");
const wasm = readFileSync(join(root, "output/web-sender/vft_send_web.wasm"));
const verifier = process.argv[2] ?? join(root, "target/debug/examples", process.platform === "win32" ? "verify-frames.exe" : "verify-frames");
const run = join(root, "output", `web-e2e-${Date.now()}`);
mkdirSync(run, { recursive: true });
const results = [];
for (const [profile, size, repairOnly] of [[0, 0, false], [1, 1024, false], [0, 1048576, false], [1, 1048576, false], [1, 1048576, true]]) {
  const data = Uint8Array.from({ length: size }, (_, i) => (i * 17 + (i >> 8)) & 255);
  const hashProgress = [];
  const sender = await instantiateSender(wasm, (offset, length) => data.subarray(offset, offset + length),
    (done, total) => hashProgress.push([done, total]));
  const info = sender.init({ name: "测试.bin", size, profile, session: new Uint8Array(16).fill(7) });
  assert.equal(info.sha256, createHash("sha256").update(data).digest("hex"));
  assert.deepEqual(hashProgress.at(-1), [size, size]);
  assert(info.maxRead <= 65536);
  const frames = [];
  const generations = Math.ceil(info.totalSources / 64);
  const maxFrames = Math.ceil(info.totalSources * 30 / 29) + generations * 64 * 4 + 60;
  for (let index = 0; index < maxFrames; index++) {
    const frame = sender.next();
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
  const diagnostics = sender.info();
  assert(diagnostics.maxRead <= 65536);
  assert(diagnostics.memoryBytes < 8 * 1024 * 1024, "sender heap unexpectedly scales with input size");
  results.push({ profile, size, repairOnly, ...diagnostics, recovery: result.stdout.trim() });
  console.log(`${label}: ${result.stdout.trim()} memory=${diagnostics.memoryBytes} maxRead=${diagnostics.maxRead}`);
}
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
