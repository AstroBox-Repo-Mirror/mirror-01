/**
 * 原型节点树 —— 只使用宿主 ui-v3 **真机确认可用**的属性。
 *
 * 这里的每个 builder 都对应 Rust 侧 `ui-core` 的一个函数签名，方便直接搬过去：
 *   div() / p() / row() / col() / gap() / pad() / bg() / fg() / size() / radius()
 *
 * 刻意没有的东西（宿主做不到，见 render.mjs 的白名单）：
 *   font-weight、letter-spacing、line-height、text-align、box-shadow、渐变、百分比宽度。
 * 所以层级只能靠 **字号落差 + 明度落差 + 留白** 建立 —— 这比 HTML 稿更依赖「大 vs 小」。
 */

const P = (props = {}) => props;
const n = (tag, props = {}, children = [], text = null) => ({ tag, props, children, text });

// ---- 设计 token（与 HTML 稿同一套色值，换的只是表达手段）----
export const C = {
  bg: '#161316',
  surface: '#211C20',
  soft: '#2C2529',
  strong: '#3A3136',
  line: 'rgba(255,255,255,0.07)',
  lineSoft: 'rgba(255,255,255,0.045)',
  text: '#F5F0F2',
  sub: '#B8ACB2',
  dim: '#8A7D94',
  accent: '#E59BB4',
  accentDeep: '#BE4E77',
  ok: '#8FBC9B',
  warn: '#D8AE6A',
  bad: '#D98A7C',
  track: 'rgba(255,255,255,0.08)',
};

// ---- 基础 ----
const div = (props = {}, ...children) => n('div', props, children.flat());
const p = (text, props = {}) => n('p', props, [], text);
const badge = (text, fg = C.sub, bg = C.soft) =>
  n('badge', { background: bg, color: fg, 'font-size': 11, radius: 6, 'padding-left': 7, 'padding-right': 7, 'padding-top': 2, 'padding-bottom': 2, 'flex-shrink': 0 }, [], text);

const row = (props = {}, ...children) =>
  div({ display: 'flex', 'flex-direction': 'row', 'align-items': 'center', ...props }, ...children);
const col = (props = {}, ...children) =>
  div({ display: 'flex', 'flex-direction': 'column', ...props }, ...children);

const pad = (v) => ({ padding: v });
const gap = (v) => ({ gap: v });
const grow = (v = 1) => ({ 'flex-grow': v });
const fixed = () => ({ 'flex-shrink': 0 });
const spacer = () => div({ 'flex-grow': 1 });
const divider = () => div({ height: 1, background: C.lineSoft, width: 'full' });

const text = (value, size = 13.5, color = C.text) => p(value, { 'font-size': size, color });
const meta = (value) => p(value, { 'font-size': 12, color: C.dim });
const sub = (value, size = 13.5) => p(value, { 'font-size': size, color: C.sub });

/** 成组列表容器：iOS 的「一个圆角面装若干行」。 */
const group = (...children) =>
  col({ background: C.surface, radius: 14, 'padding-left': 15, 'padding-right': 15, width: 'full' }, ...children);

/** 组内一行：上下 13px，行间靠 1px 分隔。右侧内容靠 spacer 推到最右。 */
const listRow = (left, right = null, props = {}) =>
  row(
    { width: 'full', 'padding-top': 13, 'padding-bottom': 13, gap: 12, ...props },
    left,
    ...(right ? [spacer(), right] : [spacer()]),
  );

/** 区块标题（HTML 稿的 .sec）。 */
const section = (title, right = null) =>
  row({ width: 'full', 'padding-left': 2, 'padding-right': 2, 'margin-top': 22, 'margin-bottom': 8 }, meta(title), spacer(), right ?? div());

