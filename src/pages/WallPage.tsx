import { useState } from 'react';
import { Alert, Box, Card, CardContent, Chip, CircularProgress, Stack, TextField, Typography } from '@mui/material';
import { useQuery } from '@tanstack/react-query';
import { useParams } from 'react-router-dom';
import { call, errorMessage, LETTERS, STEP_NAMES, type SheetDetail, type Warning, type WallFilter } from '../api';
import { EmptyState, PageHeader, StatusChip, StepStrip } from '../components/common';
import { WarningCard } from '../components/WarningCard';
import { tokens } from '../theme';

const SEVERITIES = ['High', 'Medium', 'Low'];

export function WallPage() {
  const { warningId } = useParams();
  const [filter, setFilter] = useState<WallFilter>({});
  const set = (p: Partial<WallFilter>) => setFilter((f) => ({ ...f, ...p }));

  const q = useQuery({ queryKey: ['wall', filter], queryFn: () => call<Warning[]>('wall_list', { filter }) });
  const single = useQuery({ queryKey: ['warning', warningId], queryFn: () => call<Warning>('warning_get', { id: warningId }), enabled: !!warningId });
  const areas = [...new Set((q.data ?? []).flatMap((w) => w.whoShouldCheck))].sort();

  if (warningId) {
    return (
      <Box sx={{ p: 3 }}>
        <PageHeader title="Early warning" />
        {single.isError && <Alert severity="error">{errorMessage(single.error)}</Alert>}
        {!single.data ? (
          !single.isError && <CircularProgress size={24} />
        ) : (
          // FR-LNK-02: a warning link opens the card with its source A3 alongside.
          <Box sx={{ display: 'grid', gridTemplateColumns: { xs: 'minmax(0, 1fr)', lg: 'minmax(0, 3fr) minmax(0, 2fr)' }, gap: 2, alignItems: 'start' }}>
            <WarningCard warning={single.data} showTasks />
            <SourceA3 sheetId={single.data.sheetId} />
          </Box>
        )}
      </Box>
    );
  }

  const list = q.data ?? [];
  return (
    <Box sx={{ p: 3, maxWidth: 980, mx: 'auto' }}>
      <PageHeader title="Early warning wall" subtitle="What happened elsewhere, and what to check on your own line." />
      <Stack direction="row" spacing={1} useFlexGap sx={{ mb: 2, flexWrap: 'wrap', alignItems: 'center' }}>
        <Chip label="Unread" clickable variant={filter.unread ? 'filled' : 'outlined'} color={filter.unread ? 'primary' : 'default'} onClick={() => set({ unread: !filter.unread })} aria-pressed={!!filter.unread} />
        <Chip label="My tasks" clickable variant={filter.myTasks ? 'filled' : 'outlined'} color={filter.myTasks ? 'primary' : 'default'} onClick={() => set({ myTasks: !filter.myTasks })} aria-pressed={!!filter.myTasks} />
        {SEVERITIES.map((s) => (
          <Chip key={s} label={s} clickable variant={filter.severity === s ? 'filled' : 'outlined'} color={filter.severity === s ? 'primary' : 'default'} onClick={() => set({ severity: filter.severity === s ? null : s })} aria-pressed={filter.severity === s} />
        ))}
        {areas.map((a) => (
          <Chip key={a} label={a} clickable variant={filter.area === a ? 'filled' : 'outlined'} color={filter.area === a ? 'primary' : 'default'} onClick={() => set({ area: filter.area === a ? null : a })} aria-pressed={filter.area === a} />
        ))}
        <Chip label="Archive" clickable variant={filter.archive ? 'filled' : 'outlined'} color={filter.archive ? 'primary' : 'default'} onClick={() => set({ archive: !filter.archive })} aria-pressed={!!filter.archive} />
        <Box sx={{ flex: 1 }} />
        <TextField placeholder="Search warnings" value={filter.search ?? ''} onChange={(e) => set({ search: e.target.value || null })} sx={{ width: 220 }} slotProps={{ htmlInput: { 'aria-label': 'Search warnings' } }} />
      </Stack>
      {q.isSuccess && list.length === 0 && <Card><EmptyState icon="campaign" title="No warnings here" body="Warnings are posted from an A3 with the Post warning button." /></Card>}
      <Stack spacing={2}>{list.map((w) => <WarningCard key={w.id} warning={w} />)}</Stack>
    </Box>
  );
}

function SourceA3({ sheetId }: { sheetId: string }) {
  const q = useQuery({ queryKey: ['sheet', sheetId], queryFn: () => call<SheetDetail>('sheet_detail', { id: sheetId }) });
  if (!q.data) return null;
  const { sheet, steps } = q.data;
  return (
    <Card>
      <CardContent>
        <Typography variant="caption" color="text.secondary">{sheet.number}</Typography>
        <Typography variant="h3" sx={{ mb: 1 }}>{sheet.title || 'Untitled A3'}</Typography>
        <Stack direction="row" spacing={1} sx={{ alignItems: 'center', mb: 2 }}>
          <StatusChip status={sheet.status} withdrawn={!!sheet.withdrawnAt} />
          <StepStrip strip={sheet.stepStrip} current={sheet.currentStep} />
        </Stack>
        <Stack spacing={1.5}>
          {LETTERS.map((l) => {
            const s = steps.find((x) => x.letter === l)!;
            return (
              <Box key={l} sx={{ borderTop: `1px solid ${tokens.line}`, pt: 1 }}>
                <Typography variant="h4">{l}. {STEP_NAMES[l]}</Typography>
                <Typography variant="body2" color={s.plainText ? 'text.primary' : 'text.secondary'} sx={{ whiteSpace: 'pre-wrap' }}>{s.plainText || 'Nothing recorded yet.'}</Typography>
              </Box>
            );
          })}
        </Stack>
      </CardContent>
    </Card>
  );
}
