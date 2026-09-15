//! 甜蜜女友2 导入插件（WASM 组件本体）。
//!
//! 结构分三块，**宿主相关的部分全部条件编译**，为的是让存档逻辑能在宿主机上测：
//!
//! - crate 根：与宿主无关的纯逻辑 `saves`（存档信封、槽位映射、文件名）；
//! - `#[cfg(target_arch = "wasm32")] mod host { … }`：State、章节包传输、界面分派、
//!   生命周期、剪贴板 —— 这些依赖 `astrobox-ng-wit`，宿主机上没有；
//! - 文件末尾 `#[cfg(test)]`：在宿主机上跑 `cargo test -p amakano2-import --target
//!   x86_64-pc-windows-msvc`，把「导入一个坏文件会怎样」这类问题在本地测掉。
//!
//! 插件本体是 wasm 组件，宿主机上编不过 —— 这就是为什么纯逻辑必须与宿主代码分开住。

// 文件放在 `src/host/` 下只是**目录习惯**（其余模块都在那里），模块本身是
// 宿主机无关的纯逻辑，所以挂在 crate 根上、不条件编译 —— 宿主机上跑单元测试靠它。
#[path = "host/saves.rs"]
mod saves;

// 请求通道的纯逻辑（请求种类、按种类的超时策略、单坑位）。同样不条件编译：
// 「存档的超时该比章节列表宽」「迟到的回包不能算超时」都要在宿主机上有守护用例。
mod request;

// ---- 两个半边都要用到的常量 ----
//
// `SAVE_PROTOCOL` / `MAX_SAVE_SLOTS` 同时被 `host`（hello 协商、导入截断）与
// **单元测试**读，所以挂在这里；只在宿主侧用得到的常量（包名、章节包清单路径）
// 就留在 `host` 模块里 —— 否则宿主 target 上会报「never used」。
const MAX_SAVE_SLOTS: usize = 20;
/// 与手环协商的存档协议版本。手环在 `amakano.app.hello-ok` 里回它自己的版本，
/// **缺席就等于「手环端应用太旧」**，界面上要给人话而不是超时。
const SAVE_PROTOCOL: u32 = 1;

// ------------------------------------------------------------------ 宿主侧实现

#[cfg(target_arch = "wasm32")]
mod host {
use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Read};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use amakano2_ui::{
    Action, CHUNK_OPTIONS, DeviceView, InstalledView, Limits, LogFilter, PackView, Page,
    PRESS_TIMEOUT_MS, ReadingStatsView, RefreshPlan, RefreshStep, ResumeView, SaveExportView,
    SaveImportView, Snapshot, StatusKind, TransferView, band_envelope, chunk_label, human_bytes,
    parse_action, refresh_steps,
};
use astrobox_ng_wit::FutureReader;
use astrobox_ng_wit::astrobox::psys_host;
use astrobox_ng_wit::exports::astrobox::psys_plugin::{
    event::{self, EventType},
    lifecycle,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use zip::ZipArchive;

use crate::request::{FOLLOWUP_DELAY_MS, PendingRequest, RequestKind, Slot, TimeoutAction};
use crate::saves;
use crate::saves::now_ms;
use crate::{MAX_SAVE_SLOTS, SAVE_PROTOCOL};

// 宿主专属的两个模块：界面转换与剪贴板（都碰 `astrobox_ng_wit`）。
//
// `host/dialog.rs` **整个文件已经删除**：Dialog 这一类需要用户交互的宿主调用
// 在这个宿主上永远不返回、会把事件分发器堵死（真机证据见
// docs/插件开发注意事项.md 第 7 节）。导出/导入改走剪贴板，别再往回走。
mod clipboard;
mod logger;
mod render;

/// 手环上《甜蜜女友2》的包名（interconnect 收发都用它）。
const PACKAGE_NAME: &str = "cn.amakanotwo.qihe";
/// 章节包随插件一起分发：`packs/index.json` 是清单，`packs/*.pack` 是包体。
const LIBRARY_DIR: &str = "packs";
const LIBRARY_INDEX: &str = "index.json";
const DEFAULT_CHUNK_BYTES: usize = 8192;
const MAX_PACK_BYTES: usize = 20_000_000;
const RETRY_DELAY_MS: u64 = 1500;
const MAX_RETRIES: u8 = 4;
const INITIAL_WINDOW: usize = 3;
const MIN_TIMEOUT_MS: u64 = 800;
const MAX_TIMEOUT_MS: u64 = 4000;
const APP_START_DELAY_MS: u64 = 2000;
const READY_TIMEOUT_MS: u64 = 3000;
const MAX_READY_ATTEMPTS: u8 = 2;
/// 连接后轮询手环应用：每 2.5 秒试一次，最多 12 次（30 秒内手动打开也能接上）。
const PROBE_INTERVAL_MS: u64 = 2500;
const MAX_PROBE_ATTEMPTS: u32 = 12;
/// 一次存档回包最多允许多少片（手环侧是 9000 字节/片，这个上限远大于真实需要，
/// 只是为了防止一个乱报 `seq` 的回包把内存吃掉）。
const MAX_SAVE_SHARDS: u64 = 64;

struct PackFile {
    path: String,
    bytes: Vec<u8>,
}

#[derive(Clone)]
struct PackMeta {
    name: String,
    pack_id: String,
    chapter_number: usize,
    chapter_name: String,
    file_count: usize,
    total_bytes: usize,
}

struct TransferChunk {
    path: String,
    append: bool,
    end: bool,
    data: String,
    bytes: usize,
}

struct Transfer {
    request_id: String,
    chunk_bytes: usize,
    chunks: Vec<TransferChunk>,
    next_index: usize,
    in_flight: Vec<(usize, u128)>,
    window_size: usize,
    acked_bytes: usize,
    speed_kbps: f64,
    speed_window_start: u128,
    speed_window_bytes: usize,
    rtt_ms: u64,
    retry_count: u8,
    ready: bool,
    started: bool,
    waiting_ready: bool,
    ready_attempts: u8,
    resumed: bool,
}

#[derive(Clone)]
struct InstalledPack {
    id: String,
    name: String,
    chapter_number: usize,
    bytes: usize,
    files: usize,
}

#[derive(Clone)]
struct ResumeInfo {
    pack_id: String,
    chapter_name: String,
    bytes: usize,
    chunks: usize,
    resume_from: usize,
    received_bytes: usize,
    files_done: usize,
}

/// 随插件一起分发的章节包（`packs/index.json` 的一条记录）。
#[derive(Clone)]
struct LibraryPack {
    file: String,
    id: String,
    number: usize,
    title: String,
    bytes: usize,
    minutes: usize,
    scenes: usize,
    dialogues: usize,
}

struct State {
    element_id: Option<String>,
    version: String,
    device_addr: String,
    device_name: String,
    connected: bool,
    /// 手环应用已经回应过消息（说明《甜蜜女友2》正在前台运行）。
    alive: bool,
    probe_attempt: u32,
    library: Vec<LibraryPack>,
    library_error: String,
    sync_queue: Vec<usize>,
    /// 本次连接是否已经问过未完成传输（连上之后问一次就够）。
    pending_checked: bool,
    status: String,
    status_kind: StatusKind,
    transfer: Option<Transfer>,
    installed: Vec<InstalledPack>,
    cache_bytes: usize,
    cache_files: usize,
    pack_files: Vec<PackFile>,
    pack_meta: Option<PackMeta>,
    chunk_bytes: usize,
    /// 等待回包的请求 —— **只有一个坑位**，语义全在 `request::Slot`（含那条历史结构问题）。
    request: Slot,
    /// 「刷新」里还没发的往返，按 [`RefreshStep`] 的顺序排队。
    ///
    /// 只有一个坑位，所以「刷新」不能把两个请求一起挂出去：得等前一个 settle
    /// （回包 / 失败 / 超时放弃）再发下一个。顺序由 `refresh_steps`（ui-core）判定，
    /// 这里只负责照做 —— 章节列表在前、存档在后，理由见那份实现。
    refresh_queue: Vec<RefreshStep>,
    resume: Option<ResumeInfo>,
    /// 当前界面页。
    page: Page,
    /// 连接成功后自动打开手环应用。
    auto_launch: bool,
    /// 悬停中的元素 id（界面高亮用）。
    hover: Option<String>,
    /// 章节页的线路筛选；`None` 表示全部。
    line_filter: Option<String>,
    /// 日志页的级别筛选。
    log_filter: LogFilter,
    /// 按下中的按钮动作 id + 按下时刻（时刻用于超过 PRESS_TIMEOUT_MS 后兜底复位）。
    pressed: Option<(String, u128)>,
    last_action: String,
    last_action_ms: u128,
    // ---- 存档 ----
    /// 手环上一份**原始**存档对象（自动存档 + 手动槽，`Value` 原样持有，导出时直接带走）。
    ///
    /// 这里是插件侧**唯一的存档来源**：界面上那几行（含「未安装」判定）由
    /// `amakano2_ui::Snapshot::saves()` 在**每次渲染时**用当前已安装章节列表现算。
    ///
    /// ⚠️ 再也别往 `State` 里存「算好的界面行」：那是派生判定，一旦缓存下来，
    /// 已安装章节列表后来变了它也不会跟着变（2026-09 真机：手环上装了第 1 章，
    /// 存档四条全标「未安装」，点「刷新」也不变）。见 docs/插件开发注意事项.md 7.9。
    save_auto: Option<Value>,
    save_slots: Vec<Value>,
    /// 存档通道的失败原因（空串表示没问题）。
    saves_error: String,
    /// 与手环协商到的协议版本；`None` 表示还没收到 `hello-ok`。
    save_protocol: Option<u32>,
    /// 手环端应用**不支持**存档通道（没回 `hello-ok`，或回了但能力/协议不够用）。
    ///
    /// 只是个状态位：界面上「手环端应用版本过旧」那句结论与随后那句「怎么办」
    /// **统一由 ui-core 给**（`Snapshot::saves_blocked_hint` / `saves_blocked_action`）。
    /// 这里刻意**不**往 `saves_error` 里写同义的一句话：那样卡片上会出现两句几乎一样的提示
    /// （用户实机截图就是这么来的），而且一红一黄。
    saves_unsupported: bool,
    /// 手环快应用版本（hello-ok 里带回来的），用于「版本过旧」的提示。
    band_version: String,
    /// 等待二次确认删除的存档槽；`None` 表示没有。自动存档是 `"auto"`。
    confirm_delete_save: Option<String>,
    /// 存档操作进行中（对话框/等待回包），期间禁用存档页的动作按钮。
    saves_busy: bool,
    /// 存档页顶部的一句操作结果。
    saves_notice: String,
    /// 上一次「导出到剪贴板」的结果：`(信封字节数, 槽位条数, 是否读回核对通过)`。
    save_export: Option<(usize, usize, bool)>,
    /// 导入正在等回包时的计数：`(信封槽位条数, 重复条数, 合并前手环槽位条数, 超限被截掉的条数)`。
    /// 收到 `put-ok` 时据此生成「导入 N 个存档，跳过 M 个重复」那句人话。
    save_import: Option<(usize, usize, usize, usize)>,
    // ---- 阅读统计 ----
    /// 手环上读回来的阅读统计（**已经解好的视图**，缺字段时这里是 `None` + `stats_error`）。
    ///
    /// 与存档同一口径：插件只在「收到回包那一刻」解一次，界面行由 `Snapshot` 直接带走；
    /// 这里不放「界面行」，因为统计本来就没有需要跟别的输入联动的派生判定。
    stats: Option<ReadingStatsView>,
    /// 统计通道的失败原因（空串表示没问题）。
    stats_error: String,
    /// 手环端应用能力表里有 `stats`（`hello-ok` 里报的）。
    stats_supported: bool,
    /// 正在读统计：期间把刷新按钮关掉，免得用户连点。
    stats_busy: bool,
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(State {
            element_id: None,
            version: String::new(),
            device_addr: String::new(),
            device_name: String::new(),
            connected: false,
            alive: false,
            probe_attempt: 0,
            library: Vec::new(),
            library_error: String::new(),
            sync_queue: Vec::new(),
            pending_checked: false,
            status: "先点「连接设备」，再在章节列表里点「同步」".into(),
            status_kind: StatusKind::Info,
            transfer: None,
            installed: Vec::new(),
            cache_bytes: 0,
            cache_files: 0,
            pack_files: Vec::new(),
            pack_meta: None,
            chunk_bytes: DEFAULT_CHUNK_BYTES,
            request: Slot::idle(),
            refresh_queue: Vec::new(),
            resume: None,
            page: Page::Overview,
            auto_launch: true,
            hover: None,
            line_filter: None,
            log_filter: LogFilter::All,
            pressed: None,
            last_action: String::new(),
            last_action_ms: 0,
            save_auto: None,
            save_slots: Vec::new(),
            saves_error: String::new(),
            save_protocol: None,
            saves_unsupported: false,
            band_version: String::new(),
            confirm_delete_save: None,
            saves_busy: false,
            saves_notice: String::new(),
            save_export: None,
            save_import: None,
            stats: None,
            stats_error: String::new(),
            stats_supported: false,
            stats_busy: false,
        })
    })
}

/// 改状态。返回闭包的返回值，调用方拿它判断「这次到底改了没有」。
fn update<T>(action: impl FnOnce(&mut State) -> T) -> T {
    action(&mut state().lock().unwrap_or_else(|error| error.into_inner()))
}

fn set_status(kind: StatusKind, text: impl Into<String>) {
    update(|state| {
        state.status_kind = kind;
        state.status = text.into();
    });
}

fn arm_timer(delay_ms: u64, payload: String) {
    astrobox_ng_wit::spawn(async move {
        let _ = psys_host::timer::set_timeout(delay_ms, &payload).await;
    });
}

fn build_chunks(files: &[PackFile], chunk_bytes: usize) -> Vec<TransferChunk> {
    let mut chunks = Vec::new();
    for file in files {
        let total = file.bytes.len().div_ceil(chunk_bytes).max(1);
        for (index, chunk) in file.bytes.chunks(chunk_bytes).enumerate() {
            chunks.push(TransferChunk {
                path: file.path.clone(),
                append: index > 0,
                end: index + 1 == total,
                data: STANDARD.encode(chunk),
                bytes: chunk.len(),
            });
        }
    }
    chunks
}

fn transfer_with_chunks(chunk_bytes: usize, chunks: Vec<TransferChunk>) -> Transfer {
    Transfer {
        request_id: format!("pack-{}", now_ms()),
        chunk_bytes,
        chunks,
        next_index: 0,
        in_flight: Vec::new(),
        window_size: INITIAL_WINDOW,
        acked_bytes: 0,
        speed_kbps: 0.0,
        speed_window_start: now_ms(),
        speed_window_bytes: 0,
        rtt_ms: 0,
        retry_count: 0,
        ready: false,
        started: false,
        waiting_ready: false,
        ready_attempts: 0,
        resumed: false,
    }
}

fn prefix_bytes(chunks: &[TransferChunk], count: usize) -> usize {
    chunks.iter().take(count.min(chunks.len())).map(|chunk| chunk.bytes).sum()
}

fn begin_packet(meta: &PackMeta, transfer: &Transfer) -> String {
    json!({
        "type": "amakano.files.begin",
        "requestId": transfer.request_id,
        "name": meta.name,
        "packId": meta.pack_id,
        "chapterNumber": meta.chapter_number,
        "chapterName": meta.chapter_name,
        "files": meta.file_count,
        "bytes": meta.total_bytes,
        "chunks": transfer.chunks.len(),
    })
    .to_string()
}

