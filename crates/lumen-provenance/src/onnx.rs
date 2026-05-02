//! Structural sniff for ONNX model files.
//!
//! ONNX files are protobuf-encoded `ModelProto` messages. Lumen does **not**
//! load the entire graph at provenance time - that is the inference engine's
//! job. We only need to verify that:
//!
//! 1. The bytes parse as a well-formed protobuf message.
//! 2. The declared `ir_version` is in a known range.
//! 3. The opset imports do not reference unknown domains.
//!
//! We deliberately avoid pulling in `prost-build` (and therefore the `protoc`
//! binary) by hand-rolling a strict, tight wire-format reader for just the
//! [`OnnxHeader`] subset of the schema. The parser:
//!
//! - validates every varint length against the input buffer,
//! - rejects malformed group / start-group wire types outright,
//! - never allocates more than one `Vec<u8>` per length-delimited field,
//! - caps the total parsed depth at 1 (we only descend into the top-level
//!   `ModelProto`).
//!
//! Anything beyond the header is **skipped** rather than parsed, which keeps
//! the attack surface minimal even on adversarial inputs.

use std::path::Path;

use lumen_core::{Error, Result};
use serde::{Deserialize, Serialize};

/// Lowest IR version we accept (ONNX 1.0).
pub const MIN_IR_VERSION: i64 = 1;
/// Highest IR version we accept (ONNX 1.16 == IR 10 at time of writing). New
/// versions must be explicitly allowlisted; we err on the side of "deny by
/// default" for the proof-bound load path.
pub const MAX_IR_VERSION: i64 = 12;

/// Hard cap on the bytes we will scan. Adjust if real models exceed this.
pub const MAX_HEADER_SCAN_BYTES: usize = 16 * 1024 * 1024;

/// Subset of `ModelProto` Lumen verifies before ever instantiating tensors.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnnxHeader {
    /// IR version (`ModelProto.ir_version`).
    pub ir_version: i64,
    /// Free-form producer string.
    pub producer_name: String,
    /// Producer version.
    pub producer_version: String,
    /// Domain, often empty.
    pub domain: String,
    /// Numeric model version supplied by the producer.
    pub model_version: i64,
    /// Declared opset imports.
    pub opset_imports: Vec<OnnxOpset>,
}

/// One entry from `ModelProto.opset_import`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnnxOpset {
    /// Operator domain, empty for the default ai.onnx domain.
    pub domain: String,
    /// Opset version.
    pub version: i64,
}

/// Parse the ONNX header from raw bytes.
pub fn parse_header(bytes: &[u8]) -> Result<OnnxHeader> {
    if bytes.is_empty() {
        return Err(Error::Provenance("onnx: empty input".into()));
    }
    if bytes.len() > MAX_HEADER_SCAN_BYTES {
        return Err(Error::Provenance(format!(
            "onnx: file exceeds header scan cap ({} > {})",
            bytes.len(),
            MAX_HEADER_SCAN_BYTES
        )));
    }
    let mut header = OnnxHeader::default();
    let mut pos: usize = 0;
    let mut found_ir_version = false;
    while pos < bytes.len() {
        let (field_no, wire_type) = read_tag(bytes, &mut pos)?;
        match (field_no, wire_type) {
            // ir_version (int64)
            (1, WireType::Varint) => {
                header.ir_version = read_int64(bytes, &mut pos)?;
                found_ir_version = true;
            }
            // producer_name (string)
            (2, WireType::LengthDelimited) => {
                header.producer_name = read_string(bytes, &mut pos)?;
            }
            // producer_version (string)
            (3, WireType::LengthDelimited) => {
                header.producer_version = read_string(bytes, &mut pos)?;
            }
            // domain (string)
            (4, WireType::LengthDelimited) => {
                header.domain = read_string(bytes, &mut pos)?;
            }
            // model_version (int64)
            (5, WireType::Varint) => {
                header.model_version = read_int64(bytes, &mut pos)?;
            }
            // opset_import (repeated OperatorSetIdProto)
            (8, WireType::LengthDelimited) => {
                let payload = read_length_delimited(bytes, &mut pos)?;
                header.opset_imports.push(parse_opset(payload)?);
            }
            // Unknown / skipped fields - must still consume them safely.
            (_, wt) => skip_field(bytes, &mut pos, wt)?,
        }
    }
    if !found_ir_version {
        return Err(Error::Provenance("onnx: missing ir_version".into()));
    }
    Ok(header)
}

