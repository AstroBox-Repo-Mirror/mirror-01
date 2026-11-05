//! 宿主账号、设备凭证与云同步。HTTP 等待期间不持有全局状态锁。

use serde_json::{Value, json};

use crate::cloud::{self, BandSnapshot, CloudDocument, LocalState};
use super::{State, random_cloud_id, render, save_cloud_state, update};
use super::http::{self, ApiFailure};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloudPhase {
    Unconfigured,
    Loading,
    SignedOut,
    Unbound,
    WaitingBinding,
    Idle,
    Syncing,
    Synced,
    Offline,
    Failed,
    Applying,
}

impl CloudPhase {
    pub fn wire(self) -> &'static str {
        match self {
            Self::Unconfigured => "unconfigured", Self::Loading => "loading", Self::SignedOut => "signed-out",
            Self::Unbound => "unbound", Self::WaitingBinding => "waiting-binding", Self::Idle => "idle",
            Self::Syncing => "syncing", Self::Synced => "synced", Self::Offline => "offline",
            Self::Failed => "failed", Self::Applying => "applying",
        }
    }
}

#[derive(Clone)]
pub enum BandOperation {
    List { session: u64 },
    Put { session: u64, band: BandSnapshot },
}

#[derive(Clone)]
pub struct CloudRun {
    pub generation: u64,
    pub session: u64,
    pub account_id: String,
    pub account_name: String,
}

fn fail(generation: u64, phase: CloudPhase, message: impl Into<String>) {
    update(|state| {
        if state.cloud_generation != generation { return; }
        state.cloud_phase = phase;
        state.cloud_detail = message.into();
        state.cloud_running = false;
    });
    render();
}

fn live(state: &State, run: &CloudRun) -> bool {
    state.cloud_generation == run.generation && state.cloud_running
}

async fn ensure_login(run: &CloudRun) -> Result<String, ApiFailure> {
    if !update(|state| live(state, run)) { return Err(ApiFailure::transport("云任务已失效")); }    let (server, saved) = update(|state| (state.cloud_server.clone(), state.cloud_persisted.clone()));
    if !saved.credential.is_empty() {
        match http::request(&server, "GET", "/account/session", Some(&saved.credential), None).await {
            Ok(value) => {
                apply_account(run, &value);
                return Ok(saved.credential);
            }
            Err(error) if error.status == 401 => {
                update(|state| {
                    if !live(state, run) { return; }
                    state.cloud_persisted.credential.clear();
                    state.cloud_persisted.local = LocalState::default();
                });
                save_cloud_state();
            }
            Err(error) => return Err(error),
        }
    }
    let device_id = if saved.device_id.is_empty() { random_cloud_id()? } else { saved.device_id.clone() };
    let response = http::request(&server, "POST", "/account/login", None, Some(json!({
        "accountId": run.account_id, "accountName": run.account_name, "deviceId": device_id,
    }))).await?;
    if !update(|state| live(state, run)) { return Err(ApiFailure::transport("云任务已失效")); }
    let credential = response.get("credential").and_then(Value::as_str).filter(|value| value.len() >= 32)
        .ok_or_else(|| ApiFailure::transport("云服务没有签发设备凭证"))?.to_string();
    update(|state| {
        if !live(state, run) { return; }
        state.cloud_persisted.device_id = device_id;
        state.cloud_persisted.credential = credential.clone();
    });
    save_cloud_state();
    apply_account(run, &json!({ "account": response.get("account").cloned().unwrap_or(Value::Null) }));
    Ok(credential)
}

