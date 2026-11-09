import { useState } from 'react';
import { Box, Button, Card, CardContent, Chip, Divider, MenuItem, Stack, Tab, Table, TableBody, TableCell, TableHead, TableRow, Tabs, TextField, Tooltip, Typography } from '@mui/material';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from 'react-router-dom';
import { call, errorMessage, type LabelCount, type Management, type MgmtFilter, type MyWork, type PickValue, type Project, type SheetRow } from '../api';
import { EmptyState, HealthFlags, SeverityChip, StepStrip } from '../components/common';
import { TaskRow } from '../components/WarningCard';
import { useApp } from '../context';
import { formatDate } from '../lib/text';
import { tokens } from '../theme';

export function DashboardPage() {
  const [tab, setTab] = useState<'work' | 'management'>('work');
  return (
    <Box>
      <Box sx={{ bgcolor: '#fff', borderBottom: `1px solid ${tokens.line}`, px: 2 }}>
        <Tabs value={tab} onChange={(_, v) => setTab(v)}>
          <Tab value="work" label="My Work" />
          <Tab value="management" label="Management" />
        </Tabs>
      </Box>
      {tab === 'work' ? <MyWorkTab /> : <ManagementTab />}
    </Box>
  );
}

function nextThing(r: SheetRow): string {
  if (r.pendingReview > 0) return `${r.pendingReview} to review`;
  return r.currentStep ? `${r.currentStep} is open` : '';
}

function MyWorkTab() {
  const nav = useNavigate();
  const { notify, openNewA3 } = useApp();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ['my-work'], queryFn: () => call<MyWork>('my_work') });
  const markRead = useMutation({
    mutationFn: (id: string) => call('warning_set_read', { warning: id, read: true }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['my-work'] });
      void qc.invalidateQueries({ queryKey: ['wall'] });
    },
    onError: (e) => notify({ severity: 'error', message: errorMessage(e) }),
  });

  const emailDraft = async (id: string) => {
    try {
      await call('warning_email_open', { warning: id });
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
    }
  };

  const d = q.data;
  return (
    <Box sx={{ p: 3, display: 'grid', gridTemplateColumns: { xs: 'minmax(0, 1fr)', lg: 'minmax(0, 1fr) minmax(0, 1fr)' }, gap: 2, alignItems: 'start', maxWidth: 1500 }}>
      <Stack spacing={2} sx={{ minWidth: 0 }}>
        <Card>
          <CardContent>
            <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between', mb: 1 }}>
              <Typography variant="h3" component="h2">My open A3s</Typography>
              <Button size="small" onClick={() => nav('/a3?mine=1&quick=open')}>See all</Button>
            </Stack>
            {d?.mySheets.length === 0 && <EmptyState icon="description" title="No open A3s" action={<Button variant="contained" onClick={openNewA3}>New A3</Button>} />}
            {d?.mySheets.map((r, i) => (
              <Box key={r.id}>
                {i > 0 && <Divider />}
                <Stack
                  direction="row"
                  spacing={1.5}
                  useFlexGap
                  tabIndex={0}
                  onClick={() => nav(r.pendingReview > 0 ? `/a3/${r.id}/review` : `/a3/${r.id}?tab=sheet`)}
                  onKeyDown={(e) => e.key === 'Enter' && nav(`/a3/${r.id}`)}
                  sx={{ py: 1, alignItems: 'center', cursor: 'pointer', flexWrap: 'wrap', '&:hover': { bgcolor: tokens.fill } }}
                >
                  <Box sx={{ flex: 1, minWidth: 200 }}>
                    <Typography variant="body1" noWrap>{r.title || 'Untitled A3'}</Typography>
                    <Typography variant="caption" color="text.secondary">{r.number}{r.targetCloseDate ? ` · target ${formatDate(r.targetCloseDate)}` : ''}</Typography>
                  </Box>
                  <HealthFlags row={r} />
                  <Typography variant="body2" color={r.pendingReview > 0 ? 'primary' : 'text.secondary'}>{nextThing(r)}</Typography>
                  <StepStrip strip={r.stepStrip} current={r.currentStep} />
                </Stack>
              </Box>
            ))}
          </CardContent>
        </Card>
        <Card>
          <CardContent>
            <Typography variant="h3" component="h2" sx={{ mb: 1 }}>My follow-up tasks</Typography>
            {d?.myTasks.length === 0 && <Typography variant="body2" color="text.secondary">Nothing due.</Typography>}
            {d?.myTasks.map((t, i) => (
              <Box key={t.id}>
                {i > 0 && <Divider />}
                <Typography variant="caption" color="text.secondary" sx={{ display: 'block', pt: 0.75 }}>{t.warningHeadline}</Typography>
                <TaskRow task={t} />
              </Box>
            ))}
          </CardContent>
        </Card>
      </Stack>
      <Card sx={{ minWidth: 0 }}>
        <CardContent>
          <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between', mb: 1 }}>
            <Typography variant="h3" component="h2">New early warnings{d && d.unreadTotal > 0 ? ` (${d.unreadTotal})` : ''}</Typography>
            <Button size="small" onClick={() => nav('/wall')}>See all</Button>
          </Stack>
          {d?.unreadWarnings.length === 0 && <Typography variant="body2" color="text.secondary">You are up to date.</Typography>}
          {d?.unreadWarnings.map((w, i) => (
            <Box key={w.id}>
              {i > 0 && <Divider />}
              <Box sx={{ py: 1 }}>
                <Stack direction="row" spacing={1} sx={{ alignItems: 'center', mb: 0.25 }}>
                  <Typography variant="body1" sx={{ flex: 1, fontWeight: 500 }}>{w.headline}</Typography>
                  <SeverityChip severity={w.severity} />
                </Stack>
                <Typography variant="body2" color="text.secondary" className="clamp-2">{w.whatHappened}</Typography>
                <Stack direction="row" spacing={0.5} useFlexGap sx={{ mt: 0.5, flexWrap: 'wrap' }}>
                  <Button size="small" onClick={() => markRead.mutate(w.id)}>Mark read</Button>
                  <Button size="small" onClick={() => nav(`/wall/${w.id}`)}>Create follow-up</Button>
                  <Button size="small" onClick={() => void emailDraft(w.id)}>Email draft</Button>
                  <Button size="small" onClick={() => nav(`/a3/${w.sheetId}`)}>Open A3</Button>
                </Stack>
              </Box>
            </Box>
          ))}
        </CardContent>
      </Card>
    </Box>
  );
}

