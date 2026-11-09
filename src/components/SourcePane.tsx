import { Fragment, useEffect, useMemo, useRef, useState } from 'react';
import { Alert, Box, Button, Chip, CircularProgress, Drawer, IconButton, Stack, Typography } from '@mui/material';
import { useQuery } from '@tanstack/react-query';
import { call, errorMessage, type ImageData, type Segment, type SourceView } from '../api';
import { findQuote } from '../lib/text';
import { tokens } from '../theme';
import { Icon } from './common';

/** Text with the cited passage highlighted. */
function Highlighted({ text, quote }: { text: string; quote?: string | null }) {
  const hit = findQuote(text, quote);
  if (!hit) return <>{text}</>;
  return (
    <>
      {text.slice(0, hit[0])}
      <mark className="src-hit">{text.slice(hit[0], hit[1])}</mark>
      {text.slice(hit[1])}
    </>
  );
}

/**
 * Shows a file's extracted passages, scrolled to the cited one, with the quote highlighted
 * (section 8). Pages and slides can also be shown as pictures.
 */
export function SourcePane({ segmentId, quote }: { segmentId: string; quote?: string | null }) {
  const view = useQuery({ queryKey: ['source-view', segmentId], queryFn: () => call<SourceView>('source_view', { segment: segmentId }), retry: false });
  const segments = useQuery({
    queryKey: ['segments', view.data?.attachmentId],
    queryFn: () => call<Segment[]>('segments_list', { attachment: view.data!.attachmentId }),
    enabled: !!view.data,
  });
  const [images, setImages] = useState<ImageData[] | null>(null);
  const [imageError, setImageError] = useState<string | null>(null);
  const [loadingImage, setLoadingImage] = useState(false);
  const hitRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    setImages(null);
    setImageError(null);
  }, [segmentId]);

  useEffect(() => {
    hitRef.current?.scrollIntoView({ block: 'center' });
  }, [segments.data, segmentId]);

  const current = useMemo(() => segments.data?.find((s) => s.id === segmentId), [segments.data, segmentId]);
  const canShowPicture = !!view.data && /\.(pdf|png|jpe?g|pptx)$/i.test(view.data.fileName);

  const showPicture = async () => {
    setLoadingImage(true);
    setImageError(null);
    try {
      setImages(await call<ImageData[]>('segment_preview', { segment: segmentId }));
    } catch (e) {
      setImageError(errorMessage(e));
    } finally {
      setLoadingImage(false);
    }
  };

  if (view.isError) return <Alert severity="info" sx={{ m: 2 }}>Source removed.</Alert>;
  if (!view.data) return <Box sx={{ display: 'grid', placeItems: 'center', height: 200 }}><CircularProgress size={24} /></Box>;

  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', height: '100%', minHeight: 0 }}>
      <Stack direction="row" spacing={1} sx={{ px: 2, py: 1.5, alignItems: 'center', borderBottom: `1px solid ${tokens.line}` }}>
        <Icon name="description" color={tokens.textSecondary} />
        <Box sx={{ minWidth: 0, flex: 1 }}>
          <Typography variant="h4" noWrap title={view.data.fileName}>{view.data.fileName}</Typography>
          <Typography variant="caption" color="text.secondary">{current?.label ?? view.data.label}</Typography>
        </Box>
        <Button size="small" startIcon={<Icon name="open_in_new" size={16} />} onClick={() => void call('attachment_open', { id: view.data!.attachmentId })}>Open</Button>
      </Stack>
      {view.data.deleted && <Alert severity="warning" sx={{ borderRadius: 0 }}>Source removed. This file was deleted from the A3.</Alert>}
      {current?.fromImage && <Alert severity="warning" sx={{ borderRadius: 0 }}>Read from image. Check the numbers.</Alert>}
      <Box sx={{ flex: 1, overflow: 'auto', p: 2, bgcolor: tokens.fill }}>
        {canShowPicture && !images && (
          <Button size="small" variant="outlined" sx={{ mb: 1.5 }} disabled={loadingImage} onClick={showPicture} startIcon={<Icon name="image" size={16} />}>
            {loadingImage ? 'Drawing...' : 'Show as picture'}
          </Button>
        )}
        {imageError && <Alert severity="error" sx={{ mb: 1.5 }}>{imageError}</Alert>}
        {images?.map((img, i) => (
          <img key={i} src={`data:${img.mime};base64,${img.base64}`} alt={`${view.data!.fileName}, ${current?.label ?? ''}`} style={{ width: '100%', display: 'block', marginBottom: 12, border: `1px solid ${tokens.line}` }} />
        ))}
        {images?.length === 0 && <Typography variant="body2" color="text.secondary" sx={{ mb: 1.5 }}>There is no picture to show for this passage.</Typography>}
        {segments.data?.map((s) => {
          const here = s.id === segmentId;
          return (
            <Fragment key={s.id}>
              <Box
                ref={here ? hitRef : undefined}
                sx={{
                  mb: 1.5,
                  p: 1.5,
                  bgcolor: '#fff',
                  borderRadius: 1,
                  border: `1px solid ${here ? tokens.primary : tokens.line}`,
                }}
              >
                <Stack direction="row" spacing={1} sx={{ mb: 0.5, alignItems: 'center' }}>
                  <Typography variant="caption" color="text.secondary">{s.label}</Typography>
                  {s.fromImage && <Chip size="small" label="read from image" sx={{ height: 18, fontSize: 11, bgcolor: tokens.warningLight, color: tokens.warningText }} />}
                  {!s.aiVisible && <Chip size="small" label="not sent to AI" sx={{ height: 18, fontSize: 11 }} />}
                </Stack>
                <Typography variant="body2" sx={{ whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
                  {s.needsImage && !s.text ? <em>This page has no readable text.</em> : <Highlighted text={s.text} quote={here ? quote : null} />}
                </Typography>
              </Box>
            </Fragment>
          );
        })}
      </Box>
    </Box>
  );
}

export function SourceDrawer({ source, onClose }: { source: { segmentId: string; quote?: string | null } | null; onClose: () => void }) {
  return (
    <Drawer anchor="right" open={!!source} onClose={onClose} slotProps={{ paper: { sx: { width: 560, maxWidth: '90vw' } } }}>
      <Stack direction="row" sx={{ justifyContent: 'flex-end', px: 1, pt: 0.5 }}>
        <IconButton size="small" aria-label="Close source" onClick={onClose}><Icon name="close" /></IconButton>
      </Stack>
      <Box sx={{ flex: 1, minHeight: 0 }}>{source && <SourcePane segmentId={source.segmentId} quote={source.quote} />}</Box>
    </Drawer>
  );
}
