#![allow(dead_code)]

use wire_repr::WireView;

#[derive(WireView)]
struct JsonNull {}

#[derive(WireView)]
struct JsonBool {
    value: u8,
}

#[derive(WireView)]
struct JsonBytes {
    #[wire(le)]
    length: u16,
    #[wire(bytes = length)]
    value: wire_repr::wire::Bytes,
}

#[derive(WireView)]
struct JsonConstant {
    #[wire(constant = 0xaa)]
    tag: u8,
}

#[derive(WireView)]
struct JsonArray<T> {
    #[wire(le)]
    count: u16,
    #[wire(counted_by = count)]
    items: wire_repr::wire::Array<T>,
}

#[derive(WireView)]
#[wire(selector = u8)]
enum JsonValue {
    #[wire(value = 0)]
    Null(JsonNull),
    #[wire(value = 1)]
    Bool(JsonBool),
    #[wire(value = 2)]
    Bytes(JsonBytes),
    #[wire(value = 3)]
    Array(JsonArray<JsonValue>),
    #[wire(value = 8)]
    Constant(JsonConstant),
}
#[derive(WireView)]
#[wire(selector = u8)]
enum TwinValue {
    #[wire(value = 4)]
    First(JsonArray<TwinValue>),
    #[wire(value = 5)]
    Second(JsonArray<TwinValue>),
}

#[derive(WireView)]
#[wire(selector = u8)]
enum SelfValue {
    #[wire(value = 6)]
    Array(JsonArray<Self>),
}

mod qualified {
    use wire_repr::WireView;

    #[derive(WireView)]
    pub struct Body<T> {
        pub count: u8,
        #[wire(counted_by = count)]
        pub items: wire_repr::wire::Array<T>,
    }
}

use qualified::Body as AliasedBody;

#[derive(Debug)]
struct ManualError;

impl core::fmt::Display for ManualError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("manual recursive leaf failed")
    }
}

impl core::error::Error for ManualError {}

struct ManualLeaf;

#[allow(unsafe_code)]
unsafe impl wire_repr::WireView for ManualLeaf {
    type Error = ManualError;
    type State = ();
    type View<'view> = &'view [u8];

    const FIXED_SIZE: Option<usize> = Some(1);

    fn frame(
        input: &[u8],
        _absolute_offset: usize,
    ) -> Result<wire_repr::Frame<Self::State>, Self::Error> {
        input.first().ok_or(ManualError)?;
        Ok(wire_repr::Frame::new((), 1))
    }

    unsafe fn from_validated_parts<'view>(
        input: &'view [u8],
        _state: &'view Self::State,
    ) -> Self::View<'view> {
        input
    }
}

#[derive(WireView)]
#[wire(selector = u8)]
enum ManualValue {
    #[wire(value = 10)]
    Manual(ManualLeaf),
    #[wire(value = 11)]
    Array(JsonArray<ManualValue>),
}

#[allow(non_camel_case_types)]
#[derive(WireView)]
struct DepthNameCollision<__WireReprRecursiveDepth> {
    count: u8,
    #[wire(counted_by = count)]
    items: wire_repr::wire::Array<__WireReprRecursiveDepth>,
}

#[derive(WireView)]
#[wire(selector = u8)]
enum DepthNameValue {
    #[wire(value = 12)]
    Array(DepthNameCollision<DepthNameValue>),
}

#[derive(WireView)]
#[wire(selector = u8)]
enum AliasedValue {
    #[wire(value = 7)]
    Array(AliasedBody<AliasedValue>),
}

mod disambiguation {
    use wire_repr::WireView;

    pub mod other {
        use wire_repr::WireView;

        #[derive(WireView)]
        pub struct Root {
            pub value: u8,
        }
    }

    #[derive(WireView)]
    pub struct Body<T> {
        pub count: u8,
        #[wire(counted_by = count)]
        pub items: wire_repr::wire::Array<T>,
    }

    #[derive(WireView)]
    #[wire(selector = u8)]
    pub enum Root {
        #[wire(value = 1)]
        Other(Body<other::Root>),
    }
}
fn push_null(output: &mut Vec<u8>) {
    output.push(0);
}

fn push_bool(output: &mut Vec<u8>, value: bool) {
    output.extend_from_slice(&[1, u8::from(value)]);
}

