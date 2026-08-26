#![deny(unsafe_code)]

#[path = "behavior/builder.rs"]
mod builder;
#[path = "behavior/collection.rs"]
mod collection;
#[path = "behavior/dependency.rs"]
mod dependency;
#[path = "behavior/dependency_hygiene.rs"]
mod dependency_hygiene;
#[path = "behavior/geometry.rs"]
mod geometry;
#[path = "behavior/hygiene.rs"]
mod hygiene;
#[path = "behavior/layout.rs"]
mod layout;
#[path = "behavior/ownership.rs"]
mod ownership;
#[path = "behavior/scalar.rs"]
mod scalar;
#[path = "behavior/view.rs"]
mod view;
