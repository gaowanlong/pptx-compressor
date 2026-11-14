//! 体积构成与「可优化空间」计算。
//!
//! v1.2.5 起彻底移除按媒体类型套固定压缩比的启发式预估。真实压缩结果取决于
//! 素材内容（熵、分辨率、已有压缩程度），任何固定系数都会系统性误导用户：
//! 它既与源文件无关，也会随质量滑杆给出一个「看起来精确」却无法兑现的百分比。
//!
//! 本模块只报告从真实字节数推得、可验证的量：
//! - 文稿实际大小、媒体总体积、按类型的媒体构成；
//! - **可优化空间上限** = 被启用且可压缩的媒体原始体积之和。
//!
//! 上界依据：流水线遵循「只有更小才替换」（`pipeline::should_replace`），且只有
//! 启用且可压缩的媒体会进入流水线，因此实际节省恒在 `[0, 可优化空间上限]`
//! 区间内。这是数学上的真实上界，不是对压缩结果的预测；下界为 0，因为所有
//! 媒体都可能被保留原件。对外只陈述构成与区间，不声称能预测结果。

use crate::core::{MediaInfo, MediaType};
use serde::{Deserialize, Serialize};

/// 某类媒体的真实数量与体积（无任何估算）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeBytes {
    pub count: usize,
    pub bytes: u64,
}

impl TypeBytes {
    fn add(&mut self, bytes: u64) {
        self.count = self.count.saturating_add(1);
        self.bytes = self.bytes.saturating_add(bytes);
    }
}

/// 整个文稿的媒体构成。所有字节数都来自实际文件，不含任何估算系数。
///
/// `package_size` 是源 PPTX（ZIP 包）的字节数，`media_bytes` 是解压后媒体
/// 原始体积之和，两者口径不同，因此本模块**不计算**「可优化体积占文稿的百分比」，
/// 只提供同为解压后口径的 `media_share_pct()`，避免又一个误导性数字。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaComposition {
    /// 源 PPTX 的实际字节数；0 表示未知（例如无窗口渲染冒烟测试）。
    pub package_size: u64,
    /// 清单内所有媒体的原始体积之和。
    pub media_bytes: u64,
    /// 可优化空间上限：启用且可压缩的媒体原始体积之和。
    pub optimizable_bytes: u64,
    pub optimizable_count: usize,
    pub total_count: usize,
    pub jpeg: TypeBytes,
    pub png: TypeBytes,
    pub gif: TypeBytes,
    pub video: TypeBytes,
    pub other: TypeBytes,
}

impl MediaComposition {
    /// 从真实媒体清单与源文稿大小统计构成。纯函数，不读磁盘、不依赖压缩参数。
    pub fn from_media(media: &[MediaInfo], package_size: u64) -> Self {
        let mut c = Self {
            package_size,
            total_count: media.len(),
            ..Self::default()
        };
        for item in media {
            c.media_bytes = c.media_bytes.saturating_add(item.original_size);
            match item.media_type {
                MediaType::Jpeg => c.jpeg.add(item.original_size),
                MediaType::Png => c.png.add(item.original_size),
                MediaType::Gif => c.gif.add(item.original_size),
                MediaType::Video => c.video.add(item.original_size),
                MediaType::Other => c.other.add(item.original_size),
            }
            if item.enabled && item.media_type.is_compressible() {
                c.optimizable_bytes = c.optimizable_bytes.saturating_add(item.original_size);
                c.optimizable_count = c.optimizable_count.saturating_add(1);
            }
        }
        c
    }

    pub fn by_type(&self, kind: MediaType) -> TypeBytes {
        match kind {
            MediaType::Jpeg => self.jpeg,
            MediaType::Png => self.png,
            MediaType::Gif => self.gif,
            MediaType::Video => self.video,
            MediaType::Other => self.other,
        }
    }

    /// 文稿中非媒体部分的体积。ZIP 打包后 XML 会被 deflate，媒体解压后体积之和
    /// 有可能大于包体积，此时保守返回 0 而不是一个负数或虚假的精确值。
    pub fn non_media_bytes(&self) -> u64 {
        self.package_size.saturating_sub(self.media_bytes)
    }

