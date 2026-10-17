/**
 * 课程表数据源（协议 version=4 整表模式）
 *
 * 手机端一次下发完整学期（courses + settings + times + holidays），手环按
 * class_start_time 自行推算任意日期的教学周并按周次规则过滤 —— 翻到哪一天都有课。
 * 旧版按周分桶（v1/v2）与按日期直推（v3）路径已移除，见 sync.js 顶部说明。
 */

const WEEKDAYS = ['周日', '周一', '周二', '周三', '周四', '周五', '周六']

const SECTION_META = {
  morning: { key: 'morning', label: '上午课程' },
  afternoon: { key: 'afternoon', label: '下午课程' },
  evening: { key: 'evening', label: '晚上课程' }
}

const SECTION_ORDER = ['morning', 'afternoon', 'evening']

/**
 * 课前提醒提前量：band 端先固定 15 分钟。
 *
 * 源课表 App 的口径是 CourseRepository.getPreClassReminderMinutes()（用户可调，
 * 默认 20 分钟），但 v4 的 settings 里还没有这个字段，band 端取不到，
 * 所以暂定 15 分钟；等源 App 把 pre_class_reminder_minutes 下发后即自动跟随。
 */
const DEFAULT_PRE_CLASS_LEAD = 15

/**
 * 一周课程：key 为 0-6（周日-周六，即 JS Date.getDay() 原生值）。
 * v4 整表模式下课程统一放在 fullCourses（按 dayOfWeek 过滤），本表恒为空，
 * 仅为 buildHomeViewModel 等历史调用点保留空容器。
 * 每节课：{ id, name, location, teacher, startTime, endTime, periods, section }
 */
const weeklySchedule = {
  0: [],
  1: [],
  2: [],
  3: [],
  4: [],
  5: [],
  6: []
}

/** 是否已收到过手机端课表（用于区分「未连接」和「今日无课」） */
let hasSynced = false

/** 假期/调休条目（兼容 HolidayManager.Entry）；type 0=假期 1=调休 */
let holidays = []

function markSynced() {
  hasSynced = true
}

function hasSyncedSchedule() {
  return hasSynced
}

function setHolidays(list) {
  holidays = []
  if (!list || !list.length) return
  for (let i = 0; i < list.length; i++) {
    const h = list[i]
    if (!h || !h.start) continue
    const type = h.type === 1 ? 1 : 0
    holidays.push({
      start: h.start,
      end: h.end || h.start,
      name: h.name || '',
      type: type,
      /* 调休「上哪一天的课」的绝对日期（手机端唯一可信来源，见 HolidayManager.Entry.followDate）。
         解析成手表内部星期后复用同一条映射链路；空/坏值 → -1（未配置）。 */
      followDay: followDateToWatchDay(h.followDate),
      followWeek: h.followWeek == null ? -1 : parseInt(h.followWeek, 10),
      followWeekday: h.followWeekday == null ? -1 : parseInt(h.followWeekday, 10)
    })
  }
}

function getHolidays() {
  return holidays.slice()
}

function dateKey(date) {
  const y = date.getFullYear()
  const m = date.getMonth() + 1
  const d = date.getDate()
  return y + '-' + (m < 10 ? '0' + m : m) + '-' + (d < 10 ? '0' + d : d)
}

function findHolidayEntry(date, type) {
  if (!holidays.length) return null
  const key = dateKey(date)
  for (let i = 0; i < holidays.length; i++) {
    const h = holidays[i]
    if (type != null && h.type !== type) continue
    if (key >= h.start && key <= h.end) return h
  }
  return null
}

/** 手机 dayOfWeek(1=周一..7=周日；0 也按周日容错) → 手表 week key(0=周日..6=周六)，非法 -1 */
function phoneDayToWatchDay(phoneDay) {
  const d = parseInt(phoneDay, 10)
  if (isNaN(d)) return -1
  if (d === 0 || d === 7) return 0
  return d >= 1 && d <= 6 ? d : -1
}

/**
 * 调休目标绝对日期 'yyyy-MM-dd' → 手表内部 week key(0=周日..6=周六)，非法返回 -1。
 * 手机端 Entry.followDate 是唯一可信来源：周次是相对「课表学期开始时间」算的，
 * 同一对 (followWeek, followWeekday) 在不同课表下指向不同日期，只有绝对日期换课表不跟错。
 */
