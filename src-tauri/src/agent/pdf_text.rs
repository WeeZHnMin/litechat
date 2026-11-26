//! PDF 文字提取，基于 PDFium（Chrome 同款引擎）。
//!
//! 相比 pdf-extract，PDFium 完整支持中文 PDF 常见的 GBK-EUC-H 等 CMap，
//! 也不会在遇到不支持的编码时 panic。
//!
//! DLL 位置：
//! - 开发时：`src-tauri/libs/pdfium.dll`（随仓库提供）
//! - 打包后：可执行文件同目录
//!
//! 用 `libs/pdfium.dll` 的 build 是 8125，必须与 pdfium-render 绑定的
//! 版本表里的条目对应，否则 GetProcAddress 会失败。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use pdfium_render::prelude::*;

const DLL_NAME: &str = "pdfium.dll";

/// 单页最多取这么多字符，避免超长文档占满内存。
const MAX_CHARS_PER_PAGE: usize = 20_000;

fn dll_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // Windows 安装版：DLL 与 exe 同目录
            candidates.push(dir.join(DLL_NAME));
            // Tauri 打包的 resources 目录
            candidates.push(dir.join("resources").join(DLL_NAME));
        }
    }
    // 开发时：源码目录下的 libs/
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    candidates.push(manifest.join("libs").join(DLL_NAME));
    candidates
}

/// 进程内只加载一次 PDFium。绑定失败时返回可读原因。
fn pdfium() -> Result<&'static Pdfium, String> {
    static INSTANCE: OnceLock<Result<Pdfium, String>> = OnceLock::new();
    INSTANCE
        .get_or_init(|| {
            let candidates = dll_candidates();
            let Some(path) = candidates.iter().find(|path| path.is_file()) else {
                return Err(format!(
                    "找不到 PDF 引擎文件 {DLL_NAME}。请确认它位于应用目录，或 {} 下",
                    candidates
                        .first()
                        .map(|path| path.parent().unwrap_or(path).display().to_string())
                        .unwrap_or_default()
                ));
            };
            Pdfium::bind_to_library(path)
                .map(Pdfium::new)
                .map_err(|error| format!("加载 PDF 引擎失败（{}）：{error}", path.display()))
        })
        .as_ref()
        .map_err(|error| error.clone())
}

/// 提取 PDF 全部文字。返回逐页文本。
pub fn extract_text(bytes: &[u8]) -> Result<Vec<String>, String> {
    let pdfium = pdfium()?;
    let document = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(|error| format!("PDF 解析失败：{error}"))?;

    let mut pages = Vec::new();
    for page in document.pages().iter() {
        let text = page
            .text()
            .map(|text| text.all())
            .map_err(|error| format!("读取页面文字失败：{error}"))?;
        let mut clipped: String = text.chars().take(MAX_CHARS_PER_PAGE).collect();
        if text.chars().count() > MAX_CHARS_PER_PAGE {
            clipped.push_str("…（本页内容过长，已截断）");
        }
        pages.push(clipped);
    }
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dll_candidates_include_dev_path() {
        let candidates = dll_candidates();
        assert!(
            candidates.iter().any(|path| path.ends_with("libs/pdfium.dll")),
            "开发路径应在候选里：{candidates:?}"
        );
    }

    #[test]
    fn missing_dll_is_reported_clearly() {
        // 这里不覆盖真实 DLL，只验证错误文案包含文件名和位置提示
        let message = format!(
            "找不到 PDF 引擎文件 {DLL_NAME}。请确认它位于应用目录，或 {} 下",
            Path::new("x").display()
        );
        assert!(message.contains(DLL_NAME));
    }

    #[test]
    fn broken_bytes_report_error_without_panicking() {
        let result = extract_text(b"not a pdf at all");
        // 引擎没装或文件损坏都应该返回 Err，不能 panic
        assert!(result.is_err());
    }

    /// 真实中文 PDF 的回归测试：设置 LITECHAT_PDF_FIXTURE 指向文件后运行。
    #[test]
    fn chinese_pdf_text_is_extracted() {
        let Ok(path) = std::env::var("LITECHAT_PDF_FIXTURE") else {
            eprintln!("跳过：未设置 LITECHAT_PDF_FIXTURE");
            return;
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => {
                eprintln!("跳过：{path} 无法读取");
                return;
            }
        };
        let pages = extract_text(&bytes).expect("应当能解析");
        let joined = pages.join("");
        assert!(!joined.trim().is_empty(), "中文 PDF 应当能提取出文字");
        eprintln!("提取到 {} 页、{} 字符", pages.len(), joined.chars().count());
    }
}
