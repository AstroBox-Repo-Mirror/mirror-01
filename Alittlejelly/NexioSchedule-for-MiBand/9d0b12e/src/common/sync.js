/**
 * 手机端 → 手表端 同步接口层（协议 version=4 整表，唯一支持的形态）
 *
 * 对端应用：手机端课程表（包名 com.haooz.chedule，组包见其 wearable/WatchPayload.kt）
 * 主通道：@system.interconnect（小米穿戴 MessageApi ↔ 手表 @system.interconnect）
 * 本地缓存：@system.storage —— 冷启动 / 断开手机后仍可查看上次同步的课表
 *
 * ---------------------------------------------------------------------------
 * 协议（以手机端 WatchPayload.buildFullJson 为准）
 * ---------------------------------------------------------------------------
 * {
 *   "protocol": "nexio.schedule",
 *   "version": 4,
 *   "action": "replace",                 // replace=整表覆盖 | clear=清空
 *   "sentAt": 1760000000000,
 *   "schedule_name": "默认课表",
 *   "settings": {
 *     "class_start_time": "2026/09/13",  // 开学日（手环据此推算任意日期的教学周）
 *     "current_week": 4,                 // 推送时刻的教学周（校准调休合并周偏移）
 *     "total_weeks": 18,
 *     "morning_sections": 6, "afternoon_sections": 5, "evening_sections": 3
 *   },
 *   "times": { "morning": {"1":"08:00-08:40", ...}, "afternoon": {...}, "evening": {...} },
 *   "courses": [
 *     { "id":"c1", "name":"高等数学", "dayOfWeek":1, "startSection":1, "endSection":2,
 *       "startWeek":2, "endWeek":12, "weekType":0, "selectedWeeks":[],
 *       "isCustomTime":false, "customStartTime":"", "customEndTime":"",
 *       "location":"A101", "teacher":"张老师" }
 *   ],
 *   "holidays": [
 *     { "date":"2026-10-01", "endDate":"2026-10-07", "name":"国庆节", "type":0,
 *       "followDate":"", "followWeek":-1, "followWeekday":-1, "custom":false },
 *     { "date":"2026-10-10", "endDate":"", "name":"补班", "type":1,
 *       "followDate":"2026-10-12", "followWeek":6, "followWeekday":1, "custom":false }
 *   ]
 * }
 *
 * 字段语义：
 *   - dayOfWeek 1=周一 .. 7=周日（手机域）；手环内部 0=周日 .. 6=周六，
 *     换算在 schedule.normalizeFullCourse 里完成。
 *   - weekType 0=全周 1=单周 2=双周；selectedWeeks 非空时优先于
 *     startWeek/endWeek/weekType（与手机端 Course.isActiveInWeek 严格一致）。
 *   - holidays[].type 0=假期（隐藏当天课程）1=调休（改上 followDate 那天的课）。
 *     调休优先用 followDate（绝对日期，手机端唯一可信来源，换课表不跟错）；
 *     它缺失时才回退老的 followWeek/followWeekday（迁移期兼容）。
 *
 * 手环侧自行推算学期内任意一天的教学周，公式与手机端
 * CourseScheduleDateBounds.calendarWeekForDate 一致（周一起算，第 1 周 = 开学日
 * 所在周的周一），并用推送时刻的 current_week 校准调休合并周造成的偏移，
 * 因此手环与手机「第几周」永远一致，翻到任何日期都有完整课表。
 *
 * 兼容性：旧协议的按周分桶（v1/v2）、按日期直推（v3）路径已全部移除。
 * 收到非 v4 包（没有 courses 数组）会明确拒绝，不再降级解析 —— 请与新版 APK 一起上线。
 *
 * 手机端主动要数据（手动接口，正常被动模式下不发送；手机端
 * WearableScheduleSync 收到 action=request 就整包推送，它只校验 protocol / action）：
 * { "protocol": "nexio.schedule", "version": 4, "action": "request", "reason": "manual" }
 *
 * 读取频率：手机端点「推送到手环」是主要数据入口；另外启动 / onShow（数据过期）/
 * 通道重连 / 定时轮询会发一次 request 兜底自愈。
 *
 * 注意：interconnect 要求手表 rpk 与手机 App 包名、签名一致。
 */

