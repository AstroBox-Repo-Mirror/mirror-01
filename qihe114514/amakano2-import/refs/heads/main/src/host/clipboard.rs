// ------------------------------------------------------------------ 剪贴板封装（宿主专属）
//
// 导出/导入走剪贴板的**原因**写在 `docs/插件开发注意事项.md` 第 7 节：
// Dialog（`save-file-start` / `pick-file`）这个宿主上**需要用户交互的调用永远不返回**，
// 会把事件分发器堵死（真机日志只有 `dialog probe started`、没有 finished，
// 之后连与对话框无关的按钮都点不动）。剪贴板不需要用户交互，所以它是这条路上唯一还能用的落点。
//
// 这里的两个函数都**不做任何等待用户的事**：调用方（`lib.rs` 的
// `start_save_export` / `start_save_import`）仍然把它们丢在 `spawn` 里跑 —— 那是一条硬规矩
// （`on_event` / `on_ui_event` 必须立刻返回），不是对这些接口的额外怀疑。
//
// `astrobox-ng-wit` 0.2.2 的 `astrobox-psys-host.wit`（第 25-28 行）原文：
//
// ```wit
// interface clipboard {
//     read-text: func() -> future<result<string>>;
//     write-text: func(text: string) -> future<result>;
// }
// ```
//
// 也就是说：读回的是 `result<string>`，写只回一个 `result`（没有内容），两边失败都是 `Err`，
// 但 WIT 里**没有携带错误细节** —— 所以失败时只能把「宿主返回了错误」这句话交给界面，
// 真机证据（日志行）才是下一步判断权限的唯一依据。

use astrobox_ng_wit::astrobox::psys_host::clipboard;

use crate::saves::SaveError;

/// 导出用的一步：把信封文本写进剪贴板。
///
/// 写成功后顺手**读回一次**做核对（长度 + 内容完全相同才算核对通过）。
/// 读回失败**不算导出失败**（写已经成功了，用户要的数据就在剪贴板里），
/// 它只是核对不了 —— 返回值里的 `verified` 会把这件事说清楚，界面照实讲。
///
/// 返回 `(信封字节数, 是否读回核对通过)`。
pub async fn write_envelope(text: &str) -> Result<(usize, bool), SaveError> {
    clipboard::write_text(text)
        .await
        .map_err(|()| SaveError::new("写入剪贴板失败（宿主返回了错误，可能是权限或剪贴板不可用）"))?;
    let bytes = text.len();
    let verified = match clipboard::read_text().await {
        Ok(back) if back == text => true,
        Ok(back) => {
            // 长度对不上就没必要把整段文本打进日志，给个规模就够定位了。
            tracing::warn!(
                expected = bytes,
                actual = back.len(),
                "clipboard read-back differs from what was written"
            );
            false
        }
        Err(()) => {
            // 读回要单独一次权限/焦点，失败很常见；导出本身已经成功了。
            tracing::warn!(bytes, "clipboard read-back failed, the write itself succeeded");
            false
        }
    };
    Ok((bytes, verified))
}

/// 导入用的一步：把剪贴板里的文本读出来。
///
/// 空文本按「剪贴板是空的」处理（`Ok("")`），由 `saves::parse_save_text` 给出人话 ——
/// 这里不抢着报错，免得同一件事有两种说法。
pub async fn read_envelope_text() -> Result<String, SaveError> {
    clipboard::read_text()
        .await
        .map_err(|()| SaveError::new("读取剪贴板失败（宿主返回了错误，可能是权限或剪贴板不可用）"))
}
