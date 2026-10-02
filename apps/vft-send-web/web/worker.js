import { instantiateSender } from "./bridge.js";

let sender = null;
self.onmessage = async ({ data }) => {
  try {
    if (data.type === "start") {
      if (sender) throw new Error("发送会话已启动");
      if (typeof FileReaderSync === "undefined") throw new Error("当前浏览器不支持 Worker 文件读取");
      const file = data.file;
      const reader = new FileReaderSync();
      const response = await fetch(new URL("./vft_send_web.wasm", import.meta.url));
      if (!response.ok) throw new Error(`WASM 加载失败：HTTP ${response.status}，请先构建网页`);
      let lastProgress = -Infinity;
      sender = await instantiateSender(await response.arrayBuffer(), (offset, length) => {
        if (!Number.isSafeInteger(offset) || offset < 0 || length > 65536 || offset + length > file.size) {
          throw new Error("文件读取超出范围");
        }
        return new Uint8Array(reader.readAsArrayBuffer(file.slice(offset, offset + length)));
      }, (done, total) => {
        const now = performance.now();
        if (now - lastProgress >= 100 || done === total) {
          self.postMessage({ type: "hash", done, total });
          lastProgress = now;
        }
      });
      const info = sender.init({ name: file.name, size: file.size, profile: data.profile,
        session: crypto.getRandomValues(new Uint8Array(16)) });
      self.postMessage({ type: "ready", ...info });
    } else if (data.type === "next") {
      if (!sender) throw new Error("发送会话尚未就绪");
      const frame = sender.next();
      self.postMessage({ type: "frame", ...frame }, [frame.cells.buffer]);
    } else {
      throw new Error("未知发送指令");
    }
  } catch (error) {
    self.postMessage({ type: "error", message: String(error.message ?? error) });
  }
};
