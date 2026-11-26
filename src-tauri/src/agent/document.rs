//! 把上传的附件转成模型能读的纯文本。
//!
//! 分两类：
//! - 纯文本类（txt / md / 代码 / .cmd / .bat）直接按 UTF-8 读出
//! - 容器类（xlsx / xls / pdf / docx）需要各自解析
//!
//! 解析失败一律给出可操作的原因，不做静默降级——模型和用户都应该知道
//! 「这份文件没被读到」，而不是以为看到了全部内容。

use std::io::Read;

/// 单个附件最多抽这么多字符，超出截断。约 1 万汉字。
pub const MAX_TEXT_CHARS: usize = 12_000;

/// 附件解析结果。`notes` 用来告诉模型「哪里没拿到」。
#[derive(Debug, Default)]
pub struct Extracted {
    pub text: String,
    pub notes: Vec<String>,
    /// 识别出来的格式，用于在消息里标注
    pub kind: String,
}

impl Extracted {
    fn new(kind: impl Into<String>) -> Self {
        Self {
            text: String::new(),
            notes: Vec::new(),
            kind: kind.into(),
        }
    }

    /// 拼成最终交给模型的一段文本。
    pub fn render(&self, name: &str) -> String {
        let mut out = format!("【附件：{name}（{}）】", self.kind);
        if self.text.trim().is_empty() {
            out.push_str("\n（没有提取到文字内容）");
        } else {
            out.push('\n');
            out.push_str(self.text.trim_end());
        }
        for note in &self.notes {
            out.push('\n');
            out.push_str(note);
        }
        out
    }
}

fn truncate_chars(text: String, limit: usize) -> (String, bool) {
    if text.chars().count() <= limit {
        return (text, false);
    }
    let kept: String = text.chars().take(limit).collect();
    (kept, true)
}

fn is_text_ext(ext: &str) -> bool {
    matches!(
        ext,
        "txt" | "md" | "markdown"
            | "csv" | "tsv"
            | "json" | "yaml" | "yml" | "toml" | "ini" | "conf" | "cfg" | "env"
            | "log" | "xml" | "html" | "htm" | "css" | "scss" | "less"
            | "js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx"
            | "py" | "rs" | "go" | "java" | "kt" | "swift" | "c" | "h" | "cpp" | "hpp" | "cs"
            | "php" | "rb" | "pl" | "lua" | "sh" | "bash" | "zsh" | "ps1" | "psm1"
            | "cmd" | "bat"
            | "sql" | "graphql" | "proto" | "dockerfile" | "gitignore" | "editorconfig"
    )
}

/// 按扩展名解析附件。`bytes` 是完整文件内容。
pub fn extract(name: &str, bytes: &[u8]) -> Result<Extracted, String> {
    let ext = name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();

    if bytes.is_empty() {
        return Err(format!("{name} 是空文件"));
    }

    match ext.as_str() {
        "xlsx" | "xls" | "xlsm" | "ods" => read_spreadsheet(name, bytes),
        "pdf" => read_pdf(name, bytes),
        "docx" => read_docx(name, bytes),
        "ppt" | "pptx" => Err(format!(
            "{name}：暂不支持 PPT 解析，请另存为 PDF 或导出文字后再上传"
        )),
        ext if is_text_ext(ext) => read_plain(name, bytes),
        "" => {
            // 没有扩展名时按内容判断，可能是纯文本脚本
            if looks_like_text(bytes) {
                read_plain(name, bytes)
            } else {
                Err(format!("{name}：无法识别文件类型"))
            }
        }
        _ => {
            if looks_like_text(bytes) {
                read_plain(name, bytes)
            } else {
                Err(format!(
                    "{name}：暂不支持 .{ext} 格式，目前可读 txt / 代码 / cmd / Excel / PDF / Word"
                ))
            }
        }
    }
}