/** 按钮：实色圆角，按下换底色（宿主没有 :active，靠 PointerDown 事件换色）。 */
const button = (label, opts = {}) => {
  const { kind = 'primary', height = 44, block = true, disabled = false, icon = null } = opts;
  const bg = disabled ? C.soft : kind === 'primary' ? C.accentDeep : kind === 'ghost' ? C.soft : 'rgba(0,0,0,0)';
  const fg = disabled ? C.dim : kind === 'primary' ? '#FFFFFF' : kind === 'danger' ? C.bad : C.sub;
  return n(
    'button',
    {
      background: bg,
      color: fg,
      radius: 11,
      height,
      'flex-shrink': 0,
      ...(block ? { width: 'full' } : { 'padding-left': 14, 'padding-right': 14 }),
      display: 'flex',
      'flex-direction': 'row',
      'align-items': 'center',
      'justify-content': 'center',
      gap: 6,
      ...(disabled ? { disabled: true } : {}),
    },
    icon ? [icon, p(label, { 'font-size': 13.5, color: fg })] : [p(label, { 'font-size': 13.5, color: fg })],
  );
};

/** 行内小按钮（列表右侧的「重传 / 删除 / 同步」）。 */
const chip = (label, opts = {}) => {
  const { kind = 'solid', disabled = false } = opts;
  const bg = disabled ? C.soft : kind === 'flat' ? 'rgba(0,0,0,0)' : C.soft;
  const fg = disabled ? C.dim : kind === 'danger' ? C.bad : kind === 'primary' ? '#FFFFFF' : C.sub;
  return n(
    'button',
    {
      background: kind === 'primary' && !disabled ? C.accentDeep : bg,
      color: fg,
      radius: 8,
      height: 30,
      'padding-left': 11,
      'padding-right': 11,
      'flex-shrink': 0,
      display: 'flex',
      'align-items': 'center',
      'justify-content': 'center',
      ...(disabled ? { disabled: true } : {}),
    },
    [p(label, { 'font-size': 12, color: fg })],
  );
};

/**
 * 进度条：宿主 width 只收像素，做不出「百分比宽度」。
 * 拆成 N 个等宽小块、按百分比点亮 —— 与宿主能力无关，只会粗细不同。
 */
const progressBar = (percent, opts = {}) => {
  const { segments = 24, height = 4, filled = C.accent, track = C.track } = opts;
  const lit = Math.round((Math.min(100, Math.max(0, percent)) / 100) * segments);
  return row(
    { width: 'full', gap: 2, height },
    ...Array.from({ length: segments }, (_, i) =>
      div({ 'flex-grow': 1, height, radius: height / 2, background: i < lit ? filled : track }),
    ),
  );
};

/** 分段控件（设置页的「正常 / 激进」）。 */
const segmented = (items) =>
  row(
    { background: C.soft, radius: 9, padding: 3, gap: 2, 'flex-shrink': 0 },
    ...items.map(({ label, on }) =>
      div(
        {
          radius: 7,
          'padding-left': 14,
          'padding-right': 14,
          'padding-top': 6,
          'padding-bottom': 6,
          background: on ? C.accentDeep : 'rgba(0,0,0,0)',
        },
        p(label, { 'font-size': 12.5, color: on ? '#FFFFFF' : C.dim }),
      ),
    ),
  );

/** 开关：宿主有 SWITCH 元素类型，直接用（比自己画可靠）。 */
const toggle = (on) =>
  n('switch', { width: 42, height: 25, background: on ? C.accentDeep : C.strong, 'flex-shrink': 0 }, [], on ? '1' : '0');

// ---- 图标：**风险项**。宿主有 SVG 元素类型，但内容能否渲染未验证。----
const svg = (path, color = C.dim) =>
  n('svg', { width: 20, height: 20, 'flex-shrink': 0, color }, [], path);

