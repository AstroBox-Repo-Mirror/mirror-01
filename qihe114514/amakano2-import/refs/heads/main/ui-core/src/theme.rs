//! 设计 token。
//!
//! 三件事：石墨灰的实色层级、文字层次、按钮几何。
//!
//! **这一版的主色**：面板走中性石墨灰（带一点点暖），品牌粉
//! （`ACCENT` / `ACCENT_DEEP`）**只留给主操作与关键数字**，不再到处上色 ——
//! 上一版灰蓝底 + 满屏彩色徽章，看久了像「仪表盘」而不是「工具」。
//!
//! **能生效的只有类型化方法**：真机实测 `prop()` 逃生舱对 `background` /
//! `background-image` / `box-shadow` 全是空操作，只有 `bg()` / `radius()` /
//! `border()` / `padding*()` 这类类型化方法真的下发得下去。所以：
//! 渐变、外投影、内高光、字重这些都做不出来（详见 docs/插件开发注意事项.md 6.2），
//! 本文件里的 `shadow` / `inset` / `weight` / `ls` 一律只当**预览用的装饰**，
//! 不能用来承担视觉职责 —— 界面的层次只能靠**实色深浅 + 圆角 + 留白**做出来。
//!
//! 玻璃按钮那套（半透明染色 + 亮边 + 背景模糊 + 按下缩放/光流）在这宿主上做不出来，
//! 已整批删除；按钮就是最普通的那种 —— 实色圆角胶囊 + 居中文字，按下换深一档底色。

// ---- 页面底色：**不画** ----
//
// 用户要求「不要自绘页面背景，保持空白，用 astrobox 的背景就行」，
// 所以页面外壳不挂任何填色，卡片之间透出来的就是宿主自己的背景。
// 注意：**面板自己的实色底不受影响** —— 宿主背景可能是彩色的，
// 白字直接铺在上面读不了，所以内容一律待在实色面板里。

// ---- 面板层级（实色，不透明）----
//
// 层次只靠**实色深浅**做，不靠描边堆：一级面板给一层极淡的描边帮助在彩色壁纸上
// 划出边界，二级块（列表行、内嵌小块）**不描边**，只靠比一级面板亮一点点的底色分开。

/// 一级面板（页面卡片、区块卡片）。
pub const SURFACE: &str = "#191B1F";
/// 二级块（卡片内的列表行、嵌套小块）。
pub const SURFACE_SOFT: &str = "#22252A";
/// 三级块（悬停 / 选中 / 需要再抬一层的块）。
pub const SURFACE_STRONG: &str = "#2B2F35";
/// 一级面板的描边：只用来在彩色壁纸上划边界，淡到几乎看不见。
pub const STROKE: &str = "rgba(255,255,255,0.07)";
/// 更弱的描边（内层划分、分隔线）。
pub const STROKE_SOFT: &str = "rgba(255,255,255,0.045)";

// ---- 文字 ----
//
// 三级就够：主文（标题、数值）、次文（正文、行内值）、弱文（元信息、说明）。
// **区块标签**用 `TEXT_DIM` 而不是更亮 —— 小字再亮就会跟标题抢注意力。

pub const TEXT_MAIN: &str = "#F5F6F8";
pub const TEXT_SUB: &str = "rgba(230,233,238,0.68)";
pub const TEXT_DIM: &str = "rgba(208,213,221,0.44)";
pub const TEXT_FAINT: &str = "rgba(208,213,221,0.28)";

// ---- 状态色 ----
//
// 比上一版压暗一档：状态是**辅助信息**，不该比主操作还抢眼。

pub const OK: &str = "#5BD08B";
pub const OK_BG: &str = "rgba(91,208,139,0.14)";
pub const WARN: &str = "#E8B44A";
pub const WARN_BG: &str = "rgba(232,180,74,0.15)";
pub const BAD: &str = "#E8756C";
pub const BAD_BG: &str = "rgba(232,117,108,0.15)";
pub const INFO_BG: &str = "rgba(160,170,185,0.14)";

// ---- 品牌与线路色 ----
//
// 品牌粉只用于**主操作按钮**和**关键数字**（进度百分比、总阅读时长）。
// 线路色只用于章节行的序号砖，别的地方不上色。

