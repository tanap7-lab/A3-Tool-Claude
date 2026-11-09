import { Mark, mergeAttributes } from '@tiptap/core';
import Image from '@tiptap/extension-image';
import { Table } from '@tiptap/extension-table';
import { TableCell } from '@tiptap/extension-table-cell';
import { TableHeader } from '@tiptap/extension-table-header';
import { TableRow } from '@tiptap/extension-table-row';
import Placeholder from '@tiptap/extension-placeholder';
import StarterKit from '@tiptap/starter-kit';
import { Plugin } from '@tiptap/pm/state';
import { isEdited } from '../../lib/doc';

/** FR-A3-05 and 06: marks text that came from a file. Editing it keeps the chip, marked "edited". */
export const SourceMark = Mark.create({
  name: 'source',
  inclusive: false,
  excludes: '',
  addAttributes() {
    const plain = { rendered: false };
    return {
      segmentId: { default: null, ...plain },
      label: { default: 'Source', ...plain },
      original: { default: '', ...plain },
      edited: { default: false, ...plain },
      fromImage: { default: false, ...plain },
    };
  },
  parseHTML() {
    return [
      {
        tag: 'span[data-source-segment]',
        getAttrs: (el) => {
          const e = el as HTMLElement;
          return {
            segmentId: e.getAttribute('data-source-segment'),
            label: e.getAttribute('data-source-label') ?? 'Source',
            original: e.getAttribute('data-original') ?? '',
            edited: e.getAttribute('data-edited') === 'true',
            fromImage: e.getAttribute('data-image') === 'true',
          };
        },
      },
    ];
  },
  renderHTML({ mark, HTMLAttributes }) {
    const a = mark.attrs;
    const short = String(a.label);
    const chip = a.edited ? `${short}, edited` : a.fromImage ? `${short}, from image` : short;
    return [
      'span',
      mergeAttributes(HTMLAttributes, {
        class: 'src-mark',
        'data-source-segment': a.segmentId,
        'data-source-label': a.label,
        'data-original': a.original,
        'data-edited': String(!!a.edited),
        'data-image': String(!!a.fromImage),
        'data-label': chip,
      }),
      0,
    ];
  },
  addProseMirrorPlugins() {
    const type = this.type;
    return [
      new Plugin({
        appendTransaction: (trs, _old, state) => {
          if (!trs.some((t) => t.docChanged)) return null;
          let tr = state.tr;
          let changed = false;
          state.doc.descendants((node, pos) => {
            if (!node.isText) return;
            const m = node.marks.find((x) => x.type === type);
            if (!m || m.attrs.edited) return;
            if (isEdited(node.text ?? '', m.attrs.original)) {
              tr = tr.removeMark(pos, pos + node.nodeSize, m).addMark(pos, pos + node.nodeSize, type.create({ ...m.attrs, edited: true }));
              changed = true;
            }
          });
          return changed ? tr : null;
        },
      }),
    ];
  },
});

/** Only images held inside the document. A pasted remote picture would not survive export or offline use. */
const LocalImage = Image.extend({
  parseHTML() {
    return [{ tag: 'img[src^="data:image/"]' }];
  },
}).configure({ allowBase64: true });

/** FR-A3-02: bold, italic, bullets, numbering, simple tables and pasted images. Nothing else. */
export function stepExtensions(placeholder: string) {
  return [
    StarterKit.configure({
      heading: false,
      blockquote: false,
      code: false,
      codeBlock: false,
      horizontalRule: false,
      strike: false,
      underline: false,
      link: false,
    }),
    SourceMark,
    LocalImage,
    Table.configure({ resizable: false }),
    TableRow,
    TableHeader,
    TableCell,
    Placeholder.configure({ placeholder }),
  ];
}

/** Keeps stored pictures small: screenshots scale to at most 1600 px and stay in the step's JSON. */
export async function imageFileToDataUrl(file: File): Promise<string> {
  const MAX = 1600;
  const bitmap = await createImageBitmap(file);
  const scale = Math.min(1, MAX / Math.max(bitmap.width, bitmap.height));
  const canvas = document.createElement('canvas');
  canvas.width = Math.round(bitmap.width * scale);
  canvas.height = Math.round(bitmap.height * scale);
  canvas.getContext('2d')!.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
  bitmap.close();
  return canvas.toDataURL(file.type === 'image/jpeg' ? 'image/jpeg' : 'image/png', 0.85);
}