const ICON = {
  push: '<path d="M12 5v16"/><path d="M20 19A2 2 0 0022 17V5a2 2 0 00-2-2L16 3a5 5 0 00-4 2 5 5 0 00-4-2H4a2 2 0 00-2 2v12a2 2 0 002 2h4a5 5 0 014 2 5 5 0 014-2z"/>',
  saves: '<rect width="20" height="5" x="2" y="3" rx="1"/><path d="M4 8v11a2 2 0 002 2h12a2 2 0 002-2V8"/><path d="M10 12h4"/>',
  stats: '<path d="M3 3v16a2 2 0 002 2h16"/><path d="M18 17V9"/><path d="M13 17V5"/><path d="M8 17v-3"/>',
  account: '<path d="M19 21v-2a4 4 0 00-4-4H9a4 4 0 00-4 4v2"/><circle cx="12" cy="7" r="4"/>',
  settings: '<circle cx="12" cy="12" r="3"/><path d="M19 12a7 7 0 00-.1-1.2l2-1.5-2-3.5-2.4 1a7 7 0 00-2-1.2L14 3h-4l-.5 2.6a7 7 0 00-2 1.2l-2.4-1-2 3.5 2 1.5A7 7 0 005 12a7 7 0 00.1 1.2l-2 1.5 2 3.5 2.4-1a7 7 0 002 1.2L10 21h4l.5-2.6a7 7 0 002-1.2l2.4 1 2-3.5-2-1.5A7 7 0 0019 12z"/>',
  upload: '<path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" x2="12" y1="3" y2="15"/>',
  download: '<path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" x2="12" y1="15" y2="3"/>',
  refresh: '<path d="M3 12a9 9 0 019-9 9.75 9.75 0 016.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 01-9 9 9.75 9.75 0 01-6.74-2.74L3 16"/><path d="M8 16H3v5"/>',
  trash: '<path d="M3 6h18"/><path d="M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6"/><path d="M8 6V4a2 2 0 012-2h4a2 2 0 012 2v2"/>',
  check: '<path d="M20 6 9 17l-5-5"/>',
  logout: '<path d="M9 21H5a2 2 0 01-2-2V5a2 2 0 012-2h4"/><polyline points="16 17 21 12 16 7"/><line x1="21" x2="9" y1="12" y2="12"/>',
};

// =====================================================================
// 外壳
// =====================================================================

/** 顶栏：图标 + 产品名 + 连接状态。 */
const topBar = (device) =>
  row({ width: 'full', 'padding-left': 16, 'padding-right': 16, 'padding-top': 14, 'padding-bottom': 10, gap: 10 },
    n('image', { width: 30, height: 30, radius: 8, 'flex-shrink': 0 }, [], 'icon.png'),
    p('甜蜜女友2', { 'font-size': 16, color: C.text }),
    spacer(),
    row({ gap: 6, 'flex-shrink': 1, 'padding-left': 8, 'padding-right': 8, 'padding-top': 4, 'padding-bottom': 4, radius: 8, background: device.connected ? C.soft : 'rgba(0,0,0,0)' },
      div({ width: 6, height: 6, radius: 3, background: device.connected ? C.ok : C.dim, 'flex-shrink': 0 }),
      p(device.name, { 'font-size': 11, color: C.sub }),
    ),
  );

/** 传输状态区：外壳的一部分，切页不重建。 */
const transferStrip = (t) =>
  t
    ? div({ 'padding-left': 16, 'padding-right': 16, 'padding-bottom': 6 },
        col({ background: C.surface, radius: 14, padding: 14, gap: 10, width: 'full' },
          row({ width: 'full', gap: 8 },
            p(t.title, { 'font-size': 13.5, color: C.text }),
            spacer(),
            p(t.percent + '%', { 'font-size': 16, color: C.accent }),
          ),
          progressBar(t.percent, { segments: 28, height: 6 }),
          row({ width: 'full' },
            meta(`${t.speed} · 剩余 ${t.eta}`),
            spacer(),
            meta(t.chunk),
          ),
        ),
      )
    : div({ 'padding-left': 16, 'padding-right': 16, 'padding-bottom': 6 },
        row({ gap: 8, 'padding-top': 4, 'padding-bottom': 6 },
          div({ width: 14, height: 2, radius: 1, background: C.dim, 'flex-shrink': 0 }),
          meta('手环已连接，没有传输任务'),
        ),
      );

/**
 * 导航：HTML 稿是固定底栏，宿主**无法可靠固定**（没有 fixed 的定位方法）。
 * 所以改成顶栏下方的分段标签 —— 这是本次唯一的结构性妥协。
 */
const navBar = (active) => {
  const items = [
    ['push', '推送'],
    ['saves', '存档'],
    ['stats', '统计'],
    ['account', '账号'],
    ['settings', '设置'],
  ];
  return row({ width: 'full', gap: 2, 'padding-left': 10, 'padding-right': 10, 'padding-top': 2, 'padding-bottom': 10, background: C.bg },
    ...items.map(([key, label]) => {
      const on = key === active;
      return col({ 'flex-grow': 1, 'align-items': 'center', gap: 4, 'padding-top': 8, 'padding-bottom': 6 },
        // 选中靠「颜色 + 一条指示条」表达：没有 font-weight，不能靠加粗
        div({ width: 20, height: 3, radius: 2, background: on ? C.accent : 'rgba(0,0,0,0)' }),
        svg(ICON[key], on ? C.accent : C.dim),
        p(label, { 'font-size': 11, color: on ? C.accent : C.dim }),
      );
    }),
  );
};

