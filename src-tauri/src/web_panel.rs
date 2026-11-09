// VersePC 内置网页面板（社区 / 皮肤站）
// ----------------------------------------------------------------------------
// 承载方式：本地转发代理 + 主窗口内的 iframe。
//   · 本机多窗口环境里，Tauri 无法可靠地再创建第二个 WebView 窗口（窗口句柄与
//     webview 上下文都拿不到），因此不复用第二 webview。
//   · 两个站点各由一个仅监听 127.0.0.1 的反向代理转发，代理只做三件事：
//       1) 去掉 X-Frame-Options / CSP（让页面能被 iframe 承载，站点源码不做任何改动）
//       2) 会话 Cookie 由代理侧保管（浏览器侧不落 Cookie，避开第三方 Cookie 限制）
//       3) 注入一段脚本：隐藏社区站「返回主站」、新窗口链接改成就地跳转、工具栏通信
//   · 网页最终渲染在应用自己的页面里（iframe），不存在独立窗口、不会弹出外部浏览器；
//     切换页面只是 CSS 切换，浏览位置与登录状态都保留。
//   · 「返回主站」只在应用内隐藏：原站点与浏览器访问完全不受影响。

use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode},
    response::Response,
};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

/// 社区站上游
const COMMUNITY_HOST: &str = "community.verselauncher.cn";
/// 皮肤站上游
const SKIN_HOST: &str = "skin.verselauncher.cn";

/// 首选端口（被占用时自动顺延）
const COMMUNITY_PORT: u16 = 17866;
const SKIN_PORT: u16 = 17867;

/// 代理请求体上限（8 MiB，足够发帖与图片上传）
const BODY_LIMIT: usize = 8 * 1024 * 1024;

