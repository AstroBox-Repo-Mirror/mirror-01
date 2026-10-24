//! 与穿戴设备同步课表（QAIC Interconnect 协议）
//!
//! 通过 AstroBox 的 device、interconnect、thirdpartyapp 接口与手表上的 Var 课程表快应用通信，
//! 实现课表拉取、推送及格式导入。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::astrobox::psys_host::{device, interconnect, register, thirdpartyapp, timer};
use crate::model::Course;

/// 默认快应用包名（当无法从手表获取应用列表时使用）
const DEFAULT_PKG_NAME: &str = "com.azuma.syclass";
/// 目标应用名称关键词（用于在手表应用列表中查找）
const TARGET_APP_NAME_KEYWORD: &str = "Var课程表";
/// 候选包名列表（兼容历史版本）
const CANDIDATE_PKG_NAMES: [&str; 4] = [
    "com.azuma.syclass",
    "com.azuma.varclass",
    "com.azumachiaki.syclass",
    "com.azumachiaki.varclass",
];
const CHUNK_THRESHOLD_CLASSES: usize = 30;
const CHUNK_SIZE_CLASSES: usize = 15;
const MAX_SINGLE_PAYLOAD_BYTES: usize = 6 * 1024;
const MAX_CHUNK_PART_CLASSES_BYTES: usize = 3 * 1024;
const CHUNK_ACK_TIMEOUT_MS: u64 = 8_000;
const CHUNK_STEP_MAX_SENDS: u8 = 3;
const CHUNK_SESSION_MAX_RETRY: u8 = 2;
pub fn interconnect_pkg_name() -> &'static str {
    DEFAULT_PKG_NAME
}

#[derive(Debug, Clone)]
pub struct AbTagForApply {
    pub ref_date: String,
    pub ref_type: String,
    pub ref_day: u8,
    pub semester_start: Option<String>,
    pub week_mode: Option<String>,
}

impl AbTagForApply {
    fn has_ab_ref(&self) -> bool {
        is_valid_date(&self.ref_date)
            && (self.ref_type == Course::WEEK_A || self.ref_type == Course::WEEK_B)
            && (1..=7).contains(&self.ref_day)
    }

    fn has_semester_start(&self) -> bool {
        self.semester_start
            .as_deref()
            .map(is_valid_date)
            .unwrap_or(false)
    }

    fn to_json_value(&self) -> serde_json::Value {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "refDate".to_string(),
            serde_json::Value::String(self.ref_date.clone()),
        );
        obj.insert(
            "refType".to_string(),
            serde_json::Value::String(self.ref_type.clone()),
        );
        obj.insert(
            "refDay".to_string(),
            serde_json::Value::Number(serde_json::Number::from(self.ref_day)),
        );
        if let Some(ref semester_start) = self.semester_start {
            if is_valid_date(semester_start) {
                obj.insert(
                    "semesterStart".to_string(),
                    serde_json::Value::String(semester_start.clone()),
                );
            }
        }
        if let Some(ref mode) = self.week_mode {
            obj.insert(
                "weekMode".to_string(),
                serde_json::Value::String(mode.clone()),
            );
        }
        serde_json::Value::Object(obj)
    }
}

#[derive(Debug, Clone)]
pub struct InterconnectResult {
    pub courses: Vec<Course>,
    pub ab_tag: Option<AbTagForApply>,
}

#[derive(Debug, Clone)]
pub enum InterconnectHandleResult {
    Timetable(InterconnectResult),
    Control { message: String, is_error: bool },
}

#[derive(Debug, Clone)]
pub struct SyncSnapshot {
    pub status: String,
    pub cached_course_count: usize,
    pub last_device_addr: Option<String>,
    pub subscribed: bool,
}

#[derive(Default)]
struct SyncState {
    cached_courses: Vec<Course>,
    cached_ab_tag: Option<AbTagForApply>,
    /// (versionName, versionCode)，来自手表 interconnect 回包
    cached_vela_version: Option<(String, u32)>,
    status: String,
    last_device_addr: Option<String>,
    subscribed: bool,
    incoming_chunks: HashMap<String, IncomingChunkBuffer>,
    outgoing_chunk: Option<OutgoingChunkSession>,
    pending_outgoing_send: bool,
}

#[derive(Debug, Clone)]
struct IncomingChunkBuffer {
    total: usize,
    parts: Vec<Option<String>>,
    received: usize,
    encoding: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OutgoingChunkPhase {
    WaitStartAck,
    WaitPartAck,
    WaitFinishResult,
}

impl OutgoingChunkPhase {
    fn as_str(&self) -> &'static str {
        match self {
            Self::WaitStartAck => "start",
            Self::WaitPartAck => "part",
            Self::WaitFinishResult => "finish",
        }
    }
}

#[derive(Debug, Clone)]
struct OutgoingChunkSession {
    addr: String,
    pkg_name: String,
    session_id: String,
    chunks: Vec<Vec<serde_json::Value>>,
    ab_tag: Option<serde_json::Value>,
    week_mode: String,
    total_classes: usize,
    phase: OutgoingChunkPhase,
    current_index: usize,
    step_send_count: u8,
    session_retry: u8,
    step_nonce: u64,
}

struct OutgoingSendCommand {
    addr: String,
    pkg_name: String,
    session_id: String,
    payload: serde_json::Value,
    nonce: u64,
}

static SYNC_STATE: OnceLock<Mutex<SyncState>> = OnceLock::new();

fn sync_state() -> &'static Mutex<SyncState> {
    SYNC_STATE.get_or_init(|| Mutex::new(SyncState::default()))
}

fn with_state<R>(f: impl FnOnce(&mut SyncState) -> R) -> R {
    let mut guard = sync_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard)
}

pub fn set_status(status: impl Into<String>) {
    let status = status.into();
    with_state(|state| {
        state.status = status;
    });
}

pub fn snapshot() -> SyncSnapshot {
    with_state(|state| SyncSnapshot {
        status: if state.status.is_empty() {
            "等待同步".to_string()
        } else {
            state.status.clone()
        },
        cached_course_count: state.cached_courses.len(),
        last_device_addr: state.last_device_addr.clone(),
        subscribed: state.subscribed,
    })
}

fn set_last_device(addr: &str) {
    let addr = addr.to_string();
    with_state(|state| {
        state.last_device_addr = Some(addr);
    });
}

/// 获取首个已连接设备地址；若无则回退到设备列表首个。
pub async fn first_connected_device_addr() -> Option<String> {
    let devices = device::get_connected_device_list().await;
    devices.first().map(|d| d.addr.clone())
}

/// 获取一个可用于订阅的设备地址：优先已连接设备，其次已发现设备。
async fn first_bootstrap_device_addr() -> Option<(String, bool)> {
    let connected = device::get_connected_device_list().await;
    if let Some(d) = connected.first() {
        return Some((d.addr.clone(), true));
    }

    let all = device::get_device_list().await;
    all.first().map(|d| (d.addr.clone(), false))
}

