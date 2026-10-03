//! 推送页：连接进度、章节库、手环已安装章节 —— 合并了旧的「概览 / 章节 / 设备」三页。
//!
//! 三块内容原本摊在三页里各画一遍同一个事实（连接走到哪、要同步哪几章、手环上有哪些章）。
//! 现在按用户任务收成一页：**要往手环里推章节，就都在这一页**。
//! 传输进度不在这里 —— 它由外壳底部的常驻传输条承担（见 `glass::transfer_strip`）；
//! 连接状态与主操作在页面**最上方**的常驻设备条里（见 `glass::device_bar`），本页不再重复。

use super::super::actions;
use super::super::glass::{
    accent_chip, alpha, empty, error_card, meta, panel, primary_button,
    quiet_button, scrollable_segmented, section, section_with_action, state_badge,
};
use super::super::human_bytes;
use super::super::node::{Node, Tag, badge, label};
use super::super::snapshot::{InstalledView, PackView, Snapshot, StatusKind};
use super::super::theme::*;

/// 章节列表的滚动高度上限。
const LIB_MAX_HEIGHT: u32 = 440;
/// 手环已安装列表的滚动高度上限。
const INSTALLED_MAX_HEIGHT: u32 = 320;

pub fn render(snapshot: &Snapshot) -> Node {
    let mut page = Node::new(Tag::Div).full().column().gap(GAP_LG);
    // 失败卡片排在最前：出事了就该先看见它。
    if let Some(error) = snapshot.error.as_ref() {
        page = page.child(error_card(error));
    }
    // 1.x 旧代判定就紧跟在失败卡后面：它比下面所有内容都优先 —— 同步入口已经被禁用，
    // 用户得先知道「为什么按钮都点不动」。
    page.child(legacy_notice(snapshot))
        .child(summary(snapshot))
        .child(filters(snapshot))
        .child(chapter_list(snapshot))
        .child(broken_notice(snapshot))
        .child(installed_section(snapshot))
}

/// 章节库汇总 + 「接下来」那一件事 + 批量同步。
fn summary(snapshot: &Snapshot) -> Node {
    let mut card = panel(CARD_RADIUS)
        .pad(CARD_PAD)
        .gap(GAP_SM)
        .child(label("章节库", SIZE_TITLE, TEXT_MAIN).weight(650));

    if snapshot.library.is_empty() {
        // 读不出来时要看得见原因，不能只显示一个零。
        return card.child(label(&snapshot.library_error, SIZE_TINY, BAD));
    }

    let pending = snapshot.pending_count();
    card = card.child(meta(format!(
        "共 {} 章 · 阅读时长约 {} 小时 · {} · 已装 {}/{}",
        snapshot.library.len(),
        snapshot.total_hours(),
        human_bytes(snapshot.library_bytes()),
        snapshot.installed_count(),
        snapshot.library.len()
    )));

    // 「接下来」只在最要紧的一件事上说一句 —— 上一版这里是一张列了 3~5 条的清单。
    let (kind, text) = snapshot.next_step();
    card = card.child(label(text, SIZE_SMALL, kind.color()));

    card.child(primary_button(
        &format!("同步剩余 {pending} 章"),
        actions::SYNC_ALL,
        pending > 0
            && !snapshot.is_transferring()
            && snapshot.device.connected
            && snapshot.device.alive
            // 1.x 旧代游戏读不了 2.0 的章节包：同步入口（含下面每章的「同步」）整体禁用，
            // 原因由最上面的 legacy_notice 卡说，按钮只负责不可点。
            && !snapshot.band_game_legacy,
        snapshot,
    ))
}

/// 线路筛选：外层 `Tag::Scroll` + `scroll("x")`，里面每一项 `shrink(0)` ，
/// 任何 ≤400px 宽度下都只会滚动、不会把「番外」裁掉半个。
fn filters(snapshot: &Snapshot) -> Node {
    let mut items =
        vec![("全部".to_string(), actions::line_id(None), snapshot.line_filter.is_none())];
    for line in LINES {
        items.push((
            line.to_string(),
            actions::line_id(Some(line)),
            snapshot.line_filter.as_deref() == Some(line),
        ));
    }
    panel(CARD_RADIUS)
        .pad(CARD_PAD)
        .gap(GAP_SM)
        .child(label("按路线", SIZE_SMALL, TEXT_DIM))
        .child(scrollable_segmented(&items, true, snapshot))
}

