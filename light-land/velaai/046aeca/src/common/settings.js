// 全局配置：system.storage 持久化。对话记录用 system.file，见各页面。
import storage from "@system.storage"

export const DEFAULTS = {
  apiKey: "",
  username: "用户",
  model: "deepseek-flash",
  // 去除 Markdown：AI 回复后处理，去掉 markdown 标记便于手表阅读
  stripMarkdown: true,
  // 长对话总结
  summaryEnabled: true,
  summaryEvery: 10, // 每 10 回合总结一次
  summaryMaxTokens: 512,
  // 标题
  titleEnabled: true,
  // 参数设置
  thinking: false, // 深度思考模式
  maxTokens: "", // 最大输出长度（空 = 模型默认）
  system: "", // 系统提示词（空 = 不发送）
  temperature: 1, // 采样温度 0.00~2.00
  // 自动清理
  autoCleanEnabled: true,
  autoCleanDays: 14,
  autoCleanTurns: 30,
  // 累计已用 token（真实 usage 累加）
  usedTokens: 0
}

let cache = {}

function get(key) {
  return new Promise(function (resolve) {
    storage.get({
      key: key,
      success: function (v) {
        if (key === "usedTokens") console.log("storage.get usedTokens =", JSON.stringify(v))
        resolve(v)
      },
      fail: function () {
        if (key === "usedTokens") console.log("storage.get usedTokens FAIL")
        resolve(undefined)
      },
      complete: function () {}
    })
  })
}

function set(key, value) {
  return new Promise(function (resolve) {
    storage.set({
      key: key,
      // 统一转成字符串存储：部分 Vela 版本对数字值写入/读取不可靠（usedTokens 曾写入 2270 读回 0）
      value: typeof value === "string" ? value : String(value),
      success: function () {
        if (key === "usedTokens") console.log("storage.set usedTokens OK =", JSON.stringify(value))
        resolve()
      },
      fail: function () {
        if (key === "usedTokens") console.log("storage.set usedTokens FAIL =", JSON.stringify(value))
        resolve()
      },
      complete: function () {}
    })
  })
}

// 读取全部配置（缺失项填默认值）
export async function load() {
  const keys = Object.keys(DEFAULTS)
  for (let i = 0; i < keys.length; i++) {
    const k = keys[i]
    let v = await get(k)
    if (k === "system") {
      // 系统提示词：显式保存的空串 = 不发送提示词；从未设置才用默认
      cache[k] = v === undefined || v === null ? DEFAULTS[k] : v
    } else if (v === undefined || v === null || v === "") {
      cache[k] = DEFAULTS[k]
    } else if (
      k === "summaryEnabled" || k === "autoCleanEnabled" ||
      k === "titleEnabled" || k === "thinking" ||
      k === "stripMarkdown"
    ) {
      cache[k] = v === true || v === "true"
    } else if (k === "temperature") {
      const f = parseFloat(v)
      cache[k] = isNaN(f) ? DEFAULTS[k] : Math.min(2, Math.max(0, f))
    } else if (
      k === "summaryEvery" || k === "summaryMaxTokens" ||
      k === "autoCleanDays" ||
      k === "autoCleanTurns" || k === "usedTokens"
    ) {
      const n = parseInt(v)
      cache[k] = isNaN(n) ? DEFAULTS[k] : n
    } else {
      cache[k] = v
    }
  }
  return cache
}

// 增量更新（缓存同步先行：返回上一页立即 bindAll 也能读到新值，storage 异步跟进）
export async function update(patch) {
  const keys = Object.keys(patch)
  for (let i = 0; i < keys.length; i++) {
    const k = keys[i]
    cache[k] = patch[k]
    await set(k, patch[k])
  }
  return cache
}

export function getAll() {
  return Object.assign({}, cache)
}

// 同步取缓存（load 之后可用）
export function getValue(key) {
  return cache[key]
}

// 清空全部配置（恢复默认，含中转值），但保留 API Key
export async function clear() {
  const keep = cache.apiKey
  await new Promise(function (resolve) {
    storage.clear({
      success: function () { resolve() },
      fail: function () { resolve() }
    })
  })
  cache = Object.assign({}, DEFAULTS)
  if (keep) {
    cache.apiKey = keep
    await set("apiKey", keep)
  }
}

// textinput 临时值中转
export async function setTmp(value) {
  await set("__tmp_input__", value)
}

export async function getTmp() {
  const v = await get("__tmp_input__")
  await set("__tmp_input__", "")
  return v === undefined || v === null ? "" : v
}

// 返回状态标记：区分「保存」（含空值）与「返回取消」（不修改）
// "1" = 用户点了保存（即使值是空串也要写入）；空 = 用户取消返回（不写入）
export async function setTmpSaved() {
  await set("__tmp_saved__", "1")
}

export async function clearTmpSaved() {
  await set("__tmp_saved__", "")
}

export async function getTmpSaved() {
  const s = await get("__tmp_saved__")
  await set("__tmp_saved__", "")
  return s === "1"
}
