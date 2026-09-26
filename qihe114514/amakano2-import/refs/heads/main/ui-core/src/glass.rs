//! 组件库：面板、区块、按钮、徽章、进度条这些页面零件的构造器。
//!
//! 界面只有**三级实色面**（`SURFACE` / `SURFACE_SOFT` / `SURFACE_STRONG`）和
//! **四级文字**（`TEXT_MAIN` … `TEXT_FAINT`），页面只调用这里的构造函数，
//! 不自己写颜色和圆角 —— 改一处就能全局生效，也不会出现「五种按钮五套内边距」。
//!
//! 会**换底色**的容器才挂 `transition`；真机其实补不出跨帧过渡（见 theme.rs），
//! 挂着只是让预览里的悬停好看一点，界面状态一律靠**换实色**表达。

use super::node::{Node, Tag, badge, label};
use super::errors::ErrorView;
use super::snapshot::{Page, SessionStage, Snapshot, StatusKind};
use super::theme::*;

/// 加透明度：只在 `#RRGGBB` 上用（`rgba(...)` 字符串会失效，别混用）。
pub fn alpha(color: &str, alpha: &str) -> String {
    format!("{color}{alpha}")
}

/// 悬停用的元素 id。和点击 id 分开命名空间，避免同一个 id 既是动作又是悬停。
pub fn hover_key(id: &str) -> String {
    super::actions::hover_id(id)
}

pub fn hovered(snapshot: &Snapshot, id: &str) -> bool {
    let key = hover_key(id);
    snapshot.hover.as_deref() == Some(key.as_str())
}

// ---------------------------------------------------------------- 文字零件

/// 主标题（页面主标题、卡片主标题）。
pub fn title(text: &str) -> Node {
    label(text, SIZE_TITLE, TEXT_MAIN).weight(650)
}

/// 元信息：一行里跟在主体后面的小字（`54 分钟 · 736 KB`）。
pub fn meta(text: impl Into<String>) -> Node {
    label(text, SIZE_TINY, TEXT_DIM)
}

/// 说明文字：需要读一句才懂的那种（帮助、提示）。
pub fn note(text: impl Into<String>) -> Node {
    label(text, SIZE_TINY, TEXT_DIM)
}

/// 区块标签：卡片内部的**小标题**。
///
/// 刻意压到 12px + `TEXT_SUB`：区块标签是**指路牌**，不是内容 ——
/// 做得跟主标题一样亮，界面上就有两处都在喊「看我」。
pub fn group_title(text: &str) -> Node {
    label(text, SIZE_SMALL, TEXT_SUB).weight(600).ls("0.02em")
}

// ---------------------------------------------------------------- 容器

/// 一级面板：页面卡片、区块卡片。实色 + 一层极淡描边（在彩色壁纸上划边界）。
pub fn panel(radius: u32) -> Node {
    Node::new(Tag::Div)
        .full()
        .column()
        .radius(radius)
        .bg(SURFACE)
        .border(1, STROKE)
        .transition(TRANSITION)
}

/// 二级块：卡片内嵌套的小块 / 列表行。
///
/// **不描边**：它跟一级面板的边界靠底色深浅（`SURFACE` → `SURFACE_SOFT`）区分，
/// 再画一圈线就成了「盒子里套盒子」，页面会显脏。
pub fn nested(radius: u32) -> Node {
    Node::new(Tag::Div)
        .full()
        .column()
        .radius(radius)
        .bg(SURFACE_SOFT)
        .transition(TRANSITION)
}

/// 带区块标签的卡片。`hint` 是标签右侧的小字说明。
///
/// 刻意**不画分隔线**：标签与内容之间的间距已经足够分组，
/// 每个卡片都来一条横线只会把页面切碎。
pub fn section(title: &str, hint: Option<String>) -> Node {
    let mut head = Node::new(Tag::Div).full().row().align("center").justify("between").gap(GAP_SM);
    head = head.child(group_title(title));
    if let Some(hint) = hint {
        head = head.child(meta(hint));
    }
    panel(CARD_RADIUS).pad(CARD_PAD).gap(GAP).child(head)
}

/// 带区块标签 + 右侧小动作按钮的卡片（「刷新」这类跟着列表走的小操作）。
pub fn section_with_action(title: &str, hint: Option<String>, action: Node) -> Node {
    let mut head = Node::new(Tag::Div).full().row().align("center").justify("between").gap(GAP_SM);
    head = head.child(group_title(title));
    head = head.child(action);
    if let Some(hint) = hint {
        head = head.child(meta(hint));
    }
    panel(CARD_RADIUS).pad(CARD_PAD).gap(GAP).child(head)
}

/// 空状态提示。
pub fn empty(text: &str) -> Node {
    nested(ROW_RADIUS).pad(14).align("center").child(label(text, SIZE_SMALL, TEXT_DIM))
}

/// 两栏键值行。
///
/// 键**固定宽度不许收缩**：4 个字的键（「请求超时」）在窄窗的两栏网格里只差几个像素
/// 就会被压成「请求超 / 时」两行，看着像排版坏了。值那一侧才是允许变窄的。
pub fn kv(key: &str, value: &str) -> Node {
    Node::new(Tag::Div)
        .full()
        .row()
        .align("center")
        .justify("between")
        .gap(GAP_SM)
        .child(label(key, SIZE_SMALL, TEXT_DIM).shrink(0.0))
        .child(label(value, SIZE_SMALL, TEXT_SUB).grow(1.0))
}