/// 章节列表。
fn chapter_list(snapshot: &Snapshot) -> Node {
    let packs = snapshot.filtered_library();
    let mut container =
        Node::new(Tag::Scroll).full().scroll("y").maxh(LIB_MAX_HEIGHT).column().gap(GAP_XS);
    for pack in &packs {
        container = container.child(chapter_row(snapshot, pack));
    }
    if packs.is_empty() {
        container = container.child(empty("这条路线还没有章节"));
    }

    let hint = match snapshot.line_filter.as_deref() {
        Some(line) => format!("{line} · {} 章", packs.len()),
        None => format!("{} 章", packs.len()),
    };
    section("章节列表", Some(hint)).child(container)
}

/// 每章一行：序号砖 + 标题/元信息 + 状态/动作。
///
/// **这一行的 flex 结构是用户实机反馈后修过的，动它之前先读这段**：
/// 上一版序号砖没写 `shrink(0)`、文字列又写了 `minw(180)`，两者凑在一起的结果是
/// 400px 窗口里「序号砖被压扁」+「右边的同步按钮被裁掉一点」。
/// 现在每个元素只干一件事：序号砖 36×36 + `shrink(0)`；文字列 `grow(1)/shrink(1)` 是
/// 唯一允许变窄的东西；行内按钮 `shrink(0)`；行内 `gap` 用 `GAP`(12)。
fn chapter_row(snapshot: &Snapshot, pack: &PackView) -> Node {
    let (_, line_color) = chapter_line(&pack.title);
    // 手环断点指向的这一章：按钮从「同步/重传」换成「继续」，行描边用品牌粉挑出来 ——
    // 重连后用户要找的就是这一行（「重新连接手表后没有继续按钮」的原话诉求）。
    let is_resume = snapshot.resume_pack_number() == Some(pack.number);

    let number_tile = Node::new(Tag::Div)
        .w(36)
        .h(36)
        .shrink(0.0)
        .radius(10)
        .column()
        .align("center")
        .justify("center")
        .bg(&alpha(line_color, "22"))
        .child(label(pack.number.to_string(), SIZE_BODY, line_color).weight(700));

    let mut title_row = Node::new(Tag::Div)
        .row()
        .align("center")
        .gap(GAP_XS)
        .child(label(&pack.title, SIZE_BODY, TEXT_MAIN).weight(600));
    if pack.active {
        title_row = title_row.child(state_badge("同步中", StatusKind::Warn));
    } else if is_resume {
        title_row = title_row.child(state_badge("待续传", StatusKind::Warn));
    } else if pack.installed {
        title_row = title_row.child(state_badge("已装", StatusKind::Good));
    } else if pack.queued {
        title_row = title_row.child(state_badge("排队中", StatusKind::Warn));
    }

    let text = Node::new(Tag::Div)
        .column()
        .gap(3)
        .grow(1.0)
        .shrink(1.0)
        .child(title_row)
        .child(meta(pack.meta_line()));

    // 1.x 旧代：单章同步/继续一并禁用（原因由 legacy_notice 卡说）。
    let sync_enabled = !snapshot.is_transferring() && !snapshot.band_game_legacy;
    let action = if pack.active {
        state_badge("传输中", StatusKind::Warn)
    } else if is_resume {
        accent_chip("继续", &actions::sync_id(pack.number), sync_enabled, snapshot)
    } else {
        accent_chip(
            if pack.installed { "重传" } else { "同步" },
            &actions::sync_id(pack.number),
            sync_enabled,
            snapshot,
        )
    };

    let active = pack.active || is_resume;
    // ⚠️ **这一行不许挂 hover**：宿主 `ui-v3` 的 `render` 是**整段替换视图**，行上的
    // `on.enter/on.leave` 会在滚动时（指针下方的行随滚动变化）触发重渲染，把章节列表的
    // 滚动位置打回顶部 —— 行高亮不值得用「滚动位置丢失」去换。列表行一律静态底色。
    Node::new(Tag::Div)
        .full()
        .row()
        .align("center")
        .gap(GAP)
        .pad(10)
        .radius(ROW_RADIUS)
        .bg(SURFACE_SOFT)
        .border(1, &if active { alpha(ACCENT, "55") } else { STROKE_SOFT.to_string() })
        .child(number_tile)
        .child(text)
        .child(action)
}