/// 从手表应用列表中解析目标快应用包名（优先匹配名称含「Var课程表」的应用）
async fn resolve_target_pkg_name(addr: &str) -> Result<String, String> {
    let apps = thirdpartyapp::get_thirdparty_app_list(addr)
        .await
        .map_err(|e| format!("读取手表应用列表失败: {:?}", e))?;

    if let Some(app) = apps
        .iter()
        .find(|a| a.app_name.contains(TARGET_APP_NAME_KEYWORD))
    {
        return Ok(app.package_name.clone());
    }

    if let Some(app) = apps
        .iter()
        .find(|a| CANDIDATE_PKG_NAMES.iter().any(|pkg| *pkg == a.package_name))
    {
        return Ok(app.package_name.clone());
    }

    let app_names = apps
        .iter()
        .map(|a| format!("{}({})", a.app_name, a.package_name))
        .collect::<Vec<_>>()
        .join(", ");

    Err(format!(
        "手表未找到目标快应用。需要包含“{}”的应用。当前手表应用: {}",
        TARGET_APP_NAME_KEYWORD, app_names
    ))
}

/// 注册 interconnect 接收；若已注册（already/exists/duplicate）则视为成功
async fn ensure_interconnect_registered(addr: &str, pkg_name: &str) -> Result<(), String> {
    match register::register_interconnect_recv(addr, pkg_name).await {
        Ok(()) => Ok(()),
        Err(err) => {
            let raw = format!("{:?}", err);
            let lower = raw.to_lowercase();
            if lower.contains("already") || lower.contains("exists") || lower.contains("duplicate")
            {
                Ok(())
            } else {
                Err(format!(
                    "注册接收失败(addr={}, pkg={}): {}",
                    addr, pkg_name, raw
                ))
            }
        }
    }
}

/// 启动时执行：订阅首个设备的 interconnect 通道。
pub async fn bootstrap_sync() -> Result<(), String> {
    let (addr, connected) = first_bootstrap_device_addr()
        .await
        .ok_or_else(|| "未检测到可用设备，请先连接或扫描手表".to_string())?;

    let pkg_name = if connected {
        resolve_target_pkg_name(&addr)
            .await
            .unwrap_or_else(|_| DEFAULT_PKG_NAME.to_string())
    } else {
        DEFAULT_PKG_NAME.to_string()
    };
    ensure_interconnect_registered(&addr, &pkg_name).await?;

    set_last_device(&addr);
    with_state(|state| {
        state.subscribed = true;
        state.status = if connected {
            format!("已订阅设备 {} 的同步通道 ({})", addr, pkg_name)
        } else {
            format!(
                "已预注册设备 {} 的同步通道({})，待连接后生效",
                addr, pkg_name
            )
        };
    });
    Ok(())
}

/// 在设备后连场景下自动重试订阅；已订阅时直接返回。
pub async fn bootstrap_if_needed() -> Result<(), String> {
    let already_subscribed = with_state(|state| state.subscribed);
    if already_subscribed {
        return Ok(());
    }

    match bootstrap_sync().await {
        Ok(()) => Ok(()),
        Err(err) => {
            with_state(|state| {
                state.subscribed = false;
                state.status = format!("等待设备连接后自动重试: {}", err);
            });
            Err(err)
        }
    }
}

fn courses_for_device(courses: &[Course]) -> Vec<serde_json::Value> {
    courses
        .iter()
        .map(|c| {
            let mut obj = serde_json::json!({
                "day": c.day,
                "name": c.display_name(),
                "start": c.start,
                "end": c.end,
                "weekType": Course::normalize_week_type(Some(&c.week_type)),
            });
            if !c.room.trim().is_empty() {
                obj["room"] = serde_json::json!(c.room.trim());
            }
            if !c.weeks.is_empty() {
                obj["weeks"] = serde_json::json!(c.weeks);
            }
            obj
        })
        .collect()
}

fn is_valid_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    let year = &value[0..4];
    let month = &value[5..7];
    let day = &value[8..10];
    if !year.chars().all(|c| c.is_ascii_digit())
        || !month.chars().all(|c| c.is_ascii_digit())
        || !day.chars().all(|c| c.is_ascii_digit())
    {
        return false;
    }
    let Ok(month) = month.parse::<u32>() else {
        return false;
    };
    let Ok(day) = day.parse::<u32>() else {
        return false;
    };
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

fn normalize_week_mode(value: Option<&str>) -> Option<String> {
    match value.map(str::trim) {
        Some("ab") => Some("ab".to_string()),
        Some("multi") => Some("multi".to_string()),
        Some("none") => Some("none".to_string()),
        _ => None,
    }
}

fn json_date(obj: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> Option<String> {
    json_string(obj, keys).filter(|value| is_valid_date(value))
}

fn value_has_ab_ref(value: &serde_json::Value) -> bool {
    let Some(obj) = value.as_object() else {
        return false;
    };
    let Some(ref_date) = json_string(obj, &["refDate", "ref_date"]) else {
        return false;
    };
    let ref_type =
        Course::normalize_week_type(json_string(obj, &["refType", "ref_type"]).as_deref());
    let Some(ref_day) = json_u32(obj, &["refDay", "ref_day"]) else {
        return false;
    };
    is_valid_date(&ref_date)
        && (ref_type == Course::WEEK_A || ref_type == Course::WEEK_B)
        && (1..=7).contains(&ref_day)
}

fn value_has_semester_start(value: &serde_json::Value) -> bool {
    value
        .as_object()
        .and_then(|obj| json_date(obj, &["semesterStart", "semester_start"]))
        .is_some()
}

fn infer_week_mode(courses: &[Course], ab_tag: Option<&serde_json::Value>) -> String {
    if let Some(mode) = normalize_week_mode(
        ab_tag
            .and_then(|tag| tag.get("weekMode"))
            .and_then(|v| v.as_str()),
    ) {
        return mode.to_string();
    }
    if courses.iter().any(|c| !c.weeks.is_empty()) {
        "multi".to_string()
    } else if ab_tag.map(value_has_ab_ref).unwrap_or(false) {
        "ab".to_string()
    } else {
        "none".to_string()
    }
}

fn make_session_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("ab{:x}", millis)
}

async fn send_qaic_json(
    addr: &str,
    pkg_name: &str,
    payload: &serde_json::Value,
) -> Result<(), String> {
    interconnect::send_qaic_message(addr, pkg_name, &payload.to_string())
        .await
        .map_err(|e| format!("发送失败(addr={}, pkg={}): {:?}", addr, pkg_name, e))
}

async fn schedule_outgoing_timeout(session_id: &str, nonce: u64) {
    let payload = serde_json::json!({
        "type": "varclassChunkTimeout",
        "sessionId": session_id,
        "nonce": nonce,
    })
    .to_string();
    let _ = timer::set_timeout(CHUNK_ACK_TIMEOUT_MS, &payload).await;
}

fn mark_outgoing_failed(session_id: &str, message: String) {
    with_state(|state| {
        if state
            .outgoing_chunk
            .as_ref()
            .map(|session| session.session_id.as_str() == session_id)
            .unwrap_or(false)
        {
            state.outgoing_chunk = None;
        }
        state.status = message;
    });
}

