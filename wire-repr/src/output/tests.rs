#![allow(dead_code)]

use crate::{WireBuilder, wire};

#[derive(WireBuilder)]
struct Cell {
    value: u8,
}

#[derive(WireBuilder)]
struct Counted {
    #[wire(be)]
    count: i32,
    #[wire(counted_by = count)]
    items: wire::Array<Cell>,
}

#[test]
fn signed_count_patch_checks_the_native_i32_boundary() {
    for prior in [i32::MAX as usize - 1, i32::MAX as usize] {
        let mut output = [0x55; 5];
        let result = Counted::builder(&mut output[..]).items(|mut items| {
            // Seed the counter to exercise the limit without emitting billions of items.
            items.count = prior;
            items.item(|cell| cell.value(0xa6))
        });
        if prior == i32::MAX as usize - 1 {
            let written = result.unwrap().finish().unwrap();
            assert_eq!(written.as_bytes(), [0x7f, 0xff, 0xff, 0xff, 0xa6]);
        } else {
            let error = match result {
                Ok(_) => panic!("unrepresentable signed count unexpectedly wrote"),
                Err(error) => error,
            };
            assert!(matches!(
                error,
                crate::WriteError::Schema(CountedWriteError::Layout(crate::LayoutError {
                    field: "count"
                }))
            ));
            assert_eq!(output, [0, 0, 0, 0, 0xa6]);
        }
    }
}

#[derive(WireBuilder)]
struct AddressCount {
    #[wire(as = i128, be)]
    count: isize,
    #[wire(counted_by = count)]
    items: wire::Array<Cell>,
}

#[test]
fn signed_count_patch_checks_logical_range_before_wide_physical_patch() {
    for prior in [isize::MAX as usize - 1, isize::MAX as usize] {
        let mut output = [0x55; 17];
        let result = AddressCount::builder(&mut output[..]).items(|mut items| {
            // The physical i128 still fits when the logical isize no longer does.
            items.count = prior;
            items.item(|cell| cell.value(0xa6))
        });
        if prior == isize::MAX as usize - 1 {
            let written = result.unwrap().finish().unwrap();
            let mut expected = [0; 17];
            expected[..16].copy_from_slice(&(isize::MAX as i128).to_be_bytes());
            expected[16] = 0xa6;
            assert_eq!(written.as_bytes(), expected);
        } else {
            let error = match result {
                Ok(_) => panic!("unrepresentable logical count unexpectedly wrote"),
                Err(error) => error,
            };
            assert!(matches!(
                error,
                crate::WriteError::Schema(AddressCountWriteError::Layout(crate::LayoutError {
                    field: "count"
                }))
            ));
            assert_eq!(&output[..16], &[0; 16]);
            assert_eq!(output[16], 0xa6);
        }
    }
}
