//! 内建 HTTP 服务器 + 应用内浏览器：把 HTML 前端跑起来。
//!
//! 分工：
//! - `start()` 在插件加载后调用：起回环 HTTP 服务器（`http-server`），再在后台
//!   用 `browser::open` 打开前端（不阻塞 `on_load`）。前端就是 web/ 下的静态 HTML。
//! - `handle()` 是 `http` 导出（world = `psys-world-v4-http`）：
//!   - `GET /` 与静态资源 → 返回 web/ 内嵌的文件（`include_str!`，不读盘）；
//!   - `GET /api/state` → 现有 `snapshot()` 的 JSON（见 `web.rs`）；
//!   - `POST /api/action` → body 是 action 字符串，走 `parse_action` + `handle_action`，
//!     **复用全部现有后端逻辑**，一个 action 都不重写。
//!
//! 安全：服务器只绑 127.0.0.1（回环），不申请 `.lan`，局域网访问不到。
//! 前端不接触任何凭证；账号/云存档操作仍由后端 `handle_action` 走既有通道。

use super::wit::astrobox::psys_host_v4::{browser, http_server};
use super::{handle_action, snapshot};
use super::web::snapshot_json;
use amakano2_ui::parse_action;
use serde_json::json;

// ---- 内嵌的前端静态文件（编译期打包进 wasm，不读插件目录） ----
const INDEX_HTML: &str = include_str!("../../web/index.html");
const APP_JS: &str = include_str!("../../web/app.js");
const APP_CSS: &str = include_str!("../../web/app.css");
// 字体走**插件目录文件**（manifest additional_files 随包分发），`fonts.rs` 用 std::fs
// 读并缓存。三字重 MiSans 约 6MB，内嵌进 wasm 会拖慢编译与加载，放目录按需读更合适。

/// 服务器实例 id（起成功后存下，handle 里校验用）。
static SERVER_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(u32::MAX);

/// 启动 HTTP 服务器。失败只记日志，不影响插件其余功能。
///
/// **不在这里自动开窗口。** 官方 v4 文档写明 `browser.open` **每次调用都会弹一次授权**
/// （permission `browser`，用户可在插件权限设置里撤销）。旧实现在 `on_load` 里无条件
/// `open`，于是每次启用插件都会冒出一个授权框；用户慢一拍点「拒绝」，
/// 日志里就是一条 `ERROR ... 打开 HTML 前端窗口失败 error=permission denied` ——
/// 看上去像插件坏了，实际只是那次授权没点。
///
/// 现在改成：服务器照常起（插件主面板需要一个能点的入口），窗口交给用户显式动作
/// （`Action::OpenPanel`，即主面板上的「打开控制面板」按钮）再开。
pub async fn start() {
    let options = http_server::ServerOptions { port: 0, bind_all_interfaces: false };
    match http_server::start(options).await {
        Ok(info) => {
            SERVER_ID.store(info.id, std::sync::atomic::Ordering::Relaxed);
            tracing::info!(url = %info.url, "HTML 前端服务器已启动");
        }
        Err(e) => tracing::error!(error = %e, "启动 HTML 前端服务器失败"),
    }
}

/// 重新打开前端窗口（用户点「打开控制面板」）。每次调用 `browser::open` 一次，
/// 因此每次都会走一次宿主授权 —— 这是用户显式动作换来的，不是自己冒出来的。
pub async fn reopen() {
    wit_bindgen::spawn(async move {
        match http_server::list_servers().into_iter().find(|s| s.id == SERVER_ID.load(std::sync::atomic::Ordering::Relaxed)) {
            Some(info) => open_frontend(info.url).await,
            None => tracing::warn!("控制面板服务器不在了，无法重新打开"),
        }
    });
}

/// 后台 guest 任务：打开 HTML 前端窗口（不阻塞事件分发器）。
async fn open_frontend(url: String) {
    let open = browser::OpenOptions {
        url,
        title: Some("甜蜜女友2 导入器".into()),
        user_agent: None,
        intercept_prefixes: vec![],
        close_on_intercept: false,
        ephemeral: false,
        width: Some(420),
        height: Some(860),
    };
    match browser::open(open).await {
        Ok(_) => tracing::info!("已打开 HTML 前端窗口"),
        // 用户拒绝授权不是故障：宿主返回的 `permission denied` 就落在这里。
        // 用 warn 而不是 error，日志页的「错误数」不该被一次主动拒绝顶上去；
        // 同时把「这是授权没点」说清楚，别让人去查插件是不是坏了。
        Err(e) if e.contains("permission") => {
            tracing::warn!(error = %e, "打开控制面板被拒绝：需要允许本插件的 browser 权限（插件权限设置里可重新授予）")
        }
        Err(e) => tracing::warn!(error = %e, "打开 HTML 前端窗口失败"),
    }
}

