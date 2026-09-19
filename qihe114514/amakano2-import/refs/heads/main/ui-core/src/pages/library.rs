//! 章节页：内置章节的清单、线路筛选与单章同步。
//!
//! 一页三块：汇总 + 批量同步、线路筛选、章节列表。
//! 筛选条走横向滚动区（窄窗里只滚不裁），章节行每行**只占两行文字**
//! （标题一行、元信息一行）—— 上一版每行三行小字，15 章就是 45 行。

use super::super::actions;
use super::super::glass::{
    accent_chip, alpha, empty, hover_key, hovered, meta, panel, primary_button, scrollable_segmented,
    section, state_badge,
};
use super::super::node::{Node, Tag, label};
use super::super::snapshot::{PackView, Snapshot, StatusKind};
use super::super::theme::*;
use super::super::human_bytes;

/// 列表区最大高度：宿主页面本身滚不滚不确定，给长列表一个自己的滚动容器更稳妥。
const LIST_MAX_HEIGHT: u32 = 440;

pub fn render(snapshot: &Snapshot) -> Node {
    Node::new(Tag::Div)
        .full()
        .column()
        .gap(GAP_LG)
        .child(summary(snapshot))
        .child(filters(snapshot))
        .child(list(snapshot))
}

/// 汇总 + 批量同步。
fn summary(snapshot: &Snapshot) -> Node {
    let pending = snapshot.pending_count();
    let line = if snapshot.library.is_empty() {
        snapshot.library_error.clone()
    } else {
        format!(
            "共 {} 章 · 约 {} 小时 · {}",
            snapshot.library.len(),
            snapshot.total_hours(),
            human_bytes(snapshot.library_bytes())
        )
    };

    let mut card = panel(CARD_RADIUS).pad(CARD_PAD).gap(GAP_SM);
    card = card.child(label("章节库", SIZE_TITLE, TEXT_MAIN).weight(650));
    card = card.child(if snapshot.library.is_empty() {
        // 读不出来时要看得见原因，不能只显示一个零。
        label(line, SIZE_TINY, BAD)
    } else {
        meta(line)
    });

    if !snapshot.library.is_empty() {
        card = card.child(primary_button(
            &format!("同步剩余 {pending} 章"),
            actions::SYNC_ALL,
            pending > 0 && !snapshot.is_transferring(),
            snapshot,
        ));
    }
    card
}

