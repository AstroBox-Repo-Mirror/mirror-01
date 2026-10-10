/**
 * StorageAdapter：@system.storage 封装（平台适配层）
 * - 存储不可用时退化为内存 Map，应用功能不受阻（FR-010 原则）
 * - 全部异步回调，异常吞掉
 */

const memory = new Map();

function decode(value) {
  if (typeof value !== 'string') {
    return value;
  }
  try {
    return JSON.parse(value);
  } catch (e) {
    // 兼容旧版本留下的纯字符串值。
    return value;
  }
}

function encode(value) {
  return JSON.stringify(value);
}

function defaultStorageModule() {
  try {
    // eslint-disable-next-line import/no-unresolved
    return require('@system.storage');
  } catch (e) {
    return null;
  }
}

function createStorageAdapter(storageModule) {
  const storage = storageModule || defaultStorageModule();

  function get(key, cb) {
    cb = cb || function () {};
    try {
      if (!storage || typeof storage.get !== 'function') {
        cb(memory.has(key) ? memory.get(key) : null);
        return;
      }
      storage.get({
        key: key,
        success: function (data) {
          // Vela storage 只保存字符串；在适配层还原业务数据。
          cb(data === undefined || data === null ? null : decode(data));
        },
        fail: function () {
          cb(memory.has(key) ? memory.get(key) : null);
        },
      });
    } catch (e) {
      cb(memory.has(key) ? memory.get(key) : null);
    }
  }

  function set(key, value, cb) {
    cb = cb || function () {};
    memory.set(key, value); // 内存始终同步一份
    try {
      if (!storage || typeof storage.set !== 'function') {
        cb(true);
        return;
      }
      storage.set({
        key: key,
        value: encode(value),
        success: function () { cb(true); },
        fail: function () { cb(false); },
      });
    } catch (e) {
      cb(false);
    }
  }

  return { get: get, set: set };
}

module.exports = { createStorageAdapter };
