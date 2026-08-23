use wire_repr::Wire;

#[derive(Wire)]
pub(super) struct OwnedFixed {
    pub(super) lead: u8,
    #[wire(be)]
    pub(super) word: u16,
}

#[derive(Wire)]
pub(super) struct OwnedPacket<'wire> {
    pub(super) length: u8,
    #[wire(bytes = length)]
    pub(super) payload: &'wire [u8],
}

#[derive(Wire)]
pub(super) struct OwnedBody {
    pub(super) value: u8,
}

#[derive(Wire)]
#[wire(tag = U8, unknown = reject)]
#[repr(u8)]
pub(super) enum OwnedOperation {
    Data(OwnedBody) = 1,
    Halt = 2,
}

#[derive(Wire)]
#[wire(bitfield = u16, be, reserved = zero)]
pub(super) struct OwnedFlags {
    #[wire(bit = 0)]
    pub(super) enabled: bool,
    #[wire(bits = 1..=3)]
    pub(super) mode: u8,
}
