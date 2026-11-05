//! 设置页：传输参数、行为开关、缓存、设备详情、运行日志、插件信息。
//!
//! 这一页吸收了两块原本各自占一个导航位的内容：**设备详情**（原「设备」页）与**运行日志**
//! （原「日志」页）。它们都是低频的「事实 / 排障」，不值得各占一个 tab。
//! 连接与重新连接的操作**不在这里** —— 那是任务条状态区的事（单一出口），
//! 本页只如实显示设备是谁、手环端版本多少。
//!
//! 临时自检卡（渲染自检、存档功能自检）**都已删除**，结论落在
//! `docs/插件开发注意事项.md`，别再往这一页加实验装置。

use super::super::actions;
use super::super::glass::{
    divider, ghost_button, group, group_heading, group_heading_with, group_row_with, kv_grid, meta,
    note, quiet_button, row_detail, row_title, scroll_list, segmented, value,
};
use super::super::human_bytes;
use super::super::node::{Node, Tag, badge, label};
use super::super::snapshot::{
    chunk_label, LogFilter, LogLine, Snapshot, TransferMode, CHUNK_OPTIONS,
};
use super::super::theme::*;

/// 日志区的高度上限（与旧版同值）。
const LOG_MAX_HEIGHT: u32 = 360;

pub fn render(snapshot: &Snapshot) -> Node {
    Node::new(Tag::Div)
        .full()
        .column()
        .child(chunk_settings(snapshot))
        .child(behavior(snapshot))
        .child(cache(snapshot))
        .child(device(snapshot))
        .child(logs(snapshot))
        .child(about(snapshot))
}

/// 传输分片与协议参数。**分片档位只有这一个入口**。
/// 传输模式与分片档位。**传输模式只有这一个入口**（分片跟着它走，也可以单独调）。
///
/// 两档的差别在**确认粒度**上，不只是分片大小：正常档每 4 片回执、丢片立刻补；
/// 激进档几乎不回执、分片翻倍，把消息数压到最低。所以这一块必须把代价写出来 ——
/// 用户点「激进」时要知道「丢片要重发一整窗」，而不是以为它只是「快一点」。
fn chunk_settings(snapshot: &Snapshot) -> Node {
    let busy = snapshot.is_transferring();
    let modes: Vec<(String, String, bool)> = TransferMode::ALL
        .into_iter()
        .map(|mode| (mode.label().to_string(), actions::mode_id(mode), mode == snapshot.transfer_mode))
        .collect();
    let items: Vec<(String, String, bool)> = CHUNK_OPTIONS
        .iter()
        .map(|bytes| (chunk_label(*bytes), actions::chunk_id(*bytes), *bytes == snapshot.chunk_bytes))
        .collect();
    let limits = &snapshot.limits;
    let params = snapshot.transfer_mode.params();

    // 两栏并排时每格只有 ~170px：**值一定要短**，长了就会折成两行把小字挤乱。
    let grid = kv_grid(
        &[
            ("确认节拍", format!("{} 片 / {} ms", params.ack_every.max(1), params.ack_ms)),
            ("飞行窗口", format!("{}→{} 片", params.initial_window, params.window_cap)),
            ("停滞判定", format!("{:.1} 秒", params.stall_floor_ms as f64 / 1000.0)),
            ("巡视间隔", format!("{} ms", limits.retry_delay_ms)),
            ("存档超时", format!("{} ms", limits.saves_timeout_ms)),
            ("单章上限", human_bytes(limits.max_pack_bytes)),
        ],
        2,
    );

    // 手环不认激进档时，必须说明「为什么点了没变快」—— 否则用户会以为设置没生效。
    let mode_note = if snapshot.transfer_mode == TransferMode::Turbo || snapshot.turbo_blocked.is_empty() {
        note(snapshot.transfer_mode.hint())
    } else {
        note(format!("{}（已按手环能力退回正常档：{}）", TransferMode::Steady.hint(), snapshot.turbo_blocked))
    };

    group_heading_with(
        "传输模式",
        meta(if busy { "传输中改不了" } else { "快了就要多担风险" }),
    )
    .full()
    .child(
        group().child(
            Node::new(Tag::Div)
                .full()
                .column()
                .gap(GAP_SM)
                .pad_y(GROUP_ROW_PAD_Y)
                .child(segmented(&modes, !busy, snapshot))
                .child(mode_note)
                .child(divider())
                .child(row_detail("分片大小"))
                .child(segmented(&items, !busy, snapshot))
                .child(row_detail(
                    "分片大小决定单条消息的体积：档位越高消息越少、跑得越快，遇到丢片也重发得更多。",
                )),
        ),
    )
    .child(group_heading("协议参数").full())
    .child(group().child(Node::new(Tag::Div).full().pad_y(GROUP_ROW_PAD_Y).child(grid)))
}

