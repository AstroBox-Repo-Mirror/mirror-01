//! 存档页：手环上的自动存档与手动槽、导出 / 导入 / 读档 / 删除。
//!
//! 版式是**一列卡片 + 一个滚动列表**（不是第二套网格）：宿主不认
//! `grid-template-columns`（`GRID` 会退化成每格一行，见
//! docs/插件开发注意事项.md 6.7），所以放多行的地方一律 flex + grow。
//!
//! 读档 = 把这一槽设成手环首页「继续阅读」的进度；**读档真正靠的是
//! `packId` + `packScene`**，章节标题与场景号只作展示，界面不改写它们。

use super::super::actions;
use super::super::glass::{
    accent_chip, alpha, danger_button, empty, ghost_button, hover_key, hovered, panel,
    primary_button, quiet_button, section, state_badge,
};
use super::super::node::{Node, Tag, badge, label};
use super::super::snapshot::{
    SaveSlotView, Snapshot, StatusKind, auto_save_row, manual_save_count, missing_save_count,
    MAX_SLOTS,
};
use super::super::theme::*;

/// 列表区最大高度：和章节页一致，长列表自己有滚动容器最稳。
const LIST_MAX_HEIGHT: u32 = 430;

pub fn render(snapshot: &Snapshot) -> Node {
    // 存档行**这一帧现算一次**：`Snapshot::saves()` 用的是当前的已安装章节列表
    // （「手环上装了哪几章」那个事实只有一份），下面三块共用这同一份 rows ——
    // 徽章、计数、列表就不可能各看一眼不同的输入。
    let rows = snapshot.saves();
    let mut page = Node::new(Tag::Div).full().column().gap(GAP_LG);
    if let Some((conclusion, action)) = snapshot.saves_blocked_notice() {
        page = page.child(error_card(&conclusion, &action));
    }
    page = page.child(auto_card(snapshot, &rows));
    page = page.child(slot_list(snapshot, &rows));
    page = page.child(footer(snapshot, &rows));
    page
}

/// 存档通道用不了时的那张卡：**一句结论 + 一句怎么办**，各自只说一次。
///
/// 用户实机截图里这张卡把同一件事说了两遍：插件写进 `saves_error` 的失败原因
/// （「…装了带存档同步的新版 RPK 再试」）和界面自己的结论（「…手环侧没有回应
/// amakano.app.hello」）几乎一模一样，还一红一黄两种警示色。现在：
/// - **结论只有一处**（`Snapshot::saves_blocked_hint`，插件侧只置 `saves_unsupported` 状态位）；
/// - 第二句只讲**下一步怎么办**，不再重复结论里的判断；
/// - 警示色只有标题这一种（`BAD`），正文两行按本页其它卡片的层级来（正文 `TEXT_SUB` / 说明 `TEXT_DIM`）。
fn error_card(conclusion: &str, action: &str) -> Node {
    let mut card = panel(CARD_RADIUS)
        .pad(CARD_PAD)
        .gap(GAP_SM)
        .child(label("存档通道不可用", SIZE_H2, BAD).weight(600))
        .child(label(conclusion, SIZE_SMALL, TEXT_SUB));
    if !action.is_empty() {
        card = card.child(label(action, SIZE_TINY, TEXT_DIM));
    }
    card
}

/// 自动存档（断点续读）+ 槽位统计。
fn auto_card(snapshot: &Snapshot, rows: &[SaveSlotView]) -> Node {
    let manual = manual_save_count(rows);
    let missing = missing_save_count(rows);
    // 这一格的文案只说**这一格的列表状态**（读到没有），
    // 「为什么读不到 + 怎么办」由上面那张卡说一次，两处不重复。
    let state = if snapshot.saves_blocked() {
        ("读不到手环存档", StatusKind::Warn)
    } else if !snapshot.saves_ready() {
        ("等待手环回应", StatusKind::Warn)
    } else if rows.is_empty() {
        ("手环上还没有存档", StatusKind::Info)
    } else if missing > 0 {
        ("有存档的章节未安装", StatusKind::Warn)
    } else {
        ("可以读档", StatusKind::Good)
    };

    let mut card = section("存档管理", Some(format!("{manual} / {MAX_SLOTS} 槽 · {}", state.0)));

    match auto_save_row(rows) {
        Some(save) => card = card.child(slot_row(snapshot, save, "自动存档")),
        None => card = card.child(empty("还没有自动存档。在游戏里读到新的对白，手环会自己写一份。")),
    }
    if !snapshot.saves_notice.is_empty() {
        card = card.child(label(snapshot.saves_notice.clone(), SIZE_TINY, TEXT_SUB));
    }
    if missing > 0 {
        card = card.child(label(
            format!("有 {missing} 条存档所在的章节还没装到手环上：先在「章节」页同步那一章，再回来读档。"),
            SIZE_TINY,
            WARN,
        ));
    }
    card.child(label(
        "读档就是把手环首页的「继续阅读」指到这一槽。存档记的是「哪一章 + 章内位置」，以后增删章节也不会读错。",
        SIZE_TINY,
        TEXT_DIM,
    ))
}

