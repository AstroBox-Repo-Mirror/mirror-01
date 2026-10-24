//! 主界面 UI 构建
//!
//! 负责添加课程、课程管理、导入配置三个标签页的布局与渲染。

use crate::astrobox::psys_host::ui;
use crate::model::Course;
use crate::sync;
use crate::ui::event_handler::*;
use crate::ui::state::{ui_state, CourseForm, ImportFormat, TabType, UiState};

const RADIUS: u32 = 24;
const BUTTON_PADDING: u32 = 22;
const BUTTON_FONT_SIZE: u32 = 15;
const TAB_FONT_SIZE: u32 = 14;
const INPUT_HEIGHT: u32 = 48;
const INPUT_PADDING: u32 = 12;
const COURSE_ITEM_PADDING: u32 = 24;

// 主题色：主按钮蓝、灰色按钮、激活态、输入框背景
const BLUE_BUTTON: &str = "#0090FF26";
const GRAY_BUTTON: &str = "#2A2A2AD9";
const GRAY_ACTIVE: &str = "#42424226";
const GRAY_INPUT: &str = "#2A2A2AD9";

fn msg_element(state: &UiState) -> Option<ui::Element> {
    state.message.as_ref().map(|(text, is_error)| {
        ui::Element::new(ui::ElementType::Div, None)
            .width_full()
            .bg(if *is_error { "#6B1D1D" } else { "#1F4D1F" })
            .radius(RADIUS)
            .padding(10)
            .margin_bottom(10)
            .child(ui::Element::new(ui::ElementType::P, Some(text)).text_color("#FFFFFF"))
    })
}

fn tab_button(label: &str, event_id: &str, active: bool) -> ui::Element {
    ui::Element::new(ui::ElementType::Button, Some(label))
        .width_full()
        .padding(14)
        .radius(RADIUS)
        .size(TAB_FONT_SIZE)
        .bg(if active { GRAY_ACTIVE } else { GRAY_BUTTON })
        .on(ui::Event::Click, event_id)
        .on(ui::Event::PointerUp, event_id)
}

fn form_input(label: &str, value: &str, event_id: &str) -> ui::Element {
    ui::Element::new(ui::ElementType::Div, None)
        .width_full()
        .margin_bottom(10)
        .child(ui::Element::new(ui::ElementType::P, Some(label)).margin_bottom(4))
        .child(
            ui::Element::new(ui::ElementType::Input, Some(value))
                .on(ui::Event::Change, event_id)
                .on(ui::Event::Input, event_id)
                .width_full()
                .height(INPUT_HEIGHT)
                .bg(GRAY_INPUT)
                .radius(RADIUS)
                .padding(INPUT_PADDING),
        )
}

fn action_button(label: &str, event_id: &str, primary: bool) -> ui::Element {
    ui::Element::new(ui::ElementType::Div, None)
        .width_half()
        .margin_right(4)
        .child(
            ui::Element::new(ui::ElementType::Button, Some(label))
                .width_full()
                .padding(BUTTON_PADDING)
                .radius(RADIUS)
                .size(BUTTON_FONT_SIZE)
                .bg(if primary { BLUE_BUTTON } else { GRAY_BUTTON })
                .on(ui::Event::Click, event_id)
                .on(ui::Event::PointerUp, event_id),
        )
}

/// 将周数列表格式化为区间输入格式，如 [1,2,3,5,7,9] -> "1-3,5,7,9"
fn format_weeks_input(weeks: &[u32]) -> String {
    if weeks.is_empty() {
        return String::new();
    }
    let mut sorted: Vec<u32> = weeks.to_vec();
    sorted.sort();
    sorted.dedup();
    let mut parts = Vec::new();
    let mut start = sorted[0];
    let mut end = start;
    for &w in &sorted[1..] {
        if w == end + 1 {
            end = w;
        } else {
            if start == end {
                parts.push(format!("{}", start));
            } else {
                parts.push(format!("{}-{}", start, end));
            }
            start = w;
            end = w;
        }
    }
    if start == end {
        parts.push(format!("{}", start));
    } else {
        parts.push(format!("{}-{}", start, end));
    }
    parts.join(",")
}

fn form_from_course(course: &Course) -> CourseForm {
    let weeks_str = format_weeks_input(&course.weeks);
    CourseForm {
        day: course.day.to_string(),
        name: course.name.clone(),
        room: course.room.clone(),
        start: course.start.clone(),
        end: course.end.clone(),
        week_type: course.week_type.clone(),
        weeks: weeks_str,
    }
}

