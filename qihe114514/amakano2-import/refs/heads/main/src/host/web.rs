//! HTML 前端的数据桥：把现有 `snapshot()` 序列化成 camelCase JSON。
//!
//! 设计约束：**ui-core 一个字段都不改**。Snapshot 的字段很多、类型杂（`Arc<str>`、
//! `serde_json::Value`、各种枚举），给它们全加 `Serialize` 又慢又容易把字段名写漂。
//! 所以这里手写一份「只挑 HTML 前端要用的字段」的转换，输出契约就是 web/ 前端
//! 读到的那份 JSON。前端要新字段时，只改这一处。
//!
//! 字段命名统一 camelCase（JS 习惯），与 Rust 侧的 snake_case 一一对应、不重排语义。

use amakano2_ui::{
    DeviceView, InstalledView, LogFilter, LogLevel, PackView, Page, ReadingStatsView, ResumeView,
    SessionStage, Snapshot, StatusKind, TransferMode, TransferView,
};
use serde_json::{Value, json};

fn page_name(page: Page) -> &'static str {
    match page {
        Page::Push => "push",
        Page::Saves => "saves",
        Page::Stats => "stats",
        Page::Account => "account",
        Page::Settings => "settings",
    }
}

fn stage_name(stage: SessionStage) -> &'static str {
    match stage {
        SessionStage::Idle => "idle",
        SessionStage::DeviceFound => "device-found",
        SessionStage::ChannelRegistered => "channel-registered",
        SessionStage::AppAlive => "app-alive",
        SessionStage::Handshaked => "handshaked",
        SessionStage::CatalogReady => "catalog-ready",
        SessionStage::Transferring => "transferring",
    }
}

fn status_kind(kind: StatusKind) -> &'static str {
    match kind {
        StatusKind::Info => "info",
        StatusKind::Good => "good",
        StatusKind::Warn => "warn",
        StatusKind::Bad => "bad",
    }
}

fn mode_name(mode: TransferMode) -> &'static str {
    match mode {
        TransferMode::Steady => "steady",
        TransferMode::Turbo => "turbo",
    }
}

fn log_level(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Debug | LogLevel::Info => "i",
        LogLevel::Warn => "w",
        LogLevel::Error => "e",
    }
}

fn device_json(d: &DeviceView) -> Value {
    json!({
        "name": d.name,
        "addr": d.addr,
        "connected": d.connected,
        "alive": d.alive,
        "probeAttempt": d.probe_attempt,
        "probeMax": d.probe_max,
    })
}

fn pack_line(title: &str) -> &'static str {
    if title.starts_with("共通") { "共通线" }
    else if title.starts_with("千岁") { "千岁线" }
    else if title.starts_with("玲") { "玲线" }
    else if title.starts_with("结灯") { "结灯线" }
    else if title.starts_with("番外") { "番外" }
    else { "资源" }
}

fn pack_json(p: &PackView) -> Value {
    json!({
        "number": p.number,
        "id": p.id,
        "title": p.title,
        "minutes": p.minutes,
        "bytes": p.bytes,
        "scenes": p.scenes,
        "dialogues": p.dialogues,
        "installed": p.installed,
        "queued": p.queued,
        "active": p.active,
        "line": pack_line(&p.title),
    })
}

fn installed_json(i: &InstalledView) -> Value {
    json!({ "id": i.id, "name": i.name, "bytes": i.bytes, "files": i.files, "stale": i.stale })
}

fn transfer_json(t: &TransferView) -> Value {
    json!({
        "chapter": t.chapter,
        "percent": t.percent,
        "received": t.received,
        "total": t.total,
        "chunksDone": t.chunks_done,
        "chunksTotal": t.chunks_total,
        "speedKbps": t.speed_kbps,
        "rttMs": t.rtt_ms,
        "retries": t.retries,
        "resumed": t.resumed,
        "ready": t.ready,
        "started": t.started,
        "stalled": t.stalled,
        "chunkBytes": t.chunk_bytes,
        "etaLabel": t.eta_label(),
        "speedLabel": t.speed_label(),
    })
}

fn resume_json(r: &ResumeView) -> Value {
    json!({
        "packId": r.pack_id,
        "chapter": r.chapter,
        "percent": r.percent,
        "received": r.received,
        "total": r.total,
        "filesDone": r.files_done,
    })
}