/// 键值网格：每行 `per_row` 个，等宽分配。
///
/// **刻意不用宿主的 `GRID` 元素**：真机截图证实宿主不认 `grid-template-columns`，
/// `GRID` 会退化成「每格一行」。改用 flex + `grow` 等分才稳。
pub fn kv_grid(pairs: &[(&str, String)], per_row: usize) -> Node {
    let per_row = per_row.max(1);
    let mut grid = Node::new(Tag::Div).full().column().gap(GAP_SM);
    for chunk in pairs.chunks(per_row) {
        let mut row = Node::new(Tag::Div).full().row().gap(GAP_SM);
        for (key, value) in chunk {
            row = row.child(kv(key, value).grow(1.0));
        }
        grid = grid.child(row);
    }
    grid
}

/// 等宽并排的一行砖块（统计格用）。
pub fn tile_row(tiles: Vec<Node>) -> Node {
    let mut row = Node::new(Tag::Div).full().row().gap(GAP_SM);
    for tile in tiles {
        row = row.child(tile.grow(1.0));
    }
    row
}

// ---------------------------------------------------------------- 交互

/// 按钮种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ButtonKind {
    /// 主操作：品牌粉实底 + 白字。**一页最多一个**。
    Primary,
    /// 次要操作：石墨实底 + 浅字。
    Ghost,
    /// 行内强调（小号主按钮）。
    Chip,
    /// 行内中性：静止时只有文字，按下才给底色。
    Quiet,
    /// 危险操作（删除类）：红实底 + 白字。
    Danger,
}

impl ButtonKind {
    /// 是不是行内小按钮（按密集列表行的密度压到 30 高）。
    const fn inline(self) -> bool {
        matches!(self, Self::Chip | Self::Quiet | Self::Danger)
    }
}

/// 一个按钮的外观：文字色 + 实色底（按下时换成另一支）。
#[derive(Clone, Copy, PartialEq, Debug)]
struct Skin {
    fg: &'static str,
    bg: &'static str,
}

/// 按 `kind` 选配色，`pressed` 决定用静止档还是按下档。
/// 悬停（`_on`）不改变按钮外观 —— 这一版按钮唯一的反馈就是按下换底色。
fn skin(kind: ButtonKind, pressed: bool, _on: bool) -> Skin {
    match kind {
        ButtonKind::Primary | ButtonKind::Chip => Skin {
            fg: BUTTON_PRIMARY_TEXT,
            bg: if pressed { BUTTON_PRIMARY_PRESSED } else { BUTTON_PRIMARY },
        },
        ButtonKind::Ghost => Skin {
            fg: BUTTON_GHOST_TEXT,
            bg: if pressed { BUTTON_GHOST_PRESSED } else { BUTTON_GHOST },
        },
        // 行内中性：静止时就是一行亮字，按下给一点底色作为反馈。
        ButtonKind::Quiet => Skin {
            fg: TEXT_SUB,
            bg: if pressed { BUTTON_GHOST_PRESSED } else { TRANSPARENT },
        },
        ButtonKind::Danger => Skin {
            fg: BUTTON_DANGER_TEXT,
            bg: if pressed { BUTTON_DANGER_PRESSED } else { BUTTON_DANGER },
        },
    }
}

/// 按钮外壳：几何与配色只有这一份实现。
///
/// 就是最普通的那种按钮：一块实色圆角胶囊 + 居中文字；按下时底色换一支。
/// 没有模糊、没有渐变、没有描边、没有阴影、没有缩放动效，内部也没有子元素 ——
/// 试过做玻璃按钮，这个宿主给不了（`prop()` 逃生舱对 `background` / `box-shadow`
/// 全是空操作，见 docs/插件开发注意事项.md 6.2），与其做个不像的，不如退回最朴素的形态。
pub fn button(kind: ButtonKind, text: &str, action: &str, enabled: bool, snapshot: &Snapshot) -> Node {
    let inline = kind.inline();
    let pressed = enabled && snapshot.is_pressed(action);
    let look = skin(kind, pressed, false);

    let node = Node::text(Tag::Button, text)
        .size(if inline { CHIP_FONT } else { BUTTON_FONT })
        .fg(look.fg)
        .pad_x(if inline { CHIP_PAD_X } else { BUTTON_PAD_X })
        .pad_y(if inline { CHIP_PAD_Y } else { BUTTON_PAD_Y })
        .radius(if inline { CHIP_RADIUS } else { BUTTON_RADIUS })
        // flex 子项默认可收缩：不显式禁掉的话，列表行/导航里的按钮会被压窄、文字折成两行。
        .shrink(0.0)
        .bg(look.bg);

    if !enabled {
        return node.disabled().opacity(0.42);
    }
    node.click(action).press(&super::actions::press_id(action)).hover(&hover_key(action))
}

/// 主操作按钮。
pub fn primary_button(text: &str, action: &str, enabled: bool, snapshot: &Snapshot) -> Node {
    button(ButtonKind::Primary, text, action, enabled, snapshot)
}

/// 次要按钮。
pub fn ghost_button(text: &str, action: &str, enabled: bool, snapshot: &Snapshot) -> Node {
    button(ButtonKind::Ghost, text, action, enabled, snapshot)
}

