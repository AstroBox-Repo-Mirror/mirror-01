//! 请求通道的**纯逻辑**：请求种类、按种类的超时策略、以及只有一个坑位的待答请求槽。
//!
//! 为什么单开一个文件：`src/host/` 下的东西全被 `#[cfg(target_arch = "wasm32")]` 挡着，
//! 宿主机上根本编不到（`cargo test -p amakano2-import` 跑的就是宿主机）。而
//! 「存档的超时该比章节列表宽」和「迟到的回包不能被当成超时」这两件事是**能直接
//! 判定对错的语义**，必须有能在本地跑起来的守护用例 —— 所以它们住在这里，不跟宿主走。
//!
//! 所有数值都来自 2026-09-11 / 09-12 两次真机日志的实测配对
//! （请求 id 内嵌发送毫秒戳 ↔ 回包被插件收到的时刻），不是拍脑袋定的。

/// 一次请求的种类。`label()` 是给人看的，`is_saves()` 决定失败时要不要顺带
/// 更新存档页的状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RequestKind {
    Pending,
    PackList,
    Delete,
    ClearCache,
    /// 能力查询：`amakano.app.hello` → `amakano.app.hello-ok`。
    Hello,
    SaveList,
    SavePut,
    SaveDelete,
    SaveActivate,
    /// 「阅读统计」：`amakano.stats.list` → `amakano.stats.data`。
    ///
    /// 归在**轻量类**：手环端只是把那张按天的表 JSON 化再回包（几十天、几 KB 级），
    /// 比存档（读两次 storage + 拼 6KB 分片）轻得多，没有理由跟存档一起等宽窗口。
    StatsList,
}

impl RequestKind {
    pub fn label(self) -> &'static str {
        match self {
            RequestKind::Pending => "读取未完成传输",
            RequestKind::PackList => "读取章节列表",
            RequestKind::Delete => "删除章节包",
            RequestKind::ClearCache => "清理未完成缓存",
            RequestKind::Hello => "查询手环端存档能力",
            RequestKind::SaveList => "读取手环存档",
            RequestKind::SavePut => "写入存档",
            RequestKind::SaveDelete => "删除存档",
            RequestKind::SaveActivate => "读档（设为继续阅读）",
            RequestKind::StatsList => "读取阅读统计",
        }
    }

    /// 是不是存档相关的请求 —— 它超时时要多给一句「手环端应用版本过旧」的判断。
    ///
    /// ⚠️ **只包含存档那几条**：阅读统计走的是 `stats` 能力，和存档的协议判定是两回事，
    /// 归进来会把「统计超时」说成「存档通道不可用」。它自己在 `fail_request` 里有分支。
    pub fn is_saves(self) -> bool {
        matches!(
            self,
            RequestKind::Hello
                | RequestKind::SaveList
                | RequestKind::SavePut
                | RequestKind::SaveDelete
                | RequestKind::SaveActivate
        )
    }

    /// 首包超时：第一次发出去之后等多久算「没回来」。
    pub fn timeout_ms(self) -> u64 {
        if self.is_saves() { SAVES_TIMEOUT_MS } else { QUICK_TIMEOUT_MS }
    }

    /// 一次请求最多发几次（含第一次）。
    pub fn max_attempts(self) -> u8 {
        if self.is_saves() { SAVES_ATTEMPTS } else { QUICK_ATTEMPTS }
    }
}

// ------------------------------------------------------------------ 超时策略（实测标定）

