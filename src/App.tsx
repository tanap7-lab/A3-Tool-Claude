import { useCallback, useEffect, useState } from 'react';
import { Box, CircularProgress } from '@mui/material';
import { useQueryClient } from '@tanstack/react-query';
import { listen } from '@tauri-apps/api/event';
import { Navigate, Route, Routes, useNavigate } from 'react-router-dom';
import { call, type AppUser, type JobEvent, type ResolvedLink } from './api';
import { AppContext, type Toast } from './context';
import { Shell } from './components/Shell';
import { SignInPage } from './pages/SignInPage';
import { DashboardPage } from './pages/DashboardPage';
import { SheetListPage } from './pages/SheetListPage';
import { SheetPage } from './pages/SheetPage';
import { ReviewPage } from './pages/ReviewPage';
import { WallPage } from './pages/WallPage';
import { SettingsPage } from './pages/SettingsPage';
import { NewA3Dialog } from './components/NewA3Dialog';
import { ToastHost } from './components/ToastHost';

export function App() {
  const [user, setUser] = useState<AppUser | null>(null);
  const [booting, setBooting] = useState(true);
  const [jobs, setJobs] = useState<Record<string, JobEvent>>({});
  const [toast, setToast] = useState<Toast | null>(null);
  const [newOpen, setNewOpen] = useState(false);
  const [offline, setOffline] = useState(false);
  const qc = useQueryClient();
  const navigate = useNavigate();

  const openLink = useCallback(
    async (text: string): Promise<boolean> => {
      const target = await call<ResolvedLink | null>('link_resolve', { text }).catch(() => null);
      if (!target) return false;
      navigate(target.kind === 'a3' ? `/a3/${target.id}` : `/wall/${target.id}`);
      return true;
    },
    [navigate],
  );

  useEffect(() => {
    call<AppUser | null>('auto_sign_in')
      .then(setUser)
      .catch(() => setUser(null))
      .finally(() => setBooting(false));
  }, []);

  // A link that arrived before sign-in opens now (FR-LNK-03).
  useEffect(() => {
    if (!user) return;
    call<string | null>('link_take_pending').then((url) => {
      if (url) void openLink(url);
    });
  }, [user, openLink]);

  useEffect(() => {
    const unlistenLink = listen<string>('open-link', (e) => void openLink(e.payload));
    const unlistenJob = listen<JobEvent>('job', (e) => {
      const ev = e.payload;
      setJobs((cur) => {
        const next = { ...cur };
        if (ev.phase === 'done') delete next[ev.attachmentId];
        else next[ev.attachmentId] = ev;
        return next;
      });
      if (ev.phase === 'done') {
        void qc.invalidateQueries({ queryKey: ['attachments', ev.sheetId] });
        void qc.invalidateQueries({ queryKey: ['proposals', ev.sheetId] });
        void qc.invalidateQueries({ queryKey: ['sheet', ev.sheetId] });
        if (ev.error) setToast({ severity: 'error', message: `${ev.fileName}: ${ev.error}` });
        else if (ev.readState === 'unreadable') setToast({ severity: 'warning', message: `${ev.fileName} could not be read. It is still stored.` });
      }
    });
    return () => {
      void unlistenLink.then((f) => f());
      void unlistenJob.then((f) => f());
    };
  }, [qc, openLink]);

  // Section 15: a banner appears when the connection drops, and unsaved text waits in a recovery file.
  useEffect(() => {
    if (!user) return;
    let cancelled = false;
    const check = () =>
      call<boolean>('connection_ping')
        .then((ok) => {
          if (cancelled) return;
          setOffline((was) => {
            // Editors holding unsaved text save at once instead of waiting for their next retry.
            if (was && ok) window.dispatchEvent(new Event('a3-online'));
            return !ok;
          });
        })
        .catch(() => !cancelled && setOffline(true));
    const t = window.setInterval(check, 15_000);
    return () => {
      cancelled = true;
      window.clearInterval(t);
    };
  }, [user]);

  if (booting) {
    return (
      <Box sx={{ height: '100%', display: 'grid', placeItems: 'center' }}>
        <CircularProgress size={28} />
      </Box>
    );
  }

  if (!user) return <SignInPage onSignedIn={setUser} />;

  return (
    <AppContext.Provider
      value={{
        user,
        setUser,
        jobs,
        offline,
        notify: setToast,
        openNewA3: () => setNewOpen(true),
        openLink,
        signOut: async () => {
          await call('sign_out');
          qc.clear();
          setUser(null);
        },
      }}
    >
      <Shell>
        <Routes>
          <Route path="/" element={<DashboardPage />} />
          <Route path="/a3" element={<SheetListPage />} />
          <Route path="/a3/:id" element={<SheetPage />} />
          <Route path="/a3/:id/review" element={<ReviewPage />} />
          <Route path="/wall" element={<WallPage />} />
          <Route path="/wall/:warningId" element={<WallPage />} />
          <Route path="/settings" element={<SettingsPage />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </Shell>
      <NewA3Dialog open={newOpen} onClose={() => setNewOpen(false)} />
      <ToastHost toast={toast} onClose={() => setToast(null)} />
    </AppContext.Provider>
  );
}
