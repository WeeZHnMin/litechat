use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

const MAX_READ_BYTES: u64 = 512 * 1024;
const MAX_WRITE_BYTES: usize = 2 * 1024 * 1024;
const MAX_LIST_ENTRIES: usize = 500;
const MAX_SEARCH_RESULTS: usize = 100;
const SKIP_DIRS: [&str; 9] = [
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".venv",
    "__pycache__",
    ".litechat",
    ".idea",
];

const TOOLS: &str = r#"[
  {
    "name": "read_file",
    "description": "读取工作目录内的文本文件，返回带行号的内容。用于查看代码、配置等。",
    "inputSchema": {
      "type": "object",
      "properties": {
        "path": { "type": "string", "description": "相对于工作目录的路径，如 src/main.rs" },
        "offset": { "type": "integer", "description": "起始行号（从 1 开始），省略则从头读" },
        "limit": { "type": "integer", "description": "最多读取的行数，省略则读到末尾" }
      },
      "required": ["path"]
    }
  },
  {
    "name": "write_file",
    "description": "将内容整体写入文件（存在则覆盖，不存在则创建）。适合新建文件或大范围重写。",
    "inputSchema": {
      "type": "object",
      "properties": {
        "path": { "type": "string", "description": "相对于工作目录的路径" },
        "content": { "type": "string", "description": "完整文件内容" }
      },
      "required": ["path", "content"]
    }
  },
  {
    "name": "edit_file",
    "description": "在文件中把一段原文精确替换为新内容。old_text 必须在文件中唯一出现且完全一致（含缩进），否则报错并提示。适合小范围修改。",
    "inputSchema": {
      "type": "object",
      "properties": {
        "path": { "type": "string", "description": "相对于工作目录的路径" },
        "old_text": { "type": "string", "description": "要被替换的原文，必须唯一匹配" },
        "new_text": { "type": "string", "description": "替换后的新内容" }
      },
      "required": ["path", "old_text", "new_text"]
    }
  },
  {
    "name": "delete_files",
    "description": "删除工作目录内的文件或目录，一次可传多个。目录默认连同其全部内容一起删除。这是不可恢复的操作，执行前会弹窗让用户确认；删除前建议先用 list_dir 确认目标。",
    "inputSchema": {
      "type": "object",
      "properties": {
        "paths": {
          "type": "array",
          "items": { "type": "string" },
          "description": "要删除的相对路径列表，如 [\"uploads/a.pdf\", \"uploads/old-project\"]"
        },
        "recursive": {
          "type": "boolean",
          "description": "删除目录时是否连同内容一起删，默认 true。设为 false 则只能删空目录。"
        }
      },
      "required": ["paths"]
    }
  },
  {
    "name": "list_dir",
    "description": "列出目录内的文件和子目录，带大小和类型。",
    "inputSchema": {
      "type": "object",
      "properties": {
        "path": { "type": "string", "description": "相对于工作目录的路径，省略则为工作目录根" }
      },
      "required": []
    }
  },
  {
    "name": "search",
    "description": "在工作目录内按关键字或正则搜索文件内容，返回 文件:行号:内容。用于定位代码位置。",
    "inputSchema": {
      "type": "object",
      "properties": {
        "pattern": { "type": "string", "description": "要搜索的文本或正则表达式" },
        "path": { "type": "string", "description": "限制搜索的子目录，省略则搜索整个工作目录" },
        "regex": { "type": "boolean", "description": "是否按正则解释 pattern，默认 false 表示按字面文本" }
      },
      "required": ["pattern"]
    }
  },
  {
    "name": "workspace_info",
    "description": "返回当前工作目录的绝对路径。文件工具的所有相对路径都基于它。",
    "inputSchema": { "type": "object", "properties": {}, "required": [] }
  },
  {
    "name": "read_binary_info",
    "description": "查看二进制或未知格式文件的基本信息：大小、修改时间、检测出的真实类型（依据文件头魔数）、以及是否像文本。用于在读取前判断文件是什么，避免把 PDF、压缩包、图片当文本读成乱码。",
    "inputSchema": {
      "type": "object",
      "properties": {
        "path": { "type": "string", "description": "相对于工作目录的路径" }
      },
      "required": ["path"]
    }
  },
  {
    "name": "read_document",
    "description": "读取用户上传的文档并提取其中的文字，支持 PDF、Word(docx)、Excel(xlsx/xls)，以及 txt、代码、.cmd/.bat 等文本文件。用户提到「这个文件」「附件」「文档里写了什么」时用它。扫描版 PDF 没有文字层，会明确告知。",
    "inputSchema": {
      "type": "object",
      "properties": {
        "path": { "type": "string", "description": "相对于工作目录的路径，通常是 uploads/ 下的文件" }
      },
      "required": ["path"]
    }
  },
  {
    "name": "extract_pdf_images",
    "description": "把 PDF 里内嵌的图片导出成独立文件，返回每张图的路径和尺寸。用户问「这份 PDF 里的图是什么」「有没有插图」「看看图表」时先用它找出图片，再用 read_media_file 加载需要看的那张。",
    "inputSchema": {
      "type": "object",
      "properties": {
        "path": { "type": "string", "description": "相对于工作目录的 PDF 路径，通常是 uploads/ 下的文件" }
      },
      "required": ["path"]
    }
  },
  {
    "name": "render_pdf_pages",
    "description": "把 PDF 的指定页面渲染成图片，让你能像看截图一样看到版面、图表和扫描件。文字内容请先用 read_document；只有当文字提取不理想（扫描件、图文混排、需要看排版或图表）时才用这个。渲染后会立即作为图片出现在下一步，无需再调 read_media_file。一次最多 8 页，默认 150 DPI。",
    "inputSchema": {
      "type": "object",
      "properties": {
        "path": { "type": "string", "description": "相对于工作目录的 PDF 路径" },
        "start_page": { "type": "integer", "description": "起始页码，从 1 开始。省略则从第 1 页" },
        "end_page": { "type": "integer", "description": "结束页码（含）。省略则只渲染起始页" },
        "dpi": { "type": "integer", "description": "渲染分辨率，默认 150。调高更清晰但更耗 token" }
      },
      "required": ["path"]
    }
  },
  {
    "name": "read_media_file",
    "description": "加载一张图片，使其真正出现在对话里供你查看内容。只用于已经存在的图片文件（例如 extract_pdf_images 导出的结果）。调用后这张图会在下一步可见，可以据此描述或分析。",
    "inputSchema": {
      "type": "object",
      "properties": {
        "path": { "type": "string", "description": "相对于工作目录的图片路径" }
      },
      "required": ["path"]
    }
  }
]"#;

