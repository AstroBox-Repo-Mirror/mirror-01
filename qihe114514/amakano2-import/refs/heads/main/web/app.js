/* 甜蜜女友2 导入器 · HTML 前端
 *
 * 数据流：
 *   store ← 轮询 GET /api/state（后端 snapshot() 的 JSON）
 *   act() → POST /api/action，body 是 action 字符串（与元素树 event_id 同一套词汇）
 * 后端 handle_action 执行后状态变更，下一轮轮询把新状态带回来。前端不存业务真相。
 *
 * 本地只管「界面态」：当前 tab（含后端没有的账号页）、弹层、toast。
 */

// ---------- 内联 Lucide 图标 ----------
const ICON_PATHS = {
  'book-open':'<path d="M12 5v16"/><path d="M20.001 19A2 2 0 0022 17V5a2 2 0 00-1.999-2L16 3.002A5 5 0 0012 5a5 5 0 00-4-2H4a2 2 0 00-2 2v12a2 2 0 001.999 2H8a5 5 0 014 2 5 5 0 014-2z"/>',
  'archive':'<rect width="20" height="5" x="2" y="3" rx="1"/><path d="M4 8v11a2 2 0 002 2h12a2 2 0 002-2V8"/><path d="M10 12h4"/>',
  'bar-chart-3':'<path d="M3 3v16a2 2 0 002 2h16"/><path d="M18 17V9"/><path d="M13 17V5"/><path d="M8 17v-3"/>',
  'settings':'<path d="M12.22 2h-.44a2 2 0 00-2 2v.18a2 2 0 01-1 1.73l-.43.25a2 2 0 01-2 0l-.15-.08a2 2 0 00-2.73.73l-.22.38a2 2 0 00.73 2.73l.15.1a2 2 0 011 1.72v.51a2 2 0 01-1 1.74l-.15.09a2 2 0 00-.73 2.73l.22.38a2 2 0 002.73.73l.15-.08a2 2 0 012 0l.43.25a2 2 0 011 1.73V20a2 2 0 002 2h.44a2 2 0 002-2v-.18a2 2 0 011-1.73l.43-.25a2 2 0 012 0l.15.08a2 2 0 002.73-.73l.22-.39a2 2 0 00-.73-2.73l-.15-.08a2 2 0 01-1-1.74v-.5a2 2 0 011-1.74l.15-.09a2 2 0 00.73-2.73l-.22-.38a2 2 0 00-2.73-.73l-.15.08a2 2 0 01-2 0l-.43-.25a2 2 0 01-1-1.73V4a2 2 0 00-2-2z"/><circle cx="12" cy="12" r="3"/>',
  'user':'<path d="M19 21v-2a4 4 0 00-4-4H9a4 4 0 00-4 4v2"/><circle cx="12" cy="7" r="4"/>',
  'download':'<path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" x2="12" y1="15" y2="3"/>',
  'upload':'<path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" x2="12" y1="3" y2="15"/>',
  'trash-2':'<path d="M3 6h18"/><path d="M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6"/><path d="M8 6V4a2 2 0 012-2h4a2 2 0 012 2v2"/><line x1="10" x2="10" y1="11" y2="17"/><line x1="14" x2="14" y1="11" y2="17"/>',
  'play':'<polygon points="6 3 20 12 6 21 6 3"/>',
  'pause':'<rect x="14" y="4" width="4" height="16" rx="1"/><rect x="6" y="4" width="4" height="16" rx="1"/>',
  'check':'<path d="M20 6 9 17l-5-5"/>',
  'refresh-cw':'<path d="M3 12a9 9 0 019-9 9.75 9.75 0 016.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 01-9 9 9.75 9.75 0 01-6.74-2.74L3 16"/><path d="M8 16H3v5"/>',
  'link':'<path d="M10 13a5 5 0 007.54.54l3-3a5 5 0 00-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 00-7.54-.54l-3 3a5 5 0 007.07 7.07l1.71-1.71"/>',
  'log-in':'<path d="M15 3h4a2 2 0 012 2v14a2 2 0 01-2 2h-4"/><polyline points="10 17 15 12 10 7"/><line x1="15" x2="3" y1="12" y2="12"/>',
  'log-out':'<path d="M9 21H5a2 2 0 01-2-2V5a2 2 0 012-2h4"/><polyline points="16 17 21 12 16 7"/><line x1="21" x2="9" y1="12" y2="12"/>',
  'copy':'<rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>',
  'cloud':'<path d="M17.5 19H9a7 7 0 116.71-9h1.79a4.5 4.5 0 110 9Z"/>',
  'chevron-right':'<path d="m9 18 6-6-6-6"/>',
  'chevron-down':'<path d="m6 9 6 6 6-6"/>',
  'external-link':'<path d="M15 3h6v6"/><path d="M10 14 21 3"/><path d="M18 13v6a2 2 0 01-2 2H5a2 2 0 01-2-2V8a2 2 0 012-2h6"/>',
};
function icon(n, c){ return `<span class="ic ${c||''}"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round">${ICON_PATHS[n]||''}</svg></span>`; }

