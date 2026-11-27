//! 附件落盘到工作目录的 `uploads/`，消息里只留路径。

use std::path::{Path, PathBuf};

pub const UPLOAD_DIR: &str = "uploads";

pub const MAX_UPLOAD_BYTES: usize = 32 * 1024 * 1024;

fn sanitize(name: &str) -> String {
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .trim();
    let cleaned: String = base
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' | '\u{0}' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect();
    let trimmed = cleaned.trim_matches(['.', ' ']).to_string();
    if trimmed.is_empty() {
        "attachment".to_string()
    } else {
        trimmed
    }
}

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_string(), format!(".{ext}")),
        _ => (name.to_string(), String::new()),
    };
    for index in 2..1000 {
        let next = dir.join(format!("{stem}({index}){ext}"));
        if !next.exists() {
            return next;
        }
    }
    dir.join(format!("{stem}-{}{ext}", uuid::Uuid::new_v4()))
}

pub fn save(root: &Path, name: &str, bytes: &[u8]) -> Result<String, String> {
    if bytes.is_empty() {
        return Err(format!("{name} 是空文件"));
    }
    if bytes.len() > MAX_UPLOAD_BYTES {
        return Err(format!(
            "{name} 有 {:.1} MB，超过 {} MB 上限",
            bytes.len() as f64 / 1024.0 / 1024.0,
            MAX_UPLOAD_BYTES / 1024 / 1024
        ));
    }
    let dir = root.join(UPLOAD_DIR);
    std::fs::create_dir_all(&dir).map_err(|error| format!("无法创建附件目录：{error}"))?;
    let target = unique_path(&dir, &sanitize(name));
    std::fs::write(&target, bytes).map_err(|error| format!("写入附件失败：{error}"))?;
    let relative = target
        .strip_prefix(root)
        .map(|path| path.display().to_string().replace('\\', "/"))
        .unwrap_or_else(|_| target.display().to_string());
    Ok(relative)
}

pub fn prune(root: &Path, keep: usize) {
    let dir = root.join(UPLOAD_DIR);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect();
    if files.len() <= keep {
        return;
    }
    files.sort_by_key(|(time, _)| *time);
    let excess = files.len() - keep;
    for (_, path) in files.into_iter().take(excess) {
        let _ = std::fs::remove_file(path);
    }
}

/// 上传文件夹里的一项。`path` 是相对文件夹根的路径。
#[derive(serde::Deserialize)]
pub struct FolderFile {
    pub path: String,
    pub data: String,
}

/// 保存结果：目录路径 + 给模型看的目录树。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderSaved {
    pub path: String,
    pub tree: String,
    pub file_count: usize,
    /// 超出上限被丢弃的文件总数，没有则为 None
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
}

/// 目录树最多列这么多行，避免超大项目把上下文占满。
const MAX_TREE_LINES: usize = 400;
/// 单个文件夹最多导入这么多文件。
const MAX_FOLDER_FILES: usize = 500;

/// 把文件夹按相对路径还原到 `uploads/<name>/`，并生成目录树文本。
pub fn save_folder(
    root: &Path,
    name: &str,
    files: &[(String, Vec<u8>)],
) -> Result<FolderSaved, String> {
    let dir = root.join(UPLOAD_DIR);
    std::fs::create_dir_all(&dir).map_err(|error| format!("无法创建目录：{error}"))?;
    let folder_name = sanitize(name);
    let base = unique_path(&dir, &folder_name);
    std::fs::create_dir_all(&base).map_err(|error| format!("无法创建目录：{error}"))?;

    let mut written = 0usize;
    for (relative, bytes) in files {
        // 去掉开头的文件夹名（浏览器的 webkitRelativePath 带它）
        let cleaned = strip_root(relative, &folder_name);
        let Some(target) = safe_join(&base, &cleaned) else {
            continue;
        };
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::write(&target, bytes).is_ok() {
            written += 1;
        }
    }

    let tree = build_tree(&base, MAX_TREE_LINES);
    let relative_path = base
        .strip_prefix(root)
        .map(|path| path.display().to_string().replace('\\', "/"))
        .unwrap_or_else(|_| base.display().to_string());
    Ok(FolderSaved {
        path: relative_path,
        tree,
        file_count: written,
        total: None,
    })
}

/// 去掉路径开头的文件夹名，例如 "proj/src/a.js" → "src/a.js"。
fn strip_root(path: &str, folder: &str) -> String {
    let normalized = path.replace('\\', "/");
    let trimmed = normalized.trim_start_matches('/');
    trimmed
        .strip_prefix(&format!("{folder}/"))
        .unwrap_or(trimmed)
        .to_string()
}

