use crate::{ice, Error, Result};
use pest::{iterators::Pair, Parser};

#[derive(Clone, Debug)]
pub enum BfAST {
    LoopBlock(Vec<BfAST>),
    AddOp,
    SubOp,
    AddPtr,
    SubPtr,
    PutChar,
    GetChar,
}

#[derive(Parser)]
#[grammar = "brainfuck.pest"]
struct BfParser;

pub type ParseError = pest::error::Error<Rule>;

fn visit_symbol(p: Pair<'_, Rule>, v: &mut Vec<BfAST>) -> Result<()> {
    if p.as_rule() != Rule::symbol {
        ice!(
            "wrong visitor: expected symbol, but given {:?}",
            p.as_rule()
        );
    }

    for tok in p.into_inner() {
        match tok.as_rule() {
            Rule::increment => {
                v.push(BfAST::AddOp);
            }
            Rule::decrement => {
                v.push(BfAST::SubOp);
            }
            Rule::pointer_increment => {
                v.push(BfAST::AddPtr);
            }
            Rule::pointer_decrement => {
                v.push(BfAST::SubPtr);
            }
            Rule::print_character => {
                v.push(BfAST::PutChar);
            }
            Rule::get_character => {
                v.push(BfAST::GetChar);
            }
            _ => {
                ice!("unexpected token while visiting block: {:?}", tok);
            }
        }
    }

    Ok(())
}

fn visit_block(p: Pair<'_, Rule>, v: &mut Vec<BfAST>) -> Result<()> {
    if p.as_rule() != Rule::block {
        ice!("wrong visitor: expected block, but given {:?}", p.as_rule());
    }

    for tok in p.into_inner() {
        match tok.as_rule() {
            Rule::symbol => {
                visit_symbol(tok, v)?;
            }
            Rule::loop_block => {
                visit_loop_block(tok, v)?;
            }
            _ => {
                ice!("unexpected token while visiting block: {:?}", tok);
            }
        }
    }

    Ok(())
}

fn visit_loop_block(p: Pair<'_, Rule>, v: &mut Vec<BfAST>) -> Result<()> {
    if p.as_rule() != Rule::loop_block {
        ice!(
            "wrong visitor: expected loop_block, but given {:?}",
            p.as_rule()
        );
    }

    for tok in p.into_inner() {
        match tok.as_rule() {
            Rule::left_brace => {
                continue;
            }
            Rule::right_brace => {
                break;
            }
            Rule::block => {
                let mut v2 = vec![];
                visit_block(tok, &mut v2)?;
                v.push(BfAST::LoopBlock(v2));
            }
            _ => {
                ice!("unexpected token while visiting loop block: {:?}", tok);
            }
        }
    }

    Ok(())
}

fn visit_program(p: Pair<'_, Rule>, v: &mut Vec<BfAST>) -> Result<()> {
    if p.as_rule() != Rule::program {
        ice!(
            "wrong visitor: expected program, but given {:?}",
            p.as_rule()
        );
    }

    for tok in p.into_inner() {
        match tok.as_rule() {
            Rule::EOI => {}
            Rule::block => {
                visit_block(tok, v)?;
            }
            _ => {
                ice!("unexpected token while visiting program: {:?}", tok);
            }
        }
    }

    Ok(())
}

pub fn parse<P: AsRef<str>>(program: P) -> Result<Vec<BfAST>> {
    let pairs = BfParser::parse(Rule::program, program.as_ref())?;
    let program = pairs
        .into_iter()
        .next()
        .ok_or(Error::ice("no matching program"))?;

    let mut program_out = vec![];
    visit_program(program, &mut program_out)?;

    Ok(program_out)
}

#[test]
fn test_parse_hello_world() {
    parse(
        ">+++++++++[<++++++++>-]<.>+++++++[<++++>-]<+.+++++++..+++.[-]>++++++++[<++
    ++>-]<.>+++++++++++[<+++++>-]<.>++++++++[<+++>-]<.+++.------.--------.[-]>
    ++++++++[<++++>-]<+.[-]++++++++++.",
    )
    .unwrap();
}

#[test]
#[should_panic]
fn test_parse_fail() {
    parse("[").unwrap();
}
