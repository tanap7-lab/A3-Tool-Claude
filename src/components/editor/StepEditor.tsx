import { useCallback, useEffect, useImperativeHandle, useRef, useState, forwardRef } from 'react';
import { Alert, Box, Button, Divider, IconButton, Stack, Tooltip, Typography } from '@mui/material';
import { EditorContent, useEditor, type Editor } from '@tiptap/react';
import { useQueryClient } from '@tanstack/react-query';
import { call, errorCode, STEP_HINTS, type Step } from '../../api';
import { useApp } from '../../context';
import { replaceSentence, sameDocument, type DocNode } from '../../lib/doc';
import { tokens } from '../../theme';
import { Icon } from '../common';
import { imageFileToDataUrl, stepExtensions } from './extensions';

const AUTOSAVE_MS = 2000;
const RETRY_MS = 15000;

export interface StepEditorHandle {
  /** Waits until typed text has reached the database. */
  flush: () => Promise<void>;
  applySuggestion: (original: string, rewrite: string) => boolean;
  matchesSummary: (original: DocNode, version: number) => boolean;
  loadSavedStep: (saved: Step) => void;
  selectedText: () => string;
  html: () => string;
}

interface Props {
  step: Step;
  canEdit: boolean;
  onOpenSource: (segmentId: string) => void;
  onImprove?: (selection: string | null) => void;
  aiBusy?: boolean;
  applyingSummary?: boolean;
  onHtmlChange?: (html: string) => void;
  fill?: boolean;
}

/**
 * One step's text box. Autosaves 2 seconds after typing stops (FR-A3-03), holds a one-editor lock
 * while focused (FR-A3-07), refuses stale saves (FR-A3-08), and keeps text in a local recovery file
 * whenever the database cannot be reached (section 15).
 */