// ---------- 工具 ----------
const $ = s => document.querySelector(s);
const esc = s => String(s ?? '').replace(/[&<>"]/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
/* 往 HTML 属性里的 `onclick="f(...)"` 塞字符串参数时用它，别用 `'${esc(x)}'`。
 * 章节标题来自章节包，里面一旦出现单引号或反斜杠，`onclick="…'it's'…"` 就是语法错误 ——
 * 按钮当场失效，控制台只留一句看不懂的 SyntaxError。`JSON.stringify` 给出的双引号
 * 恰好和 HTML 属性的双引号冲突，所以再把 `"` 转义成 `&quot;`（浏览器解析属性值时会还原）。
 * 单引号不用管：属性用双引号定界、JSON 字符串也用双引号定界，`'` 在两个层面都是普通字符。 */
const jsArg = v => esc(JSON.stringify(String(v ?? '')));
const fmtSize = b => b >= 1048576 ? (b/1048576).toFixed(1)+' MB' : Math.max(0, Math.round(b/1024))+' KB';
const fmtMin = m => m >= 60 ? Math.floor(m/60)+' 小时'+(m%60 ? ' '+(m%60)+' 分' : '') : m+' 分钟';
let toastTimer;
function toast(m){ const t = $('#toast'); t.textContent = m; t.classList.add('show'); clearTimeout(toastTimer); toastTimer = setTimeout(()=>t.classList.remove('show'), 2400); }
function openSheet(h){ $('#sheet').innerHTML = h; $('#sheet').classList.add('show'); $('#mask').classList.add('show'); }
function closeSheet(){ $('#sheet').classList.remove('show'); $('#mask').classList.remove('show'); }
function fallbackCopy(text, done){
  const ta = document.createElement('textarea'); ta.value = text; ta.style.cssText = 'position:fixed;opacity:0;top:0;left:0';
  document.body.appendChild(ta); ta.focus(); ta.select();
  try {
    // `execCommand` 自己会用 false 表示失败，不会抛异常。旧实现完全没看返回值，
    // 于是「浏览器拒绝复制」也会弹一条“已复制”，用户到群里粘贴时才发现是空的。
    const ok = document.execCommand('copy');
    if (!ok) toast('复制失败，请长按绑定命令手动复制');
    else done();
  } catch(e){ toast('复制失败，请长按绑定命令手动复制'); }
  finally { document.body.removeChild(ta); }
}
function copyText(text){
  const done = () => toast('已复制「'+text+'」，去 QQ 群粘贴发送');
  // WebView 里 `navigator.clipboard` 可能存在但被权限策略拒绝，因此必须保留 catch；
  // 回退路径也只在 `execCommand` 真返回 true 时才提示成功（见 fallbackCopy）。
  if (navigator.clipboard?.writeText) navigator.clipboard.writeText(text).then(done).catch(()=>fallbackCopy(text, done));
  else fallbackCopy(text, done);
}

// ---------- 状态 ----------
// 后端快照（唯一业务真相来源）。初值是一份「正在连接」的空壳。
let S = null;
// 本地界面态：当前 tab（后端 Page 没有 account，前端自己加）。
let tab = 'push';
// 统一交互闸门：业务请求只允许一条在途；按钮在等待期间统一禁用。
// 动作串行队列本身保留（删除确认需要严格顺序），但界面不会再让用户连点。
let actionPending = false;

async function fetchState(){
  try {
    const r = await fetch('/api/state', { cache: 'no-store' });
    if (!r.ok) return;
    S = await r.json();
    render(false);
  } catch(e){ /* 服务器瞬断时静默，下一轮再试 */ }
}

// 动作串行队列：保证「先 save-delete 再 save-delete-ok」这类两步操作顺序执行、
// 一个都不丢。同一次用户点击触发的多个动作会依次发完。
let chain = Promise.resolve();
const seen = new Set();   // 短时间去重：同一字符串 500ms 内只发一次，防连点
function act(actionStr, opts){
  if (!opts?.force) {
    if (seen.has(actionStr)) return chain;
    seen.add(actionStr);
    setTimeout(()=>seen.delete(actionStr), 500);
  }
  if (actionPending && !opts?.queue) {
    toast('正在处理上一步，请稍候');
    return chain;
  }
  const markPending = !opts?.queue;
  if (markPending) {
    actionPending = true;
    document.body.classList.add('busy');
    // 立即重绘按钮禁用状态；不能等请求回来才禁用，否则连点仍会在 500ms 窗口内挤进来。
    lastSig = '';
    render(false);
  }
  // 同一个 pending 计数：并发（nav 之外的队列动作）也只在内层全部结束时释放。
  pendingDepth += markPending ? 1 : 0;
  chain = chain.then(async () => {
    try {
      const r = await fetch('/api/action', {
        method: 'POST',
        body: actionStr,
      });
      if (!r.ok) { const t = await r.text(); toast(t || '操作失败'); }
    } catch(e){ toast('操作失败：连接中断'); }
    finally {
      if (markPending) {
        pendingDepth = Math.max(0, pendingDepth - 1);
        if (pendingDepth === 0) {
          actionPending = false;
          document.body.classList.remove('busy');
          // **先刷新快照再重绘**：后端忙态可能还没落回 false，
          // 如果先画再拉，按钮会停在禁用态直到下一轮轮询（约 700ms），
          // 真机上看起来像「操作完按钮变灰了」。
          await fetchState().catch(()=>{});
          lastSig = '';
          render(false);
        }
      }
    }
  });
  return chain;
}
let pendingDepth = 0;

// 纯本地切页：先同步更新 tab 与 DOM，网络动作随后异步补 `nav:*`。
// 旧实现在 `go()` 里先 `render()` 再 `act()`，网络慢时用户会看到页面没切；
// 现在切页不经过业务队列，底栏点击立即生效。
function navigatePage(t) {
  if (!TABS.some(([id]) => id === t)) return;
  if (tab === t) return;
  tab = t;
  closeSheet();
  animateNext = true;
  lastSig = '';   // 切页必须重建内容区，哪怕结构签名恰好相同
  render(true);
  // 账号页后端没有对应 Page。**不要**把它偷偷发成 `nav:saves`：
  // 那会让后端切到存档页，日志与页面状态跟着跑偏。账号页不产生业务页切换，
  // 它的刷新全靠账号自己的动作（授权 / 检查绑定 / 刷新状态）。
  const backendPage = {push:'push', saves:'saves', stats:'stats', settings:'settings'}[t];
  if (backendPage) act('nav:'+backendPage, { queue: true }).catch(()=>{});
}

// ---------- 渲染 ----------
const TABS = [['push','book-open','推送'],['saves','archive','存档'],['stats','bar-chart-3','统计'],['account','user','账号'],['settings','settings','设置']];
function renderNav(){
  $('#navbar').innerHTML = TABS.map(([id,ic,lb]) =>
    `<button class="${tab===id?'on':''}" onclick="go('${id}')" aria-label="${lb}">
      <span class="wrap">${icon(ic)}<span class="lb">${lb}</span>${id==='push'&&S?.transfer?'<span class="nb">1</span>':''}</span>
    </button>`).join('');
}
function go(t){ navigatePage(t); }

function renderDevice(){
  const d = S?.device;
  const devices = S?.devices || [];
  const name = d?.name || '未连接';
  const dev = $('#dev');
  $('#dev-name').textContent = name;
  dev.classList.toggle('off', !d?.connected);
  // 多设备：右上角从文字变成选择器。只有一个设备时保持静态文字，
  // 不为了“可能多设备”牺牲最常见场景的清晰度。
  const multi = devices.length > 1;
  dev.classList.toggle('multi', multi);
  dev.onclick = multi ? openDeviceSheet : null;
  dev.setAttribute('role', multi ? 'button' : 'status');
  dev.setAttribute('aria-label', multi ? `选择手环，当前 ${name}` : name);
}

function openDeviceSheet(){
  const devices = S?.devices || [];
  const rows = devices.map((item,index) => {
    const on = item.addr === S?.selectedDevice;
    return `<button class="dev-option ${on?'on':''}" onclick="closeSheet();act('device:${index}')">
      <span class="dev-option-name">${esc(item.name)}</span>
      <span class="dev-option-meta num">${item.connected ? '在线' : '未连接'}${on?' · 当前':''}</span>
    </button>`;
  }).join('');
  openSheet(`<h3>选择手环</h3><p>AstroBox 当前检测到 ${devices.length} 台在线设备。切换后会重新建立回包通道。</p>
    <div class="dev-list">${rows}</div>
    <div class="ops"><button class="btn ghost" onclick="closeSheet();act('devices-refresh')">重新扫描</button>
    <button class="btn ghost" onclick="closeSheet()">取消</button></div>`);
}

// 传输状态区（外壳，稳定）
function renderTransfer(){
  const el = $('#transfer');
  const t = S?.transfer, resume = S?.resume;
  el.classList.toggle('held', !!(t && t.stalled) || !!resume);
  if (t) {
    const held = t.stalled;
    el.innerHTML = `<div class="box">
      <div class="t-row"><span class="t-name">${esc(t.chapter)}${S.queue?.length?` <span class="tag">队列 +${S.queue.length}</span>`:''}</span><span class="t-pct num">${t.percent}%</span></div>
      <div class="bar"><i style="width:${t.percent}%"></i></div>
      <div class="t-meta"><span class="num">${esc(t.speedLabel||'—')} · 剩余 ${esc(t.etaLabel||'—')}</span><span class="num">${fmtSize(t.received)} / ${fmtSize(t.total)}</span></div>
      <div class="t-ops">${held?`<button class="btn sm" onclick="act('resume')">${icon('play')}继续</button>`:`<button class="btn sm ghost" onclick="act('resume')">${icon('pause')}暂停</button>`}</div>
    </div>`;
  } else if (resume) {
    el.innerHTML = `<div class="box">
      <div class="t-row"><span class="t-name">断点待续 · ${esc(resume.chapter)}</span><span class="t-pct num">${resume.percent}%</span></div>
      <div class="bar"><i style="width:${resume.percent}%"></i></div>
      <div class="t-ops"><button class="btn sm" onclick="act('resume')">${icon('play')}继续传输</button></div>
    </div>`;
  } else if (S?.resumeChecking) {
    el.innerHTML = `<div class="idle">${icon('refresh-cw')}<span>正在查询手环上的断点…</span></div>`;
  } else {
    const connected = S?.device?.connected;
    el.innerHTML = `<div class="idle">${icon(connected?'check':'cloud')}<span>${connected?'手环已连接，没有传输任务':'手环未连接，点下方连接'}</span></div>`;
  }
}

function pushHTML(){
  const lib = S?.library || [];
  const filter = S?.lineFilter || null;
  const lines = ['全部','共通线','千岁线','玲线','结灯线','番外'];
  const list = lib.filter(p => !filter || p.line === filter || (filter==='共通线' && p.line==='资源'));
  const rest = lib.filter(p => !p.installed && p.line !== '资源').length;
  const connected = !!S?.device?.connected;
  // 关键错误/提示接线：库损坏、手环应用过旧、后端状态失败都要看得见，不能静默。
  const notices = [];
  if (S?.libraryError) notices.push(`<p class="notice bad">章节包不可用：${esc(S.libraryError)}</p>`);
  if (S?.bandGameLegacy) notices.push(`<p class="notice warn">手环上《甜蜜女友2》版本过旧，请先更新手环应用再同步。</p>`);
  if (S?.error) notices.push(`<p class="notice bad">${esc(S.error.code||'出错')}：${esc(S.error.detail||'')}</p>`);
  if (S?.installedBroken?.length) notices.push(`<p class="notice warn">有 ${S.installedBroken.length} 个已装章节读不出来，建议重传：${esc(S.installedBroken.join('、'))}</p>`);
  const appReady = !!S?.saveProtocol;
  const ready = !!connected && appReady;
  return `
  ${!connected ? `<button class="btn block" style="margin-bottom:8px" ${actionPending?'disabled':''} onclick="act('connect')">连接手环</button>` : ''}
  ${connected ? `<button class="btn ghost block" style="margin-bottom:12px" ${actionPending?'disabled':''} onclick="act('launch')">打开《甜蜜女友2》</button>` : ''}
  ${!connected ? `<p class="notice warn">连接手环并等《甜蜜女友2》返回数据后，才能同步或管理数据。</p>` : ''}
  ${notices.join('')}
  <div class="filters">${lines.map(l=>{
    const val = l==='全部' ? 'line:*' : 'line:'+l;
    const on = (l==='全部' && !filter) || filter===l;
    return `<button class="chip ${on?'on':''}" onclick="act('${val}')">${l}</button>`;
  }).join('')}</div>
  <div class="panel" style="margin-top:10px">
  ${list.map(p => {
    const cur = p.active, part = S?.resume && S.resume.packId === p.id;
    return `<div class="chap">
      <div class="l1"><span class="t">${esc(p.title)}${p.line==='资源'?' <span class="tag">先同步</span>':''}</span>
        ${p.installed?`<span class="tag ok">已安装</span>`:part?`<span class="tag warn">待续传</span>`:cur?`<span class="tag acc">传输中</span>`:''}</div>
      <div class="l2"><span class="meta num">${fmtSize(p.bytes)}${p.minutes?' · 约 '+fmtMin(p.minutes):' · 跨章共享场景图'}</span>
        <span class="ops">${p.installed
          ?`<button class="btn sm ghost" ${!ready||actionPending?'disabled':''} onclick="act('sync:${p.number}')">重传</button><button class="btn sm text" ${!ready||actionPending?'disabled':''} style="color:var(--bad)" onclick="confirmDelete(${jsArg(p.id)},${jsArg(p.title)},${Number(p.bytes)||0})">删除</button>`
          :`<button class="btn sm" ${!ready||actionPending?'disabled':''} onclick="act('sync:${p.number}')">${part?'继续':'同步'}</button>`}</span></div>
      ${cur&&S.transfer?`<div class="prog"><i style="width:${S.transfer.percent}%"></i></div>`:''}
    </div>`;
  }).join('') || '<div class="empty">该线路下没有章节</div>'}
  </div>
  <button class="btn block" style="margin-top:14px" ${!ready||actionPending?'disabled':''} onclick="act('sync-all')" ${rest?'':'disabled'}>同步剩余 ${rest} 章</button>
  <p class="foot-note">剧情文本随手环应用安装，插件只推送图片。<br>旧版本章节重传即可覆盖更新。</p>`;
}

function confirmDelete(id, title, bytes){
  openSheet(`<h3>删除「${esc(title)}」？</h3><p>从手环删除该章节（${fmtSize(bytes)}），删除后需重新同步才能阅读。不影响存档。</p>
    <div class="ops"><button class="btn ghost" onclick="closeSheet()">取消</button>
    <button class="btn" style="background:var(--bad)" onclick="closeSheet();act('delete:${id}')">删除</button></div>`);
}

function saveRows(){
  const rows = [];
  if (S?.saveAuto) rows.push({ auto:true, slot:null, data:S.saveAuto });
  (S?.saveSlots||[]).forEach((d,i) => rows.push({ auto:false, slot:i, data:d }));
  return rows;
}
function saveTitle(d){ return d?.chapterTitle || d?.chapter || '未命名存档'; }
function saveScene(d){
  const sc = d?.currentScene ?? d?.packScene ?? d?.scene;
  return sc == null ? '' : '场景 ' + sc;
}
function saveTime(d){
  // savedAt 是毫秒时间戳（见 band-saves-data.json 的契约），格式化成 MM-DD HH:mm。
  const ms = Number(d?.savedAt);
  if (!ms || !isFinite(ms)) return '';
  const t = new Date(ms);
  const p = n => String(n).padStart(2, '0');
  return `${p(t.getMonth()+1)}-${p(t.getDate())} ${p(t.getHours())}:${p(t.getMinutes())}`;
}

function savesHTML(){
  const rows = saveRows();
  const ready = !!S?.saveProtocol && !S?.savesBusy && !actionPending;
  const loaded = !!S?.saveProtocol;
  const manual = rows.filter(r => !r.auto).length;
  const current = S?.pendingCurrentSave || null;
  return `
  <div class="saves-ops">
    <button class="btn ghost" ${!ready?'disabled':''} onclick="act('save-export')">${icon('upload')}导出到剪贴板</button>
    <button class="btn ghost" ${!ready?'disabled':''} onclick="act('save-import')">${icon('download')}从剪贴板导入</button>
  </div>
  <button class="btn block" style="margin-top:12px" ${!ready?'disabled':''} onclick="act('save-capture')">${icon('clock')}${current?'重新读取当前进度':'读取当前进度'}</button>
  <div class="sec">手环上的存档<span class="num">${manual} 个手动槽${rows.some(r=>r.auto) ? ' · 自动存档 1' : ''}</span><button class="lk" ${!loaded||S?.savesBusy||actionPending?'disabled':''} onclick="act('save-refresh')">刷新</button></div>
  ${!loaded?`<p class="notice warn">${S?.device?.connected?'正在等待《甜蜜女友2》返回存档能力…':'连接手环并打开游戏后才能读取存档。'}</p>`:''}
  ${S?.savesBusy?`<div class="loading-line">${icon('refresh-cw')}<span>正在处理存档…</span></div>`:''}
  ${S?.savesError?`<p class="foot-note" style="color:var(--bad)">${esc(S.savesError)}</p>`:''}
  ${current?`<p class="foot-note" style="color:var(--accent);margin-top:10px">当前进度：${esc(current.chapter)} · ${esc(current.scene_label||('场景 '+(current.scene||1)))}。选择已有槽覆盖，或在末尾新建。</p>`:''}
  <div class="panel">
  ${rows.length ? rows.map(r => {
    // 剧情正文随手环应用内置；章节资源包只影响图片。没有资源包不该禁用读档。
    const packId = r.data?.packId;
    const hasImages = !packId || (S?.installed||[]).some(i => i.id === packId);
    return `
    <div class="save-row"><div class="info">
        <div class="t">${esc(saveTitle(r.data))}${r.auto?'<span class="tag">自动</span>':`<span class="tag">槽 ${r.slot+1}</span>`}${hasImages?'':'<span class="tag warn">无图片</span>'}${r.data?.cloudConflict?'<span class="tag acc">云冲突副本</span>':''}${!r.auto&&current?'<span class="tag acc">可覆盖</span>':''}</div>
        <div class="m num">${esc(saveScene(r.data))}${saveTime(r.data)?' · '+esc(saveTime(r.data)):''}</div></div>
      <div class="ops">
        ${r.auto?'':`<button class="ibtn" aria-label="读档" ${ready?'':'disabled'} onclick="${ready?`act('save-load:${r.slot}')`:''}">${icon('play')}</button>
        <button class="btn sm ghost" ${ready&&current?'':'disabled'} onclick="${ready&&current?`confirmOverwriteSave(${r.slot},${jsArg(saveTitle(r.data))})`:''}">覆盖</button>
        <button class="ibtn warn" aria-label="删除" ${ready?'':'disabled'} onclick="confirmDeleteSave(${r.slot},${jsArg(saveTitle(r.data))})">${icon('trash-2')}</button>`}
      </div></div>`;}).join('') : '<div class="empty">手环上还没有存档</div>'}
    ${current?`<div class="save-row add-save-row"><div class="info">
        <div class="t">空存档槽 ${manual+1}</div>
        <div class="m">${esc(current.chapter)} · ${esc(current.scene_label||('场景 '+(current.scene||1)))}</div></div>
      <button class="btn sm" ${ready?'':'disabled'} onclick="act('save-write:${manual}')">保存到这里</button></div>`:''}
  </div>
  ${S?.saveExport?`<p class="foot-note" style="color:var(--ok)">已复制 ${S.saveExport.slots} 个存档到剪贴板${S.saveExport.verified?'（已核对）':''}</p>`:''}
  ${S?.saveImport?`<p class="foot-note" style="color:var(--ok)">导入 ${S.saveImport.incoming} 个，跳过 ${S.saveImport.duplicates} 个重复</p>`:''}
  <p class="foot-note">读档即设为「继续阅读」。缺章节资源包时仍可读正文，只显示纯文字。存档保存在手环上，换机请先导出。</p>`;
}
function confirmOverwriteSave(slot, title){
  openSheet(`<h3>覆盖存档 ${slot+1}？</h3><p>「${esc(title)}」会被当前游戏进度替换，原存档不可恢复。</p>
    <div class="ops"><button class="btn ghost" onclick="closeSheet()">取消</button>
    <button class="btn" onclick="closeSheet();act('save-write:${slot}')">覆盖</button></div>`);
}
function confirmDeleteSave(slot, title){
  openSheet(`<h3>删除存档？</h3><p>删除手环上「${esc(title)}」这条存档，不可恢复。</p>
    <div class="ops"><button class="btn ghost" onclick="closeSheet()">取消</button>
    <button class="btn" style="background:var(--bad)" onclick="closeSheet();act('save-delete:${slot}');act('save-delete-ok:${slot}')">删除</button></div>`);
}

function statsHTML(){
  const s = S?.stats;
  const ready = !!S?.statsSupported && !S?.statsBusy && !actionPending;
  if (!s) return `<div class="empty" style="padding-top:60px">${S?.statsBusy?'正在读取统计…':'还没有统计数据'}<br><br><button class="btn ghost" ${!ready?'disabled':''} onclick="act('stats-refresh')">${icon('refresh-cw')}读取统计</button></div>
    ${S?.statsBusy?`<div class="loading-line">${icon('refresh-cw')}<span>正在读取手环统计…</span></div>`:''}
    ${S?.statsError?`<p class="foot-note" style="color:var(--bad)">${esc(S.statsError)}</p>`:''}`;
  const recent = s.recent || [];
  const max = Math.max(...recent.map(r=>r.seconds), 1);
  return `
  <div class="stat-lead"><div class="v num">${esc(s.totalLabel||'0')}</div><div class="k">总阅读时长</div></div>
  <div class="panel" style="margin-top:16px">
    <div class="kv"><span class="k">累计阅读天数</span><span class="v">${esc(s.readingDaysLabel||'—')}</span></div>
    <div class="kv"><span class="k">今日阅读</span><span class="v">${esc(s.todayLabel||'—')}</span></div>
    <div class="kv"><span class="k">单日最长</span><span class="v">${esc(s.longestLabel||'—')}</span></div>
    <div class="kv"><span class="k">总天数</span><span class="v">${esc(s.totalDaysLabel||'—')}</span></div>
  </div>
  <div class="sec">最近 ${recent.length} 天<button class="lk" ${!ready?'disabled':''} onclick="act('stats-refresh')">刷新</button></div>
  <div class="panel" style="padding-top:4px;padding-bottom:4px">
    ${recent.map(d=>`<div class="day"><span class="d num">${esc(d.date)}</span><div class="b"><i style="width:${d.seconds/max*100}%"></i></div><span class="v num">${esc(d.label)}</span></div>`).join('') || '<div class="empty">暂无记录</div>'}
  </div>
  <p class="foot-note">统计由手环侧计算后下发，两端始终一致。</p>`;
}

function qqAvatar(url, name, cls='avatar'){
  // QQ 空间头像接口不要求登录，且按 QQ 号稳定。图片加载失败时只剩元素本身，
  // 所以底下始终放一层昵称首字：网络差、接口限流时也不会变成空白圆。
  const initial = esc(Array.from(String(name||'?').trim())[0] || '?');
  if (!url) return `<div class="${cls} avatar-fallback">${initial}</div>`;
  return `<div class="${cls} avatar-fallback">${initial}<img src="${esc(url)}" alt="" loading="lazy" onerror="this.remove()"></div>`;
}

function accountHTML(){
  const phase = S?.cloudPhase || 'loading';
  const auth = S?.accountAuth || 'idle';
  const authorized = auth === 'authorized';
  const bound = !!S?.cloudQq;
  const busy = auth === 'authorizing' || actionPending;

  if (!authorized) {
    // `error` 是「宿主读账号出错了」（网络/会话），与「你还没登录」是两件事：
    // 前者重试可能就好，后者必须先去 AstroBox 登录。别让两种都显示同一句话。
    const detail = {
      authorizing: '正在读取 AstroBox 账号…',
      denied: 'AstroBox 还没登录：请先在 AstroBox 里登录账号，再回来授权。',
      error: S?.cloudDetail || '读取 AstroBox 账号失败，请稍后重试。',
    }[auth] || '授权只用于关联当前 AstroBox 账号，插件不会读取密码或令牌。';
    return `
    <div class="acct-head">${qqAvatar('', auth==='error'?'账号读取失败':'尚未绑定账号')}
      <div class="who"><div class="nm">${auth==='error'?'账号读取失败':'尚未绑定账号'}</div><div class="sub">先用 AstroBox 账号授权，再绑定一个 QQ</div></div></div>
    <div class="account-flow">
      <div class="flow-step on"><span>1</span><div><b>授权 AstroBox 账号</b><small>获取当前账号的公开资料</small></div></div>
      <div class="flow-step"><span>2</span><div><b>绑定 QQ</b><small>QQ 成为主账号，之后自动登录</small></div></div>
    </div>
    <button class="btn block" style="margin-top:16px" ${busy?'disabled':''} onclick="act('cloud-authorize')">${icon('log-in')}${auth==='error'?'重试授权':'授权 AstroBox 账号'}</button>
    <p class="foot-note">${esc(detail)}</p>`;
  }

  const title = S?.cloudQqName || (bound ? 'QQ 主账号' : 'AstroBox 账号已授权');
  const phaseTag = {
    synced:'<span class="tag ok">已同步</span>', idle:'<span class="tag ok">已连接</span>',
    syncing:'<span class="tag acc">同步中…</span>', applying:'<span class="tag acc">写入中…</span>',
    offline:'<span class="tag warn">离线</span>', failed:'<span class="tag bad">同步失败</span>',
    unbound:'<span class="tag">未绑定 QQ</span>', 'waiting-binding':'<span class="tag warn">待绑定</span>',
  }[phase] || `<span class="tag">${esc(phase)}</span>`;
  // 页头直接给「主账号 + 号」，下面就不再重复列一遍 QQ 昵称/QQ 号 ——
  // 旧版把同一份身份写了三处（页头、流程条、信息卡），一屏里三行一样的字。
  const identity = bound
    ? `QQ ${esc(S.cloudQq)}${S?.cloudAccountName?' · '+esc(S.cloudAccountName):''}`
    : 'AstroBox 账号关联身份';
  return `
  <div class="acct-head">
    ${qqAvatar(S?.cloudQq ? `https://q1.qlogo.cn/g?b=qq&nk=${encodeURIComponent(S.cloudQq)}&s=100` : '', title)}
    <div class="who"><div class="nm">${esc(title)}</div><div class="sub num">${identity}</div></div>
    ${phaseTag}
  </div>
  <div class="account-flow compact">
    <div class="flow-step done"><span>${icon('check')}</span><div><b>AstroBox 已授权</b><small>${esc(S?.cloudAccountName||'')}</small></div></div>
    <div class="flow-step ${bound?'done':'on'}"><span>${bound?icon('check'):'2'}</span><div><b>${bound?'QQ 已绑定':'绑定 QQ'}</b><small>${bound?'之后会自动登录':'一个 AstroBox 账号只绑定一个 QQ'}</small></div></div>
  </div>` + (S?.cloudDetail && S.cloudDetail !== '—' ? `
  <p class="acct-detail">${esc(S.cloudDetail)}</p>` : '') + `
  ${bound?`<button class="btn ghost block" style="margin-top:12px" ${actionPending?'disabled':''} onclick="act('cloud-sync')">${icon('refresh-cw')}立即同步</button>`:''}
  ${bound?`
    <div class="sec">QQ 绑定</div>
    <div class="panel"><div class="manage-row static">${icon('check')}<span><b>已绑定 QQ ${esc(S.cloudQq||'')}</b><small>一个 AstroBox 账号只绑定一个 QQ；换绑请在 QQ 群联系管理员</small></span></div></div>`:S?.cloudCode?(() => {
      const expired = bindingExpired(S.cloudExpiresAt);
      return `
    <div class="sec">绑定 QQ</div>
    <div class="panel" style="padding:14px 15px">
      <div style="font-size:13px;color:var(--sub);line-height:1.8">${expired
        ? '这条绑定命令已经过期。生成一条新命令后，再去 QQ 群粘贴发送。'
        : `复制下面的命令，到 QQ 群 <b style="color:var(--text)">1105166284</b> 粘贴发送，然后点「检查绑定」。`}</div>
      <div class="code-display${expired?' expired':''}"><span class="code-text">${esc(S.cloudCode)}</span>
        <button class="copy-btn" onclick="copyText(${jsArg(S.cloudCode)})" aria-label="复制绑定命令">${icon('copy')}<span>复制</span></button></div>
      <div class="bind-foot"><span class="bind-expire" id="bind-expire">${esc(bindingExpiryLabel(S.cloudExpiresAt))}</span>
        ${expired
          ? `<button class="btn sm" ${busy?'disabled':''} onclick="act('cloud-binding-code')">生成新命令</button>`
          : `<button class="btn sm ghost" ${busy?'disabled':''} onclick="act('cloud-binding-code')">重新生成</button><button class="btn sm" ${actionPending?'disabled':''} onclick="act('cloud-session')">检查绑定</button>`}
      </div>
    </div>`;
    })():`
    <div class="sec">绑定 QQ</div>
    <div class="panel" style="padding:14px 15px">
      <div style="font-size:13px;color:var(--sub);line-height:1.8">生成一次性绑定命令，到 QQ 群 <b style="color:var(--text)">1105166284</b> 发送。一个 AstroBox 账号只绑定一个 QQ。</div>
      <button class="btn ghost block" style="margin-top:12px" ${busy?'disabled':''} onclick="act('cloud-binding-code')">${icon('link')}生成绑定命令</button>
    </div>`}
  <div class="sec">账号管理</div>
  <div class="panel account-manage">
    <button class="manage-row" ${actionPending?'disabled':''} onclick="act('cloud-session')">${icon('refresh-cw')}<span><b>刷新账号状态</b><small>重新读取 QQ 绑定与云同步状态</small></span>${icon('chevron-right')}</button>
    <button class="manage-row danger" ${actionPending?'disabled':''} onclick="confirmForgetAccount()">${icon('log-out')}<span><b>忘记本机授权</b><small>不删除云端存档和 QQ 绑定</small></span>${icon('chevron-right')}</button>
  </div>
  <p class="foot-note">QQ 是主账号，AstroBox 账号只用于确认“这台设备属于谁”。</p>`;
}

function bindingExpired(iso){ return Number.isFinite(Date.parse(iso || '')) ? Date.parse(iso) <= Date.now() : false }
function bindingExpiryLabel(iso){
  const ms = Date.parse(iso || '');
  if (!Number.isFinite(ms)) return '3 分钟内有效';
  const left = Math.max(0, Math.floor((ms - Date.now()) / 1000));
  if (!left) return '绑定码已过期';
  return `剩余 ${Math.floor(left/60)}:${String(left%60).padStart(2,'0')}`;
}

function confirmForgetAccount(){
  openSheet(`<h3>忘记本机账号授权？</h3><p>插件会清除本机设备凭证。下次需要重新授权 AstroBox 账号，但云端存档和 QQ 绑定不会被删除。</p>
    <div class="ops"><button class="btn ghost" onclick="closeSheet()">取消</button>
    <button class="btn" style="background:var(--bad)" onclick="closeSheet();act('cloud-forget')">忘记授权</button></div>`);
}

function settingsHTML(){
  const mode = S?.transferMode || 'steady';
  const auto = !!S?.autoLaunch;
  const logFilter = S?.logFilter || 'all';
  return `
  <div class="sec">传输</div>
  <div class="panel">
    <div class="set-row"><div class="l"><div class="t">分片档位</div><div class="d">${S?.turboBlocked?esc(S.turboBlocked):'正常档窗口 6–10 片；激进档 12 片起，近距离更快'}</div></div>
      <div class="seg"><button class="${mode==='steady'?'on':''}" onclick="act('mode:steady')">正常</button><button class="${mode==='turbo'?'on':''}" onclick="act('mode:turbo')">激进</button></div></div>
    <div class="set-row"><div class="l"><div class="t">连接后自动打开游戏</div><div class="d">连上手环后顺带把《甜蜜女友2》调到前台</div></div>
      <div class="sw ${auto?'on':''}" onclick="act('auto-launch:${auto?'off':'on'}')"></div></div>
    <div class="set-row"><div class="l"><div class="t">未完成缓存</div><div class="d num">${fmtSize(S?.cacheBytes||0)} · ${S?.cacheFiles||0} 个文件</div></div>
      <button class="btn sm ghost" onclick="act('clear-cache')">清理</button></div>
  </div>
  <div class="sec">设备<span>${(S?.devices||[]).length>1?`${S.devices.length} 台在线`:''}</span></div>
  <div class="panel">
    <div class="set-row"><div class="l"><div class="t">${esc(S?.device?.name||'未连接')}</div><div class="d">${S?.bandVersion?'应用 '+esc(S.bandVersion):'syncProtocol 协商后显示'}${S?.saveProtocol?' · 存档协议 v'+S.saveProtocol:''}</div></div>
      <div class="ops-inline">
        ${(S?.devices||[]).length>1?'<button class="btn sm ghost" onclick="openDeviceSheet()">切换设备</button>':''}
        <button class="btn sm ghost" ${actionPending?'disabled':''} onclick="act('devices-refresh')">扫描</button>
      </div></div>
  </div>
  <div class="sec">运行日志
    <span><button class="lk" onclick="act('logfilter:${logFilter==='all'?'warn':logFilter==='warn'?'error':'all'}')">${{all:'全部',warn:'警告',error:'错误'}[logFilter]}</button> · <button class="lk" onclick="act('log-clear')">清空</button></span></div>
  <div class="panel" style="max-height:220px;overflow-y:auto">
    ${(S?.logs||[]).length ? S.logs.map(l=>`<div class="log"><span class="${l.level==='w'?'w':l.level==='e'?'e':''}">${esc(l.text)}</span></div>`).join('') : '<div class="empty" style="padding:14px">暂无日志</div>'}
  </div>
  <div class="sec">关于</div>
  <div class="panel">
    <div class="about"><span>插件版本</span><span class="num">${esc(S?.version||'—')}</span></div>
    <div class="about"><span>项目主页</span><button class="lk link-out" onclick="act('open-website')">miband.qihe0507.top ${icon('external-link')}</button></div>
    <div class="about"><span>作者</span><span>其核</span></div>
    <button class="btn ghost block update-btn" ${actionPending?'disabled':''} onclick="act('check-update')">${icon('refresh-cw')}检查更新</button>
    ${S?.updateStatus?`<p class="update-status ${S.updateAvailable?'new':''}">${esc(S.updateStatus)}</p>`:''}
  </div>
  <p class="foot-note">《甜蜜女友2》是 AZARASHI SOFT 的作品。本项目是非官方、非商业的穿戴设备适配。</p>`;
}

// 渲染：只有「结构相关数据」变了才重绘内容区。
// 轮询到的传输进度每 700ms 都在变，但那只更新顶部传输卡，**不能**整页重绘 ——
// 否则入场动画每 700ms 重播一次（真机表现为「一直在播放动画」），还会丢焦点、闪屏。
let lastSig = '';
// 只有「切页」与「首次绘制」该播入场动画；轮询与操作完成后的重绘一律 no-anim。
// 用一次性标记，避免 animate=false 把后续切页也永久关掉。
let animateNext = true;
function structureSig(){
  if (!S) return '';
  // 只挑影响列表结构/文案的字段；进度、速度、百分比这类高频值不参与。
  return JSON.stringify({
    tab,
    lib: (S.library||[]).map(p=>[p.id,p.installed,p.active,p.queued]),
    inst: (S.installed||[]).map(i=>i.id),
    saves: [S.saveAuto, S.saveSlots, S.pendingCurrentSave, S.savesError, S.saveExport, S.saveImport],
    stats: S.stats ? [S.stats.totalLabel, S.stats.recent?.length] : null,
    acc: [S.cloudPhase, S.cloudQq, S.cloudQqName, S.cloudCode, S.cloudExpiresAt, S.cloudAccountName, S.cloudDetail, S.accountAuth],
    set: [S.transferMode, S.autoLaunch, S.logFilter, (S.logs||[]).length, S.updateStatus, S.updateAvailable],
    notices: [S.libraryError, S.bandGameLegacy, !!S.error, S.installedBroken?.length, S.savesBusy, S.statsBusy, actionPending],
    dev: [S.device?.name, S.device?.connected, (S.devices||[]).map(d=>[d.name,d.addr,d.connected])],
    line: S.lineFilter, queue: S.queue?.length,
  });
}

function render(animate){
  if (!S) { $('#view').innerHTML = '<div class="empty" style="padding-top:80px">正在连接插件…</div>'; return; }
  renderNav(); renderDevice(); renderTransfer();   // 外壳每轮都轻量更新，不做动画
  const sig = structureSig();
  if (sig === lastSig) { tickCountdown(); return; }   // 结构没变：不碰 DOM，动画自然不会重播
  lastSig = sig;
  const body = {push:pushHTML, saves:savesHTML, stats:statsHTML, account:accountHTML, settings:settingsHTML}[tab]();
  // 入场动画只在切页/首屏播一次；其余重绘（轮询、动作完成、禁用态刷新）不演。
  const cls = (animate === true || animateNext) ? 'page' : 'page no-anim';
  animateNext = false;
  const focus = document.activeElement?.id;
  $('#view').innerHTML = `<div class="${cls}">${body}</div>`;
  $('#view').querySelectorAll('.page>*').forEach((el,i)=>el.style.setProperty('--i',i));
  $('#view').querySelectorAll('.day').forEach((el,i)=>el.style.setProperty('--i',i));
  if (focus && $(`#${focus}`)) $(`#${focus}`).focus();
  tickCountdown();
}

// 绑定码倒计时：只改文本，不重绘整页（重绘会重播入场动画、也会打断复制按钮反馈）。
let bindingExpiredShown = '';
function tickCountdown(){
  const el = document.getElementById('bind-expire');
  if (!el || !S?.cloudExpiresAt) return;
  const label = bindingExpiryLabel(S.cloudExpiresAt);
  el.textContent = label;
  // 从“剩余 0:01”跨到“已过期”时，需要把按钮从「检查绑定」换成「生成新命令」。
  // 只在每个绑定码第一次跨过边界时重绘，否则每秒重绘会闪屏。
  const key = `${S.cloudCode}:${S.cloudExpiresAt}`;
  if (label !== '绑定码已过期') { bindingExpiredShown = ''; return; }
  if (bindingExpiredShown !== key && !actionPending) {
    bindingExpiredShown = key;
    lastSig = '';
    render(false);
  }
}

// ---------- 启动：首屏 + 轮询 ----------
// 首次打开自动扫描在线设备，让顶栏在多设备时立即变成选择菜单。
// 这不是“连接”，只在宿主设备列表上枚举，不会启动探测或占用手环通道。
fetchState().then(()=>act('devices-refresh', { force:true }));
setInterval(fetchState, 700);   // 传输进度刷新节拍；动作后立即 fetchState 另算
setInterval(tickCountdown, 1000); // 绑定码倒计时只改文本，不重绘
