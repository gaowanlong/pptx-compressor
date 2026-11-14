//! v1.2.5 体积构成与可优化空间：纯函数回归测试。
//!
//! 重点验证两件事：
//! 1. 所有数字都来自真实媒体构成，不含任何随压缩参数变化的启发式系数；
//! 2. 「可优化空间上限」是数学上真实的节省上界，真实节省恒落在区间内。

use pptx_compressor::core::estimate::{
    self, MediaComposition, TypeBytes,
};
use pptx_compressor::core::pipeline::should_replace;
use pptx_compressor::core::{CompressionSettings, MediaInfo, MediaStatus, MediaType, Preset};
use pptx_compressor::ui::components;
use std::path::PathBuf;

fn item(kind: MediaType, size: u64, enabled: bool) -> MediaInfo {
    let mut m = MediaInfo::new(
        format!("ppt/media/{}.bin", kind.label()),
        PathBuf::from("/tmp/estimate-test-not-on-disk"),
        kind,
    );
    m.original_size = size;
    m.enabled = enabled;
    m
}

fn mixed_media() -> Vec<MediaInfo> {
    vec![
        item(MediaType::Jpeg, 8_000, true),
        item(MediaType::Png, 5_000, true),
        item(MediaType::Video, 20_000, true),
        item(MediaType::Gif, 3_000, false), // 用户手动关闭
        item(MediaType::Other, 1_000, true), // 即使 enabled 也不该算可压缩
    ]
}

#[test]
fn empty_package_has_zero_composition() {
    let c = MediaComposition::from_media(&[], 0);
    assert_eq!(c.total_count, 0);
    assert_eq!(c.optimizable_count, 0);
    assert_eq!(c.media_bytes, 0);
    assert_eq!(c.optimizable_bytes, 0);
    assert_eq!(c.saving_upper_bound(), 0);
    assert_eq!(c.saving_lower_bound(), 0);
    assert_eq!(c.media_share_pct(), 0);
    assert_eq!(c.non_media_bytes(), 0);
    assert_eq!(c.breakdown(), "包内未发现媒体");
}

#[test]
fn upper_bound_equals_real_size_of_enabled_compressible_media() {
    let c = MediaComposition::from_media(&mixed_media(), 60_000);
    assert_eq!(c.total_count, 5);
    assert_eq!(c.media_bytes, 37_000);
    // JPEG + PNG + Video；GIF 被关闭、Other 不可压缩。
    assert_eq!(c.optimizable_count, 3);
    assert_eq!(c.optimizable_bytes, 33_000);
    assert_eq!(c.saving_upper_bound(), 33_000);
    assert!(c.saving_upper_bound() <= c.media_bytes);
    assert_eq!(c.non_media_bytes(), 23_000);
}

#[test]
fn every_type_is_counted_by_its_real_bytes() {
    let c = MediaComposition::from_media(&mixed_media(), 60_000);
    assert_eq!(c.by_type(MediaType::Jpeg), TypeBytes { count: 1, bytes: 8_000 });
    assert_eq!(c.by_type(MediaType::Png), TypeBytes { count: 1, bytes: 5_000 });
    assert_eq!(c.by_type(MediaType::Video), TypeBytes { count: 1, bytes: 20_000 });
    assert_eq!(c.by_type(MediaType::Gif), TypeBytes { count: 1, bytes: 3_000 });
    assert_eq!(c.by_type(MediaType::Other), TypeBytes { count: 1, bytes: 1_000 });
}

#[test]
fn disabled_and_other_items_never_become_optimizable() {
    let media = vec![
        item(MediaType::Jpeg, 100, false),
        item(MediaType::Other, 9_999, true),
    ];
    let c = MediaComposition::from_media(&media, 20_000);
    assert_eq!(c.optimizable_count, 0);
    assert_eq!(c.saving_upper_bound(), 0);
    assert_eq!(c.media_bytes, 10_099);
    assert!(c.headline().contains("没有可优化"));
}

