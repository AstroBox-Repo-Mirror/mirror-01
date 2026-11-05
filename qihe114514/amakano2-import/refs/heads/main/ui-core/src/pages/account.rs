//! 账号页：AstroBox 授权、QQ 绑定、云存档同步状态。
//!
//! 这一页回答一个问题：**我的存档存到哪了、换台设备还找得回来吗。**
//! 它从「存档页里的一张云卡片」独立出来，因为：
//! - 手环本地存档（存档页）和云存档（这页）是两条独立的链路，混在一页时
//!   用户分不清「这条存档到底在哪」；
//! - 授权 / 绑定是**一次性配置**，不该每次进存档页都占着首屏。
//!
//! 排版沿用成组列表：账号卡 → 绑定流程 → 状态说明 → 管理动作。
//! **登录是宿主账号，插件不碰密码**（只拿公开资料），文案不能暗示能在这里输 QQ 密码。

use super::super::actions;
use super::super::glass::{
    alpha, divider, ghost_button, group, group_heading, group_row, primary_button, row_detail,
    row_title, state_badge,
};
use super::super::node::{Node, Tag, label};
use super::super::snapshot::{Snapshot, StatusKind};
use super::super::theme::*;

pub fn render(snapshot: &Snapshot) -> Node {
    let authorized = snapshot.account_auth == "authorized";
    let mut page = Node::new(Tag::Div).full().column();

    page = page.child(account_card(snapshot, authorized));

    if !authorized {
        return page.child(authorize_flow(snapshot)).child(footnote());
    }

    let bound = !snapshot.cloud_qq.is_empty();
    page = page
        .child(bind_flow(snapshot, bound))
        .child_if(!snapshot.cloud_detail.is_empty() && snapshot.cloud_detail != "—", detail_line(snapshot))
        .child_if(bound, sync_button(snapshot))
        .child(bind_card(snapshot, bound))
        .child(manage_group(snapshot))
        .child(footnote());
    page
}

/// 顶部账号卡：QQ 头像 + 昵称 + 身份 + 状态徽章。
///
/// 原生 `IMAGE` 只接受 data URI，不能直接把 QQ 头像 URL 塞进去；同时 wasm 里也没有
/// 可靠的跨域图片下载通道。这里保留首字母身份块作为稳定兜底，HTML 面板再叠加
/// `qlogo` 头像（那里 `<img>` 可以直接加载远端 URL）。
fn account_card(snapshot: &Snapshot, authorized: bool) -> Node {
    let (title, subtitle) = if !authorized {
        let name = match snapshot.account_auth.as_str() {
            "error" => "账号读取失败",
            "authorizing" => "正在读取账号…",
            _ => "尚未绑定账号",
        };
        (name.to_string(), "先用 AstroBox 账号授权，再绑定一个 QQ".to_string())
    } else if snapshot.cloud_qq.is_empty() {
        ("AstroBox 账号已授权".to_string(), "还没有绑定 QQ 主账号".to_string())
    } else {
        let name = if snapshot.cloud_qq_name.is_empty() {
            "QQ 主账号".to_string()
        } else {
            snapshot.cloud_qq_name.clone()
        };
        let sub = if snapshot.cloud_account_name.is_empty() {
            format!("QQ {}", snapshot.cloud_qq)
        } else {
            format!("QQ {} · {}", snapshot.cloud_qq, snapshot.cloud_account_name)
        };
        (name, sub)
    };

    let initial: String = title.chars().next().map(|c| c.to_string()).unwrap_or_default();

    let status = if authorized {
        phase_badge(snapshot)
    } else {
        state_badge(
            if snapshot.account_auth == "error" { "读取失败" } else { "未授权" },
            if snapshot.account_auth == "error" { StatusKind::Bad } else { StatusKind::Info },
        )
    };

    group().child(
        Node::new(Tag::Div)
            .full()
            .row()
            .align("center")
            .gap(GAP)
            .pad_y(16)
            .child(
                // 头像：品牌粉描边的一格，里面放昵称首字（拿不到昵称时放「?」）。
                Node::new(Tag::Div)
                    .w(52)
                    .h(52)
                    .shrink(0.0)
                    .radius(26)
                    .bg(SURFACE_SOFT)
                    .border(2, &alpha(ACCENT, "59"))
                    .column()
                    .align("center")
                    .justify("center")
                    .child(label(
                        if initial.is_empty() { "?" } else { initial.as_str() },
                        TYPE_TITLE,
                        ACCENT,
                    )),
            )
            .child(
                Node::new(Tag::Div)
                    .column()
                    .gap(3)
                    .grow(1.0)
                    .shrink(1.0)
                    .child(row_title(title))
                    .child(row_detail(subtitle)),
            )
            .child(status),
    )
}