/// 行为开关。
fn behavior(snapshot: &Snapshot) -> Node {
    let items = vec![
        ("开启".to_string(), actions::AUTO_ON.to_string(), snapshot.auto_launch),
        ("关闭".to_string(), actions::AUTO_OFF.to_string(), !snapshot.auto_launch),
    ];
    group_heading("行为").full().child(
        group().child(group_row_with(
            Node::new(Tag::Div)
                .column()
                .gap(3)
                .grow(1.0)
                .shrink(1.0)
                .child(row_title("连上后自动打开《甜蜜女友2》"))
                .child(row_detail("连上手环后顺带把手环应用调到前台")),
            segmented(&items, true, snapshot),
        )),
    )
}

/// 缓存。
fn cache(snapshot: &Snapshot) -> Node {
    let hint = format!("{} · {} 个文件", human_bytes(snapshot.cache_bytes), snapshot.cache_files);
    let button = ghost_button("清理", actions::CLEAR_CACHE, snapshot.cache_files > 0, snapshot);
    let mut block = group_heading_with("未完成缓存", button).full();
    block = block.child(
        group().child(
            Node::new(Tag::Div)
                .full()
                .column()
                .gap(3)
                .pad_y(GROUP_ROW_PAD_Y)
                .child(row_title(hint))
                .child(row_detail("断点续传要用到它；清掉之后，没传完的章节只能从头再传。")),
        ),
    );
    block
}

/// 设备详情。**只摆事实，不放连接 / 打开游戏按钮**（那在顶栏与推送页）。
fn device(snapshot: &Snapshot) -> Node {
    let name = if snapshot.device.name.is_empty() {
        "尚未选择手环".to_string()
    } else {
        snapshot.device.name.clone()
    };
    let addr = if snapshot.device.addr.is_empty() { "—".to_string() } else { snapshot.device.addr.clone() };
    let band = if snapshot.band_version.is_empty() {
        "未知".to_string()
    } else {
        snapshot.band_version.clone()
    };
    let protocol = match snapshot.save_protocol {
        Some(version) => format!("v{version}"),
        None => "未协商".to_string(),
    };
    let pairs = [
        ("设备", name),
        ("地址", addr),
        ("手环端版本", band),
        ("存档协议", protocol),
    ];
    let mut list = group();
    for (index, (key, text)) in pairs.iter().enumerate() {
        if index > 0 {
            list = list.child(divider());
        }
        list = list.child(group_row_with(label(*key, TYPE_BODY, TEXT_SUB), value(text.clone())));
    }
    group_heading("设备与连接").full().child(list)
}

/// 运行日志（原「日志」页）。
fn logs(snapshot: &Snapshot) -> Node {
    let lines = snapshot.filtered_logs();
    // 行数 + 错误数挂在组标题右侧：先看「有没有错」，再看内容。
    let hint = if snapshot.log_errors > 0 {
        format!("{} 行 · {} 个错误", lines.len(), snapshot.log_errors)
    } else {
        format!("{} 行", lines.len())
    };

    let mut list = group();
    if lines.is_empty() {
        list = list.child(
            Node::new(Tag::Div)
                .full()
                .pad_y(20)
                .align("center")
                .child(label(
                    if snapshot.logs.is_empty() { "还没有日志" } else { "当前筛选下没有日志" },
                    TYPE_META,
                    TEXT_DIM,
                )),
        );
    } else {
        for (index, line) in lines.iter().enumerate() {
            if index > 0 {
                list = list.child(divider());
            }
            list = list.child(log_row(line));
        }
    }

    // 标题行右侧：行数/错误数 + 「清空」。两个都固定宽度，标题自己收缩。
    let head_action = Node::new(Tag::Div)
        .row()
        .align("center")
        .gap(GAP_SM)
        .shrink(0.0)
        .child(meta(hint))
        .child(quiet_button("清空", actions::LOG_CLEAR, !snapshot.logs.is_empty(), snapshot));

    // 日志可能有上百行 —— 包一层验证过可用的滚动区（见 `glass::scroll_list`）。
    group_heading_with("运行日志", head_action)
        .full()
        .child(
            group().child(
                Node::new(Tag::Div)
                    .full()
                    .pad_y(GAP)
                    .child(log_controls(snapshot)),
            ),
        )
        .child(scroll_list(list, LOG_MAX_HEIGHT))
        .child(
            Node::new(Tag::Div)
                .full()
                .pad_t(GAP_SM)
                .pad_x(2)
                .child(label(
                    "只留最近一段。出问题时把这一页截图发给作者，比复述状态行管用。",
                    TYPE_META,
                    TEXT_DIM,
                )),
        )
}

