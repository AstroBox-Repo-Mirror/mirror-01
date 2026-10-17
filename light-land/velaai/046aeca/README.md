<p align="center">
  <img src="src/common/logo.png" />
</p>
<h1 align="center">Vela AI</h1>
<p align="center">
    一款专为 Vela 系统设计的 AI 对话快应用，接入 DeepSeek API，让你在手腕上随时随地进行智能对话
    本项目基于小鱼yuzifu 的 daymatter-https://github.com/sf-yuzifu/daymatter
</p>

## 📱 兼容设备

本项目仅适用于以下设备（属 Watch 大类的 Band 分类，Band Pro 系列，rect 屏）：

- Xiaomi Watch 9 Pro
- Xiaomi Watch 10 Pro

> 其他设备（圆形屏 / 非 Band Pro 系列）未经适配，可能无法正常使用。

## ✨ 功能特性

- **AI 对话** - 接入 DeepSeek Chat Completions API，支持多模型切换与深度思考模式
- **对话持久化** - 每份对话独立存储，上下文互不串扰，可随时从历史记录继续
- **长对话总结** - 按回合数自动滚动摘要，用「摘要 + 新消息」发送，大幅节省 token
- **自动清理** - 超过 30 回合且 14 天未聊的对话自动删除，历史列表红色倒计时预告
- **AI 标题生成** - 首轮对话后自动为对话生成简短标题（≤20 字节）
- **Markdown 去除** - 回复自动剥离 Markdown 语法，适配小屏纯文本展示
- **参数可调** - thinking / temperature / max_tokens / system 提示词
- **模型管理** - 自动拉取最新模型列表，滚动选择器一键切换
- **余额查询** - 实时显示 DeepSeek 账户余额（¥ / $）
- **错误提示** - API key 错误 / 网络未连接以红色气泡友好展示，AI 可见
- **便捷输入** - 集成[喵喵输入法组件](https://github.com/NEORUAA/Vela_input_method)，支持拼音候选、光标移动、字符窗口渲染
- **长按 + 振动** - 危险操作需长按并振动确认，防止误触

## 📱 预览

<p align="center">
    <!-- TODO: 替换为你的设备实拍截图 -->
</p>

## 📦 安装与使用

### 环境要求
- Node.js >= 8.10

### 安装依赖
```bash
yarn
```

### 开发调试
```bash
npm run start
# 或
yarn start
```

### 构建发布
```bash
npm run release
# 或
yarn release
```

### 使用流程
1. 构建部署到 9 Pro / 10 Pro 设备
2. 进入「设置 → API Key」填写 DeepSeek API Key
3. 在「设置 → 模型」中选择模型
4. 首页输入文本开始对话

> 设备需联网；未配置 API Key 时仍可浏览历史记录。

## 📁 项目结构

```
velaai/
├── src/                    # 源代码目录
│   ├── common/            # 公共资源与核心逻辑
│   │   ├── api.js         # DeepSeek API 封装（聊天 / 余额 / 模型 / 错误分类）
│   │   ├── settings.js    # 配置持久化
│   │   ├── store.js       # 对话文件存储与自动清理
│   │   └── welcome.js     # 欢迎语
│   ├── components/        # 组件
│   │   └── InputMethod/   # 输入法组件
│   ├── pages/             # 页面
│   │   ├── chat/          # 对话主界面（入口）
│   │   ├── history/       # 历史记录
│   │   ├── setting/       # 基础设置
│   │   ├── model/         # 模型选择
│   │   ├── advanced/      # 高级设置
│   │   ├── textinput/     # 文本输入页
│   │   └── about/         # 关于
│   ├── app.ux             # 应用入口
│   ├── manifest.json      # 应用配置
│   └── config-watch.json  # 手表配置
├── sign/                  # 签名证书
├── package.json           # 项目配置
└── README.md              # 项目说明
```

## 👁️ 了解更多

你可以通过小米快应用的[官方文档](https://iot.mi.com/vela/quickapp)熟悉和了解快应用开发。

**注意**：请遵守相关法律法规，合理使用本项目。AI 生成内容仅供参考，请勿完全依赖。

## 🤝 贡献

欢迎提交 Issue 和 Pull Request 来帮助改进这个项目。

## 📄 许可证

本项目采用 [AGPL-3.0](LICENSE) 许可证开源。

## 🙏 致谢

- [小鱼yuzifu](https://github.com/sf-yuzifu) - 原始项目作者
- [无源流沙](https://www.bandbbs.cn/threads/14584/) - UI 界面风格
- [NEORUAA](https://github.com/NEORUAA/) - 输入法组件
