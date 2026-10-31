//! 两侧共用的**真机回包夹具** —— 由 `tools/gen-band-fixture.mjs` 生成，别手改。
//!
//! 为什么要单独一个文件：2026-09 的真机回归（插件界面永远 0 条）根因是
//! **两侧各自编样本**，谁都没拿真实回包当夹具。现在原文只写一次
//! （`tools/gen-band-fixture.mjs` 里的 `CAPTURED_DATA`），生成的字符串同时喂给：
//!
//! * 插件侧单测与预览 `demo()`（本文件）；
//! * 手环侧 `tests/save-sync.test.js`（`tests/fixtures/band-saves-data.json`）。
//!
//! 两份生成物逐字节一致，另外有单测盯着（见 `saves.rs` 的
//! `fixture_matches_the_file_the_band_side_test_reads`）。
//! **要改就先重新抓一份真机日志**，然后跑 `node tools/gen-band-fixture.mjs`。

/// 手环回给插件的 `amakano.saves.data` 原文（`data.str` 那一层），**逐字节保留**。
///
/// 三层结构，缺一不可：
///
/// ```text
/// 外层（分片信封）  { "type", "requestId", "seq", "total", "last", "data" }
///   └─ data 是**字符串**，内容是一份完整的 JSON 文本（要再 parse 一次）
///        └─ 里层（载荷） { "autoSave": 对象|null, "slots": 条数(数字), "saves": [存档对象…] }
/// ```
///
/// ⚠️ 旧实现直接从**外层**读 `saves` / `autoSave` / `slots`，于是永远读到 0 条 ——
/// 这份夹具就是那次事故的证据。
pub const BAND_SAVES_FIXTURE: &str = r#"{
  "type": "amakano.saves.data",
  "requestId": "saves-1789196046754",
  "seq": 0,
  "total": 1,
  "last": true,
  "data": "{\"autoSave\":{\"storyId\":\"visual-novel-template\",\"chapter\":0,\"chapterTitle\":\"共通线 第一章·归乡\",\"packId\":\"共通线1·归乡\",\"packScene\":23,\"currentDialogue\":0,\"currentScene\":23,\"savedAt\":1789196000769,\"choice\":[],\"routeState\":{},\"settings\":{\"textSpeed\":25,\"textSize\":22,\"autoPlaySpeed\":\"medium\"}},\"slots\":3,\"saves\":[{\"storyId\":\"visual-novel-template\",\"chapter\":0,\"chapterTitle\":\"共通线 第一章·归乡\",\"packId\":\"共通线1·归乡\",\"packScene\":1,\"currentDialogue\":2,\"currentScene\":1,\"savedAt\":1789195631509,\"choice\":[],\"routeState\":{},\"settings\":{\"textSpeed\":25,\"textSize\":22,\"autoPlaySpeed\":\"medium\"}},{\"storyId\":\"visual-novel-template\",\"chapter\":0,\"chapterTitle\":\"共通线 第一章·归乡\",\"packId\":\"共通线1·归乡\",\"packScene\":4,\"currentDialogue\":0,\"currentScene\":4,\"savedAt\":1789195982418,\"choice\":[],\"routeState\":{},\"settings\":{\"textSpeed\":25,\"textSize\":22,\"autoPlaySpeed\":\"medium\"}},{\"storyId\":\"visual-novel-template\",\"chapter\":0,\"chapterTitle\":\"共通线 第一章·归乡\",\"packId\":\"共通线1·归乡\",\"packScene\":26,\"currentDialogue\":0,\"currentScene\":26,\"savedAt\":1789196006924,\"choice\":[],\"routeState\":{},\"settings\":{\"textSpeed\":25,\"textSize\":22,\"autoPlaySpeed\":\"medium\"}}]}"
}"#;
