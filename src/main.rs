//! Brainfuck implementation in Rust
#[macro_use]
extern crate pest_derive;
use failure::Fail;

pub mod codegen;
pub mod parser;

#[macro_export]
macro_rules! ice {
    ($($x: expr),*) => {
        return Err(Error::ice(format!($($x),*)));
    };
}

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "{}", _0)]
    ParseError(parser::ParseError),
    #[fail(display = "internal compiler error: {}", _0)]
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

    let res = parser::parse(include_str!("../hanoi.bf")).unwrap();
    let ctx = Context::create();
    let codegen = Codegen::new(&ctx).unwrap();

    codegen.build(&res).unwrap();
}
