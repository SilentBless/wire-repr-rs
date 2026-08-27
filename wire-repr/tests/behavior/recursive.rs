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
struct Pair<T> {
    left: wire_repr::wire::Recursive<T>,
    opcode: u8,
    right: wire_repr::wire::Recursive<T>,
}

#[derive(WireView)]
#[wire(selector = u8)]
enum PairValue {
    #[wire(value = 1)]
    Leaf(JsonBool),
    #[wire(value = 2)]
    Pair(Pair<PairValue>),
}

#[derive(WireView)]
struct DecoratedPair<T> {
    prefix: [u8; 2],
    left: wire_repr::wire::Recursive<T>,
    #[wire(le)]
    opcode: u16,
    right: wire_repr::wire::Recursive<T>,
    suffix: [u8; 2],
}

#[derive(WireView)]
#[wire(selector = u8)]
enum DecoratedValue {
    #[wire(value = 1)]
    Leaf(JsonBool),
    #[wire(value = 2)]
    Pair(DecoratedPair<DecoratedValue>),
}

#[derive(WireView)]
#[wire(selector = u8)]
enum MixedValue {
    #[wire(value = 1)]
    Leaf(JsonBool),
    #[wire(value = 2)]
    Pair(Pair<MixedValue>),
    #[wire(value = 3)]
    Decorated(DecoratedPair<MixedValue>),
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

#[allow(non_camel_case_types)]
#[derive(WireView)]
struct ObjectNameCollision<__WireReprRecursiveCallback> {
    left: wire_repr::wire::Recursive<__WireReprRecursiveCallback>,
    opcode: u8,
    right: wire_repr::wire::Recursive<__WireReprRecursiveCallback>,
}

#[derive(WireView)]
#[wire(selector = u8)]
enum ObjectNameValue {
    #[wire(value = 1)]
    Leaf(JsonBool),
    #[wire(value = 2)]
    Pair(ObjectNameCollision<ObjectNameValue>),
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

fn push_width(output: &mut Vec<u8>, width: usize) {
    match width {
        1 => push_null(output),
        2 => push_bool(output, false),
        3..=258 => {
            let bytes = [0u8; 255];
            push_bytes(output, &bytes[..width - 3]);
        }
        _ => panic!("test width outside JSON fixture range"),
    }
}

fn mixed_class(index: usize, modulo: usize) -> usize {
    let mut value = index as u64 ^ 0x6a09_e667_f3bc_c909;
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    ((value ^ (value >> 31)) % modulo as u64) as usize
}

fn left_pair_chain(depth: usize) -> Vec<u8> {
    let mut input = Vec::with_capacity(depth * 4 + 2);
    input.extend(core::iter::repeat_n(2, depth));
    input.extend_from_slice(&[1, 7]);
    for index in 0..depth {
        input.extend_from_slice(&[index as u8, 1, (index as u8).wrapping_add(1)]);
    }
    input
}

fn encoded_widths(widths: &[usize]) -> Vec<u8> {
    let mut input = Vec::new();
    push_array(&mut input, |output| {
        for width in widths {
            push_width(output, *width);
        }
        u16::try_from(widths.len()).expect("fixture count")
    });
    input
}

fn assert_geometry(
    input: Vec<u8>,
    expected_kind: &str,
    widths: &[usize],
) -> Result<(), Box<dyn std::error::Error>> {
    let root = JsonValue::view::<128>(input)?;
    let JsonValueVariant::Array(array) = root.variant() else {
        panic!("geometry fixture root array")
    };
    let items = array.items();
    assert_eq!(items.geometry_kind(), expected_kind);
    assert_eq!(items.len(), widths.len());
    let base = array.as_ref()[2..].as_ptr() as usize;
    let mut expected_start = 0usize;
    for (index, expected_width) in widths.iter().copied().enumerate() {
        let item = items.get(index)?.expect("represented item");
        assert_eq!(item.as_ref().len(), expected_width, "item {index} width");
        let start = item.as_ref().as_ptr() as usize;
        assert_eq!(
            start.checked_sub(base),
            Some(expected_start),
            "item {index} start",
        );
        expected_start += expected_width;
    }
    assert!(items.get(widths.len())?.is_none());
    Ok(())
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

fn assert_pair_leaf_over_scoped_channel<const DEPTH: usize, V>(value: V, expected: u8)
where
    V: PairValueView<DEPTH> + Send,
{
    let (sender, receiver) = std::sync::mpsc::channel::<V>();
    std::thread::scope(|scope| {
        let worker = scope.spawn(move || {
            let received = receiver.recv().expect("recursive object child");
            assert!(matches!(
                received.variant(),
                PairValueVariant::Leaf(value) if value.value() == expected
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
fn recursive_pair_continuations_resume_between_children() -> Result<(), Box<dyn std::error::Error>>
{
    let input = [2, 1, 10, 0xaa, 2, 1, 20, 0xbb, 1, 30];
    let root = PairValue::view::<16>(input)?;
    let PairValueVariant::Pair(pair) = root.variant() else {
        panic!("pair root")
    };
    assert_eq!(pair.opcode(), 0xaa);
    assert!(matches!(
        pair.left()?.variant(),
        PairValueVariant::Leaf(value) if value.value() == 10
    ));
    let right = pair.right()?;
    let PairValueVariant::Pair(right) = right.variant() else {
        panic!("nested pair")
    };
    assert_eq!(right.opcode(), 0xbb);
    assert!(matches!(
        right.left()?.variant(),
        PairValueVariant::Leaf(value) if value.value() == 20
    ));
    assert!(matches!(
        right.right()?.variant(),
        PairValueVariant::Leaf(value) if value.value() == 30
    ));
    Ok(())
}

#[test]
fn recursive_object_continuations_cross_fixed_segments() -> Result<(), Box<dyn std::error::Error>> {
    let input = [2, 0xa0, 0xa1, 1, 10, 0x34, 0x12, 1, 20, 0xb0, 0xb1];
    let root = DecoratedValue::view::<16>(input)?;
    let DecoratedValueVariant::Pair(pair) = root.variant() else {
        panic!("decorated pair")
    };
    assert_eq!(pair.prefix(), [0xa0, 0xa1]);
    assert_eq!(pair.opcode(), 0x1234);
    assert_eq!(pair.suffix(), [0xb0, 0xb1]);
    assert!(matches!(
        pair.left()?.variant(),
        DecoratedValueVariant::Leaf(value) if value.value() == 10
    ));
    assert!(matches!(
        pair.right()?.variant(),
        DecoratedValueVariant::Leaf(value) if value.value() == 20
    ));
    Ok(())
}

#[test]
fn recursive_body_kind_stack_resumes_distinct_object_grammars()
-> Result<(), Box<dyn std::error::Error>> {
    let input = [
        2, 3, 0xa0, 0xa1, 1, 10, 0x34, 0x12, 1, 20, 0xb0, 0xb1, 0xcc, 1, 30,
    ];
    let root = MixedValue::view::<16>(input)?;
    let MixedValueVariant::Pair(pair) = root.variant() else {
        panic!("mixed root pair")
    };
    assert_eq!(pair.opcode(), 0xcc);
    let left = pair.left()?;
    let MixedValueVariant::Decorated(left) = left.variant() else {
        panic!("mixed decorated child")
    };
    assert_eq!(left.prefix(), [0xa0, 0xa1]);
    assert_eq!(left.opcode(), 0x1234);
    assert_eq!(left.suffix(), [0xb0, 0xb1]);
    assert!(matches!(
        pair.right()?.variant(),
        MixedValueVariant::Leaf(value) if value.value() == 30
    ));
    Ok(())
}

#[test]
fn recursive_object_child_crosses_scoped_channel_while_parent_continues()
-> Result<(), Box<dyn std::error::Error>> {
    let root = PairValue::view::<16>(vec![2, 1, 10, 0xaa, 1, 20])?;
    let PairValueVariant::Pair(pair) = root.variant() else {
        panic!("pair root")
    };
    let left = pair.left()?;
    assert_pair_leaf_over_scoped_channel::<16, _>(left, 10);

    assert!(matches!(
        pair.right()?.variant(),
        PairValueVariant::Leaf(value) if value.value() == 20
    ));
    Ok(())
}
#[test]
fn recursive_object_internal_generic_names_are_hygienic() -> Result<(), Box<dyn std::error::Error>>
{
    let root = ObjectNameValue::view::<8>([2, 1, 10, 0xaa, 1, 20])?;
    let ObjectNameValueVariant::Pair(pair) = root.variant() else {
        panic!("hygienic pair")
    };
    assert_eq!(pair.opcode(), 0xaa);
    Ok(())
}
#[test]
fn recursive_pair_descent_uses_the_caller_selected_iterative_depth()
-> Result<(), Box<dyn std::error::Error>> {
    let input = left_pair_chain(256);
    let value = PairValue::view::<257>(&input)?;
    let PairValueVariant::Pair(pair) = value.variant() else {
        panic!("pair chain")
    };
    assert_eq!(pair.opcode(), 255);
    assert!(matches!(
        pair.right()?.variant(),
        PairValueVariant::Leaf(leaf) if leaf.value() == 0
    ));

    let error = match PairValue::view::<256>(&input) {
        Ok(_) => panic!("depth-257 pair unexpectedly fit depth 256"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        PairValueViewError::DepthExceeded(wire_repr::DepthExceeded { limit: 256, .. })
    ));
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
fn recursive_geometry_selects_formula_and_segment_modes_exactly()
-> Result<(), Box<dyn std::error::Error>> {
    let fixed = vec![1; 64];
    assert_geometry(encoded_widths(&fixed), "fixed", &fixed)?;

    let formula = (0..100).map(|index| 3 + index).collect::<Vec<_>>();
    assert_geometry(encoded_widths(&formula), "exact_formula", &formula)?;

    let mut intervals = Vec::new();
    for run in 0..12 {
        intervals.extend(core::iter::repeat_n(5 + run * 3, run + 1));
    }
    assert_geometry(encoded_widths(&intervals), "interval_events", &intervals)?;

    let factorized = (0..9_000usize)
        .map(|index| {
            let variant = 2 + ((index % 16) * 7) % 13;
            let depth_class = (index / 16) % 64;
            let depth = (depth_class * 5 + depth_class / 7) % 17;
            let flags = ((index / 1_024) % 8).count_ones() as usize * 3;
            let controller = (index / 8_192) % 32;
            let controller = (controller * controller + 3 * controller) % 29;
            8 + variant + depth + flags + controller
        })
        .collect::<Vec<_>>();
    assert_geometry(encoded_widths(&factorized), "factorized", &factorized)?;

    let short_factorized = (0..128usize)
        .map(|index| {
            let low = ((index % 16) * 7) % 16;
            let high = ((index / 16) % 8) * 16;
            3 + low + high
        })
        .collect::<Vec<_>>();
    assert_geometry(
        encoded_widths(&short_factorized),
        "factorized",
        &short_factorized,
    )?;
    Ok(())
}

#[test]
fn recursive_geometry_selects_palette_shape_and_replay_modes_exactly()
-> Result<(), Box<dyn std::error::Error>> {
    assert!(core::mem::size_of::<wire_repr::__private::RecursiveGeometry>() <= 384);
    assert!(core::mem::size_of::<wire_repr::__private::RecursiveGeometryBuilder>() <= 384);

    let periodic = [2, 2, 1, 1].repeat(8);
    assert_geometry(encoded_widths(&periodic), "periodic_palette", &periodic)?;

    let ranked = (0..200)
        .map(|index| 3 + mixed_class(index, 50))
        .collect::<Vec<_>>();
    assert_geometry(encoded_widths(&ranked), "ranked_palette", &ranked)?;

    let mut packed = Vec::with_capacity(512);
    let mut previous = usize::MAX;
    for run in 0..256 {
        let mut class = mixed_class(run, 50);
        if class == previous {
            class = (class + 1) % 50;
        }
        previous = class;
        let width = 3 + class;
        packed.extend([width, width]);
    }
    assert_geometry(encoded_widths(&packed), "packed_runs", &packed)?;

    let mut recursive_shape = Vec::new();
    let mut shape_widths = Vec::new();
    push_array(&mut recursive_shape, |output| {
        for _ in 0..150 {
            push_array(output, |_| 0);
            shape_widths.push(3);
            push_array(output, |output| {
                push_null(output);
                1
            });
            shape_widths.push(4);
        }
        300
    });
    assert_geometry(recursive_shape, "recursive_shape", &shape_widths)?;

    let replay = (0..1_200)
        .map(|index| 3 + mixed_class(index, 200))
        .collect::<Vec<_>>();
    assert_geometry(encoded_widths(&replay), "replay", &replay)?;
    Ok(())
}

#[test]
fn recursive_geometry_capacity_boundaries_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
    let mut widths = Vec::with_capacity(257);
    let mut previous = usize::MAX;
    for index in 0..257 {
        let mut class = mixed_class(index, 50);
        if class == previous {
            class = (class + 1) % 50;
        }
        previous = class;
        widths.push(3 + class);
    }
    assert_geometry(
        encoded_widths(&widths[..256]),
        "ranked_palette",
        &widths[..256],
    )?;
    assert_geometry(encoded_widths(&widths), "replay", &widths)?;
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
fn recursive_pair_continuation_errors_keep_absolute_offsets() {
    let missing_opcode = PairValue::view::<16>([2, 1, 10])
        .err()
        .expect("missing pair opcode");
    assert!(matches!(
        missing_opcode,
        PairValueViewError::Recursive(wire_repr::__private::RecursiveError::NeedMore(
            wire_repr::NeedMore {
                offset: 3,
                additional_at_least: 1,
            }
        ))
    ));

    let bad_right = PairValue::view::<16>([2, 1, 10, 0xaa, 9])
        .err()
        .expect("unknown right selector");
    assert!(matches!(
        bad_right,
        PairValueViewError::Recursive(wire_repr::__private::RecursiveError::UnknownSelector {
            offset: 4
        })
    ));
}

#[test]
fn generated_recursive_skip_checks_const_stack_capacity() {
    type Slot = <JsonArray<JsonValue> as wire_repr::__private::RecursiveSlot<0>>::Marker;
    let error = <__WireReprJsonValueRecursiveCallback as wire_repr::__private::RecursiveFrame<
        Slot,
    >>::skip::<0>(
        &[3, 1, 0, 0],
        0,
        wire_repr::__private::RecursiveDepth::new(1),
    )
    .expect_err("zero-capacity generated stack");
    assert!(matches!(
        error,
        JsonValueViewError::DepthExceeded(wire_repr::DepthExceeded {
            limit: 1,
            offset: 0,
        })
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