/// 注入脚本模板（__LOCAL__/__UP__/__HIDE__ 在启动时替换，避免 format! 转义）
const PANEL_SCRIPT_TEMPLATE: &str = r#"
(function(){
  var LOCAL='__LOCAL__', UP='__UP__', HIDE=__HIDE__;
  // 内嵌场景禁用站点 Service Worker：它的网络代理会把 POST 等请求改写成跨域请求，
  // 导致登录提交直接 "Failed to fetch"。禁用后站点按普通页面运行，功能不受影响。
  try{
    if(navigator.serviceWorker){
      navigator.serviceWorker.register=function(){ return Promise.reject(new Error('sw-disabled-in-panel')); };
      if(navigator.serviceWorker.getRegistrations){
        navigator.serviceWorker.getRegistrations().then(function(rs){
          for(var i=0;i<rs.length;i++){ try{ rs[i].unregister(); }catch(e){} }
        })['catch'](function(){});
      }
    }
  }catch(e){}
  function mapUrl(u){
    if(u===undefined||u===null) return u;
    var s=String(u);
    if(s.indexOf(UP)===0) return LOCAL+s.slice(UP.length);
    return s;
  }
  function fixLinks(){
    try{
      var nodes=document.querySelectorAll('a[href^="'+UP+'"]');
      for(var i=0;i<nodes.length;i++){
        var h=nodes[i].getAttribute('href');
        if(h && h.indexOf(UP)===0) nodes[i].setAttribute('href', LOCAL+h.slice(UP.length));
      }
    }catch(e){}
  }
  function hideMain(){
    if(!HIDE) return;
    try{
      var id='__verse_panel_style__';
      if(!document.getElementById(id)){
        var s=document.createElement('style'); s.id=id;
        s.textContent='a[title="返回主站"],a[data-verse-hide-main-site]{display:none!important}';
        (document.head||document.documentElement).appendChild(s);
      }
      var list=document.querySelectorAll('a[title="返回主站"]');
      for(var i=0;i<list.length;i++) list[i].setAttribute('data-verse-hide-main-site','1');
    }catch(e){}
  }
  function tick(){
    fixLinks();
    fixForms();
    hideMain();
  }
  function fixForms(){
    try{
      var fs=document.querySelectorAll('form[action]');
      for(var i=0;i<fs.length;i++){
        var a=fs[i].getAttribute('action');
        if(a && a.indexOf(UP)===0) fs[i].setAttribute('action', mapUrl(a));
      }
    }catch(e){}
  }
  // 站内绝对地址的 XHR / fetch 一律改走本地代理，避免跨域与第三方 Cookie 限制
  // 注意：地址已经是本地时不能重建 Request，否则带请求体的 Request 会被判为已消耗，
  // 表现为登录接口直接 "Failed to fetch"
  try{
    var _fetch=window.fetch;
    window.fetch=function(input, init){
      try{
        if(typeof input==='string'){
          var mapped=mapUrl(input);
          if(mapped!==input) input=mapped;
        }else if(input && input.url){
          var mapped2=mapUrl(input.url);
          if(mapped2!==input.url) input=new Request(mapped2, input);
        }
      }catch(e){}
      return _fetch.apply(this,[input, init]);
    };
  }catch(e){}
  try{
    var _open=XMLHttpRequest.prototype.open;
    XMLHttpRequest.prototype.open=function(){
      try{ if(arguments.length>1) arguments[1]=mapUrl(arguments[1]); }catch(e){}
      return _open.apply(this, arguments);
    };
  }catch(e){}
  try{
    window.open=function(u){
      if(u){
        try{
          var t=String(u);
          if(/afdian/i.test(t) && askLoginInApp()) return null;
          if(isDownloadHref(t) && askExternal(t)) return null;
          if(t.indexOf(UP)===0) t=LOCAL+t.slice(UP.length);
          location.assign(t);
        }catch(e){}
      }
      return null;
    };
  }catch(e){}
  // 下载类链接（皮肤/资源文件）改由系统浏览器直接下载：内嵌窗口里的下载会被拦，
  // 且用户明确要求“下载时跳到浏览器”
  function toRealUrl(h){
    try{
      if(!h) return location.href;
      if(h.indexOf(LOCAL)===0) return UP + h.slice(LOCAL.length);
      if(h.indexOf(UP)===0) return h;
      if(h.charAt(0)==='/') return UP + h;
      return h;
    }catch(e){ return h; }
  }
  function isDownloadHref(h){
    try{
      if(!h) return false;
      if(/\/download\b/i.test(h)) return true;
      if(/\.(png|json|zip|jar|mcpack|mcaddon|mrpack)([?#]|$)/i.test(h)) return true;
    }catch(e){}
    return false;
  }
  function askExternal(h){
    try{
      var real = toRealUrl(h);
      if(!/^https?:\/\//i.test(real)) return false;
      parent.postMessage({__versePanelExternal:real}, '*');
      return true;
    }catch(e){ return false; }
  }
  // 爱发电授权：外站回调地址被爱发电的域名白名单拦住了（改回调会被判“非可信域名”），
  // 所以改成让软件自己的窗口去吃这个授权：主 WebView 顶层打开社区站的登录入口，
  // 回调用官方认可的域名，授权完把 WebView 里的会话抓回来写进软件。
  function askExternalAbs(u){
    try{
      if(!/^https?:\/\//i.test(u)) return false;
      parent.postMessage({__versePanelExternal:u}, '*');
      return true;
    }catch(e){ return false; }
  }
  function askLoginInApp(){
    try{
      parent.postMessage({__versePanelLogin:true}, '*');
      return true;
    }catch(e){ return false; }
  }
  function afdianStart(){
    return LOCAL + '/__verse/afdian-start';
  }
  // 站点用「动态创建 a[download] 再 click()」触发下载，这类节点不在 DOM 上，
  // 捕获不到 click 事件，所以直接接管 prototype 上的 click()
  try{
    var _aclick=HTMLAnchorElement.prototype.click;
    HTMLAnchorElement.prototype.click=function(){
      try{
        var hh=this.getAttribute('href');
        if((this.hasAttribute('download') || isDownloadHref(hh)) && askExternal(hh)) return;
      }catch(e){}
      return _aclick.apply(this, arguments);
    };
  }catch(e){}
  // 社区站的第三方登录只有「使用爱发电账号登录」，回调地址绑死在主站域名，
  // 应用内 iframe 里闭环不了，所以交给主 WebView 顶层走一次（仍在软件窗口内）
  function afdianTarget(node){
    try{
      var n=node, g=0;
      while(n && n!==document && g++<8){
        if(n.tagName){
          var tag=n.tagName.toLowerCase();
          var txt=(n.textContent||'').trim();
          if(tag==='a'){
            var h=n.getAttribute('href')||'';
            if(h.indexOf('afdian')>=0) return 'login';
            if(/^使用爱发电/.test(txt) || txt.indexOf('爱发电账号登录')>=0 || txt.indexOf('关联爱发电')>=0) return 'login';
          } else if(tag==='button'){
            if(/^使用爱发电/.test(txt) || txt.indexOf('爱发电账号登录')>=0) return 'login';
          }
        }
        n=n.parentNode;
      }
    }catch(e){}
    return null;
  }
  document.addEventListener('click', function(ev){
    var aff=afdianTarget(ev.target);
    if(aff && askLoginInApp()){
      ev.preventDefault();
      ev.stopPropagation();
      return;
    }
    var n=ev.target, g=0;

    while(n && n!==document && g++<30){
      if(n.tagName && n.tagName.toLowerCase()==='a'){
        var t=n.getAttribute('target');
        if(t && t!=='_self') n.setAttribute('target','_self');
        var h=n.getAttribute('href');
        if((n.hasAttribute('download') || isDownloadHref(h)) && askExternal(h)){
          ev.preventDefault();
          ev.stopPropagation();
          return;
        }
        if(h && h.indexOf(UP)===0) n.setAttribute('href', LOCAL+h.slice(UP.length));
        break;
      }
      n=n.parentNode;
    }
  }, true);
  var pending=false;
  var mo=new MutationObserver(function(){
    if(pending) return;
    pending=true;
    setTimeout(function(){ pending=false; tick(); },60);
  });
  function boot(){
    tick();
    try{ mo.observe(document.body, {childList:true, subtree:true, attributes:true, attributeFilter:['href']}); }catch(e){}
  }
  if(document.body) boot(); else document.addEventListener('DOMContentLoaded', boot);
  parent.postMessage && window.addEventListener('message', function(ev){
    var d=ev.data||{};
    if(!d || d.__versePanelCmd===undefined) return;
    try{
      if(d.__versePanelCmd==='back') history.back();
      else if(d.__versePanelCmd==='forward') history.forward();
      else if(d.__versePanelCmd==='reload') location.reload();
      else if(d.__versePanelCmd==='home') location.assign(LOCAL+'/');
    }catch(e){}
  });
  function report(){
    try{ parent.postMessage({__versePanel:'state', url:location.href, title:document.title}, '*'); }catch(e){}
  }
  window.addEventListener('load', report);
  window.addEventListener('popstate', report);
  setInterval(report, 1200);
  report();
})();
"#;

fn build_panel_script(local_base: &str, upstream: &str, hide: bool) -> String {
    PANEL_SCRIPT_TEMPLATE
        .replace("__LOCAL__", local_base)
        .replace("__UP__", &format!("https://{}", upstream))
        .replace("__HIDE__", if hide { "true" } else { "false" })
}

/// 诊断日志：仅在设置了环境变量 VERSEPC2_PANEL_LOG 时写入临时目录
fn wlog(msg: &str) {
    if std::env::var("VERSEPC2_PANEL_LOG").is_err() {
        return;
    }
    use std::io::Write;
    let path = std::env::temp_dir().join("verse_webpanel.log");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "[{ts}] {msg}");
    }
}

/// 单个站点的代理上下文
struct PanelCtx {
    kind: &'static str,
    upstream_host: &'static str,
    local_base: String,
    client: reqwest::Client,
    script: String,
    jar: Mutex<HashMap<String, String>>,
    jar_file: Option<std::path::PathBuf>,
    /// 是否把响应正文里的站点绝对地址改写成本地地址
    /// （皮肤站的 JS 以绝对地址加载代码分块，不改写会被 CORS 拦掉，登录表单就渲染不出来）
    rewrite_body: bool,
}

/// 会话 Cookie 落盘位置：%APPDATA%\VersePC2\webpanel-<kind>.json
/// 这样关闭软件再打开，皮肤站/社区的登录状态仍然保留
fn jar_file_for(kind: &str) -> Option<std::path::PathBuf> {
    let base = std::env::var_os("APPDATA").map(std::path::PathBuf::from)?;
    let dir = base.join("VersePC2");
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join(format!("webpanel-{kind}.json")))
}

fn load_jar(path: &Option<std::path::PathBuf>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Some(p) = path else { return map };
    let Ok(text) = std::fs::read_to_string(p) else {
        return map;
    };
    if let Ok(v) = serde_json::from_str::<HashMap<String, String>>(&text) {
        map = v;
    }
    map
}

fn save_jar(path: &Option<std::path::PathBuf>, jar: &HashMap<String, String>) {
    let Some(p) = path else { return };
    if let Ok(text) = serde_json::to_string(jar) {
        let _ = std::fs::write(p, text);
    }
}

/// 前端据此拼 iframe 地址
#[derive(Clone, Serialize)]
pub struct Endpoints {
    pub community: String,
    pub skin: String,
}

static ENDPOINTS: Mutex<Option<Endpoints>> = Mutex::new(None);

/// 已启动的代理上下文（供「同步浏览器登录」把 Cookie 写进对应站点的会话里）
static CONTEXTS: Mutex<Vec<Arc<PanelCtx>>> = Mutex::new(Vec::new());

/// 前端读取内置网页地址
#[tauri::command]
pub fn web_panel_endpoints() -> Option<Endpoints> {
    ENDPOINTS.lock().ok().and_then(|g| g.clone())
}

/// 解析用户粘贴的 Cookie 文本：
/// 支持 `a=1; b=2`、每行一条、DevTools 复制出来的 `名称<TAB>值` 行，以及 JSON 对象
fn parse_cookie_pairs(raw: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let text = raw.trim();
    if text.is_empty() {
        return out;
    }
    // JSON 形态
    if text.starts_with('{') {
        if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(text) {
            for (k, v) in map {
                let k = k.trim().to_string();
                if !k.is_empty() {
                    out.push((k, v.trim().to_string()));
                }
            }
            return out;
        }
    }
    let cleaned = text
        .strip_prefix("Cookie:")
        .map(|s| s.trim())
        .unwrap_or(text);
    for part in cleaned.split(|c| c == ';' || c == '\n' || c == '\r') {
        let item = part.trim();
        if item.is_empty() {
            continue;
        }
        // 名称与值之间可能是 '=' 或制表符/多空格（DevTools 复制）
        let (name, value) = if let Some(eq) = item.find('=') {
            (item[..eq].trim(), item[eq + 1..].trim())
        } else if let Some(tab) = item.find('\t') {
            (item[..tab].trim(), item[tab + 1..].trim())
        } else {
            continue;
        };
        if name.is_empty() || name.contains(' ') {
            continue;
        }
        out.push((name.to_string(), value.to_string()));
    }
    out
}

/// 前端「同步登录」：把浏览器里登录后的 Cookie 写进内置网页的会话
#[tauri::command]
pub fn web_panel_set_cookies(kind: String, raw: String) -> Result<usize, String> {
    let pairs = parse_cookie_pairs(&raw);
    if pairs.is_empty() {
        return Err("没有解析到可用的 Cookie，请粘贴形如 name=value 的内容".into());
    }
    let list = CONTEXTS.lock().map_err(|_| "内部状态不可用")?.clone();
    let ctx = list
        .into_iter()
        .find(|c| c.kind == kind.as_str())
        .ok_or_else(|| format!("未知站点: {kind}"))?;
    let mut jar = ctx.jar.lock().map_err(|_| "会话锁不可用")?;
    let mut count = 0usize;
    for (name, value) in pairs {
        if value.is_empty() {
            continue;
        }
        jar.insert(name, value);
        count += 1;
    }
    save_jar(&ctx.jar_file, &jar);
    wlog(&format!("{kind} 会话已注入 {count} 条 Cookie"));
    Ok(count)
}

/// 在内置网页里点“下载”时，用系统默认浏览器打开真实地址下载
#[tauri::command]
pub fn web_panel_open_external(url: String) {
    let u = url.trim();
    if !(u.starts_with("http://") || u.starts_with("https://")) {
        return;
    }
    let _ = open::that(u);
}

/// 当前代理会话里保存的 Cookie 名（不返回具体值，供前端判断登录态是否变化）
#[tauri::command]
pub fn web_panel_session_keys(kind: String) -> Result<Vec<String>, String> {
    let list = CONTEXTS.lock().map_err(|_| "内部状态不可用")?.clone();
    let ctx = list
        .into_iter()
        .find(|c| c.kind == kind.as_str())
        .ok_or_else(|| format!("未知站点: {kind}"))?;
    let jar = ctx.jar.lock().map_err(|_| "会话锁不可用")?;
    let mut keys: Vec<String> = jar.keys().cloned().collect();
    keys.sort();
    Ok(keys)
}

// ============== 爱发电授权中转（社区站第三方登录） ==============
//
// 社区站的登录入口 /api/afdian/login 会 307 到爱发电授权页，回调地址固定写死成
// https://www.verselauncher.cn，iframe 里没法闭环。这里把 redirect_uri 换成
// http://127.0.0.1:<本地端口>/__verse/afdian-callback：浏览器里授权完成后回调落在
// 本地代理，代理再把 code/state 转给社区站真正的 /api/afdian/callback，
// 社区站下发的会话 Cookie 就进了软件自己的会话罐，于是不用再手工粘贴任何东西。

fn jar_cookie_header(ctx: &Arc<PanelCtx>) -> String {
    match ctx.jar.lock() {
        Ok(jar) => jar
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; "),
        Err(_) => String::new(),
    }
}

fn absorb_set_cookie(ctx: &Arc<PanelCtx>, headers: &reqwest::header::HeaderMap) {
    let cookies: Vec<String> = headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .collect();
    if cookies.is_empty() {
        return;
    }
    if let Ok(mut jar) = ctx.jar.lock() {
        for sc in cookies {
            let first = sc.split(';').next().unwrap_or("");
            if let Some((k, v)) = first.split_once('=') {
                let key = k.trim().to_string();
                let val = v.trim().to_string();
                if val.is_empty() || sc.to_ascii_lowercase().contains("deleted") {
                    jar.remove(&key);
                } else {
                    jar.insert(key, val);
                }
            }
        }
        save_jar(&ctx.jar_file, &jar);
    }
}

/// 把 Location 里的 redirect_uri 参数替换成新的（已做过百分号编码的）值
fn rewrite_redirect_uri(location: &str, encoded: &str) -> String {
    let key = "redirect_uri=";
    match location.find(key) {
        Some(pos) => {
            let start = pos + key.len();
            let end = location[start..]
                .find('&')
                .map(|i| start + i)
                .unwrap_or(location.len());
            format!("{}{}{}", &location[..start], encoded, &location[end..])
        }
        None => {
            let sep = if location.contains('?') { '&' } else { '?' };
            format!("{location}{sep}redirect_uri={encoded}")
        }
    }
}

fn local_callback_uri(ctx: &Arc<PanelCtx>) -> String {
    // http://127.0.0.1:17866/__verse/afdian-callback -> 百分号编码
    let host_port = ctx.local_base.trim_start_matches("http://");
    let hp = host_port.replace(':', "%3A");
    format!("http%3A%2F%2F{hp}%2F__verse%2Fafdian-callback")
}

/// 把上游返回的相对 Location 补成绝对地址
fn absolutize(base: &str, loc: &str) -> String {
    let l = loc.trim();
    if l.starts_with("http://") || l.starts_with("https://") {
        return l.to_string();
    }
    let scheme_host = match base.find("://") {
        Some(i) => match base[i + 3..].find('/') {
            Some(j) => base[..i + 3 + j].to_string(),
            None => base.to_string(),
        },
        None => base.to_string(),
    };
    if l.starts_with('/') {
        format!("{scheme_host}{l}")
    } else if l.starts_with('?') {
        format!("{scheme_host}{l}")
    } else {
        format!("{scheme_host}/{l}")
    }
}

fn host_of(url: &str) -> String {
    let rest = url.splitn(2, "://").nth(1).unwrap_or("");
    let host = rest.split('/').next().unwrap_or("");
    host.to_string()
}

fn encode_component(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(ch),
            _ => {
                let mut buf = [0u8; 4];
                for b in ch.encode_utf8(&mut buf).as_bytes() {
                    out.push_str(&format!("%{:02X}", b));
                }
            }
        }
    }
    out
}

