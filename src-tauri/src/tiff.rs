//! Read-only TIFF/DNG inspector.
//!
//! Parses just enough of the TIFF structure (which DNG is built on) to report
//! honest compression numbers: what compression a file actually uses, how many
//! bytes the image data occupies according to the tile/strip byte counts, and
//! whether the file even needs recompressing.
//!
//! No dependencies. Never writes. Never shells out.
//!
//! ## The honest-ratio contract
//!
//! The headline compression ratio is
//!
//! ```text
//! true_ratio = image_data_bytes(input) / image_data_bytes(output)
//! ```
//!
//! where `image_data_bytes` is the sum of `TileByteCounts`, falling back to
//! `StripByteCounts` when the file is not tiled. Whole-file size is reported
//! as a secondary number only — previews, metadata and padding are not image
//! data and must not inflate the ratio.
//!
//! ## Skip rule
//!
//! A file whose main IFD reports `Compression == 7` (JPEG) is already
//! compressed. In the default and lossless modes the engine copies such files
//! through instead of recompressing them (`IfdSummary::already_compressed`).
//!
//! BigTIFF (magic 43) is rejected with a clear error rather than misparsed.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

// ── Public types ─────────────────────────────────────────────────────

/// Byte order declared by the TIFF header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteOrder {
    Little,
    Big,
}

/// TIFF compression codes we care about.
// COMPRESSION_UNCOMPRESSED / COMPRESSION_LOSSY_JPEG_DNG are kept for the
// upcoming lossy-marker work; not every code is matched yet.
#[allow(dead_code)]
pub const COMPRESSION_UNCOMPRESSED: u16 = 1;
pub const COMPRESSION_JPEG: u16 = 7; // Adobe lossless JPEG in DNG
#[allow(dead_code)]
pub const COMPRESSION_LOSSY_JPEG_DNG: u16 = 34892;

/// Human label for a TIFF compression code.
pub fn compression_name(code: u16) -> &'static str {
    match code {
        1 => "Uncompressed",
        2 => "CCITT Huffman",
        5 => "LZW",
        6 => "JPEG (old-style)",
        7 => "JPEG",
        8 => "Deflate",
        32773 => "PackBits",
        32946 => "Deflate (PKZIP)",
        34712 => "JPEG 2000",
        34892 => "Lossy JPEG (DNG)",
        _ => "Unknown",
    }
}

/// Crop-related tags (DNG).
#[derive(Debug, Clone, Default)]
pub struct CropTags {
    /// tag 51111
    pub default_crop_origin: Option<(u32, u32)>,
    /// tag 51112
    pub default_crop_size: Option<(u32, u32)>,
    /// tag 50829: top, left, bottom, right
    pub active_area: Option<[u32; 4]>,
    /// tag 50830: list of top, left, bottom, right rects
    pub masked_areas: Vec<[u32; 4]>,
}

/// The tags the engine needs from one IFD.
#[derive(Debug, Clone, Default)]
pub struct IfdSummary {
    pub new_subfile_type: Option<u32>, // tag 254 (bit 0 = thumbnail)
    pub compression: Option<u16>,   // tag 259
    pub photometric: Option<u16>,   // tag 262 (32803 = CFA Bayer)
    pub bits_per_sample: Vec<u16>,  // tag 258
    pub samples_per_pixel: Option<u16>, // tag 277
    pub image_width: Option<u32>,   // tag 256
    pub image_length: Option<u32>,  // tag 257
    pub rows_per_strip: Option<u32>, // tag 278
    pub tile_width: Option<u32>,    // tag 322
    pub tile_length: Option<u32>,   // tag 323
    pub strip_offsets: Vec<u32>,    // tag 273
    pub strip_byte_counts: Vec<u32>, // tag 279
    pub tile_offsets: Vec<u32>,     // tag 324
    pub tile_byte_counts: Vec<u32>, // tag 325
    pub jpeg_if_offset: Option<u32>, // tag 513 (JPEGInterchangeFormat)
    pub jpeg_if_length: Option<u32>, // tag 514 (JPEGInterchangeFormatLength)
    pub cfa_repeat_dim: Option<(u16, u16)>, // tag 33421 (rows, cols of the CFA repeat)
    pub cfa_pattern_raw: Option<[u8; 4]>, // tag 33422 (cell values 0=R 1=G 2=B)
    pub crop: CropTags,
}

impl IfdSummary {
    /// True when this IFD is a thumbnail (NewSubfileType bit 0 set).
    pub fn is_thumbnail(&self) -> bool {
        self.new_subfile_type.map(|v| v & 1 != 0).unwrap_or(false)
    }

    /// The 2x2 Bayer pattern (cell values 0=R, 1=G, 2=B) when the CFA tags
    /// describe a plain Bayer mosaic: 2x2 repeat, values in 0..=2, exactly
    /// one red, two green, one blue. Anything else returns None and the
    /// preview falls back to grayscale rather than guessing colors.
    pub fn cfa_pattern(&self) -> Option<[u8; 4]> {
        let p = self.cfa_pattern_raw?;
        if self.cfa_repeat_dim != Some((2, 2)) {
            return None;
        }
        let (mut r, mut g, mut b) = (0, 0, 0);
        for &v in &p {
            match v {
                0 => r += 1,
                1 => g += 1,
                2 => b += 1,
                _ => return None,
            }
        }
        if r == 1 && g == 2 && b == 1 {
            Some(p)
        } else {
            None
        }
    }
}

/// Whole-file report: the main IFD plus every SubIFD.
#[derive(Debug, Clone)]
pub struct FileReport {
    pub byte_order: ByteOrder,
    pub file_size: u64,
    pub main: IfdSummary,
    pub sub_ifds: Vec<IfdSummary>,
}

// ── IfdSummary helpers ───────────────────────────────────────────────

impl IfdSummary {
    /// True when the image data is stored as tiles rather than strips.
    pub fn is_tiled(&self) -> bool {
        self.tile_width.is_some() || self.tile_length.is_some()
    }

    /// Bytes of actual image data: sum of `TileByteCounts`, falling back to
    /// `StripByteCounts`. This is the numerator/denominator of the headline
    /// ratio — never the whole-file size.
    pub fn image_data_bytes(&self) -> u64 {
        let counts = if self.is_tiled() {
            &self.tile_byte_counts
        } else {
            &self.strip_byte_counts
        };
        counts.iter().map(|&c| c as u64).sum()
    }

    /// Human label for this IFD's compression, or "Unknown".
    pub fn compression_label(&self) -> &'static str {
        self.compression.map(compression_name).unwrap_or("Unknown")
    }

    /// `Compression == 7`: already JPEG-compressed. In default/lossless mode
    /// the engine copies such files through instead of recompressing.
    pub fn already_compressed(&self) -> bool {
        self.compression == Some(COMPRESSION_JPEG)
    }

    /// Expected tile count from the tile grid geometry, if fully described.
    pub fn tile_grid_count(&self) -> Option<u64> {
        let (w, h, tw, tl) = (
            self.image_width?,
            self.image_length?,
            self.tile_width?,
            self.tile_length?,
        );
        if tw == 0 || tl == 0 {
            return None;
        }
        Some(w.div_ceil(tw) as u64 * h.div_ceil(tl) as u64)
    }

    /// Structural verification: required tags, sane tile/strip geometry, data
    /// ranges inside the file, sane crop. Returns a list of problems;
    /// empty means the IFD verifies clean.
    pub fn verify(&self, file_size: u64) -> Vec<String> {
        let mut problems = Vec::new();

        if self.image_width.unwrap_or(0) == 0 {
            problems.push("missing or zero ImageWidth (tag 256)".to_string());
        }
        if self.image_length.unwrap_or(0) == 0 {
            problems.push("missing or zero ImageLength (tag 257)".to_string());
        }
        if self.bits_per_sample.is_empty() {
            problems.push("missing BitsPerSample (tag 258)".to_string());
        }
        if self.compression.is_none() {
            problems.push("missing Compression (tag 259)".to_string());
        }

        let (offsets, counts, kind) = if self.is_tiled() {
            if self.tile_width.unwrap_or(0) == 0 || self.tile_length.unwrap_or(0) == 0 {
                problems.push("tiled file missing TileWidth/TileLength (tags 322/323)".to_string());
            }
            (&self.tile_offsets, &self.tile_byte_counts, "tile")
        } else {
            if self.rows_per_strip.unwrap_or(0) == 0 {
                problems.push("stripped file missing RowsPerStrip (tag 278)".to_string());
            }
            (&self.strip_offsets, &self.strip_byte_counts, "strip")
        };

        if offsets.is_empty() {
            problems.push(format!("no {kind} offsets"));
        }
        if counts.is_empty() {
            problems.push(format!("no {kind} byte counts"));
        }
        if offsets.len() != counts.len() {
            problems.push(format!(
                "{kind} offsets ({}) != {kind} byte counts ({})",
                offsets.len(),
                counts.len()
            ));
        }
        for (i, (&off, &cnt)) in offsets.iter().zip(counts.iter()).enumerate() {
            if cnt == 0 {
                problems.push(format!("{kind} {i}: zero byte count"));
            }
            let end = off as u64 + cnt as u64;
            if end > file_size {
                problems.push(format!(
                    "{kind} {i}: data range {off}+{cnt} runs past end of file ({file_size} bytes)"
                ));
            }
        }

        if self.is_tiled() {
            if let Some(expected) = self.tile_grid_count() {
                if offsets.len() as u64 != expected {
                    problems.push(format!(
                        "tile count {} does not match tile grid ({expected} expected)",
                        offsets.len()
                    ));
                }
            }
        }

        if let (Some((cw, ch)), Some(w), Some(h)) = (
            self.crop.default_crop_size,
            self.image_width,
            self.image_length,
        ) {
            if cw > w || ch > h {
                problems.push(format!(
                    "DefaultCropSize {cw}x{ch} exceeds image {w}x{h}"
                ));
            }
        }

        problems
    }
}

