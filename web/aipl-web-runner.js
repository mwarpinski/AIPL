// Universal WebAssembly Runtime Host for AIPL Programs
class AIPLWebRunner {
    constructor() {
        this.elements = {};
        this.elementCount = 0;
    }

    getImports() {
        return {
            env: {
                dom_elem: (tagPtr, classPtr, textPtr) => {
                    const tag = "div";
                    const el = document.createElement(tag);
                    this.elementCount++;
                    this.elements[this.elementCount] = el;
                    return this.elementCount;
                },
                dom_mount: (selectorPtr, elemId) => {
                    const el = this.elements[elemId];
                    const app = document.querySelector("#app") || document.body;
                    if (el && app) app.appendChild(el);
                },
                sys_print: (val) => {
                    console.log("[AIPL Wasm Print]:", val);
                }
            }
        };
    }

    async runWasmBytes(bytes) {
        const importObj = this.getImports();
        const { instance } = await WebAssembly.instantiate(bytes, importObj);
        console.log("[AIPL Wasm Loaded] Exports:", Object.keys(instance.exports));
        return instance.exports;
    }
}

window.AIPLRunner = new AIPLWebRunner();
