import { describe, expect, it } from 'vitest';
import { tokens } from './theme';

function luminance(hex: string): number {
  const c = [1, 3, 5].map((i) => parseInt(hex.substr(i, 2), 16) / 255).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
}

function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** Section 15: text contrast at least 4.5:1, for every text and fill pair the interface uses. */
describe('text contrast', () => {
  const pairs: [string, string, string][] = [
    ['white on primary button', '#FFFFFF', tokens.primary],
    ['primary link on white', tokens.primary, '#FFFFFF'],
    ['primary text on its fill', tokens.primaryText, tokens.primaryLight],
    ['success text on its fill', tokens.successText, tokens.successLight],
    ['white on closed step', '#FFFFFF', tokens.successFill],
    ['warning text on its fill', tokens.warningText, tokens.warningLight],
    ['error text on its fill', tokens.errorText, tokens.errorLight],
    ['error on white', tokens.error, '#FFFFFF'],
    ['white on error button', '#FFFFFF', tokens.error],
    ['body text on page', tokens.text, tokens.fill],
    ['secondary text on white', tokens.textSecondary, '#FFFFFF'],
    ['secondary text on page', tokens.textSecondary, tokens.fill],
    ['deleted-diff text on suggestion fill', '#C5221F', tokens.primaryLight],
    ['hint text on white', '#70757A', '#FFFFFF'],
  ];
  for (const [name, fg, bg] of pairs) {
    it(name, () => expect(contrast(fg, bg)).toBeGreaterThanOrEqual(4.5));
  }
});