const shell = (page, { device, transfer, content }) =>
  col({ width: 'full', background: C.bg, 'min-height': 860 },
    topBar(device),
    navBar(page),
    transferStrip(transfer),
    div({ 'padding-left': 16, 'padding-right': 16, 'padding-bottom': 28, 'flex-grow': 1 }, content),
  );

// =====================================================================
// 推送
// =====================================================================

const chapterRow = (ch) => {
  const state = ch.installed
    ? badge('已安装', C.ok)
    : ch.pending
      ? badge('待续传', C.warn)
      : ch.active
        ? badge('传输中', C.accent)
        : null;
  return col({ width: 'full', 'padding-top': 13, 'padding-bottom': 13, gap: 7 },
    row({ width: 'full', gap: 8 },
      p(ch.title, { 'font-size': 15, color: C.text, 'flex-grow': 1 }),
      ...(ch.note ? [badge(ch.note, C.dim, C.soft)] : []),
      ...(state ? [state] : []),
    ),
    row({ width: 'full', gap: 10 },
      meta(ch.meta),
      spacer(),
      ...(ch.installed
        ? [chip('重传', { disabled: ch.disabled }), chip('删除', { kind: 'danger', disabled: ch.disabled })]
        : [chip(ch.pending ? '继续' : '同步', { kind: 'primary', disabled: ch.disabled })]),
    ),
    ...(ch.active ? [progressBar(ch.percent ?? 0, { segments: 20, height: 3 })] : []),
  );
};

export function pushPage() {
  const chapters = [
    { title: '公共资源', note: '先同步', meta: '5.1 MB · 跨章共享场景图', installed: true },
    { title: '共通线 · 第1-5章', meta: '395 KB · 阅读约 5 小时 38 分', installed: true },
    { title: '千岁线 · 第6-8章', meta: '2.0 MB · 阅读约 11 小时 49 分', installed: true },
    { title: '玲线 · 第9-11章', meta: '2.6 MB · 阅读约 12 小时 31 分', installed: false },
  ];
  const filters = ['全部', '共通线', '千岁线', '玲线', '结灯线', '番外'];
  return col({ width: 'full', gap: 0 },
    button('打开《甜蜜女友2》', { kind: 'ghost', height: 42 }),
    // 线路筛选：外层可横滑
    n('scroll', { width: 'full', 'margin-top': 12, 'margin-bottom': 2 }, [
      row({ gap: 8, 'padding-bottom': 2 },
        ...filters.map((f, i) =>
          n('button', {
            background: i === 0 ? C.soft : C.surface,
            color: i === 0 ? C.accent : C.dim,
            radius: 9,
            'padding-left': 14, 'padding-right': 14, 'padding-top': 7, 'padding-bottom': 7,
            'flex-shrink': 0,
          }, [p(f, { 'font-size': 13, color: i === 0 ? C.accent : C.dim })]),
        ),
      ),
    ]),
    div({ 'margin-top': 10, width: 'full' },
      group(...chapters.flatMap((c, i) => (i ? [divider(), chapterRow(c)] : [chapterRow(c)]))),
    ),
    div({ 'margin-top': 14 }, button('同步剩余 1 章', { disabled: true })),
    p('剧情文本随手环应用安装，插件只推送图片。', { 'font-size': 12, color: C.dim, 'margin-top': 16 }),
    p('旧版本章节重传即可覆盖更新。', { 'font-size': 12, color: C.dim, 'margin-top': 4 }),
  );
}

// =====================================================================
// 存档
// =====================================================================

function saveRow(s) {
  return col({ width: 'full', 'padding-top': 13, 'padding-bottom': 13, gap: 5 },
    row({ width: 'full', gap: 8 },
      p(s.title, { 'font-size': 15, color: C.text }),
      ...(s.tag ? [badge(s.tag, s.tagColor ?? C.dim)] : []),
    ),
    row({ width: 'full', gap: 10 },
      meta(s.meta),
      spacer(),
      chip('读档', { disabled: s.missing }),
      chip('删除', { kind: 'danger', disabled: s.missing }),
    ),
  );
}

