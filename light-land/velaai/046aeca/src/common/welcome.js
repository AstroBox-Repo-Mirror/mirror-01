// 欢迎语：模板中 {name} 替换为用户名，新建对话时随机选一条
const TEMPLATES = [
  "{name}，你好",
  "{name}，今天想聊点什么",
  "{name}，聊什么由你决定",
  "嘿，{name}，我在，你说",
  "{name}，有什么我能帮你的吗",
  "今天过得怎么样，{name}",
  "{name}，随时可以开始",
  "{name}，来聊点有趣的吧",
  "有问题尽管问，{name}",
  "{name}，想了解点什么",
  "{name}，很高兴和你聊天",
  "{name}，今天想听你说说"
]

export function pick(name) {
  const user = name || "用户"
  const tpl = TEMPLATES[Math.floor(Math.random() * TEMPLATES.length)]
  return tpl.replace(/\{name\}/g, user)
}
