import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Alert, Box, Button, Card, Chip, CircularProgress, Dialog, DialogActions, DialogContent, DialogTitle, IconButton, MenuItem, Stack, TextField, ToggleButton, ToggleButtonGroup, Tooltip, Typography } from '@mui/material';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { call, errorMessage, LETTERS, STEP_NAMES, SUMMARY_TONES, type A3Summary, type AiSettingsView, type Letter, type Sheet, type Step, type Suggestion, type SummaryTone } from '../../api';
import { useApp } from '../../context';
import { measureFit, type FitResult } from '../../lib/fit';
import { formatDate, wordDiff } from '../../lib/text';
import { plainText, sameDocument, storyParts, type DocNode } from '../../lib/doc';
import { tokens } from '../../theme';
import { Icon } from '../common';
import { StepEditor, type StepEditorHandle } from '../editor/StepEditor';
import { AssistantPanel } from './AssistantPanel';

const REOPEN_REASONS = ['New evidence', 'Result not sustained', 'Correction', 'Other'] as const;

function storedView(): 'quadrant' | 'accordion' {
  try {
    return localStorage.getItem('a3.sheetView') === 'accordion' ? 'accordion' : 'quadrant';
  } catch {
    return 'quadrant';
  }
}

interface Props {
  sheet: Sheet;
  steps: Step[];
  openConflicts: number;
  onOpenSource: (segmentId: string, quote?: string | null) => void;
}

