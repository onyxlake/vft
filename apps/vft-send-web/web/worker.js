import { instantiateSender } from "./bridge.js";

let sender = null;
self.onmessage = async ({ data }) => {
  try {
    if (data.type === "start") {
      if (sender) throw new Error("A sender session is already active.");
      if (typeof FileReaderSync === "undefined") throw new Error("This browser does not support file reads in a Worker.");
      const file = data.file;
      const reader = new FileReaderSync();
      // The build embeds WASM in the page. Decode and compile it here, with no fetch.
      const bytes = Uint8Array.from(atob(data.wasmBase64), char => char.charCodeAt(0));
      let lastProgress = -Infinity;
      sender = await instantiateSender(bytes, (offset, length) => {
        if (!Number.isSafeInteger(offset) || offset < 0 || length > 65536 || offset + length > file.size) {
          throw new Error("File read is out of bounds.");
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
      if (!sender) throw new Error("The sender session is not ready.");
      const frame = sender.next();
      self.postMessage({ type: "frame", ...frame }, [frame.cells.buffer]);
    } else {
      throw new Error("Unknown sender command.");
    }
  } catch (error) {
    self.postMessage({ type: "error", message: String(error.message ?? error) });
  }
};
