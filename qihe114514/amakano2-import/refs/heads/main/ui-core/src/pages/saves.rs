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
    accent_chip, danger_button, divider, ghost_button, group, group_heading, group_heading_with,
    meta, primary_button, quiet_button, row_detail, row_title, scroll_list, state_badge,
};
use super::super::node::{Node, Tag, badge, label};
use super::super::snapshot::{
    SaveSlotView, Snapshot, StatusKind, auto_save_row, manual_save_count, missing_save_count,
    MAX_SLOTS,
};
use super::super::theme::*;

/// 手动存档列表的滚动高度上限（与旧版同值）。
const LIST_MAX_HEIGHT: u32 = 430;

pub fn render(snapshot: &Snapshot) -> Node {
    // 存档行**这一帧现算一次**：`Snapshot::saves()` 用的是当前的已安装章节列表
    // （「手环上装了哪几章」那个事实只有一份），下面三块共用这同一份 rows ——
    // 徽章、计数、列表就不可能各看一眼不同的输入。
    let rows = snapshot.saves();
    let mut page = Node::new(Tag::Div).full().column().gap(GAP_LG);
    // 1.x 旧代：这张卡**替代**「存档通道不可用」卡 —— 它的信息更具体（哪一版、怎么办），
    // 两张同时挂只会让用户读两遍同一个事实。
    if snapshot.band_game_legacy {
        page = page.child(error_card("游戏版本过旧", &snapshot.band_game_legacy_hint(), &snapshot.band_game_legacy_action()));
    } else if let Some((conclusion, action)) = snapshot.saves_blocked_notice() {
        page = page.child(error_card("存档通道不可用", &conclusion, &action));
    }
    page = page.child(cloud_card(snapshot));
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
///
/// 1.x 旧代判定成立时这张卡换个标题（「游戏版本过旧」）挂同一套版式 —— 见 `render`。
fn error_card(title: &str, conclusion: &str, action: &str) -> Node {
    let mut card = group()
        .pad_y(GAP)
        .child(
            Node::new(Tag::Div)
                .full()
                .column()
                .gap(GAP_XS)
                .child(label(title, TYPE_ROW, BAD))
                .child(label(conclusion, TYPE_META, TEXT_SUB)),
        );
    if !action.is_empty() {
        card = card.child(
            Node::new(Tag::Div)
                .full()
                .child(label(action, TYPE_META, TEXT_DIM)),
        );
    }
    card
}

fn cloud_card(snapshot: &Snapshot) -> Node {
    let (title, kind) = match snapshot.cloud_phase.as_str() {
        "synced" => ("云存档已同步", StatusKind::Good),
        "syncing" | "applying" => ("正在同步云存档", StatusKind::Info),
        "unbound" | "waiting-binding" => ("等待 QQ 绑定", StatusKind::Warn),
        "failed" | "offline" => ("云同步暂未完成", StatusKind::Warn),
        "signed-out" | "unconfigured" => ("云存档尚未配置", StatusKind::Warn),
        _ => ("AstroBox 云存档", StatusKind::Info),
    };
    let account = if snapshot.cloud_qq.is_empty() { snapshot.cloud_account_name.clone() } else { format!("{} · QQ {}", snapshot.cloud_account_name, snapshot.cloud_qq) };
    let mut card = group_heading_with(title, state_badge(kind.label(), kind)).full().child(
        group().child(
            Node::new(Tag::Div)
                .full()
                .column()
                .gap(GAP_XS)
                .pad_y(GROUP_ROW_PAD_Y)
                .child_if(!account.is_empty(), row_detail(account))
                .child(label(snapshot.cloud_detail.clone(), TYPE_META, kind.color())),
        ),
    );
    if !snapshot.cloud_code.is_empty() && matches!(snapshot.cloud_phase.as_str(), "waiting-binding") {
        card = card.child(
            group().child(
                Node::new(Tag::Div)
                    .full()
                    .column()
                    .gap(GAP_XS)
                    .pad_y(GROUP_ROW_PAD_Y)
                    .child(label(snapshot.cloud_code.clone(), TYPE_TITLE, TEXT_MAIN))
                    .child(row_detail(format!("有效期至 {}；发送后点「检查绑定」", snapshot.cloud_expires_at))),
            ),
        );
    }
    let mut buttons = Node::new(Tag::Div).full().row().gap(GAP_SM);
    if matches!(snapshot.cloud_phase.as_str(), "unbound" | "waiting-binding") {
        buttons = buttons.child(primary_button("生成绑定码", actions::CLOUD_BINDING_CODE, true, snapshot))
            .child(quiet_button("检查绑定", actions::CLOUD_SESSION, true, snapshot));
    } else {
        buttons = buttons.child(primary_button("立即同步", actions::CLOUD_SYNC, true, snapshot));
    }
    card.child(Node::new(Tag::Div).full().pad_t(GAP).child(buttons))
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
        ("可读档 · 部分无图片", StatusKind::Warn)
    } else {
        ("可以读档", StatusKind::Good)
    };

    let mut card = group_heading_with(
        "存档管理",
        meta(format!("{manual} 个手动槽 · {}", state.0)),
    )
    .full();

    // 当前进度入口：先读手环上的最新自动存档，再让用户选槽。这是“在插件里存到槽”
    // 的唯一来源，不伪造进度。已有快照时按钮改成明确的“重新读取”。
    let capture_text = if snapshot.pending_current_save.is_some() {
        "重新读取当前进度"
    } else {
        "读取当前进度"
    };
    card = card.child(
        Node::new(Tag::Div)
            .full()
            .pad_t(GAP)
            .child(primary_button(
                capture_text,
                actions::SAVES_CAPTURE,
                snapshot.saves_ready() && !snapshot.saves_busy,
                snapshot,
            )),
    );

    let mut list = group();
    match auto_save_row(rows) {
        Some(save) => list = list.child(slot_row(snapshot, save, "自动存档")),
        None => {
            list = list.child(
                Node::new(Tag::Div)
                    .full()
                    .pad_y(22)
                    .align("center")
                    .child(label(
                        "还没有自动存档。在游戏里读到新的对白，手环会自己写一份。",
                        TYPE_META,
                        TEXT_DIM,
                    )),
            )
        }
    }
    card = card.child(list);
    if !snapshot.saves_notice.is_empty() {
        card = card.child(
            Node::new(Tag::Div)
                .full()
                .pad_t(GAP_SM)
                .pad_x(2)
                .child(label(snapshot.saves_notice.clone(), TYPE_META, TEXT_SUB)),
        );
    }
    // 「有 N 条存档的章节没装」这句在旧代游戏下**不说**：它教用户「去推送页同步那一章」，
    // 而旧代下同步入口正被拦着（上面那张卡就是原因），两句话互相打架。
    if missing > 0 && !snapshot.band_game_legacy {
        card = card.child(
            Node::new(Tag::Div).full().pad_t(GAP_SM).pad_x(2).child(label(
                format!(
                    "有 {missing} 条存档所在章节的资源还没同步。剧情正文随游戏内置，仍可读档；缺的只是该章图片，阅读页会自动降级为纯文字。"
                ),
                TYPE_META,
                WARN,
            )),
        );
    }
    card.child(Node::new(Tag::Div).full().pad_t(GAP_SM).pad_x(2).child(label(
        "读档就是把首页的「继续阅读」指到这一条。存档记的是「哪一章 + 章内位置」，以后增删章节也不会读错。",
        TYPE_META,
        TEXT_DIM,
    )))
}