/// 游戏图标的主粉色。
pub const ACCENT: &str = "#E4739F";
/// 主操作按钮底色（比 `ACCENT` 深一档，白字才压得住）。
pub const ACCENT_DEEP: &str = "#C2557F";
/// 进度条填充的亮端。
pub const ACCENT_LIGHT: &str = "#F08CB4";
pub const TRACK: &str = "rgba(255,255,255,0.09)";

/// 线路配色：共通线 / 千岁 / 玲 / 结灯 / 番外。
pub const LINE_COMMON: &str = "#8FA3BF";
pub const LINE_CHITOSE: &str = "#E4739F";
pub const LINE_REI: &str = "#6EA8FE";
pub const LINE_YUHI: &str = "#B98CFF";
pub const LINE_EXTRA: &str = "#E0B25C";

/// 按章节标题推断线路，返回线路名。
/// 章节标题形如「千岁线2·初恋」「番外·结灯」「共通线1·归乡」。
pub fn chapter_line(title: &str) -> (&'static str, &'static str) {
    let name = if title.starts_with("共通") {
        ("共通线", LINE_COMMON)
    } else if title.starts_with("千岁") {
        ("千岁线", LINE_CHITOSE)
    } else if title.starts_with("玲") {
        ("玲线", LINE_REI)
    } else if title.starts_with("结灯") {
        ("结灯线", LINE_YUHI)
    } else if title.starts_with("番外") {
        ("番外", LINE_EXTRA)
    } else {
        ("其他", LINE_COMMON)
    };
    name
}

/// 线路筛选用的固定顺序。
pub const LINES: [&str; 5] = ["共通线", "千岁线", "玲线", "结灯线", "番外"];

// ---- 几何 ----

/// 页面外壳的**上下**内边距。
///
/// 左右一个都不下发：宿主已有自己的左右安全区，插件再叠一层，内容会窄掉一大块
/// （实测 460px 窗口下插件拿到的画布宽从 350px 变成 386px）。
pub const PAGE_PAD_Y: u32 = 18;
/// 窄窗里的分段控件（章节库的线路筛选）每一项的水平内边距。
///
/// 默认档 10 会让 6 项（全部 + 5 条线路）在 400px 窗口里超宽，得靠滚动才看得全；
/// 收到 6 之后一屏放得下，滚动只作为兜底。
pub const SEGMENT_NARROW_PAD_X: u32 = 6;
/// 一级卡片的圆角。
pub const CARD_RADIUS: u32 = 16;
/// 二级块的圆角。
pub const ROW_RADIUS: u32 = 12;
/// 一级卡片的内边距。
pub const CARD_PAD: u32 = 14;
pub const GAP_XS: u32 = 6;
pub const GAP_SM: u32 = 8;
pub const GAP: u32 = 12;
/// 卡片与卡片之间的间距。
pub const GAP_LG: u32 = 16;

// ---- 按钮：最普通的那种 ----
//
// 一块实色圆角胶囊 + 居中文字，按下换深一档底色。没有模糊、没有渐变、没有描边、
// 没有阴影、没有缩放动效，按钮内部也没有任何子元素。

/// 主按钮高度。
pub const BUTTON_HEIGHT: u32 = 48;
/// 水平内边距。
pub const BUTTON_PAD_X: u32 = 16;
/// 标签字号。
pub const BUTTON_FONT: u32 = 15;
/// 胶囊半径 = 高度的一半。
pub const BUTTON_RADIUS: u32 = BUTTON_HEIGHT / 2;
/// 垂直内边距：让「字号 + 上下内边距」落在 48px 上。
/// 元素树里给按钮设死高度后文字不会垂直居中，用内边距逼近更稳。
pub const BUTTON_PAD_Y: u32 = 13;

/// 主按钮：品牌粉实底 + 白字。**全界面只有主操作用它**。
pub const BUTTON_PRIMARY: &str = "#C2557F";
pub const BUTTON_PRIMARY_PRESSED: &str = "#A8456A";
pub const BUTTON_PRIMARY_TEXT: &str = "#FFFFFF";
/// 次要按钮：石墨实底 + 浅字。
pub const BUTTON_GHOST: &str = "#262A30";
pub const BUTTON_GHOST_PRESSED: &str = "#32373E";
pub const BUTTON_GHOST_TEXT: &str = "#E7E9EC";
/// 危险按钮：压暗的红实底 + 白字。
pub const BUTTON_DANGER: &str = "#B4463C";
pub const BUTTON_DANGER_PRESSED: &str = "#943A32";
pub const BUTTON_DANGER_TEXT: &str = "#FFFFFF";

