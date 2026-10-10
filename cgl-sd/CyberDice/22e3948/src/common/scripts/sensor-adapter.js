/**
 * SensorAdapter：@system.sensor 封装（说明书第 8 节 Platform Adapter）
 *
 * - 仅前台页面生命周期内 start()；onHide/onDestroy 必须 stop()（FR-009 / NFR-05）
 * - start() 幂等：已订阅时不再重复订阅（AC-13）
 * - 订阅失败回调 onError，页面进入点击兜底模式（FR-010 / AC-12）
 *
 * sensorModule 可注入用于单元测试；默认在快应用环境中 require('@system.sensor')。
 */

function defaultSensorModule() {
  try {
    // eslint-disable-next-line import/no-unresolved
    return require('@system.sensor');
  } catch (e) {
    return null;
  }
}

function createSensorAdapter(sensorModule) {
  const sensor = sensorModule || defaultSensorModule();
  let subscribed = false;

  /**
   * start(onSample, onError)
   * onSample: ({ t, x, y, z })
   * onError:  ({ code, message })
   * 返回 true 表示成功发起订阅。
   */
  function start(onSample, onError) {
    if (subscribed) {
      return false; // 无重复订阅
    }
    if (!sensor || typeof sensor.subscribeAccelerometer !== 'function') {
      if (onError) {
        onError({ code: -1, message: 'sensor API unavailable' });
      }
      return false;
    }
    let syncFailed = false;
    try {
      sensor.subscribeAccelerometer({
        interval: 'game',
        // Vela 的加速度计是持续订阅；数据回调字段为 callback，而非一次性 API 的 success。
        callback: function (data) {
          if (!subscribed) {
            return;
          }
          onSample({
            t: Date.now(),
            x: data.x,
            y: data.y,
            z: data.z,
          });
        },
        fail: function (err) {
          subscribed = false;
          syncFailed = true;
          if (onError) {
            onError({ code: err && err.code, message: 'subscribeAccelerometer fail' });
          }
        },
      });
      subscribed = !syncFailed;
      return subscribed;
    } catch (e) {
      subscribed = false;
      if (onError) {
        onError({ code: -2, message: String(e) });
      }
      return false;
    }
  }

  /** stop()：取消订阅；未订阅时为安全 no-op */
  function stop() {
    if (!subscribed) {
      return;
    }
    subscribed = false;
    try {
      if (sensor && typeof sensor.unsubscribeAccelerometer === 'function') {
        sensor.unsubscribeAccelerometer();
      }
    } catch (e) {
      // 忽略取消订阅异常，避免阻塞页面销毁
    }
  }

  function isSubscribed() {
    return subscribed;
  }

  return { start: start, stop: stop, isSubscribed: isSubscribed };
}

module.exports = { createSensorAdapter };
