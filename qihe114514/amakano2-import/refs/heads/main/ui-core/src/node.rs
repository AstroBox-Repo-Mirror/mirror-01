//! 与宿主无关的抽象节点树。
//!
//! 页面只负责产出 [`Node`]，由 `ui::convert` 把它翻译成宿主的 `ui_v3::Element`。
//! 这层间接换来三件事：
//! 1. 页面逻辑是纯数据，**可以脱离 wasm 宿主做单元测试**；
//! 2. 能导出 JSON，交给 `tools/ui-preview.mjs` 在浏览器里渲染截图，装到设备前先肉眼验收；
//! 3. 以后宿主 UI 接口再迭代，只需要改一个转换函数。

use serde_json::{Map, Value, json};

/// 节点的语义标签。取值刻意和宿主 `ui-v3` 的元素类型一一对应，
/// 但**不依赖宿主类型**，所以本模块可以编译进普通测试。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tag {
    Div,
    P,
    Span,
    Button,
    Badge,
    Progress,
    Switch,
    Slider,
    Grid,
    Scroll,
    Code,
    Svg,
    Divider,
    Icon,
    /// 真正的图片元素（`ui-v3` 的 `IMAGE`），内容就是图片地址（data URI 真机验证过能显示）。
    ///
    /// 现在界面里没用它（壁纸/图标那套已删）。留着的理由：它是本宿主上**唯一**能显示位图的手段
    /// —— `prop("background-image", ...)` 是空操作。别指望它铺满整页：它既不认 `height: 100%`
    /// （父容器高度由内容撑开时解析不出来），也不按 `bottom` 拉伸（按自身尺寸渲染）。
    Image,
    TabsRoot,
}

impl Tag {
    /// 导出给预览器用的名字（也是调试时看得懂的名字）。
    pub const fn wire(self) -> &'static str {
        match self {
            Tag::Div => "div",
            Tag::P => "p",
            Tag::Span => "span",
            Tag::Button => "button",
            Tag::Badge => "badge",
            Tag::Progress => "progress",
            Tag::Switch => "switch",
            Tag::Slider => "slider",
            Tag::Grid => "grid",
            Tag::Scroll => "scroll",
            Tag::Code => "code",
            Tag::Svg => "svg",
            Tag::Divider => "divider",
            Tag::Icon => "icon",
            Tag::Image => "image",
            Tag::TabsRoot => "tabs-root",
        }
    }
}

/// 抽象节点。`props` 用有序键值对而不是 HashMap：
/// 属性顺序会影响转换时「先设基础值、再用逃生舱覆盖」的效果，顺序必须稳定。
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub tag: Tag,
    pub text: Option<String>,
    pub props: Vec<(String, String)>,
    pub children: Vec<Node>,
}

impl Node {
    pub fn new(tag: Tag) -> Self {
        Self { tag, text: None, props: Vec::new(), children: Vec::new() }
    }

    pub fn text(tag: Tag, value: impl Into<String>) -> Self {
        Self { tag, text: Some(value.into()), props: Vec::new(), children: Vec::new() }
    }

    /// 设置属性；同名属性会被覆盖（保留原有位置，避免顺序漂移）。
    pub fn prop(mut self, key: &str, value: impl Into<String>) -> Self {
        let value = value.into();
        match self.props.iter_mut().find(|(name, _)| name == key) {
            Some(slot) => slot.1 = value,
            None => self.props.push((key.to_string(), value)),
        }
        self
    }

    pub fn has(&self, key: &str) -> bool {
        self.props.iter().any(|(name, _)| name == key)
    }

    /// 设置内容。图片元素（`Tag::Image`）用它承载 `data:` URI ——
    /// 宿主的 `prop()` 逃生舱对 `background-image` 是空操作，图片只能走元素内容。
    pub fn content(mut self, value: impl Into<String>) -> Self {
        self.text = Some(value.into());
        self
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.props.iter().find(|(name, _)| name == key).map(|(_, value)| value.as_str())
    }

    pub fn child(mut self, child: Node) -> Self {
        self.children.push(child);
        self
    }

    pub fn children(mut self, list: impl IntoIterator<Item = Node>) -> Self {
        self.children.extend(list);
        self
    }

