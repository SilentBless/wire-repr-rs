//! Recursive selector enum using both object continuations and a counted runtime array.

use wire_repr::{WireBuilder, WireView, wire};

#[allow(dead_code)]
#[derive(WireView, WireBuilder)]
struct Leaf {
    value: u8,
}

#[allow(dead_code)]
#[derive(WireView, WireBuilder)]
struct Pair<T> {
    left: wire::Recursive<T>,
    opcode: u8,
    right: wire::Recursive<T>,
}

#[allow(dead_code)]
#[derive(WireView, WireBuilder)]
struct Values<T> {
    count: u8,
    #[wire(counted_by = count)]
    items: wire::Array<T>,
}

#[allow(dead_code)]
#[derive(WireView, WireBuilder)]
#[wire(selector = u8)]
enum Value {
    #[wire(value = 0)]
    Null,
    #[wire(value = 1)]
    Leaf(Leaf),
    #[wire(value = 2)]
    Pair(Pair<Value>),
    #[wire(value = 3)]
    Values(Values<Value>),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut output = Vec::new();
    let written = Value::builder(&mut output)
        .pair(|pair| {
            let pair = pair.left(|value| value.leaf(|leaf| leaf.value(10)))?;
            let pair = pair.opcode(7)?;
            pair.right(|value| {
                value.values(|values| {
                    values.items(|mut items| {
                        items = items.item(|value| Ok(value.null()?))?;
                        items = items.item(|value| value.leaf(|leaf| leaf.value(20)))?;
                        Ok(items)
                    })
                })
            })
        })?
        .finish()?;

    let root = Value::view::<32>(written.as_bytes())?;
    let ValueVariant::Pair(pair) = root.variant() else {
        unreachable!("the writer emitted a pair")
    };

    assert_eq!(pair.opcode(), 7);
    assert!(matches!(
        pair.left()?.variant(),
        ValueVariant::Leaf(leaf) if leaf.value() == 10
    ));

    let right = pair.right()?;
    let ValueVariant::Values(values) = right.variant() else {
        unreachable!("the right child is a list")
    };
    assert_eq!(values.items().len(), 2);
    let Some(second) = values.items().get(1)? else {
        unreachable!("the count was validated as two")
    };
    assert!(matches!(
        second.variant(),
        ValueVariant::Leaf(leaf) if leaf.value() == 20
    ));

    Ok(())
}
