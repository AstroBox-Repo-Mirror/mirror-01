//! 日志页：把插件的 tracing 输出搬进界面，排障时直接截图。

use super::super::actions;
use super::super::glass::{empty, quiet_button, section, segmented};
use super::super::node::{Node, Tag, badge, label};
use super::super::snapshot::{LogFilter, LogLine, Snapshot};
use super::super::theme::*;

const LOG_MAX_HEIGHT: u32 = 460;

pub fn render(snapshot: &Snapshot) -> Node {
    let lines = snapshot.filtered_logs();
    let hint = if snapshot.log_errors > 0 {
        format!("{} 行 · {} 个错误", lines.len(), snapshot.log_errors)
    } else {
        format!("{} 行", lines.len())
    };

    let mut list = Node::new(Tag::Scroll).full().scroll("y").maxh(LOG_MAX_HEIGHT).column().gap(4);
    if lines.is_empty() {
        list = list.child(empty(if snapshot.logs.is_empty() {
            "还没有日志"
        } else {
            "当前筛选下没有日志"
        }));
    } else {
        for line in &lines {
            list = list.child(row(line));
        }
    }

    section("运行日志", Some(hint))
        .child(controls(snapshot))
        .child(list)
        .child(label(
            "只留最近一段。出问题时把这一页截图发给作者，比复述状态行管用。",
            SIZE_TINY,
            TEXT_DIM,
        ))
}

/// 级别筛选 + 清空。
fn controls(snapshot: &Snapshot) -> Node {
    let items: Vec<(String, String, bool)> = LogFilter::ALL
        .into_iter()
        .map(|filter| {
            let count = match filter {
                LogFilter::All => snapshot.logs.len(),
                LogFilter::Warn => snapshot.log_warns,
                LogFilter::Error => snapshot.log_errors,
            };
            let text = if count > 0 {
                format!("{} {count}", filter.label())
            } else {
                filter.label().to_string()
            };
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
fn row(line: &LogLine) -> Node {
    Node::new(Tag::Div)
        .full()
        .row()
        .align("start")
        .gap(GAP_SM)
        .pad(8)
        .radius(10)
        .bg(SURFACE_SOFT)
        .child(badge(line.level.label(), line.level.color(), "rgba(255,255,255,0.07)"))
        .child(
            label(&line.text, SIZE_TINY, line.level.color())
                .grow(1.0)
                .prop("word-break", "break-word"),
        )
}
