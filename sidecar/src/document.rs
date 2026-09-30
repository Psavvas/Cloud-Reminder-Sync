//! Read Apple's versioned topotext String documents, matching the former Python reader.
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;
use std::io::Read;

const MAX_DOCUMENT: usize = 4 * 1024 * 1024;

pub fn text(record: &Value, name: &str) -> Option<String> {
    let value = crate::cloudkit::field(record, name)?.as_str()?;
    let result = decode(value);
    if result.is_none() {
        eprintln!("sync: could not decode {name}");
    }
    result
}

// Encode the same versioned topotext structure used by Apple's web client.
// Lengths in CRDT metadata count UTF-16 code units, including surrogate pairs.
pub fn encode(text: &str) -> crate::error::Result<String> {
    use std::io::Write;
    fn number(out: &mut Vec<u8>, field: u64, value: u64) {
        put(out, field << 3);
        put(out, value);
    }
    fn put(out: &mut Vec<u8>, mut n: u64) {
        while n >= 128 {
            out.push((n as u8 & 127) | 128);
            n >>= 7;
        }
        out.push(n as u8);
    }
    fn bytes(out: &mut Vec<u8>, field: u64, value: &[u8]) {
        put(out, (field << 3) | 2);
        put(out, value.len() as u64);
        out.extend_from_slice(value);
    }
    fn substring(replica: u64, clock: u64, len: u64, child: Option<u64>) -> Vec<u8> {
        let mut id = Vec::new();
        number(&mut id, 1, replica);
        number(&mut id, 2, clock);
        let mut out = Vec::new();
        bytes(&mut out, 1, &id);
        number(&mut out, 2, len);
        bytes(&mut out, 3, &id);
        if let Some(child) = child {
            number(&mut out, 5, child);
        }
        out
    }
    let len = text.encode_utf16().count() as u64;
    let mut string = Vec::new();
    bytes(&mut string, 2, text.as_bytes());
    bytes(&mut string, 3, &substring(0, 0, 0, Some(1)));
    if len > 0 {
        bytes(&mut string, 3, &substring(1, 0, len, Some(2)));
    }
    bytes(&mut string, 3, &substring(0, u32::MAX as u64, 0, None));
    let mut clock = Vec::new();
    bytes(&mut clock, 1, uuid::Uuid::new_v4().as_bytes());
    for length in [len, 1] {
        let mut replica = Vec::new();
        number(&mut replica, 1, length);
        bytes(&mut clock, 2, &replica);
    }
    let mut timestamp = Vec::new();
    bytes(&mut timestamp, 1, &clock);
    bytes(&mut string, 4, &timestamp);
    if len > 0 {
        let mut run = Vec::new();
        number(&mut run, 1, len);
        bytes(&mut string, 5, &run);
    }
    let mut version = Vec::new();
    number(&mut version, 1, 0);
    number(&mut version, 2, 0);
    bytes(&mut version, 3, &string);
    let mut document = Vec::new();
    number(&mut document, 1, 0);
    bytes(&mut document, 2, &version);
    let mut compressed =
        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    let encoding_error = |e: std::io::Error| {
        crate::error::AppError::internal("Could not encode reminder text", e.to_string())
    };
    compressed.write_all(&document).map_err(encoding_error)?;
    Ok(STANDARD.encode(compressed.finish().map_err(encoding_error)?))
}

