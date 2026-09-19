//! 界面动作的全部 id 词汇表，以及把它们解析成 [`Action`] 的纯函数。
//!
//! 旧实现靠一堆 `starts_with` 前缀匹配散落在两处（判断是不是动作 + 分发），
//! 加一个动作要改两个地方、还容易漏。这里集中一处：**解析成功即合法**，
//! `is_action` 与分发共用同一份逻辑，不会漂移。

use super::snapshot::{LogFilter, Page, NAV_PREFIX};
use super::theme::LINES;

pub const CONNECT: &str = "connect";
pub const LAUNCH: &str = "launch";
pub const SYNC_ALL: &str = "sync-all";
pub const REFRESH: &str = "refresh-list";
pub const CLEAR_CACHE: &str = "clear-cache";
pub const LOG_CLEAR: &str = "log-clear";
pub const AUTO_ON: &str = "auto-launch:on";
pub const AUTO_OFF: &str = "auto-launch:off";
// ---- 存档 ----
// 前缀刻意都带 `save-`，不会撞上上面的 `sync:` / `delete:` / `confirm:` / `logfilter:`。
pub const SAVES_REFRESH: &str = "save-refresh";
pub const SAVES_EXPORT: &str = "save-export";
pub const SAVES_IMPORT: &str = "save-import";
pub const SAVES_CANCEL_DELETE: &str = "save-cancel";
/// 阅读统计：从手环重新读一次（页面上叫「读取统计 / 刷新统计」）。
pub const STATS_REFRESH: &str = "stats-refresh";
/// 自动存档的槽位键（手动存档用槽号字符串）。
pub const SAVE_AUTO_KEY: &str = "auto";
pub const SAVE_LOAD_PREFIX: &str = "save-load:";
pub const SAVE_DELETE_PREFIX: &str = "save-delete:";
pub const SAVE_CONFIRM_PREFIX: &str = "save-delete-ok:";

/// 「全部线路」筛选的 id。
pub const LINE_ALL: &str = "line:*";
pub const SYNC_PREFIX: &str = "sync:";
pub const DELETE_PREFIX: &str = "delete:";
pub const CHUNK_PREFIX: &str = "chunk:";
pub const LOG_FILTER_PREFIX: &str = "logfilter:";
pub const HOVER_PREFIX: &str = "hover:";
pub const PRESS_PREFIX: &str = "press:";

pub fn sync_id(number: usize) -> String {
    format!("{SYNC_PREFIX}{number}")
}

/// 删除动作带上手环上的包 id（可能是中文，所以直接用原字符串）。
///
/// **章节包删除没有二次确认**：插件里有一份完整副本，删掉随时能重新同步回来，
/// 再让用户点一次「确认删除」只是白加一步（用户要求「减少不必要的二次确认」）。
/// 存档删除不同 —— 那是不可恢复的，所以只有它保留了确认（见 `saves.rs`）。
pub fn delete_id(pack_id: &str) -> String {
    format!("{DELETE_PREFIX}{pack_id}")
}

pub fn chunk_id(bytes: usize) -> String {
    format!("{CHUNK_PREFIX}{bytes}")
}

/// `None` 表示「全部线路」。
pub fn line_id(line: Option<&str>) -> String {
    match line {
        Some(line) => format!("{}{line}", LINE_ALL.trim_end_matches('*')),
        None => LINE_ALL.to_string(),
    }
}

pub fn log_filter_id(filter: LogFilter) -> String {
    format!("{LOG_FILTER_PREFIX}{}", filter.wire())
}

pub fn hover_id(id: &str) -> String {
    format!("{HOVER_PREFIX}{id}")
}

// ---- 存档动作 id ----

/// 自动存档的槽位键，手动存档就是槽号。
pub fn save_key(slot: Option<usize>) -> String {
    match slot {
        Some(index) => index.to_string(),
        None => SAVE_AUTO_KEY.to_string(),
    }
}

