//! 元数据搬运的纯函数回归测试：不依赖 GUI、FFmpeg 或网络。
//!
//! 用手工构造的 JPEG 段 / PNG 块验证「原样搬运」的边界行为，包括畸形输入
//! 降级、插入位置、CRC 与 EXIF 方向标签读取。

use pptx_compressor::handlers::metadata::{
    exif_orientation, inject_jpeg_segments, inject_png_chunks, jpeg_metadata_segments,
    png_metadata_chunks, JpegMetadataSegment, PngMetadataChunk,
};

fn jpeg_segment(marker: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0xFF, marker];
    out.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

fn jpeg_with(segments: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![0xFF, 0xD8];
    for segment in segments {
        out.extend_from_slice(segment);
    }
    out.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x02]); // DQT 占位
    out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02]); // SOS
    out.extend_from_slice(&[0x12, 0x34, 0x56]); // 伪图像数据
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&[0, 0, 0, 0]); // 源 CRC 不参与解析，占位即可
    out
}

const PNG_SIG: &[u8] = b"\x89PNG\r\n\x1a\n";

fn png_with(chunks: &[Vec<u8>]) -> Vec<u8> {
    let mut out = PNG_SIG.to_vec();
    out.extend_from_slice(&png_chunk(b"IHDR", &[0u8; 13]));
    for chunk in chunks {
        out.extend_from_slice(chunk);
    }
    out.extend_from_slice(&png_chunk(b"IDAT", &[1, 2, 3]));
    out.extend_from_slice(&png_chunk(b"IEND", &[]));
    out
}

/// 最小 EXIF TIFF：IFD0 只有一条 0x0112（Orientation）SHORT 记录。
fn exif_tiff(little: bool, orientation: u16) -> Vec<u8> {
    let mut out = Vec::new();
    if little {
        out.extend_from_slice(b"II");
        out.extend_from_slice(&42u16.to_le_bytes());
        out.extend_from_slice(&8u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&0x0112u16.to_le_bytes());
        out.extend_from_slice(&3u16.to_le_bytes()); // SHORT
        out.extend_from_slice(&1u32.to_le_bytes());
        let mut value = [0u8; 4];
        value[..2].copy_from_slice(&orientation.to_le_bytes());
        out.extend_from_slice(&value);
    } else {
        out.extend_from_slice(b"MM");
        out.extend_from_slice(&42u16.to_be_bytes());
        out.extend_from_slice(&8u32.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&0x0112u16.to_be_bytes());
        out.extend_from_slice(&3u16.to_be_bytes());
        out.extend_from_slice(&1u32.to_be_bytes());
        let mut value = [0u8; 4];
        value[..2].copy_from_slice(&orientation.to_be_bytes());
        out.extend_from_slice(&value);
    }
    out.extend_from_slice(&0u32.to_le_bytes()); // next IFD offset
    out
}

#[test]
fn jpeg_segments_collect_only_metadata_appn() {
    let source = jpeg_with(&[
        jpeg_segment(0xE0, b"JFIF\0"),
        jpeg_segment(0xE1, b"Exif\0\0payload"),
        jpeg_segment(0xE2, b"ICC_PROFILE\0chunk"),
        jpeg_segment(0xE3, b"ignored"),
        jpeg_segment(0xED, b"Photoshop 3.0\0"),
    ]);
    let segments = jpeg_metadata_segments(&source);
    assert_eq!(segments.len(), 3);
    assert_eq!(segments[0].marker, 0xE1);
    assert_eq!(segments[0].payload, b"Exif\0\0payload");
    assert_eq!(segments[1].marker, 0xE2);
    assert_eq!(segments[2].marker, 0xED);
    assert_eq!(segments[2].payload, b"Photoshop 3.0\0");
}

#[test]
fn jpeg_segments_stop_at_sos() {
    let mut data = vec![0xFF, 0xD8];
    data.extend_from_slice(&jpeg_segment(0xE1, b"before"));
    data.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02]);
    data.extend_from_slice(&jpeg_segment(0xE1, b"after")); // 其实是熵编码字节
    let segments = jpeg_metadata_segments(&data);
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].payload, b"before");
}

