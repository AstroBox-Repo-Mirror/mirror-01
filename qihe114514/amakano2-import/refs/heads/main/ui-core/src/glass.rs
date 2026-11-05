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
use super::chunk_label;
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

/// 元信息：一行里跟在主体后面的小字（`54 分钟 · 736 KB`）。
pub fn meta(text: impl Into<String>) -> Node {
    label(text, SIZE_TINY, TEXT_DIM)
}

/// 说明文字：需要读一句才懂的那种（帮助、提示）。
pub fn note(text: impl Into<String>) -> Node {
    label(text, SIZE_TINY, TEXT_DIM)
}

// ---------------------------------------------------------------- 容器
//
// v3 起**只有一种容器**：[`group`]（成组列表）。
// 旧的 `panel()` / `nested()` / `section()` / `section_with_action()` / `empty()`
// 已整批删除 —— 它们是「每块内容一张卡」那套版式的零件，而宿主没有阴影、没有渐变，
// 一屏等重的卡片分不出组（见下方 `group()` 的说明）。留着只会让人以为有两套版式可选。

// ---------------------------------------------------------------- 成组列表（v3 版式）
//
// iOS 的 inset grouped list：**一个圆角面装若干行**，行间 1px 细分隔线，
// 组与组之间留白。替代旧版的「每块内容一张卡」——
// 旧版在纯实色宿主上（没有阴影、没有渐变）会变成一屏等重的方块，分不出组。

/// 成组列表容器：一个圆角实色面，左右内边距 `GROUP_PAD_X`，
/// 行由调用方用 [`group_row`] / [`divider`] 拼出来。
pub fn group() -> Node {
    Node::new(Tag::Div)
        .full()
        .column()
        .radius(GROUP_RADIUS)
        .bg(SURFACE)
        .pad_x(GROUP_PAD_X)
        .transition(TRANSITION)
}

/// 组内一行：上下 `GROUP_ROW_PAD_Y`，左右已由容器代劳。
pub fn group_row(left: Node) -> Node {
    Node::new(Tag::Div)
        .full()
        .row()
        .align("center")
        .gap(GAP)
        .pad_y(GROUP_ROW_PAD_Y)
        .child(left)
}

/// 组内一行，右侧跟一个固定的动作区。
///
/// 右侧靠 `grow` 的空节点推开，**不用 `justify("between")`**：
/// 那个走的是 `prop("justify-content", ...)` 逃生舱，不在宿主白名单里
/// （见 theme.rs 的说明），推不开就会全挤在左边。
pub fn group_row_with(left: Node, right: Node) -> Node {
    group_row(left).child(Node::new(Tag::Div).grow(1.0)).child(right)
}

/// 行间分隔线。
pub fn divider() -> Node {
    Node::new(Tag::Div).full().h(GROUP_LINE_W).bg(STROKE_SOFT)
}

/// 长列表的**保底滚动区**：`Scroll` + `scroll("y")` + `maxh`。
///
/// ⚠️ 为什么必须有这一层：宿主 `ui-v3` 的元素树**是否随窗口纵向滚动，没有真机证据**
/// （`ui-v3-proto/README.md` 里就写着 `SCROLL-AREA` 的能力尚无截图佐证）。
/// 而 `scroll("y")` + `maxh` 是**旧版在真机上验证过可用**的写法。
///
/// 取舍：宁可让长列表自己滚（多一层滚动、手指落点需要适应），
/// 也不能赌「整页会滚」—— 赌错的后果是章节列表和存档列表**到底部滚不动**，
/// 那是核心功能不可达，比多一层滚动严重得多。
///
/// 短列表（一般 ≤6 行）不走这里，直接铺开：它们本来就不会超出窗口，
/// 套一层滚动区只会让滚动行为变得不可预期。
pub fn scroll_list(list: Node, max_height: u32) -> Node {
    Node::new(Tag::Scroll)
        .full()
        .scroll("y")
        .maxh(max_height)
        .child(list)
}

/// 组标题（组外面、左侧的一行小字）。iOS 的 section header。
///
/// 返回的是**纵向容器**而不是一行文字：调用方一律写 `group_heading("明细").child(组)`，
/// 如果这里返回 `P`，那个 `.child()` 会把整个列表塞进文字节点里 ——
/// 真机上表现为「标题被挤成竖排、内容全糊在标题右边」。这个坑已经踩过一次。
pub fn group_heading(text: &str) -> Node {
    Node::new(Tag::Div)
        .full()
        .column()
        .pad_t(22)
        .pad_b(8)
        .pad_x(2)
        .child(label(text, TYPE_META, TEXT_DIM))
}

