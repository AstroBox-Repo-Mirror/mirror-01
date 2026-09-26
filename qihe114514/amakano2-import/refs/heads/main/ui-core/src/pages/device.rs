//! 设备页：手环连接、手环上已安装的章节、删除与残留清理。
//!
//! 「刷新手环章节列表」搬到了「手环已安装章节」那张卡的标题右侧 —— 它刷新的是
//! **这张列表**，放在页尾单独开一张卡（上一版的「手环侧操作」）就得多一次视线往返；
//! 那张卡同时也被「传输」页抄了一份，已经整块撤掉。
//!
//! 旧版本残留也**就在这张列表里**标一下（「旧版本」徽章 + 底下那句说明），
//! 不再单独开一张卡：同一条记录出现在两处，顶栏的「N/15 章已安装」也会跟列表行数对不上。
//!
//! 章节包删除**一步到位、没有二次确认**：插件里有一份完整副本，
//! 删掉随时能重新同步回来（卡片底下那句话就是说这个）。存档删除不可恢复，
//! 所以只有它保留了确认。

use super::super::actions;
use super::super::glass::{
    alpha, empty, ghost_button, hover_key, hovered, meta, panel, primary_button, quiet_button,
    section_with_action, state_badge, status_pill, title,
};
use super::super::node::{Node, Tag, badge, label};
use super::super::snapshot::{InstalledView, Snapshot, StatusKind};
use super::super::theme::*;
use super::super::human_bytes;

const LIST_MAX_HEIGHT: u32 = 320;

pub fn render(snapshot: &Snapshot) -> Node {
    Node::new(Tag::Div)
        .full()
        .column()
        .gap(GAP_LG)
        .child(link(snapshot))
        .child(broken_notice(snapshot))
        .child(installed_list(snapshot))
}

/// 注册表里有、但 `pack.txt` 读不出来的章节。
///
/// **不许静默**：`scan` 现在只读、不再把读不出来的条目从注册表里剔掉（旧实现会，
/// 一次瞬时读失败就把那一章**永久**丢掉，真机症状是「已安装章节列表缺章/全空、
/// 重连也不好」）。剔不得，就必须说得出来 —— 否则用户看到的是一章的失踪，
/// 而原因在日志里。
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
        .child(label("这些章节在手环上登记着、但包内容读不出来。重新同步一次这一章即可恢复。", SIZE_SMALL, TEXT_DIM))
}

/// 连接状态与主操作。
fn link(snapshot: &Snapshot) -> Node {
    let (kind, text) = if snapshot.device.connected && snapshot.device.alive {
        (StatusKind::Good, "《甜蜜女友2》正在响应".to_string())
    } else if snapshot.device.connected {
        // 探测进度只说「第几次」，不再带「正在等待应用回应」这种半句话 ——
        // 状态胶囊本身就在说这件事。
        let attempt = if snapshot.device.probe_max > 0 {
            format!("（第 {}/{} 次）", snapshot.device.probe_attempt, snapshot.device.probe_max)
        } else {
            String::new()
        };
        (StatusKind::Warn, format!("等应用回应{attempt}"))
    } else if snapshot.device.addr.is_empty() {
        (StatusKind::Info, "还没连过手环".to_string())
    } else {
        (StatusKind::Info, "不在线".to_string())
    };

    let name = if snapshot.device.name.is_empty() {
        "未选择手环".to_string()
    } else {
        snapshot.device.name.clone()
    };
    // 地址只说一遍：没连过时这里是「连上后显示设备地址」这句提示。
    let addr = if snapshot.device.addr.is_empty() {
        "连上后这里会显示设备地址".to_string()
    } else {
        snapshot.device.addr.clone()
    };

    let buttons = Node::new(Tag::Div)
        .full()
        .row()
        .gap(GAP_SM)
        .child(if snapshot.device.connected {
            ghost_button("重新连接", actions::CONNECT, true, snapshot)
        } else {
            primary_button("连接设备", actions::CONNECT, true, snapshot)
        })
        .child(primary_button("打开游戏", actions::LAUNCH, snapshot.device.connected, snapshot));

    panel(CARD_RADIUS)
        .pad(CARD_PAD)
        .gap(GAP_SM)
        .child(title(&name))
        .child(status_pill(kind, &text))
        .child(meta(addr))
        .child(buttons)
}

