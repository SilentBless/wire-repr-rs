//! Sends a real DNS query over UDP, then frames and inspects the response datagram.

use std::env;
use std::fs;
use std::io;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::Duration;

use wire_repr::{WireBuilder, WireView, wire};

const DEFAULT_NAME: &str = "example.com";
const FALLBACK_RESOLVER: &str = "1.1.1.1:53";
const MAX_DATAGRAM: usize = 4096;

#[allow(dead_code)]
#[derive(WireView, WireBuilder)]
#[wire(as = u16, be)]
struct DnsFlags {
    #[wire(bit = 15)]
    response: bool,
    #[wire(bits = 11..=14)]
    opcode: u8,
    #[wire(bit = 10)]
    authoritative: bool,
    #[wire(bit = 9)]
    truncated: bool,
    #[wire(bit = 8)]
    recursion_desired: bool,
    #[wire(bit = 7)]
    recursion_available: bool,
    #[wire(bits = 4..=6)]
    reserved: u8,
    #[wire(bits = 0..=3)]
    response_code: u8,
}

#[allow(dead_code)]
#[derive(WireView, WireBuilder)]
struct DnsHeader {
    #[wire(be)]
    id: u16,
    flags: DnsFlags,
    #[wire(be)]
    questions: u16,
    #[wire(be)]
    answers: u16,
    #[wire(be)]
    authorities: u16,
    #[wire(be)]
    additional: u16,
}

#[allow(dead_code)]
#[derive(WireView, WireBuilder)]
struct DnsMessage {
    header: DnsHeader,
    #[wire(rest)]
    body: wire::Bytes,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args().skip(1);
    let name = arguments.next().unwrap_or_else(|| DEFAULT_NAME.to_owned());
    let resolver = match arguments.next() {
        Some(resolver) => resolver,
        None => default_resolver()?,
    };
    if arguments.next().is_some() {
        return Err(invalid_input("usage: dns [name] [resolver:port]").into());
    }

    let question = encode_question(&name)?;
    let id = random_id()?;
    let mut request = Vec::new();
    let request = DnsMessage::builder(&mut request)
        .header(|header| {
            header
                .id(id)
                .flags(|flags| {
                    flags
                        .response(false)
                        .opcode(0)
                        .authoritative(false)
                        .truncated(false)
                        .recursion_desired(true)
                        .recursion_available(false)
                        .reserved(0)
                        .response_code(0)
                })
                .questions(1)
                .answers(0)
                .authorities(0)
                .additional(0)
        })?
        .body(&question)?
        .finish()?;

    let address = resolve_one(&resolver)?;
    let socket = UdpSocket::bind(if address.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    })?;
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;
    socket.set_write_timeout(Some(Duration::from_secs(5)))?;
    socket.connect(address)?;
    let sent = socket.send(request.as_bytes())?;
    if sent != request.len() {
        return Err(io::Error::new(io::ErrorKind::WriteZero, "partial DNS datagram send").into());
    }

    let mut response = [0u8; MAX_DATAGRAM + 1];
    let received = socket.recv(&mut response)?;
    if received > MAX_DATAGRAM {
        return Err(invalid_data(format!("DNS datagram exceeds {MAX_DATAGRAM} bytes")).into());
    }
    let message = DnsMessage::view(&response[..received])?;
    let header = message.header();
    let flags = header.flags();

    if header.id() != id {
        return Err(invalid_data(format!(
            "transaction ID mismatch: sent {id:#06x}, received {:#06x}",
            header.id()
        ))
        .into());
    }
    if !flags.response() || flags.opcode() != 0 {
        return Err(invalid_data("resolver returned a non-query DNS message").into());
    }

    println!("DNS response from {}", socket.peer_addr()?);
    println!("  name:       {name}");
    println!("  id:         {:#06x}", header.id());
    println!("  rcode:      {}", flags.response_code());
    println!("  truncated:  {}", flags.truncated());
    println!("  questions:  {}", header.questions());
    println!("  answers:    {}", header.answers());
    println!("  authority:  {}", header.authorities());
    println!("  additional: {}", header.additional());
    println!("  body bytes: {}", message.body().len());

    Ok(())
}

fn default_resolver() -> io::Result<String> {
    match fs::read_to_string("/etc/resolv.conf") {
        Ok(contents) => {
            for line in contents.lines() {
                let mut words = line.split_whitespace();
                if words.next() != Some("nameserver") {
                    continue;
                }
                let address = words
                    .next()
                    .ok_or_else(|| invalid_data("nameserver entry has no address"))?;
                let address = address
                    .parse::<IpAddr>()
                    .map_err(|_| invalid_data(format!("invalid nameserver address: {address}")))?;
                return Ok(SocketAddr::new(address, 53).to_string());
            }
            eprintln!("no system nameserver found; using {FALLBACK_RESOLVER}");
            Ok(FALLBACK_RESOLVER.to_owned())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            eprintln!("system resolver configuration unavailable; using {FALLBACK_RESOLVER}");
            Ok(FALLBACK_RESOLVER.to_owned())
        }
        Err(error) => Err(error),
    }
}

fn resolve_one(resolver: &str) -> io::Result<SocketAddr> {
    resolver
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| invalid_input(format!("resolver did not resolve: {resolver}")))
}

fn random_id() -> io::Result<u16> {
    let mut bytes = [0u8; 2];
    getrandom::fill(&mut bytes).map_err(|error| {
        io::Error::other(format!("operating-system randomness failed: {error}"))
    })?;
    Ok(u16::from_ne_bytes(bytes))
}

fn encode_question(name: &str) -> io::Result<Vec<u8>> {
    let name = name.strip_suffix('.').unwrap_or(name);
    if name.is_empty() || name.len() > 253 || !name.is_ascii() {
        return Err(invalid_input(
            "DNS example accepts one non-empty ASCII name up to 253 bytes",
        ));
    }

    let mut encoded = Vec::with_capacity(name.len() + 6);
    for label in name.split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(invalid_input("DNS labels must contain 1..=63 bytes"));
        }
        encoded.push(label.len() as u8);
        encoded.extend_from_slice(label.as_bytes());
    }
    encoded.push(0);
    if encoded.len() > 255 {
        return Err(invalid_input("encoded DNS name exceeds 255 bytes"));
    }
    encoded.extend_from_slice(&1u16.to_be_bytes()); // QTYPE A
    encoded.extend_from_slice(&1u16.to_be_bytes()); // QCLASS IN
    Ok(encoded)
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
