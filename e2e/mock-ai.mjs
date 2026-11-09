// A local OpenAI-compatible service for the end-to-end run. It answers by reading the prompt, the way
// a model would, and it deliberately misbehaves once per import so the guardrails have work to do.
import http from 'node:http';

const PORT = Number(process.argv[2] ?? 8765);
export const stats = { calls: 0, jobs: {} };

function passages(user) {
  const out = [];
  const re = /<passage id="(S\d+)"[^>]*>\n([\s\S]*?)\n<\/passage>/g;
  let m;
  while ((m = re.exec(user))) out.push({ id: m[1], text: m[2] });
  return out;
}

function stepFor(s) {
  if (/target|scrapped|gap/i.test(s) && !/fell/i.test(s)) return 'A';
  if (/worn|cause/i.test(s)) return 'B';
  if (/containment|Temporary|sorted/i.test(s)) return 'C';
  if (/fell|WI-204|instruction/i.test(s)) return 'D';
  return null;
}

function sentences(section) {
  return [...section.matchAll(/\[([A-D]\.\d+)\] (.+)/g)].map((m) => ({ id: m[1], text: m[2] }));
}

function answer(system, user) {
  if (system.includes('read the new file')) {
    const points = [];
    const metadata = [];
    for (const p of passages(user)) {
      for (const raw of p.text.split('\n')) {
        const line = raw.trim();
        if (!line) continue;
        if (/Bracket cracking/.test(line)) metadata.push({ field: 'title', value: line, source: p.id, quote: line });
        if (/4\.8%/.test(line)) {
          const old = sentences(user).find((s) => s.id.startsWith('A.') && /3\.5%/.test(s.text));
          if (old) points.push({ result: 'conflict', step: 'A', text: line, source: p.id, quote: line, replaces: old.id, why: 'It gives a different value for the same measure.' });
          continue;
        }
        const step = stepFor(line);
        if (step) points.push({ result: 'new', step, text: line, source: p.id, quote: line, replaces: null, why: `This is what step ${step} asks for.` });
      }
      if (/12 March 2026/.test(p.text) && !metadata.some((m) => m.field === 'problem_date')) {
        metadata.push({ field: 'problem_date', value: '2026-03-12', source: p.id, quote: '12 March 2026' });
      }
    }
    // A hallucination with a real quote and a fake number. G5 has to drop it.
    if (points.length) points.push({ result: 'new', step: 'A', text: 'Line 4 scrapped 99.9% of all parts.', source: points[0].source, quote: points[0].quote, replaces: null, why: 'x' });
    return { summary: 'A report about scrap on press line 4.', points, metadata };
  }
  if (system.includes('review this A3')) {
    const a = sentences(user).find((s) => s.id.startsWith('A.'));
    return { findings: a ? [{ code: 'V3', step: 'A', text: 'The impact is not stated as a measured gap.', why: 'The step gives a value but no gap against the standard.', source: a.id, quote: a.text.slice(0, 20) }] : [] };
  }
  if (system.includes('clearer wording') || system.includes('Shorten its sentences')) {
    const letter = (user.match(/The step to work on is step ([A-D])/) ?? [])[1];
    const s = sentences(user).filter((x) => x.id.startsWith(`${letter}.`));
    return {
      suggestions: s
        .filter((x) => /target is/.test(x.text))
        .map((x) => ({ sentence: x.id, rewrite: x.text.replace('The target is', 'Target:'), why: 'Shorter, with the same value.' })),
    };
  }
  if (system.includes('You check text against its input')) return { adds_claim_ids: [] };
  if (system.includes('draft an early warning')) {
    const a = sentences(user).filter((s) => s.id.startsWith('A.'));
    const first = a[0];
    const number = (first?.text.match(/\d+(\.\d+)?%/) ?? ['scrap'])[0];
    const areas = (user.match(/Allowed areas and products: (.*)\./) ?? [, ''])[1].split(', ').filter(Boolean);
    return {
      headline: 'Scrap rise on press line 4',
      what_happened: first ? first.text.split(' ').slice(0, 30).join(' ') : `Scrap reached ${number}.`,
      who_should_check: areas.slice(0, 1),
      quick_check: null,
      quick_check_source: null,
      quick_check_quote: null,
      source: first?.id ?? 'A.1',
      quote: first ? first.text.slice(0, 20) : '',
    };
  }
  if (system.includes("answer the user's question")) {
    const a = sentences(user).find((s) => /target/i.test(s.text)) ?? sentences(user)[0];
    return a ? { answer: a.text, sources: [{ id: a.id, quote: a.text.slice(0, 18) }] } : { answer: 'That is not in this A3.', sources: [] };
  }
  if (system.includes('Suggest up to three')) return { tasks: ['Check die 4 for cracks on your line'] };
  if (system.includes('Read the text in this image')) return { text: 'Text read from a picture.' };
  if (system.includes('Summarise this conversation')) return { summary: 'They asked about the target.' };
  return { ok: true, colour: 'red' };
}

const server = http.createServer((req, res) => {
  let body = '';
  req.on('data', (c) => (body += c));
  req.on('end', () => {
    res.setHeader('content-type', 'application/json');
    if (req.url.endsWith('/models')) {
      res.end(JSON.stringify({ data: [{ id: 'mock-large' }, { id: 'mock-small' }] }));
      return;
    }
    const b = JSON.parse(body);
    const sys = b.messages.find((m) => m.role === 'system').content;
    const user = b.messages.find((m) => m.role === 'user').content.filter((p) => p.type === 'text').map((p) => p.text).join('\n');
    stats.calls++;
    const content = JSON.stringify(answer(sys, user));
    res.end(JSON.stringify({ choices: [{ finish_reason: 'stop', message: { role: 'assistant', content } }], usage: { prompt_tokens: 100, completion_tokens: 20 } }));
  });
});
server.listen(PORT, '127.0.0.1', () => console.log(`mock AI on ${PORT}`));
