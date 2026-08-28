//! Sends a real NTPv4 client request over UDP and frames the server response.
//!
//! The echoed origin timestamp correlates the reply but does not authenticate plaintext NTP.
//! Consumers requiring trusted time need an authenticated protocol such as Network Time Security.

use std::env;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use wire_repr::{WireBuilder, WireView, wire};

const DEFAULT_SERVER: &str = "time.cloudflare.com:123";
const NTP_UNIX_EPOCH_DELTA: u64 = 2_208_988_800;
const MAX_DATAGRAM: usize = 1024;

#[allow(dead_code)]
#[derive(WireView, WireBuilder)]
#[wire(as = u8)]
struct NtpFlags {
    #[wire(bits = 6..=7)]
    leap: u8,
    #[wire(bits = 3..=5)]
    version: u8,
    #[wire(bits = 0..=2)]
    mode: u8,
}

#[allow(dead_code)]
#[derive(WireView, WireBuilder)]
struct NtpPacket {
    flags: NtpFlags,
    stratum: u8,
    poll: i8,
    precision: i8,
    #[wire(be)]
    root_delay: u32,
    #[wire(be)]
    root_dispersion: u32,
    reference_id: [u8; 4],
    #[wire(be)]
    reference_timestamp: u64,
    #[wire(be)]
    origin_timestamp: u64,
    #[wire(be)]
    receive_timestamp: u64,
    #[wire(be)]
    transmit_timestamp: u64,
    #[wire(rest)]
    extensions: wire::Bytes,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args().skip(1);
    let server = arguments
        .next()
        .unwrap_or_else(|| DEFAULT_SERVER.to_owned());
    if arguments.next().is_some() {
        return Err(invalid_input("usage: ntp [server:port]").into());
    }

    let address = resolve_one(&server)?;
    let socket = UdpSocket::bind(if address.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    })?;
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;
    socket.set_write_timeout(Some(Duration::from_secs(5)))?;
    socket.connect(address)?;

    let sent_at = SystemTime::now();
    let client_timestamp = ntp_timestamp(sent_at)?;
    let mut request = Vec::new();
    let request = NtpPacket::builder(&mut request)
        .flags(|flags| flags.leap(0).version(4).mode(3))?
        .stratum(0)?
        .poll(6)?
        .precision(-20)?
        .root_delay(0)?
        .root_dispersion(0)?
        .reference_id([0; 4])?
        .reference_timestamp(0)?
        .origin_timestamp(0)?
        .receive_timestamp(0)?
        .transmit_timestamp(client_timestamp)?
        .extensions(&[])?
        .finish()?;
    let sent = socket.send(request.as_bytes())?;
    if sent != request.len() {
        return Err(io::Error::new(io::ErrorKind::WriteZero, "partial NTP datagram send").into());
    }

    let mut response = [0u8; MAX_DATAGRAM + 1];
    let received = socket.recv(&mut response)?;
    if received > MAX_DATAGRAM {
        return Err(invalid_data(format!("NTP datagram exceeds {MAX_DATAGRAM} bytes")).into());
    }
    let received_at = SystemTime::now();
    let packet = NtpPacket::view(&response[..received])?;
    let flags = packet.flags();

    if flags.mode() != 4 || !(3..=4).contains(&flags.version()) {
        return Err(invalid_data(format!(
            "unexpected NTP response mode/version: {}/{}",
            flags.mode(),
            flags.version()
        ))
        .into());
    }
    if flags.leap() == 3 {
        return Err(invalid_data("NTP server reports an unsynchronised clock").into());
    }
    if !(1..=15).contains(&packet.stratum()) {
        let reference = reference_id(packet.reference_id());
        return Err(invalid_data(format!(
            "NTP server returned unusable stratum {} ({reference})",
            packet.stratum()
        ))
        .into());
    }
    if packet.origin_timestamp() != client_timestamp {
        return Err(invalid_data("server did not echo the client transmit timestamp").into());
    }
    if packet.receive_timestamp() == 0 || packet.transmit_timestamp() == 0 {
        return Err(invalid_data("server returned an empty receive/transmit timestamp").into());
    }

    let received_timestamp = ntp_timestamp(received_at)?;
    let client_seconds = unfold_seconds(client_timestamp, received_at)?;
    let server_received = unfold_seconds(packet.receive_timestamp(), received_at)?;
    let server_transmitted = unfold_seconds(packet.transmit_timestamp(), received_at)?;
    let client_received = unfold_seconds(received_timestamp, received_at)?;
    let offset =
        ((server_received - client_seconds) + (server_transmitted - client_received)) / 2.0;
    let unix_time = server_transmitted - NTP_UNIX_EPOCH_DELTA as f64;

    println!("NTP response from {}", socket.peer_addr()?);
    println!("  version:     {}", flags.version());
    println!("  mode:        {} (server)", flags.mode());
    println!("  leap:        {}", flags.leap());
    println!("  stratum:     {}", packet.stratum());
    println!("  reference:   {}", reference_id(packet.reference_id()));
    println!("  unix time:   {unix_time:.6}");
    println!("  clock offset {offset:+.6} s");
    println!("  wire bytes:  {received}");
    println!("  extensions:  {} bytes", packet.extensions().len());

    Ok(())
}