/** A thin single-series bar. Colour says "count here", never which step is at fault (section 6). */
function CountBars({ title, data, onPick }: { title: string; data: LabelCount[]; onPick: (label: string) => void }) {
  const max = Math.max(1, ...data.map((d) => d.count));
  return (
    <Card>
      <CardContent>
        <Typography variant="h4" component="h3" sx={{ mb: 1.5 }}>{title}</Typography>
        {data.length === 0 && <Typography variant="body2" color="text.secondary">None in this period.</Typography>}
        <Stack spacing={0.25}>
          {data.map((d) => (
            <Tooltip key={d.label} title={`${d.label}: ${d.count}. Click to list them.`} placement="right">
              <Box
                component="button"
                type="button"
                onClick={() => onPick(d.label)}
                sx={{ all: 'unset', cursor: 'pointer', display: 'grid', gridTemplateColumns: '112px minmax(0, 1fr) 36px', alignItems: 'center', gap: 1, py: 0.5, borderRadius: 1, '&:hover': { bgcolor: tokens.fill }, '&:focus-visible': { outline: `2px solid ${tokens.primary}` } }}
              >
                <Typography variant="body2" noWrap>{d.label}</Typography>
                <Box sx={{ height: 10, bgcolor: tokens.fill, borderRadius: '2px' }}>
                  <Box sx={{ height: '100%', width: `${(d.count / max) * 100}%`, minWidth: d.count ? 4 : 0, bgcolor: tokens.primary, borderRadius: '0 4px 4px 0' }} />
                </Box>
                <Typography variant="body2" sx={{ textAlign: 'right', fontVariantNumeric: 'tabular-nums' }}>{d.count}</Typography>
              </Box>
            </Tooltip>
          ))}
        </Stack>
      </CardContent>
    </Card>
  );
}

