import { useCallback, useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react';
import { Alert, Box, Button, Card, CardContent, Chip, CircularProgress, IconButton, MenuItem, Stack, TextField, Tooltip, Typography } from '@mui/material';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useNavigate, useParams } from 'react-router-dom';
import { call, errorMessage, LETTERS, STEP_NAMES, type AiSettingsView, type Attachment, type Finding, type Letter, type Proposal, type ProposalDecision, type ReviewOutcome, type Segment, type SheetDetail, type Step } from '../api';
import { Icon, SourceChip } from '../components/common';
import { SourcePane } from '../components/SourcePane';
import { useApp } from '../context';
import { summariseProposals } from '../lib/text';
import { tokens } from '../theme';

const FIELD_LABELS: Record<string, string> = { title: 'Title', problem_date: 'Problem occurred on', product: 'Product or part', area: 'Area or line', category: 'Category' };

/**
 * One review screen for first and later imports (section 8). Nothing reaches a step until accepted
 * (FR-IMP-03). Without an AI key it turns into the manual path of FR-IMP-10.
 */
export function ReviewPage() {
  const { id = '' } = useParams();
  const nav = useNavigate();
  const qc = useQueryClient();
  const { jobs, notify } = useApp();
  const detail = useQuery({ queryKey: ['sheet', id], queryFn: () => call<SheetDetail>('sheet_detail', { id }) });
  const proposals = useQuery({ queryKey: ['proposals', id], queryFn: () => call<Proposal[]>('proposals_list', { sheet: id, pendingOnly: true }) });
  const attachments = useQuery({ queryKey: ['attachments', id], queryFn: () => call<Attachment[]>('attachments_list', { sheet: id }) });
  const ai = useQuery({ queryKey: ['ai-settings'], queryFn: () => call<AiSettingsView>('ai_settings_get') });

  const [focus, setFocus] = useState<{ segmentId: string; quote?: string | null } | null>(null);
  const [findings, setFindings] = useState<Finding[] | null>(null);
  const [reviewing, setReviewing] = useState(false);
  const hadProposals = useRef(false);

  const list = useMemo(() => proposals.data ?? [], [proposals.data]);
  const meta = list.filter((p) => p.kind === 'metadata');
  const steps = list.filter((p) => p.kind !== 'metadata');
  const reading = Object.values(jobs).some((j) => j.sheetId === id) || (attachments.data ?? []).some((a) => !a.deletedAt && (['pending', 'reading'].includes(a.readState) || ['pending', 'running'].includes(a.aiState)));
  const canEdit = !!detail.data?.sheet.canEdit;
  const aiReady = !!ai.data?.ready && !!detail.data?.sheet.projectAiEnabled;

  const refresh = useCallback(async () => {
    await qc.invalidateQueries({ queryKey: ['proposals', id] });
    await qc.invalidateQueries({ queryKey: ['sheet', id] });
    await qc.invalidateQueries({ queryKey: ['attachments', id] });
  }, [qc, id]);

  // Reading finishes in the background, so keep the screen current while it runs.
  useEffect(() => {
    if (!reading) return;
    const t = window.setInterval(() => void refresh(), 3000);
    return () => window.clearInterval(t);
  }, [reading, refresh]);

  useEffect(() => {
    if (!focus && list[0]?.sourceSegmentId) setFocus({ segmentId: list[0].sourceSegmentId, quote: list[0].quote });
  }, [list, focus]);

  useEffect(() => {
    if (list.length > 0) hadProposals.current = true;
  }, [list.length]);

  // Section 3 step 4: AI reviews the result once and shows at most three findings.
  useEffect(() => {
    if (!hadProposals.current || list.length > 0 || reading || !aiReady || !proposals.isSuccess || findings || reviewing) return;
    setReviewing(true);
    call<ReviewOutcome>('ai_review', { sheet: id })
      .then((r) => setFindings(r.findings))
      .catch(() => setFindings([]))
      .finally(() => setReviewing(false));
  }, [list.length, reading, aiReady, proposals.isSuccess, findings, reviewing, id]);

  const decide = async (p: Proposal, decision: ProposalDecision) => {
    try {
      await call('proposal_decide', { id: p.id, decision });
      await refresh();
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
      await refresh();
    }
  };

  const acceptAll = async (letter: Letter) => {
    const adds = steps.filter((p) => p.letter === letter && p.kind !== 'conflict' && !p.stepClosed);
    for (const p of adds) {
      try {
        await call('proposal_decide', { id: p.id, decision: { action: 'accept' } });
      } catch (e) {
        notify({ severity: 'error', message: errorMessage(e) });
        break;
      }
    }
    await refresh();
  };

  const reopen = async (letter: Letter) => {
    const step = detail.data?.steps.find((s) => s.letter === letter);
    if (!step) return;
    try {
      await call('step_reopen', { step: step.id, reason: 'New evidence' });
      await refresh();
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
    }
  };

  if (!detail.data) return <Box sx={{ display: 'grid', placeItems: 'center', height: 240 }}><CircularProgress size={28} /></Box>;
  const sheet = detail.data.sheet;
  const noAiPath = !aiReady && list.length === 0;
  const done = hadProposals.current && list.length === 0 && !reading;

  return (
    <Box sx={{ height: '100%', display: 'grid', gridTemplateColumns: 'minmax(0, 1fr) minmax(360px, 42%)', gridTemplateRows: 'auto minmax(0, 1fr)' }}>
      <Stack direction="row" spacing={2} sx={{ gridColumn: '1 / 3', px: 3, py: 1.5, alignItems: 'center', bgcolor: '#fff', borderBottom: `1px solid ${tokens.line}` }}>
        <IconButton aria-label="Back to A3" onClick={() => nav(`/a3/${id}?tab=sheet`)}><Icon name="arrow_back" /></IconButton>
        <Box sx={{ flex: 1, minWidth: 0 }}>
          <Typography variant="h2" noWrap>Review {sheet.number}</Typography>
          <Typography variant="body2" color="text.secondary" role="status">
            {reading ? 'Reading your files. You can keep working.' : list.length ? `${list.length} to review. ${summariseProposals(steps.map((s) => s.letter))}` : noAiPath ? 'Place passages from your files into a step.' : 'All reviewed.'}
          </Typography>
        </Box>
        {reading && <CircularProgress size={20} />}
        <Button variant="outlined" onClick={() => nav(`/a3/${id}?tab=sheet`)}>Open A3</Button>
      </Stack>

      <Box sx={{ overflow: 'auto', p: 2, bgcolor: tokens.fill }} aria-label="Proposals">
        {!canEdit && <Alert severity="info" sx={{ mb: 2 }}>You can read these proposals. Only project members can decide them.</Alert>}
        {(attachments.data ?? []).filter((a) => a.aiState === 'failed' && !a.deletedAt).map((a) => (
          <Alert
            key={a.id}
            severity="error"
            sx={{ mb: 2 }}
            action={<Button size="small" onClick={() => void call('attachment_retry', { id: a.id }).then(refresh)}>Retry</Button>}
          >
            {a.fileName}: {a.aiError}
          </Alert>
        ))}
        {(attachments.data ?? []).filter((a) => a.aiNote && !a.deletedAt).map((a) => (
          <Typography key={a.id} variant="caption" color="text.secondary" sx={{ display: 'block', mb: 1 }}>{a.fileName}: {a.aiNote}.</Typography>
        ))}

        {meta.length > 0 && <MetadataBlock proposals={meta} canEdit={canEdit} onDecide={decide} onFocus={setFocus} />}

        {LETTERS.map((l) => {
          const cards = steps.filter((p) => p.letter === l);
          if (!cards.length) return null;
          const anyPlain = cards.some((p) => p.kind !== 'conflict' && !p.stepClosed);
          return (
            <Box key={l} sx={{ mb: 2 }} component="section" aria-label={`Step ${l}`}>
              <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between', mb: 1 }}>
                <Typography variant="h4" component="h2">{l}. {STEP_NAMES[l]} ({cards.length})</Typography>
                {canEdit && anyPlain && <Button size="small" onClick={() => void acceptAll(l)}>Accept all</Button>}
              </Stack>
              <Stack spacing={1}>
                {cards.map((p) => (
                  <ProposalCard key={p.id} proposal={p} canEdit={canEdit} isOwner={sheet.isOwner} onDecide={decide} onFocus={setFocus} onReopen={() => void reopen(l)} />
                ))}
              </Stack>
            </Box>
          );
        })}

        {done && (
          <Card>
            <CardContent>
              <Typography variant="h3" sx={{ mb: 1 }}>All reviewed</Typography>
              {reviewing && <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }} role="status"><CircularProgress size={14} /><Typography variant="body2" color="text.secondary">Checking the result...</Typography></Stack>}
              {findings && findings.length === 0 && <Typography variant="body2" color="text.secondary">Nothing to report.</Typography>}
              {findings && findings.slice(0, 3).map((f) => <Typography key={f.id} variant="body2" sx={{ mb: 0.5 }}>• {f.text}</Typography>)}
              {findings && findings.length > 3 && <Typography variant="caption" color="text.secondary">{findings.length - 3} more in the Assistant panel.</Typography>}
              <Stack direction="row" spacing={1} sx={{ mt: 2 }}>
                <Button variant="contained" onClick={() => nav(`/a3/${id}?tab=sheet`)}>Open A3</Button>
              </Stack>
            </CardContent>
          </Card>
        )}

        {noAiPath && !reading && !done && (
          <ManualPlacement sheetId={id} steps={detail.data.steps} attachments={attachments.data ?? []} canEdit={canEdit} aiConfigured={!!ai.data?.ready} projectAi={sheet.projectAiEnabled} onPlaced={refresh} onFocus={setFocus} />
        )}
        {reading && list.length === 0 && <Stack direction="row" spacing={1} sx={{ alignItems: 'center', p: 2 }} role="status"><CircularProgress size={18} /><Typography variant="body2" color="text.secondary">Reading your files...</Typography></Stack>}
      </Box>

      <Box sx={{ minHeight: 0, bgcolor: '#fff', borderLeft: `1px solid ${tokens.line}` }} aria-label="Source">
        {focus ? (
          <SourcePane segmentId={focus.segmentId} quote={focus.quote} />
        ) : (
          <Stack sx={{ height: '100%', alignItems: 'center', justifyContent: 'center', color: tokens.textSecondary, p: 3, textAlign: 'center' }}>
            <Icon name="description" size={32} />
            <Typography variant="body2" sx={{ mt: 1 }}>The source appears here, with the passage highlighted.</Typography>
          </Stack>
        )}
      </Box>
    </Box>
  );
}