import interconnect from '@system.interconnect'
import storage from '@system.storage'
import prompt from '@system.prompt'
import schedule from './schedule'

const PROTOCOL = 'nexio.schedule'
/** 向手机端发请求时带的协议版本 */
const PROTOCOL_VERSION = 4
/** 本地缓存的内部版本号（缓存由本模块写入，格式自保证） */
const CACHE_VERSION = 0
/**
 * 存储键升过一次代（v1 → v2）：内置调试课表删除前是走真实同步链路注入的，
 * 会被 persist() 写进 storage；只删代码不清缓存的话，冷启动仍会把旧课表恢复
 * 出来。换键让旧缓存永久孤儿化，一次解决所有设备。
 */
const STORAGE_KEY = 'nexio.schedule.payload.v2'

const ACTION = {
  REPLACE: 'replace',
  CLEAR: 'clear',
  REQUEST: 'request',
  ACK: 'ack'
}

/** 手机端包名（interconnect 对端；两端 package 必须一致） */
const PEER_PACKAGE = 'com.haooz.chedule'

const listeners = []
/** 页面级 toast 监听（连接提示等 UI 事件），见 onToast */
const toastListeners = []
let connect = null
let ready = false
let lastError = null
let cachedQuote = ''
/** 已应用的最新同步序号 */
let appliedRev = 0
/** 主动请求同步的最小间隔，避免 onShow/重试把通道刷爆 */
const REQUEST_MIN_GAP_MS = 15000
/** 手动 requestSync 后多久没收到数据就重试，以及最多重试几次 */
const RETRY_DELAY_MS = 4000
const MAX_RETRY = 2
/** 本地数据超过这个时间就认为过期，页面 onShow 时主动向手机要一次 */
const STALE_MS = 5 * 60 * 1000
/** 定时轮询间隔：整表包不大，5 分钟一次足以自愈，又不会明显耗电 */
const POLL_INTERVAL_MS = 5 * 60 * 1000

/** 本地缓存时间戳（0 表示当前进程还没有可用缓存） */
let cachedAt = 0
/**
 * 冷启动本地缓存是否还在恢复中：storage.get 是异步的，回调回来前
 * 页面拿不到数据，若此时渲染会误判"没数据"闪一下"请连接手机"空态。
 * init 置 true，storage 回调（success/fail/异常）置 false；
 * 页面据此决定首帧是等通知还是直接渲染。
 */
let cachePending = false
/** 同步诊断（关于页「同步诊断」展示）：最近一次数据的来源、形态、时间 */
let lastSource = ''
let lastShape = ''
let lastAt = 0
/** 请求节流 / 重试状态 */
let lastRequestAt = 0
let retryTimer = null
let retryCount = 0
/** 定时轮询句柄 */
let pollTimer = null
/** 本次连接是否已弹过「已连接手机」toast（断开后重置，重连才再弹） */
let connectNotified = false

/** 通道接通时提示一次「已连接手机」；重复触发（onopen 与 getReadyState 都报告就绪）不重复弹 */
function notifyConnected() {
  if (connectNotified) return
  connectNotified = true
  let delivered = false
  for (let i = 0; i < toastListeners.length; i++) {
    try {
      toastListeners[i]('已连接手机')
      delivered = true
    } catch (e) {
      console.log('[sync] toast listener error', e)
    }
  }
  // 页面还没挂监听（理论上不会发生）时退回系统 toast（居中，仅兜底）
  if (!delivered) {
    try {
      prompt.showToast({ message: '已连接手机', duration: 2000 })
    } catch (e) {
      console.log('[sync] showToast fail', e)
    }
  }
}

function notify() {
  for (let i = 0; i < listeners.length; i++) {
    try {
      listeners[i]()
    } catch (e) {
      console.log('[sync] listener error', e)
    }
  }
}

function isPlainObject(v) {
  return !!v && typeof v === 'object' && !Array.isArray(v)
}