/// 手动槽列表：GalGame 式「固定槽位 + 当前进度写入」。
///
/// 有 `pending_current_save` 时，每个槽都是写入目标：已占用的写「覆盖」，空槽写
/// 「保存到这里」。没有当前进度时退化为普通浏览列表，但读档 / 删除仍可用。
fn slot_list(snapshot: &Snapshot, rows: &[SaveSlotView]) -> Node {
    let slots: Vec<&SaveSlotView> = rows.iter().filter(|save| !save.is_auto()).collect();
    // 手环的 `recoveryData` 是数组，本身没有 20 槽硬上限；这个数只作为
    // 「列表开始变长」的软提示，绝不能据此拦掉保存、导入或云同步。
    let hint = if slots.len() > MAX_SLOTS {
        format!("{} 条 · 已超过建议的 {MAX_SLOTS} 槽", slots.len())
    } else {
        format!("{} 条 · 建议不超过 {MAX_SLOTS} 条", slots.len())
    };
    let block = group_heading_with("手动存档", meta(hint)).full();
    let pending = snapshot.pending_current_save.as_ref();

    let mut list = group();
    if slots.is_empty() {
        if pending.is_some() {
            list = list.child(empty_write_slot(snapshot, 0));
        } else {
            list = list.child(
                Node::new(Tag::Div)
                    .full()
                    .pad_y(24)
                    .align("center")
                    .child(label(
                        "手环上还没有手动存档。先从游戏内读取当前进度，或从外部导入一份存档。",
                        TYPE_META,
                        TEXT_DIM,
                    )),
            );
        }
    } else {
        for (index, save) in slots.iter().enumerate() {
            if index > 0 {
                list = list.child(divider());
            }
            list = list.child(slot_row(snapshot, save, &save.title()));
        }
        // 当前进度已在手时，末尾再给一个空槽；这是 GalGame 里“新建存档”的位置。
        if pending.is_some() {
            if !slots.is_empty() {
                list = list.child(divider());
            }
            list = list.child(empty_write_slot(snapshot, slots.len()));
        }
    }
    block.child(scroll_list(list, LIST_MAX_HEIGHT)).child(
        Node::new(Tag::Div)
            .full()
            .pad_t(GAP_SM)
            .pad_x(2)
            .child(label(if pending.is_some() {
                "点「覆盖」把当前进度写进已有槽；末尾空槽可直接新建。槽位按手环里的实际顺序排列。"
            } else {
                "先点「读取当前进度」，就能直接保存到任意槽；读档和删除不需要先读取进度。"
            }, TYPE_META, TEXT_DIM)),
    )
}