/// 行内小号强调按钮：列表行里的「同步」用这个。
pub fn accent_chip(text: &str, action: &str, enabled: bool, snapshot: &Snapshot) -> Node {
    button(ButtonKind::Chip, text, action, enabled, snapshot)
}

/// 纯文字按钮，用于行内轻量操作。
pub fn quiet_button(text: &str, action: &str, enabled: bool, snapshot: &Snapshot) -> Node {
    button(ButtonKind::Quiet, text, action, enabled, snapshot)
}

/// 危险操作按钮（删除类）。
pub fn danger_button(text: &str, action: &str, enabled: bool, snapshot: &Snapshot) -> Node {
    button(ButtonKind::Danger, text, action, enabled, snapshot)
}

/// 分段控件（互斥选择）。`items` 是（显示文字, 动作 id, 是否选中）。
pub fn segmented(items: &[(String, String, bool)], enabled: bool, snapshot: &Snapshot) -> Node {
    segmented_sized(items, enabled, snapshot, 10, 6)
}

/// 分段控件，可指定每一项的水平内边距与纵向内边距。
///
/// 导航条用它把内边距压小：**标签要在 460px 里整条放下，内边距一大，
/// 单项就装不下两个字、文字会折成两行**（真机窄窗与预览截图都出现过）。
fn segmented_sized(
    items: &[(String, String, bool)],
    enabled: bool,
    snapshot: &Snapshot,
    pad_x: u32,
    pad_y: u32,
) -> Node {
    let mut bar = Node::new(Tag::Div)
        .row()
        .gap(4)
        .pad(4)
        .radius(999)
        .bg(SURFACE_SOFT)
        .transition(TRANSITION);
    for (text, action, active) in items {
        let mut item = Node::text(Tag::Button, text.as_str())
            .size(SIZE_TINY)
            .weight(if *active { 600 } else { 500 })
            .pad_x(pad_x)
            .pad_y(pad_y)
            .radius(999)
            // flex 子项默认可收缩：不显式禁掉，窄窗里每一项都会被压窄到「一个字一行」。
            .shrink(0.0)
            .transition(TRANSITION);
        if !enabled {
            // 不可点，但**当前档位必须仍然看得出来**（只压暗，不清空高亮）。
            item = if *active {
                item.disabled().fg(TEXT_DIM).bg(SURFACE_STRONG)
            } else {
                item.disabled().fg(TEXT_FAINT).bg(TRANSPARENT)
            };
        } else if *active {
            // 选中态只有**一层实色**：渐变/内高光/外投影在真机上都是空操作，不再下发。
            item = item
                .fg(TEXT_MAIN)
                .bg(SURFACE_STRONG)
                .click(action)
                .press(&super::actions::press_id(action))
                .hover(&hover_key(action));
        } else {
            let on = hovered(snapshot, action);
            item = item
                .fg(if on { TEXT_MAIN } else { TEXT_DIM })
                .bg(if on { SURFACE_STRONG } else { TRANSPARENT })
                .click(action)
                .press(&super::actions::press_id(action))
                .hover(&hover_key(action));
        }
        bar = bar.child(item);
    }
    bar
}

/// 横向可滚动的分段控件：**窄窗里的防溢出保证**（章节库的线路筛选用它）。
///
/// 分段控件不换行，每一项又固定 `shrink(0)`（不固定就会被压成「一个字一行」），
/// 于是项数一多只剩两条路 —— 溢出被裁，或者滚动。**只能是滚动**：
/// 章节库的 6 个线路标签在 400px 窗口里比可用宽度宽，上一版没包滚动区，
/// 最右边的「番外」被裁掉半个（用户实机截图）。这里给的是两层保险：
/// 外层 `Tag::Scroll` + `scroll("x")`（任何宽度都只滚不裁），
/// 以及把每一项的内边距收到 `SEGMENT_NARROW_PAD_X`（400px 下一屏正好放得下）。
///
/// **为什么收内边距而不是缩短标签**：「共通线 / 千岁线 / 结灯线」是一眼要分清的分类名，
/// 缩成两个字反而难认；内边距是纯装饰，收窄不损失信息。
pub fn scrollable_segmented(
    items: &[(String, String, bool)],
    enabled: bool,
    snapshot: &Snapshot,
) -> Node {
    Node::new(Tag::Scroll)
        .full()
        .row()
        .scroll("x")
        .child(segmented_sized(items, enabled, snapshot, SEGMENT_NARROW_PAD_X, 6))
}

/// 状态胶囊：一个圆点 + 一句话。
///
/// 外面**包一层 row**：胶囊直接挂在纵向卡片上时会被 cross-axis 拉满整行宽
/// （一颗胶囊变成一条横杠，窄窗截图里一眼就看出来）。包一层之后放在行里、列里都对。
pub fn status_pill(kind: StatusKind, text: &str) -> Node {
    let pill = Node::new(Tag::Div)
        .row()
        .align("center")
        .gap(GAP_XS)
        .pad_x(9)
        .pad_y(4)
        .radius(999)
        .bg(kind.bg())
        .shrink(0.0)
        .child(Node::new(Tag::Div).w(6).h(6).radius(999).bg(kind.color()))
        .child(label(text, SIZE_TINY, kind.color()).weight(500));
    Node::new(Tag::Div).row().child(pill)
}

