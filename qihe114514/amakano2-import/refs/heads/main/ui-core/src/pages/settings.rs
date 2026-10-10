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
use super::super::glass::{empty, ghost_button, kv_grid, note, quiet_button, section, segmented};
use super::super::human_bytes;
use super::super::node::{Node, Tag, badge, label};
use super::super::snapshot::{chunk_label, LogFilter, LogLine, Snapshot, CHUNK_OPTIONS};
use super::super::theme::*;

const LOG_MAX_HEIGHT: u32 = 360;

pub fn render(snapshot: &Snapshot) -> Node {
    Node::new(Tag::Div)
        .full()
        .column()
        .gap(GAP_LG)
        .child(chunk_settings(snapshot))
        .child(behavior(snapshot))
        .child(cache(snapshot))
        .child(device(snapshot))
        .child(logs(snapshot))
        .child(about(snapshot))
}

/// 传输分片与协议参数。**分片档位只有这一个入口**。
fn chunk_settings(snapshot: &Snapshot) -> Node {
    let busy = snapshot.is_transferring();
    let items: Vec<(String, String, bool)> = CHUNK_OPTIONS
        .iter()
        .map(|bytes| (chunk_label(*bytes), actions::chunk_id(*bytes), *bytes == snapshot.chunk_bytes))
        .collect();
    let limits = &snapshot.limits;

    // 两栏并排时每格只有 ~170px：**值一定要短**，长了就会折成两行把小字挤乱。
    let grid = kv_grid(
        &[
            ("请求超时", format!("{} ms", limits.request_timeout_ms)),
            ("存档超时", format!("{} ms", limits.saves_timeout_ms)),
            ("退避重试", format!("{} 次", limits.max_retries)),
            ("重试间隔", format!("{} ms", limits.retry_delay_ms)),
            ("初始窗口", limits.window_label()),
            ("应用探测", limits.probe_label()),
            ("单章上限", human_bytes(limits.max_pack_bytes)),
        ],
        2,
    );

    section("传输分片", Some(if busy { "传输中改不了".into() } else { "越大越快".into() }))
        .child(segmented(&items, !busy, snapshot))
        .child(grid)
}

/// 行为开关。
fn behavior(snapshot: &Snapshot) -> Node {
    let items = vec![
        ("开启".to_string(), actions::AUTO_ON.to_string(), snapshot.auto_launch),
        ("关闭".to_string(), actions::AUTO_OFF.to_string(), !snapshot.auto_launch),
    ];
    let row = Node::new(Tag::Div)
        .full()
        .column()
        .gap(GAP_SM)
        .child(label("连上后自动打开《甜蜜女友2》", SIZE_BODY, TEXT_MAIN).weight(500))
        .child(segmented(&items, true, snapshot));

    section("行为", None).child(row)
}

/// 缓存。
fn cache(snapshot: &Snapshot) -> Node {
    let hint = format!("{} · {} 个文件", human_bytes(snapshot.cache_bytes), snapshot.cache_files);
    section("未完成缓存", Some(hint))
        .child(note("断点续传要用到它；清掉之后，没传完的章节只能从头再传。"))
        .child(ghost_button("清理未完成缓存", actions::CLEAR_CACHE, snapshot.cache_files > 0, snapshot))
}

/// 设备详情。**只摆事实，不放连接 / 打开游戏按钮**（那在任务条的状态区）。
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
    let grid = kv_grid(
        &[("设备", name), ("地址", addr), ("手环端版本", band), ("存档协议", protocol)],
        2,
    );

    section("设备与连接", None)
        .child(grid)
        .child(note("连接 / 重新连接 / 打开游戏都在顶部的任务条上。"))
}

/// 运行日志（原「日志」页）。
fn logs(snapshot: &Snapshot) -> Node {
    let lines = snapshot.filtered_logs();
    let hint = if snapshot.log_errors > 0 {
        format!("{} 行 · {} 个错误", lines.len(), snapshot.log_errors)
    } else {
        format!("{} 行", lines.len())
    };

    let mut list = Node::new(Tag::Scroll).full().scroll("y").maxh(LOG_MAX_HEIGHT).column().gap(4);
    if lines.is_empty() {
        list = list.child(empty(if snapshot.logs.is_empty() { "还没有日志" } else { "当前筛选下没有日志" }));
    } else {
        for line in &lines {
            list = list.child(log_row(line));
        }
    }

    section("运行日志", Some(hint))
        .child(log_controls(snapshot))
        .child(list)
        .child(label("只留最近一段。出问题时把这一页截图发给作者，比复述状态行管用。", SIZE_TINY, TEXT_DIM))
}

/// 级别筛选 + 清空。
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
        .justify("between")
        .gap(GAP)
        .child(segmented(&items, true, snapshot))
        .child(quiet_button("清空", actions::LOG_CLEAR, !snapshot.logs.is_empty(), snapshot))
}

/// 一行日志：级别标签 + 原文。
fn log_row(line: &LogLine) -> Node {
    Node::new(Tag::Div)
        .full()
        .row()
        .align("start")
        .gap(GAP_SM)
        .pad(8)
        .radius(10)
        .bg(SURFACE_SOFT)
        .child(badge(line.level.label(), line.level.color(), "rgba(255,255,255,0.07)"))
        .child(label(&line.text, SIZE_TINY, line.level.color()).grow(1.0).prop("word-break", "break-word"))
}

/// 关于。
fn about(snapshot: &Snapshot) -> Node {
    let version = if snapshot.version.is_empty() {
        "未知".to_string()
    } else {
        format!("v{}", snapshot.version)
    };
    let grid = kv_grid(
        &[
            ("插件版本", version),
            ("内置章节", format!("{} 章", snapshot.library.len())),
            ("已装到手环", format!("{} 章", snapshot.installed_count())),
            ("章节库容量", human_bytes(snapshot.library_bytes())),
        ],
        2,
    );

    section("关于", None)
        .child(grid)
        .child(note("章节包已经随插件装好，不需要本机里的游戏文件。"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::Page;

    /// 设置页该有的块都在（别在改写时把整块删掉），也不许有自检装置。
    #[test]
    fn settings_has_all_sections_and_no_experiment_probes() {
        let mut snapshot = Snapshot::default();
        snapshot.page = Page::Settings;
        let tree = render(&snapshot);
        let texts = tree.texts();
        for text in ["传输分片", "行为", "未完成缓存", "设备与连接", "运行日志", "关于"] {
            assert!(texts.contains(&text), "设置页缺少「{text}」：{texts:?}");
        }
        assert!(!texts.iter().any(|text| text.contains("自检")), "{texts:?}");
        assert!(!texts.iter().any(|text| text.contains("临时")), "{texts:?}");
        // 连接 / 打开游戏不在这一页（单一出口）。
        assert!(tree.find(|node| node.get("on.click") == Some("connect")).is_empty());
        assert!(tree.find(|node| node.get("on.click") == Some("launch")).is_empty());
    }

    /// 分片档位只有设置页这一个入口；协议参数也钉在这里。
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
        for value in ["1000 ms", "1500 ms", "12 次"] {
            assert!(texts.iter().any(|text| text.contains(value)), "缺参数 {value}：{texts:?}");
        }
    }
}
