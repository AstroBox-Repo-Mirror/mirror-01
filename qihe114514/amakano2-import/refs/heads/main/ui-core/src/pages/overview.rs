//! 概览页：现在什么状态、接下来做什么、正在传的进度。
//!
//! **只有三张卡**（空闲时两张）：设备与主操作、进行中的传输/续传、接下来要做的一件事。
//! 上一版这里是六张卡（顶栏之外还有 hero、实时进度、三个统计格、下一步清单、
//! 未完成传输），其中「实时进度」和已经撤掉的「传输」页说的是同一件事，
//! 「下一步」那张清单一次列四件事、每件一个颜色圆点 —— 读起来像日志。
//! 现在规矩是：**一页里只出现一件需要用户动手的事**，其余等它变成第一件时再说。

use super::super::actions;
use super::super::glass::{
    error_card, ghost_button, kv_grid, meta, panel, primary_button, progress_bar, section,
    session_steps, state_badge, stat_tile, status_pill, tile_row,
};
use super::super::node::{Node, Tag, label};
use super::super::snapshot::{ResumeView, Snapshot, StatusKind, TransferView};
use super::super::theme::*;
use super::super::human_bytes;

/// 队列里最多点名几章，剩下的折成「等 N 章」。
const QUEUE_NAMES: usize = 3;

pub fn render(snapshot: &Snapshot) -> Node {
    // 失败卡片排在最前：出事了就该先看见它，而不是先看见「接下来做什么」。
    // 文案由码决定（`ErrorCode::label` / `advice`），这里只负责摆。
    let mut page = Node::new(Tag::Div).full().column().gap(GAP_LG).child(hero(snapshot));
    if let Some(error) = snapshot.error.as_ref() {
        page = page.child(error_card(error));
    }
    if let Some(card) = in_progress(snapshot) {
        page = page.child(card);
    }
    page.child(next_card(snapshot))
}

/// 主卡：手环 + 两个主操作 + 三个统计格。
///
/// 统计格从「独立的一行卡片」并入这张卡：它们是**同一件事的三个数字**
/// （装了多少、库有多大、能读多久），单独占一张卡片只是多一层边框。
fn hero(snapshot: &Snapshot) -> Node {
    let name = if !snapshot.device.name.is_empty() {
        snapshot.device.name.clone()
    } else if !snapshot.device.addr.is_empty() {
        snapshot.device.addr.clone()
    } else {
        "尚未连接手环".to_string()
    };

    let head = Node::new(Tag::Div)
        .full()
        .row()
        .align("center")
        .gap(GAP)
        .child(label(name, SIZE_TITLE, TEXT_MAIN).weight(650).grow(1.0))
        .child(status_pill(
            if snapshot.device.connected && snapshot.device.alive {
                StatusKind::Good
            } else if snapshot.device.connected {
                StatusKind::Warn
            } else {
                StatusKind::Info
            },
            &snapshot.head_line(),
        ));

    let buttons = Node::new(Tag::Div)
        .full()
        .row()
        .gap(GAP_SM)
        .align("center")
        .child(if snapshot.device.connected {
            ghost_button("重新连接", actions::CONNECT, true, snapshot)
        } else {
            primary_button("连接设备", actions::CONNECT, true, snapshot)
        })
        .child(primary_button("打开游戏", actions::LAUNCH, snapshot.device.connected, snapshot));

    let installed = format!("{}/{}", snapshot.installed_count(), snapshot.library.len());
    let capacity = format!("{:.1}", snapshot.library_bytes() as f64 / 1_048_576.0);
    let tiles = tile_row(vec![
        stat_tile(&installed, "章", "已安装", OK),
        stat_tile(&capacity, "MB", "章节库", TEXT_MAIN),
        stat_tile(&snapshot.total_hours(), "小时", "总时长", TEXT_MAIN),
    ]);

    // 连接进度就在主卡里，紧挨着两个主操作：**它回答的是「我刚才那一下点到哪一步了」**，
    // 这正好是旧实现说不清的那件事（接了、握着、列表却没拿到）。
    panel(CARD_RADIUS).pad(CARD_PAD).gap(GAP).child(head).child(buttons).child(session_steps(snapshot.stage)).child(tiles)
}

/// 进行中的事：正在传输，或者上次传到一半。两者互斥，所以共用一张卡。
///
/// 都没有时返回 `None` —— **不占位**：空状态卡片只是把「今天没事」再说一遍。
fn in_progress(snapshot: &Snapshot) -> Option<Node> {
    if let Some(transfer) = snapshot.transfer.as_ref().filter(|transfer| transfer.started) {
        return Some(transfer_card(snapshot, transfer));
    }
    snapshot.resume.as_ref().map(|resume| resume_card(snapshot, resume))
}