/// 小型状态徽章。
pub fn state_badge(text: &str, kind: StatusKind) -> Node {
    badge(text, kind.color(), kind.bg())
}

// ---------------------------------------------------------------- 品牌与统计

/// 品牌标记：就是插件自己的图标（`icon.png`）。
///
/// 用 `IMAGE` 元素承载 data URI —— 真机自检证明**图片元素能显示**，
/// 而 `prop("background-image", ...)` 是空操作。拿不到图标时才回落到品牌色方块。
pub fn brand_mark(snapshot: &Snapshot, size: u32) -> Node {
    let Some(image) = snapshot.brand.as_deref() else {
        return orb(size);
    };
    Node::new(Tag::Image).content(image).w(size).h(size).shrink(0.0).radius(8)
}

/// 品牌标记的兜底：品牌色方块。
pub fn orb(size: u32) -> Node {
    Node::new(Tag::Div).w(size).h(size).shrink(0.0).radius(size / 3).bg(ACCENT_DEEP)
}

/// 统计块：一个数 + 单位 + 说明，横排三格。
///
/// 数字给 20px 而不是上一版的 24px：三格并排时 24px 会把「18.6 MB」挤到换行，
/// 而这三格是**辅助信息**，不该比页面主标题还大。
pub fn stat_tile(value: &str, unit: &str, caption: &str, color: &str) -> Node {
    let big = Node::new(Tag::Div)
        .row()
        .align("end")
        .gap(3)
        .child(label(value, 20, color).weight(700))
        .child(label(unit, SIZE_TINY, TEXT_DIM).prop("lh", "1.6"));
    nested(ROW_RADIUS)
        .pad(10)
        .gap(2)
        .child(big)
        .child(label(caption, SIZE_TINY, TEXT_DIM))
}

/// 分段进度条（详见 `node::segmented_bar`）。
pub fn progress_bar(percent: u32, segments: u32) -> Node {
    super::node::segmented_bar(percent, segments, TRACK, &[ACCENT_LIGHT.to_string(), ACCENT_DEEP.to_string()])
}

// ---------------------------------------------------------------- 布局骨架

/// 顶栏：品牌 + 名称 + 连接状态，**一行**。
///
/// 上一版这里是「品牌 + 标题 + 副标题 + 状态胶囊」的两行卡片，而副标题
/// （`章节同步 · 插件 v0.8.0`）在「设置 → 关于」里本来就有一份。合成一行之后，
/// 页面顶部让出的空间正好给内容。版本号不再在这里出现。
pub fn top_bar(snapshot: &Snapshot) -> Node {
    let kind = if snapshot.device.connected && snapshot.device.alive {
        StatusKind::Good
    } else if snapshot.device.connected {
        StatusKind::Warn
    } else if !snapshot.library_error.is_empty() {
        StatusKind::Bad
    } else {
        StatusKind::Info
    };

    panel(CARD_RADIUS)
        .pad(12)
        .row()
        .align("center")
        .gap(GAP)
        .child(brand_mark(snapshot, 28))
        .child(label("甜蜜女友2", 16, TEXT_MAIN).weight(650).grow(1.0))
        .child(status_pill(kind, &snapshot.head_line()))
}

/// 导航条：七个页面标签，**包在横向滚动区里**，放得下时整条居中。
///
/// 内边距压到 4/4：导航条的内边距与每一项的内边距是**相加**的，用默认值会让
/// 单项装不下两个字（真机预览截图里就是「概/览、章/节」竖着写）。
/// 每一项固定 `shrink(0)`：**放不下要显式溢出，而不是把标签压成两行**。
///
/// 外层 `Tag::Scroll` + `scroll("x")` + `justify("center")` 是**兜底**：
/// 装得下时整条仍然居中，装不下时用户能滑到最右边那一项 ——
/// 没有滚动区时「溢出」在真机上表现为**最后一项点不到**，用户没有任何补救手段
/// （章节库的线路筛选也是靠同一套两层保险解决的）。
pub fn nav_bar(snapshot: &Snapshot) -> Node {
    let mut items = Vec::new();
    for page in Page::ALL {
        let mut text = page.label().to_string();
        match page {
            // 徽标只在「有事要做」时出现：装好的章节不再挂一个 0。
            Page::Library if snapshot.pending_count() > 0 => {
                text = format!("{} {}", text, snapshot.pending_count());
            }
            Page::Logs if snapshot.error_count() > 0 => {
                text = format!("{} {}", text, snapshot.error_count());
            }
            _ => {}
        }
        items.push((text, page.action(), snapshot.page == page));
    }
    Node::new(Tag::Scroll)
        .full()
        .row()
        .justify("center")
        .scroll("x")
        .child(segmented_sized(&items, true, snapshot, 4, 4).pad(4))
}

/// 底部状态行：一个圆点 + 当前状态。
///
/// 上一版这里还有第二行「页面说明」（`连接状态、同步进度与下一步建议`）——
/// 每页的主标题已经说清这一页是什么，那行只是把标题换个说法再说一遍，已删除。
pub fn footer(snapshot: &Snapshot) -> Node {
    Node::new(Tag::Div)
        .full()
        .row()
        .align("center")
        .gap(GAP_XS)
        .pad_x(4)
        .child(Node::new(Tag::Div).w(6).h(6).shrink(0.0).radius(999).bg(snapshot.status_kind.color()))
        .child(label(&snapshot.status, SIZE_TINY, snapshot.status_kind.color()).grow(1.0))
}