fn push_bytes(output: &mut Vec<u8>, value: &[u8]) {
    output.push(2);
    output.extend_from_slice(&(value.len() as u16).to_le_bytes());
    output.extend_from_slice(value);
}

fn push_array(output: &mut Vec<u8>, body: impl FnOnce(&mut Vec<u8>) -> u16) {
    output.push(3);
    let count_offset = output.len();
    output.extend_from_slice(&[0; 2]);
    let count = body(output);

    output[count_offset..count_offset + 2].copy_from_slice(&count.to_le_bytes());
}
fn assert_bool_over_scoped_channel<const DEPTH: usize, V>(value: V)
where
    V: JsonValueView<DEPTH> + Send,
{
    let (sender, receiver) = std::sync::mpsc::channel::<V>();
    std::thread::scope(|scope| {
        let worker = scope.spawn(move || {
            let received = receiver.recv().expect("receive");
            assert!(matches!(
                received.variant(),
                JsonValueVariant::Bool(value) if value.value() == 1

            ));
        });
        assert!(sender.send(value).is_ok());
        worker.join().expect("worker");
    });
}
#[test]
fn recursive_slots_support_reuse_self_and_import_aliases() -> Result<(), Box<dyn std::error::Error>>
{
    assert!(matches!(
        TwinValue::view::<8>([4, 0, 0])?.variant(),
        TwinValueVariant::First(_)
    ));
    assert!(matches!(
        TwinValue::view::<8>([5, 0, 0])?.variant(),
        TwinValueVariant::Second(_)
    ));
    assert!(matches!(
        SelfValue::view::<8>([6, 0, 0])?.variant(),
        SelfValueVariant::Array(_)
    ));
    assert!(matches!(
        AliasedValue::view::<8>([7, 0])?.variant(),
        AliasedValueVariant::Array(_)
    ));
    use disambiguation::{BodyView as _, RootView as _, other::RootView as _};
    assert!(matches!(
        ManualValue::view::<8>([10, 42])?.variant(),
        ManualValueVariant::Manual(value) if value == [42]
    ));
    assert!(matches!(
        DepthNameValue::view::<8>([12, 0])?.variant(),
        DepthNameValueVariant::Array(_)
    ));
    let ordinary = disambiguation::Root::view([1, 1, 42])?;
    let disambiguation::RootVariant::Other(body) = ordinary.variant();
    let item = body.items().iter().next().transpose()?.expect("other root");
    assert_eq!(item.view().value(), 42);
    Ok(())
}

#[test]
fn recursive_array_returns_the_ordinary_root_view_family() -> Result<(), Box<dyn std::error::Error>>
{
    let mut input = Vec::new();
    push_array(&mut input, |output| {
        push_null(output);
        push_bool(output, true);
        push_bytes(output, b"foo");
        push_array(output, |output| {
            push_bool(output, false);
            push_null(output);
            2
        });
        4
    });

    let root = JsonValue::view::<64>(input)?;
    let JsonValueVariant::Array(array) = root.variant() else {
        panic!("root array")
    };
    assert_eq!(array.count(), 4);
    let items = array.items();
    assert_eq!(items.len(), 4);

    let third = items.get(2)?.expect("third item");
    match third.variant() {
        JsonValueVariant::Bytes(value) => assert_eq!(value.value(), b"foo"),
        _ => panic!("bytes variant"),
    }

    let nested = items.get(3)?.expect("nested item");
    let JsonValueVariant::Array(nested) = nested.variant() else {
        panic!("nested array")
    };
    let nested_items = nested.items();
    assert!(matches!(
        nested_items.get(0)?.expect("bool").variant(),
        JsonValueVariant::Bool(value) if value.value() == 0
    ));
    assert!(matches!(
        nested_items.get(1)?.expect("null").variant(),
        JsonValueVariant::Null(_)
    ));
    Ok(())
}

#[test]
fn recursive_geometry_selects_periodic_and_replay_modes_exactly()
-> Result<(), Box<dyn std::error::Error>> {
    let mut periodic = Vec::new();
    push_array(&mut periodic, |output| {
        for _ in 0..2 {
            push_bool(output, true);
        }
        for _ in 0..2 {
            push_null(output);
        }
        for _ in 0..2 {
            push_bool(output, false);
        }
        for _ in 0..2 {
            push_null(output);
        }
        8
    });
    let root = JsonValue::view::<16>(periodic)?;
    let JsonValueVariant::Array(array) = root.variant() else {
        panic!("periodic root array")
    };
    assert_eq!(array.items().geometry_kind(), "periodic");

    let mut replay = Vec::new();
    push_array(&mut replay, |output| {
        for length in 0..65u8 {
            let bytes = [length; 64];
            push_bytes(output, &bytes[..usize::from(length)]);
        }
        65
    });
    let root = JsonValue::view::<16>(replay)?;
    let JsonValueVariant::Array(array) = root.variant() else {
        panic!("replay root array")
    };
    assert_eq!(array.items().geometry_kind(), "replay");
    Ok(())
}

