//! UI 事件处理
//!
//! 将宿主派发的点击、输入等事件路由到对应的业务逻辑（添加/编辑/删除课程、同步、导入等）。

use crate::astrobox::psys_host::ui;
use crate::model::Course;
use crate::sync;
use crate::ui::build::{fill_edit_form_by_index, refresh_main_ui};
use crate::ui::state::{ui_state, CourseForm, ImportFormat, TabType};

pub const EVENT_TAB_ADD: &str = "tab_add";
pub const EVENT_TAB_MANAGE: &str = "tab_manage";
pub const EVENT_TAB_IMPORT: &str = "tab_import";

pub const EVENT_ADD_COURSE: &str = "event_add_course";
pub const EVENT_PULL_FROM_WATCH: &str = "event_pull_from_watch";
pub const EVENT_PUSH_TO_WATCH: &str = "event_push_to_watch";
pub const EVENT_SELECT_COURSE: &str = "event_select_course";
/// 星期按钮：event_day_1 .. event_day_7
pub const EVENT_DAY_PREFIX: &str = "event_day_";
/// 课程项：event_course_{index}
pub const EVENT_COURSE_PREFIX: &str = "event_course_";
pub const EVENT_SAVE_EDIT: &str = "event_save_edit";
pub const EVENT_DELETE_COURSE: &str = "event_delete_course";
pub const EVENT_IMPORT_PASTE: &str = "event_import_paste";

pub const INPUT_ADD_DAY: &str = "input_add_day";
pub const INPUT_ADD_NAME: &str = "input_add_name";
pub const INPUT_ADD_ROOM: &str = "input_add_room";
pub const INPUT_ADD_START: &str = "input_add_start";
pub const INPUT_ADD_END: &str = "input_add_end";
pub const INPUT_ADD_WEEK_TYPE: &str = "input_add_week_type";
pub const INPUT_ADD_WEEKS: &str = "input_add_weeks";

pub const INPUT_EDIT_DAY: &str = "input_edit_day";
pub const INPUT_EDIT_NAME: &str = "input_edit_name";
pub const INPUT_EDIT_ROOM: &str = "input_edit_room";
pub const INPUT_EDIT_START: &str = "input_edit_start";
pub const INPUT_EDIT_END: &str = "input_edit_end";
pub const INPUT_EDIT_WEEK_TYPE: &str = "input_edit_week_type";
pub const INPUT_EDIT_WEEKS: &str = "input_edit_weeks";

pub const INPUT_IMPORT_TEXT: &str = "input_import_text";
pub const INPUT_IMPORT_FORMAT: &str = "input_import_format";

fn parse_input_value(payload: &str) -> String {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(payload) {
        json.get("value")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        payload.to_string()
    }
}

fn resolve_event_id(event_id: &str, payload: &str) -> String {
    if !event_id.trim().is_empty() {
        return event_id.to_string();
    }
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(payload) {
        for key in ["id", "eventId", "event_id"] {
            if let Some(value) = json.get(key).and_then(|v| v.as_str()) {
                if !value.trim().is_empty() {
                    return value.to_string();
                }
            }
        }
    }
    String::new()
}

/// 解析周数字符串，支持 "1,2,3" 和 "1-3,5-9" 格式
fn parse_weeks_string(s: &str) -> Vec<u32> {
    let mut weeks = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if part.contains('-') {
            let range: Vec<&str> = part.split('-').collect();
            if range.len() == 2 {
                if let (Ok(start), Ok(end)) = (
                    range[0].trim().parse::<u32>(),
                    range[1].trim().parse::<u32>(),
                ) {
                    for w in start..=end {
                        if w > 0 {
                            weeks.push(w);
                        }
                    }
                }
            }
        } else if let Ok(w) = part.parse::<u32>() {
            if w > 0 {
                weeks.push(w);
            }
        }
    }
    weeks.sort();
    weeks.dedup();
    weeks
}

