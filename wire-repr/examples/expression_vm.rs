//! Compiles, validates, frames, and evaluates a recursive binary expression program.
//!
//! The recursive writer emits `(20 + 22) * -2`. A second schema wraps that bytecode with a derived
//! length and computed FNV-1a digest. `Program::view` runs a schema validator before the VM evaluates
//! the retained recursive child views.

use std::io;

use wire_repr::{ByteSelection, WireBuilder, WireView, Written, output, wire};

const MAX_DEPTH: usize = 128;
type OwnedBytes = output::Owned<Vec<u8>>;
type OwnedWritten = Written<OwnedBytes>;

#[allow(dead_code)]
#[derive(WireView, WireBuilder)]
struct Literal {
    #[wire(le)]
    value: i32,
}

#[allow(dead_code)]
#[derive(WireView, WireBuilder)]
struct Binary<T> {
    left: wire::Recursive<T>,
    right: wire::Recursive<T>,
}

#[allow(dead_code)]
#[derive(WireView, WireBuilder)]
struct Unary<T> {
    value: wire::Recursive<T>,
}

#[allow(dead_code)]
#[derive(WireView, WireBuilder)]
#[wire(selector = u8)]
enum Expression {
    #[wire(value = 0)]
    Literal(Literal),
    #[wire(value = 1)]
    Add(Binary<Expression>),
    #[wire(value = 2)]
    Multiply(Binary<Expression>),
    #[wire(value = 3)]
    Negate(Unary<Expression>),
}

fn fnv1a(selection: impl ByteSelection) -> u32 {
    fnv1a_bytes(selection.bytes())
}

fn fnv1a_bytes(bytes: impl IntoIterator<Item = u8>) -> u32 {
    bytes.into_iter().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("program bytecode digest mismatch")]
struct DigestMismatch;

#[wire_repr::validator]
fn validate_program(view: &impl ProgramView) -> Result<(), DigestMismatch> {
    let represented = view
        .bytecode_length()
        .to_le_bytes()
        .into_iter()
        .chain(view.bytecode().iter().copied());
    (view.digest() == fnv1a_bytes(represented))
        .then_some(())
        .ok_or(DigestMismatch)
}

#[allow(dead_code)]
#[derive(WireView, WireBuilder)]
#[wire(validate = validate_program)]
struct Program {
    #[wire(le, computed = fnv1a(exclude(self)))]
    digest: u32,
    #[wire(le)]
    bytecode_length: u32,
    #[wire(bytes = bytecode_length)]
    bytecode: wire::Bytes,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let expression = compile_expression()?;
    let program = package_program(expression.as_bytes())?;

    let program_view = Program::view(program.as_bytes())?;
    let expression_view = Expression::view::<MAX_DEPTH>(program_view.bytecode())?;
    let result = evaluate(&expression_view)?;

    println!("recursive expression VM");
    println!("  source:   (20 + 22) * -2");
    println!("  program:  {}", hex(program.as_bytes()));
    println!("  bytecode: {}", hex(program_view.bytecode()));
    println!(
        "  digest:   {:#010x} (computed and validated)",
        program_view.digest()
    );
    println!("  result:   {result}");
    Ok(())
}

fn compile_expression() -> Result<OwnedWritten, Box<dyn std::error::Error>> {
    let written = Expression::builder(output::owned(Vec::new()))
        .multiply(|multiply| {
            let multiply = multiply.left(|expression| {
                expression.add(|add| {
                    let add =
                        add.left(|expression| expression.literal(|literal| literal.value(20)))?;
                    add.right(|expression| expression.literal(|literal| literal.value(22)))
                })
            })?;
            multiply.right(|expression| {
                expression.negate(|negate| {
                    negate.value(|expression| expression.literal(|literal| literal.value(2)))
                })
            })
        })?
        .finish()?;
    Ok(written)
}

fn package_program(bytecode: &[u8]) -> Result<OwnedWritten, Box<dyn std::error::Error>> {
    let written = Program::builder(output::owned(Vec::new()))
        .bytecode(bytecode)?
        .finish()?;
    Ok(written)
}

fn evaluate<const DEPTH: usize>(
    expression: &impl ExpressionView<DEPTH>,
) -> Result<i64, Box<dyn std::error::Error>> {
    match expression.variant() {
        ExpressionVariant::Literal(literal) => Ok(i64::from(literal.value())),
        ExpressionVariant::Add(add) => {
            let left = add.left()?;
            let right = add.right()?;
            evaluate(&left)?
                .checked_add(evaluate(&right)?)
                .ok_or_else(|| invalid_data("addition overflow").into())
        }
        ExpressionVariant::Multiply(multiply) => {
            let left = multiply.left()?;
            let right = multiply.right()?;
            evaluate(&left)?
                .checked_mul(evaluate(&right)?)
                .ok_or_else(|| invalid_data("multiplication overflow").into())
        }
        ExpressionVariant::Negate(negate) => {
            let value = negate.value()?;
            evaluate(&value)?
                .checked_neg()
                .ok_or_else(|| invalid_data("negation overflow").into())
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";

    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
