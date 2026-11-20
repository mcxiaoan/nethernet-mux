//! parses ICE binding requests and verifies their MESSAGE-INTEGRITY attribute.

use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;

// defines the fixed STUN header and ICE attributes used for authentication.
const HEADER_LEN: usize = 20;
const MAGIC_COOKIE: u32 = 0x2112_a442;
const ATTR_USERNAME: u16 = 0x0006;
const ATTR_MESSAGE_INTEGRITY: u16 = 0x0008;

/// borrows the fields needed to identify and authenticate a binding request.
struct BindingRequest<'a> {
    username: &'a str,
    integrity_offset: usize,
    integrity: &'a [u8],
}

/// validates a binding request and extracts its username and integrity value.
fn parse_binding_request(packet: &[u8]) -> Option<BindingRequest<'_>> {
    if packet.len() < HEADER_LEN
        || packet[0..2] != [0, 1]
        || u32::from_be_bytes(packet[4..8].try_into().ok()?) != MAGIC_COOKIE
    {
        return None;
    }

    let message_len = usize::from(u16::from_be_bytes(packet[2..4].try_into().ok()?));
    let end = HEADER_LEN.checked_add(message_len)?;
    if message_len % 4 != 0 || end > packet.len() {
        return None;
    }

    let mut username = None;
    let mut integrity = None;
    let mut offset = HEADER_LEN;
    while offset + 4 <= end {
        let attr_type = u16::from_be_bytes(packet[offset..offset + 2].try_into().ok()?);
        let attr_len = usize::from(u16::from_be_bytes(
            packet[offset + 2..offset + 4].try_into().ok()?,
        ));
        let value_start = offset + 4;
        let value_end = value_start.checked_add(attr_len)?;
        if value_end > end {
            return None;
        }

        if attr_type == ATTR_USERNAME && attr_len > 0 {
            username = std::str::from_utf8(&packet[value_start..value_end]).ok();
        } else if attr_type == ATTR_MESSAGE_INTEGRITY && attr_len == 20 {
            integrity = Some((offset, &packet[value_start..value_end]));
            break;
        }
        offset = value_start.checked_add((attr_len + 3) & !3)?;
    }

    let (integrity_offset, integrity) = integrity?;
    Some(BindingRequest {
        username: username?,
        integrity_offset,
        integrity,
    })
}

/// returns the username from a valid binding request.
pub fn username(packet: &[u8]) -> Option<&str> {
    parse_binding_request(packet).map(|request| request.username)
}

/// verifies MESSAGE-INTEGRITY using the ICE password without changing the packet.
pub fn verify_message_integrity(packet: &[u8], password: &str) -> bool {
    let Some(request) = parse_binding_request(packet) else {
        return false;
    };
    let integrity_end = request.integrity_offset.checked_add(24);
    let Some(message_len) = integrity_end
        .and_then(|end| end.checked_sub(HEADER_LEN))
        .and_then(|length| u16::try_from(length).ok())
    else {
        return false;
    };

    let mut message = packet[..request.integrity_offset].to_vec();
    message[2..4].copy_from_slice(&message_len.to_be_bytes());
    let Ok(mut mac) = <Hmac<Sha1> as KeyInit>::new_from_slice(password.as_bytes()) else {
        return false;
    };
    mac.update(&message);
    mac.verify_slice(request.integrity).is_ok()
}
