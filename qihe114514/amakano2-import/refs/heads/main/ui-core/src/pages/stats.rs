//! 阅读统计页：手环上累计的阅读天数与时长。
//!
//! 版式与其它页一致：**一列卡片 + 一个滚动列表**（宿主不认 `grid-template-columns`，
//! 等宽并排一律 flex + grow，见 docs/插件开发注意事项.md 6.7）。
//!
//! ⚠️ 这里**不做任何换算**：所有文案（`4 小时 12 分`、`连续 3 天`）都是手环侧
//! `common/reading-stats.js` 生成好一起送过来的（`labels` / `recent[].label`）。
//! 插件自己再算一遍「秒 → 几小时几分」的话，两边迟早会在某一天开始对不上，
//! 而那种不一致在界面上看不出谁对谁错。

use super::super::actions;
use super::super::glass::{
    divider, group, group_heading, group_heading_with, group_row_with, meta, primary_button,
    row_detail, scroll_list, value,
};
use super::super::node::{Node, Tag, badge, label};
use super::super::snapshot::{ReadingStatsView, Snapshot};
use super::super::theme::*;

/// 最近明细的滚动高度上限（与旧版同值）。
const LIST_MAX_HEIGHT: u32 = 320;

pub fn render(snapshot: &Snapshot) -> Node {
    let mut page = Node::new(Tag::Div).full().column();
    // 通道不可用 / 上一次读取失败：一处说清楚（结论 + 怎么办，各自只说一次）。
    if let Some((conclusion, action)) = snapshot.stats_notice() {
        page = page.child(notice_card(&conclusion, &action));
    }
    match snapshot.stats.as_ref() {
        Some(stats) => {
            page = page.child(hero(stats));
            page = page.child(detail(stats));
            page = page.child(recent(stats));
        }
        // 通道正常、只是还没读到（没连手环 / 还没点刷新）：给一处说人话的空状态。
        None if snapshot.stats_error.is_empty() && !snapshot.stats_blocked() => {
            page = page.child(waiting_card(snapshot));
        }
        None => {}
    }
    page.child(footer(snapshot))
}

/// 通道不可用 / 读取失败：**一句结论 + 一句怎么办**（和存档页同一条规矩）。
fn notice_card(conclusion: &str, action: &str) -> Node {
    let mut card = group().pad_y(GAP).child(
        Node::new(Tag::Div)
            .full()
            .column()
            .gap(GAP_XS)
            .child(label("阅读统计读不到", TYPE_ROW, BAD))
            .child(label(conclusion, TYPE_META, TEXT_SUB)),
    );
    if !action.is_empty() {
        card = card.child(
            Node::new(Tag::Div).full().child(label(action, TYPE_META, TEXT_DIM)),
        );
    }
    card
}

/// 还没读到时的空状态。
fn waiting_card(snapshot: &Snapshot) -> Node {
    let hint = if snapshot.device.connected && snapshot.device.alive {
        "点下面的「读取统计」，把《甜蜜女友2》里的阅读时长读过来。"
    } else if snapshot.device.connected {
        "手环已连接，但《甜蜜女友2》还没在前台运行：在手环上打开它，再点「读取统计」。"
    } else {
        "先点上方的「连接手环」，并在手环上打开《甜蜜女友2》，再回来读取统计。"
    };
    group_heading_with("手环上的阅读统计", meta("尚未读取"))
        .full()
        .child(
            group().child(
                Node::new(Tag::Div)
                    .full()
                    .column()
                    .gap(GAP_SM)
                    .pad_y(GROUP_ROW_PAD_Y)
                    .child(label(hint, TYPE_META, TEXT_SUB))
                    .child(label("手环读一段就记一笔，插件只是把它读出来。", TYPE_META, TEXT_DIM)),
            ),
        )
}