    /// 仅当条件成立时追加子节点，省掉调用处的 `if`。
    pub fn child_if(self, condition: bool, child: Node) -> Self {
        if condition { self.child(child) } else { self }
    }

    // ---- 布局 ----

    pub fn flex(self, direction: &str) -> Self {
        self.prop("flex", direction)
    }

    pub fn row(self) -> Self {
        self.flex("row")
    }

    pub fn column(self) -> Self {
        self.flex("column")
    }

    /// 网格列定义。
    ///
    /// ⚠️ **真机实测宿主不认这个属性**：`GRID` 元素会退化成「每格一行」。
    /// 要等宽并排请用 `glass::kv_grid` / `glass::tile_row`（flex + grow）。
    /// 保留这个方法只是为了移动端/未来的宿主可能支持。
    pub fn grid(self, columns: &str) -> Self {
        self.prop("grid", columns)
    }

    pub fn gap(self, value: u32) -> Self {
        self.prop("gap", value.to_string())
    }

    pub fn align(self, value: &str) -> Self {
        self.prop("align", value)
    }

    pub fn justify(self, value: &str) -> Self {
        self.prop("justify", value)
    }

    pub fn grow(self, value: f32) -> Self {
        self.prop("grow", format!("{value}"))
    }

    /// 收缩系数。按钮、徽章这类不该被压窄的元素要显式写成 `0`，
    /// 否则 flex 会把它们压窄、文字折成两行。
    pub fn shrink(self, value: f32) -> Self {
        self.prop("shrink", format!("{value}"))
    }

    // ---- 盒模型 ----

    pub fn pad(self, value: u32) -> Self {
        self.prop("padding", value.to_string())
    }

    pub fn pad_x(self, value: u32) -> Self {
        self.prop("pl", value.to_string()).prop("pr", value.to_string())
    }

    pub fn pad_y(self, value: u32) -> Self {
        self.prop("pt", value.to_string()).prop("pb", value.to_string())
    }

    pub fn pad_t(self, value: u32) -> Self {
        self.prop("pt", value.to_string())
    }

    pub fn pad_b(self, value: u32) -> Self {
        self.prop("pb", value.to_string())
    }

    pub fn mt(self, value: u32) -> Self {
        self.prop("mt", value.to_string())
    }

    pub fn mb(self, value: u32) -> Self {
        self.prop("mb", value.to_string())
    }

    pub fn ml(self, value: u32) -> Self {
        self.prop("ml", value.to_string())
    }

    pub fn mr(self, value: u32) -> Self {
        self.prop("mr", value.to_string())
    }

    pub fn m(self, value: u32) -> Self {
        self.prop("margin", value.to_string())
    }

    // ---- 尺寸 ----

    pub fn w(self, value: u32) -> Self {
        self.prop("w", value.to_string())
    }

    pub fn h(self, value: u32) -> Self {
        self.prop("h", value.to_string())
    }

    pub fn full(self) -> Self {
        self.prop("w", "full")
    }

    /// 高度铺满父容器（绝对定位层铺满时配 `full()` 一起用）。
    pub fn height_full(self) -> Self {
        self.prop("h", "full")
    }

    pub fn maxw(self, value: u32) -> Self {
        self.prop("maxw", value.to_string())
    }

    pub fn minw(self, value: u32) -> Self {
        self.prop("minw", value.to_string())
    }

    pub fn minh(self, value: u32) -> Self {
        self.prop("minh", value.to_string())
    }

    pub fn maxh(self, value: u32) -> Self {
        self.prop("maxh", value.to_string())
    }

    /// 字号（px）。
    pub fn size(self, value: u32) -> Self {
        self.prop("size", value.to_string())
    }

    pub fn weight(self, value: u32) -> Self {
        self.prop("weight", value.to_string())
    }

    pub fn ls(self, value: &str) -> Self {
        self.prop("letter-spacing", value)
    }

    pub fn lh(self, value: &str) -> Self {
        self.prop("line-height", value)
    }

    // ---- 视觉 ----

    pub fn radius(self, value: u32) -> Self {
        self.prop("radius", value.to_string())
    }

    pub fn bg(self, color: &str) -> Self {
        self.prop("bg", color)
    }

    pub fn fg(self, color: &str) -> Self {
        self.prop("fg", color)
    }