fn response(status: u16, content_type: &str, body: Vec<u8>) -> http_server::Response {
    http_server::Response {
        status,
        headers: vec![
            http_server::Header { name: "content-type".into(), value: content_type.into() },
            // 回环 + 仅本插件的前端会打这个端口，禁缓存保证 /api/state 永远是新的。
            http_server::Header { name: "cache-control".into(), value: "no-store".into() },
        ],
        body,
    }
}

fn text(status: u16, s: &str) -> http_server::Response {
    response(status, "text/plain; charset=utf-8", s.as_bytes().to_vec())
}

/// 404，给 fonts.rs 越界/缺失时复用。
pub fn not_found() -> http_server::Response {
    text(404, "not found")
}

fn json_response(v: serde_json::Value) -> http_server::Response {
    response(200, "application/json; charset=utf-8", v.to_string().into_bytes())
}

/// 静态资源路由。返回 None 表示不是静态路径。
fn static_file(path: &str) -> Option<http_server::Response> {
    let (ct, body): (&str, Vec<u8>) = match path {
        // index.html 原样返回（不再做占位符替换）。
        "/" | "/index.html" => ("text/html; charset=utf-8", INDEX_HTML.as_bytes().to_vec()),
        "/app.js" => ("text/javascript; charset=utf-8", APP_JS.as_bytes().to_vec()),
        "/app.css" => ("text/css; charset=utf-8", APP_CSS.as_bytes().to_vec()),
        _ => return None,
    };
    Some(response(200, ct, body))
}

/// 从请求头里取某个头的值（大小写不敏感）。
fn header<'a>(req: &'a http_server::Request, name: &str) -> Option<&'a str> {
    req.headers.iter().find(|h| h.name.eq_ignore_ascii_case(name)).map(|h| h.value.as_str())
}

/// `http` 导出：所有前端请求都从这里进。
pub async fn handle(server_id: u32, request: http_server::Request) -> http_server::Response {
    // 只认自己起的那个服务器实例：`start()` 失败时 SERVER_ID 是 u32::MAX，
    // 这时任何请求都不该被处理（也不该泄露状态）。
    let expected = SERVER_ID.load(std::sync::atomic::Ordering::Relaxed);
    if expected == u32::MAX || server_id != expected {
        return text(403, "forbidden");
    }
    let path = request.path.as_str();
    let method = request.method.as_str();

    // API：实时状态
    if method == "GET" && path == "/api/state" {
        return json_response(snapshot_json(&snapshot()));
    }

    // API：动作。body 直接就是 action 字符串（与元素树 event_id 同一套词汇）。
    if method == "POST" && path == "/api/action" {
        // 跨站防护：浏览器对「跨站简单请求」会带 Origin 头。拒绝任何带 Origin
        // 且不是本机回环的请求；同窗口的 fetch 是同源请求，Origin 为空或匹配本机。
        // **不再用注入 CSRF token**：真机上 browser 窗口的地址与 token 注入存在
        // 时序差异，曾出现 token 不一致把正常请求挡掉（界面显示 `csrf`）。
        if let Some(origin) = header(&request, "origin") {
            let ok = origin.starts_with("http://127.0.0.1:")
                || origin.starts_with("http://localhost:")
                || origin.starts_with("http://[::1]:");
            if !ok {
                return text(403, "cross-origin");
            }
        }
        let action_str = String::from_utf8_lossy(&request.body).trim().to_string();
        let Some(action) = parse_action(&action_str) else {
            return text(400, &format!("未知动作：{action_str}"));
        };
        handle_action(action).await;
        return json_response(json!({ "ok": true }));
    }

    // 静态资源
    if method == "GET" {
        if let Some(resp) = static_file(path) {
            return resp;
        }
        // 字体等其它内嵌资源
        if let Some(resp) = super::fonts::font_response(path) {
            return resp;
        }
    }

    text(404, "not found")
}