fn parse_form(form: &CourseForm) -> Result<Course, String> {
    let day: u8 = form
        .day
        .trim()
        .parse()
        .map_err(|_| "星期必须是 1-7".to_string())?;
    if !(1..=7).contains(&day) {
        return Err("星期必须是 1-7".to_string());
    }
    let name = form.name.trim().to_string();
    if name.is_empty() {
        return Err("课程名不能为空".to_string());
    }
    let start = form.start.trim().to_string();
    let end = form.end.trim().to_string();
    if start.is_empty() || end.is_empty() {
        return Err("开始/结束节次不能为空".to_string());
    }
    let weeks = parse_weeks_string(&form.weeks);

    Ok(Course {
        day,
        name,
        room: form.room.trim().to_string(),
        start,
        end,
        week_type: Course::normalize_week_type(Some(form.week_type.as_str())),
        weeks,
    })
}

pub fn set_status_message(message: String, is_error: bool) {
    let mut state = ui_state()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    state.message = Some((message, is_error));
}

fn handle_click(event_id: &str) {
    match event_id {
        EVENT_TAB_ADD => {
            let mut state = ui_state()
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.current_tab = TabType::Add;
        }
        EVENT_TAB_MANAGE => {
            let mut state = ui_state()
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.current_tab = TabType::Manage;
        }
        _ if event_id.starts_with(EVENT_DAY_PREFIX) => {
            if let Ok(day) = event_id.trim_start_matches(EVENT_DAY_PREFIX).parse::<u8>() {
                if (1..=7).contains(&day) {
                    let mut state = ui_state()
                        .write()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    state.selected_day = day;
                    state.selected_index = None;
                }
            }
        }
        _ if event_id.starts_with(EVENT_COURSE_PREFIX) => {
            if let Ok(idx) = event_id
                .trim_start_matches(EVENT_COURSE_PREFIX)
                .parse::<usize>()
            {
                fill_edit_form_by_index(idx);
            }
        }
        EVENT_TAB_IMPORT => {
            let mut state = ui_state()
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.current_tab = TabType::Import;
        }
        EVENT_ADD_COURSE => {
            let form = {
                let state = ui_state()
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                state.add_form.clone()
            };
            match parse_form(&form) {
                Ok(course) => {
                    sync::update_course(course, None);
                    set_status_message("已添加课程，可继续添加或推送到手环".to_string(), false);
                }
                Err(e) => set_status_message(e, true),
            }
        }
        EVENT_PULL_FROM_WATCH => {
            set_status_message("正在向手环请求课程...".to_string(), false);
            refresh_main_ui();
            let ret = wit_bindgen::block_on(async { sync::request_timetable_from_device().await });
            match ret {
                Ok(_) => set_status_message("请求已发送，请等待手环返回数据".to_string(), false),
                Err(e) => set_status_message(format!("请求失败: {}", e), true),
            }
        }
        EVENT_PUSH_TO_WATCH => {
            set_status_message("正在推送课程到手环...".to_string(), false);
            refresh_main_ui();
            let ret = wit_bindgen::block_on(async { sync::sync_cached_to_device().await });
            match ret {
                Ok(_) => set_status_message(sync::snapshot().status, false),
                Err(e) => set_status_message(format!("推送失败: {}", e), true),
            }
        }
        EVENT_SAVE_EDIT => {
            let (form, selected_index) = {
                let state = ui_state()
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                (state.edit_form.clone(), state.selected_index)
            };
            let Some(index) = selected_index else {
                set_status_message("请先在课程管理中选择要编辑的课程".to_string(), true);
                return;
            };
            match parse_form(&form) {
                Ok(course) => {
                    sync::update_course(course, Some(index));
                    set_status_message("课程修改已保存".to_string(), false);
                }
                Err(e) => set_status_message(e, true),
            }
        }
        EVENT_DELETE_COURSE => {
            let selected_index = {
                let state = ui_state()
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                state.selected_index
            };
            let Some(index) = selected_index else {
                set_status_message("请先在课程管理中选择要删除的课程".to_string(), true);
                return;
            };
            sync::delete_course(index);
            {
                let mut state = ui_state()
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                state.selected_index = None;
            }
            set_status_message("课程已删除".to_string(), false);
        }
        EVENT_IMPORT_PASTE => {
            let (text, format) = {
                let state = ui_state()
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                (state.import_text.clone(), state.import_format.as_str())
            };
            match sync::import_with_format(&text, format) {
                Ok(count) => set_status_message(format!("导入成功，共 {} 节课程", count), false),
                Err(e) => set_status_message(format!("导入失败: {}", e), true),
            }
        }
        _ => {}
    }
}