fn looks_like_text(bytes: &[u8]) -> bool {
    let sample = &bytes[..bytes.len().min(4096)];
    !sample.contains(&0) && std::str::from_utf8(sample).is_ok()
}

fn read_plain(name: &str, bytes: &[u8]) -> Result<Extracted, String> {
    let mut result = Extracted::new("文本");
    // .cmd/.bat 在中文 Windows 上常是 GBK，UTF-8 解不出时退回本地编码
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => decode_ansi(bytes)
            .ok_or_else(|| format!("{name}：不是有效的文本文件（编码无法识别）"))?,
    };
    let (kept, cut) = truncate_chars(text, MAX_TEXT_CHARS);
    result.text = kept;
    if cut {
        result
            .notes
            .push(format!("（内容过长，仅显示前 {MAX_TEXT_CHARS} 个字符）"));
    }
    Ok(result)
}

/// Windows 本地代码页解码。没有额外依赖时用 GBK 近似处理中文环境。
#[cfg(windows)]
fn decode_ansi(bytes: &[u8]) -> Option<String> {
    // UTF-16LE BOM
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        return String::from_utf16(&units).ok();
    }
    // 其余情况交给系统 API，避免自己实现 GBK 码表
    decode_with_system_codepage(bytes)
}

#[cfg(windows)]
fn decode_with_system_codepage(bytes: &[u8]) -> Option<String> {
    use windows_sys::Win32::Globalization::{MultiByteToWideChar, CP_ACP};
    unsafe {
        let length = MultiByteToWideChar(
            CP_ACP,
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            std::ptr::null_mut(),
            0,
        );
        if length <= 0 {
            return None;
        }
        let mut buffer = vec![0u16; length as usize];
        let written = MultiByteToWideChar(
            CP_ACP,
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            buffer.as_mut_ptr(),
            length,
        );
        if written <= 0 {
            return None;
        }
        buffer.truncate(written as usize);
        String::from_utf16(&buffer).ok()
    }
}

#[cfg(not(windows))]
fn decode_ansi(_bytes: &[u8]) -> Option<String> {
    None
}

fn read_spreadsheet(name: &str, bytes: &[u8]) -> Result<Extracted, String> {
    use calamine::{open_workbook_from_rs, Data, Reader, Sheets};

    let cursor = std::io::Cursor::new(bytes.to_vec());
    let mut workbook: Sheets<_> = open_workbook_from_rs(cursor)
        .map_err(|error| format!("{name}：无法读取表格（{error}）"))?;
    let sheets = workbook.sheet_names().to_vec();
    if sheets.is_empty() {
        return Err(format!("{name}：表格里没有工作表"));
    }

    let mut result = Extracted::new("Excel");
    let mut blocks = Vec::new();
    for sheet in &sheets {
        let Ok(range) = workbook.worksheet_range(sheet) else {
            result.notes.push(format!("（工作表 {sheet} 读取失败，已跳过）"));
            continue;
        };
        let mut lines: Vec<String> = Vec::new();
        let mut blank_streak = 0;
        for row in range.rows() {
            let cells: Vec<String> = row
                .iter()
                .map(|cell| match cell {
                    Data::Empty => String::new(),
                    Data::String(text) => text.clone(),
                    Data::Float(value) => format_number(*value),
                    Data::Int(value) => value.to_string(),
                    Data::Bool(value) => value.to_string(),
                    Data::DateTime(value) => value.to_string(),
                    Data::Error(error) => format!("#{error:?}"),
                    Data::DateTimeIso(text) | Data::DurationIso(text) => text.clone(),
                })
                .collect();
            // 整行空白不占位置，但连续 20 行空白就截断，避免表格末尾的空区
            if cells.iter().all(|cell: &String| cell.trim().is_empty()) {
                blank_streak += 1;
                if blank_streak >= 20 {
                    break;
                }
                continue;
            }
            blank_streak = 0;
            lines.push(cells.join("\t").trim_end().to_string());
            if lines.len() >= 2000 {
                lines.push("（该表内容过多，仅显示前 2000 行）".into());
                break;
            }
        }
        blocks.push(format!("— 工作表「{sheet}」—\n{}", lines.join("\n")));
    }

    let (kept, cut) = truncate_chars(blocks.join("\n\n"), MAX_TEXT_CHARS);
    result.text = kept;
    if cut {
        result
            .notes
            .push(format!("（表格内容过长，仅显示前 {MAX_TEXT_CHARS} 个字符）"));
    }
    Ok(result)
}

