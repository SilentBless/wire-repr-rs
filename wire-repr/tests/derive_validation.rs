#![deny(missing_docs, unsafe_code)]
//! Public validated-view derive coverage.

use wire_repr::Wire;

/// Validation failure returned by [`Packet::view`].
#[derive(Debug)]
pub enum PacketError {
    /// Structural framing failed.
    Decode(PacketDecodeError),
    /// The first field validator rejected the value.
    First(u8),
    /// The second field validator rejected the value.
    Second(u8),
    /// The model validator rejected the decoded view.
    Model,
}

impl core::fmt::Display for PacketError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl core::error::Error for PacketError {}

impl From<PacketDecodeError> for PacketError {
    fn from(error: PacketDecodeError) -> Self {
        Self::Decode(error)
    }
}

fn first(value: u8) -> Result<(), PacketError> {
    if value == 1 {
        Err(PacketError::First(value))
    } else {
        Ok(())
    }
}
fn second(value: u8) -> Result<(), PacketError> {
    if value == 2 {
        Err(PacketError::Second(value))
    } else {
        Ok(())
    }
}
fn model(view: &PacketView<'_>) -> Result<(), PacketError> {
    if view.kind() == view.code() {
        Err(PacketError::Model)
    } else {
        Ok(())
    }
}

/// A fixed-width packet with field and model validators.
#[derive(Wire)]
#[wire(error = PacketError, validate = model)]
pub struct Packet {
    /// Kind.
    #[wire(validate = first, validate = second)]
    pub kind: u8,
    /// Code.
    pub code: u8,
}

/// Validation failure for a dynamic frame.
#[derive(Debug)]
pub enum FrameError {
    /// Structural framing failed.
    Decode(FrameDecodeError),
    /// Field validation failed.
    Invalid,
}
impl core::fmt::Display for FrameError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl core::error::Error for FrameError {}
impl From<FrameDecodeError> for FrameError {
    fn from(error: FrameDecodeError) -> Self {
        Self::Decode(error)
    }
}
fn nonempty(value: &[u8]) -> Result<(), FrameError> {
    if value.is_empty() {
        Err(FrameError::Invalid)
    } else {
        Ok(())
    }
}

/// A dynamic prefix frame.
#[derive(Wire)]
#[wire(error = FrameError)]
pub struct Frame<'wire> {
    /// Byte count.
    pub length: u8,
    /// Data.
    #[wire(bytes = length, validate = nonempty)]
    pub payload: &'wire [u8],
}

#[test]
fn validated_fixed_views_run_field_then_model_validators_and_map_structural_errors() {
    assert!(matches!(
        Packet::view(&[1, 1]).without_trailing(),
        Err(PacketError::First(1))
    ));
    assert!(matches!(
        Packet::view(&[2, 9]).without_trailing(),
        Err(PacketError::Second(2))
    ));
    assert!(matches!(
        Packet::view(&[3, 3]).without_trailing(),
        Err(PacketError::Model)
    ));
    assert!(matches!(
        Packet::view(&[3]).without_trailing(),
        Err(PacketError::Decode(PacketDecodeError::InputTooShort {
            field: "code",
            required: 1,
            available: 0
        }))
    ));
    assert!(matches!(
        Packet::view(&[3, 4, 5]).without_trailing(),
        Err(PacketError::Decode(PacketDecodeError::TrailingBytes {
            expected: 2,
            actual: 3
        }))
    ));
    assert!(matches!(
        Packet::view(&[1, 1, 5]).without_trailing(),
        Err(PacketError::First(1))
    ));

    let (view, suffix) = Packet::view(&[3, 4, 5]).with_remainder().unwrap();
    assert_eq!(view.code(), 4);
    assert_eq!(suffix, &[5]);
}

#[test]
fn unchecked_views_retain_structural_errors_and_skip_validation() {
    let view = Packet::view(&[1, 1])
        .unchecked()
        .without_trailing()
        .unwrap();
    assert_eq!(view.kind(), 1);
    assert!(matches!(
        Packet::view(&[1]).unchecked().without_trailing(),
        Err(PacketDecodeError::InputTooShort { .. })
    ));
}

#[test]
fn dynamic_views_validate_after_framing() {
    assert!(matches!(
        Frame::view(&[0]).without_trailing(),
        Err(FrameError::Invalid)
    ));
    assert!(matches!(
        Frame::view(&[0, 9]).without_trailing(),
        Err(FrameError::Invalid)
    ));
    let (view, suffix) = Frame::view(&[2, 7, 8, 9]).with_remainder().unwrap();
    assert_eq!(view.payload(), &[7, 8]);
    assert_eq!(suffix, &[9]);
}

/// Child semantic failure.
#[derive(Debug)]
pub enum ChildError {
    /// The child value is invalid.
    Invalid(u8),
}
impl core::fmt::Display for ChildError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}
impl core::error::Error for ChildError {}
fn child_nonzero(value: u8) -> Result<(), ChildError> {
    if value == 0 {
        Err(ChildError::Invalid(value))
    } else {
        Ok(())
    }
}

/// Child with a custom semantic error.
#[derive(Wire)]
#[wire(error = ChildError)]
pub struct ValidatedChild {
    /// Child value.
    #[wire(validate = child_nonzero)]
    pub value: u8,
}

/// Parent with two children and no validators of its own.
#[derive(Wire)]
pub struct ParentWithoutError {
    /// First nested child.
    pub first: ValidatedChild,
    /// Second nested child.
    pub second: ValidatedChild,
}

