use crate::astrobox::psys_host::ui;
use super::state::{ui_state, TabType, UiState};
use super::event_handler::{TAB_COURSE_EDITOR, TAB_DATA_TRANSFER, COURSE_DATA_INPUT_EVENT, OPEN_BROWSER_EVENT, SEND_DATA_EVENT, BUTTON_MOUSE_LEAVE, HIDE_MESSAGE_EVENT};
use super::message::check_and_hide_message;

pub fn build_course_editor_ui(_state: &UiState) -> ui::Element {
    let container = ui::Element::new(ui::ElementType::Div, None)
        .flex()
        .flex_direction(ui::FlexDirection::Column);

    let tip = ui::Element::new(
        ui::ElementType::P,
        Some("文件导入将尝试制作，敬请期待！"),
    )
    .size(14)
    .margin_bottom(16);

    container.child(tip)
}

pub fn build_data_transfer_ui(state: &UiState) -> ui::Element {
    let container = ui::Element::new(ui::ElementType::Div, None)
        .flex()
        .flex_direction(ui::FlexDirection::Column);

    let tip1 = ui::Element::new(
        ui::ElementType::P,
        Some("欢迎使用澄序课程表同步插件，下面我会一步步带你创建并传输你的课程数据。"),
    )
    .size(14)
    .margin_bottom(16);

    let tip2 = ui::Element::new(
        ui::ElementType::P,
        Some("第一步：请你进入下面这个网站生成你的课程数据，支持 Wakeup 以及澄序官方数据格式的导入与编辑。编辑完成后建议保存文件，并复制网站内生成出来的数据。"),
    )
    .size(14)
    .margin_bottom(16);

    let open_browser_button = ui::Element::new(ui::ElementType::Button, Some("打开在线课程编辑器"))
        .without_default_styles()
        .on(ui::Event::Click, OPEN_BROWSER_EVENT)
        .on(ui::Event::MouseEnter, OPEN_BROWSER_EVENT)
        .on(ui::Event::MouseLeave, BUTTON_MOUSE_LEAVE)
        .radius(8)
        .padding(14)
        .bg(
            if state.hovered_button.as_deref() == Some(OPEN_BROWSER_EVENT) {
                "#4b4b4b"
            } else {
                "#2A2A2A"
            },
        )
        .width_full()
        .margin_bottom(16);

    let tip3 = ui::Element::new(
        ui::ElementType::P,
        Some("第二步：请你在下方输入框里粘贴你在第一步生成的课程数据。（请确保复制完整，部分手机输入法可能因为字数过长而无法完整粘贴，亲测微信输入法粘贴正常）"),
    )
    .size(14)
    .margin_bottom(8);

    let course_data_input = ui::Element::new(ui::ElementType::Input, Some(&state.course_data))
        .on(ui::Event::Change, COURSE_DATA_INPUT_EVENT)
        .radius(8)
        .bg("#2A2A2A")
        .width_full()
        .padding(12)
        .margin_bottom(16);

    let tip4 = ui::Element::new(
        ui::ElementType::P,
        Some("第三步：接下来请你确定 astrobox 是否已经连接手环，随后在手环上关闭并重新打开澄序课程表，进入传输相关页面，以保证此插件能与应用正常通信。当这些都完成以后你就可以点击发送了。"),
    )
    .size(14)
    .margin_bottom(16);

    let send_button = ui::Element::new(ui::ElementType::Button, Some("发送"))
        .without_default_styles()
        .on(ui::Event::Click, SEND_DATA_EVENT)
        .on(ui::Event::MouseEnter, SEND_DATA_EVENT)
        .on(ui::Event::MouseLeave, BUTTON_MOUSE_LEAVE)
        .radius(8)
        .padding(14)
        .bg(
            if state.hovered_button.as_deref() == Some(SEND_DATA_EVENT) {
                "#4b4b4b"
            } else {
                "#2A2A2A"
            },
        )
        .width_full();

    container
        .child(tip1)
        .child(tip2)
        .child(open_browser_button)
        .child(tip3)
        .child(course_data_input)
        .child(tip4)
        .child(send_button)
}

