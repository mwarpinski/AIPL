// Browser host for compiled AIPL. Compiled modules import only the
// wasi_snapshot_preview1 functions they use (AIPL_SPEC.md 6.2); this provides
// browser versions of them:
//   fd_write to fd 1/2       -> the `onOutput` callback (decoded UTF-8)
//   proc_exit                -> throws AIPLExit with the code
//   args_* / environ_*       -> an empty command line and environment
//   fd_read, path_open,      -> ENOSYS (52), which AIPL reports as -1:
//   fd_close, path_unlink_file  a browser has no preopened directory
class AIPLExit extends Error {
    constructor(code) {
        super("sys.exit(" + code + ")");
        this.code = code;
    }
}

class AIPLWebRunner {
    constructor() {
        this.memory = null;
        this.onOutput = (fd, text) => console.log(fd === 2 ? "[stderr] " + text : text);
    }

    view() {
        return new DataView(this.memory.buffer);
    }

    getImports() {
        const ENOSYS = 52;
        const decoder = new TextDecoder();
        const wasi = {
            fd_write: (fd, iovs, iovsLen, nwrittenPtr) => {
                const dv = this.view();
                let written = 0;
                let text = "";
                for (let i = 0; i < iovsLen; i++) {
                    const ptr = dv.getUint32(iovs + i * 8, true);
                    const len = dv.getUint32(iovs + i * 8 + 4, true);
                    text += decoder.decode(new Uint8Array(this.memory.buffer, ptr, len));
                    written += len;
                }
                if (fd !== 1 && fd !== 2) return 8; // EBADF
                this.onOutput(fd, text);
                dv.setUint32(nwrittenPtr, written, true);
                return 0;
            },
            proc_exit: (code) => {
                throw new AIPLExit(code);
            },
            args_sizes_get: (countPtr, sizePtr) => {
                this.view().setUint32(countPtr, 0, true);
                this.view().setUint32(sizePtr, 0, true);
                return 0;
            },
            args_get: () => 0,
            environ_sizes_get: (countPtr, sizePtr) => {
                this.view().setUint32(countPtr, 0, true);
                this.view().setUint32(sizePtr, 0, true);
                return 0;
            },
            environ_get: () => 0,
            fd_read: () => ENOSYS,
            path_open: () => ENOSYS,
            fd_close: () => ENOSYS,
            path_unlink_file: () => ENOSYS,
        };
        return { wasi_snapshot_preview1: wasi };
    }

    // Instantiates a compiled AIPL module and binds its exported memory.
    async instantiate(bytes) {
        const { instance } = await WebAssembly.instantiate(bytes, this.getImports());
        this.memory = instance.exports.memory;
        return instance;
    }
}

if (typeof window !== "undefined") {
    window.AIPLRunner = new AIPLWebRunner();
    window.AIPLExit = AIPLExit;
}
if (typeof module !== "undefined") {
    module.exports = { AIPLWebRunner, AIPLExit };
}
