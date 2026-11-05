#![recursion_limit = "256"]
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
mod cloud;
// 云服务器地址的安全判定（纯逻辑，宿主机可测）。见 `src/net.rs` 的模块注释：
// 配置里那个地址一旦被改成本机/内网，设备凭证就会被送过去。
mod net;

// 请求通道的纯逻辑（请求种类、按种类的超时策略、单坑位）。同样不条件编译：
// 「存档的超时该比章节列表宽」「迟到的回包不能算超时」都要在宿主机上有守护用例。
mod request;

// 传输模式的参数表（`TransferMode` / `ModeParams`）住在 ui-core：界面要按它写说明，
// 发片要按它定窗口与节拍，**只能有一份**。宿主机上跑 `cargo test` 时也要读它，
// 所以引入挂在 crate 根、不条件编译。
use amakano2_ui::{ModeParams, TransferMode, chunk_label};

// ---- 两个半边都要用到的常量 ----
//
// `SAVE_PROTOCOL` 同时被 `host`（hello 协商）与**单元测试**读，所以挂在这里；
// 只在宿主侧用得到的常量（包名、章节包清单路径）就留在 `host` 模块里 ——
// 否则宿主 target 上会报「never used」。
//
// 手环的 `recoveryData` 没有固定槽数上限，导入不能再按 20 条截断。
// 真正的数据边界是单条存档与整份云存档的字节数（见 `cloud.rs`）。
/// 与手环协商的存档协议版本。手环在 `amakano.app.hello-ok` 里回它自己的版本，
/// **缺席就等于「手环端应用太旧」**，界面上要给人话而不是超时。
const SAVE_PROTOCOL: u32 = 1;

// ---- 飞行窗口 / 组批 / 超时（照《穿戴端推送插件优化手册》§2、§3.2、§4.1、§6.3）----
//
// 这几个量与握手协商、超时判定、组批一起构成**协议的一半**，所以挂在 crate 根：
// 宿主机上要有守护用例（`cargo test`），而宿主侧（wasm）那一半读同一份。

/// 传输的初始飞行窗口（在途未确认的分片数）—— **正常档**的值。
///
/// 两档的完整数字住在 `amakano2_ui::TransferMode::params()`（那里唯一一份，
/// 界面按它写说明、插件按它发片）。**改参数改那一份表**，别在这里另养一套。
///
/// v3（手环 `syncProtocol >= 3`）：正常档 6 片起、上限 10 片，同时受在途字节约束。
/// 确认是累计的，窗口里只要还有空间就一直发 —— 旧的 3 片窗口把 BLE 往返当节拍器。
///
/// 旧手环（协议 2）仍用 3 片：那边是逐片确认，窗口再大也只会堆在它内存里。
const INITIAL_WINDOW: usize = TransferMode::Steady.params().initial_window;
const LEGACY_WINDOW: usize = 3;
/// 手环上报的接收窗口与我们的飞行窗口之间留的余量（手环对超窗的片回 gap）。
const WINDOW_MARGIN: usize = 2;

/// 本次传输实际要用的参数（模式 × 手环能力）。
///
/// **只在建传输时算一次**，之后从 `Transfer` 上读 —— 传输对象活得比一次能力协商长
/// 的情况（换台手环、断线重连）由 `start_transfer` 重建来覆盖。
///
/// 宿主机上（`cargo test`）宿主模块整段不编译，所以它只在那里「没人用」。
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[derive(Clone, Copy)]
struct TransferLimits {
    mode: TransferMode,
    params: ModeParams,
    /// 实际分片（用户档位与手环上限取小）。
    chunk_bytes: usize,
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
impl TransferLimits {
    fn new(mode: TransferMode, chunk_bytes: usize) -> Self {
        let params = mode.params();
        Self { mode, params, chunk_bytes }
    }
}

/// 一条消息的线上尺寸上限：以模式的上限为准，但分片被压低（旧手环只收 16 KB）时
/// 不必留那么宽 —— 闸门的意义是「拦住异常」，不是「永远留一半余量」。
fn wire_limit(params: ModeParams, chunk_bytes: usize) -> usize {
    params.wire_bytes.max(chunk_bytes.div_ceil(3) * 4 + 1024)
}

/// 由手环在 `hello-ok` 里报的 `receiveWindow` 与 `syncProtocol` 定本次会话的飞行窗口。
///
/// v3：`receiveWindow - 2`，夹在 `模式起始值..模式上限` 之间（手环侧真实上限 12）。
/// 协议 2（旧应用）退回 3 片 —— 那边是逐片确认，窗口再大只会堆在它内存里。
/// **协商，不靠版本号猜**，与 `ack: 'batch'` 同一条规矩。
fn negotiated_window(mode: TransferMode, receive_window: Option<u64>, protocol: u32) -> usize {
    if protocol < 3 {
        return receive_window
            .map(|value| (value as usize).saturating_sub(1).clamp(1, LEGACY_WINDOW))
            .unwrap_or(LEGACY_WINDOW);
    }
    let params = mode.params();
    match receive_window {
        // 手环报多少就信多少（留 2 片余量），只保底 2 片避免停等。
        Some(value) => (value as usize)
            .saturating_sub(WINDOW_MARGIN)
            .clamp(2, params.window_cap),
        None => params.initial_window.min(params.window_cap),
    }
}

/// v3 的停滞判定：多久没收到**累计确认**就把窗口从 `acked_index` 重发。
///
/// 与 v2 的「每片一个超时」不同，这里只有一条计时：正常档每写 4 片或 90 ms 就会回一条，
/// 所以正常路径下这个值总是远大于实际间隔。给 `rtt × 2 + 900ms`，再夹到模式的下限之上、
/// 5 秒（正常）/ 12 秒（激进）之下：
///
/// * 正常档：手环写 16 KB 实测 19 ms，整窗 10 片也只要 200 ms，5 秒是很宽的余量；
/// * 激进档：每 1.2 秒才报一次位点，判定下限必须远大于它（见 `stall_floor_ms`），
///   否则**自己的确认节拍会被判成停滞**，变成「整窗重发 → 再整窗重发」的死循环。
fn ack_timeout_ms(mode: TransferMode, rtt_ms: u64) -> u64 {
    let params = mode.params();
    let ceiling = (params.stall_floor_ms).max(5000);
    if rtt_ms == 0 {
        return (params.stall_floor_ms + 1000).min(ceiling);
    }
    rtt_ms
        .saturating_mul(2)
        .saturating_add(900)
        .clamp(params.stall_floor_ms, ceiling)
}

/// 现在真正生效的传输模式，以及「激进档被拒」的原因。
///
/// **能力现算，不落盘、不抄进传输对象**：换台手环、手环上换了旧版应用，都要立刻跟着回退
/// （同一条规矩见 `capabilities 现读现用`）。判定只有这一处，界面与发片都问它。
///
/// 激进档要求手环同时具备三样：
///   · `syncProtocol >= 3`（累计确认 + `gap`，粗确认才有恢复路径）；
///   · `maxChunkBytes >= 32768`（它认的最大单片；报得小说明它的接收缓冲不够）；
///   · `begin` 里能收到 `mode`（老应用静默忽略，所以只能靠上面两项间接判断）。
fn effective_mode(
    requested: TransferMode,
    protocol: u32,
    band_max_chunk: usize,
    have_capabilities: bool,
) -> (TransferMode, String) {
    if requested != TransferMode::Turbo {
        return (TransferMode::Steady, String::new());
    }
    if !have_capabilities {
        // 还没协商到能力：先按正常跑，协商完再问一次（设置页会自己刷新）。
        return (TransferMode::Steady, "还没和手环协商完能力".to_string());
    }
    if protocol < 3 {
        return (TransferMode::Steady, "手环端应用版本过旧（不支持粗确认）".to_string());
    }
    let need = TransferMode::Turbo.params().chunk_bytes;
    if band_max_chunk < need {
        return (
            TransferMode::Steady,
            format!("手环最多只收 {} 分片", chunk_label(band_max_chunk)),
        );
    }
    (TransferMode::Turbo, String::new())
}

/// 标记某个连接会话是否已经处理过首条 `hello-ok`。
/// 能力协商结果不能作为会话边界：重连后 `save_protocol` 仍可能保留旧值。
fn mark_first_hello_for_session(session: u64, handled_session: &mut Option<u64>) -> bool {
    if *handled_session == Some(session) {
        return false;
    }
    *handled_session = Some(session);
    true
}

/// 存档通道「被版本/协议卡住」的判据：**应用已经活着**（答过至少一条 interconnect 消息），
/// 却仍然没有协商出存档协议。
///
/// 少了「已经活着」这半边，连接后那几秒（应用还在启动、hello 还没人接）就会被误报成
/// 「两侧版本或协议对不上」—— 真机日志里 `hello sent` 紧跟在 `launched watch app` 之后
/// 1.7 ms，随后 6 秒内一条回包都没有，正是这个假红卡的来源。
fn saves_capability_missing(alive: bool, save_protocol: Option<u32>) -> bool {
    alive && save_protocol.is_none()
}

/// 从手环 `hello-ok` 报来的 `appVersion`（如 "2.0.0"、"1.6.3"）里取**主版本号**。
/// 容忍 "v2.0.0" 这类前缀；开头的非数字跳过，取不到数字返回 `None`。
fn major_version(app_version: &str) -> Option<u32> {
    app_version
        .trim_start_matches(|c: char| !c.is_ascii_digit())
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()
}

/// `hello-ok` 到手时判定「手环上的游戏是不是 1.x 旧代」。
///
/// 判据：报上来的主版本号 < 2 就是旧代 —— 2.0 起存档结构与章节包格式都换了，
/// 插件里的章节包（线路合并版）旧版游戏根本读不了，所以要拦在同步之前。
/// 连版本号都报不上来的（`None`），只有**同时拿不出存档能力**才按旧代算：
/// 2.x 一定既报版本又报 saves 能力，两者都缺说明是更老的 1.x；
/// 但只缺版本号、能力齐全的，宁可放过也不误拦（界面不该凭猜测禁用同步）。
fn hello_reports_legacy(app_version: &str, supports_saves: bool) -> bool {
    match major_version(app_version) {
        Some(major) => major < 2,
        None => !supports_saves,
    }
}

/// 把「已写完的字节数」换算成**当前分片布局下**的分片下标。
///
/// `layout` 是每个分片的 `(字节数, 是不是某个文件的第一片)`。
///
/// 断点续传永远落在某个文件的**第一片**上（手环按文件边界记断点），所以这里找的是
/// 「累计字节数已经够了、并且正好是某个文件开头」的那一片。
///
/// **为什么必须按字节算**：手环回的 `resumeFrom` 是**上一次那套分片布局**里的下标，
/// 换过分片档位之后两套下标毫无对应关系。按字节游标算，换档最多重传一个文件；
/// 按下标续，换档就是整章重来（真机就是这么把「刷新后必须从头传」做出来的）。
fn resume_index_for_bytes(layout: &[(usize, bool)], done_bytes: usize) -> usize {
    if done_bytes == 0 {
        return 0;
    }
    let mut seen = 0usize;
    for (index, (bytes, is_file_start)) in layout.iter().enumerate() {
        if *is_file_start && seen >= done_bytes {
            return index;
        }
        seen = seen.saturating_add(*bytes);
    }
    // 游标落在最后一片之后（手环保证 done_bytes < 总字节数，理论上到不了这里）。
    layout.len()
}

/// 把「半截文件的路径」换算成**当前分片布局下**的分片下标。
///
/// `layout` 是每个分片的 `(文件路径, 是不是某个文件的第一片)`。
///
/// 什么时候用它：断点落在**整包第一个文件**中间时，手环的字节游标恰好是 0
/// （它算的是「已写完文件的字节数」，而这时一个文件都没写完），能指的只有那个半截文件。
/// 路径与分片怎么切无关，所以在**当前**布局里按它重新定位比按手环报的分片下标靠谱 ——
/// 后者只属于手环上一次那套布局（2026-10-02 真机日志：手环写了 8 KB 却回 `resumeBytes:0`，
/// 插件只能退回用 `resumeFrom`）。
///
/// 找不到这个文件返回 `None`（例如那一版手环没给 `resumePath`，或包内容已经换过）。
fn resume_index_for_path(layout: &[(String, bool)], path: &str) -> Option<usize> {
    if path.is_empty() {
        return None;
    }
    layout.iter().position(|(item, is_file_start)| *is_file_start && item == path)
}

/// **一个时间窗内**同一章最多自动重试几次（断线自动恢复的防死循环闸门）。
const MAX_AUTO_RESUME: u8 = 3;
/// 自动续传预算的窗口长度。窗口一过，计数作废、重新给满 `MAX_AUTO_RESUME` 次。
///
/// 为什么要有窗口：真机（2026-10-02 的 iPhone 日志）里链路每 1–3 分钟就断一次，
/// 3 次预算两分钟就烧光，之后这一轮再也不自动接着传 —— 界面挂着「自动续传已达上限，
/// 重连后点继续传输」，用户看到的就是「断点续传不工作」。窗口仍留着，是为了
/// 「一个窗口内不许无限自动重连」这条底线。
///
/// 这两条与 `auto_resume_budget` 一起挂在 crate 根：纯判定 + 宿主机用例，
/// wasm-only 的 `host` 模块里编不到（那是本仓库对「能直接判定对错的语义」的一贯做法）。
const AUTO_RESUME_WINDOW_MS: u64 = 5 * 60 * 1000;

/// 断线自动续传的次数闸门。返回 `(这次算第几次, 本次窗口起点, 还让不让自动续传)`。
///
/// `window_start_ms == 0` 视为「还没有窗口」（旧版本的存档、或刚被清掉），一律重新开窗。
/// 窗口内的第 `MAX_AUTO_RESUME + 1` 次被拒 —— 拒了之后 `fail_transfer` 会把意图清掉，
/// 于是「用户手点一次继续传输」失败之后又能拿到一整窗新预算，不必卸载重装插件。
///
/// 纯函数，挂在 crate 根是为了能在宿主机上直接跑用例（wasm-only 的 `host` 模块里编不到）。
fn auto_resume_budget(now_ms: u64, window_start_ms: u64, attempts: u8) -> (u8, u64, bool) {
    let expired = window_start_ms == 0 || now_ms.saturating_sub(window_start_ms) >= AUTO_RESUME_WINDOW_MS;
    if expired {
        return (1, now_ms, true);
    }
    let used = attempts.saturating_add(1);
    (used, window_start_ms, used <= MAX_AUTO_RESUME)
}

/// 连接保活该发哪一种消息：新应用用 ping，旧应用用 hello / pack.list 交替。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Keepalive {
    Ping,
    Hello,
    PackList,
}

fn keepalive_action(supports_ping: bool, beat: u32) -> Keepalive {
    if supports_ping {
        Keepalive::Ping
    } else if beat % 3 == 0 {
        Keepalive::PackList
    } else {
        Keepalive::Hello
    }
}

/// 连续丢接收会话的预算：8 次就停止自动重开，交给用户决定。
fn session_loss_exhausted(stalls: u8) -> bool {
    stalls >= 8
}

/// 连续 gap 的预算：12 次就判定窗口协商/路径持续异常。
fn gap_streak_exhausted(streak: u8) -> bool {
    streak >= 12
}

/// 完成核对：手环的已安装列表里有没有这个包（精确匹配 packId）。
fn pack_verified(installed: &[String], pack_id: &str) -> bool {
    !pack_id.is_empty() && installed.iter().any(|id| id == pack_id)
}

/// 这次「等手环确认」的等待过期了吗？
///
/// **为什么必须有这条**：2026-10-02 的真机反馈是「重新连接后点继续传输不传输，速度和剩余
/// 都显示「—」」。病因就是这一态没人回收 —— 应用进后台/被系统挂起时插件挂的 ready 定时器会丢，
/// `transfer` 永久停在「已发请求、还没 ready」：界面上所有同步/继续按钮都被
/// `is_transferring()`（= `started && !ready`）禁掉，`start_transfer` 一句
/// 「同步已开始，请等待完成或失败」把每个入口都挡回去，而速度/剩余因为 `speed_kbps == 0`
/// 永远显示「—」。用户点什么都没反应，只能重载插件。
///
/// 有过期判定之后：超时的等待**不再算「正在传」**（按钮重新可点），而且任何一次重新发起
/// 都可以把这次废掉的握手丢掉重来（`deadline_ms == 0` 表示这次传输还没进过等待态）。
///
/// 纯函数，宿主机上有用例。
fn ready_wait_expired(deadline_ms: u128, now_ms: u128) -> bool {
    deadline_ms > 0 && now_ms > deadline_ms
}

/// 握手已完成之后，多久没有任何进展就把这次传输当成「已经死了」（毫秒）。
///
/// 为什么也要有这条：链路掉线时插件是靠**分片重传用尽**（约 16 秒）来收尾的，而重传靠的是
/// 宿主定时器 —— 应用被系统挂起时那些定时器会丢，`transfer` 就带着 `started && ready` 一直留着。
/// 那一态下同步按钮虽然可点，但每次发起都会被「已经有一次在跑」挡回去，用户还是动不了。
/// 30 秒 ≈ 心跳判定掉线的三拍，与 `HEARTBEAT_MAX_MISSES` 同量级。
const TRANSFER_SILENT_MS: u128 = 30_000;

/// 这次传输还算「在跑」吗？同步/继续按钮能不能点、新的一次发起要不要被挡，都用它。
///
/// - 还没握手：只要「等确认」没超时就还活着（超时 = 那一次握手废了，见 `ready_wait_expired`）；
/// - 握过手：看**最近一次进展**（确认、重传都算）离现在多久 —— 静默太久就是死了。
///
/// 纯函数，宿主机上有用例。
fn transfer_live(started: bool, ready: bool, ready_deadline_ms: u128, last_progress_ms: u128, now_ms: u128) -> bool {
    if !started {
        return false;
    }
    if !ready {
        // 「已开始、还没确认」只有在等待窗口有效期内才算在跑。
        // 没有截止时刻（`0`，正常路径不会出现）按**不活跃**处理：宁可让用户能重来一次，
        // 也不要因为一个缺字段的状态把人永久卡在「同步已开始」上。
        return ready_deadline_ms > 0 && !ready_wait_expired(ready_deadline_ms, now_ms);
    }
    last_progress_ms > 0 && now_ms.saturating_sub(last_progress_ms) <= TRANSFER_SILENT_MS
}

/// 要不要提示「有更新」：候选版本必须严格高于当前版本。
///
/// 版本号来自外部 JSON，**解析失败、空串、非数字段一律按 0 处理**，
/// 并且只在候选严格更大时才返回 true —— 宁可少报一次更新，绝不谎报。
///
/// 预发布标签按 SemVer 处理：`4.1.0-beta` < `4.1.0`。否则运行 beta 的人
/// 永远收不到正式版提示。没有预发布标签的一方优先级更高。
fn version_is_newer(candidate: &str, current: &str) -> bool {
    fn parse(value: &str) -> (Vec<u64>, Option<String>) {
        let value = value.trim().trim_start_matches(['v', 'V']);
        let (core, pre) = match value.split_once('-') {
            Some((core, pre)) => (core, Some(pre.to_string())),
            None => (value, None),
        };
        let numbers = core
            .split('.')
            .map(|part| part.chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0))
            .collect();
        (numbers, pre)
    }
    let (candidate_numbers, candidate_pre) = parse(candidate);
    let (current_numbers, current_pre) = parse(current);
    let len = candidate_numbers.len().max(current_numbers.len());
    for index in 0..len {
        let left = *candidate_numbers.get(index).unwrap_or(&0);
        let right = *current_numbers.get(index).unwrap_or(&0);
        if left != right { return left > right; }
    }
    match (candidate_pre, current_pre) {
        // 都有预发布标签：只比较「有没有」，不猜 beta < rc 这种词序。
        // 数字段相同且都带标签时按「不算更新」处理，宁可漏报也不谎报。
        (Some(_), Some(_)) => false,
        // 候选是正式版、当前是预发布：正式版更新。
        (None, Some(_)) => true,
        // 候选是预发布、当前是正式版：不更新。
        (Some(_), None) => false,
        (None, None) => false,
    }
}

// ------------------------------------------------------------------ 宿主侧实现