pub fn build_main_ui() -> ui::Element {
    check_and_hide_message();

    let state = ui_state()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let main_container = ui::Element::new(ui::ElementType::Div, None)
        .flex()
        .flex_direction(ui::FlexDirection::Column)
        .width_full()
        .padding(20);

    let message_element = if let Some(ref msg) = state.transfer_message {
        let bg_color = if state.is_success_message {
            "#4CAF50"
        } else {
            "#FF4444"
        };
        Some(
            ui::Element::new(ui::ElementType::Div, None)
                .bg(bg_color)
                .radius(8)
                .padding(12)
                .margin_bottom(20)
                .on(ui::Event::Click, HIDE_MESSAGE_EVENT)
                .child(
                    ui::Element::new(ui::ElementType::P, Some(msg))
                        .size(14)
                        .text_color("#FFFFFF"),
                ),
        )
    } else {
        None
    };

    let tabs_wrapper = ui::Element::new(ui::ElementType::Div, None)
        .flex()
        .justify_center();

    let tab_container = ui::Element::new(ui::ElementType::Div, None)
        .flex()
        .flex_direction(ui::FlexDirection::Row)
        .margin_bottom(20)
        .bg("#1E1E1F")
        .radius(8);

    let course_editor_tab = ui::Element::new(ui::ElementType::Button, Some("课程编辑器"))
        .without_default_styles()
        .padding_left(20)
        .padding_right(20)
        .padding_top(12)
        .padding_bottom(12)
        .margin(5)
        .bg(if state.current_tab == TabType::CourseEditor {
            "#424242"
        } else if state.hovered_button.as_deref() == Some(TAB_COURSE_EDITOR) {
            "#4b4b4b"
        } else {
            "#2A2A2A"
        })
        .on(ui::Event::Click, TAB_COURSE_EDITOR)
        .on(ui::Event::MouseEnter, TAB_COURSE_EDITOR)
        .on(ui::Event::MouseLeave, BUTTON_MOUSE_LEAVE)
        .radius(8);

    let data_transfer_tab = ui::Element::new(ui::ElementType::Button, Some("数据传输"))
        .without_default_styles()
        .padding_left(20)
        .padding_right(20)
        .padding_top(12)
        .padding_bottom(12)
        .margin(5)
        .bg(if state.current_tab == TabType::DataTransfer {
            "#424242"
        } else if state.hovered_button.as_deref() == Some(TAB_DATA_TRANSFER) {
            "#4b4b4b"
        } else {
            "#2A2A2A"
        })
        .on(ui::Event::Click, TAB_DATA_TRANSFER)
        .on(ui::Event::MouseEnter, TAB_DATA_TRANSFER)
        .on(ui::Event::MouseLeave, BUTTON_MOUSE_LEAVE)
        .radius(8);

    let content = match state.current_tab {
        TabType::CourseEditor => build_course_editor_ui(&state),
        TabType::DataTransfer => build_data_transfer_ui(&state),
    };

    let mut main_ui = main_container;

    if let Some(msg_element) = message_element {
        main_ui = main_ui.child(msg_element);
    }

    main_ui
        .child(
            tabs_wrapper.child(
                tab_container
                    .child(course_editor_tab)
                    .child(data_transfer_tab),
            ),
        )
        .child(content)
}

pub fn render_main_ui(element_id: &str) {
    let root_id: Option<String>;
    {
        let mut state = ui_state()
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.root_element_id = Some(element_id.to_string());
        root_id = state.root_element_id.clone();
    }
    
    if let Some(root_id) = root_id {
        let ui = build_main_ui();
        crate::astrobox::psys_host::ui::render(&root_id, ui);
    }
}