/// A second child semantic failure type.
#[derive(Debug)]
pub enum SecondChildError {
    /// The second child value is invalid.
    Invalid(u8),
}

impl core::fmt::Display for SecondChildError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl core::error::Error for SecondChildError {}

fn second_child_nonzero(value: u8) -> Result<(), SecondChildError> {
    if value == 0 {
        Err(SecondChildError::Invalid(value))
    } else {
        Ok(())
    }
}

/// A second child with an independent semantic error type.
#[derive(Wire)]
#[wire(error = SecondChildError)]
pub struct SecondValidatedChild {
    /// Child value.
    #[wire(validate = second_child_nonzero)]
    pub value: u8,
}

/// Human-owned parent validation error.
#[derive(Debug)]
pub enum ParentError {
    /// Parent structural framing failed.
    Decode,
    /// The first child failed semantic validation.
    First(ChildError),
    /// The second child failed semantic validation.
    Second(SecondChildError),
}

impl core::fmt::Display for ParentError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl core::error::Error for ParentError {}

impl From<ParentWithErrorDecodeError<'_>> for ParentError {
    fn from(_: ParentWithErrorDecodeError<'_>) -> Self {
        Self::Decode
    }
}

impl From<ChildError> for ParentError {
    fn from(error: ChildError) -> Self {
        Self::First(error)
    }
}

impl From<SecondChildError> for ParentError {
    fn from(error: SecondChildError) -> Self {
        Self::Second(error)
    }
}

/// A parent composing two independent child error domains.
#[derive(Wire)]
#[wire(error = ParentError)]
pub struct ParentWithError {
    /// First nested child.
    pub first: ValidatedChild,
    /// Second nested child.
    pub second: SecondValidatedChild,
}

#[test]
fn nested_validation_uses_a_generated_wrapper_or_the_exact_parent_error() {
    assert!(matches!(
        ParentWithoutError::view(&[0, 1]).without_trailing(),
        Err(ParentWithoutErrorValidationError::NestedFirst(
            ChildError::Invalid(0)
        ))
    ));
    assert!(matches!(
        ParentWithoutError::view(&[1, 0]).without_trailing(),
        Err(ParentWithoutErrorValidationError::NestedSecond(
            ChildError::Invalid(0)
        ))
    ));

    assert!(matches!(
        ParentWithError::view(&[0, 1]).without_trailing(),
        Err(ParentError::First(ChildError::Invalid(0)))
    ));
    assert!(matches!(
        ParentWithError::view(&[1, 0]).without_trailing(),
        Err(ParentError::Second(SecondChildError::Invalid(0)))
    ));
    assert!(matches!(
        ParentWithError::view(&[1]).without_trailing(),
        Err(ParentError::Decode)
    ));
}

#[test]
fn validated_cursor_is_fail_closed_and_unchecked_is_structural_only() {
    let input = [1, 1, 3, 4];
    let mut cursor = Packet::cursor(&input);
    assert!(matches!(
        cursor.next(),
        Err(wire_repr::ViewCursorError::Item(PacketError::First(1)))
    ));
    assert_eq!(cursor.remaining(), &input);
    let mut unchecked = cursor.unchecked();
    assert_eq!(unchecked.next().unwrap().unwrap().code(), 1);
    assert_eq!(unchecked.remaining(), &[3, 4]);

    let dynamic = [0, 1, 7];
    let mut cursor = Frame::cursor(&dynamic);
    assert!(matches!(
        cursor.next(),
        Err(wire_repr::ViewCursorError::Item(FrameError::Invalid))
    ));
    assert_eq!(cursor.remaining(), &dynamic);
}

/// Validation failure for a stored computed field.
#[derive(Debug)]
pub enum ComputedValidationError {
    /// Structural framing failed.
    Decode(ComputedValidatedDecodeError),
    /// The stored computed value was zero.
    Zero,
}

impl core::fmt::Display for ComputedValidationError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl core::error::Error for ComputedValidationError {}

impl From<ComputedValidatedDecodeError> for ComputedValidationError {
    fn from(error: ComputedValidatedDecodeError) -> Self {
        Self::Decode(error)
    }
}

fn computed_nonzero(value: u8) -> Result<(), ComputedValidationError> {
    if value == 0 {
        Err(ComputedValidationError::Zero)
    } else {
        Ok(())
    }
}

/// A frame validating its stored computed length as an ordinary semantic value.
#[derive(Wire)]
#[wire(error = ComputedValidationError)]
pub struct ComputedValidated<'wire> {
    /// Stored encoded payload length.
    #[wire(computed = wire_repr::computation::len(payload), validate = computed_nonzero)]
    pub length: u8,
    /// Payload occupying the rest of the representation.
    #[wire(rest)]
    pub payload: &'wire [u8],
}

#[test]
fn computed_field_validation_receives_the_stored_semantic_value() {
    assert!(matches!(
        ComputedValidated::view(&[0]).without_trailing(),
        Err(ComputedValidationError::Zero)
    ));

    let unchecked = ComputedValidated::view(&[0])
        .unchecked()
        .without_trailing()
        .unwrap();
    assert_eq!(unchecked.length(), 0);

    let validated = ComputedValidated::view(&[1, 9]).without_trailing().unwrap();
    assert_eq!(validated.length(), 1);
    assert_eq!(validated.payload(), [9]);
}
