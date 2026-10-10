//! 手环 **回包信封** 的解包（`amakano.saves.data`）。
//!
//! ## 为什么单独成模块
//!
//! 2026-09 的真机回归：手环**真的把存档发过来了**（`hello-ok` 正常、回包也在日志里），
//! 插件界面却永远显示 0 条存档。原因只有一个 —— 插件从**外层**消息里读 `saves` /
//! `autoSave` / `slots`，而这三个字段其实在**里层**（`data` 那个 **JSON 字符串**里）。
//!
//! 这件事之所以难查，是因为它是**静默**的：`serde_json` 的 `get()` 取不到就返回 `None`，
//! 于是「一条都没读到」和「手环上真的一条都没有」在界面上长得一模一样。
//! 两侧的测试都没抓住它，因为**两侧各自编了样本**：手环侧测试自己拼 `{autoSave, slots, saves}`，
//! 插件侧测试也自己拼一份，谁都没拿**真实回包**当过夹具。
//!
//! 所以这个模块承担两件事：
//!
//! 1. **分片信封的解包**（下面的 [`band_envelope`]）：按 `seq` 累积、`total`/`last` 判完，
//!    拼成完整的 JSON 文本再解析。**单片（`total == 1`）走的是同一条路，没有特例** ——
//!    真实回包就是单片；一旦为单片写「直接读外层」的捷径，等存档涨到 9 KB 以上开始分片时
//!    同样的 bug 会原样复发。
//! 2. **真实回包夹具**（[`BAND_SAVES_FIXTURE`]）：真机日志里抓下来的原文，**逐字节保留**。
//!    插件侧的单测、预览 `demo()`、手环侧 `tests/save-sync.test.js` 全都用它当输入，
//!    从此「两侧样本各编一份」这条路被物理堵死。
//!
//! 解析失败一律返回 `SaveError`（**可读的中文原因**），绝不 panic、绝不静默变 0 条：
//! 静默失败正是这次 bug 难查的根源（见 `docs/插件开发注意事项.md`）。

use serde_json::Value;

use crate::SaveError;

/// 真机抓下来的回包原文（**逐字节保留**）。定义在 `fixture.rs`（由
/// `tools/gen-band-fixture.mjs` 生成），这里重新导出给插件侧的单测与预览 `demo()` 用。
pub use crate::fixture::BAND_SAVES_FIXTURE;

/// 与 [`BAND_SAVES_FIXTURE`] **同一份**原文在仓库里的落点。
///
/// 手环侧 `tests/save-sync.test.js` 直接读这个文件，所以它不能只是「Rust 里的一个字符串」：
/// 那份测试正是要靠它验「手环发出去的信封能不能被插件解出来」。有一个单测
/// （`fixture_matches_the_file_the_band_side_test_reads`）盯着两边逐字符一致。
#[cfg(not(target_arch = "wasm32"))]
pub const BAND_SAVES_FIXTURE_PATH: &str = "../../../tests/fixtures/band-saves-data.json";

/// 解包后的手环存档列表。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BandSaves {
    /// 自动存档（断点续读）。手环上没有时是 `None`。
    pub auto_save: Option<Value>,
    /// 手动槽，下标即槽号。
    pub slots: Vec<Value>,
    /// 里层 `slots` 字段（条数）。**只用于交叉核对**，界面一律以 `slots.len()` 为准。
    pub reported: Option<u64>,
    /// 解包过程中值得记一笔的事（目前只有「条数与数组长度不一致」）。
    /// 调用方负责写日志 —— 这里不依赖宿主的日志接口。
    pub warnings: Vec<String>,
}

impl BandSaves {
    /// 手动槽条数（数组长度优先）。
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty() && self.auto_save.is_none()
    }
}