fn decode(value: &str) -> Option<String> {
    if value.len() > MAX_DOCUMENT * 2 {
        return None;
    }
    let padded = format!("{}{}", value, "=".repeat((4 - value.len() % 4) % 4));
    let bytes = STANDARD.decode(padded).ok()?;
    let mut data = Vec::new();
    let reader: Box<dyn Read + '_> = if bytes.starts_with(&[0x1f, 0x8b]) {
        Box::new(flate2::read::GzDecoder::new(bytes.as_slice()))
    } else if bytes.first() == Some(&0x78) {
        Box::new(flate2::read::ZlibDecoder::new(bytes.as_slice()))
    } else {
        Box::new(bytes.as_slice())
    };
    reader
        .take((MAX_DOCUMENT + 1) as u64)
        .read_to_end(&mut data)
        .ok()?;
    if data.len() > MAX_DOCUMENT {
        return None;
    }
    // Document.version[0] (2) -> Version.data (3) -> String.string (2).
    let document = field(&data, 2)
        .and_then(|v| field(v, 3))
        .and_then(|s| field(s, 2));
    let version = || field(&data, 3).and_then(|s| field(s, 2));
    let raw = document.or_else(version).or_else(|| field(&data, 2))?;
    String::from_utf8(raw.to_vec()).ok()
}

// Skip unknown protobuf fields without interpreting CRDT metadata. All lengths
// and varints are checked; no recursive parsing or allocation from wire lengths.
fn varint(data: &[u8], pos: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let b = *data.get(*pos)?;
        *pos += 1;
        if shift == 63 && b > 1 {
            return None;
        }
        value |= u64::from(b & 127) << shift;
        if b & 128 == 0 {
            return Some(value);
        }
    }
    None
}
fn field(data: &[u8], wanted: u64) -> Option<&[u8]> {
    let mut pos = 0;
    while pos < data.len() {
        let tag = varint(data, &mut pos)?;
        if tag >> 3 == 0 {
            return None;
        }
        match tag & 7 {
            0 => {
                varint(data, &mut pos)?;
            }
            1 => {
                pos = pos.checked_add(8)?;
            }
            2 => {
                let length = usize::try_from(varint(data, &mut pos)?).ok()?;
                let end = pos.checked_add(length)?;
                let value = data.get(pos..end)?;
                if tag >> 3 == wanted {
                    return Some(value);
                }
                pos = end;
            }
            5 => {
                pos = pos.checked_add(4)?;
            }
            _ => return None,
        }
        if pos > data.len() {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn encodes_unicode_empty_and_long_documents() {
        for text in ["", "Hello", "Flight ✈️ 🚀", &"notes ".repeat(1000)] {
            let encoded = encode(text).unwrap();
            assert_eq!(decode(&encoded).as_deref(), Some(text));
            let compressed = STANDARD.decode(encoded).unwrap();
            let mut raw = Vec::new();
            flate2::read::ZlibDecoder::new(compressed.as_slice())
                .read_to_end(&mut raw)
                .unwrap();
            let string = field(field(&raw, 2).unwrap(), 3).unwrap();
            if !text.is_empty() {
                let run = field(string, 5).unwrap();
                let mut position = 1;
                assert_eq!(
                    varint(run, &mut position),
                    Some(text.encode_utf16().count() as u64)
                );
            }
        }
    }

    #[test]
    fn reads_versioned_gzip_zlib_and_legacy_strings() {
        let bare = b"\x12\x05Hello";
        let version = [b"\x08\x01\x1a\x07".as_slice(), bare].concat();
        let document = [b"\x08\x01\x12\x0b".as_slice(), &version].concat();
        for raw in [bare.as_slice(), &version, &document] {
            assert_eq!(decode(&STANDARD.encode(raw)).as_deref(), Some("Hello"));
            let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            z.write_all(raw).unwrap();
            assert_eq!(
                decode(STANDARD.encode(z.finish().unwrap()).trim_end_matches('=')).as_deref(),
                Some("Hello")
            );
            let mut g = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            g.write_all(raw).unwrap();
            assert_eq!(
                decode(&STANDARD.encode(g.finish().unwrap())).as_deref(),
                Some("Hello")
            );
        }
    }
    #[test]
    fn rejects_truncated_and_oversized_documents() {
        assert_eq!(decode("!invalid"), None);
        assert_eq!(decode(&STANDARD.encode(b"\x12\xff\xff")), None);
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&vec![0; MAX_DOCUMENT + 1]).unwrap();
        assert_eq!(decode(&STANDARD.encode(z.finish().unwrap())), None);
    }
}