export function savesPage() {
  const rows = [
    { title: '共通线 · 第3章', tag: '自动', meta: '场景 42 · 10-31 06:20' },
    { title: '共通线 · 第1章', meta: '场景 12 · 10-30 03:33' },
    { title: '玲线 · 第10章', tag: '未安装', tagColor: C.warn, meta: '场景 8 · 10-29 21:40', missing: true },
  ];
  return col({ width: 'full', gap: 0 },
    row({ width: 'full', gap: 10, 'margin-top': 4 },
      button('导出到剪贴板', { kind: 'ghost', block: false, height: 41 }).props['flex-grow'] = 1,
      button('从剪贴板导入', { kind: 'ghost', block: false, height: 41 }).props['flex-grow'] = 1,
    ),
    section('手环上的存档', row({ gap: 10 }, meta('3 / 20'), chip('刷新'))),
    group(...rows.flatMap((r, i) => (i ? [divider(), saveRow(r)] : [saveRow(r)]))),
    p('存档从手环读出，导出后可以存进备忘录或聊天窗口。', { 'font-size': 12, color: C.dim, 'margin-top': 18 }),
  );
}

// =====================================================================
// 统计
// =====================================================================

export function statsPage() {
  const days = [
    ['10-31', '48 分钟', 42],
    ['10-30', '1 小时 20 分', 100],
    ['10-29', '22 分钟', 19],
    ['10-28', '—', 0],
    ['10-27', '56 分钟', 49],
  ];
  const kv = (k, v) => listRow(sub(k), p(v, { 'font-size': 13.5, color: C.text, 'flex-shrink': 0 }));
  return col({ width: 'full', gap: 0 },
    // 大数字起头：宿主没有 font-weight，只能靠 28px 撑分量
    col({ 'padding-top': 12, 'padding-bottom': 6, gap: 4 },
      p('12 小时 34 分', { 'font-size': 28, color: C.text }),
      meta('累计阅读时长'),
    ),
    div({ 'margin-top': 12, width: 'full' },
      group(
        listRow(sub('累计天数'), p('14 天', { 'font-size': 13.5, color: C.text, 'flex-shrink': 0 })),
        divider(),
        listRow(sub('今日'), p('48 分钟', { 'font-size': 13.5, color: C.text, 'flex-shrink': 0 })),
        divider(),
        listRow(sub('单日最长'), p('3 小时 12 分', { 'font-size': 13.5, color: C.text, 'flex-shrink': 0 })),
        divider(),
        listRow(sub('日均'), p('54 分钟', { 'font-size': 13.5, color: C.text, 'flex-shrink': 0 })),
      ),
    ),
    section('最近 7 天'),
    group(...days.flatMap(([d, label, pct], i) => [
      ...(i ? [divider()] : []),
      row({ width: 'full', gap: 12, 'padding-top': 11, 'padding-bottom': 11 },
        p(d, { 'font-size': 12.5, color: C.sub, width: 48, 'flex-shrink': 0 }),
        pct === 0
          ? div({ 'flex-grow': 1, height: 4, radius: 2, background: C.track })
          : div({ 'flex-grow': 1, height: 4, radius: 2, background: C.track },
              div({ height: 4, radius: 2, background: C.accent, width: `${pct}%` })),
        p(label, { 'font-size': 12, color: C.dim, width: 64, 'flex-shrink': 0 }),
      ),
    ])),
  );
}

// =====================================================================
// 账号
// =====================================================================

