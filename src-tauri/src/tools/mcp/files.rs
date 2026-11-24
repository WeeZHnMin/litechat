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
        "list_dir",
        "search",
        "workspace_info",
        "read_binary_info",
    ]
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

pub fn call(root: &Path, tool: &str, args: &Value) -> Result<String, String> {
    match tool {
        "read_file" => read_file(root, args),
        "write_file" => write_file(root, args),
        "edit_file" => edit_file(root, args),
        "list_dir" => list_dir(root, args),
        "search" => search(root, args),
        "workspace_info" => Ok(format!("当前工作目录：{}", root.display())),
        "read_binary_info" => read_binary_info(root, args),
        other => Err(format!("未知文件工具: {other}")),
    }
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
}
