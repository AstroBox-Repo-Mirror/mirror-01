/**
 * 手表端 × 手机端（Nexio 课程表 APK）通信回归检查 —— 协议 version=4（整表）专用
 *
 * 覆盖：
 *   1. 只接受 v4 整表：courses + settings + times + holidays；旧协议（v1/v2 周分桶、
 *      v3 按日期直推）必须被明确拒绝，不能静默降级
 *   2. 周次：按 class_start_time 逐日推算（与手机端 calendarWeekForDate 一致），
 *      并用 current_week 校准调休合并周偏移
 *   3. 周次规则：selectedWeeks / weekType 单双周 / startWeek-endWeek 范围
 *   4. 假期与调休：假期优先隐藏；调休按 followDate（绝对日期）换课，
 *      缺失时才回退 followWeekday；followWeekday 的 7 与 0 都按周日
 *   5. 时间：优先用 customStartTime/customEndTime，其次按 times 解析全局节次号
 *   6. 缓存：推过一次后冷启动（无手机）仍能看到课表
 *
 * 用法：node tools/check-wearable-sync.mjs [真实课表.json]
 * 说明：直接用 src/common 下的真实代码，只把 @system.interconnect /
 *      @system.storage 换成内存桩；不依赖 aiot-toolkit，也不需要设备。
 *      可选传入手机导出的课表 JSON 做端到端校验。
 */
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

/** 本工程源码目录 */
const SRC = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../src/common')
/** 临时工作目录放系统临时区，避免污染仓库 */
const WORKROOT = path.join(os.tmpdir(), 'nexio-watch-check')
/** 可选：真实课表 JSON（命令行参数或环境变量） */
const REAL = process.argv[2] || process.env.NEXIO_REAL_SCHEDULE || ''

const PROJECTS = [{ tag: 'watch', src: SRC }]

let failures = 0
function check(label, ok, detail) {
  if (!ok) failures++
  console.log(`  ${ok ? 'PASS' : 'FAIL'}  ${label}${detail ? '  -> ' + detail : ''}`)
}

// ---------------------------------------------------------------- 手机端组包（v4）
/**
 * 整表 fixture：与手机端 WatchPayload.buildFullJson 同结构
 * （settings.class_start_time=2026/09/13 周日；上午 6 节 / 下午 5 节 / 晚上 3 节，
 *  节次号全局 1..14）
 */
const FULL_TIMES = {
  morning: { 1: '08:00-08:40', 2: '08:50-09:30', 3: '09:40-10:20', 4: '10:30-11:10', 5: '11:20-12:00', 6: '12:10-14:30' },
  afternoon: { 1: '14:40-15:20', 2: '15:30-16:10', 3: '16:20-17:00', 4: '17:10-17:50', 5: '18:00-18:40' },
  evening: { 1: '19:30-20:10', 2: '20:20-21:00', 3: '21:10-21:50' }
}
const FULL_COURSES = [
  { id: 'c1', name: '高等数学', location: 'A101', teacher: '张老师', dayOfWeek: 1, startSection: 1, endSection: 2, isCustomTime: false, customStartTime: '', customEndTime: '', startWeek: 2, endWeek: 12, weekType: 0, selectedWeeks: [] },
  { id: 'c2', name: '单周物理', location: 'B202', teacher: '李老师', dayOfWeek: 1, startSection: 3, endSection: 4, isCustomTime: false, customStartTime: '', customEndTime: '', startWeek: 1, endWeek: 18, weekType: 1, selectedWeeks: [] },
  { id: 'c3', name: '双周化学', location: 'C303', teacher: '王老师', dayOfWeek: 2, startSection: 1, endSection: 2, isCustomTime: false, customStartTime: '', customEndTime: '', startWeek: 1, endWeek: 18, weekType: 2, selectedWeeks: [] },
  { id: 'c4', name: '选修课', location: 'D404', teacher: '赵老师', dayOfWeek: 2, startSection: 7, endSection: 8, isCustomTime: false, customStartTime: '', customEndTime: '', startWeek: 2, endWeek: 14, weekType: 0, selectedWeeks: [2, 3, 4, 6, 7] },
  { id: 'c5', name: '晚自习课', location: 'E505', teacher: '钱老师', dayOfWeek: 3, startSection: 12, endSection: 14, isCustomTime: true, customStartTime: '19:30', customEndTime: '20:30', startWeek: 1, endWeek: 18, weekType: 0, selectedWeeks: [] },
  { id: 'c6', name: '体育', location: '田径场', teacher: '孙老师', dayOfWeek: 4, startSection: 7, endSection: 9, isCustomTime: false, customStartTime: '', customEndTime: '', startWeek: 1, endWeek: 18, weekType: 0, selectedWeeks: [] },
  { id: 'c7', name: '期末讲座', location: '报告厅', teacher: '周老师', dayOfWeek: 4, startSection: 1, endSection: 2, isCustomTime: false, customStartTime: '', customEndTime: '', startWeek: 10, endWeek: 10, weekType: 0, selectedWeeks: [] }
]
/** 真实「今天」相对 fixture 学期（2026/09/13 周日开学）的日历周。
 *  整表 fixture 的 current_week 用它生成 → 周次校准偏移恒为 0，用例不随运行日期漂移 */
