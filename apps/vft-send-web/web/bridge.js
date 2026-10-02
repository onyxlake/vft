// The only WASM boundary: scalar values and bounded Rust-owned byte buffers.
export async function instantiateSender(bytes, readAt, onHashProgress = () => {}) {
  let api;
  let readError = null;
  const diagnostics = { reads: 0, maxRead: 0 };
  const { instance } = await WebAssembly.instantiate(bytes, {
    vft: {
      read_at(offset, pointer, length) {
        try {
          const data = readAt(offset, length);
          if (!(data instanceof Uint8Array) || data.length !== length) {
            throw new Error("文件读取长度不符");
          }
          // memory.grow invalidates previous views: always create a fresh one.
          new Uint8Array(api.memory.buffer, pointer, length).set(data);
          diagnostics.reads++;
          diagnostics.maxRead = Math.max(diagnostics.maxRead, length);
          return 1;
        } catch (error) {
          readError = String(error.message ?? error);
          return 0;
        }
      },
      hash_progress: onHashProgress,
    },
  });
  api = instance.exports;
  const copy = (pointer, length) => new Uint8Array(api.memory.buffer, pointer, length).slice();
  function check(ok) {
    if (ok !== 1) {
      const message = new TextDecoder().decode(copy(api.vft_error_ptr(), api.vft_error_len()));
      throw new Error(readError ? `${message}: ${readError}` : message);
    }
  }
  return {
    init({ name, size, profile, session }) {
      const filename = new TextEncoder().encode(name);
      if (!Number.isSafeInteger(size) || size < 0 || !filename.length || filename.length > 255
          || !(session instanceof Uint8Array) || session.length !== 16) {
        throw new Error("文件大小或文件名不受支持（文件名最多 255 个 UTF-8 字节）");
      }
      readError = null;
      diagnostics.reads = 0;
      diagnostics.maxRead = 0;
      const input = new Uint8Array(api.memory.buffer, api.vft_input_ptr(), 271);
      input.set(session);
      input.set(filename, 16);
      check(api.vft_init(profile, size, filename.length));
      return this.info();
    },
    info() {
      return {
        columns: api.vft_stat(0), rows: api.vft_stat(1),
        generated: api.vft_stat(2), sources: api.vft_stat(3), repairs: api.vft_stat(4),
        totalSources: api.vft_stat(5),
        sha256: Array.from(copy(api.vft_digest_ptr(), 32), b => b.toString(16).padStart(2, "0")).join(""),
        memoryBytes: api.memory.buffer.byteLength, ...diagnostics,
      };
    },
    next() {
      readError = null;
      check(api.vft_next());
      return { cells: copy(api.vft_frame_ptr(), api.vft_frame_len()), ...this.info() };
    },
  };
}
