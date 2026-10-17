//! 日志：既写到 stdout（宿主控制台），也存一份进内存环形缓冲，供插件界面里的「日志」页显示。
//!
//! 加这一层的理由很实在：前几轮排障全靠用户复述屏幕上的状态行，
//! 而真正有用的 `tracing` 输出只在宿主控制台里。日志页把最近 240 行搬进 UI，
//! 出问题时可以直接截图发给作者。

use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::{Mutex, OnceLock};

use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

use amakano2_ui::{LogLevel, LogLine};

/// 内存里保留的日志行数。
pub const LOG_CAPACITY: usize = 240;

struct LogStore {
    lines: VecDeque<LogLine>,
    /// 还没凑满一行的尾部字节（宿主按块调用 write，一次调用不等于一行）。
    partial: String,
    errors: usize,
    warns: usize,
}

fn store() -> &'static Mutex<LogStore> {
    static STORE: OnceLock<Mutex<LogStore>> = OnceLock::new();
    STORE.get_or_init(|| {
        Mutex::new(LogStore {
            lines: VecDeque::with_capacity(LOG_CAPACITY),
            partial: String::new(),
            errors: 0,
            warns: 0,
        })
    })
}

fn with_store<T>(action: impl FnOnce(&mut LogStore) -> T) -> T {
    action(&mut store().lock().unwrap_or_else(|error| error.into_inner()))
}

/// 复制一份日志（只有打开日志页时才调用）。
pub fn snapshot() -> Vec<LogLine> {
    with_store(|store| store.lines.iter().cloned().collect())
}

/// 全量日志里的（警告数, 错误数）。计数不受环形缓冲淘汰影响，也不受级别筛选影响。
pub fn counts() -> (usize, usize) {
    with_store(|store| (store.warns, store.errors))
}

pub fn clear() {
    with_store(|store| {
        store.lines.clear();
        store.partial.clear();
        store.errors = 0;
        store.warns = 0;
    });
}

/// 把一块原始输出切行、判级、入队。暴露出来是为了能直接单元测试。
pub fn ingest(chunk: &str) {
    with_store(|store| {
        store.partial.push_str(chunk);
        // 最后一段可能不完整，留在 partial 里等下一次 write。
        while let Some(index) = store.partial.find('\n') {
            let line: String = store.partial.drain(..=index).collect();
            push_line(store, line.trim_end_matches(['\n', '\r', ' ']));
        }
        // 防御：单行极长且一直不换行时别把内存拖爆。
        if store.partial.len() > 4096 {
            let rest = std::mem::take(&mut store.partial);
            push_line(store, rest.trim_end());
        }
    });
}

fn push_line(store: &mut LogStore, line: &str) {
    let text = line.trim_start_matches("[Amakano2 Import] ").trim();
    if text.is_empty() {
        return;
    }
    let level = LogLevel::sniff(text);
    match level {
        LogLevel::Error => store.errors += 1,
        LogLevel::Warn => store.warns += 1,
        _ => {}
    }
    if store.lines.len() == LOG_CAPACITY {
        store.lines.pop_front();
    }
    store.lines.push_back(LogLine { level, text: text.to_string() });
}

/// 写入器：先落 stdout（保留原本的宿主控制台行为），再进环形缓冲。
struct Sink;

impl Write for Sink {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let mut stdout = io::stdout();
        let _ = stdout.write_all(b"[Amakano2 Import] ");
        stdout.write_all(buffer)?;
        ingest(&String::from_utf8_lossy(buffer));
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stdout().flush()
    }
}

static INITIALIZED: OnceLock<()> = OnceLock::new();

pub fn init() {
    if INITIALIZED.get().is_some() {
        return;
    }
    let layer = fmt::layer().with_ansi(false).with_writer(|| Sink).compact();
    if tracing_subscriber::registry().with(layer).try_init().is_ok() {
        let _ = INITIALIZED.set(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingest_assembles_lines_split_across_writes() {
        clear();
        ingest("2026-01-01T00:00:00Z  INFO 前半");
        // 还没换行，不应该产生日志行。
        assert!(snapshot().is_empty());
        ingest("段\n");
        let lines = snapshot();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].text.contains("前半段"));
        assert_eq!(lines[0].level, LogLevel::Info);
    }

    #[test]
    fn counts_track_levels_and_survive_trimming() {
        clear();
        ingest(" INFO ok\n WARN 慢\n ERROR 炸了\n");
        let (warns, errors) = counts();
        assert_eq!((warns, errors), (1, 1));
        assert_eq!(snapshot().len(), 3);
        assert_eq!(snapshot()[2].level, LogLevel::Error);
    }

    #[test]
    fn capacity_drops_oldest_lines() {
        clear();
        for index in 0..(LOG_CAPACITY + 20) {
            ingest(&format!(" INFO 第 {index} 行\n"));
        }
        let lines = snapshot();
        assert_eq!(lines.len(), LOG_CAPACITY);
        assert!(lines[0].text.contains(&format!("第 {} 行", 20)));
    }

    #[test]
    fn strips_own_prefix_and_ignores_blank_lines() {
        clear();
        ingest("[Amakano2 Import]  INFO 有前缀\n\n   \n");
        let lines = snapshot();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "INFO 有前缀");
    }

    #[test]
    fn clear_resets_everything() {
        clear();
        ingest(" ERROR 炸了\n");
        clear();
        assert!(snapshot().is_empty());
        assert_eq!(counts(), (0, 0));
    }
}
