/** Small text helpers, kept free of React so they can be unit tested. */

/** FR-A3-11: printed size of a quadrant's text area at 96 dpi, from the export template. */
export const QUAD_WIDTH_PX = (192 * 96) / 25.4;
export const QUAD_HEIGHT_PX = (96 * 96) / 25.4;

export type DiffPart = { kind: 'same' | 'del' | 'add'; text: string };

/** Word-level diff for "struck-out and added words" (section 9.4). */
export function wordDiff(before: string, after: string): DiffPart[] {
  const a = before.split(/(\s+)/);
  const b = after.split(/(\s+)/);
  const lcs: number[][] = Array.from({ length: a.length + 1 }, () => new Array(b.length + 1).fill(0));
  for (let i = a.length - 1; i >= 0; i--) {
    for (let j = b.length - 1; j >= 0; j--) {
      lcs[i][j] = a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
    }
  }
  const parts: DiffPart[] = [];
  const push = (kind: DiffPart['kind'], text: string) => {
    const last = parts[parts.length - 1];
    if (last && last.kind === kind) last.text += text;
    else parts.push({ kind, text });
  };
  let i = 0;
  let j = 0;
  while (i < a.length || j < b.length) {
    if (i < a.length && j < b.length && a[i] === b[j]) {
      push('same', a[i]);
      i++;
      j++;
    } else if (j < b.length && (i === a.length || lcs[i][j + 1] >= lcs[i + 1][j])) {
      push('add', b[j]);
      j++;
    } else {
      push('del', a[i]);
      i++;
    }
  }
  return parts;
}

/** Finds a quote in passage text the way the guardrail does, ignoring spacing and case. */
export function findQuote(text: string, quote: string | null | undefined): [number, number] | null {
  if (!quote) return null;
  const isSpace = (c: string) => /\s/.test(c);
  const normalise = (c: string) => {
    if ('‘’‛`'.includes(c)) return "'";
    if ('“”'.includes(c)) return '"';
    if ('‐‑‒–—―−'.includes(c)) return '-';
    return c.toLowerCase();
  };
  const target = Array.from(quote).filter((c) => !isSpace(c)).map(normalise).join('');
  if (!target) return null;
  const map: number[] = [];
  let hay = '';
  for (let i = 0; i < text.length; i++) {
    if (isSpace(text[i])) continue;
    const c = normalise(text[i]);
    for (let k = 0; k < c.length; k++) map.push(i);
    hay += c;
  }
  const at = hay.indexOf(target);
  if (at < 0) return null;
  return [map[at], map[at + target.length - 1] + 1];
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

export function formatDate(d: string | null | undefined): string {
  if (!d) return 'Not set';
  const date = new Date(d.length === 10 ? `${d}T00:00:00` : d);
  return date.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
}

export function formatDateTime(d: string): string {
  return new Date(d).toLocaleString(undefined, { year: 'numeric', month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
}

export function isoDate(d: Date): string {
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

export function addDays(n: number, from = new Date()): string {
  const d = new Date(from);
  d.setDate(d.getDate() + n);
  return isoDate(d);
}

export function wordCount(s: string): number {
  const t = s.trim();
  return t ? t.split(/\s+/).length : 0;
}

/** The "3 updates suggested: B (2), D (1)" line of section 9.1. */
export function summariseProposals(letters: (string | null)[]): string {
  const steps = letters.filter((l): l is string => !!l);
  if (steps.length === 0) return '';
  const counts = new Map<string, number>();
  for (const l of steps) counts.set(l, (counts.get(l) ?? 0) + 1);
  const parts = [...counts.entries()].sort(([a], [b]) => a.localeCompare(b)).map(([l, n]) => `${l} (${n})`);
  return `${steps.length} ${steps.length === 1 ? 'update' : 'updates'} suggested: ${parts.join(', ')}`;
}

const AUDIT_LABELS: Record<string, string> = {
  a3_created: 'Created this A3',
  a3_withdrawn: 'Withdrew this A3',
  a3_exported: 'Exported',
  step_closed: 'Closed step',
  step_reopened: 'Reopened step',
  snapshot_restored: 'Restored a snapshot of step',
  attachment_deleted: 'Deleted attachment',
  attachment_restored: 'Restored attachment',
  proposal_accepted: 'Accepted a proposal',
  proposal_rejected: 'Rejected a proposal',
  warning_posted: 'Posted an early warning',
  warning_updated: 'Updated an early warning',
  warning_withdrawn: 'Withdrew an early warning',
};

export function auditLabel(action: string, detail: Record<string, unknown>): string {
  const base = AUDIT_LABELS[action] ?? action.replace(/_/g, ' ');
  if (typeof detail.letter === 'string') return `${base} ${detail.letter}`;
  if (typeof detail.fileName === 'string') return `${base}: ${detail.fileName}`;
  return base;
}
