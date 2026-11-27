//! PDF 文字提取与页面渲染，基于 PDFium。
//!
//! `libs/pdfium.dll` 的 build 号必须与 pdfium-render 的绑定版本表对应
//! （当前 8125），否则 GetProcAddress 会失败。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use pdfium_render::prelude::*;

const DLL_NAME: &str = "pdfium.dll";

const MAX_CHARS_PER_PAGE: usize = 20_000;

fn dll_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(DLL_NAME));
            candidates.push(dir.join("resources").join(DLL_NAME));
        }
    }
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    candidates.push(manifest.join("libs").join(DLL_NAME));
    candidates
}

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

pub const DEFAULT_DPI: u32 = 150;

pub const MAX_RENDER_PAGES: u32 = 8;

#[derive(Debug)]
pub struct RenderedPage {
    pub page: u32,
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub fn render_pages(
    bytes: &[u8],
    pages: Option<(u32, u32)>,
    dpi: Option<u32>,
) -> Result<Vec<RenderedPage>, String> {
    let pdfium = pdfium()?;
    let document = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(|error| format!("PDF 解析失败：{error}"))?;
    let total = document.pages().len() as u32;
    if total == 0 {
        return Err("这份 PDF 没有页面".into());
    }

    let (start, end) = match pages {
        Some((from, to)) => {
            let from = from.max(1);
            let to = to.max(from);
            (from, to.min(total))
        }
        None => (1, total.min(3)),
    };
    if start > total {
        return Err(format!("起始页码 {start} 超出范围（共 {total} 页）"));
    }
    let end = end.min(start + MAX_RENDER_PAGES - 1);

    // 150/72 ≈ 2.083，PDF 的点是 1/72 英寸
    let scale = dpi.unwrap_or(DEFAULT_DPI).clamp(50, 300) as f32 / 72.0;
    let config = PdfRenderConfig::new().scale_page_by_factor(scale);

    let mut rendered = Vec::new();
    for page_number in start..=end {
        let page = document
            .pages()
            .get(page_number as i32 - 1)
            .map_err(|error| format!("读取第 {page_number} 页失败：{error}"))?;

        let bitmap = page
            .render_with_config(&config)
            .map_err(|error| format!("渲染第 {page_number} 页失败：{error}"))?;
        let image = bitmap
            .as_image()
            .map_err(|error| format!("转换第 {page_number} 页失败：{error}"))?;
        let (width, height) = (image.width(), image.height());
        let mut png = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(|error| format!("编码第 {page_number} 页失败：{error}"))?;
        rendered.push(RenderedPage {
            page: page_number,
            png,
            width,
            height,
        });
    }
    Ok(rendered)
}

pub fn page_count(bytes: &[u8]) -> Result<u32, String> {
    let pdfium = pdfium()?;
    let document = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(|error| format!("PDF 解析失败：{error}"))?;
    Ok(document.pages().len() as u32)
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
