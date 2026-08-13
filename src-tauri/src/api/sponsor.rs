// api/sponsor.rs — 赞助者路由
// 兼容原项目 server/api/routes/sponsor.js
// 路由清单：
//   POST /api/sponsor/verify  校验赞助订单号是否在真实白名单中
//   GET  /api/sponsor/info    返回赞助白名单数量（供界面展示）
//
// 白名单数据 sponsor-orders.json：
//   - 随应用打包，通过 include_str! 内嵌到二进制（保证便携版自带）
//   - 首次访问时写入数据目录（exe 同目录/data/sponsor-orders.json），
//     之后优先读数据目录，便于后续不重编译也能更新白名单

use std::collections::HashSet;
use std::sync::OnceLock;

use serde_json::{json, Value};

use super::ApiResult;
use crate::storage;

/// 内嵌白名单原始 JSON（随版本更新）
const EMBEDDED_ORDERS: &str = include_str!("../../data/sponsor-orders.json");

/// 数据目录下的白名单文件
fn data_dir_orders_path() -> std::path::PathBuf {
    storage::resolve_data_dir().join("sponsor-orders.json")
}

/// 确保数据目录存在白名单文件，缺失时写入内嵌副本
fn ensure_orders_file() {
    let path = data_dir_orders_path();
    if path.exists() {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, EMBEDDED_ORDERS);
}

/// 解析订单号集合（优先读数据目录，回退到内嵌内容）
fn parse_orders(raw: &str) -> Option<HashSet<String>> {
    let data: Value = serde_json::from_str(raw).ok()?;
    let list = data.get("orders")?.as_array()?;
    let mut set = HashSet::new();
    for v in list {
        if let Some(s) = v.as_str() {
            set.insert(s.to_string());
        }
    }
    Some(set)
}

static ORDERS_CACHE: OnceLock<Option<HashSet<String>>> = OnceLock::new();

/// 读取并缓存赞助订单号白名单
fn load_sponsor_orders() -> Option<&'static HashSet<String>> {
    ORDERS_CACHE
        .get_or_init(|| {
            ensure_orders_file();
            // 优先读数据目录（可更新），失败则回退内嵌内容
            let raw = std::fs::read_to_string(data_dir_orders_path())
                .ok()
                .unwrap_or_else(|| EMBEDDED_ORDERS.to_string());
            parse_orders(&raw)
        })
        .as_ref()
}

pub fn handle(method: &str, path: &str, _params: &Option<Value>, body: &Option<Value>) -> Option<ApiResult> {
    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        // POST /api/sponsor/verify - 校验赞助订单号
        "POST /api/sponsor/verify" => {
            let order_id = body
                .as_ref()
                .and_then(|b| b.get("orderId"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if order_id.is_empty() {
                return Some(ApiResult::ok(json!({ "ok": false, "msg": "请输入赞助订单号" })));
            }
            match load_sponsor_orders() {
                Some(orders) if orders.contains(&order_id) => {
                    Some(ApiResult::ok(json!({ "ok": true })))
                }
                _ => Some(ApiResult::ok(json!({ "ok": false, "msg": "订单号无效，请核对后重试" }))),
            }
        }

        // GET /api/sponsor/info - 返回白名单数量
        "GET /api/sponsor/info" => {
            let count = load_sponsor_orders().map(|s| s.len()).unwrap_or(0);
            Some(ApiResult::ok(json!({ "enabled": true, "count": count })))
        }

        _ => None,
    }
}