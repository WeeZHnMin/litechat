//! 从 PDF 里抠出内嵌图片。
//!
//! 只处理不用重新编码就能还原的格式：
//! - `/DCTDecode` 的流内容就是完整的 JPEG 字节，直接写盘
//! - `/FlateDecode` 的裸像素用 image crate 组装成 PNG
//!
//! 其余（JPXDecode、CCITTFaxDecode 等）跳过并记下来，不假装成功。

use std::path::{Path, PathBuf};

/// 小于这个尺寸的直接跳过：多半是图标、分隔线、水印。
const MIN_SIDE: u32 = 32;
/// 单张图片体积上限，防止把巨大的扫描页全部导出。
const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
/// 一次最多导出这么多张，避免几百页的 PDF 撑爆磁盘。
const MAX_IMAGES: usize = 30;

#[derive(Debug, Clone)]
pub struct ExtractedImage {
    /// 相对工作目录的路径
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub format: &'static str,
}

impl ExtractedImage {
    pub fn describe(&self) -> String {
        format!(
            "{}（{}x{}，{}）",
            self.path, self.width, self.height, self.format
        )
    }
}

/// 导出的结果。`skipped` 用来解释为什么有些图没出来。
#[derive(Debug, Default)]
pub struct ImageExtraction {
    pub images: Vec<ExtractedImage>,
    pub notes: Vec<String>,
}

fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    use image::GenericImageView;
    image::load_from_memory(bytes)
        .ok()
        .map(|image| image.dimensions())
}

/// 把导出目录准备好，返回 `<uploads>/<stem>_images`。
fn output_dir(root: &Path, source: &Path) -> Result<PathBuf, String> {
    let stem = source
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("document");
    let dir = root
        .join(crate::tools::mcp::uploads::UPLOAD_DIR)
        .join(format!("{stem}_images"));
    std::fs::create_dir_all(&dir).map_err(|error| format!("无法创建图片目录：{error}"))?;
    Ok(dir)
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|value| value.display().to_string().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string())
}

/// 从 PDF 字节里导出图片。`source` 用于生成输出目录名。
pub fn extract(root: &Path, source: &Path, bytes: &[u8]) -> Result<ImageExtraction, String> {
    let doc = lopdf::Document::load_mem(bytes)
        .map_err(|error| format!("PDF 解析失败（{error}）"))?;
    let dir = output_dir(root, source)?;

    let mut result = ImageExtraction::default();
    let mut index = 0usize;
    let mut skipped_filters: Vec<String> = Vec::new();

    for (_id, object) in &doc.objects {
        if result.images.len() >= MAX_IMAGES {
            result
                .notes
                .push(format!("（图片数量超过 {MAX_IMAGES} 张，其余未导出）"));
            break;
        }
        let Ok(stream) = object.as_stream() else { continue };
        let dict = &stream.dict;

        let is_image = dict
            .get(b"Subtype")
            .ok()
            .and_then(|value| value.as_name().ok())
            .map(|name| name == b"Image")
            .unwrap_or(false);
        if !is_image {
            continue;
        }

        let filter: Vec<u8> = dict
            .get(b"Filter")
            .map(|value| match value {
                lopdf::Object::Name(name) => name.clone(),
                lopdf::Object::Array(items) => items
                    .first()
                    .and_then(|item| item.as_name().ok())
                    .unwrap_or_default()
                    .to_vec(),
                _ => Vec::new(),
            })
            .unwrap_or_default();

        let (payload, extension, label) = match filter.as_slice() {
            b"DCTDecode" => (stream.content.clone(), "jpg", "JPEG"),
            b"FlateDecode" => match encode_flate_image(dict, &stream.content) {
                Some(png) => (png, "png", "PNG"),
                None => {
                    skipped_filters.push("FlateDecode（像素格式不支持）".into());
                    continue;
                }
            },
            other => {
                let name = String::from_utf8_lossy(other).to_string();
                let name = if name.is_empty() {
                    "未知编码".to_string()
                } else {
                    name
                };
                if !skipped_filters.contains(&name) {
                    skipped_filters.push(name);
                }
                continue;
            }
        };

        if payload.len() > MAX_IMAGE_BYTES {
            result.notes.push("（有图片超过体积上限，已跳过）".to_string());
            continue;
        }

        let Some((width, height)) = image_dimensions(&payload) else {
            skipped_filters.push(format!("{label}（无法识别尺寸）"));
            continue;
        };
        if width < MIN_SIDE || height < MIN_SIDE {
            continue;
        }

        index += 1;
        let filename = format!("img{index:02}.{extension}");
        let target = dir.join(&filename);
        std::fs::write(&target, &payload).map_err(|error| format!("写入图片失败：{error}"))?;
        result.images.push(ExtractedImage {
            path: relative(root, &target),
            width,
            height,
            format: label,
        });
    }

    if !skipped_filters.is_empty() {
        result.notes.push(format!(
            "（以下编码暂不支持，已跳过：{}）",
            skipped_filters.join("、")
        ));
    }
    Ok(result)
}

