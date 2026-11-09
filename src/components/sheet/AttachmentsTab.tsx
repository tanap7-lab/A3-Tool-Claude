import { useEffect, useState, type ClipboardEvent } from 'react';
import { Alert, Box, Button, Card, Checkbox, Chip, CircularProgress, Dialog, DialogActions, DialogContent, DialogTitle, FormControlLabel, IconButton, Stack, Table, TableBody, TableCell, TableHead, TableRow, Tooltip, Typography } from '@mui/material';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog';
import { call, errorMessage, type AddResult, type Attachment, type Segment, type Sheet } from '../../api';
import { useApp } from '../../context';
import { formatBytes, formatDate } from '../../lib/text';
import { tokens } from '../../theme';
import { EmptyState, Icon } from '../common';

function ReadState({ a }: { a: Attachment }) {
  const map: Record<Attachment['readState'], [string, string, string]> = {
    pending: ['Waiting to read', tokens.primaryLight, tokens.primaryText],
    reading: ['Reading', tokens.primaryLight, tokens.primaryText],
    read: ['Read', tokens.successLight, tokens.successText],
    unreadable: ['Unreadable', tokens.errorLight, tokens.errorText],
    stored_only: ['Stored only', '#F1F3F4', tokens.textSecondary],
    needs_modern_format: ['Save as a modern format', tokens.warningLight, tokens.warningText],
  };
  const [label, bg, fg] = map[a.readState];
  const title =
    a.readState === 'needs_modern_format'
      ? 'This older Office format is stored but not read. Save it as .docx or .pptx and add it again so its content can be used.'
      : a.readError ?? '';
  return (
    <Tooltip title={title}>
      <Chip size="small" label={label} sx={{ height: 22, bgcolor: bg, color: fg }} />
    </Tooltip>
  );
}

function AiState({ a }: { a: Attachment }) {
  if (a.aiState === 'pending' || a.aiState === 'running') {
    return <Stack direction="row" spacing={0.5} sx={{ alignItems: 'center' }}><CircularProgress size={12} /><Typography variant="caption" color="text.secondary">AI is reading</Typography></Stack>;
  }
  if (a.aiState === 'failed') return <Tooltip title={a.aiError ?? ''}><Typography variant="caption" sx={{ color: tokens.error }}>AI failed</Typography></Tooltip>;
  if (a.aiState === 'done') {
    return (
      <Typography variant="caption" color="text.secondary">
        {a.confirmsCount > 0 ? `Confirms ${a.confirmsCount} points` : 'Reviewed by AI'}
        {a.aiNote ? `, ${a.aiNote}` : ''}
      </Typography>
    );
  }
  return null;
}

