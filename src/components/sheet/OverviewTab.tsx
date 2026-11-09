import { useEffect, useMemo, useState } from 'react';
import { Alert, Autocomplete, Box, Button, Card, CardContent, Chip, Dialog, DialogActions, DialogContent, DialogTitle, MenuItem, Stack, TextField, Typography } from '@mui/material';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { call, errorMessage, LETTERS, STEP_NAMES, type AppUser, type PickValue, type Project, type SheetDetail, type SheetPatch } from '../../api';
import { addDays, formatDate, formatDateTime } from '../../lib/text';
import { useApp } from '../../context';
import { ChipPicker, FieldRow, HealthFlags, Icon } from '../common';
import { tokens } from '../../theme';
import { useNavigate } from 'react-router-dom';

const SEVERITIES = ['High', 'Medium', 'Low'] as const;

function patchFrom(d: SheetDetail): SheetPatch {
  const s = d.sheet;
  return {
    title: s.title,
    projectId: s.projectId,
    ownerId: s.ownerId,
    teamMemberIds: s.teamMemberIds,
    problemDate: s.problemDate,
    targetCloseDate: s.targetCloseDate,
    category: s.category,
    severity: s.severity,
    source: s.source,
    area: s.area,
    product: s.product,
  };
}

export function OverviewTab({ detail }: { detail: SheetDetail }) {
  const { sheet, steps, link } = detail;
  const { notify } = useApp();
  const qc = useQueryClient();
  const nav = useNavigate();
  const [patch, setPatch] = useState<SheetPatch>(() => patchFrom(detail));
  const [dirtyTitle, setDirtyTitle] = useState(false);
  const [withdrawOpen, setWithdrawOpen] = useState(false);
  const [withdrawReason, setWithdrawReason] = useState('');
  const canEdit = sheet.canEdit && !sheet.withdrawnAt;

  useEffect(() => {
    if (!dirtyTitle) setPatch(patchFrom(detail));
    // Keeps typed text while a save or a poll is on its way.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [detail.sheet.updatedAt]);

  const members = useQuery({ queryKey: ['members', sheet.projectId], queryFn: () => call<AppUser[]>('project_members', { project: sheet.projectId }) });
  const picks = useQuery({ queryKey: ['picks', sheet.projectId], queryFn: () => call<PickValue[]>('pick_values', { project: sheet.projectId }) });
  const projects = useQuery({ queryKey: ['projects', 'mine'], queryFn: () => call<Project[]>('projects_list', { onlyMine: true }) });
  const list = (name: PickValue['listName']) => (picks.data ?? []).filter((p) => p.listName === name).map((p) => p.value);

  const projectOptions = useMemo(() => {
    const mine = (projects.data ?? []).filter((p) => !p.archivedAt);
    return mine.some((p) => p.id === sheet.projectId) ? mine : [{ id: sheet.projectId, name: sheet.projectName } as Project, ...mine];
  }, [projects.data, sheet.projectId, sheet.projectName]);

  const save = useMutation({
    mutationFn: (p: SheetPatch) => call('sheet_update', { id: sheet.id, patch: p }),
    onSuccess: () => {
      setDirtyTitle(false);
      void qc.invalidateQueries({ queryKey: ['sheet', sheet.id] });
      void qc.invalidateQueries({ queryKey: ['sheets'] });
    },
    onError: (e) => {
      notify({ severity: 'error', message: errorMessage(e) });
      setPatch(patchFrom(detail));
    },
  });

  const change = (p: Partial<SheetPatch>) => {
    const next = { ...patch, ...p };
    setPatch(next);
    save.mutate(next);
  };

  const withdraw = useMutation({
    mutationFn: () => call('sheet_withdraw', { id: sheet.id, reason: withdrawReason }),
    onSuccess: () => {
      setWithdrawOpen(false);
      void qc.invalidateQueries({ queryKey: ['sheet', sheet.id] });
      void qc.invalidateQueries({ queryKey: ['sheets'] });
    },
    onError: (e) => notify({ severity: 'error', message: errorMessage(e) }),
  });

  const userName = (id: string) => members.data?.find((m) => m.id === id)?.displayName ?? '';

  return (
    <Box sx={{ p: 3, display: 'grid', gridTemplateColumns: 'minmax(0, 1fr) 340px', gap: 2, alignItems: 'start', maxWidth: 1240 }}>
      <Card>
        <CardContent>
          <Stack spacing={1}>
            <FieldRow label="A3 number"><Typography variant="body1">{sheet.number}</Typography></FieldRow>
            <FieldRow label="Title">
              <TextField
                fullWidth
                disabled={!canEdit}
                value={patch.title}
                placeholder="A plain problem title"
                error={patch.title.length > 90}
                helperText={patch.title.length > 70 ? `${patch.title.length} of 90 characters` : undefined}
                onChange={(e) => {
                  setDirtyTitle(true);
                  setPatch({ ...patch, title: e.target.value });
                }}
                onBlur={() => dirtyTitle && patch.title.length <= 90 && save.mutate(patch)}
                slotProps={{ htmlInput: { 'aria-label': 'Title' } }}
              />
            </FieldRow>
            <FieldRow label="Project">
              <TextField select fullWidth disabled={!canEdit} value={patch.projectId} onChange={(e) => change({ projectId: e.target.value })} slotProps={{ htmlInput: { 'aria-label': 'Project' } }}>
                {projectOptions.map((p) => <MenuItem key={p.id} value={p.id}>{p.name}</MenuItem>)}
              </TextField>
            </FieldRow>
            <FieldRow label="Owner">
              <TextField select fullWidth disabled={!sheet.isOwner || !!sheet.withdrawnAt} value={members.data ? patch.ownerId : ''} onChange={(e) => change({ ownerId: e.target.value })} slotProps={{ htmlInput: { 'aria-label': 'Owner' } }}>
                {(members.data ?? []).map((m) => <MenuItem key={m.id} value={m.id}>{m.displayName}</MenuItem>)}
              </TextField>
            </FieldRow>
            <FieldRow label="Team members">
              <Autocomplete
                multiple
                size="small"
                disabled={!canEdit}
                options={(members.data ?? []).map((m) => m.id)}
                value={patch.teamMemberIds}
                getOptionLabel={(id) => userName(id) || 'Former member'}
                onChange={(_, v) => change({ teamMemberIds: v })}
                renderInput={(p) => <TextField {...p} placeholder={patch.teamMemberIds.length ? '' : 'Add people'} />}
              />
            </FieldRow>
            <FieldRow label="Date registered"><Typography variant="body1">{formatDate(sheet.registeredOn)}</Typography></FieldRow>
            <FieldRow label="Problem occurred on">
              <TextField type="date" disabled={!canEdit} value={patch.problemDate ?? ''} onChange={(e) => change({ problemDate: e.target.value || null })} slotProps={{ htmlInput: { 'aria-label': 'Problem occurred on' } }} />
            </FieldRow>
            <FieldRow label="Target close date">
              <Stack direction="row" spacing={1} useFlexGap sx={{ alignItems: 'center', flexWrap: 'wrap' }}>
                <TextField type="date" disabled={!canEdit} value={patch.targetCloseDate ?? ''} onChange={(e) => change({ targetCloseDate: e.target.value || null })} slotProps={{ htmlInput: { 'aria-label': 'Target close date' } }} />
                {[30, 60, 90].map((n) => (
                  <Chip key={n} size="small" label={`+${n} days`} disabled={!canEdit} variant="outlined" clickable onClick={() => change({ targetCloseDate: addDays(n, new Date(`${sheet.registeredOn}T00:00:00`)) })} />
                ))}
              </Stack>
            </FieldRow>
            <FieldRow label="Category">
              <TextField select fullWidth disabled={!canEdit} value={picks.data ? patch.category ?? '' : ''} onChange={(e) => change({ category: e.target.value || null })} slotProps={{ select: { displayEmpty: true }, htmlInput: { 'aria-label': 'Category' } }}>
                <MenuItem value=""><em>Not set</em></MenuItem>
                {[...new Set([...list('Category'), ...(patch.category ? [patch.category] : [])])].map((v) => <MenuItem key={v} value={v}>{v}</MenuItem>)}
              </TextField>
            </FieldRow>
            <FieldRow label="Severity">
              <ChipPicker ariaLabel="Severity" options={SEVERITIES} value={patch.severity as (typeof SEVERITIES)[number] | null} disabled={!canEdit} onChange={(v) => change({ severity: v })} />
            </FieldRow>
            <FieldRow label="Source">
              <TextField select fullWidth disabled={!canEdit} value={picks.data ? patch.source ?? '' : ''} onChange={(e) => change({ source: e.target.value || null })} slotProps={{ select: { displayEmpty: true }, htmlInput: { 'aria-label': 'Source' } }}>
                <MenuItem value=""><em>Not set</em></MenuItem>
                {[...new Set([...list('Source'), ...(patch.source ? [patch.source] : [])])].map((v) => <MenuItem key={v} value={v}>{v}</MenuItem>)}
              </TextField>
            </FieldRow>
            <FieldRow label="Area or line">
              <Autocomplete
                freeSolo
                size="small"
                disabled={!canEdit}
                options={list('Area')}
                value={patch.area ?? ''}
                onChange={(_, v) => change({ area: v || null })}
                onBlur={(e) => {
                  const v = (e.target as HTMLInputElement).value.trim() || null;
                  if (v !== patch.area) change({ area: v });
                }}
                renderInput={(p) => <TextField {...p} placeholder="Type to search" slotProps={{ ...p.slotProps, htmlInput: { ...p.slotProps.htmlInput, 'aria-label': 'Area or line' } }} />}
              />
            </FieldRow>
            <FieldRow label="Product or part">
              <Autocomplete
                freeSolo
                size="small"
                disabled={!canEdit}
                options={list('Product')}
                value={patch.product ?? ''}
                onChange={(_, v) => change({ product: v || null })}
                onBlur={(e) => {
                  const v = (e.target as HTMLInputElement).value.trim() || null;
                  if (v !== patch.product) change({ product: v });
                }}
                renderInput={(p) => <TextField {...p} placeholder="Type to search" slotProps={{ ...p.slotProps, htmlInput: { ...p.slotProps.htmlInput, 'aria-label': 'Product or part' } }} />}
              />
            </FieldRow>
          </Stack>
        </CardContent>
      </Card>

      <Stack spacing={2}>
        <Card>
          <CardContent>
            <Typography variant="h4" sx={{ mb: 1 }}>Status</Typography>
            <Stack spacing={1}>
              <Typography variant="body2">
                {sheet.withdrawnAt ? 'Withdrawn' : sheet.status === 'Closed' ? `Closed on ${formatDate(sheet.closedAt)}` : `Open, at step ${sheet.currentStep}`}
              </Typography>
              <HealthFlags row={sheet} />
              {LETTERS.map((l) => {
                const s = steps.find((x) => x.letter === l)!;
                return (
                  <Stack key={l} direction="row" spacing={1} sx={{ alignItems: 'center' }}>
                    <Icon name={s.state === 'closed' ? 'check_circle' : 'radio_button_unchecked'} size={18} color={s.state === 'closed' ? tokens.success : tokens.textSecondary} />
                    <Typography variant="body2" sx={{ flex: 1 }} noWrap>{l}. {STEP_NAMES[l]}</Typography>
                    <Typography variant="caption" color="text.secondary">{s.state === 'closed' ? formatDate(s.closedAt) : 'Open'}</Typography>
                  </Stack>
                );
              })}
              <Typography variant="caption" color="text.secondary">Last updated {formatDateTime(sheet.updatedAt)}</Typography>
            </Stack>
          </CardContent>
        </Card>
        {sheet.pendingReview > 0 && (
          <Alert severity="info" action={<Button size="small" onClick={() => nav(`/a3/${sheet.id}/review`)}>Review</Button>}>
            {sheet.pendingReview} proposals waiting for review.
          </Alert>
        )}
        {sheet.originWarningId && (
          <Alert severity="info" icon={<Icon name="campaign" />} action={<Button size="small" onClick={() => nav(`/wall/${sheet.originWarningId}`)}>Open</Button>}>
            Created from an early warning.
          </Alert>
        )}
        <Card>
          <CardContent>
            <Typography variant="h4" sx={{ mb: 0.5 }}>Link</Typography>
            <Typography variant="body2" sx={{ wordBreak: 'break-all', color: tokens.textSecondary }}>{link}</Typography>
          </CardContent>
        </Card>
        {sheet.isOwner && !sheet.withdrawnAt && (
          <Button color="inherit" size="small" sx={{ alignSelf: 'flex-start', color: tokens.textSecondary }} onClick={() => setWithdrawOpen(true)}>Withdraw this A3</Button>
        )}
      </Stack>

      <Dialog open={withdrawOpen} onClose={() => setWithdrawOpen(false)} fullWidth maxWidth="xs">
        <DialogTitle>Withdraw this A3?</DialogTitle>
        <DialogContent>
          <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>For a duplicate or mistaken A3. It leaves all counts but stays retrievable.</Typography>
          <TextField fullWidth autoFocus label="Reason" value={withdrawReason} onChange={(e) => setWithdrawReason(e.target.value)} />
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setWithdrawOpen(false)}>Cancel</Button>
          <Button variant="contained" color="error" disabled={!withdrawReason.trim() || withdraw.isPending} onClick={() => withdraw.mutate()}>Withdraw</Button>
        </DialogActions>
      </Dialog>
    </Box>
  );
}
