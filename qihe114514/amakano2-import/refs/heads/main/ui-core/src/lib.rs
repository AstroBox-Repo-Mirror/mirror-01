//! 甜蜜女友2 导入插件的界面树。
//!
//! 这个 crate **不依赖宿主**（不是 wasm 组件、没有 `astrobox-ng-wit`），
//! 页面产出抽象的 [`Node`] 树，由插件侧的一层薄转换翻成宿主的 `ui_v3::Element`。
//!
//! 这么拆的三个理由：
//! 1. **能测**：插件本体是 wasm 组件，宿主机上跑不了 `cargo test`；界面逻辑放这里就能
//!    `cargo test -p amakano2-ui` 直接验证（聚合、筛选、动作解析、树结构）。
//! 2. **能看**：`preview_all` 把整棵树导出成 JSON，交给 `tools/ui-preview.mjs` 在浏览器里
//!    截图，装到手环前就能肉眼验收布局。
//! 3. **好换**：宿主 UI 接口（现在是 `ui-v3`）以后迭代，只需要改插件侧那一个转换函数。

mod actions;
mod fixture;
mod glass;
mod node;
mod pages;
mod saves;
mod snapshot;
mod theme;

pub use actions::{Action, RefreshPlan, RefreshStep, parse as parse_action, refresh_steps};
pub use node::{Node, Tag};
pub use saves::{BAND_SAVES_FIXTURE, BandSaves, band_envelope, band_rows};
#[cfg(not(target_arch = "wasm32"))]
pub use saves::BAND_SAVES_FIXTURE_PATH;
pub use snapshot::{
    CHUNK_OPTIONS, DeviceView, InstalledView, Limits, LogFilter, LogLevel, LogLine, MAX_SLOTS,
    PackView, Page, ReadingStatsView, RecentDayView, ResumeView, SaveError, SaveExportView,
    SaveImportView, SaveSlotView, Snapshot, StatusKind, TransferView, auto_save_row, chunk_label,
    manual_save_count, missing_save_count,
};
pub use theme::PRESS_TIMEOUT_MS;
pub use theme::{civil_from_days, format_time_ms};

use serde_json::{Value, json};

