//! iCalendar (.ics) 课表格式适配器
//!
//! 解析常见日历导出的 VEVENT：DTSTART/DTEND/DURATION、SUMMARY、LOCATION 和
//! WEEKLY RRULE。时区按本地墙钟时间处理，适合课程表这类固定时段数据。

use crate::model::Course;

#[derive(Debug, Clone, Copy)]
struct DateTimeParts {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
}

fn unfold_ics(raw: &str) -> String {
    let re = regex::Regex::new(r"\r?\n[ \t]").unwrap();
    re.replace_all(raw, "")
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

fn extract_vevents(unfolded: &str) -> Vec<String> {
    let upper = unfolded.to_uppercase();
    let mut out = Vec::new();
    let mut index = 0;
    while let Some(start_rel) = upper[index..].find("BEGIN:VEVENT") {
        let start = index + start_rel;
        let Some(end_rel) = upper[start..].find("END:VEVENT") else {
            break;
        };
        let end = start + end_rel;
        out.push(unfolded[start..end].to_string());
        index = end + "END:VEVENT".len();
    }
    out
}

fn split_ics_property(line: &str) -> Option<(&str, &str)> {
    let idx = line.find(':')?;
    if idx == 0 {
        return None;
    }
    Some((&line[..idx], &line[idx + 1..]))
}

fn parse_datetime(value: &str) -> Option<DateTimeParts> {
    let v = value.trim().trim_end_matches('Z');
    let (date, time) = v.split_once('T').unwrap_or((v, "000000"));
    if date.len() != 8 || time.len() < 4 {
        return None;
    }
    let year = date[0..4].parse::<i32>().ok()?;
    let month = date[4..6].parse::<u32>().ok()?;
    let day = date[6..8].parse::<u32>().ok()?;
    let hour = time[0..2].parse::<u32>().ok()?;
    let minute = time[2..4].parse::<u32>().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    Some(DateTimeParts {
        year,
        month,
        day,
        hour,
        minute,
    })
}

fn format_hhmm(dt: DateTimeParts) -> String {
    format!("{:02}:{:02}", dt.hour, dt.minute)
}

fn add_minutes(dt: DateTimeParts, minutes: i64) -> DateTimeParts {
    let total = dt.hour as i64 * 60 + dt.minute as i64 + minutes;
    let minute_of_day = total.rem_euclid(24 * 60);
    DateTimeParts {
        hour: (minute_of_day / 60) as u32,
        minute: (minute_of_day % 60) as u32,
        ..dt
    }
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year - if month <= 2 { 1 } else { 0 };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let mp = month as i32 + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) as i64
}

fn civil_from_days(days: i64) -> DateTimeParts {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if month <= 2 { 1 } else { 0 };
    DateTimeParts {
        year: year as i32,
        month: month as u32,
        day: day as u32,
        hour: 0,
        minute: 0,
    }
}

fn course_day(dt: DateTimeParts) -> u8 {
    let days = days_from_civil(dt.year, dt.month, dt.day);
    ((days + 3).rem_euclid(7) + 1) as u8
}

fn monday_of(dt: DateTimeParts) -> DateTimeParts {
    let days = days_from_civil(dt.year, dt.month, dt.day);
    civil_from_days(days - (course_day(dt) as i64 - 1))
}

fn format_date(dt: DateTimeParts) -> String {
    format!("{:04}-{:02}-{:02}", dt.year, dt.month, dt.day)
}

fn parse_duration_minutes(value: &str) -> Option<i64> {
    let s = value.trim().to_uppercase();
    if !s.starts_with('P') {
        return None;
    }
    let mut total = 0_i64;
    if let Some(caps) = regex::Regex::new(r"(\d+)D").unwrap().captures(&s) {
        total += caps.get(1)?.as_str().parse::<i64>().ok()? * 24 * 60;
    }
    let time_part = s.split_once('T').map(|(_, t)| t).unwrap_or("");
    if let Some(caps) = regex::Regex::new(r"(\d+)H").unwrap().captures(time_part) {
        total += caps.get(1)?.as_str().parse::<i64>().ok()? * 60;
    }
    if let Some(caps) = regex::Regex::new(r"(\d+)M").unwrap().captures(time_part) {
        total += caps.get(1)?.as_str().parse::<i64>().ok()?;
    }
    if total > 0 {
        Some(total)
    } else {
        None
    }
}