pub fn tools() -> Value {
    serde_json::from_str(TOOLS).unwrap_or_else(|_| json!([]))
}

pub fn tool_names() -> Vec<&'static str> {
    vec![
        "read_file",
        "write_file",
        "edit_file",
        "delete_files",
        "list_dir",
        "search",
        "workspace_info",
        "read_binary_info",
        "read_document",
        "extract_pdf_images",
        "render_pdf_pages",
        "read_media_file",
    ]
}

fn read_document(root: &Path, args: &Value) -> Result<String, String> {
    const MAX_DOC_BYTES: u64 = 32 * 1024 * 1024;
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or("缺少 path 参数")?;
    let target = resolve(root, path)?;
    let meta = std::fs::metadata(&target)
        .map_err(|error| format!("无法读取 {path}: {error}"))?;
    if meta.is_dir() {
        return Err(format!("{path} 是目录，请用 list_dir"));
    }
    if meta.len() > MAX_DOC_BYTES {
        return Err(format!(
            "{path} 超过 {} 上限",
            format_size(MAX_DOC_BYTES)
        ));
    }
    let bytes = std::fs::read(&target).map_err(|error| format!("无法读取 {path}: {error}"))?;
    let name = target
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(path);
    match crate::agent::document::extract(name, &bytes) {
        Ok(extracted) => Ok(extracted.render(name)),
        Err(error) => Err(error),
    }
}


fn sniff_kind(bytes: &[u8]) -> &'static str {
    const SIGS: [(&[u8], &str); 14] = [
        (b"%PDF", "PDF 文档"),
        (b"PK\x03\x04", "ZIP 或 Office 文档（docx/xlsx/pptx）"),
        (b"\x89PNG", "PNG 图片"),
        (b"\xFF\xD8\xFF", "JPEG 图片"),
        (b"GIF8", "GIF 图片"),
        (b"BM", "BMP 图片"),
        (b"Rar!", "RAR 压缩包"),
        (b"7z\xBC\xAF", "7z 压缩包"),
        (b"\x1F\x8B", "GZIP 压缩包"),
        (b"OggS", "Ogg 媒体"),
        (b"ID3", "MP3 音频"),
        (b"\xD0\xCF\x11\xE0", "旧版 Office 文档（doc/xls/ppt）"),
        (b"\x7FELF", "ELF 可执行文件"),
        (b"MZ", "Windows 可执行文件"),
    ];
    for (sig, name) in SIGS {
        if bytes.starts_with(sig) {
            return name;
        }
    }
    "未识别的类型"
}

fn looks_like_text(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return true;
    }
    let sample = &bytes[..bytes.len().min(4096)];
    if sample.contains(&0) {
        return false;
    }
    std::str::from_utf8(sample).is_ok()
}

