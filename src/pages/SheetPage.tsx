import { useEffect, useState } from 'react';
import { Alert, Box, Button, Chip, CircularProgress, Stack, Tab, Tabs, Tooltip, Typography } from '@mui/material';
import { useQuery } from '@tanstack/react-query';
import { useNavigate, useParams, useSearchParams } from 'react-router-dom';
import { call, errorMessage, type SheetDetail } from '../api';
import { Icon, StatusChip, StepStrip } from '../components/common';
import { useApp } from '../context';
import { tokens } from '../theme';
import { OverviewTab } from '../components/sheet/OverviewTab';
import { SheetTab } from '../components/sheet/SheetTab';
import { AttachmentsTab } from '../components/sheet/AttachmentsTab';
import { WarningsTab } from '../components/sheet/WarningsTab';
import { HistoryTab } from '../components/sheet/HistoryTab';
import { ExportDialog } from '../components/ExportDialog';
import { PostWarningDialog } from '../components/PostWarningDialog';
import { SourceDrawer } from '../components/SourcePane';

const TABS = ['overview', 'sheet', 'attachments', 'warnings', 'history'] as const;
const LABELS: Record<(typeof TABS)[number], string> = {
  overview: 'Overview',
  sheet: 'Sheet',
  attachments: 'Attachments',
  warnings: 'Early Warnings',
  history: 'History',
};

export function SheetPage() {
  const { id = '' } = useParams();
  const nav = useNavigate();
  const { notify } = useApp();
  const [params, setParams] = useSearchParams();
  const tab = (TABS as readonly string[]).includes(params.get('tab') ?? '') ? (params.get('tab') as (typeof TABS)[number]) : 'overview';
  const [exportOpen, setExportOpen] = useState(false);
  const [postOpen, setPostOpen] = useState(false);
  const [source, setSource] = useState<{ segmentId: string; quote?: string | null } | null>(null);

  const q = useQuery({ queryKey: ['sheet', id], queryFn: () => call<SheetDetail>('sheet_detail', { id }) });

  useEffect(() => {
    if (q.data) document.title = `${q.data.sheet.number} - A3 Sheet`;
    return () => {
      document.title = 'A3 Sheet';
    };
  }, [q.data]);

  if (q.isLoading) return <Box sx={{ display: 'grid', placeItems: 'center', height: 240 }}><CircularProgress size={28} /></Box>;
  if (q.isError || !q.data) return <Alert severity="error" sx={{ m: 3 }}>{errorMessage(q.error)}</Alert>;
  const { sheet, steps, link } = q.data;

  const copyLink = async () => {
    await navigator.clipboard.writeText(link);
    notify({ severity: 'success', message: 'Link copied.' });
  };

  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', minHeight: '100%' }}>
      <Box sx={{ position: 'sticky', top: 0, zIndex: 5, bgcolor: '#fff', borderBottom: `1px solid ${tokens.line}` }}>
        <Stack direction="row" spacing={2} useFlexGap sx={{ px: 3, pt: 2, alignItems: 'center', flexWrap: 'wrap' }}>
          <Box sx={{ minWidth: 0, flex: 1 }}>
            <Typography variant="caption" color="text.secondary">{sheet.number} · {sheet.projectName}</Typography>
            <Typography variant="h1" noWrap title={sheet.title}>{sheet.title || 'Untitled A3'}</Typography>
          </Box>
          <StatusChip status={sheet.status} withdrawn={!!sheet.withdrawnAt} />
          <StepStrip strip={sheet.stepStrip} size="medium" current={sheet.currentStep} />
          {sheet.pendingReview > 0 && (
            <Chip
              clickable
              label={`${sheet.pendingReview} to review`}
              onClick={() => nav(`/a3/${id}/review`)}
              sx={{ bgcolor: tokens.primaryLight, color: tokens.primaryText, fontWeight: 500 }}
            />
          )}
          <Stack direction="row" spacing={1}>
            <Tooltip title={link}><Button variant="outlined" size="small" startIcon={<Icon name="link" size={18} />} onClick={copyLink}>Copy link</Button></Tooltip>
            <Button variant="outlined" size="small" startIcon={<Icon name="ios_share" size={18} />} onClick={() => setExportOpen(true)}>Export</Button>
            <Tooltip title={sheet.canEdit ? '' : 'Only project members can post from this A3'}>
              <span>
                <Button
                  variant="contained"
                  size="small"
                  color="primary"
                  startIcon={<Icon name="campaign" size={18} />}
                  disabled={!sheet.canEdit || !!sheet.withdrawnAt}
                  onClick={() => setPostOpen(true)}
                >
                  Post warning
                </Button>
              </span>
            </Tooltip>
          </Stack>
        </Stack>
        {sheet.withdrawnAt && <Alert severity="warning" sx={{ mx: 3, mt: 1 }}>Withdrawn: {sheet.withdrawnReason}</Alert>}
        <Tabs value={tab} onChange={(_, v) => setParams({ tab: v }, { replace: true })} sx={{ px: 2 }}>
          {TABS.map((t) => <Tab key={t} value={t} label={LABELS[t]} />)}
        </Tabs>
      </Box>

      <Box sx={{ flex: 1, minHeight: 0 }}>
        {tab === 'overview' && <OverviewTab detail={q.data} />}
        {tab === 'sheet' && <SheetTab sheet={sheet} steps={steps} openConflicts={q.data.openConflicts} onOpenSource={(segmentId, quote) => setSource({ segmentId, quote })} />}
        {tab === 'attachments' && <AttachmentsTab sheet={sheet} onPreview={(segmentId) => setSource({ segmentId })} />}
        {tab === 'warnings' && <WarningsTab sheetId={id} />}
        {tab === 'history' && <HistoryTab sheet={sheet} steps={steps} />}
      </Box>

      <ExportDialog open={exportOpen} onClose={() => setExportOpen(false)} detail={q.data} />
      <PostWarningDialog open={postOpen} onClose={() => setPostOpen(false)} detail={q.data} />
      <SourceDrawer source={source} onClose={() => setSource(null)} />
    </Box>
  );
}
