import { Alert, Snackbar } from '@mui/material';
import type { Toast } from '../context';

export function ToastHost({ toast, onClose }: { toast: Toast | null; onClose: () => void }) {
  return (
    <Snackbar
      open={!!toast}
      autoHideDuration={toast?.severity === 'error' ? 8000 : 4000}
      onClose={(_, reason) => reason !== 'clickaway' && onClose()}
      anchorOrigin={{ vertical: 'bottom', horizontal: 'left' }}
    >
      {toast ? (
        <Alert severity={toast.severity} variant="filled" onClose={onClose} sx={{ maxWidth: 560 }}>
          {toast.message}
        </Alert>
      ) : undefined}
    </Snackbar>
  );
}
