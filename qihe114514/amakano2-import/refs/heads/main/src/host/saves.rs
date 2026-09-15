//! 存档信封的解析 / 序列化、槽位合并、槽位列表 —— **纯逻辑，不碰宿主**。
//!
//! 宿主相关的封装在 `host/clipboard.rs`（导出/导入现在走剪贴板：Dialog 路线已作废，
//! 见 `docs/插件开发注意事项.md` 第 7 节）。
//!
//! 三条硬规矩（手环侧 `src/common/save-sync.js` 与这里必须一致）：
//!
//! 1. **槽位里的存档对象原样持有**（`serde_json::Value`），不映射成结构体。
//!    存档字段是手环应用定的（`storyId` / `chapter` / `chapterTitle` / `packId` /
//!    `packScene` / `currentScene` / `currentDialogue` / `savedAt` / `choice` /
//!    `routeState` / `settings`），以后手环侧加字段时导出导入不能把它们吃掉。
//! 2. **`chapter` 与 `currentScene` 绝不改写**：手环读档真正用的是
//!    `packId + packScene`（包内偏移，抗章节增减），这两个数只是给人看的。
//! 3. 解析失败一律返回 [`SaveError`]（可读原因），**不 panic** —— 用户在对话框里
//!    选错文件是常事，插件不该因此崩掉。

use serde::{Deserialize, Serialize};
use serde_json::Value;

// 失败原因的定义搬到了 `amakano2_ui::SaveError`：**回包解包**（ui-core 的 `saves` 模块）
// 与剪贴板导入（本文件）用的是同一套失败口径，放两处迟早会说成两句不一样的话。
// 这里原样转出去，`use crate::saves::SaveError` 的调用点不用改。
pub use amakano2_ui::SaveError;
// 「手环回包 → 界面存档行」也只有一份实现（ui-core 的 `saves` 模块，预览 `demo()` 用的是同一个函数）。
// 它**只在测试里转出去**：插件本体已经不自己算界面行了 —— 渲染时由 `Snapshot::saves()` 用
// 当前已安装章节列表现算（见 `docs/插件开发注意事项.md` 7.9）。非测试构建里挂一个没人用的
// `pub use` 只会让 wasm 构建报 `unused import`，还会让人以为插件侧另有一条算行子的路。
#[cfg(test)]
pub use amakano2_ui::band_rows;

/// 当前时间（毫秒）。
///
/// 定义在这里而不是 crate 根：`saves` 是唯一无条件模块，谁要用就 `saves::now_ms()`，
/// 不必在 crate 根上再开一个跨模块共享的名字（那会让 `host` 与 crate 根互相依赖）。
pub fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or_default()
}

/// 导出的存档文件格式标识。
pub const SAVE_FORMAT: &str = "amakano2-saves";
/// 存档信封自身的版本（与手环存档对象里的字段无关）。
pub const SAVE_VERSION: u32 = 1;
/// 与手环侧协商的协议版本（`amakano.app.hello-ok` 里的 protocol）。
pub const SAVE_PROTOCOL: u32 = 1;

/// 一个存档文件（导出信封 / 导入信封）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SaveFile {
    /// 固定为 [`SAVE_FORMAT`]；不是它就直接拒绝。
    pub format: String,
    pub save_version: u32,
    pub protocol: u32,
    /// 导出时刻（毫秒）。
    pub exported_at: u64,
    pub app_version: String,
    pub version_code: u32,
    /// 自动存档（断点续读）。没有时为 `None`。
    #[serde(default)]
    pub auto_save: Option<Value>,
    /// 手动存档，**下标即槽号**（手环侧就是这么存的）。
    #[serde(default)]
    pub slots: Vec<Value>,
}

impl SaveFile {
    /// 组一份待导出的信封。槽位对象原样带走。
    pub fn new(auto_save: Option<Value>, slots: Vec<Value>, app_version: String, version_code: u32) -> Self {
        Self {
            format: SAVE_FORMAT.into(),
            save_version: SAVE_VERSION,
            protocol: SAVE_PROTOCOL,
            exported_at: now_ms() as u64,
            app_version,
            version_code,
            auto_save,
            slots,
        }
    }

