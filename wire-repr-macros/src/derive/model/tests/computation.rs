use syn::Type;

use super::super::{
    Codec, ComputationArgument, ComputationByteSelection, FieldKind, FieldPosition, WireType,
};
use super::parse;

#[test]
fn schedules_semantic_computed_position_sources_before_geometry() {
    let WireType::Struct(model) = parse(
            "struct Packet { lead: u8, #[wire(computed = offset(lead))] offset: u8, #[wire(at = offset)] payload: u8 }",
        )
        .unwrap()
        else {
            panic!()
        };
    assert!(matches!(
        model.fields[2].position,
        Some(FieldPosition::Source(ref source)) if source == "offset"
    ));
    assert_eq!(model.preparation.computation_order, vec![1]);
    assert!(
        !model.fields[1]
            .computation
            .as_ref()
            .expect("computed offset")
            .requires_geometry
    );

    let WireType::Struct(model) = parse(
            "struct Packet { #[wire(computed = offset())] offset: u8, #[wire(at = offset)] payload: u8 }",
        )
        .unwrap()
        else {
            panic!()
        };
    assert!(
        !model.fields[0]
            .computation
            .as_ref()
            .expect("computed offset")
            .requires_geometry
    );
}

#[test]
fn rejects_computed_position_sources_that_require_physical_geometry() {
    let accepted = "struct Packet { #[wire(computed = offset(include(marker)))] offset: u8, marker: u8, #[wire(at = offset)] payload: u8 }";
    assert!(parse(accepted).is_ok());

    let source = "struct Packet { #[wire(computed = offset(exclude(marker)))] offset: u8, marker: u8, #[wire(at = offset)] payload: u8 }";
    let Err(error) = parse(source) else {
        panic!("accepted computed geometry cycle: {source}");
    };
    assert!(error.to_string().contains(
        "a computed position source cannot depend on physical geometry that its position controls"
    ));

    let transitive = "struct Packet { #[wire(computed = checksum(exclude(offset)))] checksum: u8, #[wire(computed = offset(include(checksum)))] offset: u8, #[wire(at = offset)] payload: u8 }";
    let Err(error) = parse(transitive) else {
        panic!("accepted transitive computed geometry cycle: {transitive}");
    };
    assert!(error.to_string().contains(
        "a computed position source cannot depend on physical geometry that its position controls"
    ));
}

#[test]
fn rejects_computed_byte_length_sources() {
    let source = "struct Packet<'wire> { #[wire(computed = count())] length: u8, #[wire(bytes = length)] payload: &'wire [u8] }";
    let Err(error) = parse(source) else {
        panic!("accepted conflicting framing source: {source}");
    };
    assert!(error.to_string().contains(
        "a computed field cannot be a byte length source because `bytes` owns framing geometry"
    ));
}

#[test]
fn accepts_computed_semantic_fields() {
    let WireType::Struct(model) = parse("struct Packet<'wire> { #[wire(computed = wire_repr::computation::len(payload))] length: u8, #[wire(rest)] payload: &'wire [u8] }").unwrap() else { panic!() };
    assert!(matches!(
        model.fields[0]
            .computation
            .as_ref()
            .map(|computation| &computation.value_ty),
        Some(Type::Path(path)) if path.path.is_ident("u8")
    ));
    assert!(matches!(
        model.fields[0].kind,
        FieldKind::Fixed(Codec::Builtin("U8"))
    ));
}

#[test]
fn accepts_callback_computations_and_rejects_invalid_byte_selections() {
    let WireType::Struct(model) = parse(
            "struct Packet { #[wire(computed = checksum(include(header, payload.inner)))] checksum: u8, header: u8, payload: Payload }",
        )
        .unwrap()
        else {
            panic!()
        };
    let computation = model.fields[0].computation.as_ref().expect("computation");
    assert!(computation.callback.is_ident("checksum"));
    let ComputationArgument::Bytes(ComputationByteSelection::Include(paths)) =
        &computation.arguments[0]
    else {
        panic!("include selection")
    };
    assert_eq!(paths.len(), 2);
    assert!(paths[0].top_level == "header" && paths[0].nested.is_empty());
    assert!(paths[1].top_level == "payload" && paths[1].nested[0] == "inner");

    let WireType::Struct(model) = parse(
            "struct Packet { head: u8, #[wire(computed = crate::checksum(exclude(self)))] checksum: u8, payload: Payload }",
        )
        .unwrap()
        else {
            panic!()
        };
    assert!(matches!(
        model.fields[1]
            .computation
            .as_ref()
            .map(|computation| &computation.arguments[0]),
        Some(ComputationArgument::Bytes(ComputationByteSelection::Exclude(paths)))
            if paths[0].top_level_index == 1
    ));

    for source in [
        "struct Packet { #[wire(computed = checksum)] checksum: u8, payload: Payload }",
        "struct Packet { #[wire(bytes(include(payload)))] checksum: u8, payload: Payload }",
        "struct Packet { #[wire(computed = wire_repr::computation::len(payload), bytes(include(payload)))] checksum: u8, payload: Payload }",
        "struct Packet { #[wire(computed = checksum(include(missing)))] checksum: u8, payload: Payload }",
        "struct Packet { #[wire(computed = checksum(include(checksum)))] checksum: u8, payload: Payload }",
        "struct Packet { #[wire(computed = checksum(checksum))] checksum: u8, payload: Payload }",
        "struct Packet { #[wire(computed = first(include(second)))] first: u8, #[wire(computed = second(include(first)))] second: u8 }",
        "struct Packet { #[wire(computed = first(exclude(self)))] first: u8, #[wire(computed = second(exclude(self)))] second: u8 }",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
    assert!(
        parse("struct Packet { #[wire(computed = checksum())] checksum: u8, payload: Payload }")
            .is_ok()
    );
    assert!(parse(
        "struct Packet { #[wire(computed = checksum(include()))] checksum: u8, payload: Payload }"
    )
    .is_ok());
    assert!(parse(
        "struct Packet { #[wire(computed = checksum(exclude()))] checksum: u8, payload: Payload }"
    )
    .is_ok());
}
