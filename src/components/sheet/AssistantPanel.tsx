import { useState, type FormEvent } from 'react';
import { Alert, Box, Button, Chip, CircularProgress, Divider, FormControlLabel, IconButton, Stack, Switch, TextField, ToggleButton, ToggleButtonGroup, Tooltip, Typography } from '@mui/material';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from 'react-router-dom';
import { call, errorMessage, type AiMessage, type AiSettingsView, type AskAnswer, type AskKnowledge, type AskOptions, type CitedSource, type Finding, type Letter, type ReviewOutcome, type Sheet } from '../../api';
import { afterCutoff, KNOWLEDGE_NOTE, parseKnowledge, readPref, splitSources, WORKING_NOTE, writePref } from '../../lib/ask';
import { tokens } from '../../theme';
import { Icon, SourceChip } from '../common';

const SUGGESTED = ['What is the target?', 'What evidence supports the root cause?', 'Which actions have no owner?'];

/**
 * AI items for the current A3 only (section 6). It never pops up, shows at most three findings before
 * the user asks for more, and every statement carries its source (principles 3 and 4).
 */
export function AssistantPanel({
  sheet,
  aiReady,
  onCollapse,
  onOpenSource,
  onShowStep,
}: {
  sheet: Sheet;
  aiReady: boolean;
  onCollapse: () => void;
  onOpenSource: (segmentId: string, quote?: string | null) => void;
  onShowStep: (l: Letter) => void;
}) {
  const nav = useNavigate();
  const qc = useQueryClient();
  const [showAll, setShowAll] = useState(false);
  const [question, setQuestion] = useState('');
  // Both choices belong to the person, not the A3, and are remembered on this computer.
  const [knowledge, setKnowledge] = useState<AskKnowledge>(() => parseKnowledge(readPref('a3.ask.knowledge')));
  const [memory, setMemory] = useState(() => readPref('a3.ask.memory') !== 'off');
  const sinceKey = `a3.ask.since.${sheet.id}`;
  const [since, setSince] = useState<string | null>(() => readPref(sinceKey));
  const settings = useQuery({ queryKey: ['ai-settings'], queryFn: () => call<AiSettingsView>('ai_settings_get') });
  const webOk = settings.data?.config?.provider === 'anthropic';
  // A saved Web choice falls back to general knowledge if the provider cannot search.
  const mode: AskKnowledge = knowledge === 'web' && settings.data && !webOk ? 'general' : knowledge;
  const findings = useQuery({ queryKey: ['findings', sheet.id], queryFn: () => call<Finding[]>('findings_list', { sheet: sheet.id }) });
  const messages = useQuery({ queryKey: ['messages', sheet.id], queryFn: () => call<AiMessage[]>('messages_list', { sheet: sheet.id }) });

  const review = useMutation({
    mutationFn: () => call<ReviewOutcome>('ai_review', { sheet: sheet.id }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['findings', sheet.id] }),
  });
  const dismiss = useMutation({
    mutationFn: (id: string) => call('finding_dismiss', { id }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['findings', sheet.id] }),
  });
  const ask = useMutation({
    mutationFn: ({ text, more }: { text: string; more: boolean }) => {
      const options: AskOptions = { knowledge: mode, memory, since };
      return call<AskAnswer>('ai_ask', { sheet: sheet.id, question: text, more, options });
    },
    onSuccess: () => {
      setQuestion('');
      void qc.invalidateQueries({ queryKey: ['messages', sheet.id] });
    },
  });

  const list = findings.data ?? [];
  const visible = showAll ? list : list.slice(0, 3);
  const visibleMessages = afterCutoff(messages.data ?? [], since);
  const recent = visibleMessages.slice(-6);
  const lastAnswer = ask.data;

  const pickKnowledge = (_: unknown, next: AskKnowledge | null) => {
    if (!next) return;
    setKnowledge(next);
    writePref('a3.ask.knowledge', next);
  };
  const toggleMemory = (on: boolean) => {
    setMemory(on);
    writePref('a3.ask.memory', on ? 'on' : 'off');
  };
  // Start fresh: the cutoff is the newest turn's own server timestamp, so nothing is deleted and the
  // record of questions is kept, but earlier turns are neither shown nor sent to the AI.
  const startFresh = () => {
    const newest = (messages.data ?? [])[(messages.data ?? []).length - 1];
    if (!newest) return;
    setSince(newest.createdAt);
    writePref(sinceKey, newest.createdAt);
    ask.reset();
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (question.trim()) ask.mutate({ text: question.trim(), more: false });
  };

  return (
    <Box component="aside" aria-label="Assistant" sx={{ width: 320, flex: 'none', borderLeft: `1px solid ${tokens.line}`, bgcolor: '#fff', display: 'flex', flexDirection: 'column' }}>
      <Stack direction="row" sx={{ px: 1.5, py: 1, alignItems: 'center', borderBottom: `1px solid ${tokens.line}` }}>
        <Icon name="auto_awesome" size={18} color={tokens.primary} />
        <Typography variant="h4" sx={{ flex: 1, ml: 1 }}>Assistant</Typography>
        <Tooltip title="Collapse">
          <IconButton size="small" aria-label="Collapse Assistant" onClick={onCollapse}><Icon name="chevron_right" /></IconButton>
        </Tooltip>
      </Stack>

      {!aiReady ? (
        <Box sx={{ p: 2 }}>
          {sheet.projectAiEnabled ? (
            <>
              <Typography variant="body2" color="text.secondary" sx={{ mb: 1 }}>AI can review this A3 and answer questions about it. Everything else works without it.</Typography>
              <Button size="small" variant="outlined" onClick={() => nav('/settings?tab=ai')}>Set up AI in Settings</Button>
            </>
          ) : (
            <Typography variant="body2" color="text.secondary">AI is turned off for this project.</Typography>
          )}
        </Box>
      ) : (
        <Box sx={{ flex: 1, overflow: 'auto', p: 1.5 }}>
          <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between', mb: 1 }}>
            <Typography variant="body2" sx={{ fontWeight: 500 }}>Validity review</Typography>
            <Button size="small" disabled={review.isPending} onClick={() => review.mutate()} sx={{ minHeight: 26 }}>
              {review.isPending ? 'Checking...' : 'Check now'}
            </Button>
          </Stack>
          {review.isPending && <Stack direction="row" spacing={1} sx={{ alignItems: 'center', mb: 1 }} role="status"><CircularProgress size={14} /><Typography variant="caption" color="text.secondary">Checking this A3...</Typography></Stack>}
          {review.isError && (
            <Alert severity="error" sx={{ mb: 1 }} action={<Button size="small" onClick={() => review.mutate()}>Retry</Button>}>{errorMessage(review.error)}</Alert>
          )}
          {review.data?.note && <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mb: 0.5 }}>Review {review.data.note}.</Typography>}
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mb: 1 }}>Checked against this A3's content</Typography>
          {list.length === 0 && !review.isPending && <Typography variant="body2" color="text.secondary" sx={{ mb: 1 }}>Nothing to report.</Typography>}
          <Stack spacing={1}>
            {visible.map((f) => (
              <FindingCard key={f.id} finding={f} canAct={sheet.canEdit} onDismiss={() => dismiss.mutate(f.id)} onShow={() => onShowStep(f.letter)} onOpenSource={onOpenSource} />
            ))}
          </Stack>
          {list.length > 3 && !showAll && <Button size="small" sx={{ mt: 0.5 }} onClick={() => setShowAll(true)}>More ({list.length - 3})</Button>}

          <Divider sx={{ my: 1.5 }} />
          <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between', mb: 0.5 }}>
            <Typography variant="body2" sx={{ fontWeight: 500 }}>Ask about this A3</Typography>
            {visibleMessages.length > 0 && (
              <Tooltip title="Hide the earlier conversation and stop sending it to the AI">
                <Button size="small" color="inherit" onClick={startFresh} sx={{ minHeight: 24, px: 1, color: tokens.textSecondary }}>Start fresh</Button>
              </Tooltip>
            )}
          </Stack>
          <ToggleButtonGroup
            exclusive
            fullWidth
            size="small"
            value={mode}
            onChange={pickKnowledge}
            aria-label="Where answers may come from"
            sx={{ mb: 0.5, '& .MuiToggleButton-root': { textTransform: 'none', py: 0.25, fontSize: 12, lineHeight: 1.6 } }}
          >
            <ToggleButton value="a3_only" aria-label="This A3 only">This A3</ToggleButton>
            <ToggleButton value="general" aria-label="This A3 plus general knowledge">+ General</ToggleButton>
            <Tooltip title={webOk || !settings.data ? '' : 'Web search needs an Anthropic model. Change it in Settings.'}>
              <span style={{ display: 'flex', flex: 1 }}>
                <ToggleButton value="web" aria-label="This A3 plus web search" disabled={!!settings.data && !webOk} sx={{ flex: 1 }}>+ Web</ToggleButton>
              </span>
            </Tooltip>
          </ToggleButtonGroup>
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mb: 0.5 }}>{KNOWLEDGE_NOTE[mode]}</Typography>
          <FormControlLabel
            sx={{ ml: 0, mb: 1, '& .MuiFormControlLabel-label': { fontSize: 12 } }}
            control={<Switch size="small" checked={memory} onChange={(e) => toggleMemory(e.target.checked)} slotProps={{ input: { 'aria-label': 'Remember this conversation' } }} />}
            label={memory ? 'Remember this conversation' : 'Each question stands alone'}
          />
          {recent.length === 0 && (
            <Stack direction="row" spacing={0.5} useFlexGap sx={{ flexWrap: 'wrap', mb: 1 }}>
              {SUGGESTED.map((q) => <Chip key={q} size="small" variant="outlined" label={q} clickable onClick={() => ask.mutate({ text: q, more: false })} sx={{ height: 'auto', '& .MuiChip-label': { whiteSpace: 'normal', py: 0.25 } }} />)}
            </Stack>
          )}
          <Stack spacing={1} sx={{ mb: 1 }}>
            {recent.map((m) => {
              const { a3, background, links } = splitSources(m.sources as CitedSource[]);
              return (
                <Box key={m.id}>
                  <Typography variant="body2" sx={m.role === 'user' ? { fontWeight: 500 } : undefined}>{m.text}</Typography>
                  {m.role === 'assistant' && a3.length > 0 && (
                    <Stack direction="row" spacing={0.5} useFlexGap sx={{ flexWrap: 'wrap', mt: 0.5 }}>
                      {a3.map((s, i) => (
                        <SourceChip key={i} label={s.label} onClick={() => (s.segmentId ? onOpenSource(s.segmentId, s.quote) : undefined)} />
                      ))}
                    </Stack>
                  )}
                  {m.role === 'assistant' && background && <Background label={background.label} text={background.quote} links={links} />}
                </Box>
              );
            })}
          </Stack>
          {lastAnswer?.note && <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mb: 0.5 }}>Answer {lastAnswer.note}.</Typography>}
          {lastAnswer?.hasMore && ask.variables && !ask.variables.more && !ask.isPending && (
            <Button size="small" sx={{ mb: 1 }} onClick={() => ask.mutate({ text: ask.variables!.text, more: true })}>Tell me more</Button>
          )}
          {ask.isPending && <Stack direction="row" spacing={1} sx={{ alignItems: 'center', mb: 1 }} role="status"><CircularProgress size={14} /><Typography variant="caption" color="text.secondary">{WORKING_NOTE[mode]}</Typography></Stack>}
          {ask.isError && <Alert severity="error" sx={{ mb: 1 }} action={<Button size="small" onClick={() => ask.variables && ask.mutate(ask.variables)}>Retry</Button>}>{errorMessage(ask.error)}</Alert>}
          <Box component="form" onSubmit={submit}>
            <TextField fullWidth placeholder="Type a question" value={question} onChange={(e) => setQuestion(e.target.value)} slotProps={{ htmlInput: { 'aria-label': 'Ask about this A3' } }} />
          </Box>
        </Box>
      )}
    </Box>
  );
}

