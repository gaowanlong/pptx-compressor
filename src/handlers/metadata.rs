//! 媒体元数据保真工具。
//!
//! 重编码会丢掉源文件里的 EXIF 方向 / ICC 色彩配置 / 文本与色彩块。这里只做
//! **字节级搬运**：从源文件抽出元数据段 / 块，原样插进重新编码后的候选文件，
//! 不解释其内容。这样既不会受编码器对各类元数据支持程度的限制，也不会漏掉
//! 编码器不认识的历史标签。
//!
//! 所有解析都是带边界检查的纯函数；遇到畸形数据返回空集合或 `None`，调用方
//! 降级为「不注入」，绝不产出损坏文件。搬运失败只会让候选文件少一点元数据，
//! 不会影响像素数据本身。

/// JPEG APPn 段（不含 marker 与长度字段）的原始载荷。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JpegMetadataSegment {
    /// APPn marker，例如 0xE1 = APP1。
    pub marker: u8,
    pub payload: Vec<u8>,
}

/// 需要保留的 APPn marker：
/// APP1（Exif / XMP）、APP2（ICC_PROFILE 分块）、APP13（Photoshop / IPTC）。
///
/// 刻意不搬 APP0：编码器会写自己的 JFIF APP0，重复反而容易让解码器困惑。
fn is_metadata_marker(marker: u8) -> bool {
    matches!(marker, 0xE1 | 0xE2 | 0xED)
}

/// 从 JPEG 中按出现顺序收集需要保留的 APPn 段。
///
/// 遇到 SOS（扫描数据开始）即停止：其后是熵编码字节流，不再有段结构，
/// 继续按段解析会把图片数据误当成元数据。
pub fn jpeg_metadata_segments(data: &[u8]) -> Vec<JpegMetadataSegment> {
    let mut out = Vec::new();
    if data.len() < 2 || data[0] != 0xFF || data[1] != 0xD8 {
        return out;
    }

    let mut pos = 2usize;
    while pos + 4 <= data.len() {
        if data[pos] != 0xFF {
            break;
        }
        // 连续的 0xFF 是合法的段间填充字节。
        let mut marker_pos = pos + 1;
        while marker_pos < data.len() && data[marker_pos] == 0xFF {
            marker_pos += 1;
        }
        if marker_pos >= data.len() {
            break;
        }
        let marker = data[marker_pos];
        let length_at = marker_pos + 1;

        // SOI / EOI / RSTn / TEM 没有长度字段。
        if marker == 0xD8 || marker == 0xD9 || matches!(marker, 0x01 | 0xD0..=0xD7) {
            pos = length_at;
            continue;
        }
        if marker == 0xDA {
            break; // SOS：后面是图像数据
        }
        if length_at + 2 > data.len() {
            break;
        }
        let length = u16::from_be_bytes([data[length_at], data[length_at + 1]]) as usize;
        if length < 2 || length_at + length > data.len() {
            break;
        }
        let payload_at = length_at + 2;
        let segment_end = length_at + length;
        if is_metadata_marker(marker) {
            out.push(JpegMetadataSegment {
                marker,
                payload: data[payload_at..segment_end].to_vec(),
            });
        }
        pos = segment_end;
    }
    out
}

/// 把元数据段插进重新编码后的 JPEG：紧跟 SOI；若有 APP0(JFIF) 则放在它之后。
/// 返回 `None` 表示输入不是可安全改写的 JPEG，调用方应保留原候选文件。
pub fn inject_jpeg_segments(
    jpeg: &[u8],
    segments: &[JpegMetadataSegment],
) -> Option<Vec<u8>> {
    if jpeg.len() < 2 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 {
        return None;
    }
    if segments.is_empty() {
        return Some(jpeg.to_vec());
    }

    let mut insert_at = 2usize;
    if jpeg.len() >= 6 && jpeg[2] == 0xFF && jpeg[3] == 0xE0 {
        let length = u16::from_be_bytes([jpeg[4], jpeg[5]]) as usize;
        if length < 2 || 4 + length > jpeg.len() {
            return None;
        }
        insert_at = 4 + length;
    }

    let mut out = Vec::with_capacity(jpeg.len() + 64 * segments.len());
    out.extend_from_slice(&jpeg[..insert_at]);
    for segment in segments {
        let length = segment.payload.len() + 2;
        if length > u16::MAX as usize {
            // 源段本应是合法 JPEG 段，超长只可能是畸形数据，保守跳过。
            continue;
        }
        out.push(0xFF);
        out.push(segment.marker);
        out.extend_from_slice(&(length as u16).to_be_bytes());
        out.extend_from_slice(&segment.payload);
    }
    out.extend_from_slice(&jpeg[insert_at..]);
    Some(out)
}

