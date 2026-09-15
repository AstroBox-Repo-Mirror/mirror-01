# 喵伴 AstroBox V2 发布包

本目录用于公开 GitHub 仓库 `miaoban-astrobox-release`，资源类型为付费快应用。

## 资源信息

- 名称：喵伴
- 类型：quick_app
- 价格：付费资源，应用内使用购买的兑换码激活
- 作者：阿呜（AstroBox 绑定名：A_WU）
- 版本：2.0.1
- 下载文件：downloads/miaoban-2.0.1.rpk

## 支持设备

- xmb9
- xmb9p
- xmb10
- xmb10nfc
- xmb10p
- xmrw5
- xmrw5xring
- xmrw6

## 媒体素材

- media/icon.png
- media/cover.jpg
- media/preview-decorate.png
- media/preview-map.png
- media/preview-clean.png
- media/preview-blessing.png
- media/preview-wonderland.png
- media/preview-device-pro9.png
- media/preview-device-rw6.png
- media/preview-device-band10.png

## AstroBox V2 提交信息

正式上架优先使用 AstroBox CreatorConsole。CreatorConsole 会上传/更新资源发布仓库，读取资源仓库提交短哈希，并在提交 PR 时更新官方源 `index_v2.csv`。下面 CSV 仅用于核对字段，不作为手工提交入口。

```csv
com.awu.watch.petpal,喵伴,quick_app,jianding24,miaoban-astrobox-release,<commit>,media/icon.png,media/cover.jpg,喵伴;小猫;宠物;养成;治愈;装饰;小游戏;表盘联动;付费,xiaomi,xmb9;xmb9p;xmb10;xmb10nfc;xmb10p;xmrw5;xmrw5xring;xmrw6,paid
```

## 发布注意

- 官方源付费资源需要满足免费资源数量比例要求。
- 正式提交使用 AstroBox CreatorConsole 创建官方源 PR。
- `manifest_v2.json` 的资源名称、类型、设备列表必须与官方索引一致。
