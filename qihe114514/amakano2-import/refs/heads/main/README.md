# 甜蜜女友2导入插件

**全部 15 章章节包已经内置在插件里**（`packs/p01.pack` … `p15.pack` + `packs/index.json`，合计 18.6 MB），不再需要挑选 `.pack` 文件：打开插件就能看到章节列表，连接手环后按章点「同步」即可。

## 界面

界面已整体重写：从「单页长滚动 + 纯色卡片 + 半透明玻璃」改成**七页 + 石墨灰实色面板 + 普通按钮**。每页只列自己该管的事：

- **概览**：连接状态、同步进度与队列、断点续传、下一步建议（实时进度）—— 原来的「传输」页并到了这里；
- **章节**：15 章清单（标题、阅读时长估算、体积、已安装标记）、线路筛选、单章「同步 / 重传」、「同步剩余 N 章」按顺序排队；
- **存档**：手环上的自动存档与手动槽（章节名 / 场景号 / 时间）、**导出到剪贴板** / **从剪贴板导入**、读档（设为「继续阅读」）、删除 —— **这是全插件唯一保留二次确认的操作**；
- **统计**：手环上累计的阅读天数与时长（阅读天数 / 总阅读时长 / 总阅读天数 / 今日阅读时长 / 单日阅读最长时长），以及最近 30 天的按天明细；
- **设备**：连接设备 / 重新连接、设备名与地址、打开手环游戏、手环已安装章节（**一步删除，不再二次确认**）、列表标题右侧的「刷新」；「旧版本」只在这一张列表里用徽章 + 页脚说明标出，**不再单独开一张卡**（同一条记录出现在两处，计数会跟顶栏的「N/15 章已安装」对不上）；
- **设置**：传输分片与协议参数（**分片档位只剩这一个入口**）、行为、未完成缓存、关于；
- **日志**：插件运行日志，可按级别筛选、可清空。

### 「统计」页的数据全部由手环侧算好

插件**不实现任何「秒 → 几小时几分」「连续几天」的换算**：手环回包里的 `labels` 与
`recent[].label` 就是界面要显示的字符串，插件只负责摆上去。两边各算一遍迟早会不一致，
而那种不一致在界面上看不出谁对谁错 —— 协议与理由见仓库
`docs/插件开发注意事项.md` 第 9 节、`docs/章节包导入.md` 的「阅读统计通道」。

### 存档的导出 / 导入走**剪贴板**（不是系统文件对话框）

- **导出到剪贴板**：把手环上的存档打包成一段缩进过的 JSON 文本写进系统剪贴板，界面给「已复制 N 个存档到剪贴板（约 X KB / Y 字节）」并附带建议文件名（`amakano2-saves-YYYY-MM-DD.json`）——把它粘到备忘录 / 聊天窗口 / 任意文本文件里就存下来了。
- **从剪贴板导入**：复制那段 JSON（整段，别只粘一半）再点按钮，插件解析信封、按 `savedAt` 并入手环（手环上已有同一份的覆盖、新的追加到末尾），界面给「导入 N 个存档，跳过 M 个重复（手环上现在有 K 条）」。失败全是人话：剪贴板为空 / 不是 JSON / `format` 不符 / `save_version` 太高 / 一个槽都没有。
- 为什么不走系统的保存 / 打开对话框：**真机实测那一类需要用户交互的宿主调用永远不返回，会把插件的事件分发器堵死**（日志只有 `dialog probe started`、没有 finished，之后整个插件点不动，只能靠「禁用 / 启用插件」救回来）。`src/host/dialog.rs` 已整个删除，**别再把它加回来**；`src/host/clipboard.rs` 是新的封装。完整证据与硬规矩见仓库 `docs/插件开发注意事项.md` 第 7 节。
- 剪贴板正好也满足「存档不落在插件目录里」这条老规矩（宿主安装失败会把整个插件目录删掉，用户存档放那儿等于随插件共存亡）。
- **权限**：官方权限表里没有 clipboard 条目，所以 `manifest.json` **一个权限都没加**（不猜权限名）。若真机上宿主返回错误，界面会照实渲染错误原文，日志关键字是 `save export to clipboard failed` / `save import could not read the clipboard` —— 带着这两句再决定要不要补。

### 界面接口是 `ui-v3`

界面用宿主提供的 `ui-v3` 接口（`astrobox-ng-wit` 0.2.2，Rust 路径 `astrobox_ng_wit::astrobox::psys_host::ui_v3`；剪贴板走同版本的 `clipboard` 接口）。只有 `ui-v3` 有 `backdrop-filter` / `filter` / `transform` / `transition`、网格、滚动区、徽章与弹窗——七页里的滚动导航、徽章、弹窗靠的就是这套；旧的 `ui` v2 做不出来，只能是一堆静态方块。按钮本身是最朴素的实色按钮（见下），不依赖这些能力。