function followDateToWatchDay(followDate) {
  const text = String(followDate || '').trim()
  if (!text) return -1
  const d = parseYmd(text)
  if (!d) return -1
  return d.getDay()
}

/**
 * 调休条目的目标星期 → 手表内部 week key(0=周日..6=周六)。
 * 优先 followDate（绝对日期，与手机端一致）；仅在它缺失时回退老的
 * followWeekday（手机域 1-7，0 也按周日容错）—— 老数据迁移期才会用到。
 * 返回 -1 表示该条目没有可用映射（即未配置上哪一天的课）。
 */
function workSwapTargetDay(entry) {
  if (!entry) return -1
  if (entry.followDay != null && entry.followDay >= 0) return entry.followDay
  return phoneDayToWatchDay(entry.followWeekday)
}

/**
 * 找当天命中的调休(补班)条目。
 * 同一天可能命中多条（跨年归档、custom 覆盖），优先返回「配置了有效 followWeekday」的那条，
 * 都没有配置时退回第一条 —— 手机端同样是按优先级挑一条来用。
 */
function findWorkSwap(date) {
  if (!holidays.length) return null
  const key = dateKey(date)
  let first = null
  for (let i = 0; i < holidays.length; i++) {
    const h = holidays[i]
    if (h.type !== 1) continue
    if (key < h.start || key > h.end) continue
    if (!first) first = h
    if (workSwapTargetDay(h) >= 0) return h
  }
  return first
}

/**
 * 解析某天显示用的星期键，规则与手机端 CourseReminderHelper.resolveDaySchedule 对齐：
 *   1. 当天是假期 → -1（不排课。手机端 HolidayCourseExclusion 默认也是隐藏，
 *      仅在「假期末日例外」开启时才保留命中课程，该配置不会下发给手表）
 *   2. 调休且 followWeekday(手机域 1-7) 有效 → 改上映射星期的课
 *   3. 其余（含「补班但未配置 followWeekday」）→ 当天日历星期
 * 注意顺序：假期判定必须在调休映射之后覆盖，手机端也是先算候选课再用假期清空。
 */
function resolveDisplayDayKey(date) {
  const mapped = workSwapTargetDay(findWorkSwap(date))
  if (findHolidayEntry(date, 0)) return -1
  if (mapped >= 0) return mapped
  return date.getDay()
}

/**
 * 假期判定：直接查手机下发的 holidays 列表（v4 整表是学期权威数据）。
 */
function isHolidayDate(date) {
  return findHolidayEntry(date, 0) != null
}

function holidayNameFor(date) {
  const h = findHolidayEntry(date, 0) || findHolidayEntry(date, 1)
  return h ? (h.name || '') : ''
}

/* ---------------------------------------------------------------------------
 * 整表模式（协议 version=4）：手机一次推「整学期课程 + 学期设置 + 节次时间」，
 * 手环自己算周次
 * ---------------------------------------------------------------------------
 * 字段与手机端 Course.kt / WatchPayload.buildFullJson 对齐：
 *   courses[]: { id, name, dayOfWeek(1=周一..7=周日), startSection, endSection,
 *                isCustomTime, customStartTime, customEndTime, startWeek, endWeek,
 *                weekType(0=全周 1=单周 2=双周), selectedWeeks[], location, teacher }
 *   settings:  { class_start_time:'YYYY/MM/DD', current_week, total_weeks,
 *                morning_sections, afternoon_sections, evening_sections }
 *   times:     { morning:{'1':'08:00-08:40',...}, afternoon:{...}, evening:{...} }
 *
 * 由手环按日期自己算教学周、自己按周次规则过滤，所以手环的「第几周」与手机
 * 永远一致，翻到任意一周都能正确显示。
 * 周次公式与手机端 CourseScheduleDateBounds.calendarWeekForDate 完全一致。
 * ------------------------------------------------------------------------- */

let fullCourses = []
let fullSettings = null
let fullSectionTimes = {}
/** 整表周次校准偏移（调休合并周等导致日历周与手机教学周的差），见 calibrateWeekOffset */
let weekOffset = 0

/** 目标日期所在周的周一 key（'YYYY-MM-DD'），仅用于周次标注比较 */
function mondayKey(date) {
  const d = new Date(date.getFullYear(), date.getMonth(), date.getDate())
  d.setDate(d.getDate() - ((d.getDay() + 6) % 7))
  return dateKey(d)
}