#[test]
fn jpeg_segments_tolerates_malformed_input() {
    assert!(jpeg_metadata_segments(&[0x00, 0x01, 0x02]).is_empty());
    // 声明的段长度越界：不 panic，且不会把越界数据当成元数据。
    let truncated = [0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x7F, 0x41];
    assert!(jpeg_metadata_segments(&truncated).is_empty());
    // 段间填充 0xFF 是合法的，应仍能读出 APP1。
    let padded = [
        0xFF, 0xD8, 0xFF, 0xFF, 0xE1, 0x00, 0x05, b'a', b'b', b'c',
    ];
    let segments = jpeg_metadata_segments(&padded);
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].payload, b"abc");
}

#[test]
fn inject_jpeg_segments_after_app0() {
    let app0 = jpeg_segment(0xE0, b"JFIF\0");
    let base = jpeg_with(std::slice::from_ref(&app0));
    let payload = b"Exif\0\0orientation";
    let segments = vec![JpegMetadataSegment { marker: 0xE1, payload: payload.to_vec() }];
    let out = inject_jpeg_segments(&base, &segments).unwrap();

    assert!(out.starts_with(&[0xFF, 0xD8, 0xFF, 0xE0]));
    let insert_at = 2 + app0.len();
    assert_eq!(&out[insert_at..insert_at + 2], &[0xFF, 0xE1]);
    let after_segment = insert_at + 4 + payload.len();
    assert_eq!(&out[after_segment..after_segment + 2], &[0xFF, 0xDB]);
    // 注入结果本身仍可被同一解析器完整读回。
    assert_eq!(jpeg_metadata_segments(&out), segments);
}

#[test]
fn inject_jpeg_segments_without_app0() {
    let base = jpeg_with(&[]);
    let segments = vec![JpegMetadataSegment { marker: 0xE2, payload: b"ICC".to_vec() }];
    let out = inject_jpeg_segments(&base, &segments).unwrap();
    assert_eq!(&out[2..4], &[0xFF, 0xE2]);
    assert_eq!(jpeg_metadata_segments(&out), segments);
}

#[test]
fn inject_jpeg_segments_rejects_non_jpeg() {
    let segments = vec![JpegMetadataSegment { marker: 0xE1, payload: vec![1] }];
    assert!(inject_jpeg_segments(&[0x00, 0x01], &segments).is_none());
    // 空段列表时不做任何改写。
    let base = jpeg_with(&[]);
    assert_eq!(inject_jpeg_segments(&base, &[]).unwrap(), base);
}

#[test]
fn exif_orientation_little_endian() {
    assert_eq!(exif_orientation(&exif_tiff(true, 6)), Some(6));
    assert_eq!(exif_orientation(&exif_tiff(true, 1)), Some(1));
}

#[test]
fn exif_orientation_big_endian() {
    assert_eq!(exif_orientation(&exif_tiff(false, 8)), Some(8));
    assert_eq!(exif_orientation(&exif_tiff(false, 3)), Some(3));
}

#[test]
fn exif_orientation_ignores_missing_or_invalid_tag() {
    assert_eq!(exif_orientation(&[0u8; 4]), None);
    assert_eq!(exif_orientation(b"NOPE0000"), None);
    // 未带 Exif 头、且 IFD 中没有 0x0112 的合法 TIFF。
    let mut tiff = exif_tiff(true, 6).to_vec();
    tiff[10] = 0x00;
    tiff[11] = 0x01; // 改成不存在的 tag 0x0100
    assert_eq!(exif_orientation(&tiff), None);
}

#[test]
fn png_chunks_keep_text_color_time_and_phys() {
    let kept: [(&[u8; 4], &[u8]); 10] = [
        (b"tEXt", b"Author\0Long"),
        (b"zTXt", b"Comment\0\0compressed"),
        (b"iTXt", b"XML\0\0\0\0\0payload"),
        (b"iCCP", b"profile\0\0compressed-icc"),
        (b"sRGB", &[0]),
        (b"gAMA", &0x0000_B18Fu32.to_be_bytes()),
        (b"cHRM", &[0u8; 32]),
        (b"tIME", &[0u8; 7]),
        (b"eXIf", b"II*\0"),
        (b"pHYs", &[0u8; 9]),
    ];
    let mut raw = Vec::new();
    for (kind, data) in kept {
        raw.push(png_chunk(kind, data));
    }
    let chunks = png_metadata_chunks(&png_with(&raw), true);
    assert_eq!(chunks.len(), 10);
    for (i, (kind, data)) in kept.iter().enumerate() {
        assert_eq!(&chunks[i].kind, *kind);
        assert_eq!(chunks[i].data.as_slice(), *data);
    }
}