/// 把服务端的公开账号字段写进状态。
///
/// 主账号是 QQ：`nickname` + 完整 `qq`。AstroBox 关联名单独放在
/// `cloud_account_name`，不参与主标题。旧的 `qqMasked` 只作兼容兜底。
fn apply_account(run: &CloudRun, response: &Value) {
    update(|state| {
        if !live(state, run) { return; }
        let account = response.get("account");
        let text = |key: &str| account.and_then(|value| value.get(key)).and_then(Value::as_str).unwrap_or("").to_string();
        // 兼容旧服务端：没有 `nickname` 时退回 `displayName`，再退回打码 QQ。
        let name = |key: &str| -> String {
            let primary = text(key);
            if !primary.is_empty() { return primary; }
            let alt = text("displayName");
            if !alt.is_empty() { return alt; }
            text("qqMasked")
        };
        state.cloud_bound = account.and_then(|value| value.get("bound")).and_then(Value::as_bool).unwrap_or(false);
        state.cloud_qq = text("qq");
        if state.cloud_qq.is_empty() {
            // 兼容尚未升级的服务端：旧字段是打码 QQ，只能显示，不能当完整号。
            state.cloud_qq = text("qqMasked");
        }
        state.cloud_qq_name = name("nickname");
        state.cloud_account_name = text("accountName");
        state.account_authorized = true;
        state.account_auth = "authorized".into();
        state.cloud_phase = if state.cloud_bound { CloudPhase::Idle } else { CloudPhase::Unbound };
        // 绑定成功之后那条一次性命令就作废了：不清掉的话账号页会一直停在
        // 「复制这条命令去群里发送 + 检查绑定」，用户会以为还没绑上。
        if state.cloud_bound {
            state.cloud_code.clear();
            state.cloud_expires_at.clear();
        }
        state.cloud_detail = if state.cloud_bound { "QQ 主账号已绑定，连接手环后自动同步".into() } else { "AstroBox 账号已授权；请绑定一个 QQ 作为主账号".into() };
    });
    render();
}

/// 只在用户明确点击「授权 AstroBox 账号」后调用。
///
/// 插件首次打开时**不能**在 `on_load` 里自动读账号：用户还没看到授权说明，
/// 也没有机会确认把哪个 AstroBox 账号关联到 QQ。这里把授权做成一个显式动作。
pub async fn authorize() {
    let generation = update(|state| {
        state.cloud_generation = state.cloud_generation.wrapping_add(1);
        state.cloud_running = true;
        state.cloud_phase = CloudPhase::Loading;
        state.account_auth = "authorizing".into();
        state.cloud_detail = "正在请求 AstroBox 账号资料…".into();
        state.cloud_generation
    });
    render();
    if update(|state| state.cloud_server.is_empty()) {
        fail(generation, CloudPhase::Unconfigured, "云端地址未配置，请更新插件");
        return;
    }
    match super::wit::astrobox::psys_host_v4::account::get_current().await {
        Ok(Some(profile)) if !profile.id.is_empty() => {
            let run = CloudRun { generation, session: 0, account_id: profile.id, account_name: profile.name };
            update(|state| {
                if state.cloud_generation != generation { return; }
                state.cloud_account_name = run.account_name.clone();
                state.account_authorized = true;
                state.account_auth = "authorized".into();
                if state.cloud_persisted.account_id != run.account_id {
                    state.cloud_persisted.account_id = run.account_id.clone();
                    state.cloud_persisted.credential.clear();
                    state.cloud_persisted.local = LocalState::default();
                }
                state.cloud_persisted.account_name = run.account_name.clone();
            });
            save_cloud_state();
            match ensure_login(&run).await {
                // 成功路径的「相位」由 `apply_account` 写好，**详情文案必须留着**。
                // 旧实现这里传了一个空串给 `fail()`，把 `apply_account` 刚写好的那句话
                // 又清掉了 —— 界面上「状态」一栏于是永远显示「—」。
                Ok(_) => update(|state| {
                    if state.cloud_generation != generation { return; }
                    state.cloud_running = false;
                    state.cloud_phase = if state.cloud_bound { CloudPhase::Idle } else { CloudPhase::Unbound };
                    if state.cloud_detail.is_empty() {
                        state.cloud_detail = if state.cloud_bound {
                            "QQ 主账号已绑定，连接手环后自动同步".into()
                        } else {
                            "AstroBox 账号已授权；请绑定一个 QQ 作为主账号".into()
                        };
                    }
                }),
                Err(error) => {
                    set_auth_error(generation, "authorized", error.message);
                }
            }
            render();
        }
        Ok(_) => {
            update(|state| {
                if state.cloud_generation != generation { return; }
                state.account_authorized = false;
                state.account_auth = "denied".into();
            });
            fail(generation, CloudPhase::SignedOut, "请先在 AstroBox 中登录账号，再回来授权");
        }
        Err(error) => {
            set_auth_error(generation, "error", format!("无法读取 AstroBox 账号：{error}"));
        }
    }
}

fn set_auth_error(generation: u64, auth: &str, message: impl Into<String>) {
    update(|state| {
        if state.cloud_generation != generation { return; }
        state.account_auth = auth.into();
        state.cloud_phase = CloudPhase::Offline;
        state.cloud_detail = message.into();
        state.cloud_running = false;
    });
    render();
}