/// 找到存放章节清单的目录。
///
/// AstroBox 只允许插件用 std::fs 读自身目录，但沙箱挂在哪个路径没有明确文档，
/// 所以这里依次试 `packs/` 和当前目录，并把失败的现场写进错误信息，
/// 用户直接把状态发回来就能定位。
fn locate_library_dir() -> Result<PathBuf, String> {
    let nested = PathBuf::from(LIBRARY_DIR);
    if nested.join(LIBRARY_INDEX).exists() {
        return Ok(nested);
    }
    let flat = PathBuf::from(".");
    if flat.join(LIBRARY_INDEX).exists() {
        return Ok(flat);
    }
    let cwd = std::env::current_dir().map(|path| path.display().to_string()).unwrap_or_else(|_| "未知".into());
    let visible = fs::read_dir(".")
        .map(|entries| entries.flatten().take(12).map(|entry| entry.file_name().to_string_lossy().into_owned()).collect::<Vec<_>>().join("、"))
        .unwrap_or_else(|_| "无法列目录".into());
    Err(format!("找不到 {LIBRARY_DIR}/{LIBRARY_INDEX}（当前目录 {cwd}，可见文件：{visible}）"))
}

/// 读取随插件分发的章节清单（宿主只允许插件访问自身目录，这里读的就是插件自己的文件）。
fn load_library() -> Result<Vec<LibraryPack>, String> {
    let root = locate_library_dir()?;
    let index_path = root.join(LIBRARY_INDEX);
    let raw = fs::read(&index_path).map_err(|error| format!("读取 {} 失败：{error}", index_path.display()))?;
    let parsed: Value = serde_json::from_slice(&raw).map_err(|error| format!("{} 不是合法 JSON：{error}", index_path.display()))?;
    let entries = parsed.get("packs").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut packs = Vec::new();
    for entry in entries {
        let Some(file) = entry.get("file").and_then(Value::as_str) else { continue };
        let Some(id) = entry.get("id").and_then(Value::as_str) else { continue };
        if !root.join(file).exists() {
            return Err(format!("章节包文件缺失：{file}"));
        }
        packs.push(LibraryPack {
            file: file.into(),
            id: id.into(),
            number: entry.get("number").and_then(Value::as_u64).unwrap_or(0) as usize,
            title: entry.get("title").and_then(Value::as_str).unwrap_or(id).into(),
            bytes: entry.get("bytes").and_then(Value::as_u64).unwrap_or(0) as usize,
            minutes: entry.get("minutes").and_then(Value::as_u64).unwrap_or(0) as usize,
            scenes: entry.get("scenes").and_then(Value::as_u64).unwrap_or(0) as usize,
            dialogues: entry.get("dialogues").and_then(Value::as_u64).unwrap_or(0) as usize,
        });
    }
    if packs.is_empty() {
        return Err("插件内没有章节包".into());
    }
    packs.sort_by_key(|pack| pack.number);
    Ok(packs)
}

fn library_summary(packs: &[LibraryPack]) -> String {
    let bytes: usize = packs.iter().map(|pack| pack.bytes).sum();
    let minutes: usize = packs.iter().map(|pack| pack.minutes).sum();
    let scenes: usize = packs.iter().map(|pack| pack.scenes).sum();
    let dialogues: usize = packs.iter().map(|pack| pack.dialogues).sum();
    format!(
        "内置 {} 章 · {scenes} 场景 · {dialogues} 句对白 · {} · 约 {} 小时 {} 分钟",
        packs.len(),
        human_bytes(bytes),
        minutes / 60,
        minutes % 60
    )
}

fn find_library_pack(number: usize) -> Option<LibraryPack> {
    state()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .library
        .iter()
        .find(|pack| pack.number == number)
        .cloned()
}

/// 把内置章节包读入内存，切成当前分片大小的传输块，然后发 begin。
fn start_embedded_transfer(number: usize) {
    let Some(pack) = find_library_pack(number) else {
        set_status(StatusKind::Bad, format!("找不到第 {number} 章，请检查插件内的章节清单"));
        render();
        return;
    };
    let path = match locate_library_dir() {
        Ok(root) => root.join(&pack.file),
        Err(message) => {
            set_status(StatusKind::Bad, message);
            render();
            return;
        }
    };
    let data = match fs::read(&path) {
        Ok(data) => data,
        Err(error) => {
            set_status(StatusKind::Bad, format!("读取章节包失败：{error}"));
            render();
            return;
        }
    };
    if data.len() > MAX_PACK_BYTES {
        set_status(StatusKind::Bad, format!("{} 体积异常（{}），请重新安装插件", pack.title, human_bytes(data.len())));
        render();
        return;
    }
    let (mut meta, files) = match parse_pack(&data) {
        Ok(value) => value,
        Err(message) => {
            set_status(StatusKind::Bad, format!("{}：{message}", pack.title));
            render();
            return;
        }
    };
    meta.name = pack.title.clone();
    let mut message = String::new();
    let mut kind = StatusKind::Good;
    update(|state| {
        let chunks = build_chunks(&files, state.chunk_bytes);
        let count = chunks.len();
        let matches_resume = state
            .resume
            .as_ref()
            .map(|pending| pending.pack_id == meta.pack_id && pending.bytes == meta.total_bytes && pending.chunks == count)
            .unwrap_or(false);
        state.transfer = Some(transfer_with_chunks(state.chunk_bytes, chunks));
        state.pack_files = files;
        state.pack_meta = Some(meta.clone());
        if matches_resume {
            message = format!("{}（约 {} 分钟）与未完成传输匹配，正在接着传", meta.name, pack.minutes);
        } else {
            if state.resume.is_some() {
                state.resume = None;
                kind = StatusKind::Warn;
            }
            message = format!("正在同步 {}（约 {} 分钟 · {count} 个分片）", meta.name, pack.minutes);
        }
    });
    set_status(kind, message);
    render();
    astrobox_ng_wit::block_on(async { start_transfer().await });
}

/// 依次同步队列里的章节。
fn queue_sync(numbers: Vec<usize>) {
    let first = {
        let mut current = state().lock().unwrap_or_else(|error| error.into_inner());
        current.sync_queue = numbers;
        if current.sync_queue.is_empty() {
            None
        } else {
            Some(current.sync_queue.remove(0))
        }
    };
    match first {
        Some(number) => start_embedded_transfer(number),
        None => {
            set_status(StatusKind::Info, "没有需要同步的章节");
            render();
        }
    }
}

/// 传输完成后取出下一章；返回 None 表示队列已空。
fn pop_sync_queue() -> Option<(usize, usize)> {
    let mut current = state().lock().unwrap_or_else(|error| error.into_inner());
    if current.sync_queue.is_empty() {
        return None;
    }
    let number = current.sync_queue.remove(0);
    Some((number, current.sync_queue.len()))
}

fn parse_pack(data: &[u8]) -> Result<(PackMeta, Vec<PackFile>), String> {
    let mut archive = ZipArchive::new(Cursor::new(data)).map_err(|_| "章节包无法读取".to_string())?;
    let mut meta = PackMeta {
        name: String::new(),
        pack_id: String::new(),
        chapter_number: 0,
        chapter_name: String::new(),
        file_count: 0,
        total_bytes: 0,
    };
    let mut files = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|_| "章节包条目错误".to_string())?;
        if entry.is_dir() {
            continue;
        }
        let path = entry.name().replace('\\', "/");
        if path.starts_with('/') || path.split('/').any(|part| part == ".." || part.is_empty()) {
            return Err("章节包路径无效".into());
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(|_| "章节包读取失败".to_string())?;
        if path == "pack.txt" {
            let manifest: Value = serde_json::from_slice(&bytes).map_err(|_| "章节包清单无效".to_string())?;
            meta.pack_id = manifest.get("packId").and_then(Value::as_str).unwrap_or("").to_string();
            meta.chapter_number = manifest.get("chapterNumber").and_then(Value::as_u64).unwrap_or(0) as usize;
            meta.chapter_name = manifest.get("chapterName").and_then(Value::as_str).unwrap_or("").to_string();
        }
        meta.total_bytes += bytes.len();
        meta.file_count += 1;
        files.push(PackFile { path, bytes });
    }
    if meta.pack_id.is_empty() || meta.chapter_number == 0 || meta.chapter_name.is_empty() || files.is_empty() {
        return Err("章节包元数据无效".into());
    }
    Ok((meta, files))
}

/// 把业务状态拍成渲染用的只读快照。
///
/// 一次性取值、只加一次锁：页面渲染期间不再碰 `State`，
/// 也就不会出现「渲染到一半状态被改掉」这种偶发错乱。
fn snapshot() -> Snapshot {
    let current = state().lock().unwrap_or_else(|error| error.into_inner());

    let library: Vec<PackView> = current
        .library
        .iter()
        .map(|pack| PackView {
            number: pack.number,
            id: pack.id.clone(),
            title: pack.title.clone(),
            minutes: pack.minutes,
            bytes: pack.bytes,
            scenes: pack.scenes,
            dialogues: pack.dialogues,
            installed: current.installed.iter().any(|record| record.id == pack.id),
            queued: current.sync_queue.contains(&pack.number),
            active: current
                .pack_meta
                .as_ref()
                .is_some_and(|meta| meta.chapter_number == pack.number),
        })
        .collect();

    let installed: Vec<InstalledView> = current
        .installed
        .iter()
        .map(|record| InstalledView {
            id: record.id.clone(),
            name: record.name.clone(),
            bytes: record.bytes,
            files: record.files,
            // 插件里已经没有这一章（例如旧版本的 common-01）：界面上要提醒删除。
            stale: !current.library.iter().any(|pack| pack.id == record.id),
        })
        .collect();

    let transfer = current.transfer.as_ref().map(|transfer| {
        let total = current.pack_meta.as_ref().map_or(0, |meta| meta.total_bytes);
        let received = transfer.acked_bytes;
        let percent = if total > 0 {
            ((received as u64 * 100) / total as u64).min(100) as u32
        } else {
            0
        };
        TransferView {
            chapter: current.pack_meta.as_ref().map_or_else(String::new, |meta| meta.chapter_name.clone()),
            percent,
            received,
            total,
            chunks_done: transfer.next_index.saturating_sub(transfer.in_flight.len()),
            chunks_total: transfer.chunks.len(),
            speed_kbps: transfer.speed_kbps,
            rtt_ms: transfer.rtt_ms,
            retries: transfer.retry_count,
            resumed: transfer.resumed,
            ready: transfer.ready,
            started: transfer.started,
            chunk_bytes: transfer.chunk_bytes,
        }
    });

    let resume = current.resume.as_ref().map(|pending| ResumeView {
        pack_id: pending.pack_id.clone(),
        chapter: pending.chapter_name.clone(),
        percent: if pending.bytes > 0 {
            (pending.received_bytes * 100 / pending.bytes) as u32
        } else {
            0
        },
        received: pending.received_bytes,
        total: pending.bytes,
        files_done: pending.files_done,
        resume_from: pending.resume_from,
    });

    // 日志行只在日志页才克隆（最多 240 行），别的页面只要级别计数。
    let logs = if current.page == Page::Logs { logger::snapshot() } else { Vec::new() };
    let (log_warns, log_errors) = logger::counts();

    Snapshot {
        page: current.page,
        version: current.version.clone(),
        device: DeviceView {
            name: current.device_name.clone(),
            addr: current.device_addr.clone(),
            connected: current.connected,
            alive: current.alive,
            probe_attempt: current.probe_attempt,
            probe_max: MAX_PROBE_ATTEMPTS,
        },
        library,
        library_error: current.library_error.clone(),
        installed,
        transfer,
        resume,
        cache_bytes: current.cache_bytes,
        cache_files: current.cache_files,
        chunk_bytes: current.chunk_bytes,
        queue: current.sync_queue.clone(),
        status: current.status.clone(),
        status_kind: current.status_kind,
        auto_launch: current.auto_launch,
        hover: current.hover.clone(),
        // 按下态带时间兜底：元素树收不到「在按钮外松开」，动作执行与鼠标移出也会清。
        pressed: current
            .pressed
            .as_ref()
            .filter(|(_, at)| now_ms().saturating_sub(*at) < PRESS_TIMEOUT_MS)
            .map(|(id, _)| id.clone()),
        line_filter: current.line_filter.clone(),
        // 存档只交**原始对象**：界面行（含「未安装」判定）由 `Snapshot::saves()`
        // 每次渲染时用**当前**的已安装章节列表现算 —— 这里绝不预先算好塞进去。
        save_auto: current.save_auto.clone(),
        save_slots: current.save_slots.clone(),
        saves_error: current.saves_error.clone(),
        confirm_delete_save: current.confirm_delete_save.clone(),
        save_protocol: current.save_protocol,
        saves_unsupported: current.saves_unsupported,
        saves_busy: current.saves_busy,
        band_version: current.band_version.clone(),
        saves_notice: current.saves_notice.clone(),
        save_export: current
            .save_export
            .map(|(bytes, slots, verified)| SaveExportView { bytes, slots, verified }),
        save_import: current
            .save_import
            .map(|(incoming, duplicates, existing, _truncated)| SaveImportView {
                incoming,
                duplicates,
                existing,
            }),
        stats: current.stats.clone(),
        stats_error: current.stats_error.clone(),
        stats_supported: current.stats_supported,
        stats_busy: current.stats_busy,
        logs,
        log_errors,
        log_warns,
        log_filter: current.log_filter,
        brand: brand_image(),
        limits: Limits {
            // 超时是**按请求种类**定的（见 `crate::request`）：设置页两档都报出来。
            request_timeout_ms: RequestKind::PackList.timeout_ms(),
            saves_timeout_ms: RequestKind::SaveList.timeout_ms(),
            retry_delay_ms: RETRY_DELAY_MS,
            max_retries: MAX_RETRIES,
            initial_window: INITIAL_WINDOW,
            probe_interval_ms: PROBE_INTERVAL_MS,
            probe_attempts: MAX_PROBE_ATTEMPTS,
            max_pack_bytes: MAX_PACK_BYTES,
        },
    }
}

/// 重绘当前页。
fn render() {
    let element_id = {
        let current = state().lock().unwrap_or_else(|error| error.into_inner());
        current.element_id.clone()
    };
    let Some(element_id) = element_id else { return };
    render::paint(&element_id, &amakano2_ui::build(&snapshot()));
}

/// 「打开游戏」：把《甜蜜女友2》调到前台，然后照旧走探测循环接上。
fn launch_app() {
    let addr = {
        let current = state().lock().unwrap_or_else(|error| error.into_inner());
        current.device_addr.clone()
    };
    if addr.is_empty() {
        set_status(StatusKind::Warn, "先点「连接设备」把手环连上，再打开游戏");
        render();
        return;
    }
    update(|state| {
        state.alive = false;
        state.probe_attempt = 0;
    });
    astrobox_ng_wit::block_on(async {
        let outcome = launch_watch_app(&addr).await;
        let (kind, text) = match outcome {
            LaunchOutcome::Launched => {
                (StatusKind::Good, "已打开《甜蜜女友2》，正在等待它回应…".to_string())
            }
            other => (StatusKind::Warn, other.hint()),
        };
        set_status(kind, text);
        render();
    });
    arm_timer(APP_START_DELAY_MS, json!({ "type": "amakano.timer", "kind": "probe" }).to_string());
}

fn parse_event_payload(payload: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(payload) {
        // 手环发来的消息自带 `type` 字段，原样返回，别把它的字段当成信封。
        if value.get("type").is_some() {
            return payload.to_string();
        }
        let text = if let Some(text) = value.get("payloadText").and_then(Value::as_str) {
            text.to_string()
        } else if let Some(inner) = value.get("payload") {
            inner.as_str().map_or_else(|| inner.to_string(), str::to_string)
        } else {
            payload.to_string()
        };
        if let Ok(value) = serde_json::from_str::<Value>(&text) {
            if let Some(inner) = value.get("str").and_then(Value::as_str) {
                return inner.to_string();
            }
        }
        return text;
    }
    payload.into()
}

