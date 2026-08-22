// network/upnp.rs — UPnP IGD 端口映射
// 职责：SSDP 网关发现 + SOAP 端口映射增删
//
// 协议流程：
//   1. SSDP M-SEARCH：UDP 多播 239.255.255.250:1900，发现 IGD 设备
//   2. 解析响应里的 LOCATION 头，拿到设备描述 XML URL
//   3. HTTP GET 设备描述，找到 WANIPConnection 服务的 controlURL
//   4. SOAP POST AddPortMapping / DeletePortMapping
//
// 简化：仅支持 IPv4 + WANIPConnection（覆盖大部分家用路由器）
// 函数全部 async，调用方在异步上下文中使用

use std::net::UdpSocket;
use std::time::Duration;

use serde_json::{json, Value};

/// UPnP 网关信息
pub struct UpnpGateway {
    pub address: String,
    pub location: String,
}

/// 发现 UPnP 网关（SSDP M-SEARCH，同步阻塞）
/// 超时 3 秒，返回第一个响应的 IGD 设备
pub fn discover_gateway() -> Result<UpnpGateway, String> {
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("绑定 UDP 失败: {}", e))?;
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|e| format!("设置超时失败: {}", e))?;

    // SSDP M-SEARCH 请求
    let search_msg = "M-SEARCH * HTTP/1.1\r\n\
HOST: 239.255.255.250:1900\r\n\
MAN: \"ssdp:discover\"\r\n\
MX: 2\r\n\
ST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\
\r\n";

    socket
        .send_to(search_msg.as_bytes(), "239.255.255.250:1900")
        .map_err(|e| format!("发送 SSDP 失败: {}", e))?;

    // 接收响应
    let mut buf = [0u8; 4096];
    let (n, src) = socket
        .recv_from(&mut buf)
        .map_err(|e| format!("接收 SSDP 响应超时: {}", e))?;

    let response = String::from_utf8_lossy(&buf[..n]);

    // 解析 LOCATION 头（不区分大小写）
    let location = response
        .lines()
        .find_map(|line| {
            if line.to_lowercase().starts_with("location:") {
                let value = line.splitn(2, ':').nth(1).unwrap_or("");
                Some(value.trim().to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| "响应中无 LOCATION 头".to_string())?;

    Ok(UpnpGateway {
        address: src.ip().to_string(),
        location,
    })
}

/// 添加端口映射
/// 返回 { success, externalPort, internalPort, localIP, description }
pub async fn add_port_mapping(
    internal_port: u16,
    external_port: u16,
    description: &str,
) -> Result<Value, String> {
    let gateway = discover_gateway()?;
    let local_ip = get_local_ip_for_gateway(&gateway.address)?;
    let control_url = get_wanip_control_url(&gateway.location).await?;

    // SOAP 请求：AddPortMapping
    let soap_body = format!(
        r#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:AddPortMapping xmlns:u="urn:schemas-upnp-org:service:WANIPConnection:1">
      <NewRemoteHost></NewRemoteHost>
      <NewExternalPort>{}</NewExternalPort>
      <NewProtocol>TCP</NewProtocol>
      <NewInternalPort>{}</NewInternalPort>
      <NewInternalClient>{}</NewInternalClient>
      <NewEnabled>1</NewEnabled>
      <NewPortMappingDescription>{}</NewPortMappingDescription>
      <NewLeaseDuration>0</NewLeaseDuration>
    </u:AddPortMapping>
  </s:Body>
</s:Envelope>"#,
        external_port, internal_port, local_ip, description
    );

    let url = reqwest::Url::parse(&control_url)
        .map_err(|e| format!("解析 controlURL 失败: {}", e))?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    let resp = client
        .post(url.clone())
        .header("Content-Type", "text/xml; charset=\"utf-8\"")
        .header(
            "SOAPAction",
            "\"urn:schemas-upnp-org:service:WANIPConnection:1#AddPortMapping\"",
        )
        .body(soap_body)
        .send()
        .await
        .map_err(|e| format!("发送 SOAP 请求失败: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("SOAP HTTP {}", resp.status()));
    }

    // 记录映射
    crate::network::record_mapping(crate::network::UpnpMapping {
        external_port,
        internal_port,
        description: description.to_string(),
        local_ip: local_ip.clone(),
    });

    Ok(json!({
        "success": true,
        "externalPort": external_port,
        "internalPort": internal_port,
        "localIP": local_ip,
        "description": description,
        "gateway": gateway.address
    }))
}

/// 删除端口映射
pub async fn delete_port_mapping(external_port: u16) -> Result<Value, String> {
    let gateway = discover_gateway()?;
    let control_url = get_wanip_control_url(&gateway.location).await?;

    let soap_body = format!(
        r#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:DeletePortMapping xmlns:u="urn:schemas-upnp-org:service:WANIPConnection:1">
      <NewRemoteHost></NewRemoteHost>
      <NewExternalPort>{}</NewExternalPort>
      <NewProtocol>TCP</NewProtocol>
    </u:DeletePortMapping>
  </s:Body>
</s:Envelope>"#,
        external_port
    );

    let url = reqwest::Url::parse(&control_url)
        .map_err(|e| format!("解析 controlURL 失败: {}", e))?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    let resp = client
        .post(url)
        .header("Content-Type", "text/xml; charset=\"utf-8\"")
        .header(
            "SOAPAction",
            "\"urn:schemas-upnp-org:service:WANIPConnection:1#DeletePortMapping\"",
        )
        .body(soap_body)
        .send()
        .await
        .map_err(|e| format!("发送 SOAP 请求失败: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("SOAP HTTP {}", resp.status()));
    }

    crate::network::remove_mapping(external_port);

    Ok(json!({
        "success": true,
        "externalPort": external_port
    }))
}

/// 获取 WANIPConnection 的 controlURL
/// 设备描述 XML 里找 serviceType=urn:schemas-upnp-org:service:WANIPConnection:1 的 controlURL
async fn get_wanip_control_url(location: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    let resp = client
        .get(location)
        .send()
        .await
        .map_err(|e| format!("拉取设备描述失败: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("设备描述 HTTP {}", resp.status()));
    }

    let xml = resp
        .text()
        .await
        .map_err(|e| format!("读取响应失败: {}", e))?;

    // 简化解析：找到 <serviceType>...WANIPConnection...</serviceType> 后的 <controlURL>...</controlURL>
    let st_marker = "urn:schemas-upnp-org:service:WANIPConnection:1";
    let st_pos = xml.find(st_marker).ok_or_else(|| {
        "设备不支持 WANIPConnection 服务".to_string()
    })?;

    // 在 st_pos 附近找 controlURL（注意：实际 XML 中 controlURL 可能在 serviceType 之前，
    // 所以扩大搜索范围到该 service 节点的开头）
    let search_start = if st_pos > 500 { st_pos - 500 } else { 0 };
    let search_end = (st_pos + 2000).min(xml.len());
    let search_region = &xml[search_start..search_end];

    let ctrl_start = search_region
        .find("<controlURL>")
        .ok_or_else(|| "未找到 controlURL 标签".to_string())?;
    let ctrl_value_start = ctrl_start + "<controlURL>".len();
    let ctrl_end = search_region[ctrl_value_start..]
        .find("</controlURL>")
        .ok_or_else(|| "controlURL 标签未闭合".to_string())?;
    let control_path = &search_region[ctrl_value_start..ctrl_value_start + ctrl_end];

    // 拼接 base URL + control_path
    let base_url = get_base_url(location);
    Ok(format!("{}{}", base_url, control_path))
}

/// 从 location URL 提取 base URL（scheme://host:port）
fn get_base_url(location: &str) -> String {
    if let Some(scheme_end) = location.find("://") {
        let after_scheme = &location[scheme_end + 3..];
        if let Some(path_start) = after_scheme.find('/') {
            return location[..scheme_end + 3 + path_start].to_string();
        }
        return location.to_string();
    }
    location.to_string()
}

/// 获取本机用于访问网关的本地 IP
fn get_local_ip_for_gateway(gateway_addr: &str) -> Result<String, String> {
    // 用 UDP socket 连接网关地址，操作系统会自动选择合适的本地 IP
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("绑定 UDP 失败: {}", e))?;
    socket
        .connect(format!("{}:80", gateway_addr))
        .map_err(|e| format!("连接网关失败: {}", e))?;
    let local_addr = socket
        .local_addr()
        .map_err(|e| format!("获取本地地址失败: {}", e))?;
    Ok(local_addr.ip().to_string())
}

/// 列出本机所有 IPv4 地址（非内部回环）
pub fn list_local_ipv4() -> Vec<Value> {
    let mut ips = Vec::new();
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            if iface.is_loopback() {
                continue;
            }
            if let if_addrs::IfAddr::V4(v4) = iface.addr {
                ips.push(json!({
                    "interface": iface.name,
                    "address": v4.ip.to_string()
                }));
            }
        }
    }
    ips
}
