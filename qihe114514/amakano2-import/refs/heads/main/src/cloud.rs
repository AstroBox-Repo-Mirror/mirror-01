//! 云存档的身份迁移与三方合并。剧情对象始终保持 `Value`，云元数据只住在本模块。
//!
//! 本地观察快照用于识别删除和读取之后的本机修改；基线只在上次手环写回确认后
//! 前进。任一阶段失败都保留旧基线，下一次同步仍能看见未落实的本机变化。

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SCHEMA: u32 = 1;
/// 手环 `recoveryData` 是普通数组，没有 20 槽硬上限。真正的边界是总字节数与
/// 单槽字节数；`ui-core` 里的 20 只是列表长度的视觉建议值。
pub const MAX_TOMBSTONES: usize = 200;
pub const MAX_SAVE_BYTES: usize = 256 * 1024;
pub const MAX_SLOT_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SaveRecord {
    pub id: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "deletedAt")]
    pub deleted_at: Option<String>,
    pub save: Option<Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CloudDocument {
    pub schema: u32,
    pub auto: Option<SaveRecord>,
    #[serde(default)]
    pub slots: Vec<SaveRecord>,
}

impl CloudDocument {
    #[cfg(test)]
    pub fn empty() -> Self {
        Self { schema: SCHEMA, auto: None, slots: Vec::new() }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SCHEMA { return Err("云存档 schema 不受支持".into()); }
        if self.auto.as_ref().is_some_and(|record| record.id != "auto" || record.save.is_none()) {
            return Err("自动存档记录无效".into());
        }
        let mut ids = BTreeSet::new();
        let mut deleted = 0;
        for record in self.auto.iter().chain(&self.slots) {
            if record.id.is_empty() || record.id.len() > 128 || !record.id.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte)) || !ids.insert(&record.id) {
                return Err("云存档 ID 无效或重复".into());
            }
            if parse_time(&record.updated_at).is_none() { return Err("云存档 updatedAt 无效".into()); }
            match (&record.save, &record.deleted_at) {
                (None, Some(at)) if parse_time(at) == parse_time(&record.updated_at) => deleted += 1,
                (Some(save), None) if save.is_object() && serde_json::to_vec(save).is_ok_and(|bytes| bytes.len() <= MAX_SLOT_BYTES) => {}
                _ => return Err("云存档内容或删除记录无效".into()),
            }
        }
        for record in &self.slots {
            if record.id == "auto" { return Err("auto 不能用于手动槽".into()); }
        }
        if deleted > MAX_TOMBSTONES { return Err("云存档删除记录过多".into()); }
        if serde_json::to_vec(self).map_err(|error| error.to_string())?.len() > MAX_SAVE_BYTES {
            return Err("云存档超过 256 KiB".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BandSnapshot {
    #[serde(default)]
    pub revision: u64,
    #[serde(default, rename = "autoSave")]
    pub auto_save: Option<Value>,
    #[serde(default)]
    pub slots: Vec<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LocalRecord {
    pub id: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    pub save: Value,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LocalState {
    #[serde(default, rename = "identityMap")]
    pub identity_map: BTreeMap<String, String>,
    #[serde(default)]
    pub auto: Option<LocalRecord>,
    #[serde(default)]
    pub slots: Vec<LocalRecord>,
    #[serde(default)]
    pub baseline: CloudDocument,
    #[serde(default)]
    pub observed: BandSnapshot,
    /// 上次云端确认且手环写回核对成功的内容。删除只能相对它判定，不能相对最近一次读取。
    #[serde(default, rename = "syncedObserved")]
    pub synced_observed: BandSnapshot,
    /// 与 synced_observed 同序的稳定 ID；普通读取或失败写回不能修改它。
    #[serde(default, rename = "syncedIds")]
    pub synced_ids: Vec<String>,
    /// 本地自动存档相对已同步基线被显式删除；恢复旧云端内容不能把它复活。
    #[serde(default, rename = "autoDeletedAt")]
    pub auto_deleted_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Merged {
    pub document: CloudDocument,
    pub band: BandSnapshot,
    pub mapping: BTreeMap<String, String>,
    pub conflicts: usize,
}

pub trait CloudIds {
    fn next_id(&mut self) -> Result<String, String>;
}

#[cfg(target_arch = "wasm32")]
pub struct RandomIds;

#[cfg(target_arch = "wasm32")]
impl CloudIds for RandomIds {
    fn next_id(&mut self) -> Result<String, String> {
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| "无法生成安全云存档 ID".to_string())?;
        let mut id = String::from("slot:");
        for byte in bytes { id.push_str(&format!("{byte:02x}")); }
        Ok(id)
    }
}

fn valid_id(id: &str) -> bool {
    id.starts_with("slot:") && id.len() <= 128 && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
}

/// 将每份本机存档映射到稳定 ID。精确对象匹配可跨槽位移动，重复对象按顺序分配，
/// 不把 savedAt 当作永久身份。生成后立即由调用方原子持久化。
pub fn assign_ids(state: &mut LocalState, band: &BandSnapshot, now_ms: u128, ids: &mut impl CloudIds) -> Result<(), String> {
    let mut next = BTreeMap::new();
    let mut used = BTreeSet::new();
    let mut baseline = state.baseline.slots.iter().filter(|record| record.save.is_some()).cloned().collect::<Vec<_>>();
    let mut remaining = state.slots.clone();
    let mut assigned = Vec::new();
    for (index, save) in band.slots.iter().enumerate() {
        if !save.is_object() { return Err("手环手动存档不是对象".into()); }
        let mapped = state.identity_map.get(&index.to_string()).filter(|id| valid_id(id) && !used.contains(*id)).cloned();
        let unchanged = state.observed.slots.get(index) == Some(save);
        let previous = (!unchanged).then(|| remaining.iter().position(|record| record.save == *save).map(|at| remaining.remove(at))).flatten();
        let confirmed = (!unchanged && previous.is_none()).then(|| baseline.iter().position(|record| record.save.as_ref() == Some(save)).map(|at| baseline.remove(at))).flatten();
        let id = if unchanged {
            mapped.or_else(|| confirmed.map(|record| record.id)).or_else(|| previous.map(|record| record.id)).unwrap_or(ids.next_id()?)
        } else if let Some(id) = previous.map(|record| record.id).or_else(|| confirmed.map(|record| record.id)).or(mapped) {
            next.insert(index.to_string(), id.clone());
            id
        } else if let Some(id) = state.identity_map.get(&index.to_string()).filter(|id| valid_id(id) && !used.contains(*id)) {
            id.clone()
        } else {            loop {
                let id = ids.next_id()?;
                if used.insert(id.clone()) { break id; }
            }
        };
        used.insert(id.clone());
        next.insert(index.to_string(), id.clone());
        assigned.push(LocalRecord { id, updated_at: iso_time(now_ms), save: save.clone() });
    }
    state.identity_map = next;
    state.slots = assigned;
    if state.auto.as_ref().map(|record| &record.save) != band.auto_save.as_ref() {
        state.auto = band.auto_save.clone().map(|save| LocalRecord { id: "auto".into(), updated_at: iso_time(now_ms), save });
    }
    if band.auto_save.is_none() && state.synced_observed.auto_save.is_some() {
        state.auto_deleted_at = Some(iso_time(now_ms));
    } else if band.auto_save.is_some() {
        state.auto_deleted_at = None;
    }
    state.observed = band.clone();
    Ok(())
}

fn record_map(document: &CloudDocument) -> BTreeMap<String, SaveRecord> {
    document.slots.iter().map(|record| (record.id.clone(), record.clone())).collect()
}

fn newer<'a>(left: &'a SaveRecord, right: &'a SaveRecord) -> (&'a SaveRecord, Option<&'a SaveRecord>) {
    let ordering = (parse_time(&left.updated_at), serde_json::to_string(left).unwrap_or_default())
        .cmp(&(parse_time(&right.updated_at), serde_json::to_string(right).unwrap_or_default()));
    if ordering.is_lt() { (right, (left != right).then_some(left)) } else { (left, (left != right).then_some(right)) }
}

fn conflict_copy(record: &SaveRecord, now_ms: u128, ids: &mut impl CloudIds, used: &BTreeSet<String>) -> Result<SaveRecord, String> {
    let mut save = record.save.clone().ok_or_else(|| "冲突副本缺少剧情对象".to_string())?;
    save.as_object_mut().ok_or_else(|| "冲突副本不是剧情对象".to_string())?
        .insert("cloudConflict".into(), serde_json::json!({ "sourceId": record.id, "createdAt": iso_time(now_ms) }));
    loop {
        let id = ids.next_id()?;
        if !used.contains(&id) {
            return Ok(SaveRecord { id, updated_at: iso_time(now_ms), deleted_at: None, save: Some(save) });
        }
    }
}

/// 三方合并。只有本机相对观察快照的删除才产生 tombstone；首次迁移不能把手环上
/// 暂时不存在的云槽全部删掉。删除优先级高于同 ID 的旧内容，防止离线设备复活。
pub fn merge(local: &LocalState, band: &BandSnapshot, cloud: &CloudDocument, now_ms: u128, ids: &mut impl CloudIds) -> Result<Merged, String> {
    cloud.validate()?;
    let mut result = cloud.clone();
    result.schema = SCHEMA;
    let mut used: BTreeSet<String> = cloud.slots.iter().map(|record| record.id.clone()).collect();
    let mut conflicts = Vec::new();
    let local_auto_changed = local.baseline.auto.as_ref().and_then(|record| record.save.as_ref()) != band.auto_save.as_ref();
    let remote_auto_changed = result.auto != local.baseline.auto;
    if local.auto_deleted_at.is_some() && band.auto_save.is_none() && local.synced_observed.auto_save.is_some() {
        let deleted_at = local.auto_deleted_at.clone().unwrap_or_else(|| iso_time(now_ms));
        if let Some(remote) = result.auto.clone() {
            if parse_time(&remote.updated_at).is_some_and(|remote_at| parse_time(&deleted_at).is_some_and(|local_at| remote_at > local_at)) {
                result.auto = None;
                conflicts.push(conflict_copy(&remote, now_ms, ids, &used)?);
            } else {
                result.auto = None;
            }
        }
    } else if local_auto_changed || (local.baseline.schema == 0 && result.auto.is_none()) {
        if let Some(record) = &local.auto {
            let candidate = SaveRecord { id: "auto".into(), updated_at: record.updated_at.clone(), deleted_at: None, save: Some(record.save.clone()) };
            if let Some(remote) = result.auto.clone() {
                let (selected, loser) = newer(&remote, &candidate);
                if let Some(loser) = loser {
                    if remote_auto_changed && loser.save.is_some() {
                        let conflict = conflict_copy(loser, now_ms, ids, &used)?;
                        used.insert(conflict.id.clone());
                        conflicts.push(conflict);
                    }
                }
                result.auto = Some(selected.clone());
            } else {
                result.auto = Some(candidate);
            }
        }
    } else if remote_auto_changed {
        if let Some(remote) = result.auto.clone()
            && let Some(current) = band.auto_save.clone()
        {
            let candidate = SaveRecord { id: "auto".into(), updated_at: local.auto.as_ref().map(|record| record.updated_at.clone()).unwrap_or_else(|| iso_time(now_ms)), deleted_at: None, save: Some(current) };
            result.auto = Some(newer(&remote, &candidate).0.clone());
        }
    }

    let baseline = record_map(&local.baseline);
    let baseline_ids: BTreeSet<&str> = local.baseline.slots.iter().map(|record| record.id.as_str()).collect();
    let mut merged = record_map(&result);
    let current_ids: BTreeSet<&str> = local.slots.iter().map(|record| record.id.as_str()).collect();
    let deleted_ids: BTreeSet<String> = local.baseline.slots.iter().filter(|record| record.save.is_some())
        .filter(|record| !current_ids.contains(record.id.as_str())).map(|record| record.id.clone()).collect();
    for record in &local.slots {
        if deleted_ids.contains(&record.id) { continue; }
        let locally_changed = baseline.get(&record.id).is_none_or(|base| base.save.as_ref() != Some(&record.save));
        if !locally_changed { continue; }
        let candidate = SaveRecord { id: record.id.clone(), updated_at: record.updated_at.clone(), deleted_at: None, save: Some(record.save.clone()) };
        if merged.get(&record.id).is_some_and(|remote| remote.save.is_none()) { continue; }
        let selected = match merged.get(&record.id) {
            Some(existing) => {
                let (selected, loser) = newer(existing, &candidate);
                if let Some(loser) = loser.filter(|loser| selected.save.is_some() && loser.save.is_some()) {
                    let conflict = conflict_copy(loser, now_ms, ids, &used)?;
                    used.insert(conflict.id.clone());
                    conflicts.push(conflict);
                }
                selected.clone()
            }
            None => candidate,
        };
        merged.insert(record.id.clone(), selected);
    }
    let previous_ids = if local.synced_ids.len() == local.synced_observed.slots.len() {
        local.synced_ids.clone()
    } else {
        local.baseline.slots.iter().filter(|record| record.save.is_some()).map(|record| record.id.clone()).collect()
    };
    for (index, _) in local.synced_observed.slots.iter().enumerate() {
        let Some(id) = previous_ids.get(index) else { continue };
        if current_ids.contains(id.as_str()) { continue; }
        if !baseline_ids.contains(id.as_str()) || local.baseline.schema == 0 { continue; }
        let at = iso_time(now_ms);
        let tombstone = SaveRecord { id: id.clone(), updated_at: at.clone(), deleted_at: Some(at), save: None };
        let selected = match merged.get(id) {
            Some(existing) => {
                let (selected, loser) = newer(existing, &tombstone);
                if let Some(loser) = loser.filter(|loser| selected.save.is_some() && loser.save.is_some()) {
                    let conflict = conflict_copy(loser, now_ms, ids, &used)?;
                    used.insert(conflict.id.clone());
                    conflicts.push(conflict);
                }
                selected.clone()
            }
            None => tombstone,
        };
        merged.insert(id.clone(), selected);
    }
    let mut records: Vec<SaveRecord> = merged.into_values().collect();
    records.extend(conflicts.iter().cloned());
    records.sort_by(|left, right| left.id.cmp(&right.id));
    if records.iter().filter(|record| record.save.is_none()).count() > MAX_TOMBSTONES { return Err("合并后的删除记录过多".into()); }
    result.slots = records;
    result.validate()?;
    let slots = result.slots.iter().filter_map(|record| record.save.clone()).collect();
    let mapping = result.slots.iter().filter(|record| record.save.is_some()).enumerate()
        .map(|(index, record)| (index.to_string(), record.id.clone())).collect();
    Ok(Merged { document: result.clone(), band: BandSnapshot { revision: band.revision, auto_save: result.auto.and_then(|record| record.save), slots }, mapping, conflicts: conflicts.len() })
}

pub fn accepted(local: &LocalState, merged: &Merged) -> LocalState {
    let mut next = local.clone();
    next.identity_map = merged.mapping.clone();
    next.baseline = merged.document.clone();
    next.observed = merged.band.clone();
    next.synced_observed = merged.band.clone();
    next.synced_ids = merged.document.slots.iter().filter(|record| record.save.is_some()).map(|record| record.id.clone()).collect();
    next.auto_deleted_at = None;
    next.auto = merged.document.auto.clone().map(|record| LocalRecord { id: record.id, updated_at: record.updated_at, save: record.save.unwrap_or(Value::Null) });
    next.slots = merged.document.slots.iter().filter(|record| record.save.is_some()).map(|record| LocalRecord { id: record.id.clone(), updated_at: record.updated_at.clone(), save: record.save.clone().unwrap_or(Value::Null) }).collect();
    next
}

pub fn iso_time(milliseconds: u128) -> String {
    let seconds = (milliseconds / 1000).min(i64::MAX as u128) as i64;
    let days = seconds.div_euclid(86_400);
    let daytime = seconds.rem_euclid(86_400);
    let (year, month, day) = amakano2_ui::civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z", daytime / 3600, daytime % 3600 / 60, daytime % 60, milliseconds % 1000)
}

fn parse_time(text: &str) -> Option<i128> {
    let (date, time) = text.split_once('T')?;
    let mut date = date.split('-');
    let year = date.next()?.parse::<i64>().ok()?;
    let month = date.next()?.parse::<u32>().ok()?;
    let day = date.next()?.parse::<u32>().ok()?;
    if date.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) || !text.ends_with('Z') { return None; }
    let time = time.strip_suffix('Z')?;
    let (clock, fraction) = time.split_once('.').unwrap_or((time, ""));
    let mut clock = clock.split(':');
    let hour = clock.next()?.parse::<i128>().ok()?;
    let minute = clock.next()?.parse::<i128>().ok()?;
    let second = clock.next()?.parse::<i128>().ok()?;
    if clock.next().is_some() || hour > 23 || minute > 59 || second > 59 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) || fraction.len() > 9 { return None; }
    let days = days_from_civil(year, month, day)?;
    Some(((days as i128 * 86_400 + hour * 3600 + minute * 60 + second) * 1_000_000_000) + i128::from(fraction_to_nanos(fraction)))
}

fn fraction_to_nanos(fraction: &str) -> i64 {
    let mut nanos = 0;
    let mut factor = 100_000_000;
    for byte in fraction.bytes() { nanos += i64::from(byte - b'0') * factor; factor /= 10; }
    nanos
}

fn days_from_civil(year: i64, month: u32, day: u32) -> Option<i64> {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let month = i64::from(month);
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let roundtrip_year = year + i64::from(month <= 2);
    let seconds = (days as i128 * 86_400) as i64;
    let (actual_year, actual_month, actual_day) = amakano2_ui::civil_from_days(seconds.div_euclid(86_400));
    ((actual_year, actual_month, actual_day) == (roundtrip_year, month as u32, day)).then_some(days)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Ids(u32);
    impl CloudIds for Ids {
        fn next_id(&mut self) -> Result<String, String> { self.0 += 1; Ok(format!("slot:{:032x}", self.0)) }
    }
    fn save(scene: u64) -> Value { json!({ "packId": "共通线", "packScene": scene, "savedAt": scene, "future": { "keep": true } }) }
    fn snapshot(scenes: &[u64]) -> BandSnapshot { BandSnapshot { revision: 0, auto_save: None, slots: scenes.iter().map(|scene| save(*scene)).collect() } }
    fn local(scenes: &[u64], at: u128) -> LocalState {
        let band = snapshot(scenes);
        let mut state = LocalState::default();
        assign_ids(&mut state, &band, at, &mut Ids(0)).unwrap();
        state
    }
    fn record(id: &str, scene: u64, at: u128) -> SaveRecord { SaveRecord { id: id.into(), updated_at: iso_time(at), deleted_at: None, save: Some(save(scene)) } }

    #[test]
    fn ids_survive_reordering_without_rewriting_story_fields() {
        let mut state = local(&[1, 2], 1000);
        let ids: Vec<String> = state.slots.iter().map(|record| record.id.clone()).collect();
        assign_ids(&mut state, &snapshot(&[2, 1]), 2000, &mut Ids(100)).unwrap();
        assert_eq!(state.slots.iter().map(|record| record.id.clone()).collect::<Vec<_>>(), vec![ids[1].clone(), ids[0].clone()]);
        assert_eq!(state.slots[0].save, save(2));
    }

    #[test]
    fn overwrite_keeps_slot_identity_and_offline_stale_copy_cannot_resurrect_delete() {
        let mut state = local(&[1], 1000);
        let first = merge(&state, &snapshot(&[1]), &CloudDocument::empty(), 1000, &mut Ids(10)).unwrap();
        state = accepted(&state, &first);
        let id = state.slots[0].id.clone();
        assign_ids(&mut state, &snapshot(&[5]), 2000, &mut Ids(20)).unwrap();
        assert_eq!(state.slots[0].id, id);
        let replaced = merge(&state, &snapshot(&[5]), &first.document, 3000, &mut Ids(30)).unwrap();
        state = accepted(&state, &replaced);
        let empty = snapshot(&[]);
        assign_ids(&mut state, &empty, 3500, &mut Ids(35)).unwrap();
        let removed = merge(&state, &empty, &replaced.document, 4000, &mut Ids(40)).unwrap();
        assert!(removed.document.slots[0].save.is_none());
        let stale = merge(&state, &snapshot(&[5]), &removed.document, 4000, &mut Ids(50)).unwrap();
        assert!(stale.document.slots[0].save.is_none(), "同刻离线旧内容不能复活删除");
    }

    #[test]
    fn offline_deletion_tombstones_cloud_instead_of_resurrecting() {
        let mut state = local(&[1, 2], 1000);
        let band = snapshot(&[1, 2]);
        let first = merge(&state, &band, &CloudDocument::empty(), 1000, &mut Ids(10)).unwrap();
        state = accepted(&state, &first);
        let deleted = snapshot(&[1]);
        assign_ids(&mut state, &deleted, 2000, &mut Ids(20)).unwrap();
        let merged = merge(&state, &deleted, &first.document, 3000, &mut Ids(30)).unwrap();
        assert_eq!(merged.band.slots, vec![save(1)], "state={state:?} merged={merged:?}");
        assert_eq!(merged.document.slots.iter().filter(|record| record.save.is_none()).count(), 1);
        let final_state = accepted(&state, &merged);
        let offline = merge(&final_state, &merged.band, &merged.document, 4000, &mut Ids(40)).unwrap();
        assert_eq!(offline.document.slots.iter().filter(|record| record.save.is_none()).count(), 1);
    }

    #[test]
    fn remote_and_local_new_slots_both_survive_and_fields_stay_verbatim() {
        let mut state = local(&[1], 1000);
        let first = merge(&state, &snapshot(&[1]), &CloudDocument::empty(), 1000, &mut Ids(10)).unwrap();
        state = accepted(&state, &first);
        let mut cloud = first.document.clone();
        cloud.slots.push(record("slot:remote", 2, 2000));
        let merged = merge(&state, &first.band, &cloud, 3000, &mut Ids(20)).unwrap();
        assert_eq!(merged.band.slots.len(), 2);
        assert!(merged.band.slots.contains(&save(1)));
        assert!(merged.band.slots.contains(&save(2)));
        assert_eq!(merged.document.slots[0].save.as_ref().unwrap()["future"], json!({ "keep": true }));
    }

    #[test]
    fn reread_does_not_make_local_timestamp_override_remote_progress() {
        let mut state = local(&[1], 1000);
        let first = merge(&state, &snapshot(&[1]), &CloudDocument::empty(), 1000, &mut Ids(10)).unwrap();
        state = accepted(&state, &first);
        assign_ids(&mut state, &first.band, 9000, &mut Ids(20)).unwrap();
        let mut cloud = first.document.clone();
        cloud.slots[0].save = Some(save(9));
        cloud.slots[0].updated_at = iso_time(2000);
        let merged = merge(&state, &first.band, &cloud, 10_000, &mut Ids(30)).unwrap();
        assert_eq!(merged.band.slots, vec![save(9)]);
    }

    #[test]
    fn auto_takes_newer_and_equal_content_conflicts_get_a_copy() {
        let mut state = local(&[1], 1000);
        state.auto = Some(LocalRecord { id: "auto".into(), updated_at: iso_time(2000), save: save(7) });
        state.observed.auto_save = Some(save(6));
        let band = BandSnapshot { revision: 0, auto_save: Some(save(7)), slots: vec![save(1)] };
        let cloud = CloudDocument { schema: 1, auto: Some(record("auto", 6, 1000)), slots: vec![record(&state.slots[0].id, 2, 1000)] };
        let merged = merge(&state, &band, &cloud, 3000, &mut Ids(20)).unwrap();
        assert_eq!(merged.band.auto_save, Some(save(7)));
        assert_eq!(merged.conflicts, 2);
        assert_eq!(merged.band.slots.len(), 3);
        assert!(merged.document.slots.iter().any(|record| record.save.as_ref().is_some_and(|save| save.get("cloudConflict").is_some())));
    }

    #[test]
    fn auto_concurrent_edits_preserve_loser_as_manual_conflict() {
        let mut state = local(&[], 1000);
        let first_band = BandSnapshot { revision: 0, auto_save: Some(save(1)), slots: vec![] };
        assign_ids(&mut state, &first_band, 1000, &mut Ids(10)).unwrap();
        let first = merge(&state, &first_band, &CloudDocument::empty(), 1000, &mut Ids(11)).unwrap();
        state = accepted(&state, &first);
        let local_band = BandSnapshot { revision: 0, auto_save: Some(save(2)), slots: vec![] };
        assign_ids(&mut state, &local_band, 2000, &mut Ids(20)).unwrap();
        let mut cloud = first.document.clone();
        cloud.auto = Some(record("auto", 3, 3000));
        let merged = merge(&state, &local_band, &cloud, 4000, &mut Ids(30)).unwrap();
        assert_eq!(merged.band.auto_save, Some(save(3)));
        assert_eq!(merged.conflicts, 1, "merged={merged:?} baseline={:?} cloud={:?} changed={}", state.baseline.auto, cloud.auto, cloud.auto != state.baseline.auto);
        assert!(merged.band.slots.contains(&save(2)) == false, "副本必须带冲突来源元数据");
        assert!(merged.document.slots.iter().any(|record| record.save.as_ref().is_some_and(|save| save["packScene"] == 2 && save.get("cloudConflict").is_some())));
    }

    #[test]
    fn auto_delete_does_not_resurrect_and_remote_edit_leaves_conflict_copy() {
        let mut state = local(&[], 1000);
        let first_band = BandSnapshot { revision: 0, auto_save: Some(save(1)), slots: vec![] };
        assign_ids(&mut state, &first_band, 1000, &mut Ids(10)).unwrap();
        let first = merge(&state, &first_band, &CloudDocument::empty(), 1000, &mut Ids(11)).unwrap();
        state = accepted(&state, &first);
        let deleted = BandSnapshot::default();
        assign_ids(&mut state, &deleted, 2000, &mut Ids(20)).unwrap();
        let removed = merge(&state, &deleted, &first.document, 3000, &mut Ids(30)).unwrap();
        assert_eq!(removed.band.auto_save, None);
        let mut cloud = first.document.clone();
        cloud.auto = Some(record("auto", 4, 4000));
        let concurrent = merge(&state, &deleted, &cloud, 5000, &mut Ids(40)).unwrap();
        assert_eq!(concurrent.band.auto_save, None);
        assert_eq!(concurrent.conflicts, 1);
        assert!(concurrent.document.slots.iter().any(|record| record.save.as_ref().is_some_and(|save| save["packScene"] == 4)));
    }

    #[test]
    fn more_than_twenty_slots_merge_instead_of_being_rejected() {
        let scenes: Vec<u64> = (1..=21).collect();
        let mut state = local(&scenes, 1000);
        let band = snapshot(&scenes);
        let merged = merge(&state, &band, &CloudDocument::empty(), 1000, &mut Ids(100)).unwrap();
        assert_eq!(merged.document.slots.len(), 21);
        state = accepted(&state, &merged);
        // 第二台设备再拿到同一份 21 槽也不会在合并阶段失败。
        let again = merge(&state, &merged.band, &merged.document, 2000, &mut Ids(200)).unwrap();
        assert_eq!(again.band.slots.len(), 21);
    }

    #[test]
    fn timestamp_round_trip_and_size_guards_are_deterministic() {
        for at in [0, 1, 1_700_000_000_123, 1_800_000_000_999] {
            assert_eq!(parse_time(&iso_time(at)), Some(at as i128 * 1_000_000));
        }
        assert!(parse_time("not-a-time").is_none());
        let mut state = local(&[1], 1000);
        state.slots[0].save = json!({ "text": "x".repeat(MAX_SLOT_BYTES) });
        assert!(merge(&state, &snapshot(&[1]), &CloudDocument::empty(), 2000, &mut Ids(10)).is_err());
    }
}
