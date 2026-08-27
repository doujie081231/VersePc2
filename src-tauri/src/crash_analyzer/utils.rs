// crash_analyzer/utils.rs — 正则工具与崩溃原因追加工具
// 注意：Rust 的 regex crate 不支持 lookbehind，原 lookbehind 模式需改写为捕获组

use regex::Regex;
use std::collections::HashMap;

use super::constants::CrashReason;

/// 正则匹配并返回第一个匹配项的完整文本
pub fn regex_seek(text: &str, pattern: &str) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    match Regex::new(pattern) {
        Ok(re) => match re.find(text) {
            Some(m) => Some(m.as_str().to_string()),
            None => None,
        },
        Err(_) => None,
    }
}

/// 正则匹配并返回第一个捕获组的内容
/// 用于替代 lookbehind 的场景：`(?<=prefix)content` → 用 `(prefix)(content)` 捕获组
pub fn regex_seek_group(text: &str, pattern: &str, group: usize) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    match Regex::new(pattern) {
        Ok(re) => match re.captures(text) {
            Some(caps) => caps.get(group).map(|m| m.as_str().to_string()),
            None => None,
        },
        Err(_) => None,
    }
}

/// 正则匹配并返回所有匹配项的完整文本（多行匹配）
pub fn regex_seek_all(pattern: &str, text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    match Regex::new(pattern) {
        Ok(re) => re
            .find_iter(text)
            .map(|m| m.as_str().to_string())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// 正则匹配并返回所有匹配项中指定捕获组的内容（多行匹配）
pub fn regex_seek_all_group(pattern: &str, text: &str, group: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    match Regex::new(pattern) {
        Ok(re) => re
            .captures_iter(text)
            .filter_map(|c| c.get(group).map(|m| m.as_str().to_string()))
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// 取文本中首个标记之后的部分
pub fn after_last<'a>(text: &'a str, marker: &str) -> &'a str {
    match text.rfind(marker) {
        Some(idx) => &text[idx + marker.len()..],
        None => text,
    }
}

/// 取文本中 start 与 end 两个标记之间的部分
pub fn between<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
    match text.find(start) {
        Some(si) => {
            let offset = si + start.len();
            match text[offset..].find(end) {
                Some(ei) => &text[offset..offset + ei],
                None => &text[offset..],
            }
        }
        None => "",
    }
}

/// 取文本在首个标记之前的部分
pub fn before_first<'a>(text: &'a str, marker: &str) -> &'a str {
    match text.find(marker) {
        Some(idx) => &text[..idx],
        None => text,
    }
}

/// 正则检测文本是否匹配
pub fn regex_check(text: &str, pattern: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    match Regex::new(pattern) {
        Ok(re) => re.is_match(text),
        Err(_) => false,
    }
}

/// 追加崩溃原因，additional 去重后合并
/// reasons: 当前已有的原因 Map，key 为 CrashReason，value 为附加信息列表
pub fn append_reason(
    reasons: &mut HashMap<CrashReason, Vec<String>>,
    reason: CrashReason,
    additional: Option<Vec<String>>,
) {
    match reasons.get_mut(&reason) {
        Some(items) => {
            if let Some(new_items) = additional {
                for item in new_items {
                    if !item.is_empty() && !items.contains(&item) {
                        items.push(item);
                    }
                }
            }
        }
        None => {
            let items = additional
                .map(|mut v| {
                    v.retain(|s| !s.is_empty());
                    v
                })
                .unwrap_or_default();
            reasons.insert(reason, items);
        }
    }
}

/// 从单个附加信息构建 Vec
pub fn single_additional(s: Option<String>) -> Option<Vec<String>> {
    s.map(|v| vec![v])
}