/* 旧协议（v1/v2 周分桶、v3 按日期直推）的周归档与单双周推断已随协议移除；
   v4 整表自带完整周次规则，翻任意日期都由 weekForDate 推算，无需累积观测。 */

function isFullMode() {
  return !!(fullSettings || fullCourses.length)
}

function toInt(v, fallback) {
  const n = parseInt(v, 10)
  if (isNaN(n)) return fallback == null ? 0 : fallback
  return n
}

/** 'YYYY/MM/DD' 或 'YYYY-MM-DD' → 本地零点的 Date，非法返回 null */
function parseYmd(text) {
  const m = /^(\d{4})[\/\-.](\d{1,2})[\/\-.](\d{1,2})/.exec(String(text || ''))
  if (!m) return null
  const d = new Date(toInt(m[1]), toInt(m[2]) - 1, toInt(m[3]))
  return isNaN(d.getTime()) ? null : d
}

/**
 * 全局绝对节次号 → 'HH:mm-HH:mm'。
 * 上午 1..M，下午 M+1..M+A，晚上 M+A+1..M+A+E；times 里的编号是各时段「内部」编号。
 */
function buildSectionTimes(settings, times) {
  const src = times || {}
  const counts = [
    ['morning', settings.morning || Object.keys(src.morning || {}).length],
    ['afternoon', settings.afternoon || Object.keys(src.afternoon || {}).length],
    ['evening', settings.evening || Object.keys(src.evening || {}).length]
  ]
  const out = {}
  let base = 0
  for (let i = 0; i < counts.length; i++) {
    const name = counts[i][0]
    const count = counts[i][1] || 0
    const table = src[name] || {}
    for (let n = 1; n <= count; n++) {
      const v = table[String(n)] != null ? table[String(n)] : table[n]
      if (v != null && v !== '') out[base + n] = String(v)
    }
    base += count
  }
  return out
}

function normalizeFullSettings(raw) {
  const s = raw || {}
  return {
    semesterStart: parseYmd(s.class_start_time || s.classStartTime || s.start_date),
    currentWeek: toInt(
      s.current_week != null ? s.current_week : s.currentWeek != null ? s.currentWeek : s.teachingWeek,
      0
    ),
    totalWeeks: toInt(s.total_weeks != null ? s.total_weeks : s.totalWeeks, 0),
    morning: toInt(s.morning_sections != null ? s.morning_sections : s.morningSections, 0),
    afternoon: toInt(s.afternoon_sections != null ? s.afternoon_sections : s.afternoonSections, 0),
    evening: toInt(s.evening_sections != null ? s.evening_sections : s.eveningSections, 0),
    /* 手机端课前提醒的提前量。源 App 是 CourseRepository.getPreClassReminderMinutes()
       （默认 20，用户可调）；目前 v4 的 settings 还没带这个字段，取不到就用 band 端的固定值 15。 */
    preClassLead: normalizePreClassLead(
      s.pre_class_reminder_minutes != null
        ? s.pre_class_reminder_minutes
        : s.preClassReminderMinutes
    )
  }
}

/** 课前提醒提前量（分钟）：合法区间 1-120，异常值一律回落到缺省值 */
function normalizePreClassLead(v) {
  const n = toInt(v, 0)
  if (n > 0 && n <= 120) return n
  return DEFAULT_PRE_CLASS_LEAD
}

/**
 * 「快上课了」的判定提前量。
 * 手机下发 settings.pre_class_reminder_minutes（源 App 的课前提醒设置）后跟随手机；
 * 没有该字段时用 band 端的固定值 15 分钟。
 */
function preClassLeadMinutes() {
  return fullSettings && fullSettings.preClassLead > 0
    ? fullSettings.preClassLead
    : DEFAULT_PRE_CLASS_LEAD
}

/** 手机若直接给了时段（'morning'/'afternoon'/'evening' 或 0/1/2）就照用 */
function normalizePeriodField(v) {
  if (v == null) return ''
  const s = String(v).trim()
  if (SECTION_META[s]) return s
  if (s === '0') return 'morning'
  if (s === '1') return 'afternoon'
  if (s === '2') return 'evening'
  return ''
}