pub fn save_load_id(slot: Option<usize>) -> String {
    format!("{SAVE_LOAD_PREFIX}{}", save_key(slot))
}

pub fn save_delete_id(slot: Option<usize>) -> String {
    format!("{SAVE_DELETE_PREFIX}{}", save_key(slot))
}

pub fn save_confirm_id(slot: Option<usize>) -> String {
    format!("{SAVE_CONFIRM_PREFIX}{}", save_key(slot))
}

/// 解析存档槽位键：`"auto"` → `None`，否则是槽号。
fn parse_save_key(value: &str) -> Option<Option<usize>> {
    if value == SAVE_AUTO_KEY {
        return Some(None);
    }
    value.parse::<usize>().ok().map(Some)
}

/// 按下态用的 id：把原本的动作 id 包一层，好让按钮知道「是哪一个动作被按住了」。
pub fn press_id(action: &str) -> String {
    format!("{PRESS_PREFIX}{action}")
}

/// 解析后的动作。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// 切换页面。
    Nav(Page),
    Connect,
    /// 在手环上打开《甜蜜女友2》。
    Launch,
    SyncAll,
    Sync(usize),
    RefreshList,
    ClearCache,
    Delete(String),
    Chunk(usize),
    AutoLaunch(bool),
    Line(Option<String>),
    LogFilter(LogFilter),
    LogClear,
    // ---- 存档 ----
    SavesRefresh,
    SavesExport,
    SavesImport,
    /// 读档：把这一槽设成手环上「继续阅读」的进度。`None` 表示自动存档。
    SavesLoad(Option<usize>),
    /// 第一次点删除：只是把这一行切到「确认删除」。
    SavesDelete(Option<usize>),
    SavesConfirmDelete(Option<usize>),
    SavesCancelDelete,
    // ---- 阅读统计 ----
    /// 从手环读一次阅读统计。
    StatsRefresh,
    /// 悬停进入某个元素；空字符串表示离开（清除悬停）。
    Hover(String),
    /// 某个动作的按钮被按住了（PointerDown）。
    Press(String),
}

impl Action {
    /// 纯界面动作：只改界面状态，不打状态行、也不该被去重表拦掉。
    /// 悬停与按下都属此类 —— 它们又快又密，走业务通道会把状态行刷花。
    pub fn is_ui_only(&self) -> bool {
        matches!(self, Action::Hover(_) | Action::Press(_))
    }

    /// 这个「刷新」动作要往手环重新拉哪些列表。
    ///
    /// **判定只有这一处**：插件侧的动作分派在 `#[cfg(target_arch = "wasm32")]` 里，
    /// 宿主机上跑不到（`cargo test -p amakano2-import` 编不到它），而「点一下刷新之后
    /// 界面该不该变」是用户能直接看见对错的语义，必须有能在本地跑的守护用例。
    /// 插件侧照这份计划发请求（`run_refresh`），不自己再判一遍。
    pub fn refresh_plan(self) -> RefreshPlan {
        match self {
            Action::RefreshList => RefreshPlan { pack_list: true, ..RefreshPlan::default() },
            // ⚠️ 存档页的「刷新」必须**两样都刷**：存档行上的「未安装」徽章是拿
            // **当前**已安装章节列表在渲染时现算的（`Snapshot::saves`）。只刷存档、不刷
            // 章节列表的话，章节列表一变，用户点刷新也看不到任何变化 —— 2026-09 真机
            // 就是这么报上来的（四条存档全标「未安装」，刷新也不变）。
            Action::SavesRefresh => RefreshPlan { pack_list: true, save_list: true, ..RefreshPlan::default() },
            // 统计页只拉统计：这一页的结论完全来自手环上那张按天的表，
            // 顺手拉章节列表只是白花一次往返。
            Action::StatsRefresh => RefreshPlan { stats_list: true, ..RefreshPlan::default() },
            _ => RefreshPlan::default(),
        }
    }
}

