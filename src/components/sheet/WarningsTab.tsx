import { Box, Card, Stack } from '@mui/material';
import { useQuery } from '@tanstack/react-query';
import { call, type Warning } from '../../api';
import { EmptyState } from '../common';
import { WarningCard } from '../WarningCard';

/** FR-EW-06: this A3's cards and their check results. */
export function WarningsTab({ sheetId }: { sheetId: string }) {
  const q = useQuery({ queryKey: ['sheet-warnings', sheetId], queryFn: () => call<Warning[]>('warnings_for_sheet', { sheet: sheetId }) });
  const list = q.data ?? [];
  return (
    <Box sx={{ p: 3, maxWidth: 900 }}>
      {q.isSuccess && list.length === 0 ? (
        <Card><EmptyState icon="campaign" title="No early warnings from this A3" body="Use Post warning when others should know what happened and what to check." /></Card>
      ) : (
        <Stack spacing={2}>{list.map((w) => <WarningCard key={w.id} warning={w} showTasks />)}</Stack>
      )}
    </Box>
  );
}