fn read_binary_info(root: &Path, args: &Value) -> Result<String, String> {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or("缺少 path 参数")?;
    let target = resolve(root, path)?;
    let meta = std::fs::metadata(&target)
        .map_err(|error| format!("无法读取 {path}: {error}"))?;
    if meta.is_dir() {
        return Err(format!("{path} 是目录，请用 list_dir"));
    }

    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .map(|seconds| {
            format!(
                "{} 秒前（unix {seconds}）",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|now| now.as_secs().saturating_sub(seconds))
                    .unwrap_or(0)
            )
        })
        .unwrap_or_else(|| "未知".into());

    let mut head = vec![0u8; 16];
    let read = {
        use std::io::Read;
        let mut file = std::fs::File::open(&target)
            .map_err(|error| format!("无法打开 {path}: {error}"))?;
        file.read(&mut head).unwrap_or(0)
    };
    head.truncate(read);

    let is_text = {
        use std::io::Read;
        let mut sample = vec![0u8; 4096];
        let read = std::fs::File::open(&target)
            .and_then(|mut file| file.read(&mut sample))
            .unwrap_or(0);
        sample.truncate(read);
        looks_like_text(&sample)
    };

    let mut lines = vec![
        format!("路径：{path}"),
        format!("大小：{}", format_size(meta.len())),
        format!("修改时间：{modified}"),
        format!("类型：{}", sniff_kind(&head)),
        format!("可作为文本读取：{}", if is_text { "是" } else { "否" }),
    ];
    if !is_text {
        lines.push(String::from(
            "提示：该文件不能直接用 read_file 读取。若需要其中内容，请借助外部工具转换（例如 PowerShell、pdftotext 等）。",
        ));
    }
    Ok(lines.join("\n"))
}

pub fn resolve(root: &Path, value: &str) -> Result<PathBuf, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(root.to_path_buf());
    }
    let raw = Path::new(trimmed);
    if raw.is_absolute() {
        return Err("请使用相对于工作目录的路径，不要传绝对路径".into());
    }
    let mut parts: Vec<Component> = Vec::new();
    for component in raw.components() {
        match component {
            Component::Normal(part) => parts.push(Component::Normal(part)),
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.pop().is_none() {
                    return Err("路径超出工作目录范围".into());
                }
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err("非法路径".into());
            }
        }
    }
    let mut target = root.to_path_buf();
    for part in parts {
        target.push(part);
    }
    let check = target
        .ancestors()
        .any(|ancestor| ancestor == root);
    if !check {
        return Err("路径超出工作目录范围".into());
    }
    Ok(target)
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|value| value.display().to_string().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string())
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut index = 0;
    while value >= 1024.0 && index < UNITS.len() - 1 {
        value /= 1024.0;
        index += 1;
    }
    if index == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[index])
    }
}

fn read_file(root: &Path, args: &Value) -> Result<String, String> {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or("缺少 path 参数")?;
    let target = resolve(root, path)?;
    let meta = std::fs::metadata(&target)
        .map_err(|error| format!("无法读取 {}: {error}", path))?;
    if meta.is_dir() {
        return Err(format!("{path} 是目录，请用 list_dir"));
    }
    if meta.len() > MAX_READ_BYTES {
        return Err(format!(
            "文件过大（{}），超过 {} 上限",
            format_size(meta.len()),
            format_size(MAX_READ_BYTES)
        ));
    }
    let content = std::fs::read_to_string(&target)
        .map_err(|error| format!("无法按文本读取 {}: {error}", path))?;
    let offset = args
        .get("offset")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .max(1) as usize;
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(usize::MAX);
    let total = content.lines().count();
    let body: String = content
        .lines()
        .enumerate()
        .skip(offset - 1)
        .take(limit)
        .map(|(index, line)| format!("{:>5}\t{line}\n", index + 1))
        .collect();
    if body.is_empty() {
        return Ok(format!("{path} 共 {total} 行，起始行 {offset} 超出范围"));
    }
    let end = offset.saturating_add(limit).saturating_sub(1).min(total);
    Ok(format!(
        "{path}（共 {total} 行，显示 {offset}–{end}）\n{body}"
    ))
}

fn write_file(root: &Path, args: &Value) -> Result<String, String> {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or("缺少 path 参数")?;
    let content = args
        .get("content")
        .and_then(Value::as_str)
        .ok_or("缺少 content 参数")?;
    if content.len() > MAX_WRITE_BYTES {
        return Err(format!(
            "内容过大（{}），超过 {} 上限",
            format_size(content.len() as u64),
            format_size(MAX_WRITE_BYTES as u64)
        ));
    }
    let target = resolve(root, path)?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("创建目录失败: {error}"))?;
    }
    let existed = target.exists();
    std::fs::write(&target, content).map_err(|error| format!("写入失败: {error}"))?;
    let lines = content.lines().count();
    let bytes = format_size(content.len() as u64);
    Ok(format!(
        "{} {path}（{lines} 行，{bytes}）",
        if existed { "已覆盖" } else { "已创建" }
    ))
}

