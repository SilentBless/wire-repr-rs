#![allow(dead_code)]

use core::convert::Infallible;

use wire_repr::{WireBuilder, output};

#[derive(WireBuilder)]
struct Foo {
    #[wire(le)]
    foo: u32,
    #[wire(be)]
    bar: u32,
}

type WriteFailure = wire_repr::WriteError<Infallible, Infallible>;

pub fn fixed(seed: u64) -> u64 {
    match try_fixed(seed) {
        Ok(value) => value,
        Err(_) => u64::MAX,
    }
}

#[inline(always)]
fn try_fixed(seed: u64) -> Result<u64, WriteFailure> {
    let mut output = [0u8; 8];
    Foo::builder(&mut output[..])
        .foo(seed as u32)?
        .bar((seed >> 32) as u32)?
        .finish()?;
    Ok(u64::from_le_bytes(output))
}

pub fn growable(seed: u64) -> u64 {
    match try_growable(seed) {
        Ok(value) => value,
        Err(_) => u64::MAX,
    }
}

#[inline(always)]
fn try_growable(seed: u64) -> Result<u64, WriteFailure> {
    let mut output = Vec::new();
    let written = Foo::builder(&mut output)
        .foo(seed as u32)?
        .bar((seed >> 32) as u32)?
        .finish()?;
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(written.as_bytes());
    Ok(u64::from_le_bytes(bytes))
}

pub fn owned(seed: u64) -> u64 {
    match try_owned(seed) {
        Ok(value) => value,
        Err(_) => u64::MAX,
    }
}

#[inline(always)]
fn try_owned(seed: u64) -> Result<u64, WriteFailure> {
    let written = Foo::builder(output::owned(Vec::new()))
        .foo(seed as u32)?
        .bar((seed >> 32) as u32)?
        .finish()?;
    let (output, range) = written.into_parts();
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&output.as_ref()[range]);
    let _output = output.into_inner();
    Ok(u64::from_le_bytes(bytes))
}

pub fn callback(seed: u64) -> u64 {
    match try_callback(seed) {
        Ok(value) => value,
        Err(_) => u64::MAX,
    }
}

#[inline(always)]
fn try_callback(seed: u64) -> Result<u64, WriteFailure> {
    struct Window {
        bytes: [u8; 16],
        len: usize,
    }

    impl AsRef<[u8]> for Window {
        fn as_ref(&self) -> &[u8] {
            &self.bytes[..self.len]
        }
    }

    impl AsMut<[u8]> for Window {
        fn as_mut(&mut self) -> &mut [u8] {
            &mut self.bytes[..self.len]
        }
    }

    let mut output = Window {
        bytes: [0; 16],
        len: 0,
    };
    let adapter = output::grow_with(&mut output, |output, request| {
        output.len = request.suggested_len.min(output.bytes.len());
        Ok::<_, Infallible>(())
    });
    let written = Foo::builder(adapter)
        .foo(seed as u32)?
        .bar((seed >> 32) as u32)?
        .finish()?;
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(written.as_bytes());
    Ok(u64::from_le_bytes(bytes))
}