fn format_number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// 把 panic 的 payload 转成人能读的一句话。
fn panic_reason(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        return (*text).to_string();
    }
    if let Some(text) = payload.downcast_ref::<String>() {
        return text.clone();
    }
    "未知原因".into()
}

fn read_pdf(name: &str, bytes: &[u8]) -> Result<Extracted, String> {
    // pdf-extract 遇到不支持的字体编码（如 GBK-EUC-H）会直接 panic 而不是
    // 返回错误。放任不管会把整个对话流带崩，所以这里必须兜住。
    // 同时临时屏蔽 panic 消息，否则每份问题 PDF 都会往日志刷一屏堆栈。
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(bytes));
    std::panic::set_hook(previous);

    let text = outcome
        .map_err(|payload| {
            let reason = panic_reason(&payload);
            format!("{name}：PDF 里的字体编码暂不支持（{reason}），无法提取文字")
        })?
        .map_err(|error| format!("{name}：PDF 解析失败（{error}）"))?;
    let mut result = Extracted::new("PDF");
    let trimmed = text.trim();

    if trimmed.is_empty() {
        // 扫描件：整页是图片，没有文字层。明确告知，而不是返回空内容
        return Ok(Extracted {
            text: String::new(),
            notes: vec![
                "（这份 PDF 没有可提取的文字层，很可能是扫描件或纯图片）".into(),
                "（暂不支持把 PDF 页面转成图片识别）".into(),
            ],
            kind: "PDF（无文字层）".into(),
        });
    }

    let (kept, cut) = truncate_chars(trimmed.to_string(), MAX_TEXT_CHARS);
    result.text = kept;
    if cut {
        result
            .notes
            .push(format!("（PDF 内容过长，仅显示前 {MAX_TEXT_CHARS} 个字符）"));
    }
    // 图文混排：文字拿到了，但图片没拿，要说明白
    if contains_image_objects(bytes) {
        result
            .notes
            .push("（该 PDF 含图片内容，本次只提取了文字，图片未包含）".into());
    }
    Ok(result)
}

/// 粗略判断 PDF 里有没有内嵌图片对象。
fn contains_image_objects(bytes: &[u8]) -> bool {
    const NEEDLES: [&[u8]; 4] = [b"/Subtype/Image", b"/Subtype /Image", b"/DCTDecode", b"/JPXDecode"];
    NEEDLES
        .iter()
        .any(|needle| bytes.windows(needle.len()).any(|window| window == *needle))
}

fn read_docx(name: &str, bytes: &[u8]) -> Result<Extracted, String> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|error| format!("{name}：Word 文件损坏或不是 docx（{error}）"))?;
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(|_| format!("{name}：找不到正文内容，可能是 .doc 旧格式"))?
        .read_to_string(&mut xml)
        .map_err(|error| format!("{name}：读取正文失败（{error}）"))?;

    let mut result = Extracted::new("Word");
    let (kept, cut) = truncate_chars(xml_to_text(&xml), MAX_TEXT_CHARS);
    result.text = kept;
    if cut {
        result
            .notes
            .push(format!("（文档内容过长，仅显示前 {MAX_TEXT_CHARS} 个字符）"));
    }
    Ok(result)
}