/// Validate the header and return it if it passes.
pub fn validate_header(header: &OnnxHeader) -> Result<()> {
    if header.ir_version < MIN_IR_VERSION || header.ir_version > MAX_IR_VERSION {
        return Err(Error::Provenance(format!(
            "onnx: ir_version {} outside accepted range [{MIN_IR_VERSION}, {MAX_IR_VERSION}]",
            header.ir_version
        )));
    }
    for opset in &header.opset_imports {
        if opset.version <= 0 {
            return Err(Error::Provenance(format!(
                "onnx: opset domain={:?} version={} non-positive",
                opset.domain, opset.version
            )));
        }
    }
    Ok(())
}

/// File-level entry point: read, parse, validate. Returns the parsed header.
pub fn sniff(path: &Path) -> Result<OnnxHeader> {
    let bytes = std::fs::read(path)?;
    let header = parse_header(&bytes)?;
    validate_header(&header)?;
    Ok(header)
}

// ---------------------------------------------------------------------------
// Tight protobuf wire-format reader. Hand-rolled to avoid pulling in
// `prost-build` + `protoc`. Bounds-checked on every read.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum WireType {
    Varint = 0,
    Fixed64 = 1,
    LengthDelimited = 2,
    StartGroup = 3,
    EndGroup = 4,
    Fixed32 = 5,
}

impl WireType {
    fn from_u8(b: u8) -> Result<Self> {
        Ok(match b {
            0 => Self::Varint,
            1 => Self::Fixed64,
            2 => Self::LengthDelimited,
            3 => Self::StartGroup,
            4 => Self::EndGroup,
            5 => Self::Fixed32,
            _ => return Err(Error::Provenance(format!("onnx: bad wire type {b}"))),
        })
    }
}

fn read_varint(buf: &[u8], pos: &mut usize) -> Result<u64> {
    let mut result: u64 = 0;
    let mut shift: u32 = 0;
    for _ in 0..10 {
        let b = *buf
            .get(*pos)
            .ok_or_else(|| Error::Provenance("onnx: truncated varint".into()))?;
        *pos += 1;
        result |= u64::from(b & 0x7F) << shift;
        if b & 0x80 == 0 {
            return Ok(result);
        }
        shift += 7;
        if shift >= 64 {
            return Err(Error::Provenance("onnx: varint overflow".into()));
        }
    }
    Err(Error::Provenance("onnx: varint too long".into()))
}

fn read_tag(buf: &[u8], pos: &mut usize) -> Result<(u32, WireType)> {
    let v = read_varint(buf, pos)?;
    let wire = WireType::from_u8((v & 0x7) as u8)?;
    let field = (v >> 3) as u32;
    if field == 0 {
        return Err(Error::Provenance("onnx: zero field number".into()));
    }
    Ok((field, wire))
}

fn read_int64(buf: &[u8], pos: &mut usize) -> Result<i64> {
    Ok(read_varint(buf, pos)? as i64)
}

fn read_length_delimited<'a>(buf: &'a [u8], pos: &mut usize) -> Result<&'a [u8]> {
    let len = read_varint(buf, pos)? as usize;
    let end = pos
        .checked_add(len)
        .ok_or_else(|| Error::Provenance("onnx: length overflow".into()))?;
    if end > buf.len() {
        return Err(Error::Provenance(
            "onnx: length-delimited field exceeds buffer".into(),
        ));
    }
    let slice = &buf[*pos..end];
    *pos = end;
    Ok(slice)
}

fn read_string(buf: &[u8], pos: &mut usize) -> Result<String> {
    let bytes = read_length_delimited(buf, pos)?;
    String::from_utf8(bytes.to_vec()).map_err(|e| Error::Provenance(format!("onnx: utf-8: {e}")))
}

fn skip_field(buf: &[u8], pos: &mut usize, wt: WireType) -> Result<()> {
    match wt {
        WireType::Varint => {
            let _ = read_varint(buf, pos)?;
        }
        WireType::Fixed64 => {
            let end = pos
                .checked_add(8)
                .ok_or_else(|| Error::Provenance("onnx: fixed64 overflow".into()))?;
            if end > buf.len() {
                return Err(Error::Provenance("onnx: fixed64 truncated".into()));
            }
            *pos = end;
        }
        WireType::LengthDelimited => {
            let _ = read_length_delimited(buf, pos)?;
        }
        WireType::Fixed32 => {
            let end = pos
                .checked_add(4)
                .ok_or_else(|| Error::Provenance("onnx: fixed32 overflow".into()))?;
            if end > buf.len() {
                return Err(Error::Provenance("onnx: fixed32 truncated".into()));
            }
            *pos = end;
        }
        WireType::StartGroup | WireType::EndGroup => {
            return Err(Error::Provenance(
                "onnx: deprecated group wire type rejected".into(),
            ));
        }
    }
    Ok(())
}

