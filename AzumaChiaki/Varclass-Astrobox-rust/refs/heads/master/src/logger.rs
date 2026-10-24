//! 日志输出
//!
//! 仅使用 stdout，不写入文件。WASI 插件沙箱可能无法创建 logs/ 目录，
//! rolling::daily 会导致 panic 进而导致 "Plugin thread unexpectedly closed"。

use std::io::{self, Write};
use std::sync::OnceLock;

use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

static LOGGER_INITED: OnceLock<()> = OnceLock::new();

pub fn init() {
    if LOGGER_INITED.get().is_some() {
        return;
    }

    let writer = move || PluginWriter(io::stdout());
    let console_layer = fmt::layer()
        .with_target(true)
        .with_ansi(false)
        .with_file(true)
        .with_line_number(true)
        .with_writer(writer)
        .compact();
    let result = tracing_subscriber::registry()
        .with(console_layer)
        .try_init();

    if result.is_ok() {
        let _ = LOGGER_INITED.set(());
    }
}

pub fn info(message: impl AsRef<str>) {
    tracing::info!("{}", message.as_ref());
}

pub fn warn(message: impl AsRef<str>) {
    tracing::warn!("{}", message.as_ref());
}

pub fn error(message: impl AsRef<str>) {
    tracing::error!("{}", message.as_ref());
}

struct PluginWriter<W: Write>(W);

impl<W: Write> Write for PluginWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write_all(b"[Plugin] ")?;
        self.0.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}
