//! routes authenticated client datagrams between one proxy listener and its BDS.

use crate::{config::Config, session::SessionTable};
use std::{io, net::SocketAddr, sync::Arc, time::Duration};
use tokio::net::UdpSocket;

// marks the start of STUN attributes.
const STUN_HEADER_LEN: usize = 20;
const STUN_MAGIC_COOKIE: u32 = 0x2112_a442;
const ATTR_USERNAME: u16 = 0x0006;
// bounds packet previews to keep diagnostic output small.
const MAX_LOGGED_BYTES: usize = 256;

/// holds the fields printed for a recognized STUN packet.
struct StunSummary {
    message_type: u16,
    transaction_id: String,
    username: Option<String>,
}

/// receives and relays datagrams using this instance's session table.
pub async fn serve(
    socket: UdpSocket,
    config: Config,
    sessions: Arc<SessionTable>,
) -> io::Result<()> {
    let local_addr = socket.local_addr()?;
    println!(
        "UDP mux listening on {local_addr}; advertised candidate {}:{}",
        config.public_host, config.public_port
    );

    let mut buffer = vec![0; 65_535];
    let mut cleanup = tokio::time::interval(Duration::from_secs(60));
    let mut bds_responses_logged = 0;
    let mut unknown_packets_logged = 0;
    loop {
        tokio::select! {
            received = socket.recv_from(&mut buffer) => {
                let (length, source) = match received {
                    Ok(value) => value,
                    Err(error) => {
                        eprintln!("UDP receive failed on {local_addr}: {error}");
                        continue;
                    }
                };
                let packet = &buffer[..length];
                // use established routes first; authenticate new clients through STUN.
                let route = if let Some(target) = sessions.relay_from_client(source) {
                    Some((target, true))
                } else if let Some(target) = sessions.relay_from_bds(source) {
                    Some((target, false))
                } else {
                    match sessions.claim_client(source, packet) {
                        Ok(Some(target)) => {
                            log_datagram(local_addr, &config, source, packet);
                            println!("STUN client {source} claimed; forwarding to BDS {target}");
                            Some((target, true))
                        }
                        Ok(None) => {
                            if unknown_packets_logged < 5 {
                                log_datagram(local_addr, &config, source, packet);
                                println!("Dropping unrecognized UDP datagram from {source}");
                                unknown_packets_logged += 1;
                            }
                            None
                        }
                        Err(error) => {
                            eprintln!("Rejected STUN claim from {source}: {error}");
                            None
                        }
                    }
                };

                if let Some((target, from_client)) = route {
                    if !from_client && bds_responses_logged < 3 {
                        println!("BDS response {source} -> client {target} ({} bytes)", length);
                        bds_responses_logged += 1;
                    }
                    if let Err(error) = socket.send_to(packet, target).await {
                        eprintln!("UDP relay {source} -> {target} failed: {error}");
                    }
                }
            }
            // periodically removes expired sessions and their route indexes.
            _ = cleanup.tick() => sessions.remove_idle_sessions(),
        }
    }
}

/// logs a bounded packet preview for initial connection diagnostics.
fn log_datagram(local_addr: SocketAddr, config: &Config, source: SocketAddr, packet: &[u8]) {
    println!(
        "\n[UDP datagram] source={source}, listener={local_addr}, advertised={}:{}; {} bytes",
        config.public_host,
        config.public_port,
        packet.len()
    );

    match parse_stun(packet) {
        Ok(Some(stun)) => {
            println!(
                "STUN message type=0x{:04x}, transaction_id={}, username={}",
                stun.message_type,
                stun.transaction_id,
                stun.username.as_deref().unwrap_or("<none>")
            );
        }
        Ok(None) => println!("Payload is not a recognized STUN packet."),
        Err(error) => println!("Malformed STUN packet: {error}"),
    }

    let preview = packet
        .iter()
        .take(MAX_LOGGED_BYTES)
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ");
    println!(
        "payload (first {} bytes, hex): {preview}{}",
        packet.len().min(MAX_LOGGED_BYTES),
        if packet.len() > MAX_LOGGED_BYTES {
            " ..."
        } else {
            ""
        }
    );
}

/// parses the STUN header and username for diagnostic output.
fn parse_stun(packet: &[u8]) -> Result<Option<StunSummary>, &'static str> {
    if packet.len() < STUN_HEADER_LEN
        || packet[0] & 0xc0 != 0
        || u32::from_be_bytes(packet[4..8].try_into().unwrap()) != STUN_MAGIC_COOKIE
    {
        return Ok(None);
    }

    let message_len = usize::from(u16::from_be_bytes([packet[2], packet[3]]));
    if message_len % 4 != 0 || STUN_HEADER_LEN + message_len > packet.len() {
        return Err("invalid STUN message length");
    }

    let mut username = None;
    let mut offset = STUN_HEADER_LEN;
    let end = STUN_HEADER_LEN + message_len;
    while offset < end {
        if end - offset < 4 {
            return Err("truncated STUN attribute header");
        }
        let attribute_type = u16::from_be_bytes([packet[offset], packet[offset + 1]]);
        let attribute_len =
            usize::from(u16::from_be_bytes([packet[offset + 2], packet[offset + 3]]));
        let value_start = offset + 4;
        let value_end = value_start
            .checked_add(attribute_len)
            .ok_or("STUN attribute length overflow")?;
        let padded_end = value_start
            .checked_add((attribute_len + 3) & !3)
            .ok_or("STUN attribute length overflow")?;
        if value_end > end || padded_end > end {
            return Err("STUN attribute exceeds message length");
        }

        if attribute_type == ATTR_USERNAME {
            username = Some(
                std::str::from_utf8(&packet[value_start..value_end])
                    .map_err(|_| "STUN USERNAME is not UTF-8")?
                    .to_owned(),
            );
        }
        offset = padded_end;
    }

    let transaction_id = packet[8..STUN_HEADER_LEN]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(Some(StunSummary {
        message_type: u16::from_be_bytes([packet[0], packet[1]]),
        transaction_id,
        username,
    }))
}
