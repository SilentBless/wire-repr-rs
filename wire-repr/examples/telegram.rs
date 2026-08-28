//! Starts the real MTProto authorization-key handshake against a Telegram DC.
//!
//! The example sends `req_pq_multi`, frames `resPQ`, validates the echoed nonce, and prints the
//! server's `pq` value and RSA key fingerprints. It deliberately stops before factorization and
//! Diffie-Hellman; receiving `resPQ` alone does not authenticate the server.

use std::env;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use wire_repr::{WireBuilder, WireView, wire};

const DEFAULT_DC: &str = "149.154.167.50:443";
const MAX_FRAME_LEN: usize = 1024 * 1024;

#[allow(dead_code)]
#[derive(WireView)]
struct IntermediateHeader {
    #[wire(le)]
    length: u32,
}

#[allow(dead_code)]
#[derive(WireView, WireBuilder)]
struct IntermediateConnect<T> {
    #[wire(le, constant = 0xeeee_eeee)]
    marker: u32,
    frame: T,
}

#[allow(dead_code)]
#[derive(WireView, WireBuilder)]
struct IntermediateFrame<T> {
    #[wire(le)]
    length: u32,
    #[wire(bytes = length)]
    payload: T,
}

#[allow(dead_code)]
#[derive(WireView, WireBuilder)]
struct PlainMessage<T> {
    #[wire(le, constant = 0)]
    auth_key_id: u64,
    #[wire(le, try_computed = fresh_message_id())]
    message_id: u64,
    #[wire(le)]
    body_length: u32,
    #[wire(bytes = body_length)]
    body: T,
}

#[allow(dead_code)]
#[derive(WireView, WireBuilder)]
struct ReqPqMulti {
    #[wire(le, constant = 0xbe7e_8ef1)]
    constructor: u32,
    #[wire(le, try_computed = random_nonce())]
    nonce: u128,
}
type BootstrapRequest = IntermediateConnect<IntermediateFrame<PlainMessage<ReqPqMulti>>>;

#[allow(dead_code)]
#[derive(WireView)]
struct RawBody {
    #[wire(rest)]
    bytes: wire::Bytes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("resPQ must contain a 1..=8 byte pq value")]
struct InvalidResPq;

#[wire_repr::validator]
fn validate_res_pq(view: &impl ResPqView) -> Result<(), InvalidResPq> {
    (!view.pq().is_empty() && view.pq().len() <= 8)
        .then_some(())
        .ok_or(InvalidResPq)
}

#[allow(dead_code)]
#[derive(WireView)]
#[wire(validate = validate_res_pq)]
struct ResPq {
    #[wire(le, constant = 0x0516_2463)]
    constructor: u32,
    #[wire(le)]
    nonce: u128,
    #[wire(le)]
    server_nonce: u128,
    pq_length: u8,
    #[wire(bytes = pq_length)]
    pq: wire::Bytes,
    #[wire(align_before = 4, rest)]
    fingerprint_vector: wire::Bytes,
}

#[allow(dead_code)]
#[derive(WireView)]
struct Fingerprint {
    #[wire(le)]
    value: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("resPQ must contain at least one RSA fingerprint")]
struct InvalidFingerprints;

#[wire_repr::validator]
fn validate_fingerprints(view: &impl FingerprintsView) -> Result<(), InvalidFingerprints> {
    (!view.values().is_empty())
        .then_some(())
        .ok_or(InvalidFingerprints)
}

#[allow(dead_code)]
#[derive(WireView)]
#[wire(validate = validate_fingerprints)]
struct Fingerprints {
    #[wire(le, constant = 0x1cb5_c415)]
    vector_constructor: u32,
    #[wire(le)]
    count: u32,
    #[wire(counted_by = count)]
    values: wire::Array<Fingerprint>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args().skip(1);
    let endpoint = arguments.next().unwrap_or_else(|| DEFAULT_DC.to_owned());
    if arguments.next().is_some() {
        return Err(invalid_input("usage: telegram [dc-address:port]").into());
    }