function normalizeFullCourse(raw, index) {
  if (!raw || typeof raw !== 'object') return null
  const name = raw.name || ''
  if (!name) return null
  const day = raw.day != null ? toInt(raw.day, -1) : phoneDayToWatchDay(raw.dayOfWeek)
  if (day < 0) return null
  const selected = []
  if (Array.isArray(raw.selectedWeeks)) {
    for (let i = 0; i < raw.selectedWeeks.length; i++) {
      const w = toInt(raw.selectedWeeks[i], -1)
      if (w > 0) selected.push(w)
    }
  }
  return {
    id: raw.id != null ? String(raw.id) : 'f' + day + '-' + index,
    name: name,
    location: raw.location || raw.classroom || '',
    teacher: raw.teacher || '',
    day: day,
    startSection: toInt(raw.startSection, 0),
    endSection: toInt(raw.endSection, 0),
    isCustomTime: !!raw.isCustomTime,
    customStartTime: raw.customStartTime || '',
    customEndTime: raw.customEndTime || '',
    startWeek: toInt(raw.startWeek, 0),
    endWeek: toInt(raw.endWeek, 0),
    weekType: toInt(raw.weekType, 0),
    selectedWeeks: selected,
    periodsOverride: raw.periods || raw.sectionText || '',
    periodField: normalizePeriodField(raw.period != null ? raw.period : raw.section)
  }
}

/**
 * 写入整表（手机推送与缓存恢复共用）。传 null / 无 courses 时清空整表模式。
 * v4 整表是「全学期权威数据」：覆盖写入，无需清理任何旧协议的累积状态。
 * @param {object|null} data { courses, settings, times }
 */
function setFullSchedule(data) {
  if (!data || !Array.isArray(data.courses)) {
    fullCourses = []
    fullSettings = null
    fullSectionTimes = {}
    return
  }
  const settings = normalizeFullSettings(data.settings)
  fullSettings = settings
  calibrateWeekOffset(settings)
  fullSectionTimes = buildSectionTimes(settings, data.times)
  const list = []
  for (let i = 0; i < data.courses.length; i++) {
    const c = normalizeFullCourse(data.courses[i], i)
    if (c) list.push(c)
  }
  fullCourses = list
  hasSynced = true
}

/** 清空整表状态（收到 clear 指令时用） */
function clearWeekState() {
  fullCourses = []
  fullSettings = null
  fullSectionTimes = {}
  weekOffset = 0
}

/**
 * 当前生效的教学周：直接采用手机 payload 里给的 current_week。
 * 它只代表「推送那一刻」的本周；翻看其它日期时用 weekForDate 推算。
 */
function activeWeek() {
  return fullSettings && fullSettings.currentWeek > 0 ? fullSettings.currentWeek : 0
}

/**
 * 整表模式下按日期推算教学周，公式与手机端
 * CourseScheduleDateBounds.calendarWeekForDate 完全一致：
 *   week = floorDiv(开学日到当天的天数 + (开学日星期-1), 7) + 1
 * 即第 1 周从「开学日所在周的周一」开始（周日起算的开学日，其周日属于第 1 周）。
 * 在此之上叠加 weekOffset 校准（见 calibrateWeekOffset）：
 * 手机端的实际教学周会把「调休合并周」（TeachingWeekReorganization）合并计数，
 * 日历公式不知道这些规则，合并周之后的周次会整体偏移 —— 用推送时刻手机给的
 * current_week 校准后，单双周在合并周之后不再翻转。
 * 推算不出（未下发/日期在第一周周一之前）返回 0，调用方回退到 activeWeek()。
 */
function weekForDate(date) {
  if (!fullSettings || !fullSettings.semesterStart) return 0
  const base = calendarWeekFor(date, fullSettings.semesterStart)
  if (base <= 0) return 0
  const week = base + weekOffset
  return week >= 1 ? week : 0
}

/** 日历周（不含校准）：第 1 周从开学日所在周的周一开始 */
function calendarWeekFor(date, start) {
  const d = date instanceof Date ? date : parseYmd(date)
  if (!d || !start) return 0
  const days = Math.round(
    (new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime() -
      new Date(start.getFullYear(), start.getMonth(), start.getDate()).getTime()) /
      86400000
  )
  // JS getDay(): 0=周日；手机端 ISO dayOfWeek.value: 周一=1..周日=7，偏移取 value-1
  const startMondayOffset = start.getDay() === 0 ? 6 : start.getDay() - 1
  return Math.floor((days + startMondayOffset) / 7) + 1
}

/**
 * 用推送时刻手机给的 current_week 校准周次偏移：
 * offset = current_week - 日历推算周（限制 ±2，防异常值把全部周次带偏）。
 * 无调休合并周且开学日语义一致时 offset=0，行为不变。
 */
