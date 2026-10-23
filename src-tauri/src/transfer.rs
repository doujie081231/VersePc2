// transfer.rs — VersePC 互传：信令走信令服务(124.223.80.73:8746)，文件在两台设备间 TCP 直传。
// 协议：魔数VXFER01 + 文件名(u32长度) + 大小(u64) + sha256(32B) + 文件字节流(256KB分块)，接收端最后回 1 字节确认。
// 只做信令不做中转：文件数据不经过任何服务器。
// 免费使用（2026-10-14 起）：互传不再要求激活码，安全依赖信令限流与用户守则。
// 游戏数据互传：版本/模组/存档先打包为 zip（VXFER01 单文件协议不变），
// 信令注册时透传 meta（kind/itemName/versionId），接收端按类型解压落地到对应文件夹。
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::Emitter;

const SIGNAL_BASE: &str = "http://124.223.80.73:8746";
const MAGIC: &[u8; 8] = b"VXFER01\0";
const CHUNK_SIZE: usize = 256 * 1024;

// meta.kind：0=普通文件（任意文件，落地到用户选择目录）1=版本 2=模组 3=存档
const KIND_FILE: u8 = 0;
const KIND_VERSION: u8 = 1;
const KIND_MODS: u8 = 2;
const KIND_SAVE: u8 = 3;

#[derive(Serialize, Clone)]
pub struct SendSession {
    pub code: String,
    pub file_name: String,
    pub file_size: u64,
}

#[derive(Serialize, Clone, Default)]
pub struct SendStatus {
    pub listening: bool,
    pub connected: bool,
    pub sent_bytes: u64,
    pub total_bytes: u64,
    pub transfers: u64,
    pub error: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct TransferResult {
    pub file_name: String,
    pub saved_path: String,
    pub file_size: u64,
    pub hash_ok: bool,
}

struct Offer {
    path: PathBuf,
    file_name: String,
    file_size: u64,
    hash: String,
    status: Arc<Mutex<SendStatus>>,
    /// 临时打包文件（游戏数据 zip），传输成功后清理
    tmp: Option<PathBuf>,
}

fn offers() -> &'static Mutex<HashMap<String, Arc<Offer>>> {
    static OFFERS: OnceLock<Mutex<HashMap<String, Arc<Offer>>>> = OnceLock::new();
    OFFERS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 设备标识：Windows 读注册表 MachineGuid（重装系统才会变，稳定不丢），
/// 其他平台退回机器名。用于官网激活码的设备绑定。
fn device_id() -> String {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        use winreg::RegKey;
        if let Ok(hk) = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey(r"SOFTWARE\Microsoft\Cryptography")
        {
            if let Ok(v) = hk.get_value::<String, _>("MachineGuid") {
                let v = v.trim().to_string();
                if !v.is_empty() {
                    return format!("pc-{v}");
                }
            }
        }
    }
    let host = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".into());
    format!("pc-{host}")
}

async fn hash_file(path: &PathBuf) -> Result<(String, u64), String> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| format!("无法打开文件: {e}"))?;
    let mut hasher = Sha256::new();
    let mut size: u64 = 0;
    let mut buf = vec![0u8; CHUNK_SIZE];
    loop {
        let n = tokio::io::AsyncReadExt::read(&mut file, &mut buf)
            .await
            .map_err(|e| format!("读取文件失败: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    Ok((format!("{:x}", hasher.finalize()), size))
}

// ============== 本地游戏数据列举 ==============

fn versions_dir() -> PathBuf {
    crate::storage::resolve_data_dir().join("versions")
}

fn sanitize_dir_name(name: &str) -> String {
    // 防路径穿越：仅取末段并剔除非法字符
    let owned = name.replace('\\', "/");
    let last = owned.rsplit('/').next().unwrap_or("");
    last.replace([':', '*', '?', '"', '<', '>', '|'], "_")
}

/// 列出本地已有版本（versions 目录下的子目录名）
#[tauri::command]
pub async fn transfer_list_versions() -> Result<Vec<String>, String> {
    let dir = versions_dir();
    let mut out: Vec<String> = Vec::new();
    let mut rd = tokio::fs::read_dir(&dir)
        .await
        .map_err(|e| format!("无法读取版本目录: {e}"))?;
    while let Ok(Some(entry)) = rd.next_entry().await {
        let p = entry.path();
        if p.is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                out.push(name.to_string());
            }
        }
    }
    out.sort();
    Ok(out)
}

