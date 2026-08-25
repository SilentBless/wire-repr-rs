#[path = "../generated.rs"]
mod generated;

pub fn view_bytes(_seed: u64) -> u64 {
    let view = generated::Foo::view([0u8; 8]).unwrap();
    std::mem::size_of_val(&view) as u64
}