/// 恢复已授权账号的登录状态。
///
/// 只有本机曾经明确授权过（`account_auth=authorized` 或已保存同一 AstroBox 账号的
/// 设备凭证）才自动进行；否则账号页停在“授权 AstroBox 账号”按钮。
pub async fn restore_if_authorized() {
    let authorized = update(|state| {
        state.account_authorized
            || (!state.cloud_persisted.credential.is_empty() && !state.cloud_persisted.account_id.is_empty())
    });
    if authorized {
        authorize().await;
    } else {
        update(|state| {
            state.cloud_phase = CloudPhase::SignedOut;
            state.account_auth = "idle".into();
            state.cloud_detail = "授权 AstroBox 账号后，绑定一个 QQ 作为主账号".into();
        });
        render();
    }
}

pub async fn binding_code() {
    let (generation, account_id, account_name, authorized) = update(|state| {
        state.cloud_running = true;
        state.cloud_phase = CloudPhase::Loading;
        state.cloud_detail = "正在生成一次性绑定码…".into();
        (
            state.cloud_generation,
            state.cloud_persisted.account_id.clone(),
            state.cloud_account_name.clone(),
            state.account_authorized,
        )
    });
    if !authorized {
        fail(generation, CloudPhase::SignedOut, "请先点「授权 AstroBox 账号」，再生成绑定命令");
        return;
    }
    let run = CloudRun { generation, session: 0, account_id, account_name };
    render();
    let result = async {
        let credential = ensure_login(&run).await?;
        http::request(&update(|state| state.cloud_server.clone()), "POST", "/account/binding-code", Some(&credential), None).await
    }.await;
    match result {
        Ok(value) => {
            let command = value.get("command").and_then(Value::as_str).unwrap_or("").to_string();
            update(|state| {
                if !live(state, &run) { return; }
                state.cloud_phase = CloudPhase::WaitingBinding;
                // 服务端返回的 `command` 已经包含「绑定」前缀。前端只原样显示与复制，
                // 绝不能再用 `绑定${cloudCode}` 拼一次（真机出现「绑定绑定560356」）。
                state.cloud_code = command;
                state.cloud_expires_at = value.get("expiresAt").and_then(Value::as_str).unwrap_or("").to_string();
                state.cloud_detail = "到指定 QQ 群发送这条命令；绑定码 3 分钟内有效且只能使用一次".into();
            });
        }
        Err(error) => fail(generation, CloudPhase::Failed, error.message),
    }
    render();
}

pub async fn session() {
    let (generation, account_id, account_name, authorized) = update(|state| {
        state.cloud_running = true;
        (
            state.cloud_generation,
            state.cloud_persisted.account_id.clone(),
            state.cloud_account_name.clone(),
            state.account_authorized,
        )
    });
    if !authorized {
        fail(generation, CloudPhase::SignedOut, "请先点「授权 AstroBox 账号」");
        return;
    }
    let run = CloudRun { generation, session: 0, account_id, account_name };
    match ensure_login(&run).await {
        // 成功时 `apply_account` 已经把相位与详情写好了；这里只负责收掉「正在连接」。
        Ok(_) => update(|state| state.cloud_running = false),
        Err(error) => fail(generation, CloudPhase::Offline, error.message),
    }
    render();
}

/// 忘记本机授权：清除设备凭证、AstroBox 关联和云同步基线，但**不删除云端账号**。
///
/// 下一次必须重新点「授权 AstroBox 账号」并重新绑定 QQ；服务端的一对一约束不变，
/// 因此这条动作不会释放、也不会让其他 AstroBox 账号抢占同一个 QQ。
pub async fn forget_local_account() {
    update(|state| {
        state.cloud_generation = state.cloud_generation.wrapping_add(1);
        state.cloud_persisted = super::CloudPersisted::default();
        state.cloud_running = false;
        state.cloud_bound = false;
        state.account_authorized = false;
        state.account_auth = "idle".into();
        state.cloud_qq.clear();
        state.cloud_qq_name.clear();
        state.cloud_account_name.clear();
        state.cloud_code.clear();
        state.cloud_expires_at.clear();
        state.cloud_phase = CloudPhase::SignedOut;
        state.cloud_detail = "已忘记本机授权；云端账号和存档没有被删除".into();
    });
    super::save_cloud_state();
    render();
}