const CAL_WEEK_TODAY = Math.max(
  1,
  Math.floor(
    (Math.round(
      new Date(new Date().getFullYear(), new Date().getMonth(), new Date().getDate()).getTime() -
        new Date(2026, 8, 13).getTime()
    ) /
      86400000 +
      6) /
      7
  ) + 1
)

function payloadFull() {
  return {
    protocol: 'nexio.schedule',
    version: 4,
    action: 'replace',
    sentAt: 1760000000000,
    schedule_name: '测试课表',
    settings: {
      class_start_time: '2026/09/13',
      current_week: CAL_WEEK_TODAY,
      total_weeks: 18,
      morning_sections: 6,
      afternoon_sections: 5,
      evening_sections: 3
    },
    times: FULL_TIMES,
    courses: FULL_COURSES,
    holidays: [
      // 10/10 周六补班 → 上 10/12（周一）的课：用 followDate（手机端当前口径）
      { date: '2026-10-10', endDate: '', name: '补班', type: 1, followDate: '2026-10-12', followWeek: 5, followWeekday: -1, custom: false },
      { date: '2026-10-06', endDate: '', name: '调休放假', type: 0, followDate: '', followWeek: -1, followWeekday: -1, custom: false }
    ]
  }
}

// ---------------------------------------------------------------- 旧协议（必须被拒绝）
/** v1：手表域 week 键 0-6 */
function payloadV1() {
  const week = {}
  for (let i = 0; i <= 6; i++) week[String(i)] = []
  return { protocol: 'nexio.schedule', version: 1, action: 'replace', sentAt: 1760000000000, scheduleName: '默认课表', holidays: [], week }
}

/** v2：手机域 week 键 1-7，且带每门课的周次字段 */
function payloadV2Buckets() {
  const week = {}
  for (let d = 0; d <= 6; d++) {
    week[String(d === 0 ? 7 : d)] = FULL_COURSES.filter((c) => c.dayOfWeek === (d === 0 ? 7 : d))
  }
  return {
    protocol: 'nexio.schedule',
    version: 2,
    action: 'replace',
    teachingWeek: 4,
    total_weeks: 18,
    class_start_time: '2026/09/13',
    times: FULL_TIMES,
    week: week,
    holidays: payloadFull().holidays
  }
}

/** v2：整表内容包一层 data 对象（旧手机端的包装形态） */
function payloadV2Nested() {
  return { protocol: 'nexio.schedule', version: 2, action: 'replace', data: payloadFull() }
}

/** v3：按日期直推（今天 ±14 天窗口） */
function payloadV3Dates() {
  return {
    protocol: 'nexio.schedule',
    version: 3,
    action: 'replace',
    week: 4,
    days: [
      { date: '2026-10-12', weekday: 1, courses: [{ name: '高等数学', startTime: '08:00', endTime: '09:40', periods: '第1-2节', location: 'A101', teacher: '张老师' }] }
    ]
  }
}

// ---------------------------------------------------------------- 整表模式预期
/**
 * 周次按「开学日所在周的周一」逐日推算（与手机端 calendarWeekForDate 一致，
 * fixture 开学日 2026/09/13 为周日 → 第 1 周是 09/07-09/13，09/14 起是第 2 周）。
 * 调休日按 followDate 换成目标星期的课。
 */