/// 读取 EXIF TIFF 里的方向标签（0x0112）。用于诊断与回归测试。
/// 入参是不含 `Exif\0\0` 前缀的 TIFF 字节。
pub fn exif_orientation(tiff: &[u8]) -> Option<u16> {
    if tiff.len() < 8 {
        return None;
    }
    let little = if &tiff[0..2] == b"II" {
        true
    } else if &tiff[0..2] == b"MM" {
        false
    } else {
        return None;
    };
    let u16_at = |at: usize| -> Option<u16> {
        let b = tiff.get(at..at + 2)?;
        Some(if little {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        })
    };
    let u32_at = |at: usize| -> Option<u32> {
        let b = tiff.get(at..at + 4)?;
        Some(if little {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        })
    };

    if u16_at(2)? != 42 {
        return None; // TIFF magic
    }
    let ifd = u32_at(4)? as usize;
    let count = u16_at(ifd)? as usize;
    for i in 0..count {
        let entry = ifd + 2 + i * 12;
        if u16_at(entry)? == 0x0112 {
            // SHORT 类型的值直接存在 value 字段的前两字节。
            return u16_at(entry + 8);
        }
    }
    None
}

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];

/// PNG 的单个元数据 chunk（类型 + 数据）。注入时重算 CRC。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PngMetadataChunk {
    pub kind: [u8; 4],
    pub data: Vec<u8>,
}

/// 与像素无关的 ancillary 元数据块，重编码后可安全原样保留。
///
/// 刻意排除 `tRNS` / `PLTE` / `bKGD` / `hIST` / `sBIT`：它们与调色板或色型
/// 绑定，重编码后色型可能变化，原样复制反而会让解码器报错。APNG 的
/// `acTL` / `fcTL` / `fdAT` 同样排除（本工具把 PNG 当静态图处理）。
fn is_metadata_chunk(kind: &[u8; 4], keep_phys: bool) -> bool {
    let keep = matches!(
        kind,
        b"tEXt" | b"zTXt" | b"iTXt" | b"iCCP" | b"sRGB" | b"gAMA" | b"cHRM" | b"tIME" | b"eXIf"
    );
    keep || (keep_phys && *kind == *b"pHYs")
}

/// 从 PNG 中按出现顺序收集需要保留的 ancillary 元数据块。
///
/// `keep_phys` 为 false 时丢弃 `pHYs`：图片一旦被缩放，物理像素尺寸（DPI）
/// 就失效了，留着反而会误导下游。
pub fn png_metadata_chunks(data: &[u8], keep_phys: bool) -> Vec<PngMetadataChunk> {
    let mut out = Vec::new();
    if !data.starts_with(&PNG_SIGNATURE) {
        return out;
    }

    let mut pos = PNG_SIGNATURE.len();
    while pos + 8 <= data.len() {
        let length =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let kind = [data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]];
        let data_at = pos + 8;
        let data_end = data_at + length;
        if data_end + 4 > data.len() {
            break;
        }
        if is_metadata_chunk(&kind, keep_phys) {
            out.push(PngMetadataChunk { kind, data: data[data_at..data_end].to_vec() });
        }
        if kind == *b"IEND" {
            break;
        }
        pos = data_end + 4;
    }
    out
}

/// PNG chunk 使用的 CRC-32（IEEE 802.3 多项式，覆盖类型 + 数据）。
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320u32 & mask);
        }
    }
    !crc
}

/// 把元数据块插进重新编码后的 PNG：紧跟 IHDR，位于 PLTE / IDAT 之前。
/// 色彩相关的 `sRGB` / `gAMA` / `cHRM` / `iCCP` 按规范必须出现在这个位置。
/// 返回 `None` 表示输入不是可安全改写的 PNG。
pub fn inject_png_chunks(png: &[u8], chunks: &[PngMetadataChunk]) -> Option<Vec<u8>> {
    if !png.starts_with(&PNG_SIGNATURE) {
        return None;
    }
    if chunks.is_empty() {
        return Some(png.to_vec());
    }

    let ihdr_at = PNG_SIGNATURE.len();
    if png.len() < ihdr_at + 12 {
        return None;
    }
    let length =
        u32::from_be_bytes([png[ihdr_at], png[ihdr_at + 1], png[ihdr_at + 2], png[ihdr_at + 3]])
            as usize;
    let kind = [png[ihdr_at + 4], png[ihdr_at + 5], png[ihdr_at + 6], png[ihdr_at + 7]];
    if kind != *b"IHDR" || length != 13 {
        return None;
    }
    let insert_at = ihdr_at + 8 + length + 4;
    if insert_at > png.len() {
        return None;
    }

    let mut out = Vec::with_capacity(png.len() + 24 * chunks.len());
    out.extend_from_slice(&png[..insert_at]);
    for chunk in chunks {
        let mut raw = Vec::with_capacity(4 + chunk.data.len());
        raw.extend_from_slice(&chunk.kind);
        raw.extend_from_slice(&chunk.data);
        let checksum = crc32(&raw);
        out.extend_from_slice(&(chunk.data.len() as u32).to_be_bytes());
        out.extend_from_slice(&raw);
        out.extend_from_slice(&checksum.to_be_bytes());
    }
    out.extend_from_slice(&png[insert_at..]);
    Some(out)
}
