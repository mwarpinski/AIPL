use crate::ast::Module;
use rmp_serde::{Deserializer, Serializer};
use serde::{Deserialize, Serialize};

pub struct BinaryAstCompiler;

impl BinaryAstCompiler {
    pub fn encode(module: &Module) -> Result<Vec<u8>, String> {
        let mut buf = Vec::new();
        module
            .serialize(&mut Serializer::new(&mut buf))
            .map_err(|e| format!("Binary AST serialization failed: {}", e))?;
        Ok(buf)
    }

    pub fn decode(bytes: &[u8]) -> Result<Module, String> {
        let mut de = Deserializer::new(bytes);
        Module::deserialize(&mut de).map_err(|e| format!("Binary AST deserialization failed: {}", e))
    }
}