/// 完全透明：元素树里背景必须显式给值，不能用关键字。
pub const TRANSPARENT: &str = "rgba(0,0,0,0)";
/// 按下态的兜底超时（毫秒）。
/// 元素树里收不到「在按钮外松开」，所以除了「动作执行即复位」之外再用时间兜底。
pub const PRESS_TIMEOUT_MS: u128 = 600;

// ---- 行内小按钮 ----

/// 行内小按钮高度。
/// 48 是触摸界面上常规的最小触摸目标；插件跑在桌面指针环境，且这些按钮出现在密集的
/// 列表行里，所以行内档按密度压到 30 —— 这是**本项目有意的取舍**。
pub const CHIP_HEIGHT: u32 = 30;
pub const CHIP_PAD_X: u32 = 11;
pub const CHIP_PAD_Y: u32 = 6;
pub const CHIP_FONT: u32 = 12;
pub const CHIP_RADIUS: u32 = CHIP_HEIGHT / 2;

// ---- 切页动效：**已整块移除** ----
//
// 「两帧 + transition」在这宿主上是跳变而不是动画（元素树每次渲染重建子树，
// transition 追不到同一个节点的上一个值），真机实测很卡，已撤掉。
// 外壳上不许再出现 `transform` / `transition` / `opacity`，单测盯着这一条。
// 以后要做动效只能指望宿主自己的动画 API（是否可用尚未验证）。

// ---- 字号 ----

/// 主数字（进度百分比、总阅读时长）。
pub const SIZE_HERO: u32 = 34;
/// 页面主标题 / 卡片主标题。
pub const SIZE_TITLE: u32 = 18;
/// 卡片内的块标题。
pub const SIZE_H2: u32 = 14;
/// 正文。
pub const SIZE_BODY: u32 = 13;
/// 行内值 / 次要正文。
pub const SIZE_SMALL: u32 = 12;
/// 元信息、说明、区块标签。
pub const SIZE_TINY: u32 = 11;

// ---- 动效 ----

/// 通用过渡：颜色、描边色与透明度，220ms 缓出（临界阻尼，无回弹）。
///
/// 只给**会换底色的容器**用（面板、分段控件、列表行的悬停）；按钮不挂 ——
/// 它按下只换一次底色，没有需要插值的中间态。
///
/// 注：宿主每次渲染都重建元素树，跨帧的过渡实际上补不出来，
/// 所以这条只当**预览里的礼貌**，界面不依赖它表达任何状态。
pub const TRANSITION: &str = "background 220ms cubic-bezier(0.32, 0.72, 0, 1), border-color 220ms cubic-bezier(0.32, 0.72, 0, 1), opacity 220ms cubic-bezier(0.32, 0.72, 0, 1)";

// ---- 颜色工具 ----

/// 解析 `#RRGGBB`（也接受 `#RGB`），失败返回 `None`。
pub fn parse_hex(value: &str) -> Option<(u8, u8, u8)> {
    let body = value.strip_prefix('#')?;
    let expand = |c: char| u8::from_str_radix(&format!("{c}{c}"), 16).ok();
    match body.len() {
        3 => {
            let mut chars = body.chars();
            Some((expand(chars.next()?)?, expand(chars.next()?)?, expand(chars.next()?)?))
        }
        6 => Some((
            u8::from_str_radix(&body[0..2], 16).ok()?,
            u8::from_str_radix(&body[2..4], 16).ok()?,
            u8::from_str_radix(&body[4..6], 16).ok()?,
        )),
        _ => None,
    }
}

/// 在两色之间线性插值（`t` 0→a，1→b）。任一端解析失败时返回 `a` 原样。
pub fn lerp_hex(a: &str, b: &str, t: f32) -> String {
    let Some((ar, ag, ab)) = parse_hex(a) else { return a.to_string() };
    let Some((br, bg, bb)) = parse_hex(b) else { return a.to_string() };
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round().clamp(0.0, 255.0) as u8;
    format!("#{:02X}{:02X}{:02X}", mix(ar, br), mix(ag, bg), mix(ab, bb))
}

