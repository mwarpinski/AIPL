use crate::checker::TypeChecker;
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
                let response = Response::from_string(json).with_header(
                    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                let _ = request.respond(response);
            } else if url == "/verify" && request.method() == &tiny_http::Method::Post {
                let resp = Self::handle_verify(&body);
                let json = serde_json::to_string(&resp).unwrap_or_default();
                let response = Response::from_string(json).with_header(
                    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                let _ = request.respond(response);
            } else {
                let response = Response::from_string("AIPL Agent Server Online. Use /eval or /verify").with_status_code(200);
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
}
