// saves.rs — 存档管理
// 职责：
//   1. version_list_saves：列出版本下所有存档（解析 level.dat 取名称/难度/游戏规则/最近游玩）
//   2. version_save_update：修改存档显示名、难度与游戏规则（写回 level.dat）
use crate::nbt;
use crate::storage;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// 定位版本对应的 saves 目录（与版本文件夹打开逻辑一致）
fn saves_dir(version_id: &str, is_external: bool) -> Option<PathBuf> {
    let clean_id = if is_external {
        version_id
            .split(" [外部")
            .next()
            .unwrap_or(version_id)
            .to_string()
    } else {
        version_id.to_string()
    };
    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");
    let settings = storage::load_settings();
    let game_root = if is_external {
        // 外部文件夹版本：版本目录本身就是游戏根目录
        versions_dir.join(&clean_id)
    } else {
        crate::launch::args_builder::resolve_game_dir(
            &clean_id,
            None,
            None,
            &settings,
            &versions_dir,
            &data_dir,
        )
    };
    Some(game_root.join("saves"))
}

/// 从 level.dat 读取 Data 复合标签
fn read_data(level_dat: &std::path::Path) -> Option<(String, nbt::NbtValue)> {
    let (name, root) = nbt::read_level_dat(level_dat)?;
    Some((name, root))
}

/// 列出版本全部存档
#[tauri::command]
pub fn version_list_saves(version_id: String) -> Value {
    let is_external = version_id.contains(" [外部");
    let Some(sdir) = saves_dir(&version_id, is_external) else {
        return json!({ "success": false, "saves": [] });
    };

    let mut saves: Vec<Value> = Vec::new();
    let Ok(entries) = std::fs::read_dir(&sdir) else {
        return json!({ "success": true, "saves": saves });
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let folder = entry.file_name().to_string_lossy().to_string();
        let level_dat = path.join("level.dat");
        if !level_dat.is_file() {
            continue;
        }

        let mut display_name = folder.clone();
        let mut difficulty: i8 = 2;
        let mut last_played: i64 = 0;
        let mut rules: BTreeMap<String, String> = BTreeMap::new();

        if let Some((_, root)) = read_data(&level_dat) {
            if let Some(data) = nbt::get_compound(&root, "Data") {
                if let Some(s) = nbt::compound_string(data, "LevelName") {
                    if !s.is_empty() {
                        display_name = s;
                    }
                }
                if let Some(b) = nbt::compound_byte(data, "Difficulty") {
                    difficulty = b;
                }
                if let Some(l) = nbt::compound_long(data, "LastPlayed") {
                    last_played = l;
                }
                if let Some(gr) = data
                    .get("GameRules")
                    .and_then(|v| nbt::as_compound(v))
                {
                    for (k, v) in gr {
                        if let Some(s) = nbt::as_string(v) {
                            rules.insert(k.clone(), s.to_string());
                        }
                    }
                }
            }
        }

        let icon_path = path.join("icon.png");
        let icon = if icon_path.is_file() {
            Some(icon_path.to_string_lossy().to_string())
        } else {
            None
        };

        saves.push(json!({
            "folder": folder,
            "name": display_name,
            "difficulty": difficulty,
            "lastPlayed": last_played,
            "rules": rules,
            "icon": icon,
        }));
    }

    // 最近游玩的排前面
    saves.sort_by_key(|s| -s["lastPlayed"].as_i64().unwrap_or(0));
    json!({ "success": true, "saves": saves })
}

/// 修改存档信息：显示名 / 难度 / 游戏规则
#[tauri::command]
pub fn version_save_update(
    version_id: String,
    folder: String,
    display_name: Option<String>,
    difficulty: Option<i32>,
    rules: Option<Value>,
) -> Value {
    let is_external = version_id.contains(" [外部");
    let Some(sdir) = saves_dir(&version_id, is_external) else {
        return json!({ "success": false, "error": "无法定位存档目录" });
    };

    // 文件夹名校验，防止路径穿越
    if folder.is_empty()
        || folder == "."
        || folder == ".."
        || folder.contains('/')
        || folder.contains('\\')
    {
        return json!({ "success": false, "error": "非法的存档文件夹名" });
    }

    let level_dat = sdir.join(&folder).join("level.dat");
    let Some((root_name, mut root)) = read_data(&level_dat) else {
        return json!({ "success": false, "error": "无法读取 level.dat" });
    };

    let Some(data) = nbt::get_compound_mut(&mut root, "Data") else {
        return json!({ "success": false, "error": "level.dat 缺少 Data 标签" });
    };

    // 修改显示名
    if let Some(dn) = display_name {
        let trimmed = dn.trim().to_string();
        if !trimmed.is_empty() {
            data.insert("LevelName".to_string(), nbt::NbtValue::String(trimmed));
        }
    }

    // 修改难度（0 和平 / 1 简单 / 2 普通 / 3 困难）
    if let Some(d) = difficulty {
        if (0..=3).contains(&d) {
            data.insert("Difficulty".to_string(), nbt::NbtValue::Byte(d as i8));
        }
    }

    // 合并游戏规则（只覆盖传入的键）
    if let Some(r) = rules {
        if let Some(obj) = r.as_object() {
            let gr = data.entry("GameRules".to_string()).or_insert_with(|| {
                nbt::NbtValue::Compound(BTreeMap::new())
            });
            if let Some(grmap) = nbt::as_compound_mut(gr) {
                for (k, v) in obj {
                    if let Some(s) = v.as_str() {
                        if !k.is_empty() {
                            grmap.insert(k.clone(), nbt::NbtValue::String(s.to_string()));
                        }
                    }
                }
            }
        }
    }

    match nbt::write_level_dat(&level_dat, &root_name, &root) {
        Ok(_) => json!({ "success": true }),
        Err(e) => json!({ "success": false, "error": e }),
    }
}
