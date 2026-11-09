import { useEffect, useMemo, useState } from 'react';
import { Box, Button, Card, Chip, Collapse, MenuItem, Stack, Table, TableBody, TableCell, TableHead, TablePagination, TableRow, TableSortLabel, TextField, Typography } from '@mui/material';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { call, type AppUser, type PickValue, type Project, type SheetFilter, type SheetPage } from '../api';
import { EmptyState, HealthFlags, Icon, PageHeader, SeverityChip, StatusChip, StepStrip } from '../components/common';
import { useApp } from '../context';
import { formatDate, formatDateTime } from '../lib/text';

const QUICK = [
  { key: 'mine', label: 'My A3s' },
  { key: 'open', label: 'Open' },
  { key: 'overdue', label: 'Overdue' },
  { key: 'stalled', label: 'Stalled' },
  { key: 'closed', label: 'Closed' },
  { key: 'withdrawn', label: 'Withdrawn' },
];

const COLUMNS = [
  { key: 'number', label: 'Number' },
  { key: 'title', label: 'Title' },
  { key: 'project', label: 'Project' },
  { key: 'owner', label: 'Owner' },
  { key: 'step', label: 'Steps' },
  { key: 'status', label: 'Status' },
  { key: 'health', label: 'Health' },
  { key: 'severity', label: 'Severity' },
  { key: 'target', label: 'Target' },
  { key: 'updated', label: 'Updated' },
];