    /// 导出用的文本（缩进过的 JSON，方便用户自己看一眼）。
    pub fn to_json(&self) -> Result<String, SaveError> {
        serde_json::to_string_pretty(self).map_err(|error| SaveError::new(format!("存档信封序列化失败：{error}")))
    }

/// 给界面用的一句话摘要。
    pub fn summary(&self) -> String {
        let auto = if self.auto_save.is_some() { "含自动存档" } else { "无自动存档" };
        format!("{} 个手动存档 · {auto} · 信封版本 {}", self.slots.len(), self.save_version)
    }
}

/// 解析一段存档信封文本。`source` 是错误话里的主语（「剪贴板」/「文件」），
/// 只为把失败原因说成人话 —— 用户是从剪贴板粘进来的，就别跟他提「文件」。
///
/// 先去掉 BOM 与首尾空白，再按 JSON 解析，最后校验 `format` / `save_version`
/// 与槽位里每个元素的形状。任何一步失败都给出**能读懂的原因**，绝不 panic。
pub fn parse_save_text(text: &str, source: &str) -> Result<SaveFile, SaveError> {
    let text = text.trim_start_matches('\u{feff}').trim();
    if text.is_empty() {
        return Err(SaveError::new(format!("{source}里没有内容，没有可导入的存档")));
    }
    let value: Value = serde_json::from_str(text)
        .map_err(|_| SaveError::new(format!("{source}里的内容不是 JSON，请确认复制的是完整的存档文本（别只粘了一半）")))?;
    let object = value.as_object().ok_or_else(|| SaveError::new(format!("{source}里最外层应该是一个 JSON 对象")))?;
    let format = object.get("format").and_then(Value::as_str).unwrap_or("");
    if format != SAVE_FORMAT {
        return Err(SaveError::new(format!(
            "{source}里的 JSON 不是《甜蜜女友2》的存档（format 是「{}」，期望「{SAVE_FORMAT}」）",
            if format.is_empty() { "缺失" } else { format }
        )));
    }
    let save_version = object.get("save_version").and_then(Value::as_u64).unwrap_or(0) as u32;
    if save_version > SAVE_VERSION {
        return Err(SaveError::new(format!(
            "这份存档的信封版本是 {save_version}，这个插件只认到 {SAVE_VERSION}，请更新插件后再导入"
        )));
    }
    let slots = match object.get("slots") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items.clone(),
        Some(_) => return Err(SaveError::new("存档里的 slots 应该是数组")),
    };
    if slots.iter().any(|slot| !slot.is_object()) {
        return Err(SaveError::new("存档里有槽位不是对象，内容可能被改坏了"));
    }
    let auto_save = match object.get("auto_save") {
        None | Some(Value::Null) => None,
        Some(value) if value.is_object() => Some(value.clone()),
        Some(_) => return Err(SaveError::new("存档里的 auto_save 应该是对象")),
    };
    if slots.is_empty() && auto_save.is_none() {
        return Err(SaveError::new("这份存档里一个槽都没有（既没有手动存档也没有自动存档），没有可导入的内容"));
    }
    Ok(SaveFile {
        format: format.to_string(),
        save_version,
        protocol: object.get("protocol").and_then(Value::as_u64).unwrap_or(0) as u32,
        exported_at: object.get("exported_at").and_then(Value::as_u64).unwrap_or(0),
        app_version: object.get("app_version").and_then(Value::as_str).unwrap_or("未知").to_string(),
        version_code: object.get("version_code").and_then(Value::as_u64).unwrap_or(0) as u32,
        auto_save,
        slots,
    })
}

/// 一个槽位的写档时间（`savedAt`，毫秒）。手环侧同名字段就是用来判「同一份存档」的。
///
/// 取不到（字段缺失 / 不是数字）时返回 `None`：这种槽位**永远按新增处理**，
/// 因为放弃字段也不能凭空认成「重复」。
pub fn saved_at_of(slot: &Value) -> Option<u64> {
    slot.get("savedAt").and_then(Value::as_u64)
}

/// 导入合并的结果：要写回手环的槽位 + 给用户看的四个数。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotMerge {
    /// 写回手环的槽位。**不是**只挑新增的那些：手环侧的 `upsert` 会按 `savedAt`
    /// 自己决定覆盖还是追加，所以「重复」的那些也得发过去（覆盖才是用户要的合并）。
    pub slots: Vec<Value>,
    /// 信封里的槽位总数。
    pub incoming: usize,
    /// 其中手环上**已经有同一份**（`savedAt` 相同）的条数，会被覆盖。
    pub duplicates: usize,
    /// 手环上原有的手动槽条数（合并前）。
    pub existing: usize,
}

