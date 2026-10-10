/**
 * store：应用级共享状态（跨页面）
 * - settings：震动反馈 / 动画效果 / 主题强调色
 * - mode：当前骰子模式（预设字符串或 {count,sides}）
 * - history：最近投掷记录（最多 20 条）
 * 通过 storage-adapter 持久化；页面通过 on() 订阅变更。
 */

const diceEngine = require('./dice-engine.js');

const ACCENTS = {
  blue: '#2f6bed',
  green: '#2fbf6b',
  purple: '#8a5cf6',
};

const HISTORY_KEY = 'cyberdice_history';
const SETTINGS_KEY = 'cyberdice_settings';
const MODE_KEY = 'cyberdice_mode';
const MAX_HISTORY = 20;

function createStore(storageAdapter) {
  const storage = storageAdapter;
  const listeners = [];

  const state = {
    settings: { vibration: true, animation: true, accent: 'blue' },
    mode: '1D6',
    history: [], // { label, dice, total, time: 'HH:MM' }
    rollState: 'READY', // READY | SHAKING | SETTLING | RESULT
    rollCtx: null,
    loaded: false,
  };

  function notify(what) {
    listeners.forEach(function (fn) {
      try {
        fn(what, state);
      } catch (e) {
        // 单个订阅者异常不影响其他
      }
    });
  }

  function on(fn) {
    listeners.push(fn);
    return function off() {
      const i = listeners.indexOf(fn);
      if (i >= 0) listeners.splice(i, 1);
    };
  }

  function currentTimeHM() {
    const d = new Date();
    return ('0' + d.getHours()).slice(-2) + ':' + ('0' + d.getMinutes()).slice(-2);
  }

  /** 启动时从持久层恢复 */
  function load(done) {
    done = done || function () {};
    let pending = 3;
    const fin = function () {
      state.loaded = true;
      notify('load');
      done(state);
    };
    const dec = function () {
      if (--pending === 0) fin();
    };
    storage.get(SETTINGS_KEY, function (v) {
      if (v && typeof v === 'object') {
        state.settings = {
          vibration: v.vibration !== false,
          animation: v.animation !== false,
          accent: ACCENTS[v.accent] ? v.accent : 'blue',
        };
      }
      dec();
    });
    storage.get(HISTORY_KEY, function (v) {
      if (Array.isArray(v)) state.history = v.slice(0, MAX_HISTORY);
      dec();
    });
    storage.get(MODE_KEY, function (v) {
      if (v) state.mode = v; // 字符串预设或 {count,sides}
      dec();
    });
  }

  function setSetting(key, value) {
    state.settings[key] = value;
    storage.set(SETTINGS_KEY, state.settings);
    notify('settings');
  }

  function setMode(mode) {
    state.mode = diceEngine.normalizeMode(mode).label ? mode : '1D6';
    storage.set(MODE_KEY, mode);
    notify('mode');
  }

  /** 应用级 RollController 状态广播，home 页订阅渲染 */
  function setRollState(s, ctx) {
    state.rollState = s;
    state.rollCtx = ctx || null;
    notify('rollState');
  }

  function getModeLabel() {
    return diceEngine.modeLabel(state.mode);
  }

  /** 每次结果锁定时调用；新记录插到最前 */
  function pushHistory(result, mode) {
    state.history.unshift({
      label: diceEngine.modeLabel(mode),
      dice: result.dice,
      total: result.total,
      time: currentTimeHM(),
    });
    if (state.history.length > MAX_HISTORY) {
      state.history.length = MAX_HISTORY;
    }
    storage.set(HISTORY_KEY, state.history);
    notify('history');
  }

  function clearHistory() {
    state.history = [];
    storage.set(HISTORY_KEY, state.history);
    notify('history');
  }

  return {
    on: on,
    load: load,
    setSetting: setSetting,
    setMode: setMode,
    setRollState: setRollState,
    getModeLabel: getModeLabel,
    pushHistory: pushHistory,
    clearHistory: clearHistory,
    getState: function () { return state; },
    ACCENTS: ACCENTS,
  };
}

module.exports = { createStore, ACCENTS, MAX_HISTORY };