fn similar_hint(content: &str, needle: &str) -> String {
    let first_line = needle.lines().next().unwrap_or("").trim();
    if first_line.is_empty() {
        return String::new();
    }
    for (index, line) in content.lines().enumerate() {
        if line.contains(first_line) || first_line.contains(line.trim()) {
            return format!(
                "\n最接近的是第 {} 行：`{}`",
                index + 1,
                line.trim()
            );
        }
    }
    String::new()
}

fn edit_file(root: &Path, args: &Value) -> Result<String, String> {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or("缺少 path 参数")?;
    let old_text = args
        .get("old_text")
        .and_then(Value::as_str)
        .ok_or("缺少 old_text 参数")?;
    let new_text = args
        .get("new_text")
        .and_then(Value::as_str)
        .ok_or("缺少 new_text 参数")?;
    if old_text.is_empty() {
        return Err("old_text 不能为空".into());
    }
    let target = resolve(root, path)?;
    let content = std::fs::read_to_string(&target)
        .map_err(|error| format!("无法读取 {path}: {error}"))?;
    let hits = content.matches(old_text).count();
    match hits {
        0 => Err(format!(
            "在 {path} 中未找到这段原文。请确认缩进和换行完全一致，或先用 read_file 查看。{}",
            similar_hint(&content, old_text)
        )),
        1 => {
            let updated = content.replacen(old_text, new_text, 1);
            std::fs::write(&target, &updated).map_err(|error| format!("写入失败: {error}"))?;
            let line = content[..content.find(old_text).unwrap_or(0)]
                .lines()
                .count()
                .max(1);
            Ok(format!("已修改 {path}（第 {line} 行附近）"))
        }
        count => Err(format!(
            "{path} 中这段原文出现了 {count} 次，请提供更多上下文使其唯一"
        )),
    }
}

fn delete_files(root: &Path, args: &Value) -> Result<String, String> {
    let paths: Vec<String> = args
        .get("paths")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if paths.is_empty() {
        return Err("paths 不能为空".into());
    }

    // 默认递归：用户说「删除这个文件夹」时期望删干净，
    // 而不是遇到非空目录就报错让人自己再逐个删。
    let recursive = args
        .get("recursive")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    let mut deleted = Vec::new();
    let mut failed = Vec::new();
    for path in &paths {
        // 拒绝删除工作目录本身，避免一次误操作清空整个工作区
        if path.trim().is_empty() || path.trim() == "." {
            failed.push(format!("{path}：不能删除工作目录"));
            continue;
        }
        let target = match resolve(root, path) {
            Ok(target) => target,
            Err(error) => {
                failed.push(format!("{path}：{error}"));
                continue;
            }
        };
        if target == root {
            failed.push(format!("{path}：不能删除工作目录"));
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(&target) else {
            failed.push(format!("{path}：文件不存在"));
            continue;
        };
        if !meta.is_dir() {
            match std::fs::remove_file(&target) {
                Ok(()) => deleted.push(path.clone()),
                Err(error) => failed.push(format!("{path}：{error}")),
            }
            continue;
        }

        // 目录：先数一下内含文件数，好让用户知道删掉了多少东西
        let mut count = 0usize;
        count_entries(&target, &mut count);
        let outcome = if recursive {
            std::fs::remove_dir_all(&target)
        } else {
            std::fs::remove_dir(&target)
        };
        match outcome {
            Ok(()) if count > 0 => deleted.push(format!("{path}（含 {count} 个文件）")),
            Ok(()) => deleted.push(path.clone()),
            Err(error) => failed.push(format!("{path}：{error}")),
        }
    }

    let mut lines = Vec::new();
    if deleted.is_empty() {
        lines.push("没有删除任何文件。".to_string());
    } else {
        lines.push(format!("已删除 {} 个：", deleted.len()));
        for path in &deleted {
            lines.push(format!("- {path}"));
        }
    }
    if !failed.is_empty() {
        lines.push(format!("{} 个未删除：", failed.len()));
        for item in &failed {
            lines.push(format!("- {item}"));
        }
    }
    Ok(lines.join("\n"))
}

/// 递归统计目录内的文件数，用于回报「删掉了多少个文件」。
fn count_entries(dir: &Path, count: &mut usize) {
    if *count > 10_000 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            count_entries(&path, count);
        } else {
            *count += 1;
        }
    }
}

