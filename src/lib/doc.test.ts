import { describe, expect, it } from 'vitest';
import { isEdited, plainText, replaceSentence, sameDocument, storyParts, type DocNode } from './doc';

const source = { type: 'source', attrs: { segmentId: 's1', label: 'Page 1', original: 'Scrap was 3.5%.', edited: false } };
const doc: DocNode = {
  type: 'doc',
  content: [
    { type: 'paragraph', content: [{ type: 'text', text: 'Scrap was 3.5%.', marks: [source] }] },
    { type: 'bulletList', content: [{ type: 'listItem', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Line 4 stopped. Output fell.' }] }] }] },
  ],
};

describe('replaceSentence', () => {
  it('replaces inside one text node and leaves the rest alone', () => {
    const out = replaceSentence(doc, 'Output fell.', 'Output dropped.');
    expect(plainText(out)).toBe('Scrap was 3.5%.\nLine 4 stopped. Output dropped.');
  });

  it('keeps a source chip, marked edited', () => {
    const out = replaceSentence(doc, 'Scrap was 3.5%.', 'Scrap reached 3.5%.')!;
    const node = out.content![0].content![0];
    expect(node.text).toBe('Scrap reached 3.5%.');
    expect(node.marks![0].attrs!.segmentId).toBe('s1');
    expect(node.marks![0].attrs!.edited).toBe(true);
  });

  it('does not change the original document', () => {
    replaceSentence(doc, 'Scrap was 3.5%.', 'Changed.');
    expect(doc.content![0].content![0].text).toBe('Scrap was 3.5%.');
  });

  it('returns null when the sentence has gone', () => {
    expect(replaceSentence(doc, 'Not in the document.', 'x')).toBeNull();
  });
});

describe('isEdited', () => {
  it('ignores spacing changes', () => {
    expect(isEdited('Scrap  was 3.5%.', 'Scrap was 3.5%.')).toBe(false);
    expect(isEdited('Scrap was 4.5%.', 'Scrap was 3.5%.')).toBe(true);
    expect(isEdited('anything', undefined)).toBe(false);
  });

  describe('storyParts', () => {
    it('separates the readable story from all underlying and newly appended details', () => {
      const story = { type: 'paragraph', content: [{ type: 'text', text: 'Scrap on line 4 was 3.5%.' }] };
      const extra = { type: 'paragraph', content: [{ type: 'text', text: 'Validation is pending.' }] };
      const full: DocNode = { type: 'doc', content: [
        { type: 'a3Narrative', attrs: { baseline: 'At a glance\nScrap on line 4 was 3.5%.' }, content: [story] },
        { type: 'a3Evidence', content: doc.content },
        extra,
      ] };
      expect(storyParts(full)).toEqual({ story: [story], details: [...doc.content!, extra] });
      expect(plainText(full)).toBe('At a glance\nScrap on line 4 was 3.5%.\nSupporting evidence and details\nScrap was 3.5%.\nLine 4 stopped. Output fell.\nValidation is pending.');
      expect(storyParts(doc)).toEqual({ story: [], details: doc.content });
    });
  });

  describe('sameDocument', () => {
    it('ignores object property ordering from the backend', () => {
      expect(sameDocument(doc, JSON.parse(JSON.stringify(doc)))).toBe(true);
      expect(sameDocument(
        { type: 'text', text: 'A', marks: [{ type: 'source', attrs: { segmentId: 's1', edited: false } }] },
        { marks: [{ attrs: { edited: false, segmentId: 's1' }, type: 'source' }], text: 'A', type: 'text' },
      )).toBe(true);
    });

    it('detects changed text, source marks, structure and block order', () => {
      expect(sameDocument(doc, replaceSentence(doc, 'Scrap was 3.5%.', 'Scrap was 4.5%.')!)).toBe(false);
      expect(sameDocument(doc, { ...doc, content: [...doc.content!].reverse() })).toBe(false);
      expect(sameDocument(doc, { ...doc, content: doc.content!.slice(0, 1) })).toBe(false);
      const changed = structuredClone(doc);
      changed.content![0].content![0].marks![0].attrs!.segmentId = 's2';
      expect(sameDocument(doc, changed)).toBe(false);
    });
  });
});