export function SheetTab({ sheet, steps, openConflicts, onOpenSource }: Props) {
  const [view, setView] = useState(storedView);
  const [expanded, setExpanded] = useState<Letter | null>(sheet.currentStep ?? 'A');
  const [panelOpen, setPanelOpen] = useState(true);
  const editors = useRef<Partial<Record<Letter, StepEditorHandle | null>>>({});
  const boxes = useRef<Partial<Record<Letter, HTMLDivElement | null>>>({});
  const [closing, setClosing] = useState<Step | null>(null);
  const [reopening, setReopening] = useState<Step | null>(null);
  const [fit, setFit] = useState<Partial<Record<Letter, FitResult>>>({});
  const [suggestions, setSuggestions] = useState<Partial<Record<Letter, Suggestion[]>>>({});
  const [improving, setImproving] = useState<Letter | null>(null);
  const [summarizing, setSummarizing] = useState(false);
  const [summary, setSummary] = useState<A3Summary | null>(null);
  const [applyingSummary, setApplyingSummary] = useState(false);
  const [tone, setTone] = useState<SummaryTone>('plain');
  const [summaryError, setSummaryError] = useState<string | null>(null);
  const [summarySeconds, setSummarySeconds] = useState(0);
  const [summaryView, setSummaryView] = useState<'readable' | 'diff'>('readable');
  const { notify } = useApp();
  const qc = useQueryClient();
  const ai = useQuery({ queryKey: ['ai-settings'], queryFn: () => call<AiSettingsView>('ai_settings_get') });
  const aiReady = !!ai.data?.ready && sheet.projectAiEnabled;
  const summaryDiffs = useMemo(() => new Map(
    summary?.steps.filter((s) => !sameDocument(s.original, s.content))
      .map((s) => [s.letter, wordDiff(plainText(s.original), plainText(s.content))]) ?? [],
  ), [summary]);

  useEffect(() => {
    if (!summarizing) return;
    const started = Date.now();
    const timer = window.setInterval(() => setSummarySeconds(Math.floor((Date.now() - started) / 1000)), 1000);
    return () => window.clearInterval(timer);
  }, [summarizing]);

  useEffect(() => {
    try {
      localStorage.setItem('a3.sheetView', view);
    } catch {
      // A blocked store only means the view is not remembered.
    }
  }, [view]);

  const refresh = useCallback(() => {
    void qc.invalidateQueries({ queryKey: ['sheet', sheet.id] });
    void qc.invalidateQueries({ queryKey: ['sheets'] });
    void qc.invalidateQueries({ queryKey: ['findings', sheet.id] });
  }, [qc, sheet.id]);

  const doClose = async (step: Step) => {
    setClosing(null);
    try {
      // Pending typing reaches the database before the close checks the text.
      await editors.current[step.letter]?.flush();
      await call('step_close', { step: step.id });
      refresh();
      notify({ severity: 'success', message: `Step ${step.letter} closed.` });
      // Validity review runs on step close, in the background. It never blocks the close (FR-ST-05).
      if (aiReady) {
        call('ai_review', { sheet: sheet.id }).then(() => qc.invalidateQueries({ queryKey: ['findings', sheet.id] })).catch(() => undefined);
      }
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
    }
  };

  const onCloseClick = (step: Step) => {
    if (openConflicts > 0) setClosing(step);
    else void doClose(step);
  };

  const doReopen = async (step: Step, reason: string) => {
    setReopening(null);
    try {
      await call('step_reopen', { step: step.id, reason });
      refresh();
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
    }
  };

  const improve = async (letter: Letter, selection: string | null) => {
    if (!aiReady) {
      notify({ severity: 'info', message: sheet.projectAiEnabled ? 'Set up AI in Settings' : 'AI is turned off for this project.' });
      return;
    }
    setImproving(letter);
    setSuggestions((s) => ({ ...s, [letter]: undefined }));
    try {
      await editors.current[letter]?.flush();
      const got = await call<Suggestion[]>('ai_improve', { sheet: sheet.id, letter, selection });
      setSuggestions((s) => ({ ...s, [letter]: got }));
      if (got.length === 0) notify({ severity: 'info', message: `Step ${letter} already reads clearly.` });
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
    } finally {
      setImproving(null);
    }
  };

  const summarize = async () => {
    if (summarizing || applyingSummary) return;
    if (!aiReady) {
      notify({ severity: 'info', message: sheet.projectAiEnabled ? 'Set up AI in Settings' : 'AI is turned off for this project.' });
      return;
    }
    setSummarizing(true);
    setSummarySeconds(0);
    setSummaryError(null);
    try {
      await Promise.all(LETTERS.map((letter) => editors.current[letter]?.flush()));
      const draft = await call<A3Summary>('ai_summarize', { sheet: sheet.id, tone });
      if (draft.steps.every((s) => sameDocument(s.original, s.content))) {
        notify({ severity: 'info', message: 'No summary or section moves were proposed for this A3.' });
      }
      setSummary(draft);
    } catch (e) {
      const message = errorMessage(e);
      setSummaryError(message);
      notify({ severity: 'error', message });
    } finally {
      setSummarizing(false);
    }
  };

  const acceptSummary = async () => {
    if (!summary || summarizing || tone !== summary.tone) return;
    setApplyingSummary(true);
    try {
      await Promise.all(LETTERS.map((letter) => editors.current[letter]?.flush()));
      if (!summary.steps.every((s) => editors.current[s.letter]?.matchesSummary(s.original, s.version))) {
        notify({ severity: 'warning', message: 'This A3 has changed. Dismiss this draft and generate a new summary to avoid overwriting edits.' });
        return;
      }
      const saved = await call<Step[]>('ai_apply_summary', { sheet: sheet.id, draft: summary });
      saved.forEach((s) => editors.current[s.letter]?.loadSavedStep(s));
      setSuggestions({});
      setSummary(null);
      refresh();
      notify({ severity: 'success', message: 'Summary and section moves applied.' });
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
    } finally {
      setApplyingSummary(false);
    }
  };

  const accept = (letter: Letter, s: Suggestion) => {
    const ok = editors.current[letter]?.applySuggestion(s.original, s.rewrite);
    if (!ok) notify({ severity: 'warning', message: 'That sentence has changed since the suggestion was made.' });
    setSuggestions((cur) => ({ ...cur, [letter]: cur[letter]?.filter((x) => x !== s) }));
  };

  const dismissSuggestion = (letter: Letter, s: Suggestion) => {
    setSuggestions((cur) => ({ ...cur, [letter]: cur[letter]?.filter((x) => x !== s) }));
  };

  const onHtml = useCallback((letter: Letter, html: string) => {
    setFit((f) => ({ ...f, [letter]: measureFit(html) }));
  }, []);

  const showStep = (letter: Letter) => {
    setExpanded(letter);
    boxes.current[letter]?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
  };

  const stepBox = (letter: Letter, accordion: boolean) => {
    const step = steps.find((s) => s.letter === letter)!;
    const closed = step.state === 'closed';
    const earlierOpen = steps.some((s) => s.letter < letter && s.state === 'open');
    const open = !accordion || expanded === letter;
    const canClose = sheet.isOwner && !sheet.withdrawnAt;
    const list = suggestions[letter] ?? [];

    return (
      <Card
        key={letter}
        ref={(el: HTMLDivElement | null) => {
          boxes.current[letter] = el;
        }}
        sx={{ display: 'flex', flexDirection: 'column', minHeight: 0, gridArea: accordion ? undefined : letter, overflow: 'hidden' }}
      >
        <Stack
          direction="row"
          spacing={1}
          onClick={accordion ? () => setExpanded(open ? null : letter) : undefined}
          sx={{ px: 1.5, py: 0.75, alignItems: 'center', bgcolor: tokens.fill, borderBottom: open ? `1px solid ${tokens.line}` : 'none', cursor: accordion ? 'pointer' : 'default', minHeight: 44 }}
        >
          {accordion && <Icon name={open ? 'expand_less' : 'expand_more'} color={tokens.textSecondary} />}
          <Typography variant="h4" sx={{ flex: 1, minWidth: 0 }} noWrap component="h2">{letter}. {STEP_NAMES[letter]}</Typography>
          {!accordion && fit[letter] && <FitMeter fit={fit[letter]!} />}
          <Chip
            size="small"
            label={closed ? `Closed ${formatDate(step.closedAt)}` : 'Open'}
            sx={{ height: 22, bgcolor: closed ? tokens.successLight : '#fff', color: closed ? tokens.successText : tokens.textSecondary, border: closed ? 'none' : `1px solid ${tokens.line}` }}
          />
          {canClose && !closed && (
            <Tooltip title={earlierOpen ? 'Steps close in order A, B, C, D' : ''}>
              <span>
                <Button size="small" variant="outlined" disabled={earlierOpen} onClick={(e) => { e.stopPropagation(); onCloseClick(step); }} sx={{ minHeight: 26 }}>Close step</Button>
              </span>
            </Tooltip>
          )}
          {canClose && closed && (
            <Button size="small" onClick={(e) => { e.stopPropagation(); setReopening(step); }} sx={{ minHeight: 26 }}>Reopen</Button>
          )}
        </Stack>
        <Box sx={{ display: open ? 'flex' : 'none', flexDirection: 'column', flex: 1, minHeight: 0, height: accordion ? 320 : undefined }}>
          {improving === letter && (
            <Stack direction="row" spacing={1} sx={{ px: 1.5, py: 0.75, alignItems: 'center' }} role="status">
              <CircularProgress size={14} />
              <Typography variant="caption" color="text.secondary">Looking for clearer wording...</Typography>
            </Stack>
          )}
          {list.map((s) => (
            <SuggestionRow key={s.sentenceId} suggestion={s} onAccept={() => accept(letter, s)} onDismiss={() => dismissSuggestion(letter, s)} />
          ))}
          <StepEditor
            ref={(h) => {
              editors.current[letter] = h;
            }}
            step={step}
            canEdit={sheet.canEdit && !sheet.withdrawnAt}
            onOpenSource={(id) => onOpenSource(id)}
            onImprove={(sel) => void improve(letter, sel)}
            aiBusy={improving !== null || summarizing || applyingSummary}
            applyingSummary={applyingSummary}
            onHtmlChange={(html) => onHtml(letter, html)}
            fill
          />
        </Box>
      </Card>
    );
  };

  return (
    <Box sx={{ display: 'flex', alignItems: 'stretch', minHeight: 'calc(100vh - 190px)' }}>
      <Box sx={{ flex: 1, minWidth: 0, p: 2 }}>
        <Stack direction="row" sx={{ mb: 1.5, alignItems: 'center', justifyContent: 'space-between' }}>
          <ToggleButtonGroup size="small" exclusive value={view} onChange={(_, v) => v && setView(v)} aria-label="Sheet layout">
            <ToggleButton value="quadrant" sx={{ textTransform: 'none', px: 1.5, py: 0.25 }}><Icon name="grid_view" size={18} />&nbsp;Quadrant</ToggleButton>
            <ToggleButton value="accordion" sx={{ textTransform: 'none', px: 1.5, py: 0.25 }}><Icon name="view_agenda" size={18} />&nbsp;Accordion</ToggleButton>
          </ToggleButtonGroup>
          {sheet.canEdit && !sheet.withdrawnAt && (
            <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }}>
            <SummaryTonePicker id="summary-tone" tone={tone} onChange={setTone} disabled={summarizing || applyingSummary} />
            <Button
              size="small"
              variant="outlined"
              startIcon={summarizing ? <CircularProgress size={14} /> : <Icon name="auto_awesome" size={16} />}
              disabled={summarizing || improving !== null || applyingSummary}
              onClick={() => void summarize()}
            >
              {summarizing ? `Reviewing full A3... ${summarySeconds}s` : 'Review & summarize A3'}
            </Button>
            </Stack>
          )}
          {!sheet.canEdit && <Typography variant="body2" color="text.secondary">You can read this A3. Only project members can edit it.</Typography>}
        </Stack>
        {view === 'quadrant' ? (
          <Box sx={{ display: 'grid', gridTemplateColumns: 'minmax(0, 1fr) minmax(0, 1fr)', gridTemplateRows: 'repeat(2, minmax(300px, calc(50vh - 130px)))', gridTemplateAreas: '"A C" "B D"', gap: 1.5 }}>
            {LETTERS.map((l) => stepBox(l, false))}
          </Box>
        ) : (
          <Stack spacing={1.5}>{LETTERS.map((l) => stepBox(l, true))}</Stack>
        )}
      </Box>

      {panelOpen ? (
        <AssistantPanel sheet={sheet} aiReady={aiReady} onCollapse={() => setPanelOpen(false)} onOpenSource={onOpenSource} onShowStep={showStep} />
      ) : (
        <Stack sx={{ width: 44, flex: 'none', borderLeft: `1px solid ${tokens.line}`, bgcolor: '#fff', alignItems: 'center', pt: 1 }}>
          <Tooltip title="Open Assistant" placement="left">
            <IconButton aria-label="Open Assistant" onClick={() => setPanelOpen(true)}><Icon name="auto_awesome" color={tokens.primary} /></IconButton>
          </Tooltip>
        </Stack>
      )}

      <Dialog open={!!summary} onClose={() => !applyingSummary && !summarizing && setSummary(null)} maxWidth="lg" fullWidth aria-labelledby="summary-title">
        <DialogTitle id="summary-title">Review proposed A3 changes</DialogTitle>
        <DialogContent>
          <Stack direction="row" spacing={2} sx={{ mb: 2, mt: 1, alignItems: 'center', flexWrap: 'wrap', gap: 1 }}>
            <SummaryTonePicker id="preview-summary-tone" tone={tone} onChange={setTone} disabled={summarizing || applyingSummary} />
            <Button variant="outlined" disabled={summarizing || applyingSummary} onClick={() => void summarize()}>
              {summarizing ? `Regenerating... ${summarySeconds}s` : 'Regenerate with this tone'}
            </Button>
            <Typography variant="caption" color="text.secondary">
              Preview tone: {SUMMARY_TONES.find((t) => t.value === summary?.tone)?.label}
            </Typography>
          </Stack>
          <ToggleButtonGroup size="small" exclusive value={summaryView} onChange={(_, value) => value && setSummaryView(value)} aria-label="Proposal preview mode" sx={{ mb: 2 }}>
            <ToggleButton value="readable">Readable draft</ToggleButton>
            <ToggleButton value="diff">Highlighted changes</ToggleButton>
          </ToggleButtonGroup>
          {summarizing && <Alert severity="info" sx={{ mb: 2 }}>Generating and fact-checking a new draft. The previous preview remains below; applying is disabled until this finishes.</Alert>}
          {summaryError && <Alert severity="error" sx={{ mb: 2 }}>{summaryError} The previous preview has not been replaced.</Alert>}
          {summary && summary.tone !== tone && !summarizing && (
            <Alert severity="warning" sx={{ mb: 2 }}>The selected tone differs from this preview. Regenerate to review it before applying.</Alert>
          )}
          <Alert severity="info" sx={{ mb: 2 }}>Read the story first; expand the supporting details when needed. Use Highlighted changes to inspect the exact changes. Nothing changes until you apply all changes. Closed sections remain unchanged.</Alert>
          {summary?.note && <Alert severity="warning" sx={{ mb: 2, whiteSpace: 'pre-line' }}>Review notes: {summary.note}</Alert>}
          {summary?.moves.map((move, i) => (
            <Typography key={i} variant="body2" sx={{ mb: 1 }}>Move {move.from} to {move.to}: {move.text}</Typography>
          ))}
          {summary?.steps.map((s) => (
            <Box key={s.letter} sx={{ mb: 3 }}>
              <Typography variant="h4" sx={{ mb: 1 }}>{s.letter}. {STEP_NAMES[s.letter]}</Typography>
              {sameDocument(s.original, s.content) ? <Typography variant="body2" color="text.secondary">No changes.</Typography> : (
                <Box sx={{ display: 'grid', gridTemplateColumns: { xs: '1fr', sm: '1fr 1fr' }, gap: 2 }}>
                  <Box>
                    <Typography variant="subtitle2">Current</Typography>
                    {summaryView === 'readable' ? <StoryPreview doc={s.original} onOpenSource={onOpenSource} /> : (
                      <Typography variant="body2" sx={{ whiteSpace: 'pre-wrap' }}>{plainText(s.original) || '(empty)'}</Typography>
                    )}
                  </Box>
                  <Box>
                    <Typography variant="subtitle2">{summaryView === 'readable' ? 'Proposed' : 'Proposed (changes highlighted)'}</Typography>
                    {summaryView === 'readable' ? <StoryPreview doc={s.content} onOpenSource={onOpenSource} /> : (
                      <Typography variant="body2" sx={{ whiteSpace: 'pre-wrap' }}>
                        {summaryDiffs.get(s.letter)?.map((p, i) => (
                          <span key={i} className={p.kind === 'del' ? 'diff-del' : p.kind === 'add' ? 'diff-add' : undefined}>{p.text}</span>
                        ))}
                      </Typography>
                    )}
                  </Box>
                </Box>
              )}
            </Box>
          ))}
        </DialogContent>
        <DialogActions>
          <Button disabled={applyingSummary || summarizing} onClick={() => setSummary(null)}>Dismiss</Button>
          <Button disabled={applyingSummary || summarizing || tone !== summary?.tone} variant="contained" onClick={() => void acceptSummary()}>{applyingSummary ? 'Applying...' : 'Apply all changes'}</Button>
        </DialogActions>
      </Dialog>

      <Dialog open={!!closing} onClose={() => setClosing(null)} maxWidth="xs" fullWidth>
        <DialogTitle>Close step {closing?.letter}?</DialogTitle>
        <DialogContent>
          <Alert severity="warning">{openConflicts} open {openConflicts === 1 ? 'conflict' : 'conflicts'} from new files are waiting for review.</Alert>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setClosing(null)}>Cancel</Button>
          <Button variant="contained" onClick={() => closing && void doClose(closing)}>Close step</Button>
        </DialogActions>
      </Dialog>

      <Dialog open={!!reopening} onClose={() => setReopening(null)} maxWidth="xs" fullWidth>
        <DialogTitle>Why reopen step {reopening?.letter}?</DialogTitle>
        <DialogContent>
          <Stack direction="row" spacing={1} useFlexGap sx={{ flexWrap: 'wrap' }}>
            {REOPEN_REASONS.map((r) => (
              <Chip key={r} label={r} clickable variant="outlined" onClick={() => reopening && void doReopen(reopening, r)} />
            ))}
          </Stack>
        </DialogContent>
        <DialogActions><Button onClick={() => setReopening(null)}>Cancel</Button></DialogActions>
      </Dialog>
    </Box>
  );
}