/// 注册表里有、但 `pack.txt` 读不出来的章节。**不许静默**：`scan` 只读、不剔除，
/// 所以「手环上真没装这一章」和「注册表里那条读不出来」必须能分清。
fn broken_notice(snapshot: &Snapshot) -> Node {
    if snapshot.installed_broken.is_empty() {
        return Node::new(Tag::Div).full().column();
    }
    let names = snapshot.installed_broken.join("、");
    let badge_text = format!("{} 条登记读不出来", snapshot.installed_broken.len());
    Node::new(Tag::Div)
        .full()
        .column()
        .gap(GAP_XS)
        .pad(GAP)
        .radius(ROW_RADIUS)
        .bg(SURFACE_SOFT)
        .border(1, &alpha(WARN, "55"))
        .child(
            Node::new(Tag::Div)
                .full()
                .row()
                .align("center")
                .gap(GAP_XS)
                .child(state_badge(&badge_text, StatusKind::Warn))
                .child(label("", SIZE_SMALL, TEXT_DIM).grow(1.0)),
        )
        .child(label(names, SIZE_SMALL, TEXT_SUB))
        .child(label(
            "这些章节在手环上还有登记，但包内容读不出来。重新同步一次这一章就能恢复。",
            SIZE_SMALL,
            TEXT_DIM,
        ))
}

/// 手环上的游戏是 1.x 旧代：**卸载重装**引导卡。
///
/// 排在失败卡之后、章节库之前：同步入口（批量/单章/断点续传）此时都已禁用，
/// 这张卡负责回答「为什么点不动」与「接下来怎么办」。结论与办法各只有一句，
/// 都从 `Snapshot::band_game_legacy_hint` / `_action` 取 —— 判定在插件侧，
/// 文案只住这一处，别在状态行或别的页面再手写第二遍。
fn legacy_notice(snapshot: &Snapshot) -> Node {
    if !snapshot.band_game_legacy {
        return Node::new(Tag::Div).full().column();
    }
    Node::new(Tag::Div)
        .full()
        .column()
        .gap(GAP_XS)
        .pad(GAP)
        .radius(ROW_RADIUS)
        .bg(SURFACE_SOFT)
        .border(1, &alpha(BAD, "55"))
        .child(
            Node::new(Tag::Div)
                .full()
                .row()
                .align("center")
                .gap(GAP_XS)
                .child(state_badge("游戏版本过旧", StatusKind::Bad))
                .child(Node::new(Tag::Div).grow(1.0)),
        )
        .child(label(snapshot.band_game_legacy_hint(), SIZE_SMALL, TEXT_SUB))
        .child(label(snapshot.band_game_legacy_action(), SIZE_SMALL, TEXT_DIM))
}

/// 手环上已安装的章节（+ 刷新入口 + 旧版本残留说明）。
fn installed_section(snapshot: &Snapshot) -> Node {
    let hint = format!("{} 章 · {}", snapshot.installed_count(), human_bytes(snapshot.installed_bytes()));
    let refresh = quiet_button(
        "刷新",
        actions::REFRESH,
        snapshot.device.connected && snapshot.device.alive,
        snapshot,
    );
    let card = section_with_action("手环已安装章节", Some(hint), refresh);

    if snapshot.installed.is_empty() {
        return card.child(empty("手环上一章都没有，同步第一章就会出现在这里"));
    }

    let mut container =
        Node::new(Tag::Scroll).full().scroll("y").maxh(INSTALLED_MAX_HEIGHT).column().gap(GAP_XS);
    for record in &snapshot.installed {
        container = container.child(installed_row(snapshot, record));
    }

    let stale = snapshot.stale_installed().len();
    let note = if stale > 0 {
        format!(
            "标「旧版本」的 {stale} 条不在当前插件的章节表里。删掉再重新同步，就不会和新章节混在一起；删的只是手环上那份，插件里还有副本。"
        )
    } else {
        "删掉的只是手环上那份，插件里的副本还在，随时可以重新同步。".to_string()
    };
    card.child(container).child(meta(note))
}

