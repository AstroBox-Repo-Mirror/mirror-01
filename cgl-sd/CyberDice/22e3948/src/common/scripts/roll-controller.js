/**
 * RollController：掷骰流程与状态机（说明书第 8、9 节）
 *
 * READY -> SHAKING -> SETTLING -> RESULT -> READY
 *
 * - trigger('tap') / onShakeStart() 两种来源共用同一结果流程（FR-005/008）
 * - 防重入：SHAKING / SETTLING / RESULT 期间忽略一切新触发（冷却防重复，AC-08）
 * - SETTLING 期间重新出现摇动（SHAKE_START）则回到 SHAKING（10.3 节第 4 步）
 * - 结果只在 RESULT 进入时生成一次；动画中间值与随机抽样无关（第 11 节）
 *
 * 时间与定时器均可注入，便于单元测试。
 */

const C = require('./constants.js');

const DEFAULTS = {
  tapRollMs: C.ROLL.TAP_ROLL_MS,
  shakeMinRollMs: C.ROLL.SHAKE_MIN_ROLL_MS,
  shakeMaxRollMs: C.ROLL.SHAKE_MAX_ROLL_MS,
  feedbackDelayMs: C.ROLL.FEEDBACK_DELAY_MS,
  settlingMs: C.SHAKE.SETTLING_MS,
  resultMs: C.ROLL.RESULT_MS,
};

