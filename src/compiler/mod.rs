pub mod binary_ast;
pub mod wasm;

use crate::ast::Module;

pub trait Compiler {
    type Output;
    fn compile(&self, module: &Module) -> Result<Self::Output, String>;
}
