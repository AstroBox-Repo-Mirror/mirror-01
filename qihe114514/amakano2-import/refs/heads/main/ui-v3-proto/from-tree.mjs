/**
 * 把 `ui-core` 导出的**抽象节点树 JSON** 渲染成页面。
 *
 * 抽象 props（`flex` / `pt` / `align` / `grow` …）本身不是宿主认识的东西 ——
 * 真正把它们落到宿主元素上的是插件侧的 `src/host/render.rs`。
 * 所以这里**照抄 `render.rs` 的 `apply()` 映射**，再交给白名单渲染器：
 * 抽象键 → CSS 属性名 → 宿主是否接受 → 接受就画、不接受就整条丢掉。
 *
 * 唯一的目的：让「Rust 侧真实产出的树」也能在装包之前被看见，
 * 而且看见的是**宿主会画出来的样子**，不是浏览器 CSS 的理想效果。
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { renderNode, BASE_CSS, droppedProps } from './render.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));

const ALIGN = { start: 'flex-start', center: 'center', end: 'flex-end', stretch: 'stretch' };
const JUSTIFY = {
  start: 'flex-start',
  center: 'center',
  end: 'flex-end',
  between: 'space-between',
  around: 'space-around',
};

/** 抽象键 → 宿主 CSS 属性（与 `src/host/render.rs` 的 apply() 一一对应）。 */
function abstractToCss(props) {
  const out = {};
  for (const [key, raw] of Object.entries(props || {})) {
    const value = String(raw);
    switch (key) {
      case 'flex':
        out.display = 'flex';
        out['flex-direction'] = value === 'row' ? 'row' : value === 'row-reverse' ? 'row-reverse' : value === 'column-reverse' ? 'column-reverse' : 'column';
        break;
      case 'gap': out.gap = value; break;
      case 'align': out['align-items'] = ALIGN[value] || 'center'; break;
      case 'justify': out['justify-content'] = JUSTIFY[value] || 'center'; break;
      case 'grow': out['flex-grow'] = value; break;
      case 'shrink': out['flex-shrink'] = value; break;
      case 'grid': out['grid-template-columns'] = value; break;
      case 'padding': out.padding = value; break;
      case 'pt': out['padding-top'] = value; break;
      case 'pb': out['padding-bottom'] = value; break;
      case 'pl': out['padding-left'] = value; break;
      case 'pr': out['padding-right'] = value; break;
      case 'margin': out.margin = value; break;
      case 'mt': out['margin-top'] = value; break;
      case 'mb': out['margin-bottom'] = value; break;
      case 'ml': out['margin-left'] = value; break;
      case 'mr': out['margin-right'] = value; break;
      case 'w': out.width = value; break;
      case 'h': out.height = value; break;
      case 'maxw': out['max-width'] = value; break;
      case 'maxh': out['max-height'] = value; break;
      case 'minw': out['min-width'] = value; break;
      case 'minh': out['min-height'] = value; break;
      case 'radius': out['border-radius'] = value; break;
      case 'bg': out.background = value; break;
      case 'fg': out.color = value; break;
      case 'size': out['font-size'] = value; break;
      case 'border': {
        const [w, ...rest] = value.split(' ');
        if (rest.length) out.border = `${w}px solid ${rest.join(' ')}`;
        break;
      }
      case 'opacity': out.opacity = value; break;
      case 'transition': out.transition = value; break;
      case 'transform': out.transform = value; break;
      case 'transform-origin': out['transform-origin'] = value; break;
      case 'filter': out.filter = value; break;
      case 'backdrop': out['backdrop-filter'] = value; break;
      case 'pos': out.position = value; break;
      case 'z': out['z-index'] = value; break;
      case 'top': out.top = value; break;
      case 'bottom': out.bottom = value; break;
      case 'left': out.left = value; break;
      case 'right': out.right = value; break;
      // 抽象键 → 真正的 CSS 属性名（render.rs 里同名的翻译）。
      case 'weight': out['font-weight'] = value; break;
      case 'ls': out['letter-spacing'] = value; break;
      case 'lh': out['line-height'] = value; break;
      case 'align-text': out['text-align'] = value; break;
      case 'wrap': out['flex-wrap'] = value; break;
      case 'nodflt': out.nodflt = value; break;
      case 'scroll':
        if (value === 'x') { out['overflow-x'] = 'auto'; out['overflow-y'] = 'hidden'; }
        else if (value === 'both') { out.overflow = 'auto'; }
        else { out['overflow-y'] = 'auto'; out['overflow-x'] = 'hidden'; }
        break;
      case 'value': break;
      case 'on': break;
      default:
        // 逃生舱：已经是 CSS 属性名的原样透传。
        out[key] = value;
    }
  }
  return out;
}

/** 抽象树 → 白名单渲染器认识的树。 */
function adapt(node) {
  const tag = node.tag;
  const props = abstractToCss(node.props);
  const disabled = props.nodflt !== undefined || node.props?.disabled !== undefined;
  return {
    tag,
    text: node.text,
    props,
    disabled,
    children: (node.children || []).map(adapt),
  };
}

const inputPath = process.argv[2] || 'E:/buildroot/out/ui-tree.json';
const outPath = process.argv[3] || path.join(here, 'out', 'tree.html');

const document_ = JSON.parse(fs.readFileSync(inputPath, 'utf8'));
const pages = document_.pages;

const frames = pages
  .map((page) => {
    const body = renderNode(adapt(page.tree));
    return `
  <figure class="frame">
    <figcaption>${page.name}</figcaption>
    <div class="phone">${body}</div>
  </figure>`;
  })
  .join('');

const dropped = droppedProps();

const html = `<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<title>甜蜜女友2 · ui-v3 实机保真预览</title>
<style>
${BASE_CSS}
body{background:#0b090a;padding:22px 18px 60px}
h1{color:#F5F0F2;font-size:17px;margin-bottom:6px}
.lead{color:#8A7D94;font-size:12.5px;line-height:1.8;max-width:980px;margin-bottom:20px}
.lead b{color:#E59BB4}
.row{display:flex;gap:20px;flex-wrap:wrap;align-items:flex-start}
figcaption{color:#8A7D94;font-size:11.5px;margin:0 0 6px 2px}
.phone{width:420px;height:860px;overflow:auto;background:#161316;border-radius:12px;
  border:1px solid #2a2226;display:flex;flex-direction:column}
.phone::-webkit-scrollbar{display:none}
.note{margin-top:26px;color:#8A7D94;font-size:12px;line-height:1.9;max-width:980px}
.note code{color:#B8ACB2;word-break:break-all}
</style>
</head>
<body>
<h1>甜蜜女友2 · ui-v3 实机保真预览</h1>
<p class="lead">
数据来自 <code>cargo test -p amakano2-ui dump_preview</code> 导出的真实节点树，
经 <code>render.rs</code> 同一套抽象键映射后，按<b>宿主属性白名单</b>渲染。
白名单外的属性整条丢弃 —— 图上看不见的，真机上也没有。
</p>
<div class="row">${frames}</div>
<div class="note">
<p>本树里出现过、但宿主会丢弃的属性：<br><code>${
  dropped.length ? dropped.join('、') : '（无）'
}</code></p>
</div>
</body>
</html>`;

fs.mkdirSync(path.dirname(outPath), { recursive: true });
fs.writeFileSync(outPath, html, 'utf8');
console.log('wrote', outPath);
console.log('pages:', pages.length, '· dropped props:', dropped.length ? dropped.join(', ') : '(none)');