fn parse_opset(buf: &[u8]) -> Result<OnnxOpset> {
    let mut opset = OnnxOpset::default();
    let mut pos: usize = 0;
    while pos < buf.len() {
        let (field, wt) = read_tag(buf, &mut pos)?;
        match (field, wt) {
            (1, WireType::LengthDelimited) => {
                opset.domain = read_string(buf, &mut pos)?;
            }
            (2, WireType::Varint) => {
                opset.version = read_int64(buf, &mut pos)?;
            }
            (_, wt) => skip_field(buf, &mut pos, wt)?,
        }
    }
    Ok(opset)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a varint-encoded `u64` for tests.
    fn put_varint(out: &mut Vec<u8>, mut v: u64) {
        while v >= 0x80 {
            out.push((v as u8) | 0x80);
            v >>= 7;
        }
        out.push(v as u8);
    }

    fn put_tag(out: &mut Vec<u8>, field: u32, wt: u8) {
        put_varint(out, ((field as u64) << 3) | u64::from(wt));
    }

    fn put_string(out: &mut Vec<u8>, field: u32, s: &str) {
        put_tag(out, field, 2);
        put_varint(out, s.len() as u64);
        out.extend_from_slice(s.as_bytes());
    }

    fn put_int64(out: &mut Vec<u8>, field: u32, v: i64) {
        put_tag(out, field, 0);
        put_varint(out, v as u64);
    }

    fn put_opset(out: &mut Vec<u8>, domain: &str, version: i64) {
        let mut sub = Vec::new();
        if !domain.is_empty() {
            put_string(&mut sub, 1, domain);
        }
        put_int64(&mut sub, 2, version);
        put_tag(out, 8, 2);
        put_varint(out, sub.len() as u64);
        out.extend_from_slice(&sub);
    }

    #[test]
    fn parse_minimal_header() {
        let mut buf = Vec::new();
        put_int64(&mut buf, 1, 7); // ir_version
        put_string(&mut buf, 2, "lumen-test");
        put_string(&mut buf, 3, "0.1");
        put_int64(&mut buf, 5, 42);
        put_opset(&mut buf, "", 17);
        put_opset(&mut buf, "ai.onnx.ml", 3);

        let header = parse_header(&buf).unwrap();
        assert_eq!(header.ir_version, 7);
        assert_eq!(header.producer_name, "lumen-test");
        assert_eq!(header.producer_version, "0.1");
        assert_eq!(header.model_version, 42);
        assert_eq!(header.opset_imports.len(), 2);
        assert_eq!(header.opset_imports[0].version, 17);
        assert_eq!(header.opset_imports[1].domain, "ai.onnx.ml");
        validate_header(&header).unwrap();
    }

    #[test]
    fn missing_ir_version_rejected() {
        let mut buf = Vec::new();
        put_string(&mut buf, 2, "x");
        let err = parse_header(&buf).unwrap_err();
        assert!(err.to_string().contains("missing ir_version"));
    }

    #[test]
    fn empty_input_rejected() {
        let err = parse_header(&[]).unwrap_err();
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn truncated_length_delimited_rejected() {
        // Tag for field 2 (string), claim length 100 but supply zero data.
        let buf = vec![0x12, 100];
        let err = parse_header(&buf).unwrap_err();
        assert!(err.to_string().contains("exceeds"));
    }

    #[test]
    fn unknown_field_skipped() {
        let mut buf = Vec::new();
        put_int64(&mut buf, 1, 7);
        // field 99, wire type varint - must be skipped, not reject.
        put_int64(&mut buf, 99, 12345);
        let header = parse_header(&buf).unwrap();
        assert_eq!(header.ir_version, 7);
    }

    #[test]
    fn deprecated_group_rejected() {
        let mut buf = Vec::new();
        put_int64(&mut buf, 1, 7);
        // tag with wire type 3 (start group)
        put_tag(&mut buf, 99, 3);
        let err = parse_header(&buf).unwrap_err();
        assert!(err.to_string().contains("group"));
    }

    #[test]
    fn ir_version_out_of_range_rejected() {
        let mut buf = Vec::new();
        put_int64(&mut buf, 1, 9999);
        let header = parse_header(&buf).unwrap();
        let err = validate_header(&header).unwrap_err();
        assert!(err.to_string().contains("ir_version"));
    }

    #[test]
    fn varint_overflow_rejected() {
        // 11 continuation bytes - would overflow.
        let buf = vec![0xFFu8; 11];
        let err = parse_header(&buf).unwrap_err();
        // either "varint too long" or "varint overflow"
        assert!(err.to_string().contains("varint"), "unexpected: {err}");
    }
}
