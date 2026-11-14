//! PPTX 输入安全检测。
//!
//! 在解压之前只读取 ZIP 中央目录和少量 XML，判断文稿是否适合压缩。
//! 加密、受信息权限保护（IRM）的文档重打包后必然损坏，必须阻断；
//! 带数字签名、VBA 宏、OLE 嵌入对象或修改保护的文档可以继续处理，
//! 但需要把后果明确告知用户。这样避免生成看似成功、实则打不开的文件。

use std::fs;
use std::io::{self, Read};
use std::path::Path;
use zip::ZipArchive;

/// OLE 复合文档魔数：旧版 .ppt 与加密的 OOXML 文档都使用这种容器。
const OLE_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
/// 只读检查主部件，避免对异常巨大的 XML 做全量解码。
const MAX_XML_INSPECT: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// 阻止解压，原文件不会被改动。
    Block,
    /// 允许继续，但必须在界面与活动日志中提示。
    Warn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
}

impl Finding {
    fn block(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Block,
            code,
            message: message.into(),
        }
    }

    fn warn(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warn,
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Inspection {
    pub findings: Vec<Finding>,
    pub entries: usize,
}

impl Inspection {
    pub fn is_blocked(&self) -> bool {
        self.findings
            .iter()
            .any(|f| f.severity == Severity::Block)
    }

    pub fn blockers(&self) -> Vec<&Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Block)
            .collect()
    }

    pub fn warnings(&self) -> Vec<&Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Warn)
            .collect()
    }

    /// 展示在文件卡片下方的告警文案。
    pub fn warning_messages(&self) -> Vec<String> {
        self.warnings()
            .iter()
            .map(|f| f.message.clone())
            .collect()
    }

    /// 合并成一条可直接作为错误原因显示的中文说明。
    pub fn blocker_message(&self) -> Option<String> {
        let items = self.blockers();
        if items.is_empty() {
            return None;
        }
        let mut message = String::from("该文件不能直接压缩：");
        for finding in items {
            message.push_str("\n  · ");
            message.push_str(&finding.message);
        }
        message.push_str(
            "\n建议：在 PowerPoint 中另存为未加密的 .pptx 副本后再试。原文件未被修改。",
        );
        Some(message)
    }

    pub fn codes(&self) -> Vec<&'static str> {
        self.findings.iter().map(|f| f.code).collect()
    }
}

/// 只根据 ZIP 条目名与主部件内容做分类。抽成纯函数便于回归测试。
pub fn classify(names: &[String], presentation_xml: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    // 调用方既可能传原始 ZIP 名，也可能传已归一化的名字，这里再做一次
    // 大小写与分隔符归一化，避免 IRM 的 `\u{6}DataSpaces` 之类路径漏检。
    let owned: Vec<String> = names
        .iter()
        .map(|n| {
            n.trim_start_matches('/')
                .to_ascii_lowercase()
                .replace('\\', "/")
        })
        .collect();
    let normalized: Vec<&str> = owned.iter().map(String::as_str).collect();
    let has_prefix = |p: &str| normalized.iter().any(|n| n.starts_with(p));
    let has_exact = |p: &str| normalized.contains(&p);
    let contains = |p: &str| normalized.iter().any(|n| n.contains(p));

    if normalized.is_empty() {
        out.push(Finding::block("empty", "压缩包内没有任何条目"));
        return out;
    }

    // ── 阻断项 ──────────────────────────────────────────────
    if has_exact("encryptioninfo")
        || has_exact("encryptedpackage")
        || contains("dataspace")
    {
        out.push(Finding::block(
            "encrypted",
            "演示文稿已加密或受信息权限保护（IRM），解压后无法还原为有效文稿",
        ));
    }
    if !has_exact("[content_types].xml") {
        out.push(Finding::block(
            "no_content_types",
            "缺少 [Content_Types].xml，不是有效的 OOXML 演示文稿",
        ));
    }

    // ── 告警项 ──────────────────────────────────────────────
    if has_prefix("_xmlsignatures/") || contains("_xmlsignatures") {
        out.push(Finding::warn(
            "signature",
            "包含数字签名，压缩后签名会失效，建议先另存副本",
        ));
    }
    if contains("vbaproject") {
        out.push(Finding::warn(
            "macro",
            "包含 VBA 宏，建议另存为普通 .pptx 后再压缩，并在 PowerPoint 中验证宏",
        ));
    }
    if has_prefix("ppt/embeddings/") {
        out.push(Finding::warn(
            "embedded_object",
            "包含 OLE 嵌入对象，压缩后请先在 PowerPoint 中验证这些对象",
        ));
    }
    if presentation_xml.to_ascii_lowercase().contains("modifyverifier") {
        out.push(Finding::warn(
            "modify_protected",
            "演示文稿设置了修改密码或只读保护，建议先取消保护",
        ));
    }
    if !has_exact("ppt/presentation.xml") {
        out.push(Finding::warn(
            "no_presentation",
            "未找到标准主部件 ppt/presentation.xml，请确认文稿完整",
        ));
    }

    out
}

/// 检查一个待压缩文件，不解压、不写磁盘。
pub fn inspect_pptx(path: &Path) -> io::Result<Inspection> {
    let mut head = [0u8; OLE_MAGIC.len()];
    {
        let mut file = fs::File::open(path)?;
        let read = file.read(&mut head)?;
        if read == OLE_MAGIC.len() && head == OLE_MAGIC {
            return Ok(Inspection {
                findings: vec![Finding::block(
                    "ole_container",
                    "这是旧版 .ppt 或加密的复合文档，不是可直接解压的 .pptx",
                )],
                entries: 0,
            });
        }
    }

    let file = fs::File::open(path)?;
    let mut archive = ZipArchive::new(file).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("无法读取为 PPTX 压缩包（{}）；请确认文件未损坏且已另存为 .pptx", e),
        )
    })?;

    let entries = archive.len();
    let mut names: Vec<String> = Vec::with_capacity(entries);
    let mut presentation_xml = String::new();
    for i in 0..entries {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let normalized = entry.name().to_ascii_lowercase().replace('\\', "/");
        if normalized.trim_start_matches('/') == "ppt/presentation.xml" && !entry.is_dir() {
            // 读取失败不阻断检测，只当作「没有可解析的保护标记」。
            let _ = (&mut entry)
                .take(MAX_XML_INSPECT)
                .read_to_string(&mut presentation_xml);
        }
        names.push(normalized);
    }

    Ok(Inspection {
        findings: classify(&names, &presentation_xml),
        entries,
    })
}
