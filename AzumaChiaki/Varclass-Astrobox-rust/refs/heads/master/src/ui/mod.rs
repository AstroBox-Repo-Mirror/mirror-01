//! UI 模块：界面构建、事件处理、状态管理

pub mod build;
pub mod device;
pub mod event_handler;
pub mod state;

pub use build::{refresh_main_ui, render_main_ui};
pub use event_handler::{set_status_message, ui_event_processor};