#[cfg(target_arch = "wasm32")]
mod host {
use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Read};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use amakano2_ui::{
    Action, CHUNK_OPTIONS, DeviceView, ErrorCode, ErrorView, InstalledView, Limits, LogFilter,
    PackView, Page, PRESS_TIMEOUT_MS, ReadingStatsView, RefreshPlan, RefreshStep, ResumeView,
    SaveExportView, SaveImportView, SessionStage, Snapshot, StatusKind, TransferMode, TransferView,
    band_envelope, chunk_label, human_bytes, parse_action, refresh_steps,
};
// 自己生成 `psys-world-v4-http` 的绑定：包含全部 v4 宿主接口 + `http` 导出
// （起内建 HTTP 服务器给 HTML 前端用）。wit 文件在本 crate 的 wit/ 目录。
// `default_bindings_module` 指定生成代码挂在 `host::wit` 下，对外仍以 `psys_host` 引用。
pub mod wit {
    wit_bindgen::generate!({
        path: "wit",
        world: "psys-world-v4-http",
        generate_all,
        pub_export_macro: true,
        default_bindings_module: "crate::host::wit",
    });
}
use wit::astrobox::psys_host_v4 as psys_host;
use wit::exports::astrobox::psys_plugin_v4::{
    event::{self, EventType},
    lifecycle,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use zip::ZipArchive;

use crate::request::{FOLLOWUP_DELAY_MS, PendingRequest, REFRESH_RETRY_LIMIT, RequestKind, Slot, TimeoutAction};
use crate::saves;
use crate::saves::now_ms;
use crate::{
    INITIAL_WINDOW, MAX_AUTO_RESUME, SAVE_PROTOCOL, TransferLimits,
    ack_timeout_ms, auto_resume_budget, effective_mode, hello_reports_legacy,
    Keepalive, gap_streak_exhausted, keepalive_action, mark_first_hello_for_session, negotiated_window,
    pack_verified, resume_index_for_bytes, resume_index_for_path, saves_capability_missing,
    session_loss_exhausted, transfer_live, wire_limit,
};
use amakano2_ui::SaveSlotView;

// 宿主专属的两个模块：界面转换与剪贴板（都碰 `astrobox_ng_wit`）。
//
// `host/dialog.rs` **整个文件已经删除**：Dialog 这一类需要用户交互的宿主调用
// 在这个宿主上永远不返回、会把事件分发器堵死（真机证据见
// docs/插件开发注意事项.md 第 7 节）。导出/导入改走剪贴板，别再往回走。
mod clipboard;
mod cloud;
mod fonts;
mod http;
mod logger;
mod render;
mod server;
mod web;

/// 手环上《甜蜜女友2》的包名（interconnect 收发都用它）。
const PACKAGE_NAME: &str = "cn.amakanotwo.qihe";

/// v4 的宿主接口按值收字符串（WIT 没有引用），这里统一收一次口 ——
/// 调用点仍然写 `&str`，迁移只改这一层。
async fn send_message(addr: &str, payload: &str) -> Result<(), String> {
    psys_host::interconnect::send_qaic_message(
        addr.to_string(),
        PACKAGE_NAME.to_string(),
        payload.to_string(),
    )
    .await
}

async fn launch_qa(addr: &str, app: &psys_host::thirdpartyapp::AppInfo) -> Result<(), String> {
    psys_host::thirdpartyapp::launch_qa(
        addr.to_string(),
        app.clone(),
        "pages/index".to_string(),
    )
    .await
}

async fn thirdparty_app_list(addr: &str) -> Result<Vec<psys_host::thirdpartyapp::AppInfo>, String> {
    psys_host::thirdpartyapp::get_thirdparty_app_list(addr.to_string()).await
}

async fn register_interconnect(addr: &str) -> Result<(), String> {
    psys_host::register::register_interconnect_recv(addr.to_string(), PACKAGE_NAME.to_string()).await
}
/// 章节包随插件一起分发：`packs/index.json` 是清单，`packs/*.pack` 是包体。
const LIBRARY_DIR: &str = "packs";
const LIBRARY_INDEX: &str = "index.json";
const DEFAULT_CHUNK_BYTES: usize = 16384;
const MAX_PACK_BYTES: usize = 20_000_000;
/// 窗口加法增长前要攒够多少个「顺利确认」。见 `retry_packet` 里那句减半。
const WINDOW_GROWTH_ACKS: usize = 8;
// 飞行窗口的三个常量、`negotiated_window`、`chunk_timeout_ms` 与 `plan_batch` 挂在
// crate 根（见那边）：握手协商、超时、组批都要**宿主机上有守护用例**，
// 就只能住在不条件编译的那半边。
/// 手环喊「暂停」时最多听多久（毫秒）。对端报的数只用来兜底，不许它把传输按死。
/// `complete` 之后隔多久再去拉章节列表。手环那一刻正要写注册表、重扫清单、重绘界面，
/// 姊妹项目 2026-10-07 的 `mm_malloc` panic 就发生在这一刻 —— 给它一拍，别追着要数据。
const POST_COMPLETE_DELAY_MS: u64 = 1500;
const APP_START_DELAY_MS: u64 = 2000;
const READY_TIMEOUT_MS: u64 = 3000;
const MAX_READY_ATTEMPTS: u8 = 2;
/// 「已发导入请求、等手环确认」这段等待的余量（毫秒）。实际窗口是
/// `READY_TIMEOUT_MS × (MAX_READY_ATTEMPTS + 1) + 这个余量`（见 `start_transfer`）。
const READY_WAIT_SLACK_MS: u128 = 2_000;
/// 连接后轮询手环应用：每 2.5 秒试一次，最多 12 次（30 秒内手动打开也能接上）。
const PROBE_INTERVAL_MS: u64 = 2500;
const MAX_PROBE_ATTEMPTS: u32 = 12;
/// 稳态心跳间隔。连接期用 2.5 秒的密集探测（要尽快接上），接上之后没必要这么频繁 ——
/// 手环上多打一次往返就多一次把系统堆搅动的机会（见 `transferGate` 那段真机记录）。
const HEARTBEAT_INTERVAL_MS: u64 = 5_000;
/// 心跳判定「静默」的阈值：超过这个时间没收到手环任何消息就算漏了一拍。
const HEARTBEAT_SILENT_MS: u128 = 12_000;
/// 连续漏几拍判定掉线。3 拍 ≈ 30 秒，用户能感觉到「连不上了」，又不会误判一次卡顿。
const HEARTBEAT_MAX_MISSES: u32 = 3;
/// 一次存档回包最多允许多少片（手环侧是 9000 字节/片，这个上限远大于真实需要，
/// 只是为了防止一个乱报 `seq` 的回包把内存吃掉）。
const MAX_SAVE_SHARDS: u64 = 64;

/// 插件项目官网（关于页与 manifest 使用同一个地址）。
const WEBSITE: &str = "https://miband.qihe0507.top/";
/// 官方插件索引。只读公开 JSON，不带任何本机信息。
const PLUGIN_INDEX_URL: &str = "https://raw.githubusercontent.com/AstralSightStudios/AstroBox-NG-Plugin-Repo/main/index.json";
/// 本项目在官方索引里的仓库前缀（用来在列表里定位自己）。
const PLUGIN_REPO_MARK: &str = "qihe114514/amakano2-import";

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
    /// 这一趟传输用的模式。**建传输时定下就不再改**：传输中途换模式等于换整套时序，
    /// 只能作废重来（`set_transfer_mode` 就是这么做的）。
    mode: TransferMode,
    chunks: Vec<TransferChunk>,
    /// 下一条要发出去的分片（单调递增，重传时回退到 `acked_index`）。
    next_index: usize,
    /// 手环累计确认到哪一片（它的 `upToIndex + 1`）。这是**唯一**的进度权威。
    acked_index: usize,
    in_flight: Vec<(usize, u128)>,
    in_flight_bytes: usize,
    window_size: usize,
    /// 本次传输的窗口上限（由 `hello-ok` 协商，见 `negotiated_window`）。
    /// `window_size` 在重传时减半、顺利确认后加法增长，涨回来就是涨到这个值 ——
    /// **写死 `INITIAL_WINDOW` 的话，协商出来的窗口一次超时就再也回不去了。**
    window_cap: usize,
    /// 手环喊过「暂停」的截止时刻（墙钟毫秒，0 = 没暂停）。
    /// 暂停期间不发片，也不把在途的片判成超时（见 `amakano.files.pause` 的处理）。
    paused_until_ms: u128,
    acked_bytes: usize,
    speed_kbps: f64,
    speed_window_start: u128,
    speed_window_bytes: usize,
    rtt_ms: u64,
    retry_count: u8,
    /// 连续顺利确认的次数。用来做窗口的**加法增长**：一次超时之后窗口还能涨回来。
    stable_acks: usize,
    /// 停滞次数（连续没有确认）。用来决定「重发窗口」还是「放弃」。
    stall_count: u8,
    /// 连续丢接收会话的次数。与 ack 停滞分开计，8 次才停止自动重开。
    session_losses: u8,
    /// 连续 gap 的次数。gap 是「手环回话但缺片」，单独计数，避免 gap 风暴无上限。
    gap_streak: u8,
    /// 最近一次收到累计确认的时刻。停滞判定只看它 —— **不再给每一片各挂一个重传定时器**。
    last_ack_ms: u128,
    /// 最近一次真正发出去的时刻。
    last_send_ms: u128,
    /// 协商到的同步协议：3 = 窗口 + 累计确认 + gap；2 = 旧版逐片确认。
    protocol: u32,
    ready: bool,
    started: bool,
    waiting_ready: bool,
    ready_attempts: u8,
    /// 「已发导入请求、等手环确认」这段等待的截止时刻（墙钟毫秒，0 = 还没进过这一态）。
    /// 过期之后这次握手作废：界面按钮重新可点、同步入口允许重来（见 `ready_wait_expired`）。
    ready_deadline_ms: u128,
    /// 最近一次「有进展」的时刻：收到确认、收到分片确认、重传都算。
    /// 用它判这次传输是不是已经静默死掉（`transfer_live`）。
    last_progress_ms: u128,
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

/// 在线设备条目（给顶栏的设备选择器）。
#[derive(Clone)]
struct DeviceEntry {
    name: String,
    addr: String,
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
    /// 本次连接里 `send_qaic_message` 失败的次数。**这个数字必须能看见**：
    /// 「应用打开了、插件却一条回包都收不到」有两种可能 —— 消息根本没发出去（宿主侧拒绝）
    /// 或者发出去了但应用没回。失败次数是区分两者的唯一现场证据，所以发送失败要留痕，
    /// 并且要在状态行上说出来（旧实现里它会被下一轮探测的「正在等待…」直接盖掉）。
    send_failures: u32,
    /// 连接会话号。每次「连接设备」+1，探测/心跳定时器的载荷里带着它 ——
    /// 重连之后上一轮残留的定时器回来时会因为对不上号而被丢掉，
    /// 否则会有两条心跳链并行、每轮翻一倍。
    session: u64,
    /// 本次连接会话是否已经处理过首条 `hello-ok`。不要用能力值代替会话边界。
    hello_ok_session: Option<u64>,
    /// 稳态心跳连续几次没被回应。到上限就判定掉线并自动重连。
    heartbeat_misses: u32,
    /// 最近一次收到手环任何消息的时刻（`mark_alive()` 刷新）。心跳据此判断「静默了多久」。
    last_alive_ms: u128,
    /// 连接走到了哪一步（概览页的分段进度条）。只前进不后退，重连时重置。
    stage: SessionStage,
    /// 最近一次失败（码 + 细节原文）。成功路径/重新开始时会清掉。
    error: Option<ErrorView>,
    /// 手环注册表里有、但 `pack.txt` 读不出来的章节包名（`scan` 现在只读，
    /// 不剔除它们，所以要在这里说出来）。
    installed_broken: Vec<String>,
    /// 需要在事件回调栈之外执行一次自动重连（由 `on_event` 在 await 链上调用）。
    reconnect_pending: bool,
    /// 断线前正在传的那一章 —— 重连并拿到手环断点后自动接着传。
    /// `(章节号, 包名, 本窗口内已经自动重试过几次, 本窗口起点毫秒)`。
    /// 次数是防死循环的闸门，窗口让它**会过期**（见 `auto_resume_budget`）。
    auto_resume: Option<(usize, String, u8, u64)>,
    library: Vec<LibraryPack>,
    library_error: String,
    sync_queue: Vec<usize>,
    /// 本次连接是否已经问过未完成传输（连上之后问一次就够）。
    pending_checked: bool,
    /// 断点问题的**回答**是否已经到手（`amakano.pack.pending` 回包，无论手环上有没有断点）。
    /// `pending_checked == true` 而它还是 false，就是「问了、还没答上」的查询盲区 ——
    /// 界面在这段时间里显示「正在查询手环上的断点…」。
    pending_answered: bool,
    status: String,
    status_kind: StatusKind,
    transfer: Option<Transfer>,
    installed: Vec<InstalledPack>,
    cache_bytes: usize,
    cache_files: usize,
    pack_files: Vec<PackFile>,
    pack_meta: Option<PackMeta>,
    chunk_bytes: usize,
    /// 用户选的传输模式（正常 / 激进）。**只是意图**：真正生效的是
    /// `effective_mode()` —— 手环不认激进档时会被它压回正常。
    transfer_mode: TransferMode,
    /// 手环不接受激进档的原因（空串 = 没有限制）。给界面一句「为什么点了没变快」。
    turbo_blocked: String,
    /// 等待回包的请求 —— **只有一个坑位**，语义全在 `request::Slot`（含那条历史结构问题）。
    request: Slot,
    /// 「刷新」里还没发的往返，按 [`RefreshStep`] 的顺序排队。
    ///
    /// 只有一个坑位，所以「刷新」不能把两个请求一起挂出去：得等前一个 settle
    /// （回包 / 失败 / 超时放弃）再发下一个。顺序由 `refresh_steps`（ui-core）判定，
    /// 这里只负责照做 —— 章节列表在前、存档在后，理由见那份实现。
    refresh_queue: Vec<RefreshStep>,
    /// 本次会话是否收到过权威的 `pack.list` 回包（决定存档页刷新要不要补拉目录）。
    catalog_answered: bool,
    /// 当前刷新被槽位阻塞后已经补等了几次（防病态循环）。
    refresh_retries: u8,
    resume: Option<ResumeInfo>,
    /// 当前界面页。
    page: Page,
    /// 连接成功后自动打开手环应用。
    auto_launch: bool,
    /// 主面板画**原生界面**（宿主 ui-v3）而不是引导页。
    ///
    /// 默认 `false` —— 主面板的职责是「点一下打开 HTML 控制面板」。
    /// 只有打不开 WebView 的设备才需要切过来（见 `Action::UseNative`）。
    /// **刻意不落盘**：重启插件后回到默认的引导页，免得用户切过去之后
    /// 再也找不到「打开 HTML 面板」那个按钮。
    native_ui: bool,
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
    /// 最近一次 `amakano.saves.data` 带来的手环 `savesRevision`。
    ///
    /// 所有整表 `replace`（手动写槽、云写回）都必须带上它：否则一台设备用旧列表
    /// 写回时，会把另一台设备刚同步来的槽静默抹掉。`0` 也合法（尚未写过档）。
    save_list_revision: u64,
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
    /// 本次会话的飞行窗口上限，由 `hello-ok` 的 `receiveWindow` 协商（见 `negotiated_window`）。
    /// 新建传输时抄进 `Transfer::window_cap`；没协商到（旧应用）就是 3 片。
    window_cap: usize,
    /// 本次会话协商到的同步协议（3 = 窗口 + 累计确认 + gap；2 = 旧版逐片确认）。
    sync_protocol: u32,
    /// 传输巡视链是否已经挂着一拍 —— 保证同一时刻只有一条链，不会因每个确认都新挂一条而翻倍。
    transfer_tick_armed: bool,
    /// 手环丢了接收会话（应用重启 / 写盘卡死）时要重新发一次 `begin`。
    /// 这条恢复不丢进度：手环按 `pending-install` 的字节游标回 `ready`。
    restart_transfer: bool,
    /// `complete` 回执丢失后的核对目标：拉到 `pack.list` 后确认这个包真的登记成功。
    verify_pack: Option<String>,
    /// 核对请求的 requestId：只认这条 pack.list 的回包，旧请求不参与核对。
    verify_request: Option<String>,
    /// 核对重试次数（最多 3 次）。
    verify_attempts: u8,
    /// 本次会话协商到的手环端支持 `amakano.app.ping`（features 里有 sync3）。
    supports_ping: bool,
    /// 手环上报的**单片上限**（`maxChunkBytes`，老应用没有这个字段时退回 `chunkBytes`）。
    /// 新传输按它（与用户档位取小）切包，也用它判断能不能进激进档。
    band_chunk_bytes: usize,
    /// 本次会话的组批上限，由 `hello-ok` 的 `batchChunks` / `batchBytes` 协商。
    /// `0` = 手环不认识批量消息，照旧一片一条。**发之前现读现用**（见 `packet_for`）：
    /// 抄进传输对象只会让「换了台手环」之后整晚的优化悄悄失效。
    batch_chunks: usize,
    batch_bytes: usize,
    /// 手环上的游戏是 **1.x 旧代**（hello-ok 报的主版本号 < 2，或应用活着却始终
    /// 协商不出存档协议 —— 连 hello 通道都没有的更老版本）。
    ///
    /// 2.0 起存档结构与章节包格式都已更换，旧版游戏读不了新章节包，所以判定成立时：
    /// 界面挂「卸载重装」卡片、禁用同步入口，并清掉排队中的章节。
    /// 与 `saves_unsupported` 同一模式：只在**结论真正落地的时刻**写入
    /// （hello-ok 解析 / 存档请求超时归因），连接重置，不做渲染期派生 ——
    /// 连接后 hello 还没回来的那几秒里「alive 但无协议」是正常等待，不是证据。
    save_revision_supported: bool,
    band_game_legacy: bool,
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
    /// 手环刚交给插件的“当前游戏进度”快照；存档页点槽位时写它。
    pending_current_save: Option<Value>,
    /// 正在等待当前进度快照的目标槽号（不回包时界面知道是哪次保存失败）。
    pending_save_slot: Option<usize>,
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
    // ---- 云存档（与本地 saves_busy 独立） ----
    cloud_server: String,
    cloud_phase: cloud::CloudPhase,
    cloud_detail: String,
    /// 主账号名：绑定后是 QQ 昵称。
    cloud_qq_name: String,
    /// AstroBox 关联账号名（只做关联，不当主账号名）。
    cloud_account_name: String,
    /// 用户是否已经明确授权 AstroBox 账号。
    account_authorized: bool,
    /// 账号授权状态：idle / authorizing / authorized / denied / error。
    account_auth: String,
    cloud_bound: bool,
    /// 完整 QQ 号（服务端绑定后返回）。不是打码值。
    cloud_qq: String,
    /// 绑定码命令（服务端已带「绑定」前缀，前端只显示，不再拼一次）。
    cloud_code: String,
    cloud_expires_at: String,
    /// 当前在线设备列表（顶栏选择器）。
    devices: Vec<DeviceEntry>,
    /// 官方源检查结果。
    update_latest: String,
    update_available: bool,
    update_status: String,
    cloud_reread: Option<cloud::CloudRun>,
    cloud_generation: u64,
    cloud_running: bool,
    cloud_synced_session: Option<u64>,
    cloud_conflict_retries: u8,
    cloud_persisted: CloudPersisted,
    cloud_pending: Option<CloudBandPut>,
    band_queue: std::collections::VecDeque<cloud::BandOperation>,
    save_list_session: Option<u64>,
    save_put_session: Option<u64>,
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
            send_failures: 0,
            session: 0,
            hello_ok_session: None,
            stage: SessionStage::Idle,
            error: None,
            installed_broken: Vec::new(),
            heartbeat_misses: 0,
            last_alive_ms: 0,
            reconnect_pending: false,
            auto_resume: None,
            library: Vec::new(),
            library_error: String::new(),
            sync_queue: Vec::new(),
            pending_checked: false,
            pending_answered: false,
            status: "先点「连接设备」，再在章节列表里点「同步」".into(),
            status_kind: StatusKind::Info,
            transfer: None,
            installed: Vec::new(),
            cache_bytes: 0,
            cache_files: 0,
            pack_files: Vec::new(),
            pack_meta: None,
            chunk_bytes: DEFAULT_CHUNK_BYTES,
            transfer_mode: TransferMode::default(),
            turbo_blocked: String::new(),
            request: Slot::idle(),
            refresh_queue: Vec::new(),
            catalog_answered: false,
            refresh_retries: 0,
            resume: None,
            page: Page::Push,
            auto_launch: true,
            native_ui: false,
            hover: None,
            line_filter: None,
            log_filter: LogFilter::All,
            pressed: None,
            last_action: String::new(),
            last_action_ms: 0,
            save_auto: None,
            save_slots: Vec::new(),
            save_list_revision: 0,
            saves_error: String::new(),
            save_protocol: None,
            saves_unsupported: false,
            band_version: String::new(),
            window_cap: INITIAL_WINDOW,
            sync_protocol: 2,
            transfer_tick_armed: false,
            restart_transfer: false,
            verify_pack: None,
            verify_request: None,
            verify_attempts: 0,
            supports_ping: false,
            band_chunk_bytes: DEFAULT_CHUNK_BYTES,
            batch_chunks: 0,
            batch_bytes: 0,
            save_revision_supported: false,
            band_game_legacy: false,
            confirm_delete_save: None,
            saves_busy: false,
            saves_notice: String::new(),
            save_export: None,
            save_import: None,
            pending_current_save: None,
            pending_save_slot: None,
            stats: None,
            stats_error: String::new(),
            stats_supported: false,
            stats_busy: false,
            cloud_server: String::new(),
            cloud_phase: cloud::CloudPhase::Loading,
            cloud_detail: String::new(),
            cloud_qq_name: String::new(),
            cloud_account_name: String::new(),
            account_authorized: false,
            account_auth: "idle".into(),
            cloud_bound: false,
            cloud_qq: String::new(),
            cloud_code: String::new(),
            cloud_expires_at: String::new(),
            devices: Vec::new(),
            update_latest: String::new(),
            update_available: false,
            update_status: String::new(),
            cloud_reread: None,
            cloud_generation: 0,
            cloud_running: false,
            cloud_synced_session: None,
            cloud_conflict_retries: 0,
            cloud_persisted: CloudPersisted::default(),
            cloud_pending: None,
            band_queue: std::collections::VecDeque::new(),
            save_list_session: None,
            save_put_session: None,
        })
    })
}

/// 改状态。返回闭包的返回值，调用方拿它判断「这次到底改了没有」。
fn update<T>(action: impl FnOnce(&mut State) -> T) -> T {
    action(&mut state().lock().unwrap_or_else(|error| error.into_inner()))
}

/// 推进连接阶段。**只前进不后退**：分段条是「走到哪了」，倒退会闪。
/// 重新连接时在 `connect_device` 里显式重置。
fn set_stage(stage: SessionStage) {
    update(|state| {
        if stage > state.stage {
            state.stage = stage;
        }
    });
}

/// 记一条失败。文案不在这里生成 —— 插件只产出「码 + 细节原文」，
/// 结论与「怎么办」统一由 ui-core 的 `ErrorCode` 给（派生结果只允许一处实现）。
fn set_error(code: ErrorCode, detail: impl Into<String>) {
    update(|state| state.error = Some(ErrorView::new(code, detail)));
}

fn clear_error() {
    update(|state| state.error = None);
}

fn set_status(kind: StatusKind, text: impl Into<String>) {
    update(|state| {
        state.status_kind = kind;
        state.status = text.into();
    });
}

fn arm_timer(delay_ms: u64, payload: String) {
    let _ = psys_host::timer::set_timeout(delay_ms, &payload);
}

/// 手环数据是否已经到手：连接建立且存档协议协商完成。
///
/// 刻意**不以 `alive` 为条件**：`alive` 是「应用回过消息」的心跳标记，应用刚被
/// 拉起、第一条回包还在路上时它是 false，但通道已经可用；这时拦下来只会让用户
/// 在连上后的头两秒里点不动任何按钮。真正的失败由请求超时与状态行负责。
///
/// 存档/统计/同步这些「读手环数据」的动作在按钮层禁用只是第一道；
/// 后端每一处真正发请求前也要过这道闸，防止旧页面、双击或多端的残留点击绕过按钮。
fn band_data_ready() -> bool {
    let current = state().lock().unwrap_or_else(|error| error.into_inner());
    current.connected && current.save_protocol.is_some()
}

// ---------------------------------------------------------------- 云存档持久化与串行调度

const CLOUD_STATE_FILE: &str = "cloud-state.json";
const CLOUD_CONFIG_FILE: &str = "packs/cloud-config.json";

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
struct CloudPersisted {
    #[serde(default, rename = "accountId")]
    account_id: String,
    /// 上次明确授权的 AstroBox 账号名。只用于向用户显示关联身份，不当主账号名。
    #[serde(default, rename = "accountName")]
    account_name: String,
    #[serde(default, rename = "deviceId")]
    device_id: String,
    #[serde(default)]
    credential: String,
    #[serde(default)]
    local: crate::cloud::LocalState,
}

#[derive(Clone)]
struct CloudBandPut {
    band: crate::cloud::BandSnapshot,
    local: crate::cloud::LocalState,
    merged: crate::cloud::Merged,
}

fn random_cloud_id() -> Result<String, http::ApiFailure> {
    http::random_id().map_err(http::ApiFailure::transport)
}

fn save_cloud_state() {
    let data = update(|state| state.cloud_persisted.clone());
    match serde_json::to_string(&data) {
        Ok(text) => {
            let temporary = format!("{CLOUD_STATE_FILE}.tmp");
            if let Err(error) = fs::write(&temporary, text).and_then(|_| fs::rename(&temporary, CLOUD_STATE_FILE)) {
                tracing::warn!(%error, "cloud state could not be saved");
            }
        }
        Err(error) => tracing::warn!(%error, "cloud state could not be serialized"),
    }
}

fn load_cloud() {
    if let Ok(text) = fs::read_to_string(CLOUD_CONFIG_FILE)
        && let Ok(value) = serde_json::from_str::<Value>(&text)
        && let Some(server) = value.get("server").and_then(Value::as_str)
    {
        update(|state| state.cloud_server = server.trim_end_matches('/').to_string());
    }
    if let Ok(text) = fs::read_to_string(CLOUD_STATE_FILE) {
        match serde_json::from_str::<CloudPersisted>(&text) {
            Ok(saved) => update(|state| {
                // 有已保存的设备凭证说明本机以前明确授权过；`account_authorized`
                // 不落盘，靠 credential + account_id 恢复，避免无凭证也被当成已授权。
                state.account_authorized = !saved.credential.is_empty() && !saved.account_id.is_empty();
                state.account_auth = if state.account_authorized { "authorized".into() } else { "idle".into() };
                state.cloud_account_name = saved.account_name.clone();
                state.cloud_persisted = saved;
            }),
            Err(error) => tracing::warn!(%error, "cloud state unreadable; cloud sync will rebuild its baseline"),
        }
    }
}

fn band_is_free(state: &State, session: u64) -> bool {
    state.session == session && state.connected && state.save_protocol.is_some()
        && !state.request.busy() && state.refresh_queue.is_empty()
        && !state.transfer.as_ref().is_some_and(|transfer| transfer.started)
        && !state.saves_busy
}

async fn request_band_operation(operation: cloud::BandOperation) {
    let session = match operation {
        cloud::BandOperation::List { session } | cloud::BandOperation::Put { session, .. } => session,
    };
    update(|state| {
        if state.session == session && !state.band_queue.iter().any(|queued| std::mem::discriminant(queued) == std::mem::discriminant(&operation)) {
            state.band_queue.push_back(operation);
        }
    });
    pump_band_queue().await;
}

async fn pump_band_queue() {
    let next = update(|state| {
        let session = state.session;
        if !band_is_free(state, session) { return None; }
        state.band_queue.pop_front()
    });
    let Some(operation) = next else { return };
    match operation {
        cloud::BandOperation::List { session } => {
            update(|state| state.save_list_session = Some(session));
            request_save_list().await;
        }
        cloud::BandOperation::Put { session, band } => {
            update(|state| state.save_put_session = Some(session));
            request_save_put("replace", band.slots, band.auto_save).await;
        }
    }
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

fn transfer_with_chunks(
    limits: TransferLimits,
    chunks: Vec<TransferChunk>,
    window_cap: usize,
    protocol: u32,
) -> Transfer {
    Transfer {
        request_id: format!("pack-{}", now_ms()),
        chunk_bytes: limits.chunk_bytes,
        mode: limits.mode,
        chunks,
        next_index: 0,
        acked_index: 0,
        in_flight: Vec::new(),
        in_flight_bytes: 0,
        // 起步用**模式自己的**起始窗口（正常 6 片 / 激进 8 片）：一次把整窗 43 KB 的消息
        // 砸过去，和 2026-10-07 那次大消息打崩 SAR 是同一类风险。稳定确认之后再按
        // `WINDOW_GROWTH_ACKS` 加法涨到上限。
        window_size: limits.params.initial_window.min(window_cap),
        window_cap,
        paused_until_ms: 0,
        acked_bytes: 0,
        speed_kbps: 0.0,
        speed_window_start: now_ms(),
        speed_window_bytes: 0,
        rtt_ms: 0,
        retry_count: 0,
        stable_acks: 0,
        stall_count: 0,
        session_losses: 0,
        gap_streak: 0,
        last_ack_ms: 0,
        last_send_ms: 0,
        protocol,
        ready: false,
        started: false,
        waiting_ready: false,
        ready_attempts: 0,
        ready_deadline_ms: 0,
        last_progress_ms: 0,
        resumed: false,
    }
}

fn prefix_bytes(chunks: &[TransferChunk], count: usize) -> usize {
    chunks.iter().take(count.min(chunks.len())).map(|chunk| chunk.bytes).sum()
}

fn begin_packet(meta: &PackMeta, transfer: &Transfer) -> String {
    let params = transfer.mode.params();
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
        // v3 协商：分片档位与飞行窗口一起交给手环，手环只收紧不放大。
        // 旧手环不认识这些字段，按它自己的保守值跑（逐片确认、8 KB 上限）。
        "chunkBytes": transfer.chunk_bytes,
        // gap 边界由手环的 RECEIVE_WINDOW 决定；这里报**上限**，让手环知道传输中
        // 窗口会从起始值涨到上限，不必按起始值判 gap。
        "window": transfer.window_cap,
        // 传输模式 + 这一档的回执节拍。手环按 `ackEvery` / `ackMs` 回累计确认 ——
        // **激进档的提速主要就来自这两个数变大**（回执从每 4 片降到每 32 片）。
        // 旧手环不认这些字段，仍按它自己的 ACK_EVERY / ACK_MS 跑，只是分片用我们报的这个。
        "mode": transfer.mode.wire(),
        "ackEvery": params.ack_every,
        "ackMs": params.ack_ms,
        "ack": "batch",
        "sync": transfer.protocol,
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
        "内置 {} 章 · {scenes} 场景 · {dialogues} 句对白 · {} · 阅读时长约 {} 小时 {} 分钟",
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
async fn start_embedded_transfer(number: usize) {
    // **旧代拦截是硬闸门**：手环上是 1.x 游戏时，2.0 的线路合并包（packId=章节身份、
    // p00 公共资源包）旧版根本读不了，传上去就是一堆死登记。单章同步、批量队列、
    // 断点续传、自动恢复全都汇到这一个入口，在这里拦一次就够。
    if update(|state| state.band_game_legacy) {
        set_status(
            StatusKind::Bad,
            "手环上的游戏是 1.x 旧版，读不了 2.0 的章节包：请先卸载游戏、重装 2.0 版再同步",
        );
        render();
        return;
    }
    // **没拿到手环数据前不许开传**：只连上通道还不够，必须等能力协商完成。
    // 否则同步请求会与握手/探测抢同一条通道，表现为「点了同步没反应」或「传一半卡住」。
    // 按钮层也禁用，这里是后端硬闸门。
    let blocked = {
        let current = state().lock().unwrap_or_else(|error| error.into_inner());
        if !current.connected {
            Some("还没连接手环，请先点「连接手环」")
        } else if current.save_protocol.is_none() {
            Some("还在读取手环能力，请稍等几秒再同步")
        } else {
            None
        }
    };
    if let Some(hint) = blocked {
        set_status(StatusKind::Warn, hint);
        render();
        return;
    }
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
        set_error(ErrorCode::BadPack, format!("{} 体积异常（{}）", pack.title, human_bytes(data.len())));
        set_status(StatusKind::Bad, format!("{} 体积异常（{}），请重新安装插件", pack.title, human_bytes(data.len())));
        render();
        return;
    }
    let (mut meta, files) = match parse_pack(&data) {
        Ok(value) => value,
        Err(message) => {
            set_error(ErrorCode::BadPack, format!("{}：{message}", pack.title));
            set_status(StatusKind::Bad, format!("{}：{message}", pack.title));
            render();
            return;
        }
    };
    meta.name = pack.title.clone();
    let mut message = String::new();
    let mut kind = StatusKind::Good;
    update(|state| {
        // 这一趟用哪个模式：用户意图先过一遍能力（旧手环的激进档会被压回正常）。
        let (mode, blocked) = effective_mode(
            state.transfer_mode,
            state.sync_protocol,
            state.band_chunk_bytes,
            state.save_protocol.is_some(),
        );
        state.turbo_blocked = blocked.clone();
        // 分片不得超过手环的单片上限（旧应用报 16 KB 时，激进档的 32 KB 要压回去）。
        let chunk_bytes = state.chunk_bytes.min(state.band_chunk_bytes).max(1024);
        let chunks = build_chunks(&files, chunk_bytes);
        let count = chunks.len();
        let matches_resume = state
            .resume
            .as_ref()
            // **判据里不该有分片数**：换过档位之后 `pending.chunks` 与当前布局必然不同，
            // 把它算进来会让「与未完成传输匹配」永远为假，界面就会说「正在从头同步」。
            .map(|pending| pending.pack_id == meta.pack_id && pending.bytes == meta.total_bytes)
            .unwrap_or(false);
        let limits = TransferLimits::new(mode, chunk_bytes);
        state.transfer = Some(transfer_with_chunks(limits, chunks, state.window_cap, state.sync_protocol));
        state.transfer_tick_armed = false;
        // 新的传输开始，上一章的完成核对/队列推进全部作废，避免旧核对覆盖新传输。
        state.verify_pack = None;
        state.verify_request = None;
        state.verify_attempts = 0;
        state.pack_files = files;
        state.pack_meta = Some(meta.clone());
        if matches_resume {
            message = format!("{}（阅读约 {} 分钟）与未完成传输匹配，正在接着传", meta.name, pack.minutes);
        } else {
            if state.resume.is_some() {
                state.resume = None;
                kind = StatusKind::Warn;
            }
            message = format!(
                "正在同步 {}（全文阅读约 {} 分钟 · {count} 个分片 · {}档）",
                meta.name,
                pack.minutes,
                mode.label()
            );
        }
    });
    set_status(kind, message);
    render();
    start_transfer().await;
}

/// 「继续传输」：把手环保留的断点（`state.resume`）翻译成对那一章的传输。
///
/// 断点的权威在手环（`amakano.pack.pending` 回包建立的 `ResumeInfo`），
/// `start_embedded_transfer` 内部会按字节游标续传（`matches_resume`）——
/// 这里只负责把 `pack_id` 映射回章节号。这是断点卡片「继续传输」按钮的落点：
/// 旧版断点卡片只有指路文案、没有动作，用户重连后找不到明确的「继续」入口。
///
/// ⚠️ **「有一个 transfer」不等于「在传」**：`transfer.started` 在「已发请求、还没等到
/// 手环确认」这一态下也是 true，而那一态会因为应用被挂起时插件定时器丢失而**永久停在原地**。
/// 旧写法只判 `started`，于是那种情况下点「继续传输」什么都不发生（状态行还给一句
/// 跟断点无关的「手环上没有可继续的断点」）—— 2026-10-02 的真机原话。
/// 现在只把「真的还在跑」的传输当占用：等到确认的（`ready`）或等待还没过期的。
async fn resume_transfer() {
    let (target, busy) = {
        let current = state().lock().unwrap_or_else(|error| error.into_inner());
        let now = now_ms();
        let busy = current
            .transfer
            .as_ref()
            .is_some_and(|transfer| transfer_live(transfer.started, transfer.ready, transfer.ready_deadline_ms, transfer.last_progress_ms, now));
        if busy {
            // 正在传：断点卡根本不会出现，这个动作不该被触发。
            (None, true)
        } else {
            let pack_id = current.resume.as_ref().map(|item| item.pack_id.clone());
            let number = pack_id.and_then(|id| {
                current.library.iter().find(|pack| pack.id == id).map(|pack| pack.number)
            });
            (number, false)
        }
    };
    match target {
        Some(number) => start_embedded_transfer(number).await,
        None if busy => {
            set_status(StatusKind::Warn, "上一轮传输还没结束（也没失败）：等它超时（约 20 秒）后「继续/重传」会重新可点");
            render();
        }
        None => {
            set_status(StatusKind::Warn, "手环上没有可继续的断点，请在推送页选择章节同步");
            render();
        }
    }
}

/// 依次同步队列里的章节。
async fn queue_sync(numbers: Vec<usize>) {
    let first = {
        let mut current = state().lock().unwrap_or_else(|error| error.into_inner());
        current.sync_queue = numbers;
        // 「同步全部未安装」的队列要能扛住插件刷新：落盘一份，`on_load` 再读回来。
        // 真正的进度仍然只认手环的断点，这里只是「用户想传哪些章」的意图。
        if current.sync_queue.is_empty() {
            None
        } else {
            Some(current.sync_queue.remove(0))
        }
    };
    save_persisted();
    match first {
        Some(number) => start_embedded_transfer(number).await,
        None => {
            set_status(StatusKind::Info, "没有需要同步的章节");
            render();
        }
    }
}

/// 传输完成后取出下一章；返回 None 表示队列已空。
fn pop_sync_queue() -> Option<(usize, usize)> {
    let result = {
        let mut current = state().lock().unwrap_or_else(|error| error.into_inner());
        if current.sync_queue.is_empty() {
            None
        } else {
            let number = current.sync_queue.remove(0);
            Some((number, current.sync_queue.len()))
        }
    };
    if result.is_some() {
        save_persisted();
    }
    result
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
    // 有效模式在这里算一次，整份快照（设置页 + 「传输参数」格）都用同一个值。
    let (mode, mode_blocked) = effective_mode(
        current.transfer_mode,
        current.sync_protocol,
        current.band_chunk_bytes,
        current.save_protocol.is_some(),
    );

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
            chunks_done: transfer.acked_index,
            chunks_total: transfer.chunks.len(),
            speed_kbps: transfer.speed_kbps,
            rtt_ms: transfer.rtt_ms,
            retries: transfer.retry_count,
            resumed: transfer.resumed,
            ready: transfer.ready,
            started: transfer.started,
            // 这次传输已经不再活跃（等待确认过期，或握手完成后长时间静默）：
            // 界面据此把按钮放回来、别再把它当成「传输中」占着卡片。
            stalled: !transfer_live(transfer.started, transfer.ready, transfer.ready_deadline_ms, transfer.last_progress_ms, now_ms()),
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

    // 断点查询盲区：手环应用活着、断点问题已经问出去但回答还没到。
    // 这段时间界面显示「正在查询手环上的断点…」，而不是什么都不说 ——
    // 重连后到断点出现之间可能隔好几秒（列表 → pending 两个往返），空白会被当成「没了」。
    let resume_checking = current.connected
        && current.alive
        && current.pending_checked
        && !current.pending_answered
        && current.resume.is_none()
        && !current.transfer.as_ref().is_some_and(|transfer| transfer.started);

    // 日志行只在设置页（日志就住在那儿）才克隆（最多 240 行），别的页面只要级别计数。
    let logs = if current.page == Page::Settings { logger::snapshot() } else { Vec::new() };
    let (log_warns, log_errors) = logger::counts();
    let current_save = current.pending_current_save.as_ref().map(|save| {
        SaveSlotView::from_json(save, None, &[])
    });

    Snapshot {
        page: current.page,
        version: current.version.clone(),
        stage: current.stage,
        error: current.error.clone(),
        installed_broken: current.installed_broken.clone(),
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
        resume_checking,
        cache_bytes: current.cache_bytes,
        cache_files: current.cache_files,
        chunk_bytes: current.chunk_bytes,
        // 界面显示的是**实际生效**的模式：用户选了激进、手环不认时这里就是正常，
        // 同时把原因一起交出去（设置页据此解释「为什么点了没变快」）。
        transfer_mode: mode,
        turbo_blocked: if mode_blocked.is_empty() { current.turbo_blocked.clone() } else { mode_blocked },
        queue: current.sync_queue.clone(),
        status: current.status.clone(),
        status_kind: current.status_kind,
        auto_launch: current.auto_launch,
        native_ui: current.native_ui,
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
        band_game_legacy: current.band_game_legacy,
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
        pending_current_save: current_save,
        cloud_phase: current.cloud_phase.wire().into(),
        cloud_detail: current.cloud_detail.clone(),
        cloud_qq_name: current.cloud_qq_name.clone(),
        cloud_qq: current.cloud_qq.clone(),
        cloud_account_name: current.cloud_account_name.clone(),
        cloud_code: current.cloud_code.clone(),
        cloud_expires_at: current.cloud_expires_at.clone(),
        account_auth: current.account_auth.clone(),
        devices: current
            .devices
            .iter()
            .map(|item| DeviceView {
                name: item.name.clone(),
                addr: item.addr.clone(),
                connected: item.addr == current.device_addr && current.connected,
                alive: item.addr == current.device_addr && current.alive,
                probe_attempt: 0,
                probe_max: MAX_PROBE_ATTEMPTS,
            })
            .collect(),
        selected_device: current.device_addr.clone(),
        update_latest: current.update_latest.clone(),
        update_available: current.update_available,
        update_status: current.update_status.clone(),
        website: WEBSITE.into(),
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
            // 巡视节拍与停滞重发次数都跟着**有效模式**走（设置页显示的就是真正在用的值）。
            retry_delay_ms: mode.params().tick_ms,
            max_retries: mode.params().give_up_stalls,
            // 报**这次会话协商到**的窗口上限；起步值见 `transfer_with_chunks`。
            initial_window: mode.params().initial_window.min(current.window_cap),
            probe_interval_ms: HEARTBEAT_INTERVAL_MS,
            probe_attempts: MAX_PROBE_ATTEMPTS,
            max_pack_bytes: MAX_PACK_BYTES,
        },
    }
}