/// 传输中。
fn transfer_card(snapshot: &Snapshot, transfer: &TransferView) -> Node {
    let marker = state_badge(
        if transfer.ready {
            "等待手环确认"
        } else if transfer.resumed {
            "断点续传"
        } else {
            "传输中"
        },
        if transfer.shaky() { StatusKind::Warn } else { StatusKind::Good },
    );

    let head = Node::new(Tag::Div)
        .full()
        .row()
        .align("center")
        .gap(GAP)
        .child(label(format!("{}%", transfer.percent), SIZE_HERO, ACCENT).weight(700))
        .child(label(&transfer.chapter, SIZE_BODY, TEXT_MAIN).weight(600).grow(1.0))
        .child(marker);

    let tiles = kv_grid(
        &[
            ("速度", transfer.speed_label()),
            ("剩余", transfer.eta_label()),
            ("分片", format!("{} / {}", transfer.chunks_done, transfer.chunks_total)),
            ("往返", format!("{} ms", transfer.rtt_ms)),
        ],
        2,
    );

    let mut card = section("正在同步", None)
        .child(head)
        .child(progress_bar(transfer.percent, 24))
        .child(tiles)
        .child(meta(format!("{} / {}", human_bytes(transfer.received), human_bytes(transfer.total))));

    // 队列只说一句：这一章排完还有哪几章在等。
    if !snapshot.queue.is_empty() {
        let names: Vec<String> =
            snapshot.queue.iter().take(QUEUE_NAMES).map(|number| snapshot.title_of(*number)).collect();
        let rest = snapshot.queue.len().saturating_sub(names.len());
        let tail = if rest > 0 { format!(" 等 {} 章", snapshot.queue.len()) } else { String::new() };
        card = card.child(meta(format!("队列：{}{tail}", names.join("、"))));
    }

    // 只有真的丢过包才多说一句 —— 那张卡上「让手环停在游戏页面」和
    // 「传完之前别离开游戏」本来是同一句建议说了两遍，现在合成一句、而且按需出现。
    if transfer.shaky() {
        card = card.child(label(
            "链路丢过包，正在自动重传。传完之前别离开《甜蜜女友2》，否则会停在当前这一片。",
            SIZE_TINY,
            WARN,
        ));
    }
    card
}

/// 上次传到一半。
fn resume_card(snapshot: &Snapshot, resume: &ResumeView) -> Node {
    let number = snapshot.pack_by_id(&resume.pack_id).map(|pack| pack.number);
    let head = Node::new(Tag::Div)
        .full()
        .row()
        .align("center")
        .gap(GAP)
        .child(label(format!("{}%", resume.percent), SIZE_HERO, WARN).weight(700))
        .child(label(&resume.chapter, SIZE_BODY, TEXT_MAIN).weight(600).grow(1.0))
        .child(state_badge("上次没传完", StatusKind::Warn));

    let body = kv_grid(
        &[
            ("已传", format!("{} / {}", human_bytes(resume.received), human_bytes(resume.total))),
            ("从第几片起", format!("{}", resume.resume_from + 1)),
        ],
        2,
    );

    let mut card = section("上次没传完", None).child(head).child(body);
    card = match number {
        // 断点还能用：直接给「接着传」。
        Some(number) => card.child(Node::new(Tag::Div).full().row().gap(GAP_SM).align("center").child(
            primary_button("接着传", &actions::sync_id(number), !snapshot.is_transferring(), snapshot),
        )),
        // 章节表变了（这一章已经不在了）：给一句人话，别给一个点了也没用的按钮。
        None => card.child(label(
            "这一章已经不在当前章节表里了，去「章节」页重新同步一次就行",
            SIZE_TINY,
            WARN,
        )),
    };
    card.child(ghost_button("清掉断点缓存", actions::CLEAR_CACHE, true, snapshot))
}