// ── Entry point ──────────────────────────────────────────────────────

/// Inspect a TIFF/DNG file on disk. Read-only.
pub fn inspect(path: &Path) -> Result<FileReport, String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    let mut r = Reader {
        inner: file,
        len,
        order: ByteOrder::Little, // replaced once the header is read
    };
    parse_tiff(&mut r)
}

// ── Embedded JPEG thumbnail (for the film-gate preview) ──────────────

/// Standard base64 alphabet.
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Minimal base64 encoder — keeps this module dependency-free.
fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut n: u32 = 0;
        for &b in chunk {
            n = (n << 8) | b as u32;
        }
        n <<= 8 * (3 - chunk.len());
        for i in 0..4 {
            let idx = ((n >> (18 - 6 * i)) & 63) as usize;
            out.push(if i < chunk.len() + 1 {
                B64[idx] as char
            } else {
                '='
            });
        }
    }
    out
}

/// Raw JPEG bytes of the embedded thumbnail (the IFD whose NewSubfileType
/// is 1), if the file has one. Every strip range is bounds-checked against
/// the file size; anything suspicious yields None instead of a bad read.
pub fn thumbnail_jpeg(path: &Path) -> Option<Vec<u8>> {
    let report = inspect(path).ok()?;
    let mut ifds: Vec<&IfdSummary> = Vec::with_capacity(report.sub_ifds.len() + 1);
    ifds.push(&report.main);
    ifds.extend(report.sub_ifds.iter());
    let thumb = ifds.iter().find(|s| {
        s.is_thumbnail()
            && s.compression == Some(COMPRESSION_JPEG)
            && (!s.strip_offsets.is_empty() || s.jpeg_if_offset.is_some())
    })?;
    // Fallback: JPEGInterchangeFormat / JPEGInterchangeFormatLength (tags
    // 513/514) instead of strip tags.
    if thumb.strip_offsets.is_empty() {
        let off = thumb.jpeg_if_offset?;
        let cnt = thumb.jpeg_if_length?;
        if cnt == 0 || off as u64 + cnt as u64 > report.file_size {
            return None;
        }
        let file = File::open(path).ok()?;
        let mut r = Reader {
            inner: file,
            len: report.file_size,
            order: report.byte_order,
        };
        let mut buf = vec![0u8; cnt as usize];
        r.read_at(off as u64, &mut buf).ok()?;
        return Some(buf);
    }
    let mut bytes = Vec::new();
    for (&off, &cnt) in thumb
        .strip_offsets
        .iter()
        .zip(thumb.strip_byte_counts.iter())
    {
        let end = off as u64 + cnt as u64;
        if cnt == 0 || end > report.file_size {
            return None;
        }
        let file = File::open(path).ok()?;
        let mut r = Reader {
            inner: file,
            len: report.file_size,
            order: report.byte_order,
        };
        let mut buf = vec![0u8; cnt as usize];
        r.read_at(off as u64, &mut buf).ok()?;
        bytes.extend_from_slice(&buf);
    }
    if bytes.is_empty() {
        None
    } else {
        Some(bytes)
    }
}

/// Data URL of the embedded JPEG thumbnail, for the film-gate preview.
/// Falls back to a synthesized color preview from the raw sensor data
/// when the file has no embedded JPEG — Sigma fp CinemaDNG ships no
/// thumbnail at all, so without the fallback its frames show filenames.
pub fn thumbnail_data_url(path: &Path) -> Option<String> {
    if let Some(jpeg) = thumbnail_jpeg(path) {
        return Some(format!("data:image/jpeg;base64,{}", base64_encode(&jpeg)));
    }
    raw_preview_data_url(path)
}

// ── Synthesized raw preview (no embedded JPEG) ───────────────────────

/// Preview width in pixels; height follows the frame aspect.
const PREVIEW_WIDTH: u32 = 160;
/// Refuse to unpack sources bigger than this many pixels.
const MAX_PREVIEW_PIXELS: u64 = 100_000_000;

/// Unpack one strip's raw samples to 16-bit linear values. Handles the bit
/// depths the fp writes: 8 (1 byte/px), 12 (packed, 2 px per 3 bytes),
/// 16 (u16, honoring byte order). Anything else returns None.
fn unpack_raw_strip(bytes: &[u8], bits: u16, order: ByteOrder) -> Option<Vec<u16>> {
    match bits {
        8 => Some(bytes.iter().map(|&b| b as u16).collect()),
        12 => {
            let mut px = Vec::with_capacity(bytes.len() / 3 * 2);
            for c in bytes.chunks_exact(3) {
                let b0 = c[0] as u16;
                let b1 = c[1] as u16;
                let b2 = c[2] as u16;
                px.push(b0 | ((b1 & 0x0F) << 8));
                px.push((b1 >> 4) | (b2 << 4));
            }
            Some(px)
        }
        16 => {
            let mut px = Vec::with_capacity(bytes.len() / 2);
            for c in bytes.chunks_exact(2) {
                px.push(match order {
                    ByteOrder::Little => u16::from_le_bytes([c[0], c[1]]),
                    ByteOrder::Big => u16::from_be_bytes([c[0], c[1]]),
                });
            }
            Some(px)
        }
        _ => None,
    }
}

/// Box-average the linear raw down to preview size and gamma-lift to 8-bit
/// grayscale. Averaging over whole Bayer quads also kills the mosaic pattern,
/// so the result reads as a clean video-tap frame.
fn downsample_gamma(px: &[u16], w: u32, h: u32, bits: u16) -> Option<Vec<u8>> {
    if w == 0 || h == 0 || px.len() != (w as u64 * h as u64) as usize {
        return None;
    }
    let out_w = PREVIEW_WIDTH.min(w).max(1);
    let out_h = ((h as u64 * out_w as u64 / w as u64).max(1)) as u32;
    let maxv = ((1u32 << bits) - 1).max(1);
    // gamma 1/2.2: linear sensor values -> displayable gray
    let lut: Vec<u8> = (0..=maxv)
        .map(|v| (255.0 * (v as f64 / maxv as f64).powf(1.0 / 2.2)).round() as u8)
        .collect();
    let mut out = Vec::with_capacity((out_w * out_h) as usize);
    for oy in 0..out_h {
        let y0 = oy as u64 * h as u64 / out_h as u64;
        let y1 = (oy as u64 + 1) * h as u64 / out_h as u64;
        for ox in 0..out_w {
            let x0 = ox as u64 * w as u64 / out_w as u64;
            let x1 = (ox as u64 + 1) * w as u64 / out_w as u64;
            let mut sum: u64 = 0;
            let mut n: u64 = 0;
            for y in y0..y1 {
                let row = (y * w as u64) as usize;
                for x in x0..x1 {
                    sum += px[row + x as usize] as u64;
                    n += 1;
                }
            }
            let avg = if n > 0 { (sum / n).min(maxv as u64) } else { 0 };
            out.push(lut[avg as usize]);
        }
    }
    Some(out)
}

