// index.rs — 由 server_host.rs 拆分
#![allow(unused_imports)]

// 服务器目录与索引读写
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::oneshot;
use crate::download::download_with_mirror;
use crate::storage;
use super::*;

pub(crate) fn servers_root() -> PathBuf {
    storage::resolve_data_dir().join("servers")
}

pub(crate) fn index_file() -> PathBuf {
    servers_root().join("index.json")
}

pub(crate) fn versions_dir() -> PathBuf {
    storage::resolve_data_dir().join("versions")
}

/// 由服务器 id 拼出目录。
///
/// id 来自前端或索引文件，必须先拒绝路径分隔符、`..`、盘符等写法，
/// 否则 `..\..\Users\...` 会被拼成可穿越的路径（审计 P1-2）。
pub(crate) fn server_dir_of(id: &str) -> Option<PathBuf> {
    if id.is_empty()
        || id.contains("..")
        || id.contains('/')
        || id.contains('\\')
        || id.contains(':')
    {
        return None;
    }
    Some(servers_root().join(id))
}

/// 去掉 Windows canonicalize 产生的 `\\?\` 前缀，便于做前缀比较
fn strip_verbatim(path: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let s = path.to_string_lossy();
        if let Some(rest) = s.strip_prefix("\\\\?\\UNC\\") {
            return PathBuf::from(format!("\\\\{}", rest));
        }
        if let Some(rest) = s.strip_prefix("\\\\?\\") {
            return PathBuf::from(rest);
        }
    }
    path.to_path_buf()
}

fn has_parent_component(path: &Path) -> bool {
    path.components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
}

/// 目录前缀比较：Windows 大小写不敏感，且必须按路径分隔符对齐，
/// 避免 `servers` 误匹配 `servers-backup`。
fn path_starts_with_ci(path: &Path, prefix: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        let p = path.to_string_lossy().to_lowercase();
        let q = prefix.to_string_lossy().to_lowercase();
        if q.is_empty() || !p.starts_with(&q) {
            return false;
        }
        p.len() == q.len()
            || p.as_bytes()[q.len()] == b'\\'
            || p.as_bytes()[q.len()] == b'/'
    }
    #[cfg(not(target_os = "windows"))]
    {
        path.starts_with(prefix)
    }
}

/// 校验目录确实在 servers_root 之下。
/// 已存在的目录用 canonicalize（可识别符号链接逃逸）；不存在的目录做词法前缀检查。
pub(crate) fn validate_server_dir(dir: &Path) -> Option<PathBuf> {
    if has_parent_component(dir) {
        return None;
    }
    let root = strip_verbatim(&servers_root());
    let root_c = std::fs::canonicalize(&root).map(|p| strip_verbatim(&p)).ok();
    let dir_c = std::fs::canonicalize(dir).map(|p| strip_verbatim(&p)).ok();
    if let (Some(root_c), Some(dir_c)) = (root_c.as_ref(), dir_c.as_ref()) {
        return if path_starts_with_ci(dir_c, root_c) {
            Some(dir.to_path_buf())
        } else {
            None
        };
    }
    // 目录尚不存在：要求词法路径同样位于 servers_root 下
    let dir_n = strip_verbatim(dir);
    if path_starts_with_ci(&dir_n, &root) {
        Some(dir.to_path_buf())
    } else {
        None
    }
}

/// 从索引条目解析服务器目录：优先用条目里的 dir，缺失时按 id 拼；
/// 两种情况都必须通过 servers_root 边界校验。
pub(crate) fn server_dir_from_entry(entry: &Value, id: &str) -> Option<PathBuf> {
    let dir = entry
        .get("dir")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .or_else(|| server_dir_of(id))?;
    validate_server_dir(&dir)
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn ensure_root() {
    let _ = std::fs::create_dir_all(servers_root());
    let idx = index_file();
    if !idx.exists() {
        let _ = std::fs::write(&idx, "{\"servers\":[]}");
    }
}

pub(crate) fn load_index() -> Value {
    ensure_root();
    let idx = index_file();
    match std::fs::read_to_string(&idx) {
        Ok(content) => {
            let content = content.trim_start_matches('\u{FEFF}');
            match serde_json::from_str::<Value>(content) {
                Ok(v) if v.get("servers").and_then(|s| s.as_array()).is_some() => v,
                _ => json!({ "servers": [] }),
            }
        }
        Err(_) => json!({ "servers": [] }),
    }
}

pub(crate) fn save_index(idx: &Value) {
    ensure_root();
    let _ = std::fs::write(index_file(), serde_json::to_string_pretty(idx).unwrap_or_default());
}

pub(crate) fn read_json_safe(path: &std::path::Path) -> Option<Value> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim_start_matches('\u{FEFF}').to_string())
        .and_then(|s| serde_json::from_str(&s).ok())
}

#[cfg(test)]
mod path_guard_tests {
    use super::*;

    #[test]
    fn server_id_rejects_path_traversal() {
        for bad in ["", "..", "../x", "..\\x", "a/b", "a\\b", "C:\\Windows", "C:/Windows"] {
            assert!(server_dir_of(bad).is_none(), "应拒绝非法 id: {bad}");
        }
        assert!(server_dir_of("abc123").is_some());
    }

    #[test]
    fn server_dir_must_stay_under_servers_root() {
        // 系统临时目录显然不在 servers_root 下，必须被拒绝
        assert!(validate_server_dir(&std::env::temp_dir()).is_none());
        assert!(validate_server_dir(&servers_root().join("..").join("x")).is_none());
        assert!(validate_server_dir(&servers_root().join("abc123")).is_some());
    }
}