/// 手动槽列表。
fn slot_list(snapshot: &Snapshot, rows: &[SaveSlotView]) -> Node {
    let slots: Vec<&SaveSlotView> = rows.iter().filter(|save| !save.is_auto()).collect();
    let hint = if slots.len() >= MAX_SLOTS {
        format!("{} 槽已满", slots.len())
    } else {
        format!("{} 条 · 最多 {MAX_SLOTS} 条", slots.len())
    };
    let card = section("手动存档", Some(hint));

    if slots.is_empty() {
        return card.child(empty(
            "手环上还没有手动存档。在游戏里用「保存进度」，或从外部导入一份存档。",
        ));
    }

    let mut container =
        Node::new(Tag::Scroll).full().scroll("y").maxh(LIST_MAX_HEIGHT).column().gap(6);
    for save in slots {
        container = container.child(slot_row(snapshot, save, &save.title()));
    }
    card.child(container).child(label(
        "槽号就是手环存档列表里的顺序；删掉一条，后面的会整体往前挪一位。",
        SIZE_TINY,
        TEXT_DIM,
    ))
}

/// 一条存档：标题 + 章节/场景/时间 + 未安装徽章 + 「读档」「删除」行内按钮。
///
/// 自动存档与手动槽共用这一段（`title` 由调用方给），所以两边的排版、徽章、
/// 禁用规则永远一致。
fn slot_row(snapshot: &Snapshot, save: &SaveSlotView, title: &str) -> Node {
    let key = actions::save_key(save.slot);
    let row_id = format!("save-row:{key}");
    let on = hovered(snapshot, &row_id);
    let confirming = snapshot.is_confirming_save_delete(&key);

    let mut title_row = Node::new(Tag::Div)
        .row()
        .align("center")
        .gap(6)
        .child(label(title, SIZE_BODY, TEXT_MAIN).weight(600));
    if save.missing {
        title_row = title_row.child(state_badge("未安装", StatusKind::Warn));
    }

    let text = Node::new(Tag::Div)
        .column()
        .gap(3)
        .grow(1.0)
        .minw(150)
        .child(title_row)
        .child(label(save.chapter.clone(), SIZE_SMALL, if save.missing { WARN } else { TEXT_SUB }))
        .child(label(format!("{} · {}", save.scene_label(), save.time_label()), SIZE_TINY, TEXT_DIM));

    // 自动存档没有「读档」这一步（它本来就是继续阅读的进度），只留删除与本行说明；
    // 手动槽的「读档」在章节没装时禁用（禁用的按钮**不挂任何事件**，不会骗人）。
    let action_row = if confirming {
        Node::new(Tag::Div)
            .row()
            .gap(GAP_SM)
            .child(danger_button("确认删除", &actions::save_confirm_id(save.slot), true, snapshot))
            .child(quiet_button("取消", actions::SAVES_CANCEL_DELETE, true, snapshot))
    } else if save.is_auto() {
        Node::new(Tag::Div)
            .row()
            .align("center")
            .gap(GAP_SM)
            .child(badge("继续阅读用", TEXT_SUB, INFO_BG))
            .child(quiet_button("删除", &actions::save_delete_id(save.slot), true, snapshot))
    } else {
        Node::new(Tag::Div)
            .row()
            .align("center")
            .gap(GAP_SM)
            .child(accent_chip("读档", &actions::save_load_id(save.slot), save.readable(), snapshot))
            .child(quiet_button("删除", &actions::save_delete_id(save.slot), true, snapshot))
    };

    // 先绑定再传引用：`if` 分支里的临时值活不过整条链。
    let border_color = if confirming {
        alpha(BAD, "66").to_string()
    } else if save.missing {
        alpha(WARN, "55").to_string()
    } else {
        STROKE_SOFT.to_string()
    };
    Node::new(Tag::Div)
        .full()
        .row()
        .align("center")
        .gap(GAP_SM)
        .pad(10)
        .radius(ROW_RADIUS)
        .bg(if on || confirming { SURFACE_STRONG } else { SURFACE_SOFT })
        .border(1, &border_color)
        .transition(TRANSITION)
        .hover(&hover_key(&row_id))
        .child(text)
        .child_if(save.missing, badge("需先装章节", WARN, WARN_BG))
        .child(action_row)
}

