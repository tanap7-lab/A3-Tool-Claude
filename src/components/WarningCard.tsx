import { useState } from 'react';
import { Autocomplete, Box, Button, Card, CardContent, Chip, CircularProgress, Dialog, DialogActions, DialogContent, DialogTitle, Divider, MenuItem, Stack, TextField, Tooltip, Typography } from '@mui/material';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from 'react-router-dom';
import { call, errorCode, errorMessage, type AppUser, type EmailDraft, type Project, type Task, type Warning, type WarningUpdate } from '../api';
import { useApp } from '../context';
import { addDays, formatDate, formatDateTime } from '../lib/text';
import { tokens } from '../theme';
import { ChipPicker, Icon, SeverityChip } from './common';

export const TASK_RESULTS = ['Not affected', 'Affected', 'Not applicable'] as const;

export function causeColors(status: string): [string, string] {
  if (status === 'Solution verified') return [tokens.successLight, tokens.successText];
  if (status === 'Root cause confirmed') return [tokens.primaryLight, tokens.primaryText];
  return [tokens.warningLight, tokens.warningText];
}

function useRefreshWall(sheetId: string) {
  const qc = useQueryClient();
  return () => {
    void qc.invalidateQueries({ queryKey: ['wall'] });
    void qc.invalidateQueries({ queryKey: ['warning'] });
    void qc.invalidateQueries({ queryKey: ['sheet-warnings', sheetId] });
    void qc.invalidateQueries({ queryKey: ['my-work'] });
    void qc.invalidateQueries({ queryKey: ['tasks'] });
  };
}

export function rollUp(w: Warning): string {
  if (w.checksTotal === 0) return 'No checks yet';
  const parts = [
    w.notAffected ? `${w.notAffected} Not affected` : '',
    w.affected ? `${w.affected} Affected` : '',
    w.notApplicable ? `${w.notApplicable} Not applicable` : '',
    w.checksOpen ? `${w.checksOpen} open` : '',
  ].filter(Boolean);
  return `${w.checksTotal} ${w.checksTotal === 1 ? 'check' : 'checks'}: ${parts.join(', ')}`;
}

