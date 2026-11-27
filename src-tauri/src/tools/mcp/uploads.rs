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
