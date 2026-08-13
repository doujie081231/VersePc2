// api/favorites.rs — 收藏夹相关路由
// 兼容原项目 server/api/routes/favorites.js
// 路由清单：
//   GET  /api/favorites           获取所有收藏夹
//   GET  /api/favorites/export    导出收藏夹
//   GET  /api/favorites/check     检查模组在收藏夹中的状态
//   POST /api/favorites/create    创建收藏夹
//   POST /api/favorites/rename    重命名收藏夹
//   POST /api/favorites/delete    删除收藏夹
//   POST /api/favorites/add       添加模组到收藏夹
//   POST /api/favorites/remove    从收藏夹移除模组
//   POST /api/favorites/note       设置/清除模组备注
//   POST /api/favorites/import    导入收藏数据

use serde_json::{json, Value};

use super::ApiResult;
use crate::storage;
use crate::utils;

pub fn handle(method: &str, path: &str, params: &Option<Value>, body: &Option<Value>) -> Option<ApiResult> {
    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        // ===== GET 路由 =====
        "GET /api/favorites" => Some(ApiResult::ok(storage::load_favorites())),

        "GET /api/favorites/export" => {
            let favorites = storage::load_favorites();
            let export_id = params
                .as_ref()
                .and_then(|p| p.get("id"))
                .and_then(|v| v.as_str());

            if let Some(id) = export_id {
                let fav = favorites
                    .as_array()
                    .and_then(|arr| arr.iter().find(|f| utils::get_str(f, "id") == id));

                if let Some(fav) = fav {
                    let favs = fav.get("favs").cloned().unwrap_or(json!([]));
                    Some(ApiResult::ok(json!({ "success": true, "data": favs })))
                } else {
                    Some(ApiResult::err(404, "收藏夹不存在"))
                }
            } else {
                Some(ApiResult::ok(json!({ "success": true, "data": favorites })))
            }
        }

        "GET /api/favorites/check" => {
            let project_id = params
                .as_ref()
                .and_then(|p| p.get("projectId"))
                .and_then(|v| v.as_str());

            let pid = match project_id {
                Some(p) => p,
                None => return Some(ApiResult::err(400, "Missing projectId")),
            };

            let favorites = storage::load_favorites();
            let mut result = serde_json::Map::new();
            if let Some(arr) = favorites.as_array() {
                for f in arr {
                    let id = utils::get_str(f, "id");
                    if !id.is_empty() {
                        let in_fav = f
                            .get("favs")
                            .and_then(|v| v.as_array())
                            .map(|favs| favs.iter().any(|v| v.as_str() == Some(pid)))
                            .unwrap_or(false);
                        result.insert(id, json!(in_fav));
                    }
                }
            }
            Some(ApiResult::ok(json!({ "success": true, "result": result })))
        }

        // ===== POST 路由 =====
        "POST /api/favorites/create" => {
            let data = body.clone().unwrap_or(Value::Null);
            let name = utils::get_str(&data, "name");
            if name.is_empty() {
                return Some(ApiResult::err(400, "Missing name"));
            }

            let new_fav = json!({
                "name": name,
                "id": utils::generate_simple_uuid(),
                "favs": [],
                "notes": {}
            });

            let mut favorites = storage::load_favorites();
            if let Some(arr) = favorites.as_array_mut() {
                arr.push(new_fav.clone());
            }
            storage::save_favorites(&favorites);
            Some(ApiResult::ok(json!({ "success": true, "favorite": new_fav })))
        }

        "POST /api/favorites/rename" => {
            let data = body.clone().unwrap_or(Value::Null);
            let id = utils::get_str(&data, "id");
            let name = utils::get_str(&data, "name");
            if id.is_empty() || name.is_empty() {
                return Some(ApiResult::err(400, "Missing id or name"));
            }

            let mut favorites = storage::load_favorites();
            let mut found = false;
            if let Some(arr) = favorites.as_array_mut() {
                for f in arr.iter_mut() {
                    if utils::get_str(f, "id") == id {
                        if let Some(obj) = f.as_object_mut() {
                            obj.insert("name".to_string(), json!(name));
                        }
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                return Some(ApiResult::err(404, "收藏夹不存在"));
            }
            storage::save_favorites(&favorites);
            Some(ApiResult::ok(json!({ "success": true })))
        }

        "POST /api/favorites/delete" => {
            let data = body.clone().unwrap_or(Value::Null);
            let id = utils::get_str(&data, "id");
            if id.is_empty() {
                return Some(ApiResult::err(400, "Missing id"));
            }

            let mut favorites = storage::load_favorites();
            let arr_len = favorites.as_array().map(|a| a.len()).unwrap_or(0);
            if arr_len <= 1 {
                return Some(ApiResult::err(400, "至少保留一个收藏夹"));
            }

            let found_idx = favorites
                .as_array()
                .and_then(|arr| arr.iter().position(|f| utils::get_str(f, "id") == id));

            let idx = match found_idx {
                Some(i) => i,
                None => return Some(ApiResult::err(404, "收藏夹不存在")),
            };

            if let Some(arr) = favorites.as_array_mut() {
                arr.remove(idx);
            }
            storage::save_favorites(&favorites);
            Some(ApiResult::ok(json!({ "success": true })))
        }

        "POST /api/favorites/add" => {
            let data = body.clone().unwrap_or(Value::Null);
            let fav_id = utils::get_str(&data, "favId");
            let project_id = utils::get_str(&data, "projectId");
            if fav_id.is_empty() || project_id.is_empty() {
                return Some(ApiResult::err(400, "Missing favId or projectId"));
            }

            let mut favorites = storage::load_favorites();
            let mut found = false;
            if let Some(arr) = favorites.as_array_mut() {
                for f in arr.iter_mut() {
                    if utils::get_str(f, "id") == fav_id {
                        let already = f
                            .get("favs")
                            .and_then(|v| v.as_array())
                            .map(|favs| favs.iter().any(|v| v.as_str() == Some(&project_id)))
                            .unwrap_or(false);
                        if !already {
                            if let Some(favs) = f.get_mut("favs").and_then(|v| v.as_array_mut()) {
                                favs.push(json!(project_id));
                            }
                        }
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                return Some(ApiResult::err(404, "收藏夹不存在"));
            }
            storage::save_favorites(&favorites);
            Some(ApiResult::ok(json!({ "success": true })))
        }

        "POST /api/favorites/remove" => {
            let data = body.clone().unwrap_or(Value::Null);
            let fav_id = utils::get_str(&data, "favId");
            let project_id = utils::get_str(&data, "projectId");
            if fav_id.is_empty() || project_id.is_empty() {
                return Some(ApiResult::err(400, "Missing favId or projectId"));
            }

            let mut favorites = storage::load_favorites();
            let mut found = false;
            if let Some(arr) = favorites.as_array_mut() {
                for f in arr.iter_mut() {
                    if utils::get_str(f, "id") == fav_id {
                        if let Some(favs) = f.get_mut("favs").and_then(|v| v.as_array_mut()) {
                            favs.retain(|v| v.as_str() != Some(&project_id));
                        }
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                return Some(ApiResult::err(404, "收藏夹不存在"));
            }
            storage::save_favorites(&favorites);
            Some(ApiResult::ok(json!({ "success": true })))
        }

        "POST /api/favorites/note" => {
            let data = body.clone().unwrap_or(Value::Null);
            let fav_id = utils::get_str(&data, "favId");
            let project_id = utils::get_str(&data, "projectId");
            let note = utils::get_str(&data, "note");
            if fav_id.is_empty() || project_id.is_empty() {
                return Some(ApiResult::err(400, "Missing favId or projectId"));
            }

            let mut favorites = storage::load_favorites();
            let mut found = false;
            if let Some(arr) = favorites.as_array_mut() {
                for f in arr.iter_mut() {
                    if utils::get_str(f, "id") == fav_id {
                        if f.get("notes").is_none() {
                            if let Some(obj) = f.as_object_mut() {
                                obj.insert("notes".to_string(), json!({}));
                            }
                        }
                        if let Some(notes) = f.get_mut("notes").and_then(|v| v.as_object_mut()) {
                            if !note.is_empty() {
                                notes.insert(project_id.clone(), json!(note));
                            } else {
                                notes.remove(&project_id);
                            }
                        }
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                return Some(ApiResult::err(404, "收藏夹不存在"));
            }
            storage::save_favorites(&favorites);
            Some(ApiResult::ok(json!({ "success": true })))
        }

        "POST /api/favorites/import" => {
            let data = body.clone().unwrap_or(Value::Null);
            let target_fav_id = utils::get_str(&data, "targetFavId");
            let raw_data = data.get("data").cloned().unwrap_or(Value::Null);

            let mut ids: Vec<String> = Vec::new();
            if let Some(arr) = raw_data.as_array() {
                for v in arr {
                    if let Some(s) = v.as_str() {
                        ids.push(s.to_string());
                    }
                }
            } else if let Some(s) = raw_data.as_str() {
                if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                    if let Some(arr) = parsed.as_array() {
                        for v in arr {
                            if let Some(s) = v.as_str() {
                                ids.push(s.to_string());
                            }
                        }
                    } else if let Some(obj) = parsed.as_object() {
                        for (k, v) in obj {
                            if v.as_bool() == Some(true)
                                || v.as_i64().map(|n| n != 0).unwrap_or(false)
                            {
                                ids.push(k.clone());
                            }
                        }
                    }
                }
            }

            if ids.is_empty() {
                return Some(ApiResult::err(400, "无有效数据"));
            }

            let mut favorites = storage::load_favorites();
            let target_idx = if let Some(arr) = favorites.as_array() {
                if !target_fav_id.is_empty() {
                    arr.iter()
                        .position(|f| utils::get_str(f, "id") == target_fav_id)
                } else if arr.is_empty() {
                    None
                } else {
                    Some(0)
                }
            } else {
                None
            };

            let idx = match target_idx {
                Some(i) => i,
                None => return Some(ApiResult::err(400, "无目标收藏夹")),
            };

            if let Some(arr) = favorites.as_array_mut() {
                if let Some(fav) = arr.get_mut(idx) {
                    if let Some(favs) = fav.get_mut("favs").and_then(|v| v.as_array_mut()) {
                        for id in &ids {
                            let already = favs.iter().any(|v| v.as_str() == Some(id));
                            if !already {
                                favs.push(json!(id));
                            }
                        }
                    }
                }
            }
            storage::save_favorites(&favorites);
            Some(ApiResult::ok(json!({ "success": true, "imported": ids.len() })))
        }

        _ => None,
    }
}