/// 列出某版本 mods 目录下的模组文件（文件名 + 大小）
#[tauri::command]
pub async fn transfer_list_mods(version_id: String) -> Result<Vec<serde_json::Value>, String> {
    let v = sanitize_dir_name(&version_id);
    let dir = versions_dir().join(&v).join("mods");
    if !dir.is_dir() {
        return Err("该版本没有 mods 目录".into());
    }
    let mut out: Vec<serde_json::Value> = Vec::new();
    let mut rd = tokio::fs::read_dir(&dir)
        .await
        .map_err(|e| format!("无法读取模组目录: {e}"))?;
    while let Ok(Some(entry)) = rd.next_entry().await {
        let p = entry.path();
        if p.is_file() {
            let meta = entry.metadata().await.ok();
            out.push(serde_json::json!({
                "name": entry.file_name().to_string_lossy(),
                "size": meta.as_ref().map(|m| m.len()).unwrap_or(0),
            }));
        }
    }
    out.sort_by(|a, b| {
        a.get("name").and_then(|x| x.as_str()).unwrap_or("")
            .cmp(b.get("name").and_then(|x| x.as_str()).unwrap_or(""))
    });
    Ok(out)
}

/// 列出某版本 saves 目录下的存档（子目录名）
#[tauri::command]
pub async fn transfer_list_saves(version_id: String) -> Result<Vec<String>, String> {
    let v = sanitize_dir_name(&version_id);
    let dir = versions_dir().join(&v).join("saves");
    if !dir.is_dir() {
        return Err("该版本没有存档目录".into());
    }
    let mut out: Vec<String> = Vec::new();
    let mut rd = tokio::fs::read_dir(&dir)
        .await
        .map_err(|e| format!("无法读取存档目录: {e}"))?;
    while let Ok(Some(entry)) = rd.next_entry().await {
        if entry.path().is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                out.push(name.to_string());
            }
        }
    }
    out.sort();
    Ok(out)
}

// ============== zip 打包 ==============

/// 递归收集目录下全部文件（返回相对路径，正斜杠分隔）
fn collect_files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn walk(root: &Path, cur: &Path, out: &mut Vec<String>) {
    if let Ok(rd) = std::fs::read_dir(cur) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk(root, &p, out);
            } else if p.is_file() {
                if let Ok(rel) = p.strip_prefix(root) {
                    let rel = rel.to_string_lossy().replace('\\', "/");
                    if !rel.is_empty() {
                        out.push(rel);
                    }
                }
            }
        }
    }
}

/// 把 base 目录下指定相对路径文件打包为 zip（zip 内保留相对路径结构），返回 (临时zip路径, 文件数)
fn pack_zip(base: &Path, rel_files: &[String], dest: &Path) -> Result<usize, String> {
    let file = std::fs::File::create(dest).map_err(|e| format!("无法创建临时包: {e}"))?;
    let mut zw = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);
    let mut count = 0usize;
    for rel in rel_files {
        let abs = base.join(rel);
        if !abs.is_file() {
            continue;
        }
        zw.start_file(rel.as_str(), opts.clone())
            .map_err(|e| format!("写入压缩包失败({rel}): {e}"))?;
        let mut f = std::fs::File::open(&abs).map_err(|e| format!("读取文件失败({rel}): {e}"))?;
        std::io::copy(&mut f, &mut zw).map_err(|e| format!("压缩写入失败({rel}): {e}"))?;
        count += 1;
    }
    zw.finish().map_err(|e| format!("完成压缩包失败: {e}"))?;
    Ok(count)
}

/// 互传临时目录
fn tmp_dir() -> PathBuf {
    let d = crate::storage::resolve_data_dir().join("transfer-tmp");
    let _ = std::fs::create_dir_all(&d);
    d
}

