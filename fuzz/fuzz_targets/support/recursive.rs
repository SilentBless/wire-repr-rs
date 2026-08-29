#![allow(dead_code)]

use wire_repr::WireView;

#[derive(WireView)]
pub struct RecursiveLeaf {
    value: u8,
}

#[derive(WireView)]
pub struct RecursiveArray<T> {
    count: u8,
    #[wire(counted_by = count)]
    items: wire_repr::wire::Array<T>,
}

#[derive(WireView)]
#[wire(selector = u8)]
pub enum RecursiveValue {
    #[wire(value = 1)]
    Leaf(RecursiveLeaf),
    #[wire(value = 2)]
    Array(RecursiveArray<RecursiveValue>),
}

pub fn inspect_recursive(input: &[u8]) {
    let _ = RecursiveValue::view::<0>(input);
    let _ = RecursiveValue::view::<1>(input);
    let Ok(root) = RecursiveValue::view::<32>(input) else {
        return;
    };
    assert_eq!(root.as_ref(), input);
    let RecursiveValueVariant::Array(array) = root.variant() else {
        return;
    };
    let items = array.items();
    assert_eq!(items.len(), usize::from(array.count()));
    let mut iterator = items.iter();
    let base = input.as_ptr() as usize;
    let mut cursor = 2usize;
    for index in 0..items.len() {
        let sequential = iterator
            .next()
            .expect("declared recursive item")
            .expect("recursive iterator item");
        let random = items
            .get(index)
            .expect("recursive random lookup")
            .expect("declared recursive random item");
        let span = sequential.as_ref();
        assert_eq!(span, random.as_ref());
        assert_eq!(span.as_ptr() as usize - base, cursor);
        cursor += span.len();
    }
    assert!(iterator.next().is_none());
    assert!(items.get(items.len()).expect("past-end lookup").is_none());
    assert_eq!(cursor, input.len());
}
