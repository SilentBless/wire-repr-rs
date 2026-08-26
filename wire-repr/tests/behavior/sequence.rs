#![allow(dead_code)]

use wire_repr::WireView;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(WireView)]
struct Foo {
    #[wire(le)]
    value: u16,
}

#[derive(WireView)]
struct Bar {
    length: u8,
    #[wire(bytes = length)]
    body: wire_repr::wire::Bytes,
}

#[derive(WireView)]
struct Header {
    kind: u8,
}

#[derive(WireView)]
#[wire(as = u8)]
struct Flags {
    #[wire(bit = 0)]
    enabled: bool,
}

#[derive(WireView)]
#[wire(selector = u8)]
enum Choice {
    #[wire(value = 1)]
    First(Header),
    #[wire(value = 2)]
    Second(Header),
}

#[derive(WireView)]
struct Outer<T> {
    child: T,
}

#[derive(WireView)]
struct RestBody {
    #[wire(rest)]
    bytes: wire_repr::wire::Bytes,
}

#[derive(WireView)]
#[wire(selector = u8)]
enum NonLeadingChoice {
    #[wire(value = 1)]
    First(RestBody),
}

struct Manual<const MODE: u8>;

struct ManualRoot<B>(B);

impl<B: AsRef<[u8]>> AsRef<[u8]> for ManualRoot<B> {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl<B: AsRef<[u8]>> wire_repr::__private::WireFields for ManualRoot<B> {
    type Fields = ();
    const FIELD_COUNT: usize = 0;

    fn fields(&self, _base: usize) -> Self::Fields {}

    fn field_range(&self, _index: usize) -> Option<core::ops::Range<usize>> {
        None
    }
}

#[derive(Debug, thiserror::Error)]
#[error("manual sequence validation failed")]
struct ManualError;

#[allow(unsafe_code)]
unsafe impl<const MODE: u8> wire_repr::WireView for Manual<MODE> {
    type Error = ManualError;
    type State = ();
    type View<'view> = &'view [u8];

    const FIXED_SIZE: Option<usize> = None;
    const LEADING_EXTENT: bool = true;

    fn frame(
        input: &[u8],
        _absolute_offset: usize,
    ) -> Result<wire_repr::Frame<Self::State>, Self::Error> {
        let consumed = match MODE {
            0 => input.len() + 1,
            1 => 0,
            _ => 1,
        };
        Ok(wire_repr::Frame::new((), consumed))
    }

    unsafe fn from_validated_parts<'view>(
        input: &'view [u8],
        _state: &'view Self::State,
    ) -> Self::View<'view> {
        input
    }
}

impl<const MODE: u8> wire_repr::__private::WireSelect for Manual<MODE> {
    type Root<B>
        = ManualRoot<B>
    where
        B: AsRef<[u8]>;

    fn select_view<B: AsRef<[u8]>>(input: B) -> Result<Self::Root<B>, Self::Error> {
        Ok(ManualRoot(input))
    }

    fn validated_view<B: AsRef<[u8]>>(input: B) -> Result<Self::Root<B>, Self::Error> {
        let view = ManualRoot(input);
        Self::validate_view(&view)?;
        Ok(view)
    }

    #[allow(unsafe_code)]
    unsafe fn framed_view<B: AsRef<[u8]>>(input: B, _state: Self::State) -> Self::Root<B> {
        ManualRoot(input)
    }

    fn validate_view<B: AsRef<[u8]>>(view: &Self::Root<B>) -> Result<(), Self::Error> {
        let _ = view;
        if MODE == 2 { Err(ManualError) } else { Ok(()) }
    }
}

#[test]
fn fixed_views_prevalidate_and_iterate_exactly() -> TestResult {
    let input = [1, 0, 2, 0, 3, 0];
    let views = Foo::views(&input)?;
    assert_eq!(views.len(), 3);
    let values = views.map(|view| view.value()).collect::<Vec<_>>();
    assert_eq!(values, vec![1, 2, 3]);
    Ok(())
}

#[test]
fn fixed_views_reject_a_truncated_final_item_before_iteration() {
    let error = match Foo::views(&[1, 0, 2]) {
        Ok(_) => panic!("truncated fixed sequence unexpectedly validated"),
        Err(error) => error,
    };
    assert!(matches!(error, wire_repr::SequenceError::Schema(_)));
}

#[test]
fn variable_views_frame_lazily_and_keep_prior_views_alive() -> TestResult {
    let input = [2, 10, 11, 1, 12];
    let mut views = Bar::views(&input)?;
    let first = views.next()?.expect("first item");
    let second = views.next()?.expect("second item");
    assert_eq!(first.body(), &[10, 11]);
    assert_eq!(second.body(), &[12]);
    assert!(views.next()?.is_none());
    Ok(())
}

#[test]
fn fixed_bitfields_share_exact_sequence_capabilities() -> TestResult {
    let flags = Flags::views(&[0, 1, 1])?
        .map(|view| view.enabled())
        .collect::<Vec<_>>();
    assert_eq!(flags, vec![false, true, true]);

    Ok(())
}

