AstroBox v2 插件文档 - 完整纯文本版
来源: https://plugindoc.astrobox.online

这篇文档来自官方文档，并且结合自身开发过程补充了一些注意事项（我自己踩的坑）同时将其转化为纯文本以便面向LLM编程
  AstroBox 插件

# AstroBox 插件
 在基于WebAssembly的全新v2插件系统上开发高性能、高可玩性的AstroBox插件，扩展使用体验 开始阅读


  工作原理

# 工作原理
 
## 简介

AstroBox v2 的插件系统基于 WebAssembly System Interface (WASI) 构建，并通过 WIT Component 实现 Host（宿主端）与 Plugin（插件端）之间的互操作接口。
这两项技术都是近年才趋于成熟的新兴标准，我们非常自豪地率先在 AstroBox 中集成它们，并为其带来了 卓越的跨平台兼容性。

2025 年 12 月 22 日，openvela 微信公众号发文表示将使用 LWAC 为 JS 应用注入原生动力，该技术基于 WebAssembly，与 AstroBox v2 的插件系统技术栈高度相近。

AstroBox 使用 wasmtime 运行 WASI 插件，性能几乎可与原生代码相媲美（在 iOS 上由于缺乏 JIT 支持，性能可能略有下降）。

⚠️ 注意：这是 AstroBox v2 的插件文档，如果您正在寻找 AstroBox v1 的插件文档，请访问 这里，但我们不建议继续为v1开发插件。

------------------------------------------------------------

## 运行流程

- 

插件开发与编译
开发者可以使用任何支持 wit-bindgen 的语言编写 AstroBox 插件。
编译完成后会生成一个 `.wasm` 文件，该文件即为插件的主要可执行体。

- 

插件加载
插件的 manifest 文件中需指定该 `.wasm` 文件为入口点。
当 AstroBox 启动插件时，会通过 wasmtime 加载并运行它。

- 

AOT 预编译与缓存
在加载过程中，AstroBox 首先会将插件从 WebAssembly 预编译（AOT）为 `.cwasm` 文件。
这一步可显著提升执行效率，并减少后续插件加载的启动时间。

- 

索引与版本管理
预编译完成后，系统会将插件的 wasm 哈希值 和 engine ID 记录到 `precompiled_index`（路径为 `<APP_DATA_DIR>/plugins/precompiled-index.json`）。
当检测到原始 `.wasm` 文件发生变化时，AstroBox 会自动触发重新编译，以确保插件始终保持最新的运行状态。

 Next 
 运行环境


  运行环境

# 运行环境
 
## 接口

AstroBox v2 的插件运行环境启用了对 wasi-p2 规范的支持，覆盖了当前所有已实现的稳定 WASI p2 特性，包括标准文件系统、日期时间以及系统级随机数（RNG）访问等功能。

此外，我们还集成了 `wasmtime-wasi-http` 模块，开发者可以在插件中使用 waki 库，以类似 `reqwest` 的方式发送 HTTP 网络请求。

同时，AstroBox 宿主（Host）自身也暴露了大量接口供插件调用；相应地，插件也需要实现部分接口以供宿主访问。
详细的接口定义与实现规范可参考 AstroBox API 文档。

------------------------------------------------------------

## 引擎配置

AstroBox v2 的插件执行引擎具备以下特性：

- 

执行模式

- Windows / macOS / Linux / Android：使用 Cranelift 引擎 以 JIT 模式 执行。

- iOS：使用 Pulley64 引擎 以 解释模式（Interpreter） 执行。

- 

资源与功能限制

- 每个插件的最大内存占用：128 MiB

- 启用特性：

- `memory_may_move`

- `wasm_component_model`

- `wasm_component_model_async`

- `async_support`

- 禁用特性：

- `wasm_memory64`

- 

文件系统安全策略

出于安全性考虑，标准文件系统（std fs）接口仅允许插件访问自身目录下的文件。
任何越界访问都会被立即拒绝并抛出错误。
若确需访问其他路径，可通过宿主提供的安全接口，在 用户明确授权 的前提下实现访问。

 Previous 
 工作原理 Next 
 Manifest 文件


  Manifest 文件

# Manifest 文件
 
## 什么是 Manifest 文件

Manifest 文件是 AstroBox 插件的核心描述文件，用于声明插件的基本信息、加载方式、运行环境要求以及所需权限。

AstroBox 在加载插件时，会首先解析 Manifest 文件，并根据其中的配置决定：

- 插件如何被识别与展示

- 插件的入口文件

- 插件可使用的 WASI / AstroBox API 能力

如果 Manifest 文件缺失或格式不正确，插件将无法被加载。

------------------------------------------------------------

## Manifest 结构定义（Rust）