export function accountPage() {
  const step = (no, title, detail, state) =>
    listRow(
      row({ gap: 12, 'flex-grow': 1 },
        div({ width: 26, height: 26, radius: 13, 'flex-shrink': 0, background: state === 'done' ? 'rgba(143,188,155,0.18)' : state === 'on' ? C.accentDeep : C.soft },
          p(state === 'done' ? '✓' : String(no), { 'font-size': 12, color: state === 'done' ? C.ok : state === 'on' ? '#FFFFFF' : C.dim }),
        ),
        col({ 'flex-grow': 1, gap: 2 },
          p(title, { 'font-size': 13.5, color: C.text }),
          meta(detail),
        ),
      ),
    );

  const manage = (title, detail, danger = false) =>
    listRow(
      col({ 'flex-grow': 1, gap: 2 },
        p(title, { 'font-size': 13.5, color: danger ? C.bad : C.text }),
        meta(detail),
      ),
      p('›', { 'font-size': 16, color: C.dim }),
    );

  return col({ width: 'full', gap: 0 },
    // 账号卡
    row({ width: 'full', gap: 14, padding: 16, background: C.surface, radius: 14 },
      div({ width: 52, height: 52, radius: 26, background: C.soft, 'flex-shrink': 0, border: `2 rgba(229,155,180,0.35)`, display: 'flex', 'align-items': 'center', 'justify-content': 'center' },
        p('其', { 'font-size': 20, color: C.accent }),
      ),
      col({ 'flex-grow': 1, 'min-width': 0, gap: 3 },
        p('其核(StatTrak™) | 崭新出厂', { 'font-size': 15, color: C.text }),
        meta('QQ 3576325016 · 其核'),
      ),
      badge('已同步', C.ok),
    ),
    div({ 'margin-top': 10, width: 'full' },
      group(
        step(1, 'AstroBox 已授权', '其核', 'done'),
        divider(),
        step(2, 'QQ 已绑定', '之后会自动登录', 'done'),
      ),
    ),
    p('QQ 主账号已绑定，连接手环后自动同步', { 'font-size': 12.5, color: C.dim, 'margin-top': 12 }),
    div({ 'margin-top': 12 }, button('立即同步', { kind: 'ghost', height: 42 })),
    section('账号管理'),
    group(
      manage('刷新账号状态', '重新读取 QQ 绑定与云同步状态'),
      divider(),
      manage('忘记本机授权', '不删除云端存档和 QQ 绑定', true),
    ),
    p('QQ 是主账号，AstroBox 账号只用于确认「这台设备属于谁」。', { 'font-size': 12, color: C.dim, 'margin-top': 18 }),
  );
}

// =====================================================================
// 设置
// =====================================================================

export function settingsPage() {
  const setRow = (title, detail, control) =>
    listRow(
      col({ 'flex-grow': 1, gap: 3 },
        p(title, { 'font-size': 14, color: C.text }),
        meta(detail),
      ),
      control,
    );

  return col({ width: 'full', gap: 0 },
    section('传输'),
    group(
      setRow('分片档位', '正常档窗口 6-10 片；激进档 12 片起', segmented([{ label: '正常', on: true }, { label: '激进', on: false }])),
      divider(),
      setRow('连接后自动打开游戏', '连上手环后顺带把《甜蜜女友2》调到前台', toggle(true)),
      divider(),
      setRow('未完成缓存', '1.0 MB · 3 个文件', chip('清理')),
    ),
    section('设备'),
    group(
      setRow('Xiaomi Smart Band 10 Pro', '应用 3.2.1 · 存档协议 v1', chip('断开')),
    ),
    section('运行日志', chip('全部')),
    group(
      col({ width: 'full', gap: 6, 'padding-top': 10, 'padding-bottom': 10 },
        p('INFO band capability reported version=3.2.1', { 'font-size': 11.5, color: C.dim }),
        divider(),
        p('WARN heartbeat missed misses=1', { 'font-size': 11.5, color: C.warn }),
      ),
    ),
    section('关于'),
    group(
      listRow(sub('插件版本'), p('4.2.0', { 'font-size': 13, color: C.dim, 'flex-shrink': 0 })),
      divider(),
      listRow(sub('项目主页'), p('miband.qihe0507.top', { 'font-size': 13, color: C.accent, 'flex-shrink': 0 })),
      divider(),
      listRow(sub('作者'), p('其核', { 'font-size': 13, color: C.dim, 'flex-shrink': 0 })),
    ),
  );
}

// =====================================================================

export const PAGES = {
  push: pushPage,
  saves: savesPage,
  stats: statsPage,
  account: accountPage,
  settings: settingsPage,
};

export const DEVICE = { name: 'Smart Band 10 Pro', connected: true };

export const TRANSFER = {
  title: '千岁线 · 第6-8章',
  percent: 62,
  speed: '138 KB/s',
  eta: '约 8 秒',
  chunk: '16 KB',
};

export { shell };
