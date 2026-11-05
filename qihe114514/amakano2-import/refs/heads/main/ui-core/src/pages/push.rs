//! 推送页：连接进度、章节库、手环已安装章节 —— 合并了旧的「概览 / 章节 / 设备」三页。
//!
//! 三块内容原本摊在三页里各画一遍同一个事实（连接走到哪、要同步哪几章、手环上有哪些章）。
//! 现在按用户任务收成一页：**要往手环里推章节，就都在这一页**。
//! 传输进度不在这里 —— 它由外壳底部的常驻传输条承担（见 `glass::transfer_strip`）；
//! 连接状态与主操作在页面**最上方**的常驻设备条里（见 `glass::device_bar`），本页不再重复。

use super::super::actions;
use super::super::glass::{
    accent_chip, alpha, divider, error_card, ghost_button, group, group_heading_with, group_row,
    meta, primary_button, quiet_button, row_detail, row_title, scroll_list, segmented, state_badge,
};
use super::super::human_bytes;
use super::super::node::{Node, Tag, badge, label};
use super::super::snapshot::{InstalledView, PackView, Snapshot, StatusKind};
use super::super::theme::*;

/// 章节列表的滚动高度上限（与旧版同值：这个数字是当年按真机调出来的）。
const LIB_MAX_HEIGHT: u32 = 440;
/// 手环已安装列表的滚动高度上限。
const INSTALLED_MAX_HEIGHT: u32 = 320;

pub fn render(snapshot: &Snapshot) -> Node {
    let mut page = Node::new(Tag::Div).full().column();
    // 失败卡片排在最前：出事了就该先看见它。
    if let Some(error) = snapshot.error.as_ref() {
        page = page.child(error_card(error));
    }
    // 1.x 旧代紧跟其后：同步入口已经被禁用，用户得先知道「为什么按钮都点不动」。
    page.child(legacy_notice(snapshot))
        .child(action_row(snapshot))
        .child(filters(snapshot))
        .child(summary(snapshot))
        .child(chapter_list(snapshot))
        .child(broken_notice(snapshot))
        .child(installed_section(snapshot))
}

/// 页面顶部的动作区：**打开游戏**（连上之后才有意义）。
///
/// 「连接手环」已经提升到外壳（`glass::shell`），因为它不属于某一页 ——
/// 用户在存档页发现没连上时，不应该被要求先切回推送页。
fn action_row(snapshot: &Snapshot) -> Node {
    if !snapshot.device.connected {
        return Node::new(Tag::Div).full();
    }
    Node::new(Tag::Div)
        .full()
        .pad_t(GAP)
        .pad_b(GAP)
        .child(ghost_button("打开《甜蜜女友2》", actions::LAUNCH, true, snapshot))
}

/// 资源库汇总：一句话说清「还差多少、要多久、点哪里」。
fn summary(snapshot: &Snapshot) -> Node {
    if snapshot.library.is_empty() {
        return group()
            .pad_y(GAP)
            .child(label(&snapshot.library_error, TYPE_META, BAD));
    }
    let pending = snapshot.pending_count();
    let pending_bytes = snapshot.pending_bytes();
    let mut card = group().child(
        Node::new(Tag::Div)
            .full()
            .column()
            .gap(GAP_SM)
            .pad_y(GROUP_ROW_PAD_Y)
            .child(row_title("资源库"))
            .child(row_detail(format!(
                "{} 个资源包 · {} · 已装 {}/{}",
                snapshot.library.len(),
                human_bytes(snapshot.library_bytes()),
                snapshot.installed_count(),
                snapshot.library.len()
            )))
            .child(
                Node::new(Tag::Div)
                    .full()
                    .row()
                    .gap(GAP_SM)
                    .child(quick_stat("待同步", &format!("{pending} 个")).grow(1.0))
                    .child(quick_stat("待传", &human_bytes(pending_bytes)).grow(1.0))
                    .child(quick_stat("预计", &estimate(pending_bytes)).grow(1.0)),
            ),
    );
    let (kind, text) = snapshot.next_step();
    card = card.child(
        Node::new(Tag::Div)
            .full()
            .pad_y(10)
            .child(label(text, TYPE_META, kind.color())),
    );
    card.child(divider()).child(
        Node::new(Tag::Div)
            .full()
            .pad_y(GAP)
            .child(primary_button(
                &format!("同步剩余 {pending} 个"),
                actions::SYNC_ALL,
                pending > 0
                    && !snapshot.is_transferring()
                    && snapshot.device.connected
                    && snapshot.device.alive
                    && !snapshot.band_game_legacy,
                snapshot,
            )),
    )
}

/// 汇总卡里的一个数字格。
fn quick_stat(name: &str, value: &str) -> Node {
    Node::new(Tag::Div)
        .column()
        .gap(2)
        .pad(10)
        .radius(ROW_RADIUS)
        .bg(SURFACE_SOFT)
        .child(label(name, TYPE_BADGE, TEXT_DIM))
        .child(label(value, TYPE_BODY, TEXT_MAIN))
}

