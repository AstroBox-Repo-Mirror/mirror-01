//! 插件主面板的**引导页**：默认界面，点一下打开 HTML 控制面板。
//!
//! 为什么主面板不是完整界面：HTML 控制面板是独立窗口，排版与交互都在 WebView 里，
//! 改一次样式不用重编 wasm。而宿主 `browser.open` **每次调用都要授权**，
//! 所以窗口不能自动弹 —— 必须由用户明确点一下（否则每次启用插件都冒一个授权框，
//! 用户慢一拍点「拒绝」，日志里就是一条「打开失败」，看着像插件坏了）。
//!
//! 但有些设备**打不开 WebView**（授权被拒、WebView 版本不兼容、或白屏）。
//! 所以底栏保留一个不起眼的后备入口「改用原生界面」—— 那条路走宿主自己的
//! `ui-v3` 元素树，不依赖 WebView。原生界面里也有「返回」，两边可以来回切。

use super::super::actions;
use super::super::glass::{
    ghost_button, group, group_heading, hero_button, row_detail, state_badge,
};
use super::super::node::{Node, Tag, label};
use super::super::snapshot::{Snapshot, StatusKind};
use super::super::theme::*;

pub fn render(snapshot: &Snapshot) -> Node {
    Node::new(Tag::Div)
        .full()
        .column()
        .child(head(snapshot))
        .child(open_button(snapshot))
        .child(connection_row(snapshot))
        .child(fallback_section(snapshot))
}

/// 页头：产品名 + 一句话说明「要点一下」。
///
/// 文案必须写「点下面的按钮打开」，**不能写「已经打开了」** ——
/// 窗口要等用户点过、并且点允许之后才出现。
fn head(snapshot: &Snapshot) -> Node {
    let version = if snapshot.version.is_empty() {
        String::new()
    } else {
        format!(" · v{}", snapshot.version)
    };
    Node::new(Tag::Div)
        .full()
        .column()
        .gap(GAP_SM)
        .pad_t(18)
        .pad_b(4)
        .child(label(format!("甜蜜女友2 导入器{version}"), TYPE_TITLE, TEXT_MAIN))
        .child(label(
            "章节同步、存档、统计与账号都在控制面板里操作。",
            TYPE_BODY,
            TEXT_SUB,
        ))
}

/// 主操作：打开 HTML 控制面板。
///
/// 用 [`hero_button`]（56 高 / 17px 字）而不是常规主按钮：
/// 它是整只插件唯一的入口，得在插件面板里第一眼就找到。
fn open_button(snapshot: &Snapshot) -> Node {
    Node::new(Tag::Div)
        .full()
        .pad_t(20)
        .pad_b(4)
        .child(hero_button("打开控制面板", actions::OPEN_PANEL, true, snapshot))
        .child(
            Node::new(Tag::Div)
                .full()
                .pad_t(GAP)
                .pad_x(2)
                .child(label(
                    "宿主会请求一次 browser 权限，允许后窗口才会出现。",
                    TYPE_META,
                    TEXT_DIM,
                )),
        )
}

/// 连接状态：一行事实，不抢主操作的位置。
fn connection_row(snapshot: &Snapshot) -> Node {
    let (text, kind) = if snapshot.device.connected && snapshot.device.alive {
        ("手环已连接", StatusKind::Good)
    } else if snapshot.device.connected {
        ("手环已连接，等待《甜蜜女友2》回应", StatusKind::Warn)
    } else {
        ("手环未连接", StatusKind::Info)
    };
    let name = snapshot.device.name.clone();
    group().child(
        Node::new(Tag::Div)
            .full()
            .row()
            .align("center")
            .gap(GAP)
            .pad_y(14)
            .child(state_badge(text, kind))
            .child(Node::new(Tag::Div).grow(1.0))
            .child_if(
                !name.is_empty(),
                label(name, TYPE_META, TEXT_DIM).shrink(1.0),
            ),
    )
}