function calibrateWeekOffset(settings) {
  weekOffset = 0
  if (!settings || !settings.semesterStart || settings.currentWeek <= 0) return
  const cal = calendarWeekFor(new Date(), settings.semesterStart)
  const off = settings.currentWeek - cal
  if (off >= -2 && off <= 2) weekOffset = off
}

/** 整表模式是否具备「渲染学期内任意一天」的条件（有课表且有学期起始日） */
function hasFullSemester() {
  return !!(fullSettings && fullSettings.semesterStart && fullCourses.length)
}

function weekTextFor(date, realNow) {
  const computed = weekForDate(date)
  if (computed > 0) return '第' + computed + '周'
  const week = activeWeek()
  if (week <= 0) return ''
  // 无法按日期推算周次时，只有「当前这一周」能确认周数，其它日期不乱标
  if (mondayKey(date) === mondayKey(realNow || new Date())) return '第' + week + '周'
  return ''
}

/**
 * 与手机端 Course.isActiveInWeek 逐分支一致（严格语义）：
 * selectedWeeks 优先；周次边界用原始值比较 —— endWeek=0（教务导入未解析出周次）的课
 * 在手机端任何一周都不显示，这里必须同样隐藏，否则非本周课程会泄漏显示。
 */
function isCourseActiveInWeek(c, week) {
  if (c.selectedWeeks && c.selectedWeeks.length) {
    for (let i = 0; i < c.selectedWeeks.length; i++) {
      if (c.selectedWeeks[i] === week) return true
    }
    return false
  }
  if (week < c.startWeek || week > c.endWeek) return false
  if (c.weekType === 1) return week % 2 === 1
  if (c.weekType === 2) return week % 2 === 0
  return true
}

/**
 * 某天查课用的教学周：优先按学期起始日推算（整表模式），
 * 推算不出（未下发开学日 / 日期在第一周周一之前）回退手机给的 current_week。
 * 调休的目标星期由 resolveDisplayDayKey 处理（改取哪一天的课），周次本身仍按当天算。
 */
function lookupWeekFor(date) {
  const computed = weekForDate(date)
  if (computed > 0) return computed
  return activeWeek()
}

function hmToMinutes(hm) {
  const parts = String(hm || '').split(':')
  if (parts.length !== 2) return -1
  const h = parseInt(parts[0], 10)
  const m = parseInt(parts[1], 10)
  if (isNaN(h) || isNaN(m)) return -1
  return h * 60 + m
}

function sectionStartMinutes(section) {
  const range = fullSectionTimes[section]
  if (!range) return -1
  return hmToMinutes(String(range).split('-')[0])
}

/** 与手机端 Course.periodIndex 一致：优先用手机给的 period/section 字段，其次按墙钟，最后按节次号 */
function fullCourseSection(c) {
  if (c.periodField && SECTION_META[c.periodField]) return c.periodField
  const M = fullSettings ? fullSettings.morning || 0 : 0
  const A = fullSettings ? fullSettings.afternoon || 0 : 0
  const t = fullCourseTimes(c)
  const startMin = hmToMinutes(t.startTime)
  if (startMin >= 0) {
    let aStart = sectionStartMinutes(M + 1)
    if (aStart < 0) aStart = 12 * 60
    let eStart = sectionStartMinutes(M + A + 1)
    if (eStart < 0) eStart = 18 * 60 + 30
    if (startMin < aStart) return 'morning'
    if (startMin < eStart) return 'afternoon'
    return 'evening'
  }
  if (c.startSection > 0) {
    if (c.startSection <= M) return 'morning'
    if (c.startSection <= M + A) return 'afternoon'
    return 'evening'
  }
  return resolveSection('', '')
}

/** 'HH:mm' 形式校验 */
function isHm(v) {
  return /^\d{1,2}:\d{2}$/.test(String(v || ''))
}

/**
 * 与手机端 Course.getEffectiveStartTime / getEffectiveEndTime 一致。
 * payload 里已经带了手机解析好的时间就直接用（v2 的节次编号域不确定，
 * 用现成时间更可靠）；否则按全局节次号查 times 表。
 */