/// 轻量请求（未完成传输 / 章节列表 / 删除 / 清缓存）的首包超时与重试次数。
///
/// 实测（干净样本 78 个，见模块头）：这类请求没丢包时整条链都在 200ms 以内 ——
/// `pend-` 中位 114ms、`delete-` 中位 177ms、`packs-` 中位 123ms、最快的 32ms；
/// 而**丢包**时（实测那批「紧跟一条回包之后几毫秒发出去」的请求，13 个里丢 8 个）
/// 重发那一次只要 19–40ms 就回来了。所以这类请求的正确策略是
/// **超时给短、重发给快**，而不是干等一个长窗口。
/// 1000ms 对正常路径留了 5 倍余量（实测正常路径最快 32ms），
/// 代价是极少数 1.2–1.5s 的真慢样本会白重发一次 —— 那也比让用户干等强。
const QUICK_TIMEOUT_MS: u64 = 1000;
const QUICK_ATTEMPTS: u8 = 3;

/// 存档请求（能力协商 / 存档列表 / 写回 / 删除 / 读档）的首包超时与重试次数。
///
/// 存档比章节列表重，慢是合理的：手环端要读两次 `@system.storage`
/// （`recoveryData` + `autoSave`）、拼一个 6KB 级的 JSON、再按 9000 字节切片回传；
/// 章节列表只是内存里的一个数组。实测：正常时 94–98ms（和章节列表同档），
/// 但**长尾厚得多** —— 干净样本里首包落在 5549 / 8575 / 24710ms 的全是存档请求
/// （另有 1619–1635ms 一簇，那是被旧的 1600ms 超时截断后重发的产物：1600+19…35）。
/// 存档一旦真慢下来，**重发只会往手环的处理队列里再塞一份**（实测手环会积压后
/// 一次性回吐，三个回包在 11ms 内一起到达），所以这里给的是
/// **更宽的首包 + 与轻量类相同的次数**：既不因为 1.0s 就放弃，也不额外多打扰手环。
const SAVES_TIMEOUT_MS: u64 = 2000;
const SAVES_ATTEMPTS: u8 = 3;

/// 「消息处理回调里**立刻**发下一个请求」这条路的延迟。
///
/// 这是本轮实测最硬的一条。把每个请求的发出时刻减去「上一条被插件收到的消息」的
/// 时刻，按这个间隔分桶看首次回包有没有在超时内回来：
///
/// | 距上一条消息 | 样本 | 首次就丢的比例 |
/// |---|---|---|
/// | `<10ms` | 13 | **61.5%**（8 个） |
/// | `10–50ms` | 0 | — |
/// | `200–500ms` | 8 | 0% |
/// | `500–1000ms` | 13 | 7.7%（1 个） |
///
/// 其中「`hello-ok` 到达后 2ms 发出的 `saves.list`」是 10 个样本里丢了 8 个，
/// 而它正是「连上设备 → 握手成功 → 顺手拉一次存档」这条路径 —— 用户看到的就是
/// 「第一次打开存档页要转很久甚至报超时，再点一次刷新就好了」。
///
/// 结论：**别在消息回调里立刻发下一个请求**，让事件循环先转一圈、也给手环端
/// 一点喘息再发。所以「收完一条回包顺手拉存档」一律改成 arm 一个定时器、到点再发
/// （用现成的 timer 机制，不新造机制）。
pub const FOLLOWUP_DELAY_MS: u64 = 200;

// ------------------------------------------------------------------ 单坑位

/// 一个已经发出去、正等着回包的请求。
#[derive(Clone)]
pub struct PendingRequest {
    pub kind: RequestKind,
    pub id: String,
    pub payload: String,
    pub addr: String,
    pub attempts: u8,
    /// 轮询探测用的请求：超时不报错、也不重发，交给探测循环决定下一步。
    pub probe: bool,
}

/// 等待回包的那个请求超时到点之后该干什么。
#[derive(PartialEq, Eq, Debug)]
pub enum TimeoutAction {
    /// 不是这个请求的定时器（坑位已经被后来的请求顶掉了）：**什么都不做**。
    Ignore,
    /// 探测请求：静默让位，由探测循环决定下一步。
    DropProbe,
    /// 重发一次（`attempts` 已经加过 1）。
    Retry,
    /// 次数用完：该报超时了。
    GiveUp(RequestKind),
}