/// 页面外壳：顶栏 + 导航 + 内容 + 底部状态行。
///
/// 三件事都是**用户实机反馈后定下来的**，改之前先读这里：
///
/// 1. **不下发页面底色**（`PAGE_BG` 已删除）：用户要求「不要自绘页面背景，
///    保持空白，用 astrobox 自己的背景就行」。卡片之间透出来的是宿主自己的壁纸/背景，
///    面板与卡片自己的实色底 + 圆角**不动**（那是文字读得清的前提）。
/// 2. **只给上下内边距**：宿主已经有左右安全区，插件再叠一层，内容会窄掉一大块
///    （460px 窗口里少 36px）。
/// 3. **内容直接当子节点**：0.5.0 用「舞台层 + 两帧渲染 + transition」做切页滑动，
///    真机很卡（宿主每次渲染重建子树，两帧之间插不进过渡），已整块撤掉 ——
///    所以外壳里**不许再出现** `transform` / `opacity` / `transition`，单测盯着这一条。
pub fn shell(snapshot: &Snapshot, content: Node) -> Node {
    Node::new(Tag::Div)
        .full()
        .column()
        .gap(GAP_LG)
        .pad_y(PAGE_PAD_Y)
        .child(top_bar(snapshot))
        .child(nav_bar(snapshot))
        .child(content)
        .child(footer(snapshot))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层次只靠实色深浅：一级面板有描边，二级块**没有**描边（盒子套盒子会让页面显脏）。
    /// 两者都不许出现模糊与渐变（用户要求删掉玻璃质感，且宿主本来就渲染不出来）。
    #[test]
    fn surfaces_are_flat_and_the_inner_one_has_no_stroke() {
        let card = panel(CARD_RADIUS);
        assert!(card.has("bg"), "面板必须有实色底（`bg()` 是真机唯一生效的填色手段）");
        assert!(card.has("border"), "一级面板要有极淡描边，才能在彩色壁纸上划出边界");
        assert!(!card.has("backdrop"), "不要再下发背景模糊");
        assert!(!card.has("background"), "不要再下发渐变");
        assert!(!card.has("shadow") && !card.has("inset"), "阴影在真机上是空操作，不要再下发");

        let inner = nested(ROW_RADIUS);
        assert!(inner.has("bg"), "二级块要有比一级面板亮一点的实色底");
        assert!(!inner.has("border"), "二级块不描边：层次由底色深浅表达");
        assert!(!inner.has("backdrop") && !inner.has("background"));
    }

    /// 页面外壳：**通透**的一片，四段内容，没有壁纸层、没有模糊、没有渐变。
    ///
    /// - 「不要自绘页面背景，保持空白」→ 外壳**不许有** `bg`；
    /// - 「去掉自带的左右安全区」→ 左右内边距**必须是 0**，只留上下；
    /// - 「两帧导致动画很卡」→ 外壳**不许有** `transform` / `transition` / `opacity`。
    #[test]
    fn shell_is_a_plain_flat_page() {
        let snapshot = Snapshot::default();
        let content = Node::new(Tag::Div).full().prop("marker", "content");
        let root = shell(&snapshot, content);

        assert!(!root.has("bg"), "页面外壳不许自绘底色：要透出宿主自己的背景");
        assert!(!root.has("background"), "不要渐变");
        assert!(!root.has("backdrop"), "不要模糊");
        assert!(root.find(|node| node.tag == Tag::Image).is_empty(), "没有品牌图标时不该出现图片层");

        assert_eq!(root.get("pl"), None, "不该再下发左内边距（宿主自带左右安全区）");
        assert_eq!(root.get("pr"), None, "不该再下发右内边距（宿主自带左右安全区）");
        assert_eq!(root.get("padding"), None, "四边内边距已拆成上下两档");
        assert_eq!(root.get("pt"), Some("18"), "上下内边距保留：18");
        assert_eq!(root.get("pb"), Some("18"), "上下内边距保留：18");

        assert!(!root.has("transform"), "外壳不许再有切页偏移（两帧 + transition 已移除）");
        assert!(!root.has("transition"), "外壳不许再挂切页过渡");
        assert!(!root.has("opacity"), "外壳不许再切成半透明");

        // 顶栏 + 导航 + 内容（**直接是子节点**，不再包「舞台」层）+ 底部状态行。
        assert_eq!(root.children.len(), 4);
        assert_eq!(root.children[2].get("marker"), Some("content"), "内容必须是外壳的直接子节点");
    }

    /// 顶栏压成一行：品牌方块 + 名称 + 状态胶囊，**不再有版本副标题**。
    #[test]
    fn top_bar_is_one_compact_row() {
        let bar = top_bar(&Snapshot::default());
        let blocks = bar.find(|node| node.get("bg") == Some(ACCENT_DEEP) && node.get("radius") == Some("9"));
        assert_eq!(blocks.len(), 1, "顶栏应该恰好一个品牌色方块（28/3 = 9 圆角）");
        assert_eq!(bar.children.len(), 3, "顶栏就三块：品牌、名称、状态");
        // 没有图片元素（拿不到图标时回落到品牌色块）。
        assert!(bar.find(|node| node.tag == Tag::Image).is_empty());
        // 版本号不在这里出现（它住在「设置 → 关于」）。
        assert!(!bar.texts().iter().any(|text| text.contains("插件 v")), "{:?}", bar.texts());
    }

    /// 底部状态行只有一行：状态。上一版第二行的「页面说明」已删除。
    #[test]
    fn footer_is_a_single_status_line() {
        let mut snapshot = Snapshot::default();
        snapshot.status = "已连接手环".into();
        let bar = footer(&snapshot);
        assert_eq!(bar.texts(), vec!["已连接手环"], "状态行只说状态，不再重复页面说明");
        assert_eq!(bar.get("pb"), None, "两行改一行之后不再需要额外的下内边距");
    }

    #[test]
    fn hover_key_does_not_collide_with_actions() {
        assert_eq!(hover_key("sync:3"), "hover:sync:3");
    }

    #[test]
    fn alpha_appends_hex_channel() {
        assert_eq!(alpha("#5BD08B", "14"), "#5BD08B14");
    }

    #[test]
    fn disabled_buttons_do_not_attach_handlers() {
        let snapshot = Snapshot::default();
        let button = ghost_button("清理", "cache:clear", false, &snapshot);
        assert!(!button.has("on.click"));
        assert!(button.has("disabled"));
        assert_eq!(button.get("opacity"), Some("0.42"), "禁用只变淡，不换配色");
    }

    #[test]
    fn enabled_buttons_attach_both_events() {
        let snapshot = Snapshot::default();
        let button = primary_button("连接设备", "connect", true, &snapshot);
        assert_eq!(button.get("on.click"), Some("connect"));
        assert_eq!(button.get("on.pointerup"), Some("connect"));
    }

    /// 按下态只剩一件事：底色换成 `*_PRESSED` 那一支，且按下事件挂在 PointerDown 上。
    #[test]
    fn pressed_button_swaps_to_the_pressed_fill() {
        let mut snapshot = Snapshot::default();
        snapshot.pressed = Some("connect".into());
        let pressed = primary_button("连接设备", "connect", true, &snapshot);
        let idle = primary_button("连接设备", "connect", true, &Snapshot::default());

        assert_eq!(idle.get("bg"), Some(BUTTON_PRIMARY));
        assert_eq!(pressed.get("bg"), Some(BUTTON_PRIMARY_PRESSED), "按下时底色要换成按下档");
        assert_eq!(pressed.get("fg"), idle.get("fg"), "按下只换底色，文字色不动");
        assert_eq!(pressed.get("on.press"), Some("press:connect"), "按下事件要挂在 PointerDown 上");
        assert!(!pressed.has("transform"));
        assert!(!pressed.has("transition"));
        assert!(pressed.children.is_empty());
    }

    /// 五种按钮都是同一套「普通按钮」形态：实色底 + 无模糊 / 无渐变 / 无子元素。
    #[test]
    fn every_button_kind_is_a_plain_filled_button() {
        let snapshot = Snapshot::default();
        let kinds = [
            ("primary", primary_button("同步", "sync-all", true, &snapshot), BUTTON_PRIMARY),
            ("chip", accent_chip("同步", "sync:1", true, &snapshot), BUTTON_PRIMARY),
            ("ghost", ghost_button("刷新", "refresh-list", true, &snapshot), BUTTON_GHOST),
            ("danger", danger_button("删除", "delete:x", true, &snapshot), BUTTON_DANGER),
        ];
        for (name, node, fill) in kinds {
            assert_eq!(node.get("bg"), Some(fill), "{name} 要有实色底");
            assert!(!node.has("backdrop"), "{name} 不该有背景模糊");
            assert!(!node.has("background"), "{name} 不该有渐变");
            assert!(!node.has("border"), "{name} 不需要亮边，实色底已经看得出边界");
            assert!(!node.has("shadow"), "{name} 不该有外投影");
            assert!(node.children.is_empty(), "{name} 内部不该有子元素");
        }

        // 纯文字按钮是唯一的例外：静止时不填色，按下才给一点底色作为反馈。
        let quiet = quiet_button("取消", "cancel-delete", true, &snapshot);
        assert_eq!(quiet.get("bg"), Some(TRANSPARENT));
        assert_eq!(quiet.get("fg"), Some(TEXT_SUB), "行内中性按钮静止时是一行普通亮度的字");
        let mut hot = Snapshot::default();
        hot.pressed = Some("cancel-delete".into());
        assert_eq!(quiet_button("取消", "cancel-delete", true, &hot).get("bg"), Some(BUTTON_GHOST_PRESSED));
    }

    /// 禁用的按钮既不挂点击也不挂按下，避免「看起来能点」。
    #[test]
    fn disabled_buttons_attach_no_press_handler() {
        let snapshot = Snapshot::default();
        let button = accent_chip("同步", "sync:3", false, &snapshot);
        assert!(!button.has("on.press"));
        assert!(!button.has("on.click"));
        assert!(button.has("disabled"));
    }

    /// 五种按钮共用同一套几何：主按钮 48 高（半径 = 一半）/ 水平内边距 16 / 15px 字，
    /// 行内按钮按列表行密度压到 30 / 11 / 11 / 6。高度不写进元素树（靠上下内边距逼近），
    /// 所以高度直接断言 token。
    #[test]
    fn button_kinds_share_one_geometry_table() {
        assert_eq!((BUTTON_HEIGHT, BUTTON_RADIUS, BUTTON_PAD_Y), (48, 24, 13));
        assert_eq!((CHIP_HEIGHT, CHIP_RADIUS, CHIP_PAD_Y), (30, 15, 6));

        let snapshot = Snapshot::default();
        let primary = primary_button("打开游戏", "launch", true, &snapshot);
        assert_eq!(primary.get("padding"), None);
        assert_eq!(primary.get("pl"), Some("16"));
        assert_eq!(primary.get("pt"), Some("13"));
        assert_eq!(primary.get("radius"), Some("24"));
        assert_eq!(primary.get("size"), Some("15"));
        assert_eq!(primary.get("shrink"), Some("0"), "按钮不能被 flex 压窄");

        let chip = accent_chip("同步", "sync:1", true, &snapshot);
        assert_eq!(chip.get("pl"), Some("11"));
        assert_eq!(chip.get("pt"), Some("6"));
        assert_eq!(chip.get("radius"), Some("15"));
        assert_eq!(chip.get("size"), Some("12"));
    }

    /// 分段控件的选中态**只有一层实色**：渐变、内高光、外投影都已删除
    /// （`prop()` 逃生舱是空操作，留着只会让预览与真机不一致）。
    #[test]
    fn selected_segment_is_a_single_flat_fill() {
        let snapshot = Snapshot::default();
        let bar = segmented(&[("全部".into(), "line:*".into(), true), ("玲线".into(), "line:玲线".into(), false)], true, &snapshot);
        let active = &bar.children[0];
        assert_eq!(active.get("bg"), Some(SURFACE_STRONG), "选中态就是一块更亮的实色");
        assert!(!active.has("background"), "不许再下发渐变");
        assert!(!active.has("inset") && !active.has("shadow"), "不许再下发内高光/外投影");
    }

    #[test]
    fn progress_bar_marks_by_percent_and_keeps_segment_count() {
        let bar = progress_bar(50, 10);
        assert_eq!(bar.children.len(), 10);
        let filled = bar.children.iter().filter(|child| child.get("bg") != Some(TRACK)).count();
        assert_eq!(filled, 5);
        // 每段都靠 flex-grow 撑满，不依赖百分比宽度。
        assert!(bar.children.iter().all(|child| child.get("grow") == Some("1")));
    }

    #[test]
    fn nav_bar_has_one_entry_per_page() {
        let snapshot = Snapshot::default();
        let nav = nav_bar(&snapshot);
        let buttons = nav.find(|node| node.tag == Tag::Button);
        assert_eq!(buttons.len(), Page::ALL.len());
    }

    /// 导航条必须包在**横向滚动区**里：七个两字标签在 400px 窗口里已经很满，
    /// 不包滚动区就只能被裁掉，用户点不到最后一项。
    #[test]
    fn nav_bar_scrolls_so_the_last_page_is_reachable() {
        let nav = nav_bar(&Snapshot::default());
        assert_eq!(nav.tag, Tag::Scroll, "导航条外层要是滚动容器");
        assert_eq!(nav.get("scroll"), Some("x"), "要能横向滚动");
        for button in nav.find(|node| node.tag == Tag::Button) {
            assert_eq!(button.get("shrink"), Some("0"), "导航项不能被 flex 压窄");
        }
        assert_eq!(nav.find(|node| node.tag == Tag::Button).len(), Page::ALL.len());
    }

    /// 导航徽标只在**有事要做**时出现：没装的章节数 > 0 才挂数字。
    #[test]
    fn nav_badge_only_shows_when_there_is_something_to_do() {
        let mut snapshot = Snapshot::default();
        snapshot.library = vec![crate::snapshot::PackView {
            number: 1,
            id: "p1".into(),
            title: "共通线1·归乡".into(),
            minutes: 54,
            bytes: 735_837,
            scenes: 41,
            dialogues: 1066,
            installed: true,
            queued: false,
            active: false,
        }];
        let texts = nav_bar(&snapshot).texts().join(" | ");
        assert!(!texts.contains("章节 0"), "全都装好时不该挂一个 0：{texts}");
        assert!(!texts.contains("日志 0"), "没有错误时不该挂一个 0：{texts}");

        snapshot.log_errors = 2;
        assert!(nav_bar(&snapshot).texts().iter().any(|text| *text == "日志 2"));
    }

    /// 状态胶囊外面包了一层 row：挂在纵向卡片上时不会被 cross-axis 拉满整行
    /// （一颗胶囊变成一条横杠 —— 设备页那张实时状态条上一版就是这样）。
    #[test]
    fn status_pill_keeps_its_capsule_width() {
        let outer = status_pill(StatusKind::Good, "《甜蜜女友2》正在响应");
        assert_eq!(outer.children.len(), 1, "外层只是定位用的一行，里面才是胶囊");
        assert!(!outer.has("bg"), "外层容器自己不填色");
        let pill = &outer.children[0];
        assert_eq!(pill.get("radius"), Some("999"));
        assert_eq!(pill.get("shrink"), Some("0"), "胶囊宽度只由内容决定");
        assert_eq!(pill.get("bg"), Some(StatusKind::Good.bg()));
    }

    /// 区块标签比主标题暗一档：一个卡片里不许有两个都在喊「看我」的标题。
    #[test]
    fn group_label_is_dimmer_than_the_card_title() {
        let card = section("章节列表", Some("15 章".into()));
        let label = card.children[0].children[0].clone();
        assert_eq!(label.get("fg"), Some(TEXT_SUB), "区块标签用次文色");
        assert_eq!(label.get("size"), Some("12"));
        let main = title("甜蜜女友2");
        assert_eq!(main.get("fg"), Some(TEXT_MAIN), "主标题用主文色");
        assert_eq!(main.get("size"), Some("18"));
    }
}