const FULL_CASES = [
  { date: '2026-09-14', week: 2, names: ['高等数学'], section: ['morning'] },
  { date: '2026-09-21', week: 3, names: ['高等数学', '单周物理'], section: ['morning', 'morning'], note: '第3周 单周物理生效' },
  { date: '2026-09-15', week: 2, names: ['双周化学', '选修课'], section: ['morning', 'afternoon'] },
  { date: '2026-09-29', week: 4, names: ['双周化学', '选修课'], section: ['morning', 'afternoon'] },
  { date: '2026-10-07', week: 5, names: ['晚自习课'], section: ['evening'] },
  { date: '2026-10-08', week: 5, names: ['体育'], section: ['afternoon'] },
  { date: '2026-10-10', week: 5, names: ['高等数学', '单周物理'], section: ['morning', 'morning'], note: '周六补班→周一课(followDate=2026-10-12)' },
  { date: '2026-10-11', week: 5, names: [], section: [] },
  { date: '2026-10-12', week: 6, names: ['高等数学'], section: ['morning'], note: '第6周 单周物理不生效' },
  { date: '2026-11-12', week: 10, names: ['期末讲座', '体育'], section: ['morning', 'afternoon'], note: '只在第10周出现的课要出现' }
]

const localDate = (s) => {
  const [y, m, d] = s.split('-').map(Number)
  return new Date(y, m - 1, d)
}

const STUB_INTERCONNECT = `export default {
  instance() {
    return {
      send(o) { (globalThis.__sent = globalThis.__sent || []).push(o && o.data); o && o.success && o.success() },
      getReadyState(o) { o && o.fail && o.fail({}, 1) },
      onmessage: null, onopen: null, onclose: null, onerror: null
    }
  }
}`

/** 内存版 storage：真实模拟「set 后能 get 回来」，并可切成 {key,value} 包装形态 */
const STUB_STORAGE = `const store = (globalThis.__store = globalThis.__store || {});
export default {
  set(o) { if (o.value) store[o.key] = o.value; o && o.success && o.success() },
  get(o) {
    const v = store[o.key];
    if (v == null) { o && o.fail && o.fail({}, 1); return }
    if (globalThis.__wrapGet) { o && o.success && o.success({ key: o.key, value: v }); return }
    o && o.success && o.success(v)
  }
}`

/** 记录 showToast 调用，连接 toast 的回归检查用 */
const STUB_PROMPT = `export default {
  showToast(o) { (globalThis.__toasts = globalThis.__toasts || []).push(o && o.message) }
}`

let dirSeq = 0
async function loadProject(src) {
  const dir = path.join(WORKROOT, 'w' + dirSeq++)
  fs.mkdirSync(dir, { recursive: true })
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ type: 'module' }))
  fs.writeFileSync(path.join(dir, '__stub-interconnect.js'), STUB_INTERCONNECT)
  fs.writeFileSync(path.join(dir, '__stub-storage.js'), STUB_STORAGE)
  fs.writeFileSync(path.join(dir, '__stub-prompt.js'), STUB_PROMPT)
  const rewrite = (js) => js
    .replace(/from '@system\.interconnect'/g, "from './__stub-interconnect.js'")
    .replace(/from '@system\.storage'/g, "from './__stub-storage.js'")
    .replace(/from '@system\.prompt'/g, "from './__stub-prompt.js'")
    .replace(/from '\.\/([A-Za-z0-9_-]+)'/g, "from './$1.js'")
  for (const f of ['schedule.js', 'util.js', 'sync.js']) {
    fs.writeFileSync(path.join(dir, f), rewrite(fs.readFileSync(path.join(src, f), 'utf8')))
  }
  const load = async (f) => (await import(pathToFileURL(path.join(dir, f)).href)).default
  return { dir, schedule: await load('schedule.js'), util: await load('util.js'), sync: await load('sync.js') }
}

const courseNames = (vm) => vm.groups.reduce((a, g) => a.concat(g.courses.map((c) => c.name)), [])
function view(schedule, util, dateStr) {
  const date = localDate(dateStr)
  const now = localDate(dateStr)
  now.setHours(21, 0, 0, 0)
  return schedule.buildHomeViewModel(date, util, now)
}

fs.rmSync(WORKROOT, { recursive: true, force: true })
fs.mkdirSync(WORKROOT, { recursive: true })

