//! 课程模型
//!
//! 与 Android 端 Course.kt 对应，支持 JSON 序列化及名称/教室拆分。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Course {
    pub day: u8,
    pub name: String,
    pub start: String,
    pub end: String,
    #[serde(default)]
    pub room: String,
    #[serde(
        default = "default_week_type",
        rename = "weekType",
        alias = "week_type"
    )]
    pub week_type: String,
    #[serde(default)]
    pub weeks: Vec<u32>,
}

fn default_week_type() -> String {
    crate::model::Course::WEEK_ALL.to_string()
}

impl Course {
    pub const WEEK_ALL: &'static str = "all";
    pub const WEEK_A: &'static str = "a";
    pub const WEEK_B: &'static str = "b";

    pub fn display_name(&self) -> String {
        let r = self.room.trim();
        if r.is_empty() {
            self.name.clone()
        } else {
            format!("{}（{}）", self.name, r)
        }
    }

    pub fn normalize_week_type(value: Option<&str>) -> String {
        let normalized = value.map(|s| {
            s.trim()
                .to_lowercase()
                .replace(' ', "")
                .replace('_', "")
                .replace('-', "")
        });
        match normalized.as_deref() {
            Some(Self::WEEK_A) | Some("a周") | Some("odd") | Some("oddweek") | Some("oddweeks")
            | Some("single") | Some("singleweek") | Some("singleweeks") | Some("奇")
            | Some("奇周") | Some("单") | Some("单周") => Self::WEEK_A.to_string(),
            Some(Self::WEEK_B) | Some("b周") | Some("even") | Some("evenweek")
            | Some("evenweeks") | Some("double") | Some("doubleweek") | Some("doubleweeks")
            | Some("偶") | Some("偶周") | Some("双") | Some("双周") => {
                Self::WEEK_B.to_string()
            }
            _ => Self::WEEK_ALL.to_string(),
        }
    }

    /// 解析 "1,2,3"、"1-3,5,7-9"、"第1-3周" 等周数字符串。
    pub fn parse_weeks_text(value: &str) -> Vec<u32> {
        let normalized = value
            .replace('，', ",")
            .replace('；', ",")
            .replace('、', ",")
            .replace(';', ",")
            .replace(' ', ",")
            .replace('周', "")
            .replace('第', "");
        let mut weeks = Vec::new();
        for part in normalized.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let range_part = part
                .replace('－', "-")
                .replace('—', "-")
                .replace('~', "-")
                .replace("..", "-");
            if let Some((left, right)) = range_part.split_once('-') {
                let Ok(mut start) = left.trim().parse::<u32>() else {
                    continue;
                };
                let Ok(mut end) = right.trim().parse::<u32>() else {
                    continue;
                };
                if start == 0 || end == 0 {
                    continue;
                }
                if start > end {
                    std::mem::swap(&mut start, &mut end);
                }
                for week in start..=end {
                    weeks.push(week);
                }
            } else if let Ok(week) = range_part.parse::<u32>() {
                if week > 0 {
                    weeks.push(week);
                }
            }
        }
        weeks.sort();
        weeks.dedup();
        weeks
    }

    pub fn weeks_from_range(start: u32, end: u32, week_type: &str) -> Vec<u32> {
        if start == 0 || end == 0 {
            return Vec::new();
        }
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        let normalized = Self::normalize_week_type(Some(week_type));
        (start..=end)
            .filter(|week| match normalized.as_str() {
                Self::WEEK_A => week % 2 == 1,
                Self::WEEK_B => week % 2 == 0,
                _ => true,
            })
            .collect()
    }

    /// 格式化周数列表为可读字符串，如 "1-3,5,7-9周"
    pub fn format_weeks(weeks: &[u32]) -> String {
        if weeks.is_empty() {
            return String::new();
        }
        let mut sorted = weeks.to_vec();
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
        format!("{}周", parts.join(","))
    }

    /// 检查给定周数是否在 weeks 列表中（weeks 为空时始终返回 true）
    pub fn matches_week(&self, week_number: u32) -> bool {
        if self.weeks.is_empty() {
            true
        } else {
            self.weeks.contains(&week_number)
        }
    }

    /// 从 "课程（教室）"/"课程(教室)" 中拆分 name + room
    pub fn split_name_and_room(display: &str) -> (String, String) {
        let s = display.trim();
        let re = regex::Regex::new(r"^\s*(.*?)\s*[（(]\s*(.+?)\s*[)）]\s*$").unwrap();
        if let Some(caps) = re.captures(s) {
            (
                caps.get(1)
                    .map(|m| m.as_str().trim().to_string())
                    .unwrap_or_default(),
                caps.get(2)
                    .map(|m| m.as_str().trim().to_string())
                    .unwrap_or_default(),
            )
        } else {
            (s.to_string(), String::new())
        }
    }
}
