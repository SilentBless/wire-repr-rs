//! Release-codegen regression probes for the derive frontend.

#[cfg(not(feature = "bytes"))]
#[path = "codegen/decode.rs"]
mod decode;
#[cfg(not(feature = "bytes"))]
#[path = "codegen/encode.rs"]
mod encode;
#[cfg(not(feature = "bytes"))]
#[path = "codegen/oracle.rs"]
mod oracle;
#[cfg(not(feature = "bytes"))]
#[path = "codegen/schema.rs"]
mod schema;
#[cfg(not(feature = "bytes"))]
#[path = "codegen/selection.rs"]
mod selection;

#[cfg(feature = "bytes")]
#[path = "codegen/owned.rs"]
mod owned;
