/**
 * FeedbackService：震动反馈封装（说明书第 8、14 节）
 * - 结果锁定时短震一次（FR-007）
 * - 平台异常必须吞掉，只损失触觉反馈，不影响结果显示（AC-12）
 */

function defaultVibratorModule() {
  try {
    // eslint-disable-next-line import/no-unresolved
    return require('@system.vibrator');
  } catch (e) {
    return null;
  }
}

function createFeedbackService(vibratorModule) {
  const vibrator = vibratorModule || defaultVibratorModule();
  let lastVibrateAt = 0;

  /** 结果锁定时的单次短震；调用方保证每次结果只调用一次 */
  function vibrate(mode) {
    const vibrationMode = mode === 'long' ? 'long' : 'short';
    try {
      if (!vibrator || typeof vibrator.vibrate !== 'function') {
        return false;
      }
      // 基础 vibrate；不依赖 start/stop 高级接口（第 6 节）
      vibrator.vibrate({ mode: vibrationMode });
      lastVibrateAt = Date.now();
      return true;
    } catch (e) {
      try {
        // 某些固件不支持 mode 参数，退回无参调用
        if (vibrator && typeof vibrator.vibrate === 'function') {
          vibrator.vibrate();
          lastVibrateAt = Date.now();
          return true;
        }
      } catch (e2) {
        // 完全失败：吞掉异常
      }
      return false;
    }
  }

  return {
    vibrate: vibrate,
    getLastVibrateAt: function () {
      return lastVibrateAt;
    },
  };
}

module.exports = { createFeedbackService };