/** FR-IMP-05: metadata proposals as a compact block at the top. */
function MetadataBlock({ proposals, canEdit, onDecide, onFocus }: { proposals: Proposal[]; canEdit: boolean; onDecide: (p: Proposal, d: ProposalDecision) => Promise<void>; onFocus: (f: { segmentId: string; quote?: string | null }) => void }) {
  return (
    <Card sx={{ mb: 2 }}>
      <CardContent sx={{ pb: '12px !important' }}>
        <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between', mb: 1 }}>
          <Typography variant="h4" component="h2">Details</Typography>
          {canEdit && proposals.length > 1 && (
            <Button size="small" onClick={() => void (async () => { for (const p of proposals) await onDecide(p, { action: 'accept' }); })()}>Accept all</Button>
          )}
        </Stack>
        <Stack spacing={0.75}>
          {proposals.map((p) => (
            <Stack key={p.id} direction="row" spacing={1} useFlexGap sx={{ alignItems: 'center', flexWrap: 'wrap' }}>
              <Typography variant="caption" color="text.secondary" sx={{ width: 130, flex: 'none' }}>{FIELD_LABELS[p.field ?? ''] ?? p.field}</Typography>
              <Typography variant="body2" sx={{ flex: 1, minWidth: 140 }}>{p.proposedText}</Typography>
              {p.sourceLabel && p.sourceSegmentId && <SourceChip label={p.sourceLabel} fromImage={p.fromImage} onClick={() => onFocus({ segmentId: p.sourceSegmentId!, quote: p.quote })} />}
              {canEdit && (
                <>
                  <Button size="small" variant="contained" onClick={() => void onDecide(p, { action: 'accept' })} sx={{ minHeight: 26 }}>Accept</Button>
                  <Button size="small" onClick={() => void onDecide(p, { action: 'reject' })} sx={{ minHeight: 26 }}>Reject</Button>
                </>
              )}
            </Stack>
          ))}
        </Stack>
      </CardContent>
    </Card>
  );
}