/// 重绘当前页。
///
/// 画什么由 `Snapshot::native_ui` 决定（分叉在 `amakano2_ui::build` 里）：
///
/// - **默认**：引导页 —— 一个「打开控制面板」按钮，点开独立的 HTML 面板。
///   HTML 面板走 `browser.open`，每次都要宿主授权，所以必须由用户自己点。
/// - **后备**：宿主 `ui-v3` 画的完整五页界面，给**打不开 WebView** 的设备用
///   （授权被拒、WebView 不兼容或白屏）。顶上有「返回」可以切回引导页。
///
/// 两个形态都经过同一个 `build()`，所以这里不需要判断 —— 判断只写在
/// `ui-core` 的 `pages::render` 一处，宿主机上能跑用例。
fn render() {
    let element_id = {
        let current = state().lock().unwrap_or_else(|error| error.into_inner());
        current.element_id.clone()
    };
    let Some(element_id) = element_id else { return };
    let current = snapshot();
    render::paint(&element_id, &amakano2_ui::build(&current));
}

/// 插件主面板的引导页（HTML 控制面板的入口）。
///
/// **已不再作为主界面，也没有任何调用者**：`render()` 现在直接画
/// `amakano2_ui::build()` 的完整界面。HTML 面板仍可通过
/// [`Action::OpenPanel`](amakano2_ui::Action::OpenPanel) 打开（那个动作走
/// `host::server::reopen()`，与这个函数无关）。
///
/// 函数本身留作参考：它记录了「打开 HTML 面板需要宿主授权」这段教训。
#[allow(dead_code)]
fn landing_page(snapshot: &Snapshot) -> amakano2_ui::Node {
    use amakano2_ui::{Node, Tag};
    let connected = snapshot.device.connected;
    // 与 web/app.css 同一套色值（引导页和 HTML 面板视觉一致）。
    let (badge_text, dot_color) = if connected { ("手环已连接", "#8FBC9B") } else { ("手环未连接", "#9A8D94") };
    Node::new(Tag::Div)
        .prop("flex", "column")
        .prop("gap", "14")
        .prop("padding", "20")
        .child(Node::text(Tag::P, "甜蜜女友2 导入器").prop("size", "20").prop("fg", "#F5F0F2").prop("weight", "600"))
        // 窗口不再随插件启用自动打开：`browser.open` 每次都要宿主授权，
        // 自动弹授权框只会让人以为插件在报错（见 `host/server.rs` 的 `start`）。
        // 所以文案必须说清「要自己点一下」，不能写「已经打开了」。
        .child(Node::text(Tag::P, "点下面的按钮打开控制面板。宿主会请求一次 browser 权限，允许后窗口才会出现。")
            .prop("size", "13").prop("fg", "#B8ACB2"))
        .child(Node::new(Tag::Div)
            .prop("flex", "row").prop("align", "center").prop("gap", "8")
            .child(Node::new(Tag::Div).prop("w", "8").prop("h", "8").prop("radius", "4").prop("bg", dot_color))
            .child(Node::text(Tag::P, badge_text).prop("size", "13").prop("fg", "#B8ACB2")))
        .child(Node::new(Tag::Button)
            .prop("on.click", amakano2_ui::open_panel_id())
            .prop("bg", "#BE4E77")
            .prop("fg", "#FFFFFF")
            .prop("radius", "12").prop("h", "46")
            .child(Node::text(Tag::P, "打开控制面板").prop("size", "15").prop("weight", "500")))
        .child(Node::text(Tag::P, "章节同步、存档、统计与账号都在控制面板里操作。")
            .prop("size", "12").prop("fg", "#9A8D94"))
}

/// 「打开游戏」：把《甜蜜女友2》调到前台，然后照旧走探测循环接上。
async fn launch_app() {
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
        // 递增会话号：旧的心跳/探测链到点会被丢掉，否则每点一次「打开游戏」
        // 就多一条 5 秒 ping 链（`duplicate()` 只挡 500ms 内的连点）。
        state.session = state.session.wrapping_add(1);
    });
    let outcome = launch_watch_app(&addr).await;
    let (kind, text) = match outcome {
        LaunchOutcome::Launched => {
            (StatusKind::Good, "已打开《甜蜜女友2》，正在等待它回应…".to_string())
        }
        other => (StatusKind::Warn, other.hint()),
    };
    set_status(kind, text);
    render();
    arm_probe(APP_START_DELAY_MS, update(|state| state.session));
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
    // 手环喊过暂停：这段时间一片都不发（它正忙着写盘/整理，追着发只会把它压垮）。
    if transfer.paused_until_ms > now_ms() {
        return Vec::new();
    }
    let mut packets = Vec::new();
    let mut bytes = transfer.in_flight_bytes;
    let params = transfer.mode.params();
    let limit = wire_limit(params, transfer.chunk_bytes);
    while transfer.in_flight.len() < transfer.window_size
        && transfer.next_index < transfer.chunks.len()
    {
        let index = transfer.next_index;
        let size = transfer.chunks[index].bytes;
        // 窗口同时受**片数**与**字节**约束：正常档 16 KB × 10 片就是 160 KB，
        // 手环的 quickjs 堆只有 4 MB，不能把窗口单纯按片数放大。
        if !transfer.in_flight.is_empty() && bytes + size > params.inflight_bytes {
            break;
        }
        let payload = chunk_payload(transfer, index);
        // 线上尺寸硬闸门：单片超限说明分片档位或路径异常，绝不把它发出去。
        if payload.len() > limit {
            tracing::warn!(index, bytes = payload.len(), limit, "chunk payload over the wire limit");
            break;
        }
        bytes += size;
        transfer.next_index += 1;
        transfer.in_flight.push((index, now_ms()));
        packets.push((addr.clone(), payload, index));
    }
    if !packets.is_empty() {
        transfer.in_flight_bytes = bytes;
        transfer.last_send_ms = now_ms();
    }
    packets
}