fn query_param(query: &str, key: &str) -> Option<String> {
    for part in query.split('&') {
        if let Some((k, v)) = part.split_once('=') {
            if k == key {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn bridge_page(title: &str, detail: &str) -> Response {
    let html = format!(
        "<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<title>{title}</title><style>\
html,body{{height:100%;margin:0}}\
body{{display:flex;align-items:center;justify-content:center;background:#f6f7f9;\
font-family:system-ui,-apple-system,'Segoe UI','Microsoft YaHei',sans-serif;color:#1d1d1f}}\
.card{{padding:32px 36px;border-radius:16px;background:#fff;box-shadow:0 8px 32px rgba(0,0,0,.08);text-align:center;max-width:420px}}\
h1{{font-size:18px;margin:0 0 10px}}\
p{{font-size:13px;line-height:1.8;color:#6b6b70;margin:0}}\
</style></head><body><div class=\"card\"><h1>{title}</h1><p>{detail}</p></div></body></html>"
    );
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/html; charset=utf-8")
        .body(Body::from(html))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

async fn afdian_bridge(ctx: Arc<PanelCtx>, path: &str, query: Option<&str>) -> Response {
    let cookie = jar_cookie_header(&ctx);
    let is_callback = path.contains("callback");
    let target = if is_callback {
        let q = query.map(|q| format!("?{q}")).unwrap_or_default();
        format!(
            "https://{}/api/afdian/callback{}",
            ctx.upstream_host, q
        )
    } else {
        format!("https://{}/api/afdian/login", ctx.upstream_host)
    };

    let mut req = ctx
        .client
        .get(&target)
        .header("host", ctx.upstream_host)
        .header("accept-language", "zh-CN,zh;q=0.9")
        .header("user-agent", "VersePC2/1.5.0 internal-panel");
    if !cookie.is_empty() {
        req = req.header("cookie", cookie);
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            wlog(&format!("afdian 中转请求失败: {e}"));
            return bridge_page("授权没有完成", "网络请求失败，可以回到软件里重试。");
        }
    };
    absorb_set_cookie(&ctx, resp.headers());

    if is_callback {
        // 授权回到本机后，code 不能直接丢给社区站（社区站只认主站加密过的 code），
        // 要先交给主站 verselauncher.cn 的 /api/afdian/callback 换一次，
        // 再用它跳到社区站时带的那串加密 code 去换会话。
        let raw_query = query.unwrap_or("");
        let code = query_param(raw_query, "code").unwrap_or_default();
        let state = query_param(raw_query, "state").unwrap_or_default();
        if code.is_empty() {
            return bridge_page(
                "授权没有完成",
                "没有拿到授权码，可能是授权被取消。回到软件里可以重新点一次登录。",
            );
        }

        let before: Vec<String> = web_panel_session_keys("community".to_string()).unwrap_or_default();
        let community_host = ctx.upstream_host.to_string();
        let mut current = format!(
            "https://verselauncher.cn/api/afdian/callback?code={}&state={}",
            encode_component(&code),
            encode_component(&state)
        );
        let mut login_error: Option<String> = None;
        let mut hops = 0;

        while hops < 5 {
            hops += 1;
            let host = host_of(&current);
            let cookie_now = jar_cookie_header(&ctx);
            let mut req = ctx
                .client
                .get(&current)
                .header("host", &host)
                .header("accept-language", "zh-CN,zh;q=0.9")
                .header("user-agent", "VersePC2/1.5.0 internal-panel");
            if !cookie_now.is_empty() {
                req = req.header("cookie", cookie_now);
            }
            let resp = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    wlog(&format!("afdian 授权链路请求失败: {e}"));
                    break;
                }
            };
            absorb_set_cookie(&ctx, resp.headers());
            let loc = resp
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            if loc.is_empty() {
                break;
            }
            if loc.contains("login_error") {
                login_error = Some(loc.clone());
                break;
            }
            let next = absolutize(&current, &loc);
            // 到了社区站的回调就停下（下一轮会用它换会话）
            if host_of(&next).contains(community_host.as_str()) && next.contains("/api/afdian/callback") {
                current = next;
                break;
            }
            if host_of(&next) == host && next == current {
                break;
            }
            current = next;
        }

        // 最后一步：拿加密后的 code 去社区站换会话
        if login_error.is_none() && host_of(&current).contains(community_host.as_str()) {
            let cookie_now = jar_cookie_header(&ctx);
            let mut req = ctx
                .client
                .get(&current)
                .header("host", &community_host)
                .header("accept-language", "zh-CN,zh;q=0.9")
                .header("user-agent", "VersePC2/1.5.0 internal-panel");
            if !cookie_now.is_empty() {
                req = req.header("cookie", cookie_now);
            }
            if let Ok(resp) = req.send().await {
                absorb_set_cookie(&ctx, resp.headers());
                if let Some(loc) = resp.headers().get("location").and_then(|v| v.to_str().ok()) {
                    if loc.contains("login_error") {
                        login_error = Some(loc.to_string());
                    }
                }
            }
        }

        let after: Vec<String> = web_panel_session_keys("community".to_string()).unwrap_or_default();
        let added: Vec<String> = after
            .iter()
            .filter(|k| !before.iter().any(|b| &b == k))
            .cloned()
            .collect();
        wlog(&format!(
            "afdian 授权完成: 会话新增={} 错误={:?}",
            added.join(","),
            login_error
        ));

        if let Some(err) = login_error {
            let reason = if err.contains("state_invalid") {
                "站点校验 state 失败"
            } else if err.contains("token_failed") {
                "站点换取令牌失败"
            } else {
                "站点拒绝了这次授权"
            };
            return bridge_page(
                "授权没有完成",
                &format!(
                    "{reason}。回到软件，可以改用工具栏「同步登录」粘贴一次登录令牌。"
                ),
            );
        }
        if added.iter().any(|k| k != "afdian_oauth_state") {
            return bridge_page(
                "已经登录成功",
                "请回到 VersePC2 软件，社区页面会自动刷新并显示登录状态。此页面可以关闭了。",
            );
        }
        return bridge_page(
            "授权已提交",
            "回到软件刷新一下社区页面看看；如果还是没登录，可以用工具栏「同步登录」粘贴一次登录令牌。",
        );
    }

    let location = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    if location.is_empty() {
        return bridge_page(
            "这里没法直接开始授权",
            "社区站没有返回授权地址，可以回到软件里用「同步登录」粘贴一次登录令牌。",
        );
    }
    let start = rewrite_redirect_uri(&location, &local_callback_uri(&ctx));
    wlog(&format!("afdian 授权中转: {start}"));
    Response::builder()
        .status(StatusCode::FOUND)
        .header("location", start)
        .header("content-type", "text/html; charset=utf-8")
        .body(Body::from(
            "<!doctype html><meta charset=\"utf-8\"><title>正在跳转爱发电授权…</title><p>正在跳转…</p>",
        ))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn request_header_is_skipped(name: &str) -> bool {    matches!(
        name,
        "host"
            | "connection"
            | "accept-encoding"
            | "accept-language"
            | "content-length"
            | "cookie"
            | "origin"
            | "referer"
            | "if-none-match"
            | "if-modified-since"
            | "if-match"
            | "if-range"
            | "upgrade"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "proxy-connection"
    )
}

fn response_header_is_skipped(name: &str) -> bool {
    matches!(
        name,
        "x-frame-options"
            | "content-security-policy"
            | "content-security-policy-report-only"
            | "transfer-encoding"
            | "content-length"
            | "connection"
            | "keep-alive"
            | "content-encoding"
            | "set-cookie"
    )
}

fn is_html(ctype: &str) -> bool {
    ctype.to_ascii_lowercase().contains("text/html")
}

async fn panel_handler(State(ctx): State<Arc<PanelCtx>>, req: Request) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let incoming = req.headers().clone();

    let body = req.into_body();
    let body_bytes = match axum::body::to_bytes(body, BODY_LIMIT).await {
        Ok(b) => b,
        Err(_) => axum::body::Bytes::new(),
    };

    let path_and_query = uri
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| "/".to_string());

    // 社区站的爱发电授权中转端点（不转发给站点，由代理自己处理）
    if ctx.kind == "community" && uri.path().starts_with("/__verse/afdian") {
        return afdian_bridge(ctx.clone(), uri.path(), uri.query()).await;
    }

    let target = format!("https://{}{}", ctx.upstream_host, path_and_query);

    let reqwest_method = reqwest::Method::from_bytes(method.as_str().as_bytes())
        .unwrap_or(reqwest::Method::GET);

    let mut builder = ctx
        .client
        .request(reqwest_method, &target)
        .header("host", ctx.upstream_host)
        .header("accept-encoding", "gzip")
        // 固定中文语言偏好：皮肤站等站点据此返回中文界面
        .header("accept-language", "zh-CN,zh;q=0.9,en;q=0.6");

    for (name, value) in incoming.iter() {
        let lower = name.as_str().to_ascii_lowercase();
        if request_header_is_skipped(&lower) || lower.starts_with("sec-fetch") {
            continue;
        }
        if let Ok(v) = value.to_str() {
            let rewritten = if lower == "referer" {
                v.replace(&ctx.local_base, &format!("https://{}", ctx.upstream_host))
            } else {
                v.to_string()
            };
            builder = builder.header(name.as_str(), rewritten);
        }
    }

    // 会话 Cookie：浏览器侧带过来的 + 代理自己保管的（后者用于跨重启保留登录态）
    let browser_cookie = incoming
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let mut names_in_browser: Vec<String> = Vec::new();
    for pair in browser_cookie.split(';') {
        if let Some((k, _)) = pair.split_once('=') {
            names_in_browser.push(k.trim().to_string());
        }
    }
    let mut cookie = browser_cookie.clone();
    if let Ok(jar) = ctx.jar.lock() {
        for (k, v) in jar.iter() {
            if names_in_browser.iter().any(|n| n == k) {
                continue;
            }
            if cookie.is_empty() {
                cookie = format!("{}={}", k, v);
            } else {
                cookie.push_str(&format!("; {}={}", k, v));
            }
        }
    }
    if !cookie.is_empty() {
        builder = builder.header("cookie", cookie);
    }
    if !body_bytes.is_empty() {
        builder = builder.body(body_bytes.to_vec());
    }

    let resp = match builder.send().await {
        Ok(r) => r,
        Err(e) => {
            wlog(&format!("proxy {} 请求失败: {e}", ctx.kind));
            let msg = format!("代理请求失败：{}", e);
            return Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .header("content-type", "text/plain; charset=utf-8")
                .body(Body::from(msg))
                .unwrap_or_else(|_| Response::new(Body::empty()));
        }
    };

    let status = resp.status();
    let ctype = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // 收集上游下发的会话 Cookie
    {
        let cookies: Vec<String> = resp
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .collect();
        if !cookies.is_empty() {
            if let Ok(mut jar) = ctx.jar.lock() {
                for sc in cookies {
                    let first = sc.split(';').next().unwrap_or("");
                    if let Some((k, v)) = first.split_once('=') {
                        let key = k.trim().to_string();
                        let val = v.trim().to_string();
                        if val.is_empty() || sc.to_ascii_lowercase().contains("deleted") {
                            jar.remove(&key);
                        } else {
                            jar.insert(key, val);
                        }
                    }
                }
                // 落盘保存，下次启动继续沿用该会话
                save_jar(&ctx.jar_file, &jar);
            }
        }
    }

    let mut resp_headers = HeaderMap::new();
    for (name, value) in resp.headers().iter() {
        let lower = name.as_str().to_ascii_lowercase();
        if response_header_is_skipped(&lower) {
            continue;
        }
        let mut v = value.clone();
        if lower == "location" {
            if let Ok(text) = value.to_str() {
                let upstream_base = format!("https://{}", ctx.upstream_host);
                if let Some(rest) = text.strip_prefix(&upstream_base) {
                    let replaced = format!("{}{}", ctx.local_base, rest);
                    if let Ok(nv) = HeaderValue::from_str(&replaced) {
                        v = nv;
                    }
                }
            }
        }
        resp_headers.append(name, v);
    }

    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(_) => axum::body::Bytes::new(),
    };

    let body_out: Vec<u8> = if method == Method::GET {
        let is_text = is_html(&ctype)
            || ctype.contains("javascript")
            || ctype.contains("text/css")
            || ctype.contains("json");
        match std::str::from_utf8(&bytes) {
            Ok(text) if is_text => {
                let mut out = text.to_string();
                if ctx.rewrite_body {
                    // JSON 里的地址会被转义成 https:\/\/host，两种形态都要换
                    let up_plain = format!("https://{}", ctx.upstream_host);
                    let up_esc = format!("https:\\/\\/{}", ctx.upstream_host);
                    let up_proto = format!("//{}", ctx.upstream_host);
                    if out.contains(&up_esc) {
                        out = out.replace(&up_esc, &ctx.local_base);
                    }
                    if out.contains(&up_plain) {
                        out = out.replace(&up_plain, &ctx.local_base);
                    }
                    if out.contains(&up_proto) {
                        out = out.replace(&up_proto, &ctx.local_base);
                    }
                }
                if is_html(&ctype) {
                    let injected = format!("<script>{}</script>", ctx.script);
                    let insert_at = out
                        .rfind("</body>")
                        .or_else(|| out.rfind("</html>"))
                        .unwrap_or(out.len());
                    out.insert_str(insert_at, &injected);
                }
                out.into_bytes()
            }
            _ => bytes.to_vec(),
        }
    } else {
        bytes.to_vec()
    };

    let mut builder = Response::builder().status(status);
    if is_html(&ctype) {
        resp_headers.remove("content-type");
        resp_headers.insert(
            "content-type",
            HeaderValue::from_static("text/html; charset=utf-8"),
        );
    }
    for (name, value) in resp_headers.iter() {
        builder = builder.header(name, value);
    }
    builder
        .header("content-length", body_out.len().to_string())
        .body(Body::from(body_out))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn spawn_site(
    kind: &'static str,
    upstream_host: &'static str,
    prefer_port: u16,
    hide: bool,
    rewrite_body: bool,
) -> Option<String> {
    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .gzip(true)
        .timeout(std::time::Duration::from_secs(45))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            wlog(&format!("创建 HTTP 客户端失败: {e}"));
            return None;
        }
    };

    // 端口被占用时顺延
    let mut port = prefer_port;
    let mut socket = None;
    for _ in 0..16 {
        match std::net::TcpListener::bind(("127.0.0.1", port)) {
            Ok(l) => {
                socket = Some(l);
                break;
            }
            Err(_) => port += 1,
        }
    }
    let socket = socket?;
    if socket.set_nonblocking(true).is_err() {
        wlog("设置代理监听为非阻塞失败");
        return None;
    }

    let local_base = format!("http://127.0.0.1:{}", port);
    let script = build_panel_script(&local_base, upstream_host, hide);

    // 载入上一次运行保存的会话 Cookie（保持登录状态）
    let jar_file = jar_file_for(kind);
    let jar = load_jar(&jar_file);

    let ctx = Arc::new(PanelCtx {
        kind,
        upstream_host,
        local_base: local_base.clone(),
        client,
        script,
        jar: Mutex::new(jar),
        jar_file,
        rewrite_body,
    });

    let router = axum::Router::new().fallback(panel_handler).with_state(ctx.clone());

    if let Ok(mut reg) = CONTEXTS.lock() {
        reg.push(ctx);
    }

    // 在独立线程自带的 tokio 运行时里跑代理服务，保证监听任务不会随主运行时被回收
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name(format!("verse-panel-{}", kind))
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                wlog(&format!("{kind} 代理运行时创建失败: {e}"));
                return;
            }
        };
        rt.block_on(async move {
            let listener = match tokio::net::TcpListener::from_std(socket) {
                Ok(l) => l,
                Err(e) => {
                    wlog(&format!("{kind} 代理监听转换失败: {e}"));
                    return;
                }
            };
            tokio::spawn(async move {
                if let Err(e) = axum::serve(listener, router).await {
                    wlog(&format!("{kind} 代理退出: {e}"));
                }
            });
            // 保持运行时存活
            std::future::pending::<()>().await;
        });
    });

    wlog(&format!("{kind} 代理已启动: {local_base}"));
    Some(local_base)
}