fn add_tab_ui(state: &UiState) -> ui::Element {
    ui::Element::new(ui::ElementType::Div, None)
        .width_full()
        .child(form_input("星期(1-7)", &state.add_form.day, INPUT_ADD_DAY))
        .child(form_input("课程名", &state.add_form.name, INPUT_ADD_NAME))
        .child(form_input("教室", &state.add_form.room, INPUT_ADD_ROOM))
        .child(form_input(
            "开始节次",
            &state.add_form.start,
            INPUT_ADD_START,
        ))
        .child(form_input("结束节次", &state.add_form.end, INPUT_ADD_END))
        .child(form_input(
            "周类型(all/a/b)",
            &state.add_form.week_type,
            INPUT_ADD_WEEK_TYPE,
        ))
        .child(form_input(
            "具体周数(如1,2,3或1-3,5-9，留空使用周类型)",
            &state.add_form.weeks,
            INPUT_ADD_WEEKS,
        ))
        .child(
            ui::Element::new(ui::ElementType::Div, None)
                .flex()
                .child(action_button("添加课程", EVENT_ADD_COURSE, true))
                .child(action_button("推送到手环", EVENT_PUSH_TO_WATCH, false)),
        )
}

const DAY_NAMES: [&str; 7] = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];

fn day_button(day: u8, active: bool) -> ui::Element {
    let label = DAY_NAMES
        .get((day as usize).saturating_sub(1))
        .unwrap_or(&"");
    let event_id = format!("{}{}", crate::ui::event_handler::EVENT_DAY_PREFIX, day);
    ui::Element::new(ui::ElementType::Button, Some(label))
        .width_full()
        .padding(8)
        .radius(RADIUS)
        .size(14)
        .bg(if active { GRAY_ACTIVE } else { GRAY_BUTTON })
        .on(ui::Event::Click, &event_id)
        .on(ui::Event::PointerUp, &event_id)
}

fn course_list_item(course: &Course, index: usize, is_selected: bool) -> ui::Element {
    let event_id = format!("{}{}", crate::ui::event_handler::EVENT_COURSE_PREFIX, index);
    let week_info = if course.weeks.is_empty() {
        match course.week_type.as_str() {
            "a" => " A周".to_string(),
            "b" => " B周".to_string(),
            _ => String::new(),
        }
    } else {
        format!(" {}", Course::format_weeks(&course.weeks))
    };
    let text = format!(
        "{} {}-{}节{}",
        course.display_name(),
        course.start,
        course.end,
        week_info
    );
    ui::Element::new(ui::ElementType::Button, Some(&text))
        .width_full()
        .padding(COURSE_ITEM_PADDING)
        .margin_bottom(8)
        .radius(RADIUS)
        .size(15)
        .bg(if is_selected {
            GRAY_ACTIVE
        } else {
            GRAY_BUTTON
        })
        .on(ui::Event::Click, &event_id)
        .on(ui::Event::PointerUp, &event_id)
}