/// 带右侧动作的组标题（「手环上的存档   3/20  刷新」）。
///
/// 同样是**纵向容器**：第一行是标题行，调用方 `.child(组)` 时组会落在标题下面。
pub fn group_heading_with(text: &str, right: Node) -> Node {
    let head = Node::new(Tag::Div)
        .full()
        .row()
        .align("center")
        .gap(GAP_SM)
        .child(label(text, TYPE_META, TEXT_DIM).grow(1.0).shrink(1.0))
        .child(right);
    Node::new(Tag::Div).full().column().pad_t(22).pad_b(8).pad_x(2).child(head)
}

/// 右侧对数值：靠 `grow` 的空白推到最右，自身 `shrink(0)` 不被压窄。
///
/// 宿主没有 `text-align`，做不出真正的右对齐；这里靠「左侧 grow、右侧固定」
/// 达成同样的视觉结果，而且换行时右侧整块一起移动。
pub fn value(text: impl Into<String>) -> Node {
    label(text, TYPE_BODY, TEXT_MAIN).shrink(0.0)
}

/// 行标题（章节名 / 设置项名）。
pub fn row_title(text: impl Into<String>) -> Node {
    label(text, TYPE_ROW, TEXT_MAIN)
}

/// 行说明（标题下面那一行小字）。
pub fn row_detail(text: impl Into<String>) -> Node {
    label(text, TYPE_META, TEXT_DIM)
}

/// 进度条：**分段点亮**，不用百分比宽度。
///
/// 宿主 `width` 只收 u32 像素（没有百分比），所以「一条轨道 + 一个百分比宽的填充」
/// 做不出来。改成 N 个等宽小块，按百分比点亮前若干个 ——
/// 渲染结果与宿主能力无关，只是看起来是虚线。
pub fn progress_bar(percent: u32, segments: u32, height: u32, color: &str) -> Node {
    let segments = segments.max(1);
    let lit = ((percent.min(100) as u64 * segments as u64) / 100) as u32;
    let mut bar = Node::new(Tag::Div).full().row().gap(2);
    for index in 0..segments {
        bar = bar.child(
            Node::new(Tag::Div)
                .grow(1.0)
                .h(height)
                .radius(height / 2)
                .bg(if index < lit { color } else { TRACK }),
        );
    }
    bar
}

/// 两栏键值行（`kv_grid` 的零件）。
///
/// 键**固定宽度不许收缩**：4 个字的键（「请求超时」）在窄窗的两栏网格里只差几个像素
/// 就会被压成「请求超 / 时」两行，看着像排版坏了。值那一侧才是允许变窄的。
fn kv(key: &str, value: &str) -> Node {
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
    geometry(kind, text, action, enabled, snapshot, if inline { CHIP_FONT } else { BUTTON_FONT }, if inline { CHIP_PAD_Y } else { BUTTON_PAD_Y }, if inline { CHIP_PAD_X } else { BUTTON_PAD_X })
}

/// 按钮的几何与交互，字号/内边距由调用方指定。
///
/// 抽出来是为了让[`hero_button`]复用同一套配色与按下逻辑 ——
/// 只放宽尺寸，不改外观规则（仍然是实色、无模糊、无渐变、无子元素）。
fn geometry(
    kind: ButtonKind,
    text: &str,
    action: &str,
    enabled: bool,
    snapshot: &Snapshot,
    font: u32,
    pad_y: u32,
    pad_x: u32,
) -> Node {
    let inline = kind.inline();
    let pressed = enabled && snapshot.is_pressed(action);
    let look = skin(kind, pressed, false);

    let node = Node::text(Tag::Button, text)
        .size(font)
        .fg(look.fg)
        .pad_x(pad_x)
        .pad_y(pad_y)
        .radius(if inline { CHIP_RADIUS } else { BUTTON_RADIUS })
        // flex 子项默认可收缩：不显式禁掉的话，列表行/导航里的按钮会被压窄、文字折成两行。
        .shrink(0.0)
        .bg(look.bg);

    if !enabled {
        return node.disabled().opacity(0.42);
    }
    node.click(action).press(&super::actions::press_id(action)).hover(&hover_key(action))
}