function StatTile({ label, value, sub, onClick }: { label: string; value: string | number; sub?: string; onClick?: () => void }) {
  return (
    <Card>
      <Box component={onClick ? 'button' : 'div'} type={onClick ? 'button' : undefined} onClick={onClick} sx={{ all: 'unset', display: 'block', width: '100%', boxSizing: 'border-box', p: 2, cursor: onClick ? 'pointer' : 'default', '&:hover': onClick ? { bgcolor: tokens.fill } : undefined, '&:focus-visible': { outline: `2px solid ${tokens.primary}` } }}>
        <Typography variant="body2" color="text.secondary">{label}</Typography>
        <Typography sx={{ fontSize: 28, lineHeight: 1.2, fontVariantNumeric: 'tabular-nums' }}>{value}</Typography>
        {sub && <Typography variant="caption" color="text.secondary">{sub}</Typography>}
      </Box>
    </Card>
  );
}

function SheetList({ title, rows, flagLabel }: { title: string; rows: SheetRow[]; flagLabel: string }) {
  const nav = useNavigate();
  return (
    <Card sx={{ minWidth: 0 }}>
      <CardContent>
        <Typography variant="h4" component="h3" sx={{ mb: 1 }}>{title} ({rows.length})</Typography>
        {rows.length === 0 && <Typography variant="body2" color="text.secondary">None {flagLabel}.</Typography>}
        {rows.slice(0, 8).map((r) => (
          <Stack key={r.id} direction="row" spacing={1} tabIndex={0} onClick={() => nav(`/a3/${r.id}`)} onKeyDown={(e) => e.key === 'Enter' && nav(`/a3/${r.id}`)} sx={{ py: 0.5, alignItems: 'center', cursor: 'pointer', '&:hover': { bgcolor: tokens.fill } }}>
            <Typography variant="body2" sx={{ width: 110, flex: 'none' }}>{r.number}</Typography>
            <Typography variant="body2" noWrap sx={{ flex: 1 }}>{r.title || 'Untitled A3'}</Typography>
            <Typography variant="caption" color="text.secondary" noWrap sx={{ width: 100, textAlign: 'right' }}>{r.ownerName}</Typography>
          </Stack>
        ))}
      </CardContent>
    </Card>
  );
}

