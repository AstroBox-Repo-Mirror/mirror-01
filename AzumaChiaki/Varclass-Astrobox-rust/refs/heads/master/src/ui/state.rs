//! UI 状态管理
//!
//! 维护当前标签页、表单数据、选中的课程索引等，供 build 与 event_handler 共享。

use std::sync::{OnceLock, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabType {
    Add,
    Manage,
    Import,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    Json,
    Cses,
    ClassIsland,
    Wakeup,
    Ics,
}

impl ImportFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            ImportFormat::Json => "json",
            ImportFormat::Cses => "cses",
            ImportFormat::ClassIsland => "class_island",
            ImportFormat::Wakeup => "wakeup",
            ImportFormat::Ics => "ics",
        }
    }
    pub fn from_str(s: &str) -> Self {
        let s = s.trim().to_lowercase();
        if s.contains("class island") || s.contains("class_island") || s.contains("classisland") {
            ImportFormat::ClassIsland
        } else if s.contains("cses") {
            ImportFormat::Cses
        } else if s.contains("wakeup") {
            ImportFormat::Wakeup
        } else if s.contains("ics") || s.contains("ical") || s.contains("calendar") {
            ImportFormat::Ics
        } else {
            ImportFormat::Json
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CourseForm {
    pub day: String,
    pub name: String,
    pub room: String,
    pub start: String,
    pub end: String,
    pub week_type: String,
    /// 周数列表，用户输入格式: "1,2,3,5,7" 或 "1-3,5,7-9"
    pub weeks: String,
}

#[derive(Debug)]
pub struct UiState {
    pub root_element_id: Option<String>,
    pub current_tab: TabType,
    pub add_form: CourseForm,
    pub edit_form: CourseForm,
    pub selected_index: Option<usize>,
    /// 课程管理当前查看的星期（1-7）
    pub selected_day: u8,
    pub import_text: String,
    pub import_format: ImportFormat,
    pub message: Option<(String, bool)>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            root_element_id: None,
            current_tab: TabType::Add,
            add_form: CourseForm {
                day: "1".to_string(),
                name: String::new(),
                room: String::new(),
                start: "1".to_string(),
                end: "2".to_string(),
                week_type: "all".to_string(),
                weeks: String::new(),
            },
            edit_form: CourseForm {
                day: "1".to_string(),
                name: String::new(),
                room: String::new(),
                start: "1".to_string(),
                end: "2".to_string(),
                week_type: "all".to_string(),
                weeks: String::new(),
            },
            selected_index: None,
            selected_day: 1,
            import_text: String::new(),
            import_format: ImportFormat::Json,
            message: None,
        }
    }
}

static UI_STATE: OnceLock<RwLock<UiState>> = OnceLock::new();

pub fn ui_state() -> &'static RwLock<UiState> {
    UI_STATE.get_or_init(|| RwLock::new(UiState::default()))
}