/**
 * Outside knowledge, set apart from what this A3 says: its own label, a dashed edge and a tint, so it can
 * never be mistaken for a statement about the A3 (principle 3).
 */
function Background({ label, text, links }: { label: string; text: string; links: CitedSource[] }) {
  const fromWeb = label === 'Web search';
  return (
    <Box
      role="note"
      aria-label={`${label}, not from this A3`}
      sx={{ mt: 0.75, p: 1, border: `1px dashed ${tokens.line}`, borderRadius: 1, bgcolor: tokens.fill }}
    >
      <Stack direction="row" spacing={0.5} sx={{ alignItems: 'center', mb: 0.25 }}>
        <Icon name={fromWeb ? 'public' : 'lightbulb'} size={14} color={tokens.textSecondary} />
        <Typography variant="caption" color="text.secondary" sx={{ fontWeight: 500 }}>{label} · not from this A3</Typography>
      </Stack>
      <Typography variant="body2">{text}</Typography>
      {links.length > 0 && (
        <Stack direction="row" spacing={0.5} useFlexGap sx={{ flexWrap: 'wrap', mt: 0.5 }}>
          {links.map((l, i) => (
            <Chip
              key={i}
              size="small"
              variant="outlined"
              icon={<Icon name="open_in_new" size={14} />}
              label={l.label}
              onClick={() => void call('open_web_link', { url: l.url }).catch(() => undefined)}
              sx={{ height: 22, maxWidth: 280, fontSize: 12, '& .MuiChip-icon': { color: 'inherit', ml: 0.75 } }}
            />
          ))}
        </Stack>
      )}
    </Box>
  );
}