/// 一条已安装章节：名字 + 体积 + （旧版本才有的徽章）+ 一个删除按钮。
/// 章节包删除**一步到位、没有二次确认**（插件里有完整副本，删错重传即可）。
fn installed_row(snapshot: &Snapshot, record: &InstalledView) -> Node {
    let text = Node::new(Tag::Div)
        .column()
        .gap(3)
        .grow(1.0)
        .shrink(1.0)
        .child(label(&record.name, SIZE_BODY, if record.stale { WARN } else { TEXT_MAIN }).weight(600))
        .child(meta(format!("{} · {} 个文件", human_bytes(record.bytes), record.files)));

    // 同样**不挂 hover**：它在纵向滚动列表里，理由见 `chapter_row`。
    Node::new(Tag::Div)
        .full()
        .row()
        .align("center")
        .gap(GAP_SM)
        .pad(10)
        .radius(ROW_RADIUS)
        .bg(SURFACE_SOFT)
        .child(text)
        .child_if(record.stale, badge("旧版本", WARN, WARN_BG))
        .child(quiet_button("删除", &actions::delete_id(&record.id), true, snapshot))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;

    /// 章节库页的文案：分组标题是「章节列表」、筛选小标题是「按路线」。
    #[test]
    fn group_title_and_filter_caption_use_the_new_wording() {
        let texts = render(&demo()).texts().join(" | ");
        assert!(texts.contains("章节列表"), "{texts}");
        assert!(texts.contains("按路线"), "{texts}");
        assert!(!texts.contains("内置章节"), "{texts}");
        assert!(!texts.contains("按线路看"), "{texts}");
    }

    /// 线路筛选条**横向可滚动**，每一项都 `shrink(0)`（窄窗里只滚不裁）。
    #[test]
    fn line_filter_scrolls_sideways_and_never_squeezes_its_items() {
        let tree = render(&demo());
        let areas = tree.find(|node| node.tag == Tag::Scroll && node.get("scroll") == Some("x"));
        assert!(!areas.is_empty(), "线路筛选条必须包在一层横向滚动区里");
        let bar = areas
            .iter()
            .find_map(|area| area.children.first().filter(|bar| bar.children.len() == LINES.len() + 1))
            .expect("滚动区里应该有分段控件本体");
        for item in &bar.children {
            assert_eq!(item.get("shrink"), Some("0"), "每一项都不能被 flex 压窄");
        }
    }

    /// 章节行：序号砖与行内按钮 `shrink(0)`、文字列 `grow(1)`、**没有 `minw(180)`**。
    #[test]
    fn chapter_rows_keep_the_number_and_the_button_intact() {
        let tree = render(&demo());
        let rows = tree.find(|node| {
            node.get("radius") == Some(ROW_RADIUS.to_string().as_str())
                && node.get("gap") == Some(GAP.to_string().as_str())
                && node.children.len() == 3
        });
        assert!(!rows.is_empty(), "样例里应该有章节行");
        for row in rows {
            let [number, text, action] = row.children.as_slice() else {
                panic!("章节行应有「序号砖 / 文字列 / 动作」三个子节点");
            };
            assert_eq!((number.get("w"), number.get("h")), (Some("36"), Some("36")));
            assert_eq!(number.get("shrink"), Some("0"));
            assert_eq!(text.get("grow"), Some("1"));
            assert_eq!(text.get("minw"), None, "`minw(180)` 正是按钮被挤出可视区的原因，不许回来");
            assert_eq!(action.get("shrink"), Some("0"));
        }
    }

    /// 章节行 / 已安装行**不许挂 hover**：宿主 render 整段替换视图，行上的 `on.enter/on.leave`
    /// 会在滚动时触发重渲染，把列表滚动位置打回顶部（用户实测报的就是这个）。
    #[test]
    fn list_rows_never_re_render_on_hover() {
        let tree = render(&demo());
        let hovered_rows = tree.find(|node| {
            node.get("radius") == Some(ROW_RADIUS.to_string().as_str())
                && (node.has("on.enter") || node.has("on.leave"))
        });
        assert!(hovered_rows.is_empty(), "滚动区里的列表行不许挂 hover（会导致滚动位置重置）");
    }

    /// 手环已安装清单在**这一页**，每章恰好一个删除按钮；刷新入口只有一个。
    #[test]
    fn installed_records_and_refresh_live_here_once() {
        let snapshot = demo();
        let tree = render(&snapshot);
        assert_eq!(tree.find(|node| node.get("on.click") == Some("refresh-list")).len(), 1, "刷新入口只该有一个");
        let delete_ids: Vec<&str> = tree
            .find(|node| node.text.as_deref() == Some("删除"))
            .iter()
            .filter_map(|node| node.get("on.click"))
            .collect();
        assert_eq!(delete_ids.len(), snapshot.installed.len(), "每条记录恰好一个删除按钮：{delete_ids:?}");
        let unique: std::collections::BTreeSet<&str> = delete_ids.iter().copied().collect();
        assert_eq!(unique.len(), delete_ids.len(), "同一条记录不该有两个删除按钮");
    }

    /// 只有「不正常」才挂徽章：一屏「正常」等于没有信息。
    #[test]
    fn only_stale_installed_records_get_a_badge() {
        let texts = render(&demo()).texts().join(" | ");
        assert!(texts.contains("旧版本"), "{texts}");
        assert!(!texts.contains("正常"), "{texts}");
    }

    /// 注册表里读不出来的条目**必须说出来**。
    #[test]
    fn broken_entries_are_surfaced_and_silent_when_none() {
        let mut snapshot = demo();
        let clean = render(&snapshot).texts().join(" | ");
        assert!(!clean.contains("读不出来"), "{clean}");

        snapshot.installed_broken = vec!["玲线2·初恋".to_string(), "结灯线3·相伴".to_string()];
        let texts = render(&snapshot).texts().join(" | ");
        assert!(texts.contains("2 条登记读不出来"), "{texts}");
        assert!(texts.contains("玲线2·初恋、结灯线3·相伴"), "要逐条点名：{texts}");
    }

    /// 本页**没有**连接 / 打开游戏按钮：它们在任务条的状态区（单一出口）。
    #[test]
    fn connect_and_launch_actions_live_in_the_mission_bar_only() {
        let tree = render(&demo());
        assert!(tree.find(|node| node.get("on.click") == Some("connect")).is_empty(), "连接按钮在任务条");
        assert!(tree.find(|node| node.get("on.click") == Some("launch")).is_empty(), "打开游戏按钮在任务条");
    }

    /// 手环上是 1.x 旧版游戏：卸载重装卡必须出现并说清「为什么、怎么办」，
    /// 所有同步入口（批量 + 每章 + 继续）一并禁用。
    #[test]
    fn legacy_game_shows_the_reinstall_card_and_disables_every_sync_entry() {
        let mut snapshot = demo();
        // 夹具默认「传输中」，那本身就会禁用同步按钮 —— 置空后才能看清 legacy 的独立作用。
        snapshot.transfer = None;
        snapshot.band_game_legacy = true;
        snapshot.band_version = "1.6.3".into();
        let tree = render(&snapshot);
        let texts = tree.texts().join(" | ");
        assert!(texts.contains("游戏版本过旧"), "{texts}");
        assert!(texts.contains("1.x 旧版（v1.6.3）"), "版本号要说出来：{texts}");
        assert!(texts.contains("卸载"), "{texts}");
        // 批量同步 + 每章同步/继续：全部不可点（禁用按钮不挂 on.click）。
        assert!(tree.find(|node| node.get("on.click") == Some(actions::SYNC_ALL)).is_empty());
        let chips: Vec<_> = tree
            .find(|node| node.text.as_deref().is_some_and(|t| t == "同步" || t == "重传" || t == "继续"));
        assert!(!chips.is_empty(), "样例里应该有章节行按钮");
        for chip in &chips {
            assert_eq!(chip.get("disabled"), Some("1"), "1.x 旧代时章节按钮必须禁用");
            assert_eq!(chip.get("on.click"), None);
        }
    }

    /// 正常 2.x（或还没握手）时这张卡**不许出现**：空卡片在页面顶部留一块白。
    #[test]
    fn legacy_card_is_hidden_unless_the_game_is_actually_legacy() {
        let mut snapshot = demo();
        // 同上：置空传输，才能验证「非 legacy 时同步按钮是活的」。
        snapshot.transfer = None;
        snapshot.band_version = "2.0.0".into();
        let tree = render(&snapshot);
        let texts = tree.texts().join(" | ");
        assert!(!texts.contains("游戏版本过旧"), "{texts}");
        assert!(!tree.find(|node| node.get("on.click") == Some(actions::SYNC_ALL)).is_empty());
        let chips = tree.find(|node| {
            node.text.as_deref().is_some_and(|t| t == "同步" || t == "重传" || t == "继续")
        });
        assert!(!chips.is_empty());
        for chip in &chips {
            assert_ne!(chip.get("disabled"), Some("1"), "非 legacy 的章节按钮不该禁用");
        }
    }
}
