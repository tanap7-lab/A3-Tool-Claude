import { useState, type FormEvent } from 'react';
import { Alert, Box, Button, Chip, CircularProgress, Divider, IconButton, Stack, TextField, Tooltip, Typography } from '@mui/material';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from 'react-router-dom';
import { call, errorMessage, type AiMessage, type AskAnswer, type CitedSource, type Finding, type Letter, type ReviewOutcome, type Sheet } from '../../api';
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
    mutationFn: ({ text, more }: { text: string; more: boolean }) => call<AskAnswer>('ai_ask', { sheet: sheet.id, question: text, more }),
    onSuccess: () => {
      setQuestion('');
      void qc.invalidateQueries({ queryKey: ['messages', sheet.id] });
    },
  });

  const list = findings.data ?? [];
  const visible = showAll ? list : list.slice(0, 3);
  const recent = (messages.data ?? []).slice(-6);
  const lastAnswer = ask.data;

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
          <Typography variant="body2" sx={{ fontWeight: 500, mb: 1 }}>Ask about this A3</Typography>
          {recent.length === 0 && (
            <Stack direction="row" spacing={0.5} useFlexGap sx={{ flexWrap: 'wrap', mb: 1 }}>
              {SUGGESTED.map((q) => <Chip key={q} size="small" variant="outlined" label={q} clickable onClick={() => ask.mutate({ text: q, more: false })} sx={{ height: 'auto', '& .MuiChip-label': { whiteSpace: 'normal', py: 0.25 } }} />)}
            </Stack>
          )}
          <Stack spacing={1} sx={{ mb: 1 }}>
            {recent.map((m) => (
              <Box key={m.id}>
                <Typography variant="body2" sx={m.role === 'user' ? { fontWeight: 500 } : undefined}>{m.text}</Typography>
                {m.role === 'assistant' && (m.sources as CitedSource[]).length > 0 && (
                  <Stack direction="row" spacing={0.5} useFlexGap sx={{ flexWrap: 'wrap', mt: 0.5 }}>
                    {(m.sources as CitedSource[]).map((s, i) => (
                      <SourceChip key={i} label={s.label} onClick={() => (s.segmentId ? onOpenSource(s.segmentId, s.quote) : undefined)} />
                    ))}
                  </Stack>
                )}
              </Box>
            ))}
          </Stack>
          {lastAnswer?.note && <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mb: 0.5 }}>Answer {lastAnswer.note}.</Typography>}
          {lastAnswer?.hasMore && ask.variables && !ask.variables.more && !ask.isPending && (
            <Button size="small" sx={{ mb: 1 }} onClick={() => ask.mutate({ text: ask.variables!.text, more: true })}>Tell me more</Button>
          )}
          {ask.isPending && <Stack direction="row" spacing={1} sx={{ alignItems: 'center', mb: 1 }} role="status"><CircularProgress size={14} /><Typography variant="caption" color="text.secondary">Looking in this A3...</Typography></Stack>}
          {ask.isError && <Alert severity="error" sx={{ mb: 1 }} action={<Button size="small" onClick={() => ask.variables && ask.mutate(ask.variables)}>Retry</Button>}>{errorMessage(ask.error)}</Alert>}
          <Box component="form" onSubmit={submit}>
            <TextField fullWidth placeholder="Type a question" value={question} onChange={(e) => setQuestion(e.target.value)} slotProps={{ htmlInput: { 'aria-label': 'Ask about this A3' } }} />
          </Box>
        </Box>
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
