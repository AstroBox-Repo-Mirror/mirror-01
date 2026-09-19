use std::sync::{OnceLock, RwLock};
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq)]
pub enum TabType {
    CourseEditor,
    DataTransfer,
}

pub struct UiState {
    pub root_element_id: Option<String>,
    pub current_tab: TabType,
    pub course_data: String,
    pub transfer_message: Option<String>,
    pub is_success_message: bool,
    pub message_show_time: Option<SystemTime>,
    pub hovered_button: Option<String>,
}

static UI_STATE: OnceLock<RwLock<UiState>> = OnceLock::new();

pub fn ui_state() -> &'static RwLock<UiState> {
    UI_STATE.get_or_init(|| {
        RwLock::new(UiState {
            root_element_id: None,
            current_tab: TabType::CourseEditor,
            course_data: String::new(),
            transfer_message: None,
            is_success_message: false,
            message_show_time: None,
            hovered_button: None,
        })
    })
}

pub const TAB_COURSE_EDITOR: &str = "tab_course_editor";
pub const TAB_DATA_TRANSFER: &str = "tab_data_transfer";
pub const COURSE_DATA_INPUT_EVENT: &str = "course_data_input";
pub const OPEN_BROWSER_EVENT: &str = "open_browser";
pub const SEND_DATA_EVENT: &str = "send_data";
pub const BUTTON_MOUSE_LEAVE: &str = "button_mouse_leave";
pub const HIDE_MESSAGE_EVENT: &str = "hide_message";

 