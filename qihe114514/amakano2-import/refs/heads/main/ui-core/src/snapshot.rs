//! 渲染用的只读视图模型。
//!
//! 页面只读这份快照，不直接碰 `State`：渲染期间不需要反复加锁，
//! 页面逻辑也就能脱离宿主和业务状态单独测试。

use std::sync::Arc;
use super::errors::ErrorView;

use serde_json::Value;

/// 导航页。**只有四页，按用户的任务分**，不再按数据类型平铺。
///
/// 上一版是七页（概览 / 章节 / 存档 / 统计 / 设备 / 设置 / 日志），结果是同一个连接状态
/// 在顶栏、底栏、概览主卡、设备页各播报一次；进度只在概览，而已安装清单在设备页又与章节页
/// 重复。收敛之后：
/// - **推送**：连接进度 + 章节库 + 手环已安装章节（三处合并）；
/// - **存档** / **统计**：各自一件事；
/// - **设置**：传输参数 + 行为 + 缓存 + 设备详情 + 运行日志 + 关于。
///
/// 连接与传输状态由外壳的常驻任务条（`glass::mission_bar`）统一出口，页面不再重复播报。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Push,
    Saves,
    Stats,
    /// 账号：云存档归属、QQ 绑定、AstroBox 授权。
    ///
    /// v3 从「存档页里的一张云卡片」独立成一页：它回答的是「我的存档存到哪了、
    /// 换台设备还找得回来吗」，和「手环上现在有几个存档」不是同一件事。
    Account,
    Settings,
}

pub const NAV_PREFIX: &str = "nav:";

/// 分片字节数可选项：越大越快；传输中断后不自动改档，按断点继续传。
///
/// 32 KB 档只在手环报出 `maxChunkBytes >= 32768` 时可用（3.2.0 起的应用）；
/// 旧应用会在 `hello-ok` 之后被插件自动压回 16 KB，界面上看到的是**压回之后**的值。
pub const CHUNK_OPTIONS: [usize; 3] = [8192, 16384, 32768];

/// 传输模式：**「正常」按每 4 片确认跑，「激进」几乎不确认、把消息数压到最低**。
///
/// 两者的差别不只是分片大小，而是**确认的粒度**——而确认粒度正是这条链路上
/// 最贵的东西（手环每回一条都要在它那根 JS 线程上组包、走一次 interconnect）：
///
/// | | 正常（校验） | 激进（不校验） |
/// |---|---|---|
/// | 分片 | 16 KB（单条约 22 KB） | 32 KB（单条约 43 KB） |
/// | 累计确认 | 每 4 片 / 90 ms | 写满在途 8 片 / 1.2 秒 |
/// | 飞行窗口 | 6–10 片、128 KB 在途 | 6–8 片（字节闸门定）、256 KB 在途 |
/// | 巡视节拍 | 60 ms | 25 ms |
/// | 停滞判定 | 1.5–5 秒 | 不低于 6 秒 |
/// | 丢片恢复 | `gap` 立刻补 | 粗确认报告位点 + 停滞重发 |
///
/// 两档在**同样约 13.7 MB 的全量章节**上的量级差别（按包内 334 个文件的真实切分算）：
/// 正常档 993 条数据消息 + 约 250 条确认；激进档 591 条数据消息 + 约 37 条确认。
/// 链路瓶颈是「每条消息的固定开销」而不是写盘（手环写 16 KB 实测只要 19 ms），
/// 所以消息总数就是这个档位能拿到的速度上限：**总消息数少 49%**。
///
/// 激进档**不是「不检查」**：每片都不回执，但位点每写满在途 8 片或 1.2 秒报一次，丢片仍由
/// `gap` 与停滞重发兜底，传完还有 `complete` + `pack.list` 核对。它的代价是「万一链路
/// 在这批字节里出错，要重发一整窗」。
///
/// 选哪一档**由用户定**，但插件会按手环上报的能力收紧：旧应用不认识 `turbo`，
/// 插件自动退回 `Steady`（与协议里其余能力同一条规矩：协商，不靠版本号猜）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransferMode {
    Steady,
    Turbo,
}

/// 一个模式在协议里的全部数字。**只有这一份**：插件按它发片，界面按它写说明。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeParams {
    /// 分片负载字节数（base64 之前的原始字节）。
    pub chunk_bytes: usize,
    /// 新传输的起始飞行窗口（片）。
    pub initial_window: usize,
    /// 飞行窗口上限（片）。
    pub window_cap: usize,
    /// 在途未确认的**负载字节**上限。
    pub inflight_bytes: usize,
    /// 一条消息的线上尺寸硬闸门（JSON 字节数）。
    pub wire_bytes: usize,
    /// 回执节拍：每几片一条累计确认（0 = 不按片数触发）。
    pub ack_every: usize,
    /// 回执节拍的时间上限（毫秒）。
    pub ack_ms: u64,
    /// 传输巡视的节拍（毫秒）。
    pub tick_ms: u64,
    /// 停滞判定的下限（毫秒）。粗确认档必须比确认间隔宽得多，否则会误判。
    pub stall_floor_ms: u64,
    /// 连续几次停滞才放弃自动续传。
    pub give_up_stalls: u8,
}

impl TransferMode {
    pub const ALL: [TransferMode; 2] = [TransferMode::Steady, TransferMode::Turbo];

    pub const fn wire(self) -> &'static str {
        match self {
            TransferMode::Steady => "steady",
            TransferMode::Turbo => "turbo",
        }
    }

    /// 握手/`begin` 里认的字符串。**不认识的模式一律按正常处理**：
    /// 老手环收到 `turbo` 只会用它的保守值，不认识的那一侧永远不该被放大。
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.wire() == value)
    }

    pub const fn label(self) -> &'static str {
        match self {
            TransferMode::Steady => "正常",
            TransferMode::Turbo => "激进",
        }
    }

    /// 设置页那一行说明。**必须写清代价**：激进档丢片要多重发一窗，不是白来的快。
    pub const fn hint(self) -> &'static str {
        match self {
            TransferMode::Steady => "每 4 片确认一次：丢片立刻补，最稳",
            TransferMode::Turbo => "分片翻倍、写满一窗才确认：最快，丢片要重发一整窗",
        }
    }

    pub const fn params(self) -> ModeParams {
        match self {
            TransferMode::Steady => ModeParams {
                chunk_bytes: 16384,
                initial_window: 6,
                window_cap: 10,
                inflight_bytes: 128 * 1024,
                // 16 KB → base64 21846 + JSON 约 200 = 约 22 KB。
                wire_bytes: 32 * 1024,
                ack_every: 4,
                ack_ms: 90,
                tick_ms: 60,
                stall_floor_ms: 1500,
                give_up_stalls: 6,
            },
            TransferMode::Turbo => ModeParams {
                chunk_bytes: 32768,
                initial_window: 6,
                // **8 片是字节闸门算出来的，不是拍的**：256 KB ÷ 32 KB = 8。
                // 写 16 只是好看，实际永远到不了（在途字节先撞闸），
                // 而 `ack_every` 若按 16 配就永远触发不到。
                window_cap: 8,
                inflight_bytes: 256 * 1024,
                // 32 KB → base64 43692 + JSON 约 200 = 约 43 KB。
                // 仍只有把 SAR 打崩的 78–88 KB 那一档的一半。
                wire_bytes: 48 * 1024,
                // 写满在途的 8 片就报一次位点（另有 1.2 秒的时间兜底）。
                ack_every: 8,
                ack_ms: 1200,
                tick_ms: 25,
                stall_floor_ms: 6000,
                give_up_stalls: 4,
            },
        }
    }

    /// 切模式时顺手把分片放到这一档该有的值（用户仍可以再单独调）。
    pub const fn default_chunk(self) -> usize {
        self.params().chunk_bytes
    }
}

impl Default for TransferMode {
    fn default() -> Self {
        TransferMode::Steady
    }
}

#[cfg(test)]
mod transfer_mode_tests {
    use super::*;

    /// 两档的**相对关系**是这份设计的全部意义：激进档的每一项都必须更宽松，
    /// 否则「切过去更快」就是假的，用户白点一次。
    ///
    /// 窗口按**片数**比没有意义（分片本来就变大了）：真正决定吞吐的是**在途字节**
    /// 与「每条数据消息摊到多少确认」。所以窗口这里比的是在途字节与字节闸门算出的片数。
    #[test]
    fn turbo_is_strictly_looser_than_steady() {
        let steady = TransferMode::Steady.params();
        let turbo = TransferMode::Turbo.params();
        assert!(turbo.chunk_bytes > steady.chunk_bytes, "分片要更大");
        assert!(turbo.inflight_bytes > steady.inflight_bytes, "在途字节要更多");
        assert!(turbo.ack_every > steady.ack_every, "确认要更稀：这才是快的主因");
        assert!(turbo.ack_ms > steady.ack_ms, "确认的时间上限也要放宽");
        assert!(turbo.tick_ms < steady.tick_ms, "巡视要更勤，好把窗口填满");
        assert!(turbo.stall_floor_ms > steady.stall_floor_ms, "确认稀了，停滞判定必须跟着放宽");
        // 「每一片数据要配多少条确认」——这才是用户感受到的快慢。
        let steady_cost = 1.0 / steady.ack_every as f64;
        let turbo_cost = 1.0 / turbo.ack_every as f64;
        assert!(
            turbo_cost < steady_cost,
            "激进档每条数据摊到的确认必须更少（{turbo_cost:.3} vs {steady_cost:.3}）"
        );
        // 起始窗口不许超过上限（否则第一拍就按超限的片数发）。
        for mode in TransferMode::ALL {
            let params = mode.params();
            assert!(
                params.initial_window <= params.window_cap,
                "{mode:?}：起始窗口 {} 不能超过上限 {}",
                params.initial_window,
                params.window_cap
            );
        }
    }