/// 级别筛选。
fn log_controls(snapshot: &Snapshot) -> Node {
    let items: Vec<(String, String, bool)> = LogFilter::ALL
        .into_iter()
        .map(|filter| {
            let count = match filter {
                LogFilter::All => snapshot.logs.len(),
                LogFilter::Warn => snapshot.log_warns,
                LogFilter::Error => snapshot.log_errors,
            };
            let text = if count > 0 { format!("{} {count}", filter.label()) } else { filter.label().to_string() };
            (text, actions::log_filter_id(filter), snapshot.log_filter == filter)
        })
        .collect();

    Node::new(Tag::Div)
        .full()
        .row()
        .align("center")
        .gap(GAP)
        .child(segmented(&items, true, snapshot))
}

/// 一行日志：级别标签 + 原文。
///
/// 宿主没有 `word-break`（白名单外），所以长行**不会自动断词** ——
/// 日志文本要靠外层 `grow(1)/shrink(1)` 允许收窄，让中文按字断行。
fn log_row(line: &LogLine) -> Node {
    Node::new(Tag::Div)
        .full()
        .row()
        .align("start")
        .gap(GAP_SM)
        .pad_y(10)
        .child(badge(line.level.label(), line.level.color(), "rgba(255,255,255,0.07)"))
        .child(
            Node::new(Tag::Div)
                .column()
                .grow(1.0)
                .shrink(1.0)
                .child(label(&line.text, TYPE_META, line.level.color())),
        )
}

