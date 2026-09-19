use crate::astrobox::psys_host;
use super::state::*;
use super::message::*;
use super::build::build_main_ui;

pub const TAB_COURSE_EDITOR: &str = "tab_course_editor";
pub const TAB_DATA_TRANSFER: &str = "tab_data_transfer";
pub const COURSE_DATA_INPUT_EVENT: &str = "course_data_input";
pub const OPEN_BROWSER_EVENT: &str = "open_browser";
pub const SEND_DATA_EVENT: &str = "send_data";
pub const BUTTON_MOUSE_LEAVE: &str = "button_mouse_leave";
pub const HIDE_MESSAGE_EVENT: &str = "hide_message";

pub fn handle_interconnect_message(payload: &str) {
    tracing::info!("收到手环端消息: {}", payload);
}

fn handle_input_event(event: &str, value: &str) {
    let mut state = ui_state()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let parsed_value = if let Ok(json) = serde_json::from_str::<serde_json::Value>(value) {
        json.get("value")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        value.to_string()
    };

    match event {
        COURSE_DATA_INPUT_EVENT => {
            state.course_data = parsed_value;
            state.transfer_message = None;
        }
        _ => {
            tracing::info!("未处理的事件类型");
        }
    }
}

fn handle_button_click(event: &str) {
    match event {
        TAB_COURSE_EDITOR => {
            let root_id: Option<String>;
            {
                let mut state = ui_state()
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                state.current_tab = TabType::CourseEditor;
                root_id = state.root_element_id.clone();
            }
            if let Some(root_id) = root_id {
                let ui = build_main_ui();
                crate::astrobox::psys_host::ui::render(&root_id, ui);
            }
        }
        TAB_DATA_TRANSFER => {
            let root_id: Option<String>;
            {
                let mut state = ui_state()
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                state.current_tab = TabType::DataTransfer;
                root_id = state.root_element_id.clone();
            }
            if let Some(root_id) = root_id {
                let ui = build_main_ui();
                crate::astrobox::psys_host::ui::render(&root_id, ui);
            }
        }
        OPEN_BROWSER_EVENT => {
            tracing::info!("打开浏览器");
            show_message("请手动前往 https://cte.waterflames.cn/", true);
        }
        SEND_DATA_EVENT => {
            let course_data = {
                let state = ui_state()
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                state.course_data.clone()
            };

            if course_data.is_empty() {
                show_error_message("配置信息未能读取");
                return;
            }

            tracing::info!("发送课程数据: {}", course_data);

            show_message("正在发送，请稍等···", false);

            let course_data_clone = course_data.clone();

            wit_bindgen::block_on(async move {
                if let Some(device_addr) = check_device().await {
                    if check_app_version(&device_addr).await {
                        if send_course_data(&device_addr, &course_data_clone).await {
                            let root_id: Option<String>;
                            {
                                let mut state = ui_state()
                                    .write()
                                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                                state.course_data.clear();
                                root_id = state.root_element_id.clone();
                            }
                            
                            if let Some(root_id) = root_id {
                                let ui = build_main_ui();
                                psys_host::ui::render(&root_id, ui);
                            }
                        }
                    }
                }
            });
        }
        HIDE_MESSAGE_EVENT => {
            let root_id: Option<String>;
            {
                let mut state = ui_state()
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                state.transfer_message = None;
                state.message_show_time = None;
                root_id = state.root_element_id.clone();
            }
            if let Some(root_id) = root_id {
                let ui = build_main_ui();
                crate::astrobox::psys_host::ui::render(&root_id, ui);
            }
        }
        _ => {
            tracing::info!("未处理的按钮点击事件");
        }
    }
}

fn handle_mouse_enter(event: &str) {
    let root_id: Option<String>;
    {
        let mut state = ui_state()
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.hovered_button = Some(event.to_string());
        root_id = state.root_element_id.clone();
    }
    if let Some(root_id) = root_id {
        let ui = build_main_ui();
        crate::astrobox::psys_host::ui::render(&root_id, ui);
    }
}

fn handle_mouse_leave() {
    let root_id: Option<String>;
    {
        let mut state = ui_state()
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.hovered_button = None;
        root_id = state.root_element_id.clone();
    }
    if let Some(root_id) = root_id {
        let ui = build_main_ui();
        crate::astrobox::psys_host::ui::render(&root_id, ui);
    }
}

pub fn ui_event_processor(
    event_type: crate::exports::astrobox::psys_plugin::event::Event,
    event_id: &str,
    event_payload: &str,
) {
    match event_type {
        crate::exports::astrobox::psys_plugin::event::Event::Click => {
            handle_button_click(event_id);
        }
        crate::exports::astrobox::psys_plugin::event::Event::Change => {
            handle_input_event(event_id, event_payload);
        }
        crate::exports::astrobox::psys_plugin::event::Event::Hover => {
            handle_mouse_enter(event_id);
        }
        _ => {
            tracing::info!("未处理的事件类型: {:?}", event_type);
        }
    }
    
    if event_id == BUTTON_MOUSE_LEAVE {
        handle_mouse_leave();
    }
}
