//! 失败原因：**机器可判别的错误码** + 在这里**唯一一处**生成的人话。
//!
//! 为什么要有码：以前所有失败都塌缩成一句中文状态行 —— 超时、断链、版本不匹配、
//! 空间不足、协议不一致**共用同一条通道**。界面看不出区别，插件也没法按类别决定
//! 「该重试 / 该重连 / 该让用户腾空间」。现在插件只负责产出「码 + 细节原文」，
//! 结论与「怎么办」都在这里生成（与 ui-core 其它文案同一条规矩：派生结果只允许一处实现）。
//!
//! ⚠️ **认不出来的码一律 [`ErrorCode::Unknown`]，绝不猜成某个具体原因** ——
//! 猜错会给出错误的「怎么办」，比说不知道更糟。

/// 一次失败的类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// 链路断了：蓝牙断连、宿主报发送失败、手环重启。
    Link,
    /// 对端没响应（超时）。
    Timeout,
    /// 协议/版本对不上：能力表缺项、包格式不对、请求无效。
    Protocol,
    /// 存储空间不足（手环写盘 / 登记注册表失败多半是它）。
    Space,
    /// 宿主或手环明确拒绝。
    Rejected,
    /// 章节包本身有问题：读不出、清单无效、路径不对。
    BadPack,
    /// 传输窗口协商不一致（两侧对「允许多少片在途」理解不同）。
    Window,
    /// 手环重启过 —— 断点还在，能接着传。
    DeviceReboot,
    /// 上一次操作还没结束。
    Busy,
    /// 兜底。
    Unknown,
}

impl ErrorCode {
    /// 从协议里的 `code`（插件产）或 `error`（手环产）字段解析。
    ///
    /// 手环那侧的取值来自 `pack-importer.js` 的 `errorText` 表与新增的错误码；
    /// 插件那侧用 `link` / `timeout` / `protocol` / `rejected` / `busy` / `reboot`。
    pub fn from_wire(raw: &str) -> Self {
        match raw {
            // 手环侧
            "base64" | "size" | "manifest" | "path" => ErrorCode::BadPack,
            "write" | "registry" => ErrorCode::Space,
            "request" => ErrorCode::Protocol,
            "window" | "resume-not-at-file-start" => ErrorCode::Window,
            // 插件侧
            "link" => ErrorCode::Link,
            "timeout" => ErrorCode::Timeout,
            "protocol" => ErrorCode::Protocol,
            "space" => ErrorCode::Space,
            "rejected" => ErrorCode::Rejected,
            "busy" => ErrorCode::Busy,
            "reboot" => ErrorCode::DeviceReboot,
            _ => ErrorCode::Unknown,
        }
    }

    /// 短标题：卡片第一行，也是日志里跟着码一起看的那句。
    pub fn label(self) -> &'static str {
        match self {
            ErrorCode::Link => "连接已中断",
            ErrorCode::Timeout => "手环没有响应",
            ErrorCode::Protocol => "两侧版本或协议对不上",
            ErrorCode::Space => "手环存储写入失败",
            ErrorCode::Rejected => "请求被拒绝",
            ErrorCode::BadPack => "章节包数据有问题",
            ErrorCode::Window => "传输窗口协商不一致",
            ErrorCode::DeviceReboot => "手环重启过",
            ErrorCode::Busy => "上一次操作还没结束",
            ErrorCode::Unknown => "同步失败",
        }
    }

    /// 怎么办。**一句话、可执行**，不要复述标题。
    pub fn advice(self) -> &'static str {
        match self {
            ErrorCode::Link => "断点已经留在手环上，插件会自动重连并接着传；也可以手动点「连接设备」",
            ErrorCode::Timeout => "确认手表上《甜蜜女友2》已打开且没被切到后台，然后重试",
            ErrorCode::Protocol => "更新插件或手环端 RPK 到同一条版本线上再试",
            ErrorCode::Space => "在手环「设置 → 资源包管理」里删掉不用的章节腾出空间，再重试",
            ErrorCode::Rejected => "换一块分片档位（设置页）或稍后重试",
            ErrorCode::BadPack => "重新打包这一章，或在插件里重新安装一次",
            ErrorCode::Window => "把分片档位改回默认的 8 KB 再试",
            ErrorCode::DeviceReboot => "手环刚重启完，重新点一次「同步」就会从断点接着传",
            ErrorCode::Busy => "等当前这一章传完，或先点「取消」",
            ErrorCode::Unknown => "把手环停在《甜蜜女友2》页面再重试；持续失败请把日志发给作者",
        }
    }

    /// 重试有没有意义。界面据此决定主按钮是「重试」还是「先处理别的」。
    pub fn retryable(self) -> bool {
        matches!(self, ErrorCode::Link | ErrorCode::Timeout | ErrorCode::DeviceReboot | ErrorCode::Busy | ErrorCode::Rejected | ErrorCode::Unknown)
    }

    /// 手环侧需不需要用户动手（腾空间、换档位、更新版本）。
    pub fn needs_user(self) -> bool {
        matches!(self, ErrorCode::Space | ErrorCode::Protocol | ErrorCode::BadPack | ErrorCode::Window)
    }
}

