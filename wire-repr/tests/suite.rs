#![deny(unsafe_code)]

#[path = "behavior/bitfield.rs"]
mod bitfield;
#[path = "behavior/builder.rs"]
mod builder;
#[path = "behavior/collection.rs"]
mod collection;
#[path = "behavior/computed.rs"]
mod computed;
#[path = "behavior/dependency.rs"]
mod dependency;
#[path = "behavior/dependency_hygiene.rs"]
mod dependency_hygiene;
#[path = "behavior/enumeration.rs"]
mod enumeration;
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
#[path = "behavior/selection.rs"]
mod selection;
#[path = "behavior/sequence.rs"]
mod sequence;
#[path = "behavior/view.rs"]
mod view;