    /// 数学上真实的节省上界，不是预测值。
    pub fn saving_upper_bound(&self) -> u64 {
        self.optimizable_bytes
    }

    /// 数学上真实的节省下界：所有媒体都走「变小才替换」而保留原件。
    pub fn saving_lower_bound(&self) -> u64 {
        0
    }

    /// 可优化媒体占媒体总体积的比例（同为解压后口径，严格不超过 100）。
    pub fn media_share_pct(&self) -> u8 {
        pct(self.optimizable_bytes, self.media_bytes)
    }

    /// 主文案：只陈述真实上界、同口径占比与文件数，不给出任何预测百分比。
    pub fn headline(&self) -> String {
        if self.optimizable_count == 0 {
            return "当前没有可优化的媒体（其余内容将原样保留）".to_owned();
        }
        format!(
            "可优化空间上限 {}（占媒体总量 {}% · {} 个媒体）",
            fmt_size(self.optimizable_bytes),
            self.media_share_pct(),
            self.optimizable_count
        )
    }

    /// 文稿与媒体的实际体积明细。
    pub fn detail(&self) -> String {
        let mut parts = Vec::new();
        if self.package_size > 0 {
            parts.push(format!("文稿 {}", fmt_size(self.package_size)));
        }
        parts.push(format!("媒体合计 {}", fmt_size(self.media_bytes)));
        parts.push(format!("{} 个文件", self.total_count));
        parts.join(" · ")
    }

    /// 按类型的真实媒体构成，只列出实际存在的类别，顺序固定。
    pub fn breakdown(&self) -> String {
        let parts: Vec<String> = [
            ("JPEG", self.jpeg),
            ("PNG", self.png),
            ("动图", self.gif),
            ("视频", self.video),
            ("其他", self.other),
        ]
        .into_iter()
        .filter(|(_, stat)| stat.count > 0)
        .map(|(label, stat)| format!("{} {}", label, fmt_size(stat.bytes)))
        .collect();
        if parts.is_empty() {
            "包内未发现媒体".to_owned()
        } else {
            parts.join(" · ")
        }
    }

    /// 口径说明，必须随数字一起展示，避免「上限」被误读成「预计节省」。
    pub fn caveat(&self) -> &'static str {
        "这是真实上界（变小才替换），不是预测；实际节省取决于素材内容，完成后显示真实数字"
    }
}

/// 与界面统一的体积格式化口径，下沉到 core 供 UI、测试与后续 CLI 共用。
pub fn fmt_size(bytes: u64) -> String {
    bytesize::ByteSize(bytes).to_string()
}

/// 待处理媒体在「节省」列的文案：只说明是否参与优化，不再显示按系数推算的假百分比。
pub fn pending_label(media: &MediaInfo) -> &'static str {
    if media.enabled && media.media_type.is_compressible() {
        "可优化"
    } else {
        "不处理"
    }
}

/// 已完成媒体的**真实**节省百分比（原始体积 vs 压缩后体积）。
pub fn real_saving_pct(original: u64, compressed: u64) -> u8 {
    pct(original.saturating_sub(compressed), original)
}

/// 压缩完成后，媒体层面实际省下的字节总和。用于回归断言「真实节省落在上界内」，
/// 也便于后续无头 CLI 输出结构化结果。
///
/// 按**体积差**判定而非 `MediaInfo::status`：流水线只在「变小才替换」成功时写入
/// 更小的 `compressed_size`，其余路径（跳过 / 失败 / 变大）都把 `compressed_size`
/// 回写成 `original_size`；而 `status` 是由 GUI 依据通道消息维护的展示态字段，
/// core 与未来的无头 CLI 不应依赖它。
pub fn actual_media_saving(media: &[MediaInfo]) -> u64 {
    media
        .iter()
        .filter(|m| m.compressed_size > 0 && m.compressed_size < m.original_size)
        .fold(0u64, |sum, m| {
            sum.saturating_add(m.original_size.saturating_sub(m.compressed_size))
        })
}

fn pct(part: u64, whole: u64) -> u8 {
    if whole == 0 {
        return 0;
    }
    ((part as f64 / whole as f64 * 100.0).round() as u64).min(100) as u8
}