/// 主卡片：总阅读时长。
///
/// 宿主没有 `font-weight`，大数字只能靠 **28px 字号 + 主文色** 撑起分量 ——
/// 旧版给了 `weight(700)`，真机上那一行和正文一样粗细，看着像个普通段落。
fn hero(stats: &ReadingStatsView) -> Node {
    group().child(
        Node::new(Tag::Div)
            .full()
            .column()
            .gap(GAP_XS)
            .pad_t(16)
            .pad_b(GROUP_ROW_PAD_Y)
            .child(label(stats.total_label.clone(), TYPE_HERO, ACCENT))
            // 说明行用 `head_line()`：「累计 N 天 · 统计至 X」，空数据时是
            // 「手环上还没有阅读记录」—— 一句话就能分清「真没记录」和「没读到」。
            .child(row_detail(stats.head_line()))
            // 徽章必须包在 row 里：直接挂在 column 容器的子节点上会被 cross-axis
            // 拉满整行宽，一颗胶囊变成一条横杠。
            .child(
                Node::new(Tag::Div)
                    .row()
                    .pad_t(2)
                    .child(badge(&format!("今日 {}", stats.today_label), TEXT_MAIN, INFO_BG)),
            ),
    )
}

/// 四个明细行：阅读天数 / 总阅读天数 / 今日阅读时长 / 单日阅读最长时长。
///
/// **只列这四项**：「总阅读时长」已经在上面那个大数字里显示过了，
/// 这里再来一行就是同一个数说两遍。
fn detail(stats: &ReadingStatsView) -> Node {
    let pairs = [
        ("阅读天数", &stats.reading_days_label),
        ("总阅读天数", &stats.total_days_label),
        ("今日阅读时长", &stats.today_label),
        ("单日阅读最长", &stats.longest_label),
    ];
    let mut list = group();
    for (index, (key, text)) in pairs.iter().enumerate() {
        if index > 0 {
            list = list.child(divider());
        }
        list = list.child(group_row_with(label(*key, TYPE_BODY, TEXT_SUB), value(*text)));
    }
    group_heading("明细").full().child(list)
}

/// 最近几天的明细（手环侧只送最近 30 天）。
fn recent(stats: &ReadingStatsView) -> Node {
    let block = group_heading_with("最近几天", meta(format!("近 {} 天", stats.recent.len()))).full();
    if stats.recent.is_empty() {
        return block.child(
            group().child(
                Node::new(Tag::Div)
                    .full()
                    .pad_y(24)
                    .align("center")
                    .child(label("手环上还没有按天的阅读记录", TYPE_META, TEXT_DIM)),
            ),
        );
    }
    let mut list = group();
    for (index, day) in stats.recent.iter().rev().enumerate() {
        if index > 0 {
            list = list.child(divider());
        }
        let is_today = !stats.today.is_empty() && day.date == stats.today;
        let mut row = Node::new(Tag::Div)
            .full()
            .row()
            .align("center")
            .gap(GAP_SM)
            .pad_y(GROUP_ROW_PAD_Y)
            .child(label(day.date.clone(), TYPE_META, TEXT_SUB).shrink(0.0))
            .child(Node::new(Tag::Div).grow(1.0))
            .child(value(day.label.clone()));
        if is_today {
            row = row.child(badge("今天", ACCENT, INFO_BG));
        }
        list = list.child(row);
    }
    // 手环会送最近 30 天 —— 包一层验证过可用的滚动区（见 `glass::scroll_list`）。
    block.child(scroll_list(list, LIST_MAX_HEIGHT)).child(
        Node::new(Tag::Div)
            .full()
            .pad_t(GAP_SM)
            .pad_x(2)
            .child(label("按天累计，跨零点会分别记到前后两天。", TYPE_META, TEXT_DIM)),
    )
}

/// 底部：读取 / 刷新。
fn footer(snapshot: &Snapshot) -> Node {
    let ready = !snapshot.stats_blocked() && !snapshot.stats_busy;
    let label_text = if snapshot.stats.is_some() { "刷新统计" } else { "读取统计" };

    let card = group_heading_with("从手环读取", meta(card_hint(snapshot)))
        .full()
        .child(group().child(
            Node::new(Tag::Div)
                .full()
                .column()
                .gap(GAP_SM)
                .pad_y(GROUP_ROW_PAD_Y)
                .child(primary_button(label_text, actions::STATS_REFRESH, ready, snapshot))
                .child_if(
                    snapshot.stats_busy,
                    label("正在读手环上的统计…", TYPE_META, TEXT_SUB),
                )
                .child(label(
                    "这里只读不写：插件改不了手环上的统计，也没法把时长清零。",
                    TYPE_META,
                    TEXT_DIM,
                )),
        ));
    card
}