fn handle_change(event_id: &str, payload: &str) {
    let value = parse_input_value(payload);
    match event_id {
        INPUT_ADD_DAY => {
            let mut state = ui_state()
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.add_form.day = value;
        }
        INPUT_ADD_NAME => {
            let mut state = ui_state()
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.add_form.name = value;
        }
        INPUT_ADD_ROOM => {
            let mut state = ui_state()
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.add_form.room = value;
        }
        INPUT_ADD_START => {
            let mut state = ui_state()
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.add_form.start = value;
        }
        INPUT_ADD_END => {
            let mut state = ui_state()
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.add_form.end = value;
        }
        INPUT_ADD_WEEK_TYPE => {
            let mut state = ui_state()
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.add_form.week_type = value;
        }
        INPUT_ADD_WEEKS => {
            let mut state = ui_state()
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.add_form.weeks = value;
        }
        INPUT_EDIT_DAY => {
            let mut state = ui_state()
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.edit_form.day = value;
        }
        INPUT_EDIT_NAME => {
            let mut state = ui_state()
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.edit_form.name = value;
        }
        INPUT_EDIT_ROOM => {
            let mut state = ui_state()
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.edit_form.room = value;
        }
        INPUT_EDIT_START => {
            let mut state = ui_state()
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.edit_form.start = value;
        }
        INPUT_EDIT_END => {
            let mut state = ui_state()
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.edit_form.end = value;
        }
        INPUT_EDIT_WEEK_TYPE => {
            let mut state = ui_state()
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.edit_form.week_type = value;
        }
        INPUT_EDIT_WEEKS => {
            let mut state = ui_state()
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.edit_form.weeks = value;
        }
        INPUT_IMPORT_TEXT => {
            let mut state = ui_state()
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.import_text = value;
        }
        INPUT_IMPORT_FORMAT => {
            let mut state = ui_state()
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.import_format = ImportFormat::from_str(&value);
        }
        _ => {}
    }
}

/// 判断某个字段是否是自由文本录入控件（对应宿主的 Input 元素）。
/// 这些字段在实测中发现：宿主不仅在 Input 事件、也在 Change 事件时per-keystroke
/// 触发（并非只在失焦/提交时才触发一次 Change）。只要在这些事件后调用一次
/// `refresh_main_ui()` 重建整棵树，宿主就会连带重建输入框控件，打断输入法
/// （尤其是中文）组字状态，导致每次只能敲入一个字符。因此这里对它们完全不
/// 重渲染，回显交给输入框自身；内部状态仍然通过 `handle_change` 同步，切
/// 页/点击保存等操作时读到的都是最新值。
fn is_live_text_field(id: &str) -> bool {
    matches!(
        id,
        INPUT_ADD_DAY
            | INPUT_ADD_NAME
            | INPUT_ADD_ROOM
            | INPUT_ADD_START
            | INPUT_ADD_END
            | INPUT_ADD_WEEK_TYPE
            | INPUT_ADD_WEEKS
            | INPUT_EDIT_DAY
            | INPUT_EDIT_NAME
            | INPUT_EDIT_ROOM
            | INPUT_EDIT_START
            | INPUT_EDIT_END
            | INPUT_EDIT_WEEK_TYPE
            | INPUT_EDIT_WEEKS
            | INPUT_IMPORT_TEXT
    )
}

pub fn ui_event_processor(evtype: ui::Event, event_id: &str, event_payload: &str) {
    let resolved_event_id = resolve_event_id(event_id, event_payload);
    let event_id_ref = if resolved_event_id.is_empty() {
        event_id
    } else {
        &resolved_event_id
    };

    match evtype {
        ui::Event::Click | ui::Event::PointerUp => {
            handle_click(event_id_ref);
            refresh_main_ui();
        }
        ui::Event::Change | ui::Event::Input => {
            handle_change(event_id_ref, event_payload);
            if !is_live_text_field(event_id_ref) {
                refresh_main_ui();
            }
        }
        _ => {}
    }
}