/// **引导页的主入口按钮**：比常规主按钮明显大一圈。
///
/// 为什么单独做一个而不是把 [`BUTTON_HEIGHT`] 调大：那个值是全界面的按钮基准，
/// 一动就会挤坏原生界面里所有页的排版（比如设置页的行内按钮、存档行的操作区）。
/// 而这里要的只是「主面板上最显眼的那个东西」—— 它是整个插件唯一的入口，
/// 用户在插件面板里第一眼找不到它，就等于插件不可用。
///
/// 几何：**56 高 / 17px 字 / 左右 20 内边距 / 胶囊圆角**。
/// 比常规 48/15/16 大一号，但没大到和页面标题抢眼（标题 22px）。
pub fn hero_button(text: &str, action: &str, enabled: bool, snapshot: &Snapshot) -> Node {
    geometry(ButtonKind::Primary, text, action, enabled, snapshot, HERO_FONT, HERO_PAD_Y, HERO_PAD_X)
        // 高度钉死：只靠内边距逼近时，不同字体的行高差几个像素就会变成 54 或 58。
        // 宿主里给死高度后文字不会垂直居中，所以**内边距仍然保留**（两者一起用）。
        .h(HERO_HEIGHT)
        // **占满整行**：主入口是引导页上唯一要做的事，一个内容宽度的胶囊
        // 在空白页面上看着像普通按钮。撑满后才是一眼就能按到的那个东西。
        .full()
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
                .fg(BUTTON_PRIMARY_TEXT)
                .bg(ACCENT_DEEP)
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

// ---------------------------------------------------------------- 布局骨架

/// 顶栏：插件图标 + 产品名 + 右侧设备名。
///
/// v3 版式：**不再是卡片**，就是页面最上面的一条。旧版把它做成 `panel` 卡片，
/// 与下面所有卡片同重，页面上找不到「起点」。现在靠留白与一条设备状态行划边界。
///
/// 设备名**允许收缩**（`shrink(1.0)`）：它最长、又最不重要；标题固定 `shrink(0)`。
/// 反过来做的话，长设备名（"Xiaomi Smart Band 10 Pro"）会把标题挤出容器。
pub fn top_bar(snapshot: &Snapshot) -> Node {
    let name = if snapshot.device.name.is_empty() {
        if snapshot.device.connected { "手环" } else { "未连接" }.to_string()
    } else {
        snapshot.device.name.clone()
    };
    let dot = if snapshot.device.connected && snapshot.device.alive {
        OK
    } else if snapshot.device.connected {
        WARN
    } else {
        TEXT_DIM
    };
    Node::new(Tag::Div)
        .full()
        .row()
        .align("center")
        .gap(10)
        .pad_x(PAGE_PAD_X)
        .pad_t(14)
        .pad_b(10)
        .child(brand_mark(snapshot, 30))
        .child(label("甜蜜女友2", 16, TEXT_MAIN).shrink(0.0))
        .child(Node::new(Tag::Div).grow(1.0))
        .child(
            Node::new(Tag::Div)
                .row()
                .align("center")
                .gap(6)
                .shrink(1.0)
                .pad_x(8)
                .pad_y(4)
                .radius(8)
                .child(Node::new(Tag::Div).w(6).h(6).radius(3).bg(dot).shrink(0.0))
                .child(label(name, TYPE_BADGE, TEXT_SUB).shrink(1.0)),
        )
}

/// 导航：**顶栏下方的一排标签**，当前页用一根粉色指示条 + 文字变色标出。
///
/// 为什么不放底部固定栏：宿主 `ui-v3` 没有 `position: fixed` 的对应方法
/// （只有 `relative` / `absolute`，绝对定位还要父容器有确定高度），
/// 所以「底部常驻」在这个宿主上做不出来。挪到顶部是唯一能保证**始终可见**的位置。
///
/// 选中态用「指示条 + 颜色」而不是加粗：宿主**没有 `font-weight`**
/// （`prop()` 白名单里没有这个键），加粗下发后毫无变化。
pub fn nav_bar(snapshot: &Snapshot) -> Node {
    let mut bar = Node::new(Tag::Div)
        .full()
        .row()
        .gap(2)
        .pad_x(10)
        .pad_b(10);
    for page in Page::ALL {
        let on = snapshot.page == page;
        // 徽标：待同步章节数 / 错误数。只在「有事要做」时出现。
        let badge_text = match page {
            Page::Push if snapshot.pending_count() > 0 => Some(snapshot.pending_count().to_string()),
            Page::Settings if snapshot.error_count() > 0 => Some(snapshot.error_count().to_string()),
            _ => None,
        };
        let mut item = Node::new(Tag::Button)
            .column()
            .align("center")
            .gap(4)
            .grow(1.0)
            .pad_t(8)
            .pad_b(6)
            // 必须显式给底色 + 关掉默认样式：`BUTTON` 在宿主里有自己的外观，
            // 不关就会变成一排凸起的方块（本地保真预览里一眼就能看到）。
            .bg(TRANSPARENT)
            .without_default_styles()
            .child(
                // 指示条：透明时不占位会跳，所以永远给 3px、只换颜色。
                Node::new(Tag::Div).w(20).h(3).radius(2).bg(if on { ACCENT } else { TRANSPARENT }),
            )
            .child(label(page.label(), TYPE_BADGE, if on { ACCENT } else { TEXT_DIM }));
        if let Some(count) = badge_text {
            item = item.child(label(count, TYPE_BADGE, if on { ACCENT } else { WARN }));
        }
        bar = bar.child(
            item.click(&page.action())
                .press(&super::actions::press_id(&page.action())),
        );
    }
    bar
}

/// 导航条：**已由 [`nav_bar`] 取代**，这里只保留原文说明以免误改。
///
/// 旧实现是「分段控件包在横向滚动区里」，问题是：它长得像筛选器、不像导航，
/// 而且和推送页的线路筛选是两排几乎一样的胶囊，用户第一眼分不清哪个是切页。
/// 现在导航是「文字 + 指示条」，筛选是「胶囊」——两套视觉语言，不会再混。
///
/// 下面这个函数保留给「页面数变多、顶栏放不下」的将来：那时横向滚动是唯一出路。
/// 现在没用到，所以标了 `allow(dead_code)` —— **别因为它没被调用就删掉**，
/// 它是页数从 5 涨到 7+ 时的现成退路。
#[allow(dead_code)]
pub fn nav_bar_scrollable(snapshot: &Snapshot) -> Node {
    let mut items = Vec::new();
    for page in Page::ALL {
        items.push((page.label().to_string(), page.action(), snapshot.page == page));
    }
    Node::new(Tag::Scroll)
        .full()
        .row()
        .justify("center")
        .scroll("x")
        .child(segmented_sized(&items, true, snapshot, 4, 4).pad(4))
}


/// 页面外壳：顶栏 + 设备条 + **传输任务卡（有传输/断点时）** + 导航 + 内容。
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
///
/// v2 新增第四条：**传输任务卡插在设备条与导航之间**（顶栏 → 设备条 → 传输卡 → 导航
/// → 内容）。旧版把它追加在外壳**末尾**，推送页两个内部滚动区把它挤出首屏 ——
/// 用户原话「把传输进度放在页面最下方干什么？都看不到，这明明是最重要的内容」。
pub fn shell(snapshot: &Snapshot, content: Node) -> Node {
    let mut root = Node::new(Tag::Div)
        .full()
        .column()
        .pad_y(PAGE_PAD_Y)
        .child(top_bar(snapshot));
    // 原生界面是**后备通道**：顶栏下先给一条「返回」，让用户随时能回到
    // 「打开 HTML 面板」那条主路径。引导页本身没有页面可切，所以不挂导航。
    if snapshot.native_ui {
        root = root
            .child(super::pages::landing::back_button())
            .child(nav_bar(snapshot));
    }
    root = root.child(transfer_strip(snapshot));
    // 连接是唯一的「外部门」动作：没连上时它必须一直看得见，
    // 而连上之后它就退位给「打开游戏」（在推送页里）。
    if !snapshot.device.connected {
        root = root.child(
            Node::new(Tag::Div)
                .full()
                .pad_x(PAGE_PAD_X)
                .pad_b(GAP)
                .child(primary_button("连接手环", super::actions::CONNECT, true, snapshot)),
        );
    }
    root.child(
        Node::new(Tag::Div)
            .full()
            .column()
            .pad_x(PAGE_PAD_X)
            .pad_b(28)
            .child(content),
    )
}

/// 常驻传输条：**外壳的一部分**，切页不重建、位置不跳。
///
/// 旧版是页面顶部一张大卡（34px 百分比 + 12 段粗条），占了近百像素；
/// 现在压成一条紧凑信息带 —— 传输才是这只插件的主角，但它不需要每次都喊，
/// 只要「瞟一眼就知道传到哪」。
///
/// 三种状态：
/// - 正在传：章名 + 百分比 + 细条 + 速度/剩余；
/// - 断点待续：暖黄 + 「继续传输」按钮（用户重连后唯一的诉求）；
/// - 都没有：一行灰字。
fn transfer_strip(snapshot: &Snapshot) -> Node {
    if let Some(transfer) = snapshot
        .transfer
        .as_ref()
        .filter(|transfer| transfer.started && !transfer.stalled)
    {
        let shaky = transfer.shaky();
        let color = if shaky { WARN } else { ACCENT };
        let mut card = Node::new(Tag::Div)
            .full()
            .column()
            .gap(GAP_SM)
            .radius(GROUP_RADIUS)
            .bg(if shaky { TRANSFER_HELD_BG } else { TRANSFER_BG })
            .border(1, if shaky { TRANSFER_HELD_STROKE } else { TRANSFER_STROKE })
            .pad(13)
            .child(
                Node::new(Tag::Div)
                    .full()
                    .row()
                    .align("center")
                    .gap(GAP_SM)
                    .child(row_title(&transfer.chapter).grow(1.0).shrink(1.0))
                    .child(label(format!("{}%", transfer.percent), TYPE_TITLE, color).shrink(0.0)),
            )
            .child(progress_bar(transfer.percent, 28, 6, color));
        let detail = if transfer.ready {
            format!("{} · 剩余 {}", transfer.speed_label(), transfer.eta_label())
        } else {
            "已发导入请求，正在等手环确认…".to_string()
        };
        card = card.child(
            Node::new(Tag::Div)
                .full()
                .row()
                .gap(GAP_SM)
                .child(label(detail, TYPE_META, TEXT_DIM).grow(1.0))
                .child(label(chunk_label(snapshot.chunk_bytes), TYPE_META, TEXT_DIM).shrink(0.0)),
        );
        return wrap_strip(card);
    } else if let Some(resume) = snapshot.resume.as_ref() {
        let card = Node::new(Tag::Div)
            .full()
            .column()
            .gap(GAP_SM)
            .radius(GROUP_RADIUS)
            .bg(TRANSFER_HELD_BG)
            .border(1, TRANSFER_HELD_STROKE)
            .pad(13)
            .child(
                Node::new(Tag::Div)
                    .full()
                    .row()
                    .align("center")
                    .gap(GAP_SM)
                    .child(state_badge("上次没传完", StatusKind::Warn))
                    .child(Node::new(Tag::Div).grow(1.0))
                    .child(label(format!("{}%", resume.percent), TYPE_TITLE, WARN).shrink(0.0)),
            )
            .child(row_title(&resume.chapter))
            .child(progress_bar(resume.percent, 28, 6, WARN))
            .child(primary_button("继续传输", super::actions::RESUME, !snapshot.band_game_legacy, snapshot));
        return wrap_strip(card);
    } else if snapshot.resume_checking {
        return wrap_strip(
            Node::new(Tag::Div)
                .full()
                .radius(ROW_RADIUS)
                .bg(SURFACE)
                .pad(11)
                .child(label("正在查询手环上的断点…", TYPE_META, TEXT_DIM)),
        );
    } else {
        // 空闲：一行灰字。**不用卡片**——没有内容的时候，卡片就是一块噪音。
        return Node::new(Tag::Div)
            .full()
            .pad_x(PAGE_PAD_X)
            .pad_t(2)
            .pad_b(GAP_SM)
            .child(
                Node::new(Tag::Div)
                    .full()
                    .row()
                    .align("center")
                    .gap(GAP_SM)
                    .child(Node::new(Tag::Div).w(14).h(2).radius(1).bg(TEXT_FAINT).shrink(0.0))
                    .child(idle_strip_text(snapshot)),
            );
    }
}

/// 空闲态那一句话：说清「现在能不能做事」。
fn idle_strip_text(snapshot: &Snapshot) -> Node {
    let text = if !snapshot.device.connected {
        "手环未连接"
    } else if !snapshot.device.alive {
        "手环已连接，等待《甜蜜女友2》回应"
    } else {
        "手环已连接，没有传输任务"
    };
    label(text, TYPE_META, TEXT_DIM).grow(1.0).shrink(1.0)
}

fn wrap_strip(node: Node) -> Node {
    Node::new(Tag::Div).full().pad_x(PAGE_PAD_X).pad_t(4).pad_b(GAP).child(node)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 成组列表只有一层实色面，**不描边、不模糊、不渐变**。
    ///
    /// 旧版有「一级面板 + 二级块」两层（描边 vs 不描边），v3 收敛成一种容器：
    /// 宿主没有阴影、没有渐变，两层实色叠在一起反而看不清哪层是分组。
    /// 组的层次改由**组与组之间的留白**表达。
    #[test]
    fn grouped_list_is_one_flat_surface() {
        let card = group();
        assert!(card.has("bg"), "组必须有实色底（`bg()` 是真机唯一生效的填色手段）");
        assert!(!card.has("border"), "组不描边：层次靠底色与留白，不靠线");
        assert!(!card.has("backdrop"), "不要再下发背景模糊");
        assert!(!card.has("background"), "不要再下发渐变");
        assert!(!card.has("shadow") && !card.has("inset"), "阴影在真机上是空操作，不要再下发");

        // 行内不带自己的底色：整组是同一块面，行只负责排版。
        let row = group_row(label("x", TYPE_BODY, TEXT_MAIN));
        assert!(!row.has("bg"), "行不自己铺底色，否则组里会出现一条条色带");
        assert!(!row.has("radius"), "行不需要圆角：圆角属于组");
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

        // 引导页（默认形态）的外壳：顶栏 + 传输条 + 连接条（未连接时）+ 内容容器。
        // **没有导航** —— 引导页只有一屏，没有页面可切。
        assert_eq!(root.children.len(), 4);
        // 内容包在带左右边距的容器里，所以查整棵树而不是直接子节点。
        assert_eq!(
            root.find(|node| node.get("marker") == Some("content")).len(),
            1,
            "内容必须在树里"
        );
        // 导航不该出现在引导页里。
        assert!(
            !root.children[1].texts().contains(&"推送"),
            "引导页没有页面导航：{:?}",
            root.children[1].texts()
        );
    }

    /// 原生界面（后备通道）的外壳：顶栏 → 返回 → 导航 → 传输条 → … → 内容。
    ///
    /// 这条守的是「切到原生界面之后还能回到 HTML 面板那条路」——
    /// 返回必须挂在**外壳**上（任何一页都看得见），不能藏在某一页里。
    #[test]
    fn native_shell_carries_back_button_then_navigation() {
        let mut snapshot = Snapshot::default();
        snapshot.native_ui = true;
        let content = Node::new(Tag::Div).full().prop("marker", "content");
        let root = shell(&snapshot, content);

        // 顶栏 / 返回 / 导航 / 传输条 / 连接条 / 内容。
        assert_eq!(root.children.len(), 6, "原生界面比引导页多出「返回」和「导航」");
        let back = root
            .find(|node| node.get("on.click") == Some(super::super::actions::BACK_TO_LANDING));
        assert_eq!(back.len(), 1, "外壳里要有一个「返回」");
        assert_eq!(root.children[1].tag, Tag::Div, "第二块是返回条");
        assert!(root.children[2].texts().contains(&"推送"), "第三块是导航");
    }

    /// 顶栏压成一行：品牌方块 + 名称 + 状态胶囊，**不再有版本副标题**。
    #[test]
    fn top_bar_is_one_compact_row() {
        let mut snapshot = Snapshot::default();
        snapshot.version = "3.0.0".into();
        snapshot.device.name = "Xiaomi Smart Band 10 Pro".into();
        snapshot.device.connected = true;
        let bar = top_bar(&snapshot);
        // 拿不到图标时回落成品牌色方块（30/3 = 10 圆角）。
        let blocks = bar.find(|node| node.get("bg") == Some(ACCENT_DEEP) && node.get("radius") == Some("10"));
        assert_eq!(blocks.len(), 1, "顶栏应该恰好一个品牌色方块");
        // 没有图片元素（`brand` 为空）。
        assert!(bar.find(|node| node.tag == Tag::Image).is_empty());
        let texts = bar.texts();
        assert!(texts.contains(&"甜蜜女友2"), "顶栏要有产品名：{texts:?}");
        assert!(texts.contains(&"Xiaomi Smart Band 10 Pro"), "顶栏要有设备名：{texts:?}");
        // 设备名是唯一允许收缩的一项（它最长、又最不重要）。
        let name = bar
            .find(|node| node.text.as_deref() == Some("Xiaomi Smart Band 10 Pro"))
            .pop()
            .expect("设备名节点");
        assert_eq!(name.get("shrink"), Some("1"), "设备名允许被压缩，不能挤掉标题");
    }

    /// 没连手环时，「连接手环」按钮必须一直看得见（外壳常驻，不在某一页里）。
    ///
    /// 旧版把它放在推送页内部的设备卡里，用户切到存档/统计就找不到连接入口了。
    #[test]
    fn connect_button_lives_in_the_shell_when_disconnected() {
        let root = shell(&Snapshot::default(), Node::new(Tag::Div));
        let texts = root.texts();
        assert!(texts.contains(&"连接手环"), "外壳里要有连接入口：{texts:?}");
        let buttons = root.find(|node| node.get("on.click") == Some(super::super::actions::CONNECT));
        assert_eq!(buttons.len(), 1, "连接按钮恰好一个");
    }

    /// 连上之后连接按钮退场，不再占着首屏。
    #[test]
    fn connected_shell_drops_the_connect_button() {
        let mut snapshot = Snapshot::default();
        snapshot.device.connected = true;
        snapshot.device.alive = true;
        snapshot.device.name = "Xiaomi Smart Band 10 Pro".into();
        let root = shell(&snapshot, Node::new(Tag::Div));
        assert!(
            root.find(|node| node.get("on.click") == Some(super::super::actions::CONNECT)).is_empty(),
            "已经连上就不该再给连接按钮"
        );
        assert!(root.texts().contains(&"Xiaomi Smart Band 10 Pro"), "顶栏要有设备名");
    }

    /// 传输中：进度常驻在外壳里，**每一页都看得见**（用户要求「传输显示要常驻」）。
    ///
    /// 传输条的位置是「顶栏 → 导航 → 传输条 → 内容」，所以它在**内容之上**，
    /// 切页不会把它推出屏幕。
    #[test]
    fn every_page_keeps_the_transfer_strip_above_the_content() {
        let mut snapshot = crate::demo();
        for page in Page::ALL {
            snapshot.page = page;
            let tree = crate::pages::render(&snapshot);
            let texts = tree.texts();
            assert!(
                texts.iter().any(|text| *text == "33%"),
                "{page:?} 页丢了常驻传输进度：{texts:?}"
            );
        }
    }

    /// 没有传输、没有断点、不在查询：外壳里**不出现传输卡**，只留一行灰字。
    ///
    /// 空闲时给卡片就是一块噪音 —— 旧版每页顶部都挂着一条「没有传输任务」的卡片。
    #[test]
    fn idle_shell_shows_a_plain_line_instead_of_a_card() {
        let root = shell(&Snapshot::default(), Node::new(Tag::Div));
        assert!(root.find(|node| node.get("bg") == Some(TRANSFER_BG)).is_empty());
        assert!(root.find(|node| node.get("bg") == Some(TRANSFER_HELD_BG)).is_empty());
        assert!(root.texts().contains(&"手环未连接"), "空闲态要给一句话：{:?}", root.texts());
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
        assert_eq!(active.get("bg"), Some(ACCENT_DEEP), "选中态是品牌粉实色");
        assert_eq!(active.get("fg"), Some(BUTTON_PRIMARY_TEXT), "选中态文字用白字");
        assert!(!active.has("background"), "不许再下发渐变");
        assert!(!active.has("inset") && !active.has("shadow"), "不许再下发内高光/外投影");
    }

    #[test]
    fn progress_bar_marks_by_percent_and_keeps_segment_count() {
        let bar = progress_bar(50, 28, 6, ACCENT);
        assert_eq!(bar.children.len(), 28);
        let filled = bar.children.iter().filter(|child| child.get("bg") != Some(TRACK)).count();
        assert_eq!(filled, 14);
        // 每段都靠 flex-grow 撑满，不依赖百分比宽度（宿主 width 只收像素）。
        assert!(bar.children.iter().all(|child| child.get("grow") == Some("1")));
        assert!(bar.children.iter().all(|child| child.get("h") == Some("6")));
    }

    #[test]
    fn nav_bar_has_one_entry_per_page() {
        let snapshot = Snapshot::default();
        let nav = nav_bar(&snapshot);
        let buttons = nav.find(|node| node.tag == Tag::Button);
        assert_eq!(buttons.len(), Page::ALL.len());
    }

    /// 导航的选中态**不能靠字重**：宿主没有 `font-weight`，加粗下发后毫无变化。
    ///
    /// 所以选中态必须同时有「颜色变化」和「指示条」两条视觉线索，
    /// 否则用户在真机上分不出自己在哪一页。
    #[test]
    fn nav_marks_the_current_page_without_font_weight() {
        let mut snapshot = Snapshot::default();
        snapshot.page = Page::Stats;
        let nav = nav_bar(&snapshot);
        for node in nav.find(|node| node.has("weight")) {
            panic!("导航不该依赖字重（宿主不认）：{:?}", node.texts());
        }
        // 恰好一根粉色指示条，且落在当前页那一项里。
        let bars = nav.find(|node| node.get("bg") == Some(ACCENT) && node.get("h") == Some("3"));
        assert_eq!(bars.len(), 1, "当前页要有一根粉色指示条");
        // 当前页文字也是粉色。
        let accent_texts = nav.find(|node| node.get("fg") == Some(ACCENT));
        assert!(
            accent_texts.iter().any(|node| node.text.as_deref() == Some("统计")),
            "当前页文字要用强调色"
        );
    }

    /// 四项导航等分一行：每一项都 `grow(1)`，不需要横向滚动。
    #[test]
    fn nav_items_share_the_row_equally() {
        let nav = nav_bar(&Snapshot::default());
        let buttons = nav.find(|node| node.tag == Tag::Button);
        assert_eq!(buttons.len(), Page::ALL.len());
        for button in buttons {
            assert_eq!(button.get("grow"), Some("1"), "导航项要等分整行");
        }
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
        assert!(!texts.contains("推送 0"), "全都装好时不该挂一个 0：{texts}");
        assert!(!texts.contains("设置 0"), "没有错误时不该挂一个 0：{texts}");

        snapshot.log_errors = 2;
        // 徽标现在单独占一行（导航项是纵向排列的），所以断言「有个 2」而不是「设置 2」。
        let badges = nav_bar(&snapshot).texts().join(" | ");
        assert!(badges.contains("2"), "有错误时设置页要挂数：{badges}");
    }

    /// 长列表必须包一层 `Scroll` + `scroll("y")` + `maxh`。
    ///
    /// 这条守的是**真机可达性**：宿主 `ui-v3` 的元素树是否随窗口纵向滚动，
    /// 仓库里没有任何实证（`ui-v3-proto/README.md` 明写 `SCROLL-AREA` 能力尚无截图）。
    /// 而 `scroll("y")` + `maxh` 是**旧版真机验证过**的写法。
    /// 若赌「整页会滚」而赌错，章节列表和存档列表到底部滚不动 —— 核心功能不可达。
    #[test]
    fn scroll_list_wraps_long_lists_in_a_verified_scroller() {
        let wrapped = scroll_list(group(), 400);
        assert_eq!(wrapped.tag, Tag::Scroll, "长列表外层必须是滚动容器");
        assert_eq!(wrapped.get("scroll"), Some("y"), "要能纵向滚动");
        assert_eq!(wrapped.get("maxh"), Some("400"), "要有高度上限，否则滚动区不成立");
        assert_eq!(wrapped.children.len(), 1, "滚动区里就一个列表");
        assert_eq!(wrapped.children[0].tag, Tag::Div);
    }

    /// 组标题必须是**容器**，不能是一行文字。
    ///
    /// 所有页面都写 `group_heading("明细").child(列表)`。如果它返回 `P`，
    /// `.child()` 会把整个列表塞进文字节点里 —— 真机上标题被挤成竖排、
    /// 内容全糊在标题右侧。这条用例钉死「标题后面挂得下东西」。
    #[test]
    fn group_heading_is_a_container_so_children_land_below_it() {
        let heading = group_heading("明细");
        assert_eq!(heading.tag, Tag::Div, "组标题要能挂子节点");
        assert_eq!(heading.get("flex"), Some("column"), "子节点要排在标题下面，不是右边");

        let with_action = group_heading_with("手环上的存档", meta("3/20"));
        assert_eq!(with_action.tag, Tag::Div);
        assert_eq!(with_action.get("flex"), Some("column"));
        // 第一行才是「标题 + 右侧动作」那一行。
        let head = &with_action.children[0];
        assert_eq!(head.get("flex"), Some("row"), "标题行是横排");
        assert_eq!(head.children.len(), 2);
        // 挂上列表后，列表必须是**标题节点的兄弟**（在下面），而不是标题行的一部分。
        let block = group_heading_with("x", meta("y")).child(group());
        assert_eq!(block.children.len(), 2);
        assert_eq!(block.children[1].tag, Tag::Div);
    }

    /// 区块标签用次文色：一个卡片里不许有两个都在喊「看我」的标题。
    #[test]
    fn group_heading_uses_the_meta_size_and_dim_color() {
        let heading = group_heading("章节列表");
        let text = heading
            .find(|node| node.text.as_deref() == Some("章节列表"))
            .pop()
            .expect("标题文字");
        assert_eq!(text.get("fg"), Some(TEXT_DIM), "组标题用弱文色：它是标签，不是内容");
        assert_eq!(text.get("size"), Some(TYPE_META.to_string().as_str()), "组标题用小字");

        // 右侧动作行：标题 + 动作都在，动作不收缩。
        let with_action = group_heading_with("手环上的存档", meta("3/20"));
        let head = &with_action.children[0];
        assert_eq!(head.children.len(), 2, "标题行 = 标题 + 右侧动作");
    }
}

/// 连接进度：一条分段条 + 当前阶段的说明。
///
/// 把它做成**显式阶段**而不是一句状态行，是因为旧实现把「通道通了」当成「数据就绪了」
/// —— `request_pack_list` 只挂在「还没活」那条分支上，`hello-ok` 一到就再也发不出去，
/// 章节列表永远拿不到。拆成阶段之后，「已握手」和「已拿到章节列表」再也没法混为一谈。
///
/// ⚠️ v3 起**渲染路径不再用它**：连接进度改由外壳的传输条 + 顶栏状态点承担，
/// 页面里不再画那条七段进度条（它一屏占掉一整行，而用户真正要看的是「能不能点同步」）。
/// 函数与它的用例保留 —— 阶段语义（`SessionStage`）本身没过时，将来若要
/// 恢复可视化，这里就是现成的。
#[allow(dead_code)]
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
