/**
 * 生成 ui-v3 原型预览页。
 *
 *   node ui-v3-proto/build.mjs
 *
 * 输出 `ui-v3-proto/out/index.html`：五个页面按真机宽度（420 / 400）并排，
 * 只渲染宿主白名单内的属性，页脚列出被丢弃的属性名。
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { renderNode, BASE_CSS, droppedProps } from './render.mjs';
import { PAGES, DEVICE, TRANSFER, shell } from './tree.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const out = path.join(here, 'out');
fs.mkdirSync(out, { recursive: true });

const SECTIONS = [
  ['push', '推送'],
  ['saves', '存档'],
  ['stats', '统计'],
  ['account', '账号'],
  ['settings', '设置'],
];

const frames = SECTIONS.map(([key, label]) => {
  const content = PAGES[key]();
  const tree = shell(key, {
    device: DEVICE,
    // 传输条只在推送页展示「进行中」，其余页面展示空闲态 —— 与外壳常驻的语义一致。
    transfer: key === 'push' ? TRANSFER : null,
    content,
  });
  const body = renderNode(tree);
  return `
  <figure class="frame">
    <figcaption>${label}</figcaption>
    <div class="phone" data-page="${key}">${body}</div>
  </figure>`;
}).join('');

const dropped = droppedProps();
const html = `<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<title>甜蜜女友2 · ui-v3 原型（宿主白名单保真）</title>
<style>
${BASE_CSS}
body{background:#0b090a;padding:22px 18px 60px}
h1{color:#F5F0F2;font-size:17px;font-weight:500;margin-bottom:6px}
.lead{color:#8A7D94;font-size:12.5px;line-height:1.8;max-width:900px;margin-bottom:20px}
.lead b{color:#E59BB4;font-weight:400}
.row{display:flex;gap:22px;flex-wrap:wrap;align-items:flex-start}
.frame{margin:0}
figcaption{color:#8A7D94;font-size:11.5px;margin:0 0 6px 2px}
/* 真机是 420px 宽、约 860 高；这里 1:1 出图，不做缩放。 */
.phone{width:420px;height:860px;overflow:auto;background:#161316;border-radius:12px;
  border:1px solid #2a2226;display:flex;flex-direction:column}
.phone::-webkit-scrollbar{display:none}
.note{margin-top:26px;color:#8A7D94;font-size:12px;line-height:1.9;max-width:900px}
.note code{color:#B8ACB2}
.note ul{margin:8px 0 0 18px}
</style>
</head>
<body>
<h1>甜蜜女友2 · ui-v3 原型</h1>
<p class="lead">
按宿主 <b>ui-v3 真实属性白名单</b>渲染：白名单外的属性整条丢弃，不补、不近似。
所以这张图上看到的字重、阴影、渐变缺失，就是真机上会缺的 —— 不是渲染器偷懒。
</p>
<div class="row">${frames}</div>
<div class="note">
<p>被宿主丢弃的属性（本原型里出现过、但真机不生效）：<br>
<code>${dropped.length ? dropped.join('、') : '（无：全部落在白名单内）'}</code></p>
<p><b>结构性妥协</b>（宿主能力决定的，不是设计选择）：
<ul>
<li>底栏无法固定 → 导航上移到顶栏下方，靠一根指示条表达选中（没有 font-weight，加粗这条路不通）</li>
<li>没有百分比宽度 → 进度条用 24 段等宽小块点亮</li>
<li>没有 text-align → 右侧数值靠 <code>flex-grow</code> 推挤，不靠右对齐</li>
<li>没有 line-height → 行内多行文本的疏密由字号与 gap 近似，长说明会偏紧</li>
</ul></p>
</div>
</body>
</html>`;

fs.writeFileSync(path.join(out, 'index.html'), html, 'utf8');
console.log('wrote', path.join(out, 'index.html'));
console.log('dropped props:', dropped.length ? dropped.join(', ') : '(none)');