fn manage_tab_ui(state: &UiState) -> ui::Element {
    let courses = sync::get_cached_courses();
    let selected_day = state.selected_day;

    let day_buttons = ui::Element::new(ui::ElementType::Div, None)
        .width_full()
        .margin_bottom(12)
        .child(
            ui::Element::new(ui::ElementType::Div, None)
                .flex()
                .width_full()
                .margin_bottom(4)
                .child(
                    ui::Element::new(ui::ElementType::Div, None)
                        .flex()
                        .width_half()
                        .margin_right(4)
                        .child(
                            ui::Element::new(ui::ElementType::Div, None)
                                .width_half()
                                .margin_right(4)
                                .child(day_button(1, 1 == selected_day)),
                        )
                        .child(
                            ui::Element::new(ui::ElementType::Div, None)
                                .width_half()
                                .child(day_button(2, 2 == selected_day)),
                        ),
                )
                .child(
                    ui::Element::new(ui::ElementType::Div, None)
                        .flex()
                        .width_half()
                        .margin_right(4)
                        .child(
                            ui::Element::new(ui::ElementType::Div, None)
                                .width_half()
                                .margin_right(4)
                                .child(day_button(3, 3 == selected_day)),
                        )
                        .child(
                            ui::Element::new(ui::ElementType::Div, None)
                                .width_half()
                                .child(day_button(4, 4 == selected_day)),
                        ),
                ),
        )
        .child(
            ui::Element::new(ui::ElementType::Div, None)
                .flex()
                .width_full()
                .child(
                    ui::Element::new(ui::ElementType::Div, None)
                        .flex()
                        .width_half()
                        .margin_right(4)
                        .child(
                            ui::Element::new(ui::ElementType::Div, None)
                                .width_half()
                                .margin_right(4)
                                .child(day_button(5, 5 == selected_day)),
                        )
                        .child(
                            ui::Element::new(ui::ElementType::Div, None)
                                .width_half()
                                .child(day_button(6, 6 == selected_day)),
                        ),
                )
                .child(
                    ui::Element::new(ui::ElementType::Div, None)
                        .width_half()
                        .child(day_button(7, 7 == selected_day)),
                ),
        );

    let day_courses: Vec<(usize, &Course)> = courses
        .iter()
        .enumerate()
        .filter(|(_, c)| c.day == selected_day)
        .collect();

    // 课程列表：不设固定高度，随内容动态增长
    let mut list = ui::Element::new(ui::ElementType::Div, None)
        .width_full()
        .flex()
        .flex_direction(ui::FlexDirection::Column)
        .margin_bottom(12);

    for (idx, course) in &day_courses {
        let is_selected = state.selected_index == Some(*idx);
        list = list.child(course_list_item(course, *idx, is_selected));
    }

    if day_courses.is_empty() {
        list = list.child(
            ui::Element::new(ui::ElementType::P, Some("该日暂无课程"))
                .text_color("#888888")
                .margin_bottom(8),
        );
    }

    let mut root = ui::Element::new(ui::ElementType::Div, None)
        .width_full()
        .child(
            ui::Element::new(ui::ElementType::Div, None)
                .flex()
                .margin_bottom(10)
                .child(action_button("从手环同步", EVENT_PULL_FROM_WATCH, true))
                .child(action_button("推送到手环", EVENT_PUSH_TO_WATCH, false)),
        )
        .child(day_buttons)
        .child(list);

    root = root.child(form_input(
        "星期(1-7)",
        &state.edit_form.day,
        INPUT_EDIT_DAY,
    ));
    root = root.child(form_input("课程名", &state.edit_form.name, INPUT_EDIT_NAME));
    root = root.child(form_input("教室", &state.edit_form.room, INPUT_EDIT_ROOM));
    root = root.child(form_input(
        "开始节次",
        &state.edit_form.start,
        INPUT_EDIT_START,
    ));
    root = root.child(form_input("结束节次", &state.edit_form.end, INPUT_EDIT_END));
    root = root.child(form_input(
        "周类型(all/a/b)",
        &state.edit_form.week_type,
        INPUT_EDIT_WEEK_TYPE,
    ));
    root = root.child(form_input(
        "具体周数(如1,2,3或1-3,5-9，留空使用周类型)",
        &state.edit_form.weeks,
        INPUT_EDIT_WEEKS,
    ));

    root.child(
        ui::Element::new(ui::ElementType::Div, None)
            .flex()
            .child(action_button("保存修改", EVENT_SAVE_EDIT, true))
            .child(action_button("删除课程", EVENT_DELETE_COURSE, false)),
    )
}

fn import_format_select(state: &UiState) -> ui::Element {
    let format_label = match state.import_format {
        ImportFormat::Json => "JSON",
        ImportFormat::Cses => "CSES (YAML)",
        ImportFormat::ClassIsland => "Class Island (YAML)",
        ImportFormat::Wakeup => "WakeUp",
        ImportFormat::Ics => "iCalendar (.ics)",
    };
    let mut sel = ui::Element::new(ui::ElementType::Select, Some(format_label))
        .on(
            ui::Event::Change,
            crate::ui::event_handler::INPUT_IMPORT_FORMAT,
        )
        .width_full()
        .height(INPUT_HEIGHT)
        .bg(GRAY_INPUT)
        .radius(RADIUS)
        .padding(INPUT_PADDING)
        .margin_bottom(10);
    sel = sel.child(ui::Element::new(ui::ElementType::Option, Some("JSON")));
    sel = sel.child(ui::Element::new(
        ui::ElementType::Option,
        Some("CSES (YAML)"),
    ));
    sel = sel.child(ui::Element::new(
        ui::ElementType::Option,
        Some("Class Island (YAML)"),
    ));
    sel = sel.child(ui::Element::new(ui::ElementType::Option, Some("WakeUp")));
    sel = sel.child(ui::Element::new(
        ui::ElementType::Option,
        Some("iCalendar (.ics)"),
    ));
    sel
}

