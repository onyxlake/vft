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

function controls() {
  const active = worker !== null;
  for (const control of [file, profile, fps, cell]) control.disabled = active;
  start.disabled = active || !file.files.length;
  stop.disabled = !active;
  pause.disabled = !info || !active;
  fullscreen.disabled = !info || !active;
  pause.textContent = state === "running" ? "暂停" : "继续";
}
function reset(message, nextState = "idle") {
  worker?.terminate();
  worker = null;
  cancelAnimationFrame(animation);
  animation = 0;
  state = nextState;
  info = current = pending = image = null;
  inFlight = false;
  displayed = sampleFrames = observedFps = 0;
  canvas.hidden = true;
  $("placeholder").hidden = false;
  $("placeholder").textContent = "发送画面将在这里显示";
  $("dimensions").textContent = "";
  $("digest").textContent = "准备文件后显示";
  metrics.textContent = "已显示 0 帧 · 显示 FPS 0.0";
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
    $("placeholder").textContent = "窗口太小，请扩大窗口或进入全屏后继续";
    setPaused("窗口太小，请扩大窗口或进入全屏后继续");
    return false;
  }
  canvas.width = info.columns * actualCell;
  canvas.height = info.rows * actualCell;
  canvas.style.width = `${canvas.width / dpr}px`;
  canvas.style.height = `${canvas.height / dpr}px`;
  context.imageSmoothingEnabled = false;
  $("dimensions").textContent = `${canvas.width} × ${canvas.height} px · ${actualCell} px / 单元`;
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
    metrics.textContent = `已显示 ${displayed} 帧 · 显示 FPS ${observedFps.toFixed(1)} · 源符号 ${current.sources} / ${current.totalSources} · 修复符号 ${current.repairs}`;
    status.textContent = current.sources >= current.totalSources
      ? "正在发送修复帧 · 接收完成后可停止" : "正在发送文件";
  }
  requestFrame();
  animation = requestAnimationFrame(tick);
}
function setPaused(message = "已暂停 · 点击继续发送") {
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
  status.textContent = "正在发送文件";
  controls();
  requestFrame();
  if (!animation) animation = requestAnimationFrame(tick);
}
start.onclick = () => {
  if (!file.files.length || !fps.checkValidity() || !cell.checkValidity()
      || !Number.isInteger(Number(fps.value)) || !Number.isInteger(Number(cell.value))) {
    status.textContent = "请选择文件，并使用整数 FPS（1–60）和单元像素（2–32）";
    return;
  }
  const selected = file.files[0];
  reset("正在准备文件…");
  state = "preparing";
  try {
    const sessionWorker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
    worker = sessionWorker;
    controls();
    sessionWorker.onmessage = ({ data }) => {
      if (worker !== sessionWorker) return;
      if (data.type === "hash") {
        status.textContent = `正在校验文件 ${data.total ? (data.done / data.total * 100).toFixed(0) : 100}%`;
      } else if (data.type === "ready") {
        info = data;
        $("digest").textContent = data.sha256;
        state = "paused";
        if (document.hidden) { controls(); status.textContent = "文件已就绪 · 返回页面后点击继续"; }
        else play();
      } else if (data.type === "frame") {
        inFlight = false;
        pending = data;
      } else if (data.type === "error") reset(`发送失败：${data.message}`, "error");
    };
    sessionWorker.onerror = event => {
      if (worker === sessionWorker) reset(`发送失败：${event.message}`, "error");
    };
    sessionWorker.postMessage({ type: "start", file: selected, profile: Number(profile.value) });
  } catch (error) { reset(`无法启动发送：${error.message}`, "error"); }
};
pause.onclick = () => state === "running" ? setPaused() : play();
stop.onclick = () => reset("已停止 · 可以重新选择文件");
file.onchange = () => { status.textContent = file.files.length ? `已选择 ${file.files[0].name} · ${file.files[0].size.toLocaleString()} 字节` : "等待选择文件"; controls(); };
profile.onchange = () => { cell.value = profile.value === "0" ? 16 : 8; };
async function toggleFullscreen() {
  try {
    if (document.fullscreenElement) await document.exitFullscreen();
    else await stage.requestFullscreen();
  } catch (error) { status.textContent = `无法进入全屏：${error.message}`; }
}
fullscreen.onclick = toggleFullscreen;
document.addEventListener("fullscreenchange", resize);
window.addEventListener("resize", resize);
document.addEventListener("visibilitychange", () => { if (document.hidden) setPaused("页面已转入后台 · 返回后点击继续"); });
window.addEventListener("pagehide", () => reset("已停止"));
document.addEventListener("keydown", event => {
  if (event.code === "F11" && !fullscreen.disabled) { event.preventDefault(); toggleFullscreen(); return; }
  if (["INPUT", "SELECT", "BUTTON"].includes(event.target.tagName)) return;
  if (event.code === "Space" && !pause.disabled) { event.preventDefault(); pause.click(); }
});
