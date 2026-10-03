//! 抽象节点树 → 宿主 `ui-v3` 元素。
//!
//! 这是整个界面里**唯一**碰宿主 UI 类型的地方：`amakano2_ui` 那边只产出抽象的
//! [`Node`]，宿主接口以后迭代（比如升到 ui-v4）只需要改这一个文件。
//!
//! 两条踩过的坑写在这里，别忘：
//! 1. `box-shadow` 只能下发一次 —— 外投影与内高光都写这个属性，分两次下发后者会覆盖前者，
//!    所以先收集再合并。**但真机实测宿主的 `prop()` 逃生舱是空操作，这条现在没有视觉效果**；
//!    合并代码留着是因为它是正确写法，宿主哪天接上了就直接生效。
//! 2. 宿主 `ui-v3` 有 `size` 却没有 `font-weight`，也没有渐变、行高、字距；这些只能走
//!    `prop(名字, 值)`。真机实测这些**同样全部无效**（见 docs/插件开发注意事项.md 6.2），
//!    所以凡是走逃生舱的属性都只是装饰：布局与观感不能依赖它
//!    —— 底色必须给 `bg()`，别无他法（按钮与面板现在都只是「实色底 + 圆角」）。

use amakano2_ui::{Node, Tag};
use astrobox_ng_wit::astrobox::psys_host::ui_v3::{self, Element, ElementType, Event, FlexDirection};

/// 渲染一页。
pub fn paint(element_id: &str, root: &Node) {
    ui_v3::render(element_id, to_element(root));
}

fn to_element(node: &Node) -> Element {
    let mut element = Element::new(element_type(node.tag), node.text.as_deref());
    let mut shadows: Vec<&str> = Vec::new();
    for (key, value) in &node.props {
        match key.as_str() {
            "shadow" | "inset" => shadows.push(value),
            _ => element = apply(element, key, value),
        }
    }
    if !shadows.is_empty() {
        element = element.prop("box-shadow", &shadows.join(", "));
    }
    for child in &node.children {
        element = element.child(to_element(child));
    }
    element
}

/// 把一条抽象属性落到宿主元素上。
fn apply(element: Element, key: &str, value: &str) -> Element {
    match key {        // 布局
        "flex" => element.flex().flex_direction(direction(value)),
        "gap" => element.gap(number(value)),
        "align" => match value {
            "start" => element.align_start(),
            "end" => element.align_end(),
            "stretch" => element.prop("align-items", "stretch"),
            _ => element.align_center(),
        },
        "justify" => match value {
            "start" => element.justify_start(),
            "end" => element.justify_end(),
            "between" => element.prop("justify-content", "space-between"),
            "around" => element.prop("justify-content", "space-around"),
            _ => element.justify_center(),
        },
        "grow" => element.flex_grow(decimal(value)),
        "shrink" => element.flex_shrink(decimal(value)),
        "grid" => element.grid_template_columns(value),
        // 盒模型
        "padding" => element.padding(number(value)),
        "pt" => element.padding_top(number(value)),
        "pb" => element.padding_bottom(number(value)),
        "pl" => element.padding_left(number(value)),
        "pr" => element.padding_right(number(value)),
        "margin" => element.margin(number(value)),
        "mt" => element.margin_top(number(value)),
        "mb" => element.margin_bottom(number(value)),
        "ml" => element.margin_left(number(value)),
        "mr" => element.margin_right(number(value)),
        // 尺寸
        "w" => match value {
            "full" => element.width_full(),
            "half" => element.width_half(),
            other => element.width(number(other)),
        },
        "h" => match value {
            "full" => element.height_full(),
            "half" => element.height_half(),
            other => element.height(number(other)),
        },
        "maxw" => element.max_width(number(value)),
        "maxh" => element.max_height(number(value)),
        "minw" => element.min_width(number(value)),
        "minh" => element.min_height(number(value)),
        // 视觉
        "radius" => element.radius(number(value)),
        "bg" => element.bg(value),
        "fg" => element.text_color(value),
        "size" => element.size(number(value)),
        "border" => match value.split_once(' ') {
            Some((width, color)) => element.border(number(width), color),
            None => element,
        },
        "opacity" => element.opacity(decimal(value)),
        "transition" => element.transition(value),
        "transform" => element.transform(value),
        "filter" => element.filter(value),
        "backdrop" => element.backdrop_filter(value),
        "pos" => match value {
            "absolute" => element.absolute(),
            _ => element.relative(),
        },
        "z" => element.z_index(integer(value)),
        "disabled" => element.disabled(),
        "nodflt" => element.without_default_styles(),
        "scroll" => match value {
            "x" => element.prop("overflow-x", "auto").prop("overflow-y", "hidden"),
            "both" => element.prop("overflow-x", "auto").prop("overflow-y", "auto"),
            _ => element.prop("overflow-y", "auto").prop("overflow-x", "hidden"),
        },
        // 交互
        "on.click" => element.on(Event::Click, value),
        "on.pointerup" => element.on(Event::PointerUp, value),
        "on.press" => element.on(Event::PointerDown, value),
        "on.enter" => element.on(Event::MouseEnter, value),
        "on.leave" => element.on(Event::MouseLeave, value),
        // 抽象键名 → 真正的 CSS 属性名。
        // 逃生舱是按属性名透传的，直接把 `weight` 这种抽象名字递过去宿主会静默忽略，
        // 表现就是「预览里字重正常、真机上全丢」。凡是不叫 CSS 名字的抽象键都必须在这里显式翻译。
        "weight" => element.prop("font-weight", value),
        "align-text" => element.prop("text-align", value),
        "origin" => element.transform_origin(value),
        "wrap" => element.prop("flex-wrap", value),
        "value" => element,
        // 逃生前最后一站：已经是合法 CSS 属性名的（letter-spacing / line-height / background /
        // box-shadow / word-break / min-height …）原样透传。
        other => element.prop(other, value),
    }
}

fn element_type(tag: Tag) -> ElementType {
    match tag {
        Tag::Div | Tag::Divider | Tag::Icon => ElementType::Div,
        Tag::P => ElementType::P,
        Tag::Span => ElementType::Span,
        Tag::Button => ElementType::Button,
        Tag::Badge => ElementType::Badge,
        Tag::Progress => ElementType::Progress,
        Tag::Switch => ElementType::Switch,
        Tag::Slider => ElementType::Slider,
        Tag::Grid => ElementType::Grid,
        Tag::Scroll => ElementType::ScrollArea,
        Tag::Code => ElementType::Code,
        Tag::Svg => ElementType::Svg,
        Tag::Image => ElementType::Image,
        Tag::TabsRoot => ElementType::TabsRoot,
    }
}

fn direction(value: &str) -> FlexDirection {
    match value {
        "row" => FlexDirection::Row,
        "row-reverse" => FlexDirection::RowReverse,
        "column-reverse" => FlexDirection::ColumnReverse,
        _ => FlexDirection::Column,
    }
}

/// 解析失败一律退化成 0/默认值：界面少一点样式好过整个渲染崩掉。
fn number(value: &str) -> u32 {
    value.trim().parse().unwrap_or(0)
}

fn decimal(value: &str) -> f32 {
    value.trim().parse().unwrap_or(0.0)
}

fn integer(value: &str) -> i32 {
    value.trim().parse().unwrap_or(0)
}