function StoryPreview({ doc, onOpenSource }: { doc: DocNode; onOpenSource: (segmentId: string) => void }) {
  const { story, details } = storyParts(doc);
  if (story.length === 0) return <Typography variant="body2" sx={{ whiteSpace: 'pre-wrap' }}>{plainText(doc) || '(empty)'}</Typography>;
  return (
    <Box>
      <Typography variant="overline" color="primary">At a glance</Typography>
      {story.map((paragraph, i) => {
        const refs = new Map<string, string>();
        const collect = (node: DocNode) => {
          for (const mark of node.marks ?? []) {
            if (mark.type === 'source' && typeof mark.attrs?.segmentId === 'string') {
              refs.set(mark.attrs.segmentId, typeof mark.attrs.label === 'string' ? mark.attrs.label : 'Source');
            }
          }
          node.content?.forEach(collect);
        };
        collect(paragraph);
        return (
          <Box key={i} sx={{ mb: 1.5 }}>
            <Typography variant="body2" sx={{ lineHeight: 1.7 }}>{plainText(paragraph)}</Typography>
            <Stack direction="row" spacing={0.5} useFlexGap sx={{ flexWrap: 'wrap' }}>
              {[...refs].map(([id, label]) => <Button key={id} size="small" onClick={() => onOpenSource(id)} sx={{ fontSize: 11, py: 0 }}>{label}</Button>)}
            </Stack>
          </Box>
        );
      })}
      <Box component="details" sx={{ mt: 1 }}>
        <Box component="summary" sx={{ cursor: 'pointer', color: tokens.textSecondary, typography: 'caption' }}>Supporting evidence and details</Box>
        <Typography variant="body2" sx={{ whiteSpace: 'pre-wrap', mt: 1 }}>{plainText({ type: 'doc', content: details })}</Typography>
      </Box>
    </Box>
  );
}

