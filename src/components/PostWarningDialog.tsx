import { useEffect, useMemo, useState } from 'react';
import { Alert, Box, Button, Card, CardContent, Chip, CircularProgress, Dialog, DialogActions, DialogContent, DialogTitle, Stack, TextField, Typography } from '@mui/material';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from 'react-router-dom';
import { call, errorCode, errorMessage, type PickValue, type SheetDetail, type WarningDraft } from '../api';
import { useApp } from '../context';
import { wordCount } from '../lib/text';
import { tokens } from '../theme';
import { ChipPicker, SeverityChip } from './common';
import { causeColors } from './WarningCard';

const SEVERITIES = ['High', 'Medium', 'Low'] as const;

/** Section 10: opens pre-filled by AI, the user adjusts and previews, then posts. Nothing posts by itself. */
export function PostWarningDialog({ open, onClose, detail }: { open: boolean; onClose: () => void; detail: SheetDetail }) {
  const { sheet, steps } = detail;
  const { notify } = useApp();
  const qc = useQueryClient();
  const nav = useNavigate();
  const closed = (l: string) => steps.some((s) => s.letter === l && s.state === 'closed');
  const causeStatus = closed('D') ? 'Solution verified' : closed('B') ? 'Root cause confirmed' : 'Cause not confirmed';

  const [headline, setHeadline] = useState('');
  const [what, setWhat] = useState('');
  const [who, setWho] = useState<string[]>([]);
  const [quick, setQuick] = useState('');
  const [severity, setSeverity] = useState<(typeof SEVERITIES)[number]>('Medium');
  const [preview, setPreview] = useState(false);
  const [drafting, setDrafting] = useState(false);
  const [draftNote, setDraftNote] = useState<string | null>(null);
  const [posting, setPosting] = useState(false);

  const picks = useQuery({ queryKey: ['picks', sheet.projectId], queryFn: () => call<PickValue[]>('pick_values', { project: sheet.projectId }), enabled: open });
  const targets = useMemo(() => (picks.data ?? []).filter((p) => p.listName === 'Area' || p.listName === 'Product').map((p) => p.value), [picks.data]);

  useEffect(() => {
    if (!open) return;
    setHeadline('');
    setWhat('');
    setWho([]);
    setQuick('');
    setPreview(false);
    setDraftNote(null);
    setSeverity((sheet.severity as (typeof SEVERITIES)[number]) ?? 'Medium');
    setDrafting(true);
    call<WarningDraft>('ai_draft_warning', { sheet: sheet.id })
      .then((d) => {
        setHeadline(d.headline);
        setWhat(d.whatHappened);
        setWho(d.whoShouldCheck);
        setQuick(d.quickCheck);
      })
      .catch((e) => {
        const code = errorCode(e);
        // Principle 6: without AI the card is written by hand.
        if (code !== 'AI_NOT_SET' && code !== 'AI_OFF') setDraftNote(errorMessage(e));
      })
      .finally(() => setDrafting(false));
  }, [open, sheet.id, sheet.severity]);

  const words = wordCount(what);
  const valid = headline.trim().length > 0 && headline.length <= 80 && words > 0 && words <= 40 && quick.split('\n').length <= 2;
  const [causeBg, causeFg] = causeColors(causeStatus);

  const post = async () => {
    setPosting(true);
    try {
      const id = await call<string>('warning_post', {
        new: { sheetId: sheet.id, headline, whatHappened: what, whoShouldCheck: who, quickCheck: quick, severity },
      });
      void qc.invalidateQueries({ queryKey: ['wall'] });
      void qc.invalidateQueries({ queryKey: ['sheet-warnings', sheet.id] });
      void qc.invalidateQueries({ queryKey: ['my-work'] });
      onClose();
      nav(`/wall/${id}`);
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
    } finally {
      setPosting(false);
    }
  };

  return (
    <Dialog open={open} onClose={onClose} fullWidth maxWidth="sm">
      <DialogTitle>Post an early warning</DialogTitle>
      <DialogContent>
        {drafting ? (
          <Stack direction="row" spacing={1} sx={{ alignItems: 'center', py: 4, justifyContent: 'center' }} role="status">
            <CircularProgress size={18} />
            <Typography variant="body2" color="text.secondary">Drafting from this A3...</Typography>
          </Stack>
        ) : preview ? (
          <Card sx={{ borderLeft: `3px solid ${tokens.warning}` }}>
            <CardContent>
              <Stack direction="row" spacing={1} useFlexGap sx={{ alignItems: 'center', mb: 0.5, flexWrap: 'wrap' }}>
                <Typography variant="h3" sx={{ flex: 1 }}>{headline}</Typography>
                <SeverityChip severity={severity} />
                <Chip size="small" label={causeStatus} sx={{ height: 20, bgcolor: causeBg, color: causeFg }} />
              </Stack>
              <Typography variant="body1" sx={{ mb: 1 }}>{what}</Typography>
              {quick && (
                <Box sx={{ mb: 1, p: 1, bgcolor: tokens.fill, borderRadius: 1 }}>
                  <Typography variant="caption" color="text.secondary">Quick check</Typography>
                  <Typography variant="body2" sx={{ whiteSpace: 'pre-line' }}>{quick}</Typography>
                </Box>
              )}
              {who.length > 0 && (
                <Stack direction="row" spacing={0.5} useFlexGap sx={{ flexWrap: 'wrap', alignItems: 'center' }}>
                  <Typography variant="caption" color="text.secondary">Who should check:</Typography>
                  {who.map((c) => <Chip key={c} size="small" label={c} variant="outlined" sx={{ height: 20 }} />)}
                </Stack>
              )}
              <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mt: 1 }}>{sheet.number}</Typography>
            </CardContent>
          </Card>
        ) : (
          <Stack spacing={2} sx={{ pt: 0.5 }}>
            {draftNote && <Alert severity="warning">{draftNote}</Alert>}
            <TextField fullWidth label="Headline" value={headline} onChange={(e) => setHeadline(e.target.value)} error={headline.length > 80} helperText={`${headline.length} of 80 characters`} />
            <TextField fullWidth multiline minRows={2} label="What happened" value={what} onChange={(e) => setWhat(e.target.value)} error={words > 40} helperText={`${words} of 40 words`} />
            <Box>
              <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mb: 0.5 }}>Who should check</Typography>
              {targets.length === 0 ? (
                <Typography variant="body2" color="text.secondary">This project has no areas or products on its lists yet. Add them in Settings.</Typography>
              ) : (
                <Stack direction="row" spacing={0.5} useFlexGap sx={{ flexWrap: 'wrap' }}>
                  {targets.map((t) => (
                    <Chip
                      key={t}
                      size="small"
                      label={t}
                      clickable
                      variant={who.includes(t) ? 'filled' : 'outlined'}
                      color={who.includes(t) ? 'primary' : 'default'}
                      onClick={() => setWho(who.includes(t) ? who.filter((x) => x !== t) : [...who, t])}
                    />
                  ))}
                </Stack>
              )}
            </Box>
            <TextField
              fullWidth
              multiline
              minRows={2}
              label="Quick check"
              value={quick}
              onChange={(e) => setQuick(e.target.value)}
              error={quick.split('\n').length > 2}
              helperText="Up to 2 lines. What should people check on their own line?"
            />
            <Box>
              <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mb: 0.5 }}>Severity</Typography>
              <ChipPicker ariaLabel="Severity" options={SEVERITIES} value={severity} onChange={setSeverity} />
            </Box>
            <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }}>
              <Typography variant="caption" color="text.secondary">Cause status</Typography>
              <Chip size="small" label={causeStatus} sx={{ height: 20, bgcolor: causeBg, color: causeFg }} />
            </Stack>
          </Stack>
        )}
      </DialogContent>
      <DialogActions sx={{ px: 3, pb: 2 }}>
        <Button onClick={onClose}>Cancel</Button>
        {preview ? (
          <>
            <Button onClick={() => setPreview(false)}>Back to editing</Button>
            <Button variant="contained" disabled={posting} onClick={post}>{posting ? 'Posting...' : 'Post warning'}</Button>
          </>
        ) : (
          <Button variant="contained" disabled={!valid || drafting} onClick={() => setPreview(true)}>Preview</Button>
        )}
      </DialogActions>
    </Dialog>
  );
}
