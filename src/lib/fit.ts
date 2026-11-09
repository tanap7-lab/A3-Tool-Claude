import { QUAD_HEIGHT_PX, QUAD_WIDTH_PX } from './text';

export interface FitResult {
  /** How full the printed box is at the full 11 pt size. 1 means exactly full. */
  fillAtFull: number;
  /** The font size the export will settle on. */
  fontPt: number;
  /** True when the step still overflows at 8 pt and needs a continued page. */
  continued: boolean;
}

let box: HTMLDivElement | null = null;

function measureBox(): HTMLDivElement {
  if (box) return box;
  box = document.createElement('div');
  box.setAttribute('aria-hidden', 'true');
  box.style.cssText = `position:absolute;left:-10000px;top:0;visibility:hidden;width:${QUAD_WIDTH_PX}px;line-height:1.35;font-family:Roboto,"Segoe UI",Arial,sans-serif;`;
  const style = document.createElement('style');
  style.textContent = '.a3-fit p,.a3-fit ul,.a3-fit ol,.a3-fit table{margin:0 0 .45em}.a3-fit ul,.a3-fit ol{padding-left:1.3em}.a3-fit li p{margin:0}.a3-fit img{display:block;max-width:100%;height:auto;max-height:265px}.a3-fit table{border-collapse:collapse;width:100%}.a3-fit td,.a3-fit th{padding:1px 4px}';
  document.head.appendChild(style);
  box.className = 'a3-fit';
  document.body.appendChild(box);
  return box;
}

/** FR-A3-11: the same steps the export takes (section 11), measured at the printed size. */
export function measureFit(html: string): FitResult {
  const el = measureBox();
  el.innerHTML = html;
  el.style.fontSize = '11pt';
  const fillAtFull = el.scrollHeight / QUAD_HEIGHT_PX;
  let size = 11;
  while (el.scrollHeight > QUAD_HEIGHT_PX && size > 8) {
    size -= 0.5;
    el.style.fontSize = `${size}pt`;
  }
  const continued = el.scrollHeight > QUAD_HEIGHT_PX;
  el.innerHTML = '';
  return { fillAtFull, fontPt: size, continued };
}