    /// 线上尺寸是**硬闸门**，不是性能参数：单条消息一旦越过 SAR 崩过的那一档，
    /// 用户看到的是「连不上」而不是「慢」。两档都得留足余量（32 KB 分片的 base64 约 43 KB）。
    #[test]
    fn wire_limit_leaves_room_but_stays_under_the_crash_size() {
        for mode in TransferMode::ALL {
            let params = mode.params();
            let encoded = params.chunk_bytes.div_ceil(3) * 4;
            assert!(
                params.wire_bytes > encoded,
                "{mode:?}：上限 {} 装不下 {} 字节分片的 base64（{encoded}）",
                params.wire_bytes,
                params.chunk_bytes
            );
            // 2026-10-07 打崩 SAR 的是 78–88 KB 的批量消息；单条始终要明显低于它。
            assert!(params.wire_bytes <= 64 * 1024, "{mode:?}：单条上限不许逼近 SAR 崩过的尺寸");
        }
    }

    /// 停止灯：确认的**片数门槛不能超过实际在途片数**。
    ///
    /// 在途片数不是 `window_cap`，而是 `min(window_cap, inflight_bytes / chunk_bytes)` ——
    /// 字节闸门往往先到（激进档 256 KB ÷ 32 KB = 8 片，而上限写的是 16）。
    /// 门槛高于真在途数的话那行配置永远不会触发，实际全靠时间节拍：
    /// 读代码的人以为「每 N 片确认一次」在发生，调这个数的人也以为自己在调确认频率。
    /// 这是一条**静默失效**的配置，必须钉住。
    #[test]
    fn ack_every_never_exceeds_the_effective_window() {
        for mode in TransferMode::ALL {
            let params = mode.params();
            let by_bytes = params.inflight_bytes / params.chunk_bytes.max(1);
            let effective = params.window_cap.min(by_bytes.max(1));
            assert!(
                params.ack_every > 0 && params.ack_every <= effective,
                "{mode:?}：确认门槛 {} 超过了实际在途片数 {}（window_cap {} / 字节闸门 {} 片），\
                 那行数字永远触发不到",
                params.ack_every,
                effective,
                params.window_cap,
                by_bytes
            );
        }
    }

    /// 停滞判定必须宽于「一次确认的间隔」，否则会把自己的确认节拍判成停滞：
    /// 激进档每 1.2 秒才报一次位点，判定下限却只有 1.5 秒的话，一次抖动就整窗重发。
    #[test]
    fn stall_floor_comfortably_exceeds_the_ack_interval() {
        for mode in TransferMode::ALL {
            let params = mode.params();
            assert!(
                params.stall_floor_ms >= params.ack_ms * 3,
                "{mode:?}：停滞下限 {}ms 至少该是确认间隔 {}ms 的 3 倍",
                params.stall_floor_ms,
                params.ack_ms
            );
        }
    }

    /// 模式名是协议字符串：拼写一改，手环就认不出来（然后静默退回上一档）。
    #[test]
    fn mode_wire_names_round_trip() {
        for mode in TransferMode::ALL {
            assert_eq!(TransferMode::parse(mode.wire()), Some(mode));
        }
        assert_eq!(TransferMode::parse(""), None);
        assert_eq!(TransferMode::parse("Steady"), None, "大小写不宽容：协议里是固定小写");
        assert_eq!(TransferMode::default(), TransferMode::Steady, "默认必须是稳的那一档");
    }

    /// 默认分片要真的落在可选档位里 —— 否则设置页会出现一个「没有任何按钮高亮」的状态。
    #[test]
    fn every_mode_default_chunk_is_a_selectable_option() {
        for mode in TransferMode::ALL {
            assert!(
                CHUNK_OPTIONS.contains(&mode.default_chunk()),
                "{mode:?} 的默认分片 {} 不在 CHUNK_OPTIONS 里",
                mode.default_chunk()
            );
        }
    }
}

/// 手环端保留的手动存档上限（手环 `saves.ux` 里没有硬上限，这个数只用于界面提示）。
pub const MAX_SLOTS: usize = 20;

/// 存档相关操作失败的原因。**全部是给人看的中文原文**，直接可以贴到界面上。
///
/// 定义在 `ui-core` 而不是插件侧：信封解包（[`crate::saves`]）与剪贴板导入
/// （插件的 `host/saves.rs`）用的是同一套失败口径，放两处迟早会说成两句不一样的话。
/// 插件侧 `src/host/saves.rs` 现在直接 `pub use amakano2_ui::SaveError`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveError {
    message: String,
}

impl SaveError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

pub fn chunk_label(bytes: usize) -> String {
    format!("{} KB", bytes / 1024)
}

impl Page {
    pub const ALL: [Page; 5] = [Page::Push, Page::Saves, Page::Stats, Page::Account, Page::Settings];

    pub const fn wire(self) -> &'static str {
        match self {
            Page::Push => "push",
            Page::Saves => "saves",
            Page::Stats => "stats",
            Page::Account => "account",
            Page::Settings => "settings",
        }
    }

    /// 导航标签。**一律两个字**：标签一长，四项导航在 400px 窗口里就装不下。
    pub const fn label(self) -> &'static str {
        match self {
            Page::Push => "推送",
            Page::Saves => "存档",
            Page::Stats => "统计",
            Page::Account => "账号",
            Page::Settings => "设置",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|page| page.wire() == value)
    }

    /// 点击导航项时派发的动作 id。
    pub fn action(self) -> String {
        format!("{NAV_PREFIX}{}", self.wire())
    }
}

/// 连接走到了哪一步。
///
/// 为什么要把「一条状态行」拆成阶段：旧实现里 **`request_pack_list` 只在 `alive == false`
/// 的分支里发**，而 `hello-ok` 一回来就把 `alive` 置真、顺手把探测循环终止掉 ——
/// 于是「通道通了」被当成了「数据就绪了」，章节列表永远拿不到。
/// 那是个**概念混淆**的 bug，不是某个判断写错；把它拆成显式阶段之后，
/// 「已握手」和「已拿到章节列表」是两件事，代码里再也没法把它们混为一谈。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum SessionStage {
    /// 还没点「连接设备」。
    Idle,
    /// 在宿主里找到了在线设备。
    DeviceFound,
    /// 回包通道注册成功（`register_interconnect_recv`）。
    ChannelRegistered,
    /// 手环应用回应过任何一条消息 —— 说明它在前台活着。
    AppAlive,
    /// 能力协商完成（收到 `hello-ok`）。
    Handshaked,
    /// **已安装章节列表已拿到** —— 这是独立的里程碑，不是「握手顺带」。
    CatalogReady,
    /// 正在传（或刚传完一段、断点还在）。
    Transferring,
}

impl SessionStage {
    pub const ALL: [SessionStage; 7] = [
        SessionStage::Idle,
        SessionStage::DeviceFound,
        SessionStage::ChannelRegistered,
        SessionStage::AppAlive,
        SessionStage::Handshaked,
        SessionStage::CatalogReady,
        SessionStage::Transferring,
    ];

    /// 第几步（从 0 数）。界面上的分段条按它点亮。
    pub fn index(self) -> usize {
        SessionStage::ALL.iter().position(|stage| *stage == self).unwrap_or(0)
    }

