/**
 * DiceEngine：随机结果生成（说明书第 8、11 节 + UI 概念图扩展）
 * 传感器只决定"何时掷"，点数完全由随机函数生成，不引入摇动强度/方向偏置（NFR-07）。
 * N 骰必须逐颗独立生成，不得直接生成总和范围随机数（避免概率分布错误）。
 */

const PRESET = {
  '1D6': { count: 1, sides: 6 },
  '2D6': { count: 2, sides: 6 },
  '3D6': { count: 3, sides: 6 },
  '1D20': { count: 1, sides: 20 },
};

function randInt(min, max) {
  // [min, max] 闭区间均匀整数
  return min + Math.floor(Math.random() * (max - min + 1));
}

/**
 * 规范化模式：'1D6'/'2D6'/'3D6'/'1D20' 预设，或 { count, sides } 自定义。
 * 返回 { count, sides, label }；非法输入兜底为 1D6，不阻塞 UI（FR-010）。
 */
function normalizeMode(mode) {
  if (typeof mode === 'string' && PRESET[mode]) {
    return Object.assign({ label: mode }, PRESET[mode]);
  }
  if (mode && typeof mode === 'object') {
    const count = Math.min(6, Math.max(1, Math.floor(Number(mode.count) || 1)));
    const sides = Math.min(100, Math.max(2, Math.floor(Number(mode.sides) || 6)));
    return { count: count, sides: sides, label: count + 'D' + sides };
  }
  return { count: 1, sides: 6, label: '1D6' };
}

function modeLabel(mode) {
  return normalizeMode(mode).label;
}

/**
 * roll(mode) -> { dice: number[], total: number }
 * @param {string|{count:number,sides:number}} mode
 */
function roll(mode) {
  const m = normalizeMode(mode);
  const dice = [];
  for (let i = 0; i < m.count; i++) {
    dice.push(randInt(1, m.sides));
  }
  const total = dice.reduce(function (a, b) { return a + b; }, 0);
  return { dice: dice, total: total };
}

/** 供动画使用的中间随机骰面（不是最终抽样） */
function rollAnimFace(mode) {
  return roll(mode).dice;
}

module.exports = { roll, rollAnimFace, randInt, normalizeMode, modeLabel, PRESET };