/// 空槽：只在当前进度已就绪时出现，点击即保存到 `slot`。
fn empty_write_slot(snapshot: &Snapshot, slot: usize) -> Node {
    let current = snapshot.pending_current_save.as_ref();
    let detail = current.map_or_else(
        || "先读取当前进度".to_string(),
        |save| format!("当前进度：{} · {}", save.chapter, save.scene_label()),
    );
    Node::new(Tag::Div)
        .full()
        .row()
        .align("center")
        .gap(GAP_SM)
        .pad_y(GROUP_ROW_PAD_Y)
        .child(
            Node::new(Tag::Div)
                .w(36)
                .h(36)
                .shrink(0.0)
                .radius(10)
                .bg(SURFACE_SOFT)
                .column()
                .align("center")
                .justify("center")
                .child(label(format!("{}", slot + 1), TYPE_BODY, TEXT_DIM)),
        )
        .child(
            Node::new(Tag::Div)
                .column()
                .gap(3)
                .grow(1.0)
                .shrink(1.0)
                .child(row_title(format!("空存档槽 {}", slot + 1)))
                .child(row_detail(detail)),
        )
        .child(accent_chip(
            "保存到这里",
            &actions::save_write_id(slot),
            snapshot.pending_current_save.is_some() && !snapshot.saves_busy,
            snapshot,
        ))
}

/// 一条存档：标题 + 章节/场景/时间 + 未安装徽章 + 「读档」「删除」行内按钮。
///
/// 自动存档与手动槽共用这一段（`title` 由调用方给），所以两边的排版、徽章、
/// 禁用规则永远一致。
fn slot_row(snapshot: &Snapshot, save: &SaveSlotView, title: &str) -> Node {
    let key = actions::save_key(save.slot);
    let confirming = snapshot.is_confirming_save_delete(&key);

    let mut title_row = Node::new(Tag::Div)
        .row()
        .align("center")
        .gap(6)
        .child(row_title(title).grow(1.0).shrink(1.0));
    if save.missing {
        title_row = title_row.child(state_badge("纯文字可读", StatusKind::Warn));
    }
    if save.conflict {
        title_row = title_row.child(state_badge("云冲突副本", StatusKind::Info));
    }
    if snapshot.pending_current_save.is_some() && save.slot.is_some() {
        title_row = title_row.child(state_badge("可覆盖", StatusKind::Info));
    }

    let text = Node::new(Tag::Div)
        .column()
        .gap(3)
        .grow(1.0)
        .shrink(1.0)
        .child(title_row)
        .child(row_detail(save.chapter.clone()))
        .child(row_detail(format!("{} · {}", save.scene_label(), save.time_label())));

    // 自动存档没有「读档」这一步（它本来就是继续阅读的进度），只留删除与本行说明。
    // 手动槽的「读档」**永不禁用**：章节资源包只影响图片，正文仍可读。
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
        let mut row = Node::new(Tag::Div)
            .row()
            .align("center")
            .gap(GAP_SM)
            .child(accent_chip("读档", &actions::save_load_id(save.slot), save.readable(), snapshot));
        if let Some(slot) = save.slot {
            // 覆盖会丢掉一整条存档，和删除一样需要明确动作。原生树没有弹窗，
            // 所以按钮文案直接写「覆盖」，并只在当前进度已就绪时挂事件。
            row = row.child(quiet_button(
                "覆盖",
                &actions::save_write_id(slot),
                snapshot.pending_current_save.is_some() && !snapshot.saves_busy,
                snapshot,
            ));
        }
        row.child(quiet_button("删除", &actions::save_delete_id(save.slot), true, snapshot))
    };

    // 确认删除那一行整体抬亮一档：这是唯一需要「注意」的行，而且它只在被点击后出现。
    Node::new(Tag::Div)
        .full()
        .row()
        .align("center")
        .gap(GAP_SM)
        .pad_y(GROUP_ROW_PAD_Y)
        .bg(if confirming { SURFACE_SOFT } else { TRANSPARENT })
        .child(text)
        .child_if(save.missing, badge("无图片", WARN, WARN_BG))
        .child(action_row)
}

