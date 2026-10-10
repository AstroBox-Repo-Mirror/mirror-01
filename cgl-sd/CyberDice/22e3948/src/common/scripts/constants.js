/**
 * CyberDice 全局常量
 * 数值为首版默认值，真机调参阶段可按第 10.3 节方法调整。
 */

module.exports = {
  VERSION: '1.2.0',

  // 掷骰模式
  MODE_1D6: '1D6',
  MODE_2D6: '2D6',
  MODE_1D20: '1D20',
  MODES: ['1D6', '2D6', '1D20'],

  // 状态机（见说明书第 9 节）
  STATE_READY: 'READY',
  STATE_SHAKING: 'SHAKING',
  STATE_SETTLING: 'SETTLING',
  STATE_RESULT: 'RESULT',
  STATE_ERROR_SENSOR: 'ERROR_SENSOR',

  // 摇动识别（见说明书第 10.3 节）
  SHAKE: {
    // 进入 SHAKING：250ms 窗口内至少 N 个显著运动样本
    ENTER_WINDOW_MS: 250,
    ENTER_PEAK_COUNT: 3,
    // 显著运动判定：偏离基线的幅度阈值（设备输出单位待真机标定）
    ENTER_THRESHOLD: 3.0,
    // 静止基线 EMA 系数（越慢越稳定）
    BASELINE_ALPHA: 0.02,
    // 连续该时长无显著运动则判定摇动结束
    QUIET_MS: 350,
    // SETTLING 去抖时长
    SETTLING_MS: 200,
    // 页面恢复订阅后的基线稳定窗口；期间仍允许点击掷骰，但忽略摇动触发
    STARTUP_STABILIZE_MS: 600,
  },

  // 掷骰流程
  ROLL: {
    // 点击触发的动画时长（0.4~0.9s 区间内）
    TAP_ROLL_MS: 500,
    // 摇动触发的最短动画时长
    SHAKE_MIN_ROLL_MS: 400,
    // 从首次摇动触发起计时，重新摇动不延长上限；避免持续甩腕造成过长动画
    SHAKE_MAX_ROLL_MS: 1200,
    // 结果帧提交后再请求触觉反馈，避免持续加速度计回调期间丢失震动请求。
    FEEDBACK_DELAY_MS: 40,
    // 结果展示时长：保证结果稳定显示 >= 1s，同时覆盖 500~800ms 冷却
    RESULT_MS: 1000,
  },

  // 动画：中间骰面刷新间隔
  ANIM_INTERVAL_MS: 80,

  // 传感器
  SENSOR: {
    INTERVAL: 'game', // 约 20ms/次
  },
};