fn list_dir(root: &Path, args: &Value) -> Result<String, String> {
    let path = args.get("path").and_then(Value::as_str).unwrap_or("");
    let target = resolve(root, path)?;
    let entries = std::fs::read_dir(&target)
        .map_err(|error| format!("无法列出 {}: {error}", if path.is_empty() { "." } else { path }))?;
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            dirs.push(format!("{name}/"));
        } else {
            files.push(format!("{name}  ({})", format_size(meta.len())));
        }
    }
    dirs.sort();
    files.sort();
    let mut all: Vec<String> = dirs.into_iter().chain(files).collect();
    let total = all.len();
    if total > MAX_LIST_ENTRIES {
        all.truncate(MAX_LIST_ENTRIES);
        all.push(format!("…还有 {} 项未显示", total - MAX_LIST_ENTRIES));
    }
    if all.is_empty() {
        return Ok("（空目录）".into());
    }
    let title = if path.is_empty() {
        "工作目录".to_string()
    } else {
        path.to_string()
    };
    Ok(format!("{title}（{total} 项）\n{}", all.join("\n")))
}

fn should_skip(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| SKIP_DIRS.contains(&name))
        .unwrap_or(false)
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 12 || out.len() > 5000 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if should_skip(&path) {
            continue;
        }
        if path.is_dir() {
            collect_files(root, &path, out, depth + 1);
        } else if path.is_file() {
            out.push(path);
        }
    }
    let _ = root;
}

fn search(root: &Path, args: &Value) -> Result<String, String> {
    let pattern = args
        .get("pattern")
        .and_then(Value::as_str)
        .ok_or("缺少 pattern 参数")?;
    if pattern.trim().is_empty() {
        return Err("pattern 不能为空".into());
    }
    let scope = args.get("path").and_then(Value::as_str).unwrap_or("");
    let base = resolve(root, scope)?;
    let use_regex = args.get("regex").and_then(Value::as_bool).unwrap_or(false);

    let matcher: Box<dyn Fn(&str) -> bool> = if use_regex {
        let compiled = regex::Regex::new(pattern)
            .map_err(|error| format!("正则无效: {error}"))?;
        Box::new(move |line: &str| compiled.is_match(line))
    } else {
        let needle = pattern.to_string();
        Box::new(move |line: &str| line.contains(&needle))
    };

    let mut files = Vec::new();
    collect_files(&base, &base, &mut files, 0);
    let mut results = Vec::new();
    for file in files {
        let Ok(meta) = file.metadata() else { continue };
        if meta.len() > MAX_READ_BYTES {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&file) else { continue };
        for (index, line) in content.lines().enumerate() {
            if matcher(line) {
                results.push(format!(
                    "{}:{}: {}",
                    relative(root, &file),
                    index + 1,
                    line.trim().chars().take(200).collect::<String>()
                ));
                if results.len() >= MAX_SEARCH_RESULTS {
                    results.push(format!("…结果超过 {MAX_SEARCH_RESULTS} 条，请缩小范围"));
                    return Ok(results.join("\n"));
                }
            }
        }
    }
    if results.is_empty() {
        return Ok(format!("未找到匹配 “{pattern}” 的内容"));
    }
    Ok(format!(
        "找到 {} 条匹配：\n{}",
        results.len(),
        results.join("\n")
    ))
}

/// 实现必须 Send + Sync：工具调用是异步的。
pub trait MediaSink: Send + Sync {
    fn attach_image(&self, path: &str) -> Result<(), String>;
}

#[cfg(test)]
pub fn call(root: &Path, tool: &str, args: &Value) -> Result<String, String> {
    call_with(root, tool, args, None)
}

