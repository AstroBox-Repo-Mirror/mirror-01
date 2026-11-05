//! 页面装配。

mod account;
pub(crate) mod landing;
mod push;
mod saves;
mod settings;
mod stats;

use super::glass;
use super::node::Node;
use super::snapshot::{Page, Snapshot};

/// 按当前状态装配整棵树。
///
/// 两种形态，由 [`Snapshot::native_ui`] 决定：
///
/// - **默认（`false`）**：引导页 —— 一个「打开控制面板」的主按钮 + 一个
///   「改用原生界面」的后备入口。HTML 面板才是完整界面。
/// - **后备（`true`）**：宿主 `ui-v3` 画出来的完整五页界面，给打不开 WebView
///   的设备用，顶上带一个「返回」。
pub fn render(snapshot: &Snapshot) -> Node {
    if !snapshot.native_ui {
        // 引导页仍然套外壳：用户在这儿也能看见连接状态与传输进度 ——
        // 打不开 HTML 面板的设备至少知道手环连上没。
        return glass::shell(snapshot, landing::render(snapshot));
    }
    let content = match snapshot.page {
        Page::Push => push::render(snapshot),
        Page::Saves => saves::render(snapshot),
        Page::Stats => stats::render(snapshot),
        Page::Account => account::render(snapshot),
        Page::Settings => settings::render(snapshot),
    };
    glass::shell(snapshot, content)
}