/// 把插件自带的图片读成 **data URI 字符串**。
///
/// 为什么不写文件路径：宿主渲染插件页面时相对路径是按**宿主自己的**基准解析的，不是插件目录；
/// `manifest.json` / `packs/` 能读是因为那是插件自己在 wasm 里 `fs::read`。
/// 为什么不用 `prop("background-image", ...)`：那个逃生舱在本宿主上是空操作（实测），
/// 图片只能作为 `IMAGE` 元素的内容下发 —— 真机自检证明这条路能显示。
fn inline_asset(relative: &str, mime: &str) -> Option<Arc<str>> {
    let bytes = fs::read(relative).ok()?;
    Some(Arc::from(format!("data:{mime};base64,{}", STANDARD.encode(&bytes))))
}

/// 插件图标（顶栏品牌位）。读一次就缓存。
fn brand_image() -> Option<Arc<str>> {
    static CACHE: OnceLock<Option<Arc<str>>> = OnceLock::new();
    CACHE.get_or_init(|| inline_asset("icon.png", "image/png")).clone()
}

/// 读插件自己的 manifest 拿版本号，显示在界面上，方便确认装的是哪一版。
fn plugin_version() -> String {
    fs::read_to_string("manifest.json")
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value.get("version").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_default()
}

fn timeout_ms(transfer: &Transfer) -> u64 {
    let estimate = if transfer.rtt_ms == 0 { RETRY_DELAY_MS } else { transfer.rtt_ms.saturating_mul(4) };
    estimate.clamp(MIN_TIMEOUT_MS, MAX_TIMEOUT_MS)
}

fn chunk_payload(transfer: &Transfer, index: usize) -> String {
    let chunk = &transfer.chunks[index];
    json!({
        "type": "amakano.files.chunk",
        "requestId": transfer.request_id,
        "index": index,
        "path": chunk.path,
        "append": chunk.append,
        "end": chunk.end,
        "data": chunk.data
    })
    .to_string()
}

fn next_packets() -> Vec<(String, String, usize)> {
    let mut guard = state().lock().unwrap_or_else(|error| error.into_inner());
    if guard.device_addr.is_empty() {
        return Vec::new();
    }
    let addr = guard.device_addr.clone();
    let transfer = match guard.transfer.as_mut() {
        Some(value) => value,
        None => return Vec::new(),
    };
    if !transfer.ready || transfer.next_index >= transfer.chunks.len() {
        return Vec::new();
    }
    let mut packets = Vec::new();
    while transfer.in_flight.len() < transfer.window_size && transfer.next_index < transfer.chunks.len() {
        let index = transfer.next_index;
        transfer.next_index += 1;
        transfer.in_flight.push((index, now_ms()));
        transfer.retry_count = 0;
        packets.push((addr.clone(), chunk_payload(transfer, index), index));
    }
    packets
}

/// 回包到了：**只有 id 对得上**才算销账（`Slot::clear_if_matches` 的语义，宿主机上有用例）。
fn clear_request(id: &str) {
    update(|state| {
        state.request.clear_if_matches(id);
    });
}

/// 「刷新」的下一步：队列里还有往返就隔一拍发出去，没有就什么都不做。
///
/// **一次只挂一个请求**：`State::request` 只有一个坑位，后发的那个才拿得到
/// 「超时 → 重发」的兜底，所以「刷新」必须串行 —— 前一个 settle（回包 / 失败 /
/// 超时放弃）之后，下一个才发。顺序由 ui-core 的 `refresh_steps` 判定
/// （章节列表在前、存档在后），这里只负责照做。
///
/// **为什么要隔一拍**：实测「收完一条回包之后几毫秒就发下一个请求」的那一档，
/// 首次回包丢了 61.5%（13 个里 8 个）；而隔 200ms 以上发出的那一档几乎不丢。
/// 用现成的 timer 机制实现，不新造机制。
fn settle_refresh_step() {
    if update(|state| !state.refresh_queue.is_empty()) {
        arm_timer(FOLLOWUP_DELAY_MS, json!({ "type": "amakano.timer", "kind": "refresh-next" }).to_string());
    }
}

/// 发一个「刷新」往返。
async fn run_refresh_step(step: RefreshStep) {
    match step {
        RefreshStep::PackList => request_pack_list(false).await,
        RefreshStep::SaveList => {
            // 别把上一步的失败**悄悄冲掉**（第 7 节那条「失败必须看得见」）：
            // 章节列表刚报过错、我们还要继续拉存档时，状态行留给那句错误 ——
            // 存档该拉还是照拉，只是不再多写一句「正在读取…」把红字盖掉。
            let keep_error = update(|state| matches!(state.status_kind, StatusKind::Bad));
            if !keep_error {
                set_status(StatusKind::Info, "正在读取手环存档…");
            }
            request_save_list().await;
        }
        RefreshStep::StatsList => {
            // 同一条规矩：上一步刚报过错就把状态行留红。
            let keep_error = update(|state| matches!(state.status_kind, StatusKind::Bad));
            if !keep_error {
                set_status(StatusKind::Info, "正在读取手环上的阅读统计…");
            }
            request_stats().await;
        }
    }
}

async fn dispatch_request(kind: RequestKind, payload: String, id: String, probe: bool) {
    let addr = state().lock().unwrap_or_else(|error| error.into_inner()).device_addr.clone();
    if addr.is_empty() {
        set_status(StatusKind::Bad, "尚未连接手环，请先点「连接设备」");
        render();
        return;
    }
    // **先占坑、再发消息**：反过来的话，回包万一下得比占坑还快，
    // `clear_request` 会扑个空，接着武装的定时器就会给一个已经答过的请求再重发一次
    // （实测里那些「同一 requestId 收到两份回包」就有这一份）。
    update(|state| {
        state.request.arm(PendingRequest { kind, id: id.clone(), payload: payload.clone(), addr: addr.clone(), attempts: 1, probe });
    });
    if psys_host::interconnect::send_qaic_message(&addr, PACKAGE_NAME, &payload).await.is_err() {
        clear_request(&id);
        set_status(StatusKind::Bad, format!("{}失败：无法发送消息，请重新连接设备", kind.label()));
        render();
        return;
    }
    // 超时**按请求种类**来：存档比章节列表宽（手环端要读 storage、拼 6KB 分片），
    // 轻量类给短窗口好让丢包赶紧重发。理由与实测数字见 `src/request.rs`。
    arm_timer(kind.timeout_ms(), json!({ "type": "amakano.timer", "kind": "request", "requestId": id }).to_string());
}

fn fail_request(kind: RequestKind) {
    // 存档通道：手动打开应用也不会让它长出存档支持，所以失败的归因先看「协议协商出来了没有」。
    // 没协商出来 = 手环端应用太旧 → 只置 `saves_unsupported` 状态位；
    // 那句「手环端应用版本过旧」的**结论**与**怎么办**由界面统一渲染
    // （ui-core 的 `saves_blocked_hint` / `saves_blocked_action`），这里不再写第二遍 ——
    // 否则用户会在同一张卡上看到两句几乎一样的提示（实机截图就是这么来的）。
    let saves_blocked = kind.is_saves()
        && state().lock().unwrap_or_else(|error| error.into_inner()).save_protocol.is_none();
    // 统计通道「被版本卡住」= 协议已经协商出来了，但能力表里没有 stats。
    let stats_blocked = matches!(kind, RequestKind::StatsList)
        && {
            let current = state().lock().unwrap_or_else(|error| error.into_inner());
            current.save_protocol.is_some() && !current.stats_supported
        };
    let message = match kind {
        RequestKind::Pending => "手环没有响应（未完成传输状态未知），请确认《甜蜜女友2》已打开后重新点「连接设备」",
        RequestKind::PackList => "手环没有响应（章节列表读取失败），请确认《甜蜜女友2》已打开后重新点「连接设备」",
        RequestKind::Delete => "手环没有响应，章节包未删除，请稍后重试",
        RequestKind::ClearCache => "手环没有响应，未完成缓存未清理，请稍后重试",
        // 存档通道：没协商出协议 → 状态行只给一句短状态（结论+怎么办在「存档」页那张卡上）；
        // 协议没问题、只是这一次超时 → 按普通超时讲，别再把手环说成「版本过旧」。
        RequestKind::Hello | RequestKind::SaveList | RequestKind::SavePut | RequestKind::SaveDelete | RequestKind::SaveActivate => {
            if saves_blocked {
                "存档通道不可用：手环端应用版本过旧"
            } else {
                "手环没有响应（存档操作超时），请确认《甜蜜女友2》已打开后重试"
            }
        }
        // 统计是**另一条能力**：没协商出协议（saves 都没通）时它就是「版本过旧」；
        // 协议通了但能力表里没 stats，同样归到版本过旧（界面上那张卡会说清是哪个通道）。
        RequestKind::StatsList => {
            if stats_blocked {
                "阅读统计通道不可用：手环端应用版本过旧"
            } else {
                "手环没有响应（阅读统计读取超时），请确认《甜蜜女友2》已打开后重试"
            }
        }
    };
    set_status(StatusKind::Bad, message);
    // 存档相关的超时同时落到存档页的状态上：用户可能正停在那一页。
    if kind.is_saves() {
        update(|state| {
            if saves_blocked {
                state.saves_unsupported = true;
            } else if state.saves_error.is_empty() {
                state.saves_error = message.into();
            }
            state.saves_busy = false;
        });
    }
    // 统计超时同理：用户可能正停在「统计」页。能力不足时只置状态位，
    // 「版本过旧」那句结论由界面统一渲染（`Snapshot::stats_notice`），这里不写第二遍。
    if matches!(kind, RequestKind::StatsList) {
        update(|state| {
            state.stats_busy = false;
            if !stats_blocked && state.stats_error.is_empty() {
                state.stats_error = message.into();
            }
        });
    }
    render();
    // 「刷新」的这一跨越位了（成败都算 settle）：把排在后面的那一个补上 ——
    // 一次只挂一个请求，见 `settle_refresh_step`。
    settle_refresh_step();
}

async fn handle_request_timeout(request_id: &str) {
    // 该不该重发、还是该放弃，全由 `Slot` 判定（宿主机上有用例）：
    // 不是自己的定时器就 Ignore，探测请求静默让位，次数用完就 GiveUp。
    let action = update(|state| state.request.on_timeout(request_id));
    let retry = match action {
        TimeoutAction::Ignore | TimeoutAction::DropProbe => return,
        TimeoutAction::GiveUp(kind) => {
            fail_request(kind);
            return;
        }
        TimeoutAction::Retry => {
            let current = state().lock().unwrap_or_else(|error| error.into_inner());
            match current.request.get() {
                Some(request) => Some((request.kind, request.payload.clone(), request.addr.clone())),
                None => None,
            }
        }
    };
    let Some((kind, payload, addr)) = retry else { return };
    let _ = psys_host::interconnect::send_qaic_message(&addr, PACKAGE_NAME, &payload).await;
    set_status(StatusKind::Warn, format!("{}超时，正在自动补试…", kind.label()));
    render();
    arm_timer(kind.timeout_ms(), json!({ "type": "amakano.timer", "kind": "request", "requestId": request_id }).to_string());
}

async fn send_packet(addr: String, payload: String, index: usize, retry: bool) {
    if retry || index == 0 || index % 32 == 0 {
        tracing::info!(index, bytes = payload.len(), retry, "sending pack chunk");
    }
    if psys_host::interconnect::send_qaic_message(&addr, PACKAGE_NAME, &payload).await.is_err() {
        fail_transfer("发送被链路拒绝");
        return;
    }
    let retry_payload = {
        let current = state().lock().unwrap_or_else(|error| error.into_inner());
        current.transfer.as_ref().and_then(|item| {
            (item.in_flight.iter().any(|(value, _)| *value == index)).then(|| {
                json!({
                    "type": "amakano.files.retry",
                    "requestId": item.request_id,
                    "index": index
                })
                .to_string()
            })
        })
    };
    if let Some(retry_payload) = retry_payload {
        let delay = state().lock().unwrap_or_else(|error| error.into_inner()).transfer.as_ref().map(timeout_ms).unwrap_or(RETRY_DELAY_MS);
        astrobox_ng_wit::spawn(async move {
            let _ = psys_host::timer::set_timeout(delay, &retry_payload).await;
        });
    }
}

async fn send_next_packet() {
    for (addr, payload, index) in next_packets() {
        send_packet(addr, payload, index, false).await;
    }
}

/// 传输中断：不改分片档位，保留断点，提示用户重连后继续。
fn fail_transfer(reason: &str) {
    update(|state| {
        if let Some(transfer) = state.transfer.as_mut() {
            transfer.started = false;
            transfer.ready = false;
            transfer.waiting_ready = false;
            transfer.in_flight.clear();
            transfer.window_size = INITIAL_WINDOW;
        }
        state.status = format!("连接已断开（{reason}）。请在 AstroBox 里重新连接设备，再点「继续同步」接着传");
        state.status_kind = StatusKind::Bad;
    });
    render();
}

enum RetryAction {
    Resend(String, String, usize),
    Failed,
}

fn retry_packet(payload: &str) -> Option<RetryAction> {
    let value = serde_json::from_str::<Value>(payload).ok()?;
    let message = value.get("payload").and_then(Value::as_str).and_then(|text| serde_json::from_str::<Value>(text).ok()).unwrap_or(value);
    if message.get("type").and_then(Value::as_str) != Some("amakano.files.retry") {
        return None;
    }
    let request_id = message.get("requestId").and_then(Value::as_str)?;
    let index = message.get("index").and_then(Value::as_u64)? as usize;
    let mut current = state().lock().unwrap_or_else(|error| error.into_inner());
    let addr = current.device_addr.clone();
    let transfer = current.transfer.as_mut()?;
    if transfer.request_id != request_id || !transfer.in_flight.iter().any(|(value, _)| *value == index) {
        return None;
    }
    if transfer.retry_count >= MAX_RETRIES {
        drop(current);
        fail_transfer("连续超时");
        return Some(RetryAction::Failed);
    }
    transfer.retry_count += 1;
    transfer.window_size = 1;
    transfer.in_flight.retain(|(value, _)| *value == index);
    transfer.next_index = transfer.next_index.min(index + 1);
    let retry = transfer.retry_count;
    let label = chunk_label(transfer.chunk_bytes);
    let payload = chunk_payload(transfer, index);
    current.status = format!("第{}片超时，正在重发 {retry}/{MAX_RETRIES} · {label} 分片", index + 1);
    current.status_kind = StatusKind::Warn;
    Some(RetryAction::Resend(addr, payload, index))
}

fn handle_ready_timeout(request_id: &str) {
    let resend = {
        let mut guard = state().lock().unwrap_or_else(|error| error.into_inner());
        let addr = guard.device_addr.clone();
        let meta = guard.pack_meta.clone();
        match (meta, guard.transfer.as_mut()) {
            (Some(meta), Some(transfer)) if transfer.waiting_ready && transfer.request_id == request_id => {
                if transfer.ready_attempts < MAX_READY_ATTEMPTS {
                    transfer.ready_attempts += 1;
                    Some((addr, begin_packet(&meta, transfer)))
                } else {
                    None
                }
            }
            _ => return,
        }
    };
    match resend {
        Some((addr, payload)) => {
            set_status(StatusKind::Warn, "手表未响应，正在重发导入请求…");
            render();
            astrobox_ng_wit::spawn(async move {
                let _ = psys_host::interconnect::send_qaic_message(&addr, PACKAGE_NAME, &payload).await;
            });
            arm_timer(READY_TIMEOUT_MS, json!({ "type": "amakano.timer", "kind": "ready", "requestId": request_id }).to_string());
        }
        None => fail_transfer("手表未响应导入请求"),
    }
}