    let address = resolve_one(&endpoint)?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(8))?;
    stream.set_write_timeout(Some(Duration::from_secs(8)))?;

    let request = BootstrapRequest::builder(wire_repr::output::owned(Vec::new()))
        .frame(|frame| frame.payload(|message| message.body(|request| request)))?
        .finish()?;
    let request_view = BootstrapRequest::view(request.as_bytes())?;
    let request_frame = request_view.frame();
    let request_message = request_frame.payload();
    let nonce = request_message.body().nonce();

    stream.write_all(request.as_bytes())?;
    stream.flush()?;

    let deadline = Instant::now()
        .checked_add(Duration::from_secs(8))
        .ok_or_else(|| invalid_data("response deadline overflow"))?;
    let response = read_intermediate_frame(&mut stream, deadline)?;
    if response.len() == 8 {
        let code = i32::from_le_bytes([response[4], response[5], response[6], response[7]]);
        if code < 0 {
            return Err(
                invalid_data(format!("Telegram transport error {}", code.unsigned_abs())).into(),
            );
        }
    }

    let frame = IntermediateFrame::<PlainMessage<RawBody>>::view(response)?;
    let message = frame.payload();
    let body = message.body();
    let response = ResPq::view(body.bytes())?;
    if response.nonce() != nonce {
        return Err(invalid_data("resPQ nonce does not match req_pq_multi").into());
    }
    let fingerprints = Fingerprints::view(response.fingerprint_vector())?;
    let fingerprint_values = fingerprints.values();
    for fingerprint in &fingerprint_values {
        fingerprint?;
    }

    println!("Telegram MTProto bootstrap response from {address}");
    println!("  transport:    intermediate TCP");
    println!("  frame bytes:  {}", frame.as_bytes().len());
    println!("  request id:   {:#018x}", request_message.message_id());
    println!("  response id:  {:#018x}", message.message_id());
    println!("  nonce:        {}", hex(&nonce.to_le_bytes()));
    println!(
        "  server nonce: {}",
        hex(&response.server_nonce().to_le_bytes())
    );
    println!("  pq:           0x{}", hex(response.pq()));
    println!("  RSA fingerprints ({}):", fingerprint_values.len());
    for fingerprint in (&fingerprint_values).into_iter().take(16) {
        let fingerprint = fingerprint?.view().value() as u64;
        println!("    {fingerprint:#018x}");
    }
    if fingerprint_values.len() > 16 {
        println!("    … {} more", fingerprint_values.len() - 16);
    }
    println!("  next step: factor pq, select a trusted fingerprint, then continue DH");

    Ok(())
}

// `wire-repr` frames contiguous spans; the transport owns partial reads and retry policy.
fn read_intermediate_frame(
    stream: &mut TcpStream,
    deadline: Instant,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut length_bytes = [0u8; 4];
    read_exact_until(stream, &mut length_bytes, deadline)?;
    let length = IntermediateHeader::view(&length_bytes)?.length();
    if length & 0x8000_0000 != 0 {
        return Err(invalid_data("unexpected quick-ack token").into());
    }
    let length = usize::try_from(length).map_err(|_| invalid_data("frame length overflow"))?;
    if length == 0 || length > MAX_FRAME_LEN || length % 4 != 0 {
        return Err(invalid_data(format!("invalid intermediate frame length: {length}")).into());
    }

    let total = length
        .checked_add(4)
        .ok_or_else(|| invalid_data("frame length overflow"))?;
    let mut frame = Vec::with_capacity(total);
    frame.extend_from_slice(&length_bytes);
    frame.resize(total, 0);
    read_exact_until(stream, &mut frame[4..], deadline)?;
    Ok(frame)
}

fn read_exact_until(
    stream: &mut TcpStream,
    mut output: &mut [u8],
    deadline: Instant,
) -> io::Result<()> {
    while !output.is_empty() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Telegram response deadline elapsed",
            ));
        }
        stream.set_read_timeout(Some(remaining))?;
        match stream.read(output) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Telegram closed the frame early",
                ));
            }
            Ok(read) => {
                let (_, rest) = output.split_at_mut(read);
                output = rest;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[wire_repr::computed]
fn fresh_message_id() -> Result<u64, io::Error> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| invalid_data(format!("system clock precedes Unix epoch: {error}")))?;
    let seconds = u32::try_from(elapsed.as_secs())
        .map_err(|_| invalid_data("Unix timestamp does not fit MTProto message_id"))?;
    let mut fraction = (u64::from(elapsed.subsec_nanos()) << 32) / 1_000_000_000;
    fraction &= 0xffff_fffc;
    if fraction == 0 {
        fraction = 4;
    }
    Ok((u64::from(seconds) << 32) | fraction)
}

#[wire_repr::computed]
fn random_nonce() -> Result<u128, io::Error> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| {
        io::Error::other(format!("operating-system randomness failed: {error}"))
    })?;
    Ok(u128::from_le_bytes(bytes))
}

fn resolve_one(endpoint: &str) -> io::Result<SocketAddr> {
    endpoint
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| invalid_input(format!("DC endpoint did not resolve: {endpoint}")))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";

    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
