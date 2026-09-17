pub struct WebBindings;

impl WebBindings {
    pub fn get_browser_js_bridge() -> &'static str {
        r#"
// AIPL WebAssembly Browser Bridge Runtime
window.AIPL = {
    createElement: function(tag, className, textContent) {
        const el = document.createElement(tag);
        if (className) el.className = className;
        if (textContent) el.textContent = textContent;
        if (!window._aipl_elements) window._aipl_elements = {};
        const id = Object.keys(window._aipl_elements).length + 1;
        window._aipl_elements[id] = el;
        return id;
    },
    mount: function(selector, id) {
        const parent = document.querySelector(selector) || document.body;
        const el = window._aipl_elements[id];
        if (parent && el) parent.appendChild(el);
    },
    onEvent: function(id, eventType, callbackPtr) {
        const el = window._aipl_elements[id];
        if (el) {
            el.addEventListener(eventType, (e) => {
                if (window._aipl_wasm_exports && window._aipl_wasm_exports[callbackPtr]) {
                    window._aipl_wasm_exports[callbackPtr]();
                }
            });
        }
    }
};
"#
    }
}