但 `manifest.json` 的 `api_level` **保持 2**（`wasi_version` 仍是 2，权限不变）。真机实测（AstroBox 2.1.0，2026-09-11）：声明 3 会让宿主的**安装校验直接拒绝安装**——日志只有一句 `Failed to add files to queue`、不给原因，而且**会把已安装的插件目录删掉**；把同一份包只把 `api_level` 改回 2 就能装上、插件正常加载，七页 `ui-v3` 界面完整可用。**声明等级与运行时能力是两件事**：`ui-v3` 照用，`api_level` 必须写 2，别改回去。完整证据链见仓库 `docs/插件开发注意事项.md` 第 6 节。

> **导航条仍包在横向滚动区里**（兜底，不是摆设）：七个两字标签在 400px 窗口（手机版 AstroBox）
> 里内容 **261px**、可用 **326px**（余 65px），460px 下可用 386px（余 125px）。没有滚动区时
> 「放不下」就等于「最后一项点不到」，所以 `glass::nav_bar` 外层保留 `Tag::Scroll` + `scroll("x")`：
> 装得下时整条居中，装不下时只滚不裁。改标签 / 加页之前请先量（`tools/README.md`「导航条宽度实测」），
> 见 `docs/插件开发注意事项.md` 6.13。

当前 `manifest.json`：`version 0.7.0` / `api_level 2` / `wasi_version 2`，`additional_files` 只有 15 个 `packs/pXX.pack` + `packs/index.json`（**没有 `assets/`**，随包文件共 16 个）。当前产物 `dist/amakano2-import-0.7.0.abp`：**17,975,962 字节（17.14 MB）**，19 个条目 = 16 个随包文件 + `entry`（`amakano2_import.wasm`，1,186,800 字节）+ `icon.png` + `manifest.json`；`dist/amakano2-import.abp` 是同一份（不带版本号的当前包）。`0.6.0` 及更早的副本都是**七页重构之前**的产物，别拿来验收新界面。

打包（在插件目录下执行，会先 `cargo build --release` 编 wasm，再按 `manifest.json` 的 `additional_files` 收文件打 zip）：

```powershell
$env:PYTHON = "C:\Users\<你>\.workbuddy\binaries\python\versions\3.13.12\python.exe"   # 或任意可用的 python.exe
node ..\..\tools\run-python.js scripts\build_dist.py --release --package
Copy-Item dist\amakano2-import.abp dist\amakano2-import-0.7.0.abp                      # 按版本留一份副本
```

⚠️ 改界面之后**记得先把 `manifest.json` 的 `version` 顺延一位**再打包（插件的版本号是给用户看的那一个，`plugin_version()` 运行时读的就是它）；`tools/bump-version.js` 只管**手环应用**的 `versionCode`，跟插件无关。

## 按钮与页面配色

按钮就是最普通的按钮：**一块实色圆角胶囊 + 居中文字**，按下时底色变深一点（深色底的那一档反过来是提亮）。就这些——没有模糊、没有渐变、没有描边、没有阴影、没有缩放动效，按钮内部也没有子元素。

- **几何**（`ui-core/src/theme.rs` 的 `BUTTON_*` / `CHIP_*`）：主按钮高 **48**、水平内边距 **16**、标签 **15px**、胶囊圆角 = 高度一半；行内按钮按密集列表行的密度压到 **高 30 / 水平内边距 11 / 圆角 15 / 竖直内边距 6**（本项目有意的取舍，不是漏写）。
- **外观只有一张表**：主按钮 `BUTTON_PRIMARY = "#C2557F"` + 白字（按下 `BUTTON_PRIMARY_PRESSED = "#A8456A"`）、次要按钮 `BUTTON_GHOST = "#262A30"` + 浅字（按下 `BUTTON_GHOST_PRESSED = "#32373E"`）、危险按钮 `BUTTON_DANGER = "#B4463C"` + 白字（按下 `BUTTON_DANGER_PRESSED = "#943A32"`）。全是不透明实色，没有半透明染色。**品牌粉只用于主操作**：一屏通常只有一颗主按钮，其余一律幽灵档。
- **按下只剩一件事**：底色换成按下档（`ui-core/src/glass.rs` 的 `skin()` 按按下状态选一支）；按下事件走 `PointerDown`，七页共用一个 `button()` 外壳，五种按钮（primary / ghost / chip / quiet / danger）都直接委托给它。
- **曾尝试做玻璃按钮，已放弃**：试过「半透明染色 + 亮边 + 背景模糊 + 按下缩放/光流」，这个宿主做不到——`prop()` 逃生舱对 `background` / `background-image` / `box-shadow` 全是空操作（渐变、阴影全落空），折射与色散更没有承载（界面是宿主的声明式元素树，没有 canvas、跑不了 shader、插件也不能执行 JS），剩下的「半透明 + 描边 + 模糊」看着并不像玻璃。**能达成的玻璃 ≠ 真正的玻璃质感**，相关常量与做法已整批删除，测试守住朴素形态。

