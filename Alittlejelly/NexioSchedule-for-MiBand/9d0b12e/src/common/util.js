/**
 * 通用工具：日期与倒计时文案
 */

const WEEKDAYS = ['周日', '周一', '周二', '周三', '周四', '周五', '周六']

function pad2(n) {
  return n < 10 ? '0' + n : '' + n
}

function getWeekday(date) {
  return WEEKDAYS[date.getDay()]
}

function formatDate(date) {
  return date.getFullYear() + '年' + (date.getMonth() + 1) + '月' + date.getDate() + '日'
}

/** 首页头部用的短日期（设计稿：9月30日） */
function formatShortDate(date) {
  return date.getMonth() + 1 + '月' + date.getDate() + '日'
}

function formatClock(date) {
  return pad2(date.getHours()) + ':' + pad2(date.getMinutes())
}

function isSameDay(a, b) {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  )
}

function startOfDay(date) {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate())
}

function addDays(date, n) {
  const d = startOfDay(date)
  d.setDate(d.getDate() + n)
  return d
}

/**
 * 将 "HH:mm" 解析为当天的 Date
 */
function parseTimeToday(timeStr, baseDate) {
  const base = baseDate || new Date()
  const parts = timeStr.split(':')
  const d = new Date(base.getFullYear(), base.getMonth(), base.getDate())
  d.setHours(parseInt(parts[0], 10) || 0, parseInt(parts[1], 10) || 0, 0, 0)
  return d
}

/**
 * 倒计时文案：N小时M分钟后 / N分钟后 / 已开始 / 已结束
 */
function formatCountdown(target, now) {
  const diff = target.getTime() - now.getTime()
  if (diff <= 0) {
    return {
      text: '已开始',
      ended: true,
      upcoming: false
    }
  }
  const totalMin = Math.floor(diff / 60000)
  const hours = Math.floor(totalMin / 60)
  const mins = totalMin % 60
  let text
  // 文案对齐设计稿：「还有 3 分钟」
  if (hours > 0 && mins > 0) {
    text = '还有 ' + hours + ' 小时 ' + mins + ' 分钟'
  } else if (hours > 0) {
    text = '还有 ' + hours + ' 小时'
  } else if (mins > 0) {
    text = '还有 ' + mins + ' 分钟'
  } else {
    text = '马上开始'
  }
  return {
    text: text,
    ended: false,
    upcoming: true
  }
}

/**
 * 距下课剩余文案：还剩 N 分钟 / 还剩 N小时M分钟（上课中用）
 */
function formatRemain(target, now) {
  const diff = target.getTime() - now.getTime()
  if (diff <= 0) {
    return {
      text: '即将下课',
      ended: true,
      upcoming: false
    }
  }
  const totalMin = Math.ceil(diff / 60000)
  const hours = Math.floor(totalMin / 60)
  const mins = totalMin % 60
  let body
  if (hours > 0 && mins > 0) {
    body = hours + ' 小时 ' + mins + ' 分钟'
  } else if (hours > 0) {
    body = hours + ' 小时'
  } else {
    body = totalMin + ' 分钟'
  }
  return {
    text: '还有 ' + body,
    ended: false,
    upcoming: true
  }
}

/**
 * 课程状态：未开始 / 进行中 / 已结束
 * dayDate 为课程所在日期，now 为真实当前时刻（可跨日查看）
 */
function getCourseStatus(course, now, dayDate) {
  const base = dayDate || now
  const start = parseTimeToday(course.startTime, base)
  const end = parseTimeToday(course.endTime, base)
  if (now.getTime() < start.getTime()) {
    return {
      status: '未开始',
      countdown: formatCountdown(start, now)
    }
  }
  if (now.getTime() < end.getTime()) {
    return {
      status: '进行中',
      countdown: {
        text: '上课中',
        ended: true,
        upcoming: false
      }
    }
  }
  return {
    status: '已结束',
    countdown: {
      text: '已结束',
      ended: true,
      upcoming: false
    }
  }
}

export default {
  getWeekday,
  formatDate,
  formatShortDate,
  formatClock,
  isSameDay,
  startOfDay,
  addDays,
  parseTimeToday,
  formatCountdown,
  formatRemain,
  getCourseStatus
}