/// 把相对路径接到 `base` 下，拒绝 `..` 之类的越界。
fn safe_join(base: &Path, relative: &str) -> Option<std::path::PathBuf> {
    let mut target = base.to_path_buf();
    for part in relative.split('/') {
        match part {
            "" | "." => continue,
            ".." => return None,
            name => {
                if name.contains(':') || name.contains('\\') {
                    return None;
                }
                target.push(name);
            }
        }
    }
    (target != base).then_some(target)
}

/// 递归生成目录树，超过行数上限就停止。
fn build_tree(base: &Path, limit: usize) -> String {
    fn walk(dir: &Path, prefix: &str, lines: &mut Vec<String>, limit: usize) {
        if lines.len() >= limit {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut items: Vec<_> = entries.flatten().collect();
        items.sort_by_key(|entry| {
            let is_dir = entry.path().is_dir();
            (is_dir as u8, entry.file_name().to_string_lossy().to_lowercase())
        });
        // 文件排前面，目录排后面
        items.sort_by_key(|entry| entry.path().is_dir() as u8);

        for entry in items {
            if lines.len() >= limit {
                lines.push("…（内容过多，已截断）".into());
                return;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                lines.push(format!("{prefix}{name}/"));
                walk(&path, &format!("{prefix}  "), lines, limit);
            } else {
                let size = entry
                    .metadata()
                    .map(|meta| format_size(meta.len()))
                    .unwrap_or_default();
                lines.push(format!("{prefix}{name}  ({size})"));
            }
        }
    }

    let mut lines = Vec::new();
    walk(base, "", &mut lines, limit);
    lines.join("
")
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

/// 直接从一个本地目录导入：后端遍历文件复制到 uploads/ 下。
/// 走原生目录选择器时用这条路径，避开 WebView 的文件上传确认弹窗。
pub fn import_folder(root: &Path, source: &Path) -> Result<FolderSaved, String> {
    if !source.is_dir() {
        return Err(format!("不是一个目录：{}", source.display()));
    }
    let name = source
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("folder");

    let dir = root.join(UPLOAD_DIR);
    std::fs::create_dir_all(&dir).map_err(|error| format!("无法创建目录：{error}"))?;
    let base = unique_path(&dir, &sanitize(name));
    std::fs::create_dir_all(&base).map_err(|error| format!("无法创建目录：{error}"))?;

    let mut collected = Vec::new();
    collect_files(source, source, &mut collected, 0);
    let mut written = 0usize;
    let mut skipped = 0usize;
    for file in &collected {
        if written >= MAX_FOLDER_FILES {
            skipped += 1;
            continue;
        }
        let relative = file
            .strip_prefix(source)
            .map(|path| path.display().to_string().replace('\\', "/"))
            .unwrap_or_default();
        let Some(target) = safe_join(&base, &relative) else {
            continue;
        };
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::copy(file, &target).is_ok() {
            written += 1;
        }
    }

    Ok(FolderSaved {
        path: relative_path(root, &base),
        tree: build_tree(&base, MAX_TREE_LINES),
        file_count: written,
        total: if skipped > 0 { Some(written + skipped) } else { None },
    })
}

/// 递归收集文件，跳过巨大的目录和明显的二进制缓存目录。
fn collect_files(root: &Path, dir: &Path, out: &mut Vec<std::path::PathBuf>, depth: usize) {
    const SKIP: [&str; 8] = [
        ".git",
        "node_modules",
        "target",
        "dist",
        "build",
        ".venv",
        "__pycache__",
        ".idea",
    ];
    if depth > 12 || out.len() >= MAX_FOLDER_FILES * 2 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| SKIP.contains(&name))
            .unwrap_or(false)
        {
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

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|value| value.display().to_string().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("litechat-uploads-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn saves_into_upload_directory() {
        let root = temp_root("save");
        let path = save(&root, "a.txt", b"hello").unwrap();
        assert!(path.starts_with("uploads/"), "{path}");
        assert_eq!(std::fs::read(root.join("uploads/a.txt")).unwrap(), b"hello");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn duplicate_names_get_a_suffix() {
        let root = temp_root("dup");
        let first = save(&root, "a.txt", b"1").unwrap();
        let second = save(&root, "a.txt", b"2").unwrap();
        assert_ne!(first, second);
        assert!(second.contains("(2)"), "{second}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn paths_traversal_is_neutralized() {
        let root = temp_root("traversal");
        let path = save(&root, "../../evil.txt", b"x").unwrap();
        assert!(path.starts_with("uploads/"), "{path}");
        assert!(!path.contains(".."), "{path}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn windows_reserved_chars_are_replaced() {
        assert_eq!(sanitize("a<b>c:d.txt"), "a_b_c_d.txt");
    }

    #[test]
    fn empty_name_falls_back() {
        assert_eq!(sanitize("   "), "attachment");
    }

    #[test]
    fn oversized_files_are_rejected() {
        let root = temp_root("big");
        let big = vec![0u8; MAX_UPLOAD_BYTES + 1];
        let error = save(&root, "big.bin", &big).unwrap_err();
        assert!(error.contains("上限"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_files_are_rejected() {
        let root = temp_root("empty");
        assert!(save(&root, "a.txt", b"").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn folder_is_saved_with_structure() {
        let root = temp_root("folder");
        let files = vec![
            ("proj/src/main.rs".to_string(), b"fn main(){}".to_vec()),
            ("proj/README.md".to_string(), b"# hi".to_vec()),
        ];
        let saved = save_folder(&root, "proj", &files).unwrap();
        assert_eq!(saved.file_count, 2);
        assert!(saved.path.starts_with("uploads/proj"), "{}", saved.path);
        // 相对路径里的文件夹名前缀应被剥掉
        assert!(root.join("uploads/proj/src/main.rs").is_file());
        assert!(root.join("uploads/proj/README.md").is_file());
        assert!(saved.tree.contains("src/"), "{}", saved.tree);
        assert!(saved.tree.contains("main.rs"), "{}", saved.tree);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn folder_tree_lists_dirs_before_files() {
        let root = temp_root("folder-tree");
        let files = vec![("p/b.txt".to_string(), b"x".to_vec())];
        save_folder(&root, "p", &files).unwrap();
        let saved = save_folder(
            &root,
            "q",
            &[
                ("q/a.txt".to_string(), b"1".to_vec()),
                ("q/sub/c.txt".to_string(), b"2".to_vec()),
            ],
        )
        .unwrap();
        let sub_pos = saved.tree.find("sub/").unwrap();
        let c_pos = saved.tree.find("c.txt").unwrap();
        assert!(sub_pos < c_pos, "子目录内容应在其目录行之后：{}", saved.tree);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn folder_rejects_path_traversal() {
        let root = temp_root("folder-escape");
        let files = vec![("p/../../../evil.txt".to_string(), b"x".to_vec())];
        let saved = save_folder(&root, "p", &files).unwrap();
        assert_eq!(saved.file_count, 0, "越界路径应被丢弃");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn duplicate_folder_names_get_a_suffix() {
        let root = temp_root("folder-dup");
        let files = vec![("p/a.txt".to_string(), b"1".to_vec())];
        let first = save_folder(&root, "p", &files).unwrap();
        let second = save_folder(&root, "p", &files).unwrap();
        assert_ne!(first.path, second.path);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn import_folder_copies_local_directory() {
        let root = temp_root("import-root");
        let source = temp_root("import-source");
        std::fs::create_dir_all(source.join("sub")).unwrap();
        std::fs::write(source.join("a.txt"), b"hello").unwrap();
        std::fs::write(source.join("sub/b.rs"), b"fn main(){}").unwrap();

        let saved = import_folder(&root, &source).unwrap();
        assert_eq!(saved.file_count, 2);
        assert!(root.join("uploads").join(&saved.path).exists() || saved.path.starts_with("uploads/"));
        assert!(saved.tree.contains("a.txt"), "{}", saved.tree);
        assert!(saved.tree.contains("b.rs"), "{}", saved.tree);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&source);
    }

    #[test]
    fn import_folder_skips_noise_directories() {
        let root = temp_root("import-noise");
        let source = temp_root("import-noise-src");
        std::fs::create_dir_all(source.join("node_modules/pkg")).unwrap();
        std::fs::write(source.join("node_modules/pkg/x.js"), b"x").unwrap();
        std::fs::write(source.join("keep.js"), b"k").unwrap();

        let saved = import_folder(&root, &source).unwrap();
        assert_eq!(saved.file_count, 1, "{}", saved.tree);
        assert!(!saved.tree.contains("node_modules"), "{}", saved.tree);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&source);
    }

    #[test]
    fn import_folder_rejects_file_path() {
        let root = temp_root("import-file");
        let file = root.join("a.txt");
        std::fs::write(&file, b"x").unwrap();
        assert!(import_folder(&root, &file).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn pruning_keeps_newest() {
        let root = temp_root("prune");
        for index in 0..5 {
            save(&root, &format!("f{index}.txt"), b"x").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        prune(&root, 2);
        let count = std::fs::read_dir(root.join(UPLOAD_DIR)).unwrap().count();
        assert_eq!(count, 2);
        let _ = std::fs::remove_dir_all(&root);
    }
}