/// 手环上已安装的章节。
fn installed_list(snapshot: &Snapshot) -> Node {
    let hint = format!("{} 章 · {}", snapshot.installed_count(), human_bytes(snapshot.installed_bytes()));
    let refresh = quiet_button(
        "刷新",
        actions::REFRESH,
        snapshot.device.connected && snapshot.device.alive,
        snapshot,
    );
    let card = section_with_action("手环已安装章节", Some(hint), refresh);

    if snapshot.installed.is_empty() {
        return card.child(empty("手环上一章都没有，去「章节」页同步第一章"));
    }

    let mut container =
        Node::new(Tag::Scroll).full().scroll("y").maxh(LIST_MAX_HEIGHT).column().gap(GAP_XS);
    for record in &snapshot.installed {
        container = container.child(record_row(snapshot, record));
    }

    // 说明只有一句，措辞跟着列表实际状态走：有旧版本残留时多说那半句，
    // 没有的时候不提「旧版本」两个字（免得用户去找一个不存在的东西）。
    let stale = snapshot.stale_installed().len();
    let note = if stale > 0 {
        format!(
            "标「旧版本」的 {stale} 条不在当前插件的章节表里，删掉重新同步一次就不会和新章节混在一起。删的只是手环上那份，插件里还有副本。"
        )
    } else {
        "删掉的只是手环上那份，插件里的副本还在，随时能重新同步".to_string()
    };
    card.child(container).child(meta(note))
}