const KIND_STYLE: Record<string, { label: string; bg: string; fg: string }> = {
  add: { label: 'Add', bg: tokens.primaryLight, fg: tokens.primaryText },
  update: { label: 'Update', bg: tokens.warningLight, fg: tokens.warningText },
  conflict: { label: 'Conflict', bg: tokens.errorLight, fg: tokens.errorText },
};

function ProposalCard({ proposal: p, canEdit, isOwner, onDecide, onFocus, onReopen }: { proposal: Proposal; canEdit: boolean; isOwner: boolean; onDecide: (p: Proposal, d: ProposalDecision) => Promise<void>; onFocus: (f: { segmentId: string; quote?: string | null }) => void; onReopen: () => void }) {
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(p.proposedText);
  const [showWhy, setShowWhy] = useState(false);
  const style = KIND_STYLE[p.kind] ?? KIND_STYLE.add;
  const focusSource = () => p.sourceSegmentId && onFocus({ segmentId: p.sourceSegmentId, quote: p.quote });

  return (
    <Card onFocus={focusSource} onClick={focusSource}>
      <CardContent sx={{ pb: '10px !important', pt: 1.25 }}>
        <Stack direction="row" spacing={1} useFlexGap sx={{ alignItems: 'center', mb: 0.75, flexWrap: 'wrap' }}>
          <Chip size="small" label={style.label} sx={{ height: 20, bgcolor: style.bg, color: style.fg, fontSize: 12 }} />
          {p.sourceLabel && <SourceChip label={p.sourceLabel} fromImage={p.fromImage} removed={!p.sourceSegmentId} onClick={focusSource} />}
          {p.fromImage && <Typography variant="caption" sx={{ color: tokens.warningText }}>Read from image, check the numbers</Typography>}
          {p.why && !showWhy && <Button size="small" onClick={(e) => { e.stopPropagation(); setShowWhy(true); }} sx={{ minHeight: 22, px: 1 }}>Why?</Button>}
        </Stack>
        {showWhy && <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mb: 0.75 }}>{p.why}</Typography>}

        {p.kind === 'conflict' || p.kind === 'update' ? (
          <Box sx={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 1, mb: 1 }}>
            <Box sx={{ p: 1, bgcolor: tokens.fill, borderRadius: 1 }}>
              <Typography variant="caption" color="text.secondary">Current</Typography>
              <Typography variant="body2">{p.currentText}</Typography>
            </Box>
            <Box sx={{ p: 1, bgcolor: tokens.primaryLight, borderRadius: 1 }}>
              <Typography variant="caption" color="text.secondary">New</Typography>
              <Typography variant="body2">{p.proposedText}</Typography>
            </Box>
          </Box>
        ) : editing ? (
          <TextField fullWidth multiline minRows={2} value={text} onChange={(e) => setText(e.target.value)} sx={{ mb: 1 }} autoFocus slotProps={{ htmlInput: { 'aria-label': 'Edit proposed text' } }} />
        ) : (
          <Typography variant="body1" sx={{ mb: 1 }}>{p.proposedText}</Typography>
        )}

        {canEdit && (
          p.stepClosed ? (
            <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }}>
              {isOwner ? (
                <Button size="small" variant="outlined" onClick={(e) => { e.stopPropagation(); onReopen(); }}>Reopen {p.letter} to apply</Button>
              ) : (
                <Typography variant="body2" color="text.secondary">Reopen {p.letter} to apply</Typography>
              )}
              <Button size="small" onClick={() => void onDecide(p, { action: 'reject' })}>Reject</Button>
            </Stack>
          ) : p.kind === 'conflict' ? (
            <Stack direction="row" spacing={0.5} useFlexGap sx={{ flexWrap: 'wrap' }}>
              <Button size="small" variant="outlined" onClick={() => void onDecide(p, { action: 'keepCurrent' })}>Keep current</Button>
              <Button size="small" variant="contained" onClick={() => void onDecide(p, { action: 'useNew' })}>Use new</Button>
              <Button size="small" variant="outlined" onClick={() => void onDecide(p, { action: 'keepBoth' })}>Keep both</Button>
            </Stack>
          ) : editing ? (
            <Stack direction="row" spacing={0.5}>
              <Button size="small" variant="contained" disabled={!text.trim()} onClick={() => void onDecide(p, { action: 'accept', text })}>Accept edit</Button>
              <Button size="small" onClick={() => { setEditing(false); setText(p.proposedText); }}>Cancel</Button>
            </Stack>
          ) : (
            <Stack direction="row" spacing={0.5}>
              <Button size="small" variant="contained" onClick={() => void onDecide(p, { action: 'accept' })}>Accept</Button>
              {p.kind === 'add' && <Button size="small" variant="outlined" onClick={() => setEditing(true)}>Edit</Button>}
              <Button size="small" onClick={() => void onDecide(p, { action: 'reject' })}>Reject</Button>
            </Stack>
          )
        )}
      </CardContent>
    </Card>
  );
}