#[test]
fn byte_accumulation_saturates_instead_of_overflowing() {
    let media = vec![
        item(MediaType::Jpeg, u64::MAX, true),
        item(MediaType::Jpeg, u64::MAX, true),
    ];
    let c = MediaComposition::from_media(&media, u64::MAX);
    assert_eq!(c.media_bytes, u64::MAX);
    assert_eq!(c.optimizable_bytes, u64::MAX);
    assert_eq!(c.jpeg.bytes, u64::MAX);
    assert_eq!(c.jpeg.count, 2);
    assert_eq!(c.saving_upper_bound(), u64::MAX);
}

#[test]
fn non_media_bytes_is_conservative_when_media_exceeds_package() {
    // PPTX 是 ZIP：XML 被 deflate 压缩，媒体解压后体积之和完全可能大于包体积。
    let media = vec![item(MediaType::Jpeg, 10_000, true)];
    let c = MediaComposition::from_media(&media, 4_000);
    assert_eq!(c.non_media_bytes(), 0);
    assert_eq!(c.package_size, 4_000);
}

#[test]
fn media_share_pct_is_same_unit_zero_safe_and_clamped() {
    let media = vec![
        item(MediaType::Jpeg, 250, true),
        item(MediaType::Png, 250, false),
    ];
    let c = MediaComposition::from_media(&media, 1_000);
    assert_eq!(c.media_share_pct(), 50);
    assert_eq!(MediaComposition::from_media(&[], 0).media_share_pct(), 0);

    let bigger = vec![item(MediaType::Jpeg, 100, true)];
    assert_eq!(MediaComposition::from_media(&bigger, 0).media_share_pct(), 100);
    assert_eq!(estimate::real_saving_pct(0, 0), 0);
    assert_eq!(estimate::real_saving_pct(100, 200), 0); // 变大 -> 0%
    assert_eq!(estimate::real_saving_pct(200, 50), 75);
}

#[test]
fn headline_states_an_upper_bound_and_never_predicts_saving() {
    let c = MediaComposition::from_media(&mixed_media(), 60_000);
    let text = c.headline();
    assert!(text.contains("可优化空间上限"), "实际文案：{text}");
    assert!(text.contains("33"));
    assert!(text.contains("占媒体总量"));
    for misleading in ["预计", "预计可节省", "估算", "可节省约", "≈"] {
        assert!(!text.contains(misleading), "不应再出现「{misleading}」：{text}");
    }
    assert!(c.caveat().contains("不是预测"));
    assert!(c.caveat().contains("变小才替换"));
}

#[test]
fn detail_reports_real_package_media_and_file_count() {
    let c = MediaComposition::from_media(&mixed_media(), 60_000);
    let detail = c.detail();
    assert!(detail.contains("文稿"));
    assert!(detail.contains("媒体合计"));
    assert!(detail.contains("5 个文件"));

    // 源文件大小未知时（无窗口冒烟 / 后续 CLI）不编造一个数字。
    let unknown = MediaComposition::from_media(&mixed_media(), 0);
    assert!(!unknown.detail().contains("文稿"));
}

#[test]
fn breakdown_lists_only_present_types_in_a_fixed_order() {
    let c = MediaComposition::from_media(&mixed_media(), 60_000);
    let text = c.breakdown();
    let jpeg = text.find("JPEG").unwrap();
    let png = text.find("PNG").unwrap();
    let gif = text.find("动图").unwrap();
    let video = text.find("视频").unwrap();
    let other = text.find("其他").unwrap();
    assert!(jpeg < png && png < gif && gif < video && video < other, "{text}");

    let only_jpeg = MediaComposition::from_media(&[item(MediaType::Jpeg, 10, true)], 100);
    assert!(only_jpeg.breakdown().starts_with("JPEG"));
    assert!(!only_jpeg.breakdown().contains("视频"));
}