```
`#[derive(Debug, Clone, Serialize, Deserialize)]pub struct PluginManifest { pub name: String, // 插件名称 pub icon: String, // 插件图标（路径） pub version: String, // 插件版本 pub description: String, // 插件简介 pub author: String, // 插件作者 pub website: String, // 插件网站（例如 GitHub 仓库地址） pub entry: String, // 插件入口 wasm 文件 pub wasi_version: u32, // WASI 接口版本 pub api_level: u32, // 插件 API 等级 pub permissions: Vec<String>, // 插件权限列表 #[serde(default)] pub additional_files: Vec<String>, // 插件附加文件列表}`
```

------------------------------------------------------------

## 字段说明

| 字段名 | 类型 | 是否必填 | 说明 | 
| name | String | 是 | 插件名称 | 
| icon | String | 是 | 插件图标路径（相对插件根目录） | 
| version | String | 是 | 插件版本号 | 
| description | String | 是 | 插件的简要说明 | 
| author | String | 是 | 插件作者 | 
| website | String | 是 | 插件主页或源码仓库地址 | 
| entry | String | 是 | 插件入口 WASM 文件 | 
| wasi_version | u32 | 是 | WASI 接口版本 | 
| api_level | u32 | 是 | AstroBox API 等级 | 
| permissions | Vec | 是 | 插件所需权限列表 | 
| additional_files | Vec | 否 | 插件运行所需的额外文件（填随插件包体上传的即可） | 

------------------------------------------------------------

## 字段详细说明

### name

插件的显示名称，建议简短清晰

### icon

插件图标路径，相对插件根目录，推荐使用 png，不建议使用 svg

### version

插件版本号，推荐使用语义化版本（如 1.0.0）

### description

插件的功能简介，用于帮助用户快速理解插件用途

### author

插件作者名称

### website

插件主页地址，例如 GitHub 仓库或项目官网

### entry

插件入口 WASM 文件路径，AstroBox 从此文件开始加载插件

### wasi_version

插件所依赖的 WASI 接口版本，用于运行时兼容判断，例如 2 对应 wasi-p2

### api_level

插件所使用的 AstroBox API 等级，向下兼容，在 此处 查看每个 API Level 所对应的 AstroBox 版本

### permissions

插件运行所需权限列表，未声明的权限将被拒绝访问

示例：

```
`"permissions": [ "device", "interconnect"]`
```

### additional_files（可选）

插件运行所需的附加文件列表，只需要填写随插件包体上传的文件，以便在下载插件时一起下载并进行大小计算。如果你只通过abp分发插件而不上架 AstroBox 官方插件源，则无需这么做

------------------------------------------------------------

## 完整示例

```
`{ "name": "Hello AstroBox", "icon": "icon.png", "version": "1.0.0", "description": "一个示例 AstroBox 插件", "author": "AstroBox Team", "website": "https://github.com/example/astrobox-plugin", "entry": "plugin.wasm", "wasi_version": 1, "api_level": 1, "permissions": [ "network" ], "additional_files": [ "extra_tools.rpk" ]}`
```

------------------------------------------------------------

## 注意事项

- Manifest 文件必须是合法 json

- 缺失必填字段将导致插件加载失败

- 权限声明应遵循最小权限原则
 Previous 
 运行环境 Next 
 API Level


  API Level

# API Level
 
## 什么是 API Level

API Level 用于描述插件所使用的 AstroBox 插件 API 等级。

插件在 `manifest.json` 中通过 `api_level` 字段声明自己所依赖的 API 等级：

```
`"api_level": 2`
```

AstroBox 在加载插件时，会根据当前运行环境所支持的 API Level 决定插件是否可以正常运行。

------------------------------------------------------------

## 兼容性原则

AstroBox 的 API Level 设计遵循以下原则：

### 1. 向下兼容

- 高版本 AstroBox 兼容低 API Level 的插件

- 插件只要声明的 `api_level` ≤ 当前 AstroBox 支持的最高 API Level，即可被加载

例如：

| AstroBox 版本 | 支持的最高 API Level | 
| 2.0.0 | 2 | 
| 2.1.0 | 3 | 

- 使用 `api_level = 2` 的插件可以运行在 AstroBox 2.0.x 上

- 使用 `api_level = 3` 的插件 无法 运行在 AstroBox 2.0.x 上

------------------------------------------------------------

### 2. 不保证向上兼容

- 插件 不能假定 更低版本的 AstroBox 支持更高的 API Level

- 如果插件声明的 API Level 高于当前 AstroBox 所支持的等级，插件将被拒绝加载

------------------------------------------------------------

## API Level 与 AstroBox 版本对应关系

下表列出了目前已定义的 API Level 及其对应的 AstroBox 版本：

| API Level | 最低 AstroBox 版本 | 说明 | 
| 2 | 2.0.0 | 初始版本 | 

> 

⚠️ 注意：AstroBox 只保证在最低版本及以上的版本中支持对应 API Level。

## API Level 升级说明

当 AstroBox 引入新的 API Level 时：

- 新增能力 只会在更高 API Level 中提供

- 旧 API Level 的行为保持不变

- WIT 文件中将加入新的接口，但插件作者需要 主动升级 api_level 才能使用新能力

升级 API Level 可能意味着：

- 需要调整插件代码

- 需要重新评估所需权限

- 可能影响插件可运行的 AstroBox 最低版本
 Previous 
 Manifest 文件 Next 
 WASI 版本


  WASI 版本

# WASI 版本
 
## 什么是 WASI

WASI（WebAssembly System Interface） 是一套为 WebAssembly 提供的系统接口标准，用于在不依赖特定宿主操作系统的情况下，安全地访问诸如文件系统、时间、随机数、网络等系统能力。

在 AstroBox 插件体系中：

- 插件以 WebAssembly（WASM） 形式运行

- WASI 负责定义插件与宿主环境之间的系统调用边界

- `wasi_version` 用于声明插件所依赖的 WASI 接口版本

------------------------------------------------------------

## 为什么需要 WASI Version

随着 WASI 标准的演进：

- 新的系统接口会被引入

- 旧接口可能被弃用或行为调整

- Rust / LLVM 等工具链的 WASI 支持也会随之变化

因此，AstroBox 需要通过 `wasi_version` 明确插件的运行时预期，以保证：

- 插件可以在合适的 WASI 运行环境中执行

- 不同 WASI 版本之间不会产生不可预期的行为差异

------------------------------------------------------------

## Manifest 中的表示

在插件的 `manifest.json` 中：

```
`"wasi_version": 2`
```

表示：

- 插件基于 WASI Preview 2（或等价稳定子集）构建

- AstroBox 将以对应版本的 WASI 运行环境加载该插件

> 

AstroBox 当前只支持 WASI Preview 2（或等价稳定子集）。

------------------------------------------------------------

## WASI Version 与 Rust Target 的关系

在 Rust 中，WASI 版本通常通过 编译目标（target） 体现。

常见的 WASI Rust Target 包括：

| Rust Target | 对应 WASI 版本 | 说明 | 
| `wasm32-wasip2` | WASI Preview 2 | 当前的稳定 WASI 组件模型 | 
| `wasm32-wasip3` | WASI Preview 3 | 新一代 WASI 组件模型，暂未稳定 | 

------------------------------------------------------------

## WASI 与 AstroBox API 的区别

需要特别区分：

| 项目 | 作用 | 
| WASI | 提供基础系统接口（文件、时间、IO 等） | 
| AstroBox API | 提供宿主平台能力（插件通信、UI、设备管理等） | 

- WASI 是 通用标准

- AstroBox API 是 平台私有能力

- `wasi_version` 与 `api_level` 彼此独立，但共同决定插件运行环境
 Previous 
 API Level Next 
 WIT 文件


  WIT 文件

# WIT 文件
 
## 什么是 WIT 文件

WIT（WebAssembly Interface Types） 文件用于描述 WebAssembly 组件的接口定义，包括：

- 函数签名

- 数据结构

- 模块 / 世界（world）定义

- 组件之间的调用契约

在 AstroBox 插件体系中，WIT 文件用于定义：

- 插件可以调用的 AstroBox 接口

- AstroBox 可以回调插件的能力边界

我们将 WIT 文件统一放置在 AstroBox-Plugin-WIT 仓库中，并在不同语言的插件模板中作为按需更新的git submodule存在。请确保在插件开发过程中始终使用最新的 WIT 文件。

------------------------------------------------------------

## 概念区分

- WIT 是 接口规范

- WASM 是 实现

- API Level 决定 哪些接口可用

插件通过 WIT 文件在编译期获得类型安全的接口绑定。

------------------------------------------------------------

## 更新策略

### 核心原则

> 

WIT 文件将持续更新以新增接口，但不会移除或破坏已有接口。

因此：

- ✅ 低 API Level 和高 API Level 的插件项目 都可以使用最新的 WIT 文件

- ❌ 无需停留在某个旧版本的 WIT

------------------------------------------------------------

## WIT 与 API Level 的关系

### 1. 编译期行为

- 使用最新 WIT 文件时：

- 所有接口在类型层面都是可见的

- 编译器允许你调用这些接口

### 2. 运行期行为

- 插件声明的 `api_level` 决定 运行时可用接口集合

- 低 API Level 插件：

- 无法使用高 API Level 新增的接口

- 即使代码能够成功编译，运行时仍会被拒绝或报错

------------------------------------------------------------

## 典型示例

假设：

- API Level 3 新增接口 `psys_host::dialog::show_dialog`

- 插件声明：

```
`"api_level": 2`
```

即使：

- 使用了最新的 WIT 文件

- 编译阶段没有错误

在运行时：

- 调用该接口将失败

- AstroBox 会根据 API Level 进行拦截

------------------------------------------------------------

## 常见问题

#### Q: 为什么允许“能编译但不能用”

这种设计的目的在于：

- 保证 单一、持续演进的 WIT 接口文件

- 避免 WIT 文件碎片化、版本锁死

- 将兼容性判断集中到 运行时 + Manifest

API Level 是能力声明，而不是类型系统的一部分。 Previous 
 WASI 版本 Next 
 语言选择


  语言选择

# 语言选择
 
## 语言支持

理论上，只要语言支持 wit-bindgen，就可以用于开发 AstroBox v2 插件。
然而，我们官方推荐并主要支持以下三种语言：

- Rust（强烈推荐）

- C#

- JavaScript

这些语言在生态、编译工具链和性能方面都已与 AstroBox 进行了充分适配和测试。

------------------------------------------------------------

## 其他可用语言

除上述语言外，以下语言也具备一定程度的兼容性，并可通过 wit-bindgen 开发插件（但暂未提供官方支持或测试保障）：

- Java

- Go

- C / C++

- MoonBit

- Zig

- Python

- Ruby

------------------------------------------------------------

> 

对于追求性能、安全性与长期维护的开发者，推荐优先选择 Rust。
其对 WASI 与组件模型（Component Model）的支持最为完整，并且拥有最优的执行效率与错误检测机制。
 Previous 
 WIT 文件 Next 
 Rust 上手教程


  Rust 插件开发上手教程

# Rust 插件开发上手教程
 
------------------------------------------------------------

> 

本文面向 熟悉 Rust 基础，但第一次接触 WASI / WIT / Component Model 的开发者。
目标不是“讲全”，而是 快速建立正确心智模型 + 跑通最小闭环。

如果你正在 Vibe Coding，请使用智力足够的模型（推荐 gpt-5.2-codex / gemini-3-pro），最好使用 Agent 类模型，并将此文喂给它。

------------------------------------------------------------

## 何意味？我们要写什么？

AstroBox 插件 = 一个 Rust `lib`，被编译成 `wasm32-wasip2` 的 WebAssembly Component

它绝对他妈的不是：

- 普通 CLI 程序

- WebAssembly for Web

- Tokio Server / 后端服务

而是：

> 

一个运行在 AstroBox 宿主里的“受控 Rust 组件”
通过 WIT 定义好的接口，和宿主进行 强类型、异步、安全 的交互。

------------------------------------------------------------

## 三相之力！

必须记住下面这三个概念喵！不然我会在喝完粉色魔爪后用小刀刀捅死你的喵！

### 一、Host / Plugin 不是“进程”，而是“组件边界”

- Host（AstroBox）：提供能力（UI、设备、系统、通信）

- Plugin（你写的 Rust）：实现逻辑、处理事件、调用 Host

二者之间 没有共享内存、没有直接 syscall：一切交互都必须写在 WIT 接口里

------------------------------------------------------------

### 二、WIT = 插件和宿主之间的“接口契约”

WIT 文件定义了：

- 可以调用哪些函数

- 数据结构长什么样

- 哪些是同步 / 哪些是异步

- 哪些事件会回调给插件

你 不需要自己写 ABI / FFI
`wit-bindgen` 会帮你把它们变成 Rust 代码。

详见 WIT 文件

------------------------------------------------------------

### 三、`future<T>` ≠ `async fn -> T`

在 WIT / Component Model 里：

- `future<T>` 是跨组件边界的异步承诺

- Rust 侧表现为：`FutureReader<T>`

这就是为什么你会看到：

```
`fn on_event(...) -> FutureReader<String>`
```

而不是：

```
`async fn on_event(...) -> String`
```

------------------------------------------------------------

## 环境准备

### 1. 安装 Rust

👉 https://www.rust-lang.org/learn/get-started

Windows 用户按提示装好 MSVC 即可。

------------------------------------------------------------

### 2. 安装 Python 3

我们在插件模板中准备了一个使用 Python 编写的脚本，方便你快速执行构建和打包等操作，你需要安装 Python 3 来使用它。

👉 https://www.python.org/downloads/

------------------------------------------------------------

### 3. 安装 AstroBox 插件目标

Terminal window
```
`rustup target add wasm32-wasip2`
```

> 

AstroBox V2 当前基于 WASI Preview 2

> 

未来会支持 wasi-p3，但旧插件无需修改即可继续工作

------------------------------------------------------------

## 创建你的第一个插件项目

### 克隆项目模板

Terminal window
```
`git clone --recurse-submodules https://github.com/AstralSightStudios/AstroBox-NG-Plugin-Template-Rust 
`cd AstroBox-NG-Plugin-Template-Rust`
```

