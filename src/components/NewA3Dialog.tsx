import { useEffect, useMemo, useState } from 'react';
import { Alert, Box, Button, Dialog, DialogActions, DialogContent, DialogTitle, MenuItem, Stack, TextField, Typography } from '@mui/material';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { useNavigate } from 'react-router-dom';
import { call, errorMessage, type AddResult, type Project } from '../api';
import { useApp } from '../context';
import { tokens } from '../theme';
import { Icon } from './common';

/**
 * The fast path (section 3): click New A3, drop in the files, and the review screen takes over.
 * The project defaults to the last one used.
 */
export function NewA3Dialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { user, notify } = useApp();
  const nav = useNavigate();
  const qc = useQueryClient();
  const projects = useQuery({ queryKey: ['projects', 'mine'], queryFn: () => call<Project[]>('projects_list', { onlyMine: true }), enabled: open });
  const active = useMemo(() => (projects.data ?? []).filter((p) => !p.archivedAt), [projects.data]);
  const [projectId, setProjectId] = useState('');
  const [files, setFiles] = useState<string[]>([]);
  const [dragging, setDragging] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) {
      setFiles([]);
      setError(null);
      return;
    }
    if (active.length && !active.some((p) => p.id === projectId)) {
      const preferred = active.find((p) => p.id === user.lastProjectId) ?? active.find((p) => p.id === user.defaultProjectId) ?? active[0];
      setProjectId(preferred.id);
    }
  }, [open, active, projectId, user.lastProjectId, user.defaultProjectId]);

  useEffect(() => {
    if (!open) return;
    const un = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === 'enter' || e.payload.type === 'over') setDragging(true);
      else if (e.payload.type === 'leave') setDragging(false);
      else if (e.payload.type === 'drop') {
        setDragging(false);
        const dropped = e.payload.paths;
        setFiles((cur) => [...new Set([...cur, ...dropped])]);
      }
    });
    return () => {
      void un.then((f) => f());
    };
  }, [open]);

  const pick = async () => {
    const res = await openDialog({ multiple: true });
    if (Array.isArray(res)) setFiles((cur) => [...new Set([...cur, ...res])]);
    else if (typeof res === 'string') setFiles((cur) => [...new Set([...cur, res])]);
  };

  const create = async () => {
    if (!projectId) return;
    setBusy(true);
    setError(null);
    try {
      const id = await call<string>('sheet_create', { new: { title: '', projectId } });
      if (files.length) {
        const r = await call<AddResult>('attachments_add', { sheet: id, paths: files });
        if (r.errors.length) notify({ severity: 'warning', message: r.errors.join('; ') });
      }
      void qc.invalidateQueries({ queryKey: ['sheets'] });
      onClose();
      nav(files.length ? `/a3/${id}/review` : `/a3/${id}`);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const noProjects = projects.isSuccess && active.length === 0;

  return (
    <Dialog open={open} onClose={onClose} fullWidth maxWidth="sm">
      <DialogTitle>New A3</DialogTitle>
      <DialogContent>
        <Stack spacing={2} sx={{ pt: 0.5 }}>
          {error && <Alert severity="error">{error}</Alert>}
          {noProjects ? (
            <Alert
              severity="info"
              action={<Button size="small" onClick={() => { onClose(); nav('/settings'); }}>Open Settings</Button>}
            >
              An A3 belongs to a project. Create one in Settings first.
            </Alert>
          ) : (
            <TextField select label="Project" value={projectId} onChange={(e) => setProjectId(e.target.value)} fullWidth>
              {active.map((p) => <MenuItem key={p.id} value={p.id}>{p.name}</MenuItem>)}
            </TextField>
          )}
          <Box
            role="button"
            tabIndex={0}
            onClick={pick}
            onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') void pick(); }}
            sx={{
              border: `1.5px dashed ${dragging ? tokens.primary : tokens.line}`,
              bgcolor: dragging ? tokens.primaryLight : tokens.fill,
              borderRadius: 2,
              py: 4,
              px: 2,
              textAlign: 'center',
              cursor: 'pointer',
            }}
          >
            <Icon name="upload_file" size={32} color={tokens.primary} />
            <Typography variant="body1" sx={{ mt: 1 }}>Drop your reports here, or click to choose</Typography>
            <Typography variant="caption" color="text.secondary">PDF, Word, Excel, PowerPoint, text, email. Up to 20 files, 50 MB each.</Typography>
          </Box>
          {files.length > 0 && (
            <Stack spacing={0.5}>
              {files.map((f) => (
                <Stack key={f} direction="row" spacing={1} sx={{ alignItems: 'center' }}>
                  <Icon name="draft" size={18} color={tokens.textSecondary} />
                  <Typography variant="body2" noWrap sx={{ flex: 1 }}>{f.split(/[\\/]/).pop()}</Typography>
                  <Button size="small" onClick={() => setFiles(files.filter((x) => x !== f))}>Remove</Button>
                </Stack>
              ))}
            </Stack>
          )}
        </Stack>
      </DialogContent>
      <DialogActions sx={{ px: 3, pb: 2 }}>
        <Button onClick={onClose}>Cancel</Button>
        <Button variant="contained" onClick={create} disabled={busy || !projectId}>
          {busy ? 'Creating...' : files.length ? 'Create and review' : 'Create blank A3'}
        </Button>
      </DialogActions>
    </Dialog>
  );
}