for (const p of PROJECTS) {
  console.log(`\n===== ${p.tag} =====`)

  // ---- 1. 旧协议必须被明确拒绝（不再静默降级）----
  const LEGACY = [
    { label: 'v1 周分桶（手表域键 0-6）被拒绝', payload: payloadV1() },
    { label: 'v2 周分桶（手机域键 1-7 + 周次字段）被拒绝', payload: payloadV2Buckets() },
    { label: 'v2 data 包裹的整表被拒绝（非 v4 形态不入库）', payload: payloadV2Nested() },
    { label: 'v3 按日期直推被拒绝', payload: payloadV3Dates() }
  ]
  for (const v of LEGACY) {
    const inst = await loadProject(p.src)
    const r = inst.sync.handlePhoneMessage(v.payload)
    // 拒绝后不能留下任何可用课表
    const leftover = courseNames(view(inst.schedule, inst.util, '2026-10-12')).join(',')
    check(v.label, r.ok === false && leftover === '',
      r.ok ? '竟然接受了：' + r.shape : `error="${r.error}" 残留=[${leftover}]`)
  }

  // ---- 2. v4 整表：接受并渲染 ----
  const full = await loadProject(p.src)
  const rf = full.sync.handlePhoneMessage(payloadFull())
  check('接受 v4 整表 payload（courses + settings + times + holidays）', rf.ok, rf.ok ? rf.shape : rf.error)
  check('activeWeek 保留手机给的 current_week',
    full.schedule.activeWeek() === CAL_WEEK_TODAY, '实际 ' + full.schedule.activeWeek())
  for (const c of FULL_CASES) {
    const vm = view(full.schedule, full.util, c.date)
    const got = courseNames(vm)
    const secs = vm.groups.reduce((a, g) => a.concat(g.courses.map((x) => x.section)), [])
    check(`${c.date} 第${c.week}周${c.note ? ' ' + c.note : ''}`,
      got.join(',') === c.names.join(',') && secs.join(',') === c.section.join(','),
      `课程=[${got.join(', ')}] 时段=[${secs.join(', ')}] ${vm.weekText}`)
  }
  check('周次标注按日期推算（09-14=第2周，11-12=第10周）',
    view(full.schedule, full.util, '2026-09-14').weekText === '第2周' &&
      view(full.schedule, full.util, '2026-11-12').weekText === '第10周',
    `09-14=${view(full.schedule, full.util, '2026-09-14').weekText} 11-12=${view(full.schedule, full.util, '2026-11-12').weekText}`)

  // 自定义时间优先于节次表
  const w3 = view(full.schedule, full.util, '2026-10-07').groups[0].courses[0]
  check('自定义时间课用 customStartTime/customEndTime（19:30-20:30）',
    w3 && w3.timeText === '19:30 - 20:30', `实际 ${w3 && w3.timeText}`)

  // ---- 3. 缺 class_start_time → 回退 current_week 兜底 ----
  const vNoStart = await loadProject(p.src)
  const pNoStart = payloadFull()
  delete pNoStart.settings.class_start_time
  pNoStart.settings.current_week = 4
  const rNoStart = vNoStart.sync.handlePhoneMessage(pNoStart)
  check('整表缺 class_start_time → 仍可解析（无学期起始日，按 current_week 兜底）',
    rNoStart.ok && vNoStart.schedule.hasFullSemester() === false && vNoStart.schedule.activeWeek() === 4,
    `ok=${rNoStart.ok} hasFull=${vNoStart.schedule.hasFullSemester()} activeWeek=${vNoStart.schedule.activeWeek()}`)

  // ---- 4. 假期与调休 ----
  {
    // 假期优先隐藏：10/06 是 type=0 假期，周二本来有双周化学
    const vm = view(full.schedule, full.util, '2026-10-06')
    check('假期日隐藏当天全部课程（10/06 调休放假）',
      courseNames(vm).length === 0 && full.schedule.isHolidayDate(localDate('2026-10-06')),
      `10/06=[${courseNames(vm).join(',')}] emptyState=${vm.emptyState}`)

    // followDate 生效：10/10 周六 → 上 10/12 周一的课（followWeekday=-1，只能靠 followDate）
    check('调休按 followDate 映射（10/10 → 10/12 周一的课）',
      courseNames(view(full.schedule, full.util, '2026-10-10')).join(',') === '高等数学,单周物理',
      `10/10=[${courseNames(view(full.schedule, full.util, '2026-10-10')).join(',')}]`)
  }

  // followDate 缺失 → 回退 followWeekday（迁移期兼容），7 与 0 都按周日
  {
    const fb = await loadProject(p.src)
    const pFb = payloadFull()
    pFb.holidays = [
      { date: '2026-10-17', endDate: '', name: '补班', type: 1, followDate: '', followWeek: 6, followWeekday: 7, custom: false },
      { date: '2026-10-24', endDate: '', name: '补班0', type: 1, followDate: '', followWeek: 7, followWeekday: 0, custom: false }
    ]
    const rFb = fb.sync.handlePhoneMessage(pFb)
    // 周六补班 → 上周日（周日无课 → 空），但调休条目必须被识别（否则会显示周六自己的课）
    const sat = courseNames(view(fb.schedule, fb.util, '2026-10-17')).join(',')
    const sat2 = courseNames(view(fb.schedule, fb.util, '2026-10-24')).join(',')
    check('followDate 缺失时回退 followWeekday（7 与 0 都按周日，故周六补班无课）',
      rFb.ok && sat === '' && sat2 === '', `10/17=[${sat}] 10/24=[${sat2}]`)
  }

  // 调休与假期同日：假期优先（与手机端一致）
  {
    const both = await loadProject(p.src)
    const pBoth = payloadFull()
    pBoth.holidays = [
      { date: '2026-10-06', endDate: '2026-10-07', name: '国庆节', type: 0, followDate: '', followWeek: -1, followWeekday: -1, custom: false },
      { date: '2026-10-06', endDate: '', name: '假期内补班', type: 1, followDate: '2026-10-12', followWeek: 6, followWeekday: 1, custom: false }
    ]
    both.sync.handlePhoneMessage(pBoth)
    check('调休与假期同日时假期优先隐藏',
      courseNames(view(both.schedule, both.util, '2026-10-06')).length === 0,
      `10/06=[${courseNames(view(both.schedule, both.util, '2026-10-06')).join(',')}]`)
  }

  // ---- 5. 缓存：冷启动仍可看课表 ----
  {
    globalThis.__store = {}
    const hot = await loadProject(p.src)
    hot.sync.handlePhoneMessage(payloadFull())
    check('推送后写入本地缓存', !!globalThis.__store['nexio.schedule.payload'],
      Object.keys(globalThis.__store).join(','))
    const cold = await loadProject(p.src)
    cold.sync.init()
    await new Promise((r) => setTimeout(r, 30))
    check('冷启动从缓存恢复课表并可渲染',
      courseNames(view(cold.schedule, cold.util, '2026-10-12')).join(',') === '高等数学',
      `10/12=[${courseNames(view(cold.schedule, cold.util, '2026-10-12')).join(',')}]`)

    // storage.get 返回 {key,value} 包装形态也要能恢复
    const wrapped = await loadProject(p.src)
    globalThis.__wrapGet = true
    wrapped.sync.init()
    await new Promise((r) => setTimeout(r, 30))
    globalThis.__wrapGet = false
    check('storage.get 返回 {key,value} 包装也能恢复',
      courseNames(view(wrapped.schedule, wrapped.util, '2026-10-12')).join(',') === '高等数学',
      `10/12=[${courseNames(view(wrapped.schedule, wrapped.util, '2026-10-12')).join(',')}]`)
  }

  // ---- 6. clear 指令 ----
  {
    const clr = await loadProject(p.src)
    clr.sync.handlePhoneMessage(payloadFull())
    const before = courseNames(view(clr.schedule, clr.util, '2026-10-12')).join(',')
    const rc = clr.sync.handlePhoneMessage({ protocol: 'nexio.schedule', version: 4, action: 'clear' })
    const after = courseNames(view(clr.schedule, clr.util, '2026-10-12')).join(',')
    check('action=clear 清空课表与假期',
      rc.ok && before === '高等数学' && after === '' && clr.schedule.getHolidays().length === 0,
      `clear前=[${before}] clear后=[${after}] holidays=${clr.schedule.getHolidays().length}`)
  }

  // ---- 7. protocol 不符必须拒绝 ----
  {
    const bad = await loadProject(p.src)
    const rBad = bad.sync.handlePhoneMessage({ protocol: 'other.protocol', version: 4, action: 'replace', courses: [] })
    check('protocol 不符被拒绝', rBad.ok === false, rBad.ok ? '竟然接受' : rBad.error)
  }
}