#[test]
fn recursive_items_iterate_with_one_forward_cursor() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = Vec::new();
    push_array(&mut input, |output| {
        for index in 0..200u16 {
            push_bool(output, index % 2 == 0);
        }
        200
    });
    let root = JsonValue::view::<64>(input)?;
    let JsonValueVariant::Array(array) = root.variant() else {
        panic!("root array")
    };
    let mut seen = 0usize;
    for item in array.items().iter() {
        assert!(matches!(item?.variant(), JsonValueVariant::Bool(_)));
        seen += 1;
    }
    assert_eq!(seen, 200);
    Ok(())
}

#[test]
fn owned_root_and_recursive_child_cross_a_scoped_channel() -> Result<(), Box<dyn std::error::Error>>
{
    let mut input = Vec::new();
    push_array(&mut input, |output| {
        push_bool(output, true);
        push_null(output);
        2
    });
    let root = JsonValue::view::<64>(input)?;
    let JsonValueVariant::Array(array) = root.variant() else {
        panic!("root array")
    };

    let first = array.items().get(0)?.expect("first item");
    assert_bool_over_scoped_channel::<64, _>(first);
    assert!(array.items().get(1)?.is_some());
    Ok(())
}

#[test]
fn recursive_geometry_errors_keep_the_nested_absolute_site() {
    let unknown = match JsonValue::view::<16>([3, 2, 0, 0, 9]) {
        Ok(_) => panic!("unknown nested selector unexpectedly framed"),
        Err(error) => error,
    };
    assert!(matches!(
        unknown,
        JsonValueViewError::Recursive(wire_repr::__private::RecursiveError::UnknownSelector {
            offset: 4
        })
    ));

    let truncated = match JsonValue::view::<16>([3, 2, 0, 0]) {
        Ok(_) => panic!("truncated recursive array unexpectedly framed"),
        Err(error) => error,
    };
    assert!(matches!(
        truncated,
        JsonValueViewError::Recursive(wire_repr::__private::RecursiveError::NeedMore(
            wire_repr::NeedMore {
                offset: 4,
                additional_at_least: 1,
            }
        ))
    ));

    let body_truncated = match JsonValue::view::<16>([3, 1, 0, 1]) {
        Ok(_) => panic!("truncated recursive leaf unexpectedly framed"),
        Err(error) => error,
    };
    assert!(matches!(
        body_truncated,
        JsonValueViewError::Recursive(wire_repr::__private::RecursiveError::NeedMore(
            wire_repr::NeedMore {
                offset: 4,
                additional_at_least: 1,
            }
        ))
    ));

    let bad_constant = match JsonValue::view::<16>([3, 1, 0, 8, 0]) {
        Ok(_) => panic!("bad recursive constant unexpectedly framed"),
        Err(error) => error,
    };
    assert!(matches!(
        bad_constant,
        JsonValueViewError::Recursive(wire_repr::__private::RecursiveError::Child { offset: 4 })
    ));
}

#[test]
fn caller_selected_depth_scales_beyond_sixty_four_and_fails_closed() {
    assert!(matches!(
        JsonValue::view::<0>([0]),
        Err(JsonValueViewError::DepthExceeded(
            wire_repr::DepthExceeded {
                limit: 0,
                offset: 0
            }
        ))
    ));
    let mut input = Vec::new();
    for _ in 0..127 {
        input.push(3);
        input.extend_from_slice(&1u16.to_le_bytes());
    }
    push_null(&mut input);

    assert!(JsonValue::view::<128>(&input).is_ok());
    let error = match JsonValue::view::<127>(input) {
        Ok(_) => panic!("depth-128 representation unexpectedly fit depth 127"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        JsonValueViewError::DepthExceeded(wire_repr::DepthExceeded { limit: 127, .. })
    ));
}