/// 底部动作：导出到剪贴板 / 从剪贴板导入 / 刷新。全是实色按钮，没有模糊、渐变、阴影。
///
/// **走剪贴板，不走系统文件对话框**：Dialog 那一类需要用户交互的宿主调用在这个宿主上
/// 会永远不返回、把事件分发器堵死（真机实测，见 `docs/插件开发注意事项.md` 第 7 节）。
///
/// 按钮行包在**横向滚动区**里（与线路筛选同款的两层保险）：三个按钮在 400px 窗口里
/// 恰好差几个像素放不下，没有滚动区时最右边的「刷新」会被直接裁掉（v2 预览截图实测）。
fn footer(snapshot: &Snapshot, rows: &[SaveSlotView]) -> Node {
    let ready = snapshot.saves_ready();
    let has_saves = !rows.is_empty();

    let buttons = Node::new(Tag::Scroll)
        .full()
        .row()
        .scroll("x")
        .child(
            Node::new(Tag::Div)
                .row()
                .gap(GAP_SM)
                .child(primary_button(
                    "导出到剪贴板",
                    actions::SAVES_EXPORT,
                    ready && has_saves,
                    snapshot,
                ))
                // 导入在 1.x 旧代上必须禁用：写进去的是 2.0 的存档结构，
                // 旧版游戏读不懂，轻则存档列表花脸、重则连累它自己的写档。
                // 导出保留 —— 卸载前把旧存档捞一份到剪贴板，是用户仅剩的后悔药。
                .child(ghost_button(
                    "从剪贴板导入",
                    actions::SAVES_IMPORT,
                    ready && !snapshot.band_game_legacy,
                    snapshot,
                ))
                .child(ghost_button("刷新", actions::SAVES_REFRESH, ready, snapshot)),
        );

    // 先绑定再传引用：`if` 分支里的临时值活不过整条链。

    let mut card = group_heading("复制 / 粘贴（剪贴板）").full().child(
        group().child(
            Node::new(Tag::Div)
                .full()
                .column()
                .gap(GAP_SM)
                .pad_y(GROUP_ROW_PAD_Y)
                .child(buttons)
                .child(label(
                    "导出：把存档打包成一段 JSON 放进剪贴板，界面会报字节数。粘到备忘录或聊天窗口里就存下来了。",
                    TYPE_META,
                    TEXT_DIM,
                ))
                .child(label(
                    "导入：把那段 JSON 原样复制回来（别漏开头结尾），再点这个按钮。插件会按「写档时间」并入手环：同一份覆盖，新的追加到末尾。",
                    TYPE_META,
                    TEXT_DIM,
                ))
                .child(label(
                    "走剪贴板是为了把存档交到你自己手里：插件目录里的文件一卸载就没了。",
                    TYPE_META,
                    TEXT_DIM,
                )),
        ),
    );

    // 上一次导出的结果：**字节数一定要显示**（用户靠它确认剪贴板里是一整份）。
    if let Some(done) = snapshot.save_export.as_ref() {
        card = card.child(
            Node::new(Tag::Div).full().pad_t(GAP_SM).pad_x(2).child(label(
                format!(
                    "上次导出：已复制 {} 个存档到剪贴板（约 {} KB / {} 字节）{}",
                    done.slots,
                    done.bytes.div_ceil(1024),
                    done.bytes,
                    if done.verified { "，已读回核对" } else { "，未能读回核对" }
                ),
                TYPE_META,
                if done.verified { OK } else { WARN },
            )),
        );
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
        // 剪贴板按钮行要包在**横向滚动区**里：三个按钮在 400px 窗口放不下，
        // 没有滚动区时最右边的「刷新」被直接裁掉（v2 预览截图实测）。
        let scrolls = tree.find(|node| node.tag == Tag::Scroll && node.get("scroll") == Some("x"));
        assert!(
            scrolls.iter().any(|area| area.texts().contains(&"刷新")),
            "剪贴板按钮行缺少横向滚动兜底"
        );
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
    fn capture_opens_galgame_style_write_targets() {
        let mut snapshot = page_with_saves();
        // 尚未读取当前进度：只有一个读取入口，没有“覆盖/空槽”可点。
        let tree = render(&snapshot);
        assert_eq!(
            tree.find(|node| node.get("on.click") == Some(actions::SAVES_CAPTURE)).len(),
            1,
            "需要明确的读取当前进度入口"
        );
        assert!(
            tree.find(|node| node.get("on.click").is_some_and(|id| id.starts_with("save-write:"))).is_empty(),
            "没有当前进度时不许开放写槽"
        );

        // 当前进度到手：已有槽出现“覆盖”，末尾出现新的空槽。
        snapshot.pending_current_save = Some(SaveSlotView::from_json(
            &raw_save("玲线1·序章", 77, 77, 1_756_000_900_000),
            None,
            &[],
        ));
        let tree = render(&snapshot);
        let texts = tree.texts();
        assert!(texts.iter().any(|text| text.contains("点「覆盖」")), "{texts:?}");
        assert!(texts.contains(&"空存档槽 3"), "两条已有槽之后要出现第 3 个空槽：{texts:?}");
        assert_eq!(
            tree.find(|node| node.get("on.click") == Some("save-write:0")).len(),
            1,
            "已有槽要有覆盖入口"
        );
        assert_eq!(
            tree.find(|node| node.get("on.click") == Some("save-write:2")).len(),
            1,
            "末尾空槽要有保存入口"
        );
        assert!(texts.contains(&"保存到这里") && texts.contains(&"覆盖"), "{texts:?}");
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
    fn missing_chapter_only_removes_images_not_save_management() {
        let snapshot = page_with_saves();
        let tree = render(&snapshot);
        let texts = tree.texts();
        assert!(
            texts.contains(&"纯文字可读"),
            "资源包缺失时要说清仍可读：{:?}",
            tree.texts()
        );
        // 手环端明确：正文内置，缺包只影响图片。读档不能因为缺图片被禁掉。
        assert_eq!(
            tree.find(|node| node.get("on.click") == Some("save-load:1")).len(),
            1,
            "缺资源包的存档仍要能读档"
        );
        assert_eq!(
            tree.find(|node| node.get("on.click") == Some("save-load:0")).len(),
            1
        );
        let rows = snapshot.saves();
        assert_eq!(missing_save_count(&rows), 1);
        assert!(!rows[1].missing && rows[2].missing, "第 2 槽那条的章节没装");
        assert!(rows[2].readable(), "缺资源包只影响图片，不影响能否读档");
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
    fn legacy_game_replaces_the_blocked_card_and_disables_import() {
        // 手环上是 1.x 旧版、且存档协议还是通的（1.6.x 就有存档通道）：
        // 这张「游戏版本过旧」卡**替代**「存档通道不可用」——它更具体（哪一版、怎么办），
        // 两张一起挂只会让用户把同一件事读两遍。导入必须禁用（写进去的是 2.0 的存档结构，
        // 旧版游戏读不懂）；导出保留 —— 卸载前把旧存档捞一份，是仅剩的后悔药。
        let mut snapshot = page_with_saves();
        snapshot.band_game_legacy = true;
        snapshot.band_version = "1.6.3".into();
        let tree = render(&snapshot);
        let texts = tree.texts();
        assert!(texts.iter().any(|t| t.contains("游戏版本过旧")), "{texts:?}");
        assert!(texts.iter().any(|t| t.contains("1.x 旧版（v1.6.3）")), "版本号要说出来：{texts:?}");
        assert!(texts.iter().any(|t| t.contains("卸载")), "{texts:?}");
        assert!(!texts.iter().any(|t| t.contains("存档通道不可用")), "{texts:?}");
        assert!(tree.find(|node| node.get("on.click") == Some("save-import")).is_empty(), "导入该禁用");
        assert_eq!(tree.find(|node| node.get("on.click") == Some("save-export")).len(), 1, "导出保留");
        assert_eq!(tree.find(|node| node.get("on.click") == Some("save-refresh")).len(), 1, "刷新保留");
        // 夹具里有多条存档的章节没装，但旧代下**不许**再教「去推送页同步那一章」——
        // 同步入口正被拦着，那句话与上面的卡互相打架。
        assert!(!texts.iter().any(|t| t.contains("先在「推送」页同步那一章")), "{texts:?}");
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
        assert!(conclusion.contains("没有回应"), "{conclusion}");
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