fn unescape_ics_text(value: &str) -> String {
    value
        .replace("\\n", "\n")
        .replace("\\N", "\n")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
        .trim()
        .to_string()
}

fn week_number_of(target: DateTimeParts, semester_start: DateTimeParts) -> u32 {
    let target_days = days_from_civil(target.year, target.month, target.day);
    let start_days = days_from_civil(
        semester_start.year,
        semester_start.month,
        semester_start.day,
    );
    let target_monday = target_days - (course_day(target) as i64 - 1);
    let start_monday = start_days - (course_day(semester_start) as i64 - 1);
    if target_monday < start_monday {
        0
    } else {
        ((target_monday - start_monday) / 7 + 1) as u32
    }
}

fn parse_rrule_weeks(
    rrule: Option<&str>,
    start: DateTimeParts,
    semester_start: Option<DateTimeParts>,
) -> Vec<u32> {
    let Some(rule) = rrule else {
        return Vec::new();
    };
    let upper = rule.to_uppercase();
    if !upper.contains("FREQ=WEEKLY") {
        return Vec::new();
    }

    let interval = regex::Regex::new(r"(?i)INTERVAL=(\d+)")
        .unwrap()
        .captures(rule)
        .and_then(|caps| caps.get(1)?.as_str().parse::<u32>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(1);

    let start_week = semester_start
        .map(|sem| week_number_of(start, sem))
        .filter(|&week| week > 0)
        .unwrap_or(1);

    if let Some(count) = regex::Regex::new(r"(?i)COUNT=(\d+)")
        .unwrap()
        .captures(rule)
        .and_then(|caps| caps.get(1)?.as_str().parse::<u32>().ok())
    {
        return (0..count).map(|i| start_week + i * interval).collect();
    }

    let until = regex::Regex::new(r"(?i)UNTIL=(\d{8}T?\d{0,6}Z?)")
        .unwrap()
        .captures(rule)
        .and_then(|caps| parse_datetime(caps.get(1)?.as_str()));

    let total_weeks = if let Some(until_dt) = until {
        let start_monday =
            days_from_civil(start.year, start.month, start.day) - (course_day(start) as i64 - 1);
        let until_monday = days_from_civil(until_dt.year, until_dt.month, until_dt.day)
            - (course_day(until_dt) as i64 - 1);
        if until_monday < start_monday {
            0
        } else {
            ((until_monday - start_monday) / 7 + 1) as u32
        }
    } else {
        20
    };

    (0..total_weeks)
        .filter(|i| i % interval == 0)
        .map(|i| start_week + i)
        .collect()
}

fn parse_one_vevent(block: &str, semester_start: Option<DateTimeParts>) -> Option<Course> {
    let mut dt_start_params = "";
    let mut dt_start_value = "";
    let mut dt_end_params = "";
    let mut dt_end_value = "";
    let mut duration_value = "";
    let mut rrule_value = "";
    let mut summary = String::new();
    let mut location = String::new();

    for line in block.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let upper = line.to_uppercase();
        if upper.starts_with("DTSTART") {
            let (params, value) = split_ics_property(line)?;
            dt_start_params = params;
            dt_start_value = value;
        } else if upper.starts_with("DTEND") {
            let (params, value) = split_ics_property(line)?;
            dt_end_params = params;
            dt_end_value = value;
        } else if upper.starts_with("DURATION") {
            let (_, value) = split_ics_property(line)?;
            duration_value = value;
        } else if upper.starts_with("RRULE") {
            let (_, value) = split_ics_property(line)?;
            rrule_value = value;
        } else if upper.starts_with("SUMMARY") {
            let (_, value) = split_ics_property(line)?;
            summary = unescape_ics_text(value);
        } else if upper.starts_with("LOCATION") {
            let (_, value) = split_ics_property(line)?;
            location = unescape_ics_text(value);
        }
    }

    if dt_start_value.is_empty() || dt_start_params.to_uppercase().contains("VALUE=DATE") {
        return None;
    }
    let start_dt = parse_datetime(dt_start_value)?;
    let end_dt = if !dt_end_value.is_empty() && !dt_end_params.to_uppercase().contains("VALUE=DATE")
    {
        parse_datetime(dt_end_value).unwrap_or_else(|| add_minutes(start_dt, 45))
    } else if !duration_value.is_empty() {
        add_minutes(
            start_dt,
            parse_duration_minutes(duration_value).unwrap_or(45),
        )
    } else {
        add_minutes(start_dt, 45)
    };

    let name = if summary.trim().is_empty() {
        "（无标题）".to_string()
    } else {
        summary.trim().to_string()
    };
    let weeks = parse_rrule_weeks(
        if rrule_value.is_empty() {
            None
        } else {
            Some(rrule_value)
        },
        start_dt,
        semester_start,
    );

    Some(Course {
        day: course_day(start_dt),
        name,
        start: format_hhmm(start_dt),
        end: format_hhmm(end_dt),
        room: location,
        week_type: Course::WEEK_ALL.to_string(),
        weeks,
    })
}