------------------------------------------------------------

### 高雅人士观察项目结构

```
`.├── Cargo.toml├── scripts # 预置的构建辅助脚本├── src│ ├── lib.rs # 插件入口（你主要改的地方）│ └── logger.rs # tracing 日志初始化└── wit # （submodule）Host / Plugin 的 WIT 接口定义`
```

> 

⚠️ `wit/` 是 submodule，包含 wit 接口定义文件。详见 WIT 文件

> 

AstroBox 升级时，只会 新增接口，不会破坏旧接口

------------------------------------------------------------

### 尝尝编！

你不是一个肉编器（也许？），先执行一次实实在在的编译操作应该能加深你对项目结构的理解。

在上文中说过，我们在插件模板中准备了一个使用 Python 编写的脚本，方便你快速执行构建和打包等操作，这是它的用法：
Terminal window
```
`# Debug 构建到 dist 文件夹python scripts/build_dist.py
# Release 构建到 dist 文件夹并打 abp 包python scripts/build_dist.py --release --package`
```

非常简单，不是吗？

------------------------------------------------------------

## 初读 `lib.rs`

先别被一大坨宏和impl吓到，来看这三段关键代码：

### 一、`wit_bindgen::generate!`

```
`wit_bindgen::generate!({ path: "wit", world: "psys-world", generate_all,});`
```

它帮你做了三件事：

- 

把 Host 的 WIT 接口导入成 Rust 模块

```
`psys_host::dialog::show_dialog(...)`
```

- 

生成你必须实现的 Guest trait

```
`lifecycle::Guestevent::Guest`
```

- 

生成异步桥接所需的运行时代码
（`FutureReader` / `spawn` / `block_on`）

------------------------------------------------------------

### 二、插件生命周期：`on_load`

```
`impl lifecycle::Guest for MyPlugin { fn on_load() { logger::init(); tracing::info!("Hello AstroBox V2 Plugin!"); }}`
```

- 

插件 被加载时自动调用

- 

是同步函数

- 

非常适合做：

- 日志初始化

- 设备扫描

- register 各种事件

------------------------------------------------------------

### 三、事件入口：`on_event` / `on_ui_event`

```
`impl event::Guest for MyPlugin { fn on_event(...) -> FutureReader<String> { ... } fn on_ui_event(...) -> FutureReader<String> { ... }}`
```

这是插件 90% 逻辑调用的入口

------------------------------------------------------------

## 什么是他妈的 `FutureReader`？

### 为什么不能直接发动锈术释放 `async fn`？

除了我不喜欢你，我什么都没做错。知名博主 LexBurner 曾在不经意间被吓一跳释放忍术🥷，在使用 Rust 编写 AstroBox 插件时，你肯定也会忍不住被吓一跳释放锈术，把 `async fn` 扔给 `FutureReader`。

好吧上面是在玩梗，但你的确不能这么做

因为：

> 

宿主和插件可能不在同一个 executor / runtime / 线程模型里

所以 WIT 定义的是：

```
`on-event: func(...) -> future<string>`
```

------------------------------------------------------------

### True, dude

                              ———XQC

所以实际上这才是正确写法：

```
`let (writer, reader) = wit_future::new::<String>(|| "".to_string());
wit_bindgen::spawn(async move { // 这里可以 await host 接口 writer.write("result".to_string()).await.unwrap();});
reader`
```

你可以把它理解为：

> 

oneshot channel + promise

- `reader`：马上还给宿主（“我以后会给你结果”）

- `writer`：某个时刻把结果填回去

------------------------------------------------------------

### `spawn` 是谁的？

```
`wit_bindgen::spawn(async { ... });`
```

杂鱼杂鱼，这才不是什么 tokio 呢～

这是 Component Model 自带的最小 async runtime——足够用，且 WASI 下最稳定

------------------------------------------------------------

## 在同步函数里调用异步 Host 接口

`on_load` 是同步的，但 Host 接口几乎都是 `future<T>`。

如果你熟悉Rust开发，你肯定马上要发动`tokio::block_on`之力了——没错，但hold on，把它换成`wit_bindgen::block_on`才是真没错：

```
`wit_bindgen::block_on(async { let result = psys_host::dialog::show_dialog(...).await;});`
```

原则：

- ✅ 生命周期函数里可以 `block_on`

- ⚠️ 但是请无论如何都不要把 `dialog` 之类需要等待用户操作的异步操作在 `on_load` 中 `block_on`！

- ❌ 事件回调里不要阻塞，直接返回 `FutureReader`

------------------------------------------------------------

## 第一个完整调用闭环：弹一个 Dialog

```
`psys_host::dialog::show_dialog( DialogType::Alert, DialogStyle::System, &DialogInfo { title: "Plugin Alert".into(), content: "插件正在运行".into(), buttons: vec![DialogButton { id: "ok".into(), primary: true, content: "OK".into(), }], },).await;`
```

你可以从中总结出 WIT → Rust 的映射规律：

| WIT | Rust | 
| `enum` | Rust `enum` | 
| `record` | Rust `struct` | 
| `list<T>` | `Vec<T>` | 
| `string` | `String` | 
| `future<T>` | `.await` / `FutureReader<T>` | 

------------------------------------------------------------

## Register → Event：事件是“订阅制”的

正确流程永远是：

- 

`on_load`

- `register_xxx(...)`

- 

`on_event`

- 收到对应事件

- 执行业务逻辑

------------------------------------------------------------

## UI 接口：声明式，而不是模板式或 DOM

逃离使用命令式 UI 的 Qt 和尤雨溪统治的 `<template>` 帝国，让我们来拥抱一些真正现代、真正 Next-Gen 的东西——声明式 UI。人人都喜欢 React 和 SwiftUI，除了那些仍在玩弄 WPF 或使用 Vanilla HTML 编写上世纪级页面的老登。

`ui::element` 是一个 链式 Builder API

```
`let btn = ui::element::new( ElementType::BUTTON, Some("Click me".into())).on(event::Event::CLICK, "btn-click");
ui::render(btn);`
```

- 没有 HTML

- 没有 JS

- 没有 CSS

去他妈的 XSS 注入。

------------------------------------------------------------

## Crates.io 生态兼容：你不需要重新发明轮子

虽然前面还没提到，但也许你已经发现，模板里的 `logger.rs` 做了三件事：

- stdout 打印（带 `[Plugin]` 前缀）

- 文件滚动日志（`logs/app.log`）

并且，它直接使用了目前 Rust 桌面应用采用的主流方案 tracing 库。它并没有为 WebAssembly 特别开发，但由于我们使用了 WASI，大部分 std 操作都得以实现，这些来自 Rust 现存生态的第三方库也能被直接使用。

因此：

> 

WASI ≠ 只能写玩具代码

> 

绝大多数纯 Rust 库 可以直接用

------------------------------------------------------------

## Tokio？能用，但不推荐

- WASM / WASI 下 tokio 是 子集支持

- `full` feature 会展现黑曼巴精神直接坠机

- 定时器、IO 行为大概率没适配 wasi，也不让你过编译

So，优先使用：

- `wit_bindgen::spawn`

- `FutureReader`

- Host 提供的能力

------------------------------------------------------------

## 到这里，你已经能做什么了？

你现在已经可以：

- 写一个可加载的 AstroBox 插件

- 调用 Host UI / Device / Transport 接口

- 注册并接收事件

- 正确处理跨组件异步

- 使用标准 Rust 日志与库

接下来只剩两件事：

- 业务逻辑

- 设计好你的插件 UX

发挥你的创造力，我们迫不及待地想看看你能在这个充满可能性的平台上做些什么！

------------------------------------------------------------
 Previous 
 语言选择 Next 
 Device


  Host API: Register

# Register 接口
 

用于注册插件能力（卡片、Provider、Transport 接收、Interconnect 接收等）。

## 接口定义

```
`interface register { record transport-recv-filer { xiaomi-vela-v5-channel-id: u32, xiaomi-vela-v5-protobuf-typeid: u32, }
 enum provider-type { URL, CUSTOM }
 enum card-type { ELEMENT, TEXT }
 register-transport-recv: func(addr: string, filter: transport-recv-filer) -> future<result>; register-interconnect-recv: func(addr: string, pkg-name: string) -> future<result>; register-deeplink-action: func() -> future<result>; register-provider: func(name: string, provider-type: provider-type) -> future<result>; register-card: func(card-type: card-type, id: string, name: string) -> future<result>;}`