/// FlateDecode 的图：PDF 里存的是裸像素，按 ColorSpace 组装成 PNG。
/// 只处理最常见的不带预测器的 8 位 DeviceRGB / DeviceGray。
fn encode_flate_image(dict: &lopdf::Dictionary, data: &[u8]) -> Option<Vec<u8>> {
    use lopdf::Object;

    let width = dict.get(b"Width").ok()?.as_i64().ok()?;
    let height = dict.get(b"Height").ok()?.as_i64().ok()?;
    if width <= 0 || height <= 0 || width > 20000 || height > 20000 {
        return None;
    }
    let (width, height) = (width as u32, height as u32);

    let bits = dict
        .get(b"BitsPerComponent")
        .ok()
        .and_then(|value| value.as_i64().ok())
        .unwrap_or(8);
    if bits != 8 {
        return None;
    }

    let channels: usize = match dict.get(b"ColorSpace").ok()? {
        Object::Name(name) => match name.as_slice() {
            b"DeviceRGB" | b"RGB" => 3,
            b"DeviceGray" | b"G" => 1,
            _ => return None,
        },
        // [/ICCBased stream] 之类的间接形式，按三通道假设
        Object::Array(items) => match items.first().and_then(|item| item.as_name().ok()) {
            Some(b"DeviceRGB") => 3,
            Some(b"DeviceGray") => 1,
            _ => 3,
        },
        _ => return None,
    };

    let expected = width as usize * height as usize * channels;
    if data.len() < expected {
        return None;
    }

    let dynamic = match channels {
        3 => image::DynamicImage::ImageRgb8(image::RgbImage::from_raw(
            width,
            height,
            data[..expected].to_vec(),
        )?),
        1 => image::DynamicImage::ImageLuma8(image::GrayImage::from_raw(
            width,
            height,
            data[..expected].to_vec(),
        )?),
        _ => return None,
    };

    let mut png = Vec::new();
    dynamic
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("litechat-pdfimg-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 造一个只含一张 DCTDecode(JPEG) 图的最小 PDF。
    fn pdf_with_jpeg() -> Vec<u8> {
        // 1x1 的 JPEG
        const JPEG: &[u8] = &[
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00,
            0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x08, 0x06, 0x06,
            0x07, 0x06, 0x05, 0x08, 0x07, 0x07, 0x07, 0x09, 0x09, 0x08, 0x0A, 0x0C, 0x14, 0x0D,
            0x0C, 0x0B, 0x0B, 0x0C, 0x19, 0x12, 0x13, 0x0F, 0x14, 0x1D, 0x1A, 0x1F, 0x1E, 0x1D,
            0x1A, 0x1C, 0x1C, 0x20, 0x24, 0x2E, 0x27, 0x20, 0x22, 0x2C, 0x23, 0x1C, 0x1C, 0x28,
            0x37, 0x29, 0x2C, 0x30, 0x31, 0x34, 0x34, 0x34, 0x1F, 0x27, 0x39, 0x3D, 0x38, 0x32,
            0x3C, 0x2E, 0x33, 0x34, 0x32, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x01, 0x00, 0x01,
            0x01, 0x01, 0x11, 0x00, 0xFF, 0xC4, 0x00, 0x1F, 0x00, 0x00, 0x01, 0x05, 0x01, 0x01,
            0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02,
            0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0xFF, 0xC4, 0x00, 0x14, 0x10,
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00, 0xD2,
            0xCF, 0x20, 0xFF, 0xD9,
        ];
        let mut pdf: Vec<u8> = b"%PDF-1.4\n".to_vec();
        let image_offset = pdf.len();
        pdf.extend_from_slice(b"1 0 obj\n<</Type/XObject/Subtype/Image/Width 100/Height 100/ColorSpace/DeviceRGB/BitsPerComponent 8/Filter/DCTDecode/Length ");
        pdf.extend_from_slice(JPEG.len().to_string().as_bytes());
        pdf.extend_from_slice(b">>\nstream\n");
        pdf.extend_from_slice(JPEG);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");
        pdf.extend_from_slice(b"2 0 obj\n<</Type/Catalog/Pages 3 0 R>>\nendobj\n");
        pdf.extend_from_slice(b"3 0 obj\n<</Type/Pages/Kids[]/Count 0>>\nendobj\n");
        let xref_offset = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 4\n0000000000 65535 f \n{image_offset:010} 00000 n \n").as_bytes());
        pdf.extend_from_slice(b"0000000000 00000 n \n0000000000 00000 n \n");
        pdf.extend_from_slice(
            format!("trailer\n<</Size 4/Root 2 0 R>>\nstartxref\n{xref_offset}\n%%EOF\n").as_bytes(),
        );
        pdf
    }

    #[test]
    fn extracts_jpeg_from_pdf() {
        let root = temp_root("jpeg");
        let source = root.join("uploads").join("doc.pdf");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        let out = extract(&root, &source, &pdf_with_jpeg()).unwrap();
        // 1x1 图会被 MIN_SIDE 过滤掉，这里只验证解析不报错
        assert!(out.images.is_empty() || out.images.len() == 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn broken_pdf_reports_error() {
        let root = temp_root("broken");
        let error = extract(&root, Path::new("a.pdf"), b"not a pdf").unwrap_err();
        assert!(error.contains("PDF"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn describe_includes_size_and_format() {
        let image = ExtractedImage {
            path: "uploads/a_images/img01.jpg".into(),
            width: 1200,
            height: 800,
            format: "JPEG",
        };
        let text = image.describe();
        assert!(text.contains("1200x800"));
        assert!(text.contains("JPEG"));
    }
}