    pub fn border(self, width: u32, color: &str) -> Self {
        self.prop("border", format!("{width} {color}"))
    }

    pub fn opacity(self, value: f32) -> Self {
        self.prop("opacity", format!("{value}"))
    }

    pub fn backdrop(self, value: &str) -> Self {
        self.prop("backdrop", value)
    }

    pub fn filter(self, value: &str) -> Self {
        self.prop("filter", value)
    }

    pub fn shadow(self, value: &str) -> Self {
        self.prop("shadow", value)
    }

    pub fn inset(self, value: &str) -> Self {
        self.prop("inset", value)
    }

    pub fn transform(self, value: &str) -> Self {
        self.prop("transform", value)
    }

    pub fn transition(self, value: &str) -> Self {
        self.prop("transition", value)
    }

    pub fn pos(self, value: &str) -> Self {
        self.prop("pos", value)
    }

    pub fn z(self, value: i32) -> Self {
        self.prop("z", value.to_string())
    }

    /// 相对定位：给绝对定位的子元素当基准。
    pub fn relative(self) -> Self {
        self.pos("relative")
    }

    /// 绝对定位。配 `top(0)` / `left(0)` + `full()` / `height_full()` 即可铺满父容器 ——
    /// 自带壁纸就是这么铺的（宿主的 `prop()` 逃生舱是空操作，只能靠类型化定位方法）。
    pub fn absolute(self) -> Self {
        self.pos("absolute")
    }

    pub fn top(self, value: u32) -> Self {
        self.prop("top", value.to_string())
    }

    pub fn left(self, value: u32) -> Self {
        self.prop("left", value.to_string())
    }

    /// 底边偏移。配 `top(0)` 一起用，高度由布局算出来 ——
    /// **不要用 `height_full()`**：父容器高度是内容撑开的时候，宿主的 `IMAGE` 拿不到高度（会变成 0）。
    pub fn bottom(self, value: u32) -> Self {
        self.prop("bottom", value.to_string())
    }

    /// 右边偏移。配 `left(0)` 一起用，宽度由布局算出来。
    pub fn right(self, value: u32) -> Self {
        self.prop("right", value.to_string())
    }

    pub fn without_default_styles(self) -> Self {
        self.prop("nodflt", "1")
    }

    pub fn scroll(self, axis: &str) -> Self {
        self.prop("scroll", axis)
    }

    /// 进度条填充百分比。
    pub fn value(self, percent: u32) -> Self {
        self.prop("value", percent.min(100).to_string())
    }

    // ---- 交互 ----

    /// 点击回调。宿主同时派发 Click 与 PointerUp，挂两个能明显提升响应速度。
    pub fn click(self, action: &str) -> Self {
        self.prop("on.click", action).prop("on.pointerup", action)
    }

    pub fn enter(self, action: &str) -> Self {
        self.prop("on.enter", action)
    }

    pub fn leave(self, action: &str) -> Self {
        self.prop("on.leave", action)
    }

    /// 悬停高亮：进出各发一次，页面据此重绘。
    pub fn hover(self, action: &str) -> Self {
        self.enter(action).leave(action)
    }

    /// 按下（PointerDown）回调。
    /// 宿主的 Click 要等抬起才来，按下态的即时反馈只能靠它。
    pub fn press(self, action: &str) -> Self {
        self.prop("on.press", action)
    }

    pub fn disabled(self) -> Self {
        self.prop("disabled", "1")
    }

    // ---- 导出与遍历 ----

    /// 导出成 `tools/ui-preview.mjs` 认识的 JSON。
    pub fn to_json(&self) -> Value {
        let mut props = Map::new();
        let mut handlers = Map::new();
        for (key, value) in &self.props {
            if let Some(event) = key.strip_prefix("on.") {
                handlers.insert(event.to_string(), json!(value));
            } else {
                props.insert(key.clone(), json!(value));
            }
        }
        if !handlers.is_empty() {
            props.insert("on".into(), Value::Object(handlers));
        }

        let mut object = Map::new();
        object.insert("tag".into(), json!(self.tag.wire()));
        object.insert("text".into(), self.text.clone().map_or(Value::Null, Value::String));
        object.insert("props".into(), Value::Object(props));
        object.insert(
            "children".into(),
            Value::Array(self.children.iter().map(Node::to_json).collect()),
        );
        Value::Object(object)
    }