function parseMessage(raw) {
  if (raw == null) return null
  if (typeof raw === 'string') {
    if (!raw) return null
    try {
      return JSON.parse(raw)
    } catch (e) {
      lastError = 'invalid json'
      return null
    }
  }
  // interconnect onmessage 回调可能包一层 { data: '...' }
  if (isPlainObject(raw) && typeof raw.data === 'string' && raw.protocol == null) {
    return parseMessage(raw.data)
  }
  return isPlainObject(raw) ? raw : null
}

/**
 * storage.get 的 success 按文档直接给字符串；
 * 部分实现会包一层 { key, value } / { data }，这里统一拆出来。
 */
function readStoredValue(data) {
  if (data == null) return ''
  if (typeof data === 'string') return data
  if (isPlainObject(data)) {
    if (typeof data.value === 'string') return data.value
    if (typeof data.data === 'string') return data.data
  }
  return ''
}

/**
 * 归一化假期/调休（兼容 HolidayManager.Entry）：
 * [{date|start, endDate|end, name, type, followDate, followWeek, followWeekday}]
 * type: 0=假期(隐藏课程) 1=调休(改上 followDate 那天的课)
 */
function normalizeHolidays(list) {
  const out = []
  if (!Array.isArray(list)) return out
  for (let i = 0; i < list.length; i++) {
    const item = list[i]
    if (!isPlainObject(item)) continue
    const start = String(item.start || item.date || '')
    const end = String(item.end || item.endDate || start)
    if (!start) continue
    const type = item.type == null ? 0 : parseInt(item.type, 10)
    const weekday = item.followWeekday == null ? -1 : parseInt(item.followWeekday, 10)
    const week = item.followWeek == null ? -1 : parseInt(item.followWeek, 10)
    out.push({
      start: start,
      end: end || start,
      name: String(item.name || ''),
      type: type === 1 ? 1 : 0,
      /* 调休目标绝对日期（手机端唯一可信来源）；空串表示未配置 */
      followDate: String(item.followDate || ''),
      followWeek: isNaN(week) ? -1 : week,
      followWeekday: isNaN(weekday) ? -1 : weekday
    })
  }
  return out
}

/**
 * 校验并归一化手机端 payload（仅接受 version=4 整表）。
 * 成功返回 { ok: true, payload }；失败返回 { ok: false, error }
 */
function normalizePayload(raw) {
  const parsed = parseMessage(raw)
  if (!parsed) {
    return { ok: false, error: 'empty payload' }
  }
  /** 诊断用：手机原始 payload 里出现了哪些已知字段 */
  const KNOWN_FIELDS = [
    'protocol', 'version', 'action', 'courses', 'settings', 'times', 'holidays',
    'current_week', 'class_start_time', 'total_weeks', 'schedule_name'
  ]
  const present = []
  const unknownNames = []
  let unknownMore = false
  const parsedKeys = Object.keys(parsed)
  for (let i = 0; i < parsedKeys.length; i++) {
    if (KNOWN_FIELDS.indexOf(parsedKeys[i]) >= 0) {
      present.push(parsedKeys[i])
    } else if (unknownNames.length < 4) {
      unknownNames.push('+' + parsedKeys[i])
    } else {
      unknownMore = true
    }
  }
  const rawFields = present.concat(unknownNames).join(',') + (unknownMore ? ',…' : '')

  if (parsed.protocol && parsed.protocol !== PROTOCOL) {
    return { ok: false, error: 'protocol mismatch: ' + parsed.protocol }
  }

  const version = parsed.version != null ? Number(parsed.version) : 0
  const action = parsed.action || ACTION.REPLACE
  const holidayList = normalizeHolidays(parsed.holidays)

  if (action === ACTION.CLEAR) {
    return {
      ok: true,
      shape: '清空 字段[' + rawFields + ']',
      payload: {
        action: ACTION.CLEAR,
        sentAt: parsed.sentAt || Date.now(),
        savedAt: parsed.savedAt || 0,
        quote: parsed.quote || '',
        holidays: holidayList
      }
    }
  }

  // 整表是唯一支持的形态：必须有 courses 数组
  if (!Array.isArray(parsed.courses)) {
    return {
      ok: false,
      error:
        'v4 整表 required（无 courses 数组；version=' + (parsed.version != null ? parsed.version : '?') +
        ' 字段[' + rawFields + ']）。旧协议 v1/v2/v3 已不再支持，请升级手机端。'
    }
  }
  if (version !== PROTOCOL_VERSION && version !== CACHE_VERSION) {
    // 有 courses 但仍提示版本异常：仍按整表解析（结构自描述），只是留个诊断记录。
    // CACHE_VERSION 是本模块写缓存时标记的内部版本，不是异常。
    console.log('[sync] unexpected version ' + version + '，按 v4 整表结构解析')
  }

  const st = isPlainObject(parsed.settings) ? parsed.settings : {}

  return {
    ok: true,
    shape:
      '整表 字段[' +
      rawFields +
      '] 课' +
      parsed.courses.length +
      ' 周' +
      (st.current_week != null ? st.current_week : '?') +
      '/' +
      (st.total_weeks != null ? st.total_weeks : '?') +
      ' 假' +
      holidayList.length,
    payload: {
      action: action,
      sentAt: parsed.sentAt || Date.now(),
      savedAt: parsed.savedAt || 0,
      quote: parsed.quote || '',
      scheduleName: String(parsed.schedule_name || parsed.scheduleName || ''),
      holidays: holidayList,
      courses: parsed.courses,
      settings: st,
      times: isPlainObject(parsed.times) ? parsed.times : {}
    }
  }
}