/// 待答请求槽 —— `State::request` 的**全部语义**都在这里，只有**一个坑位**。
///
/// ## 这个单坑位是一条历史结构问题
///
/// 坑位只有一个，后发的请求会把先发的**静静顶掉**：被顶掉的那个既不会重发、
/// 也不会报超时（它的定时器到点时 `on_timeout` 判为 [`TimeoutAction::Ignore`]）。
/// 它的回包即使迟到也只会被 [`Slot::clear_if_matches`] 判为「不匹配」——
/// **数据照样会被应用**（回包是按 `message.type` 分派的，不依赖坑位），但再也没有
/// 任何人替它兜底。2026-09「存档页经常超时、重试一次又成功」那阵子，
/// 存档列表恰恰是**最容易丢包、最需要重试**的那种请求（实测首次就成只有 18.2%），
/// 而「刷新」以前是 saves 先发、`pack.list` 后发 —— 存档列表就这样把重试资格让了出去。
///
/// 本轮的处置：**不改成多坑位表**，而是让宿主侧按 `RefreshStep` **串行**发请求
/// （一次只挂一个，前一个 settle 了再发下一个），谁也不顶谁。
/// 测量结论是「丢包/重入发送」才是主因、坑位被挤不是主因，所以不动这里的结构；
/// 真要动，务必保留「回包按 id 匹配、不匹配的不要误判为超时」这条语义。
#[derive(Default)]
pub struct Slot {
    current: Option<PendingRequest>,
}

impl Slot {
    pub fn idle() -> Self {
        Self { current: None }
    }

    /// 有没有请求挂在这个坑位上（探测循环据此判断要不要再发）。
    pub fn busy(&self) -> bool {
        self.current.is_some()
    }

    pub fn get(&self) -> Option<&PendingRequest> {
        self.current.as_ref()
    }

    /// 占坑。**顶掉**上一个还挂着的请求（顶掉的那个从此不管了，见类型注释）。
    ///
    /// 宿主侧一定要**先占坑、再发消息**：反过来的话，回包万一下来得比占坑还快，
    /// `clear_if_matches` 会扑个空，接着武装的定时器就会给一个**已经答过的请求**
    /// 再重发一次（实测里那些「同一 requestId 收到两份回包」就有这一份）。
    pub fn arm(&mut self, request: PendingRequest) {
        self.current = Some(request);
    }

    /// 回包到达：**只有 id 对得上**才算销账、才清坑位。返回是否真的清了。
    pub fn clear_if_matches(&mut self, id: &str) -> bool {
        if self.current.as_ref().map(|request| request.id.as_str()) == Some(id) {
            self.current = None;
            true
        } else {
            false
        }
    }