/// 一次「刷新」要重新拉哪些列表（由 [`Action::refresh_plan`] 判定，插件侧据此发请求）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RefreshPlan {
    /// 重新拉已安装章节列表（`amakano.pack.list`）。
    pub pack_list: bool,
    /// 重新拉手环存档（`amakano.saves.list`）。
    pub save_list: bool,
    /// 重新拉手环上的阅读统计（`amakano.stats.list`）。
    pub stats_list: bool,
}

/// 「刷新」里的一次往返。[`refresh_steps`] 给出的就是**发送顺序**。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshStep {
    /// `amakano.pack.list` → `amakano.pack.list`（已安装章节列表 + 缓存占用）。
    PackList,
    /// `amakano.saves.list` → `amakano.saves.data`（手环上的存档）。
    SaveList,
    /// `amakano.stats.list` → `amakano.stats.data`（手环上的阅读统计）。
    StatsList,
}

/// 把「刷什么」翻译成「按什么顺序发」。
///
/// **章节列表在前、存档在后**，理由是插件侧的 `State::request` 只有**一个坑位**
/// （见 `src/request.rs` 的 `Slot`）：后发的那个占着坑位，也就只有它拿得到
/// 「超时 → 重发」的兜底，先发的那个一旦丢包就没人管了。
/// 而这两者谁更需要兜底是清楚的 —— 存档列表是那个更容易丢、更需要重试的请求
/// （2026-09-11/12 实测：存档首次就成的比例只有 18.2%，章节列表 94.1%），
/// 所以把它排在最后发。旧实现是存档先发、章节列表后发，正好把兜底给了不需要的那个。
///
/// **统计排在最后**：它是「读了才有」的附加数据，而且统计页只刷统计，
/// 所以它既不会把存档挤出坑位，也不影响存档那套既有顺序
/// （存档页的刷新里 `stats_list` 是 false，步骤表与以前完全一致）。
///
/// 调用方（插件侧的 `run_refresh`）必须**一次只发一个**：发完第一个就等它 settle
/// （回包 / 写回 / 超时放弃），settle 之后再发下一个，不能两个一起挂出去。
pub fn refresh_steps(plan: RefreshPlan) -> Vec<RefreshStep> {
    let mut steps = Vec::new();
    if plan.pack_list {
        steps.push(RefreshStep::PackList);
    }
    if plan.save_list {
        steps.push(RefreshStep::SaveList);
    }
    if plan.stats_list {
        steps.push(RefreshStep::StatsList);
    }
    steps
}