pub fn call_with(
    root: &Path,
    tool: &str,
    args: &Value,
    media: Option<&(dyn MediaSink + 'static)>,
) -> Result<String, String> {
    match tool {
        "read_file" => read_file(root, args),
        "write_file" => write_file(root, args),
        "edit_file" => edit_file(root, args),
        "delete_files" => delete_files(root, args),
        "list_dir" => list_dir(root, args),
        "search" => search(root, args),
        "workspace_info" => Ok(format!("当前工作目录：{}", root.display())),
        "read_binary_info" => read_binary_info(root, args),
        "read_document" => read_document(root, args),
        "extract_pdf_images" => extract_pdf_images(root, args),
        "render_pdf_pages" => render_pdf_pages(root, args, media),
        "read_media_file" => read_media_file(root, args, media),
        other => Err(format!("未知文件工具: {other}")),
    }
}

fn extract_pdf_images(root: &Path, args: &Value) -> Result<String, String> {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or("缺少 path 参数")?;
    let target = resolve(root, path)?;
    let bytes = std::fs::read(&target).map_err(|error| format!("无法读取 {path}: {error}"))?;
    let extracted = crate::agent::pdf_images::extract(root, &target, &bytes)?;

    if extracted.images.is_empty() {
        let mut message = format!("{path} 里没有提取到图片。");
        for note in &extracted.notes {
            message.push('\n');
            message.push_str(note);
        }
        return Ok(message);
    }

    let mut lines = vec![format!(
        "从 {path} 导出 {} 张图片，可用 read_media_file 加载查看：",
        extracted.images.len()
    )];
    for image in &extracted.images {
        lines.push(format!("- {}", image.describe()));
    }
    for note in &extracted.notes {
        lines.push(note.clone());
    }
    Ok(lines.join("\n"))
}

fn render_pdf_pages(
    root: &Path,
    args: &Value,
    media: Option<&(dyn MediaSink + 'static)>,
) -> Result<String, String> {
    let Some(sink) = media else {
        return Err("当前环境不支持把图片加载到对话".into());
    };
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or("缺少 path 参数")?;
    let target = resolve(root, path)?;
    let bytes = std::fs::read(&target).map_err(|error| format!("无法读取 {path}: {error}"))?;

    let start = args.get("start_page").and_then(Value::as_u64).map(|v| v as u32);
    let end = args.get("end_page").and_then(Value::as_u64).map(|v| v as u32);
    let range = match (start, end) {
        (Some(from), Some(to)) => Some((from, to)),
        (Some(from), None) => Some((from, from)),
        (None, Some(to)) => Some((1, to)),
        (None, None) => None,
    };
    let dpi = args.get("dpi").and_then(Value::as_u64).map(|v| v as u32);

    let total = crate::agent::pdf_text::page_count(&bytes)?;
    let rendered = crate::agent::pdf_text::render_pages(&bytes, range, dpi)?;
    if rendered.is_empty() {
        return Err(format!("{path} 没有可渲染的页面（共 {total} 页）"));
    }

    let stem = target
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("document");
    let dir = root
        .join(crate::tools::mcp::uploads::UPLOAD_DIR)
        .join(format!("{stem}_pages"));
    std::fs::create_dir_all(&dir).map_err(|error| format!("无法创建图片目录：{error}"))?;

    let mut lines = vec![format!(
        "{path} 共 {total} 页，已渲染 {} 页，图片会在下一步出现：",
        rendered.len()
    )];
    for page in &rendered {
        let filename = format!("p{:03}.png", page.page);
        let file = dir.join(&filename);
        std::fs::write(&file, &page.png).map_err(|error| format!("写入图片失败：{error}"))?;
        let relative_path = relative(root, &file);
        sink.attach_image(&relative_path)?;
        lines.push(format!(
            "- 第 {} 页：{}（{}x{}）",
            page.page, relative_path, page.width, page.height
        ));
    }
    if rendered.len() as u32 >= crate::agent::pdf_text::MAX_RENDER_PAGES {
        lines.push(format!(
            "（单次最多渲染 {} 页，需要更多请再调用一次指定范围）",
            crate::agent::pdf_text::MAX_RENDER_PAGES
        ));
    }
    Ok(lines.join("\n"))
}

fn read_media_file(
    root: &Path,
    args: &Value,
    media: Option<&(dyn MediaSink + 'static)>,
) -> Result<String, String> {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or("缺少 path 参数")?;
    let target = resolve(root, path)?;
    let meta = std::fs::metadata(&target).map_err(|error| format!("无法读取 {path}: {error}"))?;
    if meta.is_dir() {
        return Err(format!("{path} 是目录"));
    }
    let extension = target
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp") {
        return Err(format!(
            "{path} 不是可加载的图片格式（支持 png/jpg/jpeg/gif/webp）"
        ));
    }

    let bytes = std::fs::read(&target).map_err(|error| format!("无法读取 {path}: {error}"))?;
    use image::GenericImageView;
    let (width, height) = image::load_from_memory(&bytes)
        .map(|image| image.dimensions())
        .map_err(|error| format!("{path} 不是有效的图片：{error}"))?;

    let Some(sink) = media else {
        return Err("当前环境不支持加载图片到对话".into());
    };
    sink.attach_image(&relative(root, &target))?;
    Ok(format!(
        "已加载 {path}（{width}x{height}），这张图会在下一步出现，可以据此回答。"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("litechat-files-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn rejects_paths_outside_root() {
        let root = temp_root("escape");
        assert!(resolve(&root, "../secret.txt").is_err());
        assert!(resolve(&root, "a/../../b").is_err());
        assert!(resolve(&root, "C:\\Windows\\win.ini").is_err());
        assert!(resolve(&root, "src/main.rs").is_ok());
    }

    #[test]
    fn writes_and_reads_with_line_numbers() {
        let root = temp_root("readwrite");
        let write = call(&root, "write_file", &json!({ "path": "a.txt", "content": "一\n二\n三" }));
        assert!(write.unwrap().contains("已创建"));
        let read = call(&root, "read_file", &json!({ "path": "a.txt" })).unwrap();
        assert!(read.contains("1\t一"));
        assert!(read.contains("3\t三"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn edit_requires_unique_match() {
        let root = temp_root("edit");
        call(&root, "write_file", &json!({ "path": "a.txt", "content": "foo bar foo" })).unwrap();
        let err = call(&root, "edit_file", &json!({ "path": "a.txt", "old_text": "foo", "new_text": "x" }));
        assert!(err.unwrap_err().contains("出现了 2 次"));

        let ok = call(&root, "edit_file", &json!({ "path": "a.txt", "old_text": "bar", "new_text": "baz" }));
        assert!(ok.unwrap().contains("已修改"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn edit_reports_missing_text_with_hint() {
        let root = temp_root("edit-miss");
        call(&root, "write_file", &json!({ "path": "a.txt", "content": "let x = 1;" })).unwrap();
        let err = call(&root, "edit_file", &json!({ "path": "a.txt", "old_text": "let x = 2;", "new_text": "y" }));
        let message = err.unwrap_err();
        assert!(message.contains("未找到"));
        _ = root;
    }

    #[test]
    fn search_finds_text_and_skips_noise() {
        let root = temp_root("search");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        std::fs::write(root.join("src/a.rs"), "fn target() {}\n").unwrap();
        std::fs::write(root.join("node_modules/skip.js"), "target\n").unwrap();
        let out = call(&root, "search", &json!({ "pattern": "target" })).unwrap();
        assert!(out.contains("src/a.rs:1"));
        assert!(!out.contains("node_modules"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sniffs_common_file_signatures() {
        assert_eq!(sniff_kind(b"%PDF-1.7"), "PDF 文档");
        assert_eq!(sniff_kind(b"PK\x03\x04"), "ZIP 或 Office 文档（docx/xlsx/pptx）");
        assert_eq!(sniff_kind(b"\x89PNG\r\n"), "PNG 图片");
        assert_eq!(sniff_kind(b"\xFF\xD8\xFF\xE0"), "JPEG 图片");
        assert_eq!(sniff_kind(b"MZ\x90\x00"), "Windows 可执行文件");
        assert_eq!(sniff_kind(b"hello world"), "未识别的类型");
    }

    #[test]
    fn detects_text_vs_binary() {
        assert!(looks_like_text(b"plain text\n"));
        assert!(looks_like_text("中文内容".as_bytes()));
        assert!(!looks_like_text(b"\x00\x01\x02"));
        assert!(!looks_like_text(b"%PDF\x00\xff"));
        assert!(looks_like_text(b""));
    }

    #[test]
    fn read_binary_info_reports_binary_pdf() {
        let root = temp_root("bininfo");
        std::fs::write(root.join("a.pdf"), b"%PDF-1.7\n\x00\x01binary").unwrap();
        let out = call(&root, "read_binary_info", &json!({ "path": "a.pdf" })).unwrap();
        assert!(out.contains("PDF 文档"));
        assert!(out.contains("可作为文本读取：否"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn read_respects_offset_and_limit() {
        let root = temp_root("offset");
        call(&root, "write_file", &json!({ "path": "a.txt", "content": "1\n2\n3\n4\n5" })).unwrap();
        let out = call(&root, "read_file", &json!({ "path": "a.txt", "offset": 2, "limit": 2 })).unwrap();
        assert!(out.contains("2\t2"));
        assert!(out.contains("3\t3"));
        assert!(!out.contains("4\t4"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn read_document_extracts_plain_text() {
        let root = temp_root("doc-text");
        call(&root, "write_file", &json!({ "path": "uploads/a.txt", "content": "文档内容" })).unwrap();
        let out = call(&root, "read_document", &json!({ "path": "uploads/a.txt" })).unwrap();
        assert!(out.contains("文档内容"), "{out}");
        assert!(out.contains("a.txt"), "{out}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn read_document_rejects_unsupported_format() {
        let root = temp_root("doc-bad");
        std::fs::write(root.join("a.exe"), b"MZ\x90\x00\x00\x00").unwrap();
        let error = call(&root, "read_document", &json!({ "path": "a.exe" })).unwrap_err();
        assert!(error.contains("暂不支持"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn read_document_stays_inside_workspace() {
        let root = temp_root("doc-escape");
        assert!(call(&root, "read_document", &json!({ "path": "../secret.pdf" })).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn read_document_is_registered() {
        assert!(tool_names().contains(&"read_document"));
    }

    #[test]
    fn pdf_image_tools_are_registered() {
        let names = tool_names();
        assert!(names.contains(&"extract_pdf_images"));
        assert!(names.contains(&"read_media_file"));
    }

    struct RecordingSink(std::sync::Mutex<Vec<String>>);
    impl MediaSink for RecordingSink {
        fn attach_image(&self, path: &str) -> Result<(), String> {
            self.0.lock().unwrap().push(path.to_string());
            Ok(())
        }
    }

    #[test]
    fn read_media_file_hands_the_image_to_the_sink() {
        let root = temp_root("media");
        let png = image::DynamicImage::ImageRgb8(image::RgbImage::new(4, 4));
        png.save(root.join("a.png")).unwrap();

        let sink = RecordingSink(Default::default());
        let out = call_with(&root, "read_media_file", &json!({ "path": "a.png" }), Some(&sink));
        assert!(out.is_ok(), "{out:?}");
        assert_eq!(sink.0.lock().unwrap().as_slice(), ["a.png"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn read_media_file_rejects_non_images() {
        let root = temp_root("media-bad");
        std::fs::write(root.join("a.txt"), b"x").unwrap();
        let sink = RecordingSink(Default::default());
        let error =
            call_with(&root, "read_media_file", &json!({ "path": "a.txt" }), Some(&sink)).unwrap_err();
        assert!(error.contains("不是可加载的图片"), "{error}");
        assert!(sink.0.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn read_media_file_without_sink_is_refused() {
        let root = temp_root("media-nosink");
        image::DynamicImage::ImageRgb8(image::RgbImage::new(4, 4))
            .save(root.join("a.png"))
            .unwrap();
        let error = call(&root, "read_media_file", &json!({ "path": "a.png" })).unwrap_err();
        assert!(error.contains("不支持加载图片"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn extract_pdf_images_reports_none_for_text_pdf() {
        let root = temp_root("pdf-noimg");
        std::fs::write(root.join("a.pdf"), b"not a real pdf").unwrap();
        let result = call(&root, "extract_pdf_images", &json!({ "path": "a.pdf" }));
        assert!(result.is_err(), "坏 PDF 应该报错");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn deletes_multiple_files() {
        let root = temp_root("delete-many");
        for name in ["a.txt", "b.txt"] {
            call(&root, "write_file", &json!({ "path": name, "content": "x" })).unwrap();
        }
        let out = call(&root, "delete_files", &json!({ "paths": ["a.txt", "b.txt"] })).unwrap();
        assert!(out.contains("已删除 2 个"), "{out}");
        assert!(!root.join("a.txt").exists());
        assert!(!root.join("b.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_reports_missing_files_without_aborting() {
        let root = temp_root("delete-missing");
        call(&root, "write_file", &json!({ "path": "a.txt", "content": "x" })).unwrap();
        let out = call(&root, "delete_files", &json!({ "paths": ["a.txt", "nope.txt"] })).unwrap();
        assert!(out.contains("已删除 1 个"), "{out}");
        assert!(out.contains("nope.txt"), "{out}");
        assert!(out.contains("文件不存在"), "{out}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_refuses_workspace_root() {
        let root = temp_root("delete-root");
        for path in [".", ""] {
            let out = call(&root, "delete_files", &json!({ "paths": [path] })).unwrap();
            assert!(out.contains("不能删除工作目录"), "{out}");
        }
        assert!(root.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_rejects_paths_outside_workspace() {
        let root = temp_root("delete-escape");
        let out = call(&root, "delete_files", &json!({ "paths": ["../secret.txt"] })).unwrap();
        assert!(out.contains("超出工作目录"), "{out}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_requires_non_empty_paths() {
        let root = temp_root("delete-empty");
        assert!(call(&root, "delete_files", &json!({ "paths": [] })).is_err());
        assert!(call(&root, "delete_files", &json!({})).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_removes_directory_with_contents_by_default() {
        let root = temp_root("delete-dir");
        std::fs::create_dir_all(root.join("sub/nested")).unwrap();
        std::fs::write(root.join("sub/a.txt"), b"x").unwrap();
        std::fs::write(root.join("sub/nested/b.txt"), b"y").unwrap();
        let out = call(&root, "delete_files", &json!({ "paths": ["sub"] })).unwrap();
        assert!(out.contains("已删除"), "{out}");
        assert!(out.contains("2 个文件"), "应报告删除的文件数：{out}");
        assert!(!root.join("sub").exists(), "目录应被完整删除");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_can_be_limited_to_empty_directories() {
        let root = temp_root("delete-dir-strict");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/keep.txt"), b"x").unwrap();
        let out = call(
            &root,
            "delete_files",
            &json!({ "paths": ["sub"], "recursive": false }),
        )
        .unwrap();
        assert!(out.contains("未删除"), "{out}");
        assert!(root.join("sub").exists(), "recursive=false 时不应删非空目录");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_removes_empty_directory() {
        let root = temp_root("delete-empty-dir");
        std::fs::create_dir_all(root.join("empty")).unwrap();
        let out = call(&root, "delete_files", &json!({ "paths": ["empty"] })).unwrap();
        assert!(out.contains("已删除"), "{out}");
        assert!(!root.join("empty").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