/// 后备入口：WebView 打不开时改走原生界面。
///
/// 刻意做得**不起眼**（一行说明 + 一个次要按钮）：
/// 主路径是 HTML 面板，这个只是备用。但也不能藏起来 ——
/// 打不开 WebView 的用户如果找不到它，插件对他们就是完全不可用的。
fn fallback_section(snapshot: &Snapshot) -> Node {
    group_heading("打不开面板？").full().child(
        group().child(
            Node::new(Tag::Div)
                .full()
                .column()
                .gap(GAP_SM)
                .pad_y(GROUP_ROW_PAD_Y)
                .child(row_detail(
                    "有些设备打不开内嵌网页（授权被拒或 WebView 不兼容）。可以改用插件自己的原生界面，功能一样。",
                ))
                .child(ghost_button("改用原生界面", actions::USE_NATIVE, true, snapshot)),
        ),
    )
}

/// 原生界面的**顶栏返回入口**：切回引导页。
///
/// 这个按钮挂在原生界面的外壳里（见 `glass::shell`），不在某一页里 ——
/// 用户在任何一页都能一键回到「打开 HTML 面板」那条路。
///
/// 不收 `snapshot`：返回按钮没有任何状态（不像别的按钮要按下态换底色）——
/// 它是一行文字，按下也没底色可换。少一个参数就少一处误用。
pub fn back_button() -> Node {
    Node::new(Tag::Div)
        .full()
        .row()
        .align("center")
        .gap(GAP_SM)
        .pad_x(PAGE_PAD_X)
        .pad_t(GAP_SM)
        .child(
            Node::new(Tag::Button)
                .bg(TRANSPARENT)
                .without_default_styles()
                .pad_x(4)
                .pad_y(4)
                .shrink(0.0)
                .click(actions::BACK_TO_LANDING)
                .press(&actions::press_id(actions::BACK_TO_LANDING))
                .child(label("‹ 返回", TYPE_BODY, TEXT_SUB)),
        )
        .child(
            label("原生界面（后备）", TYPE_BADGE, TEXT_DIM)
                .grow(1.0)
                .shrink(1.0),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::Page;

    fn base() -> Snapshot {
        let mut snapshot = Snapshot::default();
        snapshot.page = Page::Push;
        snapshot
    }

    /// 主面板必须有一个「打开控制面板」的主按钮，动作是 `open-panel`。
    #[test]
    fn landing_offers_the_html_panel_as_the_main_action() {
        let tree = render(&base());
        let opens = tree.find(|node| node.get("on.click") == Some(actions::OPEN_PANEL));
        assert_eq!(opens.len(), 1, "打开控制面板的按钮恰好一个");
        let texts = tree.texts();
        assert!(texts.contains(&"打开控制面板"), "{texts:?}");
        // 文案必须说清「要自己点一下」。
        assert!(
            texts.iter().any(|text| text.contains("允许后窗口才会出现")),
            "要说明宿主会请求授权：{texts:?}"
        );
    }

    /// 主入口按钮要比常规主按钮**明显大一圈**。
    ///
    /// 它是整只插件唯一的入口 —— 用户在插件面板里第一眼找不到它，等于插件不可用。
    /// 这里钉死几何，顺便守住「没有把通用按钮基准调大」（那会挤坏原生界面所有页）。
    #[test]
    fn the_open_panel_button_is_a_large_hero_button() {
        let tree = render(&base());
        let button = tree
            .find(|node| node.get("on.click") == Some(actions::OPEN_PANEL))
            .pop()
            .expect("要有打开控制面板的按钮");

        assert_eq!(button.get("h"), Some(HERO_HEIGHT.to_string().as_str()), "高度要钉死");
        assert_eq!(button.get("size"), Some(HERO_FONT.to_string().as_str()), "字号要大一号");
        assert_eq!(button.get("w"), Some("full"), "要占满整行，否则在空白页上像普通按钮");
        // `pad_y()` 拆成上下两个键下发，不是 `padding`。
        assert_eq!(button.get("pt"), Some(HERO_PAD_Y.to_string().as_str()), "上内边距");
        assert_eq!(button.get("pb"), Some(HERO_PAD_Y.to_string().as_str()), "下内边距");
        assert_eq!(button.get("pl"), Some(HERO_PAD_X.to_string().as_str()), "左右内边距");

        // 比常规主按钮大：这是「做大」的可测定义。
        assert!(HERO_HEIGHT > BUTTON_HEIGHT, "主入口要比常规按钮高");
        assert!(HERO_FONT > BUTTON_FONT, "主入口字号要比常规按钮大");

        // 外观规则不变：仍是实色、无模糊、无渐变、无子元素。
        assert_eq!(button.get("bg"), Some(BUTTON_PRIMARY));
        assert!(!button.has("backdrop") && !button.has("background"));
        assert!(!button.has("shadow") && !button.has("inset"));
        assert!(button.children.is_empty(), "按钮内部不该有子元素");
    }

    /// 「做大」**不许**通过调大通用 `BUTTON_*` 基准实现。
    ///
    /// 那几个值是全界面的按钮基准，改大会让原生界面里所有页的按钮一起变胖
    /// —— 设置页的行内按钮、存档行的操作区都会被挤。
    #[test]
    fn the_shared_button_baseline_is_untouched() {
        assert_eq!(BUTTON_HEIGHT, 48, "常规主按钮仍是 48 高");
        assert_eq!(BUTTON_FONT, 15, "常规主按钮仍是 15px 字");
        // 常规主按钮不受影响。
        let plain = super::super::glass::primary_button("打开控制面板", actions::OPEN_PANEL, true, &base());
        assert_eq!(plain.get("h"), None, "常规主按钮不额外钉高度");
        assert_eq!(plain.get("size"), Some(BUTTON_FONT.to_string().as_str()));
    }

    /// 后备入口必须在，且不能被做成主按钮（它只是备用）。
    #[test]
    fn landing_keeps_a_quiet_native_fallback() {
        let tree = render(&base());
        let natives = tree.find(|node| node.get("on.click") == Some(actions::USE_NATIVE));
        assert_eq!(natives.len(), 1, "要用恰好一个后备入口");
        let texts = tree.texts();
        assert!(texts.contains(&"改用原生界面"), "{texts:?}");
        // 后备入口说的是「打不开面板」这个前提，不能写成等价的主路径。
        assert!(
            texts.iter().any(|text| text.contains("有些设备打不开内嵌网页")),
            "要说明这是给打不开 WebView 的设备用的：{texts:?}"
        );
    }

    /// 引导页**不画**完整界面：它只负责开门。
    ///
    /// 守的是「主面板默认不要变成完整界面」这条产品决定 ——
    /// 完整界面是后备通道，不该在引导页里出现同步/存档这些动作。
    #[test]
    fn landing_is_not_the_full_panel() {
        let tree = render(&base());
        for action in [actions::SYNC_ALL, actions::SAVES_EXPORT, actions::RESUME] {
            assert!(
                tree.find(|node| node.get("on.click") == Some(action)).is_empty(),
                "引导页不该出现 {action} 这类业务动作"
            );
        }
    }

    /// 原生界面里必须有「返回」，否则用户切过去就回不到 HTML 面板那条路。
    #[test]
    fn native_ui_can_go_back_to_the_landing() {
        let back = back_button();
        let clickable = back.find(|node| node.get("on.click") == Some(actions::BACK_TO_LANDING));
        assert_eq!(clickable.len(), 1, "返回按钮要挂 back-to-landing");
        assert!(back.texts().iter().any(|text| text.contains("返回")), "{:?}", back.texts());
    }
}