/// 把元素 id 解析成动作。**解析不出来就是非法动作**，调用方不用再写前缀判断。
pub fn parse(id: &str) -> Option<Action> {
    // 悬停要排在最前面：`hover:nav:library` 也以别的规则沾边，但它属于悬停通道。
    if let Some(rest) = id.strip_prefix(HOVER_PREFIX) {
        return Some(Action::Hover(rest.to_string()));
    }
    if let Some(rest) = id.strip_prefix(PRESS_PREFIX) {
        return (!rest.is_empty()).then(|| Action::Press(rest.to_string()));
    }
    if let Some(page) = id.strip_prefix(NAV_PREFIX).and_then(Page::parse) {
        return Some(Action::Nav(page));
    }
    if let Some(rest) = id.strip_prefix(SYNC_PREFIX) {
        return rest.parse::<usize>().ok().map(Action::Sync);
    }
    if let Some(rest) = id.strip_prefix(DELETE_PREFIX) {
        return (!rest.is_empty()).then(|| Action::Delete(rest.to_string()));
    }
    if let Some(rest) = id.strip_prefix(CHUNK_PREFIX) {
        return rest.parse::<usize>().ok().map(Action::Chunk);
    }
    if let Some(rest) = id.strip_prefix(LOG_FILTER_PREFIX) {
        return LogFilter::parse(rest).map(Action::LogFilter);
    }
    // 存档：三个前缀里 `save-delete-ok:` 在 `save-delete:` 之前匹配会串味，
    // 所以先试更长的那个（`strip_prefix` 是精确前缀匹配，`save-delete-ok:0` 不会被
    // `save-delete:` 命中，但顺序仍按「长的在前」写好读）。
    if let Some(rest) = id.strip_prefix(SAVE_CONFIRM_PREFIX) {
        return parse_save_key(rest).map(Action::SavesConfirmDelete);
    }
    if let Some(rest) = id.strip_prefix(SAVE_DELETE_PREFIX) {
        return parse_save_key(rest).map(Action::SavesDelete);
    }
    if let Some(rest) = id.strip_prefix(SAVE_LOAD_PREFIX) {
        return parse_save_key(rest).map(Action::SavesLoad);
    }
    if let Some(rest) = id.strip_prefix(LINE_ALL.trim_end_matches('*')) {
        if rest.is_empty() || rest == "*" {
            return Some(Action::Line(None));
        }
        return LINES.contains(&rest).then(|| Action::Line(Some(rest.to_string())));
    }
    match id {
        CONNECT => Some(Action::Connect),
        LAUNCH => Some(Action::Launch),
        SYNC_ALL => Some(Action::SyncAll),
        REFRESH => Some(Action::RefreshList),
        CLEAR_CACHE => Some(Action::ClearCache),
        LOG_CLEAR => Some(Action::LogClear),
        AUTO_ON => Some(Action::AutoLaunch(true)),
        AUTO_OFF => Some(Action::AutoLaunch(false)),
        SAVES_REFRESH => Some(Action::SavesRefresh),
        SAVES_EXPORT => Some(Action::SavesExport),
        SAVES_IMPORT => Some(Action::SavesImport),
        SAVES_CANCEL_DELETE => Some(Action::SavesCancelDelete),
        STATS_REFRESH => Some(Action::StatsRefresh),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_generated_id_parses_back() {
        assert_eq!(parse(&sync_id(7)), Some(Action::Sync(7)));
        assert_eq!(parse(&delete_id("共通线1·归乡")), Some(Action::Delete("共通线1·归乡".into())));
        assert_eq!(parse(&chunk_id(8192)), Some(Action::Chunk(8192)));
        assert_eq!(parse(&line_id(None)), Some(Action::Line(None)));
        assert_eq!(parse(&line_id(Some("玲线"))), Some(Action::Line(Some("玲线".into()))));
        assert_eq!(parse(&log_filter_id(LogFilter::Error)), Some(Action::LogFilter(LogFilter::Error)));
        assert_eq!(parse(&hover_id("sync:7")), Some(Action::Hover("sync:7".into())));
        // 按下 id 走的是另一条通道，不能和悬停 id 撞车。
        assert_eq!(parse(&press_id("sync:7")), Some(Action::Press("sync:7".into())));
        assert_ne!(press_id("sync:7"), hover_id("sync:7"));
        for page in Page::ALL {
            // 导航 id 的拼法与派发都用 `Page::action()` 一份实现，不再另养一个 `nav_id`。
            assert_eq!(parse(&page.action()), Some(Action::Nav(page)));
        }
    }

    #[test]
    fn static_actions_parse() {
        assert_eq!(parse(CONNECT), Some(Action::Connect));
        assert_eq!(parse(LAUNCH), Some(Action::Launch));
        assert_eq!(parse(SYNC_ALL), Some(Action::SyncAll));
        assert_eq!(parse(REFRESH), Some(Action::RefreshList));
        assert_eq!(parse(CLEAR_CACHE), Some(Action::ClearCache));
        assert_eq!(parse(LOG_CLEAR), Some(Action::LogClear));
        assert_eq!(parse(AUTO_ON), Some(Action::AutoLaunch(true)));
        assert_eq!(parse(AUTO_OFF), Some(Action::AutoLaunch(false)));
        assert_eq!(parse(SAVES_REFRESH), Some(Action::SavesRefresh));
        assert_eq!(parse(SAVES_EXPORT), Some(Action::SavesExport));
        assert_eq!(parse(SAVES_IMPORT), Some(Action::SavesImport));
        assert_eq!(parse(SAVES_CANCEL_DELETE), Some(Action::SavesCancelDelete));
        assert_eq!(parse(STATS_REFRESH), Some(Action::StatsRefresh));
    }

    #[test]
    fn save_slot_ids_round_trip_and_stay_distinct() {
        assert_eq!(parse(&save_load_id(Some(3))), Some(Action::SavesLoad(Some(3))));
        assert_eq!(parse(&save_load_id(None)), Some(Action::SavesLoad(None)));
        assert_eq!(parse(&save_delete_id(Some(3))), Some(Action::SavesDelete(Some(3))));
        assert_eq!(parse(&save_confirm_id(Some(3))), Some(Action::SavesConfirmDelete(Some(3))));
        // 三个前缀必须互不串味：删除的 id 不能被当成「确认删除」。
        assert_ne!(save_delete_id(Some(3)), save_confirm_id(Some(3)));
        assert_ne!(save_load_id(Some(3)), save_delete_id(Some(3)));
        // 存档前缀与既有的章节删除前缀不能撞。
        assert_ne!(parse(&save_delete_id(Some(3))), Some(Action::Delete("3".into())));
        assert_eq!(parse("save-delete:"), None);
        assert_eq!(parse("save-load:x"), None);
        // 自动存档的键是 `auto`，不能和槽号字符串混。
        assert_eq!(save_key(None), "auto");
        assert_eq!(save_key(Some(0)), "0");
    }

    /// 「刷新」到底刷什么 —— 用户点了按钮却什么都没变，就是这里判错了。
    ///
    /// 2026-09 真机：装了「共通线 第一章·归乡」、存档四条却全标「未安装」，
    /// 用户点「刷新」**仍然**没变。两个原因叠在一起：徽章是收到回包那一刻算好存起来的
    /// （改成渲染时现算，见 `Snapshot::saves`），以及存档页的刷新**只拉存档、不拉
    /// 已安装章节列表** —— 后一条就钉在这条用例上。
    #[test]
    fn saves_page_refresh_repulls_the_chapter_list_too() {
        let plan = parse(SAVES_REFRESH).expect("存档页的刷新").refresh_plan();
        assert!(
            plan.pack_list && plan.save_list,
            "存档页的刷新必须两样都刷（存档 + 已安装章节列表），实际 {plan:?}"
        );

        // 章节页的刷新只管章节列表：那一页上没有存档行，顺手拉存档只是白花一次往返。
        let chapter = parse(REFRESH).expect("章节页的刷新").refresh_plan();
        assert_eq!(chapter, RefreshPlan { pack_list: true, ..RefreshPlan::default() });

        // 别的动作都不是「刷新」（别让「导出」这类动作跟着去拉列表）。
        for id in [SAVES_EXPORT, SAVES_IMPORT, CONNECT, CLEAR_CACHE] {
            assert_eq!(parse(id).expect(id).refresh_plan(), RefreshPlan::default(), "{id}");
        }
    }

    /// 统计页的刷新只拉统计：这一页的结论全部来自手环上那张按天的表。
    #[test]
    fn stats_page_refresh_only_repulls_the_stats() {
        let plan = parse(STATS_REFRESH).expect("统计页的刷新").refresh_plan();
        assert_eq!(plan, RefreshPlan { stats_list: true, ..RefreshPlan::default() });
        assert_eq!(refresh_steps(plan), vec![RefreshStep::StatsList]);

        // 存档页的刷新**不许**顺手拉统计：那一页上没有统计行，白花一次往返。
        let saves = parse(SAVES_REFRESH).expect("存档页的刷新").refresh_plan();
        assert!(!saves.stats_list);
        assert_eq!(refresh_steps(saves), vec![RefreshStep::PackList, RefreshStep::SaveList]);
    }

    /// 「刷新」的发送顺序：**章节列表在前、存档在后**。
    ///
    /// 为什么要钉这条：插件侧只有一个待答请求的坑位（`src/request.rs` 的 `Slot`），
    /// 后发的那个才拿得到超时重发。旧实现是存档先发、章节列表后发，
    /// 于是最容易丢、最需要兜底的存档列表反而失去了重试资格 ——
    /// 用户看到的就是「存档页刷不动/报超时，再点一次就好了」。
    /// 顺序错了不会有任何编译错误，只会在真机上以「偶发超时」的形态露头，所以要有用例。
    #[test]
    fn refresh_sends_chapter_list_before_saves() {
        let saves_page = parse(SAVES_REFRESH).expect("存档页的刷新").refresh_plan();
        assert_eq!(
            refresh_steps(saves_page),
            vec![RefreshStep::PackList, RefreshStep::SaveList],
            "存档页刷新：先章节列表（给存档让出坑位），再存档列表"
        );

        // 章节页只刷章节列表 —— 顺序表里就不该出现存档这一步。
        let chapter_page = parse(REFRESH).expect("章节页的刷新").refresh_plan();
        assert_eq!(refresh_steps(chapter_page), vec![RefreshStep::PackList]);

        // 两样都不刷的动作：一步都没有，插件侧据此不发任何请求。
        assert!(refresh_steps(RefreshPlan::default()).is_empty());
    }

    #[test]
    fn junk_is_rejected_instead_of_silently_matching() {
        assert_eq!(parse("sync:abc"), None);
        assert_eq!(parse("line:不存在的线"), None);
        assert_eq!(parse("delete:"), None);
        assert_eq!(parse("logfilter:nope"), None);
        assert_eq!(parse("nav:nope"), None);
        assert_eq!(parse(""), None);
        assert_eq!(parse("随机文字"), None);
        // 阶段 0 的两个自检动作已经整块删除（Dialog 路线作废，见 docs/插件开发注意事项.md 第 7 节）：
        // 它们的 id 必须解析不出来，谁再把这套装置加回来就会在这里露馅。
        assert_eq!(parse("save-probe-dialog"), None);
        assert_eq!(parse("save-probe-fs"), None);
    }

    /// 章节包删除的**二次确认已整块删除**（插件里有副本，删掉随时能重新同步）。
    ///
    /// 存档删除的确认还在 —— 那是不可恢复的操作，两者不能一起删。
    /// 这两个 id 必须解析不出来，谁把章节的确认流程加回来就会在这里露馅。
    #[test]
    fn chapter_delete_confirmation_stays_deleted() {
        assert_eq!(parse("confirm:共通线1·归乡"), None, "章节删除不再有二次确认");
        assert_eq!(parse("cancel-delete"), None, "取消删除这个动作随确认流程一起删除");
        // 但「删除」本身还在，而且没有 `confirm:` 这个前缀。
        assert_eq!(parse(&delete_id("共通线1·归乡")), Some(Action::Delete("共通线1·归乡".into())));
        // 存档那一套（不可恢复）保留确认：前缀与动作都还在。
        assert_eq!(parse(&save_confirm_id(Some(1))), Some(Action::SavesConfirmDelete(Some(1))));
        assert_eq!(parse(SAVES_CANCEL_DELETE), Some(Action::SavesCancelDelete));
    }

    #[test]
    fn nav_ids_do_not_collide_with_hover_ids() {
        // `nav:library` 的悬停 id 是 `hover:nav:library`，两者解析结果必须不同。
        let hover = parse(&hover_id(&Page::Library.action())).unwrap();
        assert_eq!(hover, Action::Hover("nav:library".into()));
        assert!(hover.is_ui_only());
        assert_eq!(parse("nav:library"), Some(Action::Nav(Page::Library)));
    }

    #[test]
    fn line_all_is_wildcard_not_a_line_name() {
        assert_eq!(LINE_ALL, "line:*");
        assert_eq!(parse("line:*"), Some(Action::Line(None)));
        assert_eq!(parse("line:番外"), Some(Action::Line(Some("番外".into()))));
    }
}