/// 把一个分片信封的 `data` 字段反成**完整的 JSON 文本**。
///
/// `data` 是字符串（`{"…"}` 那一层）；手环侧有时会把 `data` 直接写成对象
/// （`JSON.stringify({str: …})` 与直接 `connection.send({data: {str}})` 两种写法历史上都出现过），
/// 所以这里两种都认，但**对象形态的 `seq`/`total` 语义与字符串形态完全一样**。
fn shard_text(shard: &Value, seq: u64) -> Result<String, SaveError> {
    match shard.get("data") {
        Some(Value::String(text)) => Ok(text.clone()),
        Some(Value::Null) | None => Err(SaveError::new(format!(
            "手环回包格式不对（第 {seq} 片）：缺少 data 字段，没法拼出存档列表"
        ))),
        Some(other) => Ok(other.to_string()),
    }
}

/// **分片信封 → 载荷对象**。唯一一处实现，单片与多片走同一条路。
///
/// 调用约定：`shards` 是**已经按 `seq` 排好序**的同一次回包的各个分片。
/// 累积逻辑在插件侧（`lib.rs` 的 `SAVE_SHARDS`，按 `seq` 落位、`total`/`last` 判完），
/// 这里只做「拼 + 解析 + 挑字段」，因此可以在宿主机上直接拿真机回包当夹具测。
pub fn band_envelope(shards: &[Value]) -> Result<BandSaves, SaveError> {
    if shards.is_empty() {
        return Err(SaveError::new("手环回包是空的：一片都没有收到"));
    }
    let total = shards[0]
        .get("total")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .unwrap_or(shards.len() as u64);
    if (shards.len() as u64) < total {
        return Err(SaveError::new(format!(
            "手环回包还没收全：只收到 {} 片，应该有 {total} 片",
            shards.len()
        )));
    }
    let mut text = String::new();
    for (index, shard) in shards.iter().enumerate() {
        let seq = shard.get("seq").and_then(Value::as_u64).unwrap_or(index as u64);
        text.push_str(&shard_text(shard, seq)?);
    }
    let value: Value = serde_json::from_str(&text).map_err(|error| {
        SaveError::new(format!(
            "手环回包格式不对（{} 片拼起来共 {} 字节，不是合法 JSON）：{error}",
            shards.len(),
            text.len()
        ))
    })?;
    let object = value.as_object().ok_or_else(|| {
        SaveError::new(format!(
            "手环回包格式不对（{} 片拼起来不是 JSON 对象）：看看是不是少收了一片",
            shards.len()
        ))
    })?;
    let slots = match object.get("saves") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items.iter().filter(|item| item.is_object()).cloned().collect(),
        Some(other) => {
            return Err(SaveError::new(format!(
                "手环回包格式不对：saves 应该是数组，实际是{}",
                json_kind(other)
            )));
        }
    };
    let auto_save = match object.get("autoSave") {
        None | Some(Value::Null) => None,
        Some(value) if value.is_object() => Some(value.clone()),
        Some(other) => {
            return Err(SaveError::new(format!(
                "手环回包格式不对：autoSave 应该是对象或 null，实际是{}",
                json_kind(other)
            )));
        }
    };
    let reported = object.get("slots").and_then(Value::as_u64);
    let mut warnings = Vec::new();
    // 条数只作交叉核对：**数组长度优先**。手环侧 `slots` 是 `slots.length` 的快照，
    // 理论上永远一致；不一致说明有一侧在骗人，界面上必须照实说，不能悄悄按其中一边算。
    if let Some(reported) = reported
        && reported != slots.len() as u64
    {
        let message = format!(
            "手环报的条数（slots={reported}）和实际解出来的数组长度（{}）不一致，按实际条数显示",
            slots.len()
        );
        // 插件与预览用的是同一套日志（tracing），所以这句在真机日志与预览里长得一样。
        tracing::warn!(reported, actual = slots.len(), "band save count mismatch");
        warnings.push(message);
    }
    Ok(BandSaves { auto_save, slots, reported, warnings })
}

/// 错误话里用的「它到底是个什么类型」。
pub(crate) fn json_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "布尔值",
        Value::Number(_) => "数字",
        Value::String(_) => "字符串",
        Value::Array(_) => "数组",
        Value::Object(_) => "对象",
    }
}

