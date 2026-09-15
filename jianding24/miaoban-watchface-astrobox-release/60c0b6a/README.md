# 喵伴表盘 AstroBox V2 发布包

本目录用于公开 GitHub 仓库 `miaoban-watchface-astrobox-release`，资源类型为免费表盘。

## 资源信息

- 名称：喵伴表盘
- 类型：watchface
- 价格：免费
- 作者：阿呜（AstroBox 绑定名：A_WU）
- 版本：1.1.0

## 支持设备

- xmb9：downloads/miaoban-watchface-band9-1.1.0.face
- xmb9p：downloads/miaoban-watchface-9pro-10pro-1.1.0.face
- xmb10：downloads/miaoban-watchface-band10-1.1.0.face
- xmb10nfc：downloads/miaoban-watchface-band10-1.1.0.face
- xmb10p：downloads/miaoban-watchface-9pro-10pro-1.1.0.face
- xmrw5：downloads/miaoban-watchface-rw5-rw6-1.1.0.face
- xmrw5xring：downloads/miaoban-watchface-rw5-rw6-1.1.0.face
- xmrw6：downloads/miaoban-watchface-rw5-rw6-1.1.0.face

AstroBox 官方 `devices_v2.json` 当前未列出小米手环 8 Pro 设备 ID，因此 8Pro 包先保留在 downloads 目录备用，不写入 `manifest_v2.json`。

## 媒体素材

- media/icon.png
- media/cover.jpg：1200x800 AstroBox 横向封面
- media/preview-9pro-10pro.png
- media/preview-aod-9pro-10pro.png
- media/preview-settings.png
- media/preview-task.png
- media/preview-rw5-rw6.png
- media/preview-band10.png

## AstroBox V2 提交信息

正式上架优先使用 AstroBox CreatorConsole。CreatorConsole 会上传/更新资源发布仓库，读取资源仓库提交短哈希，并在提交 PR 时更新官方源 `index_v2.csv`。下面 CSV 仅用于核对字段，不作为手工提交入口。

```csv
979801230201,喵伴表盘,watchface,jianding24,miaoban-watchface-astrobox-release,<commit>,media/icon.png,media/cover.jpg,喵伴;免费;小猫;互动;天气;压力;专注;任务;应用联动,xiaomi,xmb9;xmb9p;xmb10;xmb10nfc;xmb10p;xmrw5;xmrw5xring;xmrw6,
```

## 发布注意

- 这是免费表盘资源，不应标为 paid。
- 表盘可独立使用；喵伴应用只是提供装饰同步、猫名、等级金币、更多任务和收获结算。
- 正式提交使用 AstroBox CreatorConsole 创建官方源 PR。
