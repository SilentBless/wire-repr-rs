#![deny(missing_docs, unsafe_code)]
//! Inferred validator error composition coverage.

use wire_repr::{Wire, validator};

/// Shared field-validation failure.
#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub enum KindError {
    /// The first validator rejected the value.
    #[error("first validator rejected {0}")]
    First(u8),
    /// The second validator rejected the value.
    #[error("second validator rejected {0}")]
    Second(u8),
}

/// Model-validation failure.
#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub enum ModelError {
    /// Both fields have the same value.
    #[error("kind and code must differ")]
    Equal,
}

/// Dynamic payload-validation failure.
#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub enum PayloadError {
    /// The payload is empty.
    #[error("payload must not be empty")]
    Empty,
}

/// First validator namespace.
pub mod checks_a {
    use super::KindError;
    use wire_repr::validator;

    /// Rejects one as a kind.
    #[validator]
    pub fn reject(value: u8) -> Result<(), KindError> {
        if value == 1 {
            Err(KindError::First(value))
        } else {
            Ok(())
        }
    }
}

/// Second validator namespace.
pub mod checks_b {
    use super::KindError;
    use wire_repr::validator;

    /// Rejects two as a kind.
    #[validator]
    pub fn reject(value: u8) -> Result<(), KindError> {
        if value == 2 {
            Err(KindError::Second(value))
        } else {
            Ok(())
        }
    }
}

/// Rejects packets whose fields are equal.
#[cfg(not(feature = "bytes"))]
#[validator]
pub fn validate_packet(view: &impl InferredPacketView) -> Result<(), ModelError> {
    if view.kind() == view.code() {
        Err(ModelError::Equal)
    } else {
        Ok(())
    }
}

/// Rejects packets whose fields are equal.
#[cfg(feature = "bytes")]
#[validator]
pub fn validate_packet(view: &impl InferredPacketView) -> Result<(), ModelError> {
    if view.kind() == view.code() {
        Err(ModelError::Equal)
    } else {
        Ok(())
    }
}

/// Rejects empty dynamic payloads.
#[validator]
pub fn nonempty(payload: &[u8]) -> Result<(), PayloadError> {
    if payload.is_empty() {
        Err(PayloadError::Empty)
    } else {
        Ok(())
    }
}

/// A fixed packet whose read errors are inferred from its validators.
#[derive(Wire)]
#[wire(validate = validate_packet)]
pub struct InferredPacket {
    /// Packet kind.
    #[wire(validate = checks_a::reject, validate = checks_b::reject)]
    pub kind: u8,
    /// Packet code.
    pub code: u8,
}

/// A dynamic packet whose validation error is inferred.
#[derive(Wire)]
pub struct DynamicPacket<'wire> {
    /// Encoded payload length.
    pub payload_length: u8,
    /// Borrowed payload bytes.
    #[wire(bytes = payload_length, validate = nonempty)]
    pub payload: &'wire [u8],
}

#[cfg(not(feature = "bytes"))]
macro_rules! wire_input {
    ($($byte:expr),* $(,)?) => {
        &[$($byte),*]
    };
}

#[cfg(feature = "bytes")]
macro_rules! wire_input {
    ($($byte:expr),* $(,)?) => {
        bytes::Bytes::from_static(&[$($byte),*])
    };
}

#[test]
fn inferred_errors_compose_decode_and_distinct_validator_sites() {
    assert!(matches!(
        InferredPacket::view(wire_input![1, 9]),
        Err(InferredPacketError::Validate(
            InferredPacketValidationError::KindReject(KindError::First(1))
        ))
    ));
    assert!(matches!(
        InferredPacket::view(wire_input![2, 9]),
        Err(InferredPacketError::Validate(
            InferredPacketValidationError::KindReject2(KindError::Second(2))
        ))
    ));
    assert!(matches!(
        InferredPacket::view(wire_input![3, 3]),
        Err(InferredPacketError::Validate(
            InferredPacketValidationError::ModelValidatePacket(ModelError::Equal)
        ))
    ));
    assert!(matches!(
        InferredPacket::view(wire_input![3]),
        Err(InferredPacketError::Decode(
            InferredPacketDecodeError::InputTooShort { field: "code", .. }
        ))
    ));
    assert!(matches!(
        InferredPacket::view(wire_input![3, 4, 5]),
        Err(InferredPacketError::Decode(
            InferredPacketDecodeError::TrailingBytes {
                expected: 2,
                actual: 3,
            }
        ))
    ));
}

#[test]
fn inferred_validation_preserves_context_and_structural_decode_errors() {
    let error = match InferredPacket::view(wire_input![2, 9]) {
        Err(error) => error,
        Ok(_) => panic!("the second validator must reject kind 2"),
    };
    assert_eq!(
        error.to_string(),
        "validator `checks_b::reject` rejected field `kind`: second validator rejected 2"
    );
    assert!(
        core::error::Error::source(&error)
            .and_then(|source| source.downcast_ref::<KindError>())
            .is_some()
    );

    assert!(matches!(
        InferredPacket::view(wire_input![1]),
        Err(InferredPacketError::Decode(
            InferredPacketDecodeError::InputTooShort { .. }
        ))
    ));
}

#[test]
fn dynamic_inferred_validation_uses_the_same_error_composition() {
    assert!(matches!(
        DynamicPacket::view(wire_input![0]).without_trailing(),
        Err(DynamicPacketError::Validate(
            DynamicPacketValidationError::PayloadNonempty(PayloadError::Empty)
        ))
    ));
    assert!(
        DynamicPacket::view(wire_input![1, 7])
            .without_trailing()
            .is_ok()
    );
}