/// 体积的人话写法（进位与旧实现的输出保持一致，别改动格式）。
pub fn human_bytes(bytes: usize) -> String {
    if bytes >= 1_048_576 {
        format!("{:.2} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

/// 按当前页生成整棵节点树。插件侧拿到后转成宿主元素并渲染。
pub fn build(snapshot: &Snapshot) -> Node {
    pages::render(snapshot)
}

/// 导出当前页为预览器认识的 JSON。
pub fn to_json(snapshot: &Snapshot) -> String {
    let document =
        json!({ "pages": [{ "name": snapshot.page.label(), "tree": build(snapshot).to_json() }] });
    serde_json::to_string_pretty(&document).unwrap_or_default()
}

/// 导出全部页面，供 `tools/ui-preview.mjs` 一次性渲染截图。
///
/// 末尾额外补一张**「存档通道不可用」**的存档页（`save_protocol = None` + `saves_unsupported`）：
/// 那张卡只在这个状态下出现，而它正是用户实机截图报上来的地方（同一件事被说了两遍、一红一黄）。
/// 预览里必须能直接看到修好的样子，否则每次都得靠「装到真机上碰运气」才发现回归。
pub fn preview_all(snapshot: &Snapshot) -> String {
    let mut pages: Vec<Value> = Page::ALL
        .into_iter()
        .map(|page| {
            let mut one = snapshot.clone();
            one.page = page;
            json!({ "name": page.label(), "tree": build(&one).to_json() })
        })
        .collect();
    // 通道被版本卡住的那一版存档页：手环没回 hello-ok，所以协议是 None、
    // 「版本过旧」那张卡出现（结论 + 怎么办各一句）。样本里的其余状态照旧。
    let mut blocked = snapshot.clone();
    blocked.page = Page::Saves;
    blocked.save_protocol = None;
    blocked.saves_unsupported = true;
    blocked.saves_error.clear();
    blocked.band_version.clear();
    pages.push(json!({ "name": BLOCKED_VIEW_NAME, "tree": build(&blocked).to_json() }));
    serde_json::to_string_pretty(&json!({ "pages": pages })).unwrap_or_default()
}

/// 预览文档里那一版「通道不可用」的视图名（截图时靠它定位这一段）。
pub const BLOCKED_VIEW_NAME: &str = "存档（通道不可用）";

// ------------------------------------------------------------------ 预览样本

/// 章节表在仓库里的位置（测试与预览的当前目录是 ui-core/）。
#[cfg(not(target_arch = "wasm32"))]
const INDEX_PATH: &str = "../packs/index.json";

/// 预览/测试用的样本快照。
///
/// 章节表**直接读真实生成的 `packs/index.json`**，所以截图看到的列表、
/// 时长、体积与插件里发出去的一模一样 —— 不会出现「预览很美、真机数据不同」这种事。
#[cfg(not(target_arch = "wasm32"))]
pub fn demo() -> Snapshot {
    let library = demo_library();
    let installed = demo_installed(&library);
    // 存档只带**原始对象**：界面行由 `Snapshot::saves()` 用当前已安装章节列表现算，
    // 预览与真机因此走的是同一条路（存好的行是上一版那份 bug 的来源）。
    let (save_auto, save_slots) = demo_saves();
    let total: usize = library.iter().map(|pack| pack.bytes).sum();
    let received = total / 3;
    // 先把借用出来的值变成自有数据，再把 library 移进快照里。
    let active = library
        .iter()
        .find(|pack| pack.number == 5)
        .map_or_else(|| "共通线 第五章·结缘".to_string(), |pack| pack.title.clone());
    let resume = library.iter().find(|pack| pack.number == 4).map(|pack| ResumeView {
        pack_id: pack.id.clone(),
        chapter: pack.title.clone(),
        percent: 62,
        received: pack.bytes * 62 / 100,
        total: pack.bytes,
        files_done: 17,
        resume_from: 28,
    });

    Snapshot {
        page: Page::Overview,
        version: demo_version(),
        device: DeviceView {
            name: "小米手环 10".into(),
            addr: "AA:BB:CC:DD:EE:FF".into(),
            connected: true,
            alive: true,
            probe_attempt: 1,
            probe_max: 12,
        },
        library,
        library_error: String::new(),
        installed,
        transfer: Some(TransferView {
            chapter: active,
            percent: 33,
            received,
            total,
            chunks_done: 41,
            chunks_total: 109,
            speed_kbps: 128.4,
            rtt_ms: 42,
            retries: 1,
            resumed: false,
            ready: false,
            started: true,
            chunk_bytes: 8192,
        }),
        resume,
        cache_bytes: 462_848,
        cache_files: 17,
        chunk_bytes: 8192,
        queue: vec![6, 7, 8, 9],
        status: "已连接《甜蜜女友2》，章节列表已同步".into(),
        status_kind: StatusKind::Good,
        auto_launch: true,
        hover: None,
        // 预览里让「重新连接」保持按下态，好让库那套按压反馈在截图上看得见。
        pressed: Some("connect".into()),
        line_filter: None,
        save_auto,
        save_slots,
        saves_error: String::new(),
        confirm_delete_save: None,
        save_protocol: Some(1),
        saves_unsupported: false,
        saves_busy: false,
        band_version: "0.1.0".into(),
        saves_notice: "已连接手环，存档列表已刷新".into(),
        // 导出那一行：条数就是真机回包里的 3 条，**字节数按真机那份信封算**（见
        // `demo_export_bytes()`）——「约 7 KB」这类话在窄窗下要放得下，得拿真数字量。
        save_export: Some(SaveExportView { bytes: demo_export_bytes(), slots: 3, verified: true }),
        save_import: None,
        stats: Some(demo_stats()),
        stats_error: String::new(),
        stats_supported: true,
        stats_busy: false,
        logs: demo_logs(),
        log_errors: 1,
        log_warns: 2,
        log_filter: LogFilter::All,
        // 预览里也显示真实图标（真机侧同样读 icon.png）。
        brand: inline_asset("../icon.png", "image/png"),
        limits: Limits::default(),
    }
}

/// 预览里「上次导出」那一行的字节数：按**真机回包**里那 3 条存档组一份导出信封，
/// 量它序列化后的长度（与真机导出走同一套字段：`format` / `save_version` / `protocol` /
/// `exported_at` / `app_version` / `version_code` / `auto_save` / `slots`）。
///
/// 预览里编一个「约 7 KB」很容易，但这条文案在 400px 窄窗下到底放不放得下，
/// 得拿真数字量 —— 所以这里真算一遍，失败了退回 0（截图里会显示「约 0 KB」，
/// 一眼能看出是样本坏了，而不是假装成功）。
#[cfg(not(target_arch = "wasm32"))]
fn demo_export_bytes() -> usize {
    let Ok(message) = serde_json::from_str::<Value>(BAND_SAVES_FIXTURE) else { return 0 };
    let Ok(saves) = band_envelope(&[message]) else { return 0 };
    let envelope = json!({
        "format": "amakano2-saves",
        "save_version": 1,
        "protocol": 1,
        "exported_at": saves
            .auto_save
            .as_ref()
            .and_then(|auto| auto.get("savedAt"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
        "app_version": demo_version(),
        "version_code": 65,
        "auto_save": saves.auto_save,
        "slots": saves.slots,
    });
    serde_json::to_string_pretty(&envelope).map(|text| text.len()).unwrap_or(0)
}

/// 预览用的阅读统计样本。
///
/// 结构与**手环侧 `common/reading-stats.js` 的 `envelope()` 逐字段一致**
/// （数值 + 手环算好的界面文案 + 最近明细），所以截图看到的排版就是真机上的排版。
/// 数字故意用「读了一阵子」的那一档：能看出「几小时几分」这种长文案在窄窗里放不放得下。
#[cfg(not(target_arch = "wasm32"))]
fn demo_stats() -> ReadingStatsView {
    ReadingStatsView {
        today: "2026-09-12".into(),
        total_day_count: 23,
        longest_date: "2026-08-31".into(),
        reading_days_label: "连续 6 天".into(),
        total_days_label: "23 天".into(),
        total_label: "12 小时 41 分".into(),
        today_label: "1 小时 8 分".into(),
        longest_label: "2 小时 35 分（8月31日）".into(),
        recent: vec![
            RecentDayView { date: "2026-09-06".into(), label: "52 分钟".into(), seconds: 3_120 },
            RecentDayView { date: "2026-09-07".into(), label: "1 小时 20 分".into(), seconds: 4_800 },
            RecentDayView { date: "2026-09-08".into(), label: "35 分钟".into(), seconds: 2_100 },
            RecentDayView { date: "2026-09-09".into(), label: "1 小时 46 分".into(), seconds: 6_360 },
            RecentDayView { date: "2026-09-10".into(), label: "48 分钟".into(), seconds: 2_880 },
            RecentDayView { date: "2026-09-11".into(), label: "2 小时 2 分".into(), seconds: 7_320 },
            RecentDayView { date: "2026-09-12".into(), label: "1 小时 8 分".into(), seconds: 4_080 },
        ],
    }
}

/// 把仓库里的自带资源内联成 `data:` URI（预览与真机走同一套做法：
/// 真机侧在插件的 `inline_asset`，图片只能走 `IMAGE` 元素的内容）。
#[cfg(not(target_arch = "wasm32"))]
fn inline_asset(relative: &str, mime: &str) -> Option<std::sync::Arc<str>> {
    use base64::Engine;
    let bytes = std::fs::read(relative).ok()?;
    Some(std::sync::Arc::from(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    )))
}

/// 插件版本号：直接读插件目录下的 `manifest.json`，预览里显示的版本不会和真实插件漂。
#[cfg(not(target_arch = "wasm32"))]
fn demo_version() -> String {
    std::fs::read_to_string("../manifest.json")
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| value.get("version").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| "未知".into())
}

/// 读真实的章节表；读不到就退回到一份 3 章的小样本，保证预览器仍可跑。
#[cfg(not(target_arch = "wasm32"))]
fn demo_library() -> Vec<PackView> {
    std::fs::read_to_string(INDEX_PATH)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| value.get("packs").and_then(Value::as_array).cloned())
        .map(|packs| {
            packs
                .iter()
                .enumerate()
                .map(|(index, entry)| {
                    let number = entry["number"].as_u64().unwrap_or(index as u64 + 1) as usize;
                    PackView {
                        number,
                        id: entry["id"].as_str().unwrap_or_default().to_string(),
                        title: entry["title"].as_str().unwrap_or_default().to_string(),
                        minutes: entry["minutes"].as_u64().unwrap_or_default() as usize,
                        bytes: entry["bytes"].as_u64().unwrap_or_default() as usize,
                        scenes: entry["scenes"].as_u64().unwrap_or_default() as usize,
                        dialogues: entry["dialogues"].as_u64().unwrap_or_default() as usize,
                        installed: number <= 4,
                        queued: number == 6,
                        active: number == 5,
                    }
                })
                .collect()
        })
        .unwrap_or_else(demo_library_fallback)
}

#[cfg(not(target_arch = "wasm32"))]
fn demo_library_fallback() -> Vec<PackView> {
    vec![
        PackView {
            number: 1,
            id: "共通线1·归乡".into(),
            title: "共通线 第一章·归乡".into(),
            minutes: 54,
            bytes: 735_837,
            scenes: 41,
            dialogues: 1066,
            installed: false,
            queued: false,
            active: false,
        },
        PackView {
            number: 2,
            id: "共通线2·转校生".into(),
            title: "共通线 第二章·转校生".into(),
            minutes: 57,
            bytes: 933_961,
            scenes: 56,
            dialogues: 1136,
            installed: false,
            queued: false,
            active: false,
        },
    ]
}

/// 手环已安装的记录：编号 1~4 视为已装，外加一条真实的旧版本残留 `common-01`，
/// 好让设备页的「旧版本残留」提醒在预览里也能看到。
#[cfg(not(target_arch = "wasm32"))]
fn demo_installed(library: &[PackView]) -> Vec<InstalledView> {
    let mut installed: Vec<InstalledView> = library
        .iter()
        .filter(|pack| pack.installed)
        .map(|pack| InstalledView {
            id: pack.id.clone(),
            name: pack.title.clone(),
            bytes: pack.bytes,
            files: 20 + pack.number,
            stale: false,
        })
        .collect();
    installed.push(InstalledView {
        id: "common-01".into(),
        name: "common-01（旧版本）".into(),
        bytes: 412_000,
        files: 12,
        stale: true,
    });
    installed
}

/// 预览/测试用的存档样本：**手环上的原始对象**（自动存档 + 手动槽），不是算好的界面行。
///
/// **它就是真机回包**（[`BAND_SAVES_FIXTURE`]）：2026-09 从小米手环 10 上抓下来的
/// `amakano.saves.data` 原文 → 走**插件解包回包的同一个函数**（[`band_envelope`]）。
/// 界面行（含「未安装」判定）由 `Snapshot::saves()` 用当前已安装章节列表现算，
/// 与真机上插件每次渲染做的事完全一样。
///
/// 于是截图里的槽位数、章节名、场景号、写档时间与真机上看到的是**同一份数据**，
/// 不存在「预览很美、真机数据不同」这种事 —— 上一版 bug（界面永远 0 条）恰恰是因为
/// 两侧各自编样本：插件从外层消息读 `saves`（读不到），预览也自己拼一份（看不出问题）。
///
/// 已安装章节列表由样本自己给（编号 1~4 已装）：手里的这份真机回包三条槽都属于
/// 第 1 章，所以它们都能读 —— 这也就是手环上当时的真实样子。
/// 解包失败时退回空（预览仍能打开，不会因为夹具坏了整页空白）。
#[cfg(not(target_arch = "wasm32"))]
fn demo_saves() -> (Option<Value>, Vec<Value>) {
    let Ok(message) = serde_json::from_str::<Value>(BAND_SAVES_FIXTURE) else {
        return (None, Vec::new());
    };
    let Ok(saves) = band_envelope(&[message]) else {
        return (None, Vec::new());
    };
    (saves.auto_save, saves.slots)
}

#[cfg(not(target_arch = "wasm32"))]
fn demo_logs() -> Vec<LogLine> {    let raw = [
        (LogLevel::Info, "INFO Amakano2 pack importer loaded"),
        (LogLevel::Info, "INFO chapter packs available count=15 version=\"0.2.1\""),
        (LogLevel::Info, "INFO connecting device addr=\"AA:BB:CC:DD:EE:FF\""),
        (LogLevel::Info, "INFO launched watch app package=\"cn.amakanotwo.qihe\""),
        (LogLevel::Warn, "WARN probe timeout, retrying attempt=1"),
        (LogLevel::Info, "INFO handshake ok, installed packs=5"),
        (LogLevel::Info, "INFO begin pack=\"共通线 第五章·结缘\" files=25 bytes=890362"),
        (LogLevel::Info, "INFO chunk acked index=41 rtt=42ms"),
        (LogLevel::Error, "ERROR request pack list failed: device busy"),
    ];
    raw.into_iter().map(|(level, text)| LogLine { level, text: text.to_string() }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_keeps_the_old_format() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2 KB");
        assert_eq!(human_bytes(1_572_864), "1.50 MB");
    }

    #[test]
    fn every_page_builds_without_host() {
        let snapshot = demo();
        for page in Page::ALL {
            let mut one = snapshot.clone();
            one.page = page;
            let tree = build(&one);
            assert_eq!(tree.tag, Tag::Div);
            assert!(!tree.children.is_empty(), "{page:?} 页渲染出来是空的");
        }
    }

    #[test]
    fn preview_export_has_one_entry_per_page() {
        let document: Value = serde_json::from_str(&preview_all(&demo())).expect("预览 JSON 必须合法");
        // 七个页面各一条，**外加**末尾那条「存档（通道不可用）」——那张卡只在通道被
        // 版本卡住时才出现，必须在预览里看得见（用户实机报的就是它）。
        assert_eq!(document["pages"].as_array().map(Vec::len), Some(Page::ALL.len() + 1));
        assert_eq!(document["pages"][0]["name"], "概览");
        assert_eq!(document["pages"][0]["tree"]["tag"], "div");
        let last = &document["pages"][Page::ALL.len()];
        assert_eq!(last["name"], BLOCKED_VIEW_NAME);
        assert_eq!(last["tree"]["tag"], "div");
    }

    #[test]
    fn single_page_export_names_the_page() {
        let document: Value = serde_json::from_str(&to_json(&demo())).expect("预览 JSON 必须合法");
        assert_eq!(document["pages"][0]["name"], "概览");
    }

    /// 界面上不允许出现空文本节点（占位符漏填会在这里炸）。
    #[test]
    fn no_empty_labels_in_any_page() {
        let snapshot = demo();
        for page in Page::ALL {
            let mut one = snapshot.clone();
            one.page = page;
            let tree = build(&one);
            let empties: Vec<&str> =
                tree.find(|node| node.text.as_deref() == Some("")).iter().map(|node| node.tag.wire()).collect();
            assert!(empties.is_empty(), "{page:?} 页有空文本节点：{empties:?}");
        }
    }

    /// 样本必须真的来自生成的章节表，而且每一章的标题都要能被线路识别 ——
    /// 标题一改名，线路筛选就会静默失效，这条测试专门盯它。
    #[test]
    fn demo_matches_generated_pack_index_and_routes() {
        let snapshot = demo();
        assert!(snapshot.library.len() >= 3, "样本章节太少，章节表大概没读到");
        for pack in &snapshot.library {
            assert!(!pack.id.is_empty(), "第 {} 章缺少 id", pack.number);
            assert!(!pack.title.is_empty(), "第 {} 章缺少标题", pack.number);
            assert!(pack.minutes > 0 && pack.bytes > 0, "{} 的时长/体积不合理", pack.title);
            assert_ne!(
                crate::theme::chapter_line(&pack.title).0,
                "其他",
                "标题「{}」没被任何线路识别，筛选会漏掉它",
                pack.title
            );
        }
        let total: usize = snapshot.library.iter().map(|pack| pack.bytes).sum();
        assert_eq!(total, snapshot.library_bytes());
        assert_eq!(snapshot.installed_count(), snapshot.installed.len());
    }

    #[test]
    fn demo_marks_pending_and_installed_consistently() {
        let snapshot = demo();
        let stale = snapshot.stale_installed().len();
        assert!(snapshot.installed_count() > 0 && snapshot.pending_count() > 0, "样本要同时体现「已装」和「未装」");
        // 旧版本残留不算内置章节，所以要减掉它才是「已装的内置章节」。
        assert_eq!(
            snapshot.installed_count() - stale + snapshot.pending_count(),
            snapshot.library.len()
        );
        assert_eq!(stale, 1, "样本里要有一条旧版本残留");
        // 每一章的「已装」标记都要和手环记录对得上。
        for pack in &snapshot.library {
            let on_band = snapshot.installed.iter().any(|record| record.id == pack.id && !record.stale);
            assert_eq!(pack.installed, on_band, "{} 的已装标记与手环记录不一致", pack.title);
        }
    }

    /// 手工把整棵界面树导出给预览器：
    /// `cargo test -p amakano2-ui -- --ignored --nocapture dump_preview`
    ///
    /// `UI_TREE_SAVES_UNINSTALLED=1` 时把已安装章节列表清空再导出：那是「存档所在的章节
    /// 还没装到手环上」的对照图（四条存档都该带「未安装」徽章、底部出现那句提示）。
    /// 出图命令见 `tools/README.md` 与 `docs/插件开发注意事项.md` 6.8。
    #[test]
    #[ignore = "手工运行：导出界面树给 tools/ui-preview.mjs"]
    fn dump_preview() {
        let path = std::env::var("UI_TREE_OUT").unwrap_or_else(|_| "../../../work/ui-tree.json".into());
        let mut snapshot = demo();
        if std::env::var("UI_TREE_SAVES_UNINSTALLED").is_ok_and(|value| value == "1") {
            snapshot.installed.clear();
        }
        let json = preview_all(&snapshot);
        std::fs::write(&path, json).expect("写预览 JSON 失败");
        println!("已写出预览树：{path}");
    }

    /// 存档样本的守护：**它就是真机回包**，所以条数、章节名、场景号、写档时间
    /// 全都能对着 2026-09 那份抓包逐项核对。上一版 bug（界面永远 0 条）出在
    /// 「预览自己拼样本、插件自己解析」，这条用例把两件事钉在一起：
    /// 预览走的是插件解包回包的**同一个函数**。
    #[test]
    fn demo_saves_come_from_the_real_capture() {
        let snapshot = demo();
        let rows = snapshot.saves();
        assert_eq!(rows.iter().filter(|save| save.is_auto()).count(), 1, "自动存档只该有一条");
        assert_eq!(manual_save_count(&rows), 3, "真机回包里有 3 条手动存档");
        // 自动存档：场景 24（currentScene 23 + 1），包内偏移 23，写档时间原样。
        let auto = auto_save_row(&rows).expect("真机回包里有自动存档");
        assert_eq!(auto.chapter, "共通线 第一章·归乡");
        assert_eq!(auto.pack_id, "共通线1·归乡");
        assert_eq!(auto.pack_scene, 23);
        assert_eq!(auto.scene, 24);
        assert_eq!(auto.saved_at, 1_789_196_000_769);
        // 三条手动槽的槽号与场景号（`packScene` 1 / 4 / 26）。
        let slots: Vec<&SaveSlotView> = rows.iter().filter(|save| !save.is_auto()).collect();
        assert_eq!(slots.iter().map(|save| save.slot).collect::<Vec<_>>(), vec![Some(0), Some(1), Some(2)]);
        assert_eq!(slots.iter().map(|save| save.pack_scene).collect::<Vec<_>>(), vec![1, 4, 26]);
        assert_eq!(slots.iter().map(|save| save.saved_at).collect::<Vec<_>>(), vec![1_789_195_631_509, 1_789_195_982_418, 1_789_196_006_924]);
        assert!(slots.iter().all(|save| save.chapter == "共通线 第一章·归乡"));
        // 「未安装」判定必须真的来自已安装列表，而不是写死的字段。
        for save in &rows {
            let on_band = snapshot.installed.iter().any(|record| record.id == save.pack_id);
            assert_eq!(save.missing, !save.pack_id.is_empty() && !on_band, "{} 的未安装标记不对", save.chapter);
        }
        assert_eq!(missing_save_count(&rows), 0, "这份回包里第 1 章是装了的，四条都该可读");
        // 预览里「上次导出」那一行的数字也是真算出来的：3 条存档的信封约 1.5 KB
        // （不是编的 6412 —— 编出来的数字量不出窄窗下这句文案的真实宽度）。
        let export = snapshot.save_export.as_ref().expect("预览要展示导出结果那一行");
        assert_eq!(export.slots, 3);
        assert!(export.bytes > 1_000, "信封字节数应该是真算的，实际 {}", export.bytes);
    }

    /// 存档页能渲染，而且「未安装」徽章真的出现在树里。
    ///
    /// 「未安装」这一条**不能再靠预览样本**：预览样本现在是真机回包（那一章装在手上，
    /// 四条都可读），所以这里往**原始槽位**里再塞一条章节没装的手环存档 —— 徽章与
    /// 禁用的「读档」是用户实机报过的问题，必须继续守着。
    #[test]
    fn saves_page_renders_the_missing_badge() {
        let mut snapshot = demo();
        snapshot.page = Page::Saves;
        let orphan = json!({
            "storyId": "visual-novel-template",
            "chapter": 11,
            "chapterTitle": "结灯线1·序章",
            "packId": "结灯线1·序章", // 样本里没装这一章
            "packScene": 3,
            "currentScene": 3,
            "currentDialogue": 0,
            "savedAt": 1_789_196_120_000i64,
            "choice": [],
            "routeState": {},
            "settings": { "textSpeed": 25, "textSize": 22, "autoPlaySpeed": "medium" }
        });
        snapshot.save_slots.push(orphan);
        assert_eq!(missing_save_count(&snapshot.saves()), 1, "造出来的这条必须是未安装状态");

        let tree = build(&snapshot);
        assert_eq!(tree.tag, Tag::Div);
        let texts = tree.texts();
        assert!(texts.contains(&"未安装"), "存档页缺少「未安装」徽章：{texts:?}");
        assert!(texts.contains(&"读档") && texts.contains(&"删除"));
        assert!(texts.contains(&"导出到剪贴板") && texts.contains(&"从剪贴板导入"));
    }

    /// 「未安装」是**派生态**：章节列表一变，同一份存档行上的徽章必须跟着变。
    ///
    /// 这条用例就是 2026-09 真机那个 bug 的守护（四条存档全标「未安装」，点「刷新」也不变）：
    /// 上一版在收到 `amakano.saves.data` 那一刻就把行算好存进状态，之后章节列表再更新
    /// 也不会重算。这里全程**不动存档对象、也不重新读一次回包**，只换已安装章节列表，
    /// 重新取一次快照 —— 徽章就得跟着翻过来。
    #[test]
    fn save_badges_follow_the_current_installed_list_not_a_cached_row() {
        // 用**真机夹具**里的真实章节 id（手环上装的就是这一章）。
        let message: Value = serde_json::from_str(BAND_SAVES_FIXTURE).expect("夹具必须是合法 JSON");
        let band = band_envelope(&[message]).expect("真机回包必须能解包");
        let pack_id = band.auto_save.as_ref().expect("夹具里有自动存档")["packId"]
            .as_str()
            .expect("存档里带 packId")
            .to_string();
        assert_eq!(pack_id, "共通线1·归乡", "夹具里的章节 id 漂了，测试的前提就没了");

        let mut snapshot = Snapshot::default();
        snapshot.save_auto = band.auto_save.clone();
        snapshot.save_slots = band.slots.clone();

        // ① 手环上还没装这一章（或章节列表还没回来）：四条全「未安装」，一条都读不了。
        let rows = snapshot.saves();
        assert_eq!(rows.len(), 4, "1 条自动存档 + 3 条手动槽");
        assert!(rows.iter().all(|row| row.missing && !row.readable()), "这时四条都不该可读");
        assert_eq!(missing_save_count(&rows), 4);

        // ② 只把这一章加进已安装列表 —— 别的什么都不做，重新取一次快照。
        snapshot.installed = vec![InstalledView {
            id: pack_id.clone(),
            name: "共通线 第一章·归乡".into(),
            bytes: 735_837,
            files: 21,
            stale: false,
        }];
        let rows = snapshot.saves();
        assert!(rows.iter().all(|row| !row.missing && row.readable()), "章节装上了，四条都该可读");
        assert_eq!(missing_save_count(&rows), 0);

        // 存档对象一个字节都没动过：变的只有那份已安装列表。
        assert_eq!(snapshot.save_auto, band.auto_save);
        assert_eq!(snapshot.save_slots, band.slots);
    }

    /// 顶栏的「N/15 章已安装」与存档行的「未安装」徽章必须**同源**：都是 `installed`。
    ///
    /// 两处各看一份列表是这类 bug 的另一种长相（一个说装了、另一个说没装），
    /// 所以这里只动 `installed` 一份数据，断言两边一起翻。
    #[test]
    fn installed_count_and_save_badges_read_the_same_installed_list() {
        let mut snapshot = demo();
        let pack_id = "共通线1·归乡"; // 真机夹具里那四条存档都在这一章
        assert!(snapshot.installed.iter().any(|record| record.id == pack_id), "样本里第 1 章是装了的");
        assert_eq!(snapshot.installed_count(), snapshot.installed.len());
        assert!(snapshot.head_line().starts_with("已连接 · "), "顶栏要有已安装计数：{}", snapshot.head_line());
        assert_eq!(missing_save_count(&snapshot.saves()), 0);

        let before_count = snapshot.installed_count();
        snapshot.installed.retain(|record| record.id != pack_id);

        // 顶栏：计数跟着这份列表变（不是另一份「算好的数字」）。
        assert_eq!(snapshot.installed_count(), before_count - 1);
        assert_eq!(
            snapshot.head_line(),
            format!("已连接 · {}/{} 章已安装", before_count - 1, snapshot.library.len()),
            "顶栏那句必须来自当前已安装列表"
        );
        // 徽章：同一次改动、同一份列表，四条一起变成「未安装」。
        let rows = snapshot.saves();
        assert_eq!(missing_save_count(&rows), 4, "徽章必须跟着同一份列表变");
        for row in &rows {
            let on_band = snapshot.installed.iter().any(|record| record.id == row.pack_id);
            assert_eq!(row.missing, !on_band, "{} 的未安装标记与顶栏那份列表不一致", row.chapter);
        }
    }
}
