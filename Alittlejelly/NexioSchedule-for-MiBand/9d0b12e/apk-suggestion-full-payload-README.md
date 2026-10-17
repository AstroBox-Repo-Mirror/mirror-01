# 给手机端作者的接入说明：整表推送（version=4）

补丁：`apk-suggestion-full-payload.patch`（在 NexioSchedule 仓库根目录执行
`git apply apk-suggestion-full-payload.patch` 即可，已验证 `:app:compileDebugKotlin` 编译通过）。

## 改了什么（2 个文件，+110/-8 行，无新权限、无 UI 改动）

1. **`wearable/WatchPayload.kt`**：新增 `buildFullJson(repository, context, scheduleId)`。
   一次打包完整学期：
   - `courses[]`：每门课带 `dayOfWeek / startSection / endSection / startWeek / endWeek /
     weekType(0=全周,1=单周,2=双周) / selectedWeeks / isCustomTime / customStartTime / customEndTime /
     location / teacher`（直接取自 Course 字段，未做任何换算）；
   - `settings`：`class_start_time / current_week / total_weeks / morning_sections /
     afternoon_sections / evening_sections`；
   - `times`：直接用 `getPeriodTimes("morning"/"afternoon"/"evening")` 的返回格式；
   - `holidays`：沿用现有 `buildHolidaysJson`。
   旧的 `buildWeekJson` / `buildDaysJson` 保留未动，随时可回退。

2. **`wearable/WearableScheduleSync.kt`**：`doPush` 与 `exportToWearable` 改为走
   `buildFullJson`（原来分别走 buildDaysJson / buildWeekJson）。

## 协议影响

- 包结构是新增的 `version=4` 整表形态；推送时机、通道、权限、包名签名全部不变。
- **手环端是纯被动接收端**：不在启动/onShow/重连时向 APK 发任何 request（读取频率 = 0），
  数据只在 APK 侧点「推送到手环」时下发；手环端保留 `action=request` 的手动接口未启用。
  APK 里 `onScheduleChanged`（课表变更自动推送）建议保留——这样用户改完课表手环自动跟进，
  仍不需要手环侧任何主动读取。
- 手环端（配合发布的 rpk）已实现整表解析，并按 `CourseScheduleDateBounds.calendarWeekForDate`
  同款公式（周一起算，第 1 周 = 开学日所在周的周一）自行推算学期内任意一天，
  因此手环与手机"第几周"永远一致，翻到任何日期都有完整课表。
- **注意**：旧版手环 rpk 不认识 version=4 包，会拒绝解析。请与新手环 rpk 一起上线
  （手环端已就绪并通过全部回归测试）。

## 手环端如何验证 APK 改动

1. 装上本补丁的 APK 后，设置页"导出到手环"会把同样的 JSON 落盘在
   `files/wearable/nexio-watch-schedule.json`。
2. 把这个文件发对手环端，执行：
   `node tools/check-wearable-sync.mjs nexio-watch-schedule.json`
   即可用真实数据做端到端回归（解析、周次推算、假期/调休、冷启动缓存）。
