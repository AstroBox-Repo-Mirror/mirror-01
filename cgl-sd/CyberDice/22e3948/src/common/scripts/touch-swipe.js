/**
 * TouchSwipe：基于 Vela 原生 touch 事件的方向识别。
 *
 * 不依赖模拟器是否将桌面拖拽转换为 swipe 快捷事件；仅输出单次有效方向。
 * 平台无关，不访问 UI 或系统 API。
 */

const DEFAULT_THRESHOLD = 24;

function pointOf(event, key) {
  const points = event && event[key];
  return points && points.length ? points[0] : null;
}

function createTouchSwipe(onSwipe, threshold) {
  const minDistance = threshold || DEFAULT_THRESHOLD;
  let start = null;

  function startTouch(event) {
    const point = pointOf(event, 'touches') || pointOf(event, 'changedTouches');
    start = point ? { x: point.clientX, y: point.clientY } : null;
  }

  function endTouch(event) {
    const point = pointOf(event, 'changedTouches') || pointOf(event, 'touches');
    if (!start || !point) {
      start = null;
      return null;
    }
    const dx = point.clientX - start.x;
    const dy = point.clientY - start.y;
    start = null;
    if (Math.abs(dx) < minDistance && Math.abs(dy) < minDistance) {
      return null;
    }
    let direction;
    if (Math.abs(dx) >= Math.abs(dy)) {
      direction = dx < 0 ? 'left' : 'right';
    } else {
      direction = dy < 0 ? 'up' : 'down';
    }
    if (onSwipe) onSwipe(direction);
    return direction;
  }

  return { startTouch: startTouch, endTouch: endTouch };
}

module.exports = { createTouchSwipe, DEFAULT_THRESHOLD };
