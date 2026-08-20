use wire_repr::{U24RangeError, wire_repr};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Kind(u16);

impl From<u16> for Kind {
    fn from(value: u16) -> Self {
        Self(value)
    }
}

impl From<Kind> for u16 {
    fn from(value: Kind) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Address([u8; 4]);

impl From<[u8; 4]> for Address {
    fn from(value: [u8; 4]) -> Self {
        Self(value)
    }
}

impl From<Address> for [u8; 4] {
    fn from(value: Address) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Flags(u8);

impl From<u8> for Flags {
    fn from(value: u8) -> Self {
        Self(value)
    }
}

impl From<Flags> for u8 {
    fn from(value: Flags) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Value(u32);

impl From<u32> for Value {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<Value> for u32 {
    fn from(value: Value) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Length(u8);

impl From<u8> for Length {
    fn from(value: u8) -> Self {
        Self(value)
    }
}

impl From<Length> for u8 {
    fn from(value: Length) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Endpoint(u16);

impl From<u16> for Endpoint {
    fn from(value: u16) -> Self {
        Self(value)
    }
}

impl From<Endpoint> for u16 {
    fn from(value: Endpoint) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Code(u8);

impl From<u8> for Code {
    fn from(value: u8) -> Self {
        Self(value)
    }
}

impl From<Code> for u8 {
    fn from(value: Code) -> Self {
        value.0
    }
}

wire_repr! {
    pub layout Sequential {
        kind: BeU16 as crate::Kind;
        address: bytes(4) as crate::Address;
        flags: U8 as crate::Flags {
            projections {
                bit enabled: 0;
                bits mode: 1..=3;
            }
        };
        value: BeU24 as crate::Value;
    }

    pub absolute layout Absolute {
        kind @ 5: BeU16 as crate::Kind;
        address @ 0: bytes(4) as crate::Address;
        code @ 4: U8 as crate::Code;
    }

    pub layout Dynamic {
        length: U8 as crate::Length;
        payload: bytes(length);
        code: U8 as crate::Code;
    }

    pub layout AbsoluteEndpoint {
        tag @ 1: U8;
        end @ 2: BeU16 as crate::Endpoint;
        padding(1) @ 3;
        align(4) @ 4;
        payload @ 5: bytes_to(end);
        tail @ 6: U8;
    }

    pub layout EndpointCases {
        tag @ 1: U8;
        end @ 2: U8;
        payload @ 3: bytes_to(end);
    }

    pub layout SignedEndpoint {
        end @ 1: BeI16;
        payload @ 2: bytes_to(end);
    }
}

#[test]
fn sequential_mappings_preserve_semantics_raw_values_and_atomic_failures() {
    let input = [0x12, 0x34, 1, 2, 3, 4, 0b0000_1011, 0x12, 0x34, 0x56, 0xee];
    let (view, suffix) = Sequential::view(&input).with_remainder().unwrap();
    assert_eq!(suffix, &[0xee]);
    assert_eq!(view.kind(), Kind(0x1234));
    assert_eq!(view.kind_raw(), 0x1234);
    assert_eq!(view.address(), Address([1, 2, 3, 4]));
    assert_eq!(view.address_raw(), [1, 2, 3, 4]);
    assert_eq!(view.flags(), Flags(0b0000_1011));
    assert_eq!(view.flags_raw(), 0b0000_1011);
    assert!(view.enabled());
    assert_eq!(view.mode(), 5);
    assert_eq!(view.value(), Value(0x12_3456));
    assert_eq!(view.value_raw(), 0x12_3456);
    assert_eq!(view.as_bytes(), &input[..10]);
    assert!(Sequential::view(&input).without_trailing().is_err());
    assert_eq!(
        Sequential::view(&input[..10])
            .without_trailing()
            .unwrap()
            .kind(),
        Kind(0x1234)
    );

    let mut owned_address = view.address();
    owned_address.0[0] = 9;
    let mut owned_raw_address = view.address_raw();
    owned_raw_address[1] = 8;
    assert_eq!(view.address(), Address([1, 2, 3, 4]));
    assert_eq!(view.address_raw(), [1, 2, 3, 4]);

    let mut bytes: [u8; 10] = input[..10].try_into().unwrap();
    let mut mutable = SequentialViewMut::parse_exact_mut(&mut bytes).unwrap();
    mutable.set_kind(Kind(1)).unwrap();
    mutable.set_kind_raw(0xabcd).unwrap();
    mutable.set_address(Address([0, 0, 0, 0])).unwrap();
    mutable.set_address_raw([9, 8, 7, 6]).unwrap();
    mutable.set_flags_raw(0).unwrap();
    mutable.set_flags(Flags(0b0000_0010)).unwrap();
    mutable.set_value(Value(1)).unwrap();
    mutable.set_value_raw(0x00_abcd).unwrap();
    assert_eq!(
        mutable.as_bytes(),
        &[0xab, 0xcd, 9, 8, 7, 6, 2, 0, 0xab, 0xcd]
    );
    let before = [0xab, 0xcd, 9, 8, 7, 6, 2, 0, 0xab, 0xcd];
    assert!(matches!(
        mutable.set_value(Value(0x01_000000)),
        Err(SequentialMutationError::FieldValue(error)) if error == U24RangeError::new(0x01_000000)
    ));
    assert_eq!(mutable.as_bytes(), before);
    assert!(matches!(
        mutable.set_value_raw(0x01_000000),
        Err(SequentialMutationError::FieldValue(error)) if error == U24RangeError::new(0x01_000000)
    ));
    assert_eq!(mutable.as_bytes(), before);

    let mut output = [0xa5; 11];
    let (built, suffix) = SequentialBuilder::new()
        .kind_raw(1)
        .kind(Kind(0x1234))
        .address(Address([9, 9, 9, 9]))
        .address_raw([1, 2, 3, 4])
        .flags_raw(0)
        .flags(Flags(0b0000_1011))
        .value_raw(1)
        .value(Value(0x12_3456))
        .build_into(&mut output)
        .unwrap();
    assert_eq!(built.as_bytes(), &input[..10]);
    assert_eq!(suffix, [0xa5]);

    let mut unchanged = [0x5a; 10];
    assert!(matches!(
        SequentialBuilder::new()
            .kind(Kind(1))
            .address(Address([1, 2, 3, 4]))
            .flags(Flags(0))
            .value(Value(0x01_000000))
            .build_into(&mut unchanged),
        Err(SequentialWriteError::FieldValue(error)) if error == U24RangeError::new(0x01_000000)
    ));
    assert_eq!(unchanged, [0x5a; 10]);
    let mut short = [0x3c; 9];
    assert!(matches!(
        SequentialBuilder::new()
            .kind(Kind(1))
            .address(Address([1, 2, 3, 4]))
            .flags(Flags(0))
            .value(Value(1))
            .build_into(&mut short),
        Err(SequentialWriteError::OutputTooShort {
            needed: 10,
            available: 9
        })
    ));
    assert_eq!(short, [0x3c; 9]);
}

#[test]
fn absolute_mappings_use_offsets_for_both_builder_forms_and_mutation() {
    let input = [1, 2, 3, 4, 7, 0x12, 0x34, 0xff];
    let (view, suffix) = Absolute::view(&input).with_remainder().unwrap();
    assert_eq!(suffix, &[0xff]);
    assert_eq!(view.address(), Address([1, 2, 3, 4]));
    assert_eq!(view.address_raw(), [1, 2, 3, 4]);
    assert_eq!(view.code(), Code(7));
    assert_eq!(view.code_raw(), 7);
    assert_eq!(view.kind(), Kind(0x1234));
    assert_eq!(view.kind_raw(), 0x1234);
    assert!(Absolute::view(&input).without_trailing().is_err());

    let mut bytes: [u8; 7] = input[..7].try_into().unwrap();
    let mut mutable = AbsoluteViewMut::parse_exact_mut(&mut bytes).unwrap();
    mutable.set_kind_raw(0xabcd).unwrap();
    mutable.set_address(Address([9, 8, 7, 6])).unwrap();
    mutable.set_code_raw(3).unwrap();
    assert_eq!(mutable.as_bytes(), &[9, 8, 7, 6, 3, 0xab, 0xcd]);

    let mut output = [0xde; 8];
    let (built, suffix) = AbsoluteBuilder::new()
        .kind_raw(1)
        .kind(Kind(0x1234))
        .address_raw([0, 0, 0, 0])
        .address(Address([1, 2, 3, 4]))
        .code(Code(2))
        .code_raw(7)
        .build_into(&mut output)
        .unwrap();
    assert_eq!(built.as_bytes(), &input[..7]);
    assert_eq!(suffix, [0xde]);
}

#[test]
fn dynamic_mappings_derive_range_lengths_without_losing_boundaries() {
    let input = [3, b'a', b'b', b'c', 7, 0xee];
    let (view, suffix) = Dynamic::view(&input).with_remainder().unwrap();
    assert_eq!(suffix, &[0xee]);
    assert_eq!(view.length(), Length(3));
    assert_eq!(view.length_raw(), 3);
    assert_eq!(view.payload(), b"abc");
    assert_eq!(view.code(), Code(7));
    assert_eq!(view.code_raw(), 7);
    assert_eq!(view.as_bytes(), &input[..5]);
    assert!(Dynamic::view(&input).without_trailing().is_err());

    let mut bytes: [u8; 5] = input[..5].try_into().unwrap();
    let mut mutable = DynamicViewMut::parse_exact_mut(&mut bytes).unwrap();
    mutable.set_code(Code(8)).unwrap();
    mutable.set_code_raw(9).unwrap();
    assert_eq!(mutable.code(), Code(9));
    assert_eq!(mutable.as_bytes(), &[3, b'a', b'b', b'c', 9]);

    let mut output = [0xa5; 7];
    let (built, suffix) = DynamicBuilder::new()
        .payload(b"wxyz")
        .code_raw(1)
        .code(Code(7))
        .build_into(&mut output)
        .unwrap();
    assert_eq!(built.length(), Length(4));
    assert_eq!(built.length_raw(), 4);
    assert_eq!(built.payload(), b"wxyz");
    assert_eq!(built.code(), Code(7));
    assert_eq!(built.as_bytes(), &[4, b'w', b'x', b'y', b'z', 7]);
    assert_eq!(suffix, [0xa5]);
}

#[test]
fn absolute_endpoint_mapped_raw_source_preserves_physical_endpoints_and_suffix() {
    let input = [0xaa, 0, 6, 0xa5, b'x', b'y', 9, 0xee];
    let (view, suffix) = AbsoluteEndpoint::view(&input).with_remainder().unwrap();
    assert_eq!(view.end(), Endpoint(6));
    assert_eq!(view.end_raw(), 6);
    assert_eq!(view.payload(), b"xy");
    assert_eq!(view.tail(), 9);
    assert_eq!(view.as_bytes(), &input[..7]);
    assert_eq!(suffix, &[0xee]);
    assert!(matches!(
        AbsoluteEndpoint::view(&input).without_trailing(),
        Err(AbsoluteEndpointError::TrailingBytes {
            expected: 7,
            actual: 8
        })
    ));

    let mut output = [0xa5; 9];
    let (view, suffix) = AbsoluteEndpointBuilder::new()
        .tag(0xaa)
        .payload(b"xy")
        .tail(9)
        .build_into(&mut output)
        .unwrap();
    assert_eq!(view.as_bytes(), &[0xaa, 0, 6, 0xa5, b'x', b'y', 9]);
    assert_eq!(view.end(), Endpoint(6));
    assert_eq!(view.end_raw(), 6);
    assert_eq!(suffix, &[0xa5, 0xa5]);
}

#[test]
fn absolute_endpoint_parse_boundaries_and_signed_conversion_are_precise() {
    let empty = EndpointCases::view(&[0xaa, 2]).without_trailing().unwrap();
    assert!(empty.payload().is_empty());
    assert_eq!(empty.as_bytes(), &[0xaa, 2]);

    assert!(matches!(
        EndpointCases::view(&[0xaa, 1]).with_remainder(),
        Err(EndpointCasesError::RangeEndBeforeStart {
            position: 3,
            source_position: 2,
            end: 1,
            start: 2,
        })
    ));
    assert!(matches!(
        EndpointCases::view(&[0xaa, 6]).with_remainder(),
        Err(EndpointCasesError::InputTooShort {
            position: 3,
            expected: 4,
            available: 0,
        })
    ));
    assert!(matches!(
        SignedEndpoint::view(&[0xff, 0xff]).with_remainder(),
        Err(SignedEndpointError::InvalidRangeSource {
            position: 2,
            source_position: 1,
        })
    ));
}