/// 底部动作：导出到剪贴板 / 从剪贴板导入 / 刷新。全是实色按钮，没有模糊、渐变、阴影。
///
/// **走剪贴板，不走系统文件对话框**：Dialog 那一类需要用户交互的宿主调用在这个宿主上
/// 会永远不返回、把事件分发器堵死（真机实测，见 `docs/插件开发注意事项.md` 第 7 节）。
fn footer(snapshot: &Snapshot, rows: &[SaveSlotView]) -> Node {
    let ready = snapshot.saves_ready();
    let has_saves = !rows.is_empty();

    let buttons = Node::new(Tag::Div)
        .row()
        .gap(GAP_SM)
        .child(primary_button(
            "导出到剪贴板",
            actions::SAVES_EXPORT,
            ready && has_saves,
            snapshot,
        ))
        .child(ghost_button("从剪贴板导入", actions::SAVES_IMPORT, ready, snapshot))
        .child(ghost_button("刷新", actions::SAVES_REFRESH, ready, snapshot));

    let mut card = section("复制 / 粘贴（剪贴板）", None)
        .child(buttons)
        .child(label(
            "导出：把存档打包成一段 JSON 放进剪贴板，界面会报字节数。粘到备忘录或聊天窗口里就存下来了。",
            SIZE_TINY,
            TEXT_DIM,
        ))
        .child(label(
            "导入：把那段 JSON 整段复制回来（别漏开头结尾），再点这个按钮，会按「写档时间」并入手环（同一份覆盖、新的追加到末尾）。",
            SIZE_TINY,
            TEXT_DIM,
        ))
        .child(label(
            "走剪贴板是为了把存档交到你手里：插件自己的目录不能放东西，一卸载就跟着没了。",
            SIZE_TINY,
            TEXT_DIM,
        ));

    // 上一次导出的结果：**字节数一定要显示**（用户靠它确认剪贴板里是一整份）。
    if let Some(done) = snapshot.save_export.as_ref() {
        card = card.child(label(
            format!(
                "上次导出：已复制 {} 个存档到剪贴板（约 {} KB / {} 字节）{}",
                done.slots,
                done.bytes.div_ceil(1024),
                done.bytes,
                if done.verified { "，已读回核对" } else { "，未能读回核对" }
            ),
            SIZE_TINY,
            if done.verified { OK } else { WARN },
        ));
    }
    card
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{InstalledView, Page, SaveExportView, SaveImportView};
    use serde_json::{Value, json};

    /// 一条手环存档的**原文**（字段与真机回包一致）：页面看到的一切都由它算出来，
    /// 测试里不再手拼 `SaveSlotView` —— 那样会绕过「未安装」判定，正好把这个 bug 漏掉。
    fn raw_save(pack_id: &str, current_scene: usize, pack_scene: usize, saved_at: u64) -> Value {
        json!({
            "storyId": "visual-novel-template",
            "chapter": 0,
            "chapterTitle": pack_id,
            "packId": pack_id,
            "packScene": pack_scene,
            "currentDialogue": 0,
            "currentScene": current_scene,
            "savedAt": saved_at,
            "choice": [],
            "routeState": {},
            "settings": { "textSpeed": 25, "textSize": 22, "autoPlaySpeed": "medium" }
        })
    }

    fn page_with_saves() -> Snapshot {
        let mut snapshot = Snapshot::default();
        snapshot.page = Page::Saves;
        snapshot.save_protocol = Some(1);
        snapshot.band_version = "0.1.0".into();
        snapshot.installed = vec![InstalledView {
            id: "玲线1·序章".into(),
            name: "玲线1·序章".into(),
            bytes: 1_000_000,
            files: 20,
            stale: false,
        }];
        // 自动存档挂在已安装的章节上（它自己不带「读档」按钮：继续阅读用的就是它）；
        // 第 2 槽那一条的章节**没装**，页面上要有「未安装」徽章与禁用的「读档」。
        snapshot.save_auto = Some(raw_save("玲线1·序章", 39, 39, 1_756_000_100_000));
        snapshot.save_slots = vec![
            raw_save("玲线1·序章", 11, 11, 1_756_000_000_000),
            raw_save("结灯线1·序章", 11, 11, 1_756_000_000_000),
        ];
        snapshot
    }

    #[test]
    fn renders_every_slot_with_inline_actions() {
        let snapshot = page_with_saves();
        let tree = render(&snapshot);
        let texts = tree.texts();

        assert!(texts.contains(&"自动存档"), "要有自动存档行");
        assert!(texts.contains(&"存档 1") && texts.contains(&"存档 2"), "两条手动槽都要在");
        // 导出/导入改走剪贴板之后，按钮文案要说清「剪贴板」这件事。
        assert!(texts.contains(&"导出到剪贴板") && texts.contains(&"从剪贴板导入") && texts.contains(&"刷新"));
        assert!(!texts.iter().any(|text| text.contains("导出存档")), "旧文案「导出存档…」不许回来");
        assert_eq!(tree.find(|node| node.get("on.click") == Some("save-export")).len(), 1);
        assert_eq!(tree.find(|node| node.get("on.click") == Some("save-import")).len(), 1);
        // 卡片里要有一行说明：怎么保存、怎么恢复。
        assert!(
            texts.iter().any(|text| text.contains("粘到备忘录")),
            "缺少「怎么保存」的说明：{texts:?}"
        );
        assert!(
            texts.iter().any(|text| text.contains("按「写档时间」并入手环")),
            "缺少「怎么恢复」的说明：{texts:?}"
        );

        // 每一条手动存档都有「读档」「删除」，删除先走二次确认。
        for action in ["save-load:0", "save-delete:0", "save-delete:auto"] {
            assert_eq!(
                tree.find(|node| node.get("on.click") == Some(action)).len(),
                1,
                "{action} 应该恰好有一个"
            );
        }
        assert!(
            tree.find(|node| node.get("on.click") == Some("save-delete-ok:0")).is_empty(),
            "没点删除之前不该出现「确认删除」"
        );
        // 自动存档没有「读档」（它本身就是继续阅读的进度），这点要在界面上说清楚。
        assert!(tree.find(|node| node.get("on.click") == Some("save-load:auto")).is_empty());
        assert!(texts.contains(&"继续阅读用"), "自动存档那一行要说明它的用途");
    }

    #[test]
    fn export_result_shows_the_envelope_size_and_read_back_state() {
        // 导出之后卡片里要多一行结果：槽位数 + KB + 精确字节数（用户靠它确认剪贴板里是一整份）。
        let find_line = |tree: &Node| -> Option<String> {
            tree.find(|node| node.text.as_deref().is_some_and(|text| text.starts_with("上次导出：")))
                .first()
                .and_then(|node| node.text.clone())
        };

        let mut snapshot = page_with_saves();
        snapshot.save_export = Some(SaveExportView { bytes: 6_412, slots: 3, verified: true });
        let tree = render(&snapshot);
        let line = find_line(&tree).expect("导出之后要有一行结果");
        assert!(line.contains("3 个存档"), "{line}");
        assert!(line.contains("约 7 KB"), "{line}");
        assert!(line.contains("6412 字节"), "{line}");
        assert!(line.contains("已读回核对"), "{line}");

        // 读回核对失败不是导出失败，但界面要照实说，而且不该显示成「成功」的绿色。
        snapshot.save_export = Some(SaveExportView { bytes: 6_412, slots: 3, verified: false });
        let tree = render(&snapshot);
        let line = find_line(&tree).expect("导出之后要有一行结果");
        assert!(line.contains("未能读回核对"), "{line}");
        let node = tree
            .find(|node| node.text.as_deref() == Some(line.as_str()))
            .first()
            .cloned()
            .expect("那一行应该是个节点");
        assert_eq!(node.get("fg"), Some(WARN));

        // 还没导出过就不该出现这一行（空文本节点另有全局守护用例）。
        let fresh = page_with_saves();
        assert!(fresh.save_export.is_none());
        assert!(find_line(&render(&fresh)).is_none(), "没导出过就不该有「上次导出」这一行");
    }

    #[test]
    fn import_result_says_how_many_were_written_and_skipped() {
        let mut snapshot = page_with_saves();
        // 界面上的那句话由 `SaveImportView::notice()` 一处生成，插件侧直接用。
        assert_eq!(
            SaveImportView { incoming: 5, duplicates: 2, existing: 3 }.notice(),
            "导入 3 个存档，跳过 2 个重复（手环上现在有 6 条）"
        );
        // 全是重复时写进去 0 条，也不能报负数或「导入 5 个」。
        assert_eq!(
            SaveImportView { incoming: 2, duplicates: 2, existing: 4 }.notice(),
            "导入 0 个存档，跳过 2 个重复（手环上现在有 4 条）"
        );
        // 界面照实显示插件给的那句话（它同时也写进状态行）。
        snapshot.saves_notice = SaveImportView { incoming: 5, duplicates: 2, existing: 3 }.notice();
        let tree = render(&snapshot);
        assert!(
            tree.texts().iter().any(|text| text.contains("导入 3 个存档，跳过 2 个重复")),
            "{:?}",
            tree.texts()
        );
    }

    #[test]
    fn missing_chapter_is_flagged_and_not_readable() {
        let snapshot = page_with_saves();
        let tree = render(&snapshot);
        assert!(
            tree.texts().contains(&"未安装"),
            "章节没装的存档要有「未安装」标记：{:?}",
            tree.texts()
        );
        // 未安装那一槽的「读档」必须禁用，且**不挂任何点击事件**（禁用的按钮看起来才不骗人）。
        assert!(tree.find(|node| node.get("on.click") == Some("save-load:1")).is_empty());
        let disabled = tree
            .find(|node| node.text.as_deref() == Some("读档") && node.has("disabled"))
            .len();
        assert_eq!(disabled, 1, "未安装那一槽的「读档」要是禁用态");
        let rows = snapshot.saves();
        assert_eq!(missing_save_count(&rows), 1);
        assert!(!rows[1].missing && rows[2].missing, "第 2 槽那条的章节没装");
    }

    #[test]
    fn confirming_a_slot_swaps_the_row_actions() {
        let mut snapshot = page_with_saves();
        snapshot.confirm_delete_save = Some("1".into());
        let tree = render(&snapshot);
        // 被确认的那一行：换成「确认删除 / 取消」，原来的「删除」不再出现（免得又点一次）。
        assert!(tree.find(|node| node.get("on.click") == Some("save-delete:1")).is_empty());
        assert_eq!(tree.find(|node| node.get("on.click") == Some("save-delete-ok:1")).len(), 1);
        assert_eq!(tree.find(|node| node.get("on.click") == Some("save-cancel")).len(), 1);
        // 确认行里不再有「读档」（这一行现在只有一件事要做）。
        assert!(tree.find(|node| node.get("on.click") == Some("save-load:1")).is_empty());
        // 没被确认的那一槽仍然是普通「删除」。
        assert_eq!(tree.find(|node| node.get("on.click") == Some("save-delete-ok:0")).len(), 0);
        assert_eq!(tree.find(|node| node.get("on.click") == Some("save-delete:0")).len(), 1);
    }

    #[test]
    fn missing_hello_ok_says_the_band_app_is_old() {
        // 手环没回 hello-ok：界面必须给出人话，**不能只报超时**。
        let mut snapshot = Snapshot::default();
        snapshot.page = Page::Saves;
        snapshot.saves_error = "读取存档超时".into();
        let tree = render(&snapshot);
        let texts = tree.texts();
        assert!(texts.iter().any(|text| text.contains("手环端应用版本过旧")), "缺人话：{texts:?}");
        assert!(texts.iter().any(|text| text.contains("更新后才能管理存档")));
        assert!(snapshot.saves_blocked_hint().contains("版本过旧"));
        // 通道被版本卡住时，插件记下的那次失败原因（这里是「读取存档超时」）**不再单独渲染**：
        // 它与结论是同一件事，渲染出来就是用户截图里那两句几乎一样的提示。
        assert!(!texts.iter().any(|text| text.contains("读取存档超时")), "同一件事只说一次：{texts:?}");
        // 三个动作全禁用（没有协议就没法谈）。
        for action in ["save-export", "save-import", "save-refresh"] {
            assert!(tree.find(|node| node.get("on.click") == Some(action)).is_empty(), "{action} 该禁用");
        }
    }

    #[test]
    fn blocked_card_renders_one_conclusion_and_one_next_step() {
        // 用户实机截图报的 bug：这张卡把同一件事说了两遍 —— 插件往 `saves_error` 里写的
        // 「…需更新后才能管理存档（装了带存档同步的新版 RPK 再试）」是一句，界面自己的
        // 「…需更新后才能管理存档（手环侧没有回应 amakano.app.hello）」又是一句，还一红一黄。
        // 现在验收的是**分工**：一句结论（为什么）＋ 一句怎么办（下一步），各自只出现一次。
        let mut snapshot = Snapshot::default();
        snapshot.page = Page::Saves;
        // 插件侧只置状态位（不再写第二句话），这是真机上的真实形态。
        snapshot.saves_unsupported = true;
        assert!(snapshot.saves_error.is_empty(), "通道被版本卡住时插件不该再写 saves_error");

        let tree = render(&snapshot);
        let texts = tree.texts();

        let conclusions = texts.iter().filter(|text| text.contains("手环端应用版本过旧")).count();
        assert_eq!(conclusions, 1, "结论只许渲染一次：{texts:?}");
        let actions = texts.iter().filter(|text| text.contains("重新安装带存档功能的新版 RPK")).count();
        assert_eq!(actions, 1, "「怎么办」只许渲染一次：{texts:?}");

        let conclusion = snapshot.saves_blocked_hint();
        let action = snapshot.saves_blocked_action();
        // 结论解释「为什么不可用」（手环没回能力查询），怎么办只说下一步动作，不重复判断。
        assert!(conclusion.contains("amakano.app.hello"), "{conclusion}");
        assert!(!action.contains("版本过旧"), "「怎么办」不许重复结论里的判断：{action}");
        assert_ne!(conclusion, action);
        // 卡片上就是这两行，不多不少。
        assert!(texts.contains(&conclusion.as_str()) && texts.contains(&action.as_str()));

        // 只用一种警示样式：标题那一行是警示色，正文两行都是普通文字色（不再红黄叠着）。
        let title = tree.find(|node| node.text.as_deref() == Some("存档通道不可用")).first().cloned();
        assert_eq!(title.expect("卡片要有标题").get("fg"), Some(BAD));
        for line in [conclusion.as_str(), action.as_str()] {
            let node = tree.find(|node| node.text.as_deref() == Some(line)).first().cloned();
            let node = node.unwrap_or_else(|| panic!("{line} 应该是卡片里的一行"));
            assert_ne!(node.get("fg"), Some(WARN), "正文不该再是黄色警示：{line}");
            assert_ne!(node.get("fg"), Some(BAD), "正文不该再是红色警示：{line}");
        }

        // 状态归状态：存档管理那一格只说「列表读没读到」，不再写「等待手环回应」——
        // 结论已经说了手环端应用过旧，再写「等待」等于让人以为再等等会好。
        assert!(texts.iter().any(|text| text.contains("读不到手环存档")), "{texts:?}");
        assert!(!texts.iter().any(|text| text.contains("等待手环回应")), "{texts:?}");
    }

    #[test]
    fn ready_channel_shows_no_blocked_card() {
        // 协商成功之后这张卡（连同那两句话）必须整个消失，别留下「版本过旧」的残影。
        let snapshot = page_with_saves();
        assert!(snapshot.saves_blocked_notice().is_none());
        let tree = render(&snapshot);
        let texts = tree.texts();
        assert!(!texts.iter().any(|text| text.contains("手环端应用版本过旧")), "{texts:?}");
        assert!(!texts.contains(&"存档通道不可用"));
    }

    #[test]
    fn empty_state_and_no_auto_save_both_render() {
        let mut snapshot = Snapshot::default();
        snapshot.page = Page::Saves;
        snapshot.save_protocol = Some(1);
        let tree = render(&snapshot);
        assert_eq!(manual_save_count(&snapshot.saves()), 0);
        assert!(tree.texts().iter().any(|text| text.contains("还没有自动存档")));
        assert!(tree.texts().iter().any(|text| text.contains("还没有手动存档")));
        // 一条存档都没有时不能导出（没什么可导的），但可以导入/刷新。
        assert!(tree.find(|node| node.get("on.click") == Some("save-export")).is_empty());
        assert_eq!(tree.find(|node| node.get("on.click") == Some("save-import")).len(), 1);
        assert_eq!(tree.find(|node| node.get("on.click") == Some("save-refresh")).len(), 1);
    }
}