#[test]
fn closed_enums_frame_lazily_when_every_body_has_a_leading_extent() -> TestResult {
    let input = [1, 7, 2, 8];
    let mut choices = Choice::views(&input)?;
    let first = choices.next()?.expect("first choice");
    let second = choices.next()?.expect("second choice");
    assert!(matches!(first.variant(), ChoiceVariant::First(_)));
    assert!(matches!(second.variant(), ChoiceVariant::Second(_)));
    assert!(choices.next()?.is_none());
    Ok(())
}

#[test]
fn terminal_nested_children_inherit_their_child_leading_capability() -> TestResult {
    let input = [7, 8];
    let mut views = Outer::<Header>::views(&input)?;
    assert_eq!(views.next()?.expect("first").child().kind(), 7);
    assert_eq!(views.next()?.expect("second").child().kind(), 8);
    assert!(views.next()?.is_none());
    Ok(())
}

#[test]
fn transitive_nonleading_enum_body_returns_unavailable_without_consuming() {
    let error = match NonLeadingChoice::views(&[1, 2, 3]) {
        Ok(_) => panic!("nonleading enum unexpectedly exposed a facade"),
        Err(error) => error,
    };
    assert!(matches!(error, wire_repr::SequenceError::Unavailable));

    let input = [2, 3];
    let mut direct_views = wire_repr::VariableViews::<RestBody>::new(&input);
    assert!(matches!(
        direct_views.next(),
        Err(wire_repr::SequenceError::Unavailable)
    ));
    assert_eq!(direct_views.position(), 0);

    let mut direct_cursor = wire_repr::Cursor::new(&input);
    assert!(matches!(
        direct_cursor.read::<RestBody>(),
        Err(wire_repr::SequenceError::Unavailable)
    ));
    assert_eq!(direct_cursor.position(), 0);
}
#[test]
fn variable_view_failure_does_not_advance_the_facade() {
    let input = [2, 10];
    let mut views = Bar::views(&input).expect("Bar has a leading extent");
    assert!(views.next().is_err());
    assert_eq!(views.position(), 0);
    assert_eq!(views.remaining(), &input);
}

#[test]
fn heterogeneous_cursor_yields_coexisting_views_and_exact_remaining_input() -> TestResult {
    let input = [7, 2, 10, 11, 99];
    let (header, mut cursor) = Header::cursor(&input)?;
    let body = Bar::next(&mut cursor)?;
    assert_eq!(header.kind(), 7);
    assert_eq!(body.body(), &[10, 11]);
    assert_eq!(cursor.remaining(), &[99]);
    Ok(())
}

#[test]
fn variable_sequence_errors_keep_absolute_offsets_after_prior_items() -> TestResult {
    let input = [1, 12, 2, 10];
    let mut views = Bar::views(&input)?;
    assert!(views.next()?.is_some());
    let error = match views.next() {
        Ok(_) => panic!("second item must be truncated"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        wire_repr::SequenceError::Schema(BarViewError::Body(wire_repr::NeedMore {
            offset: 4,
            additional_at_least: 1,
        }))
    ));
    assert_eq!(views.position(), 2);
    Ok(())
}

#[test]
fn cursor_failure_keeps_its_previous_position() -> TestResult {
    let input = [7, 2, 10];
    let (_, mut cursor) = Header::cursor(&input)?;
    let position = cursor.position();
    assert!(Bar::next(&mut cursor).is_err());
    assert_eq!(cursor.position(), position);
    assert_eq!(cursor.remaining(), &[2, 10]);
    Ok(())
}

#[test]
fn cursor_rejects_invalid_extent_nonprogress_and_validator_failure_without_advancing() {
    let input = [1u8];
    let mut overlong = wire_repr::Cursor::new(&input);
    assert!(matches!(
        overlong.read::<Manual<0>>(),
        Err(wire_repr::SequenceError::InvalidFrame(_))
    ));
    assert_eq!(overlong.position(), 0);

    let mut zero = wire_repr::Cursor::new(&input);
    assert!(matches!(
        zero.read::<Manual<1>>(),
        Err(wire_repr::SequenceError::NonProgress { offset: 0 })
    ));
    assert_eq!(zero.position(), 0);

    let mut rejected = wire_repr::Cursor::new(&input);
    assert!(matches!(
        rejected.read::<Manual<2>>(),
        Err(wire_repr::SequenceError::Schema(ManualError))
    ));
    assert_eq!(rejected.position(), 0);
}

#[test]
fn manual_failure_paths_keep_nonzero_cursor_offsets_after_prior_success() -> TestResult {
    let input = [7u8, 1];

    let (_, mut overlong) = Header::cursor(&input)?;
    assert!(matches!(
        overlong.read::<Manual<0>>(),
        Err(wire_repr::SequenceError::InvalidFrame(
            wire_repr::InvalidFrameExtent {
                offset: 1,
                consumed: 2,
                available: 1,
            }
        ))
    ));
    assert_eq!(overlong.position(), 1);

    let (_, mut zero) = Header::cursor(&input)?;
    assert!(matches!(
        zero.read::<Manual<1>>(),
        Err(wire_repr::SequenceError::NonProgress { offset: 1 })
    ));
    assert_eq!(zero.position(), 1);

    let (_, mut rejected) = Header::cursor(&input)?;
    assert!(matches!(
        rejected.read::<Manual<2>>(),
        Err(wire_repr::SequenceError::Schema(ManualError))
    ));
    assert_eq!(rejected.position(), 1);
    Ok(())
}