async fn handle_timer(payload: &str) {
    let Ok(value) = serde_json::from_str::<Value>(payload) else { return };
    if value.get("type").and_then(Value::as_str) != Some("amakano.timer") {
        return;
    }
    match value.get("kind").and_then(Value::as_str) {
        Some("request") => {
            if let Some(id) = value.get("requestId").and_then(Value::as_str) {
                handle_request_timeout(id).await;
            }
        }
        Some("ready") => {
            if let Some(id) = value.get("requestId").and_then(Value::as_str) {
                handle_ready_timeout(id);
            }
        }
        Some("pending") => request_pending().await,
        Some("probe") => probe_tick().await,
        Some("list") => request_pack_list(false).await,
        // 「刷新」的下一个往返（前一个已经 settle，见 `settle_refresh_step`）。
        Some("refresh-next") => {
            let next = update(|state| {
                if state.refresh_queue.is_empty() { None } else { Some(state.refresh_queue.remove(0)) }
            });
            if let Some(step) = next {
                run_refresh_step(step).await;
            }
        }
        // 「收完一条回包顺手再拉一次存档」：**不在消息回调里立刻发**，隔一拍再发。
        // 实测「紧跟回包几毫秒就发」的那一档丢了 61.5%（`request::FOLLOWUP_DELAY_MS`）。
        Some("saves-list") => request_save_list().await,
        // 切到「统计」页时顺手续拉一次（同样隔一拍再发，理由同上）。
        Some("stats-list") => request_stats().await,
        _ => {}
    }
}

fn connect_device() {
    astrobox_ng_wit::block_on(async {
        let devices = psys_host::device::get_connected_device_list().await;
        let Some(device) = devices.first() else {
            // 在线列表为空时再看一眼宿主的设备记录，把「没连过」和「连过但掉线」区分开。
            let known: Vec<String> = psys_host::device::get_device_list()
                .await
                .into_iter()
                .take(3)
                .map(|item| item.name)
                .collect();
            let detail = if known.is_empty() { "（AstroBox 里也没有设备记录）".to_string() } else { format!("（记录里有 {}，但当前都不在线）", known.join("、")) };
            set_status(StatusKind::Bad, format!("未找到在线的手环{detail}，请先在 AstroBox 里连接设备"));
            update(|state| state.connected = false);
            render();
            return;
        };
        let addr = device.addr.clone();
        let name = device.name.clone();
        update(|state| {
            state.device_addr = addr.clone();
            state.device_name = name;
            state.alive = false;
            state.probe_attempt = 0;
            state.pending_checked = false;
            state.sync_queue.clear();
            state.status = "正在连接手环…".into();
            state.status_kind = StatusKind::Info;
        });
        render();
        let registered = psys_host::register::register_interconnect_recv(&addr, PACKAGE_NAME).await.is_ok();
        if !registered {
            set_status(StatusKind::Bad, "已找到设备，但回包注册失败，请重试或重启 AstroBox");
            update(|state| state.connected = false);
            render();
            return;
        }
        // 注册成功即视为「通道已建立」；应用是否在前台由探测循环判断。
        update(|state| state.connected = true);
        let auto_launch = {
            let current = state().lock().unwrap_or_else(|error| error.into_inner());
            current.auto_launch
        };
        let launch = if registered && auto_launch {
            launch_watch_app(&addr).await
        } else {
            LaunchOutcome::Skipped
        };
        let hint = launch.hint();
        update(|state| {
            state.status = format!("已连接手环。{hint}");
            state.status_kind = if launch.is_problem() { StatusKind::Warn } else { StatusKind::Good };
        });
        render();
        tracing::info!(?hint, "connect finished");
        // 先给应用 2 秒启动时间，然后按固定间隔轮询；手动打开也能接上。
        arm_timer(APP_START_DELAY_MS, json!({ "type": "amakano.timer", "kind": "probe" }).to_string());
        // 通道建立后顺手问一次存档能力：`hello-ok` 缺席就等于「手环端应用太旧」。
        // 探测循环随后仍会照旧发 pack.list（那是既有链路，不动它）。
        request_hello().await;
    });
}

#[derive(Debug, Clone)]
enum LaunchOutcome {
    Launched,
    NotFound(String),
    Denied,
    ListFailed,
    Skipped,
}

impl LaunchOutcome {
    fn hint(&self) -> String {
        match self {
            LaunchOutcome::Launched => "已自动打开《甜蜜女友2》，正在等待它回应…".into(),
            LaunchOutcome::NotFound(names) => format!("但手环应用列表里没有本应用（检测到 {names}），请确认 RPK 已安装"),
            LaunchOutcome::Denied => "尝试打开《甜蜜女友2》被系统拒绝，请手动在手表上打开".into(),
            LaunchOutcome::ListFailed => "读取手环应用列表失败（需要 thirdpartyapp 权限），请手动在手表上打开《甜蜜女友2》".into(),
            LaunchOutcome::Skipped => "未自动打开应用，可在「设置」页开启自动打开，或点「打开游戏」".into(),
        }
    }

    fn is_problem(&self) -> bool {
        !matches!(self, LaunchOutcome::Launched)
    }
}

/// 自动打开手环上的《甜蜜女友2》。
///
/// 官方文档：`launch-qa` 会优先使用传入的 app-info，**package-name 有值而 fingerprint 为空时，
/// 宿主会按包名从设备已安装应用里补全签名信息**。所以这里先直接按包名启动，
/// 不再依赖 `get-thirdparty-app-list`（那一步要设备侧 ResourceSystem 回包，慢且需要额外权限）；
/// 只有直接启动失败时才去取列表，用来判断到底是「没这个应用」还是「权限/列表不可用」。
async fn launch_watch_app(addr: &str) -> LaunchOutcome {
    let direct = psys_host::thirdpartyapp::AppInfo {
        package_name: PACKAGE_NAME.into(),
        fingerprint: Vec::new(),
        version_code: 0,
        can_remove: false,
        app_name: "甜蜜女友2".into(),
    };
    if psys_host::thirdpartyapp::launch_qa(addr, &direct, "pages/index").await.is_ok() {
        tracing::info!("launched watch app by package name");
        return LaunchOutcome::Launched;
    }
    tracing::warn!("direct launch rejected, falling back to the app list");
    let Ok(apps) = psys_host::thirdpartyapp::get_thirdparty_app_list(addr).await else {
        return LaunchOutcome::ListFailed;
    };
    let Some(app) = apps.iter().find(|item| item.package_name == PACKAGE_NAME) else {
        let names: Vec<&str> = apps.iter().take(3).map(|item| item.app_name.as_str()).collect();
        let summary = if names.is_empty() { "0 个应用".to_string() } else { format!("{} 个：{}", apps.len(), names.join("、")) };
        return LaunchOutcome::NotFound(summary);
    };
    match psys_host::thirdpartyapp::launch_qa(addr, app, "pages/index").await {
        Ok(()) => {
            tracing::info!("launched watch app from the app list");
            LaunchOutcome::Launched
        }
        Err(_) => {
            tracing::warn!("launch_qa rejected by host");
            LaunchOutcome::Denied
        }
    }
}

async fn request_pending() {
    let id = format!("pend-{}", now_ms());
    let payload = json!({ "type": "amakano.pack.pending", "requestId": id }).to_string();
    dispatch_request(RequestKind::Pending, payload, id, false).await;
}

async fn request_pack_list(probe: bool) {
    let id = format!("packs-{}", now_ms());
    let payload = json!({ "type": "amakano.pack.list", "requestId": id }).to_string();
    dispatch_request(RequestKind::PackList, payload, id, probe).await;
}

async fn request_delete_pack(pack_id: &str) {
    let id = format!("delete-{}", now_ms());
    let payload = json!({ "type": "amakano.pack.delete", "requestId": id, "packId": pack_id }).to_string();
    dispatch_request(RequestKind::Delete, payload, id, false).await;
}

async fn request_clear_cache() {
    let id = format!("cache-{}", now_ms());
    let payload = json!({ "type": "amakano.pack.clear-cache", "requestId": id }).to_string();
    dispatch_request(RequestKind::ClearCache, payload, id, false).await;
}

// ------------------------------------------------------------------ 存档请求

/// 能力查询：连接成功之后发一次，问手环认不认存档协议。
///
/// **`hello-ok` 缺席就是「手环端应用太旧」**：旧版手环的白名单 if 链没有 else 分支，
/// 收到不认识的消息直接丢掉，所以这里永远不会等到回应 —— 这正是要靠它判断的信号。
async fn request_hello() {
    let id = format!("hello-{}", now_ms());
    tracing::info!(request = %id, "hello sent (amakano.app.hello)");
let payload = json!({ "type": "amakano.app.hello", "requestId": id, "protocol": SAVE_PROTOCOL }).to_string();
    dispatch_request(RequestKind::Hello, payload, id, false).await;
}

async fn request_save_list() {
    // 每拉一次就清空分片缓存：上一次没收回来的碎片留着，只会把新回包拼进旧片段里。
    with_shards(&SAVE_SHARDS, SaveShardSlot::reset);
    let id = format!("saves-{}", now_ms());
    let payload = json!({ "type": "amakano.saves.list", "requestId": id }).to_string();
    dispatch_request(RequestKind::SaveList, payload, id, false).await;
}

/// 从手环读一次阅读统计。
///
/// 与存档同一套骨架（先清分片缓存 → 占坑 → 发消息 → 按种类的超时兜底），
/// 差别只在请求名与坑位：统计走 `amakano.stats.*`，分片累积在 `STATS_SHARDS`。
async fn request_stats() {
    with_shards(&STATS_SHARDS, SaveShardSlot::reset);
    let id = format!("stats-{}", now_ms());
    let payload = json!({ "type": "amakano.stats.list", "requestId": id }).to_string();
    dispatch_request(RequestKind::StatsList, payload, id, false).await;
}

/// 把存档写回手环。
///
/// `action`：`upsert`（按 `savedAt` 并，默认）/ `replace`（整表替换，需要 confirm）。
/// `saves` 里的对象是**原样**从导入文件/手环取出来的 `Value`，插件不解释、不改写字段。
async fn request_save_put(action: &str, saves: Vec<Value>, auto_save: Option<Value>) {
    let id = format!("savput-{}", now_ms());
    let payload = json!({
        "type": "amakano.saves.put",
        "requestId": id,
        "action": action,
        "confirm": action == "replace",
        "saves": saves,
        "autoSave": auto_save,
    })
    .to_string();
    dispatch_request(RequestKind::SavePut, payload, id, false).await;
}
async fn request_save_delete(slot: usize) {
    let id = format!("savdel-{}", now_ms());
    let payload = json!({ "type": "amakano.saves.delete", "requestId": id, "slot": slot }).to_string();
    dispatch_request(RequestKind::SaveDelete, payload, id, false).await;
}

/// 读档：把手环上这一槽设成「继续阅读」的进度（手环首页按 autoSave 走）。
async fn request_save_activate(slot: usize) {
    let id = format!("savact-{}", now_ms());
    let payload = json!({ "type": "amakano.saves.activate", "requestId": id, "slot": slot }).to_string();
    dispatch_request(RequestKind::SaveActivate, payload, id, false).await;
}

/// 连接后轮询：手动打开手环应用也能自动接上，回应一次就进入「已连接」。
async fn probe_tick() {
    let (addr, alive, attempt, busy) = {
        let current = state().lock().unwrap_or_else(|error| error.into_inner());
        (current.device_addr.clone(), current.alive, current.probe_attempt, current.request.busy())
    };
    if addr.is_empty() {
        return;
    }
    if alive {
        // 应用已经被证明是活的：若还没协商到存档协议，就**再问一次**。
        // 不能只在「连接设备」里问一次 —— 那一下手环可能还没醒（或还没装新版 RPK），
        // 握手就永远没人应答，界面会一直说「手环端应用过旧」，除非用户手动重连。
        let need_hello = {
            let current = state().lock().unwrap_or_else(|error| error.into_inner());
            current.save_protocol.is_none()
        };
        if need_hello && !busy && attempt < MAX_PROBE_ATTEMPTS {
            update(|state| state.probe_attempt = attempt + 1);
            tracing::info!(attempt = attempt + 1, "app is alive: asking save capability");
            request_hello().await;
            arm_timer(PROBE_INTERVAL_MS, json!({ "type": "amakano.timer", "kind": "probe" }).to_string());
        }
        return;
    }
    if !busy {
        if attempt >= MAX_PROBE_ATTEMPTS {
            set_status(
                StatusKind::Bad,
                format!("等待《甜蜜女友2》回应超时（已试 {MAX_PROBE_ATTEMPTS} 次）。请确认手表上已打开应用，再点「连接设备」重试"),
            );
            render();
            return;
        }
        update(|state| {
            state.probe_attempt = attempt + 1;
            state.status = format!("正在等待《甜蜜女友2》回应…（{}/{MAX_PROBE_ATTEMPTS}）", attempt + 1);
            state.status_kind = StatusKind::Info;
        });
        render();
        request_pack_list(true).await;
    }
    arm_timer(PROBE_INTERVAL_MS, json!({ "type": "amakano.timer", "kind": "probe" }).to_string());
}

fn mark_alive() {
    update(|state| {
        if !state.alive {
            state.alive = true;
            state.probe_attempt = 0;
        }
    });
}

fn set_chunk_size(size: usize) -> bool {
    if !CHUNK_OPTIONS.contains(&size) {
        return false;
    }
    let mut message = String::new();
    let mut kind = StatusKind::Info;
    update(|state| {
        if state.chunk_bytes == size {
            return;
        }
        let busy = state.transfer.as_ref().map(|transfer| transfer.started).unwrap_or(false);
        state.chunk_bytes = size;
        let count = if state.pack_meta.is_some() {
            let chunks = build_chunks(&state.pack_files, size);
            let count = chunks.len();
            state.transfer = Some(transfer_with_chunks(size, chunks));
            count
        } else {
            state.transfer = None;
            0
        };
        if busy {
            state.resume = None;
            kind = StatusKind::Warn;
            message = format!("已切换为 {} 分片，当前同步已取消（断点作废），请重新点同步", chunk_label(size));
        } else if state.resume.is_some() {
            state.resume = None;
            kind = StatusKind::Warn;
            message = format!("分片大小已设为 {}，与未完成传输不一致，断点已作废", chunk_label(size));
        } else if count > 0 {
            message = format!("分片大小已设为 {}，共 {count} 个分片", chunk_label(size));
        } else {
            message = format!("分片大小已设为 {}", chunk_label(size));
        }
    });
    set_status(kind, message);
    render();
    true
}