/// 把「手环回包（已解包）」拍成界面用的存档行：第 0 条固定是自动存档。
///
/// 与 `host/saves.rs` 的旧实现同一口径，只是**输入从「外层消息」换成了「解包后的载荷」**。
/// 字段解读只有一处实现（`SaveSlotView::from_json`），这里不重复第二份。
pub fn band_rows(auto_save: &Option<Value>, slots: &[Value], installed: &[String]) -> Vec<crate::SaveSlotView> {
    let mut rows = Vec::new();
    if let Some(auto) = auto_save {
        rows.push(crate::SaveSlotView::from_json(auto, None, installed));
    }
    rows.extend(crate::SaveSlotView::rows(slots, installed));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 把真机回包原文解析成外层消息（插件在 `lib.rs` 里收到的就是它）。
    fn fixture_message() -> Value {
        serde_json::from_str(BAND_SAVES_FIXTURE).expect("真机回包夹具本身必须是合法 JSON")
    }

    #[test]
    fn real_capture_unpacks_into_one_auto_save_and_three_manual_slots() {
        // 这条用例的存在理由：**它就是 2026-09 真机那个「界面显示 0 条」的 bug**。
        // 拿真机回包当夹具（而不是自己编一份），才可能发现「载荷在 data 字符串里」。
        let message = fixture_message();
        assert_eq!(message["type"], "amakano.saves.data");
        assert_eq!(message["requestId"], "saves-1789196046754");
        assert_eq!(message["seq"], 0);
        assert_eq!(message["total"], 1);
        assert_eq!(message["last"], true);
        // 外层**没有** saves/autoSave/slots —— 旧实现就是在这里读空的（永远 0 条）。
        assert!(message.get("saves").is_none(), "外层不该有 saves：载荷在 data 里");
        assert!(message.get("autoSave").is_none());
        assert!(message.get("slots").is_none());
        assert!(message["data"].is_string(), "data 是 JSON 字符串，需要再 parse 一次");

        let saves = band_envelope(&[message]).expect("真机回包必须能解包");
        assert_eq!(saves.len(), 3, "手环上有 3 条手动存档");
        assert_eq!(saves.reported, Some(3), "slots=3 是数字（条数），不是数组");
        assert!(saves.warnings.is_empty(), "真机回包两边一致，不该有 warning：{:?}", saves.warnings);

        let auto = saves.auto_save.as_ref().expect("必须有一条自动存档");
        assert_eq!(auto["chapterTitle"], "共通线 第一章·归乡");
        assert_eq!(auto["packId"], "共通线1·归乡");
        assert_eq!(auto["packScene"], 23);
        assert_eq!(auto["savedAt"], 1_789_196_000_769u64);

        // 槽位逐字段原样：三条槽号、场景号、写档时间都要对得上。
        assert_eq!(saves.slots[0]["packScene"], 1);
        assert_eq!(saves.slots[0]["savedAt"], 1_789_195_631_509u64);
        assert_eq!(saves.slots[1]["packScene"], 4);
        assert_eq!(saves.slots[1]["savedAt"], 1_789_195_982_418u64);
        assert_eq!(saves.slots[2]["packScene"], 26);
        assert_eq!(saves.slots[2]["savedAt"], 1_789_196_006_924u64);
        assert_eq!(saves.slots[2]["chapterTitle"], "共通线 第一章·归乡");

        // 拍成界面行：1 条自动存档 + 3 条手动槽，槽号就是数组下标。
        let installed = vec!["共通线1·归乡".to_string()];
        let rows = band_rows(&saves.auto_save, &saves.slots, &installed);
        assert_eq!(rows.len(), 4);
        assert!(rows[0].is_auto() && rows[0].title() == "自动存档");
        assert_eq!(rows[0].scene, 24, "展示用场景号 = currentScene + 1");
        assert_eq!(rows[0].pack_scene, 23, "包内偏移原样带出来，不改写");
        assert_eq!(rows[1].slot, Some(0));
        assert_eq!(rows[3].slot, Some(2));
        assert!(rows.iter().all(|row| !row.missing), "这一章装了，四条都该可读");
    }

    #[test]
    fn real_capture_marks_missing_pack_when_the_chapter_is_not_installed() {
        // 同一份真机回包，只是手环上**没装**这一章 —— 界面要标「未安装」并禁用读档。
        let saves = band_envelope(&[fixture_message()]).expect("真机回包必须能解包");
        let rows = band_rows(&saves.auto_save, &saves.slots, &[]);
        assert_eq!(rows.len(), 4);
        assert!(rows.iter().all(|row| row.missing && !row.readable()), "一条都不该可读");
    }

    #[test]
    fn shards_accumulate_by_seq_and_survive_out_of_order_arrival() {
        // 同一份载荷切成 2 片：**seq 乱序到达也要拼对**（插件是按 seq 落位的，
        // 不是按到达顺序 push 的）。这里把「落位」后的顺序数组交给解包函数，
        // 与 lib.rs 里 SAVE_SHARDS 的语义完全一致。
        let payload = {
            let message = fixture_message();
            message["data"].as_str().expect("data 是字符串").to_string()
        };
        let at = payload.len() / 3; // 切在字符边界不安全：这里是 ASCII/UTF-8 混排，所以按字节切
        let head = slice_utf8(&payload, 0, at);
        let tail = slice_utf8(&payload, at, payload.len());
        let shard0 = json!({ "type": "amakano.saves.data", "requestId": "r1", "seq": 0, "total": 2, "last": false, "data": head });
        let shard1 = json!({ "type": "amakano.saves.data", "requestId": "r1", "seq": 1, "total": 2, "last": true, "data": tail });

        // 到达顺序：1 → 0。插件按 seq 落位后交出来的是 [0, 1]。
        let mut arrived = vec![(1u64, shard1.clone()), (0u64, shard0.clone())];
        arrived.sort_by_key(|(seq, _)| *seq);
        let ordered: Vec<Value> = arrived.into_iter().map(|(_, shard)| shard).collect();

        let saves = band_envelope(&ordered).expect("分片拼起来必须是合法 JSON");
        assert_eq!(saves.len(), 3);
        assert_eq!(saves.slots[2]["savedAt"], 1_789_196_006_924u64);
        assert_eq!(saves.auto_save.as_ref().unwrap()["packScene"], 23);
        // 分片与单片解出来必须**一模一样**：这正是「不为单片写特例」的守护。
        assert_eq!(saves, band_envelope(&[fixture_message()]).unwrap());
    }

    /// 按字节切一个 UTF-8 字符串（切点必须落在字符边界上；这里用它来造分片）。
    fn slice_utf8(text: &str, from: usize, to: usize) -> String {
        let mut start = from;
        while start < text.len() && !text.is_char_boundary(start) {
            start += 1;
        }
        let mut end = to;
        while end < text.len() && !text.is_char_boundary(end) {
            end += 1;
        }
        text[start..end].to_string()
    }

    #[test]
    fn shard_problems_are_reported_in_plain_chinese() {
        let message = fixture_message();
        let payload = message["data"].as_str().unwrap().to_string();
        let half = slice_utf8(&payload, 0, payload.len() / 2);

        // 只收到一片但 total 说有两片：不能假装成功（那会解出半截 JSON）。
        let partial = json!({ "seq": 0, "total": 2, "last": false, "data": half });
        let reason = band_envelope(&[partial]).unwrap_err().message().to_string();
        assert!(reason.contains("还没收全"), "partial: {reason}");

        // 片里没有 data 字段：点名是第几片（这一片自己说自己有 1 片，绕过上面那条「没收全」）。
        let missing_data = json!({ "seq": 1, "total": 1, "last": true });
        let reason = band_envelope(&[missing_data]).unwrap_err().message().to_string();
        assert!(reason.contains("第 1 片") && reason.contains("data"), "missing_data: {reason}");

        // 拼起来不是合法 JSON：要说清「几片、多少字节」。
        let junk = json!({ "seq": 0, "total": 1, "last": true, "data": "{不是 JSON" });
        let reason = band_envelope(&[junk]).unwrap_err().message().to_string();
        assert!(reason.contains("不是合法 JSON"), "junk: {reason}");

        // 最外层不是对象（例如 data 里其实是一个数组）。
        let array = json!({ "seq": 0, "total": 1, "last": true, "data": "[1,2,3]" });
        let reason = band_envelope(&[array]).unwrap_err().message().to_string();
        assert!(reason.contains("不是 JSON 对象"), "array: {reason}");

        // 一片都没收到（不该崩，也不该当成「手环上没有存档」）。
        let reason = band_envelope(&[]).unwrap_err().message().to_string();
        assert!(reason.contains("一片都没有收到"), "empty: {reason}");
    }

    #[test]
    fn saves_field_and_count_mismatch_are_named_not_swallowed() {
        // saves 不是数组 → 报错（不是静默 0 条）。
        let wrong = json!({ "seq": 0, "total": 1, "last": true, "data": r#"{"saves":{},"slots":0}"# });
        let reason = band_envelope(&[wrong]).unwrap_err().message().to_string();
        assert!(reason.contains("saves 应该是数组") && reason.contains("对象"), "{reason}");

        // autoSave 不是对象 → 同样报错。
        let bad_auto = json!({ "seq": 0, "total": 1, "last": true, "data": r#"{"autoSave":[1],"saves":[]}"# });
        let reason = band_envelope(&[bad_auto]).unwrap_err().message().to_string();
        assert!(reason.contains("autoSave 应该是对象"), "{reason}");

        // 条数与数组长度不一致：**数组长度优先**，但必须留下一条 warn。
        let mismatch = json!({ "seq": 0, "total": 1, "last": true, "data": r#"{"saves":[{"savedAt":1},{"savedAt":2}],"slots":5}"# });
        let saves = band_envelope(&[mismatch]).expect("条数不一致不该让整次读取失败");
        assert_eq!(saves.len(), 2, "数组长度优先");
        assert_eq!(saves.reported, Some(5));
        assert_eq!(saves.warnings.len(), 1);
        assert!(saves.warnings[0].contains("按实际条数显示"), "{:?}", saves.warnings);

        // 手环上真的一条存档都没有：这是**正常**结果（autoSave 为 null、saves 为空），
        // 与「解析失败」必须能分清。
        let empty = json!({ "seq": 0, "total": 1, "last": true, "data": r#"{"autoSave":null,"slots":0,"saves":[]}"# });
        let saves = band_envelope(&[empty]).expect("空存档列表是正常结果");
        assert!(saves.is_empty());
        assert!(saves.auto_save.is_none());
    }

    #[test]
    fn fixture_matches_the_file_the_band_side_test_reads() {
        // 手环侧 `tests/save-sync.test.js` 读的是这个文件；两边必须逐字符一致，
        // 否则「同一份真机回包」又会变成「各编一份」（这次 bug 的根源）。
        // 文件以一个换行结尾（POSIX 习惯），比较时只去掉这一个换行。
        let text = std::fs::read_to_string(BAND_SAVES_FIXTURE_PATH)
            .unwrap_or_else(|error| panic!("读不到 {BAND_SAVES_FIXTURE_PATH}：{error}"));
        assert_eq!(text.trim_end_matches('\n'), BAND_SAVES_FIXTURE, "夹具与手环侧读的文件漂开了");

        // 文件解析出来的外层消息必须和常量一致（含 data 那一层字符串）。
        let from_file: Value = serde_json::from_str(&text).expect("夹具文件必须是合法 JSON");
        assert_eq!(from_file, fixture_message());
    }
}