/// 云同步状态徽章（`cloud_phase` → 一词 + 一色）。
fn phase_badge(snapshot: &Snapshot) -> Node {
    let (text, kind) = match snapshot.cloud_phase.as_str() {
        "synced" => ("已同步", StatusKind::Good),
        "syncing" | "applying" => ("同步中", StatusKind::Info),
        "offline" => ("离线", StatusKind::Warn),
        "failed" => ("同步失败", StatusKind::Bad),
        "unbound" => ("未绑定", StatusKind::Info),
        "waiting-binding" => ("待绑定", StatusKind::Warn),
        "signed-out" | "unconfigured" => ("未配置", StatusKind::Warn),
        _ => ("已连接", StatusKind::Info),
    };
    state_badge(text, kind)
}

/// 未授权时的两步流程 + 授权按钮。
fn authorize_flow(snapshot: &Snapshot) -> Node {
    let busy = snapshot.account_auth == "authorizing";
    let detail = match snapshot.account_auth.as_str() {
        "authorizing" => "正在读取 AstroBox 账号…".to_string(),
        "denied" => "AstroBox 还没登录：请先在 AstroBox 里登录账号，再回来授权。".to_string(),
        "error" => {
            if snapshot.cloud_detail.is_empty() {
                "读取 AstroBox 账号失败，请稍后重试。".to_string()
            } else {
                snapshot.cloud_detail.clone()
            }
        }
        _ => "授权只用于关联当前 AstroBox 账号，插件不会读取密码或令牌。".to_string(),
    };
    let retry = snapshot.account_auth == "error";

    group_heading("绑定流程")
        .full()
        .child(
            group()
                .child(flow_step("1", "授权 AstroBox 账号", "获取当前账号的公开资料", "on"))
                .child(divider())
                .child(flow_step("2", "绑定 QQ", "QQ 成为主账号，之后自动登录", "idle")),
        )
        .child(
            Node::new(Tag::Div).full().pad_t(GAP).child(primary_button(
                if retry { "重试授权" } else { "授权 AstroBox 账号" },
                actions::CLOUD_AUTHORIZE,
                !busy,
                snapshot,
            )),
        )
        .child(
            Node::new(Tag::Div)
                .full()
                .pad_t(GAP_SM)
                .pad_x(2)
                .child(label(detail, TYPE_META, TEXT_DIM)),
        )
}

/// 已授权后的两步状态（都完成 / 还差绑定）。
fn bind_flow(snapshot: &Snapshot, bound: bool) -> Node {
    let account = if snapshot.cloud_account_name.is_empty() {
        "已授权".to_string()
    } else {
        snapshot.cloud_account_name.clone()
    };
    group_heading("绑定流程").full().child(
        group()
            .child(flow_step("✓", "AstroBox 已授权", &account, "done"))
            .child(divider())
            .child(flow_step(
                if bound { "✓" } else { "2" },
                if bound { "QQ 已绑定" } else { "绑定 QQ" },
                if bound { "之后会自动登录" } else { "一个 AstroBox 账号只绑定一个 QQ" },
                if bound { "done" } else { "on" },
            )),
    )
}