function createRollController(opts) {
  const cfg = Object.assign({}, DEFAULTS, opts && opts.config ? opts.config : {});
  const diceEngine = opts.diceEngine;
  const feedback = opts.feedback || { vibrate: function () {} };
  const now = opts.now || Date.now;
  const setTimeoutFn = opts.setTimeout || setTimeout;
  const clearTimeoutFn = opts.clearTimeout || clearTimeout;

  const onStateChange = opts.onStateChange || function () {};
  const onResult = opts.onResult || function () {};

  let state = C.STATE_READY;
  let mode = (opts && opts.mode) || C.MODE_1D6;
  let source = null; // 'shake' | 'tap'
  let lastResult = null;
  let rollStartedAt = 0;
  let settleTimer = null;
  let resultTimer = null;
  let tapTimer = null;
  let maxRollTimer = null;
  let feedbackTimer = null;
  let shakeActive = false;
  let waitForQuiet = false;

  function transition(next, ctx) {
    state = next;
    onStateChange(next, ctx || {});
  }

  function clearTimers() {
    if (maxRollTimer !== null) {
      clearTimeoutFn(maxRollTimer);
      maxRollTimer = null;
    }
    if (settleTimer) {
      clearTimeoutFn(settleTimer);
      settleTimer = null;
    }
    if (resultTimer) {
      clearTimeoutFn(resultTimer);
      resultTimer = null;
    }
    if (tapTimer) {
      clearTimeoutFn(tapTimer);
      tapTimer = null;
    }
    if (feedbackTimer !== null) {
      clearTimeoutFn(feedbackTimer);
      feedbackTimer = null;
    }
  }

  function isBusy() {
    return state !== C.STATE_READY;
  }

  function startRoll(src) {
    if (isBusy() || (src === 'shake' && waitForQuiet)) {
      return false; // 防重入 / 冷却
    }
    source = src;
    rollStartedAt = now();
    transition(C.STATE_SHAKING, { source: src });
    if (src === 'shake') {
      shakeActive = true;
      maxRollTimer = setTimeoutFn(function () {
        maxRollTimer = null;
        waitForQuiet = shakeActive;
        finishRoll({ forced: true });
      }, cfg.shakeMaxRollMs);
    }
    if (src === 'tap') {
      // 点击触发：固定动画时长后自动收束
      tapTimer = setTimeoutFn(function () {
        tapTimer = null;
        onTapRollDone();
      }, cfg.tapRollMs);
    }
    return true;
  }

  function scheduleSettle(delayMs) {
    if (settleTimer) {
      return;
    }
    settleTimer = setTimeoutFn(function () {
      settleTimer = null;
      if (state !== C.STATE_SETTLING) {
        return;
      }
      finishRoll();
    }, delayMs);
  }

  function finishRoll(reason) {
    if (state !== C.STATE_SHAKING && state !== C.STATE_SETTLING) {
      return;
    }
    clearTimers();
    const result = diceEngine.roll(mode); // 最终结果只生成一次
    lastResult = result;
    const forced = Boolean(reason && reason.forced);
    transition(C.STATE_RESULT, { result: result, source: source, forced: forced });
    onResult(result, source);

    // 强制结算发生在持续的高频加速度计回调中。让出一个短帧间隔，先提交
    // RESULT 视觉状态，再请求震动；否则部分真机可能显示结果但丢失触觉请求。
    feedbackTimer = setTimeoutFn(function () {
      feedbackTimer = null;
      if (state !== C.STATE_RESULT) {
        return;
      }
      try {
        // 到达持续摇腕上限意味着用户仍在明显运动，使用官方 long 模式保证
        // 触觉提醒可感知；普通停稳与点击保持 short 模式。
        feedback.vibrate(forced ? 'long' : 'short');
      } catch (e) {
        // 震动失败吞掉异常，不阻塞 UI（第 14 节）
      }
    }, cfg.feedbackDelayMs);

    resultTimer = setTimeoutFn(function () {
      resultTimer = null;
      source = null;
      transition(C.STATE_READY, {});
    }, cfg.resultMs);
  }

  /** 点击主区域触发（FR-008） */
  function trigger(src) {
    if (src !== 'tap' && src !== 'shake') {
      return false;
    }
    return startRoll(src);
  }

  /** Detector 发出 SHAKE_START */
  function onShakeStart() {
    shakeActive = true;
    if (waitForQuiet) {
      return false;
    }
    if (state === C.STATE_SETTLING && source === 'shake') {
      // SETTLING 去抖期间重新出现强运动 -> 回到 SHAKING
      if (settleTimer) {
        clearTimeoutFn(settleTimer);
        settleTimer = null;
      }
      transition(C.STATE_SHAKING, { source: 'shake' });
      return true;
    }
    return startRoll('shake');
  }

  /** Detector 发出 SHAKE_END */
  function onShakeEnd() {
    shakeActive = false;
    waitForQuiet = false;
    if (state !== C.STATE_SHAKING || source !== 'shake') {
      return false;
    }
    transition(C.STATE_SETTLING, { source: 'shake' });
    // 保证最短动画时长
    const elapsed = now() - rollStartedAt;
    const delay = Math.max(cfg.settlingMs, cfg.shakeMinRollMs - elapsed);
    scheduleSettle(delay);
    return true;
  }

  /** 点击触发的自动收束 */
  function onTapRollDone() {
    if (state !== C.STATE_SHAKING || source !== 'tap') {
      return false;
    }
    transition(C.STATE_SETTLING, { source: 'tap' });
    scheduleSettle(cfg.settlingMs);
    return true;
  }

  function setMode(next) {
    if (isBusy()) {
      return false; // 摇动/结算过程中禁止切换（7.3 节）
    }
    mode = next;
    return true;
  }

  function getMode() {
    return mode;
  }

  function getState() {
    return state;
  }

  function getLastResult() {
    return lastResult;
  }

  /** 页面离开前取消未完成流程，避免后台产生结果、历史记录或震动。 */
  function cancel() {
    const wasBusy = isBusy();
    clearTimers();
    source = null;
    shakeActive = false;
    waitForQuiet = false;
    if (state !== C.STATE_READY) {
      transition(C.STATE_READY, { cancelled: true });
    }
    return wasBusy;
  }

  /** 页面销毁时清理定时器，避免幽灵回调 */
  function destroy() {
    clearTimers();
    shakeActive = false;
    waitForQuiet = false;
    state = C.STATE_READY;
  }

  return {
    trigger: trigger,
    onShakeStart: onShakeStart,
    onShakeEnd: onShakeEnd,
    onTapRollDone: onTapRollDone,
    setMode: setMode,
    getMode: getMode,
    getState: getState,
    getLastResult: getLastResult,
    isBusy: isBusy,
    cancel: cancel,
    destroy: destroy,
  };
}

module.exports = { createRollController };