/// 启动两个内置网页代理（每个进程只启动一次）
pub fn init(_app: tauri::AppHandle) {
    {
        let guard = match ENDPOINTS.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if guard.is_some() {
            return;
        }
    }

    let community = spawn_site("community", COMMUNITY_HOST, COMMUNITY_PORT, true, false)
        .map(|base| format!("{base}/"));
    // 皮肤站首屏带上语言参数，默认中文界面
    let skin = spawn_site("skin", SKIN_HOST, SKIN_PORT, false, true)
        .map(|base| format!("{base}/?lang=zh_CN"));

    match (community, skin) {
        (Some(community), Some(skin)) => {
            if let Ok(mut guard) = ENDPOINTS.lock() {
                *guard = Some(Endpoints { community, skin });
                wlog("内置网页代理就绪");
            }
        }
        _ => wlog("内置网页代理启动失败（端口不可用）"),
    }
}
fn ctx_by_kind(kind: &str) -> Option<Arc<PanelCtx>> {
    let guard = match CONTEXTS.lock() {
        Ok(g) => g,
        Err(_) => return None,
    };
    guard.iter().find(|c| c.kind == kind).cloned()
}

/// 把指定 WebView 上该站点的 Cookie 写回软件自己的 cookie 罐
fn import_cookies_from(win: &tauri::WebviewWindow, kind: &str) -> Result<usize, String> {
    let ctx = ctx_by_kind(kind).ok_or_else(|| "面板未就绪".to_string())?;
    let url = format!("https://{}/", ctx.upstream_host)
        .parse::<tauri::Url>()
        .map_err(|e| e.to_string())?;
    let cookies = win
        .cookies_for_url(url)
        .map_err(|e| format!("读取 WebView 会话失败: {e}"))?;

    let mut count = 0usize;
    if let Ok(mut jar) = ctx.jar.lock() {
        for cookie in cookies.iter() {
            let name = cookie.name().to_string();
            let value = cookie.value().to_string();
            if name.is_empty() {
                continue;
            }
            if value.is_empty() || value.contains("deleted") {
                jar.remove(&name);
            } else {
                jar.insert(name, value);
            }
            count += 1;
        }
        if count > 0 {
            save_jar(&ctx.jar_file, &jar);
        }
    }
    Ok(count)
}