/// 流程里的一步：左侧圆点（数字或勾）+ 标题 + 说明。
fn flow_step(mark: &str, title: &str, detail: &str, state: &str) -> Node {
    // 三态配色：已完成（绿）/ 正在进行（粉）/ 还没轮到（灰）。
    // 这里用 `String` 而不是 `&str`：`alpha()` 会拼出新字符串，生命期活不过 match。
    let (fg, bg): (&str, String) = match state {
        "done" => (OK, alpha(OK, "2E")),
        "on" => (BUTTON_PRIMARY_TEXT, BUTTON_PRIMARY.to_string()),
        _ => (TEXT_DIM, SURFACE_SOFT.to_string()),
    };
    group_row(
        Node::new(Tag::Div)
            .row()
            .align("center")
            .gap(GAP)
            .grow(1.0)
            .shrink(1.0)
            .child(
                Node::new(Tag::Div)
                    .w(26)
                    .h(26)
                    .shrink(0.0)
                    .radius(13)
                    .bg(&bg)
                    .column()
                    .align("center")
                    .justify("center")
                    .child(label(mark, TYPE_BADGE, fg)),
            )
            .child(
                Node::new(Tag::Div)
                    .column()
                    .gap(3)
                    .grow(1.0)
                    .shrink(1.0)
                    .child(label(title, TYPE_BODY, TEXT_MAIN))
                    .child(row_detail(detail)),
            ),
    )
}

/// 云同步状态的一句话说明（`cloud_detail`）。
fn detail_line(snapshot: &Snapshot) -> Node {
    Node::new(Tag::Div)
        .full()
        .pad_t(GAP)
        .pad_x(2)
        .child(label(snapshot.cloud_detail.clone(), TYPE_META, TEXT_DIM))
}

/// 「立即同步」：只有绑定之后才有意义。
fn sync_button(snapshot: &Snapshot) -> Node {
    Node::new(Tag::Div).full().pad_t(GAP).child(primary_button(
        "立即同步",
        actions::CLOUD_SYNC,
        true,
        snapshot,
    ))
}

/// QQ 绑定卡：已绑定 → 一行事实；未绑定 → 生成命令 / 显示命令 + 检查绑定。
fn bind_card(snapshot: &Snapshot, bound: bool) -> Node {
    let mut block = group_heading("QQ 绑定").full();

    if bound {
        return block.child(
            group().child(
                Node::new(Tag::Div)
                    .full()
                    .row()
                    .align("center")
                    .gap(GAP)
                    .pad_y(GROUP_ROW_PAD_Y)
                    .child(
                        Node::new(Tag::Div)
                            .column()
                            .gap(3)
                            .grow(1.0)
                            .shrink(1.0)
                            .child(row_title(format!("已绑定 QQ {}", snapshot.cloud_qq)))
                            .child(row_detail("一个 AstroBox 账号只绑定一个 QQ；换绑请在 QQ 群联系管理员")),
                    ),
            ),
        );
    }

    // 已经生成过绑定命令：命令、复制、检查与过期后的重取都必须在同一块里。
    // 只能“检查绑定”不行：码过期后检查永远失败，用户没有恢复路径。
    if !snapshot.cloud_code.is_empty() {
        let expired = binding_code_expired(&snapshot.cloud_expires_at, current_time_ms());
        let mut actions_row = Node::new(Tag::Div)
            .full()
            .row()
            .align("center")
            .gap(GAP_SM)
            .child(ghost_button("复制命令", actions::CLOUD_COPY_CODE, true, snapshot))
            .child(primary_button(
                if expired { "生成新命令" } else { "检查绑定" },
                if expired { actions::CLOUD_BINDING_CODE } else { actions::CLOUD_SESSION },
                !expired || snapshot.account_auth != "authorizing",
                snapshot,
            ));
        if !expired {
            actions_row = actions_row.child(ghost_button(
                "重新生成",
                actions::CLOUD_BINDING_CODE,
                snapshot.account_auth != "authorizing",
                snapshot,
            ));
        }
        block = block.child(
            group().child(
                Node::new(Tag::Div)
                    .full()
                    .column()
                    .gap(GAP_SM)
                    .pad_y(GROUP_ROW_PAD_Y)
                    .child(row_detail(if expired {
                        "这条绑定命令已经过期。生成一条新命令后，再去 QQ 群发送。"
                    } else {
                        "在 QQ 群里发送下面这条命令，然后点「检查绑定」。"
                    }))
                    .child(
                        Node::new(Tag::Div)
                            .full()
                            .radius(ROW_RADIUS)
                            .bg(SURFACE_SOFT)
                            .pad(12)
                            .column()
                            .gap(4)
                            .align("center")
                            .child(label(snapshot.cloud_code.clone(), TYPE_TITLE, ACCENT))
                            .child(label(
                                if expired {
                                    "绑定码已过期，旧命令不能再使用"
                                } else {
                                    "绑定码 3 分钟内有效且只能使用一次"
                                },
                                TYPE_BADGE,
                                if expired { WARN } else { TEXT_DIM },
                            )),
                    )
                    .child(
                        Node::new(Tag::Div)
                            .full()
                            .row()
                            .align("center")
                            .gap(GAP_SM)
                            .child(
                                label(expiry_label(snapshot, expired), TYPE_META, if expired { BAD } else { WARN })
                                    .grow(1.0)
                                    .shrink(1.0),
                            )
                            .child(state_badge(if expired { "已过期" } else { "待发送" }, if expired { StatusKind::Bad } else { StatusKind::Warn })),
                    )
                    .child(actions_row),
            ),
        );
        return block;
    }

    block.child(
        group().child(
            Node::new(Tag::Div)
                .full()
                .column()
                .gap(GAP_SM)
                .pad_y(GROUP_ROW_PAD_Y)
                .child(row_detail(
                    "生成一次性绑定命令，到指定的 QQ 群发送。一个 AstroBox 账号只绑定一个 QQ。",
                ))
                .child(primary_button(
                    "生成绑定命令",
                    actions::CLOUD_BINDING_CODE,
                    snapshot.account_auth != "authorizing",
                    snapshot,
                )),
        ),
    )
}