    /// 某个请求的定时器到点了。**id 不匹配就是别人的定时器**，什么都不做。
    pub fn on_timeout(&mut self, id: &str) -> TimeoutAction {
        let Some(request) = self.current.as_mut() else { return TimeoutAction::Ignore };
        if request.id != id {
            return TimeoutAction::Ignore;
        }
        if request.probe {
            self.current = None;
            return TimeoutAction::DropProbe;
        }
        if request.attempts < request.kind.max_attempts() {
            request.attempts += 1;
            TimeoutAction::Retry
        } else {
            let kind = request.kind;
            self.current = None;
            TimeoutAction::GiveUp(kind)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(kind: RequestKind, id: &str) -> PendingRequest {
        PendingRequest {
            kind,
            id: id.into(),
            payload: format!("{{\"requestId\":\"{id}\"}}"),
            addr: "AA:BB".into(),
            attempts: 1,
            probe: false,
        }
    }

    /// 每个请求种类都得有一句**给人看的话**（状态行上直接显示的就是它），
    /// 而且存档类的那几句要能让人认出「这是存档的事」。
    #[test]
    fn 每种请求都有给人看的名字() {
        let all = [
            RequestKind::Pending,
            RequestKind::PackList,
            RequestKind::Delete,
            RequestKind::ClearCache,
            RequestKind::Hello,
            RequestKind::SaveList,
            RequestKind::SavePut,
            RequestKind::SaveDelete,
            RequestKind::SaveActivate,
            RequestKind::StatsList,
        ];
        for kind in all {
            let label = kind.label();
            assert!(!label.trim().is_empty(), "{kind:?} 没有状态行文案");
            if kind.is_saves() {
                // 「读档」也是存档通道的说法，所以词汇表收这三个。
                let readable = ["存档", "读档", "能力"].iter().any(|word| label.contains(word));
                assert!(readable, "{kind:?} 的文案看不出是存档的事：{label}");
            }
        }
        // 文案还得两两不同：超时时状态行只说这一句，撞车了就没法从界面判断是哪个请求挂了。
        for (index, kind) in all.iter().enumerate() {
            for other in &all[index + 1..] {
                assert_ne!(kind.label(), other.label(), "{kind:?} 和 {other:?} 的文案撞车了");
            }
        }
    }

    /// 重发用的是**同一份 payload、同一个地址**（`handle_request_timeout` 就靠它），
    /// 而且重发之后 id 不变 —— 回包还是按这个 id 认账的。
    #[test]
    fn 重发沿用同一份载荷与地址() {
        let mut slot = Slot::idle();
        slot.arm(pending(RequestKind::SaveList, "saves-7"));
        assert_eq!(slot.on_timeout("saves-7"), TimeoutAction::Retry);
        let request = slot.get().expect("重发之后坑位还是它的");
        assert_eq!(request.id, "saves-7");
        assert_eq!(request.payload, "{\"requestId\":\"saves-7\"}");
        assert_eq!(request.addr, "AA:BB");
        assert_eq!(request.attempts, 2);
    }

    /// ① 不同请求种类的超时值 —— 「为什么存档更宽」的理由写在常量注释里（见模块里
    /// `SAVES_TIMEOUT_MS` / `QUICK_TIMEOUT_MS` 的说明：手环端存档要读两次 storage
    /// 再拼 6KB 分片回包，章节列表只是内存里的数组；实测存档长尾到 5.5–24.7s，
    /// 轻量类最长也就 1.5s）。
    #[test]
    fn 存档类的超时比轻量类宽() {
        for kind in [RequestKind::Hello, RequestKind::SaveList, RequestKind::SavePut, RequestKind::SaveDelete, RequestKind::SaveActivate] {
            assert!(kind.is_saves(), "{kind:?} 属于存档通道");
            assert_eq!(kind.timeout_ms(), SAVES_TIMEOUT_MS, "{kind:?}");
            assert_eq!(kind.max_attempts(), SAVES_ATTEMPTS, "{kind:?}");
            assert!(kind.timeout_ms() > RequestKind::PackList.timeout_ms(), "存档必须比章节列表宽");
        }
        for kind in [RequestKind::Pending, RequestKind::PackList, RequestKind::Delete, RequestKind::ClearCache, RequestKind::StatsList] {
            assert!(!kind.is_saves(), "{kind:?} 不是存档通道");
            assert_eq!(kind.timeout_ms(), QUICK_TIMEOUT_MS, "{kind:?}");
            assert_eq!(kind.max_attempts(), QUICK_ATTEMPTS, "{kind:?}");
        }
        // 探测/轻量类不许比轻量档更慢：它们是「手环在不在」的探测，等久了没意义。
        assert!(QUICK_TIMEOUT_MS < SAVES_TIMEOUT_MS);
        // 每次都要发出去（含第一次），而且必须有上限 —— 不许变成无限循环。
        assert!(QUICK_ATTEMPTS >= 2 && SAVES_ATTEMPTS >= 2, "至少要有一次重发兜底");
        assert!(QUICK_ATTEMPTS <= 4 && SAVES_ATTEMPTS <= 4, "重发次数要有上限，不能无限循环");
    }

    /// 「紧跟一条回包之后」的延迟必须是正数：0 就等于还在消息回调里重入发送，
    /// 那正是实测丢了 61.5% 的那种发法。
    #[test]
    fn 收完回包之后顺手续拉要隔一拍() {
        assert!(FOLLOWUP_DELAY_MS >= 100, "隔得太短等于没有隔，实测 <10ms 那一档丢了 61.5%");
        assert!(FOLLOWUP_DELAY_MS < QUICK_TIMEOUT_MS, "顺手续拉的延迟要远小于首包超时，不然白等");
    }

    /// ② 迟到的回包仍按 id 匹配：**不匹配就不算销账，也绝不误判成超时**。
    #[test]
    fn 迟到的回包按_id_匹配不会被误判超时() {
        let mut slot = Slot::idle();
        slot.arm(pending(RequestKind::PackList, "packs-1"));
        // 另一个请求（比如先发出去的 saves）的回包迟到了：不匹配，坑位不许动。
        assert!(!slot.clear_if_matches("saves-0"));
        assert!(slot.busy(), "不匹配的回包不许把坑位清掉");
        // 它的定时器到点也不许被判成超时重发/报错 —— 那是别人的定时器。
        assert_eq!(slot.on_timeout("saves-0"), TimeoutAction::Ignore);
        assert!(slot.busy());
        // 正主回来了才算销账。
        assert!(slot.clear_if_matches("packs-1"));
        assert!(!slot.busy());
        // 销账之后正主的定时器再响也只是空转（延迟的回包已经把请求结束了）。
        assert_eq!(slot.on_timeout("packs-1"), TimeoutAction::Ignore);
    }

    /// 被后来的请求顶掉的那个：既不重发也不报超时（这就是单坑位的历史行为，
    /// 本轮靠「串行发」让它不再发生，而不是靠改坑位）。
    #[test]
    fn 被顶掉的请求既不重发也不报超时() {
        let mut slot = Slot::idle();
        slot.arm(pending(RequestKind::SaveList, "saves-1"));
        slot.arm(pending(RequestKind::PackList, "packs-2"));
        assert_eq!(slot.on_timeout("saves-1"), TimeoutAction::Ignore);
        assert_eq!(slot.get().map(|request| request.id.as_str()), Some("packs-2"), "坑位已经是后发的那个了");
        // 顶掉之后坑位主人还是新的那个，重试逻辑照旧。
        assert_eq!(slot.on_timeout("packs-2"), TimeoutAction::Retry);
        assert_eq!(slot.get().map(|request| request.attempts), Some(2));
    }

    /// 重试有上限：次数用完就报超时，不许无限重发。
    #[test]
    fn 重试次数用完就放弃() {
        let mut slot = Slot::idle();
        slot.arm(pending(RequestKind::SaveList, "saves-1"));
        let mut retries = 0;
        loop {
            match slot.on_timeout("saves-1") {
                TimeoutAction::Retry => retries += 1,
                TimeoutAction::GiveUp(kind) => {
                    assert_eq!(kind, RequestKind::SaveList);
                    break;
                }
                other => panic!("存档请求不该走到 {other:?}"),
            }
            assert!(retries < 10, "重发必须有上限");
        }
        assert_eq!(retries, SAVES_ATTEMPTS as i32 - 1, "总共发 {SAVES_ATTEMPTS} 次 = 第一次 + {} 次重发", SAVES_ATTEMPTS - 1);
        assert!(!slot.busy(), "放弃之后要把坑位让出来");
    }

    /// 探测请求超时是静默让位，不重发、不报错。
    #[test]
    fn 探测请求超时静默让位() {
        let mut slot = Slot::idle();
        let mut probe = pending(RequestKind::PackList, "packs-probe");
        probe.probe = true;
        slot.arm(probe);
        assert_eq!(slot.on_timeout("packs-probe"), TimeoutAction::DropProbe);
        assert!(!slot.busy());
    }
}