/// 一条已安装章节：名字 + 体积 + （旧版本才有的徽章）+ 一个删除按钮。
///
/// **没有二次确认**：删错了重新同步一次就回来了，再点一下「确认删除」只是白加一步。
///
/// 「正常」不挂徽章：一屏都是「正常」的徽章等于没有信息，只有**不正常**
/// （旧版本残留）才需要标出来 —— 它也就和正常的行并排躺在同一张列表里，
/// 不另开一张卡把同一条记录再说一遍。
fn record_row(snapshot: &Snapshot, record: &InstalledView) -> Node {
    let row_id = format!("rec:{}", record.id);
    let on = hovered(snapshot, &row_id);

    let text = Node::new(Tag::Div)
        .column()
        .gap(3)
        .grow(1.0)
        .shrink(1.0)
        .child(label(&record.name, SIZE_BODY, if record.stale { WARN } else { TEXT_MAIN }).weight(600))
        .child(meta(format!("{} · {} 个文件", human_bytes(record.bytes), record.files)));

    Node::new(Tag::Div)
        .full()
        .row()
        .align("center")
        .gap(GAP_SM)
        .pad(10)
        .radius(ROW_RADIUS)
        .bg(if on { SURFACE_STRONG } else { SURFACE_SOFT })
        .transition(TRANSITION)
        .hover(&hover_key(&row_id))
        .child(text)
        .child_if(record.stale, badge("旧版本", WARN, WARN_BG))
        .child(quiet_button("删除", &actions::delete_id(&record.id), true, snapshot))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;

    /// 「刷新手环章节列表」只有**一个**入口，而且在已安装列表那张卡里。
    #[test]
    fn refresh_lives_in_the_installed_list_only() {
        let tree = render(&demo());
        assert_eq!(tree.find(|node| node.get("on.click") == Some("refresh-list")).len(), 1, "刷新入口只该有一个");
        // 上一版页尾那张「手环侧操作」卡已经撤掉。
        assert!(!tree.texts().contains(&"手环侧操作"), "{:?}", tree.texts());
        assert!(!tree.texts().iter().any(|text| text.contains("刷新手环章节列表")), "四个字的旧按钮文案已收成两个字");
    }

    /// 章节包删除**一步到位**：没有「确认删除 / 取消」这一对。
    #[test]
    fn chapter_delete_takes_one_click() {
        let tree = render(&demo());
        let texts = tree.texts();
        assert!(!texts.contains(&"确认删除"), "章节包删除不再有二次确认：{texts:?}");
        assert!(!texts.contains(&"取消"), "{texts:?}");
        // 每一条已安装章节都有一个删除按钮，点了就直接删。
        let deletable = tree.find(|node| node.text.as_deref() == Some("删除"));
        assert!(deletable.len() >= snapshot_delete_count(&demo()), "每条已安装章节都要能删");
        assert!(tree.find(|node| node.get("on.click") == Some("cancel-delete")).is_empty());
    }

    fn snapshot_delete_count(snapshot: &Snapshot) -> usize {
        snapshot.installed.len()
    }

    /// 只有「不正常」才挂徽章：一屏「正常」等于没有信息。
    #[test]
    fn only_stale_records_get_a_badge() {
        let tree = render(&demo());
        let texts = tree.texts();
        assert!(texts.contains(&"旧版本"), "旧版本残留要标出来：{texts:?}");
        assert!(!texts.contains(&"正常"), "「正常」这种全是正例的徽章不该存在：{texts:?}");
    }

    /// 同一条手环记录**只出现在一处**，而且「旧版本残留」不再单独开卡。
    ///
    /// 上一版把 `common-01` 既画在「手环已安装章节」列表里、又在页尾那张「旧版本残留」
    /// 卡里再说一遍（两个删除按钮）；而且卡片标题里的计数与顶栏那句「N/15 章已安装」
    /// 一旦分开算就会对不上。这条用例把「一条记录一行」钉死。
    #[test]
    fn every_installed_record_is_listed_exactly_once() {
        let snapshot = demo();
        let tree = render(&snapshot);
        // 页面名：「旧版本残留」那张卡已撤掉，残留只在列表里带徽章。
        assert!(!tree.texts().contains(&"旧版本残留"), "{:?}", tree.texts());
        // 删除按钮逐个对应记录 id，且没有重复。
        let delete_ids: Vec<&str> = tree
            .find(|node| node.text.as_deref() == Some("删除"))
            .iter()
            .filter_map(|node| node.get("on.click"))
            .collect();
        assert_eq!(delete_ids.len(), snapshot.installed.len(), "每条记录恰好一个删除按钮：{delete_ids:?}");
        let unique: std::collections::BTreeSet<&str> = delete_ids.iter().copied().collect();
        assert_eq!(unique.len(), delete_ids.len(), "同一条记录不该有两个删除按钮：{delete_ids:?}");
        // 列表标题里的计数与顶栏同一份口径（`installed` 全表，含旧版本残留）。
        assert!(
            tree.texts().iter().any(|text| text.contains(&format!("{} 章 ·", snapshot.installed_count()))),
            "{:?}",
            tree.texts()
        );
    }

    /// 说明那句跟着列表状态换：有残留才提「旧版本」。
    #[test]
    fn the_footnote_only_mentions_stale_rows_when_there_are_any() {
        let mut clean = demo();
        clean.installed.retain(|record| !record.stale);
        let clean_tree = render(&clean);
        let clean_texts = clean_tree.texts();
        assert!(
            !clean_texts.iter().any(|text| text.contains("旧版本")),
            "没有残留时不该提「旧版本」：{clean_texts:?}"
        );

        let stale_tree = render(&demo());
        let stale_texts = stale_tree.texts();
        assert!(
            stale_texts.iter().any(|text| text.contains("标「旧版本」的 1 条")),
            "有残留时要说明它是什么：{stale_texts:?}"
        );
    }

    /// 连上但应用没在跑时，状态胶囊说的是「等应用回应」，不再重复半句话。
    #[test]
    fn connected_but_silent_device_says_one_short_thing() {
        let mut snapshot = demo();
        snapshot.device.alive = false;
        snapshot.device.probe_attempt = 3;
        snapshot.device.probe_max = 12;
        let tree = render(&snapshot);
        let texts = tree.texts();
        assert!(texts.iter().any(|text| *text == "等应用回应（第 3/12 次）"), "{texts:?}");
        assert!(!texts.iter().any(|text| text.contains("正在等待应用回应")), "半句话已收短：{texts:?}");
    }
}

/// 注册表里读不出来的条目**必须说出来**（`scan` 现在只读，不剔除它们）。
#[cfg(test)]
mod broken_tests {
    use super::*;
    use crate::demo;

    #[test]
    fn broken_entries_are_surfaced_and_silent_when_none() {
        let mut snapshot = demo();
        assert!(snapshot.installed_broken.is_empty());
        let clean = render(&snapshot);
        assert!(
            !clean.texts().iter().any(|text| text.contains("读不出来")),
            "没有坏条目时不该出现任何相关文案：{:?}",
            clean.texts()
        );

        snapshot.installed_broken = vec!["玲线2·初恋".to_string(), "结灯线3·相伴".to_string()];
        let tree = render(&snapshot);
        let texts = tree.texts();
        assert!(texts.iter().any(|text| text.contains("2 条登记读不出来")), "{texts:?}");
        assert!(texts.iter().any(|text| text.contains("玲线2·初恋、结灯线3·相伴")), "要逐条点名：{texts:?}");
        assert!(texts.iter().any(|text| text.contains("重新同步一次这一章")), "要给「怎么办」：{texts:?}");
    }
}