/** The card as a reader sees it, with reader actions (FR-EW-02) and author actions. */
export function WarningCard({ warning: w, showTasks, compact }: { warning: Warning; showTasks?: boolean; compact?: boolean }) {
  const { user, notify } = useApp();
  const nav = useNavigate();
  const refresh = useRefreshWall(w.sheetId);
  const [followUp, setFollowUp] = useState(false);
  const [updating, setUpdating] = useState(false);
  const [withdrawing, setWithdrawing] = useState(false);
  const [text, setText] = useState('');
  const [expanded, setExpanded] = useState(false);
  const isAuthor = w.authorId === user.id;
  const withdrawn = !!w.withdrawnAt;
  const [causeBg, causeFg] = causeColors(w.causeStatus);

  const updates = useQuery({ queryKey: ['warning', w.id, 'updates'], queryFn: () => call<WarningUpdate[]>('warning_updates', { warning: w.id }), enabled: expanded || w.updated });

  const run = async (fn: () => Promise<unknown>, success?: string) => {
    try {
      await fn();
      refresh();
      if (success) notify({ severity: 'success', message: success });
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
    }
  };

  const copyEmail = async () => {
    try {
      const d = await call<EmailDraft>('warning_email_draft', { warning: w.id });
      const full = `${d.subject}\n\n${d.text}`;
      const htmlFull = `<p><b>${d.subject.replace(/&/g, '&amp;').replace(/</g, '&lt;')}</b></p>${d.html}`;
      try {
        await navigator.clipboard.write([
          new ClipboardItem({ 'text/plain': new Blob([full], { type: 'text/plain' }), 'text/html': new Blob([htmlFull], { type: 'text/html' }) }),
        ]);
      } catch {
        await navigator.clipboard.writeText(full);
      }
      notify({ severity: 'success', message: 'Email copied. Paste it into your mail app.' });
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
    }
  };

  return (
    <Card sx={{ borderLeft: `3px solid ${withdrawn ? tokens.line : tokens.warning}`, opacity: withdrawn ? 0.75 : 1 }}>
      <CardContent sx={{ pb: '12px !important' }}>
        <Stack direction="row" spacing={1} useFlexGap sx={{ alignItems: 'center', mb: 0.5, flexWrap: 'wrap' }}>
          {w.unread && !withdrawn && <Box aria-label="Unread" role="img" sx={{ width: 8, height: 8, borderRadius: '50%', bgcolor: tokens.primary, flex: 'none' }} />}
          <Typography variant="h3" sx={{ flex: 1, minWidth: 200, fontWeight: w.unread ? 500 : 400 }} component="h2">{w.headline}</Typography>
          {w.updated && <Chip size="small" label="Updated" sx={{ height: 20, bgcolor: tokens.primaryLight, color: tokens.primaryText }} />}
          {withdrawn && <Chip size="small" label="Withdrawn" sx={{ height: 20 }} />}
          {w.archived && <Chip size="small" label="Archived" sx={{ height: 20 }} />}
          <SeverityChip severity={w.severity} />
          <Chip size="small" label={w.causeStatus} sx={{ height: 20, bgcolor: causeBg, color: causeFg }} />
        </Stack>
        <Typography variant="body1" sx={{ mb: 1 }}>{w.whatHappened}</Typography>
        {withdrawn && <Typography variant="body2" sx={{ mb: 1, color: tokens.textSecondary }}>Withdrawn: {w.withdrawnReason}</Typography>}
        {w.quickCheck && (
          <Box sx={{ mb: 1, p: 1, bgcolor: tokens.fill, borderRadius: 1 }}>
            <Typography variant="caption" color="text.secondary">Quick check</Typography>
            <Typography variant="body2" sx={{ whiteSpace: 'pre-line' }}>{w.quickCheck}</Typography>
          </Box>
        )}
        {w.whoShouldCheck.length > 0 && (
          <Stack direction="row" spacing={0.5} useFlexGap sx={{ flexWrap: 'wrap', mb: 1, alignItems: 'center' }}>
            <Typography variant="caption" color="text.secondary">Who should check:</Typography>
            {w.whoShouldCheck.map((c) => <Chip key={c} size="small" label={c} variant="outlined" sx={{ height: 20, fontSize: 12 }} />)}
          </Stack>
        )}
        {(updates.data ?? []).length > 0 && (
          <Stack spacing={0.5} sx={{ mb: 1 }}>
            {updates.data!.map((u) => (
              <Typography key={u.id} variant="body2">
                <Box component="span" sx={{ color: tokens.textSecondary }}>{formatDateTime(u.createdAt)}, {u.authorName}: </Box>{u.text}
              </Typography>
            ))}
          </Stack>
        )}
        <Stack direction="row" spacing={2} useFlexGap sx={{ flexWrap: 'wrap', alignItems: 'center', mb: 1 }}>
          <Typography variant="caption" color="text.secondary">{w.sheetNumber} · {w.authorName} · {formatDateTime(w.postedAt)}</Typography>
          <Typography variant="caption" color="text.secondary">{rollUp(w)}</Typography>
        </Stack>
        <Divider sx={{ mb: 1 }} />
        <Stack direction="row" spacing={0.5} useFlexGap sx={{ flexWrap: 'wrap', alignItems: 'center' }}>
          <Button size="small" variant={w.unread ? 'contained' : 'outlined'} onClick={() => void run(() => call('warning_set_read', { warning: w.id, read: w.unread }))}>
            {w.unread ? 'Mark read' : 'Mark unread'}
          </Button>
          {!withdrawn && <Button size="small" startIcon={<Icon name="task_alt" size={16} />} onClick={() => setFollowUp(true)}>Create follow-up</Button>}
          <Button size="small" startIcon={<Icon name="mail" size={16} />} onClick={() => void run(() => call('warning_email_open', { warning: w.id }))}>Open in mail app</Button>
          <Button size="small" startIcon={<Icon name="content_copy" size={16} />} onClick={() => void copyEmail()}>Copy email</Button>
          <Button size="small" startIcon={<Icon name="description" size={16} />} onClick={() => nav(`/a3/${w.sheetId}`)}>Open A3</Button>
          {!compact && <Button size="small" startIcon={<Icon name="link" size={16} />} onClick={() => void run(async () => {
            await navigator.clipboard.writeText(await call<string>('link_make', { kind: 'warning', id: w.id }));
          }, 'Link copied.')}>Copy link</Button>}
          {w.updated || expanded ? null : <Button size="small" onClick={() => setExpanded(true)}>Updates</Button>}
          {isAuthor && !withdrawn && (
            <>
              <Box sx={{ flex: 1 }} />
              <Button size="small" onClick={() => { setText(''); setUpdating(true); }}>Add update</Button>
              <Button size="small" color="inherit" sx={{ color: tokens.textSecondary }} onClick={() => { setText(''); setWithdrawing(true); }}>Withdraw</Button>
            </>
          )}
        </Stack>
        {showTasks && <TaskList warningId={w.id} sheetId={w.sheetId} />}
      </CardContent>

      <FollowUpDialog open={followUp} onClose={() => setFollowUp(false)} warning={w} onCreated={refresh} />
      <Dialog open={updating} onClose={() => setUpdating(false)} fullWidth maxWidth="sm">
        <DialogTitle>Add an update</DialogTitle>
        <DialogContent>
          <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>Readers see this card as Unread again, tagged Updated.</Typography>
          <TextField fullWidth multiline minRows={2} autoFocus placeholder="For example: Root cause found" value={text} onChange={(e) => setText(e.target.value)} />
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setUpdating(false)}>Cancel</Button>
          <Button variant="contained" disabled={!text.trim()} onClick={() => { setUpdating(false); setExpanded(true); void run(() => call('warning_add_update', { warning: w.id, text })); }}>Add update</Button>
        </DialogActions>
      </Dialog>
      <Dialog open={withdrawing} onClose={() => setWithdrawing(false)} fullWidth maxWidth="xs">
        <DialogTitle>Withdraw this warning?</DialogTitle>
        <DialogContent>
          <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>It stays visible, labelled Withdrawn.</Typography>
          <TextField fullWidth autoFocus label="Reason" value={text} onChange={(e) => setText(e.target.value)} />
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setWithdrawing(false)}>Cancel</Button>
          <Button variant="contained" color="error" disabled={!text.trim()} onClick={() => { setWithdrawing(false); void run(() => call('warning_withdraw', { warning: w.id, reason: text })); }}>Withdraw</Button>
        </DialogActions>
      </Dialog>
    </Card>
  );
}