/** 写缓存：只保留有用字段（storage 的 value 必须是字符串） */
function compactForCache(payload) {
  return {
    protocol: PROTOCOL,
    version: CACHE_VERSION,
    action: payload.action || ACTION.REPLACE,
    savedAt: payload.savedAt || Date.now(),
    quote: payload.quote || '',
    scheduleName: payload.scheduleName || '',
    holidays: payload.holidays || [],
    courses: payload.courses || [],
    settings: payload.settings || {},
    times: payload.times || {}
  }
}

function persist(payload, done) {
  const value = JSON.stringify(compactForCache(payload))
  // 文档：value 为空字符串等于删除该项，所以空内容不写
  if (!value || value === '{}') {
    if (done) done(null)
    return
  }
  try {
    storage.set({
      key: STORAGE_KEY,
      value: value,
      success: function () {
        cachedAt = payload.savedAt || Date.now()
        if (done) done(null)
      },
      fail: function (data, code) {
        lastError = 'storage.set fail ' + code
        if (done) done(lastError)
      }
    })
  } catch (e) {
    lastError = String(e)
    if (done) done(lastError)
  }
}

function applyPayload(payload) {
  cachedQuote = payload.quote || ''
  if (payload.action === ACTION.CLEAR) {
    schedule.clearWeekState()
    schedule.setHolidays([])
    schedule.setQuote(cachedQuote)
    return
  }
  schedule.setFullSchedule({
    courses: payload.courses || [],
    settings: payload.settings || {},
    times: payload.times || {}
  })
  schedule.setHolidays(payload.holidays || [])
  schedule.setQuote(cachedQuote)
}

/**
 * 手机端消息统一入口（也可被调试工具直接调用）
 * @param {string|object} raw 手机推送
 * @returns {{ok:boolean, error?:string, shape?:string}}
 */
function handlePhoneMessage(raw) {
  const result = normalizePayload(raw)
  if (!result.ok) {
    lastError = result.error
    console.log('[sync] reject payload:', result.error)
    return result
  }
  appliedRev += 1
  const rev = appliedRev
  result.payload.savedAt = Date.now()
  lastSource = 'phone'
  lastShape = result.shape || ''
  lastAt = result.payload.savedAt
  applyPayload(result.payload)
  persist(result.payload, function () {
    if (rev === appliedRev) notify()
  })
  notify()
  console.log('[sync] applied rev=' + rev + ' ' + lastShape)
  return { ok: true, shape: lastShape }
}

function handleMessageEvent(evt) {
  if (!evt) return
  // 文档：connect.onmessage 回调参数 data 为 String，实际可能是 { data }
  const raw = evt.data != null ? evt.data : evt
  handlePhoneMessage(raw)
}

