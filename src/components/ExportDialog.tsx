import { useEffect, useRef, useState } from 'react';
import { Alert, Box, Button, Checkbox, CircularProgress, Dialog, DialogActions, DialogContent, DialogTitle, FormControlLabel, Stack, ToggleButton, ToggleButtonGroup, Typography } from '@mui/material';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { convertFileSrc } from '@tauri-apps/api/core';
import { save as saveDialog } from '@tauri-apps/plugin-dialog';
import { call, errorMessage, type AiSettingsView, type ExportFormat, type ExportLayout, type ExportOptions, type Letter, type SheetDetail, type Step, type Suggestion } from '../api';
import { useApp } from '../context';
import { replaceSentence, type DocNode } from '../lib/doc';
import { wordDiff } from '../lib/text';
import { tokens } from '../theme';

type FitReport = Partial<Record<Letter, { size: number; continued: boolean }>>;

function stored<T extends string>(key: string, fallback: T, allowed: readonly T[]): T {
  try {
    const v = localStorage.getItem(key) as T | null;
    return v && allowed.includes(v) ? v : fallback;
  } catch {
    return fallback;
  }
}

function remember(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // A blocked store only means the choice is not remembered.
  }
}

/** FR-EXP-01: two choices, layout and format, with last-used defaults and a preview. */
export function ExportDialog({ open, onClose, detail }: { open: boolean; onClose: () => void; detail: SheetDetail }) {
  const { notify } = useApp();
  const qc = useQueryClient();
  const id = detail.sheet.id;
  const [layout, setLayout] = useState<ExportLayout>(() => stored('a3.exportLayout', 'quadrant', ['quadrant', 'accordion'] as const));
  const [format, setFormat] = useState<ExportFormat>(() => stored('a3.exportFormat', 'pdf', ['pdf', 'docx', 'html'] as const));
  const [includeAttachments, setIncludeAttachments] = useState(true);
  const [includeSourceNotes, setIncludeSourceNotes] = useState(false);
  const [previewUrl, setPreviewUrl] = useState<string | null>(null);
  const [fit, setFit] = useState<FitReport>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [condensing, setCondensing] = useState<Letter | null>(null);
  const [offers, setOffers] = useState<{ letter: Letter; items: Suggestion[] } | null>(null);
  const frameRef = useRef<HTMLIFrameElement | null>(null);
  const [frameBox, setFrameBox] = useState<HTMLDivElement | null>(null);
  const [scale, setScale] = useState(0.4);

  const ai = useQuery({ queryKey: ['ai-settings'], queryFn: () => call<AiSettingsView>('ai_settings_get'), enabled: open });
  const aiReady = !!ai.data?.ready && detail.sheet.projectAiEnabled;
  const options: ExportOptions = { layout, includeAttachments, includeSourceNotes };
  const pageWidthPx = ((layout === 'quadrant' ? 420 : 210) * 96) / 25.4;
  const pageHeightPx = (297 * 96) / 25.4;

  useEffect(() => {
    if (!open) return;
    setError(null);
    setFit({});
    let live = true;
    call<string>('export_preview', { sheet: id, options })
      .then((p) => {
        if (live) setPreviewUrl(convertFileSrc(p));
      })
      .catch((e) => live && setError(errorMessage(e)));
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, id, layout, includeAttachments, includeSourceNotes, detail.sheet.updatedAt]);

  useEffect(() => {
    if (!open) return;
    const onMessage = (e: MessageEvent) => {
      if (e.source === frameRef.current?.contentWindow && e.data?.a3fit) setFit(e.data.a3fit as FitReport);
    };
    window.addEventListener('message', onMessage);
    return () => window.removeEventListener('message', onMessage);
  }, [open]);

  useEffect(() => {
    if (!frameBox) return;
    const ro = new ResizeObserver(() => setScale(Math.min(1, frameBox.clientWidth / pageWidthPx)));
    ro.observe(frameBox);
    return () => ro.disconnect();
  }, [frameBox, pageWidthPx]);

  const overflowing = (Object.entries(fit) as [Letter, { size: number; continued: boolean }][]).filter(([, v]) => v.continued).map(([l]) => l);

  const run = async () => {
    const name = await call<string>('export_default_name', { sheet: id, layout, format });
    const filters = { pdf: [{ name: 'PDF', extensions: ['pdf'] }], docx: [{ name: 'Word document', extensions: ['docx'] }], html: [{ name: 'Web page', extensions: ['html'] }] }[format];
    const dest = await saveDialog({ defaultPath: name, filters });
    if (!dest) return;
    setBusy(true);
    setError(null);
    try {
      await call('export_save', { sheet: id, options, format, dest });
      remember('a3.exportLayout', layout);
      remember('a3.exportFormat', format);
      notify({ severity: 'success', message: `Saved ${dest.split(/[\\/]/).pop()}` });
      onClose();
      void call('path_open', { path: dest }).catch(() => undefined);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const condense = async (letter: Letter) => {
    setCondensing(letter);
    setError(null);
    try {
      const step = detail.steps.find((s) => s.letter === letter)!;
      const target = Math.round(step.plainText.length * 0.7);
      const items = await call<Suggestion[]>('ai_condense', { sheet: id, letter, targetChars: target });
      if (items.length === 0) notify({ severity: 'info', message: `AI could not shorten step ${letter} without losing facts.` });
      else setOffers({ letter, items });
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setCondensing(null);
    }
  };

  // Condensed text passes through the same accept step as everything else from AI.
  const acceptOffers = async () => {
    if (!offers) return;
    const step = detail.steps.find((s) => s.letter === offers.letter)!;
    let doc: DocNode | null = step.content as DocNode;
    let applied = 0;
    for (const o of offers.items) {
      const next: DocNode | null = doc ? replaceSentence(doc, o.original, o.rewrite) : null;
      if (next) {
        doc = next;
        applied++;
      }
    }
    try {
      await call<Step>('step_save', { step: step.id, expectedVersion: step.version, content: doc });
      setOffers(null);
      await qc.invalidateQueries({ queryKey: ['sheet', id] });
      notify({ severity: 'success', message: `${applied} sentences shortened in step ${offers.letter}.` });
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  return (
    <Dialog open={open} onClose={onClose} fullWidth maxWidth="lg">
      <DialogTitle>Export</DialogTitle>
      <DialogContent>
        <Box sx={{ display: 'grid', gridTemplateColumns: '280px minmax(0, 1fr)', gap: 3, minHeight: 520 }}>
          <Stack spacing={2}>
            <Box>
              <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mb: 0.5 }}>Layout</Typography>
              <ToggleButtonGroup exclusive size="small" fullWidth value={layout} onChange={(_, v) => v && setLayout(v)} aria-label="Layout">
                <ToggleButton value="quadrant" sx={{ textTransform: 'none' }}>4-quadrant</ToggleButton>
                <ToggleButton value="accordion" sx={{ textTransform: 'none' }}>Accordion</ToggleButton>
              </ToggleButtonGroup>
            </Box>
            <Box>
              <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mb: 0.5 }}>Format</Typography>
              <ToggleButtonGroup exclusive size="small" fullWidth value={format} onChange={(_, v) => v && setFormat(v)} aria-label="Format">
                <ToggleButton value="pdf" sx={{ textTransform: 'none' }}>PDF</ToggleButton>
                <ToggleButton value="docx" sx={{ textTransform: 'none' }}>Word</ToggleButton>
                <ToggleButton value="html" sx={{ textTransform: 'none' }}>HTML</ToggleButton>
              </ToggleButtonGroup>
            </Box>
            <Box>
              <FormControlLabel control={<Checkbox checked={includeAttachments} onChange={(e) => setIncludeAttachments(e.target.checked)} />} label={<Typography variant="body2">Attachment list</Typography>} />
              <FormControlLabel control={<Checkbox checked={includeSourceNotes} onChange={(e) => setIncludeSourceNotes(e.target.checked)} />} label={<Typography variant="body2">Source notes</Typography>} />
            </Box>
            {error && <Alert severity="error">{error}</Alert>}
            {layout === 'quadrant' && overflowing.length > 0 && (
              <Alert severity="warning">
                <Typography variant="body2" sx={{ mb: aiReady ? 1 : 0 }}>
                  Step {overflowing.join(', ')} {overflowing.length === 1 ? 'does' : 'do'} not fit its box at 8 pt, so a continued page will be added. Nothing is cut.
                </Typography>
                {aiReady &&
                  overflowing.map((l) => (
                    <Button key={l} size="small" variant="outlined" disabled={condensing !== null} onClick={() => void condense(l)} sx={{ mr: 0.5, mt: 0.5 }}>
                      {condensing === l ? 'Shortening...' : `Condense ${l} to fit`}
                    </Button>
                  ))}
              </Alert>
            )}
            {offers && (
              <Box sx={{ border: `1px solid ${tokens.line}`, borderRadius: 1, p: 1, maxHeight: 300, overflow: 'auto' }}>
                <Typography variant="body2" sx={{ fontWeight: 500, mb: 0.5 }}>Shorter wording for step {offers.letter}</Typography>
                {offers.items.map((o) => (
                  <Typography key={o.sentenceId} variant="caption" sx={{ display: 'block', mb: 0.75 }}>
                    {wordDiff(o.original, o.rewrite).map((p, i) => (p.kind === 'same' ? <span key={i}>{p.text}</span> : <span key={i} className={p.kind === 'del' ? 'diff-del' : 'diff-add'}>{p.text}</span>))}
                  </Typography>
                ))}
                <Stack direction="row" spacing={0.5}>
                  <Button size="small" variant="contained" onClick={() => void acceptOffers()}>Accept</Button>
                  <Button size="small" onClick={() => setOffers(null)}>Reject</Button>
                </Stack>
              </Box>
            )}
          </Stack>
          <Box ref={setFrameBox} sx={{ bgcolor: tokens.fill, border: `1px solid ${tokens.line}`, borderRadius: 1, overflow: 'auto', position: 'relative', height: 560 }}>
            {previewUrl ? (
              <Box sx={{ width: pageWidthPx * scale, height: pageHeightPx * scale + 400, overflow: 'hidden' }}>
                <iframe
                  ref={frameRef}
                  title="Export preview"
                  src={previewUrl}
                  sandbox="allow-scripts"
                  style={{ width: pageWidthPx, height: (pageHeightPx * scale + 400) / scale, border: 0, transform: `scale(${scale})`, transformOrigin: '0 0', background: '#fff' }}
                />
              </Box>
            ) : (
              <Box sx={{ display: 'grid', placeItems: 'center', height: '100%' }}><CircularProgress size={24} /></Box>
            )}
          </Box>
        </Box>
      </DialogContent>
      <DialogActions sx={{ px: 3, pb: 2 }}>
        <Button onClick={onClose}>Cancel</Button>
        <Button variant="contained" disabled={busy} onClick={() => void run()}>{busy ? 'Exporting...' : 'Export'}</Button>
      </DialogActions>
    </Dialog>
  );
}
