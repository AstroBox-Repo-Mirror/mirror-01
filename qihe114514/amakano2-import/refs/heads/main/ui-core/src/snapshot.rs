//! 渲染用的只读视图模型。
//!
//! 页面只读这份快照，不直接碰 `State`：渲染期间不需要反复加锁，
//! 页面逻辑也就能脱离宿主和业务状态单独测试。

use std::sync::Arc;
use super::errors::ErrorView;

use serde_json::Value;

/// 导航页。
///
/// **只有七页**：原来那一页「传输」已经撤掉 —— 它跟「概览」页的传输卡片、
/// 「设备」页的刷新手环清单、「设置」页的分片选择器各重叠一块，用户要从概览
/// 再跳一次「查看传输详情」才看得到同样的数据。撤掉之后传输的实时数据留在概览，
/// 分片参数留在设置，手环清单留在设备，一处一件事。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Overview,
    Library,
    Saves,
    Stats,
    Device,
    Settings,
    Logs,
}

pub const NAV_PREFIX: &str = "nav:";

/// 分片字节数可选项：越大越快；传输中断后不自动改档，按断点继续传。
pub const CHUNK_OPTIONS: [usize; 3] = [4096, 8192, 16384];

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
    pub const ALL: [Page; 7] = [
        Page::Overview,
        Page::Library,
        Page::Saves,
        Page::Stats,
        Page::Device,
        Page::Settings,
        Page::Logs,
    ];

    pub const fn wire(self) -> &'static str {
        match self {
            Page::Overview => "overview",
            Page::Library => "library",
            Page::Saves => "saves",
            Page::Stats => "stats",
            Page::Device => "device",
            Page::Settings => "settings",
            Page::Logs => "logs",
        }
    }

    /// 导航标签。**一律两个字**：标签一长，导航条在 400px 窗口里就装不下，
    /// 每一项会被压成「一个字一行」（见 `glass::nav_bar`）。
    pub const fn label(self) -> &'static str {
        match self {
            Page::Overview => "概览",
            Page::Library => "章节",
            Page::Saves => "存档",
            Page::Stats => "统计",
            Page::Device => "设备",
            Page::Settings => "设置",
            Page::Logs => "日志",
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
            SessionStage::ChannelRegistered => "正在等手环应用响应（没反应就在手表上打开《甜蜜女友2》）",
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
    pub fn minutes_label(&self) -> String {
        if self.minutes >= 60 {
            let hours = self.minutes / 60;
            let rest = self.minutes % 60;
            if rest == 0 {
                format!("约 {hours} 小时")
            } else {
                format!("约 {hours} 小时 {rest} 分")
            }
        } else {
            format!("约 {} 分钟", self.minutes)
        }
    }

    /// 章节行下面那一行元信息。
    ///
    /// **一行说完**：上一版分两行（「约 54 分钟 · 736 KB · 41 幕」+「共通线 · 1066 句对白」），
    /// 15 行列表就是 30 行小字。线路名不进这一行 —— 行首的序号砖已经用颜色在标线路了。
    pub fn meta_line(&self) -> String {
        format!(
            "{} · {} · {} 幕 · {} 句",
            self.minutes_label(),
            super::human_bytes(self.bytes),
            self.scenes,
            self.dialogues
        )
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
    pub ready: bool,
    pub started: bool,
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
    /// 存档记的章节包没装在手环上 —— 读档之前得先同步那一章。
    pub missing: bool,
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
            missing: !pack_id.is_empty() && !installed.iter().any(|id| id == &pack_id),
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

    /// 这一条能不能读：章节包在手环上。
    pub fn readable(&self) -> bool {
        !self.missing
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

/// 存档所在章节没装的条数 —— 这些条目不能读，界面上要标出来。
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

    /// 概览卡片的副标题。
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
            retry_delay_ms: 1500,
            max_retries: 4,
            initial_window: 3,
            probe_interval_ms: 2500,
            probe_attempts: 12,
            max_pack_bytes: 20_000_000,
        }
    }
}

impl Limits {
    pub fn window_label(&self) -> String {
        format!("{} 片", self.initial_window)
    }

    pub fn probe_label(&self) -> String {
        format!("{} ms × {} 次", self.probe_interval_ms, self.probe_attempts)
    }
}

/// 一页需要的全部数据。
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub page: Page,
    pub version: String,
    /// 连接走到了哪一步。概览页的分段进度条按它点亮。
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
    pub cache_bytes: usize,
    pub cache_files: usize,
    pub chunk_bytes: usize,
    /// 待同步的章节号（先进先出）。
    pub queue: Vec<usize>,
    pub status: String,
    pub status_kind: StatusKind,
    pub auto_launch: bool,
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
    /// 存档页顶部的操作结果（导出/导入/读档/删除各一句）。
    pub saves_notice: String,
    /// 上一次「导出到剪贴板」的结果；`None` 表示这一轮还没导出过。
    /// 导出成功要显示**信封字节数**（用户靠它判断剪贴板里是不是完整的一份）。
    pub save_export: Option<SaveExportView>,
    /// 上一次「从剪贴板导入」的结果；`None` 表示还没导入过。
    pub save_import: Option<SaveImportView>,
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
            page: Page::Overview,
            version: String::new(),
            device: DeviceView::default(),
            library: Vec::new(),
            library_error: String::new(),
            installed: Vec::new(),
            transfer: None,
            resume: None,
            cache_bytes: 0,
            cache_files: 0,
            chunk_bytes: 8192,
            queue: Vec::new(),
            status: String::new(),
            status_kind: StatusKind::Info,
            auto_launch: true,
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
            saves_notice: String::new(),
            save_export: None,
            save_import: None,
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

    pub fn is_transferring(&self) -> bool {
        self.transfer.as_ref().is_some_and(|transfer| transfer.started && !transfer.ready)
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
            "手环端应用版本过旧，需更新后才能管理存档（手环侧没有回应 amakano.app.hello）".into()
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
        "到 AIoT-IDE 重新安装带存档功能的新版 RPK 后重试".into()
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
                "手环端应用版本过旧，需更新后才能读取阅读统计（手环端应用不支持 stats 通道）".to_string()
            } else {
                format!("手环端应用版本过旧，需更新后才能读取阅读统计（手环端应用 v{version} 不支持 stats 通道）")
            };
            return Some((conclusion, "到 AIoT-IDE 重新安装新版 RPK 后重试".into()));
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

    /// 概览页「接下来」那一行：**只挑最要紧的一件事**，一句话说清。
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
            chunk_bytes: 8192,
        }
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
        assert_eq!(view.minutes_label(), "约 1 小时 35 分");
        assert_eq!(view.meta_line(), "约 1 小时 35 分 · 1.50 MB · 10 幕 · 100 句");
        view.minutes = 59;
        assert_eq!(view.minutes_label(), "约 59 分钟");
        view.minutes = 120;
        assert_eq!(view.minutes_label(), "约 2 小时");
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
        assert!(conclusion.contains("amakano.app.hello"), "{conclusion}");
        assert_eq!(action, "到 AIoT-IDE 重新安装带存档功能的新版 RPK 后重试");
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