function SummaryTonePicker({ id, tone, onChange, disabled }: { id: string; tone: SummaryTone; onChange: (tone: SummaryTone) => void; disabled: boolean }) {
  return (
    <TextField
      select
      id={id}
      label="Writing tone"
      size="small"
      value={tone}
      disabled={disabled}
      onChange={(event) => {
        const selected = SUMMARY_TONES.find((t) => t.value === event.target.value);
        if (selected) onChange(selected.value);
      }}
      sx={{ minWidth: 180 }}
    >
      {SUMMARY_TONES.map((t) => <MenuItem key={t.value} value={t.value}>{t.label}</MenuItem>)}
    </TextField>
  );
}

function FitMeter({ fit }: { fit: FitResult }) {
  const pct = Math.round(fit.fillAtFull * 100);
  const state = fit.continued ? 'over' : fit.fontPt < 11 ? 'shrink' : 'ok';
  const title =
    state === 'over'
      ? 'Too long for the printed box, even at 8 pt. It will carry on to a continued page.'
      : state === 'shrink'
        ? `Fits the printed box at ${fit.fontPt} pt.`
        : 'Fits the printed box.';
  const color = state === 'over' ? tokens.error : state === 'shrink' ? tokens.warning : tokens.success;
  return (
    <Tooltip title={title}>
      <Box role="img" aria-label={`Print fit: ${title}`} sx={{ width: 48, height: 6, borderRadius: 3, bgcolor: tokens.line, overflow: 'hidden', flex: 'none' }}>
        <Box sx={{ width: `${Math.min(100, Math.max(pct, 4))}%`, height: '100%', bgcolor: color }} />
      </Box>
    </Tooltip>
  );
}

function SuggestionRow({ suggestion, onAccept, onDismiss }: { suggestion: Suggestion; onAccept: () => void; onDismiss: () => void }) {
  const [why, setWhy] = useState(false);
  return (
    <Box sx={{ px: 1.5, py: 1, bgcolor: tokens.primaryLight, borderBottom: `1px solid ${tokens.line}` }}>
      <Typography variant="body2">
        {wordDiff(suggestion.original, suggestion.rewrite).map((p, i) =>
          p.kind === 'same' ? <span key={i}>{p.text}</span> : <span key={i} className={p.kind === 'del' ? 'diff-del' : 'diff-add'}>{p.text}</span>,
        )}
      </Typography>
      {why && <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mt: 0.5 }}>{suggestion.why}</Typography>}
      <Stack direction="row" spacing={0.5} sx={{ mt: 0.5 }}>
        <Button size="small" variant="contained" onClick={onAccept} sx={{ minHeight: 24 }}>Accept</Button>
        <Button size="small" onClick={onDismiss} sx={{ minHeight: 24 }}>Dismiss</Button>
        {!why && suggestion.why && <Button size="small" onClick={() => setWhy(true)} sx={{ minHeight: 24 }}>Why?</Button>}
      </Stack>
    </Box>
  );
}
