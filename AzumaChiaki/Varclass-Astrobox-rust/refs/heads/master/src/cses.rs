//! CSES YAML 课表格式适配器
//!
//! 与 Android CsesTimetableAdapter.kt 对应，解析 version=1 的 subjects/schedules 结构。

use crate::model::Course;
use serde_yaml::Value;
use std::collections::BTreeMap;

fn to_hhmm(value: &str) -> Option<String> {
    let v = value.trim();
    let re_hhmm = regex::Regex::new(r"^(\d{1,2}):([0-5]\d)$").unwrap();
    let re_hhmmss = regex::Regex::new(r"^(\d{1,2}):([0-5]\d):([0-5]\d)$").unwrap();
    if let Some(caps) = re_hhmm.captures(v) {
        let h: i32 = caps.get(1)?.as_str().parse().ok()?;
        let m: i32 = caps.get(2)?.as_str().parse().ok()?;
        if !(0..=23).contains(&h) {
            return None;
        }
        return Some(format!("{:02}:{:02}", h, m));
    }
    if let Some(caps) = re_hhmmss.captures(v) {
        let h: i32 = caps.get(1)?.as_str().parse().ok()?;
        let m: i32 = caps.get(2)?.as_str().parse().ok()?;
        if !(0..=23).contains(&h) {
            return None;
        }
        return Some(format!("{:02}:{:02}", h, m));
    }
    None
}

fn number_to_hhmm(raw: i64) -> Option<String> {
    if (0..=86_399).contains(&raw) {
        let h = raw / 3600;
        let m = (raw % 3600) / 60;
        if (0..=23).contains(&h) && (0..=59).contains(&m) {
            return Some(format!("{:02}:{:02}", h, m));
        }
    }
    if (0..=235_959).contains(&raw) {
        let h = raw / 10_000;
        let m = (raw / 100) % 100;
        let s = raw % 100;
        if (0..=23).contains(&h) && (0..=59).contains(&m) && (0..=59).contains(&s) {
            return Some(format!("{:02}:{:02}", h, m));
        }
    }
    None
}

fn extract_time_from_loose_string(value: &str) -> Option<String> {
    let re = regex::Regex::new(r"([01]?\d|2[0-3]):([0-5]\d)(?::[0-5]\d)?").unwrap();
    let caps = re.captures(value)?;
    let h: i32 = caps.get(1)?.as_str().parse().ok()?;
    let m: i32 = caps.get(2)?.as_str().parse().ok()?;
    Some(format!("{:02}:{:02}", h, m))
}

fn get_i64(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

/// Class Island / CSES `weeks` → 内部 week_type（odd→a, even→b）
fn weeks_to_course_week_type(weeks: &str) -> String {
    Course::normalize_week_type(Some(weeks))
}

/// 从 YAML Value 提取字符串（兼容 08:00:00、'10:00:00' 等时间格式）
fn value_to_str(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.trim().to_string();
    }
    if let Some(n) = v.as_i64() {
        return n.to_string();
    }
    if let Some(n) = v.as_u64() {
        return n.to_string();
    }
    serde_yaml::to_string(v)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn yaml_time_to_hhmm(v: &Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        return to_hhmm(s).or_else(|| extract_time_from_loose_string(s));
    }
    if let Some(n) = get_i64(v) {
        return number_to_hhmm(n).or_else(|| to_hhmm(&n.to_string()));
    }
    let loose = value_to_str(v);
    to_hhmm(&loose).or_else(|| extract_time_from_loose_string(&loose))
}

fn weeks_from_yaml_value(v: Option<&Value>) -> (String, Vec<u32>) {
    let Some(value) = v else {
        return (Course::WEEK_ALL.to_string(), Vec::new());
    };
    if let Some(seq) = value.as_sequence() {
        let mut weeks = Vec::new();
        for item in seq {
            if let Some(n) = get_i64(item) {
                if n > 0 {
                    weeks.push(n as u32);
                }
            } else if let Some(s) = item.as_str() {
                weeks.extend(Course::parse_weeks_text(s));
            }
        }
        weeks.sort();
        weeks.dedup();
        return (Course::WEEK_ALL.to_string(), weeks);
    }

    let raw = value_to_str(value);
    let week_type = weeks_to_course_week_type(&raw);
    let weeks = if week_type == Course::WEEK_ALL {
        Course::parse_weeks_text(&raw)
    } else {
        Vec::new()
    };
    (week_type, weeks)
}

