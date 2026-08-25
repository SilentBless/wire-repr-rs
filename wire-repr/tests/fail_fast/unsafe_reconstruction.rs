//! error: call to unsafe function
//! error: requires unsafe block

struct Foo;

unsafe impl wire_repr::WireView for Foo {
    type Error = core::convert::Infallible;
    type State = ();
    type View<'view> = &'view [u8];

    const FIXED_SIZE: Option<usize> = Some(0);

    fn frame(
        _input: &[u8],
        _absolute_offset: usize,
    ) -> Result<wire_repr::Frame<Self::State>, Self::Error> {
        Ok(wire_repr::Frame::new((), 0))
    }

    unsafe fn from_validated_parts<'view>(
        input: &'view [u8],
        _state: &'view Self::State,
    ) -> Self::View<'view> {
        input
    }
}

fn bar() {
    let _ = <Foo as wire_repr::WireView>::from_validated_parts(&[], &());
}