fn import_tab_ui(state: &UiState) -> ui::Element {
    let hint = match state.import_format {
        ImportFormat::Json => "粘贴 JSON 课程配置后导入",
        ImportFormat::Cses => "粘贴 CSES YAML 课表后导入",
        ImportFormat::ClassIsland => "粘贴 Class Island YAML（与 CSES 兼容，支持 weeks: odd/even）",
        ImportFormat::Wakeup => "粘贴 WakeUp 多段 JSON 文本后导入",
        ImportFormat::Ics => "粘贴 iCalendar / .ics 日历文本后导入",
    };
    ui::Element::new(ui::ElementType::Div, None)
        .width_full()
        .child(
            ui::Element::new(ui::ElementType::Div, None)
                .width_full()
                .margin_bottom(6)
                .child(
                    ui::Element::new(ui::ElementType::P, Some("解析格式"))
                        .text_color("#BBBBBB")
                        .margin_bottom(4),
                )
                .child(import_format_select(state)),
        )
        .child(
            ui::Element::new(ui::ElementType::P, Some(hint))
                .text_color("#BBBBBB")
                .margin_bottom(6),
        )
        .child(
            ui::Element::new(ui::ElementType::Input, Some(&state.import_text))
                .on(ui::Event::Change, INPUT_IMPORT_TEXT)
                .on(ui::Event::Input, INPUT_IMPORT_TEXT)
                .width_full()
                .height(160)
                .bg(GRAY_INPUT)
                .radius(RADIUS)
                .padding(INPUT_PADDING)
                .margin_bottom(10),
        )
        .child(
            ui::Element::new(ui::ElementType::Div, None)
                .flex()
                .child(action_button("导入配置", EVENT_IMPORT_PASTE, true))
                .child(action_button("推送到手环", EVENT_PUSH_TO_WATCH, false)),
        )
}

pub fn build_main_ui() -> ui::Element {
    let state = ui_state()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut root = ui::Element::new(ui::ElementType::Div, None)
        .flex()
        .flex_direction(ui::FlexDirection::Column)
        .width_full()
        .padding(16);

    if let Some(m) = msg_element(&state) {
        root = root.child(m);
    }

    // 顶部三按钮各占 50%，分两行
    let tabs = ui::Element::new(ui::ElementType::Div, None)
        .width_full()
        .margin_bottom(12)
        .child(
            ui::Element::new(ui::ElementType::Div, None)
                .flex()
                .width_full()
                .margin_bottom(4)
                .child(
                    ui::Element::new(ui::ElementType::Div, None)
                        .width_half()
                        .margin_right(4)
                        .child(tab_button(
                            "添加课程",
                            EVENT_TAB_ADD,
                            state.current_tab == TabType::Add,
                        )),
                )
                .child(
                    ui::Element::new(ui::ElementType::Div, None)
                        .width_half()
                        .child(tab_button(
                            "课程管理",
                            EVENT_TAB_MANAGE,
                            state.current_tab == TabType::Manage,
                        )),
                ),
        )
        .child(
            ui::Element::new(ui::ElementType::Div, None)
                .flex()
                .width_full()
                .child(
                    ui::Element::new(ui::ElementType::Div, None)
                        .width_half()
                        .margin_right(4)
                        .child(tab_button(
                            "导入配置",
                            EVENT_TAB_IMPORT,
                            state.current_tab == TabType::Import,
                        )),
                )
                .child(ui::Element::new(ui::ElementType::Div, None).width_half()),
        );
    root = root.child(tabs);

    let content = match state.current_tab {
        TabType::Add => add_tab_ui(&state),
        TabType::Manage => manage_tab_ui(&state),
        TabType::Import => import_tab_ui(&state),
    };

    root.child(content)
}

pub fn render_main_ui(element_id: &str) {
    {
        let mut state = ui_state()
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.root_element_id = Some(element_id.to_string());
    }
    ui::render(element_id, build_main_ui());
}

pub fn refresh_main_ui() {
    let root_id = {
        let state = ui_state()
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.root_element_id.clone()
    };
    if let Some(root_id) = root_id {
        ui::render(&root_id, build_main_ui());
    }
}

pub fn fill_edit_form_by_index(index: usize) {
    let courses = sync::get_cached_courses();
    if let Some(course) = courses.get(index) {
        let mut state = ui_state()
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.selected_index = Some(index);
        state.edit_form = form_from_course(course);
    }
}
