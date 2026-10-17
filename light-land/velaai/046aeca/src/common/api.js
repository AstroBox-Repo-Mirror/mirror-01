// DeepSeek API 封装：@system.fetch，OpenAI 兼容格式
import fetch from "@system.fetch"

const BASE = "https://api.deepseek.com"

function safeParse(s) {
  try {
    return JSON.parse(s)
  } catch (e) {
    return null
  }
}

function errMsg(data, code) {
  if (data && data.error && data.error.message) return data.error.message
  return "HTTP " + code
}

function request(method, path, apiKey, body) {
  return new Promise(function (resolve, reject) {
    fetch.fetch({
      url: BASE + path,
      method: method,
      header: {
        "Content-Type": "application/json",
        "Authorization": "Bearer " + apiKey
      },
      data: body ? JSON.stringify(body) : "",
      responseType: "json",
      success: function (res) {
        const data = typeof res.data === "string" ? safeParse(res.data) : res.data
        if (res.code >= 200 && res.code < 300) {
          if (data === null) {
            reject(new Error("返回数据异常"))
          } else {
            resolve(data)
          }
        } else if (res.code === 401 || res.code === 403) {
          // API key 无效/无权限：统一文案并打上分类标记
          const err = new Error("API key 错误")
          err.kind = "apikey"
          reject(err)
        } else {
          // 其余 HTTP 错误：透出服务端返回信息
          const err = new Error(errMsg(data, res.code))
          err.kind = "server"
          reject(err)
        }
      },
      fail: function (data, code) {
        // 网络层失败（无 HTTP 响应，如未联网/超时）
        const err = new Error("网络未连接")
        err.kind = "network"
        reject(err)
      }
    })
  })
}

// 对话补全。opts: {apiKey, model, messages, system, temperature, maxTokens, thinking}
// 返回 {content, usage}
export async function chat(opts) {
  const msgs = []
  if (opts.system) msgs.push({ role: "system", content: opts.system })
  for (let i = 0; i < opts.messages.length; i++) {
    const m = opts.messages[i]
    msgs.push({ role: m.role, content: m.content })
  }
  const body = {
    model: opts.model,
    messages: msgs,
    stream: false,
    temperature: opts.temperature
  }
  if (opts.maxTokens !== "" && opts.maxTokens !== undefined && opts.maxTokens !== null) {
    body.max_tokens = parseInt(opts.maxTokens, 10)
  }
  // 深度思考：仅开启时传（thinking 模型支持，其余模型会忽略或报错）
  if (opts.thinking) body.thinking = { type: "enabled" }

  const data = await request("POST", "/chat/completions", opts.apiKey, body)
  if (!data.choices || !data.choices.length || !data.choices[0].message) {
    throw new Error("返回数据异常")
  }
  return {
    content: data.choices[0].message.content,
    usage: data.usage || null
  }
}

// 模型列表：GET /models，返回模型 id 数组
export async function listModels(apiKey) {
  const data = await request("GET", "/models", apiKey, null)
  if (!data.data || !data.data.length) {
    throw new Error("返回数据异常")
  }
  const ids = []
  for (let i = 0; i < data.data.length; i++) {
    ids.push(data.data[i].id)
  }
  return ids
}

// 余额查询：格式化为 "500¥+7$" 紧凑风格（去掉多余小数零）
function trimNum(s) {
  const f = parseFloat(s)
  return isNaN(f) ? String(s) : String(f)
}

export async function balance(apiKey) {
  const data = await request("GET", "/user/balance", apiKey, null)
  if (!data.balance_infos || !data.balance_infos.length) {
    throw new Error("返回数据异常")
  }
  let cny = ""
  let usd = ""
  for (let i = 0; i < data.balance_infos.length; i++) {
    const info = data.balance_infos[i]
    if (info.currency === "CNY") cny = info.total_balance
    else if (info.currency === "USD") usd = info.total_balance
  }
  let text = trimNum(cny) + "¥"
  if (usd) text += "+" + trimNum(usd) + "$"
  return text
}