/// 用独立窗口完成站点登录（爱发电授权有回调域名白名单，不能改 redirect_uri）
///
/// 社区站的爱发电 OAuth 回调必须落在官方域名上，所以这里新建一个登录窗口，
/// 直接访问站点自己的登录入口：授权页 → 官方回调 → 站点写入会话 Cookie，
/// 全程都在这个窗口里走完；结束后用 cookies_for_url 把会话取回写进 cookie 罐，
/// 关闭窗口并通知前端刷新内嵌页。主界面（Vue 应用）完全不受影响。
#[tauri::command]
pub async fn web_panel_login_in_app(app: tauri::AppHandle, kind: String) -> Result<(), String> {
    let ctx = ctx_by_kind(&kind).ok_or_else(|| "面板未就绪".to_string())?;
    let host = ctx.upstream_host.clone();
    let login_url = if kind == "community" {
        format!("https://{host}/api/afdian/login")
    } else {
        format!("https://{host}/auth/login")
    };
    let parsed = login_url
        .parse::<tauri::Url>()
        .map_err(|e| format!("登录地址无效: {e}"))?;

    // 登录在窗口里完成：先把主窗口临时切到站点登录页，用户点一下授权，
    // 回调落到站点首页后自动取回会话、切回软件界面。全程不需要跳到系统浏览器。
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不可用".to_string())?;
    main.navigate(parsed)
        .map_err(|e| format!("打开登录页失败: {e}"))?;
    wlog(&format!("{kind} 登录页已打开: {login_url}"));

    let handle = app.clone();
    let kind2 = kind.clone();
    let host2 = host.clone();
    std::thread::spawn(move || {
        const APP_URL: &str = "http://tauri.localhost/";
        let app_url = match APP_URL.parse::<tauri::Url>() {
            Ok(u) => u,
            Err(_) => return,
        };
        let t0 = std::time::Instant::now();
        let mut done = false;
        while t0.elapsed() < std::time::Duration::from_secs(300) {
            std::thread::sleep(std::time::Duration::from_millis(600));
            let main = match handle.get_webview_window("main") {
                Some(w) => w,
                None => break,
            };
            let cur = match main.url() {
                Ok(u) => u.to_string(),
                Err(_) => continue,
            };
            if cur.starts_with(APP_URL) {
                // 用户自己退出登录流程
                break;
            }
            let landed = cur.contains(&host2)
                && !cur.contains("/api/afdian/")
                && !cur.contains("/auth/login")
                && !cur.contains("login_error")
                && !cur.contains("ifdian.net");
            if landed {
                // 给站点留点时间把会话 cookie 写完
                std::thread::sleep(std::time::Duration::from_millis(1200));
                match import_cookies_from(&main, &kind2) {
                    Ok(n) => wlog(&format!("{kind2} 登录完成，取回 {n} 条 Cookie")),
                    Err(e) => wlog(&format!("{kind2} 取回会话失败: {e}")),
                }
                done = true;
                break;
            }
        }
        if !done {
            wlog(&format!("{kind2} 登录未完成（超时或用户取消）"));
        }
        // 切回软件界面
        if let Some(main) = handle.get_webview_window("main") {
            let _ = main.navigate(app_url);
        }
        let _ = handle.emit("webpanel:session-refreshed", kind2.clone());
    });

    Ok(())
}
