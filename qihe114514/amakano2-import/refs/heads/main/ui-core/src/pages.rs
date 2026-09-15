//! 页面装配。

mod device;
mod library;
mod logs;
mod overview;
mod saves;
mod settings;
mod stats;

use super::glass;
use super::node::Node;
use super::snapshot::{Page, Snapshot};

/// 按当前页装配整棵树（含外壳）。
pub fn render(snapshot: &Snapshot) -> Node {
    let content = match snapshot.page {
        Page::Overview => overview::render(snapshot),
        Page::Library => library::render(snapshot),
        Page::Saves => saves::render(snapshot),
        Page::Stats => stats::render(snapshot),
        Page::Device => device::render(snapshot),
        Page::Settings => settings::render(snapshot),
        Page::Logs => logs::render(snapshot),
    };
    glass::shell(snapshot, content)
}