/// 8-bit grayscale BMP: 54-byte header, 256-entry palette, bottom-up rows.
/// Tiny, dependency-free, and every browser renders it from a data URL.
fn bmp_grayscale_8(w: u32, h: u32, px: &[u8]) -> Option<Vec<u8>> {
    if w == 0 || h == 0 || px.len() != (w * h) as usize {
        return None;
    }
    let stride = ((w + 3) / 4) * 4;
    let img_size = stride * h;
    let file_size = 54 + 1024 + img_size;
    let mut b = Vec::with_capacity(file_size as usize);
    b.extend_from_slice(b"BM");
    b.extend_from_slice(&file_size.to_le_bytes());
    b.extend_from_slice(&[0u8; 4]); // reserved
    b.extend_from_slice(&(54u32 + 1024).to_le_bytes()); // pixel data offset
    b.extend_from_slice(&40u32.to_le_bytes()); // DIB header size
    b.extend_from_slice(&w.to_le_bytes());
    b.extend_from_slice(&h.to_le_bytes()); // positive: bottom-up
    b.extend_from_slice(&1u16.to_le_bytes()); // planes
    b.extend_from_slice(&8u16.to_le_bytes()); // bits per pixel
    b.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB, uncompressed
    b.extend_from_slice(&img_size.to_le_bytes());
    b.extend_from_slice(&2835u32.to_le_bytes()); // px/m, unused
    b.extend_from_slice(&2835u32.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes()); // palette colors used
    b.extend_from_slice(&0u32.to_le_bytes()); // important colors
    for i in 0..256u32 {
        let v = i as u8;
        b.extend_from_slice(&[v, v, v, 0]); // grayscale ramp
    }
    let pad = (stride - w) as usize;
    for y in (0..h).rev() {
        let s = (y * w) as usize;
        b.extend_from_slice(&px[s..s + w as usize]);
        b.extend_from_slice(&vec![0u8; pad]);
    }
    Some(b)
}

/// Demosaic-by-downsampling: box-average each CFA phase into its own R/G/B
/// channel (this kills the mosaic the same way the grayscale path does),
/// then gray-world white balance and gamma lift. Returns interleaved RGB
/// plus the output dimensions, which match `downsample_gamma`'s geometry.
pub fn downsample_demosaic(
    px: &[u16],
    w: u32,
    h: u32,
    bits: u16,
    pattern: [u8; 4],
) -> Option<(Vec<u8>, u32, u32)> {
    if w == 0 || h == 0 || px.len() != (w as u64 * h as u64) as usize {
        return None;
    }
    let out_w = PREVIEW_WIDTH.min(w).max(1);
    let out_h = ((h as u64 * out_w as u64 / w as u64).max(1)) as u32;
    let maxv = ((1u32 << bits) - 1).max(1) as f64;
    let n = (out_w * out_h) as usize;
    let mut sum = vec![[0u64; 3]; n];
    let mut cnt = vec![[0u32; 3]; n];
    for (i, &v) in px.iter().enumerate() {
        let i = i as u64;
        let x = i % w as u64;
        let y = i / w as u64;
        // Same partition as the box-averager: floor(x*out_w/w) inverts
        // x0(ox) = ox*w/out_w exactly.
        let ox = (x * out_w as u64 / w as u64) as usize;
        let oy = (y * out_h as u64 / h as u64) as usize;
        let c = pattern[((y & 1) * 2 + (x & 1)) as usize];
        if c > 2 {
            continue;
        }
        let k = oy * out_w as usize + ox;
        sum[k][c as usize] += v as u64;
        cnt[k][c as usize] += 1;
    }
    // Gray-world white balance from the thumbnail's channel means,
    // normalized to green and clamped so odd scenes can't go psychedelic.
    let mut tot = [0u64; 3];
    let mut num = [0u64; 3];
    for k in 0..n {
        for c in 0..3 {
            tot[c] += sum[k][c];
            num[c] += cnt[k][c] as u64;
        }
    }
    let mean = |c: usize| {
        if num[c] > 0 {
            tot[c] as f64 / num[c] as f64
        } else {
            0.0
        }
    };
    let (mr, mg, mb) = (mean(0), mean(1), mean(2));
    let clamp_gain = |g: f64| g.max(0.25).min(4.0);
    let gr = if mr > 0.0 && mg > 0.0 {
        clamp_gain(mg / mr)
    } else {
        1.0
    };
    let gb = if mb > 0.0 && mg > 0.0 {
        clamp_gain(mg / mb)
    } else {
        1.0
    };
    let gains = [gr, 1.0, gb];
    let lut: Vec<u8> = (0..=maxv as u32)
        .map(|v| (255.0 * (v as f64 / maxv).powf(1.0 / 2.2)).round() as u8)
        .collect();
    let mut rgb = Vec::with_capacity(n * 3);
    for k in 0..n {
        for c in 0..3 {
            let avg = if cnt[k][c] > 0 {
                sum[k][c] as f64 / cnt[k][c] as f64 * gains[c]
            } else {
                0.0
            };
            let idx = (avg.round() as u64).min(maxv as u64) as usize;
            rgb.push(lut[idx]);
        }
    }
    Some((rgb, out_w, out_h))
}

/// 24-bit RGB BMP: 54-byte header, no palette, bottom-up BGR rows.
pub fn bmp_rgb_24(w: u32, h: u32, rgb: &[u8]) -> Option<Vec<u8>> {
    if w == 0 || h == 0 || rgb.len() != (w * h * 3) as usize {
        return None;
    }
    let row = w * 3;
    let stride = ((row + 3) / 4) * 4;
    let img_size = stride * h;
    let file_size = 54 + img_size;
    let mut b = Vec::with_capacity(file_size as usize);
    b.extend_from_slice(b"BM");
    b.extend_from_slice(&file_size.to_le_bytes());
    b.extend_from_slice(&[0u8; 4]); // reserved
    b.extend_from_slice(&54u32.to_le_bytes()); // pixel data offset
    b.extend_from_slice(&40u32.to_le_bytes()); // DIB header size
    b.extend_from_slice(&w.to_le_bytes());
    b.extend_from_slice(&h.to_le_bytes()); // positive: bottom-up
    b.extend_from_slice(&1u16.to_le_bytes()); // planes
    b.extend_from_slice(&24u16.to_le_bytes()); // bits per pixel
    b.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB, uncompressed
    b.extend_from_slice(&img_size.to_le_bytes());
    b.extend_from_slice(&2835u32.to_le_bytes()); // px/m, unused
    b.extend_from_slice(&2835u32.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes()); // palette colors used
    b.extend_from_slice(&0u32.to_le_bytes()); // important colors
    let pad = (stride - row) as usize;
    for y in (0..h).rev() {
        for x in 0..w {
            let s = ((y * w + x) * 3) as usize;
            b.push(rgb[s + 2]);
            b.push(rgb[s + 1]);
            b.push(rgb[s]);
        }
        b.extend_from_slice(&vec![0u8; pad]);
    }
    Some(b)
}

/// "Video tap" preview synthesized from the raw sensor data, for DNGs with
/// no embedded JPEG thumbnail. Unpacks the uncompressed raw strip, averages
/// down to ~160 px wide (demosaicing to color when the CFA pattern is known,
/// grayscale otherwise), gamma-lifts, encodes a BMP data URL. Returns None
/// when the raw isn't cheaply unpackable (compressed, tiled, odd bit depth)
/// — the filename label stays, same as before.
pub fn raw_preview_data_url(path: &Path) -> Option<String> {
    let report = inspect(path).ok()?;
    let m = &report.main;
    if m.compression != Some(COMPRESSION_UNCOMPRESSED) {
        return None;
    }
    if m.is_tiled() {
        return None;
    }
    match m.photometric {
        Some(32803) | Some(1) => {} // CFA Bayer or grayscale
        _ => return None,
    }
    if m.samples_per_pixel.unwrap_or(1) != 1 {
        return None;
    }
    let bits = *m.bits_per_sample.first()?;
    if bits != 8 && bits != 12 && bits != 16 {
        return None;
    }
    let (w, h) = (m.image_width?, m.image_length?);
    if w == 0 || h == 0 {
        return None;
    }
    let npix = w as u64 * h as u64;
    if npix > MAX_PREVIEW_PIXELS {
        return None;
    }
    if m.strip_offsets.len() != m.strip_byte_counts.len() {
        return None;
    }
    let mut bytes = Vec::new();
    for (&off, &cnt) in m.strip_offsets.iter().zip(m.strip_byte_counts.iter()) {
        let end = off as u64 + cnt as u64;
        if cnt == 0 || end > report.file_size {
            return None;
        }
        let file = File::open(path).ok()?;
        let mut r = Reader {
            inner: file,
            len: report.file_size,
            order: report.byte_order,
        };
        let mut buf = vec![0u8; cnt as usize];
        r.read_at(off as u64, &mut buf).ok()?;
        bytes.extend_from_slice(&buf);
    }
    let px = unpack_raw_strip(&bytes, bits, report.byte_order)?;
    // Strips are sometimes padded past the last pixel; keep exactly npix.
    if (px.len() as u64) < npix {
        return None;
    }
    let px = &px[..npix as usize];
    let out_w = PREVIEW_WIDTH.min(w).max(1);
    let out_h = ((h as u64 * out_w as u64 / w as u64).max(1)) as u32;
    // CFA Bayer with a known 2x2 pattern demosaics to color; anything else
    // (plain grayscale, or a pattern we can't trust) stays grayscale.
    let bmp = if m.photometric == Some(32803) {
        if let Some(pat) = m.cfa_pattern() {
            let (rgb, cw, ch) = downsample_demosaic(px, w, h, bits, pat)?;
            bmp_rgb_24(cw, ch, &rgb)?
        } else {
            let gray = downsample_gamma(px, w, h, bits)?;
            bmp_grayscale_8(out_w, out_h, &gray)?
        }
    } else {
        let gray = downsample_gamma(px, w, h, bits)?;
        bmp_grayscale_8(out_w, out_h, &gray)?
    };
    Some(format!("data:image/bmp;base64,{}", base64_encode(&bmp)))
}