function bindConnect() {
  if (!connect) return
  connect.onmessage = handleMessageEvent
  connect.onopen = function (data) {
    ready = true
    lastError = null
    console.log('[sync] interconnect open, reconnected=', data && data.isReconnected)
    notifyConnected()
    // force：重连事件必须立刻拉一次，不能被节流窗口吞掉。
    // （之前不带 force：开机时通道没连上的失败请求会占掉 15s 窗口，
    // 手机随后连上时这次 reconnect 被节流 → 课表最长等一个轮询周期才同步）
    requestSync('reconnect', true)
    startPolling()
  }
  connect.onclose = function (data) {
    ready = false
    connectNotified = false
    lastError = (data && data.data) || 'closed'
    console.log('[sync] interconnect closed', lastError)
    // 手机断连后停止轮询，避免无效空转；重连时恢复
    stopPolling()
  }
  connect.onerror = function (data) {
    ready = false
    connectNotified = false
    lastError = (data && (data.data || data.code)) || 'error'
    console.log('[sync] interconnect error', lastError)
    stopPolling()
  }
}

/**
 * 向手机端请求最新课表。
 * 手机端 WearableScheduleSync 收到 action=request 会立刻整包推送，带节流 + 重试。
 */
function requestSync(reason, force) {
  if (!connect) return
  const now = Date.now()
  if (!force && now - lastRequestAt < REQUEST_MIN_GAP_MS) {
    console.log('[sync] request throttled (' + (reason || 'manual') + ')')
    return
  }
  lastRequestAt = now
  const body = {
    protocol: PROTOCOL,
    version: PROTOCOL_VERSION,
    action: ACTION.REQUEST,
    reason: reason || 'manual',
    sentAt: now
  }
  connect.send({
    data: body,
    success: function () {
      console.log('[sync] request sent (' + body.reason + ')')
      scheduleRetry()
    },
    fail: function (data, code) {
      lastError = 'request fail ' + code
      console.log('[sync] request fail', lastError)
      scheduleRetry()
    }
  })
}

/** 请求发出后若迟迟没有新数据，最多重试 MAX_RETRY 次 */
function scheduleRetry() {
  if (retryTimer) return
  const revAtRequest = appliedRev
  retryTimer = setTimeout(function () {
    retryTimer = null
    if (appliedRev !== revAtRequest) {
      retryCount = 0
      return
    }
    if (retryCount >= MAX_RETRY) {
      console.log('[sync] retry give up, 手机端没回应')
      retryCount = 0
      return
    }
    retryCount += 1
    requestSync('retry' + retryCount, true)
  }, RETRY_DELAY_MS)
}

/**
 * 页面 onShow 调用：数据过期（或从未同步过）就主动向手机要一次。
 * 打开应用/从表盘回到应用时不再被动等推送。
 */
function ensureFresh(reason) {
  const tag = reason || 'ensure-fresh'
  if (!lastAt || Date.now() - lastAt > STALE_MS) {
    retryCount = 0
    requestSync(tag, true)
    return true
  }
  // 数据还新，但通道没连上时也试一次（可能刚开机/刚重连）
  if (!ready) {
    requestSync(tag)
    return true
  }
  return false
}

/**
 * 启动同步通道：先恢复本地缓存，再监听手机消息
 * 有缓存时即使手机不在身边，页面也能显示上次同步的课表。
 */