async fn start_transfer() {
    let packet = {
        let mut current = state().lock().unwrap_or_else(|error| error.into_inner());
        if current.device_addr.is_empty() {
            None
        } else {
            let addr = current.device_addr.clone();
            let meta = current.pack_meta.clone();
            match (meta, current.transfer.as_mut()) {
                (Some(meta), Some(transfer)) => {
                    if transfer.started && transfer.waiting_ready {
                        None
                    } else {
                        transfer.request_id = format!("pack-{}", now_ms());
                        transfer.window_size = INITIAL_WINDOW;
                        transfer.acked_bytes = 0;
                        transfer.speed_kbps = 0.0;
                        transfer.speed_window_start = now_ms();
                        transfer.speed_window_bytes = 0;
                        transfer.rtt_ms = 0;
                        transfer.retry_count = 0;
                        transfer.ready = false;
                        transfer.started = true;
                        transfer.waiting_ready = true;
                        transfer.ready_attempts = 1;
                        transfer.resumed = false;
                        transfer.in_flight.clear();
                        Some((addr, transfer.request_id.clone(), begin_packet(&meta, transfer)))
                    }
                }
                _ => None,
            }
        }
    };
    let Some((addr, request_id, payload)) = packet else {
        update(|state| {
            if state.transfer.as_ref().map(|transfer| transfer.started && transfer.waiting_ready).unwrap_or(false) {
                state.status = "同步已开始，请等待完成或失败".into();
                state.status_kind = StatusKind::Info;
            } else if state.pack_meta.is_none() {
                state.status = "请先在章节列表里点某一章的「同步」".into();
                state.status_kind = StatusKind::Warn;
            } else {
                state.status = "请先点「连接设备」".into();
                state.status_kind = StatusKind::Warn;
            }
        });
        render();
        return;
    };
    if psys_host::interconnect::send_qaic_message(&addr, PACKAGE_NAME, &payload).await.is_ok() {
        set_status(StatusKind::Info, "已发送导入请求，等待手表确认…");
        arm_timer(READY_TIMEOUT_MS, json!({ "type": "amakano.timer", "kind": "ready", "requestId": request_id }).to_string());
        tracing::info!(bytes = payload.len(), "sent pack begin request");
    } else {
        update(|state| {
            if let Some(transfer) = state.transfer.as_mut() {
                transfer.started = false;
                transfer.waiting_ready = false;
            }
        });
        set_status(StatusKind::Bad, "无法发送，请确认《甜蜜女友2》已在手环上打开");
    }
    render();
}

fn apply_ready(transfer: &mut Transfer, resume_from: usize, resumed: bool) {
    let resume_from = resume_from.min(transfer.chunks.len());
    transfer.next_index = resume_from;
    transfer.in_flight.clear();
    transfer.window_size = INITIAL_WINDOW;
    transfer.retry_count = 0;
    transfer.rtt_ms = 0;
    transfer.speed_window_start = now_ms();
    transfer.speed_window_bytes = 0;
    transfer.speed_kbps = 0.0;
    transfer.acked_bytes = prefix_bytes(&transfer.chunks, resume_from);
    transfer.ready = true;
    transfer.started = true;
    transfer.waiting_ready = false;
    transfer.resumed = resumed || resume_from > 0;
}