fn resolve_one(server: &str) -> io::Result<SocketAddr> {
    server
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| invalid_input(format!("server did not resolve: {server}")))
}

fn ntp_timestamp(time: SystemTime) -> io::Result<u64> {
    let elapsed = time
        .duration_since(UNIX_EPOCH)
        .map_err(|error| invalid_data(format!("system clock precedes Unix epoch: {error}")))?;
    let seconds = elapsed
        .as_secs()
        .checked_add(NTP_UNIX_EPOCH_DELTA)
        .ok_or_else(|| invalid_data("NTP timestamp seconds overflow"))?;
    let wire_seconds = u32::try_from(seconds % (1u64 << 32))
        .map_err(|_| invalid_data("NTP era reduction failed"))?;
    let fraction = (u64::from(elapsed.subsec_nanos()) << 32) / 1_000_000_000;
    Ok((u64::from(wire_seconds) << 32) | fraction)
}

fn unfold_seconds(timestamp: u64, reference: SystemTime) -> io::Result<f64> {
    const ERA_SECONDS: i128 = 1i128 << 32;

    let reference = reference
        .duration_since(UNIX_EPOCH)
        .map_err(|error| invalid_data(format!("system clock precedes Unix epoch: {error}")))?;
    let reference_seconds = i128::from(reference.as_secs())
        .checked_add(i128::from(NTP_UNIX_EPOCH_DELTA))
        .ok_or_else(|| invalid_data("NTP reference timestamp overflow"))?;
    let wire_seconds = i128::from(timestamp >> 32);
    let era = (reference_seconds - wire_seconds + ERA_SECONDS / 2).div_euclid(ERA_SECONDS);
    let unfolded = wire_seconds + era * ERA_SECONDS;
    let fraction = f64::from(timestamp as u32) / 4_294_967_296.0;
    Ok(unfolded as f64 + fraction)
}

fn reference_id(bytes: [u8; 4]) -> String {
    if bytes.iter().all(u8::is_ascii_graphic) {
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        format!(
            "{:02x}{:02x}{:02x}{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3]
        )
    }
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_unfold_across_the_2036_era_boundary() -> io::Result<()> {
        let unix_seconds = 2_200_000_000;
        let reference = UNIX_EPOCH + Duration::from_secs(unix_seconds);
        let timestamp = ntp_timestamp(reference)?;
        let unfolded = unfold_seconds(timestamp, reference)?;
        let recovered_unix = unfolded - NTP_UNIX_EPOCH_DELTA as f64;
        assert!((recovered_unix - unix_seconds as f64).abs() < 1e-6);
        Ok(())
    }
}
