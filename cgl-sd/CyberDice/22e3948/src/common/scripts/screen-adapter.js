/**
 * ScreenAdapter：屏幕常亮平台适配层。
 *
 * 仅首页前台摇骰期间使用，避免息屏使应用进入后台并停止加速度计回调。
 * 平台 API 不可用或调用失败时全部降级为 no-op，不阻塞点击掷骰（FR-010）。
 */

function defaultBrightnessModule() {
  try {
    // eslint-disable-next-line import/no-unresolved
    return require('@system.brightness');
  } catch (e) {
    return null;
  }
}

function createScreenAdapter(brightnessModule) {
  const brightness = brightnessModule || defaultBrightnessModule();
  let keepingScreenOn = false;

  function setKeepScreenOn(enabled) {
    if (!brightness || typeof brightness.setKeepScreenOn !== 'function') {
      keepingScreenOn = false;
      return false;
    }
    try {
      brightness.setKeepScreenOn({
        keepScreenOn: !!enabled,
        fail: function () {
          keepingScreenOn = false;
        },
      });
      keepingScreenOn = !!enabled;
      return true;
    } catch (e) {
      keepingScreenOn = false;
      return false;
    }
  }

  return {
    keepOn: function () { return setKeepScreenOn(true); },
    release: function () { return setKeepScreenOn(false); },
    isKeepingScreenOn: function () { return keepingScreenOn; },
  };
}

module.exports = { createScreenAdapter };