#[test]
fn png_chunks_exclude_pixel_bound_chunks() {
    let excluded: [(&[u8; 4], &[u8]); 6] = [
        (b"tRNS", &[0u8; 6]),
        (b"PLTE", &[0u8; 9]),
        (b"bKGD", &[0u8; 2]),
        (b"hIST", &[0u8; 6]),
        (b"sBIT", &[0u8; 3]),
        (b"acTL", &[0u8; 8]),
    ];
    let raw: Vec<Vec<u8>> = excluded.iter().map(|(k, d)| png_chunk(k, d)).collect();
    assert!(png_metadata_chunks(&png_with(&raw), true).is_empty());
}

#[test]
fn png_chunks_drop_phys_when_resized() {
    let raw = vec![
        png_chunk(b"tEXt", b"Author\0Long"),
        png_chunk(b"pHYs", &[0u8; 9]),
    ];
    let source = png_with(&raw);
    let chunks = png_metadata_chunks(&source, false);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].kind, *b"tEXt");
    assert_eq!(png_metadata_chunks(&source, true).len(), 2);
}

#[test]
fn png_chunks_tolerates_truncated_input() {
    assert!(png_metadata_chunks(b"not a png", true).is_empty());
    let mut truncated = png_with(&[png_chunk(b"tEXt", b"Author\0Long")]);
    truncated.truncate(20); // 切在 IHDR 中间
    assert!(png_metadata_chunks(&truncated, true).is_empty());
}

#[test]
fn inject_png_chunks_after_ihdr() {
    let base = png_with(&[]);
    let chunks = vec![
        PngMetadataChunk { kind: *b"tEXt", data: b"Author\0Long".to_vec() },
        PngMetadataChunk { kind: *b"sRGB", data: vec![0] },
    ];
    let out = inject_png_chunks(&base, &chunks).unwrap();

    // IHDR chunk 占 4 + 4 + 13 + 4 = 25 字节，注入点必须紧跟其后。
    let insert_at = 8 + 25;
    assert_eq!(&out[insert_at + 4..insert_at + 8], b"tEXt");
    assert_eq!(&out[insert_at..insert_at + 4], &(11u32).to_be_bytes());
    // IDAT 仍在注入块之后，像素数据未被破坏。
    let idat = out.windows(4).position(|w| w == b"IDAT").unwrap();
    assert!(idat > insert_at);
    // 注入结果可被同一解析器读回，且 CRC 重算后数据完整。
    assert_eq!(png_metadata_chunks(&out, true), chunks);
    assert_eq!(inject_png_chunks(&base, &[]).unwrap(), base);
}

#[test]
fn injected_png_chunk_crc_matches_spec_value() {
    // PNG 规范里空 IEND 块的 CRC 固定为 AE 42 60 82，用它校验自实现的 CRC-32。
    let base = png_with(&[]);
    let chunk = PngMetadataChunk { kind: *b"IEND", data: Vec::new() };
    let out = inject_png_chunks(&base, &[chunk]).unwrap();
    let insert_at = 8 + 25;
    assert_eq!(
        &out[insert_at..insert_at + 12],
        &[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82]
    );
}

#[test]
fn inject_png_chunks_rejects_non_png() {
    let chunks = vec![PngMetadataChunk { kind: *b"tEXt", data: vec![1] }];
    assert!(inject_png_chunks(b"not a png", &chunks).is_none());
    let mut bad = png_with(&[]);
    bad[12] = b'X'; // 把 IHDR 类型改坏
    assert!(inject_png_chunks(&bad, &chunks).is_none());
}
