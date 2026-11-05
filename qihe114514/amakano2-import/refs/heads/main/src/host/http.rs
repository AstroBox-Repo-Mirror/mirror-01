//! 云服务的 WASI HTTP 客户端。只发送宿主公开账号 ID 与本设备凭证，不接触宿主令牌。

use std::time::Duration;

use serde_json::Value;

const MAX_RESPONSE_BYTES: usize = 384 * 1024;
const API_ROOT: &str = "/api/cloud/v1";

#[derive(Debug)]
pub struct ApiFailure {
    pub status: u16,
    #[allow(dead_code)]
    pub code: String,
    pub message: String,
    #[allow(dead_code)]
    pub body: Option<Value>,
}

impl ApiFailure {
    pub fn transport(message: impl Into<String>) -> Self {
        Self { status: 0, code: "network_error".into(), message: message.into(), body: None }
    }
}

pub fn random_id() -> Result<String, String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| "无法生成安全设备 ID".to_string())?;
    let mut id = String::with_capacity(32);
    for byte in bytes { id.push_str(&format!("{byte:02x}")); }
    Ok(id)
}

/// 云服务器地址校验：只允许 `https://`，且目标不能是回环/私有/保留地址。
///
/// 为什么要这么严：`packs/cloud-config.json` 虽然随包分发，但它在插件目录里是可写的
/// （宿主更新插件时会整个重写目录，用户/其它插件也能碰）。拿到这个地址的人只要把它改成
/// 一台自己控制的机器，就能收到我们每次请求带的 `Authorization: Bearer <设备凭证>`。
/// 所以「协议必须是 https」不够，还得把目标限制在公网地址上 —— 否则
/// `https://127.0.0.1:8443` 一样能把凭证发给本机上的任意监听服务。
///
/// 判定逻辑本身住在 `crate::net`（宿主机上可测），这里只做 URI 解析与协议检查。
pub fn validate_server(value: &str) -> Result<String, String> {
    let uri = value.trim().trim_end_matches('/').parse::<wstd::http::Uri>().map_err(|_| "云服务器地址无效".to_string())?;
    if uri.scheme_str() != Some("https") {
        return Err("云服务器必须使用 HTTPS".into());
    }
    let host = uri.authority().map(|authority| authority.host().to_string()).unwrap_or_default();
    if host.is_empty() {
        return Err("云服务器地址缺少主机名".into());
    }
    if !crate::net::is_public_host(&host) {
        return Err("云服务器地址指向本机或内网，已拒绝".into());
    }
    if uri.path() != "/" && !uri.path().is_empty() { return Err("云服务器地址不要带路径".into()); }
    if uri.query().is_some() { return Err("云服务器地址不要带查询参数".into()); }
    Ok(value.trim().trim_end_matches('/').to_string())
}

pub async fn request(server: &str, method: &str, path: &str, credential: Option<&str>, body: Option<Value>) -> Result<Value, ApiFailure> {
    let url = format!("{}{API_ROOT}{path}", validate_server(server).map_err(ApiFailure::transport)?);
    let mut request = wstd::http::Request::builder().method(method).uri(url);
    if let Some(credential) = credential {
        request = request.header("Authorization", format!("Bearer {credential}"));
    }
    let request = if let Some(body) = body {
        request.header("Content-Type", "application/json").body(wstd::http::Body::from_json(&body).map_err(|error| ApiFailure::transport(error.to_string()))?)
    } else {
        request.body(wstd::http::Body::empty())
    }.map_err(|error| ApiFailure::transport(error.to_string()))?;
    let mut client = wstd::http::Client::new();
    client.set_connect_timeout(Duration::from_secs(15));
    client.set_first_byte_timeout(Duration::from_secs(15));
    client.set_between_bytes_timeout(Duration::from_secs(15));
    // AstroBox 的 async export 不等于 wstd 的运行时：HTTP 与响应体都依赖
    // wstd 的 Reactor，必须在同一个 block_on 中完成发送和读取。
    let (status, bytes) = wstd::runtime::block_on(async {
        let mut response = client.send(request).await.map_err(|error| ApiFailure::transport(format!("无法连接云服务器：{error}")))?;
        let status = response.status().as_u16();
        let bytes = response.body_mut().bytes_contents().await.map_err(|error| ApiFailure::transport(format!("云服务器响应读取失败：{error}")))?;
        Ok::<_, ApiFailure>((status, bytes))
    })?;
    if bytes.len() > MAX_RESPONSE_BYTES { return Err(ApiFailure::transport("云服务器响应过大")); }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| ApiFailure::transport("云服务器返回的不是合法 JSON"))?;
    if !(200..300).contains(&status) {
        return Err(ApiFailure {
            status,
            code: value.get("error").and_then(Value::as_str).unwrap_or("http_error").to_string(),
            message: value.get("message").and_then(Value::as_str).unwrap_or("云服务请求失败").to_string(),
            body: Some(value),
        });
    }
    Ok(value)
}