async fn send_outgoing_current_step(session_id: &str) -> Result<(), String> {
    let command = with_state(|state| {
        let session = state
            .outgoing_chunk
            .as_mut()
            .ok_or_else(|| "没有活动的分片发送会话".to_string())?;
        if session.session_id != session_id {
            return Err("分片发送会话不匹配".to_string());
        }
        if session.step_send_count >= CHUNK_STEP_MAX_SENDS {
            return Err(format!(
                "分片阶段 {} 重试次数已用尽",
                session.phase.as_str()
            ));
        }
        session.step_send_count += 1;
        session.step_nonce = session.step_nonce.saturating_add(1);
        let nonce = session.step_nonce;
        let payload = match session.phase {
            OutgoingChunkPhase::WaitStartAck => serde_json::json!({
                "type": "pushTimetableChunkStart",
                "sessionId": session.session_id,
                "total": session.chunks.len(),
                "totalClasses": session.total_classes,
                "autoAddBreaks": false,
                "abTag": session.ab_tag,
                "weekMode": session.week_mode,
            }),
            OutgoingChunkPhase::WaitPartAck => {
                let chunk = session
                    .chunks
                    .get(session.current_index)
                    .cloned()
                    .unwrap_or_default();
                serde_json::json!({
                    "type": "pushTimetableChunkPart",
                    "sessionId": session.session_id,
                    "index": session.current_index,
                    "total": session.chunks.len(),
                    "classes": chunk,
                })
            }
            OutgoingChunkPhase::WaitFinishResult => serde_json::json!({
                "type": "pushTimetableChunkFinish",
                "sessionId": session.session_id,
                "total": session.chunks.len(),
                "totalClasses": session.total_classes,
            }),
        };
        state.status = match session.phase {
            OutgoingChunkPhase::WaitStartAck => format!(
                "分片同步: 发送开始请求（第 {} 次）",
                session.step_send_count
            ),
            OutgoingChunkPhase::WaitPartAck => format!(
                "分片同步: 发送分片 {}/{}（第 {} 次）",
                session.current_index + 1,
                session.chunks.len(),
                session.step_send_count
            ),
            OutgoingChunkPhase::WaitFinishResult => format!(
                "分片同步: 发送完成请求（第 {} 次）",
                session.step_send_count
            ),
        };
        Ok(OutgoingSendCommand {
            addr: session.addr.clone(),
            pkg_name: session.pkg_name.clone(),
            session_id: session.session_id.clone(),
            payload,
            nonce,
        })
    })?;

    send_qaic_json(&command.addr, &command.pkg_name, &command.payload).await?;
    schedule_outgoing_timeout(&command.session_id, command.nonce).await;
    Ok(())
}

fn spawn_send_outgoing_current_step(session_id: String) {
    with_state(|state| {
        if state
            .outgoing_chunk
            .as_ref()
            .map(|session| session.session_id == session_id)
            .unwrap_or(false)
        {
            state.pending_outgoing_send = true;
        }
    });
}

pub async fn flush_pending_outgoing_send() -> Option<InterconnectHandleResult> {
    let mut last_message: Option<(String, bool)> = None;

    for _ in 0..=CHUNK_STEP_MAX_SENDS {
        let session_id = with_state(|state| {
            if !state.pending_outgoing_send {
                return None;
            }
            let session_id = state
                .outgoing_chunk
                .as_ref()
                .map(|session| session.session_id.clone());
            state.pending_outgoing_send = false;
            session_id
        });

        let Some(session_id) = session_id else {
            break;
        };

        match send_outgoing_current_step(&session_id).await {
            Ok(()) => {
                let message = with_state(|state| state.status.clone());
                return Some(InterconnectHandleResult::Control {
                    message,
                    is_error: false,
                });
            }
            Err(err) => {
                if let Some((message, is_error)) =
                    retry_outgoing_current_step(&session_id, &format!("发送失败: {}", err))
                {
                    last_message = Some((message, is_error));
                    if is_error {
                        break;
                    }
                    continue;
                }

                let message = format!("分片同步发送失败: {}", err);
                mark_outgoing_failed(&session_id, message.clone());
                last_message = Some((message, true));
                break;
            }
        }
    }

    last_message.map(|(message, is_error)| InterconnectHandleResult::Control { message, is_error })
}

