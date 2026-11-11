/** Operations on stored step JSON, kept free of the editor so they can be unit tested. */

export interface DocNode {
  type: string;
  text?: string;
  attrs?: Record<string, unknown>;
  marks?: { type: string; attrs?: Record<string, unknown> }[];
  content?: DocNode[];
}

export function storyParts(doc: DocNode): { story: DocNode[]; details: DocNode[] } {
  const story: DocNode[] = [];
  const details: DocNode[] = [];
  for (const block of doc.content ?? []) {
    if (block.type === 'a3Narrative') story.push(...(block.content ?? []));
    else if (block.type === 'a3Evidence') details.push(...(block.content ?? []));
    else details.push(block);
  }
  return { story, details };
}

/** Compares JSON documents without depending on object property order. */
export function sameDocument(a: DocNode, b: DocNode): boolean {
  const canonical = (value: unknown): unknown => {
    if (Array.isArray(value)) return value.map(canonical);
    if (value !== null && typeof value === 'object') {
      return Object.fromEntries(Object.entries(value).sort(([a], [b]) => a.localeCompare(b)).map(([key, item]) => [key, canonical(item)]));
    }
    return value;
  };
  return JSON.stringify(canonical(a)) === JSON.stringify(canonical(b));
}

/**
 * Applies an accepted rewrite (FR "Phrasing", section 9.4). Works inside one text node, keeping that
 * node's marks. A sourced sentence keeps its chip, marked edited (FR-A3-06). Returns null when the
 * sentence is no longer there as written.
 */
export function replaceSentence(doc: DocNode, original: string, rewrite: string): DocNode | null {
  let done = false;
  const walk = (n: DocNode): DocNode => {
    if (done) return n;
    if (n.type === 'text' && n.text && n.text.includes(original)) {
      done = true;
      const marks = n.marks?.map((m) => (m.type === 'source' ? { ...m, attrs: { ...m.attrs, edited: true } } : m));
      const next: DocNode = { ...n, text: n.text.replace(original, rewrite) };
      if (marks) next.marks = marks;
      return next;
    }
    if (n.content) return { ...n, content: n.content.map(walk) };
    return n;
  };
  const out = walk(doc);
  return done ? out : null;
}

/** Plain text of a document, in the same way the backend derives it. */
export function plainText(doc: DocNode | undefined | null): string {
  if (!doc) return '';
  const out: string[] = [];
  const walk = (n: DocNode) => {
    if (n.type === 'text') out.push(n.text ?? '');
    else if (n.type === 'hardBreak') out.push('\n');
    else {
      if (n.type === 'a3Narrative') out.push('At a glance\n');
      if (n.type === 'a3Evidence') out.push('Supporting evidence and details\n');
      n.content?.forEach(walk);
      if (n.type === 'paragraph') out.push('\n');
    }
  };
  walk(doc);
  return out.join('').trim();
}

/** FR-A3-06: compares a mark's text with what it held when accepted. */
export function isEdited(text: string, original: unknown): boolean {
  if (typeof original !== 'string' || !original) return false;
  const squash = (s: string) => s.replace(/\s+/g, ' ').trim();
  return squash(text) !== squash(original);
}