function FollowUpDialog({ open, onClose, warning, onCreated }: { open: boolean; onClose: () => void; warning: Warning; onCreated: () => void }) {
  const { user, notify } = useApp();
  const [description, setDescription] = useState('');
  const [assignee, setAssignee] = useState<string>(user.id);
  const [area, setArea] = useState('');
  const [dueChip, setDueChip] = useState<'Today' | '+3 days' | '+7 days'>('+3 days');
  const [added, setAdded] = useState<string[]>([]);
  const users = useQuery({ queryKey: ['users'], queryFn: () => call<AppUser[]>('users_list'), enabled: open });
  const suggestions = useQuery({
    queryKey: ['task-suggestions', warning.id],
    queryFn: () => call<string[]>('ai_suggest_tasks', { warning: warning.id }),
    enabled: open && !!warning.quickCheck,
    retry: false,
    refetchInterval: false,
    staleTime: Infinity,
  });
  const dueOn = dueChip === 'Today' ? addDays(0) : dueChip === '+3 days' ? addDays(3) : addDays(7);

  const create = useMutation({
    mutationFn: (text: string) => call('task_create', { new: { warningId: warning.id, description: text, assigneeId: assignee, targetArea: area, dueOn } }),
    onSuccess: (_, text) => {
      setAdded((a) => [...a, text]);
      setDescription('');
      onCreated();
    },
    onError: (e) => notify({ severity: 'error', message: errorMessage(e) }),
  });
  const options = (users.data ?? []).filter((u) => u.active);
  const aiNotSet = suggestions.isError && errorCode(suggestions.error) === 'AI_NOT_SET';

  return (
    <Dialog open={open} onClose={onClose} fullWidth maxWidth="sm">
      <DialogTitle>Create follow-up</DialogTitle>
      <DialogContent>
        <Stack spacing={2} sx={{ pt: 0.5 }}>
          <Typography variant="body2" color="text.secondary">{warning.headline}</Typography>
          {suggestions.isFetching && <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }} role="status"><CircularProgress size={14} /><Typography variant="caption" color="text.secondary">Suggesting tasks...</Typography></Stack>}
          {suggestions.data && suggestions.data.length > 0 && (
            <Stack spacing={0.5}>
              <Typography variant="caption" color="text.secondary">Suggested from the quick check. One click adds it.</Typography>
              {suggestions.data.map((s) => {
                const done = added.includes(s);
                return (
                  <Stack key={s} direction="row" spacing={1} sx={{ alignItems: 'center', border: `1px solid ${tokens.line}`, borderRadius: 1, px: 1, py: 0.5 }}>
                    <Typography variant="body2" sx={{ flex: 1 }}>{s}</Typography>
                    <Button size="small" disabled={done} onClick={() => create.mutate(s)}>{done ? 'Added' : 'Add'}</Button>
                  </Stack>
                );
              })}
            </Stack>
          )}
          {suggestions.isError && !aiNotSet && <Typography variant="caption" color="text.secondary">{errorMessage(suggestions.error)}</Typography>}
          <TextField fullWidth label="Or describe the task" value={description} onChange={(e) => setDescription(e.target.value)} />
          <Autocomplete
            size="small"
            options={options.map((u) => u.id)}
            value={assignee}
            disableClearable
            getOptionLabel={(id) => options.find((u) => u.id === id)?.displayName ?? (id === user.id ? user.displayName : '')}
            onChange={(_, v) => setAssignee(v)}
            renderInput={(p) => <TextField {...p} label="Assignee" />}
          />
          <TextField fullWidth label="Project or line" value={area} onChange={(e) => setArea(e.target.value)} />
          <Box>
            <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mb: 0.5 }}>Due</Typography>
            <ChipPicker ariaLabel="Due" options={['Today', '+3 days', '+7 days'] as const} value={dueChip} onChange={setDueChip} />
          </Box>
          {added.length > 0 && <Typography variant="body2" sx={{ color: tokens.successText }}>{added.length} {added.length === 1 ? 'task' : 'tasks'} added.</Typography>}
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Done</Button>
        <Button variant="contained" disabled={!description.trim() || create.isPending} onClick={() => create.mutate(description.trim())}>Add task</Button>
      </DialogActions>
    </Dialog>
  );
}