pub fn infer_semester_start(text: &str) -> Option<String> {
    let unfolded = unfold_ics(text);
    let blocks = extract_vevents(&unfolded);
    let mut earliest: Option<DateTimeParts> = None;
    for block in blocks {
        for line in block.lines().map(str::trim).filter(|line| !line.is_empty()) {
            let upper = line.to_uppercase();
            if !upper.starts_with("DTSTART") || upper.contains("VALUE=DATE") {
                continue;
            }
            let Some((params, value)) = split_ics_property(line) else {
                continue;
            };
            if params.to_uppercase().contains("VALUE=DATE") {
                continue;
            }
            let Some(dt) = parse_datetime(value) else {
                continue;
            };
            let days = days_from_civil(dt.year, dt.month, dt.day);
            let replace = earliest
                .map(|old| days < days_from_civil(old.year, old.month, old.day))
                .unwrap_or(true);
            if replace {
                earliest = Some(dt);
            }
        }
    }
    earliest.map(|dt| format_date(monday_of(dt)))
}

pub fn import_from_ics_with_meta(
    text: &str,
    semester_start: Option<&str>,
) -> Result<(Vec<Course>, Option<String>), String> {
    let inferred_semester_start = infer_semester_start(text);
    let effective_semester_start = semester_start.or(inferred_semester_start.as_deref());
    let courses = import_from_ics(text, effective_semester_start)?;
    Ok((
        courses,
        effective_semester_start
            .map(str::to_string)
            .filter(|s| !s.trim().is_empty()),
    ))
}

pub fn import_from_ics(text: &str, semester_start: Option<&str>) -> Result<Vec<Course>, String> {
    let unfolded = unfold_ics(text);
    let blocks = extract_vevents(&unfolded);
    if blocks.is_empty() {
        return Err("未找到 VEVENT 日程".to_string());
    }

    let semester_start =
        semester_start.and_then(|s| parse_datetime(&format!("{}T000000", s.replace('-', ""))));
    let mut out = Vec::new();
    for block in blocks {
        if let Some(course) = parse_one_vevent(&block, semester_start) {
            out.push(course);
        }
    }

    if out.is_empty() {
        return Err("未解析到有效日程（需含 DTSTART 时间的 VEVENT）".to_string());
    }

    out.sort_by(|a, b| {
        a.day
            .cmp(&b.day)
            .then_with(|| a.start.cmp(&b.start))
            .then_with(|| a.end.cmp(&b.end))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(out)
}