fs.rmSync(WORKROOT, { recursive: true, force: true })

// ---------------------------------------------------------------- 真实课表（可选）
if (REAL && fs.existsSync(REAL)) {
  console.log('\n===== 真实课表（手机导出的课表 JSON）=====')
  const real = JSON.parse(fs.readFileSync(REAL, 'utf8'))
  for (const p of PROJECTS) {
    globalThis.__store = {}
    const inst = await loadProject(p.src)
    const r = inst.sync.handlePhoneMessage(real)
    check(`${p.tag}: 接受真实课表`, r.ok, r.ok ? r.shape : r.error)
    const flat = (dateStr) => view(inst.schedule, inst.util, dateStr).groups.reduce((a, g) => a.concat(g.courses), [])
    const wantWeek = Number(real.settings && real.settings.current_week) || 0
    if (wantWeek > 0) {
      check(`${p.tag}: activeWeek 与手机 current_week=${wantWeek} 一致`, inst.schedule.activeWeek() === wantWeek, '实际第' + inst.schedule.activeWeek() + '周')
    }
    const hasMinsu = (real.courses || []).some((c) => c.name === '中国民俗文化')
    if (hasMinsu) {
      const has13 = flat('2026-12-01').some((c) => c.name === '中国民俗文化' && c.timeText === '19:30 - 20:30')
      check(`${p.tag}: 第13周限定的自定义时间课在第13周(12-01)出现`, has13, `w13出现=${has13}`)
    }
    const flatOf = (inst2, dateStr) =>
      view(inst2.schedule, inst2.util, dateStr).groups.reduce((a, g) => a.concat(g.courses), [])
    const printDays = (inst2, tag, days) => {
      for (const [d, label] of days) {
        const vm = view(inst2.schedule, inst2.util, d)
        const list = flatOf(inst2, d).map((c) => `${c.name}(${c.timeText} ${c.section}${c.periods ? ' ' + c.periods : ''})`)
        console.log(`  ${tag} ${label} ${d} ${vm.weekText}: ${list.join('  ') || '无课'}`)
      }
    }
    printDays(inst, p.tag, [['2026-09-28', '周一'], ['2026-09-29', '周二'], ['2026-09-30', '周三'], ['2026-10-01', '周四'], ['2026-10-02', '周五']])
    printDays(inst, p.tag, [['2026-10-05', '周一'], ['2026-10-06', '周二'], ['2026-10-07', '周三'], ['2026-10-08', '周四'], ['2026-10-09', '周五'], ['2026-10-10', '周六'], ['2026-10-11', '周日']])

    // 假期/调休随整表下发：10/07 假期隐藏、10/10 补班按 followDate 换成目标星期的课
    const withHolidays = Object.assign({}, real, {
      holidays: [
        { date: '2026-10-07', endDate: '', name: '调休放假', type: 0, followDate: '', followWeek: -1, followWeekday: -1, custom: false },
        { date: '2026-10-10', endDate: '', name: '补班', type: 1, followDate: '2026-10-05', followWeek: 5, followWeekday: 1, custom: false }
      ]
    })
    const inst2 = await loadProject(p.src)
    const r2 = inst2.sync.handlePhoneMessage(withHolidays)
    const wed = flatOf(inst2, '2026-10-07')
    check(`${p.tag}: 带 holidays 后 10/07 假期隐藏`,
      r2.ok && wed.length === 0 && inst2.schedule.isHolidayDate(new Date(2026, 9, 7)),
      `周三=[${wed.map((c) => c.name).join(',')}]`)
    const sat = flatOf(inst2, '2026-10-10')
    check(`${p.tag}: 10/10 补班按 followDate=2026-10-05 映射到周一的课`,
      r2.ok && sat.length > 0 &&
        sat.every((s) => (real.courses || []).some((c) => c.dayOfWeek === 1 && c.name === s.name)),
      `周六=[${sat.map((c) => c.name).join(',')}]`)
  }
} else {
  console.log('\n(未找到真实课表文件，跳过真实数据检查)')
}

console.log(`\n${failures === 0 ? 'ALL CHECKS PASSED' : failures + ' CHECK(S) FAILED'}`)
process.exit(failures === 0 ? 0 : 1)
