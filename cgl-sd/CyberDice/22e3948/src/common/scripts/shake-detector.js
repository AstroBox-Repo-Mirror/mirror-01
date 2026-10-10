/**
 * ShakeDetector：摇动识别（说明书第 8、10 节）
 *
 * 原则：加速度计只负责判断"是否发生一次明确摇动"，不参与随机结果计算。
 *
 * 特征：
 *   m(t)    = sqrt(x² + y² + z²)
 *   d(t)    = |m(t) - baseline|
 *   baseline 用慢速 EMA 从真机样本自适应估计，不硬编码单位（10.2 节）。
 *
 * 首版策略（10.3 节）：
 *   1. IDLE：enterWindowMs 窗口内出现 >= enterPeakCount 个显著运动样本 -> SHAKE_START
 *   2. SHAKING：显著运动刷新 lastMotionAt
 *   3. 连续 quietMs 无显著运动 -> SHAKE_END
 *   4. 结束后的去抖（SETTLING）由 RollController 处理；若 SETTLING 期间再次出现
 *      显著运动，Detector 会重新发出 SHAKE_START，Controller 据此回到 SHAKING。
 */

const C = require('./constants.js');

const SHAKE_DEFAULTS = {
  enterWindowMs: 250,
  enterPeakCount: 3,
  enterThreshold: 3.0,
  baselineAlpha: 0.02,
  quietMs: 350,
  warmupMs: C.SHAKE.STARTUP_STABILIZE_MS,
};

function createShakeDetector(config) {
  const cfg = Object.assign({}, SHAKE_DEFAULTS, config || {});
  const EV = { SHAKE_START: 'SHAKE_START', SHAKE_END: 'SHAKE_END' };

  let state = 'IDLE'; // IDLE | SHAKING
  let baseline = null;
  let recentPeaks = []; // 窗口内显著运动样本的时间戳
  let lastMotionAt = 0;
  let warmupStartedAt = null;

  function magnitude(s) {
    return Math.sqrt(s.x * s.x + s.y * s.y + s.z * s.z);
  }

  function reset() {
    state = 'IDLE';
    baseline = null;
    recentPeaks = [];
    lastMotionAt = 0;
    warmupStartedAt = null;
  }

  function updateBaseline(m) {
    if (baseline === null) {
      baseline = m;
    } else {
      baseline = baseline + cfg.baselineAlpha * (m - baseline);
    }
  }

  /**
   * pushSample({ t, x, y, z }) -> event | null
   * event: 'SHAKE_START' | 'SHAKE_END'
   */
  function pushSample(sample) {
    const t = sample.t;
    const m = magnitude(sample);
    if (warmupStartedAt === null) {
      warmupStartedAt = t;
    }
    updateBaseline(m);

    // 订阅刚恢复时，先让 EMA 建立静止基线；防止抬腕打开应用的动作直接掷骰。
    if (t - warmupStartedAt < cfg.warmupMs) {
      return null;
    }

    const significant = Math.abs(m - baseline) > cfg.enterThreshold;

    if (state === 'IDLE') {
      if (!significant) {
        return null;
      }
      recentPeaks.push(t);
      recentPeaks = recentPeaks.filter(function (ts) {
        return t - ts <= cfg.enterWindowMs;
      });
      if (recentPeaks.length >= cfg.enterPeakCount) {
        state = 'SHAKING';
        lastMotionAt = t;
        recentPeaks = [];
        return EV.SHAKE_START;
      }
      return null;
    }

    // state === 'SHAKING'
    if (significant) {
      lastMotionAt = t;
      return null;
    }
    if (t - lastMotionAt >= cfg.quietMs) {
      state = 'IDLE';
      return EV.SHAKE_END;
    }
    return null;
  }

  return {
    pushSample: pushSample,
    reset: reset,
    getState: function () {
      return state;
    },
  };
}

module.exports = { createShakeDetector, SHAKE_DEFAULTS };