/**
 * FR-IMP-10: with no AI key, extracted text sits in a pane and the user drags passages into a step.
 * Source chips still apply. Dragging uses pointer events, because the window's file-drop handling
 * takes over the browser's own drag and drop. The step chips do the same in one click.
 */
function ManualPlacement({ sheetId, steps, attachments, canEdit, aiConfigured, projectAi, onPlaced, onFocus }: { sheetId: string; steps: Step[]; attachments: Attachment[]; canEdit: boolean; aiConfigured: boolean; projectAi: boolean; onPlaced: () => Promise<void>; onFocus: (f: { segmentId: string }) => void }) {
  const { notify } = useApp();
  const nav = useNavigate();
  const qc = useQueryClient();
  const usable = attachments.filter((a) => a.readState === 'read' && !a.deletedAt);
  const [attachmentId, setAttachmentId] = useState('');
  const current = attachmentId || usable[0]?.id || '';
  const segments = useQuery({ queryKey: ['segments', current], queryFn: () => call<Segment[]>('segments_list', { attachment: current }), enabled: !!current });
  const [placed, setPlaced] = useState<string[]>([]);
  const [drag, setDrag] = useState<{ x: number; y: number; segment: Segment; text: string; over: Letter | null } | null>(null);

  const place = useCallback(
    async (letter: Letter, segment: Segment, text: string) => {
      try {
        const detail = await qc.fetchQuery<SheetDetail>({ queryKey: ['sheet', sheetId], queryFn: () => call('sheet_detail', { id: sheetId }), staleTime: 0 });
        const step = detail.steps.find((s) => s.letter === letter)!;
        await call('step_add_passage', { step: step.id, expectedVersion: step.version, segment: segment.id, text });
        setPlaced((p) => [...p, segment.id]);
        await onPlaced();
        notify({ severity: 'success', message: `Added to step ${letter}.` });
      } catch (e) {
        notify({ severity: 'error', message: errorMessage(e) });
      }
    },
    [notify, onPlaced, qc, sheetId],
  );

  const passageText = (s: Segment) => {
    // A selected part of the passage wins over the whole passage.
    const sel = window.getSelection()?.toString().trim();
    return sel && s.text.includes(sel) ? sel : s.text;
  };

  const startDrag = (e: ReactPointerEvent, segment: Segment) => {
    e.preventDefault();
    const text = passageText(segment);
    setDrag({ x: e.clientX, y: e.clientY, segment, text, over: null });
    const overAt = (x: number, y: number) => {
      const el = document.elementFromPoint(x, y)?.closest('[data-drop-step]');
      return (el?.getAttribute('data-drop-step') as Letter | null) ?? null;
    };
    const move = (ev: PointerEvent) => setDrag((d) => (d ? { ...d, x: ev.clientX, y: ev.clientY, over: overAt(ev.clientX, ev.clientY) } : d));
    const up = (ev: PointerEvent) => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      const target = overAt(ev.clientX, ev.clientY);
      setDrag(null);
      if (target) void place(target, segment, text);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  };

  if (!usable.length) {
    return <Card><CardContent><Typography variant="body2" color="text.secondary">Add a file on the Attachments tab to place its text here.</Typography></CardContent></Card>;
  }

  return (
    <Box>
      {!aiConfigured && projectAi && (
        <Alert severity="info" sx={{ mb: 2 }} action={<Button size="small" onClick={() => nav('/settings?tab=ai')}>Set up AI in Settings</Button>}>
          With AI set up, these files turn into proposals you can accept with one click.
        </Alert>
      )}
      <Box sx={{ display: 'grid', gridTemplateColumns: 'repeat(4, minmax(0, 1fr))', gap: 1, mb: 2, position: 'sticky', top: -16, zIndex: 2, bgcolor: tokens.fill, py: 1 }}>
        {LETTERS.map((l) => {
          const st = steps.find((s) => s.letter === l)!;
          return (
            <Box
              key={l}
              data-drop-step={st.state === 'open' ? l : undefined}
              sx={{ border: `1.5px dashed ${drag?.over === l ? tokens.primary : tokens.line}`, bgcolor: drag?.over === l ? tokens.primaryLight : '#fff', borderRadius: 1, p: 1, textAlign: 'center', opacity: st.state === 'open' ? 1 : 0.5 }}
            >
              <Typography variant="body2" sx={{ fontWeight: 500 }}>{l}</Typography>
              <Typography variant="caption" color="text.secondary" noWrap sx={{ display: 'block' }}>{st.state === 'open' ? STEP_NAMES[l] : 'Closed'}</Typography>
            </Box>
          );
        })}
      </Box>
      <TextField select fullWidth label="File" value={current} onChange={(e) => setAttachmentId(e.target.value)} sx={{ mb: 2, bgcolor: '#fff' }}>
        {usable.map((a) => <MenuItem key={a.id} value={a.id}>{a.fileName}</MenuItem>)}
      </TextField>
      <Stack spacing={1}>
        {(segments.data ?? []).filter((s) => s.text.trim()).map((s) => (
          <Card key={s.id} onClick={() => onFocus({ segmentId: s.id })}>
            <CardContent sx={{ pb: '10px !important', pt: 1 }}>
              <Stack direction="row" spacing={1} sx={{ alignItems: 'center', mb: 0.5 }}>
                {canEdit && (
                  <Tooltip title="Drag into a step">
                    <Box onPointerDown={(e) => startDrag(e, s)} sx={{ cursor: 'grab', color: tokens.textSecondary, display: 'flex', touchAction: 'none' }} aria-hidden="true"><Icon name="drag_indicator" size={18} /></Box>
                  </Tooltip>
                )}
                <Typography variant="caption" color="text.secondary" sx={{ flex: 1 }}>{s.label}</Typography>
                {placed.includes(s.id) && <Icon name="check" size={16} color={tokens.success} />}
                {canEdit && LETTERS.map((l) => (
                  <Chip key={l} size="small" label={l} variant="outlined" clickable sx={{ height: 22, minWidth: 28 }} aria-label={`Add to step ${l}`} disabled={steps.find((x) => x.letter === l)?.state === 'closed'} onClick={(e) => { e.stopPropagation(); void place(l, s, passageText(s)); }} />
                ))}
              </Stack>
              <Typography variant="body2" sx={{ whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>{s.text}</Typography>
            </CardContent>
          </Card>
        ))}
      </Stack>
      {drag && (
        <Box sx={{ position: 'fixed', left: drag.x + 12, top: drag.y + 12, zIndex: 2000, maxWidth: 320, p: 1, bgcolor: '#fff', border: `1px solid ${tokens.primary}`, borderRadius: 1, pointerEvents: 'none', boxShadow: 2 }}>
          <Typography variant="caption" className="clamp-2">{drag.text}</Typography>
        </Box>
      )}
    </Box>
  );
}
