import { useMemo, useState } from 'react';
import { Box, Button, Card, CardContent, Chip, List, ListItemButton, Stack, Tab, Tabs, Typography } from '@mui/material';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { call, errorMessage, LETTERS, type AuditRow, type Letter, type Sheet, type Snapshot, type Step } from '../../api';
import { useApp } from '../../context';
import { auditLabel, formatDateTime, wordDiff } from '../../lib/text';
import { tokens } from '../../theme';
import { EmptyState } from '../common';

const REASONS: Record<string, string> = { accept: 'Accepted proposal', close: 'Closed', reopen: 'Reopened', autosave: 'Editing', restore: 'Restored' };

export function HistoryTab({ sheet, steps }: { sheet: Sheet; steps: Step[] }) {
  const [view, setView] = useState<'changes' | 'versions'>('changes');
  const audit = useQuery({ queryKey: ['history', sheet.id], queryFn: () => call<AuditRow[]>('sheet_history', { id: sheet.id }) });
  const snaps = useQuery({ queryKey: ['snapshots', sheet.id], queryFn: () => call<Snapshot[]>('sheet_snapshots', { id: sheet.id }) });

  return (
    <Box sx={{ p: 3, maxWidth: 1240 }}>
      <Tabs value={view} onChange={(_, v) => setView(v)} sx={{ mb: 2 }}>
        <Tab value="changes" label="Who changed what" />
        <Tab value="versions" label="Versions" />
      </Tabs>
      {view === 'changes' ? (
        <Card>
          {audit.data?.length === 0 && <EmptyState icon="history" title="No changes yet" />}
          <Stack>
            {(audit.data ?? []).map((a, i) => (
              <Stack key={a.id} direction="row" spacing={2} sx={{ px: 2, py: 1, borderTop: i ? `1px solid ${tokens.line}` : 'none', alignItems: 'baseline' }}>
                <Typography variant="caption" color="text.secondary" sx={{ width: 150, flex: 'none' }}>{formatDateTime(a.at)}</Typography>
                <Typography variant="body2" sx={{ width: 170, flex: 'none' }} noWrap>{a.actorName ?? 'System'}</Typography>
                <Typography variant="body2" sx={{ flex: 1 }}>{auditLabel(a.action, a.detail)}{a.reason ? `: ${a.reason}` : ''}</Typography>
              </Stack>
            ))}
          </Stack>
        </Card>
      ) : (
        <Versions sheet={sheet} steps={steps} snapshots={snaps.data ?? []} />
      )}
    </Box>
  );
}

/** FR-A3-10: snapshots side by side with the current text. The owner can restore one. */
function Versions({ sheet, steps, snapshots }: { sheet: Sheet; steps: Step[]; snapshots: Snapshot[] }) {
  const { notify } = useApp();
  const qc = useQueryClient();
  const [letter, setLetter] = useState<Letter>(sheet.currentStep ?? 'A');
  const [selected, setSelected] = useState<string | null>(null);
  const list = useMemo(() => snapshots.filter((s) => s.letter === letter), [snapshots, letter]);
  const snap = list.find((s) => s.id === selected) ?? list[0];
  const step = steps.find((s) => s.letter === letter)!;

  const restore = async () => {
    if (!snap) return;
    try {
      await call('snapshot_restore', { snapshot: snap.id });
      void qc.invalidateQueries({ queryKey: ['sheet', sheet.id] });
      void qc.invalidateQueries({ queryKey: ['snapshots', sheet.id] });
      void qc.invalidateQueries({ queryKey: ['history', sheet.id] });
      notify({ severity: 'success', message: `Step ${letter} restored. The previous text stays in History.` });
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
    }
  };

  return (
    <Box>
      <Stack direction="row" spacing={1} sx={{ mb: 2 }}>
        {LETTERS.map((l) => (
          <Chip key={l} label={`Step ${l}`} clickable color={letter === l ? 'primary' : 'default'} variant={letter === l ? 'filled' : 'outlined'} onClick={() => { setLetter(l); setSelected(null); }} />
        ))}
      </Stack>
      {list.length === 0 ? (
        <Card><EmptyState icon="history" title={`No versions of step ${letter} yet`} body="A version is kept on every accept, close and reopen, and every 10 minutes while editing." /></Card>
      ) : (
        <Box sx={{ display: 'grid', gridTemplateColumns: '260px minmax(0, 1fr) minmax(0, 1fr)', gap: 2, alignItems: 'start' }}>
          <Card>
            <List dense disablePadding sx={{ maxHeight: 520, overflow: 'auto' }}>
              {list.map((s) => (
                <ListItemButton key={s.id} selected={snap?.id === s.id} onClick={() => setSelected(s.id)} sx={{ display: 'block' }}>
                  <Typography variant="body2">{formatDateTime(s.createdAt)}</Typography>
                  <Typography variant="caption" color="text.secondary">{REASONS[s.reason] ?? s.reason}{s.authorName ? `, ${s.authorName}` : ''}</Typography>
                </ListItemButton>
              ))}
            </List>
          </Card>
          <Card>
            <CardContent>
              <Stack direction="row" sx={{ mb: 1, alignItems: 'center', justifyContent: 'space-between' }}>
                <Typography variant="h4">This version</Typography>
                {sheet.isOwner && step.state === 'open' && <Button size="small" variant="outlined" onClick={restore}>Restore</Button>}
              </Stack>
              <Typography variant="body2" sx={{ whiteSpace: 'pre-wrap' }}>{snap?.plainText || <em>Empty</em>}</Typography>
            </CardContent>
          </Card>
          <Card>
            <CardContent>
              <Typography variant="h4" sx={{ mb: 1 }}>Current text, compared</Typography>
              <Typography variant="body2" sx={{ whiteSpace: 'pre-wrap' }}>
                {wordDiff(snap?.plainText ?? '', step.plainText).map((p, i) =>
                  p.kind === 'same' ? <span key={i}>{p.text}</span> : <span key={i} className={p.kind === 'del' ? 'diff-del' : 'diff-add'}>{p.text}</span>,
                )}
              </Typography>
            </CardContent>
          </Card>
        </Box>
      )}
    </Box>
  );
}
