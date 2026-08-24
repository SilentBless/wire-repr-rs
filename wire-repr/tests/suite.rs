#![deny(missing_docs, unsafe_code)]
#![allow(dead_code)]
//! Behavioral contracts for the public facade.

#[cfg(not(feature = "bytes"))]
#[path = "cases/bitfield_frame.rs"]
mod bitfield_frame;
#[path = "cases/sequential_frame/bounded.rs"]
mod bounded_frame;
#[cfg(not(feature = "bytes"))]
#[path = "cases/byte_stream.rs"]
mod byte_stream;
#[cfg(not(feature = "bytes"))]
#[path = "cases/derived_field.rs"]
mod derived_field;
#[cfg(not(feature = "bytes"))]
#[path = "cases/sequential_frame/fixed.rs"]
mod fixed_frame;
#[cfg(not(feature = "bytes"))]
#[path = "cases/physical_projection/generated.rs"]
mod generated_projection;
#[path = "cases/inferred_validation.rs"]
mod inferred_validation;
#[cfg(not(feature = "bytes"))]
#[path = "cases/sequential_frame/nested.rs"]
mod nested_frame;
#[cfg(not(feature = "bytes"))]
#[path = "cases/positioned_frame/offset.rs"]
mod positioned_frame;
#[cfg(not(feature = "bytes"))]
#[path = "cases/positioned_frame/padding.rs"]
mod positioned_padding;
#[cfg(not(feature = "bytes"))]
#[path = "cases/self_delimited_field/prefix.rs"]
mod prefixed_field;
#[path = "cases/self_delimited_field/rest.rs"]
mod remainder_field;
#[cfg(not(feature = "bytes"))]
#[path = "cases/physical_projection/runtime.rs"]
mod runtime_projection;
#[cfg(not(feature = "bytes"))]
#[path = "cases/tagged_frame.rs"]
mod tagged_frame;
#[cfg(not(feature = "bytes"))]
#[path = "cases/validated_frame.rs"]
mod validated_frame;
#[path = "cases/validator_metadata.rs"]
mod validator_metadata;

#[cfg(feature = "bytes")]
#[path = "cases/shared_backing.rs"]
mod shared_backing;
