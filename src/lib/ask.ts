import type { AskKnowledge, CitedSource } from '../api';

/** One stored answer, split into what this A3 says and what came from outside it. */
export function splitSources(sources: CitedSource[] | null | undefined) {
  const all = sources ?? [];
  return {
    a3: all.filter((s) => !s.kind || s.kind === 'a3'),
    background: all.find((s) => s.kind === 'background' && s.quote.trim() !== '') ?? null,
    links: all.filter((s) => s.kind === 'web' && !!s.url),
  };
}

/** Turns up to and including the cutoff are hidden. The cutoff is a server timestamp, so clock skew between
 * this computer and the database cannot hide a new answer. */
export function afterCutoff<T extends { createdAt: string }>(messages: T[], since: string | null): T[] {
  if (!since) return messages;
  const cut = Date.parse(since);
  if (Number.isNaN(cut)) return messages;
  return messages.filter((m) => Date.parse(m.createdAt) > cut);
}

export const KNOWLEDGE_NOTE: Record<AskKnowledge, string> = {
  a3_only: 'Answers only from this A3.',
  general: "Adds the AI's general knowledge, shown separately and marked as not from this A3.",
  web: 'Also searches the web. The AI writes the search terms, and they may include details from this A3.',
};

export const WORKING_NOTE: Record<AskKnowledge, string> = {
  a3_only: 'Looking in this A3...',
  general: 'Looking in this A3 and drawing on general knowledge...',
  web: 'Looking in this A3 and searching the web...',
};

const KNOWLEDGE_VALUES: AskKnowledge[] = ['a3_only', 'general', 'web'];

export function parseKnowledge(raw: string | null | undefined): AskKnowledge {
  return KNOWLEDGE_VALUES.find((k) => k === raw) ?? 'a3_only';
}

/** Browser storage can be missing or blocked, and the panel must work without it. */
export function readPref(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

export function writePref(key: string, value: string | null) {
  try {
    if (value === null) window.localStorage.removeItem(key);
    else window.localStorage.setItem(key, value);
  } catch {
    /* the choice just lasts until the app closes */
  }
}