function periodStart(p: string): string | null {
  if (p === 'all') return null;
  const d = new Date();
  d.setDate(d.getDate() - Number(p));
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

function ManagementTab() {
  const nav = useNavigate();
  const [projectId, setProjectId] = useState('');
  const [area, setArea] = useState('');
  const [period, setPeriod] = useState('90');
  const from = periodStart(period);
  const filter: MgmtFilter = { projectId: projectId || null, area: area || null, from, to: null };
  const q = useQuery({ queryKey: ['management', filter], queryFn: () => call<Management>('management', { filter }) });
  const projects = useQuery({ queryKey: ['projects', 'all'], queryFn: () => call<Project[]>('projects_list', { onlyMine: false }) });
  const picks = useQuery({ queryKey: ['picks', projectId || 'company'], queryFn: () => call<PickValue[]>('pick_values', { project: projectId || null }), enabled: !!projectId });
  const m = q.data;

  // Every number clicks through to the matching A3 list.
  const list = (extra: Record<string, string | null>) => {
    const p = new URLSearchParams();
    if (projectId) p.set('project', projectId);
    if (area) p.set('area', area);
    for (const [k, v] of Object.entries(extra)) if (v) p.set(k, v);
    nav(`/a3?${p.toString()}`);
  };

  return (
    <Box sx={{ p: 3, maxWidth: 1500 }}>
      <Stack direction="row" spacing={1.5} useFlexGap sx={{ mb: 2, flexWrap: 'wrap' }} role="group" aria-label="Filters">
        <TextField select label="Project" value={projects.data ? projectId : ''} onChange={(e) => { setProjectId(e.target.value); setArea(''); }} sx={{ minWidth: 200 }} slotProps={{ select: { displayEmpty: true }, inputLabel: { shrink: true } }}>
          <MenuItem value="">All projects</MenuItem>
          {(projects.data ?? []).map((p) => <MenuItem key={p.id} value={p.id}>{p.name}</MenuItem>)}
        </TextField>
        {projectId ? (
          <TextField select label="Area" value={area} onChange={(e) => setArea(e.target.value)} sx={{ minWidth: 180 }}>
            <MenuItem value="">All areas</MenuItem>
            {(picks.data ?? []).filter((p) => p.listName === 'Area').map((p) => <MenuItem key={p.id} value={p.value}>{p.value}</MenuItem>)}
          </TextField>
        ) : (
          <TextField label="Area" value={area} onChange={(e) => setArea(e.target.value)} sx={{ minWidth: 180 }} />
        )}
        <TextField select label="Period" value={period} onChange={(e) => setPeriod(e.target.value)} sx={{ minWidth: 150 }}>
          <MenuItem value="30">Last 30 days</MenuItem>
          <MenuItem value="90">Last 90 days</MenuItem>
          <MenuItem value="365">Last 12 months</MenuItem>
          <MenuItem value="all">All time</MenuItem>
        </TextField>
      </Stack>
      {m && (
        <Stack spacing={2}>
          <Box sx={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(190px, 1fr))', gap: 2 }}>
            <StatTile label="Overdue" value={m.overdue.length} onClick={() => list({ quick: 'overdue' })} />
            <StatTile label="Stalled" value={m.stalled.length} onClick={() => list({ quick: 'stalled' })} />
            <StatTile label="Closed in period" value={m.closedCount} sub={m.averageDaysToClose != null ? `Average ${Math.round(m.averageDaysToClose)} days to close` : undefined} onClick={() => list({ quick: 'closed', closedFrom: from })} />
            <StatTile label="Reopened in period" value={m.reopenedCount} onClick={() => list({ reopenedFrom: from ?? '2000-01-01' })} />
          </Box>
          <Box sx={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(320px, 1fr))', gap: 2 }}>
            <CountBars title="Open A3s by current step" data={m.byStep} onPick={(l) => list({ quick: 'open', step: l })} />
            <CountBars title="Age of open A3s, in days" data={m.byAge} onPick={(l) => list({ quick: 'open', age: l })} />
            <CountBars title="Reopened, by reason" data={m.reopenedByReason} onPick={(l) => list({ reason: l, reopenedFrom: from ?? '2000-01-01' })} />
          </Box>
          <Box sx={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(360px, 1fr))', gap: 2 }}>
            <SheetList title="Overdue, oldest first" rows={m.overdue} flagLabel="overdue" />
            <SheetList title="Stalled, oldest first" rows={m.stalled} flagLabel="stalled" />
          </Box>
          <Card sx={{ overflowX: 'auto' }}>
            <CardContent>
              <Typography variant="h4" component="h3" sx={{ mb: 1 }}>Warning reach</Typography>
              {m.warningReach.length === 0 ? (
                <Typography variant="body2" color="text.secondary">No active warnings.</Typography>
              ) : (
                <Table size="small">
                  <TableHead>
                    <TableRow>
                      <TableCell>Warning</TableCell>
                      <TableCell sx={{ width: 220 }}>Marked read</TableCell>
                      <TableCell align="right">Checks open</TableCell>
                      <TableCell align="right">Checks done</TableCell>
                      <TableCell align="right">Affected</TableCell>
                    </TableRow>
                  </TableHead>
                  <TableBody>
                    {m.warningReach.map((w) => (
                      <TableRow key={w.id} hover sx={{ cursor: 'pointer' }} tabIndex={0} onClick={() => nav(`/wall/${w.id}`)} onKeyDown={(e) => e.key === 'Enter' && nav(`/wall/${w.id}`)}>
                        <TableCell>{w.headline}</TableCell>
                        <TableCell>
                          <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }}>
                            <Box sx={{ flex: 1, height: 8, bgcolor: tokens.fill, borderRadius: '2px' }}>
                              <Box sx={{ height: '100%', width: `${w.readPercent}%`, minWidth: w.readPercent ? 4 : 0, bgcolor: tokens.primary, borderRadius: '0 4px 4px 0' }} />
                            </Box>
                            <Typography variant="body2" sx={{ width: 40, textAlign: 'right', fontVariantNumeric: 'tabular-nums' }}>{w.readPercent}%</Typography>
                          </Stack>
                        </TableCell>
                        <TableCell align="right">{w.checksOpen}</TableCell>
                        <TableCell align="right">{w.checksDone}</TableCell>
                        <TableCell align="right">
                          {w.affected > 0 ? <Chip size="small" label={w.affected} sx={{ height: 20, bgcolor: tokens.errorLight, color: tokens.errorText }} /> : 0}
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              )}
            </CardContent>
          </Card>
        </Stack>
      )}
      {q.isError && <Typography color="error">{errorMessage(q.error)}</Typography>}
    </Box>
  );
}