```

## 类型

### transport-recv-filer

- `xiaomi-vela-v5-channel-id`：Vela V5 通道 ID。

- `xiaomi-vela-v5-protobuf-typeid`：Vela V5 Protobuf 类型 ID。

### provider-type

- `URL`：URL 型 Provider。

- `CUSTOM`：自定义 Provider。

### card-type

- `ELEMENT`：元素卡片，与 `ui::render-to-element-card` 配合。

- `TEXT`：纯文本卡片，与 `ui::render-to-text-card` 配合。

## 函数

### register-transport-recv

根据过滤条件订阅设备端发送来的消息，将以 `transport-packet` 为事件类型调用 Plugin 端的 `on_event` 函数，支持订阅多个条件或设备

- 参数：

- `addr: string` 设备地址。

- `filter: transport-recv-filer` 接收过滤条件。

- 返回：`future<result>`。

### register-interconnect-recv

根据过滤条件订阅设备端快应用发送来的 Interconnect 消息，将以 `interconnect-message` 为事件类型调用 Plugin 端的 `on_event` 函数，支持订阅多个包名或设备

- 参数：

- `addr: string` 设备地址。

- `pkg-name: string` 快应用包名。

- 返回：`future<result>`。

### register-deeplink-action

订阅插件 DeepLink 事件，订阅后在浏览器中打开 `astrobox://open?source=openPlugin&pluginName=<plugin_name>&data=<plugin_data>` 并拉起 AstroBox 后，将以 `deeplink-action` 为事件类型调用 Plugin 端的 `on_event` 函数，`data` 中的字符串内容将作为 `event-payload`，只支持订阅一次

