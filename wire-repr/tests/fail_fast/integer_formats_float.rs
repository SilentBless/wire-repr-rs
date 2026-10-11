//! error: integer encodings require a physical integer

use wire_repr::WireView;

#[derive(WireView)]
struct FloatingEncoding {
    #[wire(vu128)]
    value: f32,
}
