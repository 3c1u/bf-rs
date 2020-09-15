//! Brainfuck implementation in Rust
#[macro_use]
extern crate pest_derive;
use thiserror::Error;

pub mod codegen;
pub mod parser;

#[macro_export]
macro_rules! ice {
    ($($x: expr),*) => {
        return Err(Error::ice(format!($($x),*)));
    };
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    ParseError(parser::ParseError),
    #[error("internal compiler error: {0}")]
    Ice(std::borrow::Cow<'static, str>),
}

impl Error {
    pub fn ice<S: Into<std::borrow::Cow<'static, str>>>(message: S) -> Error {
        Error::Ice(message.into())
    }
}

impl From<parser::ParseError> for Error {
    fn from(p: parser::ParseError) -> Self {
        Self::ParseError(p)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

fn main() {
    use crate::codegen::Codegen;
    use inkwell::context::Context;

    let mut args = std::env::args();
    if args.len() < 2 {
        eprintln!("No file specified. Abort.");
        return;
    }

    let res = parser::parse(std::fs::read_to_string(args.nth(1).unwrap()).unwrap()).unwrap();
    let opt_flag = args.nth(0).map(|v| v.starts_with("--opt")).unwrap_or(false);

    let ctx = Context::create();
    let codegen = Codegen::new(&ctx, opt_flag).unwrap();

    codegen.run(&res).unwrap();
}