#[test]
fn pending_labels_contain_no_fake_percentages() {
    let on = item(MediaType::Jpeg, 1_000, true);
    let off = item(MediaType::Png, 1_000, false);
    let other = item(MediaType::Other, 1_000, true);
    assert_eq!(estimate::pending_label(&on), "可优化");
    assert_eq!(estimate::pending_label(&off), "不处理");
    assert_eq!(estimate::pending_label(&other), "不处理");
    for label in [
        estimate::pending_label(&on),
        estimate::pending_label(&off),
        estimate::pending_label(&other),
    ] {
        assert!(!label.contains('%'));
        assert!(!label.contains('≈'));
    }
}

#[test]
fn composition_is_independent_of_compression_settings() {
    // 旧实现把 image_quality / video_quality / 缩放开关直接乘进体积系数，
    // 滑杆一动「预计节省」就变。新模型只读真实字节数与启用状态。
    let mut high = CompressionSettings::default();
    high.apply_preset(Preset::High);
    let mut small = CompressionSettings::default();
    small.apply_preset(Preset::Small);
    assert_ne!(high.image_quality, small.image_quality);
    assert_ne!(high.video_quality, small.video_quality);

    let media = mixed_media();
    let a = MediaComposition::from_media(&media, 60_000);
    let b = MediaComposition::from_media(&media, 60_000);
    assert_eq!(a, b);
    assert_eq!(a.headline(), b.headline());
    assert_eq!(a.saving_upper_bound(), b.saving_upper_bound());
}

#[test]
fn real_saving_always_falls_inside_the_reported_bounds() {
    let media = vec![
        item(MediaType::Jpeg, 1_000, true),
        item(MediaType::Png, 500, true),
        item(MediaType::Other, 300, false),
        item(MediaType::Video, 2_000, false),
    ];
    let c = MediaComposition::from_media(&media, 10_000);
    assert_eq!(c.saving_upper_bound(), 1_500);
    assert_eq!(c.saving_lower_bound(), 0);

    for outcomes in [
        [0u64, 0u64],
        [999, 499],
        [1_000, 500],
        [1_200, 600],
        [1, 1],
    ] {
        let mut simulated = media.clone();
        for (idx, size) in outcomes.iter().enumerate() {
            // 与流水线裁决完全一致的「变小才替换」。
            if should_replace(simulated[idx].original_size, *size) {
                simulated[idx].compressed_size = *size;
                simulated[idx].status = MediaStatus::Done;
            } else {
                simulated[idx].status = MediaStatus::Skipped;
            }
        }
        let saved = estimate::actual_media_saving(&simulated);
        assert!(saved >= c.saving_lower_bound(), "saved={saved}");
        assert!(saved <= c.saving_upper_bound(), "saved={saved}");
    }
}

#[test]
fn actual_media_saving_only_counts_real_replacements() {
    let mut media = vec![item(MediaType::Jpeg, 1_000, true)];
    assert_eq!(estimate::actual_media_saving(&media), 0);

    // 变大 / 跳过 / 失败时，流水线统一把 compressed_size 回写成 original_size。
    for status in [
        MediaStatus::Pending,
        MediaStatus::Skipped,
        MediaStatus::Failed,
    ] {
        media[0].compressed_size = 1_000;
        media[0].status = status;
        assert_eq!(estimate::actual_media_saving(&media), 0);
    }

    media[0].compressed_size = 400;
    media[0].status = MediaStatus::Done;
    assert_eq!(estimate::actual_media_saving(&media), 600);

    // 判定不依赖只由 GUI 维护的 status，core / 未来 CLI 读体积差结果一致。
    media[0].status = MediaStatus::Pending;
    assert_eq!(estimate::actual_media_saving(&media), 600);
}

#[test]
fn composition_serializes_for_future_headless_cli() {
    let c = MediaComposition::from_media(&mixed_media(), 60_000);
    let json = serde_json::to_string(&c).expect("serialize");
    let back: MediaComposition = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, c);
    assert!(json.contains("optimizable_bytes"));
    assert!(json.contains("package_size"));
}

#[test]
fn core_and_ui_share_one_size_format() {
    for bytes in [0u64, 999, 1_000, 1_048_576, 48_600_000] {
        assert_eq!(estimate::fmt_size(bytes), components::fmt_size(bytes));
    }
}