function fullCourseTimes(c) {
  if (isHm(c.customStartTime) && isHm(c.customEndTime)) {
    return { startTime: c.customStartTime, endTime: c.customEndTime }
  }
  const s = fullSectionTimes[c.startSection]
  const e = fullSectionTimes[c.endSection]
  return {
    startTime: s ? String(s).split('-')[0].trim() : '',
    endTime: e ? String(e).split('-').pop().trim() : ''
  }
}

function fullSectionText(c) {
  if (c.periodsOverride) return c.periodsOverride
  if (c.isCustomTime) return ''
  if (c.startSection <= 0 && c.endSection <= 0) return ''
  return c.startSection === c.endSection
    ? '第' + c.startSection + '节'
    : '第' + c.startSection + '-' + c.endSection + '节'
}

/** 整表模式下某天的展示课程（周次按手机给的值过滤；手机没给周次就不过滤） */
function fullCoursesForDay(day, week) {
  const out = []
  for (let i = 0; i < fullCourses.length; i++) {
    const c = fullCourses[i]
    if (c.day !== day) continue
    if (week > 0 && !isCourseActiveInWeek(c, week)) continue
    const t = fullCourseTimes(c)
    out.push({
      id: c.id,
      name: c.name,
      location: c.location,
      teacher: c.teacher,
      startTime: t.startTime,
      endTime: t.endTime,
      periods: fullSectionText(c),
      section: fullCourseSection(c),
      week: week
    })
  }
  out.sort(function (a, b) {
    const x = a.startTime || ''
    const y = b.startTime || ''
    return x < y ? -1 : x > y ? 1 : 0
  })
  return out
}

let quoteText = ''

function setQuote(text) {
  quoteText = text || ''
}

function getQuote() {
  return quoteText
}

/**
 * 按开始时间推断上午/下午/晚上
 */
function resolveSection(section, startTime) {
  if (section && SECTION_META[section]) return section
  const hh = parseInt((startTime || '').split(':')[0], 10)
  if (isNaN(hh)) return 'afternoon'
  if (hh < 12) return 'morning'
  if (hh < 18) return 'afternoon'
  return 'evening'
}

function cloneCourse(course, index, dayKey) {
  return {
    id: course.id || dayKey + '-' + index,
    name: course.name || '',
    location: course.location || '',
    teacher: course.teacher || '',
    startTime: course.startTime || '',
    endTime: course.endTime || '',
    periods: course.periods || '',
    section: resolveSection(course.section, course.startTime)
  }
}

/**
 * 指定日期的课程（v4 整表：按目标星期取，周次按推算值过滤）。
 * 调休由 resolveDisplayDayKey 换成目标星期，假期返回 -1 表示当天不排课。
 */
function getTodayCourses(date) {
  const day = resolveDisplayDayKey(date)
  if (day < 0) return []
  return fullCoursesForDay(day, lookupWeekFor(date))
}

/**
 * 指定日期的课程（周课表按星期取）
 */
function getCoursesForDate(date) {
  return getTodayCourses(date)
}

/**
 * 将课程按上午/下午/晚上分组，保持时间顺序
 */
function groupBySection(courses) {
  const buckets = {
    morning: [],
    afternoon: [],
    evening: []
  }
  for (let i = 0; i < courses.length; i++) {
    const c = courses[i]
    const key = buckets[c.section] ? c.section : 'afternoon'
    buckets[key].push(c)
  }
  const groups = []
  for (let i = 0; i < SECTION_ORDER.length; i++) {
    const key = SECTION_ORDER[i]
    if (buckets[key].length > 0) {
      groups.push({
        key: key,
        label: SECTION_META[key].label,
        courses: buckets[key]
      })
    }
  }
  return groups
}

/**
 * 找出正在上的课（开始含、结束不含；若当前无课则返回 null）
 */
function getCurrentCourse(courses, now, dayDate) {
  const base = dayDate || now
  for (let i = 0; i < courses.length; i++) {
    const c = courses[i]
    const start = parseCourseTime(c.startTime, base)
    const end = parseCourseTime(c.endTime, base)
    if (now.getTime() >= start.getTime() && now.getTime() < end.getTime()) {
      return c
    }
  }
  return null
}

/**
 * 找出下节课（今天尚未开始的第一节；若今天已上完则返回 null）
 */
function getNextCourse(courses, now, dayDate) {
  const base = dayDate || now
  for (let i = 0; i < courses.length; i++) {
    const c = courses[i]
    const start = parseCourseTime(c.startTime, base)
    if (start.getTime() > now.getTime()) {
      return c
    }
  }
  return null
}