export function AttachmentsTab({ sheet, onPreview }: { sheet: Sheet; onPreview: (segmentId: string) => void }) {
  const { notify } = useApp();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ['attachments', sheet.id], queryFn: () => call<Attachment[]>('attachments_list', { sheet: sheet.id }) });
  const [dragging, setDragging] = useState(false);
  const [busy, setBusy] = useState(false);
  const [sheetsFor, setSheetsFor] = useState<Attachment | null>(null);
  const canEdit = sheet.canEdit && !sheet.withdrawnAt;
  const refresh = () => void qc.invalidateQueries({ queryKey: ['attachments', sheet.id] });

  const add = async (paths: string[]) => {
    if (!paths.length) return;
    setBusy(true);
    try {
      const r = await call<AddResult>('attachments_add', { sheet: sheet.id, paths });
      refresh();
      void qc.invalidateQueries({ queryKey: ['sheet', sheet.id] });
      if (r.errors.length) notify({ severity: 'warning', message: r.errors.join('; ') });
      else if (r.duplicates.length) notify({ severity: 'info', message: `Already on this A3, so not stored twice: ${r.duplicates.join(', ')}` });
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    if (!canEdit) return;
    const un = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === 'enter' || e.payload.type === 'over') setDragging(true);
      else if (e.payload.type === 'leave') setDragging(false);
      else if (e.payload.type === 'drop') {
        setDragging(false);
        void add(e.payload.paths);
      }
    });
    return () => {
      void un.then((f) => f());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [canEdit, sheet.id]);

  const pick = async () => {
    const res = await openDialog({ multiple: true });
    if (Array.isArray(res)) await add(res);
    else if (typeof res === 'string') await add([res]);
  };

  // FR-ATT-01: add by paste as well.
  const onPaste = async (e: ClipboardEvent) => {
    if (!canEdit) return;
    const files = Array.from(e.clipboardData.files);
    for (const f of files) {
      const buf = new Uint8Array(await f.arrayBuffer());
      let bin = '';
      for (let i = 0; i < buf.length; i += 0x8000) bin += String.fromCharCode(...buf.subarray(i, i + 0x8000));
      const name = f.name || `pasted-${Date.now()}.png`;
      try {
        await call('attachment_add_bytes', { sheet: sheet.id, name, base64: btoa(bin) });
        refresh();
      } catch (err) {
        notify({ severity: 'error', message: errorMessage(err) });
      }
    }
  };

  const act = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
      refresh();
      void qc.invalidateQueries({ queryKey: ['sheet', sheet.id] });
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
    }
  };

  const preview = async (a: Attachment) => {
    try {
      const segs = await call<Segment[]>('segments_list', { attachment: a.id });
      if (segs.length) onPreview(segs[0].id);
      else void call('attachment_open', { id: a.id });
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
    }
  };

  const saveAs = async (a: Attachment) => {
    const dest = await saveDialog({ defaultPath: a.fileName });
    if (dest) await act(() => call('attachment_save', { id: a.id, dest }));
  };

  const list = q.data ?? [];

  return (
    <Box sx={{ p: 3 }} onPaste={onPaste} tabIndex={-1}>
      <Stack direction="row" spacing={2} sx={{ mb: 2, alignItems: 'center' }}>
        {canEdit && <Button variant="contained" startIcon={<Icon name="add" />} disabled={busy} onClick={pick}>Add files</Button>}
        {busy && <CircularProgress size={18} />}
        {canEdit && <Typography variant="body2" color="text.secondary">Or drop files anywhere here, or paste them. Up to 20 at once, 50 MB each.</Typography>}
      </Stack>
      {dragging && <Alert severity="info" icon={<Icon name="upload_file" />} sx={{ mb: 2 }}>Drop to add to this A3.</Alert>}
      {q.isSuccess && list.length === 0 ? (
        <Card><EmptyState icon="attach_file" title="No files yet" body="Reports added here stay with this A3 and feed its steps." /></Card>
      ) : (
        <Card sx={{ overflowX: 'auto' }}>
          <Table size="small">
            <TableHead>
              <TableRow>
                <TableCell>Name</TableCell>
                <TableCell>Type</TableCell>
                <TableCell>Added by</TableCell>
                <TableCell>Date</TableCell>
                <TableCell>Informs</TableCell>
                <TableCell>Reading</TableCell>
                <TableCell align="right">Actions</TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              {list.map((a) => (
                <TableRow key={a.id} sx={{ opacity: a.deletedAt ? 0.55 : 1 }}>
                  <TableCell sx={{ maxWidth: 320 }}>
                    <Typography variant="body2" noWrap title={a.fileName}>{a.fileName}{a.version > 1 ? ` (v${a.version})` : ''}</Typography>
                    <Typography variant="caption" color="text.secondary">{formatBytes(a.sizeBytes)}{a.deletedAt ? ', deleted' : ''}</Typography>
                  </TableCell>
                  <TableCell>{a.fileType.toUpperCase()}</TableCell>
                  <TableCell>{a.addedByName}</TableCell>
                  <TableCell sx={{ whiteSpace: 'nowrap' }}>{formatDate(a.addedAt)}</TableCell>
                  <TableCell>{a.informsSteps.length ? a.informsSteps.join(', ') : ''}</TableCell>
                  <TableCell>
                    <Stack spacing={0.25}>
                      <Box><ReadState a={a} /></Box>
                      <AiState a={a} />
                      {a.pendingImages > 0 && <Typography variant="caption" sx={{ color: tokens.warningText }}>{a.pendingImages} {a.pendingImages === 1 ? 'page needs' : 'pages need'} AI image reading</Typography>}
                    </Stack>
                  </TableCell>
                  <TableCell align="right" sx={{ whiteSpace: 'nowrap' }}>
                    {a.deletedAt ? (
                      sheet.isOwner && <Button size="small" onClick={() => void act(() => call('attachment_restore', { id: a.id }))}>Restore</Button>
                    ) : (
                      <>
                        <Tooltip title="Preview"><IconButton size="small" aria-label={`Preview ${a.fileName}`} onClick={() => void preview(a)}><Icon name="visibility" size={18} /></IconButton></Tooltip>
                        <Tooltip title="Open in its app"><IconButton size="small" aria-label={`Open ${a.fileName}`} onClick={() => void act(() => call('attachment_open', { id: a.id }))}><Icon name="open_in_new" size={18} /></IconButton></Tooltip>
                        <Tooltip title="Save a copy"><IconButton size="small" aria-label={`Save ${a.fileName}`} onClick={() => void saveAs(a)}><Icon name="download" size={18} /></IconButton></Tooltip>
                        {canEdit && a.sheetNames.length > 1 && (
                          <Tooltip title="Choose sheets"><IconButton size="small" aria-label={`Choose sheets in ${a.fileName}`} onClick={() => setSheetsFor(a)}><Icon name="table_view" size={18} /></IconButton></Tooltip>
                        )}
                        {canEdit && (a.aiState === 'failed' || a.readState === 'unreadable' || (a.aiState === 'none' && a.readState === 'read')) && (
                          <Tooltip title={a.aiState === 'none' ? 'Ask AI to propose from this file' : 'Retry'}><IconButton size="small" aria-label={`Retry ${a.fileName}`} onClick={() => void act(() => call('attachment_retry', { id: a.id }))}><Icon name="refresh" size={18} /></IconButton></Tooltip>
                        )}
                        {sheet.isOwner && <Tooltip title="Delete"><IconButton size="small" aria-label={`Delete ${a.fileName}`} onClick={() => void act(() => call('attachment_delete', { id: a.id }))}><Icon name="delete" size={18} /></IconButton></Tooltip>}
                      </>
                    )}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </Card>
      )}
      {list.some((a) => a.deletedAt) && <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mt: 1 }}>Deleted files can be restored for 30 days.</Typography>}
      {sheetsFor && <SheetPicker attachment={sheetsFor} onClose={() => setSheetsFor(null)} onSaved={refresh} />}
    </Box>
  );
}

function SheetPicker({ attachment, onClose, onSaved }: { attachment: Attachment; onClose: () => void; onSaved: () => void }) {
  const { notify } = useApp();
  const [excluded, setExcluded] = useState(attachment.excludedSheets);
  const save = async () => {
    try {
      await call('attachment_set_sheets', { id: attachment.id, excluded });
      onSaved();
      onClose();
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
    }
  };
  return (
    <Dialog open onClose={onClose} maxWidth="xs" fullWidth>
      <DialogTitle>Which sheets count?</DialogTitle>
      <DialogContent>
        <Typography variant="body2" color="text.secondary" sx={{ mb: 1 }}>AI receives column names, row counts and the first 50 rows of each chosen sheet.</Typography>
        <Stack>
          {attachment.sheetNames.map((n) => (
            <FormControlLabel
              key={n}
              label={n}
              control={<Checkbox checked={!excluded.includes(n)} onChange={(e) => setExcluded(e.target.checked ? excluded.filter((x) => x !== n) : [...excluded, n])} />}
            />
          ))}
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Cancel</Button>
        <Button variant="contained" onClick={save}>Save</Button>
      </DialogActions>
    </Dialog>
  );
}