/// 按写档时间把导入的槽位与手环上已有的槽位合并（`upsert` 的口径）。
///
/// 手环侧的 `upsert` **不解释**存档对象、只比 `savedAt`，所以插件这边的「重复」判定
/// 必须用同一个键，两边的口径才不会漂。`same` 那些会写进去覆盖旧值 ——
/// 这不是浪费，正是「用备份把老进度换回来」所必需的一步。
pub fn merge_slots(existing: &[Value], incoming: &[Value]) -> SlotMerge {
    let known: Vec<u64> = existing.iter().filter_map(saved_at_of).collect();
    let duplicates = incoming
        .iter()
        .filter_map(saved_at_of)
        .filter(|moment| known.contains(moment))
        .count();
    SlotMerge {
        slots: incoming.to_vec(),
        incoming: incoming.len(),
        duplicates,
        existing: existing.len(),
    }
}

/// 槽位标识：自动存档是 `auto`，手动存档是槽号字符串。
///
/// 必须和 `ui-core` 的 `actions::save_key()` 保持一致 —— 界面用它拼动作 id，
/// 插件用它比对「哪一行在等二次确认」，两边写法不同就会变成「点了没反应」。
pub fn slot_key(slot: Option<usize>) -> String {
    match slot {
        Some(index) => index.to_string(),
        None => "auto".to_string(),
    }
}

