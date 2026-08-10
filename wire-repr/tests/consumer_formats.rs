#![deny(missing_docs, unsafe_code)]

//! Consumer-format integration coverage.

#[path = "consumer_formats/png.rs"]
mod png;
#[path = "consumer_formats/sqlite.rs"]
mod sqlite;
#[path = "consumer_formats/wasm.rs"]
mod wasm;
