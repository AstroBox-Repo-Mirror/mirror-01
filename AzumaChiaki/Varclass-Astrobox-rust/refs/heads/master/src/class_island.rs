//! Class Island YAML 课表（与 CSES 结构兼容，含 `weeks` / `simplified_name` 等字段）
//!
//! 导入与扩展后的 CSES 解析一致；导出为 Class Island 典型 YAML。

use crate::model::Course;
use serde::Serialize;
use std::collections::BTreeMap;

/// 与 `import_from_cses` 相同（已解析 `weeks`: odd/even → a/b）
pub fn import_from_class_island(yaml_text: &str) -> Result<Vec<Course>, String> {
    crate::cses::import_from_cses(yaml_text)
}

fn subject_display_key(c: &Course) -> String {
    let name = c.name.trim();
    let room = c.room.trim();
    if room.is_empty() {
        name.to_string()
    } else {
        format!("{}（{}）", name, room)
    }
}

fn simplified_name(name: &str) -> String {
    let t = name.trim();
    t.chars().take(1).collect()
}

fn to_hhmmss(hhmm: &str) -> String {
    let v = hhmm.trim();
    let re = regex::Regex::new(r"^([01]\d|2[0-3]):([0-5]\d)$").unwrap();
    if let Some(caps) = re.captures(v) {
        return format!(
            "{}:{}:00",
            caps.get(1).unwrap().as_str(),
            caps.get(2).unwrap().as_str()
        );
    }
    let re2 = regex::Regex::new(r"^([01]\d|2[0-3]):([0-5]\d):([0-5]\d)$").unwrap();
    if let Some(caps) = re2.captures(v) {
        return format!(
            "{}:{}:{}",
            caps.get(1).unwrap().as_str(),
            caps.get(2).unwrap().as_str(),
            caps.get(3).unwrap().as_str()
        );
    }
    "00:00:00".to_string()
}

fn weeks_yaml_from_internal(wt: &str) -> &'static str {
    match Course::normalize_week_type(Some(wt)).as_str() {
        x if x == Course::WEEK_A => "odd",
        x if x == Course::WEEK_B => "even",
        _ => "all",
    }
}

#[derive(Serialize)]
struct CiSubject {
    name: String,
    simplified_name: String,
    teacher: String,
    room: String,
}

#[derive(Serialize)]
struct CiClass {
    subject: String,
    start_time: String,
    end_time: String,
}

#[derive(Serialize)]
struct CiSchedule {
    name: String,
    classes: Vec<CiClass>,
    enable_day: u8,
    weeks: String,
}

#[derive(Serialize)]
struct CiRoot {
    version: i32,
    subjects: Vec<CiSubject>,
    schedules: Vec<CiSchedule>,
}

/// 导出为 Class Island 风格 YAML（subjects 含 simplified_name / teacher，schedules 含 weeks）
pub fn export_to_class_island_yaml(courses: &[Course]) -> String {
    let re_hhmm = regex::Regex::new(r"^([01]\d|2[0-3]):([0-5]\d)$").unwrap();
    let mut normalized: Vec<&Course> = courses
        .iter()
        .filter(|c| {
            (1..=7).contains(&c.day)
                && !c.name.trim().is_empty()
                && re_hhmm.is_match(c.start.trim())
                && re_hhmm.is_match(c.end.trim())
        })
        .collect();
    normalized.sort_by(|a, b| {
        a.day
            .cmp(&b.day)
            .then_with(|| a.start.cmp(&b.start))
            .then_with(|| a.week_type.cmp(&b.week_type))
    });

    let mut subjects_by_key: BTreeMap<String, CiSubject> = BTreeMap::new();
    for c in &normalized {
        let key = subject_display_key(c);
        if subjects_by_key.contains_key(&key) {
            continue;
        }
        let base = c.name.trim().to_string();
        let room = c.room.trim().to_string();
        subjects_by_key.insert(
            key.clone(),
            CiSubject {
                name: key,
                simplified_name: simplified_name(&base),
                teacher: String::new(),
                room,
            },
        );
    }
    let subjects: Vec<CiSubject> = subjects_by_key.into_values().collect();

    let mut groups: BTreeMap<(u8, String), Vec<&Course>> = BTreeMap::new();
    for c in &normalized {
        let wt = Course::normalize_week_type(Some(&c.week_type));
        groups.entry((c.day, wt)).or_default().push(c);
    }

    let schedules: Vec<CiSchedule> = groups
        .into_iter()
        .map(|((day, week_type), list)| {
            let classes: Vec<CiClass> = list
                .into_iter()
                .map(|c| CiClass {
                    subject: subject_display_key(c),
                    start_time: to_hhmmss(&c.start),
                    end_time: to_hhmmss(&c.end),
                })
                .collect();
            CiSchedule {
                name: "新课表".to_string(),
                classes,
                enable_day: day,
                weeks: weeks_yaml_from_internal(&week_type).to_string(),
            }
        })
        .collect();

    let root = CiRoot {
        version: 1,
        subjects,
        schedules,
    };

    serde_yaml::to_string(&root).unwrap_or_else(|e| format!("# export error: {}\n", e))
}
