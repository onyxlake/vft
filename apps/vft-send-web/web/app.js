const $ = id => document.getElementById(id);
const file = $("file"), profile = $("profile"), fps = $("fps"), cell = $("cell");
const start = $("start"), pause = $("pause"), stop = $("stop"), fullscreen = $("fullscreen");
const stage = $("stage"), canvas = $("frame"), status = $("status"), metrics = $("metrics");
const scratch = document.createElement("canvas");
const context = canvas.getContext("2d", { alpha: false });
const scratchContext = scratch.getContext("2d", { alpha: false });
let worker = null, state = "idle", info = null, current = null, pending = null, inFlight = false;
let animation = 0, nextDue = 0, displayed = 0, sampleFrames = 0, sampleStart = 0, observedFps = 0;
let image = null;
let workerUrl = null;

function controls() {
  const active = worker !== null;
  for (const control of [file, profile, fps, cell]) control.disabled = active;
  start.disabled = active || !file.files.length;
  stop.disabled = !active;
  pause.disabled = !info || !active;
  fullscreen.disabled = !info || !active;
  pause.textContent = state === "paused" ? "Resume" : "Pause";
}
function reset(message, nextState = "idle") {
  worker?.terminate();
  worker = null;
  if (workerUrl) URL.revokeObjectURL(workerUrl);
  workerUrl = null;
  cancelAnimationFrame(animation);
  animation = 0;
  state = nextState;
  info = current = pending = image = null;
  inFlight = false;
  displayed = sampleFrames = observedFps = 0;
  canvas.hidden = true;
  $("placeholder").hidden = false;
  $("placeholder").textContent = "The transfer frame will appear here.";
  $("dimensions").textContent = "";
  $("digest").textContent = "Available after preparing the file.";
  metrics.textContent = "Displayed frames: 0 · Display FPS: 0.0";
  status.textContent = message;
  controls();
}
function resize() {
  if (!info) return false;
  const dpr = window.devicePixelRatio || 1;
  const full = document.fullscreenElement === stage;
  const availableWidth = full ? window.innerWidth : stage.clientWidth;
  const availableHeight = full ? window.innerHeight
    : Math.max(0, window.innerHeight - Math.max(0, stage.getBoundingClientRect().top) - 16);
  const actualCell = Math.min(Number(cell.value),
    Math.floor(availableWidth * dpr / info.columns), Math.floor(availableHeight * dpr / info.rows));
  if (actualCell < 2) {
    canvas.hidden = true;
    $("placeholder").hidden = false;
    $("placeholder").textContent = "Window too small. Enlarge it or use fullscreen, then resume.";
    setPaused("Window too small. Enlarge it or use fullscreen, then resume.");
    return false;
  }
  canvas.width = info.columns * actualCell;
  canvas.height = info.rows * actualCell;
  canvas.style.width = `${canvas.width / dpr}px`;
  canvas.style.height = `${canvas.height / dpr}px`;
  context.imageSmoothingEnabled = false;
  $("dimensions").textContent = `${canvas.width} × ${canvas.height} px · ${actualCell} px / cell`;
  if (current) paint(current);
  return true;
}
function paint(frame) {
  if (!image) {
    scratch.width = info.columns;
    scratch.height = info.rows;
    image = scratchContext.createImageData(info.columns, info.rows);
  }
  for (let i = 0; i < frame.cells.length; i++) {
    const value = frame.cells[i];
    image.data[i * 4] = image.data[i * 4 + 1] = image.data[i * 4 + 2] = value;
    image.data[i * 4 + 3] = 255;
  }
  scratchContext.putImageData(image, 0, 0);
  context.drawImage(scratch, 0, 0, canvas.width, canvas.height);
  canvas.hidden = false;
  $("placeholder").hidden = true;
}
function requestFrame() {
  if (state === "running" && !pending && !inFlight && worker) {
    inFlight = true;
    worker.postMessage({ type: "next" });
  }
}
function tick(now) {
  animation = 0;
  if (state !== "running") return;
  const interval = 1000 / Number(fps.value);
  if (pending && now >= nextDue - 0.1) {
    current = pending;
    pending = null;
    paint(current);
    displayed++;
    sampleFrames++;
    nextDue = now - nextDue > interval ? now + interval : nextDue + interval;
    if (now - sampleStart >= 1000) {
      observedFps = sampleFrames * 1000 / (now - sampleStart);
      sampleStart = now;
      sampleFrames = 0;
    }
    metrics.textContent = `Displayed frames: ${displayed} · Display FPS: ${observedFps.toFixed(1)} · Source symbols: ${current.sources} / ${current.totalSources} · Repair symbols: ${current.repairs}`;
    status.textContent = current.sources >= current.totalSources
      ? "Sending repair frames. Stop when the receiver finishes." : "Sending file…";
  }
  requestFrame();
  animation = requestAnimationFrame(tick);
}
function setPaused(message = "Paused. Click Resume to continue.") {
  if (!info || !worker) return;
  state = "paused";
  cancelAnimationFrame(animation);
  animation = 0;
  status.textContent = message;
  controls();
}
function play() {
  if (!info || !worker || document.hidden || !resize()) return;
  state = "running";
  nextDue = performance.now();
  sampleFrames = observedFps = 0;
  sampleStart = performance.now();
  status.textContent = "Sending file…";
  controls();
  requestFrame();
  if (!animation) animation = requestAnimationFrame(tick);
}
start.onclick = () => {
  if (!file.files.length || !fps.checkValidity() || !cell.checkValidity()
      || !Number.isInteger(Number(fps.value)) || !Number.isInteger(Number(cell.value))) {
    status.textContent = "Select a file and use whole numbers for FPS (1–60) and pixels per cell (2–32).";
    return;
  }
  const selected = file.files[0];
  reset("Preparing file…");
  state = "preparing";
  try {
    const source = JSON.parse($("vft-worker").textContent);
    workerUrl = URL.createObjectURL(new Blob([source], { type: "text/javascript" }));
    const sessionWorker = new Worker(workerUrl);
    worker = sessionWorker;
    controls();
    sessionWorker.onmessage = ({ data }) => {
      if (worker !== sessionWorker) return;
      if (data.type === "hash") {
        status.textContent = `Hashing file: ${data.total ? (data.done / data.total * 100).toFixed(0) : 100}%`;
      } else if (data.type === "ready") {
        info = data;
        $("digest").textContent = data.sha256;
        state = "paused";
        if (document.hidden) { controls(); status.textContent = "File ready. Return to this tab and click Resume."; }
        else play();
      } else if (data.type === "frame") {
        inFlight = false;
        pending = data;
      } else if (data.type === "error") reset(`Sending failed: ${data.message}`, "error");
    };
    sessionWorker.onerror = event => {
      if (worker === sessionWorker) reset(`Sending failed: ${event.message}`, "error");
    };
    sessionWorker.postMessage({ type: "start", file: selected, profile: Number(profile.value),
      wasmBase64: $("vft-wasm").textContent.trim() });
  } catch (error) { reset(`Cannot start sending: ${error.message}`, "error"); }
};
pause.onclick = () => state === "running" ? setPaused() : play();
stop.onclick = () => reset("Stopped. Select another file or start again.");
file.onchange = () => { status.textContent = file.files.length ? `${file.files[0].name} · ${file.files[0].size.toLocaleString()} bytes` : "Select a file to begin."; controls(); };
profile.onchange = () => { cell.value = profile.value === "0" ? 16 : 8; };
async function toggleFullscreen() {
  try {
    if (document.fullscreenElement) await document.exitFullscreen();
    else await stage.requestFullscreen();
  } catch (error) { status.textContent = `Cannot change fullscreen mode: ${error.message}`; }
}
fullscreen.onclick = toggleFullscreen;
document.addEventListener("fullscreenchange", resize);
window.addEventListener("resize", resize);
document.addEventListener("visibilitychange", () => { if (document.hidden) setPaused("Tab hidden. Return here and click Resume."); });
window.addEventListener("pagehide", () => reset("Stopped."));
document.addEventListener("keydown", event => {
  if (event.code === "F11" && !fullscreen.disabled) { event.preventDefault(); toggleFullscreen(); return; }
  if (["INPUT", "SELECT", "BUTTON"].includes(event.target.tagName)) return;
  if (event.code === "Space" && !pause.disabled) { event.preventDefault(); pause.click(); }
});