/// 恢复被合并的换行（粘贴时换行可能变成空格）
fn normalize_cses_newlines(text: &str) -> String {
    let mut s = text.trim().to_string();
    s = s.replace(" subjects:", "\nsubjects:");
    s = s.replace(" schedules:", "\nschedules:");
    s = s.replace(" - name:", "\n- name:");
    s = s.replace("   room:", "\n  room:");
    s = s.replace(" - subject:", "\n  - subject:");
    s = s.replace("   enable_day:", "\n  enable_day:");
    s = s.replace("   weeks:", "\n  weeks:");
    s = s.replace(" simplified_name:", "\n  simplified_name:");
    s = s.replace(" teacher:", "\n  teacher:");
    s = s.replace("   classes:", "\n  classes:");
    s = s.replace("     start_time:", "\n    start_time:");
    s = s.replace("     end_time:", "\n    end_time:");
    s
}

pub fn import_from_cses(yaml_text: &str) -> Result<Vec<Course>, String> {
    let normalized = normalize_cses_newlines(yaml_text);
    let root: Value = serde_yaml::from_str(&normalized)
        .map_err(|e| format!("YAML 解析失败: {}（若从剪贴板粘贴，请确保保留换行）", e))?;

    let root = root
        .as_mapping()
        .ok_or_else(|| "根节点必须是对象".to_string())?;

    let version = root.get(&Value::String("version".into())).and_then(get_i64);
    if version != Some(1) {
        return Err("YAML 课表需 version=1（CSES / Class Island）".to_string());
    }

    let mut subject_index: BTreeMap<String, (String, String)> = BTreeMap::new();
    if let Some(subjects) = root.get(&Value::String("subjects".into())) {
        if let Some(arr) = subjects.as_sequence() {
            for subj in arr {
                if let Some(m) = subj.as_mapping() {
                    let key = m
                        .get(&Value::String("name".into()))
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default();
                    if key.is_empty() {
                        continue;
                    }
                    let room_field = m
                        .get(&Value::String("room".into()))
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default();
                    let (base, room_from_name) = Course::split_name_and_room(&key);
                    let room = if room_field.is_empty() {
                        room_from_name
                    } else {
                        room_field
                    };
                    subject_index.insert(key, (base, room));
                }
            }
        }
    }

    let mut out = Vec::new();
    if let Some(schedules) = root.get(&Value::String("schedules".into())) {
        if let Some(arr) = schedules.as_sequence() {
            for schedule_any in arr {
                if let Some(schedule) = schedule_any.as_mapping() {
                    let day = schedule
                        .get(&Value::String("enable_day".into()))
                        .and_then(get_i64)
                        .unwrap_or(0) as u8;
                    if day < 1 || day > 7 {
                        continue;
                    }
                    let (week_type, schedule_weeks) =
                        weeks_from_yaml_value(schedule.get(&Value::String("weeks".into())));
                    if let Some(classes) = schedule
                        .get(&Value::String("classes".into()))
                        .and_then(|v| v.as_sequence())
                    {
                        for clazz_any in classes {
                            if let Some(clazz) = clazz_any.as_mapping() {
                                let subject_key = clazz
                                    .get(&Value::String("subject".into()))
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.trim().to_string())
                                    .unwrap_or_default();
                                if subject_key.is_empty() {
                                    continue;
                                }
                                let Some(start) = clazz
                                    .get(&Value::String("start_time".into()))
                                    .and_then(yaml_time_to_hhmm)
                                else {
                                    continue;
                                };
                                let Some(end) = clazz
                                    .get(&Value::String("end_time".into()))
                                    .and_then(yaml_time_to_hhmm)
                                else {
                                    continue;
                                };
                                let (name, room) = subject_index
                                    .get(&subject_key)
                                    .cloned()
                                    .unwrap_or_else(|| Course::split_name_and_room(&subject_key));
                                if name.is_empty() {
                                    continue;
                                }
                                out.push(Course {
                                    day,
                                    name,
                                    start,
                                    end,
                                    room,
                                    week_type: week_type.clone(),
                                    weeks: schedule_weeks.clone(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    if out.is_empty() {
        return Err("未解析到有效课程".to_string());
    }

    out.sort_by(|a, b| a.day.cmp(&b.day).then_with(|| a.start.cmp(&b.start)));
    Ok(out)
}
