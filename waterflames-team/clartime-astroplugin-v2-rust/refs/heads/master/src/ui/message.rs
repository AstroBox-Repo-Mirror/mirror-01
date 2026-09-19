use crate::astrobox::psys_host::{self, device, interconnect, register, thirdpartyapp};
use std::time::{Duration, SystemTime};
use super::state::*;
use super::build::build_main_ui;

pub fn show_message(msg: &str, is_success: bool) {
    let root_id: Option<String>;
    {
        let mut state = ui_state()
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.transfer_message = Some(msg.to_string());
        state.is_success_message = is_success;
        state.message_show_time = Some(SystemTime::now());
        root_id = state.root_element_id.clone();
    }
    if let Some(root_id) = root_id {
        let ui = build_main_ui();
        psys_host::ui::render(&root_id, ui);
    }
}

pub fn check_and_hide_message() {
    let should_hide = {
        let state = ui_state()
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        
        if let Some(show_time) = state.message_show_time {
            if let Ok(elapsed) = show_time.elapsed() {
                if elapsed >= Duration::from_secs(3) {
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        }
    };
    
    if should_hide {
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
            psys_host::ui::render(&root_id, ui);
        }
    }
}

pub fn show_error_message(msg: &str) {
    show_message(msg, false);
}

pub fn show_success_message(msg: &str) {
    show_message(msg, true);
}

pub async fn check_device() -> Option<String> {
    let device_list = device::get_connected_device_list().await;
    if let Some(device) = device_list.first() {
        tracing::info!("device: {:?}", device_list);
        let device_addr = device.addr.clone();
        tracing::info!("device_addr: {:?}", device_addr);
        Some(device_addr)
    } else {
        show_error_message("请先安装澄序课程表快应用/连接设备");
        None
    }
}

pub async fn check_app_version(device_addr: &str) -> bool {
    let app_list = thirdpartyapp::get_thirdparty_app_list(device_addr).await;

    if let Ok(apps) = app_list {
        tracing::info!("app: {:?}", apps);
        let app = apps.iter().find(|app: &&thirdpartyapp::AppInfo| {
            app.package_name == "com.waterflames.clartime"
        });
        if let Some(app) = app {
            let _ = thirdpartyapp::launch_qa(device_addr, app, "/").await;
            std::thread::sleep(Duration::from_secs(2));
            true
        } else {
            show_error_message("请先安装澄序课程表快应用");
            false
        }
    } else {
        show_error_message("获取应用列表失败");
        false
    }
}

pub async fn send_course_data(device_addr: &str, course_data: &str) -> bool {
    ensure_interconnect_registered(device_addr).await;
    
    match serde_json::from_str::<serde_json::Value>(course_data) {
        Ok(_) => {
            let result = interconnect::send_qaic_message(device_addr, "com.waterflames.clartime", course_data).await;
            if let Ok(_) = result {
                show_success_message("发送成功，如果手环上出现数据加载异常/黑屏，大概率是数据问题，请自行检查；如果手环没有任何反应，请检查是否进入相关传输页面");
                true
            } else {
                show_error_message("发送失败，请检查：1.课程数据是否符合json格式/数据是否完整｜2.连接手环后，传输数据前，是否已经重启应用并进入了相关传输界面");
                false
            }
        }
        Err(_) => {
            show_error_message("配置信息未能读取，请检查课程数据是否符合json格式");
            false
        }
    }
}

async fn ensure_interconnect_registered(device_addr: &str) {
    let _ = register::register_interconnect_recv(
        device_addr,
        "com.waterflames.clartime",
    )
    .await;
}