// ── Parser internals ─────────────────────────────────────────────────

/// Largest single tag value we will read (guards against corrupt counts).
const MAX_VALUE_BYTES: u64 = 32 * 1024 * 1024;
/// Cap on SubIFD / next-IFD recursion.
const MAX_IFD_DEPTH: u8 = 4;
const MAX_IFD_CHAIN: usize = 64;

struct Reader<R: Read + Seek> {
    inner: R,
    len: u64,
    order: ByteOrder,
}

impl<R: Read + Seek> Reader<R> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), String> {
        let end = offset
            .checked_add(buf.len() as u64)
            .filter(|&end| end <= self.len)
            .ok_or_else(|| format!("read at offset {offset} runs past end of file"))?;
        let _ = end;
        self.inner
            .seek(SeekFrom::Start(offset))
            .map_err(|e| e.to_string())?;
        self.inner.read_exact(buf).map_err(|e| e.to_string())
    }

    fn u16_from(&self, b: &[u8]) -> u16 {
        let arr = [b[0], b[1]];
        match self.order {
            ByteOrder::Little => u16::from_le_bytes(arr),
            ByteOrder::Big => u16::from_be_bytes(arr),
        }
    }

    fn u32_from(&self, b: &[u8]) -> u32 {
        let arr = [b[0], b[1], b[2], b[3]];
        match self.order {
            ByteOrder::Little => u32::from_le_bytes(arr),
            ByteOrder::Big => u32::from_be_bytes(arr),
        }
    }

    fn u16_at(&mut self, offset: u64) -> Result<u16, String> {
        let mut b = [0u8; 2];
        self.read_at(offset, &mut b)?;
        Ok(self.u16_from(&b))
    }

    fn u32_at(&mut self, offset: u64) -> Result<u32, String> {
        let mut b = [0u8; 4];
        self.read_at(offset, &mut b)?;
        Ok(self.u32_from(&b))
    }

    fn read_value(&mut self, typ: u16, count: u32, field: &[u8; 4]) -> Result<TagValue, String> {
        let size: u64 = match typ {
            1 | 2 | 6 | 7 => 1,          // BYTE, ASCII, SBYTE, UNDEFINED
            3 | 8 => 2,                 // SHORT, SSHORT
            4 | 9 | 11 | 13 => 4,       // LONG, SLONG, FLOAT, IFD
            5 | 10 | 12 => 8,           // RATIONAL, SRATIONAL, DOUBLE
            _ => return Err(format!("unsupported TIFF field type {typ}")),
        };
        let total = count as u64 * size;
        if total == 0 {
            return Ok(TagValue::Byte(Vec::new()));
        }
        let raw: Vec<u8> = if total <= 4 {
            field[..total as usize].to_vec()
        } else {
            if total > MAX_VALUE_BYTES {
                return Err(format!("tag value too large ({total} bytes)"));
            }
            let off = self.u32_from(field);
            let mut buf = vec![0u8; total as usize];
            self.read_at(off as u64, &mut buf).map_err(|_| {
                format!("tag value at offset {off} runs past end of file")
            })?;
            buf
        };
        match typ {
            2 => {
                let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
                Ok(TagValue::Ascii(
                    String::from_utf8_lossy(&raw[..end]).into_owned(),
                ))
            }
            3 | 8 => {
                let mut v = Vec::with_capacity(count as usize);
                for c in raw.chunks_exact(2) {
                    v.push(self.u16_from(c));
                }
                Ok(TagValue::Short(v))
            }
            4 | 9 | 13 => {
                let mut v = Vec::with_capacity(count as usize);
                for c in raw.chunks_exact(4) {
                    v.push(self.u32_from(c));
                }
                Ok(TagValue::Long(v))
            }
            5 | 10 => {
                let mut v = Vec::with_capacity(count as usize);
                for c in raw.chunks_exact(8) {
                    v.push((self.u32_from(&c[0..4]), self.u32_from(&c[4..8])));
                }
                Ok(TagValue::Rational(v))
            }
            _ => Ok(TagValue::Byte(raw)),
        }
    }
}

// Byte/Ascii payloads are parsed for completeness but the engine only
// reads numeric tags today.
#[allow(dead_code)]
#[derive(Debug, Clone)]
enum TagValue {
    Byte(Vec<u8>),
    Ascii(String),
    Short(Vec<u16>),
    Long(Vec<u32>),
    Rational(Vec<(u32, u32)>),
}

fn first_u16(v: &TagValue) -> Option<u16> {
    match v {
        TagValue::Short(s) => s.first().copied(),
        TagValue::Long(l) => l.first().and_then(|&x| u16::try_from(x).ok()),
        _ => None,
    }
}

fn first_u32(v: &TagValue) -> Option<u32> {
    match v {
        TagValue::Short(s) => s.first().map(|&x| x as u32),
        TagValue::Long(l) => l.first().copied(),
        TagValue::Rational(r) => r.first().and_then(|&(n, d)| {
            if d != 0 {
                Some(n / d)
            } else {
                None
            }
        }),
        _ => None,
    }
}

fn all_u16(v: &TagValue) -> Vec<u16> {
    match v {
        TagValue::Short(s) => s.clone(),
        _ => Vec::new(),
    }
}

fn all_u32(v: &TagValue) -> Vec<u32> {
    match v {
        TagValue::Short(s) => s.iter().map(|&x| x as u32).collect(),
        TagValue::Long(l) => l.clone(),
        TagValue::Rational(r) => r
            .iter()
            .filter_map(|&(n, d)| if d != 0 { Some(n / d) } else { None })
            .collect(),
        _ => Vec::new(),
    }
}

fn pair_u32(v: &TagValue) -> Option<(u32, u32)> {
    let a = all_u32(v);
    if a.len() >= 2 {
        Some((a[0], a[1]))
    } else {
        None
    }
}

fn quad_u32(v: &TagValue) -> Option<[u32; 4]> {
    let a = all_u32(v);
    if a.len() >= 4 {
        Some([a[0], a[1], a[2], a[3]])
    } else {
        None
    }
}

fn quads_u32(v: &TagValue) -> Vec<[u32; 4]> {
    all_u32(v)
        .chunks_exact(4)
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect()
}

