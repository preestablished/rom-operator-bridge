use crate::private_config::RealRuntimeConfig;
use dh_proto::v1 as dh;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Debug, Clone)]
pub struct ResolvedCaptureSpec {
    pub spec: dh::CaptureSpec,
    pub layout_hash: String,
    pub capture_spec_hash: String,
    pub map_hash: String,
    pub total_len: u64,
    decoders: Vec<FeatureDecoder>,
}

impl ResolvedCaptureSpec {
    pub fn decode_values(
        &self,
        feature_bytes: &[u8],
    ) -> Result<DecodedFeatureValues, CaptureSpecError> {
        let expected_len = usize::try_from(self.total_len).map_err(|_| CaptureSpecError)?;
        if feature_bytes.len() != expected_len {
            return Err(CaptureSpecError);
        }

        let mut offset = 0usize;
        let mut order = Vec::with_capacity(self.decoders.len());
        let mut values = Vec::with_capacity(self.decoders.len());
        for decoder in &self.decoders {
            let len = decoder.kind.width();
            let end = offset.checked_add(len).ok_or(CaptureSpecError)?;
            let bytes = feature_bytes.get(offset..end).ok_or(CaptureSpecError)?;
            order.push(decoder.name.clone());
            values.push(decoder.kind.decode(bytes)?);
            offset = end;
        }
        Ok(DecodedFeatureValues { order, values })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedFeatureValues {
    pub order: Vec<String>,
    pub values: Vec<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureSpecError;

pub fn resolve_capture_spec(
    runtime_config: &RealRuntimeConfig,
) -> Result<ResolvedCaptureSpec, CaptureSpecError> {
    let bundle = runtime_config.reference_workload_checkout();
    let layout_path = bundle.join("layout.json");
    let map_path = bundle.join("feature-map.yaml");
    let layout: LayoutFile = read_json(layout_path)?;
    let feature_map: FeatureMapFile = read_yaml(map_path)?;

    if layout.total_len == 0
        || layout.ranges.is_empty()
        || feature_map.schema_version != 1
        || feature_map.kind != "feature-map"
        || feature_map.features.is_empty()
        || feature_map.features.len() != layout.ranges.len()
        || !valid_blake3_ref(&layout.blake3)
    {
        return Err(CaptureSpecError);
    }

    let computed_map_hash = blake3_ref(feature_map.raw.as_bytes());
    if layout.compiled_from_feature_map_hash.as_deref() != Some(&computed_map_hash) {
        return Err(CaptureSpecError);
    }

    let capture_spec_hash = layout.capture_spec_hash.clone().ok_or(CaptureSpecError)?;
    if !looks_hash_or_ref(&capture_spec_hash)
        || capture_spec_hash != runtime_config.capture_spec_ref()
        || layout.compiler_or_exporter_commit.trim().is_empty()
    {
        return Err(CaptureSpecError);
    }

    let mut total_len = 0u64;
    let mut proto_ranges = Vec::with_capacity(layout.ranges.len());
    for range in &layout.ranges {
        if range.region.trim().is_empty() || range.layout_version == 0 || range.len == 0 {
            return Err(CaptureSpecError);
        }
        total_len = total_len.checked_add(range.len).ok_or(CaptureSpecError)?;
        proto_ranges.push(dh::ExtractRange {
            region: range.region.clone(),
            layout_version: u32::try_from(range.layout_version).map_err(|_| CaptureSpecError)?,
            offset: range.offset,
            len: u32::try_from(range.len).map_err(|_| CaptureSpecError)?,
        });
    }
    if total_len != layout.total_len {
        return Err(CaptureSpecError);
    }

    let mut decoded_len = 0u64;
    let mut decoders = Vec::with_capacity(feature_map.features.len());
    for (feature, range) in feature_map.features.iter().zip(layout.ranges.iter()) {
        if feature.name.trim().is_empty()
            || feature.region != range.region
            || feature.offset != range.offset
        {
            return Err(CaptureSpecError);
        }
        let kind = FeatureKind::parse(&feature.feature_type, feature.width)?;
        if kind.width() as u64 != range.len {
            return Err(CaptureSpecError);
        }
        decoded_len = decoded_len
            .checked_add(kind.width() as u64)
            .ok_or(CaptureSpecError)?;
        decoders.push(FeatureDecoder {
            name: feature.name.clone(),
            kind,
        });
    }
    if decoded_len != layout.total_len {
        return Err(CaptureSpecError);
    }

    let layout_hash = compute_layout_hash(
        &layout.ranges,
        layout.total_len,
        &computed_map_hash,
        &capture_spec_hash,
        &layout.compiler_or_exporter_commit,
    )?;
    if layout_hash != layout.blake3 {
        return Err(CaptureSpecError);
    }

    Ok(ResolvedCaptureSpec {
        spec: dh::CaptureSpec {
            ranges: proto_ranges,
            framebuffer: true,
        },
        layout_hash,
        capture_spec_hash,
        map_hash: computed_map_hash,
        total_len: layout.total_len,
        decoders,
    })
}

fn compute_layout_hash(
    ranges: &[LayoutRange],
    total_len: u64,
    map_hash: &str,
    capture_spec_hash: &str,
    compiler_or_exporter_commit: &str,
) -> Result<String, CaptureSpecError> {
    let preimage = serde_json::json!({
        "ranges": ranges,
        "total_len": total_len,
        "compiled_from_feature_map_hash": map_hash,
        "capture_spec_hash": capture_spec_hash,
        "compiler_or_exporter_commit": compiler_or_exporter_commit,
    });
    let bytes = serde_json::to_vec(&preimage).map_err(|_| CaptureSpecError)?;
    Ok(blake3_ref(&bytes))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: PathBuf) -> Result<T, CaptureSpecError> {
    let bytes = fs::read(path).map_err(|_| CaptureSpecError)?;
    serde_json::from_slice(&bytes).map_err(|_| CaptureSpecError)
}

fn read_yaml(path: PathBuf) -> Result<FeatureMapFile, CaptureSpecError> {
    let raw = fs::read_to_string(path).map_err(|_| CaptureSpecError)?;
    let mut parsed: FeatureMapFile = serde_yaml::from_str(&raw).map_err(|_| CaptureSpecError)?;
    parsed.raw = raw;
    Ok(parsed)
}

#[derive(Debug, Deserialize)]
struct LayoutFile {
    ranges: Vec<LayoutRange>,
    total_len: u64,
    blake3: String,
    compiled_from_feature_map_hash: Option<String>,
    capture_spec_hash: Option<String>,
    compiler_or_exporter_commit: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct LayoutRange {
    region: String,
    layout_version: u64,
    offset: u64,
    len: u64,
}

#[derive(Debug, Deserialize)]
struct FeatureMapFile {
    schema_version: u32,
    kind: String,
    features: Vec<FeatureEntry>,
    #[serde(skip)]
    raw: String,
}

#[derive(Debug, Deserialize)]
struct FeatureEntry {
    name: String,
    region: String,
    offset: u64,
    #[serde(rename = "type")]
    feature_type: String,
    #[serde(default)]
    width: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FeatureDecoder {
    name: String,
    kind: FeatureKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeatureKind {
    U8,
    U16le,
    U16be,
    U32le,
    U32be,
    I8,
    I16le,
    I16be,
    I32le,
    I32be,
    Bitflags8,
    Bitflags16le,
    Bitflags32le,
    Bcd8,
    Bcd16le,
}

impl FeatureKind {
    fn parse(value: &str, width: Option<u32>) -> Result<Self, CaptureSpecError> {
        let kind = match value {
            "u8" => Self::U8,
            "u16le" => Self::U16le,
            "u16be" => Self::U16be,
            "u32le" => Self::U32le,
            "u32be" => Self::U32be,
            "i8" => Self::I8,
            "i16le" => Self::I16le,
            "i16be" => Self::I16be,
            "i32le" => Self::I32le,
            "i32be" => Self::I32be,
            "bitflags8" => Self::Bitflags8,
            "bitflags16le" => Self::Bitflags16le,
            "bitflags32le" => Self::Bitflags32le,
            "bcd8" => Self::Bcd8,
            "bcd16le" => Self::Bcd16le,
            _ => return Err(CaptureSpecError),
        };
        if width.is_some_and(|width| width != kind.width() as u32) {
            return Err(CaptureSpecError);
        }
        Ok(kind)
    }

    fn width(self) -> usize {
        match self {
            Self::U8 | Self::I8 | Self::Bitflags8 | Self::Bcd8 => 1,
            Self::U16le
            | Self::U16be
            | Self::I16le
            | Self::I16be
            | Self::Bitflags16le
            | Self::Bcd16le => 2,
            Self::U32le | Self::U32be | Self::I32le | Self::I32be | Self::Bitflags32le => 4,
        }
    }

    fn decode(self, bytes: &[u8]) -> Result<i64, CaptureSpecError> {
        match self {
            Self::U8 | Self::Bitflags8 => Ok(i64::from(bytes[0])),
            Self::U16le | Self::Bitflags16le => {
                Ok(i64::from(u16::from_le_bytes([bytes[0], bytes[1]])))
            }
            Self::U16be => Ok(i64::from(u16::from_be_bytes([bytes[0], bytes[1]]))),
            Self::U32le | Self::Bitflags32le => Ok(i64::from(u32::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3],
            ]))),
            Self::U32be => Ok(i64::from(u32::from_be_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3],
            ]))),
            Self::I8 => Ok(i64::from(i8::from_ne_bytes([bytes[0]]))),
            Self::I16le => Ok(i64::from(i16::from_le_bytes([bytes[0], bytes[1]]))),
            Self::I16be => Ok(i64::from(i16::from_be_bytes([bytes[0], bytes[1]]))),
            Self::I32le => Ok(i64::from(i32::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3],
            ]))),
            Self::I32be => Ok(i64::from(i32::from_be_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3],
            ]))),
            Self::Bcd8 => decode_bcd(bytes),
            Self::Bcd16le => decode_bcd(&[bytes[0], bytes[1]]),
        }
    }
}

fn decode_bcd(bytes: &[u8]) -> Result<i64, CaptureSpecError> {
    let mut factor = 1i64;
    let mut value = 0i64;
    for byte in bytes {
        let low = byte & 0x0f;
        let high = byte >> 4;
        if low > 9 || high > 9 {
            return Err(CaptureSpecError);
        }
        value += i64::from(low) * factor;
        factor *= 10;
        value += i64::from(high) * factor;
        factor *= 10;
    }
    Ok(value)
}

fn valid_blake3_ref(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("blake3:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn looks_hash_or_ref(value: &str) -> bool {
    if let Some(hex) = value.strip_prefix("blake3:") {
        hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
    } else {
        !value.trim().is_empty()
    }
}

fn blake3_ref(bytes: &[u8]) -> String {
    format!("blake3:{}", blake3::hash(bytes).to_hex())
}