// ============== 发送 ==============

async fn register_session(
    file_name: String,
    size: u64,
    hash: String,
    port: u16,
    activation_key: &str,
    kind: u8,
    item_name: &str,
    version_id: &str,
) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{SIGNAL_BASE}/api/transfer/register"))
        .json(&serde_json::json!({
            "fileName": file_name,
            "fileSize": size,
            "fileHash": hash,
            "port": port,
            "activationKey": activation_key,
            "deviceId": device_id(),
            "kind": kind,
            "itemName": item_name,
            "versionId": version_id,
        }))
        .send()
        .await
        .map_err(|e| format!("信令服务器不可达: {e}"))?;
    let reg: serde_json::Value = resp.json().await.map_err(|e| format!("信令响应异常: {e}"))?;
    reg.get("code")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            format!(
                "生成识别码失败: {}",
                reg.get("error").and_then(|v| v.as_str()).unwrap_or("未知错误")
            )
        })
}

async fn bind_listener() -> Result<(tokio::net::TcpListener, u16), String> {
    let listener = tokio::net::TcpListener::bind("0.0.0.0:0")
        .await
        .map_err(|e| format!("无法监听端口: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("获取端口失败: {e}"))?
        .port();
    Ok((listener, port))
}

fn insert_offer(
    code: &str,
    path: PathBuf,
    file_name: String,
    size: u64,
    hash: String,
    tmp: Option<PathBuf>,
) -> (Arc<Offer>, Arc<Mutex<SendStatus>>) {
    let status = Arc::new(Mutex::new(SendStatus {
        listening: true,
        total_bytes: size,
        ..Default::default()
    }));
    let offer = Arc::new(Offer {
        path,
        file_name,
        file_size: size,
        hash,
        status: status.clone(),
        tmp,
    });
    offers().lock().unwrap().insert(code.to_string(), offer.clone());
    (offer, status)
}

/// 发送方：任意文件互传（保留兼容：拖选单个文件）
#[tauri::command]
pub async fn transfer_start_send(path: String, activation_key: String) -> Result<SendSession, String> {
    // 免费使用：激活码参数保留兼容旧前端，不再校验
    let key = activation_key.trim().to_string();
    let file_path = PathBuf::from(&path);
    if !file_path.is_file() {
        return Err("文件不存在".into());
    }
    let file_name = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());

    let (hash, size) = hash_file(&file_path).await?;
    let (listener, port) = bind_listener().await?;
    let code = register_session(file_name.clone(), size, hash.clone(), port, &key, KIND_FILE, "", "").await?;
    let resp_file_name = file_name.clone();
    let (offer, status) = insert_offer(&code, file_path, file_name, size, hash, None);
    tokio::spawn(serve_file(listener, offer, status));

    Ok(SendSession { code, file_name: resp_file_name, file_size: size })
}

/// 发送方：游戏数据互传（版本/模组/存档）。
/// kind: 1=版本(version_id 的整个版本目录) 2=模组(version_id 版本的 mods 下 selection 列出的文件)
///       3=存档(version_id 版本下 saves/item_name 存档目录)
/// 流程：收集文件 → 打 zip → 注册（携带 meta，信令发识别码）→ 监听等待对端直传
#[tauri::command]
pub async fn transfer_start_send_game(
    kind: u8,
    version_id: String,
    item_name: String,
    selection: Vec<String>,
    activation_key: String,
) -> Result<SendSession, String> {
    // 免费使用：激活码参数保留兼容旧前端，不再校验
    let key = activation_key.trim().to_string();
    let vid = sanitize_dir_name(&version_id);
    if vid.is_empty() {
        return Err("版本名无效".into());
    }

    // 1. 确定打包基目录与文件清单
    let (base, rel_files, item) = match kind {
        KIND_VERSION => {
            let base = versions_dir().join(&vid);
            if !base.is_dir() {
                return Err("版本目录不存在".into());
            }
            let files = collect_files(&base);
            (base, files, vid.clone())
        }
        KIND_MODS => {
            let base = versions_dir().join(&vid).join("mods");
            if !base.is_dir() {
                return Err("该版本没有 mods 目录".into());
            }
            let files: Vec<String> = selection
                .iter()
                .map(|s| sanitize_dir_name(s))
                .filter(|s| !s.is_empty())
                .collect();
            if files.is_empty() {
                return Err("请先选择要传输的模组".into());
            }
            (base, files, vid.clone())
        }
        KIND_SAVE => {
            let save_name = sanitize_dir_name(&item_name);
            if save_name.is_empty() {
                return Err("存档名无效".into());
            }
            let base = versions_dir().join(&vid).join("saves").join(&save_name);
            if !base.is_dir() {
                return Err("存档目录不存在".into());
            }
            let files = collect_files(&base);
            (base, files, save_name)
        }
        _ => return Err("未知的传输类型".into()),
    };
    if rel_files.is_empty() {
        return Err("没有可传输的文件".into());
    }

    // 2. 打包 zip（可能较大，放阻塞线程池）
    let zip_path = tmp_dir().join(format!("vxfer_{}_{}.zip", std::process::id(), chrono::Local::now().timestamp_millis()));
    let base2 = base.clone();
    let rel2 = rel_files.clone();
    let zip_path2 = zip_path.clone();
    let packed = tokio::task::spawn_blocking(move || pack_zip(&base2, &rel2, &zip_path2))
        .await
        .map_err(|e| format!("打包任务失败: {e}"))?;
    packed.map_err(|e| e)?;
    if !zip_path.is_file() {
        return Err("打包失败".into());
    }

    // 3. 哈希 + 注册 + 监听
    let (hash, size) = hash_file(&zip_path).await?;
    let (listener, port) = bind_listener().await?;
    let zip_name = format!("{item}.zip");
    let code = register_session(zip_name.clone(), size, hash.clone(), port, &key, kind, &item, &vid).await?;
    let (offer, status) = insert_offer(&code, zip_path.clone(), zip_name, size, hash, Some(zip_path));
    tokio::spawn(serve_file(listener, offer, status));

    Ok(SendSession { code, file_name: format!("{item}"), file_size: size })
}

/// 发送方后台任务：监听 TCP。成功完成一次传输后停止监听并清理临时包。
async fn serve_file(
    listener: tokio::net::TcpListener,
    offer: Arc<Offer>,
    status: Arc<Mutex<SendStatus>>,
) {
    loop {
        match listener.accept().await {
            Ok((stream, _peer)) => {
                let offer2 = offer.clone();
                let res = send_file_to(stream, offer2).await;
                if let Err(e) = res {
                    let mut st = offer.status.lock().unwrap();
                    st.error = Some(e);
                }
                let done = offer.status.lock().unwrap().transfers > 0;
                if done {
                    // 传输成功：停止监听并清理临时打包文件
                    if let Some(tmp) = &offer.tmp {
                        let _ = tokio::fs::remove_file(tmp).await;
                    }
                    break;
                }
            }
            Err(e) => {
                let mut st = status.lock().unwrap();
                st.listening = false;
                st.error = Some(format!("接受连接失败: {e}"));
                break;
            }
        }
    }
}

/// 发送方：查询会话状态（前端轮询展示进度）
#[tauri::command]
pub async fn transfer_send_status(code: String) -> Result<SendStatus, String> {
    let offer = offers()
        .lock()
        .unwrap()
        .get(&code)
        .cloned()
        .ok_or("会话不存在或已失效")?;
    let st = offer.status.lock().unwrap().clone();
    Ok(st)
}

async fn send_file_to(
    mut stream: tokio::net::TcpStream,
    offer: Arc<Offer>,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    {
        let mut st = offer.status.lock().unwrap();
        st.connected = true;
    }
    // 头部：魔数 + 文件名 + 大小 + 哈希
    let name_bytes = offer.file_name.as_bytes();
    let mut header = Vec::with_capacity(8 + 4 + name_bytes.len() + 8 + 32);
    header.extend_from_slice(MAGIC);
    header.extend_from_slice(&(name_bytes.len() as u32).to_be_bytes());
    header.extend_from_slice(name_bytes);
    header.extend_from_slice(&offer.file_size.to_be_bytes());
    header.extend_from_slice(&hex_to_bytes(&offer.hash)?);
    stream
        .write_all(&header)
        .await
        .map_err(|e| format!("发送头部失败: {e}"))?;

    // 文件字节流
    let mut file = tokio::fs::File::open(&offer.path)
        .await
        .map_err(|e| format!("打开文件失败: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK_SIZE];
    let mut sent: u64 = 0;
    loop {
        let n = tokio::io::AsyncReadExt::read(&mut file, &mut buf)
            .await
            .map_err(|e| format!("读取文件失败: {e}"))?;
        if n == 0 {
            break;
        }
        stream
            .write_all(&buf[..n])
            .await
            .map_err(|e| format!("发送数据失败: {e}"))?;
        hasher.update(&buf[..n]);
        sent += n as u64;
        offer.status.lock().unwrap().sent_bytes = sent;
    }
    stream.flush().await.ok();

    // 接收端确认（1 字节：0x01 校验通过）
    let mut ack = [0u8; 1];
    use tokio::io::AsyncReadExt;
    let read_res = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::io::AsyncReadExt::read_exact(&mut stream, &mut ack),
    )
    .await;
    let verified = matches!(read_res, Ok(Ok(_))) && ack[0] == 0x01;
    {
        let mut st = offer.status.lock().unwrap();
        st.connected = false;
        if verified {
            st.transfers += 1;
        } else {
            st.error = Some("对端校验未通过".into());
        }
    }
    Ok(())
}

// ============== 接收 ==============

async fn lookup_session(code: &str) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{SIGNAL_BASE}/api/transfer/lookup"))
        .query(&[("code", &code)])
        .send()
        .await
        .map_err(|e| format!("信令服务器不可达: {e}"))?;
    let lookup: serde_json::Value = resp.json().await.map_err(|e| format!("信令响应异常: {e}"))?;
    if lookup.get("ip").is_none() {
        let em = lookup
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("会话不存在")
            .to_string();
        return Err(em);
    }
    Ok(lookup)
}

async fn connect_sender(ip: &str, port: u16) -> Result<tokio::net::TcpStream, String> {
    let mut stream = None;
    for target in [format!("{ip}:{port}"), format!("127.0.0.1:{port}")] {
        if let Ok(s) = tokio::time::timeout(
            std::time::Duration::from_secs(8),
            tokio::net::TcpStream::connect(&target),
        )
        .await
        {
            if let Ok(sock) = s {
                stream = Some(sock);
                break;
            }
        }
    }
    stream.ok_or("无法连接到发送方（可能不在线或端口不可达）".into())
}

/// 接收方：查询识别码的传输内容信息（类型/名称/所属版本/文件名/大小），用于展示落地预览
#[tauri::command]
pub async fn transfer_lookup_meta(code: String) -> Result<serde_json::Value, String> {
    let code = code.trim().to_uppercase().replace(' ', "");
    if code.is_empty() {
        return Err("请输入识别码".into());
    }
    let lookup = lookup_session(&code).await?;
    Ok(serde_json::json!({
        "kind": lookup.get("kind").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
        "itemName": lookup.get("itemName").and_then(|v| v.as_str()).unwrap_or(""),
        "versionId": lookup.get("versionId").and_then(|v| v.as_str()).unwrap_or(""),
        "fileName": lookup.get("fileName").and_then(|v| v.as_str()).unwrap_or(""),
        "fileSize": lookup.get("fileSize").and_then(|v| v.as_u64()).unwrap_or(0),
    }))
}

/// 接收方：凭识别码连接发送方接收 zip，并按类型解压落地：
///   1=版本 → versions/<itemName>（同名自动加"(互传)"后缀）
///   2=模组 → versions/<目标版本>/mods/（target_version 为空时用发送端所属版本）
///   3=存档 → versions/<目标版本>/saves/<itemName>（同名自动加"(互传)"后缀）
///   0=普通文件 → 保存到 fallback_dir
#[tauri::command]
pub async fn transfer_receive_game(
    window: tauri::Window,
    code: String,
    target_version: String,
    fallback_dir: String,
) -> Result<serde_json::Value, String> {
    let code = code.trim().to_uppercase().replace(' ', "");
    if code.is_empty() {
        return Err("请输入识别码".into());
    }

    let lookup = lookup_session(&code).await?;
    let ip = lookup.get("ip").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let port = lookup.get("port").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
    let kind = lookup.get("kind").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
    let item_name = lookup.get("itemName").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let version_id = lookup.get("versionId").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let file_name = lookup.get("fileName").and_then(|v| v.as_str()).unwrap_or("file").to_string();

    let stream = connect_sender(&ip, port).await?;

    // 先落地为临时 zip（kind>0），普通文件直接写到目标目录
    let tmp_zip = if kind == KIND_FILE {
        None
    } else {
        Some(tmp_dir().join(format!("vxfer_recv_{}_{}.zip", std::process::id(), chrono::Local::now().timestamp_millis())))
    };

    let save_path: PathBuf = match &tmp_zip {
        Some(p) => p.clone(),
        None => {
            let dir = PathBuf::from(&fallback_dir);
            if dir.as_os_str().is_empty() {
                return Err("请先选择保存位置".into());
            }
            let safe_name = file_name.replace('\\', "/").rsplit('/').next().unwrap_or("file").to_string();
            dir.join(safe_name)
        }
    };

    let (result, total_bytes) = receive_to_path(stream, save_path.clone(), |received, total| {
        let _ = window.emit(
            "transfer-receive-progress",
            serde_json::json!({ "received": received, "total": total }),
        );
    })
    .await?;

    if !result.hash_ok {
        let _ = tokio::fs::remove_file(&save_path).await;
        return Err("接收完成但校验未通过，文件可能已损坏".into());
    }

    // 按类型解压落地
    let dest = if let Some(zip_path) = &tmp_zip {
        let target = if target_version.trim().is_empty() { version_id.clone() } else { target_version.trim().to_string() };
        let dest = resolve_dest(kind, &item_name, &target)?;
        let dest_desc = dest.to_string_lossy().to_string();
        // 解压为阻塞操作，放线程池
        let zp = zip_path.clone();
        let dp = dest.clone();
        tokio::task::spawn_blocking(move || unzip_to(&zp, &dp))
            .await
            .map_err(|e| format!("解压任务失败: {e}"))?
            .map_err(|e| e)?;
        let _ = tokio::fs::remove_file(zip_path).await;
        dest_desc
    } else {
        result.saved_path.clone()
    };

    let kind_name = match kind {
        KIND_VERSION => "版本",
        KIND_MODS => "模组",
        KIND_SAVE => "存档",
        _ => "文件",
    };
    Ok(serde_json::json!({
        "ok": true,
        "kind": kind,
        "kindName": kind_name,
        "itemName": if item_name.is_empty() { result.file_name.clone() } else { item_name },
        "dest": dest,
        "fileSize": total_bytes,
    }))
}

/// 计算接收落地目录（含同名冲突处理）
fn resolve_dest(kind: u8, item_name: &str, target_version: &str) -> Result<PathBuf, String> {
    let versions = versions_dir();
    let safe_item = sanitize_dir_name(if item_name.is_empty() { "item" } else { item_name });
    match kind {
        KIND_VERSION => {
            let mut name = safe_item.clone();
            let mut n = 1;
            while versions.join(&name).exists() {
                n += 1;
                name = format!("{safe_item} (互传{n})");
            }
            Ok(versions.join(name))
        }
        KIND_MODS => {
            let t = sanitize_dir_name(if target_version.is_empty() { "item" } else { target_version });
            Ok(versions.join(t).join("mods"))
        }
        KIND_SAVE => {
            let t = sanitize_dir_name(if target_version.is_empty() { "item" } else { target_version });
            let saves = versions.join(t).join("saves");
            let mut name = safe_item.clone();
            let mut n = 1;
            while saves.join(&name).exists() {
                n += 1;
                name = format!("{safe_item} (互传{n})");
            }
            Ok(saves.join(name))
        }
        _ => Err("未知的传输类型".into()),
    }
}

/// 解压 zip 到目标目录（zip crate 内置 zip-slip 防护）
fn unzip_to(zip_path: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("无法创建目标目录: {e}"))?;
    let file = std::fs::File::open(zip_path).map_err(|e| format!("无法打开临时包: {e}"))?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))
        .map_err(|e| format!("临时包读取失败: {e}"))?;
    archive.extract(dest).map_err(|e| format!("解压失败: {e}"))?;
    Ok(())
}