/// 导出用的默认文件名，例如 `amakano2-saves-2026-09-13.json`。
///
/// **导出本身已经不用它了**（落点是剪贴板，没有文件名这回事），留着是为了给用户一个
/// 现成的名字：粘进备忘录 / 存成文本文件时，标题写这个最省事。纯函数，跟着一起测。
pub fn default_file_name(exported_at: u64) -> String {
    let seconds = exported_at / 1000;
    let days = seconds / 86_400;
    let (year, month, day) = amakano2_ui::civil_from_days(days as i64);
    format!("amakano2-saves-{year:04}-{month:02}-{day:02}.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn slot(pack: &str, scene: i64, saved_at: i64) -> Value {
        json!({
            "storyId": "visual-novel-template",
            "chapter": 2,
            "chapterTitle": "玲线1·序章",
            "packId": pack,
            "packScene": scene,
            "currentScene": scene + 140,
            "currentDialogue": 3,
            "savedAt": saved_at,
            "choice": [1, 0],
            "routeState": { "route": ["玲"] },
            "settings": { "textSpeed": 30, "textSize": 22, "autoPlaySpeed": 0 }
        })
    }

    fn envelope(slots: Vec<Value>) -> String {
        SaveFile::new(Some(slot("玲线1·序章", 12, 1_756_000_000_000)), slots, "0.1.0".into(), 58)
            .to_json()
            .unwrap()
    }

    #[test]
    fn round_trips_through_json_and_keeps_slot_objects_verbatim() {
        let original = slot("玲线1·序章", 12, 1_756_000_000_000);
        let parsed = parse_save_text(&envelope(vec![original.clone()]), "剪贴板").expect("必须能解析自己导出的信封");
        assert_eq!(parsed.format, SAVE_FORMAT);
        assert_eq!(parsed.save_version, SAVE_VERSION);
        assert_eq!(parsed.protocol, SAVE_PROTOCOL);
        // 槽位对象逐字段原样往返：以后手环加字段也不会被吃掉。
        assert_eq!(parsed.slots, vec![original.clone()]);
        assert_eq!(parsed.auto_save, Some(original));
        assert!(parsed.summary().contains("1 个手动存档"));
    }

    #[test]
    fn rejects_junk_with_a_readable_reason() {
        let reason = |text: &str| parse_save_text(text, "文件").unwrap_err().message().to_string();
        assert!(reason("   ").contains("里没有内容"));
        assert!(reason("[1,2,3]").contains("JSON 对象"));
        assert!(reason(r#"{"format":"other","slots":[]}"#).contains("不是《甜蜜女友2》"));
        // 版本比插件新要拒绝，而不是硬着头皮读。
        let future = format!(r#"{{"format":"{SAVE_FORMAT}","save_version":99,"slots":[{{"savedAt":1}}]}}"#);
        assert!(reason(&future).contains("信封版本"));
        // 空信封（既没手动也没自动）也算失败，避免「导入成功但什么都没变」。
        let empty = format!(r#"{{"format":"{SAVE_FORMAT}","save_version":1,"slots":[]}}"#);
        assert!(reason(&empty).contains("一个槽都没有"));
        // 槽位里混进非对象同样是坏文件。
        let broken = format!(r#"{{"format":"{SAVE_FORMAT}","save_version":1,"slots":["x"]}}"#);
        assert!(reason(&broken).contains("不是对象"));
        // slots 不是数组、auto_save 不是对象，都要说清楚。
        let wrong_slots = format!(r#"{{"format":"{SAVE_FORMAT}","save_version":1,"slots":{{}}}}"#);
        assert!(reason(&wrong_slots).contains("应该是数组"));
    }

    #[test]
    fn clipboard_parse_errors_talk_about_the_clipboard_and_name_each_failure() {
        // 导入改走剪贴板之后，失败画面上的每一句都必须**点名是哪一种错**：
        // 空 / 不是 JSON / 格式不对 / 版本过高 / 解析后一个槽都没有。
        let empty = parse_save_text("   \r\n", "剪贴板").unwrap_err().message().to_string();
        assert!(empty.contains("剪贴板") && empty.contains("没有内容"), "{empty}");

        let junk = parse_save_text("这不是 JSON", "剪贴板").unwrap_err().message().to_string();
        assert!(junk.contains("不是 JSON"), "{junk}");

        let other = parse_save_text(r#"{"format":"别的游戏","slots":[{"savedAt":1}]}"#, "剪贴板")
            .unwrap_err()
            .message()
            .to_string();
        assert!(other.contains("format") && other.contains(SAVE_FORMAT), "{other}");

        let future = parse_save_text(
            &format!(r#"{{"format":"{SAVE_FORMAT}","save_version":9,"slots":[{{"savedAt":1}}]}}"#),
            "剪贴板",
        )
        .unwrap_err()
        .message()
        .to_string();
        assert!(future.contains("信封版本是 9") && future.contains("更新插件"), "{future}");

        let none = parse_save_text(
            &format!(r#"{{"format":"{SAVE_FORMAT}","save_version":1,"slots":[]}}"#),
            "剪贴板",
        )
        .unwrap_err()
        .message()
        .to_string();
        assert!(none.contains("一个槽都没有"), "{none}");

        // 同样的文本从文件那条路读进来，话里就该说「文件」而不是「剪贴板」。
        let as_file = parse_save_text(&format!(r#"{{"format":"{SAVE_FORMAT}","save_version":9}}"#), "文件")
            .unwrap_err()
            .message()
            .to_string();
        assert!(!as_file.contains("剪贴板"), "{as_file}");

        // 正常文本照样能解析（含 BOM / 首尾空白：剪贴板里粘来的文本这两样都可能有）。
        let good = format!("\u{feff}\n  {}\n", envelope(vec![slot("玲线1·序章", 12, 7)]));
        let parsed = parse_save_text(&good, "剪贴板").expect("剪贴板里的信封必须能解析");
        assert_eq!(parsed.slots.len(), 1);
    }

    #[test]
    fn merge_counts_duplicates_by_saved_at_and_still_sends_them_for_overwrite() {
        let existing = vec![slot("玲线1·序章", 12, 1_000), slot("结灯线1·序章", 3, 2_000)];
        let incoming = vec![
            slot("玲线1·序章", 12, 1_000), // 与手环上第一槽同一份 → 覆盖
            slot("玲线1·序章", 30, 3_000), // 新
            slot("结灯线1·序章", 3, 9_000), // 新
        ];
        let merged = merge_slots(&existing, &incoming);
        assert_eq!(merged.incoming, 3);
        assert_eq!(merged.duplicates, 1, "只有 savedAt 相同的那一条算重复");
        assert_eq!(merged.existing, 2);
        // 重复的**也要**发回去：手环侧 upsert 靠它把旧进度覆盖回来。
        assert_eq!(merged.slots.len(), 3);
        assert_eq!(merged.slots, incoming);
        // 「写进去几条」＝ 文件条数 − 重复条数：这个等式就是界面文案的依据。
        assert_eq!(merged.incoming - merged.duplicates, 2);
    }

    #[test]
    fn merge_treats_missing_saved_at_as_new_and_survives_an_empty_band() {
        let timed = slot("玲线1·序章", 12, 1_000);
        let untimed = serde_json::json!({ "packId": "玲线1·序章", "packScene": 1 });
        assert_eq!(saved_at_of(&timed), Some(1_000));
        assert_eq!(saved_at_of(&untimed), None, "取不到写档时间");

        // 手环上没有存档：全是新增，一条都不算重复。
        let merged = merge_slots(&[], &[timed.clone(), untimed.clone()]);
        assert_eq!((merged.incoming, merged.duplicates, merged.existing), (2, 0, 0));

        // 手环上那些**取不到 savedAt** 的槽位不能把任何东西算成重复。
        let band = vec![serde_json::json!({ "packScene": 5 })];
        let merged = merge_slots(&band, &[timed]);
        assert_eq!(merged.duplicates, 0);
        assert_eq!(merged.existing, 1);
    }

    #[test]
    fn accepts_bom_and_auto_save_only_files() {
        let body = format!(r#"{{"format":"{SAVE_FORMAT}","save_version":1,"auto_save":{{"currentScene":9}}}}"#);
        let with_bom = format!("\u{feff}{body}");
        let parsed = parse_save_text(&with_bom, "剪贴板").expect("带 BOM 也要认");
        assert_eq!(parsed.slots.len(), 0);
        assert_eq!(parsed.auto_save.as_ref().unwrap()["currentScene"], 9);
        // 只有自动存档时摘要也要说清楚。
        assert!(parsed.summary().contains("含自动存档"));
    }

    #[test]
    fn band_rows_put_the_auto_save_first_and_flag_missing_packs() {
        let slots = vec![slot("玲线1·序章", 12, 1_756_000_000_000), slot("结灯线1·序章", 3, 0)];
        let installed = vec!["玲线1·序章".to_string()];
        let auto = Some(slot("共通线1·归乡", 30, 1_756_000_500_000));
        let rows = band_rows(&auto, &slots, &installed);

        assert_eq!(rows.len(), 3, "自动存档 + 两条手动槽");
        // 第 0 条一定是自动存档：界面靠这个决定哪一行不带「读档」。
        assert!(rows[0].is_auto());
        assert_eq!(rows[0].title(), "自动存档");
        assert!(rows[0].missing, "自动存档那章没装也要标出来");

        assert_eq!(rows[1].slot, Some(0));
        assert_eq!(rows[1].title(), "存档 1");
        assert_eq!(rows[1].chapter, "玲线1·序章");
        // 展示用场景号 = currentScene + 1（手环侧同款写法）。
        assert_eq!(rows[1].scene, 153);
        assert_eq!(rows[1].pack_scene, 12, "包内偏移原样带出来，不改写");
        assert!(rows[1].readable(), "包已安装就能读");
        assert_eq!(rows[1].time_label().len(), 11, "MM-DD HH:MM");

        assert_eq!(rows[2].slot, Some(1));
        assert!(rows[2].missing, "包没装必须标未安装");
        assert!(!rows[2].readable());
    }

    #[test]
    fn band_rows_never_rewrite_chapter_or_scene() {
        // 章节包升级后全局场景号会变，所以手环读档靠 packId + packScene。
        // 界面是只读的：chapter / chapterTitle / currentScene 原样展示，绝不改写。
        let mut value = slot("共通线1·归乡", 7, 1_700_000_000_000);
        value["chapterTitle"] = json!("共通线 第一章·归乡");
        value["chapter"] = json!(0);
        value["currentScene"] = json!(41);
        let rows = band_rows(&None, &[value], &[]);
        assert_eq!(rows.len(), 1, "没有自动存档时不该多出一行");
        assert_eq!(rows[0].chapter, "共通线 第一章·归乡");
        assert_eq!(rows[0].scene, 42);
        assert_eq!(rows[0].pack_scene, 7, "包内偏移是唯一被信任的定位信息");
    }

    #[test]
    fn slot_key_matches_the_ui_action_contract() {
        // 界面用 `save_key(槽)` 拼动作 id，插件用同一套键比对「哪一行在等确认」。
        assert_eq!(slot_key(None), "auto");
        assert_eq!(slot_key(Some(0)), "0");
        assert_eq!(slot_key(Some(19)), "19");
        assert_eq!(
            amakano2_ui::parse_action(&format!("save-delete:{}", slot_key(Some(2)))),
            Some(amakano2_ui::Action::SavesDelete(Some(2)))
        );
    }

    #[test]
    fn default_file_name_is_a_dated_json() {
        let name = default_file_name(946_684_800_000);
        assert_eq!(name, "amakano2-saves-2000-01-01.json");
        assert!(name.ends_with(".json"));
    }
}