/// 服务端的 `expiresAt` 是 ISO-8601 UTC。这里只做一件事：是否已经过期。
///
/// 不把绝对时间直接印给用户看：`2026-11-01T...Z` 既不好读，也没有告诉他
/// **现在还能不能继续用**。缺少/损坏的时间按“未过期”处理，避免因为字段兼容
/// 问题把还能用的绑定码藏起来；这时仍保留「重新生成」这个手动退路。
fn binding_code_expired(expires_at: &str, now_ms: u128) -> bool {
    let Some(expires_ms) = parse_iso_utc_ms(expires_at) else { return false };
    now_ms > expires_ms
}

/// 宿主与浏览器预览都不需要精确到毫秒，但**过期判断必须用真实当前时间**。
/// wasm 侧 `SystemTime` 与宿主一致；测试里由传入值直接驱动纯函数。
fn current_time_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or_default()
}

fn parse_iso_utc_ms(value: &str) -> Option<u128> {
    let value = value.trim();
    let (date, time) = value.strip_suffix('Z')?.split_once('T')?;
    let mut date = date.split('-');
    let year = date.next()?.parse::<i64>().ok()?;
    let month = date.next()?.parse::<u32>().ok()?;
    let day = date.next()?.parse::<u32>().ok()?;
    if date.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (clock, millis) = time.split_once('.').unwrap_or((time, ""));
    let mut clock = clock.split(':');
    let hour = clock.next()?.parse::<u128>().ok()?;
    let minute = clock.next()?.parse::<u128>().ok()?;
    let second = clock.next()?.parse::<u128>().ok()?;
    if clock.next().is_some() || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let millis = if millis.is_empty() {
        0
    } else {
        let digits: String = millis.chars().take(3).collect();
        if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        format!("{digits:0<3}").parse::<u128>().ok()?
    };
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let month = i64::from(month);
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let roundtrip = super::super::theme::civil_from_days(days);
    if roundtrip != (year + i64::from(month <= 2), month as u32, day) {
        return None;
    }
    let seconds = days as u128 * 86_400 + hour * 3_600 + minute * 60 + second;
    Some(seconds * 1_000 + millis)
}