/// Parse one IFD. Returns the summary, SubIFD offsets, and the next-IFD offset.
fn parse_ifd<R: Read + Seek>(
    r: &mut Reader<R>,
    offset: u64,
    depth: u8,
) -> Result<(IfdSummary, Vec<u64>, u64), String> {
    if depth > MAX_IFD_DEPTH {
        return Err("IFD nesting too deep".to_string());
    }
    let count = r.u16_at(offset)?;
    let mut s = IfdSummary::default();
    let mut sub_offsets: Vec<u64> = Vec::new();

    for i in 0..count as u64 {
        let eoff = offset
            .checked_add(2 + i * 12)
            .ok_or_else(|| "IFD entry offset overflow".to_string())?;
        let tag = r.u16_at(eoff)?;
        let typ = r.u16_at(eoff + 2)?;
        let cnt = r.u32_at(eoff + 4)?;
        let mut field = [0u8; 4];
        r.read_at(eoff + 8, &mut field)?;
        let value = r.read_value(typ, cnt, &field)?;
        match tag {
            254 => s.new_subfile_type = first_u32(&value),
            256 => s.image_width = first_u32(&value),
            257 => s.image_length = first_u32(&value),
            258 => s.bits_per_sample = all_u16(&value),
            259 => s.compression = first_u16(&value),
            262 => s.photometric = first_u16(&value),
            273 => s.strip_offsets = all_u32(&value),
            277 => s.samples_per_pixel = first_u16(&value),
            278 => s.rows_per_strip = first_u32(&value),
            279 => s.strip_byte_counts = all_u32(&value),
            322 => s.tile_width = first_u32(&value),
            323 => s.tile_length = first_u32(&value),
            324 => s.tile_offsets = all_u32(&value),
            325 => s.tile_byte_counts = all_u32(&value),
            330 => sub_offsets = all_u32(&value).iter().map(|&o| o as u64).collect(),
            513 => s.jpeg_if_offset = first_u32(&value),
            514 => s.jpeg_if_length = first_u32(&value),
            33421 => {
                // CFARepeatPatternDim: two SHORTs (rows, cols), inline.
                if let TagValue::Short(d) = &value {
                    if d.len() == 2 {
                        s.cfa_repeat_dim = Some((d[0], d[1]));
                    }
                }
            }
            33422 => {
                // CFAPattern: one BYTE per repeat cell, inline for 2x2.
                if let TagValue::Byte(p) = &value {
                    if p.len() == 4 {
                        s.cfa_pattern_raw = Some([p[0], p[1], p[2], p[3]]);
                    }
                }
            }
            50829 => s.crop.active_area = quad_u32(&value),
            50830 => s.crop.masked_areas = quads_u32(&value),
            51111 => s.crop.default_crop_origin = pair_u32(&value),
            51112 => s.crop.default_crop_size = pair_u32(&value),
            _ => {}
        }
    }

    let next = r.u32_at(
        offset
            .checked_add(2 + count as u64 * 12)
            .ok_or_else(|| "IFD next-offset overflow".to_string())?,
    )?;
    Ok((s, sub_offsets, next as u64))
}

fn parse_tiff<R: Read + Seek>(r: &mut Reader<R>) -> Result<FileReport, String> {
    let mut hdr = [0u8; 8];
    r.read_at(0, &mut hdr)
        .map_err(|_| "could not read TIFF header".to_string())?;
    let order = match &hdr[0..2] {
        [0x49, 0x49] => ByteOrder::Little,
        [0x4D, 0x4D] => ByteOrder::Big,
        _ => return Err("not a TIFF file (bad byte-order mark)".to_string()),
    };
    r.order = order;
    let magic = r.u16_from(&hdr[2..4]);
    if magic == 43 {
        return Err("BigTIFF is not supported".to_string());
    }
    if magic != 42 {
        return Err(format!("not a TIFF file (bad magic {magic})"));
    }
    let first = r.u32_from(&hdr[4..8]);

    let (main, sub_offsets, mut next) = parse_ifd(r, first as u64, 0)?;
    let mut sub_ifds = Vec::new();
    for off in sub_offsets {
        let (s, _, _) = parse_ifd(r, off, 1)?;
        sub_ifds.push(s);
    }
    // Follow the next-IFD chain too (some writers chain IFDs instead of SubIFDs).
    let mut chained = 0;
    while next != 0 && chained < MAX_IFD_CHAIN {
        let (s, s_subs, n) = parse_ifd(r, next, 0)?;
        for off in s_subs {
            let (ss, _, _) = parse_ifd(r, off, 1)?;
            sub_ifds.push(ss);
        }
        sub_ifds.push(s);
        next = n;
        chained += 1;
    }

    // Promote the full-resolution frame to `main`. Adobe DNG Converter nests
    // the real frame in a SubIFD (tag 254 == 0, "full-resolution image") and
    // leaves a reduced thumbnail at IFD0; the ratio, skip-if-compressed, and
    // the verify pass all key off `main`, so it must resolve to the full-res
    // frame or they silently read the thumbnail. Prefer the explicit 254==0
    // marker (largest frame wins on ties); when no IFD carries the tag, keep
    // IFD0 — plain TIFFs and frames written at IFD0 are unaffected.
    let mut all: Vec<IfdSummary> = Vec::with_capacity(sub_ifds.len() + 1);
    all.push(main);
    all.extend(sub_ifds);
    let mut best: Option<(usize, u64)> = None;
    for (i, ifd) in all.iter().enumerate() {
        if ifd.new_subfile_type == Some(0) {
            let area = (ifd.image_width.unwrap_or(0) as u64)
                .saturating_mul(ifd.image_length.unwrap_or(0) as u64);
            if best.map_or(true, |(_, ba)| area > ba) {
                best = Some((i, area));
            }
        }
    }
    let (main, sub_ifds) = match best.map(|(i, _)| i) {
        Some(i) if i != 0 => {
            let mut v = all;
            let promoted = v.remove(i);
            (promoted, v) // v[0] is the old IFD0, now a sub-IFD beside the rest
        }
        _ => {
            let main = all.remove(0);
            (main, all)
        }
    };

    Ok(FileReport {
        byte_order: order,
        file_size: r.len,
        main,
        sub_ifds,
    })
}