function init() {
  // 缓存必须先读：冷启动时不依赖任何连接状态
  cachePending = true
  // 看门狗：个别平台实现可能不回 storage 回调，1.5s 后强制结束 pending 并通知一次，
  // 页面首帧不会永远等下去（无缓存的首装场景也靠这条路径渲染空态）。
  setTimeout(function () {
    if (cachePending) {
      cachePending = false
      notify()
    }
  }, 1500)
  try {
    storage.get({
      key: STORAGE_KEY,
      success: function (data) {
        cachePending = false
        const raw = readStoredValue(data)
        if (raw && appliedRev === 0) {
          // 手机已经推过数据（更新），不要用旧缓存覆盖
          const result = normalizePayload(raw)
          if (result.ok) {
            cachedAt = result.payload.savedAt || 0
            lastSource = 'cache'
            lastShape = result.shape || ''
            lastAt = cachedAt
            applyPayload(result.payload)
            console.log('[sync] cache restored, savedAt=' + cachedAt + ' ' + lastShape)
          } else {
            lastError = 'cache rejected: ' + result.error
            console.log('[sync] cache rejected:', result.error)
          }
        }
        // 无论恢复成功与否都通知一次：页面首帧在等这个通知再渲染，
        // 避免缓存还没读回时误判"没数据"闪一下"请连接手机"空态。
        notify()
      },
      fail: function (data, code) {
        cachePending = false
        lastError = 'storage.get fail ' + code
        notify()
      }
    })
  } catch (e) {
    cachePending = false
    lastError = String(e)
    notify()
  }

  if (connect) return
  try {
    connect = interconnect.instance()
    bindConnect()
    connect.getReadyState({
      success: function (data) {
        ready = !!(data && data.status === 1)
        if (ready) {
          // 应用启动时通道已就绪：onopen 可能不会再触发，这里直接提示
          notifyConnected()
          requestSync('app-open')
          startPolling()
        }
      },
      fail: function (data, code) {
        lastError = 'getReadyState fail ' + code
      }
    })
  } catch (e) {
    lastError = String(e)
    console.log('[sync] init fail', lastError)
  }
}

/** 定时轮询：兜底自愈（手机端在身边时，错过的推送最迟一个周期被补上） */
function startPolling() {
  if (pollTimer) return
  pollTimer = setInterval(function () {
    // 手机不在身边时 request 会失败，重试机制会兜住；节流在 requestSync 内部
    requestSync('poll')
  }, POLL_INTERVAL_MS)
}

function stopPolling() {
  if (pollTimer) {
    clearInterval(pollTimer)
    pollTimer = null
  }
}

function teardown() {
  stopPolling()
  if (retryTimer) {
    clearTimeout(retryTimer)
    retryTimer = null
  }
  if (connect) {
    try {
      connect.onmessage = null
      connect.onopen = null
      connect.onclose = null
      connect.onerror = null
    } catch (e) {
      // ignore
    }
    connect = null
  }
  ready = false
}

/**
 * 页面订阅数据变化，返回取消函数
 */
function onScheduleChange(fn) {
  if (typeof fn !== 'function') {
    return function () {}
  }
  listeners.push(fn)
  return function () {
    const idx = listeners.indexOf(fn)
    if (idx >= 0) listeners.splice(idx, 1)
  }
}

/**
 * 页面订阅 toast 消息（如「已连接手机」），自行在页面内定位展示，
 * 返回取消函数。系统 showToast 不可定位，只能作为无页面时的兜底。
 */
function onToast(fn) {
  if (typeof fn !== 'function') {
    return function () {}
  }
  toastListeners.push(fn)
  return function () {
    const idx = toastListeners.indexOf(fn)
    if (idx >= 0) toastListeners.splice(idx, 1)
  }
}

function getStatus() {
  return {
    ready: ready,
    lastError: lastError,
    peerPackage: PEER_PACKAGE,
    protocol: PROTOCOL,
    version: PROTOCOL_VERSION,
    cachedAt: cachedAt,
    hasCache: cachedAt > 0 || schedule.hasSyncedSchedule(),
    rev: appliedRev,
    lastSource: lastSource,
    lastShape: lastShape,
    lastAt: lastAt,
    /** 整表是否完整（有课表 + 学期起始日），可渲染学期内任意一天 */
    hasFull: schedule.hasFullSemester()
  }
}

export default {
  ACTION: ACTION,
  PROTOCOL: PROTOCOL,
  PROTOCOL_VERSION: PROTOCOL_VERSION,
  PEER_PACKAGE: PEER_PACKAGE,
  init: init,
  teardown: teardown,
  requestSync: requestSync,
  ensureFresh: ensureFresh,
  handlePhoneMessage: handlePhoneMessage,
  normalizePayload: normalizePayload,
  onScheduleChange: onScheduleChange,
  onToast: onToast,
  getStatus: getStatus,
  isCachePending: function () { return cachePending }
}
