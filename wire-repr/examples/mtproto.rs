//! Generic composition of two real MTProto TL constructors.

#![allow(dead_code)]

use wire_repr::{WireBuilder, WireView};

#[derive(WireView, WireBuilder)]
struct HelpGetConfig {
    #[wire(le, constant = 0xc4f9_186b)]
    constructor: u32,
}

#[derive(WireView, WireBuilder)]
struct InvokeWithLayer<Q> {
    #[wire(le, constant = 0xda9b_0d0d)]
    constructor: u32,
    #[wire(le)]
    layer: i32,
    query: Q,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = [
        0x0d, 0x0d, 0x9b, 0xda, 0xc8, 0x00, 0x00, 0x00, 0x6b, 0x18, 0xf9, 0xc4,
    ];
    let view = InvokeWithLayer::<HelpGetConfig>::view(input)?;
    assert_eq!(view.layer(), 200);
    assert_eq!(view.query().constructor(), 0xc4f9_186b);

    let mut output = [0u8; 12];
    InvokeWithLayer::<HelpGetConfig>::builder(&mut output[..])
        .layer(200)?
        .query(|query| query)?
        .finish()?;
    assert_eq!(output, input);
    Ok(())
}