/// 回包到了：**只有 id 对得上**才算销账（`Slot::clear_if_matches` 的语义，宿主机上有用例）。
fn clear_request(id: &str) {
    update(|state| {
        state.request.clear_if_matches(id);
    });
    arm_timer(FOLLOWUP_DELAY_MS, json!({ "type": "amakano.timer", "kind": "band-pump" }).to_string());
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
    // 定时器带会话号：重连会把 `refresh_queue` 清空，但已经注册的定时器撤不掉；
    // 不带会话号的话，旧会话的定时器到点会把新会话刚排的步骤提前取走一条。
    let session = update(|state| (!state.refresh_queue.is_empty()).then_some(state.session));
    if let Some(session) = session {
        arm_timer(FOLLOWUP_DELAY_MS, json!({
            "type": "amakano.timer", "kind": "refresh-next", "session": session
        }).to_string());
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
    let (addr, session, busy) = {
        let current = state().lock().unwrap_or_else(|error| error.into_inner());
        (current.device_addr.clone(), current.session, current.request.busy())
    };
    if busy {
        // **不再静默丢弃**：槽位被别的请求占着时，这次刷新排进现有队列，
        // 等前一个 settle 后由 `refresh-next` 补发。旧实现在这里直接 return，
        // 调用方却已经把 `saves_busy` / `stats_busy` 置上 —— 用户看到的是
        // 「一直在查询、什么都查不到」，而请求从未发出。
        enum Busy {
            Queued,
            Pump,
            Exhausted,
        }
        let outcome = update(|state| {
            if state.session != session { return Busy::Pump; }
            match kind {
                RequestKind::SaveList if state.save_list_session == Some(session) => {
                    let operation = cloud::BandOperation::List { session };
                    if !state.band_queue.iter().any(|queued| matches!(queued, cloud::BandOperation::List { .. })) { state.band_queue.push_back(operation); }
                    Busy::Pump
                }
                RequestKind::SavePut if state.save_put_session == Some(session) => {
                    if let Some(band) = state.cloud_pending.as_ref().map(|pending| pending.band.clone()) {
                        let operation = cloud::BandOperation::Put { session, band };
                        if !state.band_queue.iter().any(|queued| matches!(queued, cloud::BandOperation::Put { .. })) { state.band_queue.push_back(operation); }
                    }
                    Busy::Pump
                }
                RequestKind::PackList | RequestKind::SaveList | RequestKind::StatsList => {
                    if state.refresh_retries >= REFRESH_RETRY_LIMIT {
                        // **补发次数用完必须收口**：不能留下一个永远为真的 busy 位。
                        // 之前这里只重置计数就返回，`saves_busy` / `stats_busy` 会一直挂在
                        // 界面上（用户看到「一直正在读取」），而且再也不会有下一次补发。
                        state.refresh_retries = 0;
                        match kind {
                            RequestKind::SaveList => {
                                state.saves_busy = false;
                                if state.saves_error.is_empty() {
                                    state.saves_error = "手环通道一直被占用，请稍后点「刷新」重试".into();
                                }
                            }
                            RequestKind::StatsList => {
                                state.stats_busy = false;
                                if state.stats_error.is_empty() {
                                    state.stats_error = "手环通道一直被占用，请稍后点「刷新」重试".into();
                                }
                            }
                            _ => {}
                        }
                        Busy::Exhausted
                    } else {
                        state.refresh_retries = state.refresh_retries.saturating_add(1);
                        let step = match kind {
                            RequestKind::PackList => RefreshStep::PackList,
                            RequestKind::SaveList => RefreshStep::SaveList,
                            _ => RefreshStep::StatsList,
                        };
                        if !state.refresh_queue.contains(&step) { state.refresh_queue.push(step); }
                        Busy::Queued
                    }
                }
                _ => Busy::Pump,
            }
        });
        match outcome {
            Busy::Queued => {
                // 定时器带上会话号：重连后旧定时器到点必须被丢弃，
                // 否则它会把新会话刚排好的队列提前取走一条。
                arm_timer(FOLLOWUP_DELAY_MS, json!({
                    "type": "amakano.timer", "kind": "refresh-next", "session": session
                }).to_string());
            }
            Busy::Pump => {
                arm_timer(FOLLOWUP_DELAY_MS, json!({ "type": "amakano.timer", "kind": "band-pump", "session": session }).to_string());
            }
            Busy::Exhausted => {
                set_status(StatusKind::Warn, "手环通道一直被占用，本次刷新已放弃；请稍后重试");
                render();
                settle_refresh_step();
            }
        }
        return;
    }
    if addr.is_empty() {
        update(|state| {
            if state.verify_request.as_deref() == Some(id.as_str()) {
                state.verify_request = None;
                state.verify_attempts = state.verify_attempts.saturating_add(1);
                if state.verify_attempts >= 3 {
                    state.verify_pack = None;
                }
            }
        });
        set_status(StatusKind::Bad, "尚未连接手环，请先点「连接设备」");
        render();
        return;
    }
    // **先占坑、再发消息**：反过来的话，回包万一下得比占坑还快，
    // `clear_request` 会扑个空，接着武装的定时器就会给一个已经答过的请求再重发一次
    // （实测里那些「同一 requestId 收到两份回包」就有这一份）。
    update(|state| {
        state.request.arm(PendingRequest { kind, id: id.clone(), payload: payload.clone(), addr: addr.clone(), session, attempts: 1, probe });
        state.refresh_retries = 0;
    });
    // **分片缓存只在这里清，不在 `request_*` 里清。**
    // 分片是「这条请求的回包」，清空点必须和「真正发出去」对齐：旧实现在
    // `request_save_list` / `request_stats` 的**入口**就 reset，可那两个函数在坑位忙时
    // 只是把请求排进 `refresh_queue` —— 一条**还没发出去**的排队请求，会先把
    // 在途请求已经收到的碎片全部抹掉。真机 2026-10-31 17:14:52 那批
    // `late or foreign stats shard ignored` 就是这么来的：三条 `stats-*` 排队，
    // 52.9 秒齐到的回包在前面几条的 reset 面前一片不剩。
    match kind {
        RequestKind::SaveList => with_shards(&SAVE_SHARDS, SaveShardSlot::reset),
        RequestKind::StatsList => with_shards(&STATS_SHARDS, SaveShardSlot::reset),
        _ => {}
    }
    if send_message(&addr, &payload).await.is_err() {
        clear_request(&id);
        let failures = update(|state| {
            state.send_failures += 1;
            if state.send_failures >= 3 && state.connected {
                // 连续三次「消息根本没出去」= 应用已经不在前台（或蓝牙断了）。
                // 交给 `on_event` 在回包栈之外重走连接，不在这里递归调用。
                state.alive = false;
                state.reconnect_pending = true;
            }
            state.send_failures
        });
        // 这条日志是「连不上」的现场证据：宿主返回 Err 说明消息**根本没出去**
        // （宿主按「设备地址 + 包名」精确匹配，匹配不到就直接失败），
        // 与「发出去了但应用没回」是完全不同的两件事，别再让两者在界面上长得一样。
        tracing::warn!(kind = kind.label(), addr = %addr, failures, "interconnect send failed");
        // 核对请求发不出去时也要有下一步：重试最多 3 次，否则清掉核对并如实报错。
        let (retry_verify, drop_verify) = update(|state| {
            let mine = state.verify_request.as_deref() == Some(id.as_str());
            if !mine {
                return (false, false);
            }
            state.verify_request = None;
            state.verify_attempts = state.verify_attempts.saturating_add(1);
            if state.verify_attempts < 3 {
                (true, false)
            } else {
                state.verify_pack = None;
                (false, true)
            }
        });
        if retry_verify {
            arm_timer(1000, json!({ "type": "amakano.timer", "kind": "verify-complete" }).to_string());
        } else if drop_verify {
            set_status(StatusKind::Bad, "核对安装状态失败：请刷新章节列表确认这一章是否已装上");
        }
        set_status(
            StatusKind::Bad,
            format!("{}失败：消息发不出去（已失败 {failures} 次），请重新连接设备", kind.label()),
        );
        // **发送失败也是一次 settle**：否则「刷新」队列里排着的下一步永远不会发，
        // 而发起刷新的动作（存档 / 统计）已经把 busy 置上了 —— 界面会一直停在读取中。
        // 云端那条链要单独收口：PUT 成功后 `cloud_pending` + Applying 正等着一次
        // SaveList 来核对写回结果，这条核对请求发不出去时必须走 `apply_failed`，
        // 否则界面永久停在「云端已保存，正在核对手环最新存档…」，而且 `cloud_running`
        // 一直是 true，后续所有云同步都会被挡回去。
        let (session, cloud_pending, save_list_waiting) = update(|state| {
            let session = state.session;
            state.saves_busy = false;
            state.stats_busy = false;
            state.pending_save_slot = None;
            let cloud_pending = state.cloud_pending.is_some();
            let save_list_waiting = state.save_list_session.take().is_some();
            if state.save_put_session.take().is_some() {
                state.cloud_pending = None;
            }
            (session, cloud_pending, save_list_waiting)
        });
        if cloud_pending && save_list_waiting {
            cloud::apply_failed(session, "无法核对手环写回结果：消息发不出去，请重新连接设备后再同步");
        }
        settle_refresh_step();
        render();
        return;
    }
    // 超时**按请求种类**来：存档比章节列表宽（手环端要读 storage、拼 6KB 分片），
    // 轻量类给短窗口好让丢包赶紧重发。理由与实测数字见 `src/request.rs`。
    arm_timer(kind.timeout_ms(), json!({ "type": "amakano.timer", "kind": "request", "requestId": id }).to_string());
}

fn fail_request(kind: RequestKind, request_id: &str) {
    let failure = update(|state| {
        let session = state.session;
        let mut failure = None;
        if state.save_put_session.take() == Some(session) && kind == RequestKind::SavePut {
            failure = Some("手环写回超时，云端数据仍保留");
        }
        if state.save_list_session.take() == Some(session) && kind == RequestKind::SaveList {
            if state.cloud_pending.is_some() {
                failure = Some("无法核对手环写回结果");
            } else {
                state.cloud_phase = cloud::CloudPhase::Failed;
                state.cloud_detail = "读取手环存档超时，请重试".into();
                state.cloud_running = false;
            }
        }
        if matches!(kind, RequestKind::SaveCapture | RequestKind::SavePut | RequestKind::SaveDelete | RequestKind::SaveActivate) {
            state.pending_save_slot = None;
        }
        (session, failure)
    });
    if let (session, Some(message)) = failure { cloud::apply_failed(session, message); }
    // 存档通道：手动打开应用也不会让它长出存档支持，所以失败的归因先看「协议协商出来了没有」。
    // **但必须先确认应用已经活着**（`saves_capability_missing` 的 alive 那一半）：
    // 应用还没醒时的同款超时只是「手环没有响应」，不能替用户下「版本过旧」的结论。
    // 真判成版本问题时只置 `saves_unsupported` 状态位；那句「手环端应用版本过旧」的
    // **结论**与**怎么办**由界面统一渲染（ui-core 的 `saves_blocked_hint` /
    // `saves_blocked_action`），这里不再写第二遍 —— 否则用户会在同一张卡上看到
    // 两句几乎一样的提示（实机截图就是这么来的）。
    let (alive, save_protocol) = {
        let current = state().lock().unwrap_or_else(|error| error.into_inner());
        (current.alive, current.save_protocol)
    };
    let saves_blocked = kind.is_saves() && saves_capability_missing(alive, save_protocol);
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
        RequestKind::Ping => "手环没有响应（心跳超时）",
        // 存档通道：没协商出协议 → 状态行只给一句短状态（结论+怎么办在「存档」页那张卡上）；
        // 协议没问题、只是这一次超时 → 按普通超时讲，别再把手环说成「版本过旧」。
        RequestKind::Hello | RequestKind::SaveList | RequestKind::SavePut | RequestKind::SaveDelete | RequestKind::SaveActivate | RequestKind::SaveCapture => {
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
    // **码是给机器看的**：界面靠它区分「等一等就好」和「得去腾空间 / 换档位 / 更新版本」。
    // 判定顺序和上面那句人话完全一致，别让两者漂开。
    let code = match kind {
        RequestKind::StatsList if stats_blocked => ErrorCode::Protocol,
        RequestKind::StatsList => ErrorCode::Timeout,
        RequestKind::Hello | RequestKind::SaveList | RequestKind::SavePut | RequestKind::SaveDelete | RequestKind::SaveActivate | RequestKind::SaveCapture => {
            if saves_blocked {
                ErrorCode::Protocol
            } else {
                ErrorCode::Timeout
            }
        }
        _ => ErrorCode::Timeout,
    };
    if matches!(kind, RequestKind::PackList) {
        // 只有失败的那条请求**就是核对请求**时，才走核对重试。
        // 用户自己的「刷新」超时不能被吞成核对失败，也不能清掉核对目标。
        let mine = update(|state| state.verify_request.as_deref() == Some(request_id));
        if mine {
            let retry = update(|state| {
                state.verify_request = None;
                state.verify_attempts = state.verify_attempts.saturating_add(1);
                state.verify_attempts < 3
            });
            if retry {
                set_status(StatusKind::Warn, "核对安装状态失败，正在重试…");
                render();
                arm_timer(500, json!({ "type": "amakano.timer", "kind": "verify-complete" }).to_string());
                return;
            }
            update(|state| {
                state.verify_pack = None;
                state.verify_attempts = 0;
            });
            set_status(StatusKind::Bad, "核对安装状态失败：请刷新章节列表确认这一章是否已装上");
            render();
            return;
        }
    }
    set_error(code, message);
    set_status(StatusKind::Bad, message);
    // 存档相关的超时同时落到存档页的状态上：用户可能正停在那一页。
    if kind.is_saves() {
        update(|state| {
            if saves_blocked {
                state.saves_unsupported = true;
                // 应用活着（答过章节列表）却始终协商不出协议：连 hello 通道都没有，
                // 一定是 1.x 旧代 —— 推送页的「卸载重装」卡片由此点亮，同步入口一并拦下
                // （章节包同样是换代格式，传了也没用）。
                //
                // ⚠️ **这条路上不清 `sync_queue` / `auto_resume`**：这是**推测**（真 1.x 是
                // 一码事，应用单纯忙半拍导致 hello 超时是另一码事），而队列是用户的批次意图。
                // 猜错就毁掉一整批任务，代价不对称。队列留在那儿只是显示成「排队中」（同步入口
                // 本来就被拦着），hello 一旦真的答上（10 秒一次的心跳会再问），hello-ok 那条
                // 权威路径会把状态连同队列一起收拾干净。
                state.band_game_legacy = true;
            } else if state.saves_error.is_empty() {
                state.saves_error = message.into();
            }
            state.saves_busy = false;
            state.pending_save_slot = None;
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
            fail_request(kind, request_id);
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
    let _ = send_message(&addr, &payload).await;
    set_status(StatusKind::Warn, format!("{}超时，正在自动补试…", kind.label()));
    render();
    arm_timer(kind.timeout_ms(), json!({ "type": "amakano.timer", "kind": "request", "requestId": request_id }).to_string());
}

async fn send_packet(addr: String, payload: String, index: usize, retry: bool) {
    if retry || index == 0 || index % 64 == 0 {
        tracing::info!(index, bytes = payload.len(), retry, "sending pack chunk");
    }
    if send_message(&addr, &payload).await.is_err() {
        fail_transfer(ErrorCode::Link, "发送被链路拒绝");
    }
}

async fn send_next_packet() {
    for (addr, payload, index) in next_packets() {
        let alive = update(|state| {
            state
                .transfer
                .as_ref()
                .is_some_and(|transfer| transfer.started && transfer.request_id == payload_request_id(&payload))
        });
        if !alive {
            break;
        }
        send_packet(addr, payload, index, false).await;
    }
    arm_transfer_tick();
}

/// 从一条分片负载里读回 requestId —— 发送循环用它确认「这一批还属于当前传输」。
fn payload_request_id(payload: &str) -> String {
    serde_json::from_str::<Value>(payload)
        .ok()
        .and_then(|value| value.get("requestId").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_default()
}

/// 挂下一拍传输巡视。**同一时刻只有一条链**：载荷里带 requestId，
/// 旧传输的定时器到点会被 `tick_transfer` 的身份判据丢掉。
///
/// 节拍取自**这一趟传输的模式**：正常档 60 ms，激进档 25 ms。激进档的窗口更大
/// （16 片 × 32 KB），节拍不跟着压密的话，窗口填满之后就干等确认，白拿不到吞吐。
fn arm_transfer_tick() {
    let (request_id, tick_ms) = {
        let mut current = state().lock().unwrap_or_else(|error| error.into_inner());
        if current.transfer_tick_armed {
            return;
        }
        let (id, tick) = match current.transfer.as_ref() {
            Some(transfer) if transfer.started => {
                (transfer.request_id.clone(), transfer.mode.params().tick_ms)
            }
            _ => return,
        };
        current.transfer_tick_armed = true;
        (id, tick)
    };
    arm_timer(
        tick_ms,
        json!({ "type": "amakano.timer", "kind": "transfer-tick", "requestId": request_id })
            .to_string(),
    );
}

/// 传输巡视：**v3 的唯一调度点**。
///
/// 旧实现给在途的每一片各挂一个 `retry` 定时器 —— 8 片窗口就是 8 个定时器，
/// 每个确认回来都要再挂一次，定时器比数据消息还多。现在只有这一条巡视链
/// （节拍按模式取：正常 60 ms / 激进 25 ms）：
///   · 有空间就继续发（窗口 + 字节双约束）；
///   · 超过 `ack_timeout_ms` 没有累计确认，就从 `acked_index` 整窗重发（go-back-N）；
///   · 连续停滞到模式规定的次数（正常 6 / 激进 4）才放弃，交给自动续传。
///
/// **激进档在这里的风险最高**：它的确认间隔是 1.2 秒，判定下限却是 6 秒 —— 只要
/// 这个关系被改坏（比如有人把 `stall_floor_ms` 调小），每一次正常确认都会被判成停滞，
/// 于是整窗重发 → 窗口减半 → 永远跑不完。ui-core 的 `transfer_mode_tests` 钉着这条比例。
async fn tick_transfer() {
    let now = now_ms();
    let mut send = false;
    let mut request_id = String::new();
    let mut stalled = false;
    let mut give_up = false;
    let mut completed_early = false;
    update(|state| {
        let Some(transfer) = state.transfer.as_mut() else { return };
        if !transfer.started {
            return;
        }
        request_id = transfer.request_id.clone();
        if !transfer.ready {
            return;
        }
        // 手环主动让路期间**不许判停滞**：它本来就不回确认，判超时只会把
        // 「让它喘口气」变成「整窗重发 + 六次连败」。
        if transfer.paused_until_ms > now {
            return;
        }
        let params = transfer.mode.params();
        let timeout = ack_timeout_ms(transfer.mode, transfer.rtt_ms) as u128;
        if transfer.acked_index >= transfer.chunks.len() {
            // 数据都写完了，只差 `complete` 回执。回执丢一次不许把人永久卡在 100%：
            // 12 秒没等到就按「已完成」收尾（比手环 10 秒提交看门狗宽 2 秒）。
            // 手环提交阶段自己就有 10 秒看门狗；这里必须比它宽，否则会在登记完成前
            // 就去 pack.list 核对。12 秒 = 提交上限 + 2 秒余量。
            completed_early = now.saturating_sub(transfer.last_ack_ms) > 12_000;
            return;
        }
        // 窗口里还有空间（或压根还没发满）就先推数据。
        if transfer.in_flight.len() < transfer.window_size
            && transfer.next_index < transfer.chunks.len()
        {
            send = true;
        }
        // 停滞：有在途片，且超过一个确认窗口都没收到累计确认。
        let quiet = now.saturating_sub(transfer.last_ack_ms.max(transfer.last_send_ms));
        if !transfer.in_flight.is_empty() && quiet > timeout {
            transfer.stall_count = transfer.stall_count.saturating_add(1);
            transfer.retry_count = transfer.retry_count.saturating_add(1);
            transfer.window_size = (transfer.window_size / 2).max(2);
            transfer.next_index = transfer.acked_index;
            transfer.in_flight.clear();
            transfer.in_flight_bytes = 0;
            transfer.last_ack_ms = now;
            transfer.last_progress_ms = now;
            stalled = true;
            if transfer.stall_count >= params.give_up_stalls {
                give_up = true;
            } else {
                send = true;
            }
            tracing::warn!(
                window = transfer.window_size,
                acked = transfer.acked_index,
                stalls = transfer.stall_count,
                quiet,
                "transfer stalled; resending from the last cumulative ack"
            );
        }
    });
    if completed_early {
        let verify = update(|state| {
            let verify = state.pack_meta.as_ref().map(|meta| meta.pack_id.clone());
            state.transfer = None;
            state.transfer_tick_armed = false;
            state.pack_meta = None;
            state.pack_files = Vec::new();
            state.resume = None;
            state.verify_pack = verify.clone();
            state.verify_request = None;
            state.verify_attempts = 0;
            state.status = "数据已传完，正在核对手环安装状态…".into();
            state.status_kind = StatusKind::Info;
            verify
        });
        render();
        if verify.is_some() {
            // 不盲判成功：拉一次 pack.list，确认这个 packId 真的登记了再推进队列。
            arm_timer(300, json!({ "type": "amakano.timer", "kind": "verify-complete" }).to_string());
        } else {
            arm_timer(300, json!({ "type": "amakano.timer", "kind": "next-queued" }).to_string());
        }
        return;
    }
    if give_up {
        fail_transfer(ErrorCode::Timeout, "连续多次没有收到手环确认");
        return;
    }
    if stalled {
        set_status(StatusKind::Warn, "手环确认变慢，正在从已确认的位置重发…");
        render();
    }
    if send {
        send_next_packet().await;
    } else if update(|state| {
        state
            .transfer
            .as_ref()
            .is_some_and(|transfer| transfer.started && transfer.ready)
    }) {
        arm_transfer_tick();
    }
}

/// 传输中断：不改分片档位，保留断点，提示用户重连后继续。
///
/// **必须把 `pack_meta` / `transfer` 一起清掉。** 界面上的 `PackView::active` 判的是
/// 「`pack_meta.chapter_number` 等于这一章的号」，而章节行在 `active` 时渲染的是
/// `state_badge("传输中")` —— **一个徽章，不是按钮**（`ui-core/src/pages/library.rs:154`）。
/// 只清 `started` 不清 `pack_meta` 的话，失败的那一章会永久显示「传输中」且点不动，
/// 只能靠「同步任意其它章」或重载插件脱困。
/// 真机复现：2026-09-24 18:02 第 2 章传输失败后，那一章的按钮再也点不动。
///
/// 断点本身不会因此丢：权威进度在手环的 `pending-install` 里，这里放开的只是
/// 「当前这一章正在传」这个**会话内**状态，让按钮重新可点。
fn fail_transfer(code: ErrorCode, reason: &str) {
    // 自动续传的预算耗尽与否决定状态行说什么：还有预算就说「自动接着传」，
    // 耗尽了就明说「要点继续传输」—— 旧版这里静默放弃，界面还挂着「正在自动重连并接着传」，
    // 用户等来的却是什么都不发生。
    let mut exhausted = false;
    update(|state| {
        if let Some(transfer) = state.transfer.as_mut() {
            transfer.started = false;
            transfer.ready = false;
            transfer.waiting_ready = false;
            transfer.ready_deadline_ms = 0;
            transfer.last_progress_ms = 0;
            transfer.in_flight.clear();
            transfer.window_size = transfer.mode.params().initial_window.min(transfer.window_cap);
        }
        // 记下「刚才在传哪一章」—— 重连并拿到手环断点后自动接着传。
        // **必须在清 `pack_meta` 之前取**。次数是防死循环的闸门：一个时间窗内最多自动重试
        // `MAX_AUTO_RESUME` 次（见 `auto_resume_budget`），超了就留给用户决定。
        if let Some(meta) = state.pack_meta.as_ref() {
            let pack_id = meta.pack_id.clone();
            let chapter_number = meta.chapter_number;
            let previous = state.auto_resume.as_ref().filter(|item| item.1 == pack_id);
            let attempts = previous.map(|item| item.2).unwrap_or(0);
            let window_start = previous.map(|item| item.3).unwrap_or(0);
            let (used, window_start, allowed) = auto_resume_budget(now_ms() as u64, window_start, attempts);
            if allowed {
                state.auto_resume = Some((chapter_number, pack_id, used, window_start));
            } else {
                tracing::warn!(attempts = used, pack = %pack_id, "auto-resume budget exhausted");
                state.auto_resume = None;
                exhausted = true;
            }
        }
        state.transfer = None;
                state.transfer_tick_armed = false;
        state.pack_meta = None;
        state.pack_files = Vec::new();
        // 阶段退回到「应用活着」：链路断了，后面的章节列表/传输都还不成立。
        state.stage = state.stage.min(SessionStage::AppAlive);
        state.error = Some(ErrorView::new(code, reason));
        state.status = if exhausted {
            format!("连接已断开（{reason}）。自动续传已达上限，重连后点「继续传输」接着传")
        } else {
            format!("连接已断开（{reason}）。正在自动重连并接着传（也可以在 AstroBox 里手动重连）")
        };
        state.status_kind = StatusKind::Bad;
        // 断线恢复不该等用户点：直接排队一次自动重连（由 `on_event` 在 await 链上执行）。
        state.reconnect_pending = true;
        state.alive = false;
    });
    // 「刚才在传哪一章」也要落盘：插件被重启/刷新后 `on_load` 读回它，
    // 配合手环的断点就能接着传，而不是等用户重新点一遍。
    save_persisted();
    render();
}


async fn handle_ready_timeout(request_id: &str) {
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
            set_status(StatusKind::Warn, "手环未响应，正在重发导入请求…");
            render();
            let _ = send_message(&addr, &payload).await;
            arm_timer(READY_TIMEOUT_MS, json!({ "type": "amakano.timer", "kind": "ready", "requestId": request_id }).to_string());
        }
        None => fail_transfer(ErrorCode::Timeout, "手环未响应导入请求"),
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
                handle_ready_timeout(id).await;
            }
        }
        Some("pending") => request_pending().await,
        Some("hello") => request_hello().await,
        Some("send-next") => send_next_packet().await,
        // 核对巡视：请求坑位可能被用户的刷新顶掉、地址可能刚断线，单靠一次
        // `verify-complete` 会永久悬空。每 1 秒检查一次，直到有结论。
        Some("verify-tick") => {
            let (pending, waiting) = update(|state| {
                (
                    state.verify_pack.clone(),
                    state.verify_request.clone(),
                )
            });
            if pending.is_none() {
                return;
            }
            if let Some(id) = waiting {
                let still_mine = update(|state| {
                    state.request.get().map(|request| request.id == id).unwrap_or(false)
                });
                if still_mine {
                    arm_timer(1000, json!({ "type": "amakano.timer", "kind": "verify-tick" }).to_string());
                } else {
                    // 被用户刷新顶掉了：**不算核对失败**，只清掉旧 requestId，
                    // 等坑位空出来再发。用户自己的刷新回包若命中 packId 会直接结案。
                    update(|state| state.verify_request = None);
                    arm_timer(500, json!({ "type": "amakano.timer", "kind": "verify-complete" }).to_string());
                }
            } else {
                arm_timer(200, json!({ "type": "amakano.timer", "kind": "verify-complete" }).to_string());
            }
        }
        Some("verify-complete") => {
            let (pending, in_flight) = update(|state| {
                (state.verify_pack.is_some(), state.verify_request.is_some())
            });
            if !pending {
                return;
            }
            // 已经有一条核对请求在途：不要重复发（每次 pack.list 都是手环侧全量扫描）。
            if in_flight {
                arm_timer(500, json!({ "type": "amakano.timer", "kind": "verify-tick" }).to_string());
                return;
            }
            if update(|state| state.request.busy()) {
                arm_timer(FOLLOWUP_DELAY_MS, json!({ "type": "amakano.timer", "kind": "verify-complete" }).to_string());
                return;
            }
            let id = format!("packs-verify-{}", now_ms());
            update(|state| state.verify_request = Some(id.clone()));
            arm_timer(1000, json!({ "type": "amakano.timer", "kind": "verify-tick" }).to_string());
            let payload = json!({ "type": "amakano.pack.list", "requestId": id }).to_string();
            dispatch_request(RequestKind::PackList, payload, id, false).await;
        }
        Some("next-queued") => {
            // 用户可能已经手动开始了另一章；这个定时器是上一章核对成功后挂的，
            // 不许覆盖正在进行的传输。
            let busy = update(|state| {
                state.verify_pack.is_some()
                    || state
                        .transfer
                        .as_ref()
                        .is_some_and(|transfer| transfer.started)
            });
            if busy {
                return;
            }
            if let Some((number, remaining)) = pop_sync_queue() {
                tracing::info!(number, remaining, "starting queued pack after a lost completion receipt");
                start_embedded_transfer(number).await;
            }
        }
        // 探测/心跳：载荷里带会话号，**上一轮连接残留的定时器一律丢掉** ——
        // 否则重连之后会有两条心跳链并行，而且每轮翻一倍。
        Some("probe") => {
            let session = value.get("session").and_then(Value::as_u64).unwrap_or(0);
            if update(|state| state.session == session) {
                probe_tick(session).await;
            } else {
                tracing::debug!(session, "stale probe/heartbeat timer ignored");
            }
        }
        // 补拉章节列表。坑位被占就**隔一拍重试**，**绝不静默丢弃** ——
        // 「静默丢弃」正是「连接后半天读不到已安装章节」那条真机故障的形态
        // （2026-09-24 20:10：用户卡在「正在读取手环上已安装的章节…」）。
        // 坑位最终一定会空出来（请求有 1–2 秒超时兜底），所以这个重试会收敛。
        Some("list") => {
            if update(|state| state.request.busy()) {
                arm_timer(FOLLOWUP_DELAY_MS, json!({ "type": "amakano.timer", "kind": "list" }).to_string());
            } else {
                request_pack_list(false).await;
            }
        }
        // 传输收尾之后隔一拍再拉章节列表（见 `amakano.files.complete` 那一支）。
        Some("post-complete") => request_pack_list(false).await,
        // 手环让暂停时自己安排恢复：暂停期间它不会回确认，窗口里没有别的路会推下一轮。
        Some("resume") => tick_transfer().await,
        // v3 的传输巡视：填窗口 + 查停滞。载荷里的 requestId 用来丢掉上一条传输的残留定时器。
        Some("transfer-tick") => {
            let request_id = value.get("requestId").and_then(Value::as_str).unwrap_or("");
            let current = update(|state| {
                state.transfer_tick_armed = false;
                state.transfer.as_ref().map(|transfer| transfer.request_id.clone()).unwrap_or_default()
            });
            if current == request_id {
                tick_transfer().await;
            }
        }
        // 「刷新」的下一个往返（前一个已经 settle，见 `settle_refresh_step`）。
        // **必须核对会话号**：重连会清空队列、但撤不掉已注册的定时器；
        // 旧定时器到点若直接取队列，会把新会话的步骤提前抽走。
        Some("refresh-next") => {
            let timer_session = value.get("session").and_then(Value::as_u64);
            let next = update(|state| {
                if timer_session.is_some_and(|session| session != state.session) {
                    tracing::debug!(?timer_session, "stale refresh-next timer ignored");
                    return None;
                }
                if state.refresh_queue.is_empty() { None } else { Some(state.refresh_queue.remove(0)) }
            });
            if let Some(step) = next {
                run_refresh_step(step).await;
            }
        }
        // 「收完一条回包顺手再拉一次存档」：**不在消息回调里立刻发**，隔一拍再发。
        // 实测「紧跟回包几毫秒就发」的那一档丢了 61.5%（`request::FOLLOWUP_DELAY_MS`）。
        Some("saves-list") => request_band_operation(cloud::BandOperation::List { session: update(|state| state.session) }).await,
        Some("band-pump") => {
            let timer_session = value.get("session").and_then(Value::as_u64);
            if timer_session.is_none_or(|session| update(|state| state.session) == session) { pump_band_queue().await; }
        }
        // 切到「统计」页时顺手续拉一次（同样隔一拍再发，理由同上）。
        Some("stats-list") => request_stats().await,
        _ => {}
    }
}

/// 刷新在线设备列表（顶栏的设备选择器用）。
///
/// 只枚举，不连接、不碰 interconnect —— 枚举失败也保留上一份列表，
/// 避免网络/宿主瞬断把设备选择器清空。
async fn refresh_devices() {
    let devices: Vec<DeviceEntry> = psys_host::device::get_connected_device_list()
        .await
        .into_iter()
        .map(|item| DeviceEntry { name: item.name, addr: item.addr })
        .collect();
    if devices.is_empty() {
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
    }
    update(|state| state.devices = devices);
    render();
}

/// 连接手环。`index` 是在线设备列表里的下标；`None` 表示沿用已选设备或取第一台。
///
/// **多设备**：旧实现无条件取 `devices.first()`，于是 AstroBox 连了两台手环时
/// 用户永远只能连到第一台，右上角也没有任何选择入口。现在由界面选下标，
/// 列表本身通过 [`refresh_devices`] 同步。
///
/// `auto` 是「这次连接是用户在点，还是我们自己重连」。它只影响一件事：
/// **自动重连不许拉起游戏**。真机日志里 `heartbeat lost → auto-reconnecting` 每几轮就来一次，
/// 而重连一旦带上 `auto_launch`，就等于「你没在用的时候插件也在反复把游戏推到前台」。
/// 用户手点「连接设备」时仍按设置里的开关走。
async fn connect_device(index: Option<usize>, auto: bool) {
    {
        let devices = psys_host::device::get_connected_device_list().await;
        // 先把点击时那一台解析成地址，再做在线复核：设备列表可能在用户点开菜单之后
        // 又变了（拔掉、重连），不能沿用旧下标去连一台已经不在的设备。
        let clicked_addr = index.and_then(|index| {
            state()
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .devices
                .get(index)
                .map(|item| item.addr.clone())
        });
        let _ = update(|state| {
            state.devices = devices
                .iter()
                .map(|item| DeviceEntry { name: item.name.clone(), addr: item.addr.clone() })
                .collect();
        });
        let chosen = {
            let current = state().lock().unwrap_or_else(|error| error.into_inner());
            clicked_addr
                .as_deref()
                .and_then(|addr| devices.iter().find(|device| device.addr == addr))
                .or_else(|| devices.iter().find(|device| device.addr == current.device_addr))
                .or_else(|| devices.first())
        };
        let Some(device) = chosen else {
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
        // 换了一台设备：上一台的回包通道注册不能沿用，必须重新 register。
        update(|state| {
            state.device_addr = addr.clone();
            state.device_name = name;
            state.alive = false;
            state.probe_attempt = 0;
            state.send_failures = 0;
            // 新会话：上一轮残留的探测/心跳定时器就此作废（见 `session` 字段的说明）。
            state.session = state.session.wrapping_add(1);
            state.request = Slot::idle();
            state.refresh_queue.clear();
            state.catalog_answered = false;
            state.refresh_retries = 0;
            state.band_queue.clear();
            state.save_list_session = None;
            state.save_put_session = None;
            state.pending_current_save = None;
            state.pending_save_slot = None;
            state.cloud_pending = None;
            state.cloud_generation = state.cloud_generation.wrapping_add(1);
            state.cloud_reread = None;
            state.cloud_running = false;
            if matches!(state.cloud_phase, cloud::CloudPhase::Syncing | cloud::CloudPhase::Applying) {
                state.cloud_phase = cloud::CloudPhase::Offline;
                state.cloud_detail = "连接已变化，本次云同步已取消；将重新读取并合并".into();
            }
            with_shards(&SAVE_SHARDS, SaveShardSlot::reset);
            with_shards(&STATS_SHARDS, SaveShardSlot::reset);
            // 重新连接就把进度条**重置**：分段条说的是「这一次走到哪了」，
            // 上一轮的「章节列表已同步」不成立（列表还得重新拉）。
            state.stage = SessionStage::DeviceFound;
            state.error = None;
            state.installed_broken.clear();
            // 代际判定也按**本次会话的新证据**重来：上一轮的「1.x 旧版」可能已经被用户
            // 卸载重装解决了，别让旧结论盖着新会话（hello-ok 一到就会重新给出判定）。
            state.band_game_legacy = false;
            state.heartbeat_misses = 0;
            state.last_alive_ms = now_ms();
            state.reconnect_pending = false;
            state.pending_checked = false;
            state.pending_answered = false;
            // 「已发导入请求、还没等到手环确认」的那次握手**属于上一条链路**：新会话不可能
            // 再有人来应答它。留着它就等于把这次传输永久挂在「正在传」上 ——
            // 界面所有同步/继续按钮被 `is_transferring()` 禁掉、速度与剩余永远显示「—」，
            // 用户点什么都没反应（2026-10-02 真机：重连后点「继续传输」不传输）。
            // 断点本身不会因此丢：它在手环的 `pending-install.txt` 里，接着传由
            // `amakano.pack.pending` + 自动续传/「继续传输」重新发起。
            if state.transfer.as_ref().is_some_and(|transfer| !transfer.ready) {
                state.transfer = None;
                state.transfer_tick_armed = false;
                state.pack_meta = None;
                state.pack_files = Vec::new();
            }
            // **多章队列不许在这里清**：重连（含断线自动重连）不是用户的本意，
            // 「同步剩余 N 章」的意图要活过断线 —— 当前那章由断点续传/「继续传输」接上，
            // 传完 `pop_sync_queue` 自然继续剩下的。清掉它等于一次断线毁掉整批任务。
            state.status = "正在连接手环…".into();
            state.status_kind = StatusKind::Info;
        });
        render();
        let registered = register_interconnect(&addr).await.is_ok();
        if !registered {
            set_status(StatusKind::Bad, "已找到设备，但回包注册失败，请重试或重启 AstroBox");
            update(|state| state.connected = false);
            render();
            return;
        }
        // 注册成功即视为「通道已建立」；应用是否在前台由探测循环判断。
        update(|state| state.connected = true);
        set_stage(SessionStage::ChannelRegistered);
        let auto_launch = {
            let current = state().lock().unwrap_or_else(|error| error.into_inner());
            current.auto_launch
        };
        // 自动重连（心跳判掉线）**不拉起游戏**：用户可能正在手环上做别的事，
        // 或者干脆没在用；把游戏推到前台是「用户点连接」才该有的副作用。
        let launch = if registered && auto_launch && !auto {
            launch_watch_app(&addr).await
        } else {
            LaunchOutcome::Skipped
        };
        if auto {
            // 自动重连是插件自己发起的一次「悄悄接回去」：状态行只报告连接结果，
            // 不拿「未自动打开应用」这种对用户无意义的提示去占一行、还标成警告色。
            update(|state| {
                state.status = "已重新接通手环，正在等待《甜蜜女友2》回应…".into();
                state.status_kind = StatusKind::Info;
            });
            render();
            tracing::info!("auto-reconnect registered the interconnect channel");
        } else {
            let hint = launch.hint();
            update(|state| {
                state.status = format!("已连接手环。{hint}");
                state.status_kind = if launch.is_problem() { StatusKind::Warn } else { StatusKind::Good };
            });
            render();
            tracing::info!(?hint, "connect finished");
        }
        // 先给应用 2 秒启动时间，然后按固定间隔轮询；手动打开也能接上。
        arm_probe(APP_START_DELAY_MS, update(|state| state.session));
        // **能力协商（amakano.app.hello）不在这里发。** 应用刚被 `launch-qa` 拉起、还没注册
        // interconnect 监听，这时发出去的 hello 只会全部落空；三次重试用完（3×2 秒）就会把
        // 「应用还没醒」误判成「手环端应用版本过旧」。改由 `probe_tick` 在应用**答过消息**
        // 之后发出 —— 那时 `hello-ok` 缺席才真是版本问题。
    }
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
            LaunchOutcome::Denied => "尝试打开《甜蜜女友2》被系统拒绝，请手动在手环上打开".into(),
            LaunchOutcome::ListFailed => "读取手环应用列表失败（需要 thirdpartyapp 权限），请手动在手环上打开《甜蜜女友2》".into(),
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
    if launch_qa(addr, &direct).await.is_ok() {
        tracing::info!("launched watch app by package name");
        return LaunchOutcome::Launched;
    }
    tracing::warn!("direct launch rejected, falling back to the app list");
    let Ok(apps) = thirdparty_app_list(addr).await else {
        return LaunchOutcome::ListFailed;
    };
    let Some(app) = apps.iter().find(|item| item.package_name == PACKAGE_NAME) else {
        let names: Vec<&str> = apps.iter().take(3).map(|item| item.app_name.as_str()).collect();
        let summary = if names.is_empty() { "0 个应用".to_string() } else { format!("{} 个：{}", apps.len(), names.join("、")) };
        return LaunchOutcome::NotFound(summary);
    };
    match launch_qa(addr, app).await {
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
    let id = format!("saves-{}", now_ms());
    let payload = json!({ "type": "amakano.saves.list", "requestId": id }).to_string();
    dispatch_request(RequestKind::SaveList, payload, id, false).await;
}

/// 从手环读一次阅读统计。
///
/// 与存档同一套骨架（先清分片缓存 → 占坑 → 发消息 → 按种类的超时兜底），
/// 差别只在请求名与坑位：统计走 `amakano.stats.*`，分片累积在 `STATS_SHARDS`。
async fn request_stats() {
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
    let revision = update(|state| {
        state.cloud_pending
            .as_ref()
            .map_or(state.save_list_revision, |pending| pending.band.revision)
    });
    let payload = json!({
        "type": "amakano.saves.put",
        "requestId": id,
        "action": action,
        "confirm": action == "replace",
        "saves": saves,
        "autoSave": auto_save,
        "expectedRevision": revision,
    })
    .to_string();
    dispatch_request(RequestKind::SavePut, payload, id, false).await;
}

/// 只替换手动槽、完全不碰自动存档的写回。
///
/// `save-sync.js` 的 replace 只有在报文字段里**显式带 `autoSave`** 时才写 autoSave；
/// 当前进度保存不该顺便回滚游戏刚刚推进的自动存档，所以这条路径不携带该字段。
async fn request_manual_slots_replace(saves: Vec<Value>) {
    let id = format!("savput-{}", now_ms());
    let revision = update(|state| state.save_list_revision);
    let payload = json!({
        "type": "amakano.saves.put",
        "requestId": id,
        "action": "replace",
        "confirm": true,
        "saves": saves,
        "expectedRevision": revision,
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

/// 取一份“当前进度”快照，供插件在存档页直接写入指定槽。
///
/// 存档页本身拿不到游戏页面内存里的进度；手环端 `amakano.saves.capture` 会把
/// `autoSave`（最新自动存档）原样回传。插件只在得到对象后才允许写入，绝不伪造存档。
async fn request_save_capture() {
    let id = format!("savcap-{}", now_ms());
    let payload = json!({ "type": "amakano.saves.capture", "requestId": id }).to_string();
    dispatch_request(RequestKind::SaveCapture, payload, id, false).await;
}

/// 连接后轮询：手动打开手环应用也能自动接上，回应一次就进入「已连接」。
/// 连接期探测 + 稳态心跳，一条链走完。
///
/// 旧实现里 `alive` 分支处理完存档能力就 `return` 且**不重新挂定时器** —— 探测循环
/// 就此终止，之后掉线、手环应用被杀都发现不了，要等用户下一次操作才暴露。
/// 现在这个分支改成挂**较长的**心跳间隔，并由心跳判定掉线。
async fn probe_tick(session: u64) {
    let (addr, alive, busy, last_alive) = {
        let current = state().lock().unwrap_or_else(|error| error.into_inner());
        (
            current.device_addr.clone(),
            current.alive,
            current.request.busy(),
            current.last_alive_ms,
        )
    };
    if addr.is_empty() {
        return;
    }
    if alive {
        // ── 稳态：心跳 ──
        let ping_supported = update(|state| state.supports_ping);
        // 不认 ping 的旧应用靠 hello/pack.list 保活：这两条往返本身要几秒，
        // 静默阈值放宽到 20 秒；新应用保持 12 秒。
        let silent_limit = if ping_supported {
            HEARTBEAT_SILENT_MS
        } else {
            20_000
        };
        let silent = now_ms().saturating_sub(last_alive);
        if silent > silent_limit {
            let misses = update(|state| {
                state.heartbeat_misses += 1;
                state.heartbeat_misses
            });
            if misses >= HEARTBEAT_MAX_MISSES {
                // **静默 ≠ 链路坏了。** 手环侧的 `ping` 只有应用在前台时才回，
                // 所以「连漏三拍」最常见的真实原因是用户退出了《甜蜜女友2》（或熄屏），
                // 不是蓝牙断了。旧实现在这里立 `reconnect_pending`，于是每 20 秒就整轮
                // 重连一次（重新 register + 按 auto_launch 拉起游戏），日志里刷满
                // `heartbeat lost: scheduling reconnect`，用户还会被莫名推到游戏界面。
                //
                // 正确的归位是把 `alive` 交还给「连接期探测」：那条链本来就一直探、
                // 不设放弃终点，应用一开就会自己接上（`mark_alive()` 置回 alive）。
                // 只有消息**根本发不出去**（`send_failures`，见 `dispatch_request`）
                // 才是链路问题，那种情况才值得重连。
                tracing::info!(misses, silent, "band app went silent; falling back to the probe loop");
                set_status(StatusKind::Info, "《甜蜜女友2》当前没有运行，正在等待它重新打开…");
                update(|state| {
                    state.alive = false;
                    state.probe_attempt = 0;
                    state.heartbeat_misses = 0;
                });
                render();
                arm_probe(PROBE_INTERVAL_MS, session);
                return;
            }
            // 单拍漏答只是「可能刚好在写盘」，不值得占一行警告刷屏。
            tracing::debug!(misses, silent, "heartbeat missed");
        }
        if !busy {
            // **必须把结果写回 `probe_attempt`**：只读 `wrapping_add` 会让 beat 恒为 1，
            // 旧应用的 pack.list 分支永远选不中（上一轮评审发现的死代码）。
            // 也不要在这里清 `heartbeat_misses`：`mark_alive()` 已经在收到任何回包时清零，
            // 这里每拍清零会让「连续漏 3 拍判掉线」永远不成立。
            let beat = update(|state| {
                state.probe_attempt = state.probe_attempt.wrapping_add(1);
                state.probe_attempt
            });
            match keepalive_action(ping_supported, beat) {
                Keepalive::Ping => request_ping().await,
                Keepalive::Hello => request_hello().await,
                Keepalive::PackList => request_pack_list(true).await,
            }
        }
        arm_probe(HEARTBEAT_INTERVAL_MS, session);
        return;
    }
    // ── 连接期：一直探，不设「放弃」终点 ──
    //
    // 旧实现在 12 次 × 2.5 秒之后**不再挂定时器**，于是用户过一会儿才在手表上打开应用，
    // 插件永远接不上，只能再点一次「连接设备」—— 这就是「经常连不上」最常见的形态。
    // 现在失败次数只影响提示与轮询间隔，探测链本身不断。
    if !busy {
        let attempt = update(|state| {
            state.probe_attempt = state.probe_attempt.saturating_add(1);
            state.probe_attempt
        });
        if attempt == 1 {
            set_status(StatusKind::Info, "正在等待《甜蜜女友2》回应…（可在手表上打开应用）");
        } else if attempt == MAX_PROBE_ATTEMPTS {
            let send_failures = update(|state| state.send_failures);
            set_status(
                StatusKind::Warn,
                if send_failures > 0 {
                    "消息发不到手环：请确认蓝牙连接正常，必要时在 AstroBox 里重新连接设备（插件会继续重试）".to_string()
                } else {
                    "还没收到《甜蜜女友2》的回应。请在手表上打开应用并停在它上面（插件会继续重试）".to_string()
                },
            );
            log_installed_apps(&addr).await;
        }
        render();
        // 心跳只发一小段 JSON，手环在接收链最前面就回 pong —— 比 `pack.list` 便宜得多，
        // 而且不会因为手环正在写盘而排在写盘队列后面。
        // 每 3 次里夹一次 `pack.list`：v3 之前的手环应用不认识 `ping`（未知消息静默丢弃），
        // 只发 ping 会让「插件已更新、手环 RPK 没更新」的用户永远连不上。
        // `pack.list` 是旧应用一定认得的消息；它一回来 `alive` 就置位，随后 hello 会给出
        // 「手环端版本过旧」的结论，而不是让用户对着「等待回应」猜。
        if attempt % 3 == 0 {
            request_pack_list(true).await;
        } else {
            request_ping().await;
        }
    }
    // 前 30 秒密集，之后放慢到 6 秒一次，但**永远不停**。
    let attempt = update(|state| state.probe_attempt);
    arm_probe(
        if attempt < MAX_PROBE_ATTEMPTS { PROBE_INTERVAL_MS } else { 6000 },
        session,
    );
}

/// 把手环上装的第三方快应用（包名 + 版本号）记进日志，并在装了本应用时回一句结论。
///
/// 用途只有一个：**给「应用打开了却一条回包都收不到」留证据**。连着探测 12 次都没回包时，
/// 必须能回答「手环上装的到底是不是本应用、是哪一版」—— 这个问题不能靠猜，
/// 也不能让「应用没装/装了旧版」和「装了但没在跑」在界面上长得一模一样。
async fn log_installed_apps(addr: &str) {
    match thirdparty_app_list(addr).await {
        Ok(apps) => {
            let summary: Vec<String> = apps
                .iter()
                .map(|app| format!("{}({})@{}", app.app_name, app.package_name, app.version_code))
                .collect();
            match apps.iter().find(|app| app.package_name == PACKAGE_NAME) {
                Some(app) => tracing::warn!(version_code = app.version_code, installed = %summary.join("、"), "band third-party app list"),
                None => tracing::warn!(installed = %summary.join("、"), "band third-party app list: our package is NOT installed"),
            }
        }
        Err(_) => tracing::warn!("band third-party app list unavailable"),
    }
}

/// 心跳 / 连接探测：`amakano.app.ping`。
///
/// 手环在 `pack-importer` 的接收链最前面就回 `pong`（不读盘、不看传输会话），
/// 所以它比 `pack.list`（全量扫描 + 注册表重写）便宜一个量级，也不会在
/// 传输进行中被写盘队列拖住。`probe = true`：超时静默，由探测循环判掉线。
async fn request_ping() {
    let id = format!("ping-{}", now_ms());
    let payload = json!({ "type": "amakano.app.ping", "requestId": id, "t": now_ms() as u64 }).to_string();
    dispatch_request(RequestKind::Ping, payload, id, true).await;
}

fn mark_alive() {
    let now = now_ms();
    update(|state| {
        if !state.alive {
            state.alive = true;
            state.probe_attempt = 0;
        }
        // 心跳靠这个时间戳判断「静默了多久」：手环回任何一条消息都算活着。
        state.last_alive_ms = now;
        state.heartbeat_misses = 0;
    });
    // 「手环应用回应过任何一条消息」本身就是一级里程碑：它证明应用在前台活着。
    set_stage(SessionStage::AppAlive);
}

/// 插件自己的持久化：**只放偏好与「上次在传哪一章」这类线索，绝不放进度权威。**
///
/// 进度权威在手环的 `pending-install` 里 —— 宿主在**更新/重装插件时会把整个插件目录删掉**
/// （宿主源码 `activate_staged_plugin` 就是 rename 到 backup 再 `remove_dir_all`，
/// 项目文档也记着「安装失败会把已安装插件的目录删掉」）。所以这里的东西随时可能丢，
/// 丢了只该影响「选项回默认值」，**绝不能让断点失效** —— 这也正是断点必须以手环的
/// 字节游标为准的原因（见 `resume_index_for_bytes`）。
const SETTINGS_FILE: &str = "state.json";

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Persisted {
    /// 设置文件自己的版本。v2（插件 2.4 / 同步协议 3）把默认分片改成 16 KB，
    /// 老用户的 state.json 里存着 8 KB，不迁移的话他们永远享受不到新协议的吞吐。
    #[serde(default, rename = "settingsVersion")]
    settings_version: u32,
    #[serde(default, rename = "chunkBytes")]
    chunk_bytes: usize,
    /// 传输模式（`"steady"` / `"turbo"`）。存的是**用户意图**：能力收紧在
    /// `effective_mode()` 里现算，不落盘 —— 否则换了台手环还会拿着旧结论。
    #[serde(default, rename = "transferMode")]
    transfer_mode: String,
    #[serde(default, rename = "autoLaunch")]
    auto_launch: Option<bool>,
    #[serde(default, rename = "queue")]
    queue: Vec<usize>,
    #[serde(default, rename = "autoResumeChapter")]
    auto_resume_chapter: Option<usize>,
    #[serde(default, rename = "autoResumePack")]
    auto_resume_pack: String,
    #[serde(default, rename = "autoResumeAttempts")]
    auto_resume_attempts: u8,
    /// 自动续传预算窗口的起点（墙钟毫秒）。**必须落盘**：窗口判定要跨插件重启成立，
    /// 否则每次重载都会白送一整窗预算（那就不是闸门了）。
    #[serde(default, rename = "autoResumeWindowStart")]
    auto_resume_window_start: u64,
}

/// 落盘一次。失败只记日志、**绝不打断任何流程** —— 存不下偏好不该让同步失败。
fn save_persisted() {
    let data = {
        let current = state().lock().unwrap_or_else(|error| error.into_inner());
        Persisted {
            settings_version: 3,
            chunk_bytes: current.chunk_bytes,
            transfer_mode: current.transfer_mode.wire().to_string(),
            auto_launch: Some(current.auto_launch),
            queue: current.sync_queue.clone(),
            auto_resume_chapter: current.auto_resume.as_ref().map(|item| item.0),
            auto_resume_pack: current.auto_resume.as_ref().map(|item| item.1.clone()).unwrap_or_default(),
            auto_resume_attempts: current.auto_resume.as_ref().map(|item| item.2).unwrap_or(0),
            auto_resume_window_start: current.auto_resume.as_ref().map(|item| item.3).unwrap_or(0),
        }
    };
    match serde_json::to_string(&data) {
        Ok(text) => {
            if let Err(error) = fs::write(SETTINGS_FILE, text) {
                tracing::warn!(%error, file = SETTINGS_FILE, "settings could not be saved");
            }
        }
        Err(error) => tracing::warn!(%error, "settings could not be serialized"),
    }
}

/// `on_load` 时读回来。同样**只影响预设值**：读不到 / 读坏了都退回默认，不报错挡路。
fn load_persisted() {
    let text = match fs::read_to_string(SETTINGS_FILE) {
        Ok(text) => text,
        // 第一次运行、或者插件目录刚被重装清掉 —— 都是正常的，不是错误。
        Err(_) => return,
    };
    let saved = match serde_json::from_str::<Persisted>(&text) {
        Ok(saved) => saved,
        Err(error) => {
            // 读坏了**必须看得见**：静默退回默认会让「设置老是自己变回去」查不出来。
            tracing::warn!(%error, file = SETTINGS_FILE, "saved settings unreadable; using defaults");
            return;
        }
    };
    update(|state| {
        // 分片档位只在它是合法档位时才采用（旧版本可能存过已经删掉的档位）。
        if CHUNK_OPTIONS.contains(&saved.chunk_bytes) {
            state.chunk_bytes = saved.chunk_bytes;
        }
        // 老用户（settingsVersion < 2）存的是 8 KB 那一档：升上来时给正常档的默认分片。
        if saved.settings_version < 2 && saved.chunk_bytes == 8192 {
            state.chunk_bytes = TransferMode::Steady.default_chunk();
        }
        // 模式：存的是用户意图，**这里不做能力收紧**（那一刻还没协商到手环能力）。
        // 收紧由 `effective_mode()` 现算，所以重连到另一台手环时会自动跟着变。
        if let Some(mode) = TransferMode::parse(&saved.transfer_mode) {
            state.transfer_mode = mode;
        }
        if let Some(flag) = saved.auto_launch {
            state.auto_launch = flag;
        }
        state.sync_queue = saved.queue.into_iter().filter(|number| *number >= 1).collect();
        if let Some(number) = saved.auto_resume_chapter {
            if !saved.auto_resume_pack.is_empty() {
                state.auto_resume = Some((number, saved.auto_resume_pack, saved.auto_resume_attempts.min(MAX_AUTO_RESUME), saved.auto_resume_window_start));
            }
        }
    });
    let (chunk_bytes, queue, auto_resume) = {
        let current = state().lock().unwrap_or_else(|error| error.into_inner());
        (current.chunk_bytes, current.sync_queue.len(), current.auto_resume.is_some())
    };
    tracing::info!(chunk_bytes, queue, auto_resume, "restored saved settings");
}

/// 挂一个带会话号的探测/心跳定时器。
fn arm_probe(delay_ms: u64, session: u64) {
    arm_timer(delay_ms, json!({ "type": "amakano.timer", "kind": "probe", "session": session }).to_string());
}

/// 按当前状态算一次 `effective_mode()`（给界面和发片用）。
fn current_mode() -> (TransferMode, String) {
    update(|state| {
        effective_mode(
            state.transfer_mode,
            state.sync_protocol,
            state.band_chunk_bytes,
            state.save_protocol.is_some(),
        )
    })
}

/// 切分片档位。**分片只在建传输时生效**：正在传的那一趟会作废（断点也随之作废）。
fn set_chunk_size(size: usize) -> bool {
    if !CHUNK_OPTIONS.contains(&size) {
        return false;
    }
    apply_chunk_size(size);
    true
}

fn apply_chunk_size(size: usize) {
    let mut message = String::new();
    let mut kind = StatusKind::Info;
    update(|state| {
        if state.chunk_bytes == size {
            return;
        }
        let busy = state.transfer.as_ref().map(|transfer| transfer.started).unwrap_or(false);
        // 分片不许超过手环的单片上限（激进档选了 32 KB、手环只收 16 KB 时要压回去）。
        let size = size.min(state.band_chunk_bytes).max(1024);
        state.chunk_bytes = size;
        // 建传输用**有效模式**（不是用户意图）：旧手环上的激进档不能拿去发片。
        let (mode, _) = effective_mode(
            state.transfer_mode,
            state.sync_protocol,
            state.band_chunk_bytes,
            state.save_protocol.is_some(),
        );
        let count = if state.pack_meta.is_some() {
            let chunks = build_chunks(&state.pack_files, size);
            let count = chunks.len();
            let limits = TransferLimits::new(mode, size);
            state.transfer = Some(transfer_with_chunks(limits, chunks, state.window_cap, state.sync_protocol));
            count
        } else {
            state.transfer = None;
            state.transfer_tick_armed = false;
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
    save_persisted();
}

/// 切传输模式。
///
/// 模式决定分片与确认节拍两件事，所以切过去要**顺手把分片放到这一档的默认值** ——
/// 用户点「激进」想要的是「最快」，让他再去手动改一次分片是多余的一步。
/// 正在传的那一趟会被取消（换时序等于换协议，不能热改），与切分片同一条处理。
fn set_transfer_mode(mode: TransferMode) {
    let (busy, hint) = update(|state| {
        if state.transfer_mode == mode {
            return (false, String::new());
        }
        state.transfer_mode = mode;
        let busy = state.transfer.as_ref().map(|transfer| transfer.started).unwrap_or(false);
        (busy, mode.hint().to_string())
    });
    if hint.is_empty() {
        return;
    }
    // 分片跟着模式走（用户之后仍可以单独调）。
    apply_chunk_size(mode.default_chunk());
    let (effective, blocked) = current_mode();
    let message = if effective != mode {
        format!("已选「{}」，但{}，这一轮仍按正常档跑", mode.label(), blocked)
    } else if busy {
        format!("已切换到「{}」：{}。当前同步已取消，请重新点同步", mode.label(), hint)
    } else {
        format!("已切换到「{}」：{}", mode.label(), hint)
    };
    set_status(if effective == mode { StatusKind::Info } else { StatusKind::Warn }, message);
    render();
    save_persisted();
}

async fn start_transfer() {
    let packet = {
        let mut current = state().lock().unwrap_or_else(|error| error.into_inner());
        if current.device_addr.is_empty() {
            None
        } else {
            let addr = current.device_addr.clone();
            let meta = current.pack_meta.clone();
            current.transfer_tick_armed = false;
            match (meta, current.transfer.as_mut()) {
                (Some(meta), Some(transfer)) => {
                    // 已经有一次传输在跑：**只有它真的还活着时才拒绝重来**。
                    // 「等确认」的窗口会过期、「握手完成」之后长时间静默也算死 —— 两种情况下都必须
                    // 允许重发，否则用户点什么都被这里挡回去、界面上速度与剩余永远是「—」
                    // （应用被挂起时插件挂的定时器会丢，这些状态就再也没人回收）。
                    if transfer_live(transfer.started, transfer.ready, transfer.ready_deadline_ms, transfer.last_progress_ms, now_ms()) {
                        None
                    } else {
                        transfer.request_id = format!("pack-{}", now_ms());
                        transfer.window_size = transfer.mode.params().initial_window.min(transfer.window_cap);
                        transfer.gap_streak = 0;
                        transfer.next_index = transfer.acked_index;
                        transfer.in_flight_bytes = 0;
                        // **不要在这里清 `session_losses`**：它是「手环连续丢会话」的预算，
                        // 每次重开都归零的话，8 次闸门永远不成立（评审发现的实际 bug）。
                        transfer.last_ack_ms = now_ms();
                        transfer.last_send_ms = now_ms();
                        transfer.acked_bytes = prefix_bytes(&transfer.chunks, transfer.acked_index);
                        transfer.speed_kbps = 0.0;
                        transfer.speed_window_start = now_ms();
                        transfer.speed_window_bytes = 0;
                        transfer.rtt_ms = 0;
                        transfer.retry_count = 0;
                        transfer.ready = false;
                        transfer.started = true;
                        transfer.waiting_ready = true;
                        transfer.ready_attempts = 1;
                        // 等待窗口从这一刻重新起算（`READY_TIMEOUT_MS` 的重发次数 + 余量）。
                        transfer.ready_deadline_ms = now_ms()
                            + READY_TIMEOUT_MS as u128 * (MAX_READY_ATTEMPTS as u128 + 1)
                            + READY_WAIT_SLACK_MS;
                        transfer.last_progress_ms = now_ms();
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
            let live = state.transfer.as_ref().is_some_and(|transfer| {
                transfer_live(transfer.started, transfer.ready, transfer.ready_deadline_ms, transfer.last_progress_ms, now_ms())
            });
            if live {
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
    if send_message(&addr, &payload).await.is_ok() {
        set_stage(SessionStage::Transferring);
        clear_error();
        set_status(StatusKind::Info, "已发送导入请求，等待手环确认…");
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

fn apply_ready(transfer: &mut Transfer, reported_from: usize, resume_bytes: usize, resume_path: &str, resumed: bool) {
    // **以字节游标为准。** 手环回的 `resumeFrom` 是上一次那套布局里的下标，换过分片档位
    // 之后不能用；`resumeBytes`（已写完文件的字节数之和）跟分片怎么切无关，
    // 按它在**当前**布局里重新定位，换档最多重传一个文件。
    //
    // 游标为 0 但手环给了半截文件名（断点落在整包**第一个文件**中间 —— 这时
    // 「已写完文件的字节数」确实是 0）时按**文件路径**定位：路径与分片怎么切无关，
    // 比手环报的分片下标可靠。两条路都走不通才退回手环报的下标。
    let resume_from = if resume_bytes > 0 {
        let layout: Vec<(usize, bool)> = transfer.chunks.iter().map(|chunk| (chunk.bytes, !chunk.append)).collect();
        resume_index_for_bytes(&layout, resume_bytes)
    } else if !resume_path.is_empty() {
        let layout: Vec<(String, bool)> = transfer.chunks.iter().map(|chunk| (chunk.path.clone(), !chunk.append)).collect();
        resume_index_for_path(&layout, resume_path).unwrap_or_else(|| reported_from.min(transfer.chunks.len()))
    } else {
        reported_from.min(transfer.chunks.len())
    };
    let resume_from = resume_from.min(transfer.chunks.len());
    transfer.next_index = resume_from;
    transfer.acked_index = resume_from;
    transfer.in_flight.clear();
    transfer.in_flight_bytes = 0;
    transfer.window_size = transfer.mode.params().initial_window.min(transfer.window_cap);
    transfer.retry_count = 0;
    // 新会话的 gap 连续计数从头算；会话预算 `session_losses` 保留。
    transfer.gap_streak = 0;
    // **不要清 `session_losses`**：它是「连续丢接收会话」的预算。
    // `ready` 是每次重开 begin 都会收到的，在这里清零等于预算永远为 0。
    transfer.stable_acks = 0;
    transfer.rtt_ms = 0;
    transfer.speed_window_start = now_ms();
    transfer.speed_window_bytes = 0;
    transfer.speed_kbps = 0.0;
    transfer.acked_bytes = prefix_bytes(&transfer.chunks, resume_from);
    transfer.ready = true;
    transfer.started = true;
    transfer.waiting_ready = false;
    // 确认已到，等待窗口作废（它只管「还没等到确认」那一段）。
    transfer.ready_deadline_ms = 0;
    transfer.last_progress_ms = now_ms();
    transfer.last_ack_ms = now_ms();
    transfer.last_send_ms = now_ms();
    transfer.resumed = resumed || resume_from > 0;
}

async fn handle_interconnect(payload: &str) -> bool {
    let Ok(message) = serde_json::from_str::<Value>(&parse_event_payload(payload)) else { return false };
    // 手环应用回了任何一条消息，说明它正在前台运行。
    mark_alive();
    let request_id = message.get("requestId").and_then(Value::as_str).unwrap_or("").to_string();
    // 传输对象已经被 12 秒兜底清掉了，但手环的完成回执迟到了：只要它属于正在核对的包，
    // 就是比 pack.list 更强的证据，直接判成功（不能进下面的 `is_current` 闸门）。
    if message.get("type").and_then(Value::as_str) == Some("amakano.files.complete") {
        let pack_id = message.get("packId").and_then(Value::as_str).unwrap_or("").to_string();
        let (verified, waiting) = update(|state| {
            let matches = state
                .verify_pack
                .as_deref()
                .is_some_and(|pending| pending == pack_id);
            let waiting = state.verify_request.clone();
            if matches {
                state.verify_pack = None;
                state.verify_request = None;
                state.verify_attempts = 0;
            }
            (matches, waiting)
        });
        if verified {
            if let Some(vid) = waiting.as_deref() {
                clear_request(vid);
            }
        }
        if verified {
            set_status(StatusKind::Good, "本章导入完成（已收到手环完成回执）");
            render();
            arm_timer(300, json!({ "type": "amakano.timer", "kind": "next-queued" }).to_string());
            return false;
        }
    }
    match message.get("type").and_then(Value::as_str) {
        Some("amakano.app.pong") => {
            clear_request(&request_id);
            // 应用醒了：隔一拍去问能力表（不在回包栈里直接发下一个请求）。
            let need = update(|state| {
                state.save_protocol.is_none() && !state.request.busy()
            });
            if need {
                arm_timer(FOLLOWUP_DELAY_MS, json!({ "type": "amakano.timer", "kind": "hello" }).to_string());
            }
            return false;
        }
        Some("amakano.pack.pending") => {
            clear_request(&request_id);
            let pending = message.get("pending").filter(|value| !value.is_null()).and_then(|value| {
                Some(ResumeInfo {
                    pack_id: value.get("packId").and_then(Value::as_str)?.to_string(),
                    chapter_name: value.get("chapterName").and_then(Value::as_str).unwrap_or("未命名章节包").to_string(),
                    bytes: value.get("bytes").and_then(Value::as_u64).unwrap_or(0) as usize,
                    // 手环报的分片数是**它那套布局**下的数量，只用来打诊断日志 ——
                    // 它绝不能参与「能不能续传」的判定（换过档位就对不上了，见 `resume_index_for_bytes`）。
                    chunks: value.get("chunks").and_then(Value::as_u64).unwrap_or(0) as usize,
                    resume_from: value.get("resumeFrom").and_then(Value::as_u64).unwrap_or(0) as usize,
                    received_bytes: value.get("receivedBytes").and_then(Value::as_u64).unwrap_or(0) as usize,
                    files_done: value.get("filesDone").and_then(Value::as_u64).unwrap_or(0) as usize,
                })
            });
            update(|state| {
                state.resume = pending;
                // 断点问题的回答到手了：查询盲区到此结束（无论手环上有没有断点）。
                state.pending_answered = true;
                state.status = match state.resume.as_ref() {
                    Some(pending) => {
                        let percent = if pending.bytes > 0 { pending.received_bytes * 100 / pending.bytes } else { 0 };
                        format!("发现未完成传输：{} · 已传 {}%，点「继续传输」接着传", pending.chapter_name, percent)
                    }
                    None => "手环上没有未完成的传输".into(),
                };
                state.status_kind = if state.resume.is_some() { StatusKind::Warn } else { StatusKind::Info };
            });
            if let Some(item) = state().lock().unwrap_or_else(|error| error.into_inner()).resume.clone() {
                tracing::info!(
                    pack = %item.pack_id,
                    bytes = item.bytes,
                    chunks = item.chunks,
                    resume_bytes = item.received_bytes,
                    files_done = item.files_done,
                    "band reported an unfinished transfer (cursor is authoritative, chunks is diagnostic only)"
                );
            }
            render();
            // 自动恢复：断线/被重启前正在传的那一章，只要手环的断点还在，就接着传下去。
            // 断点不在了（传完了、或被清缓存清掉）就把标记丢掉，别留着反复试。
            let resume_now = update(|state| {
                let pending_id = state.resume.as_ref().map(|item| item.pack_id.clone());
                match (&state.auto_resume, pending_id) {
                    (Some((number, id, _, _)), Some(pending)) if *id == pending => {
                        let number = *number;
                        state.auto_resume = None;
                        Some(number)
                    }
                    _ => {
                        state.auto_resume = None;
                        None
                    }
                }
            });
            save_persisted();
            if let Some(number) = resume_now {
                tracing::info!(number, "auto-resuming the interrupted chapter");
                start_embedded_transfer(number).await;
                return false;
            }
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
            // 注册表里有、但 `pack.txt` 读不出来的条目。手环侧 `scan` 现在只读、**不剔除**
            // 它们（旧实现会剔，一次瞬时读失败就永久丢一章），所以必须在这里说出来。
            let broken: Vec<String> = message
                .get("broken")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            if !broken.is_empty() {
                tracing::warn!(?broken, "band registry has unreadable entries");
            }
            let installed_ids: Vec<String> = installed.iter().map(|pack| pack.id.clone()).collect();
            let ask_pending = {
                let mut current = state().lock().unwrap_or_else(|error| error.into_inner());
                let ask = !current.pending_checked;
                current.pending_checked = true;
                current.installed = installed;
                current.installed_broken = broken;
                current.catalog_answered = true;
                // **这一行才是「章节列表拿到了」那个独立里程碑。**
                // 旧实现把它当成「握手顺带的事」，于是握手一成功就再也没人去拉列表。
                current.stage = current.stage.max(SessionStage::CatalogReady);
                current.cache_bytes = cache_bytes;
                current.cache_files = cache_files;
                if current.resume.is_none() && cache_bytes == 0 {
                    current.status = format!("已连接《甜蜜女友2》，手环已安装 {} 个章节包", current.installed.len());
                    current.status_kind = StatusKind::Good;
                }
                ask
            };
            // 完成回执丢失后的核对：任意 pack.list 回包命中 packId 都算权威证据
            // （用户刷新与核对同时发生时，刷新回包也能直接结案）；「未命中」只认
            // 核对请求自己的回包，避免用户刷新烧掉核对预算。
            let (verify, verify_request) = update(|state| {
                (state.verify_pack.clone(), state.verify_request.clone())
            });
            if let Some(id) = verify {
                if pack_verified(&installed_ids, &id) {
                    // 结案时把**在途的核对请求与坑位一起收掉**：否则它后续超时
                    // 还会走一次 fail_request，把成功提示覆盖成「核对失败」。
                    if let Some(vid) = verify_request.as_deref() {
                        if vid != request_id {
                            clear_request(vid);
                        }
                    }
                    update(|state| {
                        state.verify_pack = None;
                        state.verify_request = None;
                        state.verify_attempts = 0;
                    });
                    set_status(StatusKind::Good, "本章导入完成（已核对手环登记）");
                    render();
                    arm_timer(300, json!({ "type": "amakano.timer", "kind": "next-queued" }).to_string());
                } else {
                    // **只有核对请求自己的回包**才用于「未命中」计数：
                    // 用户刷新/旧版保活 probe 的回包不算，否则会白白烧掉核对预算。
                    let own = update(|state| {
                        state.verify_request.as_deref() == Some(request_id.as_str())
                    });
                    // **只包住计数**：外部回包（用户刷新 / 旧版保活）未命中时什么都不做，
                    // 但控制流必须继续落到函数尾部的 render / ask_pending / settle_refresh_step，
                    // 否则会把「刷新」的下一步静默吞掉。
                    if own {
                        // 可能是提交还没落盘：再等一轮 pack.list，最多 3 次。
                        let attempts = update(|state| {
                            state.verify_request = None;
                            state.verify_attempts = state.verify_attempts.saturating_add(1);
                            state.verify_attempts
                        });
                        if attempts < 3 {
                            set_status(StatusKind::Warn, "手环还没有登记这一章，正在重试核对…");
                            render();
                            arm_timer(1500, json!({ "type": "amakano.timer", "kind": "verify-complete" }).to_string());
                        } else {
                            let waiting = update(|state| {
                                let waiting = state.verify_request.take();
                                state.verify_pack = None;
                                waiting
                            });
                            if let Some(vid) = waiting.as_deref() {
                                clear_request(vid);
                            }
                            set_status(StatusKind::Bad, "手环没有登记这一章，请重新同步");
                            render();
                        }
                    }
                }
            }
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
            let save_revision_supported = features.iter().any(|feature| feature == "save-revision");
            // 阅读统计是独立的一条能力：老版本手环只报 ['saves','packs']。
            let supports_stats = features.iter().any(|feature| feature == "stats");
            // 接收窗口：手环报的是它**拒收**的边界（`index >= next + receiveWindow` 直接报错），
            // 没报这个字段（2.2 之前的应用）就按 3 片保守跑。见 `negotiated_window`。
            let receive_window = message.get("receiveWindow").and_then(Value::as_u64);
            let sync_protocol = if features.iter().any(|feature| feature == "sync3") {
                3
            } else {
                message.get("syncProtocol").and_then(Value::as_u64).unwrap_or(2) as u32
            };
            // 单片上限：新手环报 `maxChunkBytes`（它能收的最大一片）；老手环只有
            // `chunkBytes`（默认档位）。**两个都要认** —— 激进档的判据是上限够不够 32 KB，
            // 而正常档的默认值来自 `chunkBytes`。
            let band_max_chunk = message
                .get("maxChunkBytes")
                .and_then(Value::as_u64)
                .or_else(|| message.get("chunkBytes").and_then(Value::as_u64))
                .unwrap_or(DEFAULT_CHUNK_BYTES as u64)
                .clamp(1024, 64 * 1024) as usize;
            let band_chunk_bytes = message
                .get("chunkBytes")
                .and_then(Value::as_u64)
                .unwrap_or(DEFAULT_CHUNK_BYTES as u64)
                .clamp(1024, 64 * 1024) as usize;
            // 组批：手环报的一条消息最多装几片 / 多少负载字节（`amakano.files.batch`）。
            // **没报就不组批** —— 老手环不认识这条消息。见 `plan_batch`。
            let batch_chunks = message.get("batchChunks").and_then(Value::as_u64).unwrap_or(0) as usize;
            let batch_bytes = message.get("batchBytes").and_then(Value::as_u64).unwrap_or(0) as usize;
            // 模式：用户意图先落进 state，再按**刚到手的能力**算一遍有效值；
            // 窗口必须按有效模式协商 —— 否则激进档会拿着正常档的 10 片上限跑。
            let (requested_mode, _) = update(|state| {
                state.band_chunk_bytes = band_max_chunk;
                (state.transfer_mode, ())
            });
            let (effective, blocked) =
                effective_mode(requested_mode, sync_protocol, band_max_chunk, true);
            let window_cap = negotiated_window(effective, receive_window, sync_protocol);
            tracing::info!(
                protocol, version = %version, version_code, ?features, receive_window, window_cap,
                sync_protocol, band_chunk_bytes, band_max_chunk, batch_chunks, batch_bytes,
                mode = effective.wire(), blocked = %blocked,
                "band capability reported"
            );
            set_stage(SessionStage::Handshaked);
            // 以连接会话为边界：能力值可能沿用上一会话，不能用它判断是否首次。
            let first_hello = update(|state| mark_first_hello_for_session(state.session, &mut state.hello_ok_session));
            update(|state| {
                state.band_version = version.clone();
                // 每一次 hello-ok 都重算：换了台手环（或手环上换了旧版应用）时，
                // 窗口与组批能力必须跟着回退，不能沿用上一个会话协商到的值。
                state.window_cap = window_cap;
                state.sync_protocol = sync_protocol;
                state.supports_ping = sync_protocol >= 3;
                // `band_chunk_bytes` 在算有效模式之前就写好了（见上面），这里只记拒绝原因。
                state.turbo_blocked = blocked;
                // 用户档位高于手环上限时按手环的来；新传输才生效，进行中的保持原布局。
                state.chunk_bytes = state.chunk_bytes.min(band_max_chunk).max(1024);
                state.batch_chunks = if batch_chunks > 1 { batch_chunks } else { 0 };
                state.batch_bytes = batch_bytes;
                // 代际判定：1.x 旧代就置位，并清掉「排队中的章节」与「断线自动续传」的意图 ——
                // 旧版游戏读不了 2.0 的章节包，排队的章节不该再传
                // （传输入口 `start_embedded_transfer` 里还有第二道闸，这里是意图层）。
                state.band_game_legacy = hello_reports_legacy(&version, supports_saves);
                if state.band_game_legacy {
                    state.sync_queue.clear();
                    state.auto_resume = None;
                }
                state.save_protocol = (protocol >= SAVE_PROTOCOL && supports_saves).then_some(protocol);
                state.stats_supported = supports_stats;
                state.save_revision_supported = save_revision_supported;
                // 不支持就把上一次的统计清掉：留着旧数据会让「版本过旧」那张卡和一堆
                // 看起来正常的数字同时出现，用户不知道该信哪个。
                if !supports_stats {
                    state.stats = None;
                    state.stats_busy = false;
                }
                if state.save_protocol.is_some() {
                    state.saves_error.clear();
                    state.saves_unsupported = false;
                    // 协商成功 = 之前那张「手环端应用版本过旧 / 两侧版本或协议对不上」被证伪，
                    // 必须把红卡撤掉：应用启动慢半拍时第一轮 hello 会超时，之后协商成功，
                    // 卡不撤用户就会在握手已经正常的情况下一直看着「版本或协议不匹配」。
                    if state.error.as_ref().map(|error| error.code) == Some(ErrorCode::Protocol) {
                        state.error = None;
                    }
                    // 1.x 旧代但存档协议是通的（1.6.x 就有存档通道）：协议协商成功的话照说，
                    // 但「游戏是旧版、要卸载重装」这件事必须占住状态行 —— 2.0 起存档结构
                    // 和章节包都换了，这条比「支持存档管理」更要紧。
                    if state.band_game_legacy {
                        state.status = format!("手环上的游戏是 1.x 旧版（v{version}）：存档结构与章节包已换代，请卸载游戏并重装 2.0 版");
                        state.status_kind = StatusKind::Warn;
                    } else {
                        state.status = format!("手环端支持存档管理（应用 v{version} · 协议 {protocol}）");
                        state.status_kind = StatusKind::Good;
                    }
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
            // 是不是本次会话的**首条** hello：心跳也走同一条 `hello`，如果每次回包都补拉列表，
            // 10 秒一次的探测就变成「10 秒一次 storage 读 + 10 秒一次注册表重写」，
            // 白搅动手环的内存与闪存。
            if update(|state| state.band_game_legacy) {
                // 排队意图被清掉了，落盘的那份也要跟上（`on_load` 恢复时才不会复活旧队列）。
                save_persisted();
            }
            render();
            // **握手成功 = 手环应用已经活过来了 → 必须补一次章节列表。**
            //
            // 这一条是「连接手环后拿不到已安装章节列表」的直接修复。旧实现里 `pack.list`
            // 只在 `probe_tick` 的 `!alive` 分支里发（见 `probe_tick`），而 `hello-ok` 一到
            // `mark_alive()` 就把 `alive` 置真、`save_protocol` 也协商好了，于是下一次探测
            // 走进 `if alive` 直接 `return` —— **`pack.list` 再也发不出去**。
            // 更麻烦的是 `amakano.pack.pending`（断点查询）是挂在 `pack.list` 回包分支里的，
            // 于是「未完成传输」也一并看不见了。
            //
            // 真机复现（2026-09-24 17:10 与 18:12 两次连接）：`hello-ok` 都在 2.04 s 返回，
            // 两次全程没有任何 `pack.list` 出站，插件日志里 `installed=0`。
            // 注意它**不是竞态**：`hello-ok` 早到或晚到，两条路都发不出 `pack.list`。
            //
            // 同样不在回调里直接发（实测紧跟回包 <10 ms 的那一档丢了 61.5%），隔一拍再发。
            if first_hello {
                // 首次握手之后要补两件事：**已安装章节列表** + 手环存档。
                //
                // ⚠️ **必须串行，而且必须走既有的 `refresh_queue`。**
                // 这里原来是各自 arm 一个同样 200ms 的定时器 —— 两个定时器同时到点抢**同一个**
                // 请求坑位（`State::request` 只有一个坑），`saves-list` 先到就把坑占了，
                // `list` 到点发现 `busy` 就**静默丢掉**，章节列表永远拿不到。
                //
                // 真机实证（2026-09-24 20:10）：宿主日志里 `packs-` 命中 **0 次**，
                // 20:10:05 收到 hello-ok、20:10:07 收到存档列表，然后界面就停在
                // 「能力协商完成 · 正在读取手环上已安装的章节…」不动，直到用户放弃。
                //
                // `refresh_queue` 就是为「一次只挂一个请求」写的现成机制（章节列表在前、
                // 存档在后，前一个 settle 了再隔一拍发下一个），别再造一份并行定时器。
                run_refresh(RefreshPlan {
                    pack_list: true,
                    save_list: supports_saves && protocol >= SAVE_PROTOCOL,
                    stats_list: false,
                })
                .await;
            }
            // 1.x 旧代时状态行留住那句「卸载重装」，别被「正在读取手环存档…」盖掉 ——
            // 存档列表照拉（读取无害，卡片还在推送页挂着），但最要紧的话必须是这句。
            let legacy = update(|state| state.band_game_legacy);
            if first_hello && supports_saves && protocol >= SAVE_PROTOCOL && !legacy {
                // 协商成功就顺手拉一次存档列表，用户切到「存档」页时数据已经在了。
                //
                // ⚠️ **不许在这里直接发**：实测「收到一条回包之后几毫秒内就发出下一个请求」
                // 的那一档，第一次回包丢了 61.5%（13 个里 8 个），而这条 `hello-ok → saves.list`
                // 的链更是 10 个样本里丢了 8 个 —— 用户看到的就是「连上设备后第一次打开存档页
                // 要转半天甚至报超时，再点一次刷新就好了」。隔一拍（用现成的 timer）再发，
                // 200ms 以上发出的一档几乎不丢。理由与数字见 `src/request.rs` 的 `FOLLOWUP_DELAY_MS`。
                set_status(StatusKind::Info, "正在读取手环存档…");
                render();
            }
            // 用户正停在「统计」页、而刚连上时：顺手把统计也读一次，
            // 否则他得再点一下「读取统计」（同样是隔一拍再发，不许在回调里立刻发）。
            if first_hello && supports_stats && state().lock().unwrap_or_else(|error| error.into_inner()).page == Page::Stats {
                arm_timer(FOLLOWUP_DELAY_MS, json!({ "type": "amakano.timer", "kind": "stats-list" }).to_string());
            }
            return false;
        }
        Some("amakano.saves.data") => {
            let session = update(|state| state.request.get().and_then(|request| {
                (request.id == request_id && request.session == state.session).then_some(request.session)
            }));
            let Some(session) = session else { return false };
            // 分片未收齐前保留请求身份与超时重试；收齐后再统一销账。
            // 回包是**分片信封**：先把各片按 seq 拼起来（单片走同一条路），
            // 再从**里层载荷**读字段。旧实现直接读外层，于是永远 0 条 —— 见
            // `amakano2_ui::saves` 的模块注释与 `docs/章节包导入.md` 的「回包信封」一节。
            let shards = match push_save_shard(&message) {
                ShardOutcome::Complete(shards) => shards,
                // 还没收齐：不报错、也不动界面，安静等着。
                ShardOutcome::Pending => return false,
                // 迟到的片属于别的请求：同样什么都不做（它会自己去超时兜底）。
                ShardOutcome::Foreign => return false,
                ShardOutcome::Broken(reason) => {
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
            clear_request(&request_id);
            let band = crate::cloud::BandSnapshot { revision: saves.revision, auto_save: auto_save.clone(), slots: slots.clone() };
            let assignment = update(|state| {
                if state.session != session { return Err("连接已变化，忽略旧存档回包".into()); }
                crate::cloud::assign_ids(&mut state.cloud_persisted.local, &band, now_ms(), &mut crate::cloud::RandomIds)
            });
            if let Err(error) = assignment {
                update(|state| {
                    state.saves_error = error.clone();
                    state.saves_busy = false;
                });
                render();
                return false;
            }
            save_cloud_state();
            let cloud_decision = update(|state| -> (Option<cloud::CloudRun>, bool, bool) {
                let run = cloud::CloudRun {
                    generation: state.cloud_generation,
                    session,
                    account_id: state.cloud_persisted.account_id.clone(),
                    account_name: state.cloud_account_name.clone(),
                };
                let eligible = state.session == session && state.cloud_bound && state.save_revision_supported && state.cloud_synced_session != Some(session)
                    && !state.transfer.as_ref().is_some_and(|transfer| transfer.started);
                if state.cloud_pending.is_some() && state.cloud_reread.is_some() {
                    (state.cloud_reread.take(), false, false)
                } else if let Some(run) = state.cloud_reread.clone().filter(|run| run.session == session) {
                    state.cloud_reread = None;
                    (Some(run), false, false)
                } else if eligible {
                    state.cloud_synced_session = Some(session);
                    state.cloud_generation = state.cloud_generation.wrapping_add(1);
                    state.cloud_conflict_retries = 0;
                    state.cloud_running = true;
                    state.cloud_phase = cloud::CloudPhase::Syncing;
                    state.cloud_detail = "本机存档已读取，正在与云端合并…".into();
                    (Some(cloud::CloudRun { generation: state.cloud_generation, ..run }), false, false)
                } else if state.cloud_pending.is_some() && state.cloud_persisted.local.observed == band {
                    state.cloud_pending = None;
                    (None, true, false)
                } else {
                    (None, false, state.cloud_pending.take().is_some())
                }
            });
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
                state.save_list_revision = band.revision;
                state.saves_error.clear();
                state.saves_busy = false;
                state.status = format!("手环上有 {} 条手动存档", state.save_slots.len());
                state.status_kind = StatusKind::Info;
            });
            render();
            // 这一跨越位了（存档是「刷新」的最后一步，正常情况下队列已经空了）。
            settle_refresh_step();
            if let Some(run) = cloud_decision.0 { cloud::band_ready(run, band).await; }
            if cloud_decision.1 { cloud::applied(session); }
            if cloud_decision.2 { cloud::apply_failed(session, "写回后的本机存档与云端合并结果不一致，请重新同步"); }
            return false;
        }
        // ---- 阅读统计 ----
        Some("amakano.stats.data") => {
            // **顺序很重要**：必须先确认这条回包属于当前坑位上的请求，再动分片缓存。
            // `push_shard_into` 靠「坑位里那条请求」来判定 session；旧代码在这里先
            // `clear_request` 把坑位清空，于是 `push_shard_into` 拿到的 session 是 `None`，
            // 一律判成「这是旧请求或旧连接的回包，已忽略」—— 分片回包必然失败。
            // 真机 2026-10-31 16:19:33 的日志正是这条：2655 字节的统计回包被判旧请求。
            // 存档通道一直是「先判 session 再清」，两个通道必须同一写法。
            let session = update(|state| state.request.get().and_then(|request| {
                (request.id == request_id && request.session == state.session).then_some(request.session)
            }));
            // 不是当前请求的回包：可能是上一轮重发留下的迟到片，也可能属于别的会话。
            // 不报错、不动界面（和存档通道一致），避免一条迟到片把正在等的请求判死。
            let Some(session) = session else {
                tracing::debug!(request_id = %request_id, "late or foreign stats shard ignored");
                return false;
            };
            // 与存档回包同一套分片信封：按 seq 拼起来，再从**里层载荷**读字段。
            let shards = match push_stats_shard(&message) {
                ShardOutcome::Complete(shards) => shards,
                // 还没收齐：安静等着，不动界面。**坑位保留**，超时重发才有兜底。
                ShardOutcome::Pending => return false,
                // 属于别的请求的片：丢掉就好，别拿它去判当前这次读取失败。
                ShardOutcome::Foreign => return false,
                ShardOutcome::Broken(reason) => {
                    tracing::warn!(request_id = %request_id, reason = %reason, "band stats reply rejected");
                    clear_request(&request_id);
                    fail_stats(&format!("读取阅读统计失败：{reason}"));
                    return false;
                }
            };
            let payload = match shard_payload(&shards) {
                Ok(value) => value,
                Err(reason) => {
                    tracing::warn!(request_id = %request_id, shards = shards.len(), reason = %reason, "band stats reply unreadable");
                    clear_request(&request_id);
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
                    clear_request(&request_id);
                    fail_stats(&format!("读取阅读统计失败：{reason}"));
                    return false;
                }
            };
            tracing::info!(days = stats.total_day_count, recent = stats.recent.len(), "band stats received");
            // **收齐之后才销账**（与存档通道一致）：中途清坑位会让重发失去兜底。
            clear_request(&request_id);
            update(|state| {
                if state.session != session { return; }
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
        Some("amakano.saves.capture-ok") => {
            let session = update(|state| state.request.get().and_then(|request| (request.id == request_id && request.session == state.session).then_some(request.session)));
            let Some(session) = session else { return false };
            clear_request(&request_id);
            let save = message.get("save").filter(|value| value.is_object()).cloned();
            let Some(save) = save else {
                update(|state| {
                    if state.session != session { return; }
                    state.saves_busy = false;
                    state.pending_current_save = None;
                    state.pending_save_slot = None;
                    state.saves_error = "手环没有返回可保存的当前进度：请先进入游戏并读到一段剧情".into();
                    state.status = state.saves_error.clone();
                    state.status_kind = StatusKind::Bad;
                });
                render();
                return false;
            };
            let chapter = save.get("chapterTitle").and_then(Value::as_str).unwrap_or("当前章节");
            tracing::info!(chapter, "band delivered current save snapshot");
            update(|state| {
                if state.session != session { return; }
                state.saves_busy = false;
                // capture 读到的就是手环最新的 autoSave。必须同步刷新本地快照，
                // 否则紧接着的整表写回会拿旧 autoSave 回滚“继续阅读”。
                state.save_auto = Some(save.clone());
                state.pending_current_save = Some(save);
                state.saves_notice = "当前进度已读取：点已有槽覆盖，或点末尾空槽新建。".into();
                state.status = "当前进度已读取，可以选择要保存的槽位".into();
                state.status_kind = StatusKind::Good;
            });
            render();
            return false;
        }
        Some("amakano.saves.put-ok") => {
            let session = update(|state| state.request.get().and_then(|request| (request.id == request_id && request.session == state.session).then_some(request.session)));
            let Some(session) = session else { return false };
            clear_request(&request_id);
            let cloud_put = update(|state| state.save_put_session.take() == Some(session));
            if cloud_put {
                update(|state| state.saves_busy = false);
                request_band_operation(cloud::BandOperation::List { session }).await;
                return false;
            }
            let count = message.get("count").and_then(Value::as_u64).unwrap_or(0);
            let total = message.get("slots").and_then(Value::as_u64).unwrap_or(0);
            // 根据这次是“保存当前进度”还是“导入”给出不同反馈。
            // 当前进度保存成功后，提示必须点明写入了哪个槽，并清掉快照。
            let (notice, saved_slot) = {
                let current = state().lock().unwrap_or_else(|error| error.into_inner());
                let import = current.save_import.clone();
                let slot = current.pending_save_slot;
                let chapter = current.pending_current_save.as_ref()
                    .and_then(|save| save.get("chapterTitle"))
                    .and_then(Value::as_str)
                    .unwrap_or("当前章节")
                    .to_string();
                drop(current);
                if let Some((incoming, duplicates, existing, _)) = import {
                    update(|state| { state.save_import = None; });
                    (SaveImportView { incoming, duplicates, existing }.notice(), None)
                } else if slot.is_some() {
                    (format!("已保存到存档 {}（{chapter}）", slot.unwrap_or(0) + 1), slot)
                } else {
                    (format!("已写入 {count} 条存档，手环上现在有 {total} 条"), None)
                }
            };
            tracing::info!(count, total, saved_slot, notice = %notice, "band accepted save write");
            update(|state| {
                state.saves_busy = false;
                state.saves_notice = notice.clone();
                state.status = notice;
                state.status_kind = StatusKind::Good;
                // 当前进度写进手环后，下一次保存前必须重新读取，避免用户误以为
                // 点槽保存的是之后又推进过的剧情。
                if saved_slot.is_some() {
                    state.pending_current_save = None;
                }
                state.pending_save_slot = None;
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
            let session = update(|state| state.request.get().and_then(|request| (request.id == request_id && request.session == state.session).then_some(request.session)));
            let Some(session) = session else { return false };
            clear_request(&request_id);
            if update(|state| state.save_put_session.take() == Some(session)) {
                update(|state| state.saves_busy = false);
                cloud::apply_failed(session, "手环拒绝了写回，请重试");
            }
            let read_failed = update(|state| {
                if state.save_list_session.take() != Some(session) { return false; }
                state.saves_busy = false;
                if state.cloud_pending.is_some() { true } else {
                    state.cloud_phase = cloud::CloudPhase::Failed;
                    state.cloud_detail = "无法重新读取手环存档，请重试".into();
                    state.cloud_running = false;
                    false
                }
            });
            if read_failed { cloud::apply_failed(session, "写回后无法重新读取手环存档"); }
            let error = message.get("error").and_then(Value::as_str).unwrap_or("unknown").to_string();
            let hint = match error.as_str() {
                "slot-missing" => "手环上这个槽位已经不存在了，点「刷新」看看现在的列表",
                "save-revision-conflict" => "手环上已有更新的存档变化，请先点「刷新」再保存，避免覆盖",
                "empty" => "没有可写入的存档",
                "replace-needs-confirm" => "整表替换没有被确认",
                "write-failed" => "手环写入存储失败（空间不足或被系统拒绝）",
                _ => "手环端处理存档请求失败",
            };
            tracing::warn!(%error, "band rejected a save request");
            let operation = update(|state| {
                let operation = if let Some(slot) = state.pending_save_slot.take() {
                    format!("保存存档 {} 失败", slot + 1)
                } else if let Some(key) = state.confirm_delete_save.take() {
                    let slot = key.parse::<usize>().map_or(0, |slot| slot + 1);
                    format!("删除存档 {slot} 失败")
                } else {
                    "存档操作失败".to_string()
                };
                state.saves_busy = false;
                operation
            });
            update(|state| {
                state.saves_notice = format!("{operation}：{hint}");
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
            // 权威游标：已写完文件的字节数之和。手环没给（旧版）就退回按分片下标续，
            // 也就是以前的行为。
            let resume_bytes = message.get("resumeBytes").and_then(Value::as_u64).unwrap_or(0) as usize;
            // 半截文件的路径。游标为 0 时它是唯一的锚点（见 `apply_ready`）。
            let resume_path = message.get("resumePath").and_then(Value::as_str).unwrap_or("").to_string();
            let resumed = message.get("resumed").and_then(Value::as_bool).unwrap_or(false) || resume_bytes > 0 || !resume_path.is_empty();
            let mut should_send = false;
            let mut resumed_now = false;
            let mut next_index = 0;
            update(|state| {
                if let Some(item) = state.transfer.as_mut() {
                    apply_ready(item, resume_from, resume_bytes, &resume_path, resumed);
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
                        format!("手环已就绪，从第 {} 个分片继续传输", next_index + 1)
                    } else {
                        "手环已就绪，正在传输".into()
                    };
                    state.status_kind = StatusKind::Info;
                });
            }
            tracing::info!(resume_from, resume_bytes, resumed, "received pack ready response");
            render();
            send_next = should_send;
        }
        Some("amakano.files.chunk-ok") => {
            // **累计确认**：`upToIndex` 是手环已经写盘成功的最后一篇。
            // 一次确认可以把窗口里的好几片一起销掉，插件不必为每一片都等一条独立回包。
            //
            // 旧手环只给 `index`（没有 `upToIndex`），按「这一片已写」处理 —— 兼容路径。
            let reported = message
                .get("upToIndex")
                .and_then(Value::as_i64)
                .or_else(|| message.get("index").and_then(Value::as_i64))
                .unwrap_or(0);
            let new_acked = (reported + 1).max(0) as usize;
            let mut accepted = false;
            update(|state| {
                let now = now_ms();
                let mut progress = None;
                if let Some(item) = state.transfer.as_mut() {
                    item.last_ack_ms = now;
                    item.last_progress_ms = now;
                    if new_acked > item.acked_index {
                        let old_acked = item.acked_index;
                        let new_acked = new_acked.min(item.chunks.len());
                        // 销账：在途里所有 `< new_acked` 的片都算完成。
                        let mut sample = 0u64;
                        item.in_flight.retain(|(value, sent_at)| {
                            if *value < new_acked {
                                let rtt = now.saturating_sub(*sent_at) as u64;
                                sample = sample.max(rtt);
                                false
                            } else {
                                true
                            }
                        });
                        item.in_flight_bytes = item
                            .in_flight
                            .iter()
                            .filter_map(|(value, _)| item.chunks.get(*value).map(|chunk| chunk.bytes))
                            .sum();
                        if sample > 0 {
                            item.rtt_ms = if item.rtt_ms == 0 {
                                sample
                            } else {
                                (item.rtt_ms * 3 + sample) / 4
                            };
                        }
                        item.acked_index = new_acked;
                        let before = prefix_bytes(&item.chunks, old_acked);
                        item.acked_bytes = prefix_bytes(&item.chunks, new_acked);
                        item.speed_window_bytes += item.acked_bytes.saturating_sub(before);
                        let elapsed = now.saturating_sub(item.speed_window_start);
                        if elapsed >= 250 {
                            item.speed_kbps =
                                (item.speed_window_bytes as f64 * 1000.0 / elapsed.max(1) as f64 / 1024.0 * 10.0).round() / 10.0;
                            item.speed_window_start = now;
                            item.speed_window_bytes = 0;
                        }
                        item.retry_count = 0;
                        item.stall_count = 0;
                        item.session_losses = 0;
                        item.gap_streak = 0;
                        // 窗口加法增长：每连续 `WINDOW_GROWTH_ACKS` 片顺利确认涨 1 片，
                        // 上限是协商出来的 `window_cap`。重传会清零 `stable_acks`。
                        item.stable_acks += new_acked - old_acked;
                        if item.stable_acks >= WINDOW_GROWTH_ACKS && item.window_size < item.window_cap {
                            item.window_size += 1;
                            item.stable_acks = 0;
                        }
                        progress = Some(format!(
                            "正在同步 {}/{} · {} 分片",
                            item.acked_index,
                            item.chunks.len(),
                            chunk_label(item.chunk_bytes)
                        ));
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
                tracing::debug!(acked = new_acked, "cumulative chunk acknowledgement");
            }
            // **不要在这个回调栈里立刻发下一批**：实测「收完一条回包几毫秒内就发」
            // 的首次丢失率 61.5%。隔 25ms 让手环与宿主链路先回到可接收状态，
            // 再由巡视链保持窗口。
            if accepted {
                arm_timer(25, json!({ "type": "amakano.timer", "kind": "send-next" }).to_string());
            }
            send_next = false;
        }
        Some("amakano.files.gap") => {
            // 手环收到了超前于当前位点的片：中间有缺口，从它要的 `next` 重发。
            // 这条路径比等超时快一个数量级，也是 v3 相对 v2 最关键的恢复能力。
            let next = message.get("next").and_then(Value::as_u64).unwrap_or(0) as usize;
            let mut healed = false;
            update(|state| {
                if let Some(item) = state.transfer.as_mut() {
                    let next = next.min(item.chunks.len());
                    item.next_index = next;
                    item.in_flight.clear();
                    item.in_flight_bytes = 0;
                    // gap 是「手环回了话、只是中间缺片」，不是 ack 停滞：
                    // 不算 `stall_count`，但单独累计 `gap_streak`，防止 gap 风暴无上限。
                    item.retry_count = item.retry_count.saturating_add(1);
                    item.gap_streak = item.gap_streak.saturating_add(1);
                    item.window_size = (item.window_size / 2).max(2);
                    item.last_ack_ms = now_ms();
                    item.last_progress_ms = now_ms();
                    healed = true;
                    tracing::warn!(next, window = item.window_size, "band reported a gap; resending from next");
                }
            });
            if healed {
                let exhausted = update(|state| {
                    state
                        .transfer
                        .as_ref()
                        .is_some_and(|transfer| gap_streak_exhausted(transfer.gap_streak))
                });
                if exhausted {
                    fail_transfer(ErrorCode::Window, "手环连续报告分片缺口");
                    return false;
                }
                set_status(StatusKind::Warn, "手环报告分片缺口，正在补发…");
                render();
                send_next = true;
            }
        }
        Some("amakano.files.pause") => {
            // 手环写盘明显变慢：它主动让我们停一拍。恢复用现有 timer 机制，不新造。
            let ms = message.get("ms").and_then(Value::as_u64).unwrap_or(1000).min(8000);
            update(|state| {
                if let Some(item) = state.transfer.as_mut() {
                    let deadline = now_ms() + ms as u128;
                    item.paused_until_ms = deadline;
                    // 暂停期间本来就不会有确认：把两个计时基准推到暂停结束，
                    // 恢复后的第一拍才不会把整段暂停算成「停滞」。
                    item.last_ack_ms = deadline;
                    item.last_send_ms = deadline;
                }
            });
            set_status(StatusKind::Warn, format!("手环写盘较慢，暂停 {ms} ms 后自动继续"));
            render();
            arm_timer(ms + 40, json!({ "type": "amakano.timer", "kind": "resume" }).to_string());
        }
        Some("amakano.files.complete") => {
            update(|state| {
                state.transfer = None;
                state.transfer_tick_armed = false;
                // **必须一起清 `pack_meta` / `pack_files`**：章节行的「传输中」徽章
                // 判的是 `pack_meta.chapter_number`，只清 transfer 会让那一章永久卡住。
                state.pack_meta = None;
                state.pack_files = Vec::new();
                state.verify_pack = None;
                state.verify_request = None;
                state.verify_attempts = 0;
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
                    start_embedded_transfer(number).await;
                }
                // **隔一拍再去拉列表**：手环这一刻正要写注册表、重扫清单、重绘界面
                // （姊妹项目 2026-10-07 的 `mm_malloc` panic 就发生在这段之后紧接着的扫描上）。
                // 立刻追着要 `pack.list`，等于在它最吃紧的时候又压一轮往返与一次全量解析。
                None => arm_timer(POST_COMPLETE_DELAY_MS, json!({ "type": "amakano.timer", "kind": "post-complete" }).to_string()),
            }
            return false;
        }
        // 手环说「先别发了」：它那边写盘慢/要喘口气（见 `pack-importer.js` 的 `requestPause`）。
        // 听话 —— 这段时间一片都不发，并把在途分片的「发送时刻」跟着往后挪，
        // 免得暂停一结束就把它们全判成超时一起重发（那正好是它喊停时要避免的事）。
        Some("amakano.files.error") => {
            // 手环回的 `error` 是**码**（`size` / `base64` / `write` / `window` …），
            // 以前直接把它拼进状态行给用户看 —— 那是给机器看的，用户读不懂。
            // 现在交给 `ErrorCode` 映射成人话 + 「怎么办」，原文放到细节里。
            let raw = message.get("error").and_then(Value::as_str).unwrap_or("unknown").to_string();
            // 手环把细节拼在同一串里（`write-301` / `size-…`），所以先按整串认，
            // 认不出来再按第一个 `-` 之前那段认；仍然认不出才是 Unknown。
            // （`resume-not-at-file-start` 这类带 `-` 的完整码在整串那一步就命中。）
            let code = match ErrorCode::from_wire(&raw) {
                ErrorCode::Unknown => ErrorCode::from_wire(raw.split('-').next().unwrap_or(&raw)),
                known => known,
            };
            let detail = message
                .get("detail")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| raw.clone());
            // 这些错误的意思是「我这边的接收会话没了 / 这一片不能按当前会话写」，
            // 而不是「这个包有问题」：**重发 begin，让手环按磁盘游标续上**。
            let recoverable = ["no-session", "session-timeout", "write-stall", "window", "resume-not-at-file-start"]
                .iter()
                .any(|prefix| raw == *prefix || raw.starts_with(&format!("{prefix}-")));
            if recoverable {
                tracing::warn!(raw = %raw, "band lost the receive session; restarting from the persisted cursor");
                let mut give_up = false;
                update(|state| {
                    if let Some(transfer) = state.transfer.as_mut() {
                        transfer.started = false;
                        transfer.ready = false;
                        transfer.waiting_ready = false;
                        transfer.in_flight.clear();
                        transfer.in_flight_bytes = 0;
                        // 重开会话也要有预算：手环反复重启时不能无限 begin/chunk 循环。
                        transfer.session_losses = transfer.session_losses.saturating_add(1);
                        give_up = session_loss_exhausted(transfer.session_losses);
                    }
                    state.restart_transfer = state.transfer.is_some() && !give_up;
                    state.status = if give_up {
                        "手环连续多次丢失接收会话，已暂停自动重试".into()
                    } else {
                        "手环的接收会话已重置，正在按断点续传…".into()
                    };
                    state.status_kind = StatusKind::Warn;
                });
                if give_up {
                    // 先让 fail_transfer 正常收尾（它会清 transfer/pack_meta），
                    // **再**清 auto_resume：顺序反过来的话 fail_transfer 会重新写回意图，
                    // 重连后新建 transfer、计数归零，闸门永远不成立。
                    fail_transfer(ErrorCode::Timeout, "手环连续丢失接收会话");
                    update(|state| {
                        state.auto_resume = None;
                        state.status = "手环连续丢失接收会话，已停止自动续传；重连后点「继续传输」".into();
                        state.status_kind = StatusKind::Bad;
                    });
                    save_persisted();
                    render();
                    return false;
                }
                render();
                return false;
            }
            tracing::warn!(raw = %raw, ?code, "band rejected the import");
            update(|state| {
                if let Some(transfer) = state.transfer.as_mut() {
                    transfer.started = false;
                    transfer.waiting_ready = false;
                    transfer.in_flight.clear();
                }
                state.error = Some(ErrorView::new(code, detail));
                state.stage = state.stage.min(SessionStage::AppAlive);
                state.status = format!("{}：{}", code.label(), code.advice());
                state.status_kind = StatusKind::Bad;
                // 失败之后把「正在传」放开，章节按钮重新可点（`pack_meta` 也要清，
                // 否则那一章会停在「传输中」徽章上 —— 真机踩过）。
                state.transfer = None;
                state.transfer_tick_armed = false;
                state.pack_meta = None;
                state.pack_files = Vec::new();
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
    request_id: String,
    session: Option<u64>,
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
        self.request_id.clear();
        self.session = None;
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
/// 分片累积的结果。
///
/// 分成三态而不是 `Result<Option<..>>`，是因为「迟到的回包」和「坏掉的回包」
/// 必须分开对待：前者是正常现象（手环那时候正忙），后者才该报给人看。
/// 旧实现把两者都塞进 `Err(String)`，于是一条无关痛痒的迟到片会把界面上
/// 正在等的请求判死，报一句「读取阅读统计失败：这是旧请求或旧连接的回包」——
/// 用户看到的是红字，而真实原因是它来晚了几秒。
enum ShardOutcome {
    /// 收齐了，按 `seq` 排好序。
    Complete(Vec<Value>),
    /// 还没齐：安静等着，不动界面。
    Pending,
    /// 属于别的请求/别的会话：**不是错误**，丢掉即可。
    Foreign,
    /// 真的坏了（片号越界、`total` 矛盾…），必须报出去。
    Broken(String),
}

fn push_shard_into(
    slot: &'static OnceLock<Mutex<SaveShardSlot>>,
    message: &Value,
    what: &str,
) -> ShardOutcome {
    let seq = message.get("seq").and_then(Value::as_u64).unwrap_or(0);
    let total = message.get("total").and_then(Value::as_u64).filter(|value| *value > 0);
    let last = message.get("last").and_then(Value::as_bool).unwrap_or(false);
    let bytes = message.get("data").and_then(Value::as_str).map(str::len).unwrap_or(0);
    if seq >= MAX_SAVE_SHARDS {
        return ShardOutcome::Broken(format!(
            "手环回包的第 {seq} 片超出上限（最多 {MAX_SAVE_SHARDS} 片），请重试一次"
        ));
    }

    // 下面整段在**一次加锁**里跑完：累积、判完、把结果取出来。解包（可能很慢）
    // 放到锁外做，日志与界面回调也就不会在锁里跑。
    let outcome = with_shards(slot, |slot| -> ShardOutcome {
        let request_id = message.get("requestId").and_then(Value::as_str).unwrap_or("");
        let session = update(|state| state.request.get().and_then(|request| {
            (request.id == request_id && request.session == state.session).then_some(request.session)
        }));
        if slot.parts.is_empty() {
            slot.request_id = request_id.to_string();
            slot.session = session;
        }
        // 不属于当前请求：不是错误，也不是「读取失败」，只是这条片来得不是时候。
        // 分片可能是上一轮重发的迟到片，也可能属于已翻页的旧会话。
        if slot.request_id != request_id || slot.session.is_none() || slot.session != session {
            return ShardOutcome::Foreign;
        }
        // 片数前后矛盾（两套回包混在一帧里）：宁可明说，也不要把两份数据拼成一段坏 JSON。
        if let (Some(seen), Some(expected)) = (total, slot.total)
            && seen != expected
        {
            slot.reset();
            return ShardOutcome::Broken(format!("手环回包前后对不上：先说 {expected} 片，这一片又说 {seen} 片，请重试"));
        }
        if total.is_some() {
            slot.total = total;
        }
        if slot.parts.contains_key(&seq) {
            let previous = slot.parts.get(&seq).and_then(|value| value.get("data")).and_then(Value::as_str);
            let had = previous.map(str::len).unwrap_or(0);
            let identical = previous
                .map(|value| value == message.get("data").and_then(Value::as_str).unwrap_or(""))
                .unwrap_or(false);
            // **内容一样的重复片直接丢弃**：这不是错误，是同一条回包被投递了多次
            // （手环端 `onmessage` 被多个 bundle 各装一次就会这样，见 `pack-importer.js`
            // 的 `isDuplicateInbound`）。覆盖不会有副作用，但重复计入 `finished`/字节数
            // 会让「收齐」判定错乱，还会把「重发才到的新内容」和「纯重复」混为一谈。
            if identical {
                tracing::debug!(what, seq, had, "band shard duplicate ignored (identical payload)");
            } else {
                tracing::warn!(what, seq, had, now = bytes, "band shard replaced with different content");
                slot.bytes = slot.bytes.saturating_sub(had).saturating_add(bytes);
                slot.parts.insert(seq, message.clone());
            }
        } else {
            slot.bytes += bytes;
            slot.parts.insert(seq, message.clone());
        }
        slot.finished |= last;
        let complete = slot.finished && slot.missing() == 0;
        if !complete {
            let (total, have) = (slot.total.unwrap_or(0), slot.parts.len());
            tracing::info!(what, seq, total, have, "band shard buffered");
            return ShardOutcome::Pending;
        }
        ShardOutcome::Complete(slot.ordered())
    });

    let ShardOutcome::Complete(shards) = outcome else {
        // `Pending` / `Foreign` 都是「什么都不做」；只有 `Broken` 需要报到界面。
        return outcome;
    };
    // 收齐了就清空：下一次请求万一没走到对应的 `request_*`（例如手环自己重发），
    // 也不会把新回包拼到这一份已经用过的碎片上。
    let bytes = with_shards(slot, |slot| {
        let bytes = slot.bytes;
        slot.reset();
        bytes
    });
    tracing::info!(what, shards = shards.len(), bytes, "band shard reply complete");
    ShardOutcome::Complete(shards)
}

/// `amakano.saves.data` 的分片累积。
fn push_save_shard(message: &Value) -> ShardOutcome {
    push_shard_into(&SAVE_SHARDS, message, "存档")
}

/// `amakano.stats.data` 的分片累积。
///
/// **单独一个坑位**：两种回包的碎片绝不能混在同一个槽里 ——
/// 统计回包刚好把存档那份还没收齐的片段「补齐」成一段坏 JSON，是最难查的那种故障。
fn push_stats_shard(message: &Value) -> ShardOutcome {
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
async fn dispatch_ui_event(event_id: &str, kind: event::UiEvent) {
    let Some(action) = parse_action(event_id) else { return };
    let pointer = matches!(kind, event::UiEvent::Click | event::UiEvent::PointerUp);
    let down = matches!(kind, event::UiEvent::PointerDown);
    let hover_event = matches!(kind, event::UiEvent::MouseEnter | event::UiEvent::MouseLeave);

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
            handle_action(action).await;
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
    handle_action(action).await;
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
async fn run_refresh(plan: RefreshPlan) {
    let mut steps = refresh_steps(plan);
    if steps.is_empty() {
        return;
    }
    let first = steps.remove(0);
    // 先把「还没发的」排进队列，再发第一个：第一个的回包可能来得很快，
    // 那时 `settle_refresh_step` 得能从队列里拿到下一步。
    update(|state| state.refresh_queue = steps);
    run_refresh_step(first).await;
}

/// 执行一个动作。
async fn handle_action(action: Action) {
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
        Action::Connect => connect_device(None, false).await,
        Action::DevicesRefresh => refresh_devices().await,
        Action::Device(index) => connect_device(Some(index), false).await,
        Action::Launch => launch_app().await,
        // 重新打开 HTML 控制面板窗口（用户关掉窗口后从插件主面板回来）。
        Action::OpenPanel => server::reopen().await,
        // 后备通道：WebView 打不开时，主面板改画宿主自己的元素树。
        //
        // 切换后必须重绘 —— 这正是「用户点一下就看到界面变了」的那一步。
        // 也要把状态行收口：`dispatch_ui_event` 在进入这里之前把它设成了
        // 「正在处理…」，不覆盖的话那一行会一直挂着（界面上看着像卡住了）。
        Action::UseNative => {
            update(|state| {
                state.native_ui = true;
                state.status = "已切到原生界面".into();
                state.status_kind = StatusKind::Info;
            });
            render();
            tracing::info!("切换到原生界面（WebView 后备通道）");
        }
        Action::BackToLanding => {
            update(|state| {
                state.native_ui = false;
                state.status = "已回到主面板".into();
                state.status_kind = StatusKind::Info;
            });
            render();
            tracing::info!("回到引导页（HTML 面板入口）");
        }
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
            queue_sync(queue).await;
        }
        Action::Sync(number) => start_embedded_transfer(number).await,
        Action::Resume => resume_transfer().await,
        Action::RefreshList => run_refresh(action.refresh_plan()).await,
        Action::ClearCache => request_clear_cache().await,
        // 章节包删除**一步到位**：插件里有一份完整副本，删掉随时能重新同步回来，
        // 再要一次确认只是白加一步（用户要求「减少不必要的二次确认」）。
        // 存档删除不一样 —— 那是不可恢复的，所以只有它保留了确认（见 `SavesDelete`）。
        Action::Delete(pack_id) => {
            request_delete_pack(&pack_id).await;
        }
        Action::Chunk(size) => {
            set_chunk_size(size);
        }
        Action::Mode(mode) => {
            set_transfer_mode(mode);
        }
        Action::AutoLaunch(enabled) => {
            update(|state| state.auto_launch = enabled);
            save_persisted();
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
            if !band_data_ready() {
                update(|state| state.saves_error = "还没从手环读到数据：请先连接并打开《甜蜜女友2》".into());
                set_status(StatusKind::Warn, "还没从手环读到数据，暂时不能刷新存档");
                render();
                return;
            }
            update(|state| {
                state.saves_error.clear();
                state.saves_notice.clear();
                state.saves_busy = true;
            });
            // 目录还没有权威答案（首次连接 / 上次目录失败）才补一次 `pack.list`；
            // 已答过就只读存档，避免「刷新几条存档先扫一遍手环文件系统」。
            let plan = {
                let current = state().lock().unwrap_or_else(|error| error.into_inner());
                if crate::request::need_catalog_refresh(current.catalog_answered, &current.library_error) {
                    RefreshPlan { pack_list: true, save_list: true, ..RefreshPlan::default() }
                } else {
                    action.refresh_plan()
                }
            };
            run_refresh(plan).await;
        }
        // ---- 阅读统计 ----
        Action::StatsRefresh => {
            if !band_data_ready() {
                update(|state| state.stats_error = "还没从手环读到数据：请先连接并打开《甜蜜女友2》".into());
                set_status(StatusKind::Warn, "还没从手环读到数据，暂时不能读取统计");
                render();
                return;
            }
            update(|state| {
                state.stats_error.clear();
                state.stats_busy = true;
            });
            // 「刷什么」同样只有 ui-core 一处判定（统计页只刷统计）。
            run_refresh(action.refresh_plan()).await;
        }
        Action::SavesExport => start_save_export().await,
        Action::SavesImport => start_save_import().await,
        Action::SavesCapture => {
            if !band_data_ready() {
                set_status(StatusKind::Warn, "还没连接手环或游戏未响应，暂时不能保存当前进度");
                render();
                return;
            }
            update(|state| {
                state.saves_error.clear();
                state.saves_busy = true;
                state.pending_current_save = None;
                state.pending_save_slot = None;
                state.saves_notice = "正在从手环读取当前游戏进度…".into();
            });
            request_save_capture().await;
        }
        Action::SavesWrite(slot) => {
            let save = update(|state| state.pending_current_save.clone());
            let Some(save) = save else {
                set_status(StatusKind::Warn, "当前进度还没读到，请先点「读取当前进度」");
                render();
                return;
            };
            if !band_data_ready() {
                set_status(StatusKind::Warn, "还没连接手环或游戏未响应，暂时不能保存到槽位");
                render();
                return;
            }
            update(|state| {
                state.saves_error.clear();
                state.saves_busy = true;
                state.pending_save_slot = Some(slot);
                // 旧实现只写 saves_notice，但原生页读取的是**状态行**；不一起更新的话
                // 用户点完槽位看不到任何反馈，像是按钮没反应。
                state.saves_notice = format!("正在保存到存档 {}…", slot + 1);
                state.status = state.saves_notice.clone();
                state.status_kind = StatusKind::Info;
            });
            render();
            // 空槽和已有槽都走 `replace`：用一份完整、按用户选择顺序排好的槽位表写回。
            // 这样“空槽保存”和“覆盖”只有一条路径，不会出现手环端下标错位。
            let mut slots = update(|state| state.save_slots.clone());
            if slot < slots.len() {
                slots[slot] = save;
            } else {
                slots.resize(slot, Value::Null);
                slots.push(save);
            }
            request_manual_slots_replace(slots).await;
        }
        Action::SavesLoad(slot) => {
            let Some(slot) = slot else {
                // 自动存档没有「读档」语义（它本来就是继续阅读的进度）。
                set_status(StatusKind::Info, "自动存档就是「继续阅读」，不用手动读");
                return;
            };
            if !band_data_ready() {
                set_status(StatusKind::Warn, "还没从手环读到存档数据，请等读取完成后再读档");
                render();
                return;
            }
            update(|state| {
                state.saves_error.clear();
                state.saves_busy = true;
            });
            request_save_activate(slot).await;
        }
        // 存档删除要点两次：第一次只是把这一行切到「确认删除」——
        // 存档没了就真没了，这一步不能省（章节包删除相反，见上面的 `Action::Delete`）。
        Action::SavesDelete(slot) => {
            update(|state| state.confirm_delete_save = Some(saves::slot_key(slot)));
            set_status(StatusKind::Warn, "再点一次「确认删除」才会真的从手环上删掉这条存档");
        }
        Action::SavesConfirmDelete(slot) => {
            // 二次确认必须在状态里真的存在：只有先经过 `SavesDelete` 置了对应槽位的
            // 确认态，才允许真删。HTML 前端的 action 走 HTTP，虽然同一窗口里
            // 前端会先发 `save-delete:`，但后端不能依赖调用方守规矩 ——
            // 直接 POST `save-delete-ok:N` 必须被挡回去。
            let expected_key = saves::slot_key(slot);
            let confirmed = update(|state| {
                if state.confirm_delete_save.as_deref() == Some(expected_key.as_str()) {
                    state.confirm_delete_save = None;
                    true
                } else {
                    false
                }
            });
            if !confirmed {
                set_status(StatusKind::Warn, "删除需要先点一次删除、再确认；这次没有执行");
                return;
            }
            match slot {
                Some(slot) => {
                    update(|state| {
                        state.saves_error.clear();
                        state.saves_busy = true;
                    });
                    request_save_delete(slot).await;
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
        Action::CloudAuthorize => cloud::authorize().await,
        Action::CloudBindingCode => cloud::binding_code().await,
        Action::CloudCopyCode => {
            let command = update(|state| state.cloud_code.clone());
            if command.is_empty() {
                set_status(StatusKind::Warn, "还没有绑定命令，请先生成");
            } else {
                match clipboard::write_envelope(&command).await {
                    Ok((bytes, verified)) => {
                        let notice = if verified {
                            "绑定命令已复制，去 QQ 群粘贴发送"
                        } else {
                            "绑定命令已复制（未能读回核对）"
                        };
                        tracing::info!(bytes, verified, "binding command copied to clipboard");
                        set_status(if verified { StatusKind::Good } else { StatusKind::Warn }, notice);
                    }
                    Err(error) => {
                        tracing::warn!(reason = error.message(), "binding command copy failed");
                        set_status(StatusKind::Bad, format!("复制失败：{}", error.message()));
                    }
                }
            }
            render();
        }
        Action::CloudSession => cloud::session().await,
        Action::CloudSync => {
            let request = update(|state| {
                if state.cloud_running || !state.cloud_bound { return None; }
                state.cloud_synced_session = None;
                Some(state.session)
            });
            if let Some(session) = request {
                request_band_operation(cloud::BandOperation::List { session }).await;
            }
        }
        Action::CloudForget => cloud::forget_local_account().await,
        Action::OpenWebsite => psys_host::dialog::open_url(WEBSITE),
        Action::CheckUpdate => check_update().await,
    }
}

/// 查询官方插件源的最新发布版本。
///
/// 官方索引是公开 JSON，按仓库地址找到本项目。只比较**版本号**，不下载安装包；
/// 若索引尚未收录新版本，就如实显示索引中的版本，不能声称「无更新」。
async fn check_update() {
    update(|state| {
        state.update_status = "正在检查更新…".into();
    });
    render();
    let result = async {
        let uri = PLUGIN_INDEX_URL.parse::<wstd::http::Uri>().map_err(|_| "更新源地址无效".to_string())?;
        let request = wstd::http::Request::builder()
            .method("GET")
            .uri(uri)
            .header("accept", "application/json")
            .body(wstd::http::Body::empty())
            .map_err(|error| error.to_string())?;
        let mut client = wstd::http::Client::new();
        client.set_connect_timeout(std::time::Duration::from_secs(12));
        client.set_first_byte_timeout(std::time::Duration::from_secs(12));
        client.set_between_bytes_timeout(std::time::Duration::from_secs(12));
        wstd::runtime::block_on(async {
            let mut response = client.send(request).await.map_err(|error| format!("无法连接更新源：{error}"))?;
            if !response.status().is_success() { return Err(format!("更新源返回 {}", response.status().as_u16())); }
            // **先看 Content-Length 再读**：更新源是不可信外部输入，等读完再比大小
            // 等于让对方决定我们缓冲多少字节。没有长度时仍保留读完后的兜底检查。
            const MAX_INDEX_BYTES: u64 = 512 * 1024;
            if response.body().content_length().is_some_and(|length| length > MAX_INDEX_BYTES) {
                return Err("更新信息过大".to_string());
            }
            let bytes = response.body_mut().bytes_contents().await.map_err(|error| format!("读取更新信息失败：{error}"))?;
            if bytes.len() as u64 > MAX_INDEX_BYTES { return Err("更新信息过大".to_string()); }
            let value: Value = serde_json::from_slice(&bytes).map_err(|_| "更新源返回的不是合法 JSON".to_string())?;
            let version = value
                .get("plugins")
                .and_then(Value::as_array)
                .and_then(|plugins| plugins.iter().find(|plugin| {
                    plugin.get("repo").and_then(Value::as_str).is_some_and(|repo| repo.contains(PLUGIN_REPO_MARK))
                }))
                .and_then(|plugin| plugin.pointer("/manifest/version"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| "官方源还没有收录当前插件".to_string())?;
            Ok(version)
        })
    }.await;
    match result {
        Ok(latest) => {
            let current = update(|state| state.version.clone());
            let available = crate::version_is_newer(&latest, &current);
            update(|state| {
                state.update_latest = latest.clone();
                state.update_available = available;
                state.update_status = if available {
                    format!("官方源有 {latest}，当前运行 {current}")
                } else {
                    format!("当前 {current}，官方源 {latest}，已是最新")
                };
            });
        }
        Err(message) => update(|state| state.update_status = message),
    }
    render();
}

// ------------------------------------------------------------------ 存档：导出 / 导入
//
// 两条入口都是「分发器里只改状态 + 渲染一帧 + spawn 出真正的活儿 + 立刻返回」：
// **`on_ui_event` / `on_event` 是宿主的事件分发线程，不能在里面等用户。**
//
// 这一条不是预防性写法，是这一轮真机实测换来的教训（见 docs/插件开发注意事项.md 第 7 节）：
// 0.5.1 的导出/导入走 Dialog（`save_file_start` / `pick_file`），而 Dialog **需要用户交互**——
// 在本宿主上一次**永远不返回**：点「测试导出对话框」后日志只有 `dialog probe started`、
// 没有 finished，随后连与对话框毫无关系的「测试插件目录可写」也不再有 finished，
// 整个插件只能靠禁用再启用救回来。分发器被堵死之后，所有点击都石沉大海。
//
// 所以现在的硬规矩是：**任何可能等待用户交互的宿主调用，都不许跑在事件分发器里**
// （哪怕看起来「很快」）。剪贴板读写不弹任何窗口、不等任何点击，v4 里直接在
// `on_ui_event` 的 async 链上 await 即可 —— 不再需要 `spawn` 兜底。

/// 导出：把当前存档打成信封，写进剪贴板。
///
/// `Dialog`（用户选路径）这条路已经作废，剪贴板是新的落点 —— 用户自己把那段 JSON 粘到
/// 备忘录 / 聊天窗口 / 文本文件里就完成了「保存」。
async fn start_save_export() {
    if !band_data_ready() {
        update(|state| state.saves_error = "还没从手环读到数据：请先连接并打开《甜蜜女友2》".into());
        set_status(StatusKind::Warn, "还没从手环读到数据，暂时不能导出存档");
        render();
        return;
    }
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

        let task = async move {
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
        };
    task.await;
}

/// 导入：读剪贴板文本 → 解析信封 → 按 `savedAt` 并入手环。
async fn start_save_import() {
    if !band_data_ready() {
        update(|state| state.saves_error = "还没从手环读到数据：请先连接并打开《甜蜜女友2》".into());
        set_status(StatusKind::Warn, "还没从手环读到数据，暂时不能导入存档");
        render();
        return;
    }
    update(|state| {
        state.saves_busy = true;
        state.saves_error.clear();
        state.saves_notice = "正在从剪贴板读取存档…".into();
        state.save_import = None;
    });
    set_status(StatusKind::Info, "正在从剪贴板读取存档…");
    render();
    tracing::info!("save import from clipboard started");

        let task = async move {
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
            state.save_import = Some((incoming, duplicates, band_before, incoming));
        });
        render();

        // 导入走 upsert：按 `savedAt` 认同一份存档（有就覆盖、没有就追加到末尾），
        // **不会把用户手环上原有的存档一把清掉**。整表替换（replace）留给将来的「还原」。
        // 重复的那些**照样发给手环**：覆盖回来才是用户要的合并（`amakano.saves.put-ok`
        // 那一支会把「写进去几条 / 跳过几条重复」念给用户听）。
        // 这里不再按 20 条截断：手环 `recoveryData` 是普通数组，没有这个硬上限。
        request_save_put("upsert", merge.slots, file.auto_save).await;
        };
    task.await;
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
    async fn on_event(event_type: EventType, payload: String) -> String {
        let is_interconnect = matches!(event_type, EventType::InterconnectMessage);
        if is_interconnect {
            tracing::info!(bytes = payload.len(), "received interconnect message");
        }
        let send_next = is_interconnect && handle_interconnect(&payload).await;
        if send_next {
            send_next_packet().await;
        }
        if matches!(event_type, EventType::Timer) {
            // 宿主派发定时器事件时会把我们传入的字符串包一层信封：
            // `{"timerId":1,"kind":"timeout","payload":"<我们传入的字符串>"}`
            // （见官方 Timer 接口文档），所以这里先拆信封再分发。
            let inner = parse_event_payload(&payload);
            handle_timer(&inner).await;
        }
        // 手环丢了接收会话：用新的 requestId 重发 begin，断点由手环的字节游标接上。
        if update(|state| std::mem::take(&mut state.restart_transfer)) {
            tracing::info!("restarting transfer after a lost band session");
            start_transfer().await;
        }
        // 自动重连放在这里：所有 interconnect 回包都可能把 `reconnect_pending` 立起来。
        // `auto = true`：这是插件自己接回去，不是用户点连接 —— 不重开游戏窗口。
        if update(|state| std::mem::take(&mut state.reconnect_pending)) {
            tracing::info!("auto-reconnecting");
            connect_device(None, true).await;
        }
        "accepted".into()
    }

    async fn on_ui_event(event_id: String, event: event::UiEvent, _payload: String) -> String {
        dispatch_ui_event(&event_id, event).await;
        "accepted".into()
    }

    async fn on_ui_render(element_id: String) {
        update(|state| state.element_id = Some(element_id));
        render();
    }

    async fn on_card_render(_card_id: String) {}
}

impl lifecycle::Guest for ImportPlugin {
    async fn on_load() {
        logger::init();
        tracing::info!("Amakano2 pack importer loaded");
        // 把上次的分片档位 / 自动打开 / 未传完的队列读回来。
        // **只恢复偏好**：断点权威在手环，这里读不到也不会导致从头传。
        load_persisted();
        load_cloud();
        // 首次安装不自动读取 AstroBox 账号：必须先由用户在账号页明确授权。
        // 只有本机曾授权过同一 AstroBox 账号时，才恢复已绑定的 QQ。
        cloud::restore_if_authorized().await;
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
                set_error(ErrorCode::BadPack, &message);
                update(|state| {
                    state.library_error = message;
                    state.status = "插件内的章节包不可用".into();
                    state.status_kind = StatusKind::Bad;
                });
            }
        }
        // 首次枚举在线设备：顶栏在多设备时直接变成选择菜单，用户不用先点“连接”才知道有哪几台。
        // 只读宿主的在线列表，不建立 interconnect 通道，也不打扰手环。
        refresh_devices().await;
        // 起内建 HTTP 服务器 + 打开 HTML 前端窗口（HTML 界面取代元素树界面）。
        server::start().await;
    }
}

// `http` 导出：HTML 前端的请求入口（world = psys-world-v4-http）。
impl wit::exports::astrobox::psys_plugin_v4::http::Guest for ImportPlugin {
    async fn handle(
        server_id: u32,
        request: psys_host::http_server::Request,
    ) -> psys_host::http_server::Response {
        server::handle(server_id, request).await
    }
}

wit::export!(ImportPlugin with_types_in crate::host::wit);
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
    use crate::{
        INITIAL_WINDOW, SAVE_PROTOCOL,
        Keepalive, ack_timeout_ms, effective_mode, gap_streak_exhausted, hello_reports_legacy,
        keepalive_action, major_version, mark_first_hello_for_session, negotiated_window,
        pack_verified, saves, saves_capability_missing, session_loss_exhausted, version_is_newer,
        wire_limit,
    };
    use crate::TransferMode;

    /// 绑定命令必须原样来自服务端，不能在前端再拼一次「绑定」。
    #[test]
    fn binding_command_carries_exactly_one_prefix() {
        // 服务端返回的 command 已经是「绑定123456」；前端只负责显示与复制。
        let command = "绑定123456";
        assert!(command.starts_with("绑定"));
        assert_eq!(command.matches("绑定").count(), 1, "重复拼接会产生「绑定绑定123456」");
        // 这条用例锁住的是契约：前端拿 cloudCode 直接显示/复制，不再做字符串拼装。
        let rendered = command.to_string();
        assert_eq!(rendered, "绑定123456");
    }

    /// 更新比较只认「候选严格更大」：索引里的版本可能就是当前版本，
    /// 也可能因为索引格式变化给出空值/非数字，这些都不许报「有更新」。
    #[test]
    fn update_check_only_reports_strictly_newer_versions() {
        assert!(version_is_newer("4.1.0", "4.0.0"));
        assert!(version_is_newer("v4.0.1", "4.0.0"), "v 前缀要能容忍");
        assert!(version_is_newer("5.0.0", "4.9.9"));
        assert!(!version_is_newer("4.0.0", "4.0.0"), "相等不是更新");
        assert!(!version_is_newer("4.0", "4.0.0"), "短写法的缺位按 0 比");
        assert!(!version_is_newer("3.9.9", "4.0.0"), "旧版本不许反过来报更新");
        assert!(!version_is_newer("", "4.0.0"), "空版本不许报更新");
        assert!(!version_is_newer("unknown", "4.0.0"), "解析不出数字不许报更新");
        // 预发布按 SemVer：beta 升到正式版是更新，正式版不会被 beta 顶掉。
        assert!(version_is_newer("4.1.0", "4.1.0-beta"), "beta → 正式版应提示更新");
        assert!(!version_is_newer("4.1.0-beta", "4.1.0"), "正式版不该被旧 beta 顶掉");
        assert!(!version_is_newer("4.1.0-beta", "4.1.0-rc"), "同为预发布不猜词序，宁可不报");
        assert!(version_is_newer("4.2.0", "4.1.0-beta"), "数字段更大仍然优先");
    }

    /// 激进档要三样能力同时在手，缺一样就**当场退回正常档并给出原因**。
    /// 悄悄降级是最糟的：用户点了「激进」、速度没变、也没有一句话。
    #[test]
    fn 激进档缺能力时退回正常并说明原因() {
        let turbo = TransferMode::Turbo;
        // 能力齐备（32 KB 单片 + 协议 3）才放行。
        assert_eq!(effective_mode(turbo, 3, 32768, true).0, TransferMode::Turbo);
        assert_eq!(effective_mode(turbo, 3, 65536, true).1, "", "够用就不该有原因");

        // 还没协商完：先按正常跑，但**不写死结论**（下一次 hello 会再算一遍）。
        let (mode, why) = effective_mode(turbo, 3, 32768, false);
        assert_eq!(mode, TransferMode::Steady);
        assert!(!why.is_empty(), "退回时必须能说清为什么");

        // 旧应用：不认粗确认。
        let (mode, why) = effective_mode(turbo, 2, 32768, true);
        assert_eq!(mode, TransferMode::Steady);
        assert!(why.contains("版本"), "{why}");

        // 单片上限不够 32 KB：不能硬发，否则手环直接判 size 错误。
        let (mode, why) = effective_mode(turbo, 3, 16384, true);
        assert_eq!(mode, TransferMode::Steady);
        assert!(why.contains("16 KB"), "{why}");

        // 正常档永远不需要理由。
        assert_eq!(effective_mode(TransferMode::Steady, 0, 1024, false), (TransferMode::Steady, String::new()));
    }

    /// 连接保活：新应用用 ping；旧应用 hello / pack.list 交替，每 3 拍一次全量扫描。
    #[test]
    fn keepalive_switches_to_ping_only_when_the_app_supports_it() {
        assert_eq!(keepalive_action(true, 0), Keepalive::Ping);
        assert_eq!(keepalive_action(true, 7), Keepalive::Ping);
        assert_eq!(keepalive_action(false, 0), Keepalive::PackList);
        assert_eq!(keepalive_action(false, 1), Keepalive::Hello);
        assert_eq!(keepalive_action(false, 2), Keepalive::Hello);
        assert_eq!(keepalive_action(false, 3), Keepalive::PackList);
    }

    /// 重开会话的预算：只有连续丢会话才累加，8 次才停。
    #[test]
    fn session_loss_budget_stops_at_eight() {
        for stalls in 0..8u8 {
            assert!(!session_loss_exhausted(stalls), "{stalls} 次还没到上限");
        }
        assert!(session_loss_exhausted(8));
        assert!(session_loss_exhausted(9));
    }

    /// gap 有独立预算，不会吃掉 ack 停滞的 6 次额度。
    #[test]
    fn gap_streak_has_its_own_budget() {
        assert!(!gap_streak_exhausted(0));
        assert!(!gap_streak_exhausted(11));
        assert!(gap_streak_exhausted(12));
    }

    /// 完成核对只认精确的 packId；空 id 永远不算装上。
    #[test]
    fn completion_verification_requires_an_exact_pack_id() {
        let installed = vec!["公共资源".to_string(), "共通线".to_string()];
        assert!(pack_verified(&installed, "共通线"));
        assert!(!pack_verified(&installed, "千岁线"));
        assert!(!pack_verified(&installed, ""));
    }

    /// 单条消息的线上尺寸是协议数字：正常档 16 KB 分片的 base64 + JSON 约 22 KB，
    /// 32 KB 上限只作硬闸门（2026-10-07 打崩 SAR 的是 78–88 KB 的批量消息）。
    #[test]
    fn 线上尺寸上限是约定值() {
        let steady = TransferMode::Steady.params();
        let turbo = TransferMode::Turbo.params();
        assert_eq!(steady.wire_bytes, 32 * 1024, "正常档单条 32 KB 上限：超了就不发，绝不放大消息");
        assert_eq!(
            turbo.wire_bytes,
            48 * 1024,
            "激进档 32 KB 分片的 base64 约 43 KB：上限要装得下，但别逼近崩过的尺寸"
        );
        // 分片被手环压低时，闸门也跟着收紧（留给 JSON 头尾 1 KB）。
        assert_eq!(wire_limit(steady, 8192), wire_limit(steady, 16384), "上限以模式为准");
        assert!(wire_limit(steady, 32768) > steady.wire_bytes, "分片比模式上限还大时闸门要跟着放大，否则合法片被判超限");
    }

    /// 飞行窗口由 `hello-ok` 里手环报的 `receiveWindow`、`syncProtocol` **与传输模式**共同决定。
    /// 正常档报 12 → 10 片；激进档报 16 → 14 片；旧手环（协议 2）仍是 3 片。
    #[test]
    fn 飞行窗口按手环上报的接收窗口协商() {
        let steady = TransferMode::Steady;
        let turbo = TransferMode::Turbo;
        assert_eq!(negotiated_window(steady, None, 2), INITIAL_WINDOW.min(3), "旧应用不报窗口：3 片");
        assert_eq!(negotiated_window(steady, Some(4), 2), 3, "旧应用报 4：留 1 片余量");
        assert_eq!(negotiated_window(steady, Some(64), 2), 3, "旧协议封顶 3 片");
        assert_eq!(negotiated_window(steady, None, 3), INITIAL_WINDOW, "v3 没报窗口：按模式的起始值");
        assert_eq!(negotiated_window(steady, Some(12), 3), steady.params().window_cap, "v3 报 12：留 2 片余量后 10 片");
        assert_eq!(negotiated_window(steady, Some(4), 3), 2, "手环只报 4 片就按 2 片飞，不许放大");
        assert_eq!(negotiated_window(steady, Some(1), 3), 2, "窗口不许变成 0：停等会死锁，保底 2 片");
        assert_eq!(negotiated_window(steady, Some(64), 3), steady.params().window_cap, "报得再宽也封顶");
        // 激进档：手环报 16 片，但它的窗口上限是 8 片（**字节闸门算出来的**：
        // 256 KB ÷ 32 KB = 8），所以这里照样封在 8。
        // 比**片数**没有意义（分片本来就大了一倍），要比的是在途字节。
        assert_eq!(negotiated_window(turbo, Some(16), 3), turbo.params().window_cap);
        assert_eq!(negotiated_window(turbo, Some(16), 3), 8);
        assert_eq!(negotiated_window(turbo, None, 3), turbo.params().initial_window);
        let steady_chunks = negotiated_window(steady, Some(16), 3);
        let turbo_chunks = negotiated_window(turbo, Some(16), 3);
        assert!(
            turbo_chunks * turbo.params().chunk_bytes > steady_chunks * steady.params().chunk_bytes,
            "激进档的在途字节必须真的更多（{turbo_chunks}×32KB vs {steady_chunks}×16KB），\
             否则切过去只是白改分片"
        );
        // 手环报得再宽也不许越过激进档的上限。
        assert_eq!(negotiated_window(turbo, Some(64), 3), turbo.params().window_cap);
    }

    /// v3 的停滞判定只看「多久没有累计确认」，不再给每片各挂超时。
    /// **激进档的确认稀得多，判定下限必须跟着抬高**，否则会把正常节拍判成停滞。
    #[test]
    fn 累计确认的停滞窗口有上下限() {
        let steady = TransferMode::Steady;
        let turbo = TransferMode::Turbo;
        assert_eq!(ack_timeout_ms(steady, 0), 2500, "还没有 RTT 样本：2.5 秒");
        assert_eq!(ack_timeout_ms(steady, 200), 1500, "rtt 200ms → 1300 夹到 1500");
        assert_eq!(ack_timeout_ms(steady, 3000), 5000, "再慢也封顶，免得死等");
        assert!(ack_timeout_ms(steady, 1000) > 2500, "慢链路要能等到累计确认");
        // 激进档：每 1.2 秒才报一次位点，下限必须远大于它。
        assert!(
            ack_timeout_ms(turbo, 0) >= turbo.params().ack_ms * 3,
            "激进档的停滞下限必须比确认间隔宽得多"
        );
        assert!(
            ack_timeout_ms(turbo, 200) >= turbo.params().stall_floor_ms,
            "激进档再快的链路也不许把判定压到确认间隔以下"
        );
    }

    /// 「版本或协议不匹配」的判据必须有两半：应用已经活着 + 协议没协商出来。
    /// 少了「已经活着」这半边，连接时应用还没起来的那几秒会被误报成版本不一致。
    #[test]
    fn 应用未醒时的存档超时不算版本或协议不匹配() {
        assert!(!saves_capability_missing(false, None), "应用一条消息都没答：只能是「手环没有响应」");
        assert!(saves_capability_missing(true, None), "应用活着但协商不出协议：这才是版本/协议问题");
        assert!(!saves_capability_missing(true, Some(SAVE_PROTOCOL)), "协商成功就不该再判成版本问题");
        assert!(!saves_capability_missing(false, Some(SAVE_PROTOCOL)), "上一会话残留的能力值不能顶替「应用活着」");
    }

    /// 主版本号从 `hello-ok` 的 `appVersion` 里取：容忍 "v" 前缀，取不到数字就是 `None`。
    #[test]
    fn 主版本号只认开头的数字() {
        assert_eq!(major_version("2.0.0"), Some(2));
        assert_eq!(major_version("2.10.3"), Some(2));
        assert_eq!(major_version("1.6.3"), Some(1));
        assert_eq!(major_version("v2.0.0"), Some(2), "带 v 前缀也要认");
        assert_eq!(major_version("未知"), None);
        assert_eq!(major_version(""), None);
    }

    /// 1.x 旧代判定：报上来的主版本 < 2 就是旧代；报不上版本号时，
    /// 只有**同时拿不出存档能力**才按旧代算 —— 2.x 一定两样都报。
    #[test]
    fn 游戏代际判定_两样都报才算新版() {
        assert!(hello_reports_legacy("1.6.3", true), "1.x 就是有存档通道也要拦：存档结构和包格式都换了");
        assert!(hello_reports_legacy("1.0.0", false));
        assert!(!hello_reports_legacy("2.0.0", true));
        assert!(!hello_reports_legacy("2.5.1", false), "2.x 报什么能力都不拦：版本号是唯一判据");
        assert!(hello_reports_legacy("未知", false), "版本号都没有、能力也缺：更老的 1.x");
        assert!(!hello_reports_legacy("未知", true), "能力齐全只是没报版本：宁可放过也不误拦同步");
    }

    #[test]
    fn hello_refresh_is_once_per_session_not_once_per_capability() {
        // session 10 已经完成过能力协商；即使能力仍然有效，session 11 的首条 hello 也必须刷新。
        let mut handled_session = Some(10);
        let capability = Some(SAVE_PROTOCOL);
        assert!(mark_first_hello_for_session(11, &mut handled_session));
        assert_eq!(capability, Some(SAVE_PROTOCOL));
        assert!(!mark_first_hello_for_session(11, &mut handled_session));

        // 下一次重连再次获得一次刷新机会，同一会话的心跳仍不会重复刷新。
        assert!(mark_first_hello_for_session(12, &mut handled_session));
        assert!(!mark_first_hello_for_session(12, &mut handled_session));
    }

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

        // 手环上装了第 1 章，所以四条都有图；没装时只标“无图片”，正文仍可读。
        assert!(rows.iter().all(|row| row.readable() && !row.missing));
        let missing = real_capture_rows(&[]);
        assert!(missing.iter().all(|row| row.missing && row.readable()), "缺资源包不等于不能读档");
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
        assert!(!parsed.slots.is_empty());
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
    fn save_protocol_is_the_documented_number() {
        // 手环侧 `save-sync.js` 里的 PROTOCOL 必须与这里一致（两边各写一份是刻意的：
        // 一个是 JS、一个是 Rust，没法共享常量），改一处就要改另一处 ——
        // tests/save-sync.test.js 里有对应的那一条守护用例。
        assert_eq!(SAVE_PROTOCOL, 1);
        // 手环的存档数组没有固定槽数上限；界面里的 20 只是列表长度的视觉建议。
        assert_eq!(amakano2_ui::MAX_SLOTS, 20);
    }

    /// 断点游标 → 分片下标的换算。挂在 crate 根（不是 wasm-only 的 host 模块）就是为了
    /// 能在宿主机上直接跑这几条 —— 「换档之后从哪一片接上」是能直接判定对错的语义。
    ///
    /// 注意游标的语义：`done_bytes` 是**已写完文件的字节数之和**，所以它只会落在文件边界上
    /// （手环不会报「半个文件」的字节数）。三条文件：A=6+4、B=5、C=3。
    #[test]
    fn resume_index_maps_bytes_to_the_current_layout() {
        let layout = [(6usize, true), (4, false), (5, true), (3, true)];
        assert_eq!(crate::resume_index_for_bytes(&layout, 0), 0, "游标 0 = 从头");
        assert_eq!(crate::resume_index_for_bytes(&layout, 10), 2, "A 写完 → B 的第一片");
        assert_eq!(crate::resume_index_for_bytes(&layout, 15), 3, "B 也写完 → C 的第一片");
        assert_eq!(crate::resume_index_for_bytes(&[], 10), 0, "空布局不 panic");
        // 结果永远落在文件边界上（`append: false` 的那一片）。
        for done in [10usize, 15] {
            let index = crate::resume_index_for_bytes(&layout, done);
            assert!(layout[index].1, "游标 {done} 的换算结果必须落在文件边界上");
        }
    }

    /// 换分片档位之后，同一个字节游标在两套布局里指向的是**同一个文件**，
    /// 但分片下标不同 —— 这正是「不能按下标续传、必须按字节游标」的依据，
    /// 也是「换档最多重传一个文件」而不是整章重来的依据。
    #[test]
    fn resume_index_follows_the_cursor_across_a_chunk_size_change() {
        // 同一个包：文件 A = 8192 字节，文件 B = 1024 字节。
        let coarse = [(8192usize, true), (1024, true)]; // 8 KB 档：A 一片、B 一片
        let fine = [(4096usize, true), (4096, false), (1024, true)]; // 4 KB 档：A 两片、B 一片
        let a = crate::resume_index_for_bytes(&coarse, 8192);
        let b = crate::resume_index_for_bytes(&fine, 8192);
        assert_eq!(a, 1, "8 KB 档下 B 从第 1 片开始");
        assert_eq!(b, 2, "4 KB 档下 B 从第 2 片开始");
        assert!(coarse[a].1 && fine[b].1, "两套布局里都落在文件边界上");
        assert_ne!(a, b, "两套布局的下标本来就不该相等 —— 这正是不能按下标续传的原因");
    }

    #[test]
    fn import_notice_counts_written_and_skipped_saves() {
        // 导入之后界面上那句话的口径：**写进去的** ＝ 信封条数 − 与手环重复的条数。
        // 重复的那些会被覆盖（不是丢），所以「跳过」这个词在这句里是准确的。
        let notice = amakano2_ui::SaveImportView { incoming: 5, duplicates: 2, existing: 3 }.notice();
        assert_eq!(notice, "导入 3 个存档，跳过 2 个重复（手环上现在有 6 条）");
    }

    /// 游标为 0 时按**半截文件的路径**定位续传起点。
    ///
    /// 断点落在整包第一个文件中间时，手环的字节游标恰好是 0（它算的是「已写完文件的
    /// 字节数」），能指的只有那个半截文件 —— 而路径与分片怎么切无关，所以在当前布局里
    /// 按它定位比按手环报的分片下标靠谱（下标只属于手环上一次那套布局）。
    /// 真机依据：2026-10-02 的 iPhone 日志，手环写了 8 KB 却回 `resumeBytes:0`。
    #[test]
    fn resume_index_falls_back_to_the_partial_file_path() {
        let layout = [
            ("images/a.png".to_string(), true),
            ("images/a.png".to_string(), false),
            ("images/b.png".to_string(), true),
            ("scripts/x.txt".to_string(), true),
        ];
        assert_eq!(crate::resume_index_for_path(&layout, "images/a.png"), Some(0), "整包第一个文件 → 第 0 片");
        assert_eq!(crate::resume_index_for_path(&layout, "images/b.png"), Some(2), "第一个文件之后的文件照样能定位");
        assert_eq!(crate::resume_index_for_path(&layout, "images/zzz.png"), None, "布局里没有这个文件 → 不猜");
        assert_eq!(crate::resume_index_for_path(&layout, ""), None, "旧版手环不给 resumePath → 不猜");
        // 只认文件的第一片：半截文件的后续片不能当起点。
        let index = crate::resume_index_for_path(&layout, "images/a.png").unwrap();
        assert!(layout[index].1, "定位结果必须落在文件边界上");
    }

    /// 自动续传预算：**一个时间窗内**最多 `MAX_AUTO_RESUME` 次，窗口一过重新给满。    ///
    /// 旧行为是「同一章一辈子 3 次」并且次数落盘 —— 真机里一次链路抖动就把 3 次烧光，
    /// 之后这一轮再也不自动接着传（界面挂着「自动续传已达上限」），用户看到的就是
    /// 「断点续传不工作」。这条用例盯的就是「窗口内仍然是闸门、窗口外能自愈」。
    #[test]
    fn auto_resume_budget_is_a_window_not_a_lifetime_cap() {
        let zero = 1_790_000_000_000u64;
        // 没有窗口（旧存档 / 刚清空）→ 立刻开窗，从第 1 次算起。
        assert_eq!(crate::auto_resume_budget(zero, 0, 0), (1, zero, true));
        // 窗口内：3 次放行，第 4 次拒绝，且窗口起点不许被推后（否则又会无限续）。
        assert_eq!(crate::auto_resume_budget(zero + 1000, zero, 0), (1, zero, true));
        assert_eq!(crate::auto_resume_budget(zero + 2000, zero, 1), (2, zero, true));
        assert_eq!(crate::auto_resume_budget(zero + 3000, zero, 2), (3, zero, true));
        assert_eq!(crate::auto_resume_budget(zero + 4000, zero, 3), (4, zero, false), "窗口内第 4 次必须被拒");
        // 窗口过期 → 计数作废、重新开窗。
        let later = zero + crate::AUTO_RESUME_WINDOW_MS;
        assert_eq!(crate::auto_resume_budget(later, zero, 3), (1, later, true), "窗口一过重新给满");
        // 落盘的次数上限被截到 MAX_AUTO_RESUME，不会因为旧存档里的脏数字放行更多次。
        assert_eq!(crate::MAX_AUTO_RESUME, 3);
        assert_eq!(crate::auto_resume_budget(zero + 1, zero, u8::MAX), (u8::MAX, zero, false), "饱和加不 panic");
    }

    /// 一次传输还算不算「在跑」（同步/继续按钮能不能点、新的一次发起要不要被挡）。
    ///
    /// 真机原话（2026-10-02）：**重新连接后点「继续传输」不传输，速度和剩余都显示「—」**。
    /// 病因是这一态没人回收 —— 应用被挂起时插件挂的定时器会丢，`transfer` 带着
    /// `started` 一直留着，界面上所有同步按钮被禁、每个入口都被一句「同步已开始」挡回去。
    #[test]
    fn transfer_liveness_expires_instead_of_hanging_forever() {
        let zero = 1_790_000_000_000u128;
        let deadline = zero + 11_000; // 3 秒 × (2 次重发 + 1) + 2 秒余量
        // 没开始过 = 不活跃（按钮可点、可以发起）。
        assert_eq!(crate::transfer_live(false, false, deadline, zero, zero), false);
        // 已发请求、还没等到确认：窗口内算在跑，超时就作废。
        assert_eq!(crate::transfer_live(true, false, deadline, zero, zero + 1_000), true, "还在等确认");
        assert_eq!(crate::transfer_live(true, false, deadline, zero, deadline), true, "正好到点仍算在跑");
        assert_eq!(crate::transfer_live(true, false, deadline, zero, deadline + 1), false, "过期 = 可以重来");
        // 没有截止时刻（不该有，但旧状态可能缺）→ 不活跃，别把用户卡死。
        assert_eq!(crate::transfer_live(true, false, 0, zero, zero), false);
        // 握手完成之后看「最近一次进展」：刚确认过 = 在跑；静默超过窗口 = 已经死了。
        let ack = zero + 100_000;
        assert_eq!(crate::transfer_live(true, true, 0, ack, ack + 1_000), true, "刚收到确认");
        assert_eq!(crate::transfer_live(true, true, 0, ack, ack + crate::TRANSFER_SILENT_MS), true, "刚好静默到窗口边界");
        assert_eq!(crate::transfer_live(true, true, 0, ack, ack + crate::TRANSFER_SILENT_MS + 1), false, "静默超窗口 = 死了");
        // 从来没有任何进展（apply_ready 没跑到）→ 不活跃。
        assert_eq!(crate::transfer_live(true, true, 0, 0, ack), false);
    }
}