/// 绑定码有效期文案：剩下的时间 + 是否已过期，比绝对 UTC 时间更能指导下一步。
fn expiry_label(snapshot: &Snapshot, expired: bool) -> String {
    if expired {
        return "绑定码已过期，不能再发送".to_string();
    }
    let Some(expires_ms) = parse_iso_utc_ms(&snapshot.cloud_expires_at) else {
        return "3 分钟内有效".to_string();
    };
    let left = expires_ms.saturating_sub(current_time_ms()) / 1_000;
    format!("剩余 {:02}:{:02}", left / 60, left % 60)
}

/// 管理动作：刷新状态 / 忘记本机授权。
fn manage_group(snapshot: &Snapshot) -> Node {
    let busy = snapshot.account_auth == "authorizing";
    group_heading("账号管理").full().child(
        group()
            .child(manage_row(
                "刷新账号状态",
                "重新读取 QQ 绑定与云同步状态",
                false,
                actions::CLOUD_SESSION,
                !busy,
                snapshot,
            ))
            .child(divider())
            .child(manage_row(
                "忘记本机授权",
                "不删除云端存档和 QQ 绑定",
                true,
                actions::CLOUD_FORGET,
                !busy,
                snapshot,
            )),
    )
}

/// 一整行可点的管理项（标题 + 说明 + 右箭头）。
///
/// `snapshot` 只用于按下态：`press_id` 让按钮知道自己被按住了，
/// 宿主没有 `:active`，按下反馈只能靠事件搬进界面状态。
fn manage_row(
    title: &str,
    detail: &str,
    danger: bool,
    action: &str,
    enabled: bool,
    snapshot: &Snapshot,
) -> Node {
    let _ = snapshot;
    let row = Node::new(Tag::Button)
        .full()
        .row()
        .align("center")
        .gap(GAP_SM)
        .pad_y(GROUP_ROW_PAD_Y)
        .bg(TRANSPARENT)
        .without_default_styles()
        .child(
            Node::new(Tag::Div)
                .column()
                .gap(3)
                .grow(1.0)
                .shrink(1.0)
                .child(label(title, TYPE_BODY, if danger { BAD } else { TEXT_MAIN }))
                .child(row_detail(detail)),
        )
        .child(label("›", TYPE_TITLE, TEXT_DIM));
    if enabled {
        row.click(action).press(&actions::press_id(action))
    } else {
        row.disabled().opacity(0.42)
    }
}