/// 按 140 KB/s 估一个「大概多久」（v3 稳定窗口下的保守值）。
/// 只用于汇总格的预期，不参与进度计算。
fn estimate(bytes: usize) -> String {
    if bytes == 0 {
        return "已齐".into();
    }
    let seconds = (bytes as f64 / (140.0 * 1024.0)).ceil() as u64;
    if seconds < 60 {
        format!("{seconds} 秒")
    } else if seconds < 3600 {
        format!("{} 分", (seconds + 59) / 60)
    } else {
        format!("{} 小时", (seconds + 3599) / 3600)
    }
}

/// 线路筛选：一排可横滑的胶囊。
///
/// 外层 `Tag::Scroll` + `scroll("x")` + 每一项 `shrink(0)`：
/// 6 项在 400px 窗口里必然超宽，没有滚动区时「番外」会被裁掉半个。
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
    Node::new(Tag::Scroll)
        .full()
        .row()
        .scroll("x")
        .pad_b(GAP)
        .child(segmented(&items, true, snapshot))
}

/// 章节列表。
fn chapter_list(snapshot: &Snapshot) -> Node {
    let packs = snapshot.filtered_library();
    let hint = match snapshot.line_filter.as_deref() {
        Some(line) => format!("{line} · {} 章", packs.len()),
        None => format!("{} 章", packs.len()),
    };
    let block = group_heading_with("资源包", meta(hint)).full();
    if packs.is_empty() {
        return block.child(group().child(
            Node::new(Tag::Div)
                .full()
                .pad_y(28)
                .align("center")
                .child(label("这条线路下没有章节", TYPE_META, TEXT_DIM)),
        ));
    }
    // 长列表包一层**验证过可用**的滚动区（见 `glass::scroll_list` 的说明：
    // 「整页会跟着窗口滚」没有真机证据，不能拿核心功能去赌）。
    let mut list = group();
    for (index, pack) in packs.iter().enumerate() {
        if index > 0 {
            list = list.child(divider());
        }
        list = list.child(chapter_row(snapshot, pack));
    }
    block.child(scroll_list(list, LIB_MAX_HEIGHT))
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
    // 手环断点指向的这一章：按钮换成「继续」，行底色挑出来 ——
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
        .bg(&alpha(line_color, "26"))
        .child(label(pack.number.to_string(), TYPE_BODY, line_color));

    let mut title_row = Node::new(Tag::Div)
        .row()
        .align("center")
        .gap(GAP_XS)
        .child(row_title(&pack.title).grow(1.0).shrink(1.0));
    if pack.active {
        title_row = title_row.child(state_badge("同步中", StatusKind::Warn));
    } else if is_resume {
        title_row = title_row.child(state_badge("待续传", StatusKind::Warn));
    } else if pack.installed {
        title_row = title_row.child(state_badge("已装", StatusKind::Good));
    } else if pack.queued {
        title_row = title_row.child(state_badge("排队中", StatusKind::Warn));
    }

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

    // ⚠️ **这一行不许挂 hover**：宿主 `ui-v3` 的 `render` 是**整段替换视图**，
    // 行上的 `on.enter/on.leave` 会在滚动时（指针下方的行随滚动变化）触发重渲染，
    // 把页面滚动位置打回顶部。列表行一律静态底色。
    group_row(
        // 序号砖与按钮 shrink(0)，文字列是这一行里唯一允许变窄的东西（中文按字断行）。
        Node::new(Tag::Div)
            .row()
            .align("center")
            .gap(GAP)
            .grow(1.0)
            .shrink(1.0)
            .child(number_tile)
            .child(
                Node::new(Tag::Div)
                    .column()
                    .gap(4)
                    .grow(1.0)
                    .shrink(1.0)
                    .child(title_row)
                    .child(row_detail(pack.meta_line())),
            ),
    )
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
    group()
        .pad_y(GAP)
        .child(
            Node::new(Tag::Div)
                .full()
                .column()
                .gap(GAP_XS)
                .child(state_badge("游戏版本过旧", StatusKind::Bad))
                .child(label(snapshot.band_game_legacy_hint(), TYPE_META, TEXT_SUB))
                .child(label(snapshot.band_game_legacy_action(), TYPE_META, TEXT_DIM)),
        )
}

