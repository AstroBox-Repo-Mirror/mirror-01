//! 前端静态资源里的字体与图片：从插件目录取（manifest additional_files 随包分发）。
//!
//! 为什么不内嵌进 wasm：三字重 MiSans 约 6MB，`include_bytes!` 会把组件撑大、
//! 拖慢 AOT 预编译与加载。放插件目录按需读 + 内存缓存，首次读后即驻留。
//!
//! 路径约定：前端请求 `/fonts/<Weight>/<file>.woff2` → 插件目录 `web/fonts/<Weight>/<file>`。
//! 只允许这个前缀下的 `.woff2` / `.png` / `.css`，拒绝 `..` 越界。

use std::fs;
use std::sync::{Mutex, OnceLock};
use std::collections::HashMap;
use super::wit::astrobox::psys_host_v4::http_server;

const FONT_ROOT: &str = "web/fonts";

/// 已读文件的缓存（路径 → 字节）。字体分片小、总量有限，驻留内存换零磁盘往返。
fn cache() -> &'static Mutex<HashMap<String, Vec<u8>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Vec<u8>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn content_type(path: &str) -> &'static str {
    if path.ends_with(".woff2") { "font/woff2" }
    else if path.ends_with(".css") { "text/css; charset=utf-8" }
    else if path.ends_with(".png") { "image/png" }
    else { "application/octet-stream" }
}

/// 处理 `/fonts/...` 与 `/icon.png` 请求；非这些路径返回 None。
pub fn font_response(path: &str) -> Option<http_server::Response> {
    // 只接受 /fonts/ 前缀或 /icon.png，且不允许越界。
    let rel = if let Some(rest) = path.strip_prefix("/fonts/") {
        format!("{FONT_ROOT}/{rest}")
    } else if path == "/icon.png" {
        "icon.png".to_string()
    } else {
        return None;
    };
    if rel.contains("..") || rel.contains('\\') {
        return Some(super::server::not_found());
    }

    let bytes = {
        let mut guard = cache().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(hit) = guard.get(&rel) {
            Some(hit.clone())
        } else {
            match fs::read(&rel) {
                Ok(data) => {
                    guard.insert(rel.clone(), data.clone());
                    Some(data)
                }
                Err(_) => None,
            }
        }
    };

    bytes.map(|body| http_server::Response {
        status: 200,
        headers: vec![
            http_server::Header { name: "content-type".into(), value: content_type(&rel).into() },
            // 字体带内容哈希、长期不变，允许前端缓存；icon.png 同理。
            http_server::Header { name: "cache-control".into(), value: "public, max-age=86400".into() },
        ],
        body,
    })
}