    pub fn label(self) -> &'static str {
        match self {
            SessionStage::Idle => "未连接",
            SessionStage::DeviceFound => "已找到设备",
            SessionStage::ChannelRegistered => "回包通道已就绪",
            SessionStage::AppAlive => "手环应用已响应",
            SessionStage::Handshaked => "能力协商完成",
            SessionStage::CatalogReady => "章节列表已同步",
            SessionStage::Transferring => "章节同步中",
        }
    }

    /// 这一阶段的一条短说明（「还没走到这一步」时显示，告诉用户卡在哪）。
    pub fn pending_hint(self) -> &'static str {
        match self {
            SessionStage::Idle => "点上面的「连接设备」开始",
            SessionStage::DeviceFound => "正在注册回包通道…",
            SessionStage::ChannelRegistered => "正在等手环应用响应（没反应就在手环上打开《甜蜜女友2》）",
            SessionStage::AppAlive => "正在协商版本与能力…",
            SessionStage::Handshaked => "正在读取手环上已安装的章节…",
            SessionStage::CatalogReady => "可以点某一章的「同步」了",
            SessionStage::Transferring => "传输中，请把手环停在《甜蜜女友2》页面",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatusKind {
    Info,
    Good,
    Warn,
    Bad,
}

impl StatusKind {
    pub const fn color(self) -> &'static str {
        match self {
            StatusKind::Info => super::theme::TEXT_SUB,
            StatusKind::Good => super::theme::OK,
            StatusKind::Warn => super::theme::WARN,
            StatusKind::Bad => super::theme::BAD,
        }
    }

    pub const fn bg(self) -> &'static str {
        match self {
            StatusKind::Good => super::theme::OK_BG,
            StatusKind::Warn => super::theme::WARN_BG,
            StatusKind::Bad => super::theme::BAD_BG,
            StatusKind::Info => super::theme::INFO_BG,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            StatusKind::Info => "进行中",
            StatusKind::Good => "正常",
            StatusKind::Warn => "注意",
            StatusKind::Bad => "错误",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    /// 从 tracing 输出的一行里嗅探级别。
    pub fn sniff(line: &str) -> Self {
        if line.contains("ERROR") {
            Self::Error
        } else if line.contains("WARN") {
            Self::Warn
        } else if line.contains("DEBUG") || line.contains("TRACE") {
            Self::Debug
        } else {
            Self::Info
        }
    }

    pub const fn color(self) -> &'static str {
        match self {
            Self::Error => super::theme::BAD,
            Self::Warn => super::theme::WARN,
            Self::Info => super::theme::TEXT_SUB,
            Self::Debug => super::theme::TEXT_FAINT,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Error => "ERR",
            Self::Warn => "WRN",
            Self::Info => "INF",
            Self::Debug => "DBG",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogFilter {
    All,
    Warn,
    Error,
}

impl LogFilter {
    pub const ALL: [LogFilter; 3] = [LogFilter::All, LogFilter::Warn, LogFilter::Error];

    pub const fn wire(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "全部",
            Self::Warn => "警告",
            Self::Error => "错误",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|filter| filter.wire() == value)
    }

    pub fn accepts(self, level: LogLevel) -> bool {
        match self {
            Self::All => true,
            Self::Warn => matches!(level, LogLevel::Warn | LogLevel::Error),
            Self::Error => matches!(level, LogLevel::Error),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LogLine {
    pub level: LogLevel,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeviceView {
    pub name: String,
    pub addr: String,
    pub connected: bool,
    /// 手环应用已经回过消息（《甜蜜女友2》确实在前台运行）。
    pub alive: bool,
    /// 探测尝试次数，用于显示「正在等待应用回应（第 n/12 次）」。
    pub probe_attempt: u32,
    pub probe_max: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PackView {
    pub number: usize,
    pub id: String,
    pub title: String,
    pub minutes: usize,
    pub bytes: usize,
    pub scenes: usize,
    pub dialogues: usize,
    pub installed: bool,
    /// 已排队等待同步。
    pub queued: bool,
    /// 正在传输的就是这一章。
    pub active: bool,
}

impl PackView {
    /// 阅读时长的人话写法。
    ///
    /// **必须带「阅读」字样**：不带的话，用户把「约 54 分钟」理解成「推送要花 54 分钟」
    /// （真有人这么理解过），这个数字是这一章全文的阅读时长，与传输耗时无关。
    pub fn minutes_label(&self) -> String {
        if self.minutes >= 60 {
            let hours = self.minutes / 60;
            let rest = self.minutes % 60;
            if rest == 0 {
                format!("阅读约 {hours} 小时")
            } else {
                format!("阅读约 {hours} 小时 {rest} 分")
            }
        } else {
            format!("阅读约 {} 分钟", self.minutes)
        }
    }

    /// 章节行下面那一行元信息。
    ///
    /// **一行说完**：上一版分两行（「阅读约 54 分钟 · 736 KB · 41 幕」+「共通线 · 1066 句对白」），
    /// 15 行列表就是 30 行小字。线路名不进这一行 —— 行首的序号砖已经用颜色在标线路了。
    pub fn meta_line(&self) -> String {
        // 列表行只放**两个数字**：阅读时长与体积。幕数 / 句数在窄窗里会折成两三行，
        // 而且它们对「要不要现在同步」这个决定没有帮助。
        format!("{} · {}", self.minutes_label(), super::human_bytes(self.bytes))
    }

    /// 是否属于当前线路筛选。
    pub fn in_line(&self, filter: Option<&str>) -> bool {
        filter.is_none_or(|line| super::theme::chapter_line(&self.title).0 == line)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct InstalledView {
    pub id: String,
    pub name: String,
    pub bytes: usize,
    pub files: usize,
    /// 不在当前插件的章节表里（旧版本残留）。
    pub stale: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransferView {
    pub chapter: String,
    pub percent: u32,
    pub received: usize,
    pub total: usize,
    pub chunks_done: usize,
    pub chunks_total: usize,
    pub speed_kbps: f64,
    pub rtt_ms: u64,
    pub retries: u8,
    pub resumed: bool,
    /// 手环已确认（握手完成，正在收分片）。`false` = 还在「已发请求、等手环确认」。
    pub ready: bool,
    pub started: bool,
    /// 这次传输已经不再活跃（插件侧 `transfer_live` 为假）：等确认过期了，或者握手完成后
    /// 长时间没有任何进展。
    ///
    /// 这一态必须能被界面看见：它就是真机上「点继续传输不传输、速度与剩余全是「—」」的那个
    /// 状态 —— 旧的 `is_transferring()`（`started && !ready`）会把所有同步按钮永久禁掉。
    /// 判定为不活跃之后按钮要放回来、卡片要让位给可点的出口（断点卡/章节行）。
    pub stalled: bool,
    pub chunk_bytes: usize,
}

impl TransferView {
    /// 剩余时间（秒）。速度太低或已经传完时返回 `None`。
    pub fn eta_seconds(&self) -> Option<u64> {
        if self.speed_kbps <= 0.5 || self.received >= self.total {
            return None;
        }
        let remaining_kb = (self.total - self.received) as f64 / 1024.0;
        Some((remaining_kb / self.speed_kbps).round() as u64)
    }

    pub fn eta_label(&self) -> String {
        match self.eta_seconds() {
            Some(seconds) if seconds >= 60 => format!("约 {} 分 {} 秒", seconds / 60, seconds % 60),
            Some(seconds) => format!("约 {seconds} 秒"),
            None => "—".into(),
        }
    }

    pub fn speed_label(&self) -> String {
        if self.speed_kbps <= 0.0 {
            "—".into()
        } else if self.speed_kbps >= 1024.0 {
            format!("{:.2} MB/s", self.speed_kbps / 1024.0)
        } else {
            format!("{:.1} KB/s", self.speed_kbps)
        }
    }

    /// 发生过重试，说明链路不稳。
    pub fn shaky(&self) -> bool {
        self.retries > 0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResumeView {
    pub pack_id: String,
    pub chapter: String,
    pub percent: u32,
    pub received: usize,
    pub total: usize,
    pub files_done: usize,
    pub resume_from: usize,
}

/// 手环上的一条存档，按「槽」看。
///
/// `slot = None` 表示自动存档（断点续读，手环首页「继续阅读」读的就是它）；
/// `slot = Some(index)` 就是 `recoveryData` 里的下标 —— 手环侧删除用的就是这个数。
///
/// `chapter` / `scene` 只是给人看的，**读档真正靠的是 `pack_id` + `pack_scene`**
/// （包内偏移，抗章节增减），所以这两个值原样展示、绝不改写。
#[derive(Clone, Debug, PartialEq)]
pub struct SaveSlotView {
    pub slot: Option<usize>,
    pub chapter: String,
    pub scene: usize,
    pub saved_at: u64,
    pub pack_id: String,
    pub pack_scene: usize,
    /// 存档记的章节资源包没装在手环上 —— 只影响图片，正文仍可读。
    pub missing: bool,
    /// 这条存档是云同步为避免覆盖而保留的冲突副本（存档对象里带 `cloudConflict`）。
    pub conflict: bool,
}

/// 这一章的身份（存档里的 `packId`）对应的资源包装没装。
fn pack_available(pack_id: &str, installed: &[String]) -> bool {
    installed.iter().any(|id| {
        id == pack_id
            || (!id.is_empty()
                && pack_id.starts_with(id.as_str())
                && pack_id[id.len()..]
                    .chars()
                    .next()
                    .is_some_and(|next| next.is_ascii_digit() || next == '·'))
    })
}

impl SaveSlotView {
    /// 从手环的存档对象（`serde_json::Value`）拍出一条槽位。
    ///
    /// **存档对象原样持有在插件侧**（导出要逐字带走），这里只**读**几个展示字段：
    /// - `chapter` / `currentScene` 只用于展示，**绝不改写**：读档真正靠
    ///   `packId` + `packScene`（包内偏移，抗章节增减）；
    /// - `missing` 由「`packId` 在不在手环已安装列表里」决定。
    ///
    /// 预览样本（`demo()`）与真机走的是同一个函数，所以截图里的槽位和手环上看到的一致。
    pub fn from_json(value: &serde_json::Value, slot: Option<usize>, installed: &[String]) -> Self {
        let text = |key: &str| value.get(key).and_then(serde_json::Value::as_str).unwrap_or("").to_string();
        let number = |key: &str| {
            value
                .get(key)
                .and_then(serde_json::Value::as_i64)
                .filter(|value| *value >= 0)
                .unwrap_or(0) as usize
        };
        let pack_id = text("packId");
        let chapter = {
            let title = text("chapterTitle");
            if !title.is_empty() {
                title
            } else {
                match value.get("chapter").and_then(serde_json::Value::as_u64) {
                    Some(number) => format!("第{}章", number + 1),
                    None => "未知章节".into(),
                }
            }
        };
        Self {
            slot,
            chapter,
            // 手环侧的场景号也是 `currentScene + 1`，两边保持一致。
            scene: number("currentScene") + 1,
            saved_at: number("savedAt") as u64,
            // 2.0 起资源包按线路合并：存档里的 `packId` 是**章节身份**（「共通线1·归乡」），
            // 手环注册表里装的却是线路包（「共通线」）。只做全等比较会把所有存档标成「未安装」——
            // 2026-09 真机就是这么报上来的。判据：全等，或者已装的是它所属线路包的前缀
            // 且分界正好落在数字 / 「·」上（避免「玲线」误配「玲线外传」这类假设性名字）。
            missing: !pack_id.is_empty() && !pack_available(&pack_id, installed),
            conflict: value.get("cloudConflict").is_some_and(|metadata| metadata.is_object()),
            pack_id,
            pack_scene: number("packScene"),
        }
    }

    /// 批量：手环回包里的手动槽（下标即槽号）。
    pub fn rows(slots: &[serde_json::Value], installed: &[String]) -> Vec<Self> {
        slots
            .iter()
            .enumerate()
            .map(|(index, value)| Self::from_json(value, Some(index), installed))
            .collect()
    }

    pub fn is_auto(&self) -> bool {
        self.slot.is_none()
    }

    /// 槽位标题：自动存档就叫「自动存档」，手动槽带槽号。
    pub fn title(&self) -> String {
        match self.slot {
            Some(index) => format!("存档 {}", index + 1),
            None => "自动存档".into(),
        }
    }

    pub fn scene_label(&self) -> String {
        format!("场景 {}", self.scene)
    }

    /// 时间：`MM-DD HH:MM`；拿不到就说明白。
    pub fn time_label(&self) -> String {
        super::theme::format_time_ms(self.saved_at)
    }

    /// 这一条能不能读档。
    ///
    /// **始终为 true**：剧情正文随手环应用内置，章节资源包只影响图片。
    /// 手环端 `saves.ux` 已明确「没装包也能读，只是没有图片，阅读页走纯文本」，
    /// 插件不能拿资源包状态把读档整条路堵死。
    pub fn readable(&self) -> bool {
        true
    }
}

/// 一份存档行（[`Snapshot::saves`] 的产物）里的几件「找一条 / 数一数」。
///
/// **只在这里实现一份**：页面往往在渲染时就拿到了 `rows`，直接用它们即可，
/// 不必为了一个计数把整份存档行再算一遍（那还会让两处各看一眼不同的输入）。
pub fn auto_save_row(rows: &[SaveSlotView]) -> Option<&SaveSlotView> {
    rows.iter().find(|save| save.is_auto())
}

/// 手动存档条数（不算自动存档）。
pub fn manual_save_count(rows: &[SaveSlotView]) -> usize {
    rows.iter().filter(|save| !save.is_auto()).count()
}

/// 存档所在章节的资源没同步的条数 —— 只影响图片，不影响读档。
pub fn missing_save_count(rows: &[SaveSlotView]) -> usize {
    rows.iter().filter(|save| save.missing).count()
}

/// 上一次导出的结果：剪贴板里现在有什么、多大。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveExportView {
    /// 信封字节数（= 剪贴板里那串文本的长度）。
    pub bytes: usize,
    /// 槽位条数（手动 + 自动），给人一句「复制了几个存档」。
    pub slots: usize,
    /// 写完之后有没有**读回核对**成功。核对不了不影响导出成功，但界面要照实说。
    pub verified: bool,
}

/// 上一次导入的结果：写进去几条、跳过几条重复、手环上现在共几条。
///
/// 「哪些导入的存档所在章节还没装」不在这里 —— 那是存档列表自己的事
/// （[`Snapshot::saves`] 每帧都按**当前**已安装章节列表把没装的标出来），不重复第二份。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveImportView {
    /// 剪贴板里那封信封的槽位条数。
    pub incoming: usize,
    /// 其中手环上已经有同一份（`savedAt` 相同）的条数 —— 这些会被覆盖，算「跳过重复」。
    pub duplicates: usize,
    /// 合并前手环上的手动槽条数。
    pub existing: usize,
}

impl SaveImportView {
    /// 一句话说清这次导入做了什么。**条目数只报手环真的变了的那部分**：
    /// 手环侧的 `upsert` 按 `savedAt` 合并，重复的那些是覆盖旧值，不该算成「新写进去的」。
    pub fn notice(&self) -> String {
        let written = self.incoming.saturating_sub(self.duplicates);
        format!(
            "导入 {} 个存档，跳过 {} 个重复（手环上现在有 {} 条）",
            written,
            self.duplicates,
            self.existing + written
        )
    }
}

/// 最近某一天的阅读明细（手环侧已经把「秒 → 几小时几分」算好了）。
#[derive(Clone, Debug, PartialEq)]
pub struct RecentDayView {
    pub date: String,
    pub label: String,
    pub seconds: u64,
}

/// 手环上的阅读统计。
///
/// **所有展示文案都由手环侧生成**（`labels` / `RecentDayView::label`）：
/// 插件只负责把字符串摆到界面上，绝不自己再算一遍「秒 → 几小时几分」或
/// 「连续几天」—— 两边各算一遍迟早会不一致，而那种不一致在界面上看不出对错。
/// 这里只保留最小的数值字段（供状态行和「有没有数据」的判断用）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReadingStatsView {
    /// 手环统计当天的日期（`YYYY-MM-DD`）。
    pub today: String,
    pub total_day_count: u64,
    pub longest_date: String,
    pub reading_days_label: String,
    pub total_days_label: String,
    pub total_label: String,
    pub today_label: String,
    pub longest_label: String,
    /// 最近 N 天，日期升序（手环侧只带最近 30 天）。
    pub recent: Vec<RecentDayView>,
}

impl ReadingStatsView {
    /// 从手环回包（**已解包**的载荷）读出来。
    ///
    /// 缺字段 / 类型不对一律返回 [`SaveError`]（可读中文），**不静默变 0** ——
    /// 「手环上真没读过」与「插件没读懂回包」在界面上必须能分清
    /// （2026-09 存档那次「永远 0 条」就是静默解析失败，见 `docs/插件开发注意事项.md` 7.8）。
    pub fn from_json(value: &Value) -> Result<Self, SaveError> {
        let object = value
            .as_object()
            .ok_or_else(|| SaveError::new("手环回包格式不对：阅读统计的载荷不是 JSON 对象"))?;
        let text = |key: &str| -> Result<String, SaveError> {
            object
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| SaveError::new(format!("手环回包缺字段：{key}")))
        };
        let labels = object
            .get("labels")
            .and_then(Value::as_object)
            .ok_or_else(|| SaveError::new("手环回包缺字段：labels（界面文案由手环侧生成）"))?;
        let label_of = |key: &str| -> Result<String, SaveError> {
            labels
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| SaveError::new(format!("手环回包缺字段：labels.{key}")))
        };
        let recent = match object.get("recent") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|item| {
                    let item = item.as_object()?;
                    Some(RecentDayView {
                        date: item.get("date").and_then(Value::as_str).unwrap_or("").to_string(),
                        label: item.get("label").and_then(Value::as_str).unwrap_or("").to_string(),
                        seconds: item.get("seconds").and_then(Value::as_u64).unwrap_or(0),
                    })
                })
                .collect(),
            Some(other) => {
                return Err(SaveError::new(format!(
                    "手环回包格式不对：recent 应该是数组，实际是{}",
                    crate::saves::json_kind(other)
                )));
            }
        };
        Ok(Self {
            today: text("today")?,
            total_day_count: object.get("totalDayCount").and_then(Value::as_u64).unwrap_or(0),
            longest_date: object.get("longestDate").and_then(Value::as_str).unwrap_or("").to_string(),
            reading_days_label: label_of("readingDays")?,
            total_days_label: label_of("totalDays")?,
            total_label: label_of("total")?,
            today_label: label_of("today")?,
            longest_label: label_of("longest")?,
            recent,
        })
    }

    /// 手环上到底有没有阅读记录（读了才有下文）。
    pub fn has_data(&self) -> bool {
        self.total_day_count > 0
    }

    /// 统计页「总阅读时长」那一行的副标题。
    pub fn head_line(&self) -> String {
        if !self.has_data() {
            return "手环上还没有阅读记录".into();
        }
        let date = if self.today.is_empty() { "统计日期未知".to_string() } else { format!("统计至 {}", self.today) };
        format!("累计 {} 天 · {date}", self.total_day_count)
    }
}

/// 协议侧的固定参数。设置页要如实显示，所以当成数据传进来，而不是在页面里写死第二份。
#[derive(Clone, Debug, PartialEq)]
pub struct Limits {
    /// 轻量请求（章节列表 / 未完成传输 / 删除）的首包超时。
    pub request_timeout_ms: u64,
    /// 存档请求的首包超时 —— **比轻量类宽**，手环端读两次 storage 再拼 6KB 分片回包，
    /// 实测长尾到 5.5–24.7s，而章节列表最长 1.5s。理由写在 `src/request.rs`。
    pub saves_timeout_ms: u64,
    pub retry_delay_ms: u64,
    pub max_retries: u8,
    pub initial_window: usize,
    pub probe_interval_ms: u64,
    pub probe_attempts: u32,
    pub max_pack_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        // 与插件里的常量一致；插件侧构造快照时会显式覆盖，这里只是给预览/测试一份样本。
        Self {
            request_timeout_ms: 1000,
            saves_timeout_ms: 2000,
            retry_delay_ms: 120,
            max_retries: 5,
            initial_window: 6,
            probe_interval_ms: 5000,
            probe_attempts: 12,
            max_pack_bytes: 20_000_000,
        }
    }
}

impl Limits {
    pub fn window_label(&self) -> String {
        format!("{} 个分片", self.initial_window)
    }

    pub fn probe_label(&self) -> String {
        // v3 的连接探测没有「次数上限」：前 30 秒 2.5 秒一次，之后 6 秒一次，永远重试。
        format!("{} ms 心跳 · 自动重连", self.probe_interval_ms)
    }
}

/// 一页需要的全部数据。
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub page: Page,
    pub version: String,
    /// 连接走到了哪一步。推送页的分段进度条按它点亮。
    pub stage: SessionStage,
    /// 最近一次失败（码 + 细节原文）。`None` 表示当前没有失败要讲。
    pub error: Option<ErrorView>,
    pub device: DeviceView,
    pub library: Vec<PackView>,
    pub library_error: String,
    pub installed: Vec<InstalledView>,
    /// 手环注册表里有、但 `pack.txt` 读不出来的章节包名。
    ///
    /// **不许静默**：「手环上真没装这一章」和「注册表里那条读不出来」是两件事，
    /// 后者要么重传、要么清理注册表，用户得看得见才能决定。
    pub installed_broken: Vec<String>,
    pub transfer: Option<TransferView>,
    pub resume: Option<ResumeView>,
    /// 重连后手环应用已就绪、但断点回包还没到：界面在这段盲区里显示
    /// 「正在查询手环上的断点…」，而不是什么都不说。
    pub resume_checking: bool,
    pub cache_bytes: usize,
    pub cache_files: usize,
    pub chunk_bytes: usize,
    /// 用户选的传输模式（正常 / 激进）。手环能力不够时插件会把它压回 `Steady`，
    /// 所以这里显示的是**实际生效**的那一档。
    pub transfer_mode: TransferMode,
    /// 手环不接受激进档的原因（空串 = 没有限制）。界面据此解释「为什么点了激进还是正常跑」。
    pub turbo_blocked: String,
    /// 待同步的章节号（先进先出）。
    pub queue: Vec<usize>,
    pub status: String,
    pub status_kind: StatusKind,
    pub auto_launch: bool,
    /// 主面板当前显示的是**原生界面**（宿主 ui-v3）而不是引导页。
    ///
    /// 默认 `false`：主面板就是引导页，点一下打开 HTML 控制面板。
    /// 只有用户为了「WebView 打不开」而点了后备入口才会变成 `true`。
    /// 这个开关**不落盘** —— 重启插件后回到默认的引导页，
    /// 免得用户某次临时切过去之后就再也找不到「打开 HTML 面板」。
    pub native_ui: bool,
    /// 悬停中的元素 id；`None` 表示没有。
    pub hover: Option<String>,
    /// 按下中的按钮动作 id；`None` 表示没有。
    pub pressed: Option<String>,
    /// 插件图标（`icon.png`，已内联成 `data:` URI），给顶栏品牌位显示。
    /// `IMAGE` 元素的内容就是这个值；拿不到时为 `None`，界面回落到品牌色块。
    pub brand: Option<Arc<str>>,
    /// 手环上的存档**原始对象**：自动存档（没有时 `None`）+ 手动槽（下标即槽号）。
    ///
    /// ⚠️ 这里刻意**只放原始数据**，不放「算好的界面行」：像「未安装」这样的派生判定
    /// 必须**渲染时用当前输入重算**（见 [`Snapshot::saves`]）。算好存起来的那一版，
    /// 章节列表后来变了也不会跟着变 —— 2026-09 真机就是四条存档全标「未安装」、
    /// 点「刷新」也不变（见 `docs/插件开发注意事项.md` 7.9）。
    /// 导出信封也直接用这两份原样对象，所以它们本来就得在。
    pub save_auto: Option<Value>,
    /// 手动槽原文（下标即槽号）。
    pub save_slots: Vec<Value>,
    /// 存档通道的失败原因（读不出来、手环应用太旧、导入文件不对…），空串表示没问题。
    pub saves_error: String,
    /// 等待二次确认删除的存档槽；`None` 表示没有。自动存档用 `"auto"`。
    pub confirm_delete_save: Option<String>,
    /// 与手环协商到的存档协议版本；`None` 表示**还没收到 `amakano.app.hello-ok`**
    /// —— 界面上要给出「手环端应用版本过旧」这句人话，不能只说超时。
    pub save_protocol: Option<u32>,
    /// 手环端应用**不支持**存档通道（没回 `hello-ok`，或回了但能力/协议不够用）。
    ///
    /// 这只是个**状态位**：那句「手环端应用版本过旧」的结论与随后那句「怎么办」统一由界面给
    /// （[`Snapshot::saves_blocked_hint`] / [`Snapshot::saves_blocked_action`]），
    /// 插件侧**不再**往 `saves_error` 里写第二句同义的话 —— 用户实机截图里那张卡
    /// 就是同一个事实说了两遍（一红一黄），别再走回老路。
    pub saves_unsupported: bool,
    /// 存档操作进行中（对话框开着或正在等回包）：期间要禁用相关按钮，
    /// 免得用户连点两次弹出两个保存对话框。
    pub saves_busy: bool,
    /// 手环快应用版本（来自 hello-ok），用于「版本过旧」的提示。
    pub band_version: String,
    /// 手环上的游戏是 **1.x 旧代**（主版本号 < 2，或活着却协商不出协议 —— 连 hello 都没有）。
    ///
    /// 判定在插件侧落定（hello-ok 解析 / 存档请求超时归因），这里只负责把结论与
    /// 「怎么办」用 [`Snapshot::band_game_legacy_hint`] / [`Snapshot::band_game_legacy_action`]
    /// 说出来；推送页挂卡、禁用同步，存档页挂卡、禁用导入。
    pub band_game_legacy: bool,
    /// 存档页顶部的操作结果（导出/导入/读档/删除各一句）。
    pub saves_notice: String,
    /// 上一次「导出到剪贴板」的结果；`None` 表示这一轮还没导出过。
    /// 导出成功要显示**信封字节数**（用户靠它判断剪贴板里是不是完整的一份）。
    pub save_export: Option<SaveExportView>,
    /// 上一次「从剪贴板导入」的结果；`None` 表示还没导入过。
    pub save_import: Option<SaveImportView>,
    /// 手环交给插件的“当前游戏进度”快照。存在时，存档页把每个槽都变成可点的
    /// 写入目标：空槽显示“保存到这里”，已有槽显示“覆盖”。
    pub pending_current_save: Option<SaveSlotView>,
    pub cloud_phase: String,
    pub cloud_detail: String,
    /// 已授权并绑定后，主账号名是 QQ 昵称；空串表示还没绑定。
    pub cloud_qq_name: String,
    /// 完整 QQ 号。只在已绑定且服务端返回时填入，空串表示未绑定。
    pub cloud_qq: String,
    /// AstroBox 关联账号名（只做关联，不冒充主账号）。
    pub cloud_account_name: String,
    pub cloud_code: String,
    /// 绑定码到期时刻（ISO 8601 UTC），由服务端返回。
    pub cloud_expires_at: String,
    /// 账号授权状态：`idle` / `authorizing` / `authorized` / `denied` / `error`。
    pub account_auth: String,
    /// 在线设备列表（顶栏设备选择器用；空表示没查到）。
    pub devices: Vec<DeviceView>,
    /// 当前选中的设备地址；不在列表里时为空。
    pub selected_device: String,
    /// 官方源最新版本（检查更新后才有值）。
    pub update_latest: String,
    /// 当前运行版本是否落后于官方源。
    pub update_available: bool,
    /// 更新检查的一句话结果（正在检查 / 已是最新 / 有新版本 / 失败）。
    pub update_status: String,
    /// 插件项目官网。
    pub website: String,
    /// 手环上的阅读统计。`None` 表示这一轮还没读到（还没连上 / 还没点刷新）。
    ///
    /// 与存档同一口径：**原始回包在插件侧解**（`ReadingStatsView::from_json`），
    /// 这里只持有解好的视图，不在渲染路径上重算任何结论。
    pub stats: Option<ReadingStatsView>,
    /// 阅读统计通道的失败原因（回包读不懂 / 手环没响应），空串表示没问题。
    pub stats_error: String,
    /// 手环端应用的能力表里有 `stats`（`amakano.app.hello-ok` 里报的）。
    pub stats_supported: bool,
    /// 正在读统计：期间禁用刷新按钮，免得用户连点两次。
    pub stats_busy: bool,
    /// 线路筛选；`None` 表示全部。
    pub line_filter: Option<String>,
    /// 日志页的行（只有日志页才填充，避免每次重绘都克隆上百行）。
    pub logs: Vec<LogLine>,
    /// 全量日志里的级别计数，任何页面都拿得到，供导航徽标用。
    pub log_errors: usize,
    pub log_warns: usize,
    pub log_filter: LogFilter,
    pub limits: Limits,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            stage: SessionStage::Idle,
            error: None,
            installed_broken: Vec::new(),
            page: Page::Push,
            version: String::new(),
            device: DeviceView::default(),
            library: Vec::new(),
            library_error: String::new(),
            installed: Vec::new(),
            transfer: None,
            resume: None,
            resume_checking: false,
            cache_bytes: 0,
            cache_files: 0,
            chunk_bytes: 16384,
            transfer_mode: TransferMode::Steady,
            turbo_blocked: String::new(),
            queue: Vec::new(),
            status: String::new(),
            status_kind: StatusKind::Info,
            auto_launch: true,
            native_ui: false,
            hover: None,
            pressed: None,
            brand: None,
            save_auto: None,
            save_slots: Vec::new(),
            saves_error: String::new(),
            confirm_delete_save: None,
            save_protocol: None,
            saves_unsupported: false,
            saves_busy: false,
            band_version: String::new(),
            band_game_legacy: false,
            saves_notice: String::new(),
            save_export: None,
            save_import: None,
            pending_current_save: None,
            cloud_phase: "loading".into(),
            cloud_detail: String::new(),
            cloud_qq_name: String::new(),
            cloud_qq: String::new(),
            cloud_account_name: String::new(),
            cloud_code: String::new(),
            cloud_expires_at: String::new(),
            account_auth: "idle".into(),
            devices: Vec::new(),
            selected_device: String::new(),
            update_latest: String::new(),
            update_available: false,
            update_status: String::new(),
            website: "https://miband.qihe0507.top/".into(),
            stats: None,
            stats_error: String::new(),
            stats_supported: false,
            stats_busy: false,
            line_filter: None,
            logs: Vec::new(),
            log_errors: 0,
            log_warns: 0,
            log_filter: LogFilter::All,
            limits: Limits::default(),
        }
    }
}

impl Snapshot {
    pub fn installed_count(&self) -> usize {
        self.installed.len()
    }

    /// 还没装到手环的内置章节数。
    pub fn pending_count(&self) -> usize {
        self.library.iter().filter(|pack| !pack.installed).count()
    }

    pub fn library_bytes(&self) -> usize {
        self.library.iter().map(|pack| pack.bytes).sum()
    }

    pub fn pending_bytes(&self) -> usize {
        self.library.iter().filter(|pack| !pack.installed).map(|pack| pack.bytes).sum()
    }

    pub fn pending_minutes(&self) -> usize {
        self.library.iter().filter(|pack| !pack.installed).map(|pack| pack.minutes).sum()
    }

    pub fn total_minutes(&self) -> usize {
        self.library.iter().map(|pack| pack.minutes).sum()
    }

    pub fn installed_bytes(&self) -> usize {
        self.installed.iter().map(|record| record.bytes).sum()
    }

    /// 小时数（保留 1 位），用于统计格。
    pub fn total_hours(&self) -> String {
        format!("{:.1}", self.total_minutes() as f64 / 60.0)
    }

    pub fn pending_hours(&self) -> String {
        format!("{:.1}", self.pending_minutes() as f64 / 60.0)
    }

    /// 旧版本残留的已安装章节。
    pub fn stale_installed(&self) -> Vec<&InstalledView> {
        self.installed.iter().filter(|record| record.stale).collect()
    }

    /// 传输**真的在跑**吗（同步/继续与传输设置按钮据此禁用）。
    ///
    /// 等握手与 `ready=true` 的数据阶段都算在传；但已经过期/停滞的那一态不算：
    /// 应用被挂起时定时器会丢，若不把它放回来，所有同步/继续按钮会永久禁用
    /// （2026-10-02 真机）。过期之后重来由插件侧 `start_transfer` 的过期判定接管。
    pub fn is_transferring(&self) -> bool {
        self.transfer
            .as_ref()
            .is_some_and(|transfer| transfer.started && !transfer.stalled)
    }

    /// 断点指向的那一章在章节库里的编号。
    ///
    /// 推送页用它把那一行的按钮从「同步/重传」换成「继续」——断点就在手环上，
    /// 点这一行就是接着传。章节表对不上（版本变动）时返回 `None`，行按钮保持原样。
    pub fn resume_pack_number(&self) -> Option<usize> {
        let resume = self.resume.as_ref()?;
        self.pack_by_id(&resume.pack_id).map(|pack| pack.number)
    }

    /// 这个动作对应的按钮是不是正被按住。
    pub fn is_pressed(&self, action: &str) -> bool {
        self.pressed.as_deref() == Some(action)
    }
    /// 按线路筛选后的章节。
    pub fn filtered_library(&self) -> Vec<&PackView> {
        self.library.iter().filter(|pack| pack.in_line(self.line_filter.as_deref())).collect()
    }

    pub fn filtered_logs(&self) -> Vec<&LogLine> {
        self.logs.iter().filter(|line| self.log_filter.accepts(line.level)).collect()
    }

    pub fn error_count(&self) -> usize {
        self.log_errors
    }

    pub fn warn_count(&self) -> usize {
        self.log_warns
    }

    pub fn pack_by_id(&self, id: &str) -> Option<&PackView> {
        self.library.iter().find(|pack| pack.id == id)
    }

    // ---- 存档 ----

    /// 手环上的存档行：第 0 条是自动存档，其余是手动槽（`missing` 等判定都在这里算出来）。
    ///
    /// **每次调用都现算**，输入只有两样：
    /// - [`Snapshot::save_auto`] / [`Snapshot::save_slots`] —— 手环回包的**原始**对象；
    /// - [`Snapshot::installed`] —— **当前**的已安装章节列表（顶栏那句
    ///   「已连接 · N/15 章已安装」读的就是同一份）。
    ///
    /// 判定本身也只有一份实现（[`crate::saves::band_rows`] → `SaveSlotView::from_json`），
    /// 所以「手环上装了哪几章」这一个事实，只会有一个结论。
    ///
    /// 为什么坚持现算（2026-09 真机回归）：`missing` 是**派生态**。上一版把它算好存进
    /// 插件状态、再原样搬进快照，于是「存档回包先到、章节列表后到」这个顺序就把界面上
    /// 那四个徽章永久定死了 —— 章节列表后来更新、用户点「刷新」都不会重算。
    /// 见 `docs/插件开发注意事项.md` 7.9。
    pub fn saves(&self) -> Vec<SaveSlotView> {
        let installed: Vec<String> = self.installed.iter().map(|record| record.id.clone()).collect();
        crate::saves::band_rows(&self.save_auto, &self.save_slots, &installed)
    }

    /// 存档通道能不能用：协商过协议，并且手环回过 hello-ok。
    pub fn saves_ready(&self) -> bool {
        self.save_protocol.is_some()
    }

    /// 手环端能力不足时的**结论**（缺 hello-ok 一定要说这句，不能只说超时）。
    ///
    /// 这里是这句话的**唯一出处**：卡片、状态行都从它取，谁都不许再手写一遍。
    pub fn saves_blocked_hint(&self) -> String {
        if self.save_protocol.is_some() {
            return String::new();
        }
        if self.band_version.is_empty() {
            // 手环压根没回 `hello-ok`：旧版白名单 `if` 链没有 `else` 分支，
            // 收到不认识的消息会静默丢弃，所以「没回应」就是这句话的依据。
            "手环端应用版本过旧，需更新后才能管理存档（手环端应用没有回应版本查询）".into()
        } else {
            // 回了，但报回来的协议/能力不够用：版本号并进这一句里，
            // 不再单独渲染一行「报告协议 x、能力 [ ]」——那是同一件事的细节。
            format!(
                "手环端应用版本过旧，需更新后才能管理存档（手环端应用 v{} 不支持存档同步）",
                self.band_version
            )
        }
    }

    /// 结论之后那句**怎么办**：只说下一步动作，**不重复**结论里的判断（分工写死在守护用例里）。
    pub fn saves_blocked_action(&self) -> String {
        if self.save_protocol.is_some() {
            return String::new();
        }
        "重新安装带存档功能的新版 RPK 后重试".into()
    }

    // ---- 游戏代际（1.x 旧版 → 卸载重装） ----

    /// 手环上的游戏是 1.x 旧代时的**结论**（唯一出处；插件侧只置 [`Snapshot::band_game_legacy`] 位）。
    ///
    /// 报得上版本号就把版本号说进去（「v1.6.3」），报不上（连 hello 都没有的更老版本）
    /// 就说清依据是「没有回应版本查询」。
    pub fn band_game_legacy_hint(&self) -> String {
        if self.band_version.is_empty() {
            "手环上的《甜蜜女友2》是 1.x 旧版（没有回应版本查询）。2.0 起存档结构与章节包格式都已更换，旧版读不了新章节包".into()
        } else {
            format!(
                "手环上的《甜蜜女友2》是 1.x 旧版（v{}）。2.0 起存档结构与章节包格式都已更换，旧版读不了新章节包",
                self.band_version
            )
        }
    }

    /// 结论之后那句**怎么办**：卸载重装是唯一出路（覆盖安装跨不了大版本），
    /// 并把「存档会跟着卸载一起没」先说在前面，别让用户存档丢了才发现。
    pub fn band_game_legacy_action(&self) -> String {
        "请先在手环上卸载游戏，再重新安装 2.0 版 RPK（卸载会连存档一起清掉，无法保留）".into()
    }

    /// 存档通道被版本卡住：协议没协商出来，而且**已经试过并失败**（不是「还在等」）。
    ///
    /// 这是「版本过旧」那张卡的唯一触发条件 —— 只连上、还没问过能力时不该弹这张卡。
    pub fn saves_blocked(&self) -> bool {
        self.save_protocol.is_none() && (self.saves_unsupported || !self.saves_error.is_empty())
    }

    /// 「存档通道不可用」卡片要显示的正文：`(结论, 怎么办)`，**各自只说一次**。
    ///
    /// - 通道被版本卡住 → 结论取 [`Snapshot::saves_blocked_hint`]，再跟一句怎么办；
    ///   插件记下的那次失败原因（`saves_error`）与结论是**同一件事**，这里不再渲染第二遍。
    /// - 协议没问题、只是某一次操作失败 → 回显那次失败的原因，没有额外的「怎么办」。
    ///
    /// 返回 `None` 表示这一页不该出现这张卡。
    pub fn saves_blocked_notice(&self) -> Option<(String, String)> {
        if self.saves_blocked() {
            return Some((self.saves_blocked_hint(), self.saves_blocked_action()));
        }
        (!self.saves_error.is_empty()).then(|| (self.saves_error.clone(), String::new()))
    }

    /// 某个槽（`"auto"` 或槽号字符串）是不是正等二次确认。
    pub fn is_confirming_save_delete(&self, key: &str) -> bool {
        self.confirm_delete_save.as_deref() == Some(key)
    }

    // ---- 阅读统计 ----

    /// 统计通道被版本卡住：**已经收到过 `hello-ok`**（协议协商出来了），
    /// 但那份能力表里没有 `stats` —— 也就是手环端应用太旧。
    ///
    /// 刻意要求「协议已经有了」：只连上、还没问过能力时不该弹这张卡，
    /// 那句「版本过旧」必须是有依据的结论（和存档那张卡同一条规矩）。
    pub fn stats_blocked(&self) -> bool {
        self.save_protocol.is_some() && !self.stats_supported
    }

    /// 统计页那张卡片的正文：`(结论, 怎么办)`，各自只说一次。
    /// 返回 `None` 表示这一页不该出现这张卡（通道正常，或者只是还没读到）。
    pub fn stats_notice(&self) -> Option<(String, String)> {
        if self.stats_blocked() {
            let version = self.band_version.clone();
            let conclusion = if version.is_empty() {
                "手环端应用版本过旧，需更新后才能读取阅读统计（手环端应用不支持阅读统计）".to_string()
            } else {
                format!("手环端应用版本过旧，需更新后才能读取阅读统计（手环端应用 v{version} 不支持阅读统计）")
            };
            return Some((conclusion, "重新安装新版 RPK 后重试".into()));
        }
        (!self.stats_error.is_empty()).then(|| (self.stats_error.clone(), String::new()))
    }

    /// 页面标题右侧的一句话状态。
    pub fn head_line(&self) -> String {
        if self.library.is_empty() {
            return "章节包不可用".into();
        }
        if self.device.connected && self.device.alive {
            format!("已连接 · {}/{} 章已安装", self.installed_count(), self.library.len())
        } else if self.device.connected {
            "已连接手环，等待应用回应".into()
        } else {
            "未连接手环".into()
        }
    }

    /// 推送页「章节库」卡片里「接下来」那一行：**只挑最要紧的一件事**，一句话说清。
    ///
    /// 上一版这里是一张列了 3~5 条的清单（未装章节、传输中提醒、未完成传输、队列），
    /// 每条都带颜色圆点，读起来像日志而不像建议。实际上这四件事里**永远只有第一件**
    /// 需要用户动手，其余的等它变成第一件时再说。
    pub fn next_step(&self) -> (StatusKind, String) {
        if !self.library_error.is_empty() {
            return (StatusKind::Bad, "插件里的章节包读不出来，重装一次插件试试".into());
        }
        if !self.device.connected {
            return (StatusKind::Warn, "手环还没连上".into());
        }
        if !self.device.alive {
            return (StatusKind::Warn, "在手环上打开《甜蜜女友2》，这里就会自动接上".into());
        }
        // 旧代游戏排在「还有几章没同步」前面：它连一章都同步不了，先解决版本问题。
        if self.band_game_legacy {
            return (StatusKind::Bad, "手环上是 1.x 旧版游戏：先卸载重装 2.0 版，再同步章节".into());
        }
        if self.pending_count() > 0 {
            return (StatusKind::Info, format!("还有 {} 章没同步到手环", self.pending_count()));
        }
        (StatusKind::Good, "章节都装好了，去手环上开玩吧".into())
    }

    /// 章节号 → 标题，用于队列显示。
    pub fn title_of(&self, number: usize) -> String {
        self.library
            .iter()
            .find(|pack| pack.number == number)
            .map_or_else(|| format!("第 {number} 章"), |pack| pack.title.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(number: usize, bytes: usize, minutes: usize, installed: bool) -> PackView {
        PackView {
            number,
            id: format!("p{number}"),
            title: format!("第{number}章"),
            minutes,
            bytes,
            scenes: 10,
            dialogues: 100,
            installed,
            queued: false,
            active: false,
        }
    }

    fn transfer(received: usize, total: usize, speed: f64) -> TransferView {
        TransferView {
            chapter: "共通线1".into(),
            percent: 0,
            received,
            total,
            chunks_done: 0,
            chunks_total: 10,
            speed_kbps: speed,
            rtt_ms: 40,
            retries: 0,
            resumed: false,
            ready: false,
            started: true,
            stalled: false,
            chunk_bytes: 8192,
        }
    }

    #[test]
    fn a_stalled_handshake_no_longer_blocks_every_button() {
        // 真机原话（2026-10-02）：**重新连接后点「继续传输」不传输，速度和剩余都显示「—」**。
        // 病因是「已发请求、等手环确认」这一态没人回收（应用被挂起时插件挂的定时器会丢），
        // 而 `is_transferring()` 把它算成「在传」→ 推送页每一章的同步/继续按钮全被禁掉。
        let mut snapshot = Snapshot::default();
        snapshot.transfer = Some(transfer(0, 1000, 0.0));
        assert!(snapshot.is_transferring(), "还没到期的等待仍算在传（按钮先别放）");

        let mut stalled = transfer(0, 1000, 0.0);
        stalled.stalled = true;
        snapshot.transfer = Some(stalled);
        assert!(
            !snapshot.is_transferring(),
            "等待过期 / 握手后长时间静默之后按钮必须放回来，否则用户点什么都动不了"
        );

        // 握手完成后的数据阶段仍在传：推送页要挡住重复同步，设置页也要禁改模式/分片。
        let mut running = transfer(500, 1000, 12.0);
        running.ready = true;
        snapshot.transfer = Some(running);
        assert!(snapshot.is_transferring(), "ready=true 的活跃数据阶段仍算在传");

        // 已停滞的数据传输可以重来，但尚未过期的等待仍受保护。
        let mut stopped = transfer(500, 1000, 12.0);
        stopped.ready = true;
        stopped.stalled = true;
        snapshot.transfer = Some(stopped);
        assert!(!snapshot.is_transferring(), "停滞后必须允许用户重来");
    }

    #[test]
    fn aggregates_only_count_pending_work() {
        let mut snapshot = Snapshot::default();
        snapshot.library = vec![pack(1, 1000, 10, true), pack(2, 2000, 20, false)];
        snapshot.installed = vec![InstalledView {
            id: "p1".into(),
            name: "第1章".into(),
            bytes: 1000,
            files: 3,
            stale: false,
        }];
        assert_eq!(snapshot.pending_count(), 1);
        assert_eq!(snapshot.pending_bytes(), 2000);
        assert_eq!(snapshot.pending_minutes(), 20);
        assert_eq!(snapshot.library_bytes(), 3000);
        assert_eq!(snapshot.total_minutes(), 30);
        assert_eq!(snapshot.total_hours(), "0.5");
    }

    #[test]
    fn eta_is_none_without_speed_and_finite_with_it() {
        assert_eq!(transfer(0, 1024 * 100, 0.0).eta_seconds(), None);
        // 还剩 100KB，速度 10KB/s → 10 秒。
        assert_eq!(transfer(0, 1024 * 100, 10.0).eta_seconds(), Some(10));
        // 传完了就没有 ETA。
        assert_eq!(transfer(1024 * 100, 1024 * 100, 10.0).eta_seconds(), None);
    }

    #[test]
    fn log_filter_selects_levels() {
        assert!(LogFilter::All.accepts(LogLevel::Debug));
        assert!(!LogFilter::Warn.accepts(LogLevel::Info));
        assert!(LogFilter::Warn.accepts(LogLevel::Error));
        assert!(!LogFilter::Error.accepts(LogLevel::Warn));
    }

    #[test]
    fn page_wire_round_trips() {
        for page in Page::ALL {
            assert_eq!(Page::parse(page.wire()), Some(page));
            assert_eq!(page.action(), format!("nav:{}", page.wire()));
        }
        assert_eq!(Page::parse("nope"), None);
    }

    #[test]
    fn sniff_detects_level_tokens() {
        assert_eq!(LogLevel::sniff("2026-01-01T00:00:00Z ERROR boom"), LogLevel::Error);
        assert_eq!(LogLevel::sniff(" WARN slow"), LogLevel::Warn);
        assert_eq!(LogLevel::sniff(" INFO ok"), LogLevel::Info);
    }

    #[test]
    fn line_filter_keeps_only_matching_routes() {
        let mut first = pack(1, 1, 1, false);
        first.title = "千岁线1·序章".into();
        let mut second = pack(2, 1, 1, false);
        second.title = "玲线1·序章".into();
        let mut snapshot = Snapshot::default();
        snapshot.library = vec![first, second];

        snapshot.line_filter = None;
        assert_eq!(snapshot.filtered_library().len(), 2);
        snapshot.line_filter = Some("玲线".into());
        let filtered = snapshot.filtered_library();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].number, 2);
    }

    #[test]
    fn pack_meta_and_labels_read_naturally() {
        let mut view = pack(1, 1_572_864, 95, false);
        // 「阅读」字样必须有：不然「约 54 分钟」会被当成推送耗时（真有人这么误解过）。
        assert_eq!(view.minutes_label(), "阅读约 1 小时 35 分");
        assert_eq!(view.meta_line(), "阅读约 1 小时 35 分 · 1.50 MB");
        assert!(view.minutes_label().starts_with("阅读"));
        view.minutes = 59;
        assert_eq!(view.minutes_label(), "阅读约 59 分钟");
        view.minutes = 120;
        assert_eq!(view.minutes_label(), "阅读约 2 小时");
    }

    /// 「接下来」永远只说**最要紧的那一件**，优先级从「插件坏了」一路排到「都装好了」。
    ///
    /// 这条顺序是语义：写反了用户会先看到「在手环上打开游戏」，而其实插件自己就读不到章节包。
    #[test]
    fn next_step_picks_the_most_urgent_thing_first() {
        let mut snapshot = Snapshot::default();
        // ① 章节包都读不出来：别的都不用谈。
        snapshot.library_error = "packs/index.json 缺失".into();
        assert_eq!(snapshot.next_step().0, StatusKind::Bad);
        snapshot.library_error.clear();

        // ② 没连手环。
        snapshot.library = vec![pack(1, 1, 1, false)];
        assert_eq!(snapshot.next_step().0, StatusKind::Warn);
        assert!(snapshot.next_step().1.contains("还没连上"));

        // ③ 连上了但手环应用没在跑。
        snapshot.device.connected = true;
        assert!(snapshot.next_step().1.contains("打开《甜蜜女友2》"));

        // ④ 一切就绪但还有没装的章节。
        snapshot.device.alive = true;
        let (kind, text) = snapshot.next_step();
        assert_eq!(kind, StatusKind::Info);
        assert!(text.contains("还有 1 章没同步"), "{text}");

        // ⑤ 都装好了：手环记录里有它，章节行上的「已装」标记也跟着翻过来
        //    （`pending_count()` 读的是**章节自己的 `installed` 标记**，插件侧每次拍快照
        //    都按手环的已安装清单重算这一列，所以这里两样一起给）。
        snapshot.installed = vec![InstalledView {
            id: "p1".into(),
            name: "第1章".into(),
            bytes: 1,
            files: 1,
            stale: false,
        }];
        snapshot.library = vec![pack(1, 1, 1, true)];
        assert_eq!(snapshot.next_step().0, StatusKind::Good);
    }

    /// 通道被版本卡住时，卡片正文就是**一句结论 + 一句怎么办**，两句话分工不重叠。
    ///
    /// 这条守的是用户实机截图那个 bug：以前同一个事实（手环端应用过旧）被插件与界面
    /// 各写一遍，卡片上并排两句几乎一样的话。现在结论只有 [`Snapshot::saves_blocked_hint`]
    /// 一处出处，「怎么办」只讲下一步动作。
    #[test]
    fn blocked_saves_copy_is_one_conclusion_plus_one_next_step() {
        let mut snapshot = Snapshot::default();
        assert!(!snapshot.saves_blocked(), "刚起来、还没问过能力时不该出现这张卡");
        assert!(snapshot.saves_blocked_notice().is_none());

        // ① 没回 hello-ok：结论要给出「没回应」这个依据。
        snapshot.saves_unsupported = true;
        assert!(snapshot.saves_blocked());
        let (conclusion, action) = snapshot.saves_blocked_notice().expect("卡住时要有这张卡");
        assert_eq!(conclusion.matches("手环端应用版本过旧").count(), 1, "{conclusion}");
        assert!(conclusion.contains("没有回应"), "{conclusion}");
        assert_eq!(action, "重新安装带存档功能的新版 RPK 后重试");
        assert!(!action.contains("版本过旧"), "「怎么办」不许重复结论的判断：{action}");

        // ② 手环回了、但能力不够：版本号并进同一句结论里，不再多出第三行。
        snapshot.band_version = "0.1.0".into();
        let (answered, _) = snapshot.saves_blocked_notice().expect("卡住时要有这张卡");
        assert!(answered.contains("手环端应用版本过旧"), "{answered}");
        assert!(answered.contains("0.1.0"), "{answered}");
        assert!(!answered.contains("没有回应"), "手环明明回了，别写成没回应：{answered}");

        // ③ 协议协商成功 → 整张卡（连同两句话）都消失。
        snapshot.save_protocol = Some(1);
        snapshot.saves_unsupported = false;
        assert!(snapshot.saves_blocked_hint().is_empty());
        assert!(snapshot.saves_blocked_action().is_empty());
        assert!(snapshot.saves_blocked_notice().is_none());
    }

    /// 协议没问题、只是某一次操作失败：卡片回显那次失败的原因，不追加「怎么办」。
    #[test]
    fn failed_operation_with_a_good_protocol_shows_its_own_reason() {
        let mut snapshot = Snapshot::default();
        snapshot.save_protocol = Some(1);
        snapshot.saves_error = "手环写入存储失败（空间不足或被系统拒绝）（write-failed）".into();
        assert!(!snapshot.saves_blocked(), "协议是好的，不算「版本过旧」");
        let (conclusion, action) = snapshot.saves_blocked_notice().expect("失败要给一张卡");
        assert_eq!(conclusion, snapshot.saves_error);
        assert!(action.is_empty(), "不是版本问题就不给「重装 RPK」这种下一步");
    }
}
