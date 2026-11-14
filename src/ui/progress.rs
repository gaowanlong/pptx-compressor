//! 右侧从属区下半部：进度、结果、活动日志与环境提示。

use crate::app::{
    AppState, PptxCompressorApp, AMBER, AMBER_SOFT, GREEN, GREEN_SOFT, LINE,
    MUTED, RED, RED_SOFT, TEXT, TEXT_2, WHITE,
};
use crate::ui::components;

pub fn show(ui: &mut egui::Ui, app: &mut PptxCompressorApp) {
    let meta = match app.state {
        AppState::Done => "最终结果",
        AppState::Error => "含错误",
        _ => "实时更新",
    };
    components::section_head(ui, "进度与结果", meta, |_| {});

    match app.state {
        AppState::Idle | AppState::Loading => {
            let hint = if app.state == AppState::Loading {
                "正在后台解析文稿，完成后这里会显示媒体清单与预估。"
            } else {
                "选择一个 PPTX 后，这里会显示处理进度与体积对比。"
            };
            components::card(ui, WHITE, LINE, |ui| {
                ui.label(egui::RichText::new(hint).color(MUTED).size(11.5));
            });
        }
        AppState::Ready => {
            let total = app
                .media_files
                .iter()
                .filter(|m| m.enabled && m.media_type.is_compressible())
                .count();
            let skipped = app.media_files.len().saturating_sub(total);
            components::card(ui, WHITE, LINE, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("准备就绪").color(TEXT).size(11.5).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("0 / {}", total))
                                .color(MUTED)
                                .size(11.0),
                        );
                    });
                });
                ui.add_space(8.0);
                components::progress_bar(ui, 0.0);
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(format!(
                        "共 {} 个媒体，其中 {} 个可压缩、{} 个将原样保留。点击右下角开始。",
                        app.media_files.len(),
                        total,
                        skipped
                    ))
                    .color(TEXT_2)
                    .size(11.0),
                );
            });
        }
        AppState::Compressing => {
            let pct = app.progress_pct();
            let total = app.progress_total;
            let current = app.progress_current.min(total);
            let file = app.progress_file.clone();
            components::card(ui, WHITE, LINE, |ui| {
                ui.horizontal(|ui| {
                    let text = if file.is_empty() {
                        "准备处理媒体…".to_owned()
                    } else {
                        format!("正在处理 {}", file)
                    };
                    ui.label(egui::RichText::new(text).color(TEXT).size(11.5).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("{} / {} · {}%", current, total, pct))
                                .color(TEXT_2)
                                .size(11.0),
                        );
                    });
                });
                ui.add_space(8.0);
                components::progress_bar(ui, pct as f32 / 100.0);
            });
        }
        AppState::Done => {
            let from = app.result_original;
            let to = app.result_compressed;
            let saved = from.saturating_sub(to);
            let pct = if from > 0 {
                (saved as f64 / from as f64 * 100.0) as u8
            } else {
                0
            };
            components::card(ui, WHITE, LINE, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(components::fmt_size(from))
                            .color(MUTED)
                            .size(15.0)
                            .strikethrough(),
                    );
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("→").color(MUTED).size(14.0));
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(components::fmt_size(to))
                            .color(GREEN)
                            .size(26.0)
                            .strong(),
                    );
                });
                ui.add_space(8.0);
                components::badge(
                    ui,
                    &format!("节省 {}% · {}", pct, components::fmt_size(saved)),
                    GREEN,
                    GREEN_SOFT,
                );
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(
                        "输出包已回读校验通过，原文件未改动；变大的媒体自动保留原件。",
                    )
                    .color(TEXT_2)
                    .size(10.5),
                );
                if let Some(path) = app.output_path.clone() {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(path.to_string_lossy().into_owned())
                            .color(MUTED)
                            .size(10.0),
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if components::small_button(ui, "打开输出文件").clicked() {
                            let _ = components::open_folder(&path);
                        }
                        if components::small_button(ui, "复制路径").clicked() {
                            ui.output_mut(|o| {
                                o.copied_text = path.to_string_lossy().into_owned();
                            });
                            app.log("输出路径已复制到剪贴板");
                        }
                    });
                }
            });
        }
        AppState::Error => {
            components::card(ui, RED_SOFT, RED.linear_multiply(0.3), |ui| {
                ui.label(
                    egui::RichText::new("处理未完成")
                        .color(RED)
                        .size(12.0)
                        .strong(),
                );
                ui.add_space(8.0);
                if app.activity_log.is_empty() {
                    ui.label(
                        egui::RichText::new("未提供具体原因，请更换文件后重试。")
                            .color(RED)
                            .size(11.0),
                    );
                } else {
                    for line in app.activity_log.iter().rev().take(6) {
                        ui.label(
                            egui::RichText::new(format!("• {}", line))
                                .color(RED)
                                .size(10.5),
                        );
                    }
                }
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("源文件未被修改，可查看日志后重试，或关闭兼容模式再试一次。")
                        .color(TEXT_2)
                        .size(10.5),
                );
            });
        }
    }

    if app.state != AppState::Error && !app.activity_log.is_empty() {
        ui.add_space(12.0);
        show_log(ui, app, 4);
    }
}