页面**自己不下发底色**：`ui-core/src/glass.rs` 的 `shell()` 不挂 `bg()`，页面背景交给 AstroBox 自己的背景（用户要求「不要自绘页面背景，保持空白，用 astrobox 的背景就行」；原来的页面底色常量 `PAGE_BG` 已整个删除）。同一处**只给上下内边距**（`pad_y(PAGE_PAD_Y)`，不给左右）——宿主已经自带左右安全区，再叠一层内容会窄掉一大块。面板仍然是**不透明实色**，但换成了**中性石墨灰**的三级层次（`ui-core/src/theme.rs`：`SURFACE = "#191B1F"` / `SURFACE_SOFT = "#22252A"` / `SURFACE_STRONG = "#2B2F35"`；一级面板一层极淡描边 `STROKE`，二级块不描边），`panel()` / `nested()` 只下发填色与描边，**不挂背景模糊、也没有光泽渐变**——原因不是审美而是宿主能力：`prop()` 逃生舱是**空操作**（`background` / `background-image` / `box-shadow` 下发后界面上毫无变化），渐变与外投影/内高光都做不出来。**层次只能靠实色深浅 + 圆角 + 留白**，所以上一版的灰蓝底 + 满屏彩色徽章整个换掉了。

页面的模糊与玻璃质感、插件自带的壁纸与图标、设置页的临时「渲染自检」卡与「存档功能自检（临时）」卡**都已删除**（顶栏品牌位现在是一枚品牌色圆角方块 `glass::orb()`；设置页只剩传输分片与协议参数 / 行为 / 未完成缓存 / 关于四块）。完整原因与实测证据见仓库 `docs/插件开发注意事项.md` 6.2 / 6.6 / 6.9 / 6.10b。

## 界面代码分成两个 crate

- `astrobox-plugin/amakano2-import/ui-core/`（crate 名 `amakano2-ui`）：**与宿主无关的界面树**，只产出抽象的 `Node`，不依赖 `astrobox-ng-wit`。因为不含宿主类型，可以在宿主机上直接跑测试（插件本体是 wasm 组件，宿主机上编不过）：

  ```powershell
  cargo test -p amakano2-ui --target x86_64-pc-windows-msvc
  ```

  目前 **100 个 Rust 单元测试通过** + 1 项 `--ignored` 的手工预览导出，覆盖主题 token（石墨灰三级层次 + 四级文字 + 品牌粉只上主操作）、按钮外观（实色底 + 圆角 + 按下换底色 + 禁用变淡）、面板装配、页面装配与动作映射、七页导航（页数、顺序、每项 `shrink(0)`、横向滚动兜底）、存档页的剪贴板文案与导入计数、统计页的五个数（含「文案来自手环侧、插件不换算」的守护），以及窄窗布局的守护（页面外壳不下发底色 / 不给左右内边距 / 不挂切页动效；章节库的线路筛选条横向可滚动且每一项 `shrink(0)`；章节行的序号砖与按钮 `shrink(0)`、文字列 `grow(1.0)`；设备页每条已安装记录只出现一次）。

  **`--target` 不能省**：本目录的 `.cargo/config.toml` 把默认 target 钉在 `wasm32-wasip2`（给插件本体用），不写 `--target` 会直接编译失败——`demo()` 被 `#[cfg(not(target_arch = "wasm32"))]` 关掉，而预览导出的测试要用它。
- 插件本体：`src/host/render.rs` 是**唯一**碰宿主 UI 类型的地方（`Node` → `ui_v3::Element`），`src/lib.rs` 只负责业务与状态；`src/host/saves.rs` 是**纯逻辑**（存档信封的解析 / 校验 / 按 `savedAt` 合并，宿主机上可测），`src/host/clipboard.rs` 是唯一的剪贴板封装。

## 装到设备前先肉眼验收界面

`ui-core` 能把整棵界面树导出成 JSON；离线渲染成 HTML 后截图，就能在没连手环时确认界面没崩：

