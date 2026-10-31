//! 页面装配。

mod push;
mod saves;
mod settings;
mod stats;

use super::glass;
use super::node::Node;
use super::snapshot::{Page, Snapshot};

/// 按当前页装配整棵树（含外壳）。
pub fn render(snapshot: &Snapshot) -> Node {
    let content = match snapshot.page {
        Page::Push => push::render(snapshot),
        Page::Saves => saves::render(snapshot),
        Page::Stats => stats::render(snapshot),
        Page::Settings => settings::render(snapshot),
    };
    glass::shell(snapshot, content)
}
