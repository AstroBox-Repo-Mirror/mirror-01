//! 设备连接检查
//!
//! 封装 sync 层，供 UI 或外部调用。

use crate::sync;

/// 获取首个已连接设备地址，若无则返回错误
pub async fn check_connected_device() -> Result<String, String> {
    sync::first_connected_device_addr()
        .await
        .ok_or_else(|| "当前无已连接设备，请先在 AstroBox 连接手表".to_string())
}
