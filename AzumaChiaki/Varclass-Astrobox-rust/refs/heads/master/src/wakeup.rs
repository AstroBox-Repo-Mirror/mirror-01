//! WakeUp 课表格式适配器
//!
//! 与 Android WakeupTimetableAdapter.kt 对应，解析 5 段 JSON 结构。

use crate::model::Course;
use serde_json::Value;
use std::collections::BTreeMap;

fn value_i64(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<i64> {
    for key in keys {
        if let Some(value) = obj.get(*key) {
            if let Some(n) = value.as_i64().or_else(|| value.as_u64().map(|n| n as i64)) {
                return Some(n);
            }
            if let Some(s) = value.as_str() {
                if let Ok(n) = s.trim().parse::<i64>() {
                    return Some(n);
                }
            }
        }
    }
    None
}

fn value_str(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = obj.get(*key) {
            if let Some(s) = value.as_str() {
                let s = s.trim();
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            } else if value.is_number() || value.is_boolean() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn is_valid_hhmm(value: &str) -> bool {
    let parts: Vec<&str> = value.split(':').collect();
    if parts.len() != 2 {
        return false;
    }
    let h = parts[0].parse::<i32>().unwrap_or(-1);
    let m = parts[1].parse::<i32>().unwrap_or(-1);
    (0..=23).contains(&h) && (0..=59).contains(&m)
}

fn parse_json_array(json_text: &str) -> Vec<Value> {
    let Ok(value) = serde_json::from_str::<Value>(json_text) else {
        return Vec::new();
    };
    if let Some(arr) = value.as_array() {
        return arr.clone();
    }
    if let Some(obj) = value.as_object() {
        for key in ["data", "list", "items", "rows", "courses", "details"] {
            if let Some(arr) = obj.get(key).and_then(|v| v.as_array()) {
                return arr.clone();
            }
        }
    }
    Vec::new()
}

/// 解析 WakeUp 节次表 JSON（node -> startTime/endTime）
fn parse_node_table(json_text: &str) -> BTreeMap<i32, (String, String)> {
    let arr = parse_json_array(json_text);
    let mut out = BTreeMap::new();
    for obj in arr {
        if let Some(obj) = obj.as_object() {
            if let (Some(node), Some(start), Some(end)) = (
                value_i64(obj, &["node", "section", "sectionNode", "id"]),
                value_str(obj, &["startTime", "start_time", "start"]),
                value_str(obj, &["endTime", "end_time", "end"]),
            ) {
                let start = start.trim().to_string();
                let end = end.trim().to_string();
                if node > 0 && is_valid_hhmm(&start) && is_valid_hhmm(&end) {
                    out.insert(node as i32, (start, end));
                }
            }
        }
    }
    out
}

/// 解析课程元数据 JSON（id -> courseName）
fn parse_course_meta(json_text: &str) -> BTreeMap<i32, String> {
    let arr = parse_json_array(json_text);
    let mut out = BTreeMap::new();
    for obj in arr {
        if let Some(obj) = obj.as_object() {
            if let (Some(id), Some(name)) = (
                value_i64(obj, &["id", "courseId", "course_id"]),
                value_str(obj, &["courseName", "course_name", "name", "subject"]),
            ) {
                let name = name.trim().to_string();
                if id >= 0 && !name.is_empty() {
                    out.insert(id as i32, name);
                }
            }
        }
    }
    out
}

fn week_type_from_wakeup(value: Option<&Value>) -> String {
    match value {
        Some(Value::Number(n)) => match n.as_i64().unwrap_or_default() {
            1 => Course::WEEK_A.to_string(),
            2 => Course::WEEK_B.to_string(),
            _ => Course::WEEK_ALL.to_string(),
        },
        Some(Value::String(s)) => {
            if let Ok(n) = s.trim().parse::<i64>() {
                match n {
                    1 => Course::WEEK_A.to_string(),
                    2 => Course::WEEK_B.to_string(),
                    _ => Course::WEEK_ALL.to_string(),
                }
            } else {
                Course::normalize_week_type(Some(s))
            }
        }
        _ => Course::WEEK_ALL.to_string(),
    }
}

fn parse_weeks_from_detail(obj: &serde_json::Map<String, Value>, week_type: &str) -> Vec<u32> {
    if let Some(value) = obj.get("weeks").or_else(|| obj.get("weekList")) {
        if let Some(arr) = value.as_array() {
            let mut weeks: Vec<u32> = arr
                .iter()
                .filter_map(|v| {
                    v.as_u64()
                        .map(|n| n as u32)
                        .or_else(|| v.as_str().and_then(|s| s.trim().parse::<u32>().ok()))
                })
                .filter(|&n| n > 0)
                .collect();
            weeks.sort();
            weeks.dedup();
            return weeks;
        }
        if let Some(text) = value.as_str() {
            return Course::parse_weeks_text(text);
        }
    }

    let start_week = value_i64(obj, &["startWeek", "start_week"]).unwrap_or(0);
    let end_week = value_i64(obj, &["endWeek", "end_week"]).unwrap_or(0);
    if start_week > 0 && end_week > 0 {
        return Course::weeks_from_range(start_week as u32, end_week as u32, week_type);
    }
    Vec::new()
}

/// 恢复被合并的换行（粘贴时 5 段 JSON 可能用空格分隔或连成一行）
fn normalize_wakeup_newlines(text: &str) -> String {
    let mut s = text.trim().to_string();
    s = s.replace("} [", "}\n[");
    s = s.replace("] [", "]\n[");
    s = s.replace("] {", "]\n{");

    s = s.replace("][", "]\n[");
    s = s.replace("]{", "]\n{");
    s = s.replace("}[", "}\n[");
    s
}

/// 按换行分割为多段，每段为一行 JSON（header、节次表、config、课程元数据、课程安排）
fn split_wakeup_blocks(text: &str) -> Vec<String> {
    let normalized = normalize_wakeup_newlines(text);
    normalized
        .split('\n')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

pub fn import_from_wakeup(text: &str) -> Result<Vec<Course>, String> {
    let blocks = split_wakeup_blocks(text);

    if blocks.len() < 5 {
        return Err(format!(
            "WakeUp 格式需 5 段 JSON（当前 {} 段），每段一行",
            blocks.len()
        ));
    }

    let node_table = parse_node_table(blocks.get(1).map(|s| s.as_str()).unwrap_or("[]"));
    if node_table.is_empty() {
        return Err("未解析到节次时间表".to_string());
    }

    let course_meta = parse_course_meta(blocks.get(3).map(|s| s.as_str()).unwrap_or("[]"));

    let details = parse_json_array(blocks.get(4).map(|s| s.as_str()).unwrap_or("[]"));
    if details.is_empty() {
        return Err("课程安排段解析失败或为空".to_string());
    }

    let mut out = Vec::new();
    for obj in details {
        let obj = match obj.as_object() {
            Some(o) => o,
            None => continue,
        };
        let day = match value_i64(obj, &["day", "weekday", "weekDay"]) {
            Some(d) => d as u8,
            None => continue,
        };
        if day < 1 || day > 7 {
            continue;
        }
        let start_node = value_i64(obj, &["startNode", "start_node", "node"]).unwrap_or(0) as i32;
        let step = value_i64(obj, &["step", "length", "duration"])
            .unwrap_or(1)
            .max(1) as i32;
        let end_node = start_node + step - 1;

        let own_time = obj
            .get("ownTime")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let custom_start = value_str(obj, &["startTime", "start_time", "start"]);
        let custom_end = value_str(obj, &["endTime", "end_time", "end"]);
        let (start, end) = if own_time
            && custom_start.as_deref().is_some_and(is_valid_hhmm)
            && custom_end.as_deref().is_some_and(is_valid_hhmm)
        {
            (custom_start.unwrap(), custom_end.unwrap())
        } else {
            (
                node_table
                    .get(&start_node)
                    .map(|(s, _)| s.clone())
                    .unwrap_or_default(),
                node_table
                    .get(&end_node)
                    .map(|(_, e)| e.clone())
                    .unwrap_or_default(),
            )
        };
        if start.is_empty() || end.is_empty() {
            continue;
        }

        let course_id = value_i64(obj, &["id", "courseId", "course_id"]).unwrap_or(-1) as i32;
        let name = value_str(obj, &["courseName", "course_name", "name", "subject"])
            .or_else(|| course_meta.get(&course_id).cloned())
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let room = value_str(obj, &["room", "classroom", "location", "place"]).unwrap_or_default();
        let week_type = week_type_from_wakeup(obj.get("type").or_else(|| obj.get("weekType")));
        let weeks = parse_weeks_from_detail(obj, &week_type);

        out.push(Course {
            day,
            name,
            start,
            end,
            room,
            week_type,
            weeks,
        });
    }

    if out.is_empty() {
        return Err("未解析到有效课程".to_string());
    }

    Ok(out)
}