```powershell
cargo test -p amakano2-ui --target x86_64-pc-windows-msvc -- --ignored dump_preview  # 导出 work/ui-tree.json
node ..\..\tools\ui-preview.mjs ..\..\work\ui-tree.json ..\..\work\ui-preview.html 460
node ..\..\tools\ui-shot.mjs ..\..\work\ui-preview.html ..\..\work\ui-shot\ui-460.png 460 9800
```

产物：`work/ui-preview.html`（可直接用浏览器打开）与 `work/ui-shot/ui-460.png`（全页全览，高度按内容调）。**宽度给 460，并且要再出一次 400**：插件页在 AstroBox 里是个窄窗（桌面版约 460px，**手机版更窄**），工具默认的 980px 会漏掉「按钮被挤出卡片」「本该并排的格子竖着堆」这类只有窄宽才暴露的问题。⚠️ `ui-shot.mjs` 的窗口**有最小宽度**（给 400 实际得到 500 的视口，于是图看起来像右边被切了），要看全容器请按 `宽度 + 100` 截图再按像素裁掉两侧留白，细节见 `tools/README.md`。预览样本**直接读真实的 `packs/index.json` 与 `manifest.json`**，所以预览里的章节数、阅读时长、体积、版本号和真机一致。

导出的树默认含 **8 个视图**：`Page::ALL` 的 **7 个页面** + 一张「**存档（通道不可用）**」对照（`preview_all()` 里手工追加的 `BLOCKED_VIEW_NAME`，手环没回 `hello-ok` 时长什么样）。再加环境变量 `UI_TREE_SAVES_UNINSTALLED=1` 可另出一份「存档所在的章节还没装」的对照树。**页数改了这两处不用改代码**（都从 `Page::ALL` 来），但截图脚本里的视图名要跟着对。

预览工具的第 4 个参数可以传一张真实图片当壁纸（`node tools/ui-preview.mjs <树> <html> 460 <图片>`），但那是**可选的调试手段**，不是必需项：插件自己不自带壁纸，面板也仍是实色底；不过**页面已经不下发底色**，所以那层图片会在卡片之间透出来（正好用来确认「页面真的没有自绘背景」）。

## 连接行为

`连接设备` → 找已配对设备（在线列表为空时补查宿主的设备记录，区分「没连过」和「掉线」）→ 注册回包通道（`register_interconnect_recv`）→ **按包名直接 `launch-qa` 自动打开手环应用**（官方文档：fingerprint 为空时宿主会按包名补全签名，所以不必先等设备回应用列表）→ 失败才取一次应用列表用于诊断（没这个应用 / 列表不可用 / 宿主拒绝）→ **每 2.5 秒探测一次，最多 12 次**。

只要手环应用在前台，探测就会成功：状态变成「已连接《甜蜜女友2》，章节列表已同步」，并自动带上未完成传输信息。所以自动打开失败也没关系，手动打开应用后插件会自己接上。

> ⚠️ 排障提醒：所有 `thirdpartyapp` / `device` / `interconnect` / `register_interconnect_recv` 接口都需要在 `manifest.json` 的 `permissions` 里声明，否则一律返回 `Err(())`；定时器事件的载荷是 `{"timerId":..,"kind":..,"payload":"<你传入的字符串>"}`，要拆一层再用。细节见仓库 `docs/插件开发注意事项.md`。

## 通信行为

所有请求（未完成传输、章节列表、删除、清理缓存）都带 1.6 秒超时与最多 3 次重试，不会无限期卡住；探测请求超时不报错，由探测循环统一决定下一步。传输中断时重试 4 次后停下并提示重新连接设备，保留断点，重连后点同一章的「同步」会从手环返回的 `resumeFrom` 接着传。

日志从只写宿主控制台改成**同时进内存环形缓冲（最近 240 行）**，插件界面的「日志」页可以直接看、按级别筛选、清空——排障时截图就够了。

## 章节包怎么进来的

AstroBox 只允许插件用 `std::fs` 读取**自身目录**下的文件（见官方“运行环境”页的文件系统安全策略），所以章节包作为 `additional_files` 随插件一起安装，运行时从 `packs/index.json` 读取清单。若目录结构有出入，插件会在状态行给出「当前目录 + 可见文件」的诊断信息。

仓库里的生成顺序：

```powershell
npm run build:packs --prefix ..\..      # 先生成 dist/*.pack
npm run bundle:plugin --prefix ..\..    # 复制进插件 packs/（ASCII 文件名）+ 刷新手环端章节表
node ../../tools/run-python.js scripts/build_dist.py --release --package
```
