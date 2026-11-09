import { useEffect, useState, type FormEvent } from 'react';
import { Alert, Box, Button, Card, CardContent, Checkbox, Collapse, FormControlLabel, Link, Stack, TextField, Typography } from '@mui/material';
import { open } from '@tauri-apps/plugin-dialog';
import { call, errorMessage, type AppUser, type ConnectionProfile } from '../api';
import { tokens } from '../theme';

const LOCAL: ConnectionProfile = { host: 'localhost', port: 5433, database: 'a3sheet', sslMode: 'disable', sslRootCert: null };

export function SignInPage({ onSignedIn }: { onSignedIn: (u: AppUser) => void }) {
  const [profile, setProfile] = useState<ConnectionProfile | null>(null);
  const [showProfile, setShowProfile] = useState(false);
  const [draft, setDraft] = useState<ConnectionProfile>(LOCAL);
  const [login, setLogin] = useState('');
  const [password, setPassword] = useState('');
  const [remember, setRemember] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    call<ConnectionProfile | null>('profile_get').then((p) => {
      setProfile(p);
      if (p) setDraft(p);
      else setShowProfile(true);
    });
    call<string | null>('remembered_login').then((l) => l && setLogin(l));
  }, []);

  const saveProfile = async () => {
    setError(null);
    try {
      await call('profile_save', { profile: draft });
      setProfile(draft);
      setShowProfile(false);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const importProfile = async () => {
    setError(null);
    const path = await open({ multiple: false, filters: [{ name: 'A3 Sheet connection', extensions: ['json'] }] });
    if (typeof path !== 'string') return;
    try {
      const p = await call<ConnectionProfile>('profile_import', { path });
      setProfile(p);
      setDraft(p);
      setShowProfile(false);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      onSignedIn(await call<AppUser>('sign_in', { login, password, remember }));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Box sx={{ height: '100%', overflow: 'auto', display: 'grid', placeItems: 'center', p: 2 }}>
      <Card sx={{ width: 400, maxWidth: '100%' }}>
        <CardContent sx={{ p: 4, '&:last-child': { pb: 4 } }}>
          <Typography variant="h1" sx={{ color: tokens.primary, mb: 0.5 }}>A3 Sheet</Typography>
          <Typography variant="body2" color="text.secondary" sx={{ mb: 3 }}>Sign in with the account your admin gave you.</Typography>
          {error && <Alert severity="error" sx={{ mb: 2 }}>{error}</Alert>}

          <Collapse in={!showProfile} unmountOnExit>
            <Stack component="form" spacing={2} onSubmit={submit}>
              <TextField label="Login name" value={login} onChange={(e) => setLogin(e.target.value)} autoFocus={!login} autoComplete="username" required />
              <TextField label="Password" type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoFocus={!!login} autoComplete="current-password" required />
              <FormControlLabel
                control={<Checkbox checked={remember} onChange={(e) => setRemember(e.target.checked)} />}
                label={<Typography variant="body2">Keep me signed in on this computer</Typography>}
              />
              <Button type="submit" variant="contained" disabled={busy || !login || !password}>{busy ? 'Signing in...' : 'Sign in'}</Button>
              <Typography variant="caption" color="text.secondary">
                Connected to {profile?.host}:{profile?.port}.{' '}
                <Link component="button" type="button" onClick={() => setShowProfile(true)}>Change</Link>
              </Typography>
            </Stack>
          </Collapse>

          <Collapse in={showProfile} unmountOnExit>
            <Stack spacing={2}>
              <Typography variant="h4">Database connection</Typography>
              <Button variant="outlined" onClick={importProfile}>Use a profile file from my admin</Button>
              <Typography variant="caption" color="text.secondary">Or enter the details once:</Typography>
              <TextField label="Server" value={draft.host} onChange={(e) => setDraft({ ...draft, host: e.target.value })} />
              <TextField label="Port" type="number" value={draft.port} onChange={(e) => setDraft({ ...draft, port: Number(e.target.value) })} />
              <TextField label="Database" value={draft.database} onChange={(e) => setDraft({ ...draft, database: e.target.value })} />
              <FormControlLabel
                control={
                  <Checkbox
                    checked={draft.sslMode === 'verify-full'}
                    onChange={(e) => setDraft({ ...draft, sslMode: e.target.checked ? 'verify-full' : 'disable' })}
                  />
                }
                label={<Typography variant="body2">Use a verified TLS connection (required for a remote server)</Typography>}
              />
              <Stack direction="row" spacing={1}>
                <Button variant="contained" onClick={saveProfile}>Save</Button>
                {profile && <Button onClick={() => setShowProfile(false)}>Cancel</Button>}
              </Stack>
            </Stack>
          </Collapse>
        </CardContent>
      </Card>
    </Box>
  );
}