/// 连接进度：一条分段条 + 当前阶段的说明。
///
/// 把它做成**显式阶段**而不是一句状态行，是因为旧实现把「通道通了」当成「数据就绪了」
/// —— `request_pack_list` 只挂在「还没活」那条分支上，`hello-ok` 一到就再也发不出去，
/// 章节列表永远拿不到。拆成阶段之后，「已握手」和「已拿到章节列表」再也没法混为一谈。
pub fn session_steps(stage: SessionStage) -> Node {
    let total = SessionStage::ALL.len() as u32;
    let done = stage.index() as u32 + 1;

    let mut row = Node::new(Tag::Div).full().row().gap(4).align("center");
    for step in 0..total {
        let reached = step < done;
        row = row.child(
            Node::new(Tag::Div)
                .h(4)
                .grow(1.0)
                .shrink(1.0)
                .radius(2)
                .bg(if reached { ACCENT } else { TRACK }),
        );
    }

    let line = Node::new(Tag::Div)
        .full()
        .row()
        .align("center")
        .gap(GAP_XS)
        .child(label(format!("{done}/{total}"), SIZE_SMALL, TEXT_DIM).shrink(0.0))
        .child(label(stage.label(), SIZE_SMALL, TEXT_MAIN).weight(600).shrink(0.0));

    Node::new(Tag::Div)
        .full()
        .column()
        .gap(GAP_XS)
        .child(row)
        .child(line)
        .child(label(stage.pending_hint(), SIZE_SMALL, TEXT_SUB))
}