fn stats_json(s: &ReadingStatsView) -> Value {
    json!({
        "today": s.today,
        "totalDayCount": s.total_day_count,
        "longestDate": s.longest_date,
        "readingDaysLabel": s.reading_days_label,
        "totalDaysLabel": s.total_days_label,
        "totalLabel": s.total_label,
        "todayLabel": s.today_label,
        "longestLabel": s.longest_label,
        "recent": s.recent.iter().map(|d| json!({
            "date": d.date, "label": d.label, "seconds": d.seconds,
        })).collect::<Vec<_>>(),
    })
}

/// 整份快照 → 前端 JSON。
pub fn snapshot_json(s: &Snapshot) -> Value {
    json!({
        "version": s.version,
        "page": page_name(s.page),
        "stage": stage_name(s.stage),
        "error": s.error.as_ref().map(|e| json!({ "code": e.code.label(), "detail": e.detail })),
        "device": device_json(&s.device),
        "devices": s.devices.iter().map(|item| json!({
            "name": item.name,
            "addr": item.addr,
            "connected": item.connected,
            "alive": item.alive,
        })).collect::<Vec<_>>(),
        "selectedDevice": s.selected_device,
        "library": s.library.iter().map(pack_json).collect::<Vec<_>>(),
        "libraryError": s.library_error,
        "installed": s.installed.iter().map(installed_json).collect::<Vec<_>>(),
        "installedBroken": s.installed_broken,
        "transfer": s.transfer.as_ref().map(transfer_json),
        "resume": s.resume.as_ref().map(resume_json),
        "resumeChecking": s.resume_checking,
        "cacheBytes": s.cache_bytes,
        "cacheFiles": s.cache_files,
        "chunkBytes": s.chunk_bytes,
        "transferMode": mode_name(s.transfer_mode),
        "turboBlocked": s.turbo_blocked,
        "queue": s.queue,
        "status": s.status,
        "statusKind": status_kind(s.status_kind),
        "autoLaunch": s.auto_launch,
        "lineFilter": s.line_filter,
        // 存档原始对象（自动 + 手动槽），前端按现状渲染徽章，派生判定不缓存。
        "saveAuto": s.save_auto,
        "saveSlots": s.save_slots,
        "savesError": s.saves_error,
        "savesUnsupported": s.saves_unsupported,
        "savesBusy": s.saves_busy,
        "saveProtocol": s.save_protocol,
        "bandVersion": s.band_version,
        "bandGameLegacy": s.band_game_legacy,
        "savesNotice": s.saves_notice,
        "confirmDeleteSave": s.confirm_delete_save,
        "saveExport": s.save_export.as_ref().map(|e| json!({
            "bytes": e.bytes, "slots": e.slots, "verified": e.verified,
        })),
        "saveImport": s.save_import.as_ref().map(|i| json!({
            "incoming": i.incoming, "duplicates": i.duplicates, "existing": i.existing,
        })),
        "pendingCurrentSave": s.pending_current_save.as_ref().map(|save| json!({
            "slot": save.slot,
            "chapter": save.chapter,
            "scene": save.scene,
            "sceneLabel": save.scene_label(),
            "savedAt": save.saved_at,
            "timeLabel": save.time_label(),
            "packId": save.pack_id,
            "packScene": save.pack_scene,
            "missing": save.missing,
            "conflict": save.conflict,
        })),
        // 云存档 / 账号
        "cloudPhase": s.cloud_phase,
        "cloudDetail": s.cloud_detail,
        "cloudQqName": s.cloud_qq_name,
        "cloudAccountName": s.cloud_account_name,
        "cloudQq": s.cloud_qq,
        "cloudCode": s.cloud_code,
        "cloudExpiresAt": s.cloud_expires_at,
        "accountAuth": s.account_auth,
        // 官网与更新检查
        "website": s.website,
        "updateLatest": s.update_latest,
        "updateAvailable": s.update_available,
        "updateStatus": s.update_status,
        // 统计
        "stats": s.stats.as_ref().map(stats_json),
        "statsError": s.stats_error,
        "statsSupported": s.stats_supported,
        "statsBusy": s.stats_busy,
        // 日志
        "logs": s.logs.iter().map(|l| json!({ "level": log_level(l.level), "text": l.text })).collect::<Vec<_>>(),
        "logErrors": s.log_errors,
        "logWarns": s.log_warns,
        "logFilter": match s.log_filter {
            LogFilter::All => "all", LogFilter::Warn => "warn", LogFilter::Error => "error",
        },
        "brand": s.brand.as_ref().map(|b| b.to_string()),
    })
}
