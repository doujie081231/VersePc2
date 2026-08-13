// network/mod.rs — 网络工具模块入口
// 职责：UPnP 端口映射、公网 IP 检测、本地 IP 枚举
// 对应原项目 server/network/ 的 upnp + public-ip 部分

pub mod public_ip;
pub mod upnp;

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::{json, Value};

/// UPnP 端口映射记录
#[derive(Clone)]
pub struct UpnpMapping {
    pub external_port: u16,
    pub internal_port: u16,
    pub description: String,
    pub local_ip: String,
}

/// 全局 UPnP 映射表（external_port → mapping）
static UPNP_MAPPINGS: Mutex<Option<HashMap<u16, UpnpMapping>>> = Mutex::new(None);

fn init_mappings() {
    let mut g = UPNP_MAPPINGS.lock().unwrap();
    if g.is_none() {
        *g = Some(HashMap::new());
    }
}

/// 记录 UPnP 映射
pub fn record_mapping(mapping: UpnpMapping) {
    init_mappings();
    let mut g = UPNP_MAPPINGS.lock().unwrap();
    if let Some(map) = g.as_mut() {
        map.insert(mapping.external_port, mapping);
    }
}

/// 移除映射记录
pub fn remove_mapping(ext_port: u16) {
    let mut g = UPNP_MAPPINGS.lock().unwrap();
    if let Some(map) = g.as_mut() {
        map.remove(&ext_port);
    }
}

/// 列出所有映射（给 GET /api/lan/upnp-status 用）
pub fn list_mappings() -> Vec<Value> {
    let g = UPNP_MAPPINGS.lock().unwrap();
    if let Some(map) = g.as_ref() {
        map.iter()
            .map(|(ext, m)| {
                json!({
                    "externalPort": ext,
                    "internalPort": m.internal_port,
                    "description": m.description,
                    "localIP": m.local_ip,
                })
            })
            .collect()
    } else {
        Vec::new()
    }
}

/// 检测的 LAN 端口（Minecraft 自动开放局域网时探测到的端口）
static DETECTED_LAN_PORT: Mutex<Option<u16>> = Mutex::new(None);

pub fn set_detected_lan_port(port: u16) {
    let mut g = DETECTED_LAN_PORT.lock().unwrap();
    *g = Some(port);
}

pub fn get_detected_lan_port() -> Option<u16> {
    *DETECTED_LAN_PORT.lock().unwrap()
}