/// 线路筛选。
///
/// 分段控件走 `glass::scrollable_segmented`：外层 `Tag::Scroll` + `scroll("x")`，
/// 里面每一项 `shrink(0)`，所以**任何 ≤400px 宽度下都只会滚动、不会把「番外」裁掉半个**。
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
fn list(snapshot: &Snapshot) -> Node {
    let packs = snapshot.filtered_library();
    let mut container = Node::new(Tag::Scroll).full().scroll("y").maxh(LIST_MAX_HEIGHT).column().gap(GAP_XS);
    for pack in &packs {
        container = container.child(row(snapshot, pack));
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
/// 上一版序号砖没写 `shrink(0)`，而文字列写了 `minw(180)` —— 两者凑在一起的结果是
/// 400px 窗口里「序号砖被压扁」+「右边的同步按钮被裁掉一点」：该收缩的文字列因为有
/// `minw(180)` 顶住不缩，flex 只好去挤**本来固定宽度的**序号砖，挤不动的部分就整行溢出。
///
/// 现在的分工是**每个元素只干一件事**：
/// - 序号砖 `36×36` + `shrink(0)`：它是个图形，宽度绝不许变（否则数字被压扁）；
/// - 文字列 `grow(1.0)` + `shrink(1.0)`：**唯一允许变窄的东西**（窄了就换行，
///   中文按字断行不会拦腰截断），这样行尾的按钮永远有位置；
/// - 行内按钮 `shrink(0)`：写在 `glass::button()` 里，宽度只由字号和内边距决定；
/// - 行内 `gap` 用 `GAP`(12)：三块之间留够呼吸，不靠挤压去省空间。
fn row(snapshot: &Snapshot, pack: &PackView) -> Node {
    let (_, line_color) = chapter_line(&pack.title);
    let row_id = format!("row:{}", pack.number);
    let on = hovered(snapshot, &row_id);

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
    } else if pack.installed {
        title_row = title_row.child(state_badge("已装", StatusKind::Good));
    } else if pack.queued {
        title_row = title_row.child(state_badge("排队中", StatusKind::Warn));
    }

    // 文字列：标题一行 + 元信息一行，就这两行。
    let text = Node::new(Tag::Div)
        .column()
        .gap(3)
        .grow(1.0)
        .shrink(1.0)
        .child(title_row)
        .child(meta(pack.meta_line()));

    let action = if pack.active {
        state_badge("传输中", StatusKind::Warn)
    } else {
        accent_chip(
            if pack.installed { "重传" } else { "同步" },
            &actions::sync_id(pack.number),
            !snapshot.is_transferring(),
            snapshot,
        )
    };

    // 先绑定再传引用：`if` 分支里的临时值活不过整条链。
    let active = pack.active;
    Node::new(Tag::Div)
        .full()
        .row()
        .align("center")
        .gap(GAP)
        .pad(10)
        .radius(ROW_RADIUS)
        .bg(if on { SURFACE_STRONG } else { SURFACE_SOFT })
        .border(1, &if active { alpha(ACCENT, "55") } else { STROKE_SOFT.to_string() })
        .transition(TRANSITION)
        .hover(&hover_key(&row_id))
        .child(number_tile)
        .child(text)
        .child(action)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;

    /// 章节库页的两处文案：用户要求「内置章节」改成「章节列表」、
    /// 「按路线看」改成「按路线」。旧文案不许偷偷回来（回来就是没改干净）。
    #[test]
    fn group_title_and_filter_caption_use_the_new_wording() {
        let tree = render(&demo());
        let texts = tree.texts();
        assert!(texts.contains(&"章节列表"), "分组标题应是「章节列表」：{texts:?}");
        assert!(texts.contains(&"按路线"), "筛选小标题应是「按路线」：{texts:?}");
        assert!(!texts.contains(&"内置章节"), "「内置章节」已按用户要求改名");
        assert!(!texts.contains(&"按线路看"), "「按线路看」已按用户要求改名");
    }

    /// 线路筛选条：**横向可滚动**，每一项都 `shrink(0)`。
    ///
    /// 这条守的是用户实机报的「按路线看的选项框右边超出」：6 个线路标签比 400px 窗口里
    /// 卡片的可用宽度宽，不包滚动区、又不许 flex 压窄每一项时，最右边的「番外」只能被裁。
    #[test]
    fn line_filter_scrolls_sideways_and_never_squeezes_its_items() {
        let tree = render(&demo());
        let areas = tree.find(|node| node.tag == Tag::Scroll && node.get("scroll") == Some("x"));
        assert!(!areas.is_empty(), "线路筛选条必须包在一层横向滚动区里（窄窗里只滚不裁）");

        let bar = areas
            .iter()
            .find_map(|area| area.children.first().filter(|bar| bar.children.len() == LINES.len() + 1))
            .expect("滚动区里应该有分段控件本体");
        for item in &bar.children {
            assert_eq!(item.get("shrink"), Some("0"), "每一项都不能被 flex 压窄（压窄就是「一个字一行」）");
            assert_eq!(item.get("pl"), Some(SEGMENT_NARROW_PAD_X.to_string().as_str()));
        }
    }

    /// 章节行：序号砖与行内按钮 `shrink(0)`、文字列 `grow(1.0)`、行内 gap 足够。
    ///
    /// 守的是用户实机报的「序号的数字被左右压扁 + 右边的同步按钮也被遮挡住一点」。
    /// ⚠️ **`minw(180)` 是那条 bug 的成因，不许加回来。**
    #[test]
    fn chapter_rows_keep_the_number_and_the_button_intact() {
        let tree = render(&demo());
        let rows = tree.find(|node| node.get("on.enter").is_some_and(|key| key.starts_with("hover:row:")));
        assert!(!rows.is_empty(), "样例里应该有章节行");
        for row in rows {
            assert_eq!(row.get("gap"), Some(GAP.to_string().as_str()), "行内 gap 要留够");
            let [number, text, action] = row.children.as_slice() else {
                panic!("章节行应有「序号砖 / 文字列 / 动作」三个子节点");
            };
            assert_eq!((number.get("w"), number.get("h")), (Some("36"), Some("36")));
            assert_eq!(number.get("shrink"), Some("0"), "序号砖是图形，压窄了数字就左右挤扁");
            assert_eq!(text.get("grow"), Some("1"), "文字列要吃掉剩余宽度");
            assert_eq!(text.get("shrink"), Some("1"), "文字列是唯一允许变窄的东西（窄了就换行）");
            assert_eq!(text.get("minw"), None, "`minw(180)` 正是按钮被挤出可视区的原因，不许回来");
            assert_eq!(action.get("shrink"), Some("0"), "行内按钮/徽章的宽度只由字号与内边距决定");
        }
    }

    /// 每行**只有两行文字**（标题 + 元信息）。上一版是标题 + 「时长 · 体积 · 幕数」+
    /// 「线路 · 句数」，15 章列表就是 45 行小字。
    #[test]
    fn chapter_rows_carry_two_text_lines_only() {
        let tree = render(&demo());
        let rows = tree.find(|node| node.get("on.enter").is_some_and(|key| key.starts_with("hover:row:")));
        for row in rows {
            let text = &row.children[1];
            assert_eq!(text.children.len(), 2, "文字列只留「标题行 + 元信息行」");
            let meta_line = text.children[1].text.clone().unwrap_or_default();
            assert!(meta_line.contains("幕") && meta_line.contains("句"), "元信息要一行说完：{meta_line}");
        }
    }

    /// 「重传」与「同步」是同一件事的两档措辞：装过的才叫重传。
    #[test]
    fn installed_rows_say_resync_while_new_ones_say_sync() {
        let tree = render(&demo());
        let texts = tree.texts();
        assert!(texts.iter().any(|text| *text == "同步"), "没装的章节是「同步」：{texts:?}");
        assert!(texts.iter().any(|text| *text == "重传"), "装过的是「重传」：{texts:?}");
        // 行内徽章也压成两个字，行尾才放得下动作按钮。
        assert!(texts.iter().any(|text| *text == "已装"), "{texts:?}");
        assert!(!texts.contains(&"重新同步"), "四个字的旧按钮文案已收成两个字");
    }
}
