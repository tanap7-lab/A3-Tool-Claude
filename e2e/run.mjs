// Drives the real exe through its WebView, against the real dev database and a mock AI service.
//   node e2e/run.mjs            (needs: scripts\dev-db.ps1, a fresh schema, a built target\debug\a3-sheet.exe)
//
// Native file dialogs cannot be scripted, so adding files and choosing an export path go through the
// same commands those dialogs call. Everything else is real clicks and typing.
import { spawn, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { chromium } from 'playwright-core';

const root = path.resolve(import.meta.dirname, '..');
const work = path.join(root, '.dev', 'e2e');
const shots = path.join(work, 'shots');
const fixtures = path.join(work, 'fixtures');
const exports_ = path.join(work, 'exports');
const exe = process.env.A3_E2E_EXE ?? path.join(root, 'target', 'debug', 'a3-sheet.exe');
for (const d of [shots, fixtures, exports_]) fs.mkdirSync(d, { recursive: true });
execFileSync('python', [path.join(root, 'e2e', 'make_fixtures.py'), fixtures], { stdio: 'ignore' });

const mock = spawn('node', [path.join(root, 'e2e', 'mock-ai.mjs'), '8765'], { stdio: 'ignore' });
let app = spawn(exe, [], {
  env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-port=9333' },
  stdio: 'ignore',
});

let passed = 0;
const failures = [];
const consoleErrors = [];
function ok(cond, label) {
  if (cond) {
    passed++;
    console.log(`  ok   ${label}`);
  } else {
    failures.push(label);
    console.log(`  FAIL ${label}`);
  }
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
/** Polls until the check passes. Waiting on data, not on a passing toast, keeps the run steady. */
async function until(check, ms = 20000) {
  const end = Date.now() + ms;
  for (;;) {
    const v = await check();
    if (v) return v;
    if (Date.now() > end) return v;
    await sleep(250);
  }
}

async function connect() {
  for (let i = 0; i < 60; i++) {
    try {
      return await chromium.connectOverCDP('http://127.0.0.1:9333');
    } catch {
      await sleep(500);
    }
  }
  throw new Error('could not reach the WebView');
}

async function main() {
  const browser = await connect();
  const ctx = browser.contexts()[0];
  let page = ctx.pages()[0];
  for (let i = 0; i < 40 && !page; i++) {
    await sleep(250);
    page = browser.contexts()[0].pages()[0];
  }
  page.on('console', (m) => m.type() === 'error' && consoleErrors.push(m.text()));
  page.on('pageerror', (e) => consoleErrors.push(String(e)));
  await page.waitForLoadState('domcontentloaded');
  page.setDefaultTimeout(20000);
  const invoke = (cmd, args) => page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);
  const shot = (name) => page.screenshot({ path: path.join(shots, `${name}.png`) });
  const goto = (hash) => page.evaluate((h) => { location.hash = h; }, hash);

  console.log('Sign-in and connection');
  await page.getByRole('heading', { name: 'A3 Sheet' }).or(page.getByText('A3 Sheet').first()).waitFor();
  const needsProfile = await page.getByText('Database connection').isVisible().catch(() => false);
  if (needsProfile) await page.getByRole('button', { name: 'Save' }).click();
  await page.getByLabel('Password').waitFor();
  await page.getByLabel('Login name').fill('paul');
  await page.getByLabel('Password').fill('wrong-password');
  await page.getByRole('button', { name: 'Sign in' }).click();
  await page.getByText('That login name or password is not right.').waitFor();
  ok(true, 'a wrong password shows one plain message');
  await page.getByLabel('Password').fill(process.env.A3_E2E_PASSWORD);
  await page.getByText('Keep me signed in on this computer').click();
  await page.getByRole('button', { name: 'Sign in' }).click();
  await page.getByRole('tab', { name: 'My Work' }).waitFor();
  ok(true, 'signed in with a personal PostgreSQL login');

  const scheme = await page.evaluate(() => getComputedStyle(document.documentElement).colorScheme);
  ok(scheme.includes('light'), `the interface forces light mode (color-scheme: ${scheme})`);
  await shot('01-dashboard-empty');

  console.log('Settings: users, project, pick-lists');
  await goto('/settings?tab=admin');
  await page.getByRole('button', { name: 'Create user' }).click();
  await page.getByLabel('Login name').fill('e2e.member');
  await page.getByLabel('Display name').fill('Mina Member');
  await page.getByLabel('First password').fill('member-password');
  await page.getByRole('dialog').getByRole('button', { name: 'Create' }).click();
  await page.getByRole('cell', { name: 'Mina Member', exact: true }).waitFor();
  ok(true, 'an admin creates a user with no self-registration');
  await shot('02-admin');

  await goto('/settings?tab=projects');
  await page.getByRole('button', { name: 'New project' }).first().click();
  await page.getByRole('dialog').getByLabel('Name').fill('Press shop');
  await page.getByRole('dialog').getByLabel('Short code').fill('PRESS');
  await page.getByRole('dialog').getByRole('button', { name: 'Create' }).click();
  await page.getByRole('heading', { name: 'Press shop' }).waitFor();
  await page.getByLabel('Add a area').fill('Line 4');
  await page.getByLabel('Add a area').press('Enter');
  await page.getByText('Line 4', { exact: true }).waitFor();
  await page.getByLabel('Add a area').fill('Line 7');
  await page.getByLabel('Add a area').press('Enter');
  await page.getByText('Line 7', { exact: true }).waitFor();
  ok(true, 'a project and its pick-lists are created by clicks');
  await page.getByPlaceholder('Add a person').click();
  await page.getByRole('option', { name: /Mina Member/ }).click();
  await page.getByText('Mina Member', { exact: true }).first().waitFor();
  ok(true, 'a project owner adds a member with a people picker');
  await shot('03-project');

  console.log('No AI key: everything else works');
  const blank = await page.evaluate(async () => {
    const projects = await window.__TAURI_INTERNALS__.invoke('projects_list', { onlyMine: true });
    return projects[0].id;
  });
  await page.getByRole('button', { name: 'New A3' }).click();
  await page.getByRole('dialog').getByRole('button', { name: 'Create blank A3' }).click();
  await page.getByRole('tab', { name: 'Overview' }).waitFor();
  const blankId = page.url().split('#/a3/')[1].split('?')[0];
  await goto(`/a3/${blankId}?tab=attachments`);
  await page.getByRole('button', { name: 'Add files' }).waitFor();
  await invoke('attachments_add', { sheet: blankId, paths: [path.join(fixtures, '8D report.pdf')] });
  await page.getByText('8D report.pdf').first().waitFor();
  await page.getByText('Read', { exact: true }).waitFor({ timeout: 30000 });
  ok(true, 'with no key, a PDF is stored and read locally, through the child-process reader');
  await shot('04-attachments-no-ai');
  await goto(`/a3/${blankId}/review`);
  await page.getByText('Set up AI in Settings').waitFor();
  await page.getByText('Scrapped').or(page.getByText(/scrapped 3\.5%/)).first().waitFor();
  await page.getByRole('button', { name: 'Add to step A' }).first().click();
  const placedA = await until(async () => (await invoke('sheet_detail', { id: blankId })).steps[0].plainText.includes('3.5%'));
  ok(placedA, 'FR-IMP-10: a passage is placed in a step without AI');
  await shot('05-manual-placement');
  // A real pointer drag into step B.
  const handle = page.locator('[data-testid="drag"], .ms:text("drag_indicator")').nth(1);
  const target = page.locator('[data-drop-step="B"]');
  const hb = await handle.boundingBox();
  const tb = await target.boundingBox();
  await page.mouse.move(hb.x + 4, hb.y + 4);
  await page.mouse.down();
  await page.mouse.move(tb.x + 10, tb.y + 10, { steps: 6 });
  await page.mouse.up();
  const placedB = await until(async () => (await invoke('sheet_detail', { id: blankId })).steps[1].plainText.includes('Die 4'));
  ok(placedB, 'FR-IMP-10: a passage is dragged into a step');
  const noAi = await invoke('sheet_detail', { id: blankId });
  ok(noAi.steps[0].plainText.includes('3.5%') && noAi.steps[0].content.content[0].content[0].marks?.[0]?.type === 'source', 'the placed passage keeps its source chip');
  await goto(`/a3/${blankId}?tab=sheet`);
  await page.getByRole('heading', { name: 'A. Define the problem' }).waitFor();
  await page.getByText('Set up AI in Settings').waitFor();
  ok(true, 'FR-AI-07: AI buttons say Set up AI in Settings');

  console.log('AI settings');
  await goto('/settings?tab=ai');
  await page.getByLabel('Provider').waitFor();
  await page.getByRole('combobox', { name: 'Provider' }).click().catch(async () => page.getByLabel('Provider').click());
  await page.getByRole('option', { name: /Custom/ }).click();
  await page.getByLabel('Address').fill('http://localhost:8765/v1');
  await page.getByLabel('API key').fill('e2e-test-key');
  await page.getByRole('button', { name: 'Fetch list' }).click();
  await page.getByLabel('Model').click();
  await page.getByRole('option', { name: 'mock-large' }).click();
  await page.getByRole('button', { name: 'Save' }).click();
  await page.getByText('AI settings saved.').waitFor();
  await page.getByRole('button', { name: 'Test connection' }).click();
  await page.getByText('Image reading works').waitFor();
  const testText = await page.getByRole('status').filter({ hasText: 'Key works' }).innerText();
  ok(!/not working/.test(testText), 'FR-AI-04: Test connection reports all three checks');
  await shot('06-ai-settings');
  const keyOnDisk = fs.readdirSync(path.join(os.homedir(), 'AppData', 'Roaming', 'com.a3sheet.desktop')).filter((f) => f.startsWith('ai-')).map((f) => fs.readFileSync(path.join(os.homedir(), 'AppData', 'Roaming', 'com.a3sheet.desktop', f), 'utf8')).join('');
  ok(!keyOnDisk.includes('e2e-test-key'), 'FR-AI-02: the API key is not in any settings file');

  console.log('The fast path: files to a reviewed A3');
  const started = Date.now();
  const id = await invoke('sheet_create', { new: { title: '', projectId: blank } });
  const added = await invoke('attachments_add', { sheet: id, paths: ['8D report.pdf', 'Investigation.docx', 'Scrap data.xlsx', 'Review deck.pptx'].map((f) => path.join(fixtures, f)) });
  ok(added.added.length === 4 && added.errors.length === 0, 'PDF, DOCX, XLSX and PPTX are all accepted');
  await goto(`/a3/${id}/review`);
  await page.getByRole('heading', { name: /Review A3-/ }).waitFor();
  await page.getByRole('button', { name: 'Accept' }).first().waitFor({ timeout: 60000 });
  for (let i = 0; i < 60; i++) {
    const atts = await invoke('attachments_list', { sheet: id });
    if (atts.every((a) => a.aiState === 'done' || a.aiState === 'failed')) break;
    await sleep(500);
  }
  const atts = await invoke('attachments_list', { sheet: id });
  ok(atts.every((a) => a.readState === 'read'), `every type was read locally (${atts.map((a) => a.readState).join(', ')})`);
  ok(atts.every((a) => a.aiState === 'done'), `AI finished on every file (${atts.map((a) => `${a.fileType}:${a.aiState}`).join(', ')})`);

  let props = await invoke('proposals_list', { sheet: id, pendingOnly: true });
  ok(props.length > 0, `proposals appeared (${props.length})`);
  ok(!props.some((p) => p.proposedText.includes('99.9')), 'G5: the injected fake number is not on any card');
  const beforeAccept = await invoke('sheet_detail', { id });
  ok(beforeAccept.steps.every((s) => s.plainText === ''), 'FR-IMP-03: no step text changes until a proposal is accepted');
  ok(props.some((p) => p.kind === 'metadata' && p.field === 'title'), 'FR-IMP-05: title is proposed in the metadata block');
  const sources = new Set(props.map((p) => p.sourceLabel?.split(',')[0]));
  ok(sources.size >= 3, `proposals trace to several files (${[...sources].join(' | ')})`);
  await page.getByRole('button', { name: /Accept/ }).first().waitFor();
  await sleep(600);
  await shot('07-review');

  // Each proposal opens its source. Click a PDF card and check the highlighted passage.
  const pdfCard = page.locator('.MuiCard-root').filter({ hasText: 'The target is 1.0%' }).first();
  await pdfCard.click();
  await page.locator('mark.src-hit').first().waitFor();
  const hit = await page.locator('mark.src-hit').first().innerText();
  ok(hit.length > 5, `the source pane highlights the cited passage: "${hit.slice(0, 40)}"`);
  await page.getByText(/Page \d/).first().waitFor();
  await shot('08-review-source');

  // Accept with one click, edit one, reject one, then accept the rest.
  await page.locator('.MuiCard-root').filter({ hasText: 'Details' }).getByRole('button', { name: 'Accept all' }).click();
  await sleep(800);
  const targetCard = page.locator('.MuiCard-root').filter({ hasText: 'The target is 1.0%' }).first();
  await targetCard.getByRole('button', { name: 'Edit' }).click();
  await targetCard.getByLabel('Edit proposed text').fill('The target is 1.0% scrap for this part.');
  await targetCard.getByRole('button', { name: 'Accept edit' }).click();
  await sleep(600);
  props = await invoke('proposals_list', { sheet: id, pendingOnly: true });
  const toReject = props.find((p) => p.letter === 'D') ?? props.find((p) => p.kind === 'add');
  await page.locator('.MuiCard-root').filter({ hasText: toReject.proposedText.slice(0, 30) }).first().getByRole('button', { name: 'Reject' }).click();
  await sleep(500);
  for (let guard = 0; guard < 8; guard++) {
    const left = await invoke('proposals_list', { sheet: id, pendingOnly: true });
    if (!left.length) break;
    const b = page.getByRole('button', { name: 'Accept all' }).first();
    if (await b.isVisible().catch(() => false)) await b.click();
    await sleep(800);
  }
  await page.getByRole('heading', { name: 'All reviewed' }).waitFor();
  await page.getByText('The impact is not stated as a measured gap.').waitFor({ timeout: 15000 });
  ok(true, 'after the review ends, AI reviews the result once');
  await shot('09-review-done');
  console.log(`  (files to reviewed A3 took ${Math.round((Date.now() - started) / 1000)}s under script)`);
  const reviewed = await invoke('sheet_detail', { id });
  ok(reviewed.sheet.title.includes('Bracket'), `the title came from the file: ${reviewed.sheet.title}`);
  ok(reviewed.sheet.problemDate === '2026-03-12', 'the problem date came from the file');
  ok(reviewed.steps[0].plainText.includes('1.0% scrap for this part'), 'the edited text reached step A');
  const rejected = toReject.proposedText;
  ok(!reviewed.steps.some((s) => s.plainText.includes(rejected)), 'the rejected point stayed out');

  console.log('Sheet tab: step rules, editing, source chips, phrasing');
  await goto(`/a3/${id}?tab=sheet`);
  await page.getByRole('heading', { name: 'A. Define the problem' }).waitFor();
  await sleep(500);
  await shot('10-sheet-quadrant');
  const boxes = await page.evaluate(() => Object.fromEntries(['A', 'B', 'C', 'D'].map((l) => {
    const h = [...document.querySelectorAll('h2')].find((x) => x.textContent.startsWith(`${l}.`));
    const r = h.closest('.MuiCard-root').getBoundingClientRect();
    return [l, { x: r.x, y: r.y }];
  })));
  ok(boxes.A.x < boxes.C.x && boxes.A.y < boxes.B.y && boxes.B.x < boxes.D.x && boxes.C.y < boxes.D.y, 'Quadrant view: A top-left, B bottom-left, C top-right, D bottom-right');
  const bClose = page.locator('.MuiCard-root').filter({ has: page.getByRole('heading', { name: 'B. Analyze root cause' }) }).getByRole('button', { name: 'Close step' });
  ok(await bClose.isDisabled(), 'B cannot close while A is open');

  const dEditor = page.getByLabel('Step D text');
  await dEditor.click();
  await page.keyboard.type('Scrap held at 0.9% through April 2026. ');
  await page.getByText('Saved').first().waitFor({ timeout: 8000 });
  let after = await invoke('sheet_detail', { id });
  ok(after.steps[3].plainText.includes('Scrap held at 0.9%'), 'FR-A3-03: typing autosaves after 2 seconds');

  const srcCount = await page.locator('.src-mark').count();
  ok(srcCount > 0, `FR-A3-05: sourced sentences show chips (${srcCount})`);
  const chip = page.locator('.src-mark').first();
  const chipBox = await chip.evaluate((el) => { const r = el.getClientRects(); const l = r[r.length - 1]; return { x: l.right - 6, y: l.top + l.height / 2 }; });
  await page.mouse.click(chipBox.x, chipBox.y);
  await page.getByRole('button', { name: 'Close source' }).waitFor();
  await sleep(400);
  await shot('11-source-drawer');
  await page.getByRole('button', { name: 'Close source' }).click();
  await sleep(300);

  // Phrasing: Improve shows a before and after, accepted with one click.
  const aEditor = page.getByLabel('Step A text');
  await aEditor.click();
  await page.locator('.MuiCard-root').filter({ has: page.getByRole('heading', { name: 'A. Define the problem' }) }).getByRole('button', { name: 'Improve' }).click();
  await page.locator('.diff-add').first().waitFor();
  await shot('12-phrasing');
  await page.getByRole('button', { name: 'Accept', exact: true }).first().click();
  await page.getByText('Saved').first().waitFor({ timeout: 8000 });
  await sleep(600);
  after = await invoke('sheet_detail', { id });
  ok(after.steps[0].plainText.includes('Target:'), 'an accepted phrasing suggestion changes only that sentence');
  const marks = JSON.stringify(after.steps[0].content);
  ok(marks.includes('"edited":true'), 'FR-A3-06: the edited sentence keeps its chip, marked edited');

  // Close A, try to edit it, reopen with a reason chip.
  await page.locator('.MuiCard-root').filter({ has: page.getByRole('heading', { name: 'A. Define the problem' }) }).getByRole('button', { name: 'Close step' }).click();
  await page.getByText('Step A closed.').waitFor();
  await sleep(500);
  after = await invoke('sheet_detail', { id });
  ok(after.steps[0].state === 'closed' && after.sheet.currentStep === 'B', 'closing A moves the current step to B');
  ok((await aEditor.getAttribute('contenteditable')) === 'false', 'FR-ST-06: a closed step is read-only');
  await page.locator('.MuiCard-root').filter({ has: page.getByRole('heading', { name: 'A. Define the problem' }) }).getByRole('button', { name: 'Reopen' }).click();
  await page.getByRole('dialog').getByText('New evidence').click();
  await sleep(600);
  after = await invoke('sheet_detail', { id });
  ok(after.steps[0].state === 'open' && after.sheet.reopened, 'reopening needs a reason chip and raises the flag');
  await page.locator('.MuiCard-root').filter({ has: page.getByRole('heading', { name: 'A. Define the problem' }) }).getByRole('button', { name: 'Close step' }).click();
  await sleep(600);

  // Assistant: at most three findings, source chips, Ask.
  await page.getByLabel('Ask about this A3').fill('What is the target?');
  await page.getByLabel('Ask about this A3').press('Enter');
  await page.getByRole('button', { name: 'Tell me more' }).waitFor();
  await shot('13-assistant');
  ok(true, 'Ask answers from this A3');
  await page.getByRole('tab', { name: 'Overview' }).click();
  await sleep(400);
  await shot('14-overview');

  console.log('New information: a later file');
  await invoke('attachments_add', { sheet: id, paths: [path.join(fixtures, 'Change note.txt')] });
  for (let i = 0; i < 40; i++) {
    const a = await invoke('attachments_list', { sheet: id });
    if (a.every((x) => x.aiState === 'done' || x.aiState === 'failed')) break;
    await sleep(500);
  }
  await goto(`/a3/${id}/review`);
  await page.getByRole('heading', { name: /Review A3-/ }).waitFor();
  const conflict = page.locator('.MuiCard-root').filter({ hasText: 'Conflict' });
  await conflict.first().waitFor();
  ok(await conflict.first().getByRole('button', { name: /Reopen A to apply/ }).isVisible(), 'a changed value against a closed step shows Conflict with Reopen to apply');
  ok(await conflict.first().getByText('Current').isVisible() && await conflict.first().getByText('New', { exact: true }).isVisible(), 'FR-IMP-06: current and new sit side by side');
  await shot('15-conflict');
  await conflict.first().getByRole('button', { name: /Reopen A to apply/ }).click();
  await page.getByRole('button', { name: 'Keep both' }).waitFor();
  await page.getByRole('button', { name: 'Keep both' }).click();
  await sleep(600);
  after = await invoke('sheet_detail', { id });
  ok(after.steps[0].plainText.includes('4.8%') && after.steps[0].plainText.includes('3.5%'), 'Keep both holds both values');

  console.log('Early warning');
  await goto(`/a3/${id}?tab=overview`);
  await page.getByRole('button', { name: 'Post warning' }).click();
  await page.getByRole('dialog').getByLabel('Headline').waitFor();
  await page.waitForFunction(() => { const i = [...document.querySelectorAll('[role=dialog] input')].find((x) => x.value); return !!i; });
  const causeChip = await page.getByRole('dialog').getByText('Cause not confirmed').isVisible();
  ok(causeChip, 'a warning posted while B is open shows Cause not confirmed');
  await shot('16-warning-draft');
  await page.getByRole('dialog').getByRole('button', { name: 'Preview' }).click();
  await page.getByRole('dialog').getByRole('button', { name: 'Post warning' }).click();
  await page.getByRole('heading', { name: 'Early warning', exact: true }).waitFor();
  await sleep(800);
  await shot('17-warning-card');
  const warningId = page.url().split('#/wall/')[1];
  ok(!!warningId, 'the card opens by its own link, with its source A3 alongside');
  await page.getByRole('button', { name: 'Create follow-up' }).click();
  await page.getByRole('dialog').getByLabel('Or describe the task').fill('Check die 4 on line 7');
  await page.getByRole('dialog').getByRole('button', { name: 'Add task' }).click();
  await page.getByRole('dialog').getByText('1 task added.').waitFor();
  await page.getByRole('dialog').getByRole('button', { name: 'Done' }).click();
  await page.getByText('Check die 4 on line 7').waitFor();
  await page.getByText('Affected', { exact: true }).first().click();
  await page.getByRole('button', { name: 'Create linked A3' }).last().click();
  await page.getByRole('tab', { name: 'Overview' }).waitFor();
  await sleep(600);
  const linkedId = page.url().split('#/a3/')[1].split('?')[0];
  const linked = await invoke('sheet_detail', { id: linkedId });
  ok(linked.sheet.originWarningId === warningId && linked.steps[0].plainText.length > 0, 'FR-TSK-04: Affected creates a linked A3, pre-filled with the warning');
  const w = await invoke('warning_get', { id: warningId });
  ok(w.affected === 1 && w.checksTotal === 1, 'FR-TSK-05: the card rolls up its check results');
  const draft = await invoke('warning_email_draft', { warning: warningId });
  ok(draft.subject.startsWith('Early warning: ') && draft.subject.includes(after.sheet.number) && draft.text.includes(`a3sheet://warning/${warningId}`) && draft.text.includes('a3sheet://a3/'), 'FR-EML-01: the email draft has the subject, body and both links');

  await goto('/wall');
  await page.getByRole('heading', { name: 'Early warning wall' }).waitFor();
  await sleep(400);
  await page.getByRole('button', { name: 'Mark read' }).first().click();
  await page.getByRole('button', { name: 'Mark unread' }).first().waitFor();
  ok(true, 'FR-EW-02: a reader marks a card read');
  await shot('18-wall');

  console.log('Export');
  const opts = (layout) => ({ layout, includeAttachments: true, includeSourceNotes: false });
  const results = {};
  for (const layout of ['quadrant', 'accordion']) {
    for (const format of ['pdf', 'docx', 'html']) {
      const name = await invoke('export_default_name', { sheet: id, layout, format });
      const dest = path.join(exports_, name);
      await invoke('export_save', { sheet: id, options: opts(layout), format, dest });
      results[`${layout}.${format}`] = dest;
      ok(fs.statSync(dest).size > 500, `${layout} ${format} written: ${name}`);
    }
  }
  fs.writeFileSync(path.join(work, 'export-paths.json'), JSON.stringify(results, null, 1));
  const quadPdf = fs.readFileSync(results['quadrant.pdf']).toString('latin1');
  const accPdf = fs.readFileSync(results['accordion.pdf']).toString('latin1');
  ok(/\/MediaBox\s*\[\s*0\s+0\s+119[01]/.test(quadPdf) || /\/MediaBox\s*\[\s*0\s+0\s+1190\.\d+/.test(quadPdf), 'quadrant PDF is A3 landscape (1191 x 842 pt)');
  ok(/\/Outlines/.test(accPdf), 'accordion PDF has bookmarks');
  ok(!fs.readFileSync(results['quadrant.html'], 'utf8').includes('99.9%'), 'no discarded AI text anywhere in an export');

  await goto(`/a3/${id}?tab=overview`);
  await page.getByRole('button', { name: 'Export' }).click();
  await page.locator('iframe[title="Export preview"]').waitFor();
  await sleep(1500);
  const frame = page.frames().find((f) => f.url().includes('asset'));
  ok(!!frame, 'the export preview loads through the asset protocol under the strict CSP');
  if (frame) ok(await frame.evaluate(() => !!window.__a3fit), 'the fit script ran in the preview');
  await shot('19-export-dialog');
  await page.getByRole('dialog').getByRole('button', { name: 'Cancel' }).click();

  console.log('Overflow');
  const long = await invoke('sheet_create', { new: { title: 'Long content', projectId: blank } });
  const ld = await invoke('sheet_detail', { id: long });
  const para = fs.readFileSync(path.join(fixtures, 'Long log.txt'), 'utf8').split('\n');
  const doc = { type: 'doc', content: para.map((t) => ({ type: 'paragraph', content: [{ type: 'text', text: t }] })) };
  await invoke('step_save', { step: ld.steps[0].id, expectedVersion: ld.steps[0].version, content: doc });
  const longPdf = path.join(exports_, 'overflow-quadrant.pdf');
  await invoke('export_save', { sheet: long, options: opts('quadrant'), format: 'pdf', dest: longPdf });
  fs.writeFileSync(path.join(work, 'overflow-path.txt'), longPdf);
  const lastLine = para[para.length - 1];
  const htmlPath = path.join(exports_, 'overflow-quadrant.html');
  await invoke('export_save', { sheet: long, options: opts('quadrant'), format: 'html', dest: htmlPath });
  ok(fs.readFileSync(htmlPath, 'utf8').includes(lastLine), 'the over-long A3 keeps every line in the export');

  console.log('Links');
  spawn(exe, [`a3sheet://a3/${after.sheet.number}`], { stdio: 'ignore', detached: true }).unref();
  await page.waitForFunction((n) => location.hash.includes(n), id, { timeout: 15000 });
  ok(true, 'a link in a second launch focuses the running app and opens the A3');
  spawn(exe, ['a3sheet://a3/../../evil;drop'], { stdio: 'ignore', detached: true }).unref();
  await sleep(1500);
  ok(page.url().includes(id), 'a malformed link is ignored');
  await page.getByPlaceholder('Search, or paste an A3 number or link').fill(linked.sheet.number);
  await page.getByPlaceholder('Search, or paste an A3 number or link').press('Enter');
  await page.waitForFunction((n) => location.hash.includes(n), linkedId);
  ok(true, 'the search box opens an A3 by number');

  console.log('List and dashboard');
  await goto('/a3');
  await page.getByRole('heading', { name: 'A3 Sheets' }).waitFor();
  await sleep(500);
  await shot('20-list');
  await page.getByRole('button', { name: 'My A3s' }).click().catch(() => page.getByText('My A3s').click());
  await sleep(300);
  await page.getByPlaceholder('Search, or paste an A3 number or link').fill('WI-204');
  await page.getByPlaceholder('Search, or paste an A3 number or link').press('Enter');
  await page.getByText(/1 A3$/).waitFor();
  ok(true, 'FR-LST-03: search reaches attachment text');
  await goto('/');
  await page.getByRole('tab', { name: 'My Work' }).waitFor();
  await sleep(500);
  await shot('21-my-work');
  await page.getByRole('tab', { name: 'Management' }).click();
  await page.getByText('Warning reach').waitFor();
  await sleep(500);
  await shot('22-management');
  await page.getByText('Open A3s by current step').locator('..').getByRole('button').nth(1).click();
  await page.getByRole('heading', { name: 'A3 Sheets' }).waitFor();
  ok(page.url().includes('step=B'), 'every Management number clicks through to the matching list');

  await goto(`/a3/${id}?tab=history`);
  await page.getByRole('tab', { name: 'Versions' }).waitFor();
  await sleep(300);
  await shot('23-history');
  await page.getByRole('tab', { name: 'Versions' }).click();
  await sleep(300);
  await shot('24-versions');

  console.log('Connection loss');
  await goto(`/a3/${id}?tab=sheet`);
  await page.getByLabel('Step D text').click();
  await page.keyboard.press('End');
  const pgBin = path.join(root, '.dev', 'pg-extract', 'pgsql', 'bin', 'pg_ctl.exe');
  const pgData = path.join(root, '.dev', 'pgdata');
  execFileSync(pgBin, ['-D', pgData, 'stop', '-m', 'fast'], { stdio: 'ignore' });
  await page.keyboard.type(' Typed while offline.');
  const dCard = page.locator('.MuiCard-root').filter({ has: page.getByRole('heading', { name: 'D. Validate and standardize' }) });
  await dCard.getByText('Kept on this computer').waitFor({ timeout: 30000 });
  ok(true, 'with the database gone, typed text is kept on this computer');
  const stepD = reviewed.steps[3].id;
  const recoveryFile = path.join(os.homedir(), 'AppData', 'Local', 'com.a3sheet.desktop', 'recovery', `${stepD}.json`);
  const inFile = () => fs.existsSync(recoveryFile) && fs.readFileSync(recoveryFile, 'utf8').includes('Typed while offline');
  for (let i = 0; i < 20 && !inFile(); i++) await sleep(250);
  ok(inFile(), `the text is in a local recovery file (exists: ${fs.existsSync(recoveryFile)})`);
  await page.getByText('The connection to the database was lost.').waitFor({ timeout: 30000 });
  ok(true, 'a banner says the connection was lost');
  await shot('25-offline');
  await new Promise((resolve) => spawn(pgBin, ['-D', pgData, '-l', path.join(root, '.dev', 'pg.log'), '-w', 'start'], { stdio: 'ignore' }).on('exit', resolve));
  await page.getByText('The connection to the database was lost.').waitFor({ state: 'detached', timeout: 40000 });
  let healed = await invoke('sheet_detail', { id });
  for (let i = 0; i < 40 && !healed.steps[3].plainText.includes('Typed while offline'); i++) {
    await sleep(500);
    healed = await invoke('sheet_detail', { id });
  }
  ok(healed.steps[3].plainText.includes('Typed while offline'), `when the connection returns, the text saves (${JSON.stringify(healed.steps[3].plainText).slice(0, 120)})`);
  for (let i = 0; i < 20 && fs.existsSync(recoveryFile); i++) await sleep(250);
  ok(!fs.existsSync(recoveryFile), 'and the recovery file goes');

  const unexpected0 = consoleErrors.filter((e) => !/favicon/.test(e));
  ok(unexpected0.length === 0, `no console errors or CSP violations (${unexpected0.length})${unexpected0.length ? ': ' + unexpected0.slice(0, 5).join(' || ').slice(0, 900) : ''}`);

  console.log('A link while the app is closed');
  const a3Number = after.sheet.number;
  await browser.close().catch(() => undefined);
  app.kill();
  await sleep(1500);
  app = spawn(exe, [`a3sheet://a3/${a3Number}`], { env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-port=9333' }, stdio: 'ignore' });
  await sleep(1500);
  const browser2 = await connect();
  const page2 = browser2.contexts()[0].pages()[0];
  page2.setDefaultTimeout(20000);
  await page2.getByLabel('Password').waitFor();
  await page2.getByLabel('Password').fill(process.env.A3_E2E_PASSWORD);
  await page2.getByRole('button', { name: 'Sign in' }).click();
  await page2.waitForFunction((n) => location.hash.includes(n), id);
  ok(true, 'FR-LNK-03: with the app closed, a link launches it, signs in first, then opens the A3');
  await browser2.close().catch(() => undefined);
}

let exitCode = 0;
try {
  await main();
} catch (e) {
  exitCode = 2;
  console.log(`\nSTOPPED: ${e.message?.split('\n').slice(0, 6).join('\n')}`);
  try {
    const b = await chromium.connectOverCDP('http://127.0.0.1:9333');
    await b.contexts()[0].pages()[0].screenshot({ path: path.join(shots, 'zz-stopped.png') });
    await b.close();
  } catch {
    // The screenshot is a convenience.
  }
}
console.log(`\n${passed} passed, ${failures.length} failed`);
for (const f of failures) console.log(`  FAILED: ${f}`);
if (failures.length) exitCode = exitCode || 1;
if (consoleErrors.length) console.log('Console:', consoleErrors.slice(0, 8).join('\n').slice(0, 1500));
app.kill();
mock.kill();
process.exit(exitCode);