/// 从 WordprocessingML 里抠出可见文字：段落换行，表格单元格用制表符分隔。
///
/// 只关心 `<w:t>` 里的文字和几种结构标签，所以不做完整 XML 解析，
/// 而是按标签边界做简单的扫描——先找到 `<` 再判断标签种类，绝不按固定
/// 长度跳过，避免把正文吃掉。
fn xml_to_text(xml: &str) -> String {
    let mut out = String::new();
    let mut rest = xml;
    let mut in_text = false;

    while let Some(start) = rest.find('<') {
        // 标签之前的文字只有在 <w:t> 里才算正文
        if in_text && start > 0 {
            push_unescaped(&mut out, &rest[..start]);
        }
        let tail = &rest[start..];
        let Some(end) = tail.find('>') else { break };
        let tag = &tail[..=end];

        if tag.starts_with("<w:t") && !tag.starts_with("</w:t") {
            in_text = true;
        } else if tag.starts_with("</w:t>") {
            in_text = false;
        } else if tag.starts_with("<w:tab") {
            out.push('\t');
        } else if tag.starts_with("<w:br") || tag.starts_with("</w:p>") {
            out.push('\n');
        } else if tag.starts_with("</w:tc>") {
            out.push('\t');
        }
        rest = &tail[end + 1..];
    }

    // 压掉行尾空白和过多空行
    let joined = out
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    let mut compact = String::new();
    let mut blank = 0;
    for line in joined.lines() {
        if line.trim().is_empty() {
            blank += 1;
            if blank > 2 {
                continue;
            }
        } else {
            blank = 0;
        }
        compact.push_str(line);
        compact.push('\n');
    }
    compact.trim().to_string()
}