/// 每次网络往返后先读取最新本机，再决定合并/写回；旧任务不会越过此屏障。
pub async fn band_ready(run: CloudRun, band: BandSnapshot) {
    if update(|state| state.cloud_pending.is_none() && state.cloud_phase != CloudPhase::Applying && state.cloud_phase != CloudPhase::Syncing) {
        let result = async {
            ensure_login(&run).await?;
            if !update(|state| live(state, &run) && state.session == run.session && state.cloud_bound) {
                return Err(ApiFailure::transport("账号或手环连接已变化，云同步已取消"));
            }
            update(|state| {
                state.cloud_phase = CloudPhase::Syncing;
                state.cloud_reread = Some(run.clone());
            });
            super::request_band_operation(BandOperation::List { session: run.session }).await;
            Ok(())
        }.await;
        if let Err(error) = result { fail(run.generation, CloudPhase::Failed, error.message); }
        return;
    }
    if update(|state| state.cloud_pending.is_some()) {
        let same = update(|state| state.cloud_pending.as_ref().is_some_and(|pending| {
            let mut expected = pending.band.clone();
            expected.revision = band.revision;
            expected == band
        }));
        if same {
            super::request_band_operation(BandOperation::Put { session: run.session, band }).await;
        } else {
            update(|state| {
                state.cloud_pending = None;
                state.cloud_reread = Some(run.clone());
                state.cloud_phase = CloudPhase::Syncing;
                state.cloud_detail = "检测到新的本机进度，正在重新合并…".into();
            });
            super::request_band_operation(BandOperation::List { session: run.session }).await;
        }
        return;
    }
    let result = async {
        let (server, credential) = update(|state| (state.cloud_server.clone(), state.cloud_persisted.credential.clone()));
        if credential.is_empty() { return Err(ApiFailure::transport("设备凭证缺失，请重新登录")); }
        let local = update(|state| state.cloud_persisted.local.clone());
        let latest = http::request(&server, "GET", "/saves", Some(&credential), None).await?;
        let document: CloudDocument = serde_json::from_value(latest.get("document").cloned().unwrap_or(Value::Null))
            .map_err(|_| ApiFailure::transport("云存档文档格式错误"))?;
        let revision = latest.get("revision").and_then(Value::as_u64).unwrap_or(0);
        let merged = cloud::merge(&local, &band, &document, super::now_ms(), &mut cloud::RandomIds).map_err(ApiFailure::transport)?;
        let result = http::request(&server, "PUT", "/saves", Some(&credential), Some(json!({
            "expectedRevision": revision, "document": merged.document,
        }))).await;
        match result {
            Ok(_) => {
                update(|state| {
                    if !live(state, &run) { return; }
                    state.cloud_pending = Some(super::CloudBandPut { band: merged.band.clone(), local: state.cloud_persisted.local.clone(), merged });
                    state.cloud_phase = CloudPhase::Applying;
                    state.cloud_detail = "云端已保存，正在核对手环最新存档…".into();
                    state.cloud_reread = Some(run.clone());
                });
                render();
                super::request_band_operation(BandOperation::List { session: run.session }).await;
            }
            Err(error) if error.status == 409 => {
                if !update(|state| {
                    if !live(state, &run) { return false; }
                    state.cloud_conflict_retries += 1;
                    state.cloud_conflict_retries <= 3
                }) { return Err(ApiFailure::transport("云端连续发生修订冲突，请稍后重试")); }
                update(|state| state.cloud_reread = Some(run.clone()));
                super::request_band_operation(BandOperation::List { session: run.session }).await;
            }
            Err(error) => return Err(error),
        }
        Ok(())
    }.await;
    if let Err(error) = result { fail(run.generation, CloudPhase::Failed, error.message); }
}

pub fn applied(session: u64) {
    let saved = update(|state| {
        if state.session != session { return false; }
        let Some(pending) = state.cloud_pending.take() else { return false };
        if state.cloud_persisted.local.observed != pending.band { return false; }
        state.cloud_persisted.local = cloud::accepted(&pending.local, &pending.merged);
        state.cloud_phase = CloudPhase::Synced;
        state.cloud_detail = if pending.merged.conflicts > 0 {
            format!("云端与手环已同步；保留了 {} 份冲突副本", pending.merged.conflicts)
        } else { "云端与手环已同步".into() };
        state.cloud_running = false;
        true
    });
    if saved { save_cloud_state(); }
    render();
}

pub fn apply_failed(session: u64, message: &str) {
    update(|state| {
        if state.session != session { return; }
        state.cloud_pending = None;
        state.cloud_phase = CloudPhase::Failed;
        state.cloud_detail = format!("云端已保存，但手环尚未应用：{message}");
        state.cloud_running = false;
    });
    render();
}