/// 底部说明：讲清「谁是主账号」。
fn footnote() -> Node {
    Node::new(Tag::Div)
        .full()
        .pad_t(18)
        .pad_x(2)
        .child(label(
            "QQ 是主账号，AstroBox 账号只用于确认「这台设备属于谁」。",
            TYPE_META,
            TEXT_DIM,
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::Page;

    fn base() -> Snapshot {
        let mut snapshot = Snapshot::default();
        snapshot.page = Page::Account;
        snapshot
    }

    /// 未授权：只能看到「授权」入口，**不许**出现同步 / 绑定动作。
    #[test]
    fn unauthorized_shows_only_the_authorize_action() {
        let snapshot = base();
        let tree = render(&snapshot);
        let texts = tree.texts();
        assert!(texts.contains(&"授权 AstroBox 账号"), "{texts:?}");
        assert!(!texts.iter().any(|text| text.contains("立即同步")), "{texts:?}");
        assert!(
            tree.find(|node| node.get("on.click") == Some(actions::CLOUD_BINDING_CODE)).is_empty(),
            "还没授权就不该能生成绑定命令"
        );
    }

    /// 已授权未绑定：给「生成绑定命令」，不给「立即同步」（没有 QQ 就没有归属）。
    #[test]
    fn authorized_but_unbound_offers_binding_not_sync() {
        let mut snapshot = base();
        snapshot.account_auth = "authorized".into();
        snapshot.cloud_phase = "unbound".into();
        let tree = render(&snapshot);
        assert!(
            !tree.find(|node| node.get("on.click") == Some(actions::CLOUD_BINDING_CODE)).is_empty(),
            "要给生成绑定命令的入口"
        );
        assert!(
            tree.find(|node| node.get("on.click") == Some(actions::CLOUD_SYNC)).is_empty(),
            "没绑 QQ 时「立即同步」没有意义"
        );
    }

    /// 已绑定：显示 QQ 号 + 两个管理动作，并且**不再给生成绑定命令**。
    #[test]
    fn bound_account_shows_identity_and_management() {
        let mut snapshot = base();
        snapshot.account_auth = "authorized".into();
        snapshot.cloud_phase = "synced".into();
        snapshot.cloud_qq = "3576325016".into();
        snapshot.cloud_qq_name = "其核".into();
        snapshot.cloud_account_name = "其核".into();
        let tree = render(&snapshot);
        let texts = tree.texts();
        assert!(texts.iter().any(|text| text.contains("3576325016")), "{texts:?}");
        assert!(texts.contains(&"已同步"), "{texts:?}");
        assert!(texts.contains(&"刷新账号状态"), "{texts:?}");
        assert!(texts.contains(&"忘记本机授权"), "{texts:?}");
        assert!(
            tree.find(|node| node.get("on.click") == Some(actions::CLOUD_BINDING_CODE)).is_empty(),
            "已经绑定了就不该再给生成绑定命令"
        );
        assert!(
            !tree.find(|node| node.get("on.click") == Some(actions::CLOUD_SYNC)).is_empty(),
            "已绑定要给「立即同步」"
        );
    }

    /// 绑定命令在：复制必须真的挂动作；未过期给“检查 + 重新生成”，
    /// 过期后把主按钮换成“生成新命令”，让用户有明确的恢复路径。
    #[test]
    fn binding_code_can_be_copied_and_regenerated_after_expiry() {
        let mut snapshot = base();
        snapshot.account_auth = "authorized".into();
        snapshot.cloud_phase = "waiting-binding".into();
        snapshot.cloud_code = "绑定123456".into();
        snapshot.cloud_expires_at = "2999-01-01T00:00:00.000Z".into();
        let tree = render(&snapshot);
        assert_eq!(
            tree.find(|node| node.get("on.click") == Some(actions::CLOUD_COPY_CODE)).len(),
            1,
            "绑定命令要有一个真正的复制动作"
        );
        assert!(
            tree.find(|node| node.get("on.click") == Some(actions::CLOUD_SESSION)).len() >= 1,
            "未过期时要有检查绑定入口"
        );
        assert_eq!(
            tree.find(|node| node.get("on.click") == Some(actions::CLOUD_BINDING_CODE)).len(),
            1,
            "未过期也可以主动重新生成"
        );

        snapshot.cloud_expires_at = "2000-01-01T00:00:00.000Z".into();
        let tree = render(&snapshot);
        let texts = tree.texts();
        assert!(texts.contains(&"已过期"), "{texts:?}");
        assert!(texts.contains(&"生成新命令"), "{texts:?}");
        assert!(
            tree.find(|node| node.get("on.click") == Some(actions::CLOUD_BINDING_CODE)).len() == 1,
            "过期后必须能重新获取"
        );
        assert!(
            tree.find(|node| node.get("on.click") == Some(actions::CLOUD_SESSION)).len() <= 1,
            "过期码仍可通过管理区手动刷新，但绑定卡里不该把检查当成主按钮"
        );
    }

    #[test]
    fn binding_expiry_parser_handles_fraction_and_junk() {
        assert_eq!(parse_iso_utc_ms("1970-01-01T00:00:01Z"), Some(1_000));
        assert_eq!(parse_iso_utc_ms("1970-01-01T00:00:01.5Z"), Some(1_500));
        assert!(binding_code_expired("1970-01-01T00:00:01Z", 1_001));
        assert!(!binding_code_expired("1970-01-01T00:00:01Z", 1_000));
        assert!(!binding_code_expired("not-a-time", u128::MAX), "坏时间不能误判成过期");
    }

    /// 页面**不收集密码**：文案里不能出现任何暗示「在这里输入 QQ 密码」的说法。
    #[test]
    fn the_page_never_asks_for_a_password() {
        let mut snapshot = base();
        snapshot.account_auth = "authorized".into();
        let texts = render(&snapshot).texts().join(" ");
        assert!(!texts.contains("密码"), "插件不碰密码：{texts}");
        assert!(!texts.contains("登录 QQ"), "登录的是 AstroBox 账号，不是 QQ：{texts}");
    }
}