fn handle_interconnect(payload: &str) -> bool {
    let Ok(message) = serde_json::from_str::<Value>(&parse_event_payload(payload)) else { return false };
    // 手环应用回了任何一条消息，说明它正在前台运行。
    mark_alive();
    let request_id = message.get("requestId").and_then(Value::as_str).unwrap_or("").to_string();
    match message.get("type").and_then(Value::as_str) {
        Some("amakano.pack.pending") => {
            clear_request(&request_id);
            let pending = message.get("pending").filter(|value| !value.is_null()).and_then(|value| {
                Some(ResumeInfo {
                    pack_id: value.get("packId").and_then(Value::as_str)?.to_string(),
                    chapter_name: value.get("chapterName").and_then(Value::as_str).unwrap_or("未命名章节包").to_string(),
                    bytes: value.get("bytes").and_then(Value::as_u64).unwrap_or(0) as usize,
                    chunks: value.get("chunks").and_then(Value::as_u64).unwrap_or(0) as usize,
                    resume_from: value.get("resumeFrom").and_then(Value::as_u64).unwrap_or(0) as usize,
                    received_bytes: value.get("receivedBytes").and_then(Value::as_u64).unwrap_or(0) as usize,
                    files_done: value.get("filesDone").and_then(Value::as_u64).unwrap_or(0) as usize,
                })
            });
            update(|state| {
                state.resume = pending;
                state.status = match state.resume.as_ref() {
                    Some(pending) => {
                        let percent = if pending.bytes > 0 { pending.received_bytes * 100 / pending.bytes } else { 0 };
                        format!("发现未完成传输：{} · 已传 {}%，点「继续同步」接着传", pending.chapter_name, percent)
                    }
                    None => "手环上没有未完成的传输".into(),
                };
                state.status_kind = if state.resume.is_some() { StatusKind::Warn } else { StatusKind::Info };
            });
            render();
            arm_timer(300, json!({ "type": "amakano.timer", "kind": "list" }).to_string());
            return false;
        }
        Some("amakano.pack.list") => {
            clear_request(&request_id);
            let installed: Vec<InstalledPack> = message
                .get("packs")
                .and_then(Value::as_array)
                .map(|packs| {
                    packs
                        .iter()
                        .filter_map(|pack| {
                            let id = pack.get("packId").and_then(Value::as_str)?;
                            let name = pack.get("chapterName").or_else(|| pack.get("name")).and_then(Value::as_str).unwrap_or(id);
                            Some(InstalledPack {
                                id: id.into(),
                                name: name.into(),
                                chapter_number: pack.get("chapterNumber").and_then(Value::as_u64).unwrap_or(usize::MAX as u64) as usize,
                                bytes: pack.get("bytes").and_then(Value::as_u64).unwrap_or(0) as usize,
                                files: pack.get("files").and_then(Value::as_u64).unwrap_or(0) as usize,
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let mut installed = installed;
            installed.sort_by_key(|pack| pack.chapter_number);
            let cache_bytes = message.get("cacheBytes").and_then(Value::as_u64).unwrap_or(0) as usize;
            let cache_files = message.get("cacheFiles").and_then(Value::as_u64).unwrap_or(0) as usize;
            let ask_pending = {
                let mut current = state().lock().unwrap_or_else(|error| error.into_inner());
                let ask = !current.pending_checked;
                current.pending_checked = true;
                current.installed = installed;
                current.cache_bytes = cache_bytes;
                current.cache_files = cache_files;
                if current.resume.is_none() && cache_bytes == 0 {
                    current.status = format!("已连接《甜蜜女友2》，手环已安装 {} 个章节包", current.installed.len());
                    current.status_kind = StatusKind::Good;
                }
                ask
            };
            render();
            if ask_pending {
                arm_timer(200, json!({ "type": "amakano.timer", "kind": "pending" }).to_string());
            }
            // 这一跨越位了：如果「刷新」还有下一个往返（存档列表），隔一拍补上。
            settle_refresh_step();
            return false;
        }
        Some("amakano.pack.deleted") => {
            clear_request(&request_id);
            let id = message.get("packId").and_then(Value::as_str).unwrap_or("").to_string();
            let success = message.get("success").and_then(Value::as_bool).unwrap_or(false);
            update(|state| {
                if success {
                    state.installed.retain(|pack| pack.id != id);
                }
                state.status = if success { format!("已删除 {id}") } else { format!("删除 {id} 失败") };
                state.status_kind = if success { StatusKind::Good } else { StatusKind::Bad };
            });
            render();
            return false;
        }
        Some("amakano.pack.cache-cleared") => {
            clear_request(&request_id);
            let success = message.get("success").and_then(Value::as_bool).unwrap_or(false);
            update(|state| {
                if success {
                    state.cache_bytes = 0;
                    state.cache_files = 0;
                    state.resume = None;
                }
                state.status = if success { "未完成缓存已清理".into() } else { "清理未完成缓存失败".into() };
                state.status_kind = if success { StatusKind::Good } else { StatusKind::Bad };
            });
            render();
            return false;
        }
        // ---- 存档通道 ----
        Some("amakano.app.hello-ok") => {
            clear_request(&request_id);
            let protocol = message.get("protocol").and_then(Value::as_u64).unwrap_or(0) as u32;
            let version = message.get("appVersion").and_then(Value::as_str).unwrap_or("未知").to_string();
            let version_code = message.get("versionCode").and_then(Value::as_u64).unwrap_or(0);
            let features: Vec<String> = message
                .get("features")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            let supports_saves = features.iter().any(|feature| feature == "saves");
            // 阅读统计是独立的一条能力：老版本手环只报 ['saves','packs']。
            let supports_stats = features.iter().any(|feature| feature == "stats");
            tracing::info!(protocol, version = %version, version_code, ?features, "band save capability reported");
            update(|state| {
                state.band_version = version.clone();
                state.save_protocol = (protocol >= SAVE_PROTOCOL && supports_saves).then_some(protocol);
                state.stats_supported = supports_stats;
                // 不支持就把上一次的统计清掉：留着旧数据会让「版本过旧」那张卡和一堆
                // 看起来正常的数字同时出现，用户不知道该信哪个。
                if !supports_stats {
                    state.stats = None;
                    state.stats_busy = false;
                }
                if state.save_protocol.is_some() {
                    state.saves_error.clear();
                    state.saves_unsupported = false;
                    state.status = format!("手环端支持存档管理（应用 v{version} · 协议 {protocol}）");
                    state.status_kind = StatusKind::Good;
                } else {
                    // 回了但能力不够：同样归到「手环端应用版本过旧」，只置状态位，
                    // 卡片的结论 + 怎么办由界面统一给（那句「报告协议 x、能力 [ ]」的细节
                    // 已经由上面那条 `tracing::info!` 记进日志，不占界面的一行）。
                    tracing::warn!(protocol, ?features, "band app reported insufficient save capability");
                    state.saves_unsupported = true;
                    state.status = "存档通道不可用：手环端应用版本过旧".into();
                    state.status_kind = StatusKind::Warn;
                }
            });
            render();
            if supports_saves && protocol >= SAVE_PROTOCOL {
                // 协商成功就顺手拉一次存档列表，用户切到「存档」页时数据已经在了。
                //
                // ⚠️ **不许在这里直接发**：实测「收到一条回包之后几毫秒内就发出下一个请求」
                // 的那一档，第一次回包丢了 61.5%（13 个里 8 个），而这条 `hello-ok → saves.list`
                // 的链更是 10 个样本里丢了 8 个 —— 用户看到的就是「连上设备后第一次打开存档页
                // 要转半天甚至报超时，再点一次刷新就好了」。隔一拍（用现成的 timer）再发，
                // 200ms 以上发出的一档几乎不丢。理由与数字见 `src/request.rs` 的 `FOLLOWUP_DELAY_MS`。
                set_status(StatusKind::Info, "正在读取手环存档…");
                render();
                arm_timer(FOLLOWUP_DELAY_MS, json!({ "type": "amakano.timer", "kind": "saves-list" }).to_string());
            }
            // 用户正停在「统计」页、而刚连上时：顺手把统计也读一次，
            // 否则他得再点一下「读取统计」（同样是隔一拍再发，不许在回调里立刻发）。
            if supports_stats && state().lock().unwrap_or_else(|error| error.into_inner()).page == Page::Stats {
                arm_timer(FOLLOWUP_DELAY_MS, json!({ "type": "amakano.timer", "kind": "stats-list" }).to_string());
            }
            return false;
        }
        Some("amakano.saves.data") => {
            clear_request(&request_id);
            // 回包是**分片信封**：先把各片按 seq 拼起来（单片走同一条路），
            // 再从**里层载荷**读字段。旧实现直接读外层，于是永远 0 条 —— 见
            // `amakano2_ui::saves` 的模块注释与 `docs/章节包导入.md` 的「回包信封」一节。
            let shards = match push_save_shard(&message) {
                Ok(Some(shards)) => shards,
                // 还没收齐：不报错、也不动界面，安静等着。
                Ok(None) => return false,
                Err(reason) => {
                    tracing::warn!(request_id = %request_id, reason = %reason, "band save reply rejected");
                    let hint = format!("读取手环存档失败：{reason}");
                    update(|state| {
                        state.saves_busy = false;
                        state.saves_error = hint.clone();
                        state.status = hint;
                        state.status_kind = StatusKind::Bad;
                    });
                    render();
                    return false;
                }
            };
            let saves = match band_envelope(&shards) {
                Ok(saves) => saves,
                Err(error) => {
                    // **绝不静默变 0 条**：这次的 bug 就是静默失败，害得「手环上真没有存档」
                    // 和「插件没读懂回包」在界面上长得一模一样。
                    let reason = error.message().to_string();
                    tracing::warn!(request_id = %request_id, shards = shards.len(), reason = %reason, "band save reply unreadable");
                    let hint = format!("读取手环存档失败：{reason}");
                    update(|state| {
                        state.saves_busy = false;
                        state.saves_error = hint.clone();
                        state.status = hint;
                        state.status_kind = StatusKind::Bad;
                    });
                    render();
                    return false;
                }
            };
            for warning in &saves.warnings {
                tracing::warn!(request_id = %request_id, warning = %warning, "band save list warning");
            }
            let auto_save = saves.auto_save.clone();
            let slots = saves.slots.clone();
            let reported = saves.reported;
            // 顺手把「手环上装了几个章节包」记进日志：存档行的「未安装」徽章就是拿这份列表
            // 在渲染时现算的，真机上再出这类问题时，一眼就能看出当时列表是不是空的。
            let installed = state().lock().unwrap_or_else(|error| error.into_inner()).installed.len();
            tracing::info!(
                count = slots.len(),
                reported = reported.unwrap_or(0),
                auto = auto_save.is_some(),
                installed,
                "band save list received"
            );
            update(|state| {
                // 只存**原始对象**：界面行在这里算的话，就等于把「未安装」的结论
                // 永久定死在「收到回包这一刻」的已安装列表上了（见 `State::save_auto` 的说明）。
                state.save_auto = auto_save;
                state.save_slots = slots;
                state.saves_error.clear();
                state.saves_busy = false;
                state.status = format!("手环上有 {} 条手动存档", state.save_slots.len());
                state.status_kind = StatusKind::Info;
            });
            render();
            // 这一跨越位了（存档是「刷新」的最后一步，正常情况下队列已经空了）。
            settle_refresh_step();
            return false;
        }
        // ---- 阅读统计 ----
        Some("amakano.stats.data") => {
            clear_request(&request_id);
            // 与存档回包同一套分片信封：按 seq 拼起来，再从**里层载荷**读字段。
            let shards = match push_stats_shard(&message) {
                Ok(Some(shards)) => shards,
                // 还没收齐：安静等着，不动界面。
                Ok(None) => return false,
                Err(reason) => {
                    tracing::warn!(request_id = %request_id, reason = %reason, "band stats reply rejected");
                    fail_stats(&format!("读取阅读统计失败：{reason}"));
                    return false;
                }
            };
            let payload = match shard_payload(&shards) {
                Ok(value) => value,
                Err(reason) => {
                    tracing::warn!(request_id = %request_id, shards = shards.len(), reason = %reason, "band stats reply unreadable");
                    fail_stats(&format!("读取阅读统计失败：{reason}"));
                    return false;
                }
            };
            let stats = match ReadingStatsView::from_json(&payload) {
                Ok(stats) => stats,
                Err(error) => {
                    // **绝不静默变 0**：「手环上真没读过」和「插件没读懂回包」必须能分清。
                    let reason = error.message().to_string();
                    tracing::warn!(request_id = %request_id, reason = %reason, "band stats payload unreadable");
                    fail_stats(&format!("读取阅读统计失败：{reason}"));
                    return false;
                }
            };
            tracing::info!(days = stats.total_day_count, recent = stats.recent.len(), "band stats received");
            update(|state| {
                state.status = if stats.has_data() {
                    format!("手环上累计阅读 {} 天 · 今日 {}", stats.total_days_label, stats.today_label)
                } else {
                    "手环上还没有阅读记录".into()
                };
                state.status_kind = StatusKind::Info;
                state.stats = Some(stats);
                state.stats_error.clear();
                state.stats_busy = false;
            });
            render();
            settle_refresh_step();
            return false;
        }
        Some("amakano.saves.put-ok") => {
            clear_request(&request_id);
            let count = message.get("count").and_then(Value::as_u64).unwrap_or(0);
            let total = message.get("slots").and_then(Value::as_u64).unwrap_or(0);
            // 这句人话的口径只有一处实现（`SaveImportView::notice`）：写进去几条、跳过几条重复。
            // 超出 20 槽被截掉的那些要额外说一句 —— 悄悄丢数据是最不能接受的失败方式。
            let (notice, truncated) = match state()
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .save_import
                .take()
            {
                Some((incoming, duplicates, existing, truncated)) => {
                    let mut notice =
                        SaveImportView { incoming, duplicates, existing }.notice();
                    if truncated > MAX_SAVE_SLOTS {
                        notice.push_str(&format!(
                            "；剪贴板里有 {truncated} 条，超出 {MAX_SAVE_SLOTS} 槽的没有写入"
                        ));
                    }
                    (notice, truncated > MAX_SAVE_SLOTS)
                }
                // 不是导入触发的写入（例如将来的其它写回路径）：退回手环自己的计数。
                None => (format!("已写入 {count} 条存档，手环上现在有 {total} 条"), false),
            };
            tracing::info!(count, total, notice = %notice, truncated, "band accepted save write");
            update(|state| {
                state.saves_busy = false;
                state.saves_notice = notice.clone();
                state.status = notice;
                state.status_kind = if truncated { StatusKind::Warn } else { StatusKind::Good };
            });
            render();
            // 写回成功后重新拉一次列表 —— 同样隔一拍再发，别在这个消息回调里重入发送。
            arm_timer(FOLLOWUP_DELAY_MS, json!({ "type": "amakano.timer", "kind": "saves-list" }).to_string());
            return false;
        }
        Some("amakano.saves.deleted") => {
            clear_request(&request_id);
            let removed = message.get("removed").and_then(Value::as_u64).unwrap_or(0);
            let count = message.get("count").and_then(Value::as_u64).unwrap_or(0);
            tracing::info!(removed, count, "band deleted a save slot");
            update(|state| {
                state.saves_busy = false;
                state.confirm_delete_save = None;
                state.saves_notice = format!("已删除存档 {}，还剩 {count} 条", removed + 1);
                state.status = state.saves_notice.clone();
                state.status_kind = StatusKind::Good;
            });
            render();
            // 删除成功后重新拉一次列表 —— 同样隔一拍再发（见 `FOLLOWUP_DELAY_MS`）。
            arm_timer(FOLLOWUP_DELAY_MS, json!({ "type": "amakano.timer", "kind": "saves-list" }).to_string());
            return false;
        }
        Some("amakano.saves.activated") => {
            clear_request(&request_id);
            let slot = message.get("slot").and_then(Value::as_u64).unwrap_or(0);
            let chapter = message.get("chapterTitle").and_then(Value::as_str).unwrap_or("未知章节").to_string();
            tracing::info!(slot, chapter = %chapter, "band activated a save for continue");
            update(|state| {
                state.saves_busy = false;
                state.saves_notice = format!("已把存档 {} 设为继续阅读（{chapter}）", slot + 1);
                state.status = "读档完成：手环首页的「继续阅读」会从这里开始".into();
                state.status_kind = StatusKind::Good;
            });
            render();
            return false;
        }
        Some("amakano.saves.error") => {
            clear_request(&request_id);
            let error = message.get("error").and_then(Value::as_str).unwrap_or("unknown").to_string();
            let hint = match error.as_str() {
                "slot-missing" => "手环上这个槽位已经不存在了，点「刷新」看看现在的列表",
                "empty" => "没有可写入的存档",
                "replace-needs-confirm" => "整表替换没有被确认",
                "write-failed" => "手环写入存储失败（空间不足或被系统拒绝）",
                _ => "手环端处理存档请求失败",
            };
            tracing::warn!(%error, "band rejected a save request");
            update(|state| {
                state.saves_busy = false;
                state.saves_error = format!("{hint}（{error}）");
                state.status = state.saves_error.clone();
                state.status_kind = StatusKind::Bad;
            });
            render();
            return false;
        }
        _ => {}
    }
    let is_current = state()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .transfer
        .as_ref()
        .map(|item| item.request_id == request_id)
        .unwrap_or(false);
    if !is_current {
        return false;
    }
    let mut send_next = false;
    match message.get("type").and_then(Value::as_str) {
        Some("amakano.files.ready") => {
            let resume_from = message.get("resumeFrom").and_then(Value::as_u64).unwrap_or(0) as usize;
            let resumed = message.get("resumed").and_then(Value::as_bool).unwrap_or(false);
            let mut should_send = false;
            let mut resumed_now = false;
            let mut next_index = 0;
            update(|state| {
                if let Some(item) = state.transfer.as_mut() {
                    apply_ready(item, resume_from, resumed);
                    resumed_now = item.resumed;
                    next_index = item.next_index;
                    should_send = true;
                }
            });
            if should_send {
                update(|state| {
                    if resumed_now {
                        state.resume = None;
                    }
                    state.status = if resumed_now {
                        format!("手表已就绪，从第 {} 片继续传输", next_index + 1)
                    } else {
                        "手表已就绪，正在传输".into()
                    };
                    state.status_kind = StatusKind::Info;
                });
            }
            tracing::info!(resume_from, resumed, "received pack ready response");
            render();
            send_next = should_send;
        }
        Some("amakano.files.chunk-ok") => {
            let index = message.get("index").and_then(Value::as_u64).unwrap_or(u64::MAX) as usize;
            let mut accepted = false;
            update(|state| {
                let mut progress = None;
                if let Some(item) = state.transfer.as_mut() {
                    if let Some(position) = item.in_flight.iter().position(|(value, _)| *value == index) {
                        let (_, sent_at) = item.in_flight.remove(position);
                        let now = now_ms();
                        let sample = now.saturating_sub(sent_at) as u64;
                        item.rtt_ms = if item.rtt_ms == 0 { sample } else { (item.rtt_ms * 3 + sample) / 4 };
                        item.acked_bytes += item.chunks[index].bytes;
                        if item.speed_window_start == 0 {
                            item.speed_window_start = now;
                        }
                        item.speed_window_bytes += item.chunks[index].bytes;
                        let elapsed = now.saturating_sub(item.speed_window_start);
                        if elapsed >= 250 {
                            item.speed_kbps = (item.speed_window_bytes as f64 * 1000.0 / elapsed.max(1) as f64 / 1024.0 * 10.0).round() / 10.0;
                            item.speed_window_start = now;
                            item.speed_window_bytes = 0;
                        }
                        item.retry_count = 0;
                        let acknowledged = item.next_index.saturating_sub(item.in_flight.len());
                        progress = Some(format!("正在同步 {}/{} · {} 分片", acknowledged, item.chunks.len(), chunk_label(item.chunk_bytes)));
                        accepted = true;
                    }
                }
                if let Some(progress) = progress {
                    state.status = progress;
                    state.status_kind = StatusKind::Info;
                }
            });
            if accepted {
                render();
            }
            if accepted && (index == 0 || index % 32 == 0) {
                tracing::info!(index, "received pack chunk acknowledgement");
            }
            send_next = accepted;
        }
        Some("amakano.files.complete") => {
            update(|state| {
                state.transfer = None;
                state.resume = None;
                state.cache_bytes = 0;
                state.cache_files = 0;
                state.status = "本章导入完成".into();
                state.status_kind = StatusKind::Good;
            });
            render();
            // 队列里还有下一章就接着传；否则刷新手环上的已安装列表。
            match pop_sync_queue() {
                Some((number, remaining)) => {
                    tracing::info!(number, remaining, "starting queued pack");
                    start_embedded_transfer(number);
                }
                None => astrobox_ng_wit::block_on(async { request_pack_list(false).await }),
            }
            return false;
        }
        Some("amakano.files.error") => {
            let error = message.get("error").and_then(Value::as_str).unwrap_or("unknown").to_string();
            update(|state| {
                if let Some(transfer) = state.transfer.as_mut() {
                    transfer.started = false;
                    transfer.waiting_ready = false;
                    transfer.in_flight.clear();
                }
                state.status = format!("手表导入失败：{error}。重新点「继续同步」会从断点接着传");
                state.status_kind = StatusKind::Bad;
            });
            render();
            return false;
        }
        _ => {}
    }
    send_next
}

fn duplicate(event_id: &str) -> bool {
    let now = now_ms();
    let mut result = false;
    update(|state| {
        result = state.last_action == event_id && now.saturating_sub(state.last_action_ms) < 500;
        if !result {
            state.last_action = event_id.into();
            state.last_action_ms = now;
        }
    });
    result
}

fn mark_action_completed(event_id: &str) {
    update(|state| {
        state.last_action = event_id.into();
        state.last_action_ms = now_ms();
    });
}

// ---------------------------------------------------------------- 存档回包的分片累积

/// 正在收的 `amakano.saves.data` 分片（`seq` 位置的原文 + 判完标志）。
///
/// 回包是**分片信封**：`{seq, total, last, data}`，**载荷在 `data`（字符串）里**，
/// 要按 `seq` 拼起来再 parse 一次。单片只是「恰好只有一片」，走同一条路 ——
/// 一旦为单片写「直接读外层字段」的捷径，等存档涨到 9 KB 以上开始分片时同样的 bug 会复发
/// （2026-09 真机回归：界面永远 0 条，根因就是读了外层）。
///
/// 放 `OnceLock<Mutex<…>>` 与文件里既有的静态缓存（`STATE` / `CACHE`）一个风格，
/// **不动 `State` 结构体**：那会让「界面的只读快照」多出一个纯粹属于收包过程的字段。
static SAVE_SHARDS: OnceLock<Mutex<SaveShardSlot>> = OnceLock::new();

/// 正在收的 `amakano.stats.data` 分片。
///
/// **与存档各占一个坑位**：共用同一个槽的话，统计回包会把存档那些还没收齐的片段
/// 「补齐」成一段坏 JSON（反之亦然），而这种故障在日志里看起来完全正常。
static STATS_SHARDS: OnceLock<Mutex<SaveShardSlot>> = OnceLock::new();

#[derive(Default)]
struct SaveShardSlot {
    /// 这次回包的 `total`（手环说有几片）。
    total: Option<u64>,
    /// 按 `seq` 落位：**到达顺序不算数**（乱序到达也要拼对）。
    parts: BTreeMap<u64, Value>,
    /// 已经收到 `last: true` 的那一片。
    finished: bool,
    bytes: usize,
}

impl SaveShardSlot {
    /// 下一次「读存档列表」开始前清空：留着上一次的碎片只会把新回包拼进旧片段里。
    fn reset(&mut self) {
        self.total = None;
        self.parts.clear();
        self.finished = false;
        self.bytes = 0;
    }

    /// 还有几片没到。手环侧的分片是连续的 `0..total`（`save-sync.js` 的 `sendShards`），
    /// 所以「收到的片数 == total」就等价于「一片不缺」。
    /// **没说过 `total` 时返回 0**（无从判断缺不缺，交给 `finished` 那一半去判）。
    fn missing(&self) -> u64 {
        match self.total {
            Some(total) => total.saturating_sub(self.parts.len() as u64),
            None => 0,
        }
    }

    fn ordered(&self) -> Vec<Value> {
        self.parts.values().cloned().collect()
    }
}

/// 借一下某个分片累积器（`OnceLock` 初始化 + 中毒兜底 + 解锁，只有这一处写法）。
fn with_shards<T>(
    slot: &'static OnceLock<Mutex<SaveShardSlot>>,
    body: impl FnOnce(&mut SaveShardSlot) -> T,
) -> T {
    let mut guard = slot
        .get_or_init(|| Mutex::new(SaveShardSlot::default()))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    body(&mut guard)
}

/// 收一片分片回包。收齐了（`total` 片全到，**且**有一片 `last: true`）就返回
/// **按 `seq` 排好序**的整组；没齐就安静地等着。
///
/// 返回值三种含义：
/// - `Ok(Some(片))` —— 齐了，交给调用方解包（存档走 `band_envelope`，统计走
///   `ReadingStatsView::from_json`）；
/// - `Ok(None)` —— 还没齐（不打状态，免得每片都闪一下）；
/// - `Err(原因)` —— 这次回包**病了**：片号超上限、`total` 前后矛盾、
///   `seq` 重复、或片数够了却没有 `last`。调用方必须把它当人话报出去，**不许静默变 0 条**。
///
/// `what` 只用来写日志与报错（「存档」/「阅读统计」），**两种通道共用这一份实现**：
/// 分片协议是同一条，分成两份迟早会只修好一边。
fn push_shard_into(
    slot: &'static OnceLock<Mutex<SaveShardSlot>>,
    message: &Value,
    what: &str,
) -> Result<Option<Vec<Value>>, String> {
    let seq = message.get("seq").and_then(Value::as_u64).unwrap_or(0);
    let total = message.get("total").and_then(Value::as_u64).filter(|value| *value > 0);
    let last = message.get("last").and_then(Value::as_bool).unwrap_or(false);
    let bytes = message.get("data").and_then(Value::as_str).map(str::len).unwrap_or(0);
    if seq >= MAX_SAVE_SHARDS {
        return Err(format!(
            "手环回包的第 {seq} 片超出上限（最多 {MAX_SAVE_SHARDS} 片），请重试一次"
        ));
    }

    // 下面整段在**一次加锁**里跑完：累积、判完、把结果取出来。解包（可能很慢）
    // 放到锁外做，日志与界面回调也就不会在锁里跑。
    let outcome = with_shards(slot, |slot| {
        // 片数前后矛盾（两套回包混在一帧里）：宁可明说，也不要把两份数据拼成一段坏 JSON。
        if let (Some(seen), Some(expected)) = (total, slot.total)
            && seen != expected
        {
            slot.reset();
            return Err(format!("手环回包前后对不上：先说 {expected} 片，这一片又说 {seen} 片，请重试"));
        }
        if total.is_some() {
            slot.total = total;
        }
        if slot.parts.contains_key(&seq) {
            let had = slot.parts[&seq].get("data").and_then(Value::as_str).map(str::len).unwrap_or(0);
            tracing::warn!(what, seq, had, now = bytes, "band shard repeated: keeping the latest copy");
        } else {
            slot.bytes += bytes;
        }
        slot.parts.insert(seq, message.clone());
        slot.finished |= last;
        let complete = slot.finished && slot.missing() == 0;
        if !complete {
            let (total, have) = (slot.total.unwrap_or(0), slot.parts.len());
            tracing::info!(what, seq, total, have, "band shard buffered");
            return Ok(None);
        }
        Ok(Some((slot.ordered(), slot.bytes)))
    })?;

    let Some((shards, bytes)) = outcome else { return Ok(None) };
    // 收齐了就清空：下一次请求万一没走到对应的 `request_*`（例如手环自己重发），
    // 也不会把新回包拼到这一份已经用过的碎片上。
    with_shards(slot, SaveShardSlot::reset);
    tracing::info!(what, shards = shards.len(), bytes, "band shard reply complete");
    Ok(Some(shards))
}

/// `amakano.saves.data` 的分片累积。
fn push_save_shard(message: &Value) -> Result<Option<Vec<Value>>, String> {
    push_shard_into(&SAVE_SHARDS, message, "存档")
}

/// `amakano.stats.data` 的分片累积。
///
/// **单独一个坑位**：两种回包的碎片绝不能混在同一个槽里 ——
/// 统计回包刚好把存档那份还没收齐的片段「补齐」成一段坏 JSON，是最难查的那种故障。
fn push_stats_shard(message: &Value) -> Result<Option<Vec<Value>>, String> {
    push_shard_into(&STATS_SHARDS, message, "统计")
}

/// 把分片信封拼成载荷对象（**唯一一处**这么做的地方）。
///
/// 单片只是「恰好一片」，走的是同一条路 —— 为单片写「直接读外层」的捷径，
/// 等数据涨过 9 KB 开始分片时同一个 bug 会原样复发（存档那次就是这么踩的）。
fn shard_payload(shards: &[Value]) -> Result<Value, String> {
    let mut text = String::new();
    for (index, shard) in shards.iter().enumerate() {
        match shard.get("data") {
            Some(Value::String(piece)) => text.push_str(piece),
            Some(other) => text.push_str(&other.to_string()),
            None => return Err(format!("手环回包格式不对（第 {index} 片）：缺少 data 字段")),
        }
    }
    serde_json::from_str(&text).map_err(|error| {
        format!(
            "手环回包格式不对（{} 片拼起来共 {} 字节，不是合法 JSON）：{error}",
            shards.len(),
            text.len()
        )
    })
}

/// 统计读取失败：界面上一句人话 + 一条日志（绝不允许静默）。
fn fail_stats(hint: &str) {
    tracing::warn!(hint, "band stats read failed");
    update(|state| {
        state.stats_busy = false;
        state.stats_error = hint.to_string();
        state.status = hint.to_string();
        state.status_kind = StatusKind::Bad;
    });
    render();
}

/// 界面事件分派。
///
/// 四类分开处理，别混在一起：
/// - 悬停 / 按下：纯界面动作，**状态没变就不重绘**（这两类事件又密又快，
///   每次重绘都要重建整棵树），也不打状态行；
/// - 切页：纯界面动作，同样不打状态行，否则切页时状态行会闪；
/// - 业务动作：先亮状态、再干活、最后记进去重表（宿主 Click 与 PointerUp 会各发一次）。
fn dispatch_ui_event(event_id: &str, kind: event::Event) {
    let Some(action) = parse_action(event_id) else { return };
    let pointer = matches!(kind, event::Event::Click | event::Event::PointerUp);
    let down = matches!(kind, event::Event::PointerDown);
    let hover_event = matches!(kind, event::Event::MouseEnter | event::Event::MouseLeave);

    if action.is_ui_only() {
        let changed = match &action {
            Action::Hover(_) if hover_event => {
                let moved = set_hover(&action);
                // 鼠标移出顺手把按下态收回：元素树收不到「在按钮外松开」。
                let released = clear_pressed();
                moved || released
            }
            Action::Press(_) if down => set_pressed(&action),
            _ => false,
        };
        if changed {
            render();
        }
        return;
    }
    if matches!(&action, Action::Nav(_)) {
        if pointer {
            clear_pressed();
            handle_action(action);
            render();
        }
        return;
    }
    if !pointer || duplicate(event_id) {
        return;
    }
    clear_pressed();
    set_status(StatusKind::Info, "正在处理…");
    render();
    handle_action(action);
    mark_action_completed(event_id);
}

/// 悬停目标变更，返回是否真的变了。
fn set_hover(action: &Action) -> bool {
    let Action::Hover(target) = action else { return false };
    let next = (!target.is_empty()).then(|| target.clone());
    update(|state| {
        if state.hover == next {
            return false;
        }
        state.hover = next;
        true
    })
}

/// 按下态变更，返回是否真的变了。
///
/// **只改按下态，不做别的**：0.5.0 这里曾经顺手把导航项的「按下」当成切页
/// （为了让内容以偏移态抢渲染一帧、靠 transition 补出切页滑动）。
/// 真机上那套「两帧 + transition」很卡，已整块撤掉，切页**恢复成只在 Click 时发生**
/// （见下面的 `Action::Nav(_)` 分支），所以这里不再需要解析动作类型。
fn set_pressed(action: &Action) -> bool {
    let Action::Press(target) = action else { return false };
    let next = target.clone();
    update(|state| {
        if state.pressed.as_ref().is_some_and(|(id, _)| *id == next) {
            return false;
        }
        state.pressed = Some((next, now_ms()));
        true
    })
}

/// 收回按下态，返回是否真的改了。
fn clear_pressed() -> bool {
    update(|state| state.pressed.take().is_some())
}

/// 按 [`Action::refresh_plan`] 说的去拉列表 —— **「刷什么、按什么顺序刷」都只有一处判定**
/// （ui-core 的 `refresh_plan` / `refresh_steps`，宿主机上有守护用例），这里只照做。
///
/// **串行**：`State::request` 只有一个坑位，两个请求一起挂出去的话，后发的那个会把先发的
/// 静静顶掉 —— 被顶掉的那个既不会重发、也不会报超时，回包迟到也没人替它兜底。
/// 而这两者谁更需要兜底很清楚：实测存档列表「第一次就成」的比例只有 18.2%，
/// 章节列表 94.1%。所以 ui-core 把**存档排在最后**发（先章节列表，它 settle 了再发存档），
/// 让最需要重试的那个拿到坑位。旧实现是存档先发、章节列表后发，正好反了。
fn run_refresh(plan: RefreshPlan) {
    let mut steps = refresh_steps(plan);
    if steps.is_empty() {
        return;
    }
    let first = steps.remove(0);
    // 先把「还没发的」排进队列，再发第一个：第一个的回包可能来得很快，
    // 那时 `settle_refresh_step` 得能从队列里拿到下一步。
    update(|state| state.refresh_queue = steps);
    astrobox_ng_wit::block_on(async { run_refresh_step(first).await });
}

/// 执行一个动作。
fn handle_action(action: Action) {
    match action {
        Action::Nav(page) => {
            update(|state| {
                state.page = page;
                state.hover = None;
            });
            // 切到「统计」页时顺手拉一次：用户不必再点一下「读取统计」。
            // 已经读到过就不重复拉 —— 每次切页都打一次手环是白花往返。
            // ⚠️ 同样**不在回调里立刻发**，隔一拍（理由见 `request::FOLLOWUP_DELAY_MS`）。
            if page == Page::Stats {
                let need = {
                    let current = state().lock().unwrap_or_else(|error| error.into_inner());
                    current.stats.is_none()
                        && current.save_protocol.is_some()
                        && current.stats_supported
                        && !current.stats_busy
                };
                if need {
                    arm_timer(FOLLOWUP_DELAY_MS, json!({ "type": "amakano.timer", "kind": "stats-list" }).to_string());
                }
            }
        }
        Action::Hover(_) | Action::Press(_) => {}
        Action::Connect => connect_device(),
        Action::Launch => launch_app(),
        Action::SyncAll => {
            let queue: Vec<usize> = {
                let current = state().lock().unwrap_or_else(|error| error.into_inner());
                current
                    .library
                    .iter()
                    .filter(|pack| !current.installed.iter().any(|item| item.id == pack.id))
                    .map(|pack| pack.number)
                    .collect()
            };
            queue_sync(queue);
        }
        Action::Sync(number) => start_embedded_transfer(number),
        Action::RefreshList => run_refresh(action.refresh_plan()),
        Action::ClearCache => astrobox_ng_wit::block_on(async { request_clear_cache().await }),
        // 章节包删除**一步到位**：插件里有一份完整副本，删掉随时能重新同步回来，
        // 再要一次确认只是白加一步（用户要求「减少不必要的二次确认」）。
        // 存档删除不一样 —— 那是不可恢复的，所以只有它保留了确认（见 `SavesDelete`）。
        Action::Delete(pack_id) => {
            astrobox_ng_wit::block_on(async { request_delete_pack(&pack_id).await });
        }
        Action::Chunk(size) => {
            set_chunk_size(size);
        }
        Action::AutoLaunch(enabled) => {
            update(|state| state.auto_launch = enabled);
            set_status(
                StatusKind::Info,
                if enabled {
                    "连接后会顺便打开《甜蜜女友2》"
                } else {
                    "连接后不再自动打开游戏，需要手动打开"
                },
            );
        }
        Action::Line(line) => update(|state| state.line_filter = line),
        Action::LogFilter(filter) => update(|state| state.log_filter = filter),
        Action::LogClear => {
            logger::clear();
            set_status(StatusKind::Info, "已清空日志");
        }
        // ---- 存档 ----
        Action::SavesRefresh => {
            update(|state| {
                state.saves_error.clear();
                state.saves_notice.clear();
                state.saves_busy = true;
            });
            // 「刷什么」由 ui-core 判定（`Action::refresh_plan`，宿主机上有守护用例）：
            // 存档页的刷新**两样都刷** —— 只刷存档的话，已安装章节列表一变，用户点刷新
            // 仍然看到旧结论（2026-09 真机就是这么报上来的）。
            run_refresh(action.refresh_plan());
        }
        // ---- 阅读统计 ----
        Action::StatsRefresh => {
            update(|state| {
                state.stats_error.clear();
                state.stats_busy = true;
            });
            // 「刷什么」同样只有 ui-core 一处判定（统计页只刷统计）。
            run_refresh(action.refresh_plan());
        }
        Action::SavesExport => start_save_export(),
        Action::SavesImport => start_save_import(),
        Action::SavesLoad(slot) => {
            let Some(slot) = slot else {
                // 自动存档没有「读档」语义（它本来就是继续阅读的进度）。
                set_status(StatusKind::Info, "自动存档就是「继续阅读」，不用手动读");
                return;
            };
            update(|state| {
                state.saves_error.clear();
                state.saves_busy = true;
            });
            astrobox_ng_wit::block_on(async { request_save_activate(slot).await });
        }
        // 存档删除要点两次：第一次只是把这一行切到「确认删除」——
        // 存档没了就真没了，这一步不能省（章节包删除相反，见上面的 `Action::Delete`）。
        Action::SavesDelete(slot) => {
            update(|state| state.confirm_delete_save = Some(saves::slot_key(slot)));
            set_status(StatusKind::Warn, "再点一次「确认删除」才会真的从手环上删掉这条存档");
        }
        Action::SavesConfirmDelete(slot) => {
            update(|state| state.confirm_delete_save = None);
            match slot {
                Some(slot) => {
                    update(|state| {
                        state.saves_error.clear();
                        state.saves_busy = true;
                    });
                    astrobox_ng_wit::block_on(async { request_save_delete(slot).await });
                }
                // 自动存档不给从插件里删：它是游戏自己的断点，删了等于「继续阅读」失效，
                // 而那件事在手环上做更清楚（游戏里玩到新进度就会覆盖它）。
                None => set_status(StatusKind::Warn, "自动存档不能在这里删除：手环上继续玩一段就会覆盖它"),
            }
        }
        Action::SavesCancelDelete => {
            update(|state| state.confirm_delete_save = None);
            set_status(StatusKind::Info, "已取消删除");
        }
    }
}

// ------------------------------------------------------------------ 存档：导出 / 导入
//
// 两条入口都是「分发器里只改状态 + 渲染一帧 + spawn 出真正的活儿 + 立刻返回」：
// **`on_ui_event` / `on_event` 是宿主的事件分发线程，必须立刻把 FutureReader 还回去。**
//
// 这一条不是预防性写法，是这一轮真机实测换来的教训（见 docs/插件开发注意事项.md 第 7 节）：
// 0.5.1 的导出/导入走 Dialog（`save_file_start` / `pick_file`），而 Dialog **需要用户交互**——
// 在本宿主上一次**永远不返回**：点「测试导出对话框」后日志只有 `dialog probe started`、
// 没有 finished，随后连与对话框毫无关系的「测试插件目录可写」也不再有 finished，
// 整个插件只能靠禁用再启用救回来。分发器被堵死之后，所有点击都石沉大海。
//
// 所以现在的硬规矩是：**任何可能等待用户交互的宿主调用，都不许跑在事件分发器里**
// （哪怕看起来「很快」）；而在 spawn 里也只做「不等用户」的事 —— 剪贴板读写正好属于这一类，
// 它不弹任何窗口、不等任何点击。`block_on` 仍然只用于**等回包**这种短活儿（现有代码同款）。

/// 导出：把当前存档打成信封，写进剪贴板。
///
/// `Dialog`（用户选路径）这条路已经作废，剪贴板是新的落点 —— 用户自己把那段 JSON 粘到
/// 备忘录 / 聊天窗口 / 文本文件里就完成了「保存」。
fn start_save_export() {
    let (auto_save, slots) = {
        let current = state().lock().unwrap_or_else(|error| error.into_inner());
        (current.save_auto.clone(), current.save_slots.clone())
    };
    if auto_save.is_none() && slots.is_empty() {
        set_status(StatusKind::Warn, "手环上没有可导出的存档，先点「刷新」");
        render();
        return;
    }
    let version = state().lock().unwrap_or_else(|error| error.into_inner()).version.clone();
    let version_code = plugin_version_code();
    // 信封字段不变（format / save_version / protocol / exported_at / app_version /
    // version_code / auto_save / slots），槽位对象**原样**带走。
    let envelope = saves::SaveFile::new(auto_save, slots, version, version_code);
    let text = match envelope.to_json() {
        Ok(text) => text,
        Err(error) => {
            set_status(StatusKind::Bad, error.message());
            render();
            return;
        }
    };
    let summary = envelope.summary();
    let slot_count = envelope.slots.len() + usize::from(envelope.auto_save.is_some());
    let (bytes, kb) = (text.len(), text.len().div_ceil(1024));
    // 剪贴板没有文件名这回事，但用户总得给这份存档起个名 —— 直接给一个现成的。
    let hint_name = saves::default_file_name(envelope.exported_at);

    update(|state| {
        state.saves_busy = true;
        state.saves_error.clear();
        state.saves_notice = format!("正在把 {summary} 复制到剪贴板…");
    });
    set_status(StatusKind::Info, "正在复制到剪贴板…");
    render();
    tracing::info!(bytes, slots = slot_count, name = %hint_name, "save export to clipboard started");

    astrobox_ng_wit::spawn(async move {
        match clipboard::write_envelope(&text).await {
            Ok((bytes, verified)) => {
                tracing::info!(
                    bytes,
                    slots = slot_count,
                    verified,
                    name = %hint_name,
                    "save export to clipboard finished"
                );
                let notice = format!(
                    "已复制 {slot_count} 个存档到剪贴板（约 {kb} KB / {bytes} 字节）{verify}；建议命名 {hint_name}，粘到备忘录等地方保存",
                    verify = if verified { "，已读回核对" } else { "" },
                );
                update(|state| {
                    state.saves_busy = false;
                    state.saves_notice = notice.clone();
                    // 字节数是给用户看的第一手证据（证明剪贴板里是一整份）；
                    // 读回核对只影响这句的颜色与半句补充，不影响导出是否算成功。
                    state.save_export = Some((bytes, slot_count, verified));
                    state.status = if verified {
                        notice
                    } else {
                        format!("{notice}（未能读回核对，但写入是成功的）")
                    };
                    state.status_kind = if verified { StatusKind::Good } else { StatusKind::Warn };
                });
                render();
            }
            Err(error) => {
                // 失败**不静默**：一句人话进界面 + 一条日志（这轮就是靠日志定位的）。
                tracing::warn!(reason = error.message(), "save export to clipboard failed");
                update(|state| {
                    state.saves_busy = false;
                    state.saves_notice = format!("导出未完成：{}", error.message());
                    state.save_export = None;
                    state.status = state.saves_notice.clone();
                    state.status_kind = StatusKind::Bad;
                });
                render();
            }
        }
    });
}

/// 导入：读剪贴板文本 → 解析信封 → 按 `savedAt` 并入手环。
fn start_save_import() {
    update(|state| {
        state.saves_busy = true;
        state.saves_error.clear();
        state.saves_notice = "正在从剪贴板读取存档…".into();
        state.save_import = None;
    });
    set_status(StatusKind::Info, "正在从剪贴板读取存档…");
    render();
    tracing::info!("save import from clipboard started");

    astrobox_ng_wit::spawn(async move {
        let text = match clipboard::read_envelope_text().await {
            Ok(text) => text,
            Err(error) => {
                tracing::warn!(reason = error.message(), "save import could not read the clipboard");
                fail_save_import(error.message(), true);
                return;
            }
        };
        let file = match saves::parse_save_text(&text, "剪贴板") {
            Ok(file) => file,
            Err(error) => {
                // 空剪贴板 / 不是 JSON / 格式不对 / 版本过高 / 一个槽都没有 —— 都是同一句人话的来源。
                tracing::warn!(chars = text.len(), reason = error.message(), "clipboard is not an importable save");
                fail_save_import(error.message(), false);
                return;
            }
        };
        // 手环上已有的存档槽：按写档时间判「重复」（和手环侧 upsert 同一把尺子）。
        let existing = state().lock().unwrap_or_else(|error| error.into_inner()).save_slots.clone();
        let merge = saves::merge_slots(&existing, &file.slots);
        let (incoming, duplicates, band_before) = (merge.incoming, merge.duplicates, merge.existing);
        // 防御：手环端的存档是「下标即槽号」，导入一份超出上限的文件会让列表失控。
        // 这里直接截断并说清楚截了多少，而不是悄悄写进去。
        let truncated = if incoming > MAX_SAVE_SLOTS {
            Some(incoming)
        } else {
            None
        };
        if let Some(count) = truncated {
            tracing::warn!(count, kept = MAX_SAVE_SLOTS, "clipboard save file has more slots than the band supports");
        }
        // 导出方是哪一版：只在信封版本 / 插件版本与现在不一致时说一句，供排障用。
        let from = if file.save_version == saves::SAVE_VERSION && file.app_version == plugin_version() {
            String::new()
        } else {
            format!("（信封版本 {} · 插件 {}）", file.save_version, file.app_version)
        };
        tracing::info!(
            raw_chars = text.len(),
            incoming,
            duplicates,
            band_before,
            auto = file.auto_save.is_some(),
            "clipboard save parsed, writing to band"
        );

        update(|state| {
            state.saves_notice = format!("从剪贴板读到 {incoming} 个存档{from}，正在写入手环…");
            state.save_import = Some((incoming, duplicates, band_before, truncated.unwrap_or(incoming)));
        });
        render();

        // 导入走 upsert：按 `savedAt` 认同一份存档（有就覆盖、没有就追加到末尾），
        // **不会把用户手环上原有的存档一把清掉**。整表替换（replace）留给将来的「还原」。
        // 重复的那些**照样发给手环**：覆盖回来才是用户要的合并（`amakano.saves.put-ok`
        // 那一支会把「写进去几条 / 跳过几条重复」念给用户听）。
        let mut slots = merge.slots;
        slots.truncate(MAX_SAVE_SLOTS);
        request_save_put("upsert", slots, file.auto_save).await;
    });
}

/// 导入失败：界面上一句人话 + 状态行同款。`hard` 表示连剪贴板都没读到（读失败）。
fn fail_save_import(reason: &str, hard: bool) {
    let notice = format!("导入未完成：{reason}");
    update(|state| {
        state.saves_busy = false;
        state.save_import = None;
        state.saves_notice = notice.clone();
        state.status = notice;
        // 读剪贴板失败更像环境问题（权限/焦点），给警告色；内容不对是用户操作问题，给错误色。
        state.status_kind = if hard { StatusKind::Warn } else { StatusKind::Bad };
    });
    render();
}

/// 插件 manifest 里的 `versionCode`：存档信封里记一下来源，方便排查「这是哪一版导出的」。
fn plugin_version_code() -> u32 {
    fs::read_to_string("manifest.json")
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value.get("versionCode").and_then(Value::as_u64))
        .map_or(0, |code| code as u32)
}

struct ImportPlugin;

impl event::Guest for ImportPlugin {
    fn on_event(event_type: EventType, payload: String) -> FutureReader<String> {
        let (writer, reader) = astrobox_ng_wit::wit_future::new::<String>(String::new);
        let is_interconnect = matches!(event_type, EventType::InterconnectMessage);
        if is_interconnect {
            tracing::info!(bytes = payload.len(), payload = %payload, "received interconnect message");
        }
        let send_next = is_interconnect && handle_interconnect(&payload);
        if send_next {
            astrobox_ng_wit::block_on(async { send_next_packet().await });
        }
        if matches!(event_type, EventType::Timer) {
            // 宿主派发定时器事件时会把我们传入的字符串包一层信封：
            // `{"timerId":1,"kind":"timeout","payload":"<我们传入的字符串>"}`
            // （见官方 Timer 接口文档），所以这里先拆信封再分发。
            let inner = parse_event_payload(&payload);
            if let Some(action) = retry_packet(&inner) {
                match action {
                    RetryAction::Resend(addr, packet, index) => {
                        render();
                        astrobox_ng_wit::block_on(async { send_packet(addr, packet, index, true).await });
                    }
                    RetryAction::Failed => {}
                }
            } else {
                astrobox_ng_wit::block_on(async { handle_timer(&inner).await });
            }
        }
        astrobox_ng_wit::spawn(async move {
            let _ = writer.write("accepted".into()).await;
        });
        reader
    }

    fn on_ui_event(event_id: String, event: event::Event, _payload: String) -> FutureReader<String> {
        let (writer, reader) = astrobox_ng_wit::wit_future::new::<String>(String::new);
        dispatch_ui_event(&event_id, event);
        astrobox_ng_wit::spawn(async move {
            let _ = writer.write("accepted".into()).await;
        });
        reader
    }

    fn on_ui_render(element_id: String) -> FutureReader<()> {
        let (writer, reader) = astrobox_ng_wit::wit_future::new::<()>(|| ());
        update(|state| state.element_id = Some(element_id));
        render();
        astrobox_ng_wit::spawn(async move {
            let _ = writer.write(()).await;
        });
        reader
    }

    fn on_card_render(_card_id: String) -> FutureReader<()> {
        let (writer, reader) = astrobox_ng_wit::wit_future::new::<()>(|| ());
        astrobox_ng_wit::spawn(async move {
            let _ = writer.write(()).await;
        });
        reader
    }
}

impl lifecycle::Guest for ImportPlugin {
    fn on_load() {
        logger::init();
        tracing::info!("Amakano2 pack importer loaded");
        let version = plugin_version();
        update(|state| state.version = version.clone());
        match load_library() {
            Ok(packs) => {
                tracing::info!(count = packs.len(), summary = %library_summary(&packs), %version, "chapter packs available");
                update(|state| {
                    state.library = packs;
                    state.library_error.clear();
                    state.status = "点「连接设备」开始：插件已内置全部章节包".into();
                });
            }
            Err(message) => {
                tracing::error!(%message, "chapter pack library unavailable");
                update(|state| {
                    state.library_error = message;
                    state.status = "插件内的章节包不可用".into();
                    state.status_kind = StatusKind::Bad;
                });
            }
        }
    }
}

astrobox_ng_wit::export!(ImportPlugin with_types_in astrobox_ng_wit);
}

// ------------------------------------------------------------------ 与界面契约的守护用例
//
// 这些用例在**宿主机**上跑：`cargo test -p amakano2-import --target x86_64-pc-windows-msvc`。
// 存档信封本身的解析/校验在 `host/saves.rs` 里测（那里是纯逻辑，导入导出都走它）；
// 这里守的是**插件与手环回包之间那层信封**（`amakano.saves.data` 的三层结构）
// 与**插件与界面之间那几句话**（「手环端应用过旧」怎么讲、槽位键怎么对）。
//
// 回包解包本身在 `amakano2-ui` 的 `saves` 模块里测（那里能直接跑），下面的用例
// 用的是**同一份真机回包夹具**（`amakano2_ui::BAND_SAVES_FIXTURE`，由
// `tools/gen-band-fixture.mjs` 生成、手环侧测试也在读同一份）。
#[cfg(test)]
mod tests {
    use crate::{MAX_SAVE_SLOTS, SAVE_PROTOCOL, saves};

    /// 真机回包（小米手环 10，3 条手动槽 + 1 条自动存档）拍成的界面行。
    ///
    /// 走的是**插件渲染时的同一条路**：原始存档对象进快照，`Snapshot::saves()` 用当前
    /// 已安装章节列表现算（插件 `snapshot()` 干的正是这件事，插件的 `State` 里没有
    /// 也不用有「算好的行」）。
    fn real_capture_rows(installed: &[String]) -> Vec<amakano2_ui::SaveSlotView> {
        let message: serde_json::Value = serde_json::from_str(amakano2_ui::BAND_SAVES_FIXTURE)
            .expect("真机回包夹具必须是合法 JSON");
        let band = amakano2_ui::band_envelope(&[message]).expect("真机回包必须能解包");
        let mut snapshot = amakano2_ui::Snapshot::default();
        snapshot.save_auto = band.auto_save;
        snapshot.save_slots = band.slots;
        snapshot.installed = installed
            .iter()
            .map(|id| amakano2_ui::InstalledView {
                id: id.clone(),
                name: id.clone(),
                bytes: 0,
                files: 0,
                stale: false,
            })
            .collect();
        snapshot.saves()
    }

    #[test]
    fn real_band_reply_becomes_one_auto_save_plus_three_slots() {
        // 2026-09 真机回归的守护：界面显示 0 条，就是因为插件从**外层**读字段。
        // 这条用例拿真机回包当输入（不是自己编样本），走插件侧的入口函数。
        let rows = real_capture_rows(&["共通线1·归乡".to_string()]);
        assert_eq!(rows.len(), 4, "1 条自动存档 + 3 条手动槽");

        // 第 0 条永远是自动存档，而且不带槽号（界面靠这个决定哪一行不给「读档」）。
        assert!(rows[0].is_auto());
        assert_eq!(rows[0].slot, None);
        assert_eq!(rows[0].title(), "自动存档");
        assert_eq!(rows[0].chapter, "共通线 第一章·归乡");
        assert_eq!(rows[0].pack_id, "共通线1·归乡");
        assert_eq!(rows[0].pack_scene, 23, "读档真正靠的是包内偏移，原样带出来");
        assert_eq!(rows[0].scene, 24, "展示用场景号 = currentScene + 1");
        assert_eq!(rows[0].saved_at, 1_789_196_000_769);
        assert_eq!(rows[0].time_label().len(), 11, "MM-DD HH:MM");

        // 手动槽的**槽号就是数组下标**（删除/读档把它原样发回手环）。
        assert_eq!(rows[1].slot, Some(0));
        assert_eq!(rows[1].title(), "存档 1");
        assert_eq!(rows[1].pack_scene, 1);
        assert_eq!(rows[2].slot, Some(1));
        assert_eq!(rows[2].pack_scene, 4);
        assert_eq!(rows[3].slot, Some(2));
        assert_eq!(rows[3].pack_scene, 26);
        assert_eq!(rows[3].saved_at, 1_789_196_006_924);

        // 手环上装了第 1 章，所以四条**都能读**；没装时四条都要标未安装。
        assert!(rows.iter().all(|row| row.readable() && !row.missing));
        let missing = real_capture_rows(&[]);
        assert!(missing.iter().all(|row| row.missing && !row.readable()), "没装这一章就该标未安装");
    }

    #[test]
    fn real_capture_export_envelope_keeps_the_slot_objects_verbatim() {
        // 「导出/导入」那条路：真机回包解出来的存档**原样**进导出信封（缩进版 JSON），
        // 再原样解析回来。存档对象逐字段往返，是「备份能换回老进度」的前提。
        let message: serde_json::Value = serde_json::from_str(amakano2_ui::BAND_SAVES_FIXTURE)
            .expect("真机回包夹具必须是合法 JSON");
        let band = amakano2_ui::band_envelope(&[message]).expect("真机回包必须能解包");
        let envelope = saves::SaveFile::new(band.auto_save.clone(), band.slots.clone(), "0.1.0".into(), 65);
        let text = envelope.to_json().expect("导出信封必须能序列化");
        let parsed = saves::parse_save_text(&text, "剪贴板").expect("自己导出的信封必须能读回来");

        assert_eq!(parsed.format, saves::SAVE_FORMAT);
        assert_eq!(parsed.save_version, saves::SAVE_VERSION);
        assert_eq!(parsed.slots, band.slots, "槽位对象逐字段原样往返");
        assert_eq!(parsed.auto_save, band.auto_save);
        assert_eq!(parsed.slots.len(), 3);
        assert!(parsed.summary().contains("3 个手动存档"));
        // 3 条都在上限之内 —— 导入不会被截断（界面上那句「超出 N 槽没写入」不该出现）。
        assert!(parsed.slots.len() <= MAX_SAVE_SLOTS);
        // 导出的信封里 `slots` 是**数组**（手环回包里的同名字段是数字，两者别混）；
        // 顺带确认导出信封没有把回包的 `data` 那一层带进来。
        let value: serde_json::Value = serde_json::from_str(&text).expect("导出的信封是 JSON");
        assert!(value["slots"].is_array(), "导出信封的 slots 是数组：{}", value["slots"]);
        assert!(value.get("data").is_none(), "导出信封不该有回包那层 data 字段");
    }

    #[test]
    fn hello_ok_absence_is_explained_as_an_old_band_app() {
        // 守住「手环端应用过旧」那句人话：用户看到的必须是**可行动的建议**，
        // 不能只报一句超时（旧手环根本不会回 hello-ok，超时是必然的）。
        let snapshot = amakano2_ui::Snapshot::default();
        let hint = snapshot.saves_blocked_hint();
        assert!(hint.contains("手环端应用版本过旧"), "{hint}");
        assert!(hint.contains("更新后才能管理存档"), "{hint}");
        // 而且这句话只说一遍：插件侧只置状态位，不再往 `saves_error` 里写同义的第二句
        // （用户实机截图里那张卡就是两句话并排）。第二行是「怎么办」，不重复上面的判断。
        assert_eq!(hint.matches("手环端应用版本过旧").count(), 1, "{hint}");
        let action = snapshot.saves_blocked_action();
        assert!(action.contains("新版 RPK"), "{action}");
        assert!(!action.contains("版本过旧"), "「怎么办」不许重复结论：{action}");
        // 协商成功之后就不该再出现这句话。
        let mut ready = amakano2_ui::Snapshot::default();
        ready.save_protocol = Some(1);
        assert!(ready.saves_blocked_hint().is_empty());
        assert!(ready.saves_blocked_action().is_empty());
    }

    #[test]
    fn save_protocol_and_slot_limit_are_the_documented_numbers() {
        // 手环侧 `save-sync.js` 里的 PROTOCOL 必须与这里一致（两边各写一份是刻意的：
        // 一个是 JS、一个是 Rust，没法共享常量），改一处就要改另一处 ——
        // tests/save-sync.test.js 里有对应的那一条守护用例。
        assert_eq!(SAVE_PROTOCOL, 1);
        // 槽位上限只是「导入时别写失控」的防御，界面也拿它做「x / N 槽」的显示。
        assert_eq!(MAX_SAVE_SLOTS, 20);
        assert_eq!(amakano2_ui::MAX_SLOTS, MAX_SAVE_SLOTS, "界面与插件的槽位上限必须一致");
    }

    #[test]
    fn import_notice_counts_written_and_skipped_saves() {
        // 导入之后界面上那句话的口径：**写进去的** ＝ 信封条数 − 与手环重复的条数。
        // 重复的那些会被覆盖（不是丢），所以「跳过」这个词在这句里是准确的。
        let notice = amakano2_ui::SaveImportView { incoming: 5, duplicates: 2, existing: 3 }.notice();
        assert_eq!(notice, "导入 3 个存档，跳过 2 个重复（手环上现在有 6 条）");
    }
}