- 返回：`future<result>`。

- 说明：将插件注册为 Deeplink 行为处理方。

### register-provider

注册一个社区源。类型为 URL 时，仅支持 GitHub 上的仓库，结构必须与 AstroBox 官方源一致，并提供指向 `index_v2.csv` 的URL。类型为 Custom 时，将在执行社区源操作（如拉取资源列表、获取资源详情）时以 `provider-action` 为事件类型调用 Plugin 端的 `on_event` 函数，并在 `event-payload` 中提供操作类型与操作参数，你需要返回对应的数据。支持注册多个社区源

- 参数：

- `name: string` Provider 名称。

- `provider-type: provider-type` Provider 类型。

- 返回：`future<result>`。

### register-card

注册一个设备详情页卡片，类型为 Text 时只支持渲染纯文本，类型为 Element 时支持渲染 UI Element。支持注册多个详情页卡片

- 参数：

- `card-type: card-type` 卡片类型。

- `id: string` 卡片标识。

- `name: string` 卡片展示名称。

- 返回：`future<result>`。

## Rust 示例

```
`use crate::astrobox::psys_host;
pub async fn register_card() { psys_host::register::register_card( psys_host::register::CardType::Text, "demo-card", "示例卡片", ) .await .unwrap();}`
```
 Previous 
 Queue Next 
 ThirdPartyApp


  Host API: Dialog

# Dialog 接口
 

用于在宿主 UI 中弹出系统级或 Web 风格对话框，并提供文件选择能力。

## 接口定义

```
`interface dialog { enum dialog-type { ALERT, INPUT }
 enum dialog-style { WEBSITE, SYSTEM }
 record dialog-button { id: string, primary: bool, content: string }
 record dialog-info { title: string, content: string, buttons: list<dialog-button> }
 record dialog-result { clicked-btn-id: string, input-result: string }
 record pick-config { read: bool, copy-to: option<string>, }
 record filter-config { multiple: bool, extensions: list<string>, default-directory: string, default-file-name: string, }
 record pick-result { name: string, data: list<u8>, }
 show-dialog: func(dialog-type: dialog-type, style: dialog-style, info: dialog-info) -> future<dialog-result>;
 pick-file: func(config: pick-config, filter: filter-config) -> future<pick-result>;}`
```

## 类型

### dialog-type

- `ALERT`：提示/确认框。

- `INPUT`：带输入框的对话框。

### dialog-style

- `WEBSITE`：Web 风格。

- `SYSTEM`：系统原生风格。

### dialog-button

- `id`：按钮标识，回传到结果。

- `primary`：是否为主按钮。

- `content`：按钮文字。

### dialog-info

- `title`：标题。

- `content`：内容正文。

- `buttons`：按钮列表。

### dialog-result

- `clicked-btn-id`：点击按钮的 `id`。

- `input-result`：输入内容，仅 `INPUT` 类型会有值。

### pick-config

- `read`：是否由宿主读取文件内容。

- `copy-to`：可选复制目标路径。

### filter-config

- `multiple`：是否允许多选。

- `extensions`：可选扩展名过滤。

- `default-directory`：默认打开目录。

- `default-file-name`：默认文件名。

### pick-result

- `name`：文件名。

- `data`：文件内容字节，是否返回取决于 `pick-config.read`。

## 函数

### show-dialog

- 参数：

- `dialog-type: dialog-type` 对话框类型。

- `style: dialog-style` UI 风格。

- `info: dialog-info` 展示信息。

- 返回：`future<dialog-result>`。

### pick-file

- 参数：