fn card_hint(snapshot: &Snapshot) -> String {
    if snapshot.stats_blocked() {
        return "通道不可用".into();
    }
    if snapshot.stats_busy {
        return "读取中".into();
    }
    if snapshot.stats.is_some() {
        return "已读到".into();
    }
    match (snapshot.device.connected, snapshot.device.alive) {
        (true, true) => "可以读取".into(),
        (true, false) => "等待应用回应".into(),
        _ => "未连接手环".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{Page, RecentDayView};

    /// 一份手环回包解出来的样本（字段名与 `reading-stats.js` 的 `envelope()` 一致）。
    fn view() -> ReadingStatsView {
        ReadingStatsView {
            today: "2026-09-12".into(),
            total_day_count: 15,
            longest_date: "2026-09-01".into(),
            reading_days_label: "连续 3 天".into(),
            total_days_label: "15 天".into(),
            total_label: "4 小时 12 分".into(),
            today_label: "25 分钟".into(),
            longest_label: "1 小时 40 分（9月1日）".into(),
            recent: vec![
                RecentDayView { date: "2026-09-11".into(), label: "40 分钟".into(), seconds: 2400 },
                RecentDayView { date: "2026-09-12".into(), label: "25 分钟".into(), seconds: 1500 },
            ],
        }
    }

    fn page_with_stats() -> Snapshot {
        let mut snapshot = Snapshot::default();
        snapshot.page = Page::Stats;
        snapshot.save_protocol = Some(1);
        snapshot.stats_supported = true;
        snapshot.band_version = "0.2.0".into();
        snapshot.stats = Some(view());
        snapshot
    }

    #[test]
    fn renders_every_required_number_exactly_once() {
        let tree = render(&page_with_stats());
        let texts = tree.texts();
        // 需求点名的名目都要在界面上出现（「总阅读时长」由大数字那块表达，
        // 它自己不再有标题行 —— 所以这里列的是明细四项）。
        for required in ["阅读天数", "总阅读天数", "今日阅读时长", "单日阅读最长"] {
            assert!(texts.contains(&required), "缺「{required}」：{texts:?}");
        }
        // 主块用 `head_line()` 当说明行（「累计 N 天 · 统计至 X」），
        // 所以「总阅读时长」这个名目不再单独占一行 —— 大数字本身已经说明它是时长。
        assert!(
            texts.iter().any(|text| text.contains("累计") && text.contains("统计至")),
            "主块要有说明行：{texts:?}"
        );
        // 对应的值也要在（这些字符串全部来自手环侧，插件不许自己重算）
        for value in ["4 小时 12 分", "15 天", "连续 3 天", "25 分钟", "1 小时 40 分（9月1日）"] {
            assert!(texts.iter().any(|text| *text == value), "缺值 {value}：{texts:?}");
        }
        // 总时长只显示一次，不在明细里重复同一个数。
        assert_eq!(texts.iter().filter(|text| **text == "4 小时 12 分").count(), 1);
        // 最近明细要带上「今天」徽章
        assert!(texts.contains(&"今天"), "{texts:?}");
        // 刷新按钮恰好一个
        assert_eq!(tree.find(|node| node.get("on.click") == Some("stats-refresh")).len(), 1);
    }

    #[test]
    fn unsupported_band_app_gets_one_conclusion_and_one_next_step() {
        // 手环回过 hello-ok，但能力表里没有 stats → 「版本过旧」那张卡出现。
        let mut snapshot = Snapshot::default();
        snapshot.page = Page::Stats;
        snapshot.save_protocol = Some(1);
        snapshot.stats_supported = false;
        snapshot.band_version = "0.1.0".into();
        assert!(snapshot.stats_blocked());

        let tree = render(&snapshot);
        let texts = tree.texts();
        assert_eq!(
            texts.iter().filter(|text| text.contains("手环端应用版本过旧")).count(),
            1,
            "结论只许说一次：{texts:?}"
        );
        assert!(texts.iter().any(|text| text.contains("0.1.0")), "结论里要有手环版本号");
        assert!(texts.iter().any(|text| text.contains("重新安装新版 RPK")));
        // 通道不可用时刷新按钮禁用（禁用的按钮不挂事件）
        assert!(tree.find(|node| node.get("on.click") == Some("stats-refresh")).is_empty());

        // 还没问过能力（协议都没协商出来）时**不该**弹这张卡，只是「还没读到」。
        let mut fresh = Snapshot::default();
        fresh.page = Page::Stats;
        assert!(!fresh.stats_blocked());
        assert!(fresh.stats_notice().is_none());
        let fresh_tree = render(&fresh);
        let fresh_texts = fresh_tree.texts();
        assert!(!fresh_texts.iter().any(|text| text.contains("版本过旧")), "{fresh_texts:?}");
        assert!(fresh_texts.iter().any(|text| text.contains("尚未读取")), "{fresh_texts:?}");
    }

    #[test]
    fn empty_stats_is_not_the_same_as_a_read_failure() {
        // 手环上真的没有记录：读到的是「0 分钟」，不是报错。
        let mut snapshot = page_with_stats();
        snapshot.stats = Some(ReadingStatsView {
            today: "2026-09-12".into(),
            reading_days_label: "连续 0 天".into(),
            total_days_label: "0 天".into(),
            total_label: "0 分钟".into(),
            today_label: "0 分钟".into(),
            longest_label: "—".into(),
            ..ReadingStatsView::default()
        });
        let empty_tree = render(&snapshot);
        let empty_texts = empty_tree.texts();
        assert!(empty_texts.iter().any(|text| text.contains("还没有阅读记录")), "{empty_texts:?}");
        // 明细也是空的，所以「最近几天」那张卡给出空状态 —— 这是**正常结果**，不是错误。
        assert!(empty_texts.iter().any(|text| text.contains("还没有按天的阅读记录")), "{empty_texts:?}");
        assert!(!empty_texts.iter().any(|text| text.contains("读不到")), "{empty_texts:?}");
        assert!(!empty_texts.iter().any(|text| text.contains("尚未读取")), "{empty_texts:?}");

        // 解析失败则是另一回事：要给一句人话（日志那半由插件侧写）。
        let mut broken = page_with_stats();
        broken.stats = None;
        broken.stats_error = "手环回包缺字段：labels（界面文案由手环侧生成）".into();
        let broken_tree = render(&broken);
        let broken_texts = broken_tree.texts();
        assert!(broken_texts.iter().any(|text| text.contains("阅读统计读不到")), "{broken_texts:?}");
        assert!(broken_texts.iter().any(|text| text.contains("缺字段：labels")), "{broken_texts:?}");
        assert!(
            !broken_texts.iter().any(|text| text.contains("尚未读取")),
            "失败要说失败，不能说「还没读」：{broken_texts:?}"
        );
    }

    #[test]
    fn missing_labels_are_reported_in_plain_chinese() {
        // 手环端回包缺了 labels（例如旧版本手环只回数字）→ 必须报出来，不许静默变 0。
        let without_labels = serde_json::json!({ "today": "2026-09-12", "totalDayCount": 3 });
        let error = ReadingStatsView::from_json(&without_labels).unwrap_err();
        assert!(error.message().contains("labels"), "{}", error.message());

        let not_object = serde_json::json!([1, 2, 3]);
        let error = ReadingStatsView::from_json(&not_object).unwrap_err();
        assert!(error.message().contains("不是 JSON 对象"), "{}", error.message());

        // 缺 today 同样要报错（它是「今天」这一行的依据）
        let without_today = serde_json::json!({ "labels": {
            "readingDays": "连续 0 天", "totalDays": "0 天", "total": "0 分钟", "today": "0 分钟", "longest": "—"
        }});
        let error = ReadingStatsView::from_json(&without_today).unwrap_err();
        assert!(error.message().contains("today"), "{}", error.message());
    }

    #[test]
    fn recent_listing_keeps_the_handset_labels() {
        let tree = render(&page_with_stats());
        let texts = tree.texts();
        // 两天的明细都在，值就是手环侧给的那两个字符串（插件不重算）
        assert!(texts.contains(&"2026-09-11") && texts.contains(&"2026-09-12"), "{texts:?}");
        assert!(texts.iter().any(|text| *text == "40 分钟"), "{texts:?}");
        assert_eq!(view().recent.len(), 2);
    }
}
