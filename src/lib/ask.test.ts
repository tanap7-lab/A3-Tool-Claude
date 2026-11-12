import { describe, expect, it } from 'vitest';
import { afterCutoff, parseKnowledge, splitSources } from './ask';

describe('splitSources', () => {
  it('keeps A3 citations apart from background and web pages', () => {
    const got = splitSources([
      { segmentId: 's1', label: 'Report, page 2', quote: 'target is 1.0%' },
      { segmentId: null, label: 'A.1', quote: 'x', kind: 'a3' },
      { segmentId: null, label: 'General knowledge', quote: 'Scrap is rejected material.', kind: 'background' },
      { segmentId: null, label: 'Scrap guide', quote: '', kind: 'web', url: 'https://example.com/scrap' },
      { segmentId: null, label: 'No address', quote: '', kind: 'web', url: null },
    ]);
    expect(got.a3).toHaveLength(2);
    expect(got.background?.quote).toBe('Scrap is rejected material.');
    expect(got.links.map((l) => l.url)).toEqual(['https://example.com/scrap']);
  });

  it('reads messages stored before background existed', () => {
    const got = splitSources([{ segmentId: 's1', label: 'Report', quote: 'q' }]);
    expect(got.a3).toHaveLength(1);
    expect(got.background).toBeNull();
    expect(splitSources(undefined)).toEqual({ a3: [], background: null, links: [] });
  });

  it('ignores an empty background', () => {
    expect(splitSources([{ segmentId: null, label: 'General knowledge', quote: '  ', kind: 'background' }]).background).toBeNull();
  });
});

describe('afterCutoff', () => {
  const m = (id: string, createdAt: string) => ({ id, createdAt });
  const list = [m('a', '2026-11-12T10:00:00Z'), m('b', '2026-11-12T10:00:00.250000Z'), m('c', '2026-11-12T10:05:00.5Z')];

  it('shows everything when nothing was cleared', () => {
    expect(afterCutoff(list, null)).toHaveLength(3);
  });

  it('hides turns up to and including the cutoff, whatever the fractional digits', () => {
    expect(afterCutoff(list, '2026-11-12T10:00:00.250000Z').map((x) => x.id)).toEqual(['c']);
    expect(afterCutoff(list, '2026-11-12T10:05:00.5Z')).toEqual([]);
  });

  it('shows everything if the cutoff cannot be read', () => {
    expect(afterCutoff(list, 'not a date')).toHaveLength(3);
  });
});

describe('parseKnowledge', () => {
  it('falls back to this A3 only', () => {
    expect(parseKnowledge('web')).toBe('web');
    expect(parseKnowledge('general')).toBe('general');
    expect(parseKnowledge('anything else')).toBe('a3_only');
    expect(parseKnowledge(null)).toBe('a3_only');
  });
});