- `config: pick-config` 读取与复制配置。

- `filter: filter-config` 过滤与默认路径配置。

- 返回：`future<pick-result>`。

## Rust 示例

```
`use crate::{ astrobox::psys_host::{ self, dialog::{DialogButton, DialogInfo, FilterConfig, PickConfig}, },};
pub async fn show_alert() { let ret = psys_host::dialog::show_dialog( psys_host::dialog::DialogType::Alert, psys_host::dialog::DialogStyle::System, &DialogInfo { title: "插件 Alert".to_string(), content: "该插件正在 AstroBox V2 的 WASI 插件系统中运行".to_string(), buttons: vec![ DialogButton { id: "ok".to_string(), primary: true, content: "好样的！".to_string(), }, DialogButton { id: "cancel".to_string(), primary: false, content: "算了".to_string(), }, ], }, ) .await;
 tracing::info!("clicked_btn_id={}", ret.clicked_btn_id); tracing::info!("input_result={}", ret.input_result);}
pub async fn pick_file() { let ret = psys_host::dialog::pick_file( &PickConfig { read: true, copy_to: None, }, &FilterConfig { multiple: false, extensions: vec!["png".into(), "jpg".into()], default_directory: "".into(), default_file_name: "".into(), }, ) .await;
 tracing::info!("picked_name={}", ret.name); tracing::info!("picked_bytes={}", ret.data.len());}`
```

## 注意事项

- `show_dialog` 必须在宿主事件循环存活期间调用。

- 在 `on_load` 中调用时，应使用异步 spawn。
 Previous 
 Device Next 
 Event


  Host API: OS

# OS 接口
 

提供宿主系统的基础信息查询能力，全部为异步调用。

## 接口定义

```
`interface os { arch: func() -> future<string>; hostname: func() -> future<string>; locale: func() -> future<string>; platform: func() -> future<string>; version: func() -> future<string>; astrobox-language: func() -> future<string>;}`
```

## 函数

### arch

- 返回：`future<string>`，CPU 架构字符串。

### hostname

- 返回：`future<string>`，宿主设备名称。

### locale

- 返回：`future<string>`，系统 locale 字符串。

### platform

- 返回：`future<string>`，宿主平台标识。

### version

- 返回：`future<string>`，宿主系统版本号。

### astrobox-language

- 返回：`future<string>`，AstroBox 语言设置。

## Rust 示例

```
`use crate::astrobox::psys_host;
pub async fn print_os_info() { let arch = psys_host::os::arch().await; let platform = psys_host::os::platform().await; let version = psys_host::os::version().await; let locale = psys_host::os::locale().await; let hostname = psys_host::os::hostname().await; let language = psys_host::os::astrobox_language().await;
 tracing::info!("arch={}", arch); tracing::info!("platform={}", platform); tracing::info!("version={}", version); tracing::info!("locale={}", locale); tracing::info!("hostname={}", hostname); tracing::info!("astrobox_language={}", language);}`
```
 Previous 
 Interconnect Next 
 Queue


  Host API: UI

# UI 接口
 

用于构建宿主 UI 卡片。该接口提供链式 Builder 风格的元素描述，并由宿主渲染。

## 接口定义

```
`interface ui { enum element-type { BUTTON, IMAGE, VIDEO, AUDIO, SVG, DIV, SPAN, P, }
 resource element{ constructor(element-type:element-type,content:option<string>); content:func(content:option<string>) -> element;
 flex:func() -> element;
 margin:func(margin:u32) -> element; margin-top:func(margin:u32) -> element; margin-bottom:func(margin:u32) -> element; margin-left:func(margin:u32) -> element; margin-right:func(margin:u32) -> element;
 padding:func(padding:u32) -> element; padding-top:func(padding:u32) -> element; padding-bottom:func(padding:u32) -> element; padding-left:func(padding:u32) -> element; padding-right:func(padding:u32) -> element;
 align-center:func() -> element; align-end:func() -> element; align-start:func() -> element;
 justify-center:func() -> element; justify-start:func() -> element; justify-end:func() -> element;
 bg:func(color:string) -> element; text-color:func(color:string) -> element;
 size:func(size:u32) -> element; width:func(width:u32) -> element; height:func(height:u32) -> element; radius:func(radius:u32) -> element; border:func(width:u32,color:string) -> element;
 relative:func() -> element; absolute:func() -> element;
 top:func(position:u32) -> element; bottom:func(position:u32) -> element; left:func(position:u32) -> element; right:func(position:u32) -> element;
 opacity:func(opacity:f32) -> element; transition:func(transition:string) -> element;
 z-index:func(z:s32) -> element; disabled:func() -> element;
 child:func(child:element) -> element;
 on:func(event:event,id:string) -> element; }
 enum event { CLICK, HOVER, CHANGE, POINTER-DOWN, POINTER-UP, POINTER-MOVE, }
 render:func(el:element); render-to-element-card:func(id:string,el:element); render-to-text-card:func(id:string,text:string);}`
```

## 核心概念

- `element` 资源是链式 Builder，方法调用返回新的 `element`。

- `element-type` 决定元素的语义与渲染方式。

- `event` 用于绑定交互事件。

## 元素类型

- `BUTTON` / `IMAGE` / `VIDEO` / `AUDIO` / `SVG` / `DIV` / `SPAN` / `P`

## 事件类型

- `CLICK` / `HOVER` / `CHANGE` / `POINTER-DOWN` / `POINTER-UP` / `POINTER-MOVE`

## 链式构建方法

### 创建与内容

- `element::new(element-type, content)`：创建元素，可选内容。

- `content(content)`：更新元素内容。

### 布局与对齐

- `flex()`：使用 Flex 布局。

- `align-start()` / `align-center()` / `align-end()`：交叉轴对齐。

- `justify-start()` / `justify-center()` / `justify-end()`：主轴对齐。

### 间距

- `margin(margin)` / `margin-top(margin)` / `margin-bottom(margin)` / `margin-left(margin)` / `margin-right(margin)`

- `padding(padding)` / `padding-top(padding)` / `padding-bottom(padding)` / `padding-left(padding)` / `padding-right(padding)`

### 视觉样式

- `bg(color)`：背景色。

- `text-color(color)`：文字颜色。

- `radius(radius)`：圆角。

- `border(width, color)`：边框。

### 尺寸

- `size(size)` / `width(width)` / `height(height)`

### 位置与层级

- `relative()` / `absolute()`

- `top(position)` / `bottom(position)` / `left(position)` / `right(position)`

- `z-index(z)`

### 状态与动效

- `opacity(opacity)`

