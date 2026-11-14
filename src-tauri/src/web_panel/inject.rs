// web_panel/inject.rs — 面板注入脚本模板与构建（从 web_panel.rs 拆出，控制单文件行数）

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

pub(super) fn build_panel_script(local_base: &str, upstream: &str, hide: bool) -> String {
    PANEL_SCRIPT_TEMPLATE
        .replace("__LOCAL__", local_base)
        .replace("__UP__", &format!("https://{}", upstream))
        .replace("__HIDE__", if hide { "true" } else { "false" })
}
