#![allow(dead_code)]

use wire_repr::WireView;

#[derive(WireView)]
struct Bar {
    #[wire(be)]
    value: u16,
}

#[derive(WireView)]
struct Foo<T> {
    length: u8,
    count: u8,
    #[wire(as = u8)]
    present: bool,
    #[wire(flag = present)]
    details: bool,
    #[wire(depends_on = details)]
    first: u8,
    #[wire(be, depends_on = details)]
    second: u16,
    #[wire(bytes = length)]
    body: wire_repr::wire::Bytes,
    #[wire(counted_by = count)]
    items: wire_repr::wire::Array<T>,
    tail: u8,
}

#[test]
fn arbitrary_structural_inputs_never_escape_geometry_or_iteration_bounds() {
    let mut state = 0x4d59_5df4_d0f3_3173u64;
    for length in 0usize..=64 {
        for _ in 0..128 {
            let mut input = [0u8; 64];
            for byte in &mut input[..length] {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                *byte = state as u8;
            }
            let bytes = &input[..length];
            if let Ok(view) = Foo::<Bar>::view(bytes) {
                assert_eq!(view.as_bytes(), bytes);
                assert_eq!(view.body().len(), usize::from(view.length()));
                let items = view.items();
                assert_eq!(items.len(), usize::from(view.count()));
                let mut iterator = items.iter();
                let mut observed = 0usize;
                while let Some(item) = iterator.next() {
                    observed += 1;
                    assert!(observed <= items.len());
                    if item.is_err() {
                        assert!(iterator.next().is_none());
                        break;
                    }
                }
                assert!(observed <= items.len());
            }
        }
    }
}

#[test]
fn count_bombs_and_truncated_variable_sequences_terminate_without_advancing() {
    let count_bomb = [0, u8::MAX, 0, 9];
    assert!(Foo::<Bar>::view(count_bomb).is_err());

    let input = [3, 0, 0, 1, 2];
    let mut views = Foo::<Bar>::views(&input).expect("schema has a leading extent");
    assert!(views.next().is_err());
    assert_eq!(views.position(), 0);
    assert_eq!(views.remaining(), &input);
}
