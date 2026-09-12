//! Clean-room glyph decoder for Han Unified Font File 1.0.
//!
//! This module contains behavior-level interoperability code only. It does not embed
//! Hancom binaries, font data, or copied source code. The file is intentionally
//! standalone so it can be validated before being wired into the desktop font path.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutlineOp {
    MoveTo(Point),
    LineTo(Point),
    CubicTo {
        control1: Point,
        control2: Point,
        to: Point,
    },
    Close,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedOutline {
    pub ops: Vec<OutlineOp>,
    /// Number of bytes consumed through the terminating 0x00 command.
    pub consumed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    UnexpectedEof,
    MissingEnd,
    InvalidOpcode { opcode: u8, offset: usize },
    CoordinateOverflow,
    MalformedMetadata { offset: usize },
    InvalidHeader(&'static str),
    InvalidLocator(&'static str),
    InvalidBitmap(&'static str),
    GlyphIndexOutOfRange { index: usize, glyph_count: usize },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "unexpected end of HFT outline stream"),
            Self::MissingEnd => write!(f, "HFT outline stream has no terminating 0x00 command"),
            Self::InvalidOpcode { opcode, offset } => {
                write!(
                    f,
                    "invalid HFT outline opcode 0x{opcode:02x} at byte {offset}"
                )
            }
            Self::CoordinateOverflow => write!(f, "HFT outline coordinate overflow"),
            Self::MalformedMetadata { offset } => {
                write!(f, "malformed HFT outline metadata at byte {offset}")
            }
            Self::InvalidHeader(message) => write!(f, "invalid HFT header: {message}"),
            Self::InvalidLocator(message) => write!(f, "invalid HFT glyph locator: {message}"),
            Self::InvalidBitmap(message) => write!(f, "invalid HFT bitmap glyph: {message}"),
            Self::GlyphIndexOutOfRange { index, glyph_count } => write!(
                f,
                "HFT glyph locator produced index {index} outside glyph count {glyph_count}"
            ),
        }
    }
}

impl std::error::Error for DecodeError {}

pub const HFT_HEADER_SIZE: usize = 0x200;
pub const HFT_MAGIC: &[u8; 32] = b"Han Unified Font File 1.0\x1a\x04\x03\x02\x01\xff\x00";