    /// 深度优先遍历，供测试断言用。
    pub fn walk(&self, visit: &mut impl FnMut(&Node)) {
        visit(self);
        for child in &self.children {
            child.walk(visit);
        }
    }

    /// 收集所有满足条件的节点。
    pub fn find(&self, predicate: impl Fn(&Node) -> bool) -> Vec<&Node> {
        let mut found = Vec::new();
        self.collect(&predicate, &mut found);
        found
    }

    /// 显式带生命周期参数的递归，别用 `walk` 加闭包捕获：
    /// `walk` 的闭包参数是匿名生命周期，往里塞 `&'a Node` 会被可变引用的不变性挡下来。
    fn collect<'a>(&'a self, predicate: &impl Fn(&Node) -> bool, found: &mut Vec<&'a Node>) {
        if predicate(self) {
            found.push(self);
        }
        for child in &self.children {
            child.collect(predicate, found);
        }
    }

    /// 所有可见文本，按渲染顺序。
    pub fn texts(&self) -> Vec<&str> {
        self.find(|node| node.text.is_some()).iter().filter_map(|node| node.text.as_deref()).collect()
    }
}

/// 一行文本。
pub fn label(value: impl Into<String>, size: u32, color: &str) -> Node {
    Node::text(Tag::P, value).size(size).fg(color).prop("lh", "1.45")
}

/// 小号胶囊徽章。`shrink(0)` 是必要的：flex 里徽章被压窄会让文字折行。
pub fn badge(value: impl Into<String>, fg: &str, bg: &str) -> Node {
    Node::text(Tag::Badge, value).size(11).fg(fg).bg(bg).pad_x(8).pad_y(3).radius(999).shrink(0.0)
}

/// 分段进度条：把 `percent` 摊到 `segments` 个等宽小块上。
///
/// 刻意不用「一个轨道 + 一个百分比宽度的填充」：宿主 `ui-v3` 的 `width` 只收 u32 像素，
/// 百分比宽度得走 `prop("width","62%")` 这个逃生舱，万一宿主不认就整条塌掉。
/// 换成「每段 `flex-grow(1)` + 逐段改色」，渲染结果与宿主能力无关，只会粗细不同。
pub fn segmented_bar(percent: u32, segments: u32, track: &str, colors: &[String]) -> Node {
    let segments = segments.max(1);
    let filled = ((percent.min(100) as u64 * segments as u64) / 100) as u32;
    let mut bar = Node::new(Tag::Div).full().row().gap(2);
    for index in 0..segments {
        let color = if index < filled {
            // 逐段在两色之间插值，让「一条渐变」跨在多个独立元素上。
            let t = index as f32 / (segments.saturating_sub(1)).max(1) as f32;
            colors
                .get(0)
                .zip(colors.get(1))
                .map_or_else(|| track.to_string(), |(a, b)| crate::theme::lerp_hex(a, b, t))
        } else {
            track.to_string()
        };
        bar = bar.child(
            Node::new(Tag::Div).grow(1.0).h(8).radius(4).bg(&color).transition(crate::theme::TRANSITION),
        );
    }
    bar
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prop_overrides_in_place_and_keeps_order() {
        let node = Node::new(Tag::Div).pad(8).gap(4).pad(12);
        assert_eq!(node.props[0], ("padding".into(), "12".into()));
        assert_eq!(node.props[1].0, "gap");
        assert_eq!(node.props.len(), 2);
    }

    #[test]
    fn json_folds_event_props_into_object() {
        let value = Node::text(Tag::Button, "同步").click("sync:3").to_json();
        assert_eq!(value["props"]["on"]["click"], "sync:3");
        assert_eq!(value["props"]["on"]["pointerup"], "sync:3");
        assert_eq!(value["text"], "同步");
        assert_eq!(value["tag"], "button");
    }

    #[test]
    fn texts_walks_in_render_order() {
        let tree = Node::new(Tag::Div)
            .child(label("甲", 12, "#fff"))
            .child(Node::new(Tag::Div).child(label("乙", 12, "#fff")));
        assert_eq!(tree.texts(), vec!["甲", "乙"]);
    }
}