fn installed_section(snapshot: &Snapshot) -> Node {
    let hint = format!("{} 章 · {}", snapshot.installed_count(), human_bytes(snapshot.installed_bytes()));
    let refresh = quiet_button(
        "刷新",
        actions::REFRESH,
        snapshot.device.connected && snapshot.device.alive,
        snapshot,
    );
    let block = group_heading_with("手环已安装章节", refresh).full();
    if snapshot.installed.is_empty() {
        return block.child(
            group().child(
                Node::new(Tag::Div)
                    .full()
                    .pad_y(24)
                    .align("center")
                    .child(label("手环上一章都没有，同步第一章就会出现在这里", TYPE_META, TEXT_DIM)),
            ),
        );
    }

    let mut list = group();
    for (index, record) in snapshot.installed.iter().enumerate() {
        if index > 0 {
            list = list.child(divider());
        }
        list = list.child(installed_row(snapshot, record));
    }

    let stale = snapshot.stale_installed().len();
    let note = if stale > 0 {
        format!(
            "标「旧版本」的 {stale} 条不在当前插件的章节表里。删掉再重新同步，就不会和新章节混在一起；删的只是手环上那份，插件里还有副本。"
        )
    } else {
        "删掉的只是手环上那份，插件里的副本还在，随时可以重新同步。".to_string()
    };
    block.child(scroll_list(list, INSTALLED_MAX_HEIGHT)).child(
        Node::new(Tag::Div)
            .full()
            .pad_t(GAP)
            .pad_x(2)
            .child(label(format!("{hint} · {note}"), TYPE_META, TEXT_DIM)),
    )
}


/// 一条已安装章节：名字 + 体积 + （旧版本才有的徽章）+ 一个删除按钮。
/// 章节包删除**一步到位、没有二次确认**（插件里有完整副本，删错重传即可）。
fn installed_row(snapshot: &Snapshot, record: &InstalledView) -> Node {
    let text = Node::new(Tag::Div)
        .column()
        .gap(3)
        .grow(1.0)
        .shrink(1.0)
        .child(label(&record.name, TYPE_ROW, if record.stale { WARN } else { TEXT_MAIN }))
        .child(row_detail(format!("{} · {} 个文件", human_bytes(record.bytes), record.files)));

    // 同样**不挂 hover**：它在纵向列表里，理由见 `chapter_row`。
    Node::new(Tag::Div)
        .full()
        .row()
        .align("center")
        .gap(GAP_SM)
        .pad_y(GROUP_ROW_PAD_Y)
        .child(text)
        .child_if(record.stale, badge("旧版本", WARN, WARN_BG))
        .child(quiet_button("删除", &actions::delete_id(&record.id), true, snapshot))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;

    /// 章节库页的文案：分组标题是「资源包」，筛选没有自己的小标题（一排胶囊自解释）。
    #[test]
    fn group_title_and_filter_caption_use_the_new_wording() {
        let texts = render(&demo()).texts().join(" | ");
        assert!(texts.contains("资源包"), "{texts}");
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
    ///
    /// 这条守的是真机 bug：「序号的数字被左右压扁，右边的同步按钮也被遮挡住一点」。
    /// 那次成因是「序号砖允许收缩 + 文字列顶住不缩」，两头都坏。
    #[test]
    fn chapter_rows_keep_the_number_and_the_button_intact() {
        let tree = render(&demo());
        // 章节行 = 包着「序号砖 36×36」的那一层。
        let tiles = tree.find(|node| node.get("w") == Some("36") && node.get("h") == Some("36"));
        assert!(!tiles.is_empty(), "样例里应该有章节序号砖");
        for tile in tiles {
            assert_eq!(tile.get("shrink"), Some("0"), "序号砖不许被压扁");
        }
        // 序号砖的父级是那一行的内容块：必须有 grow，且**不许**有 minw。
        let text_columns = tree.find(|node| {
            node.get("grow") == Some("1") && node.get("shrink") == Some("1") && !node.children.is_empty()
        });
        assert!(!text_columns.is_empty(), "文字列要能吃剩余宽度");
        for column in text_columns {
            assert_eq!(column.get("minw"), None, "`minw(180)` 正是按钮被挤出可视区的原因，不许回来");
        }
        // 行内动作按钮不收缩。
        let chips = tree.find(|node| {
            node.tag == Tag::Button && node.get("h") == Some(&CHIP_HEIGHT.to_string())
        });
        for chip in chips {
            assert_eq!(chip.get("shrink"), Some("0"), "行内按钮不许被压窄");
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

    /// 本页**没有**连接 / 打开游戏按钮：连接在外壳（常驻），打开游戏在推送页顶部动作区。
    ///
    /// v3 起「连接手环」提升到外壳：用户在存档页发现没连上时，不该被要求先切回推送页。
    /// 所以这里断言的是「连接不在页面里」，而「打开游戏」确实在页面里（它只对推送有意义）。
    #[test]
    fn connect_lives_in_the_shell_and_launch_on_the_push_page() {
        let mut snapshot = demo();
        snapshot.device.connected = true;
        snapshot.device.alive = true;
        let tree = render(&snapshot);
        assert!(tree.find(|node| node.get("on.click") == Some("connect")).is_empty(), "连接按钮在外壳");
        assert!(
            !tree.find(|node| node.get("on.click") == Some("launch")).is_empty(),
            "连上之后推送页要有「打开游戏」"
        );
        // 没连上时不显示打开游戏（没有手环，打开也没意义）。
        let mut offline = demo();
        offline.device.connected = false;
        offline.device.alive = false;
        assert!(
            render(&offline).find(|node| node.get("on.click") == Some("launch")).is_empty(),
            "没连上手环时不该出现「打开游戏」"
        );
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