- `transition(transition)`

- `disabled()`

### 结构与事件

- `child(child)`：追加子元素。

- `on(event, id)`：绑定事件并指定标识。

## 渲染函数

- `render(el)`：渲染到插件功能页。

- `render-to-element-card(id, el)`：渲染到指定元素卡片，`id` 需先通过 `register::register_card` 注册。

- `render-to-text-card(id, text)`：渲染纯文本卡片。

## Rust 示例

```
`use crate::astrobox::psys_host::ui::{self, ElementType};
pub fn render_element_card() { let button = ui::element::new(ElementType::BUTTON, Some("开始同步".into())) .padding(10) .radius(8) .bg("#2B5BE8".into()) .text_color("#FFFFFF".into());
 let root = ui::element::new(ElementType::DIV, None) .flex() .justify_center() .align_center() .padding(16) .child(button);
 ui::render_to_element_card("main-card", root);}`
```
 Previous 
 Transport


  Host API: Device

# Device 接口
 

用于获取与管理设备列表。

## 接口定义

```
`interface device { record device-info { name: string, addr: string }
 get-device-list: func() -> future<list<device-info>>; get-connected-device-list: func() -> future<list<device-info>>; disconnect-device: func(addr: string) -> future<result>;}`
```

## 类型

### device-info

- `name`：设备名称。

- `addr`：设备地址，作为其他接口的 `device-addr` 入参。

## 函数

### get-device-list

- 返回：`future<list<device-info>>`，宿主可识别的全部设备。

### get-connected-device-list

- 返回：`future<list<device-info>>`，当前已连接设备。

### disconnect-device

- 参数：`addr: string` 设备地址。

- 返回：`future<result>`。

## Rust 示例

```
`use crate::astrobox::psys_host;
pub async fn list_devices() { let devices = psys_host::device::get_device_list().await; for d in devices { tracing::info!("{} ({})", d.name, d.addr); }}`
```
 Previous 
 Rust 上手教程 Next 
 Dialog


  Host API: Transport

# Transport 接口
 

用于与穿戴设备进行数据传输与协议转换。

## 接口定义

```
`interface transport { enum protocol { XIAOMI-VELA-V5-PROTOBUF, }
 send: func(device-addr: string, data: list<u8>) -> future; request: func(device-addr: string, data: list<u8>) -> future<result<list<u8>>>; to-json: func(protocol: protocol, data: list<u8>) -> string; from-json: func(protocol: protocol, data: string) -> result<list<u8>>;}`
```

## 类型

### protocol

- `XIAOMI-VELA-V5-PROTOBUF`：基于 Xiaomi Vela 5 的 Protobuf 传输协议

## 函数

### send

- 参数：

- `device-addr: string` 设备地址。

- `data: list<u8>` 待发送的二进制数据。

- 返回：`future<()>`。

- 说明：仅发送数据，不等待响应。

### request

- 参数：

- `device-addr: string` 设备地址。

- `data: list<u8>` 待发送的二进制数据。

- 返回：`future<result<list<u8>>>`，成功返回响应数据。

- 说明：请求-响应模式。

### to-json

- 参数：

- `protocol: protocol` 协议类型。

- `data: list<u8>` 二进制数据。

- 返回：`string`。

- 说明：将协议数据转换为 JSON 字符串，便于日志与调试。

### from-json

- 参数：

- `protocol: protocol` 协议类型。

- `data: string` JSON 字符串。

- 返回：`result<list<u8>>`。

- 说明：将 JSON 反序列化为协议二进制数据。不推荐使用。

## Rust 示例

```
`use crate::astrobox::psys_host;
pub async fn send_packet(addr: &str, data: Vec<u8>) { psys_host::transport::send(addr, &data).await;}
pub async fn request_packet(addr: &str, data: Vec<u8>) -> Option<Vec<u8>> { match psys_host::transport::request(addr, &data).await { Ok(resp) => Some(resp), Err(_) => None, }}`
```
 Previous 
 ThirdPartyApp Next 
 UI


  Host API: Interconnect

# Interconnect 接口
 

与穿戴设备上的快应用通信。

## 接口定义

```
`interface interconnect { send-qaic-message: func(device-addr: string, pkg-name: string, data: string) -> future<result>;}`
```

## 函数

### send-qaic-message

- 参数：

- `device-addr: string` 设备地址。

- `pkg-name: string` 快应用包名。

- `data: string` 发送数据字符串。

- 返回：`future<result>`。

## Rust 示例

```
`use crate::astrobox::psys_host;
pub async fn send_message(addr: &str) { psys_host::interconnect::send_qaic_message( addr, "com.example.app", "{\"hello\":1}", ) .await .unwrap();}`
```
 Previous 
 Event Next 
 OS


  Host API: Queue

# Queue 接口
 

向 AstroBox 资源队列添加任务。

## 接口定义

```
`interface queue { enum resource-type { quickapp, watchface, firmware }
 add-resource-to-queue: func(res-type: resource-type, file-path: string);}`
```

## 类型

### resource-type

- `quickapp`：快应用资源。

- `watchface`：表盘资源。

- `firmware`：固件资源。

## 函数

### add-resource-to-queue

- 参数：

- `res-type: resource-type` 资源类型。

- `file-path: string` 本地文件路径。

- 返回：`()`。

## Rust 示例

```
`use crate::astrobox::psys_host;
pub fn add_firmware(path: &str) { psys_host::queue::add_resource_to_queue( psys_host::queue::ResourceType::Firmware, path, );}`
```
 Previous 
 OS Next 
 Register


  Host API: Event

# Event 接口
 

向其它插件发送事件。

## 接口定义

```
`interface event { send-event: func(event-name: string, payload: string);}`
```

## 函数

### send-event

- 参数：

- `event-name: string` 事件名。

- `payload: string` 事件载荷字符串（通常为 JSON）。

- 返回：`()`。

## Rust 示例

```
`use crate::astrobox::psys_host;
pub fn send_event() { psys_host::event::send_event("plugin-ready", "{\"ok\":true}");}`
```
 Previous 
 Dialog Next 
 Interconnect


  Host API: ThirdPartyApp

# ThirdpartyApp 接口
 

管理和启动第三方应用。

## 接口定义

```
`interface thirdpartyapp { record app-info { package-name: string, fingerprint: list<u32>, version-code: u32, can-remove: bool, app-name: string }
 launch-qa: func(addr: string, app-info: app-info, page-name: string) -> future<result>; get-thirdparty-app-list: func(addr: string) -> future<result<list<app-info>>>;}`
```