/// Validate the fixed HFT 1.0 prefix and 512-byte header checksum.
pub fn validate_hft_header(bytes: &[u8]) -> Result<(), DecodeError> {
    if bytes.len() < HFT_HEADER_SIZE {
        return Err(DecodeError::InvalidHeader("file is shorter than 512 bytes"));
    }
    if bytes.get(..HFT_MAGIC.len()) != Some(HFT_MAGIC.as_slice()) {
        return Err(DecodeError::InvalidHeader("unexpected HFT 1.0 signature"));
    }
    let stored = u16::from_le_bytes([bytes[0x2a], bytes[0x2b]]);
    let mut sum = 0u16;
    for (index, &byte) in bytes[..HFT_HEADER_SIZE].iter().enumerate() {
        if index == 0x2a || index == 0x2b {
            continue;
        }
        sum = sum.wrapping_add(byte as u16);
    }
    if sum != stored {
        return Err(DecodeError::InvalidHeader("header checksum mismatch"));
    }
    Ok(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn position(&self) -> usize {
        self.pos
    }

    fn read_u8(&mut self) -> Result<u8, DecodeError> {
        let value = *self.bytes.get(self.pos).ok_or(DecodeError::UnexpectedEof)?;
        self.pos += 1;
        Ok(value)
    }

    fn read_i16_le(&mut self) -> Result<i16, DecodeError> {
        let lo = self.read_u8()?;
        let hi = self.read_u8()?;
        Ok(i16::from_le_bytes([lo, hi]))
    }

    fn skip(&mut self, count: usize) -> Result<(), DecodeError> {
        let end = self
            .pos
            .checked_add(count)
            .ok_or(DecodeError::UnexpectedEof)?;
        if end > self.bytes.len() {
            return Err(DecodeError::UnexpectedEof);
        }
        self.pos = end;
        Ok(())
    }

    /// HFT's compact signed integer coding.
    fn read_delta(&mut self) -> Result<i32, DecodeError> {
        let first = self.read_u8()? as i8 as i32;
        match first {
            -123..=123 => Ok(first),
            124..=127 => {
                let tail = self.read_u8()? as i32;
                Ok((first << 8) + tail - 31_620)
            }
            -127..=-124 => {
                let tail = self.read_u8()? as i32;
                Ok((first << 8) - tail + 31_620)
            }
            -128 => Ok(self.read_i16_le()? as i32),
            _ => unreachable!("i8 is fully covered by the ranges above"),
        }
    }
}

fn checked_add(lhs: i32, rhs: i32) -> Result<i32, DecodeError> {
    lhs.checked_add(rhs).ok_or(DecodeError::CoordinateOverflow)
}

/// Decode one raw HFT outline bytecode stream into absolute font-unit path operations.
///
/// The caller is responsible for locating the glyph record and removing its leading
/// little-endian `u16` byte length. If the font enables the optional H&C transform,
/// run [`decode_hnc_obfuscation_in_place`] on the bytecode bytes before this function.
pub fn decode_outline_stream(bytes: &[u8]) -> Result<DecodedOutline, DecodeError> {
    let mut reader = Reader::new(bytes);
    let mut ops = Vec::new();
    let mut current = Point { x: 0, y: 0 };
    let mut contour_start = current;
    let mut contour_open = false;

    loop {
        let opcode_offset = reader.position();
        let opcode = match reader.read_u8() {
            Ok(value) => value,
            Err(DecodeError::UnexpectedEof) => return Err(DecodeError::MissingEnd),
            Err(err) => return Err(err),
        };

        match opcode {
            // End / explicit close. Unlike the implicit close before a new MoveTo,
            // the original decoder restores the raw current point to contour start.
            0x00 | 0x04 => {
                if contour_open {
                    ops.push(OutlineOp::Close);
                    contour_open = false;
                    current = contour_start;
                }
                if opcode == 0x00 {
                    return Ok(DecodedOutline {
                        ops,
                        consumed: reader.position(),
                    });
                }
            }

            // MoveTo variants: bit 0 => first-axis delta, bit 1 => second-axis delta.
            // A new MoveTo implicitly closes an open contour, but HFT keeps the raw
            // delta accumulator at the previous endpoint for decoding the new move.
            0x01..=0x03 => {
                if contour_open {
                    ops.push(OutlineOp::Close);
                }
                let dx = if opcode & 0x01 != 0 {
                    reader.read_delta()?
                } else {
                    0
                };
                let dy = if opcode & 0x02 != 0 {
                    reader.read_delta()?
                } else {
                    0
                };
                current = Point {
                    x: checked_add(current.x, dx)?,
                    y: checked_add(current.y, dy)?,
                };
                contour_start = current;
                contour_open = true;
                ops.push(OutlineOp::MoveTo(current));
            }

            // LineTo variants use the same two optional delta bits.
            0x05..=0x07 => {
                let dx = if opcode & 0x01 != 0 {
                    reader.read_delta()?
                } else {
                    0
                };
                let dy = if opcode & 0x02 != 0 {
                    reader.read_delta()?
                } else {
                    0
                };
                current = Point {
                    x: checked_add(current.x, dx)?,
                    y: checked_add(current.y, dy)?,
                };
                contour_open = true;
                ops.push(OutlineOp::LineTo(current));
            }

            // CubicTo variants. The final pair uses the opposite presence bits:
            // byte order is opt(bit0), opt(bit1), required, required,
            // opt(bit1), opt(bit0). This compact rule is required for byte-exact
            // consumption of real HFT glyph streams.
            0x09..=0x0b => {
                let first_x = if opcode & 0x01 != 0 {
                    reader.read_delta()?
                } else {
                    0
                };
                let first_y = if opcode & 0x02 != 0 {
                    reader.read_delta()?
                } else {
                    0
                };
                let middle_x = reader.read_delta()?;
                let middle_y = reader.read_delta()?;
                let final_x = if opcode & 0x02 != 0 {
                    reader.read_delta()?
                } else {
                    0
                };
                let final_y = if opcode & 0x01 != 0 {
                    reader.read_delta()?
                } else {
                    0
                };

                let control1 = Point {
                    x: checked_add(current.x, first_x)?,
                    y: checked_add(current.y, first_y)?,
                };
                let control2 = Point {
                    x: checked_add(control1.x, middle_x)?,
                    y: checked_add(control1.y, middle_y)?,
                };
                let to = Point {
                    x: checked_add(control2.x, final_x)?,
                    y: checked_add(control2.y, final_y)?,
                };
                current = to;
                contour_open = true;
                ops.push(OutlineOp::CubicTo {
                    control1,
                    control2,
                    to,
                });
            }

            // Hint/metadata instructions. They do not alter the outline geometry,
            // but their exact byte lengths must be consumed to stay synchronized.
            0x20 => {
                let count_a = reader.read_u8()? as usize;
                reader.skip(
                    count_a
                        .checked_mul(2)
                        .ok_or(DecodeError::MalformedMetadata {
                            offset: opcode_offset,
                        })?,
                )?;
                let _tag = reader.read_u8()?;
                let count_b = reader.read_u8()? as usize;
                reader.skip(
                    count_b
                        .checked_mul(2)
                        .ok_or(DecodeError::MalformedMetadata {
                            offset: opcode_offset,
                        })?,
                )?;
            }
            0x21 => {
                let group_count = reader.read_u8()? as usize;
                for _ in 0..group_count {
                    let len = reader.read_u8()? as usize;
                    reader.skip(len)?;
                }
            }
            0x22 | 0x23 => {
                reader.skip(1)?;
            }
            0x40 | 0x42 => {
                let _ = reader.read_delta()?;
                let _ = reader.read_delta()?;
            }
            0x41 | 0x43 => {
                for _ in 0..6 {
                    let _ = reader.read_delta()?;
                }
            }
            0x44 => {}

            _ => {
                return Err(DecodeError::InvalidOpcode {
                    opcode,
                    offset: opcode_offset,
                });
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitmapMetrics {
    pub x: i16,
    pub y: i16,
    pub width: i16,
    pub height: i16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedBitmapGlyph<'a> {
    pub metrics: BitmapMetrics,
    /// Source HFT rows are packed to the minimum whole-byte width. The renderer may
    /// choose a wider destination stride, but that padding is not present in the file.
    pub row_bytes: usize,
    pub pixels: &'a [u8],
    /// Total bytes consumed from `record`, including the optional 8-byte metrics prefix.
    pub consumed: usize,
}

pub fn bitmap_payload_len(metrics: BitmapMetrics) -> Result<usize, DecodeError> {
    if metrics.width < 0 || metrics.height < 0 {
        return Err(DecodeError::InvalidBitmap("negative dimensions"));
    }
    let row_bytes = (metrics.width as usize + 7) >> 3;
    row_bytes
        .checked_mul(metrics.height as usize)
        .ok_or(DecodeError::InvalidBitmap("dimension overflow"))
}

/// Decode one HFT 1bpp bitmap record.
///
/// Fixed-metric ranges store only packed pixels and pass `Some(metrics)`. Variable-metric
/// ranges store four little-endian i16 values `(x, y, width, height)` before the pixels
/// and pass `None`. This follows the two HncBaseDraw bitmap paths without copying any
/// proprietary implementation.
pub fn decode_bitmap_glyph_record<'a>(
    record: &'a [u8],
    fixed_metrics: Option<BitmapMetrics>,
) -> Result<DecodedBitmapGlyph<'a>, DecodeError> {
    let (metrics, prefix_len) = match fixed_metrics {
        Some(metrics) => (metrics, 0usize),
        None => {
            if record.len() < 8 {
                return Err(DecodeError::InvalidBitmap("truncated metrics"));
            }
            (
                BitmapMetrics {
                    x: i16::from_le_bytes([record[0], record[1]]),
                    y: i16::from_le_bytes([record[2], record[3]]),
                    width: i16::from_le_bytes([record[4], record[5]]),
                    height: i16::from_le_bytes([record[6], record[7]]),
                },
                8,
            )
        }
    };
    let byte_len = bitmap_payload_len(metrics)?;
    let consumed = prefix_len
        .checked_add(byte_len)
        .ok_or(DecodeError::InvalidBitmap("record length overflow"))?;
    let pixels = record
        .get(prefix_len..consumed)
        .ok_or(DecodeError::InvalidBitmap("truncated pixel data"))?;
    Ok(DecodedBitmapGlyph {
        metrics,
        row_bytes: (metrics.width as usize + 7) >> 3,
        pixels,
        consumed,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlyphComponent {
    pub glyph_index: usize,
    pub x_offset: i16,
    pub y_offset: i16,
}

const HFT_JOHAB_L: [i8; 32] = [
    20, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 21, 22, 23, 24, 25,
    26, 27, 28, 29, 30, 31,
];
const HFT_JOHAB_V: [i8; 32] = [
    -1, -1, 0, 1, 2, 3, 4, 5, -1, -1, 6, 7, 8, 9, 10, 11, -1, 22, 12, 13, 14, 15, 16, 17, 23, 24,
    18, 19, 20, 21, 25, 26,
];
const HFT_JOHAB_T: [i8; 32] = [
    28, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 29, 17, 18, 19, 20, 21, 22, 23,
    24, 25, 26, 27, 30, 31,
];

fn slice_u16(bytes: &[u8], offset: usize) -> Result<u16, DecodeError> {
    let pair = bytes
        .get(offset..offset + 2)
        .ok_or(DecodeError::InvalidLocator("truncated u16"))?;
    Ok(u16::from_le_bytes([pair[0], pair[1]]))
}

fn slice_i16(bytes: &[u8], offset: usize) -> Result<i16, DecodeError> {
    Ok(slice_u16(bytes, offset)? as i16)
}

fn cumulative_component_base(
    cache: &[u8],
    dimensions: [usize; 3],
    axis: usize,
    element: usize,
) -> Result<usize, DecodeError> {
    const COUNT_OFFSETS: [usize; 3] = [0x08, 0x48, 0x88];
    let mut running = 0usize;
    for current_axis in 0..3 {
        let dimension = dimensions[current_axis];
        for current_element in 0..=dimension {
            if current_axis == axis && current_element == element {
                return Ok(running);
            }
            running = running
                .checked_add(
                    slice_u16(cache, COUNT_OFFSETS[current_axis] + current_element * 2)? as usize,
                )
                .ok_or(DecodeError::InvalidLocator("component base overflow"))?;
        }
    }
    Err(DecodeError::InvalidLocator(
        "component index is outside cache dimensions",
    ))
}

/// Resolve HFT subtype-2/subtype-4 Johab composite mapping into physical glyph records.
///
/// `cache` starts at the on-disk cache-size word immediately following the 22-byte range
/// header. `legacy_dimension_decrement` mirrors the old Human-font loader flag which
/// decrements the three cache dimensions before building cumulative component bases.
/// Subtype 4 can additionally carry per-component x/y offsets after its three mapping
/// planes; subtype 2 uses the same mapping without those adjustment arrays.
pub fn locate_composite_components(
    cache: &[u8],
    subtype: u8,
    code: u16,
    glyph_count: usize,
    legacy_dimension_decrement: bool,
) -> Result<Vec<GlyphComponent>, DecodeError> {
    if !matches!(subtype, 2 | 4) {
        return Err(DecodeError::InvalidLocator(
            "composite locator requires subtype 2 or 4",
        ));
    }
    let cache_size = slice_u16(cache, 0)? as usize;
    if cache_size < 0x12c || cache_size > cache.len() {
        return Err(DecodeError::InvalidLocator("invalid composite cache size"));
    }
    let cache = &cache[..cache_size];

    let mut dimensions = [
        *cache
            .get(0x04)
            .ok_or(DecodeError::InvalidLocator("missing L dimension"))? as usize,
        *cache
            .get(0x05)
            .ok_or(DecodeError::InvalidLocator("missing V dimension"))? as usize,
        *cache
            .get(0x06)
            .ok_or(DecodeError::InvalidLocator("missing T dimension"))? as usize,
    ];
    if legacy_dimension_decrement {
        for dimension in &mut dimensions {
            *dimension = dimension
                .checked_sub(1)
                .ok_or(DecodeError::InvalidLocator("zero legacy cache dimension"))?;
        }
    }

    let components = [
        HFT_JOHAB_L[((code >> 10) & 0x1f) as usize],
        HFT_JOHAB_V[((code >> 5) & 0x1f) as usize],
        HFT_JOHAB_T[(code & 0x1f) as usize],
    ];
    if components[1] < 0
        || components
            .iter()
            .enumerate()
            .any(|(axis, &component)| component < 0 || component as usize > dimensions[axis])
    {
        return Ok(Vec::new());
    }
    let original = [
        components[0] as usize,
        components[1] as usize,
        components[2] as usize,
    ];

    // The broad Johab range includes reserved bucket values (for example T=28
    // from low bits 0). HncBaseDraw assumes callers never request those values,
    // but their cumulative base can equal glyph_count. Treat a nonzero component
    // bucket with zero physical records as an unmapped code rather than exposing
    // that unchecked sentinel index.
    const COUNT_OFFSETS: [usize; 3] = [0x08, 0x48, 0x88];
    for axis in 0..3 {
        if original[axis] != 0 && slice_u16(cache, COUNT_OFFSETS[axis] + original[axis] * 2)? == 0 {
            return Ok(Vec::new());
        }
    }

    let remap_offsets = [0xccusize, 0xec, 0x10c];
    let mut remapped = [0usize; 3];
    for axis in 0..3 {
        remapped[axis] = *cache
            .get(remap_offsets[axis] + original[axis])
            .ok_or(DecodeError::InvalidLocator("truncated component remap"))?
            as usize;
    }
    let cube_dimensions = [
        *cache
            .get(0xc8)
            .ok_or(DecodeError::InvalidLocator("missing cube L dimension"))? as usize,
        *cache
            .get(0xc9)
            .ok_or(DecodeError::InvalidLocator("missing cube V dimension"))? as usize,
        *cache
            .get(0xca)
            .ok_or(DecodeError::InvalidLocator("missing cube T dimension"))? as usize,
    ];
    if cube_dimensions.contains(&0) || (0..3).any(|axis| remapped[axis] >= cube_dimensions[axis]) {
        return Ok(Vec::new());
    }
    let cube_size = cube_dimensions[0]
        .checked_mul(cube_dimensions[1])
        .and_then(|value| value.checked_mul(cube_dimensions[2]))
        .ok_or(DecodeError::InvalidLocator("composite cube overflow"))?;
    let linear = ((remapped[0] * cube_dimensions[1]) + remapped[1])
        .checked_mul(cube_dimensions[2])
        .and_then(|value| value.checked_add(remapped[2]))
        .ok_or(DecodeError::InvalidLocator("composite index overflow"))?;
    if linear >= cube_size {
        return Err(DecodeError::InvalidLocator(
            "composite linear index exceeds cube",
        ));
    }

    let map_offset = 0x12cusize;
    let map_len = cube_size
        .checked_mul(3)
        .ok_or(DecodeError::InvalidLocator("mapping length overflow"))?;
    let map_end = map_offset
        .checked_add(map_len)
        .ok_or(DecodeError::InvalidLocator("mapping offset overflow"))?;
    if map_end > cache.len() {
        return Err(DecodeError::InvalidLocator("truncated composite mapping"));
    }

    let adjustment_flags = *cache
        .get(0x07)
        .ok_or(DecodeError::InvalidLocator("missing adjustment flags"))?;
    let adjustment_bytes = map_len
        .checked_mul(2)
        .ok_or(DecodeError::InvalidLocator("adjustment length overflow"))?;
    let x_adjustment_offset = if adjustment_flags & 0x01 != 0 {
        if map_end + adjustment_bytes > cache.len() {
            return Err(DecodeError::InvalidLocator("truncated x adjustment array"));
        }
        Some(map_end)
    } else {
        None
    };
    let y_adjustment_offset = if adjustment_flags & 0x02 != 0 {
        let offset = map_end
            + if x_adjustment_offset.is_some() {
                adjustment_bytes
            } else {
                0
            };
        if offset + adjustment_bytes > cache.len() {
            return Err(DecodeError::InvalidLocator("truncated y adjustment array"));
        }
        Some(offset)
    } else {
        None
    };

    let mut result = Vec::with_capacity(3);
    for (axis, &original_component) in original.iter().enumerate() {
        if original_component == 0 {
            continue;
        }
        let plane_index = axis
            .checked_mul(cube_size)
            .and_then(|value| value.checked_add(linear))
            .ok_or(DecodeError::InvalidLocator("mapping plane overflow"))?;
        let base = cumulative_component_base(cache, dimensions, axis, original_component)?;
        let glyph_index = base
            .checked_add(cache[map_offset + plane_index] as usize)
            .ok_or(DecodeError::InvalidLocator("glyph index overflow"))?;
        if glyph_index >= glyph_count {
            return Err(DecodeError::GlyphIndexOutOfRange {
                index: glyph_index,
                glyph_count,
            });
        }
        let x_offset = match x_adjustment_offset {
            Some(offset) => slice_i16(cache, offset + plane_index * 2)?,
            None => 0,
        };
        let y_offset = match y_adjustment_offset {
            Some(offset) => slice_i16(cache, offset + plane_index * 2)?,
            None => 0,
        };
        result.push(GlyphComponent {
            glyph_index,
            x_offset,
            y_offset,
        });
    }
    Ok(result)
}

/// Resolve HFT subtype-3's packed Johab-special code into its physical glyph index.
/// Reserved filler ranges return `None`, matching the clean-room behavior observed in
/// HncBaseDraw before the packed bit fields are assembled.
pub fn locate_packed_special_index(code: u16) -> Option<usize> {
    if code & 0x83c0 != 0x8000 {
        return None;
    }
    if (0xe829..=0xe83f).contains(&code) {
        return None;
    }
    let high = (code >> 8) as u8;
    let low_window = code.wrapping_sub(0x20) as u8;
    if low_window <= 0x1f && matches!(high, 0xec | 0xf0 | 0xf4 | 0xf8 | 0xfc) {
        return None;
    }
    Some(
        (((code as usize) >> 5) & 0x3e0)
            | (((code as usize) & 0x20) << 5)
            | ((code as usize) & 0x1f),
    )
}

/// Apply the optional H&C byte transform used by some HFT outline chunks.
///
/// The state update intentionally uses the original input byte, not the transformed
/// output byte. The two-byte glyph length prefix is not part of this transform.
pub fn decode_hnc_obfuscation_in_place(bytes: &mut [u8]) {
    let mut state: u16 = 0xe696;
    for byte in bytes {
        let input = *byte;
        *byte = input ^ (state >> 8) as u8;
        let mixed = state.wrapping_add(input as u16);
        state = 0xc863u16.wrapping_sub(0x38c2u16.wrapping_mul(mixed));
    }
}

/// Apply the transform used by HWP 2.5 Mapsi HFT files.
pub fn decode_mapsi_obfuscation_in_place(bytes: &mut [u8]) {
    let mut state: u16 = 0xa729;
    for byte in bytes {
        let input = *byte;
        *byte = input ^ (state >> 8) as u8;
        let mixed = state.wrapping_add(input as u16);
        state = 0xe696u16.wrapping_sub(0x38c2u16.wrapping_mul(mixed));
    }
}

/// Apply the transform used by Human HFT files. `seed` is the per-file word
/// stored in the HFT header and copied into the decoder context at open time.
pub fn decode_human_obfuscation_in_place(bytes: &mut [u8], seed: u16) {
    if seed == 0 || bytes.is_empty() {
        return;
    }
    let mut state = seed;
    for byte in bytes {
        let input = *byte;
        *byte = input ^ state as u8;
        state = state.wrapping_add(input as u16).wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn le16(data: &[u8], offset: usize) -> u16 {
        u16::from_le_bytes([data[offset], data[offset + 1]])
    }

    fn le32(data: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ])
    }

    fn le_i16(data: &[u8], offset: usize) -> i16 {
        i16::from_le_bytes([data[offset], data[offset + 1]])
    }

    #[test]
    fn compact_delta_boundaries_decode() {
        // MoveTo(x=124,y=-124), then LineTo(x+=1147,y-=1147), then end.
        let stream = [
            0x03, 0x7c, 0x00, 0x84, 0x00, // +/-124
            0x07, 0x7f, 0xff, 0x81, 0xff, // +/-1147
            0x00,
        ];
        let decoded = decode_outline_stream(&stream).unwrap();
        assert_eq!(decoded.consumed, stream.len());
        assert_eq!(
            decoded.ops,
            vec![
                OutlineOp::MoveTo(Point { x: 124, y: -124 }),
                OutlineOp::LineTo(Point { x: 1271, y: -1271 }),
                OutlineOp::Close,
            ]
        );
    }

    #[test]
    fn cubic_presence_bits_follow_hft_order() {
        // 0x09 consumes: first-x, middle-x, middle-y, final-y.
        let stream = [0x03, 10, 20, 0x09, 1, 2, 3, 4, 0x00];
        let decoded = decode_outline_stream(&stream).unwrap();
        assert_eq!(
            decoded.ops,
            vec![
                OutlineOp::MoveTo(Point { x: 10, y: 20 }),
                OutlineOp::CubicTo {
                    control1: Point { x: 11, y: 20 },
                    control2: Point { x: 13, y: 23 },
                    to: Point { x: 13, y: 27 },
                },
                OutlineOp::Close,
            ]
        );
    }

    #[test]
    fn bitmap_record_modes_decode() {
        let fixed = BitmapMetrics {
            x: 1,
            y: -2,
            width: 9,
            height: 2,
        };
        let fixed_bytes = [0xaa, 0x80, 0x55, 0x00];
        let decoded = decode_bitmap_glyph_record(&fixed_bytes, Some(fixed)).unwrap();
        assert_eq!(decoded.row_bytes, 2);
        assert_eq!(decoded.consumed, 4);
        assert_eq!(decoded.pixels, &fixed_bytes);

        let variable = [
            1, 0, 0xfe, 0xff, 9, 0, 2, 0, // x=1,y=-2,width=9,height=2
            0xaa, 0x80, 0x55, 0x00,
        ];
        let decoded = decode_bitmap_glyph_record(&variable, None).unwrap();
        assert_eq!(decoded.metrics, fixed);
        assert_eq!(decoded.row_bytes, 2);
        assert_eq!(decoded.consumed, variable.len());
        assert_eq!(decoded.pixels, &variable[8..]);
    }

    #[test]
    fn packed_special_locator_covers_1865_records() {
        let indices: Vec<_> = (0x8000u32..=0xffff)
            .filter_map(|value| locate_packed_special_index(value as u16))
            .collect();
        assert_eq!(indices.len(), 1865);
        assert_eq!(indices.iter().copied().max(), Some(1864));
        let mut sorted = indices;
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, (0usize..1865).collect::<Vec<_>>());
    }

    #[test]
    fn vendor_transforms_cover_seed_rules() {
        let mut hnc = [0x00];
        decode_hnc_obfuscation_in_place(&mut hnc);
        assert_eq!(hnc, [0xe6]);

        let mut mapsi = [0x00];
        decode_mapsi_obfuscation_in_place(&mut mapsi);
        assert_eq!(mapsi, [0xa7]);

        let mut human_zero_seed = [0x20, 0x02];
        decode_human_obfuscation_in_place(&mut human_zero_seed, 0);
        assert_eq!(human_zero_seed, [0x20, 0x02]);

        let mut human_seeded = [0x00];
        decode_human_obfuscation_in_place(&mut human_seeded, 0x1234);
        assert_eq!(human_seeded, [0x34]);
    }

    /// Optional local interoperability check. No HFT bytes are embedded in the test.
    /// Set HOP_HFT_SAMPLE to an owned `Han Unified Font File 1.0` file before running.
    #[test]
    fn local_hft_header_validates() {
        let Some(path) = std::env::var_os("HOP_HFT_SAMPLE").map(PathBuf::from) else {
            eprintln!("HOP_HFT_SAMPLE is unset; local HFT sample test skipped");
            return;
        };
        let data = std::fs::read(&path).expect("read HFT sample");
        validate_hft_header(&data).expect("validate HFT header");
    }

    /// Optional local interoperability check. No HFT bytes are embedded in the test.
    /// Set HOP_HFT_SAMPLE to an owned `Han Unified Font File 1.0` file before running.
    #[test]
    fn local_hft_composite_and_special_locators_are_bounded() {
        let Some(path) = std::env::var_os("HOP_HFT_SAMPLE").map(PathBuf::from) else {
            eprintln!("HOP_HFT_SAMPLE is unset; local HFT sample test skipped");
            return;
        };
        let data = std::fs::read(&path).expect("read HFT sample");
        assert!(data.len() >= 0x200, "HFT file shorter than 512-byte header");
        const MAGIC: &[u8; 32] = b"Han Unified Font File 1.0\x1a\x04\x03\x02\x01\xff\x00";
        assert_eq!(&data[..MAGIC.len()], MAGIC, "unexpected HFT header magic");

        let human_legacy = data[0x134..0x154].starts_with(b"Human Font for HWP 2.1");
        let mut block_pos = le32(&data, 0x1ae) as usize;
        let mut composite_codes = 0usize;
        let mut composite_components = 0usize;
        let mut packed_codes = 0usize;
        while block_pos + 14 <= data.len() {
            let block_span = le32(&data, block_pos) as usize;
            if block_span < 14 || block_pos + block_span > data.len() {
                break;
            }
            let block_format = le16(&data, block_pos + 6);
            let range_count = le16(&data, block_pos + 8) as usize;
            let mut range_pos = block_pos + le32(&data, block_pos + 10) as usize;
            for range_index in 0..range_count {
                assert!(
                    range_pos + 22 <= block_pos + block_span,
                    "truncated locator range"
                );
                let range_span = le32(&data, range_pos) as usize;
                let declared_range_end = range_pos.checked_add(range_span).expect("range overflow");
                assert!(range_span >= 22, "invalid locator range span");
                let range_end = if range_index + 1 == range_count {
                    block_pos + block_span
                } else {
                    assert!(
                        declared_range_end <= block_pos + block_span,
                        "invalid locator range span"
                    );
                    declared_range_end
                };
                let flags = le16(&data, range_pos + 4);
                let subtype = (flags & 0x0f) as u8;
                let code_start = le16(&data, range_pos + 6);
                let code_end = le16(&data, range_pos + 8);
                let glyph_count = le16(&data, range_pos + 10) as usize;
                let payload = range_pos + 22;

                if matches!(subtype, 2 | 4) {
                    assert!(payload + 2 <= range_end, "truncated composite cache");
                    let cache_size = le16(&data, payload) as usize;
                    assert!(
                        payload + cache_size <= range_end,
                        "composite cache exceeds range"
                    );
                    let data_base = payload + cache_size;
                    let cache = &data[payload..payload + cache_size];
                    for code in code_start..=code_end {
                        match locate_composite_components(
                            cache,
                            subtype,
                            code,
                            glyph_count,
                            human_legacy,
                        ) {
                            Ok(components) => {
                                if !components.is_empty() {
                                    composite_codes += 1;
                                }
                                for component in components {
                                    assert!(component.glyph_index < glyph_count);
                                    if block_format == 1 || flags & 0x10 == 0 {
                                        assert!(
                                            data_base + component.glyph_index * 4 + 4 <= range_end,
                                            "component offset entry exceeds range"
                                        );
                                        let rel = le32(
                                            &data,
                                            data_base + component.glyph_index * 4,
                                        );
                                        assert_ne!(
                                            rel, 0,
                                            "composite component points at missing physical glyph"
                                        );
                                    }
                                    composite_components += 1;
                                }
                            }
                            Err(DecodeError::GlyphIndexOutOfRange { index, glyph_count }) => {
                                panic!(
                                    "composite locator escaped reserved-bucket filtering: index {index}, count {glyph_count}"
                                );
                            }
                            Err(error) => panic!(
                                "composite locator failed in {} at range {range_pos:#x}, code {code:#06x}: {error}",
                                path.display()
                            ),
                        }
                    }
                } else if subtype == 3 {
                    let data_base = payload;
                    for code in code_start..=code_end {
                        if let Some(index) = locate_packed_special_index(code) {
                            assert!(
                                index < glyph_count,
                                "packed locator produced out-of-range physical index"
                            );
                            if block_format == 1 || flags & 0x10 == 0 {
                                assert!(data_base + index * 4 + 4 <= range_end);
                                assert_ne!(le32(&data, data_base + index * 4), 0);
                            }
                            packed_codes += 1;
                        }
                    }
                }
                range_pos = declared_range_end;
            }
            block_pos += block_span;
        }
        eprintln!(
            "validated {composite_codes} composite codes / {composite_components} components and {packed_codes} packed codes from {}",
            path.display()
        );
    }

    #[test]
    fn local_hft_all_bitmap_records_decode_exactly() {
        let Some(path) = std::env::var_os("HOP_HFT_SAMPLE").map(PathBuf::from) else {
            eprintln!("HOP_HFT_SAMPLE is unset; local HFT sample test skipped");
            return;
        };
        let data = std::fs::read(&path).expect("read HFT sample");
        assert!(data.len() >= 0x200, "HFT file shorter than 512-byte header");
        const MAGIC: &[u8; 32] = b"Han Unified Font File 1.0\x1a\x04\x03\x02\x01\xff\x00";
        assert_eq!(&data[..MAGIC.len()], MAGIC, "unexpected HFT header magic");

        let mut block_pos = le32(&data, 0x1ae) as usize;
        let mut decoded_records = 0usize;
        let mut bitmap_blocks = 0usize;
        while block_pos + 14 <= data.len() {
            let block_span = le32(&data, block_pos) as usize;
            if block_span < 14 || block_pos + block_span > data.len() {
                break;
            }
            let block_format = le16(&data, block_pos + 6);
            let range_count = le16(&data, block_pos + 8) as usize;
            let mut range_pos = block_pos + le32(&data, block_pos + 10) as usize;
            if block_format == 0 {
                bitmap_blocks += 1;
                for range_index in 0..range_count {
                    assert!(
                        range_pos + 22 <= block_pos + block_span,
                        "truncated bitmap range record"
                    );
                    let range_span = le32(&data, range_pos) as usize;
                    let declared_range_end = range_pos
                        .checked_add(range_span)
                        .expect("bitmap range overflow");
                    assert!(range_span >= 22, "invalid bitmap range span");
                    let range_end = if range_index + 1 == range_count {
                        block_pos + block_span
                    } else {
                        assert!(
                            declared_range_end <= block_pos + block_span,
                            "invalid bitmap range span"
                        );
                        declared_range_end
                    };
                    let flags = le16(&data, range_pos + 4);
                    let subtype = flags & 0x0f;
                    let glyph_count = le16(&data, range_pos + 10) as usize;
                    let payload = range_pos + 22;
                    let cache_size = if matches!(subtype, 1 | 2 | 4) {
                        assert!(payload + 4 <= range_end, "truncated bitmap locator cache");
                        le16(&data, payload) as usize
                    } else {
                        0
                    };
                    let data_base = payload + cache_size;

                    if flags & 0x10 != 0 {
                        let metrics = BitmapMetrics {
                            x: le_i16(&data, range_pos + 14),
                            y: le_i16(&data, range_pos + 16),
                            width: le_i16(&data, range_pos + 18),
                            height: le_i16(&data, range_pos + 20),
                        };
                        let byte_len = bitmap_payload_len(metrics).expect("fixed bitmap metrics");
                        let all_len = byte_len
                            .checked_mul(glyph_count)
                            .expect("bitmap range length overflow");
                        assert!(
                            data_base + all_len <= range_end,
                            "truncated fixed-metric bitmap data"
                        );
                        for glyph_index in 0..glyph_count {
                            let glyph_pos = data_base + glyph_index * byte_len;
                            let decoded = decode_bitmap_glyph_record(
                                &data[glyph_pos..range_end],
                                Some(metrics),
                            )
                            .expect("decode fixed-metric bitmap glyph");
                            assert_eq!(decoded.consumed, byte_len);
                            assert_eq!(decoded.pixels.len(), byte_len);
                            decoded_records += 1;
                        }
                    } else {
                        assert!(
                            data_base + glyph_count * 4 <= range_end,
                            "truncated bitmap offset table"
                        );
                        for glyph_index in 0..glyph_count {
                            let rel = le32(&data, data_base + glyph_index * 4) as usize;
                            if rel == 0 {
                                continue;
                            }
                            let glyph_pos = data_base
                                .checked_add(rel)
                                .expect("bitmap glyph offset overflow");
                            assert!(glyph_pos < range_end, "bitmap glyph starts outside range");
                            let decoded =
                                decode_bitmap_glyph_record(&data[glyph_pos..range_end], None)
                                    .expect("decode variable-metric bitmap glyph");
                            assert!(glyph_pos + decoded.consumed <= range_end);
                            decoded_records += 1;
                        }
                    }
                    range_pos = declared_range_end;
                }
            }
            block_pos += block_span;
        }
        eprintln!(
            "decoded {decoded_records} bitmap records across {bitmap_blocks} bitmap blocks from {}",
            path.display()
        );
    }

    #[test]
    fn local_hft_all_outline_records_decode_exactly() {
        let Some(path) = std::env::var_os("HOP_HFT_SAMPLE").map(PathBuf::from) else {
            eprintln!("HOP_HFT_SAMPLE is unset; local HFT sample test skipped");
            return;
        };
        let data = std::fs::read(&path).expect("read HFT sample");
        assert!(data.len() >= 0x200, "HFT file shorter than 512-byte header");
        const MAGIC: &[u8; 32] = b"Han Unified Font File 1.0\x1a\x04\x03\x02\x01\xff\x00";
        assert_eq!(&data[..MAGIC.len()], MAGIC, "unexpected HFT header magic");

        let marker = &data[0x134..0x154];
        let mut block_pos = le32(&data, 0x1ae) as usize;
        let mut decoded_records = 0usize;
        let mut outline_blocks = 0usize;
        while block_pos + 14 <= data.len() {
            let block_span = le32(&data, block_pos) as usize;
            if block_span < 14 || block_pos + block_span > data.len() {
                break;
            }
            let block_format = le16(&data, block_pos + 6);
            let range_count = le16(&data, block_pos + 8) as usize;
            let mut range_pos = block_pos + le32(&data, block_pos + 10) as usize;
            if block_format == 1 {
                outline_blocks += 1;
                for range_index in 0..range_count {
                    assert!(
                        range_pos + 22 <= block_pos + block_span,
                        "truncated HFT range record"
                    );
                    let range_span = le32(&data, range_pos) as usize;
                    let declared_range_end =
                        range_pos.checked_add(range_span).expect("range overflow");
                    assert!(range_span >= 22, "invalid HFT range span");
                    let range_end = if range_index + 1 == range_count {
                        block_pos + block_span
                    } else {
                        assert!(
                            declared_range_end <= block_pos + block_span,
                            "invalid HFT range span"
                        );
                        declared_range_end
                    };
                    let flags = le16(&data, range_pos + 4);
                    let subtype = flags & 0x0f;
                    let glyph_count = le16(&data, range_pos + 10) as usize;
                    let payload = range_pos + 22;
                    let cache_size = if matches!(subtype, 1 | 2 | 4) {
                        assert!(payload + 4 <= range_end, "truncated HFT locator cache");
                        le16(&data, payload) as usize
                    } else {
                        0
                    };
                    let offset_base = payload + cache_size;
                    assert!(
                        offset_base + glyph_count * 4 <= range_end,
                        "truncated outline offset table"
                    );
                    for glyph_index in 0..glyph_count {
                        let rel = le32(&data, offset_base + glyph_index * 4) as usize;
                        if rel == 0 {
                            continue;
                        }
                        let glyph_pos =
                            offset_base.checked_add(rel).expect("glyph offset overflow");
                        let metric_prefix = if flags & 0x10 == 0 { 8 } else { 0 };
                        let length_pos = glyph_pos + metric_prefix;
                        assert!(
                            length_pos + 2 <= range_end,
                            "truncated outline glyph length"
                        );
                        let byte_len = le16(&data, length_pos) as usize;
                        if byte_len == 0 {
                            continue;
                        }
                        let start = length_pos + 2;
                        let end = start
                            .checked_add(byte_len)
                            .expect("outline glyph length overflow");
                        assert!(end <= range_end, "truncated outline glyph bytecode");
                        let mut stream = data[start..end].to_vec();
                        if marker.starts_with(b"Hanyang outline font for HWP ") {
                            decode_hnc_obfuscation_in_place(&mut stream);
                        } else if marker.starts_with(b"Hangul Mapsi font for HWP 2.5") {
                            decode_mapsi_obfuscation_in_place(&mut stream);
                        } else if marker.starts_with(b"Human Font for HWP 2.1") {
                            let seed = u16::from_be_bytes([data[0x151], data[0x152]]);
                            decode_human_obfuscation_in_place(&mut stream, seed);
                        } else if marker.iter().any(|&byte| byte != 0) {
                            panic!("unknown nonzero HFT transform marker");
                        }
                        let decoded = decode_outline_stream(&stream).unwrap_or_else(|err| {
                            panic!("outline block at {block_pos:#x}, range {range_pos:#x}, glyph {glyph_index} failed: {err}")
                        });
                        assert_eq!(
                            decoded.consumed, byte_len,
                            "outline glyph left trailing bytecode"
                        );
                        decoded_records += 1;
                    }
                    range_pos = declared_range_end;
                }
            }
            block_pos += block_span;
        }
        eprintln!(
            "decoded {decoded_records} outline records across {outline_blocks} outline blocks from {}",
            path.display()
        );
    }
}