/// 接收字节流到指定路径（协议头 + 分块流 + 回执），期间通过回调推送进度
async fn receive_to_path<F: Fn(u64, u64)>(
    mut stream: tokio::net::TcpStream,
    save_path: PathBuf,
    progress: F,
) -> Result<(TransferResult, u64), String> {
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;
    // 头部
    let mut magic = [0u8; 8];
    tokio::io::AsyncReadExt::read_exact(&mut stream, &mut magic)
        .await
        .map_err(|e| format!("读取头部失败: {e}"))?;
    if &magic != MAGIC {
        return Err("协议不匹配，对端不是 Verse 互传".into());
    }
    let mut len_buf = [0u8; 4];
    tokio::io::AsyncReadExt::read_exact(&mut stream, &mut len_buf)
        .await
        .map_err(|e| format!("读取文件名失败: {e}"))?;
    let name_len = u32::from_be_bytes(len_buf) as usize;
    if name_len == 0 || name_len > 1024 {
        return Err("文件名长度异常".into());
    }
    let mut name_buf = vec![0u8; name_len];
    tokio::io::AsyncReadExt::read_exact(&mut stream, &mut name_buf)
        .await
        .map_err(|e| format!("读取文件名失败: {e}"))?;
    let file_name = String::from_utf8_lossy(&name_buf).to_string();
    let mut size_buf = [0u8; 8];
    tokio::io::AsyncReadExt::read_exact(&mut stream, &mut size_buf)
        .await
        .map_err(|e| format!("读取大小失败: {e}"))?;
    let file_size = u64::from_be_bytes(size_buf);
    let mut hash_buf = [0u8; 32];
    tokio::io::AsyncReadExt::read_exact(&mut stream, &mut hash_buf)
        .await
        .map_err(|e| format!("读取校验值失败: {e}"))?;
    let expected_hash: String = hash_buf.iter().map(|b| format!("{:02X}", b)).collect();

    // 普通文件防路径穿越：仅取文件名部分（zip 由解压器做安全处理）
    let final_path = if save_path.is_dir() {
        let safe_name = file_name.replace('\\', "/").rsplit('/').next().unwrap_or("file").to_string();
        save_path.join(safe_name)
    } else {
        if let Some(parent) = save_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("无法创建保存目录: {e}"))?;
        }
        save_path.clone()
    };

    let mut out = tokio::fs::File::create(&final_path)
        .await
        .map_err(|e| format!("无法写入文件: {e}"))?;
    let mut hasher = Sha256::new();
    let mut received: u64 = 0;
    let mut last_emit: u64 = 0;
    let mut buf = vec![0u8; CHUNK_SIZE];
    loop {
        let want = std::cmp::min(buf.len(), (file_size - received) as usize);
        if want == 0 {
            break;
        }
        let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf[..want])
            .await
            .map_err(|e| format!("接收数据失败: {e}"))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])
            .await
            .map_err(|e| format!("写入文件失败: {e}"))?;
        hasher.update(&buf[..n]);
        received += n as u64;
        // 进度事件（每 ~1MB 推一次，避免刷屏）
        if file_size > 0 && received - last_emit >= 1024 * 1024 {
            last_emit = received;
            progress(received, file_size);
        }
    }
    out.flush().await.ok();
    progress(received, file_size);

    let actual = hasher.finalize();
    let actual_hash: String = actual.iter().map(|b| format!("{:02X}", b)).collect();
    let hash_ok = actual_hash == expected_hash && received == file_size;
    // 回执确认
    stream
        .write_all(&[if hash_ok { 0x01 } else { 0x00 }])
        .await
        .ok();

    Ok((
        TransferResult {
            file_name,
            saved_path: final_path.to_string_lossy().to_string(),
            file_size: received,
            hash_ok,
        },
        file_size,
    ))
}