fn chunk_classes_for_transport(classes: &[serde_json::Value]) -> Vec<Vec<serde_json::Value>> {
    let mut chunks: Vec<Vec<serde_json::Value>> = Vec::new();
    let mut current: Vec<serde_json::Value> = Vec::new();
    let mut current_json_len = 2; // []

    for class_item in classes {
        let item_json_len = class_item.to_string().len();
        let separator_len = if current.is_empty() { 0 } else { 1 };
        let exceeds_count = current.len() >= CHUNK_SIZE_CLASSES;
        let exceeds_bytes = !current.is_empty()
            && current_json_len + separator_len + item_json_len > MAX_CHUNK_PART_CLASSES_BYTES;

        if exceeds_count || exceeds_bytes {
            chunks.push(std::mem::take(&mut current));
            current_json_len = 2;
        }

        let separator_len = if current.is_empty() { 0 } else { 1 };
        current_json_len += separator_len + item_json_len;
        current.push(class_item.clone());
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

async fn send_chunked_timetable(
    addr: &str,
    pkg_name: &str,
    classes: &[serde_json::Value],
    ab_tag: Option<&serde_json::Value>,
    week_mode: &str,
) -> Result<usize, String> {
    let chunks = chunk_classes_for_transport(classes);
    if chunks.is_empty() {
        return Ok(0);
    }
    let session_id = make_session_id();
    let chunk_count = chunks.len();
    with_state(|state| {
        state.outgoing_chunk = Some(OutgoingChunkSession {
            addr: addr.to_string(),
            pkg_name: pkg_name.to_string(),
            session_id: session_id.clone(),
            chunks,
            ab_tag: ab_tag.cloned(),
            week_mode: week_mode.to_string(),
            total_classes: classes.len(),
            phase: OutgoingChunkPhase::WaitStartAck,
            current_index: 0,
            step_send_count: 0,
            session_retry: 0,
            step_nonce: 0,
        });
        state.status = format!("分片同步: 已建立会话 {}，共 {} 片", session_id, chunk_count);
    });
    send_outgoing_current_step(&session_id).await?;
    Ok(chunk_count)
}

/// 推送课程到设备（pushTimetable / pushTimetableChunk 协议）。
pub async fn sync_to_device(courses: &[Course], ab_tag_json: Option<&str>) -> Result<(), String> {
    let addr = first_connected_device_addr()
        .await
        .ok_or_else(|| "无已连接设备，请先连接手表".to_string())?;
    let pkg_name = resolve_target_pkg_name(&addr).await?;
    ensure_interconnect_registered(&addr, &pkg_name).await?;

    let classes = courses_for_device(courses);
    let incoming_ab_tag =
        ab_tag_json.and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok());
    let week_mode = infer_week_mode(courses, incoming_ab_tag.as_ref());
    let ab_tag =
        incoming_ab_tag.filter(|tag| value_has_ab_ref(tag) || value_has_semester_start(tag));

    let payload = serde_json::json!({
        "type": "pushTimetable",
        "classes": classes,
        "abTag": ab_tag,
        "weekMode": week_mode,
    });
    let payload_len = payload.to_string().len();

    let chunk_count =
        if courses.len() > CHUNK_THRESHOLD_CLASSES || payload_len > MAX_SINGLE_PAYLOAD_BYTES {
            let classes = payload
                .get("classes")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            Some(
                send_chunked_timetable(
                    &addr,
                    &pkg_name,
                    &classes,
                    payload.get("abTag"),
                    payload
                        .get("weekMode")
                        .and_then(|v| v.as_str())
                        .unwrap_or("none"),
                )
                .await?,
            )
        } else {
            send_qaic_json(&addr, &pkg_name, &payload).await?;
            None
        };

    set_last_device(&addr);
    with_state(|state| {
        state.subscribed = true;
        state.status = if let Some(chunks) = chunk_count {
            format!(
                "已开始向设备 {} 的 {} 分片推送 {} 节课程（{} 片）",
                addr,
                pkg_name,
                courses.len(),
                chunks
            )
        } else {
            format!(
                "已向设备 {} 的 {} 推送 {} 节课程",
                addr,
                pkg_name,
                courses.len()
            )
        };
    });
    Ok(())
}

/// 将当前内存缓存推送到设备。
pub async fn sync_cached_to_device() -> Result<(), String> {
    let (courses, ab_tag_json) = with_state(|state| {
        let ab_tag_json = state
            .cached_ab_tag
            .as_ref()
            .filter(|tag| tag.has_ab_ref() || tag.has_semester_start() || tag.week_mode.is_some())
            .map(|tag| tag.to_json_value().to_string());
        (state.cached_courses.clone(), ab_tag_json)
    });

    if courses.is_empty() {
        return Err("当前无可推送课程，请先从手表拉取".to_string());
    }

    sync_to_device(&courses, ab_tag_json.as_deref()).await
}

/// 主动向设备请求课表（requestTimetable 协议）。
pub async fn request_timetable_from_device() -> Result<(), String> {
    let addr = first_connected_device_addr()
        .await
        .ok_or_else(|| "无已连接设备，请先连接手表".to_string())?;
    let pkg_name = resolve_target_pkg_name(&addr).await?;

    ensure_interconnect_registered(&addr, &pkg_name).await?;

    let payload = serde_json::json!({ "type": "requestTimetable" });
    interconnect::send_qaic_message(&addr, &pkg_name, &payload.to_string())
        .await
        .map_err(|e| format!("请求失败(addr={}, pkg={}): {:?}", addr, pkg_name, e))?;

    set_last_device(&addr);
    with_state(|state| {
        state.subscribed = true;
        state.status = format!("已向设备 {} 的 {} 请求最新课表", addr, pkg_name);
    });
    Ok(())
}

/// 在 `on_event` 里处理 interconnect 消息：只更新内存缓存，不写文件。
pub fn handle_interconnect_message(payload: &str) -> Result<InterconnectHandleResult, String> {
    if let Some(reassembled) = try_reassemble_generic_chunk(payload) {
        if let Some(full_payload) = reassembled {
            return handle_interconnect_message(&full_payload);
        }
        return Ok(InterconnectHandleResult::Control {
            message: "正在接收分片消息...".to_string(),
            is_error: false,
        });
    }

    if let Some((message, is_error)) = handle_chunk_control_message(payload) {
        return Ok(InterconnectHandleResult::Control { message, is_error });
    }

    if let Some((message, is_error)) = handle_protocol_status_message(payload) {
        return Ok(InterconnectHandleResult::Control { message, is_error });
    }

    let (mut courses, ab_tag, vela_version, _week_mode) = parse_timetable_data(payload)
        .ok_or_else(|| {
            let mut brief = payload.replace('\n', " ");
            if brief.len() > 220 {
                brief.truncate(220);
                brief.push_str("...");
            }
            format!("消息不是可识别课表回包，payload={}", brief)
        })?;

    normalize_and_deduplicate(&mut courses);

    with_state(|state| {
        state.cached_courses = courses.clone();
        state.cached_ab_tag = ab_tag.clone();
        state.cached_vela_version = vela_version;
        state.subscribed = true;
        state.status = format!("已从手表同步 {} 节课程", state.cached_courses.len());
    });

    Ok(InterconnectHandleResult::Timetable(InterconnectResult {
        courses,
        ab_tag,
    }))
}

pub fn handle_timer_event(payload: &str) -> Option<InterconnectHandleResult> {
    let parsed: serde_json::Value = serde_json::from_str(payload).ok()?;
    let obj = parsed.as_object()?;
    if obj.get("type").and_then(|v| v.as_str()) != Some("varclassChunkTimeout") {
        return None;
    }
    let session_id = obj.get("sessionId")?.as_str()?.trim().to_string();
    let nonce = obj.get("nonce")?.as_u64()?;

    let decision = with_state(|state| {
        let Some(session) = state.outgoing_chunk.as_mut() else {
            return None;
        };
        if session.session_id != session_id || session.step_nonce != nonce {
            return None;
        }
        if session.step_send_count >= CHUNK_STEP_MAX_SENDS {
            let message = format!(
                "分片同步超时: 阶段 {} 已发送 {} 次仍无响应",
                session.phase.as_str(),
                session.step_send_count
            );
            state.outgoing_chunk = None;
            state.status = message.clone();
            return Some((message, true, false));
        }
        let message = format!("分片同步超时: 重试阶段 {}", session.phase.as_str());
        session.step_nonce = session.step_nonce.saturating_add(1);
        state.status = message.clone();
        Some((message, false, true))
    })?;

    if decision.2 {
        spawn_send_outgoing_current_step(session_id);
    }

    Some(InterconnectHandleResult::Control {
        message: decision.0,
        is_error: decision.1,
    })
}

/// 获取从手表 interconnect 回包中解析的 Vela 版本（versionName），若无则返回 None。
pub fn get_cached_vela_version() -> Option<String> {
    with_state(|state| {
        state
            .cached_vela_version
            .as_ref()
            .map(|(name, _)| name.clone())
    })
}

fn find_typed_payload(raw_payload: &str, accepted_types: &[&str]) -> Option<serde_json::Value> {
    let parsed: serde_json::Value = serde_json::from_str(raw_payload).ok()?;
    let mut stack = vec![parsed];

    while let Some(current) = stack.pop() {
        if let Some(object) = current.as_object() {
            if let Some(t) = object.get("type").and_then(|v| v.as_str()) {
                if accepted_types.iter().any(|accepted| *accepted == t) {
                    return Some(current);
                }
            }
            for value in object.values() {
                stack.push(value.clone());
                if let Some(text) = value.as_str() {
                    if let Ok(next) = serde_json::from_str::<serde_json::Value>(text) {
                        stack.push(next);
                    }
                }
            }
        } else if let Some(array) = current.as_array() {
            for value in array {
                stack.push(value.clone());
            }
        } else if let Some(text) = current.as_str() {
            if let Ok(next) = serde_json::from_str::<serde_json::Value>(text) {
                stack.push(next);
            }
        }
    }
    None
}

fn find_generic_chunk_payload(raw_payload: &str) -> Option<serde_json::Value> {
    let parsed: serde_json::Value = serde_json::from_str(raw_payload).ok()?;
    let mut stack = vec![parsed];

    while let Some(current) = stack.pop() {
        if let Some(object) = current.as_object() {
            if object.get("__c").and_then(|v| v.as_bool()).unwrap_or(false) {
                return Some(current);
            }
            for value in object.values() {
                stack.push(value.clone());
                if let Some(text) = value.as_str() {
                    if let Ok(next) = serde_json::from_str::<serde_json::Value>(text) {
                        stack.push(next);
                    }
                }
            }
        } else if let Some(text) = current.as_str() {
            if let Ok(next) = serde_json::from_str::<serde_json::Value>(text) {
                stack.push(next);
            }
        }
    }
    None
}

fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buffer = 0_u32;
    let mut bits = 0_u8;
    for ch in input.chars().filter(|c| !c.is_whitespace()) {
        if ch == '=' {
            break;
        }
        let value = match ch {
            'A'..='Z' => ch as u32 - 'A' as u32,
            'a'..='z' => ch as u32 - 'a' as u32 + 26,
            '0'..='9' => ch as u32 - '0' as u32 + 52,
            '+' => 62,
            '/' => 63,
            _ => return None,
        };
        buffer = (buffer << 6) | value;
        bits += 6;
        while bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

fn try_reassemble_generic_chunk(raw_payload: &str) -> Option<Option<String>> {
    let chunk = find_generic_chunk_payload(raw_payload)?;
    let obj = chunk.as_object()?;
    let id = obj.get("id")?.as_str()?.trim().to_string();
    let index = obj.get("i")?.as_u64()? as usize;
    let total = obj.get("t")?.as_u64()? as usize;
    let data = obj.get("d")?.as_str()?.to_string();
    if id.is_empty() || total == 0 || index >= total {
        return Some(None);
    }
    let encoding = obj.get("e").and_then(|v| v.as_str()).map(|s| s.to_string());

    let completed = with_state(|state| {
        let buffer =
            state
                .incoming_chunks
                .entry(id.clone())
                .or_insert_with(|| IncomingChunkBuffer {
                    total,
                    parts: vec![None; total],
                    received: 0,
                    encoding: encoding.clone(),
                });

        if buffer.total != total {
            *buffer = IncomingChunkBuffer {
                total,
                parts: vec![None; total],
                received: 0,
                encoding: encoding.clone(),
            };
        }
        if buffer.parts[index].is_none() {
            buffer.parts[index] = Some(data);
            buffer.received += 1;
        }
        if buffer.received != buffer.total {
            return None;
        }
        let joined = buffer
            .parts
            .iter()
            .filter_map(|part| part.as_deref())
            .collect::<String>();
        let encoding = buffer.encoding.clone();
        state.incoming_chunks.remove(&id);
        Some((joined, encoding))
    });

    let Some((joined, encoding)) = completed else {
        return Some(None);
    };
    if encoding.as_deref() == Some("b64") {
        let Some(bytes) = base64_decode(&joined) else {
            return Some(None);
        };
        return Some(String::from_utf8(bytes).ok());
    }
    Some(Some(joined))
}

fn retry_outgoing_current_step(session_id: &str, reason: &str) -> Option<(String, bool)> {
    let decision = with_state(|state| {
        let Some(session) = state.outgoing_chunk.as_mut() else {
            return None;
        };
        if session.session_id != session_id {
            return None;
        }
        if session.step_send_count >= CHUNK_STEP_MAX_SENDS {
            let message = format!(
                "分片同步失败: {}，阶段 {} 已重试 {} 次",
                reason,
                session.phase.as_str(),
                session.step_send_count
            );
            state.outgoing_chunk = None;
            state.status = message.clone();
            return Some((message, true, false));
        }
        let message = format!("分片同步: {}，重试阶段 {}", reason, session.phase.as_str());
        session.step_nonce = session.step_nonce.saturating_add(1);
        state.status = message.clone();
        Some((message, false, true))
    })?;
    if decision.2 {
        spawn_send_outgoing_current_step(session_id.to_string());
    }
    Some((decision.0, decision.1))
}

fn restart_outgoing_chunk_session(session_id: &str, reason: &str) -> Option<(String, bool)> {
    let decision = with_state(|state| {
        let Some(session) = state.outgoing_chunk.as_mut() else {
            return None;
        };
        if session.session_id != session_id {
            return None;
        }
        if session.session_retry >= CHUNK_SESSION_MAX_RETRY {
            let message = format!("分片同步失败: {}，整轮重试次数已用尽", reason);
            state.outgoing_chunk = None;
            state.status = message.clone();
            return Some((message, true, false));
        }

        session.session_retry += 1;
        session.phase = OutgoingChunkPhase::WaitStartAck;
        session.current_index = 0;
        session.step_send_count = 0;
        session.step_nonce = session.step_nonce.saturating_add(1);
        let message = format!(
            "分片同步: {}，重新发起整轮发送（第 {} 次）",
            reason, session.session_retry
        );
        state.status = message.clone();
        Some((message, false, true))
    })?;
    if decision.2 {
        spawn_send_outgoing_current_step(session_id.to_string());
    }
    Some((decision.0, decision.1))
}

fn handle_outgoing_chunk_ack(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Option<(String, bool)> {
    let session_id = obj.get("sessionId")?.as_str()?.trim().to_string();
    let phase = obj.get("phase").and_then(|v| v.as_str()).unwrap_or("");
    let ok = obj.get("ok").and_then(|v| v.as_bool()).unwrap_or(true);
    if !ok {
        let message = obj
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("手环返回 ACK 失败");
        if message.contains("会话不存在") || message.contains("重新发起") {
            return restart_outgoing_chunk_session(&session_id, message);
        }
        return retry_outgoing_current_step(&session_id, message);
    }

    let action = with_state(|state| {
        let Some(session) = state.outgoing_chunk.as_mut() else {
            return None;
        };
        if session.session_id != session_id {
            return None;
        }
        match phase {
            "start" if session.phase == OutgoingChunkPhase::WaitStartAck => {
                session.step_nonce = session.step_nonce.saturating_add(1);
                session.phase = OutgoingChunkPhase::WaitPartAck;
                session.current_index = 0;
                session.step_send_count = 0;
                let message = format!(
                    "分片同步: 手环已确认开始，准备发送 1/{}",
                    session.chunks.len()
                );
                state.status = message.clone();
                Some((message, false, true))
            }
            "part" if session.phase == OutgoingChunkPhase::WaitPartAck => {
                let index = obj
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(u64::MAX) as usize;
                if index != session.current_index {
                    let message = format!(
                        "分片同步: ACK 分片序号不匹配，期望 {}，收到 {}",
                        session.current_index, index
                    );
                    state.status = message.clone();
                    return Some((message, true, false));
                }
                let next = index + 1;
                session.step_nonce = session.step_nonce.saturating_add(1);
                session.step_send_count = 0;
                if next < session.chunks.len() {
                    session.current_index = next;
                    let message = format!(
                        "分片同步: 手环已确认 {}/{}，准备发送下一片",
                        next,
                        session.chunks.len()
                    );
                    state.status = message.clone();
                    Some((message, false, true))
                } else {
                    session.phase = OutgoingChunkPhase::WaitFinishResult;
                    let message = "分片同步: 所有分片已确认，准备发送完成请求".to_string();
                    state.status = message.clone();
                    Some((message, false, true))
                }
            }
            _ => {
                let message = format!("分片同步: 忽略不匹配 ACK phase={}", phase);
                state.status = message.clone();
                Some((message, false, false))
            }
        }
    })?;
    if action.2 {
        spawn_send_outgoing_current_step(session_id);
    }
    Some((action.0, action.1))
}

fn handle_outgoing_chunk_result(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Option<(String, bool)> {
    let session_id = obj.get("sessionId")?.as_str()?.trim().to_string();
    let success = obj
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let count = obj.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
    let raw_message = obj.get("message").and_then(|v| v.as_str()).unwrap_or("");

    if success {
        let message = with_state(|state| {
            if state
                .outgoing_chunk
                .as_ref()
                .map(|session| session.session_id.as_str() == session_id)
                .unwrap_or(false)
            {
                state.outgoing_chunk = None;
            }
            let message = format!("手环分片导入完成，共 {} 节课程", count);
            state.status = message.clone();
            message
        });
        return Some((message, false));
    }

    let action = with_state(|state| {
        let Some(session) = state.outgoing_chunk.as_mut() else {
            return None;
        };
        if session.session_id != session_id {
            return None;
        }
        if session.session_retry >= CHUNK_SESSION_MAX_RETRY {
            let detail = if raw_message.is_empty() {
                "未知错误"
            } else {
                raw_message
            };
            let message = format!("手环分片导入失败: {}", detail);
            state.outgoing_chunk = None;
            state.status = message.clone();
            return Some((message, true, false));
        }
        session.session_retry += 1;
        session.phase = OutgoingChunkPhase::WaitStartAck;
        session.current_index = 0;
        session.step_send_count = 0;
        session.step_nonce = session.step_nonce.saturating_add(1);
        let message = format!(
            "手环解析失败，重新整轮发送（第 {} 次）",
            session.session_retry
        );
        state.status = message.clone();
        Some((message, false, true))
    })?;
    if action.2 {
        spawn_send_outgoing_current_step(session_id);
    }
    Some((action.0, action.1))
}

fn handle_protocol_status_message(raw_payload: &str) -> Option<(String, bool)> {
    let root = find_typed_payload(raw_payload, &["timetableStatus", "logsStatus", "error"])?;
    let obj = root.as_object()?;
    let msg_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or_default();

    match msg_type {
        "timetableStatus" => {
            let phase = obj.get("phase").and_then(|v| v.as_str()).unwrap_or("");
            let message = match phase {
                "start" => "手环已收到请求，正在准备课程数据".to_string(),
                "" => "手环正在处理课程同步请求".to_string(),
                other => format!("手环课程同步状态: {}", other),
            };
            with_state(|state| {
                state.status = message.clone();
            });
            Some((message, false))
        }
        "logsStatus" => {
            let phase = obj.get("phase").and_then(|v| v.as_str()).unwrap_or("");
            let message = match phase {
                "start" => "手环已收到日志请求，正在准备日志".to_string(),
                "" => "手环正在处理日志请求".to_string(),
                other => format!("手环日志同步状态: {}", other),
            };
            with_state(|state| {
                state.status = message.clone();
            });
            Some((message, false))
        }
        "error" => {
            let code = json_string(obj, &["code"]).unwrap_or_else(|| "UNKNOWN".to_string());
            let detail = json_string(obj, &["message", "msg", "error"])
                .unwrap_or_else(|| "手环返回错误".to_string());
            let message = if code.is_empty() {
                detail
            } else {
                format!("手环返回错误({}): {}", code, detail)
            };
            with_state(|state| {
                state.status = message.clone();
            });
            Some((message, true))
        }
        _ => None,
    }
}

fn handle_chunk_control_message(raw_payload: &str) -> Option<(String, bool)> {
    let root = find_typed_payload(
        raw_payload,
        &["pushTimetableChunkAck", "pushTimetableChunkResult"],
    )?;
    let obj = root.as_object()?;
    let msg_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or_default();
    match msg_type {
        "pushTimetableChunkAck" => {
            if let Some(result) = handle_outgoing_chunk_ack(obj) {
                return Some(result);
            }
            let phase = obj.get("phase").and_then(|v| v.as_str()).unwrap_or("");
            let ok = obj.get("ok").and_then(|v| v.as_bool()).unwrap_or(true);
            let message = if phase == "part" {
                let received = obj.get("received").and_then(|v| v.as_u64()).unwrap_or(0);
                let total = obj.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
                format!("手环已确认分片 {}/{}", received, total)
            } else {
                format!("手环已确认分片阶段: {}", phase)
            };
            with_state(|state| {
                state.status = message.clone();
            });
            Some((message, !ok))
        }
        "pushTimetableChunkResult" => {
            if let Some(result) = handle_outgoing_chunk_result(obj) {
                return Some(result);
            }
            let success = obj
                .get("success")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let count = obj.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
            let raw_message = obj.get("message").and_then(|v| v.as_str()).unwrap_or("");
            let message = if success {
                format!("手环分片导入完成，共 {} 节课程", count)
            } else if raw_message.is_empty() {
                "手环分片导入失败".to_string()
            } else {
                format!("手环分片导入失败: {}", raw_message)
            };
            with_state(|state| {
                state.status = message.clone();
            });
            Some((message, !success))
        }
        _ => None,
    }
}

fn json_string(obj: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = obj.get(*key) {
            if let Some(s) = value.as_str() {
                let s = s.trim();
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            } else if value.is_number() || value.is_boolean() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn json_u32(obj: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> Option<u32> {
    for key in keys {
        if let Some(value) = obj.get(*key) {
            if let Some(n) = value.as_u64() {
                return Some(n as u32);
            }
            if let Some(n) = value.as_i64() {
                if n >= 0 {
                    return Some(n as u32);
                }
            }
            if let Some(s) = value.as_str() {
                if let Ok(n) = s.trim().parse::<u32>() {
                    return Some(n);
                }
            }
        }
    }
    None
}

fn json_day(obj: &serde_json::Map<String, serde_json::Value>) -> Option<u8> {
    if let Some(day) = json_u32(obj, &["day", "weekday", "weekDay", "dayOfWeek"]) {
        if (1..=7).contains(&day) {
            return Some(day as u8);
        }
    }
    let raw = json_string(obj, &["day", "weekday", "weekDay", "dayOfWeek"])?;
    let lower = raw.trim().to_lowercase();
    match lower.as_str() {
        "mon" | "monday" | "周一" | "星期一" => Some(1),
        "tue" | "tuesday" | "周二" | "星期二" => Some(2),
        "wed" | "wednesday" | "周三" | "星期三" => Some(3),
        "thu" | "thursday" | "周四" | "星期四" => Some(4),
        "fri" | "friday" | "周五" | "星期五" => Some(5),
        "sat" | "saturday" | "周六" | "星期六" => Some(6),
        "sun" | "sunday" | "周日" | "周天" | "星期日" | "星期天" => Some(7),
        _ => None,
    }
}

fn parse_weeks_value(value: Option<&serde_json::Value>) -> Vec<u32> {
    let Some(value) = value else {
        return Vec::new();
    };
    if let Some(arr) = value.as_array() {
        let mut weeks: Vec<u32> = arr
            .iter()
            .flat_map(|v| {
                if let Some(n) = v.as_u64() {
                    vec![n as u32]
                } else if let Some(s) = v.as_str() {
                    Course::parse_weeks_text(s)
                } else {
                    Vec::new()
                }
            })
            .filter(|&n| n > 0)
            .collect();
        weeks.sort();
        weeks.dedup();
        return weeks;
    }
    if let Some(s) = value.as_str() {
        return Course::parse_weeks_text(s);
    }
    Vec::new()
}

fn parse_week_mode_from_object(obj: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    normalize_week_mode(
        obj.get("weekMode")
            .or_else(|| obj.get("week_mode"))
            .and_then(|v| v.as_str()),
    )
}

fn parse_ab_tag(
    obj: &serde_json::Map<String, serde_json::Value>,
    week_mode: Option<String>,
) -> Option<AbTagForApply> {
    let top_semester_start = json_date(obj, &["semesterStart", "semester_start"]);
    let tag_obj = obj
        .get("abTag")
        .or_else(|| obj.get("ab_tag"))
        .and_then(|tag| tag.as_object());

    let tag_week_mode = tag_obj.and_then(parse_week_mode_from_object);
    let week_mode = week_mode.or(tag_week_mode);
    let semester_start = tag_obj
        .and_then(|tag| json_date(tag, &["semesterStart", "semester_start"]))
        .or(top_semester_start);

    let ref_date = tag_obj
        .and_then(|tag| json_string(tag, &["refDate", "ref_date"]))
        .unwrap_or_default();
    let ref_type = Course::normalize_week_type(
        tag_obj
            .and_then(|tag| json_string(tag, &["refType", "ref_type"]))
            .as_deref(),
    );
    let ref_day = tag_obj
        .and_then(|tag| json_u32(tag, &["refDay", "ref_day"]))
        .filter(|day| (1..=7).contains(day))
        .unwrap_or(0) as u8;

    let has_ab_ref =
        is_valid_date(&ref_date) && (ref_type == Course::WEEK_A || ref_type == Course::WEEK_B);
    if !has_ab_ref && semester_start.is_none() && week_mode.is_none() {
        return None;
    }

    Some(AbTagForApply {
        ref_date: if has_ab_ref { ref_date } else { String::new() },
        ref_type: if has_ab_ref { ref_type } else { String::new() },
        ref_day: if has_ab_ref { ref_day } else { 0 },
        semester_start,
        week_mode,
    })
}

/// 从 JSON payload 解析课表数据，支持 classes/data 字段及多种字段命名
fn parse_timetable_data(
    payload: &str,
) -> Option<(
    Vec<Course>,
    Option<AbTagForApply>,
    Option<(String, u32)>,
    Option<String>,
)> {
    let root = find_timetable_payload(payload)?;
    let obj = root.as_object();

    let classes_arr = if let Some(arr) = root.as_array() {
        Some(arr)
    } else if let Some(obj) = obj {
        [
            "classes",
            "data",
            "courses",
            "items",
            "list",
            "timetable",
            "schedule",
        ]
        .iter()
        .find_map(|key| obj.get(*key).and_then(|v| v.as_array()))
    } else {
        None
    }?;
    let mut courses = Vec::new();

    for class_item in classes_arr {
        let Some(class_obj) = class_item.as_object() else {
            continue;
        };

        let Some(day) = json_day(class_obj) else {
            continue;
        };
        let Some(display_name) = json_string(
            class_obj,
            &[
                "name",
                "courseName",
                "course_name",
                "subject",
                "title",
                "summary",
            ],
        ) else {
            continue;
        };
        let Some(start) = json_string(
            class_obj,
            &[
                "start",
                "startTime",
                "start_time",
                "startSection",
                "startNode",
            ],
        ) else {
            continue;
        };
        let Some(end) = json_string(
            class_obj,
            &["end", "endTime", "end_time", "endSection", "endNode"],
        ) else {
            continue;
        };

        let room_field = json_string(
            class_obj,
            &["room", "classroom", "location", "place", "address"],
        )
        .unwrap_or_default();
        let (name, room_from_name) = Course::split_name_and_room(&display_name);
        let room = if room_field.is_empty() {
            room_from_name
        } else {
            room_field
        };
        let week_type = Course::normalize_week_type(
            class_obj
                .get("weekType")
                .or_else(|| class_obj.get("week_type"))
                .or_else(|| class_obj.get("weeksType"))
                .or_else(|| class_obj.get("week"))
                .and_then(|v| v.as_str()),
        );

        let mut weeks = parse_weeks_value(
            class_obj
                .get("weeks")
                .or_else(|| class_obj.get("weekList"))
                .or_else(|| class_obj.get("week_list")),
        );
        if weeks.is_empty() {
            if let (Some(start_week), Some(end_week)) = (
                json_u32(class_obj, &["startWeek"]),
                json_u32(class_obj, &["endWeek"]),
            ) {
                weeks = Course::weeks_from_range(start_week, end_week, &week_type);
            }
        }

        if (1..=7).contains(&day) && !name.is_empty() && !start.is_empty() && !end.is_empty() {
            courses.push(Course {
                day,
                name,
                start,
                end,
                room,
                week_type,
                weeks,
            });
        }
    }

    let (vela_version, week_mode, ab_tag) = if let Some(obj) = obj {
        let vela_version = obj.get("versionName").and_then(|v| v.as_str()).map(|s| {
            let vc = obj.get("versionCode").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            (s.to_string(), vc)
        });

        let week_mode = parse_week_mode_from_object(obj).or_else(|| {
            obj.get("abTag")
                .or_else(|| obj.get("ab_tag"))
                .and_then(|tag| tag.as_object())
                .and_then(parse_week_mode_from_object)
        });

        let ab_tag = parse_ab_tag(obj, week_mode.clone());

        (vela_version, week_mode, ab_tag)
    } else {
        (None, None, None)
    };

    Some((courses, ab_tag, vela_version, week_mode))
}

/// 在嵌套 JSON 中递归查找包含课程数组的对象（兼容 payloadText、payload 等包装）
fn find_timetable_payload(raw_payload: &str) -> Option<serde_json::Value> {
    let parsed: serde_json::Value = serde_json::from_str(raw_payload).ok()?;
    let mut stack = vec![parsed];

    while let Some(current) = stack.pop() {
        if let Some(array) = current.as_array() {
            if array.iter().any(|item| item.as_object().is_some()) {
                return Some(current);
            }
            for value in array {
                stack.push(value.clone());
            }
            continue;
        }
        if let Some(object) = current.as_object() {
            // 宽松匹配：只要包含课程数组就认为是有效课表数据
            if [
                "classes",
                "data",
                "courses",
                "items",
                "list",
                "timetable",
                "schedule",
            ]
            .iter()
            .any(|key| object.get(*key).and_then(|v| v.as_array()).is_some())
            {
                return Some(current);
            }

            for key in [
                "data",
                "payload",
                "payloadText",
                "payload_text",
                "eventPayload",
                "event_payload",
                "message",
                "content",
                "body",
                "result",
            ] {
                if let Some(value) = object.get(key) {
                    stack.push(value.clone());
                    if let Some(text) = value.as_str() {
                        if let Ok(next) = serde_json::from_str::<serde_json::Value>(text) {
                            stack.push(next);
                        }
                    }
                }
            }
        } else if let Some(text) = current.as_str() {
            if let Ok(next) = serde_json::from_str::<serde_json::Value>(text) {
                stack.push(next);
            }
        }
    }

    None
}

fn normalize_and_deduplicate(courses: &mut Vec<Course>) {
    use std::collections::HashSet;

    let mut seen = HashSet::new();
    courses.retain(|course| {
        let weeks_key: Vec<u32> = course.weeks.iter().copied().collect();
        let key = (
            course.day,
            course.name.trim().to_string(),
            course.start.trim().to_string(),
            course.end.trim().to_string(),
            course.room.trim().to_string(),
            Course::normalize_week_type(Some(&course.week_type)),
            weeks_key,
        );
        seen.insert(key)
    });

    courses.sort_by(|a, b| {
        a.day
            .cmp(&b.day)
            .then_with(|| a.start.cmp(&b.start))
            .then_with(|| a.end.cmp(&b.end))
            .then_with(|| a.name.cmp(&b.name))
    });
}

pub fn get_cached_courses() -> Vec<Course> {
    with_state(|state| state.cached_courses.clone())
}

pub fn update_course(course: Course, index: Option<usize>) {
    with_state(|state| {
        if let Some(idx) = index {
            if idx < state.cached_courses.len() {
                state.cached_courses[idx] = course;
            }
        } else {
            state.cached_courses.push(course);
        }
        normalize_and_deduplicate(&mut state.cached_courses);
    });
}

pub fn delete_course(index: usize) {
    with_state(|state| {
        if index < state.cached_courses.len() {
            state.cached_courses.remove(index);
        }
    });
}

fn do_import(mut courses: Vec<Course>, ab_tag: Option<AbTagForApply>) -> usize {
    normalize_and_deduplicate(&mut courses);
    let count = courses.len();
    with_state(|state| {
        state.cached_courses = courses;
        state.cached_ab_tag = ab_tag;
        state.status = format!("已导入 {} 节课程", count);
    });
    count
}

fn import_tag_for_courses(
    courses: &[Course],
    semester_start: Option<String>,
) -> Option<AbTagForApply> {
    let has_weeks = courses.iter().any(|course| !course.weeks.is_empty());
    let semester_start = semester_start.filter(|value| is_valid_date(value));
    if semester_start.is_none() {
        return None;
    }
    Some(AbTagForApply {
        ref_date: String::new(),
        ref_type: String::new(),
        ref_day: 0,
        semester_start,
        week_mode: Some(if has_weeks { "multi" } else { "none" }.to_string()),
    })
}

/// 按指定格式导入课表；格式失败时按内容特征自动尝试 CSES/WakeUp/JSON
pub fn import_with_format(text: &str, format: &str) -> Result<usize, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("请输入要导入的内容".to_string());
    }

    let try_format = |fmt: &str| -> Result<usize, String> {
        match fmt {
            "cses" => {
                let courses = crate::cses::import_from_cses(trimmed)?;
                Ok(do_import(courses, None))
            }
            "class_island" => {
                let courses = crate::class_island::import_from_class_island(trimmed)?;
                Ok(do_import(courses, None))
            }
            "wakeup" => {
                let courses = crate::wakeup::import_from_wakeup(trimmed)?;
                Ok(do_import(courses, None))
            }
            "ics" => {
                let (courses, semester_start) =
                    crate::ics::import_from_ics_with_meta(trimmed, None)?;
                let tag = import_tag_for_courses(&courses, semester_start);
                Ok(do_import(courses, tag))
            }
            _ => import_from_json(trimmed),
        }
    };

    // 先按用户选择的格式尝试
    if let Ok(n) = try_format(format.trim().to_lowercase().as_str()) {
        return Ok(n);
    }

    // 选择格式失败时，按内容特征自动尝试
    if trimmed.contains("BEGIN:VCALENDAR") || trimmed.contains("BEGIN:VEVENT") {
        if let Ok(n) = try_format("ics") {
            return Ok(n);
        }
    }
    // CSES: YAML，首行 version: 1，含 subjects/schedules
    if (trimmed.starts_with("version") || trimmed.contains("\nversion"))
        && trimmed.contains("subjects")
        && trimmed.contains("schedules")
    {
        if let Ok(n) = try_format("cses") {
            return Ok(n);
        }
    }
    // WakeUp: 5 段 JSON，首段含 courseLen，第二段为节次表含 node/startTime
    let first_line = trimmed.lines().next().unwrap_or("");
    let blocks: Vec<&str> = trimmed
        .lines()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    let looks_wakeup = blocks.len() >= 5
        && (first_line.contains("courseLen")
            || blocks
                .get(1)
                .map(|b| b.contains("startTime") && b.contains("node"))
                .unwrap_or(false));
    if looks_wakeup {
        if let Ok(n) = try_format("wakeup") {
            return Ok(n);
        }
    }
    // JSON: 以 { 或 [ 开头的单段 JSON
    let first_char = trimmed.chars().next().unwrap_or(' ');
    if first_char == '{' || first_char == '[' {
        if let Ok(n) = try_format("json") {
            return Ok(n);
        }
    }

    Err("无法解析：请确认格式选择正确（JSON/CSES/Class Island/WakeUp/iCalendar），或检查内容是否完整".to_string())
}

pub fn import_from_json(text: &str) -> Result<usize, String> {
    if let Some((courses, ab_tag, _, _)) = parse_timetable_data(text) {
        Ok(do_import(courses, ab_tag))
    } else {
        // 尝试解析为简单课程数组（非 timetableData 格式）
        if let Ok(courses_arr) = serde_json::from_str::<Vec<Course>>(text) {
            Ok(do_import(courses_arr, None))
        } else {
            Err("无法解析课程数据，请确保格式正确".to_string())
        }
    }
}