/// 还原 XML 实体。
fn push_unescaped(out: &mut String, raw: &str) {
    let mut rest = raw;
    while let Some(index) = rest.find('&') {
        out.push_str(&rest[..index]);
        let entity = &rest[index..];
        let replacement = if entity.starts_with("&amp;") {
            Some(("&", 5))
        } else if entity.starts_with("&lt;") {
            Some(("<", 4))
        } else if entity.starts_with("&gt;") {
            Some((">", 4))
        } else if entity.starts_with("&quot;") {
            Some(("\"", 6))
        } else if entity.starts_with("&apos;") {
            Some(("'", 6))
        } else {
            None
        };
        match replacement {
            Some((text, length)) => {
                out.push_str(text);
                rest = &entity[length..];
            }
            None => {
                out.push('&');
                rest = &entity[1..];
            }
        }
    }
    out.push_str(rest);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_decoded() {
        let out = extract("a.txt", "第一行\n第二行".as_bytes()).unwrap();
        assert_eq!(out.text, "第一行\n第二行");
        assert_eq!(out.kind, "文本");
    }

    #[test]
    fn cmd_scripts_are_treated_as_text() {
        let out = extract("run.cmd", b"@echo off\r\necho hi\r\n").unwrap();
        assert!(out.text.contains("echo hi"));
    }

    #[test]
    fn code_files_are_treated_as_text() {
        for name in ["main.rs", "app.js", "x.py", "style.css", "q.sql"] {
            assert!(extract(name, b"content").is_ok(), "{name}");
        }
    }

    #[test]
    fn long_text_is_truncated_with_note() {
        let long = "字".repeat(MAX_TEXT_CHARS + 50);
        let out = extract("a.txt", long.as_bytes()).unwrap();
        assert_eq!(out.text.chars().count(), MAX_TEXT_CHARS);
        assert!(!out.notes.is_empty());
        assert!(out.notes[0].contains("过长"));
    }

    #[test]
    fn empty_file_is_rejected() {
        assert!(extract("a.txt", b"").is_err());
    }

    #[test]
    fn unknown_binary_is_rejected_with_hint() {
        let error = extract("a.exe", b"MZ\x90\x00\x00\x00").unwrap_err();
        assert!(error.contains("暂不支持"), "{error}");
    }

    #[test]
    fn ppt_is_rejected_clearly() {
        let error = extract("a.pptx", b"PK\x03\x04").unwrap_err();
        assert!(error.contains("PPT"), "{error}");
    }

    #[test]
    fn render_includes_name_and_kind() {
        let out = Extracted {
            text: "正文".into(),
            notes: vec!["（提示）".into()],
            kind: "文本".into(),
        };
        let rendered = out.render("a.txt");
        assert!(rendered.contains("a.txt"));
        assert!(rendered.contains("文本"));
        assert!(rendered.contains("正文"));
        assert!(rendered.contains("（提示）"));
    }

    #[test]
    fn empty_extraction_says_so() {
        let out = Extracted {
            text: String::new(),
            notes: vec![],
            kind: "PDF".into(),
        };
        assert!(out.render("a.pdf").contains("没有提取到文字"));
    }

    #[test]
    fn number_formatting_drops_trailing_zero() {
        assert_eq!(format_number(42.0), "42");
        assert_eq!(format_number(1.5), "1.5");
    }

    #[test]
    fn docx_xml_is_flattened() {
        let xml = r#"<w:document><w:body>
            <w:p><w:r><w:t>第一段</w:t></w:r></w:p>
            <w:p><w:r><w:t>第二</w:t></w:r><w:r><w:t>段</w:t></w:r></w:p>
            <w:tbl><w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc>
            <w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
        </w:body></w:document>"#;
        let text = xml_to_text(xml);
        assert!(text.contains("第一段"), "{text}");
        assert!(text.contains("第二段"), "{text}");
        assert!(text.contains('A') && text.contains('B'), "{text}");
    }

    #[test]
    fn docx_xml_unescapes_entities() {
        let xml = "<w:p><w:r><w:t>a &amp; b &lt; c</w:t></w:r></w:p>";
        assert_eq!(xml_to_text(xml), "a & b < c");
    }

    #[test]
    fn image_objects_are_detected_in_pdf_bytes() {
        assert!(contains_image_objects(b"...  /Subtype/Image ..."));
        assert!(!contains_image_objects(b"... plain text ..."));
    }

    #[test]
    fn scanned_pdf_reports_missing_text_layer() {
        // 最小 PDF：没有文字内容
        let pdf = b"%PDF-1.4\n1 0 obj<</Type/Catalog>>endobj\ntrailer<</Root 1 0 R>>\n%%EOF";
        let out = extract("scan.pdf", pdf);
        // 解析器可能报错或返回空，两种情况都要有明确说明
        match out {
            Ok(extracted) => {
                assert!(extracted.text.trim().is_empty());
                assert!(extracted.notes.iter().any(|note| note.contains("文字层")));
            }
            Err(error) => assert!(error.contains("PDF"), "{error}"),
        }
    }
}

#[cfg(test)]
mod real_pdf_tests {
    use super::*;

    /// 真实的一份带 GBK-EUC-H 字体的 PDF：pdf-extract 会 panic，
    /// 必须被 catch_unwind 接住并转成普通错误。
    ///
    /// 用 LITECHAT_PDF_FIXTURE 指向一份会触发该 panic 的 PDF 才会真正执行；
    /// 平时跳过，不依赖仓库里的二进制样本。
    #[test]
    fn unsupported_font_encoding_does_not_panic() {
        let Ok(path) = std::env::var("LITECHAT_PDF_FIXTURE") else {
            eprintln!("跳过：未设置 LITECHAT_PDF_FIXTURE");
            return;
        };
        if !std::path::Path::new(&path).is_file() {
            eprintln!("跳过：{path} 不存在");
            return;
        }
        let bytes = std::fs::read(&path).unwrap();
        let outcome = extract("sample.pdf", &bytes);
        match outcome {
            Ok(extracted) => eprintln!("解析成功，{} 字符", extracted.text.chars().count()),
            Err(message) => {
                assert!(!message.is_empty());
                eprintln!("按预期返回错误而不是崩溃：{message}");
            }
        }
    }
}