export function SheetListPage() {
  const nav = useNavigate();
  const { openNewA3 } = useApp();
  const [params, setParams] = useSearchParams();
  const [showFilters, setShowFilters] = useState(false);
  const [page, setPage] = useState(0);
  const [sort, setSort] = useState('updated');
  const [desc, setDesc] = useState(true);

  const get = (k: string) => params.get(k) || null;
  const quickKey = get('quick');
  const filter: SheetFilter = useMemo(
    () => ({
      mine: get('mine') === '1',
      quick: quickKey,
      projectId: get('project'),
      ownerId: get('owner'),
      category: get('category'),
      area: get('area'),
      from: get('from'),
      to: get('to'),
      search: get('search'),
      currentStep: get('step'),
      ageBand: get('age'),
      closedFrom: get('closedFrom'),
      reopenedFrom: get('reopenedFrom'),
      reopenReason: get('reason'),
      sort,
      descending: desc,
      page,
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [params, sort, desc, page],
  );
  useEffect(() => setPage(0), [params]);

  const q = useQuery({ queryKey: ['sheets', filter], queryFn: () => call<SheetPage>('sheets_list', { filter }), placeholderData: keepPreviousData });
  const projects = useQuery({ queryKey: ['projects', 'all'], queryFn: () => call<Project[]>('projects_list', { onlyMine: false }), enabled: showFilters });
  const users = useQuery({ queryKey: ['users'], queryFn: () => call<AppUser[]>('users_list'), enabled: showFilters });
  const picks = useQuery({ queryKey: ['picks', filter.projectId ?? 'company'], queryFn: () => call<PickValue[]>('pick_values', { project: filter.projectId ?? null }), enabled: showFilters });

  const setParam = (k: string, v: string | null) => {
    const next = new URLSearchParams(params);
    if (v) next.set(k, v);
    else next.delete(k);
    setParams(next, { replace: true });
  };

  const toggleQuick = (k: string) => {
    if (k === 'mine') setParam('mine', get('mine') === '1' ? null : '1');
    else setParam('quick', quickKey === k ? null : k);
  };

  const rows = q.data?.rows ?? [];
  const drill = ['step', 'age', 'closedFrom', 'reopenedFrom', 'reason', 'search'].filter((k) => get(k));

  return (
    <Box sx={{ p: 3 }}>
      <PageHeader title="A3 Sheets" subtitle={q.data ? `${q.data.total} ${q.data.total === 1 ? 'A3' : 'A3s'}` : undefined} actions={<Button variant="text" startIcon={<Icon name="tune" size={18} />} onClick={() => setShowFilters(!showFilters)}>Filters</Button>} />
      <Stack direction="row" spacing={1} useFlexGap sx={{ mb: 1.5, flexWrap: 'wrap', alignItems: 'center' }}>
        {QUICK.map((c) => {
          const on = c.key === 'mine' ? get('mine') === '1' : quickKey === c.key;
          return <Chip key={c.key} label={c.label} clickable variant={on ? 'filled' : 'outlined'} color={on ? 'primary' : 'default'} onClick={() => toggleQuick(c.key)} aria-pressed={on} />;
        })}
        {drill.map((k) => <Chip key={k} label={`${k === 'search' ? 'Search' : k === 'step' ? 'Current step' : k === 'age' ? 'Age' : k === 'reason' ? 'Reason' : 'Since'}: ${get(k)}`} onDelete={() => setParam(k, null)} />)}
      </Stack>
      <Collapse in={showFilters}>
        <Stack direction="row" spacing={1.5} useFlexGap sx={{ mb: 2, flexWrap: 'wrap' }}>
          <TextField select label="Project" value={get('project') ?? ''} onChange={(e) => setParam('project', e.target.value)} sx={{ minWidth: 180 }} slotProps={{ select: { displayEmpty: true }, inputLabel: { shrink: true } }}>
            <MenuItem value="">Any</MenuItem>
            {(projects.data ?? []).map((p) => <MenuItem key={p.id} value={p.id}>{p.name}</MenuItem>)}
          </TextField>
          <TextField select label="Owner" value={get('owner') ?? ''} onChange={(e) => setParam('owner', e.target.value)} sx={{ minWidth: 180 }} slotProps={{ select: { displayEmpty: true }, inputLabel: { shrink: true } }}>
            <MenuItem value="">Any</MenuItem>
            {(users.data ?? []).map((u) => <MenuItem key={u.id} value={u.id}>{u.displayName}</MenuItem>)}
          </TextField>
          <TextField select label="Category" value={get('category') ?? ''} onChange={(e) => setParam('category', e.target.value)} sx={{ minWidth: 150 }} slotProps={{ select: { displayEmpty: true }, inputLabel: { shrink: true } }}>
            <MenuItem value="">Any</MenuItem>
            {(picks.data ?? []).filter((p) => p.listName === 'Category').map((p) => <MenuItem key={p.id} value={p.value}>{p.value}</MenuItem>)}
          </TextField>
          <TextField label="Area" value={get('area') ?? ''} onChange={(e) => setParam('area', e.target.value)} sx={{ minWidth: 150 }} />
          <TextField label="Registered from" type="date" value={get('from') ?? ''} onChange={(e) => setParam('from', e.target.value)} slotProps={{ inputLabel: { shrink: true } }} />
          <TextField label="Registered to" type="date" value={get('to') ?? ''} onChange={(e) => setParam('to', e.target.value)} slotProps={{ inputLabel: { shrink: true } }} />
        </Stack>
      </Collapse>
      <Card sx={{ overflowX: 'auto' }}>
        {q.isSuccess && rows.length === 0 ? (
          <EmptyState icon="description" title="No A3s here" body="Change the filters, or start a new A3 from the files you already have." action={<Button variant="contained" onClick={openNewA3}>New A3</Button>} />
        ) : (
          <>
            <Table size="small">
              <TableHead>
                <TableRow>
                  {COLUMNS.map((c) => (
                    <TableCell key={c.key} sortDirection={sort === c.key ? (desc ? 'desc' : 'asc') : false}>
                      <TableSortLabel active={sort === c.key} direction={sort === c.key && desc ? 'desc' : 'asc'} onClick={() => { if (sort === c.key) setDesc(!desc); else { setSort(c.key); setDesc(false); } }}>
                        {c.label}
                      </TableSortLabel>
                    </TableCell>
                  ))}
                </TableRow>
              </TableHead>
              <TableBody>
                {rows.map((r) => (
                  <TableRow key={r.id} hover tabIndex={0} sx={{ cursor: 'pointer' }} onClick={() => nav(`/a3/${r.id}`)} onKeyDown={(e) => e.key === 'Enter' && nav(`/a3/${r.id}`)}>
                    <TableCell sx={{ whiteSpace: 'nowrap' }}>{r.number}</TableCell>
                    <TableCell sx={{ maxWidth: 320 }}>
                      <Typography variant="body2" noWrap title={r.title}>{r.title || 'Untitled A3'}</Typography>
                      {r.pendingReview > 0 && <Typography variant="caption" color="primary">{r.pendingReview} to review</Typography>}
                    </TableCell>
                    <TableCell>{r.projectCode}</TableCell>
                    <TableCell sx={{ whiteSpace: 'nowrap' }}>{r.ownerName}</TableCell>
                    <TableCell><StepStrip strip={r.stepStrip} current={r.currentStep} /></TableCell>
                    <TableCell><StatusChip status={r.status} withdrawn={r.withdrawn} /></TableCell>
                    <TableCell><HealthFlags row={r} /></TableCell>
                    <TableCell><SeverityChip severity={r.severity} /></TableCell>
                    <TableCell sx={{ whiteSpace: 'nowrap' }}>{r.targetCloseDate ? formatDate(r.targetCloseDate) : ''}</TableCell>
                    <TableCell sx={{ whiteSpace: 'nowrap' }}>{formatDateTime(r.updatedAt)}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
            <TablePagination component="div" count={q.data?.total ?? 0} page={page} rowsPerPage={50} rowsPerPageOptions={[50]} onPageChange={(_, p) => setPage(p)} />
          </>
        )}
      </Card>
    </Box>
  );
}
