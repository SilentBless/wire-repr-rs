use wire_repr::Wire;

#[derive(Wire)]
pub(super) struct FixedPacket {
    #[wire(be)]
    pub(super) word: u16,
}

#[allow(dead_code)]
#[derive(Wire)]
pub(super) struct DynamicPacket<'wire> {
    pub(super) length: u8,
    #[wire(bytes = length)]
    pub(super) payload: &'wire [u8],
    pub(super) tail: u8,
}

#[derive(Wire)]
pub(super) struct ChoiceBody {
    #[wire(be)]
    pub(super) value: u16,
}

#[allow(dead_code)]
#[derive(Wire)]
#[wire(tag = U8)]
#[wire(unknown = reject)]
#[repr(u8)]
pub(super) enum CodegenChoice {
    Halt = 1,
    Data(ChoiceBody) = 2,
}

#[allow(dead_code)]
#[derive(Wire)]
#[wire(tag = [u8; 4], unknown = reject)]
pub(super) enum CodegenByteChoice {
    #[wire(tag = b"HALT")]
    Halt,
    #[wire(tag = b"DATA")]
    Data(ChoiceBody),
}

#[derive(Wire)]
#[wire(bitfield = u16, be, reserved = zero)]
pub(super) struct CodegenFlags {
    #[wire(bit = 0)]
    pub(super) enabled: bool,
    #[wire(bits = 1..=3)]
    pub(super) mode: u8,
}

#[derive(Wire)]
pub(super) struct PositionedPacket {
    pub(super) tag: u8,
    #[wire(at = 4, be)]
    pub(super) word: u16,
}

#[allow(dead_code)]
#[derive(Wire)]
pub(super) struct ComputedPacket<'wire> {
    #[wire(computed = wire_repr::computation::len(payload))]
    pub(super) length: u8,
    pub(super) kind: u8,
    #[wire(rest)]
    pub(super) payload: &'wire [u8],
}

#[derive(Debug)]
pub(super) struct NonzeroError;

impl core::fmt::Display for NonzeroError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("zero is invalid")
    }
}

impl core::error::Error for NonzeroError {}

fn nonzero(value: u8) -> Result<(), NonzeroError> {
    if value == 0 {
        Err(NonzeroError)
    } else {
        Ok(())
    }
}

#[derive(Wire)]
#[wire(error = NonzeroError)]
pub(super) struct ValidatedChild {
    #[wire(validate = nonzero)]
    pub(super) value: u8,
}

#[allow(dead_code)]
#[derive(Wire)]
#[wire(tag = U8, unknown = reject)]
#[repr(u8)]
pub(super) enum ValidatedChoice {
    Data(ValidatedChild) = 1,
    Halt = 2,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum TableCode {
    Data,
    Halt,
}

#[derive(Debug)]
pub(super) struct TableError;

impl core::fmt::Display for TableError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("table lookup failed")
    }
}

impl core::error::Error for TableError {}

pub(super) struct TinyTable {
    pub(super) data: u8,
    pub(super) halt: u8,
}

impl TinyTable {
    fn decode(&self, raw: u8) -> Result<Option<TableCode>, TableError> {
        Ok(if raw == self.data {
            Some(TableCode::Data)
        } else if raw == self.halt {
            Some(TableCode::Halt)
        } else {
            None
        })
    }

    fn encode(&self, code: TableCode) -> Result<Option<u8>, TableError> {
        Ok(Some(match code {
            TableCode::Data => self.data,
            TableCode::Halt => self.halt,
        }))
    }
}

#[allow(dead_code)]
#[derive(Wire)]
#[wire(tag = U8, table = TinyTable, table_error = TableError, unknown = reject)]
pub(super) enum TableChoice {
    #[wire(table = TableCode::Data)]
    Data(ChoiceBody),
    #[wire(table = TableCode::Halt)]
    Halt,
}

#[derive(Wire)]
pub(super) struct WidePlanPacket {
    pub(super) a: u8,
    pub(super) b: u8,
    pub(super) c: u8,
    pub(super) d: u8,
    pub(super) e: u8,
    pub(super) f: u8,
    pub(super) g: u8,
    pub(super) h: u8,
    pub(super) i: u8,
    pub(super) j: u8,
    pub(super) k: u8,
    pub(super) l: u8,
}

#[derive(Wire)]
pub(super) struct DirectSelectionPacket {
    pub(super) first: u8,
    pub(super) skipped: u8,
    pub(super) last: u8,
}

#[derive(Wire)]
pub(super) struct NestedSelectionChild {
    pub(super) tag: u8,
    #[wire(be)]
    pub(super) member: u16,
}

#[derive(Wire)]
pub(super) struct NestedSelectionPacket {
    pub(super) lead: u8,
    pub(super) child: NestedSelectionChild,
    pub(super) tail: u8,
}