function FindingCard({ finding, canAct, onDismiss, onShow, onOpenSource }: { finding: Finding; canAct: boolean; onDismiss: () => void; onShow: () => void; onOpenSource: (id: string, quote?: string | null) => void }) {
  const [why, setWhy] = useState(false);
  return (
    <Box sx={{ border: `1px solid ${tokens.line}`, borderRadius: 1, p: 1 }}>
      <Typography variant="body2">{finding.text}</Typography>
      {why && finding.why && <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mt: 0.5 }}>{finding.why}</Typography>}
      <Stack direction="row" spacing={0.25} useFlexGap sx={{ mt: 0.5, alignItems: 'center', flexWrap: 'wrap' }}>
        {finding.sourceSegmentId && finding.sourceLabel && <SourceChip label={finding.sourceLabel} onClick={() => onOpenSource(finding.sourceSegmentId!, finding.quote)} />}
        <Button size="small" onClick={onShow} sx={{ minHeight: 24, px: 1 }}>Show me</Button>
        {finding.why && !why && <Button size="small" onClick={() => setWhy(true)} sx={{ minHeight: 24, px: 1 }}>Why?</Button>}
        {canAct && <Button size="small" color="inherit" onClick={onDismiss} sx={{ minHeight: 24, px: 1, color: tokens.textSecondary }}>Dismiss</Button>}
      </Stack>
    </Box>
  );
}