// ── Unit tests (synthetic TIFFs) ─────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Minimal in-memory TIFF builder for tests.
    struct Tif {
        buf: Vec<u8>,
        le: bool,
        /// (patch position, payload) for values longer than 4 bytes
        deferred: Vec<(usize, Vec<u8>)>,
    }

    impl Tif {
        fn new(le: bool) -> Self {
            let mut t = Tif {
                buf: Vec::new(),
                le,
                deferred: Vec::new(),
            };
            t.buf.extend_from_slice(if le { b"II" } else { b"MM" });
            t.u16(42);
            t.u32(8); // first IFD at offset 8
            t
        }

        fn u16(&mut self, v: u16) {
            self.buf
                .extend_from_slice(&if self.le { v.to_le_bytes() } else { v.to_be_bytes() });
        }

        fn u32(&mut self, v: u32) {
            self.buf
                .extend_from_slice(&if self.le { v.to_le_bytes() } else { v.to_be_bytes() });
        }

        fn s16(&self, v: u16) -> [u8; 2] {
            if self.le {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        }

        fn s32(&self, v: u32) -> [u8; 4] {
            if self.le {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        }

        fn shorts(&self, vals: &[u16]) -> Vec<u8> {
            let mut b = Vec::new();
            for &v in vals {
                b.extend_from_slice(&self.s16(v));
            }
            b
        }

        fn longs(&self, vals: &[u32]) -> Vec<u8> {
            let mut b = Vec::new();
            for &v in vals {
                b.extend_from_slice(&self.s32(v));
            }
            b
        }

        /// One IFD entry. `payload` is the raw value bytes (count × type size);
        /// values longer than 4 bytes are appended after the IFD.
        fn entry(&mut self, tag: u16, typ: u16, count: u32, payload: &[u8]) {
            self.u16(tag);
            self.u16(typ);
            self.u32(count);
            if payload.len() <= 4 {
                let mut f = [0u8; 4];
                f[..payload.len()].copy_from_slice(payload);
                self.buf.extend_from_slice(&f);
            } else {
                let patch_at = self.buf.len();
                self.u32(0);
                self.deferred.push((patch_at, payload.to_vec()));
            }
        }

        /// Entry whose 4-byte field is an offset patched later; returns the
        /// patch position.
        fn entry_offset_placeholder(&mut self, tag: u16, typ: u16, count: u32) -> usize {
            self.u16(tag);
            self.u16(typ);
            self.u32(count);
            let at = self.buf.len();
            self.u32(0);
            at
        }

        fn patch_u32(&mut self, at: usize, v: u32) {
            let b = self.s32(v);
            self.buf[at..at + 4].copy_from_slice(&b);
        }

        fn begin_ifd(&mut self, count: u16) {
            self.u16(count);
        }

        fn end_ifd(&mut self, next: u32) {
            self.u32(next);
        }

        fn finish(mut self) -> Vec<u8> {
            // drain the mutable borrow before reading through &self
            let deferred: Vec<_> = self.deferred.drain(..).collect();
            for (patch_at, payload) in deferred {
                let off = self.buf.len() as u32;
                let b = self.s32(off);
                self.buf[patch_at..patch_at + 4].copy_from_slice(&b);
                self.buf.extend_from_slice(&payload);
            }
            self.buf
        }
    }

    fn parse(bytes: &[u8]) -> Result<FileReport, String> {
        let len = bytes.len() as u64;
        let mut r = Reader {
            inner: Cursor::new(bytes.to_vec()),
            len,
            order: ByteOrder::Little,
        };
        parse_tiff(&mut r)
    }

    /// Little-endian stripped TIFF with a JPEG thumbnail SubIFD.
    fn stripped_tiff() -> Vec<u8> {
        let mut t = Tif::new(true);
        t.begin_ifd(10);
        t.entry(256, 3, 1, &t.s16(6000)); // ImageWidth
        t.entry(257, 3, 1, &t.s16(4000)); // ImageLength
        t.entry(258, 3, 1, &t.s16(16)); // BitsPerSample
        t.entry(259, 3, 1, &t.s16(1)); // Compression = Uncompressed
        t.entry(262, 3, 1, &t.s16(32803)); // PhotometricInterpretation = CFA
        let p = t.longs(&[1000, 5000]);
        t.entry(273, 4, 2, &p); // StripOffsets
        t.entry(277, 3, 1, &t.s16(1)); // SamplesPerPixel
        t.entry(278, 4, 1, &t.s32(2000)); // RowsPerStrip
        let p = t.longs(&[4000, 4000]);
        t.entry(279, 4, 2, &p); // StripByteCounts
        let sub_patch = t.entry_offset_placeholder(330, 4, 1); // SubIFDs
        t.end_ifd(0);

        let sub_at = t.buf.len() as u32;
        t.patch_u32(sub_patch, sub_at);
        t.begin_ifd(4);
        t.entry(256, 3, 1, &t.s16(160));
        t.entry(257, 3, 1, &t.s16(120));
        t.entry(259, 3, 1, &t.s16(7)); // Compression = JPEG
        t.entry(279, 4, 1, &t.s32(8192));
        t.end_ifd(0);
        t.finish()
    }

    /// Big-endian tiled DNG-like file with crop tags.
    fn tiled_dng() -> Vec<u8> {
        let mut t = Tif::new(false);
        t.begin_ifd(11);
        t.entry(256, 4, 1, &t.s32(6000)); // ImageWidth
        t.entry(257, 4, 1, &t.s32(4000)); // ImageLength
        t.entry(258, 3, 1, &t.s16(16)); // BitsPerSample
        t.entry(259, 3, 1, &t.s16(7)); // Compression = JPEG
        let p = t.longs(&[4096, 8192, 12288, 16384]);
        t.entry(324, 4, 4, &p); // TileOffsets
        let p = t.longs(&[1000, 1000, 1000, 1000]);
        t.entry(325, 4, 4, &p); // TileByteCounts
        t.entry(322, 4, 1, &t.s32(3000)); // TileWidth
        t.entry(323, 4, 1, &t.s32(2000)); // TileLength
        let p = t.shorts(&[0, 0]);
        t.entry(51111, 3, 2, &p); // DefaultCropOrigin
        let p = t.shorts(&[6000, 4000]);
        t.entry(51112, 3, 2, &p); // DefaultCropSize
        let p = t.longs(&[0, 0, 4000, 6000]);
        t.entry(50829, 4, 4, &p); // ActiveArea
        t.end_ifd(0);
        t.finish()
    }

    #[test]
    fn parses_little_endian_stripped_tiff() {
        let rep = parse(&stripped_tiff()).expect("should parse");
        assert_eq!(rep.byte_order, ByteOrder::Little);
        let m = &rep.main;
        assert_eq!(m.image_width, Some(6000));
        assert_eq!(m.image_length, Some(4000));
        assert_eq!(m.bits_per_sample, vec![16]);
        assert_eq!(m.compression, Some(1));
        assert_eq!(m.compression_label(), "Uncompressed");
        assert!(!m.is_tiled());
        assert!(!m.already_compressed());
        assert_eq!(m.strip_offsets, vec![1000, 5000]);
        assert_eq!(m.strip_byte_counts, vec![4000, 4000]);
        assert_eq!(m.rows_per_strip, Some(2000));
        // headline bytes come from StripByteCounts here
        assert_eq!(m.image_data_bytes(), 8000);

        assert_eq!(rep.sub_ifds.len(), 1);
        let thumb = &rep.sub_ifds[0];
        assert_eq!(thumb.image_width, Some(160));
        assert_eq!(thumb.compression, Some(7));
        assert_eq!(thumb.compression_label(), "JPEG");
        assert!(thumb.already_compressed());
    }

    /// Adobe DNG Converter layout: reduced thumbnail at IFD0, full-resolution
    /// lossless frame nested in a SubIFD (tag 254 == 0). Regression for the
    /// bug where main=IFD0 made ratio / skip / verify read the thumbnail.
    fn adobe_layout() -> Vec<u8> {
        let mut t = Tif::new(true);
        t.begin_ifd(7);
        t.entry(254, 4, 1, &t.s32(2));       // NewSubfileType = 2 (thumbnail)
        t.entry(256, 4, 1, &t.s32(256));     // ImageWidth
        t.entry(257, 4, 1, &t.s32(144));     // ImageLength
        t.entry(258, 3, 1, &t.s16(8));       // BitsPerSample
        t.entry(259, 3, 1, &t.s16(1));       // Compression = none
        t.entry(279, 4, 1, &t.s32(110592));  // StripByteCounts
        let sub_patch = t.entry_offset_placeholder(330, 4, 1); // SubIFDs
        t.end_ifd(0);

        let sub_at = t.buf.len() as u32;
        t.patch_u32(sub_patch, sub_at);
        t.begin_ifd(7);
        t.entry(254, 4, 1, &t.s32(0));       // NewSubfileType = 0 (full-res)
        t.entry(256, 4, 1, &t.s32(1936));
        t.entry(257, 4, 1, &t.s32(1090));
        t.entry(258, 3, 1, &t.s16(16));
        t.entry(259, 3, 1, &t.s16(7));       // Compression = lossless JPEG
        t.entry(325, 4, 1, &t.s32(21939));   // TileByteCounts
        t.entry(322, 4, 1, &t.s32(256));     // TileWidth
        t.end_ifd(0);
        t.finish()
    }

    #[test]
    fn promotes_full_res_subifd_to_main() {
        let rep = parse(&adobe_layout()).expect("should parse");
        // main must be the full-resolution frame, not the IFD0 thumbnail
        assert_eq!(rep.main.new_subfile_type, Some(0));
        assert_eq!(rep.main.image_width, Some(1936));
        assert_eq!(rep.main.image_length, Some(1090));
        assert_eq!(rep.main.compression, Some(7));
        assert!(rep.main.already_compressed());
        assert_eq!(rep.main.image_data_bytes(), 21939);
        // the thumbnail is demoted to a sub-IFD
        assert_eq!(rep.sub_ifds.len(), 1);
        assert_eq!(rep.sub_ifds[0].new_subfile_type, Some(2));
        assert_eq!(rep.sub_ifds[0].image_width, Some(256));
    }

    #[test]
    fn parses_big_endian_tiled_dng() {
        // synthetic tile offsets point into a larger file; pad so verify passes
        let mut bytes = tiled_dng();
        bytes.resize(18_000, 0);
        let rep = parse(&bytes).expect("should parse");
        assert_eq!(rep.byte_order, ByteOrder::Big);
        let m = &rep.main;
        assert!(m.is_tiled());
        assert_eq!(m.tile_width, Some(3000));
        assert_eq!(m.tile_length, Some(2000));
        assert_eq!(m.tile_offsets.len(), 4);
        assert_eq!(m.tile_byte_counts, vec![1000, 1000, 1000, 1000]);
        // headline bytes come from TileByteCounts, not strips
        assert_eq!(m.image_data_bytes(), 4000);
        assert!(m.already_compressed());
        // 6000/3000 × 4000/2000 = 2×2 tiles
        assert_eq!(m.tile_grid_count(), Some(4));
        // crop tags
        assert_eq!(m.crop.default_crop_origin, Some((0, 0)));
        assert_eq!(m.crop.default_crop_size, Some((6000, 4000)));
        assert_eq!(m.crop.active_area, Some([0, 0, 4000, 6000]));
        assert!(m.verify(rep.file_size).is_empty());
    }

    #[test]
    fn verify_accepts_good_stripped_file() {
        // give the synthetic file a plausible size so data ranges fit
        let mut bytes = stripped_tiff();
        bytes.resize(9000, 0);
        let rep = parse(&bytes).expect("should parse");
        assert!(rep.main.verify(rep.file_size).is_empty());
    }

    #[test]
    fn verify_catches_count_mismatch() {
        let mut t = Tif::new(true);
        t.begin_ifd(7);
        t.entry(256, 3, 1, &t.s16(100));
        t.entry(257, 3, 1, &t.s16(100));
        t.entry(258, 3, 1, &t.s16(8));
        t.entry(259, 3, 1, &t.s16(1));
        let p = t.longs(&[500, 600]);
        t.entry(273, 4, 2, &p); // 2 offsets …
        let p = t.longs(&[700]);
        t.entry(279, 4, 1, &p); // … but 1 byte count
        t.entry(278, 4, 1, &t.s32(100));
        t.end_ifd(0);
        let rep = parse(&t.finish()).expect("should parse");
        let problems = rep.main.verify(rep.file_size);
        assert!(
            problems.iter().any(|p| p.contains("offsets (2) != strip byte counts (1)"))
        );
    }

    #[test]
    fn verify_catches_data_past_eof() {
        let mut t = Tif::new(true);
        t.begin_ifd(7);
        t.entry(256, 3, 1, &t.s16(100));
        t.entry(257, 3, 1, &t.s16(100));
        t.entry(258, 3, 1, &t.s16(8));
        t.entry(259, 3, 1, &t.s16(1));
        t.entry(273, 4, 1, &t.s32(999_999)); // offset past EOF
        t.entry(279, 4, 1, &t.s32(100));
        t.entry(278, 4, 1, &t.s32(100));
        t.end_ifd(0);
        let rep = parse(&t.finish()).expect("should parse");
        let problems = rep.main.verify(rep.file_size);
        assert!(problems.iter().any(|p| p.contains("past end of file")));
    }

    #[test]
    fn verify_catches_bad_crop_and_tile_grid() {
        let mut t = Tif::new(true);
        t.begin_ifd(9);
        t.entry(256, 3, 1, &t.s16(100));
        t.entry(257, 3, 1, &t.s16(100));
        t.entry(258, 3, 1, &t.s16(8));
        t.entry(259, 3, 1, &t.s16(7));
        let p = t.longs(&[500, 600, 700]);
        t.entry(324, 4, 3, &p); // 3 tiles …
        let p = t.longs(&[10, 10, 10]);
        t.entry(325, 4, 3, &p);
        t.entry(322, 4, 1, &t.s32(64));
        t.entry(323, 4, 1, &t.s32(64)); // … but grid is 2×2 = 4
        let p = t.shorts(&[200, 200]);
        t.entry(51112, 3, 2, &p); // crop bigger than the image
        t.end_ifd(0);
        let rep = parse(&t.finish()).expect("should parse");
        let problems = rep.main.verify(rep.file_size);
        assert!(problems.iter().any(|p| p.contains("tile count 3")));
        assert!(problems.iter().any(|p| p.contains("DefaultCropSize")));
    }

    #[test]
    fn rejects_garbage_and_truncation() {
        assert!(parse(&[]).is_err());
        assert!(parse(b"NOPE").is_err());
        assert!(parse(b"II\x2a\x00").is_err()); // truncated header
        // bad magic
        let mut bad = Tif::new(true).finish();
        bad[2] = 0x2b;
        bad[3] = 0x00;
        assert!(parse(&bad).is_err());
        // BigTIFF rejected, not misparsed
        let mut big = Tif::new(true).finish();
        big[2] = 43;
        assert!(parse(&big).unwrap_err().contains("BigTIFF"));
    }

    #[test]
    fn huge_count_fails_without_allocating() {
        let mut t = Tif::new(true);
        t.begin_ifd(1);
        // LONG count u32::MAX → 16 GB value; must error, not allocate
        t.entry(279, 4, u32::MAX, &[]);
        t.end_ifd(0);
        let err = parse(&t.finish()).unwrap_err();
        assert!(err.contains("too large") || err.contains("past end of file"));
    }

    #[test]
    fn compression_names_cover_dng_codes() {
        assert_eq!(compression_name(1), "Uncompressed");
        assert_eq!(compression_name(7), "JPEG");
        assert_eq!(compression_name(8), "Deflate");
        assert_eq!(compression_name(34892), "Lossy JPEG (DNG)");
        assert_eq!(compression_name(9999), "Unknown");
    }

    #[test]
    fn parses_new_subfile_type_tag() {
        let mut t = Tif::new(true);
        t.begin_ifd(2);
        t.entry(254, 4, 1, &t.s32(1)); // NewSubfileType = thumbnail
        t.entry(259, 3, 1, &t.s16(7));
        t.end_ifd(0);
        let rep = parse(&t.finish()).expect("should parse");
        assert_eq!(rep.main.new_subfile_type, Some(1));
    }

    #[test]
    fn thumbnail_bit_semantics() {
        let mut s = IfdSummary::default();
        assert!(!s.is_thumbnail());
        s.new_subfile_type = Some(1);
        assert!(s.is_thumbnail());
        s.new_subfile_type = Some(3); // bits 0+1 still a thumbnail
        assert!(s.is_thumbnail());
        s.new_subfile_type = Some(2); // bit 1 only, not a thumbnail
        assert!(!s.is_thumbnail());
    }

    #[test]
    fn base64_encoder_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        // JPEG SOI marker
        assert_eq!(base64_encode(&[0xFF, 0xD8, 0xFF]), "/9j/");
    }

    #[test]
    fn unpacks_12bit_packed_pairs() {
        let p0: u16 = 0xABC;
        let p1: u16 = 0x123;
        let b0 = (p0 & 0xFF) as u8;
        let b1 = (((p0 >> 8) & 0x0F) | ((p1 & 0x0F) << 4)) as u8;
        let b2 = ((p1 >> 4) & 0xFF) as u8;
        let px = unpack_raw_strip(&[b0, b1, b2], 12, ByteOrder::Little).unwrap();
        assert_eq!(px, vec![p0, p1]);
        assert!(unpack_raw_strip(&[0u8; 3], 10, ByteOrder::Little).is_none());
    }

    #[test]
    fn bmp_grayscale_header_sane() {
        let px = vec![0u8, 85, 170, 255, 255, 170, 85, 0]; // 4x2
        let bmp = bmp_grayscale_8(4, 2, &px).unwrap();
        assert_eq!(&bmp[0..2], b"BM");
        let file_size = u32::from_le_bytes(bmp[2..6].try_into().unwrap());
        assert_eq!(file_size as usize, bmp.len());
        let data_off = u32::from_le_bytes(bmp[10..14].try_into().unwrap());
        assert_eq!(data_off, 54 + 1024);
        let w = u32::from_le_bytes(bmp[18..22].try_into().unwrap());
        let h = u32::from_le_bytes(bmp[22..26].try_into().unwrap());
        assert_eq!((w, h), (4, 2));
        // palette entry 255 is white
        assert_eq!(&bmp[54 + 255 * 4..54 + 256 * 4], &[255, 255, 255, 0]);
        assert!(bmp_grayscale_8(4, 2, &[0u8; 7]).is_none()); // short buffer
    }

    #[test]
    fn gamma_lift_brightens_linear_midtones() {
        let px = vec![2048u16; 4]; // half-scale 12-bit
        let out = downsample_gamma(&px, 2, 2, 12).unwrap();
        assert_eq!(out.len(), 4);
        // (0.5)^(1/2.2) ~= 0.73 -> ~186
        assert!(out[0] > 150 && out[0] < 220, "got {}", out[0]);
    }

    #[test]
    fn synthesized_preview_from_bayer_dng() {
        // 8x4 packed 12-bit Bayer ramp, written as a real temp file.
        // No thumbnail IFD anywhere: the fallback must do the work.
        let (w, h): (u32, u32) = (8, 4);
        let mut strip = Vec::new();
        for i in 0..w * h / 2 {
            let p0 = (i * 128 % 4096) as u16;
            let p1 = ((i * 128 + 64) % 4096) as u16;
            strip.push((p0 & 0xFF) as u8);
            strip.push((((p0 >> 8) & 0x0F) | ((p1 & 0x0F) << 4)) as u8);
            strip.push(((p1 >> 4) & 0xFF) as u8);
        }
        let mut t = Tif::new(true);
        t.begin_ifd(9);
        t.entry(254, 4, 1, &t.s32(0)); // raw frame, not a thumbnail
        t.entry(256, 4, 1, &t.s32(w));
        t.entry(257, 4, 1, &t.s32(h));
        t.entry(258, 3, 1, &t.s16(12));
        t.entry(259, 3, 1, &t.s16(1)); // uncompressed
        t.entry(262, 3, 1, &t.s16(32803)); // CFA
        let off_patch = t.entry_offset_placeholder(273, 4, 1);
        t.entry(277, 3, 1, &t.s16(1));
        t.entry(279, 4, 1, &t.s32(strip.len() as u32));
        t.end_ifd(0);
        let strip_at = t.buf.len() as u32;
        t.buf.extend_from_slice(&strip);
        t.patch_u32(off_patch, strip_at);
        let bytes = t.finish();

        let p = std::env::temp_dir().join("cmprssr-test-bayer.dng");
        std::fs::write(&p, &bytes).expect("write temp dng");
        assert!(thumbnail_jpeg(&p).is_none()); // no embedded JPEG, as intended
        let url = thumbnail_data_url(&p).expect("synthesized preview");
        assert!(url.starts_with("data:image/bmp;base64,"));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn cfa_pattern_parses_rggb() {
        let mut t = Tif::new(true);
        t.begin_ifd(3);
        t.entry(262, 3, 1, &t.s16(32803));
        let p = t.shorts(&[2, 2]);
        t.entry(33421, 3, 2, &p); // CFARepeatPatternDim = 2x2
        t.entry(33422, 1, 4, &[0, 1, 1, 2]); // RGGB
        t.end_ifd(0);
        let rep = parse(&t.finish()).expect("should parse");
        assert_eq!(rep.main.cfa_pattern(), Some([0, 1, 1, 2]));
    }

    #[test]
    fn cfa_pattern_rejects_non_bayer() {
        // 2x3 repeat: not a plain Bayer quad.
        let mut t = Tif::new(true);
        t.begin_ifd(2);
        let p = t.shorts(&[2, 3]);
        t.entry(33421, 3, 2, &p);
        t.entry(33422, 1, 4, &[0, 1, 1, 2]);
        t.end_ifd(0);
        assert_eq!(parse(&t.finish()).unwrap().main.cfa_pattern(), None);

        // All-red 2x2: values in range but not a Bayer mosaic.
        let mut t = Tif::new(true);
        t.begin_ifd(2);
        let p = t.shorts(&[2, 2]);
        t.entry(33421, 3, 2, &p);
        t.entry(33422, 1, 4, &[0, 0, 0, 0]);
        t.end_ifd(0);
        assert_eq!(parse(&t.finish()).unwrap().main.cfa_pattern(), None);

        // Missing tags entirely: no guessing.
        let mut t = Tif::new(true);
        t.begin_ifd(1);
        t.entry(262, 3, 1, &t.s16(32803));
        t.end_ifd(0);
        assert_eq!(parse(&t.finish()).unwrap().main.cfa_pattern(), None);
    }

    #[test]
    fn demosaic_red_frame_comes_out_red() {
        // 8x8 RGGB, only red photosites lit. At 1:1 each output pixel is one
        // photosite, so this checks the phase mapping precisely.
        let (w, h) = (8u32, 8u32);
        let pat = [0u8, 1, 1, 2];
        let mut px = vec![0u16; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                let c = pat[((y & 1) * 2 + (x & 1)) as usize];
                px[(y * w + x) as usize] = if c == 0 { 4000 } else { 100 };
            }
        }
        let (rgb, ow, oh) = downsample_demosaic(&px, w, h, 12, pat).unwrap();
        assert_eq!((ow, oh), (8, 8));
        // (0,0) is pattern[0] = red.
        assert!(rgb[0] > rgb[1] + 60 && rgb[0] > rgb[2] + 60,
            "r={} g={} b={}", rgb[0], rgb[1], rgb[2]);
        // (7,7) is pattern[3] = blue.
        let k = (7 * 8 + 7) as usize * 3;
        assert!(rgb[k + 2] > rgb[k] + 30 && rgb[k + 2] > rgb[k + 1] + 30,
            "r={} g={} b={}", rgb[k], rgb[k + 1], rgb[k + 2]);
    }

    #[test]
    fn demosaic_uniform_white_stays_white() {
        // 320x320 -> 160x160, each output box is exactly one Bayer quad.
        let (w, h) = (320u32, 320u32);
        let px = vec![4000u16; (w * h) as usize];
        let (rgb, ow, oh) = downsample_demosaic(&px, w, h, 12, [0, 1, 1, 2]).unwrap();
        assert_eq!((ow, oh), (160, 160));
        assert_eq!(rgb.len(), 160 * 160 * 3);
        for (i, &v) in rgb.iter().enumerate() {
            assert!(v > 240, "channel {i} = {v}");
        }
    }

    #[test]
    fn rgb_bmp_is_24bit_bgr() {
        // 2x1: stride = 8 (6 data + 2 pad).
        let bmp = bmp_rgb_24(2, 1, &[10, 20, 30, 40, 50, 60]).unwrap();
        assert_eq!(&bmp[0..2], b"BM");
        let file_size = u32::from_le_bytes(bmp[2..6].try_into().unwrap());
        assert_eq!(file_size as usize, 54 + 8);
        assert_eq!(file_size as usize, bmp.len());
        let data_off = u32::from_le_bytes(bmp[10..14].try_into().unwrap());
        assert_eq!(data_off, 54);
        let bpp = u16::from_le_bytes(bmp[28..30].try_into().unwrap());
        assert_eq!(bpp, 24);
        // BGR order, then 2 pad bytes.
        assert_eq!(&bmp[54..62], &[30, 20, 10, 60, 50, 40, 0, 0]);
        assert!(bmp_rgb_24(2, 1, &[0u8; 5]).is_none()); // short buffer
    }

    /// Test-only base64 decoder (the crate only needs the encoder).
    fn b64_decode(s: &str) -> Vec<u8> {
        let mut out = Vec::new();
        let mut acc = 0u32;
        let mut bits = 0;
        for ch in s.bytes() {
            let v = match ch {
                b'A'..=b'Z' => ch - b'A',
                b'a'..=b'z' => ch - b'a' + 26,
                b'0'..=b'9' => ch - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' => break,
                _ => continue,
            } as u32;
            acc = (acc << 6) | v;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
                acc &= (1 << bits) - 1;
            }
        }
        out
    }

    #[test]
    fn synthesized_color_preview_from_bayer_dng() {
        // 8x8 packed 12-bit Bayer *with* CFA tags: the fallback must demosaic
        // to color. Pattern [1,0,2,1] proves the tag is actually read.
        let (w, h): (u32, u32) = (8, 8);
        let mut strip = Vec::new();
        for i in 0..w * h / 2 {
            let p0 = (i * 251 % 4096) as u16;
            let p1 = ((i * 251 + 999) % 4096) as u16;
            strip.push((p0 & 0xFF) as u8);
            strip.push((((p0 >> 8) & 0x0F) | ((p1 & 0x0F) << 4)) as u8);
            strip.push(((p1 >> 4) & 0xFF) as u8);
        }
        let mut t = Tif::new(true);
        t.begin_ifd(11);
        t.entry(254, 4, 1, &t.s32(0));
        t.entry(256, 4, 1, &t.s32(w));
        t.entry(257, 4, 1, &t.s32(h));
        t.entry(258, 3, 1, &t.s16(12));
        t.entry(259, 3, 1, &t.s16(1)); // uncompressed
        t.entry(262, 3, 1, &t.s16(32803)); // CFA
        let p = t.shorts(&[2, 2]);
        t.entry(33421, 3, 2, &p);
        t.entry(33422, 1, 4, &[1, 0, 2, 1]);
        let off_patch = t.entry_offset_placeholder(273, 4, 1);
        t.entry(277, 3, 1, &t.s16(1));
        t.entry(279, 4, 1, &t.s32(strip.len() as u32));
        t.end_ifd(0);
        let strip_at = t.buf.len() as u32;
        t.buf.extend_from_slice(&strip);
        t.patch_u32(off_patch, strip_at);
        let bytes = t.finish();

        let pth = std::env::temp_dir().join("cmprssr-test-bayer-color.dng");
        std::fs::write(&pth, &bytes).expect("write temp dng");
        let url = thumbnail_data_url(&pth).expect("synthesized color preview");
        assert!(url.starts_with("data:image/bmp;base64,"));
        let bmp = b64_decode(&url["data:image/bmp;base64,".len()..]);
        assert_eq!(&bmp[0..2], b"BM");
        let bpp = u16::from_le_bytes(bmp[28..30].try_into().unwrap());
        assert_eq!(bpp, 24, "color fallback must emit 24-bit BMP");
        let _ = std::fs::remove_file(&pth);
    }
}