/// 接收方（保留兼容）：任意文件保存到指定目录
#[tauri::command]
pub async fn transfer_receive(
    window: tauri::Window,
    code: String,
    save_dir: String,
) -> Result<TransferResult, String> {
    let code = code.trim().to_uppercase().replace(' ', "");
    if code.is_empty() {
        return Err("请输入识别码".into());
    }
    let lookup = lookup_session(&code).await?;
    let ip = lookup.get("ip").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let port = lookup.get("port").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
    let stream = connect_sender(&ip, port).await?;
    receive_to_path(stream, PathBuf::from(&save_dir), |r, t| {
        let _ = window.emit(
            "transfer-receive-progress",
            serde_json::json!({ "received": r, "total": t }),
        );
    })
    .await
    .map(|(r, _)| r)
}

fn hex_to_bytes(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err("哈希长度异常".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| format!("哈希解析失败: {e}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 本地 TCP 回路测试（不经信令，信令+激活校验链路由部署侧实测）：
    /// serve_file ↔ receive_to_path，验证协议、分块流与 SHA-256 校验
    #[tokio::test(flavor = "multi_thread")]
    async fn transfer_roundtrip_local() {
        let dir = std::env::temp_dir().join(format!("vxfer_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("创建临时目录失败");
        let file_path = dir.join("sample.bin");
        let data: Vec<u8> = (0..1_000_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&file_path, &data).expect("写入测试文件失败");

        let (hash, size) = hash_file(&file_path).await.expect("计算哈希失败");
        assert_eq!(size, data.len() as u64);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("绑定端口失败");
        let port = listener.local_addr().unwrap().port();

        let status = Arc::new(Mutex::new(SendStatus {
            listening: true,
            total_bytes: size,
            ..Default::default()
        }));
        let offer = Arc::new(Offer {
            path: file_path.clone(),
            file_name: "sample.bin".into(),
            file_size: size,
            hash: hash.clone(),
            status: status.clone(),
            tmp: None,
        });
        tokio::spawn(serve_file(listener, offer, status));

        let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .expect("连接发送方失败");
        let save_path = dir.join("received").join("sample.bin");
        std::fs::create_dir_all(save_path.parent().unwrap()).expect("创建接收目录失败");
        // 测试环境没有 Tauri Window：用 no-op 进度回调
        let (result, _total) = receive_to_path(stream, save_path.clone(), |_, _| {})
            .await
            .expect("接收失败");
        assert!(result.hash_ok, "哈希校验未通过");
        assert_eq!(result.file_name, "sample.bin");
        assert_eq!(result.file_size, data.len() as u64);
        let got = std::fs::read(&save_path).expect("读取接收文件失败");
        assert_eq!(got, data, "接收内容与发送内容不一致");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// zip 打包 ↔ 解压 回路测试：嵌套目录结构与内容一致
    #[test]
    fn zip_pack_extract_roundtrip() {
        let dir = std::env::temp_dir().join(format!("vxfer_zip_{}", std::process::id()));
        let base = dir.join("base");
        std::fs::create_dir_all(base.join("region")).expect("创建目录失败");
        std::fs::write(base.join("level.dat"), b"hello level").expect("写文件失败");
        std::fs::write(base.join("region").join("r.0.0.mca"), vec![7u8; 300_000]).expect("写文件失败");

        let rel = collect_files(&base);
        assert_eq!(rel.len(), 2);
        let zip_path = dir.join("pack.zip");
        let n = pack_zip(&base, &rel, &zip_path).expect("打包失败");
        assert_eq!(n, 2);

        let dest = dir.join("dest");
        unzip_to(&zip_path, &dest).expect("解压失败");
        assert_eq!(std::fs::read(dest.join("level.dat")).unwrap(), b"hello level");
        assert_eq!(
            std::fs::read(dest.join("region").join("r.0.0.mca")).unwrap().len(),
            300_000
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