/// 接下来：**只有一件事**，加一个直接把它做掉的按钮。
fn next_card(snapshot: &Snapshot) -> Node {
    let (kind, text) = snapshot.next_step();
    let mut card = section("接下来", None).child(label(text, SIZE_SMALL, kind.color()));

    // 有没装的章节时，主操作就在这一张卡里 —— 用户不用再想「去哪一页点」。
    if snapshot.pending_count() > 0 && snapshot.library_error.is_empty() {
        card = card.child(primary_button(
            &format!("同步剩余 {} 章", snapshot.pending_count()),
            actions::SYNC_ALL,
            !snapshot.is_transferring() && snapshot.device.connected && snapshot.device.alive,
            snapshot,
        ));
    }
    card
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;

    /// 空闲时**只有两张卡**（主卡 + 接下来）。上一版是六张，这一条防它再长回去。
    #[test]
    fn idle_overview_stays_at_two_cards() {
        let mut snapshot = demo();
        snapshot.transfer = None;
        snapshot.resume = None;
        let tree = render(&snapshot);
        let cards = tree.find(|node| node.get("bg") == Some(SURFACE) && node.get("radius") == Some("16"));
        assert_eq!(cards.len(), 2, "空闲的概览页只该有「主卡 + 接下来」两张卡");
        // 三种统计数字并进了主卡，不再是单独一张卡。
        let texts = tree.texts();
        for caption in ["已安装", "章节库", "总时长"] {
            assert!(texts.contains(&caption), "主卡里要有「{caption}」这一格：{texts:?}");
        }
    }

    /// 「接下来」那句话按状态变，而且**同一时刻只渲染一句**。
    #[test]
    fn next_card_says_exactly_one_thing() {
        let mut snapshot = demo();
        snapshot.transfer = None;
        snapshot.resume = None;
        let _ = snapshot.next_step();
        let tree = render(&snapshot);
        // 上一版那四句长文案一句都不该再出现。
        for gone in ["在手环上打开《甜蜜女友2》，插件会在 30 秒内自动接管", "传输期间请让手环停在", "同步队列里还有"] {
            assert!(!tree.texts().iter().any(|text| text.contains(gone)), "旧文案还在：{gone}");
        }
        assert!(tree.texts().iter().any(|text| text.contains(snapshot.next_step().1.as_str())));
    }

    /// 传输中：进度、四个明细、队列一句话都在，而且**没有「查看传输详情」这个跳转**
    /// （传输页已撤掉，详情就在本页）。
    #[test]
    fn transferring_shows_progress_and_queue_inline() {
        let mut snapshot = demo();
        snapshot.queue = vec![6, 7, 8, 9];
        let tree = render(&snapshot);
        let texts = tree.texts();
        assert!(texts.contains(&"33%"), "要显示进度：{texts:?}");
        for key in ["速度", "剩余", "分片", "往返"] {
            assert!(texts.contains(&key), "缺明细「{key}」：{texts:?}");
        }
        assert!(texts.iter().any(|text| text.contains("队列：")), "要有队列那一句：{texts:?}");
        assert!(!texts.iter().any(|text| text.contains("查看传输详情")), "传输页已撤销，不该再有这个跳转");
        assert!(
            tree.find(|node| node.get("on.click") == Some("nav:transfer")).is_empty(),
            "不许再有指向已删除页面的导航动作"
        );
    }

    /// 断点续传：给「接着传」，并把清理缓存的入口收在同一张卡里。
    #[test]
    fn resume_card_offers_continue_and_cleanup() {
        let mut snapshot = demo();
        snapshot.transfer = None;
        let tree = render(&snapshot);
        assert_eq!(tree.find(|node| node.get("on.click") == Some("sync:4")).len(), 1, "「接着传」要指向断点所在那章");
        assert_eq!(tree.find(|node| node.get("on.click") == Some("clear-cache")).len(), 1);
        assert!(tree.texts().contains(&"上次没传完"), "{:?}", tree.texts());
    }

    /// 主操作按钮同时只有一个：空闲且有未装章节时，「同步这 N 章」是唯一的主按钮。
    #[test]
    fn only_one_primary_action_at_a_time() {
        let mut snapshot = demo();
        snapshot.transfer = None;
        snapshot.resume = None;
        snapshot.device.connected = false;
        snapshot.device.alive = false;
        let tree = render(&snapshot);
        // 没连手环时「打开游戏」与「同步这 N 章」都不该是可点的主操作。
        assert!(tree.find(|node| node.get("on.click") == Some("sync-all")).is_empty());
        assert!(tree.find(|node| node.get("on.click") == Some("launch")).is_empty());
        assert_eq!(tree.find(|node| node.get("on.click") == Some("connect")).len(), 1);

        snapshot.device.connected = true;
        snapshot.device.alive = true;
        let tree = render(&snapshot);
        assert_eq!(tree.find(|node| node.get("on.click") == Some("sync-all")).len(), 1);
        assert_eq!(tree.find(|node| node.get("on.click") == Some("launch")).len(), 1);
    }

    /// 传输中不该再出现「同步这 N 章」——正在进行时那个按钮已经换成禁用态。
    #[test]
    fn batch_sync_is_disabled_while_transferring() {
        let snapshot = demo();
        let tree = render(&snapshot);
        let buttons = tree.find(|node| node.text.as_deref() == Some("同步剩余 11 章"));
        assert_eq!(buttons.len(), 1, "{}", tree.texts().join(" | "));
        assert!(buttons[0].has("disabled"), "传输中批量的入口要关掉");
        assert!(!buttons[0].has("on.click"));
        assert!(tree.texts().iter().any(|text| text.contains("正在同步")));
    }
}