/// 失败卡片：码的标题 + 细节原文 + 「怎么办」。
///
/// **细节原文原样显示**（手环回什么就写什么）：这一层存在的意义就是让
/// 「超时」和「空间不足」在界面上长得不一样，顺手把对端那句话吞掉就白做了。
pub fn error_card(error: &ErrorView) -> Node {
    let mut card = Node::new(Tag::Div)
        .full()
        .column()
        .gap(GAP_XS)
        .pad(GAP)
        .radius(CARD_RADIUS)
        .bg(SURFACE)
        .border(1, &alpha(BUTTON_DANGER, "66"))
        .child(
            Node::new(Tag::Div)
                .full()
                .row()
                .align("center")
                .gap(GAP_XS)
                .child(state_badge(error.code.label(), StatusKind::Bad))
                .child(Node::new(Tag::Div).grow(1.0)),
        );
    if !error.detail.trim().is_empty() {
        card = card.child(label(error.detail.trim(), SIZE_SMALL, TEXT_SUB));
    }
    card.child(label(error.code.advice(), SIZE_SMALL, TEXT_DIM))
}

/// 连接进度条与失败卡片。
#[cfg(test)]
mod progress_tests {
    use super::*;
    use crate::ErrorCode;

    /// 分段条的点亮段数 = 当前阶段的下标 + 1；当前阶段名与「下一步提示」都要写出来。
    #[test]
    fn session_steps_light_up_the_current_stage() {
        for stage in SessionStage::ALL {
            let node = session_steps(stage);
            let filled = node
                .find(|item| item.get("bg") == Some(ACCENT))
                .len();
            assert_eq!(
                filled,
                stage.index() + 1,
                "{stage:?} 应该点亮 {} 段（走到哪一步就是几步）",
                stage.index() + 1
            );
            let texts = node.texts();
            assert!(texts.contains(&stage.label()), "{stage:?} 要写出阶段名：{texts:?}");
            assert!(
                texts.contains(&stage.pending_hint()),
                "{stage:?} 要写出下一步提示：{texts:?}"
            );
        }
        // 分段总数固定 = 阶段总数：少一段就说明有阶段永远点不亮。
        let total = SessionStage::ALL.len();
        let all_segments = session_steps(SessionStage::Idle).find(|item| item.has("bg")).len();
        assert_eq!(all_segments, total);
    }

    /// 失败卡片：码的标题 + 细节原文 + 「怎么办」，三样都要出现，而且**原文原样带出来**。
    #[test]
    fn error_card_shows_code_detail_and_advice() {
        let code = ErrorCode::Space;
        let card = error_card(&ErrorView::new(code, "write-空间不足"));
        let texts = card.texts();
        assert!(texts.contains(&code.label()), "{texts:?}");
        assert!(texts.contains(&code.advice()), "「怎么办」不能省：{texts:?}");
        assert!(texts.contains(&"write-空间不足"), "对端的原文要原样显示：{texts:?}");
    }

    /// 细节为空时不留一行空白。
    #[test]
    fn error_card_without_detail_has_no_blank_line() {
        let card = error_card(&ErrorView::new(ErrorCode::Timeout, ""));
        let texts = card.texts();
        assert_eq!(texts.len(), 2, "只该有标题和「怎么办」：{texts:?}");
    }
}
