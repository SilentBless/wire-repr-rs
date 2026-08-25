#![deny(unsafe_code)]

#[path = "behavior/builder.rs"]
mod builder;
#[path = "behavior/hygiene.rs"]
mod hygiene;
#[path = "behavior/ownership.rs"]
mod ownership;
#[path = "behavior/scalar.rs"]
mod scalar;
#[path = "behavior/view.rs"]
mod view;