/** FR-TSK-03 and 04: record one result. Affected offers a linked A3. */
export function TaskRow({ task, sheetId }: { task: Task; sheetId?: string }) {
  const { user, notify } = useApp();
  const qc = useQueryClient();
  const nav = useNavigate();
  const [linking, setLinking] = useState(false);
  const [projectId, setProjectId] = useState('');
  const projects = useQuery({ queryKey: ['projects', 'mine'], queryFn: () => call<Project[]>('projects_list', { onlyMine: true }), enabled: linking });
  const options = (projects.data ?? []).filter((p) => !p.archivedAt);
  const mine = task.assigneeId === user.id;
  const refresh = () => {
    void qc.invalidateQueries({ queryKey: ['my-work'] });
    void qc.invalidateQueries({ queryKey: ['tasks'] });
    void qc.invalidateQueries({ queryKey: ['wall'] });
    void qc.invalidateQueries({ queryKey: ['warning'] });
    if (sheetId) void qc.invalidateQueries({ queryKey: ['sheet-warnings', sheetId] });
  };
  const record = async (result: string) => {
    try {
      await call('task_close', { task: task.id, result });
      refresh();
      if (result === 'Affected') {
        setLinking(true);
      }
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
    }
  };
  const createLinked = async () => {
    const project = projectId || options.find((p) => p.id === user.lastProjectId)?.id || options[0]?.id;
    if (!project) {
      notify({ severity: 'info', message: 'An A3 belongs to a project. Create one in Settings first.' });
      return;
    }
    try {
      const id = await call<string>('task_create_linked_a3', { task: task.id, project });
      setLinking(false);
      refresh();
      nav(`/a3/${id}`);
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
    }
  };
  return (
    <Box sx={{ py: 0.75 }}>
      <Stack direction="row" spacing={1} useFlexGap sx={{ alignItems: 'center', flexWrap: 'wrap' }}>
        <Box sx={{ flex: 1, minWidth: 220 }}>
          <Typography variant="body2">{task.description}</Typography>
          <Typography variant="caption" color="text.secondary">
            {task.assigneeName}{task.targetArea ? ` · ${task.targetArea}` : ''}{task.dueOn ? ` · due ${formatDate(task.dueOn)}` : ''}
          </Typography>
        </Box>
        {task.result ? (
          <Stack direction="row" spacing={0.5} sx={{ alignItems: 'center' }}>
            <Chip
              size="small"
              label={task.result}
              sx={{
                height: 22,
                bgcolor: task.result === 'Affected' ? tokens.errorLight : task.result === 'Not affected' ? tokens.successLight : '#F1F3F4',
                color: task.result === 'Affected' ? tokens.errorText : task.result === 'Not affected' ? tokens.successText : tokens.textSecondary,
              }}
            />
            {task.result === 'Affected' && !task.linkedA3Id && mine && <Button size="small" onClick={() => setLinking(true)}>Create linked A3</Button>}
            {task.linkedA3Id && <Button size="small" onClick={() => nav(`/a3/${task.linkedA3Id}`)}>{task.linkedA3Number}</Button>}
          </Stack>
        ) : mine ? (
          <Stack direction="row" spacing={0.5}>
            {TASK_RESULTS.map((r) => <Chip key={r} size="small" label={r} variant="outlined" clickable onClick={() => void record(r)} />)}
          </Stack>
        ) : (
          <Tooltip title="Only the assignee records the result"><Chip size="small" label="Open" variant="outlined" sx={{ height: 22 }} /></Tooltip>
        )}
      </Stack>
      <Dialog open={linking} onClose={() => setLinking(false)} maxWidth="xs" fullWidth>
        <DialogTitle>Create linked A3</DialogTitle>
        <DialogContent>
          <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>Pre-filled with this warning and linked in both directions.</Typography>
          <TextField select fullWidth label="Project" value={projectId || options.find((p) => p.id === user.lastProjectId)?.id || options[0]?.id || ''} onChange={(e) => setProjectId(e.target.value)}>
            {options.map((p) => <MenuItem key={p.id} value={p.id}>{p.name}</MenuItem>)}
          </TextField>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setLinking(false)}>Not now</Button>
          <Button variant="contained" onClick={() => void createLinked()}>Create linked A3</Button>
        </DialogActions>
      </Dialog>
    </Box>
  );
}

function TaskList({ warningId, sheetId }: { warningId: string; sheetId: string }) {
  const q = useQuery({ queryKey: ['tasks', warningId], queryFn: () => call<Task[]>('tasks_for_warning', { warning: warningId }) });
  if (!q.data?.length) return null;
  return (
    <Box sx={{ mt: 1 }}>
      <Divider sx={{ mb: 0.5 }} />
      {q.data.map((t) => <TaskRow key={t.id} task={t} sheetId={sheetId} />)}
    </Box>
  );
}