function parseCourseTime(timeStr, baseDate) {
  const parts = (timeStr || '').split(':')
  const d = new Date(baseDate.getFullYear(), baseDate.getMonth(), baseDate.getDate())
  d.setHours(parseInt(parts[0], 10) || 0, parseInt(parts[1], 10) || 0, 0, 0)
  return d
}

function getWeekdayName(date) {
  return WEEKDAYS[date.getDay()]
}

/**
 * 组装首页展示模型（与手机端今日页字段对齐）
 * viewDate：要查看的日期；realNow：真实当前时刻（用于状态/倒计时）
 * 上课中：顶部卡片显示正在上的课，倒计时为距下课；否则显示下节课与距上课。
 */
function buildHomeViewModel(viewDate, util, realNow) {
  const date = viewDate || new Date()
  const now = realNow || date
  const isToday = util.isSameDay(date, now)
  const courses = getCoursesForDate(date)
  const groups = groupBySection(courses)
  const current = isToday ? getCurrentCourse(courses, now, date) : null
  const next = isToday ? getNextCourse(courses, now, date) : null

  const sectionViews = []
  for (let i = 0; i < groups.length; i++) {
    const g = groups[i]
    const items = []
    for (let j = 0; j < g.courses.length; j++) {
      const c = g.courses[j]
      const statusInfo = util.getCourseStatus(c, now, date)
      items.push({
        id: c.id,
        name: c.name,
        timeText: c.startTime + ' - ' + c.endTime,
        startTime: c.startTime,
        endTime: c.endTime,
        meta: buildMeta(c),
        status: statusInfo.status,
        countdown: statusInfo.countdown.text,
        section: c.section
      })
    }
    sectionViews.push({
      key: g.key,
      label: g.label,
      courses: items
    })
  }

  // 上课中优先展示当前课，倒计时改距下课；否则维持「下节课 + 距上课」
  const focus = current || next
  let nextView = null
  if (focus) {
    const isCurrent = !!current
    const base = date
    const target = isCurrent
      ? parseCourseTime(focus.endTime, base)
      : parseCourseTime(focus.startTime, base)
    const cd = isCurrent ? util.formatRemain(target, now) : util.formatCountdown(target, now)
    nextView = {
      id: focus.id,
      name: focus.name,
      label: isCurrent ? '正在上课' : '下节课',
      timeText: focus.startTime + ' - ' + focus.endTime,
      startTime: focus.startTime,
      endTime: focus.endTime,
      meta: buildMeta(focus),
      countdown: cd.text
    }
  }

  const hasCourses = courses.length > 0
  let emptyState = 'none'
  if (!hasCourses) {
    if (isHolidayDate(date) || resolveDisplayDayKey(date) < 0) {
      emptyState = 'holiday'
    } else {
      emptyState = hasSynced ? 'no-class' : 'need-phone'
    }
  }

  return {
    isToday: isToday,
    weekday: getWeekdayName(date),
    dateText: util.formatDate(date),
    week: activeWeek(),
    weekText: weekTextFor(date, now),
    quote: quoteText,
    hasCourses: hasCourses,
    hasNext: !!nextView,
    nextCourse: nextView,
    groups: sectionViews,
    hasSynced: hasSynced,
    isHoliday: isHolidayDate(date),
    holidayName: holidayNameFor(date),
    emptyState: emptyState
  }
}

/**
 * 卡片副文案：节次 | 地点 | 教师（与手机端一致，空字段自动省略）
 */
function buildMeta(course) {
  const parts = []
  if (course.periods) parts.push(course.periods)
  if (course.location) parts.push(course.location)
  if (course.teacher) parts.push(course.teacher)
  return parts.join(' | ')
}

export default {
  WEEKDAYS,
  SECTION_META,
  getTodayCourses,
  getCoursesForDate,
  groupBySection,
  getCurrentCourse,
  getNextCourse,
  setFullSchedule,
  clearWeekState,
  isFullMode,
  hasFullSemester,
  weekForDate,
  activeWeek,
  weekTextFor,
  setQuote,
  getQuote,
  setHolidays,
  getHolidays,
  isHolidayDate,
  holidayNameFor,
  findWorkSwap,
  resolveDisplayDayKey,
  resolveSection,
  getWeekdayName,
  buildHomeViewModel,
  buildMeta,
  preClassLeadMinutes,
  markSynced,
  hasSyncedSchedule
}
