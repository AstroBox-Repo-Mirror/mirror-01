// 对话持久化：internal://files/conversations/ 下每个对话一个 JSON 文件（上下文隔离），
// index.json 维护列表索引（按 updatedAt 降序，最新在最上）
import file from "@system.file"

const DIR = "internal://files/conversations"
const INDEX = DIR + "/index.json"

function readText(uri) {
  return new Promise(function (resolve, reject) {
    file.readText({
      uri: uri,
      success: function (data) { resolve(data.text) },
      fail: function (msg, code) { reject(new Error(code + ":" + msg)) }
    })
  })
}

function writeText(uri, text) {
  return new Promise(function (resolve, reject) {
    file.writeText({
      uri: uri,
      text: text,
      success: function () { resolve() },
      fail: function (msg, code) { reject(new Error(code + ":" + msg)) }
    })
  })
}

// 目录可能已存在，失败忽略
function mkdir() {
  return new Promise(function (resolve) {
    file.mkdir({
      uri: DIR,
      success: function () { resolve() },
      fail: function () { resolve() }
    })
  })
}

function del(uri) {
  return new Promise(function (resolve) {
    file.delete({
      uri: uri,
      success: function () { resolve() },
      fail: function () { resolve() } // 文件不存在也视为删除成功
    })
  })
}

async function readIndex() {
  try {
    const text = await readText(INDEX)
    const list = JSON.parse(text)
    return Array.isArray(list) ? list : []
  } catch (e) {
    return []
  }
}

async function writeIndex(list) {
  await mkdir()
  await writeText(INDEX, JSON.stringify(list))
}

// 回合数 = 用户消息条数（一问一答为一回合）
function turnsOf(messages) {
  let n = 0
  for (let i = 0; i < messages.length; i++) {
    if (messages[i].role === "user") n++
  }
  return n
}

export function newId() {
  return Date.now().toString(36) + Math.floor(Math.random() * 1296).toString(36)
}

// 列表（最新更新在最上）
export async function list() {
  const idx = await readIndex()
  idx.sort(function (a, b) { return b.updatedAt - a.updatedAt })
  return idx
}

// 加载单个对话：{id, title, updatedAt, messages}
export async function load(id) {
  try {
    const conv = JSON.parse(await readText(DIR + "/conv_" + id + ".json"))
    if (!conv || !Array.isArray(conv.messages)) return null
    return conv
  } catch (e) {
    return null
  }
}

// 保存对话（新增或覆盖）并更新索引
export async function save(conv) {
  await mkdir()
  await writeText(DIR + "/conv_" + conv.id + ".json", JSON.stringify(conv))
  const idx = await readIndex()
  const meta = {
    id: conv.id,
    title: conv.title,
    updatedAt: conv.updatedAt,
    turns: turnsOf(conv.messages)
  }
  let found = false
  for (let i = 0; i < idx.length; i++) {
    if (idx[i].id === conv.id) {
      idx[i] = meta
      found = true
      break
    }
  }
  if (!found) idx.push(meta)
  idx.sort(function (a, b) { return b.updatedAt - a.updatedAt })
  await writeText(INDEX, JSON.stringify(idx))
}

export async function remove(id) {
  await del(DIR + "/conv_" + id + ".json")
  const idx = await readIndex()
  const next = []
  for (let i = 0; i < idx.length; i++) {
    if (idx[i].id !== id) next.push(idx[i])
  }
  await writeIndex(next)
}

// 清空全部对话（危险操作用）
export async function clear() {
  const idx = await readIndex()
  for (let i = 0; i < idx.length; i++) {
    await del(DIR + "/conv_" + idx[i].id + ".json")
  }
  await writeIndex([])
}

// 自动清理：两个条件需同时满足才删除对话——
// 最后更新时间超过 days 天，且回合数超过 turns（索引里已存 turns，免读文件）
export async function autoClean(opts) {
  if (!opts || !opts.enabled) return 0
  const dayMs = (opts.days > 0 ? opts.days : 14) * 86400000
  const turnMin = opts.turns > 0 ? opts.turns : 30
  const idx = await readIndex()
  const now = Date.now()
  const removed = []
  for (let i = 0; i < idx.length; i++) {
    if (now - idx[i].updatedAt > dayMs && idx[i].turns > turnMin) {
      await del(DIR + "/conv_" + idx[i].id + ".json")
      removed.push(idx[i].id)
    }
  }
  if (removed.length) {
    await writeIndex(
      idx.filter(function (m) { return removed.indexOf(m.id) < 0 })
    )
  }
  return removed.length
}

// 计算某对话距自动清理还有几天（历史页倒计时显示用）
// 不满足清理条件（未超回合数/清理已关）返回 -1；剩余不足 1 天返回 0
export function daysToClean(meta, opts) {
  if (!opts || !opts.enabled) return -1
  const turnMin = opts.turns > 0 ? opts.turns : 30
  if (!(meta.turns > turnMin)) return -1
  const dayMs = (opts.days > 0 ? opts.days : 14) * 86400000
  const left = Math.ceil((meta.updatedAt + dayMs - Date.now()) / 86400000)
  return left > 0 ? left : 0
}