/// 关于。
fn about(snapshot: &Snapshot) -> Node {
    let version = if snapshot.version.is_empty() {
        "未知".to_string()
    } else {
        format!("v{}", snapshot.version)
    };
    let pairs = [
        ("插件版本", version),
        ("内置章节", format!("{} 章", snapshot.library.len())),
        ("已装到手环", format!("{} 章", snapshot.installed_count())),
        ("章节库容量", human_bytes(snapshot.library_bytes())),
    ];
    let mut list = group();
    for (index, (key, text)) in pairs.iter().enumerate() {
        if index > 0 {
            list = list.child(divider());
        }
        list = list.child(group_row_with(label(*key, TYPE_BODY, TEXT_SUB), value(text.clone())));
    }
    group_heading("关于")
        .full()
        .child(list)
        .child(
            Node::new(Tag::Div)
                .full()
                .pad_t(GAP_SM)
                .pad_x(2)
                .child(label("章节包已经随插件装好，不需要本机里的游戏文件。", TYPE_META, TEXT_DIM)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{Page, TransferView};

    /// 设置页该有的块都在（别在改写时把整块删掉），也不许有自检装置。
    #[test]
    fn settings_has_all_sections_and_no_experiment_probes() {
        let mut snapshot = Snapshot::default();
        snapshot.page = Page::Settings;
        let tree = render(&snapshot);
        let texts = tree.texts();
        for text in ["传输模式", "行为", "未完成缓存", "设备与连接", "运行日志", "关于"] {
            assert!(texts.contains(&text), "设置页缺少「{text}」：{texts:?}");
        }
        assert!(!texts.iter().any(|text| text.contains("自检")), "{texts:?}");
        assert!(!texts.iter().any(|text| text.contains("临时")), "{texts:?}");
        // 连接 / 打开游戏不在这一页（单一出口）。
        assert!(tree.find(|node| node.get("on.click") == Some("connect")).is_empty());
        assert!(tree.find(|node| node.get("on.click") == Some("launch")).is_empty());
    }

    /// 分片档位与传输模式都只有设置页这一个入口；协议参数也钉在这里。
    #[test]
    fn chunk_size_and_protocol_limits_stay_visible() {
        let mut snapshot = Snapshot::default();
        snapshot.page = Page::Settings;
        let tree = render(&snapshot);
        let texts = tree.texts();
        for option in CHUNK_OPTIONS {
            let id = actions::chunk_id(option);
            assert_eq!(tree.find(|node| node.get("on.click") == Some(id.as_str())).len(), 1, "{id} 应该恰好一个入口");
        }
        for mode in TransferMode::ALL {
            let id = actions::mode_id(mode);
            assert_eq!(tree.find(|node| node.get("on.click") == Some(id.as_str())).len(), 1, "{id} 应该恰好一个入口");
        }
        for value in ["2000 ms", "120 ms", "1.5 秒", "6→10 片", "4 片 / 90 ms"] {
            assert!(texts.iter().any(|text| text.contains(value)), "缺参数 {value}：{texts:?}");
        }
    }

    /// 活跃传输的两种阶段都必须禁改：等握手与 `ready=true` 的数据阶段。
    /// 直接盯按钮属性，防止状态函数和设置页再次各自理解「在传」。
    #[test]
    fn active_transfer_disables_mode_and_chunk_buttons() {
        for ready in [false, true] {
            let mut snapshot = Snapshot::default();
            snapshot.page = Page::Settings;
            snapshot.transfer = Some(TransferView {
                chapter: "共通线".into(),
                percent: 20,
                received: 200,
                total: 1000,
                chunks_done: 2,
                chunks_total: 10,
                speed_kbps: 20.0,
                rtt_ms: 50,
                retries: 0,
                resumed: false,
                ready,
                started: true,
                stalled: false,
                chunk_bytes: 16384,
            });
            let tree = render(&snapshot);
            for mode in TransferMode::ALL {
                let id = actions::mode_id(mode);
                assert!(tree.find(|node| node.get("on.click") == Some(id.as_str())).is_empty(),
                    "ready={ready} 时禁用按钮不应再挂 {id} 点击动作");
            }
            for bytes in CHUNK_OPTIONS {
                let id = actions::chunk_id(bytes);
                assert!(tree.find(|node| node.get("on.click") == Some(id.as_str())).is_empty(),
                    "ready={ready} 时禁用按钮不应再挂 {id} 点击动作");
            }
            // 这一卡有 2 个模式 + 3 个分片；任务条/缓存等状态还可能贡献禁用按钮，
            // 所以只断言下限，真正的精确约束是上面「所有传输动作都已移除」。
            let disabled = tree.find(|node| node.get("disabled") == Some("1"));
            assert!(disabled.len() >= 5, "ready={ready} 时两档与三个分片都应禁用，实际 {disabled:?}");
            assert!(tree.texts().contains(&"传输中改不了"));
        }

        // 停滞之后允许重新选择，不能把用户永久锁在旧参数上。
        let mut snapshot = Snapshot::default();
        snapshot.page = Page::Settings;
        snapshot.transfer = Some(TransferView {
            chapter: "共通线".into(), percent: 20, received: 200, total: 1000,
            chunks_done: 2, chunks_total: 10, speed_kbps: 0.0, rtt_ms: 50,
            retries: 1, resumed: false, ready: true, started: true, stalled: true,
            chunk_bytes: 16384,
        });
        let tree = render(&snapshot);
        let id = actions::mode_id(TransferMode::Turbo);
        let button = tree.find(|node| node.get("on.click") == Some(id.as_str()));
        assert_eq!(button.len(), 1);
        assert_ne!(button[0].get("disabled"), Some("1"));
    }

    /// 手环不认激进档时，界面必须**当场解释**，不能只是把选中态悄悄换回「正常」——
    /// 用户点了「激进」却看到速度没变、还没有一句话，只会以为设置坏了。
    #[test]
    fn a_blocked_turbo_mode_says_why() {
        let mut snapshot = Snapshot::default();
        snapshot.page = Page::Settings;
        snapshot.transfer_mode = TransferMode::Steady;
        snapshot.turbo_blocked = "手环端应用 3.1.1 不支持".into();
        let blocked_tree = render(&snapshot);
        let texts = blocked_tree.texts();
        assert!(texts.iter().any(|text| text.contains("退回正常档")), "{texts:?}");
        assert!(texts.iter().any(|text| text.contains("不支持")), "{texts:?}");

        // 正常档且没有限制时不该冒出一条奇怪的括号说明。
        let plain = Snapshot::default();
        let plain_tree = render(&plain);
        let texts = plain_tree.texts();
        assert!(!texts.iter().any(|text| text.contains("退回正常档")), "{texts:?}");
    }
}