export const StepEditor = forwardRef<StepEditorHandle, Props>(function StepEditor({ step, canEdit, onOpenSource, onImprove, aiBusy, applyingSummary, onHtmlChange, fill }, ref) {
  const { user } = useApp();
  const qc = useQueryClient();
  const versionRef = useRef(step.version);
  const dirtyRef = useRef(false);
  const lockHeldRef = useRef(false);
  const timerRef = useRef<number | null>(null);
  const editorRef = useRef<Editor | null>(null);
  const inFlightRef = useRef<Promise<void>>(Promise.resolve());
  const saveRef = useRef<() => Promise<void>>(() => Promise.resolve());
  const [saving, setSaving] = useState<'idle' | 'saving' | 'saved' | 'offline'>('idle');
  const [conflict, setConflict] = useState(false);
  const [recovery, setRecovery] = useState<{ content: DocNode; version: number } | null>(null);
  const [lockedBy, setLockedBy] = useState<string | null>(step.lockedBy && step.lockedBy !== user.id ? step.lockedByName : null);

  const editable = canEdit && step.state === 'open' && !lockedBy && !conflict && !applyingSummary;

  const saveOnce = useCallback(async () => {
    const editor = editorRef.current;
    if (!editor || !dirtyRef.current) return;
    const content = editor.getJSON();
    dirtyRef.current = false;
    setSaving('saving');
    try {
      const saved = await call<Step>('step_save', { step: step.id, expectedVersion: versionRef.current, content });
      versionRef.current = saved.version;
      setSaving('saved');
      await call('recovery_clear', { step: step.id });
      void qc.invalidateQueries({ queryKey: ['sheet', step.sheetId] });
    } catch (e) {
      const code = errorCode(e);
      await call('recovery_save', { step: step.id, content, version: versionRef.current }).catch(() => undefined);
      if (code === 'STALE') {
        setConflict(true);
        setSaving('idle');
      } else if (code === 'A3_STEP_LOCKED') {
        setLockedBy('someone else');
        setSaving('idle');
      } else if (code === 'OFFLINE' || code === 'DB') {
        dirtyRef.current = true;
        setSaving('offline');
        timerRef.current = window.setTimeout(() => void saveRef.current(), RETRY_MS);
      } else {
        setSaving('idle');
      }
    }
  }, [step.id, step.sheetId, qc]);

  // Saves run one at a time. Two overlapping saves would carry the same version, and the second
  // would look like someone else's edit.
  const save = useCallback(() => {
    const next = inFlightRef.current.then(saveOnce);
    inFlightRef.current = next;
    return next;
  }, [saveOnce]);
  saveRef.current = save;

  const editor = useEditor({
    extensions: stepExtensions(STEP_HINTS[step.letter]),
    content: step.content as never,
    editable,
    onUpdate: ({ editor: ed, transaction }) => {
      onHtmlChange?.(ed.getHTML());
      if (!transaction.docChanged || transaction.getMeta('a3-external')) return;
      dirtyRef.current = true;
      if (timerRef.current) window.clearTimeout(timerRef.current);
      timerRef.current = window.setTimeout(() => void save(), AUTOSAVE_MS);
    },
    onFocus: () => {
      if (!canEdit || step.state !== 'open' || lockHeldRef.current) return;
      call<Step>('step_lock', { step: step.id })
        .then((s) => {
          if (s.lockedBy && s.lockedBy !== user.id) {
            setLockedBy(s.lockedByName ?? 'someone else');
          } else {
            lockHeldRef.current = true;
            setLockedBy(null);
          }
        })
        .catch(() => undefined);
    },
    onBlur: () => {
      void (async () => {
        if (timerRef.current) window.clearTimeout(timerRef.current);
        await save();
        if (lockHeldRef.current) {
          lockHeldRef.current = false;
          await call('step_unlock', { step: step.id }).catch(() => undefined);
        }
      })();
    },
    editorProps: {
      attributes: { 'aria-label': `Step ${step.letter} text` },
      handlePaste: (view, event) => {
        const file = Array.from(event.clipboardData?.files ?? []).find((f) => f.type.startsWith('image/'));
        if (!file) return false;
        void imageFileToDataUrl(file).then((src) => {
          const node = view.state.schema.nodes.image.create({ src });
          view.dispatch(view.state.tr.replaceSelectionWith(node));
        });
        return true;
      },
      handleClick: (_view, _pos, event) => {
        // The chip is drawn after the sentence, so a click counts when it lands on the chip's part of the last line.
        const span = (event.target as HTMLElement).closest?.('.src-mark') as HTMLElement | null;
        if (!span) return false;
        const rects = span.getClientRects();
        const last = rects[rects.length - 1];
        const label = span.getAttribute('data-label') ?? '';
        const chipWidth = Math.min(200, label.length * 6 + 16);
        if (last && event.clientX >= last.right - chipWidth && event.clientY >= last.top) {
          const id = span.getAttribute('data-source-segment');
          if (id) onOpenSource(id);
          return true;
        }
        return false;
      },
    },
  });

  useEffect(() => {
    editorRef.current = editor;
  }, [editor]);

  useEffect(() => {
    editor?.setEditable(editable);
  }, [editor, editable]);

  useEffect(() => {
    const onOnline = () => {
      if (!dirtyRef.current) return;
      if (timerRef.current) window.clearTimeout(timerRef.current);
      void save();
    };
    window.addEventListener('a3-online', onOnline);
    return () => window.removeEventListener('a3-online', onOnline);
  }, [save]);

  useEffect(() => {
    if (editor) onHtmlChange?.(editor.getHTML());
    // Only on first mount of the editor.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editor]);

  // Newer text from someone else arrives through polling. It never overwrites unsaved typing.
  useEffect(() => {
    if (!editor) return;
    setLockedBy(step.lockedBy && step.lockedBy !== user.id ? (step.lockedByName ?? 'someone else') : null);
    if (step.version > versionRef.current && !dirtyRef.current && !editor.isFocused) {
      versionRef.current = step.version;
      editor.commands.setContent(step.content as never, { emitUpdate: false });
      onHtmlChange?.(editor.getHTML());
    } else if (step.version > versionRef.current && !dirtyRef.current) {
      versionRef.current = step.version;
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editor, step.version, step.lockedBy, step.lockedByName, step.state]);

  useEffect(() => {
    call<{ content: DocNode; version: number } | null>('recovery_load', { step: step.id }).then((r) => {
      if (r) setRecovery(r);
    });
    return () => {
      if (timerRef.current) window.clearTimeout(timerRef.current);
      void save();
      if (lockHeldRef.current) {
        lockHeldRef.current = false;
        void call('step_unlock', { step: step.id }).catch(() => undefined);
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [step.id]);

  useImperativeHandle(
    ref,
    () => ({
      flush: () => {
        if (timerRef.current) window.clearTimeout(timerRef.current);
        return save();
      },
      applySuggestion: (original, rewrite) => {
        if (!editor) return false;
        const next = replaceSentence(editor.getJSON() as DocNode, original, rewrite);
        if (!next) return false;
        dirtyRef.current = true;
        editor.commands.setContent(next as never, { emitUpdate: false });
        onHtmlChange?.(editor.getHTML());
        void save();
        return true;
      },
      matchesSummary: (original, version) => !!editor && versionRef.current === version && !dirtyRef.current && sameDocument(editor.getJSON(), original),
      loadSavedStep: (saved) => {
        if (!editor || saved.id !== step.id) return;
        versionRef.current = saved.version;
        dirtyRef.current = false;
        if (timerRef.current) window.clearTimeout(timerRef.current);
        editor.commands.setContent(saved.content as DocNode, { emitUpdate: false });
        onHtmlChange?.(editor.getHTML());
        setSaving('saved');
      },
      selectedText: () => {
        if (!editor) return '';
        const { from, to } = editor.state.selection;
        return from === to ? '' : editor.state.doc.textBetween(from, to, ' ');
      },
      html: () => editor?.getHTML() ?? '',
    }),
    [editor, step.id, save, onHtmlChange],
  );

  const reloadLatest = async () => {
    const detail = await qc.fetchQuery<{ steps: Step[] }>({
      queryKey: ['sheet', step.sheetId],
      queryFn: () => call('sheet_detail', { id: step.sheetId }),
      staleTime: 0,
    });
    const fresh = detail.steps.find((s) => s.id === step.id);
    if (fresh && editor) {
      versionRef.current = fresh.version;
      dirtyRef.current = false;
      editor.commands.setContent(fresh.content as never, { emitUpdate: false });
    }
    setConflict(false);
    await call('recovery_clear', { step: step.id });
  };

  const keepMine = async () => {
    const detail = await call<{ steps: Step[] }>('sheet_detail', { id: step.sheetId });
    const fresh = detail.steps.find((s) => s.id === step.id);
    if (fresh) versionRef.current = fresh.version;
    dirtyRef.current = true;
    setConflict(false);
    void save();
  };

  const restoreRecovery = () => {
    if (!recovery || !editor) return;
    dirtyRef.current = true;
    editor.commands.setContent(recovery.content as never, { emitUpdate: false });
    setRecovery(null);
    void save();
  };

  const discardRecovery = async () => {
    setRecovery(null);
    await call('recovery_clear', { step: step.id });
  };

  const Tool = ({ icon, label, active, onClick }: { icon: string; label: string; active?: boolean; onClick: () => void }) => (
    <Tooltip title={label}>
      <IconButton
        size="small"
        aria-label={label}
        aria-pressed={active}
        onMouseDown={(e) => e.preventDefault()}
        onClick={onClick}
        sx={{ borderRadius: 1, color: active ? tokens.primary : tokens.textSecondary, bgcolor: active ? tokens.primaryLight : 'transparent' }}
      >
        <Icon name={icon} size={18} />
      </IconButton>
    </Tooltip>
  );

  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', minHeight: 0, flex: fill ? 1 : undefined }}>
      {lockedBy && step.state === 'open' && canEdit && (
        <Alert severity="info" icon={<Icon name="lock" size={18} />} sx={{ py: 0, borderRadius: 0 }}>Being edited by {lockedBy}</Alert>
      )}
      {conflict && (
        <Alert
          severity="warning"
          sx={{ borderRadius: 0 }}
          action={
            <Stack direction="row" spacing={0.5}>
              <Button size="small" onClick={reloadLatest}>Load latest</Button>
              <Button size="small" onClick={keepMine}>Keep mine</Button>
            </Stack>
          }
        >
          Someone else saved this step first.
        </Alert>
      )}
      {recovery && (
        <Alert
          severity="info"
          sx={{ borderRadius: 0 }}
          action={
            <Stack direction="row" spacing={0.5}>
              <Button size="small" onClick={restoreRecovery}>Restore</Button>
              <Button size="small" onClick={discardRecovery}>Discard</Button>
            </Stack>
          }
        >
          Unsaved text from an earlier session was kept.
        </Alert>
      )}
      {editable && editor && (
        <Stack direction="row" spacing={0.25} sx={{ px: 1, py: 0.25, alignItems: 'center', borderBottom: `1px solid ${tokens.line}`, flexWrap: 'wrap' }} role="toolbar" aria-label={`Step ${step.letter} formatting`}>
          <Tool icon="format_bold" label="Bold" active={editor.isActive('bold')} onClick={() => editor.chain().focus().toggleBold().run()} />
          <Tool icon="format_italic" label="Italic" active={editor.isActive('italic')} onClick={() => editor.chain().focus().toggleItalic().run()} />
          <Tool icon="format_list_bulleted" label="Bullets" active={editor.isActive('bulletList')} onClick={() => editor.chain().focus().toggleBulletList().run()} />
          <Tool icon="format_list_numbered" label="Numbering" active={editor.isActive('orderedList')} onClick={() => editor.chain().focus().toggleOrderedList().run()} />
          <Tool icon="table" label="Table" active={editor.isActive('table')} onClick={() => editor.chain().focus().insertTable({ rows: 2, cols: 2, withHeaderRow: true }).run()} />
          {editor.isActive('table') && (
            <>
              <Tool icon="table_rows" label="Add row" onClick={() => editor.chain().focus().addRowAfter().run()} />
              <Tool icon="view_column" label="Add column" onClick={() => editor.chain().focus().addColumnAfter().run()} />
              <Tool icon="delete" label="Delete table" onClick={() => editor.chain().focus().deleteTable().run()} />
            </>
          )}
          <Divider orientation="vertical" flexItem sx={{ mx: 0.5 }} />
          {onImprove && (
            <Button
              size="small"
              disabled={aiBusy}
              startIcon={<Icon name="auto_awesome" size={16} />}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => {
                const { from, to } = editor.state.selection;
                onImprove(from === to ? null : editor.state.doc.textBetween(from, to, ' '));
              }}
              sx={{ minHeight: 26 }}
            >
              Improve
            </Button>
          )}
          <Box sx={{ flex: 1 }} />
          <Typography variant="caption" color="text.secondary" role="status" sx={{ pr: 0.5 }}>
            {saving === 'saving' ? 'Saving...' : saving === 'saved' ? 'Saved' : saving === 'offline' ? 'Kept on this computer' : ''}
          </Typography>
        </Stack>
      )}
      <Box
        className={`a3-editor${editable ? '' : ' readonly'}`}
        onClick={() => editable && editor?.commands.focus()}
        sx={{ flex: 1, minHeight: 0, overflow: 'auto', p: 1.5, cursor: editable ? 'text' : 'default' }}
      >
        <EditorContent editor={editor} />
      </Box>
    </Box>
  );
});