fn show_log(ui: &mut egui::Ui, app: &PptxCompressorApp, limit: usize) {
    if app.activity_log.is_empty() {
        return;
    }
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new("活动日志")
            .color(MUTED)
            .size(10.5)
            .strong(),
    );
    for line in app.activity_log.iter().rev().take(limit) {
        ui.label(
            egui::RichText::new(format!("• {}", line))
                .color(TEXT_2)
                .size(10.0),
        );
    }
}

/// 风险告知与环境状态，位于右栏底部。
pub fn show_notes(ui: &mut egui::Ui, app: &mut PptxCompressorApp) {
    components::notice(
        ui,
        "含宏（.pptm）或数字签名的演示文稿，压缩后签名会失效；建议先另存副本。",
        AMBER,
        AMBER_SOFT,
    );
    ui.add_space(10.0);

    if app.ffmpeg_available {
        let source = app
            .ffmpeg_source
            .map(|s| s.label())
            .unwrap_or("未知来源");
        let license = app
            .ffmpeg_license
            .map(|l| l.label())
            .unwrap_or("许可未识别");
        let path = app.ffmpeg_path.clone().unwrap_or_default();
        let has_override = app.ffmpeg_override.is_some();

        ui.horizontal(|ui| {
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 3.5, GREEN);
            ui.add_space(5.0);
            ui.label(
                egui::RichText::new(format!("FFmpeg 已就绪（{source} · {license}）"))
                    .color(MUTED)
                    .size(10.0),
            );
        });
        ui.add_space(3.0);
        ui.label(
            egui::RichText::new(shorten_path(&path))
                .color(MUTED)
                .size(9.5),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if components::text_link(ui, "重新检测").clicked() {
                app.refresh_ffmpeg();
            }
            ui.add_space(12.0);
            if components::text_link(ui, "更换 FFmpeg…").clicked() {
                app.pick_ffmpeg();
            }
            if has_override {
                ui.add_space(12.0);
                if components::text_link(ui, "恢复自动查找").clicked() {
                    app.clear_ffmpeg_override();
                }
            }
        });
    } else {
        components::notice(
            ui,
            "未找到 FFmpeg：图片、GIF 与 XML 清理照常工作，视频项会被明确跳过。",
            RED,
            RED_SOFT,
        );
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(
                "请安装系统 FFmpeg（macOS: brew install ffmpeg；Windows: 官方或 LGPL 构建），\
                 或点击下方按钮直接选择本机可执行文件。",
            )
            .color(TEXT_2)
            .size(10.0),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if components::small_button(ui, "选择本机 FFmpeg…").clicked() {
                app.pick_ffmpeg();
            }
            ui.add_space(10.0);
            if components::text_link(ui, "重新检测").clicked() {
                app.refresh_ffmpeg();
            }
        });
    }

    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(
            "v1.2.0 起不再随包分发 FFmpeg：用户自备二进制的许可证由使用者自行负责，\
             项目源码仍为 MIT（详见 THIRD_PARTY_NOTICES.md）。",
        )
        .color(MUTED)
        .size(9.5),
    );
}

/// 长路径在窄栏里截断中间，避免把布局撑破。
fn shorten_path(path: &str) -> String {
    let chars: Vec<char> = path.chars().collect();
    if chars.len() <= 64 {
        return path.to_owned();
    }
    let head: String = chars[..24].iter().collect();
    let tail: String = chars[chars.len() - 34..].iter().collect();
    format!("{head}…{tail}")
}
