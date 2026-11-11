import { describe, expect, it } from 'vitest';
import { addDays, findQuote, summariseProposals, wordCount, wordDiff } from './text';

describe('wordDiff', () => {
  it('marks struck and added words and keeps the rest', () => {
    const d = wordDiff('The die was worn out', 'The die wore out');
    expect(d.filter((p) => p.kind === 'same').map((p) => p.text).join('')).toContain('The die');
    expect(d.some((p) => p.kind === 'del' && p.text.includes('was'))).toBe(true);
    expect(d.some((p) => p.kind === 'add' && p.text.includes('wore'))).toBe(true);
  });

  it('rebuilds both sides exactly', () => {
    const before = 'Scrap rose to 3.5% on line 4.';
    const after = 'Line 4 scrap rose to 3.5%.';
    const d = wordDiff(before, after);
    expect(d.filter((p) => p.kind !== 'add').map((p) => p.text).join('')).toBe(before);
    expect(d.filter((p) => p.kind !== 'del').map((p) => p.text).join('')).toBe(after);
  });

  it('handles large section rewrites without a quadratic matrix and preserves both versions', () => {
    const before = `Header\n${'old text '.repeat(10_000)}\nEnd`;
    const after = `Header\n${'new wording '.repeat(10_000)}\nEnd`;
    const d = wordDiff(before, after);
    expect(d.length).toBeLessThanOrEqual(4);
    expect(d.filter((p) => p.kind !== 'add').map((p) => p.text).join('')).toBe(before);
    expect(d.filter((p) => p.kind !== 'del').map((p) => p.text).join('')).toBe(after);
    expect(wordDiff(before, before)).toEqual([{ kind: 'same', text: before }]);
  });
});

describe('findQuote', () => {
  it('ignores spacing and case like the guardrail', () => {
    const text = 'Scrap  rose\nto 3.5% on line 4';
    const hit = findQuote(text, 'scrap rose to 3.5%');
    expect(hit).not.toBeNull();
    expect(text.slice(hit![0], hit![1])).toBe('Scrap  rose\nto 3.5%');
  });

  it('treats typographic dashes and quotes as the same', () => {
    const text = 'Line 4–B said “stop”';
    expect(findQuote(text, 'line 4-b said "stop"')).not.toBeNull();
  });

  it('returns null when absent or empty', () => {
    expect(findQuote('abc', 'xyz')).toBeNull();
    expect(findQuote('abc', '   ')).toBeNull();
    expect(findQuote('abc', null)).toBeNull();
  });
});

describe('summaries and counts', () => {
  it('writes the spec line', () => {
    expect(summariseProposals(['D', 'B', 'B'])).toBe('3 updates suggested: B (2), D (1)');
    expect(summariseProposals(['A'])).toBe('1 update suggested: A (1)');
    expect(summariseProposals([null])).toBe('');
  });

  it('counts words', () => {
    expect(wordCount('  one two  three ')).toBe(3);
    expect(wordCount('   ')).toBe(0);
  });

  it('adds days across a month end', () => {
    expect(addDays(1, new Date(2026, 0, 31))).toBe('2026-02-01');
    expect(addDays(30, new Date(2026, 10, 9))).toBe('2026-12-09');
  });
});
