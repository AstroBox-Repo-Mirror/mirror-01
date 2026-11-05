/**
 * ui-v3 原型渲染器 —— **只认宿主白名单里的属性**。
 *
 * 为什么不能直接用浏览器渲染节点树：宿主 `ui-v3` 的 `prop()` 只接受一张固定表
 * （从 `AstroBox-ng.exe` 的字符串表里读出来的，见 `HOST_PROP_WHITELIST`），
 * 表外的属性**整条丢弃**，不报错。用普通 CSS 渲染就会出现「预览里字重分明、
 * 真机上全是一个粗细」这种假象 —— 这个项目 2026-09 已经吃过一次亏。
 *
 * 所以这里的规则是：节点树里出现宿主不认的属性，一律**当作不存在**渲染，
 * 并在页脚列出来。屏幕上看到的就是真机上会看到的。
 */

// ---- 宿主真正接受的属性（来自宿主二进制的字符串表，逐字抄）----
// 注意：这张表里**没有** font-weight / letter-spacing / line-height / text-align /
// flex-wrap / overflow / box-shadow / background-image / background-color。
export const HOST_PROP_WHITELIST = new Set([
  'margin-top', 'margin-bottom', 'margin-left', 'margin-right',
  'padding-top', 'padding-bottom', 'padding-left', 'padding-right',
  'color', 'transition', 'align-items', 'justify-content',
  'width', 'height', 'flex-shrink', 'flex-grow',
  'row', 'column', 'row-reverse', 'column-reverse', 'flex-direction',
  'background', 'top', 'display', 'left', 'font-size', 'right',
  'bottom', 'border', 'radius', 'margin', 'border-radius', 'opacity',
  'padding', 'z-index', 'position', 'disabled', 'animation',
  'autofocus', 'max-width', 'min-width', 'tab-index', 'transform',
  'max-height', 'min-height', 'scroll-top', 'scroll-left', 'will-change',
  'animation-name', 'backdrop-filter', 'scroll-behavior',
  'animation-timing-function', 'animation-preset', 'transform-origin',
  'animation-delay', 'animation-direction', 'animation-fill-mode',
  'backface-visibility', 'animation-play-state', 'animation-duration',
  'grid-template-columns', 'animation-iteration-count', 'gap', 'filter',
  'perspective', 'display',
]);

// 类型化方法能设的（一定生效）：
const TYPED = {
  display: (v) => ({ display: v }),
  'flex-direction': (v) => ({ 'flex-direction': v }),
  'align-items': (v) => ({ 'align-items': v }),
  'justify-content': (v) => ({ 'justify-content': v }),
  gap: (v) => ({ gap: px(v) }),
  'flex-grow': (v) => ({ 'flex-grow': String(v), 'min-width': '0' }),
  'flex-shrink': (v) => ({ 'flex-shrink': String(v) }),
  padding: (v) => ({ padding: px(v) }),
  'padding-top': (v) => ({ 'padding-top': px(v) }),
  'padding-bottom': (v) => ({ 'padding-bottom': px(v) }),
  'padding-left': (v) => ({ 'padding-left': px(v) }),
  'padding-right': (v) => ({ 'padding-right': px(v) }),
  margin: (v) => ({ margin: px(v) }),
  'margin-top': (v) => ({ 'margin-top': px(v) }),
  'margin-bottom': (v) => ({ 'margin-bottom': px(v) }),
  'margin-left': (v) => ({ 'margin-left': px(v) }),
  'margin-right': (v) => ({ 'margin-right': px(v) }),
  width: (v) => ({ width: v === 'full' ? '100%' : v === 'half' ? '50%' : px(v) }),
  height: (v) => ({ height: v === 'full' ? '100%' : v === 'half' ? '50%' : px(v) }),
  'min-width': (v) => ({ 'min-width': px(v) }),
  'min-height': (v) => ({ 'min-height': px(v) }),
  'max-width': (v) => ({ 'max-width': px(v) }),
  'max-height': (v) => ({ 'max-height': px(v) }),
  radius: (v) => ({ 'border-radius': px(v) }),
  border: (v) => ({ border: borderValue(v) }),
  background: (v) => ({ background: v }),
  color: (v) => ({ color: v }),
  'font-size': (v) => ({ 'font-size': px(v) }),
  opacity: (v) => ({ opacity: String(v) }),
  transform: (v) => ({ transform: v }),
  'transform-origin': (v) => ({ 'transform-origin': v }),
  transition: (v) => ({ transition: v }),
  filter: (v) => ({ filter: v }),
  'backdrop-filter': (v) => ({ 'backdrop-filter': v }),
  'z-index': (v) => ({ 'z-index': String(v) }),
  position: (v) => ({ position: v }),
  top: (v) => ({ top: px(v) }),
  bottom: (v) => ({ bottom: px(v) }),
  left: (v) => ({ left: px(v) }),
  right: (v) => ({ right: px(v) }),
  'grid-template-columns': (v) => ({ 'grid-template-columns': v }),
};