## 类型

### app-info

- `package-name`：包名。

- `fingerprint`：签名指纹。

- `version-code`：版本号。

- `can-remove`：是否允许卸载。

- `app-name`：应用名称。

## 函数

### launch-qa

- 参数：

- `addr: string` 设备地址。

- `app-info: app-info` 目标应用信息。

- `page-name: string` 目标页面。

- 返回：`future<result>`。

### get-thirdparty-app-list

- 参数：`addr: string` 设备地址。

- 返回：`future<result<list<app-info>>>`。

## Rust 示例

```
`use crate::astrobox::psys_host;
pub async fn list_apps(addr: &str) { let ret = psys_host::thirdpartyapp::get_thirdparty_app_list(addr).await; if let Ok(apps) = ret { for app in apps { tracing::info!("{} ({})", app.app_name, app.package_name); } }}`
```
 Previous 
 Register Next 
 Transport

  实战开发注意事项（高频坑位）

# 开发注意事项（自身经历补充）

> 本节补充“文档定义之外”的实战注意点，重点覆盖插件开发中最容易导致“看起来没报错、实际不可用”的问题。
> 建议将本节作为上线前 checklist 使用。

------------------------------------------------------------

## 注意事项 1：UI 事件回调必须可观测，先确认“事件到了没有”

### 常见误区

- 误以为“按钮渲染出来了”就一定能触发 `on-ui-event`。
- 只在业务分支里打印日志，导致事件没进来时完全无提示。

### 正确做法

1. 在 `on-ui-event` 的最外层先记录日志（包含 `event` / `event-id` / `event-payload`）。
2. 给 UI 页面保留一个简易“最近事件”调试区域（发布前可隐藏）。
3. 点击类事件至少兼容 `CLICK` 与 `POINTER-UP`。

### 示例（推荐）

```
`tracing::info!("on_ui_event: event={:?}, id={}, payload={}", event, event_id, event_payload);

match event {
    ui::Event::Click | ui::Event::PointerUp => handle_click(event_id),
    ui::Event::Change | ui::Event::Input => handle_change(event_id, event_payload),
    _ => {}
}`
```

### 反例（不推荐）

```
`if event_id == "sync_push" {
    tracing::info!("start push");
    ...
}`
```

如果事件根本没进来，上述写法会表现为“按钮完全没反应”且无任何线索。

------------------------------------------------------------

## 注意事项 2：以本地 WIT 为准，不要假设网页文档方法一定可用

### 常见误区

- 直接照抄旧示例 API（如不存在的方法），导致编译失败或行为异常。

### 正确做法

- 以项目内 `wit/deps/*.wit` 为接口真相源。
- 每次升级模板或子模块后，先核对 UI builder 可用方法集合再写样式链。

### 举例说明

- 某些环境/版本中，`justify-content` / `text-align` / `flex-grow` 这类方法或枚举可能并不存在。
- 直接调用会报“method not found”或类型解析错误。

### 建议

- 先用最小可运行样式（`flex`、`justify-start/center`、`padding`、`margin`）打通功能，再逐步增强视觉。

------------------------------------------------------------

## 注意事项 3：Interconnect 不要写死包名，先动态发现目标快应用

### 常见误区

- 代码写死 `pkg-name = com.xxx.yyy`，在你设备能用，换一台手表就报：
  - `Quick app xxx not found`

### 正确做法

1. 使用 `thirdpartyapp.get-thirdparty-app-list(addr)` 获取手表应用列表。
2. 优先按应用名关键字匹配（例如“Var课程表”）。
3. 匹配成功后再执行：
   - `register-interconnect-recv(addr, pkg)`
   - `send-qaic-message(addr, pkg, data)`

### 示例（推荐）

```
`let apps = thirdpartyapp::get_thirdparty_app_list(addr).await?;
let pkg = apps.iter()
    .find(|a| a.app_name.contains("Var课程表"))
    .map(|a| a.package_name.clone())
    .ok_or("target app not found")?;

register::register_interconnect_recv(addr, &pkg).await?;
interconnect::send_qaic_message(addr, &pkg, payload).await?;`
```

### 反例（不推荐）

```
`interconnect::send_qaic_message(addr, "com.azuma.syclass", payload).await?;`
```

------------------------------------------------------------

## 注意事项 4：回调解析要“宽松兼容”，不要只认单一 JSON 结构

### 常见误区

- 只接受一种固定结构（如必须有 `type=timetableData` 且 `classes` 存在）。
- 手环实际回包多一层 `payloadText` / `data` 包裹就解析失败。

### 正确做法

- 递归解包常见字段：`data`、`payload`、`payloadText`、`eventPayload`、`message`、`content`、`body`、`result`。
- 字段名做兼容别名：
  - `name` / `courseName`
  - `start` / `startSection`
  - `end` / `endSection`
  - `weekType` / `week_type`

### 示例思路

```
`for key in ["data","payload","payloadText","eventPayload","message","content","body","result"] {
    push(value[key]);
}`
```

### 诊断建议

- 解析失败时不要只报“格式错误”，应附带回包摘要（截断前 200 字符），便于快速适配。

------------------------------------------------------------

## 注意事项 5：UI 事件与业务状态更新要同一节拍

### 常见误区

- 业务函数执行了，但没刷新 UI；
- 或 UI 先刷新后改状态，导致用户看到旧状态。

### 正确做法

- 统一流程：
  1. 更新状态
  2. 调用刷新（`render` / `refresh_main_ui`）
- 对于耗时操作（获取/推送）：
  - 先显示“正在执行”状态
  - 再发请求
  - 最后更新成功/失败状态并刷新

------------------------------------------------------------

## 注意事项 6：上线前最小回归清单（建议固定执行）

### 功能回归

- 添加课程（字段校验）
- 手动粘贴 JSON 导入
- 从手环获取课程
- 编辑课程并保存
- 删除课程
- 推送到手环

### 连接回归

- 手表已连接但目标快应用未安装
- 手表已连接且目标快应用已安装
- 重新连接设备后再次获取/推送

### 诊断回归

- 点击按钮时是否能看到事件日志/调试信息
- 回调失败时是否能看到 payload 摘要

------------------------------------------------------------

## 注意事项 7：推荐的排错顺序（节省时间）

1. 先看是否收到 `on-ui-event`
2. 再看是否匹配到正确 `event-id`
3. 再看是否找到目标快应用包名
4. 再看发送是否成功
5. 最后看回调解析是否命中

这个顺序可以避免在“包名错误/事件没进来”时误判为“解析逻辑有问题”。