/// 界面上要显示的一条失败：码 + 细节原文（细节可能来自手环，原样带出来别改）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorView {
    pub code: ErrorCode,
    /// 细节原文（手环回的 `detail` / 宿主报的错）。空串表示没有更多细节。
    pub detail: String,
}

impl ErrorView {
    pub fn new(code: ErrorCode, detail: impl Into<String>) -> Self {
        Self { code, detail: detail.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个码都要有标题 + 可执行的「怎么办」，而且两者不能互相复述。
    #[test]
    fn 每个错误码都有标题和怎么办() {
        let all = [
            ErrorCode::Link,
            ErrorCode::Timeout,
            ErrorCode::Protocol,
            ErrorCode::Space,
            ErrorCode::Rejected,
            ErrorCode::BadPack,
            ErrorCode::Window,
            ErrorCode::DeviceReboot,
            ErrorCode::Busy,
            ErrorCode::Unknown,
        ];
        for code in all {
            assert!(!code.label().trim().is_empty(), "{code:?} 没有标题");
            assert!(!code.advice().trim().is_empty(), "{code:?} 没有「怎么办」");
            assert_ne!(code.label(), code.advice(), "{code:?} 的标题和「怎么办」不能是同一句");
            // 「怎么办」必须是可执行的一句话，不能只是复述结论。
            assert!(code.advice().len() >= 8, "{code:?} 的「怎么办」太短：{}", code.advice());
        }
        // 标题两两不同：状态行只显示这一句，撞车就没法判断是哪个失败。
        for (index, code) in all.iter().enumerate() {
            for other in &all[index + 1..] {
                assert_ne!(code.label(), other.label(), "{code:?} 与 {other:?} 标题撞车");
            }
        }
    }

    /// 认不出来的码必须是 `Unknown`，**绝不猜**。
    #[test]
    fn 认不出来的码一律_unknown() {
        for raw in ["", "nonsense", "E_SOMETHING_NEW", "写入失败"] {
            assert_eq!(ErrorCode::from_wire(raw), ErrorCode::Unknown, "{raw} 不该被猜成具体原因");
        }
    }

    /// 手环侧那几张已知的表都要落到对码上（改了 `pack-importer.js` 的 `errorText` 就要同步看这里）。
    #[test]
    fn 手环侧的错误码映射到正确的类别() {
        assert_eq!(ErrorCode::from_wire("size"), ErrorCode::BadPack);
        assert_eq!(ErrorCode::from_wire("base64"), ErrorCode::BadPack);
        assert_eq!(ErrorCode::from_wire("manifest"), ErrorCode::BadPack);
        assert_eq!(ErrorCode::from_wire("write"), ErrorCode::Space);
        assert_eq!(ErrorCode::from_wire("registry"), ErrorCode::Space);
        assert_eq!(ErrorCode::from_wire("window"), ErrorCode::Window);
        assert_eq!(ErrorCode::from_wire("resume-not-at-file-start"), ErrorCode::Window);
    }

    /// 需要用户动手的那几类不许被当成「重试就好」。
    #[test]
    fn 需要用户动手的失败不可重试() {
        for code in [ErrorCode::Space, ErrorCode::Protocol, ErrorCode::BadPack, ErrorCode::Window] {
            assert!(code.needs_user(), "{code:?} 应该需要用户动手");
            assert!(!code.retryable(), "{code:?} 不该被当成重试就好");
        }
        // 反过来：链路/超时/重启这类重试有意义。
        for code in [ErrorCode::Link, ErrorCode::Timeout, ErrorCode::DeviceReboot] {
            assert!(code.retryable(), "{code:?} 应该可重试");
        }
    }
}