// ---- 时间 ----

/// 毫秒时间戳 → `MM-DD HH:MM`；0 或非法值返回「时间未知」。
///
/// 用纯整数算日历（`civil_from_days`），**不依赖任何宿主接口**，
/// 所以预览（`demo()`）与真机看到的字符串一致。
///
/// 按 **UTC** 显示：插件是宿主的 wasm 沙箱里的组件，本地时区要靠
/// `os::timezone-offset-minutes` 再问一次宿主，为了一个展示用的时间不值当。
/// 小时数可能差几小时，但**存档里的 `savedAt` 原值不会被改写**。
pub fn format_time_ms(value: u64) -> String {
    if value == 0 {
        return "时间未知".into();
    }
    let seconds = value / 1000;
    let (_, month, day) = civil_from_days((seconds / 86_400) as i64);
    let rest = seconds % 86_400;
    format!("{month:02}-{day:02} {:02}:{:02}", rest / 3600, (rest % 3600) / 60)
}

/// 天数（1970-01-01 起）→ 年月日。Howard Hinnant 的 `civil_from_days`，纯整数运算。
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// 线性插值出来的十六进制颜色是不是合法的 `#RRGGBB`。
/// 给测试用：进度条那些逐段插值的颜色不许出现非法值。
#[cfg(test)]
fn is_hex6(value: &str) -> bool {
    value.len() == 7 && parse_hex(value).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_hex_interpolates_and_degrades_gracefully() {
        assert_eq!(lerp_hex("#000000", "#FFFFFF", 0.5), "#808080");
        assert_eq!(lerp_hex("#000000", "#FFFFFF", 0.0), "#000000");
        assert_eq!(lerp_hex("#000000", "#FFFFFF", 1.0), "#FFFFFF");
        // 越界的 t 会被夹住，非法颜色原样返回，不 panic。
        assert_eq!(lerp_hex("#000000", "#FFFFFF", 5.0), "#FFFFFF");
        assert_eq!(lerp_hex("nope", "#FFFFFF", 0.5), "nope");
    }

    #[test]
    fn parse_hex_handles_short_and_long_forms() {
        assert_eq!(parse_hex("#fff"), Some((255, 255, 255)));
        assert_eq!(parse_hex("#C2557F"), Some((194, 85, 127)));
        assert_eq!(parse_hex("C2557F"), None);
        assert_eq!(parse_hex("#12345"), None);
    }

    #[test]
    fn time_labels_are_utc_and_never_panic() {
        assert_eq!(format_time_ms(0), "时间未知");
        assert_eq!(format_time_ms(1_000), "01-01 00:00");
        assert_eq!(format_time_ms(3_660_000), "01-01 01:01");
        // 2000-01-01T00:00:00Z
        assert_eq!(format_time_ms(946_684_800_000), "01-01 00:00");
        // 2026-09-13T00:00:00Z（闰年之后的日期换算要与日历一致）
        assert_eq!(format_time_ms(1_789_257_600_000), "09-13 00:00");
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29), "2000 是闰年");
        assert_eq!(civil_from_days(11_017), (2000, 3, 1));
    }

    #[test]
    fn chapter_line_maps_every_route() {
        assert_eq!(chapter_line("共通线1·归乡").0, "共通线");
        assert_eq!(chapter_line("千岁线2·初恋").0, "千岁线");
        assert_eq!(chapter_line("玲线3·相伴").0, "玲线");
        assert_eq!(chapter_line("结灯线1·序章").0, "结灯线");
        assert_eq!(chapter_line("番外·结灯").0, "番外");
        assert_eq!(chapter_line("未知").0, "其他");
    }

    /// 面板一律**不透明实色**：层次靠实色深浅做，不靠模糊/渐变/阴影
    /// （后三者在真机上都是空操作，见文件头注释）。
    #[test]
    fn surfaces_are_three_opaque_steps() {
        let steps = [("SURFACE", SURFACE), ("SURFACE_SOFT", SURFACE_SOFT), ("SURFACE_STRONG", SURFACE_STRONG)];
        for (name, color) in steps {
            assert!(color.starts_with('#'), "{name} 应该是不透明实色，实得 {color}");
            assert_eq!(color.len(), 7, "{name} 应该是 6 位十六进制");
        }
        // 三级要真的**一级比一级亮**，否则「抬一层」的语义就没了。
        let luma = |color: &str| {
            let (r, g, b) = parse_hex(color).expect("面板色必须是合法十六进制");
            u32::from(r) + u32::from(g) + u32::from(b)
        };
        assert!(luma(SURFACE) < luma(SURFACE_SOFT), "二级块要比一级面板亮");
        assert!(luma(SURFACE_SOFT) < luma(SURFACE_STRONG), "三级块要比二级块亮");
        assert!(STROKE.starts_with("rgba("), "描边可以半透明");
    }

    /// 文字四级要**一级比一级淡** —— 层次全靠这一条，写反了界面就糊了。
    #[test]
    fn text_steps_get_dimmer_in_order() {
        assert!(TEXT_MAIN.starts_with('#'), "主文是实色");
        for (name, value) in [("TEXT_SUB", TEXT_SUB), ("TEXT_DIM", TEXT_DIM), ("TEXT_FAINT", TEXT_FAINT)] {
            assert!(value.starts_with("rgba("), "{name} 用 alpha 表示深浅");
        }
        let alpha = |value: &str| {
            value.rsplit(',').next().unwrap().trim_end_matches(')').parse::<f32>().expect("alpha 要能解析")
        };
        assert!(alpha(TEXT_SUB) > alpha(TEXT_DIM), "次文要比弱文亮");
        assert!(alpha(TEXT_DIM) > alpha(TEXT_FAINT), "弱文要比更弱文亮");
    }

    /// 按钮配色只有实色十六进制：每档一份静止 + 一份按下，两支配色必须不同。
    #[test]
    fn button_tokens_are_solid_hex_with_a_distinct_pressed_fill() {
        let pairs = [
            ("PRIMARY", BUTTON_PRIMARY, BUTTON_PRIMARY_PRESSED),
            ("GHOST", BUTTON_GHOST, BUTTON_GHOST_PRESSED),
            ("DANGER", BUTTON_DANGER, BUTTON_DANGER_PRESSED),
        ];
        for (name, idle, pressed) in pairs {
            for (state, color) in [("静止档", idle), ("按下档", pressed)] {
                assert!(
                    is_hex6(color),
                    "BUTTON_{name} 的{state}必须是不透明实色（6 位十六进制），实得 {color}"
                );
            }
            assert_ne!(idle, pressed, "BUTTON_{name} 按下要换一支底色，否则按下去看不出反馈");
        }

        for (name, color) in [
            ("BUTTON_PRIMARY_TEXT", BUTTON_PRIMARY_TEXT),
            ("BUTTON_GHOST_TEXT", BUTTON_GHOST_TEXT),
            ("BUTTON_DANGER_TEXT", BUTTON_DANGER_TEXT),
        ] {
            assert!(is_hex6(color), "{name} 应该是 6 位十六进制，实得 {color}");
        }

        // 深色页面上的直觉：粉/红按钮按下变深；石墨底那一支反过来，按下提亮才看得见。
        assert!(luma(BUTTON_PRIMARY_PRESSED) < luma(BUTTON_PRIMARY), "粉色主按钮按下要变深");
        assert!(luma(BUTTON_DANGER_PRESSED) < luma(BUTTON_DANGER), "红色危险按钮按下要变深");
        assert!(luma(BUTTON_GHOST_PRESSED) > luma(BUTTON_GHOST), "石墨底按下要提亮");
    }

    /// 主操作底色必须是品牌粉那一支 —— 「品牌色只给主操作」是这一版的视觉规矩。
    #[test]
    fn primary_button_carries_the_brand_color() {
        assert_eq!(BUTTON_PRIMARY, ACCENT_DEEP, "主按钮底色必须来自品牌色");
        assert_eq!(BUTTON_PRIMARY_TEXT, "#FFFFFF");
    }

    /// 三通道之和，只用来比较明暗。
    fn luma(color: &str) -> u32 {
        let (r, g, b) = parse_hex(color).expect("测试里的颜色必须是合法十六进制");
        u32::from(r) + u32::from(g) + u32::from(b)
    }
}