const px = (v) => (/^-?\d+(\.\d+)?$/.test(String(v).trim()) ? `${String(v).trim()}px` : String(v));
const borderValue = (v) => {
  const parts = String(v).trim().split(/\s+/);
  if (parts.length === 1) return `1px solid ${parts[0]}`;
  return `${px(parts[0])} solid ${parts.slice(1).join(' ')}`;
};

const dropped = new Set();

/**
 * 节点 → HTML。
 * node: { tag, text?, props?: {cssProp: value}, children?: [] }
 * `tag` 用宿主元素类型的小写：div / p / span / button / badge / progress / scroll / image …
 */
export function renderNode(node) {
  const tag = node.tag || 'div';
  const style = {};
  for (const [key, value] of Object.entries(node.props || {})) {
    const typed = TYPED[key];
    if (typed) Object.assign(style, typed(value));
    else if (HOST_PROP_WHITELIST.has(key)) Object.assign(style, { [key]: px(value) });
    // 白名单外：宿主整条丢弃，这里也整条丢弃 —— 预览与真机必须同一结果。
    else dropped.add(key);
  }
  const htmlTag = { badge: 'span', progress: 'div', switch: 'div', image: 'div', scroll: 'div', svg: 'div' }[tag] || tag;
  const css = Object.entries(style)
    .map(([k, v]) => `${k}:${v}`)
    .join(';');
  const attrs = [`data-tag="${tag}"`];
  if (css) attrs.push(`style="${css.replace(/"/g, '&quot;')}"`);
  if (node.disabled) attrs.push('data-disabled="1"');
  // 元素默认样式：只给「块/行内/按钮」的浏览器默认值，不给任何字体粗细 ——
  // 宿主没有 font-weight，真机上所有字都是同一档粗细。
  const cls = `u-${tag}`;
  let inner;
  if (tag === 'svg') {
    // SVG 元素的内容是 markup，原样注入（宿主那边是元素内容，不经过文本转义）。
    inner = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" id="ic">${node.text || ''}</svg>`;
  } else if (tag === 'image') {
    // 图片元素的内容是地址；预览里指向仓库里的真图标。
    inner = `<img class="imgsrc" src="../../web/icon.png" alt="">`;
  } else if (tag === 'switch') {
    // SWITCH 的内部结构由宿主画，节点本身不收子元素也不收文本。
    inner = '';
  } else {
    inner = escape(node.text || '') + (node.children || []).map(renderNode).join('');
  }
  return `<${htmlTag} class="${cls}" ${attrs.join(' ')}>${inner}</${htmlTag}>`;
}

const escape = (s) =>
  String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

export function droppedProps() {
  return [...dropped];
}

/** 页面外壳：只有宿主白名单能表达的东西。 */
export const BASE_CSS = `
*{margin:0;padding:0;box-sizing:border-box}
html,body{height:100%}
body{background:#161316;color:#F5F0F2;
  /* 宿主不认 font-family，这里用系统字是为了让截图接近设备字体宽度，不代表能下发 */
  font-family:-apple-system,"PingFang SC","Microsoft YaHei",sans-serif;
  /* 宿主没有 font-weight：全局锁 400，任何地方都粗不起来 —— 这是本次改版最大的约束 */
  font-weight:400;-webkit-font-smoothing:antialiased}
.u-div{display:block}
.u-p{display:block}
.u-span{display:inline}
.u-button{cursor:pointer;text-align:center;border:0;appearance:none;font:inherit}
.u-badge{display:inline-block}
.u-scroll{overflow:auto}
.u-image{position:relative;overflow:hidden;background:#2a2226;border-radius:8px}
.imgsrc{width:100%;height:100%;object-fit:cover;display:block}
.u-switch{background:#3A3136;border-radius:999px;position:relative}
/* 开关：真机由宿主画，预览里画一个等价外形（开=粉底 + 右侧白块）。 */
.u-switch::after{content:"";position:absolute;top:3px;right:3px;width:19px;height:19px;border-radius:50%;background:#fff}
.u-svg{display:inline-block;line-height:0}
.u-svg svg{width:100%;height:100%;display:block}
.u-badge{white-space:nowrap}
.u-progress{}
/* 滚动条在宿主里不显示 */
::-webkit-scrollbar{display:none}
`;
