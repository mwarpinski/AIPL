use crate::checker::TypeChecker;
use crate::compiler::wasm::WasmCompiler;
use crate::parser::Parser;
use crate::vm::{Value, VM};
use serde::{Deserialize, Serialize};
use tiny_http::{Response, Server as HttpServer};

#[derive(Deserialize)]
pub struct EvalRequest {
    pub source: String,
    pub fn_name: String,
    pub args: Option<Vec<i64>>,
}

#[derive(Serialize)]
pub struct EvalResponse {
    pub success: bool,
    pub result: Option<String>,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct CompileResponse {
    pub success: bool,
    pub wasm_base64: Option<String>,
    pub error: Option<String>,
}

pub struct AgentServer;

impl AgentServer {
    pub fn start(addr: &str) -> Result<(), String> {
        let server = HttpServer::http(addr).map_err(|e| format!("Server start error: {}", e))?;
        println!("AIPL Agent RPC Server listening on http://{}", addr);

        for mut request in server.incoming_requests() {
            let url = request.url().to_string();
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);

            if url == "/eval" && request.method() == &tiny_http::Method::Post {
                let resp = Self::handle_eval(&body);
                let json = serde_json::to_string(&resp).unwrap_or_default();
                let response = Response::from_string(json)
                    .with_header(
                        tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                    )
                    .with_header(Self::cors_header());
                let _ = request.respond(response);
            } else if url == "/verify" && request.method() == &tiny_http::Method::Post {
                let resp = Self::handle_verify(&body);
                let json = serde_json::to_string(&resp).unwrap_or_default();
                let response = Response::from_string(json)
                    .with_header(
                        tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                    )
                    .with_header(Self::cors_header());
                let _ = request.respond(response);
            } else if url == "/compile" && request.method() == &tiny_http::Method::Post {
                let resp = Self::handle_compile(&body);
                let json = serde_json::to_string(&resp).unwrap_or_default();
                let response = Response::from_string(json)
                    .with_header(
                        tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                    )
                    .with_header(Self::cors_header());
                let _ = request.respond(response);
            } else {
                let response = Response::from_string("AIPL Agent Server Online. Use /eval, /verify, or /compile")
                    .with_status_code(200)
                    .with_header(Self::cors_header());
                let _ = request.respond(response);
            }
        }
        Ok(())
    }

    fn handle_eval(body: &str) -> EvalResponse {
        let req: EvalRequest = match serde_json::from_str(body) {
            Ok(r) => r,
            Err(e) => {
                return EvalResponse {
                    success: false,
                    result: None,
                    error: Some(format!("Invalid JSON payload: {}", e)),
                }
            }
        };

        let module = match Parser::parse(&req.source) {
            Ok(m) => m,
            Err(e) => {
                return EvalResponse {
                    success: false,
                    result: None,
                    error: Some(format!("Parse error: {}", e)),
                }
            }
        };

        let mut checker = TypeChecker::new();
        if let Err(e) = checker.check_module(&module) {
            return EvalResponse {
                success: false,
                result: None,
                error: Some(format!("Type check error: {}", e)),
            };
        }

        let mut vm = VM::new();
        vm.load_module(module);

        let vm_args: Vec<Value> = req
            .args
            .unwrap_or_default()
            .into_iter()
            .map(Value::Int)
            .collect();

        match vm.invoke(&req.fn_name, vm_args) {
            Ok(val) => EvalResponse {
                success: true,
                result: Some(format!("{:?}", val)),
                error: None,
            },
            Err(e) => EvalResponse {
                success: false,
                result: None,
                error: Some(format!("VM Execution error: {}", e)),
            },
        }
    }

    fn handle_verify(body: &str) -> EvalResponse {
        let module = match Parser::parse(body) {
            Ok(m) => m,
            Err(e) => {
                return EvalResponse {
                    success: false,
                    result: None,
                    error: Some(format!("Parse error: {}", e)),
                }
            }
        };

        let mut checker = TypeChecker::new();
        match checker.check_module(&module) {
            Ok(_) => EvalResponse {
                success: true,
                result: Some("Module type check and contract verification passed.".to_string()),
                error: None,
            },
            Err(e) => EvalResponse {
                success: false,
                result: None,
                error: Some(format!("Verification failed: {}", e)),
            },
        }
    }

    /// Parses, type-checks, and compiles raw AIPL source (the POST body) to a real
    /// WebAssembly module, returned base64-encoded so browser hosts (which cannot
    /// invoke the Rust toolchain directly) can `WebAssembly.instantiate` it.
    fn handle_compile(body: &str) -> CompileResponse {
        let module = match Parser::parse(body) {
            Ok(m) => m,
            Err(e) => {
                return CompileResponse {
                    success: false,
                    wasm_base64: None,
                    error: Some(format!("Parse error: {}", e)),
                }
            }
        };

        let mut checker = TypeChecker::new();
        if let Err(e) = checker.check_module(&module) {
            return CompileResponse {
                success: false,
                wasm_base64: None,
                error: Some(format!("Type check error: {}", e)),
            };
        }

        match WasmCompiler::compile(&module) {
            Ok(bytes) => CompileResponse {
                success: true,
                wasm_base64: Some(base64_encode(&bytes)),
                error: None,
            },
            Err(e) => CompileResponse {
                success: false,
                wasm_base64: None,
                error: Some(format!("Wasm codegen error: {}", e)),
            },
        }
    }

    fn cors_header() -> tiny_http::Header {
        tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap()
    }
}

/// Minimal RFC 4648 base64 encoder. Hand-rolled instead of pulling in a crate so the
/// RPC boundary stays as small a non-AIPL surface as possible.
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(CHARS[((n >> 18) & 0x3F) as usize] as char);
        out.push(CHARS[((n >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 { CHARS[((n >> 6) & 0x3F) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { CHARS[(n & 0x3F) as usize] as char } else { '=' });
    }
    out
}
