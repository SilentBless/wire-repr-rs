use wire_repr::wire_repr;

wire_repr! {
    pub layout ImplicitFixed {
        field tag: U8;
        padding { length: 2; }
        align { boundary: 4; }
        field flags: U8 { projections {
            bit enabled: 0;
            bits mode: 1..=3;
        } }
    }

    pub layout ImplicitDynamic {
        field length: U8;
        field payload: bytes(current_pos..current_pos + length);
        field tail: BeU16;
    }
}

#[test]
fn implicit_fixed_layout_normalizes_spacing_and_write_paths() {
    assert_eq!(ImplicitFixedView::WIDTH, 5);

    let input = [7, 0xaa, 0xbb, 0xcc, 0x0b, 0x99];
    let (view, suffix) = ImplicitFixedView::parse_prefix(&input).expect("valid prefix");
    assert_eq!(view.as_bytes(), &input[..5]);
    assert_eq!(suffix, &[0x99]);
    assert_eq!(view.tag(), 7);
    assert_eq!(view.flags(), 0x0b);
    assert!(view.enabled());
    assert_eq!(view.mode(), 5);

    let mut output = [0xde, 0xaa, 0xbb, 0xcc, 0xad, 0x99];
    let (mut built, suffix) = ImplicitFixedBuilder::new()
        .tag(7)
        .flags(0x0b)
        .build_into(&mut output)
        .expect("complete builder");
    assert_eq!(built.as_bytes(), &[7, 0xaa, 0xbb, 0xcc, 0x0b]);
    built.set_flags(0x04).expect("fixed setter");
    assert_eq!(built.as_bytes(), &[7, 0xaa, 0xbb, 0xcc, 0x04]);
    assert_eq!(suffix, &[0x99]);
    assert_eq!(output, [7, 0xaa, 0xbb, 0xcc, 0x04, 0x99]);
}

#[test]
fn implicit_dynamic_layout_frames_ranges_in_declaration_order() {
    let input = [2, 0xca, 0xfe, 0x12, 0x34, 0x99];
    let (view, suffix) = ImplicitDynamicView::parse_prefix(&input).expect("valid prefix");
    assert_eq!(view.as_bytes(), &input[..5]);
    assert_eq!(suffix, &[0x99]);
    assert_eq!(view.length(), 2);
    assert_eq!(view.payload(), &[0xca, 0xfe]);
    assert_eq!(view.tail(), 0x1234);

    let mut output = [0; 6];
    let (built, suffix) = ImplicitDynamicBuilder::new()
        .payload(&[0xca, 0xfe])
        .tail(0x1234)
        .build_into(&mut output)
        .expect("derived length and trailing field");
    assert_eq!(built.as_bytes(), &input[..5]);
    assert_eq!(built.length(), 2);
    assert_eq!(built.payload(), &[0xca, 0xfe]);
    assert_eq!(built.tail(), 0x1234);
    assert_eq!(suffix, &[0]);
}
