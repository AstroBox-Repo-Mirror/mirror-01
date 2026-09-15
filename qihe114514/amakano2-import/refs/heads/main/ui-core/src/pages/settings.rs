//! 设置页：传输参数、行为开关、缓存、插件信息。
//!
//! 四块都是**摆事实**：参数是协议里定死的值，缓存是当前占用的字节数，
//! 关于里是版本与容量。这里没有可点的「优化」按钮 —— 能自动的都自动了。
//!
//! 临时自检卡（渲染自检、存档功能自检）**都已删除**，结论落在
//! `docs/插件开发注意事项.md`，别再往这一页加实验装置。

use super::super::actions;
use super::super::glass::{ghost_button, kv_grid, note, section, segmented};
use super::super::node::{Node, Tag, label};
use super::super::snapshot::{chunk_label, Snapshot, CHUNK_OPTIONS};
use super::super::theme::*;
use super::super::human_bytes;

pub fn render(snapshot: &Snapshot) -> Node {
    Node::new(Tag::Div)
        .full()
        .column()
        .gap(GAP_LG)
        .child(chunk_settings(snapshot))
        .child(behavior(snapshot))
        .child(cache(snapshot))
        .child(about(snapshot))
}

/// 传输分片与协议参数。**分片档位只有这一个入口**（原先传输页里还有一份，已删）。
fn chunk_settings(snapshot: &Snapshot) -> Node {
    let busy = snapshot.is_transferring();
    let items: Vec<(String, String, bool)> = CHUNK_OPTIONS
        .iter()
        .map(|bytes| (chunk_label(*bytes), actions::chunk_id(*bytes), *bytes == snapshot.chunk_bytes))
        .collect();
    let limits = &snapshot.limits;

    // 两栏并排时每格只有 ~170px：**值一定要短**，长了就会折成两行把小字挤乱。
    // 超时是**按请求种类**定的（存档比章节列表宽，理由见 `src/request.rs`），
    // 所以拆成两格各自报一个数字，别在一格里写括号说明。
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
        .child(note("断点续传靠它。清掉之后，没传完的章节要从头再传。"))
        .child(ghost_button("清理未完成缓存", actions::CLEAR_CACHE, snapshot.cache_files > 0, snapshot))
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
        .child(note("章节包随插件一起装好，不看本机里的游戏文件。"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::Page;

    #[test]
    fn settings_page_has_no_experiment_probes_left() {
        // 本轮把最后一块临时自检（「存档功能自检（临时）」）整块删除：
        // 结论已经落进 docs/插件开发注意事项.md 第 7 节，实验装置不留常驻。
        // 谁再往设置页加自检卡，这条会红。
        let mut snapshot = Snapshot::default();
        snapshot.page = Page::Settings;
        let tree = render(&snapshot);
        let texts = tree.texts();

        assert!(!texts.iter().any(|text| text.contains("自检")), "设置页不许再有自检卡：{texts:?}");
        assert!(!texts.iter().any(|text| text.contains("临时")), "设置页不许再有临时装置：{texts:?}");
        for action in ["save-probe-dialog", "save-probe-fs"] {
            assert!(
                tree.find(|node| node.get("on.click") == Some(action)).is_empty(),
                "{action} 已经删除，不该还有按钮"
            );
        }
        // 设置页该有的四块还在（别把整页一起删掉了）。
        for text in ["传输分片", "行为", "未完成缓存", "关于"] {
            assert!(texts.contains(&text), "设置页缺少「{text}」：{texts:?}");
        }
    }

    /// 分片档位**只有设置页这一个入口**：传输页撤掉之后，改档位不该再需要跳页。
    /// 同时把协议参数（超时/重试/窗口）也钉在这里 —— 它们原本跟着分片卡走，
    /// 别在改写时被顺手丢掉。
    #[test]
    fn chunk_size_is_settable_only_here_and_protocol_limits_stay_visible() {
        let mut snapshot = Snapshot::default();
        snapshot.page = Page::Settings;
        let tree = render(&snapshot);
        let texts = tree.texts();

        for option in CHUNK_OPTIONS {
            let id = actions::chunk_id(option);
            assert_eq!(
                tree.find(|node| node.get("on.click") == Some(id.as_str())).len(),
                1,
                "{id} 应该恰好有一个入口"
            );
        }
        // 协议的几个数字还在页面上（它们是排障时要报的值）。
        for value in ["1000 ms", "1500 ms", "12 次"] {
            assert!(texts.iter().any(|text| text.contains(value)), "缺参数 {value}：{texts:?}");
        }
    }
}
