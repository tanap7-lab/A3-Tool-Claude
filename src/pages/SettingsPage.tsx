import { useEffect, useMemo, useState } from 'react';
import { Alert, Autocomplete, Box, Button, Card, CardContent, Checkbox, Chip, CircularProgress, Dialog, DialogActions, DialogContent, DialogTitle, Divider, FormControlLabel, IconButton, InputAdornment, List, ListItem, ListItemButton, MenuItem, Stack, Switch, Tab, Table, TableBody, TableCell, TableHead, TableRow, Tabs, TextField, Tooltip, Typography } from '@mui/material';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useSearchParams } from 'react-router-dom';
import { call, errorMessage, type AiConfig, type AiSettingsView, type AppUser, type ConnectionProfile, type ConnectionTest, type PickValue, type Project, type Settings } from '../api';
import { EmptyState, FieldRow, Icon, PageHeader } from '../components/common';
import { useApp } from '../context';
import { tokens } from '../theme';

export function SettingsPage() {
  const { user } = useApp();
  const [params, setParams] = useSearchParams();
  const tabs = ['projects', 'ai', 'account', ...(user.isAdmin ? ['admin'] : [])];
  const tab = tabs.includes(params.get('tab') ?? '') ? params.get('tab')! : 'projects';
  return (
    <Box sx={{ p: 3, maxWidth: 1100 }}>
      <PageHeader title="Settings" />
      <Tabs value={tab} onChange={(_, v) => setParams({ tab: v }, { replace: true })} sx={{ mb: 2, borderBottom: `1px solid ${tokens.line}` }}>
        <Tab value="projects" label="Projects" />
        <Tab value="ai" label="AI" />
        <Tab value="account" label="Account" />
        {user.isAdmin && <Tab value="admin" label="Admin" />}
      </Tabs>
      {tab === 'projects' && <ProjectsTab />}
      {tab === 'ai' && <AiTab />}
      {tab === 'account' && <AccountTab />}
      {tab === 'admin' && <AdminTab />}
    </Box>
  );
}

// ------------------------------------------------------------------ Projects

function ProjectsTab() {
  const { user, setUser, notify } = useApp();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ['projects', 'mine'], queryFn: () => call<Project[]>('projects_list', { onlyMine: true }) });
  const [selected, setSelected] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [form, setForm] = useState({ name: '', code: '', description: '' });
  const list = q.data ?? [];
  const project = list.find((p) => p.id === selected) ?? list[0];

  const create = useMutation({
    mutationFn: () => call<Project>('project_create', form),
    onSuccess: (p) => {
      setCreating(false);
      setForm({ name: '', code: '', description: '' });
      setSelected(p.id);
      void qc.invalidateQueries({ queryKey: ['projects'] });
    },
    onError: (e) => notify({ severity: 'error', message: errorMessage(e) }),
  });

  const setDefault = async (p: Project) => {
    try {
      setUser(await call<AppUser>('account_update', { displayName: user.displayName, email: user.email, defaultProjectId: p.id }));
      notify({ severity: 'success', message: `${p.name} is now your default project.` });
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
    }
  };

  return (
    <Box sx={{ display: 'grid', gridTemplateColumns: '280px minmax(0, 1fr)', gap: 2, alignItems: 'start' }}>
      <Card>
        <List dense disablePadding>
          {list.map((p) => (
            <ListItemButton key={p.id} selected={project?.id === p.id} onClick={() => setSelected(p.id)} sx={{ display: 'block', opacity: p.archivedAt ? 0.6 : 1 }}>
              <Typography variant="body2">{p.name}</Typography>
              <Typography variant="caption" color="text.secondary">{p.code}{p.isOwner ? ' · Owner' : ''}{user.defaultProjectId === p.id ? ' · Default' : ''}{p.archivedAt ? ' · Archived' : ''}</Typography>
            </ListItemButton>
          ))}
        </List>
        {list.length > 0 && <Divider />}
        <Box sx={{ p: 1 }}><Button fullWidth startIcon={<Icon name="add" />} onClick={() => setCreating(true)}>New project</Button></Box>
      </Card>
      {project ? (
        <ProjectDetail key={project.id} project={project} isDefault={user.defaultProjectId === project.id} onSetDefault={() => void setDefault(project)} />
      ) : (
        q.isSuccess && <Card><EmptyState icon="folder" title="No projects yet" body="An A3 belongs to a project. Anyone can create one." action={<Button variant="contained" onClick={() => setCreating(true)}>New project</Button>} /></Card>
      )}
      <Dialog open={creating} onClose={() => setCreating(false)} fullWidth maxWidth="xs">
        <DialogTitle>New project</DialogTitle>
        <DialogContent>
          <Stack spacing={2} sx={{ pt: 0.5 }}>
            <TextField label="Name" autoFocus value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} />
            <TextField label="Short code" value={form.code} onChange={(e) => setForm({ ...form, code: e.target.value.toUpperCase() })} helperText="For example PRESS" />
            <TextField label="Description" multiline minRows={2} value={form.description} onChange={(e) => setForm({ ...form, description: e.target.value })} />
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setCreating(false)}>Cancel</Button>
          <Button variant="contained" disabled={!form.name.trim() || !form.code.trim() || create.isPending} onClick={() => create.mutate()}>Create</Button>
        </DialogActions>
      </Dialog>
    </Box>
  );
}

function ProjectDetail({ project, isDefault, onSetDefault }: { project: Project; isDefault: boolean; onSetDefault: () => void }) {
  const { notify } = useApp();
  const qc = useQueryClient();
  const members = useQuery({ queryKey: ['members', project.id], queryFn: () => call<AppUser[]>('project_members', { project: project.id }) });
  const users = useQuery({ queryKey: ['users'], queryFn: () => call<AppUser[]>('users_list'), enabled: project.isOwner });
  const picks = useQuery({ queryKey: ['picks', project.id], queryFn: () => call<PickValue[]>('pick_values', { project: project.id }) });

  const run = async (fn: () => Promise<unknown>, keys: string[][]) => {
    try {
      await fn();
      keys.forEach((k) => void qc.invalidateQueries({ queryKey: k }));
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
    }
  };

  const available = useMemo(() => (users.data ?? []).filter((u) => u.active && !(members.data ?? []).some((m) => m.id === u.id)), [users.data, members.data]);

  return (
    <Card>
      <CardContent>
        <Stack direction="row" spacing={2} useFlexGap sx={{ alignItems: 'center', mb: 2, flexWrap: 'wrap' }}>
          <Box sx={{ flex: 1, minWidth: 200 }}>
            <Typography variant="h2">{project.name}</Typography>
            <Typography variant="body2" color="text.secondary">{project.code}{project.description ? ` · ${project.description}` : ''}</Typography>
            <Typography variant="caption" color="text.secondary">Owner: {project.ownerName}</Typography>
          </Box>
          {!isDefault && <Button size="small" variant="outlined" onClick={onSetDefault}>Set as default</Button>}
          {project.isOwner && (
            <Button size="small" onClick={() => void run(() => call('project_set_archived', { project: project.id, archived: !project.archivedAt }), [['projects']])}>
              {project.archivedAt ? 'Restore project' : 'Archive project'}
            </Button>
          )}
        </Stack>
        <FormControlLabel
          sx={{ mb: 2 }}
          control={<Switch checked={project.aiEnabled} disabled={!project.isOwner} onChange={(e) => void run(() => call('project_set_ai', { project: project.id, enabled: e.target.checked }), [['projects'], ['sheet']])} />}
          label={<Typography variant="body2">Allow AI for this project. When off, no A3 text from this project goes to an AI provider.</Typography>}
        />
        <Divider sx={{ mb: 2 }} />
        <Typography variant="h4" sx={{ mb: 1 }}>Members</Typography>
        {project.isOwner && (
          <Autocomplete
            size="small"
            sx={{ mb: 1, maxWidth: 360 }}
            options={available}
            value={null}
            getOptionLabel={(u) => `${u.displayName} (${u.roleName})`}
            onChange={(_, u) => u && void run(() => call('project_add_member', { project: project.id, user: u.id }), [['members', project.id], ['projects']])}
            renderInput={(p) => <TextField {...p} placeholder="Add a person" />}
          />
        )}
        <Stack direction="row" spacing={0.75} useFlexGap sx={{ flexWrap: 'wrap', mb: 2 }}>
          {(members.data ?? []).map((m) => (
            <Chip
              key={m.id}
              label={m.displayName}
              variant="outlined"
              onDelete={project.isOwner && m.id !== project.ownerId ? () => void run(() => call('project_remove_member', { project: project.id, user: m.id }), [['members', project.id], ['projects']]) : undefined}
            />
          ))}
        </Stack>
        <Divider sx={{ mb: 2 }} />
        <Box sx={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 3 }}>
          {(['Area', 'Product'] as const).map((listName) => (
            <PickEditor key={listName} listName={listName} project={project} values={(picks.data ?? []).filter((p) => p.listName === listName)} refresh={() => void qc.invalidateQueries({ queryKey: ['picks', project.id] })} />
          ))}
        </Box>
      </CardContent>
    </Card>
  );
}

function PickEditor({ listName, project, values, refresh }: { listName: 'Area' | 'Product' | 'Category' | 'Source'; project: Project | null; values: PickValue[]; refresh: () => void }) {
  const { notify } = useApp();
  const [text, setText] = useState('');
  const editable = project ? project.isOwner : true;
  const add = async () => {
    try {
      await call('pick_add', { listName, value: text, project: project?.id ?? null });
      setText('');
      refresh();
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
    }
  };
  const remove = async (id: string) => {
    try {
      await call('pick_remove', { id });
      refresh();
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
    }
  };
  return (
    <Box>
      <Typography variant="h4" sx={{ mb: 1 }}>{listName}{listName === 'Area' ? ' or line' : listName === 'Product' ? ' or part' : ''} list</Typography>
      {editable && (
        <Box component="form" onSubmit={(e) => { e.preventDefault(); if (text.trim()) void add(); }} sx={{ mb: 1 }}>
          <TextField fullWidth placeholder={`Add a ${listName.toLowerCase()}`} value={text} onChange={(e) => setText(e.target.value)} slotProps={{ htmlInput: { 'aria-label': `Add a ${listName.toLowerCase()}` }, input: { endAdornment: <InputAdornment position="end"><IconButton size="small" type="submit" aria-label="Add"><Icon name="add" size={18} /></IconButton></InputAdornment> } }} />
        </Box>
      )}
      <Stack direction="row" spacing={0.75} useFlexGap sx={{ flexWrap: 'wrap' }}>
        {values.length === 0 && <Typography variant="body2" color="text.secondary">None yet.</Typography>}
        {values.map((v) => <Chip key={v.id} size="small" label={v.value} variant="outlined" onDelete={editable ? () => void remove(v.id) : undefined} />)}
      </Stack>
    </Box>
  );
}

// ------------------------------------------------------------------ AI

const PROVIDERS = [
  { value: 'anthropic', label: 'Anthropic' },
  { value: 'openai', label: 'OpenAI' },
  { value: 'google', label: 'Google' },
  { value: 'custom', label: 'Custom (OpenAI-compatible)' },
];

function AiTab() {
  const { notify } = useApp();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ['ai-settings'], queryFn: () => call<AiSettingsView>('ai_settings_get') });
  const [config, setConfig] = useState<AiConfig>({ provider: 'anthropic', baseUrl: '', model: '' });
  const [key, setKey] = useState('');
  const [models, setModels] = useState<string[]>([]);
  const [loadingModels, setLoadingModels] = useState(false);
  const [modelError, setModelError] = useState<string | null>(null);
  const [test, setTest] = useState<ConnectionTest | null>(null);
  const [testing, setTesting] = useState(false);

  useEffect(() => {
    if (q.data?.config) setConfig({ ...q.data.config, baseUrl: q.data.config.baseUrl ?? '' });
    // Runs when the stored settings first load.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [q.data?.config?.provider, q.data?.config?.model]);

  const draft = { config: { ...config, baseUrl: config.baseUrl || null }, key: key || null };
  const haveKey = !!key || !!q.data?.hasKey;

  const fetchModels = async () => {
    setLoadingModels(true);
    setModelError(null);
    try {
      setModels(await call<string[]>('ai_list_models', { draft }));
    } catch (e) {
      setModelError(errorMessage(e));
      setModels([]);
    } finally {
      setLoadingModels(false);
    }
  };

  const save = async () => {
    try {
      await call('ai_settings_save', { config: { ...config, baseUrl: config.baseUrl || null, visionOk: test?.imageReadingWorks ?? config.visionOk ?? false }, key: key || null });
      setKey('');
      await qc.invalidateQueries({ queryKey: ['ai-settings'] });
      notify({ severity: 'success', message: 'AI settings saved.' });
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
    }
  };

  const runTest = async () => {
    setTesting(true);
    setTest(null);
    try {
      const t = await call<ConnectionTest>('ai_test', { draft });
      setTest(t);
      await qc.invalidateQueries({ queryKey: ['ai-settings'] });
    } catch (e) {
      setTest({ keyWorks: false, structuredOutputWorks: false, imageReadingWorks: false, message: errorMessage(e) });
    } finally {
      setTesting(false);
    }
  };

  const clearKey = async () => {
    try {
      await call('ai_key_clear');
      await qc.invalidateQueries({ queryKey: ['ai-settings'] });
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
    }
  };

  const Result = ({ ok, label }: { ok: boolean; label: string }) => (
    <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }}>
      <Icon name={ok ? 'check_circle' : 'cancel'} size={18} color={ok ? tokens.success : tokens.error} />
      <Typography variant="body2">{label}{ok ? '' : ': not working'}</Typography>
    </Stack>
  );

  return (
    <Card sx={{ maxWidth: 760 }}>
      <CardContent>
        <Alert severity="info" sx={{ mb: 2 }}>A3 and attachment text is sent to the provider you choose here. Your calls use your own key and bill to your account.</Alert>
        <Stack spacing={1}>
          <FieldRow label="Provider">
            <TextField select fullWidth value={config.provider} onChange={(e) => { setConfig({ ...config, provider: e.target.value, model: '' }); setModels([]); setTest(null); }} slotProps={{ htmlInput: { 'aria-label': 'Provider' } }}>
              {PROVIDERS.map((p) => <MenuItem key={p.value} value={p.value}>{p.label}</MenuItem>)}
            </TextField>
          </FieldRow>
          {config.provider === 'custom' && (
            <FieldRow label="Address">
              <TextField fullWidth placeholder="https://llm.yourcompany.example/v1" value={config.baseUrl ?? ''} onChange={(e) => setConfig({ ...config, baseUrl: e.target.value })} slotProps={{ htmlInput: { 'aria-label': 'Address' } }} />
            </FieldRow>
          )}
          <FieldRow label="API key">
            <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }}>
              <TextField
                fullWidth
                type="password"
                autoComplete="off"
                placeholder={q.data?.hasKey ? 'Stored in Windows Credential Manager' : 'Paste your key'}
                value={key}
                onChange={(e) => setKey(e.target.value)}
                slotProps={{ htmlInput: { 'aria-label': 'API key' } }}
              />
              {q.data?.hasKey && <Button size="small" onClick={() => void clearKey()}>Remove</Button>}
            </Stack>
          </FieldRow>
          <FieldRow label="Model">
            <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }}>
              <Autocomplete
                freeSolo
                fullWidth
                size="small"
                options={models}
                value={config.model}
                onChange={(_, v) => setConfig({ ...config, model: v ?? '' })}
                onInputChange={(_, v) => setConfig((c) => ({ ...c, model: v }))}
                renderInput={(p) => <TextField {...p} placeholder="Choose or type a model" slotProps={{ ...p.slotProps, htmlInput: { ...p.slotProps.htmlInput, 'aria-label': 'Model' } }} />}
              />
              <Button size="small" variant="outlined" sx={{ whiteSpace: 'nowrap', flex: 'none' }} disabled={loadingModels || (!haveKey && config.provider !== 'custom')} onClick={() => void fetchModels()}>
                {loadingModels ? 'Loading...' : 'Fetch list'}
              </Button>
            </Stack>
          </FieldRow>
          {modelError && <Alert severity="warning">{modelError} You can type the model name instead.</Alert>}
        </Stack>
        <Stack direction="row" spacing={1} sx={{ mt: 2 }}>
          <Button variant="contained" disabled={!config.model} onClick={() => void save()}>Save</Button>
          <Button variant="outlined" disabled={testing || !config.model} onClick={() => void runTest()}>{testing ? 'Testing...' : 'Test connection'}</Button>
          {testing && <CircularProgress size={20} />}
        </Stack>
        {test && (
          <Stack spacing={0.5} sx={{ mt: 2 }} role="status">
            <Result ok={test.keyWorks} label="Key works" />
            <Result ok={test.structuredOutputWorks} label="Structured output works" />
            <Result ok={test.imageReadingWorks} label="Image reading works" />
            {test.message && <Typography variant="body2" color="text.secondary">{test.message}</Typography>}
          </Stack>
        )}
      </CardContent>
    </Card>
  );
}

// ------------------------------------------------------------------ Account

function AccountTab() {
  const { user, setUser, notify, signOut } = useApp();
  const projects = useQuery({ queryKey: ['projects', 'mine'], queryFn: () => call<Project[]>('projects_list', { onlyMine: true }) });
  const [name, setName] = useState(user.displayName);
  const [email, setEmail] = useState(user.email);
  const [defaultProject, setDefaultProject] = useState(user.defaultProjectId ?? '');
  const [oldPassword, setOldPassword] = useState('');
  const [newPassword, setNewPassword] = useState('');

  const save = async () => {
    try {
      setUser(await call<AppUser>('account_update', { displayName: name, email, defaultProjectId: defaultProject || null }));
      notify({ severity: 'success', message: 'Saved.' });
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
    }
  };

  const changePassword = async () => {
    try {
      await call('account_change_password', { oldPassword, newPassword });
      setOldPassword('');
      setNewPassword('');
      notify({ severity: 'success', message: 'Password changed.' });
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
    }
  };

  return (
    <Stack spacing={2} sx={{ maxWidth: 620 }}>
      <Card>
        <CardContent>
          <Stack spacing={1}>
            <FieldRow label="Login name"><Typography variant="body1">{user.roleName}</Typography></FieldRow>
            <FieldRow label="Display name"><TextField fullWidth value={name} onChange={(e) => setName(e.target.value)} slotProps={{ htmlInput: { 'aria-label': 'Display name' } }} /></FieldRow>
            <FieldRow label="Email"><TextField fullWidth value={email} onChange={(e) => setEmail(e.target.value)} slotProps={{ htmlInput: { 'aria-label': 'Email' } }} /></FieldRow>
            <FieldRow label="Default project">
              <TextField select fullWidth value={projects.data ? defaultProject : ''} onChange={(e) => setDefaultProject(e.target.value)} slotProps={{ htmlInput: { 'aria-label': 'Default project' } }}>
                <MenuItem value=""><em>None</em></MenuItem>
                {(projects.data ?? []).map((p) => <MenuItem key={p.id} value={p.id}>{p.name}</MenuItem>)}
              </TextField>
            </FieldRow>
          </Stack>
          <Button variant="contained" sx={{ mt: 2 }} onClick={() => void save()}>Save</Button>
        </CardContent>
      </Card>
      <Card>
        <CardContent>
          <Typography variant="h4" sx={{ mb: 1.5 }}>Change password</Typography>
          <Stack spacing={1.5}>
            <TextField type="password" label="Current password" autoComplete="current-password" value={oldPassword} onChange={(e) => setOldPassword(e.target.value)} />
            <TextField type="password" label="New password" autoComplete="new-password" helperText="At least 8 characters" value={newPassword} onChange={(e) => setNewPassword(e.target.value)} />
          </Stack>
          <Button variant="outlined" sx={{ mt: 2 }} disabled={!oldPassword || newPassword.length < 8} onClick={() => void changePassword()}>Change password</Button>
        </CardContent>
      </Card>
      <Box><Button color="inherit" startIcon={<Icon name="logout" />} onClick={() => void signOut()}>Sign out</Button></Box>
    </Stack>
  );
}

// ------------------------------------------------------------------ Admin

function AdminTab() {
  const { user, notify } = useApp();
  const qc = useQueryClient();
  const users = useQuery({ queryKey: ['users'], queryFn: () => call<AppUser[]>('users_list') });
  const settings = useQuery({ queryKey: ['settings'], queryFn: () => call<Settings>('settings_get') });
  const picks = useQuery({ queryKey: ['picks', 'company'], queryFn: () => call<PickValue[]>('pick_values', { project: null }) });
  const profile = useQuery({ queryKey: ['profile'], queryFn: () => call<ConnectionProfile | null>('profile_get') });
  const [form, setForm] = useState<Settings | null>(null);
  const [creating, setCreating] = useState(false);
  const [newUser, setNewUser] = useState({ login: '', name: '', email: '', password: '', isAdmin: false });
  const [resetting, setResetting] = useState<AppUser | null>(null);
  const [resetPassword, setResetPassword] = useState('');
  const current = form ?? settings.data;

  const run = async (fn: () => Promise<unknown>, success?: string) => {
    try {
      await fn();
      void qc.invalidateQueries({ queryKey: ['users'] });
      void qc.invalidateQueries({ queryKey: ['settings'] });
      if (success) notify({ severity: 'success', message: success });
      return true;
    } catch (e) {
      notify({ severity: 'error', message: errorMessage(e) });
      return false;
    }
  };

  return (
    <Stack spacing={2}>
      <Card>
        <CardContent>
          <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between', mb: 1 }}>
            <Typography variant="h4">Users</Typography>
            <Button size="small" variant="contained" startIcon={<Icon name="person_add" size={18} />} onClick={() => setCreating(true)}>Create user</Button>
          </Stack>
          <Box sx={{ overflowX: 'auto' }}>
            <Table size="small">
              <TableHead>
                <TableRow><TableCell>Name</TableCell><TableCell>Login</TableCell><TableCell>Email</TableCell><TableCell>Admin</TableCell><TableCell>Active</TableCell><TableCell align="right">Actions</TableCell></TableRow>
              </TableHead>
              <TableBody>
                {(users.data ?? []).map((u) => (
                  <TableRow key={u.id} sx={{ opacity: u.active ? 1 : 0.55 }}>
                    <TableCell>{u.displayName}</TableCell>
                    <TableCell>{u.roleName}</TableCell>
                    <TableCell>{u.email}</TableCell>
                    <TableCell><Checkbox size="small" checked={u.isAdmin} disabled={u.id === user.id} onChange={(e) => void run(() => call('admin_set_admin', { user: u.id, isAdmin: e.target.checked }))} slotProps={{ input: { 'aria-label': `${u.displayName} is an admin` } }} /></TableCell>
                    <TableCell><Switch size="small" checked={u.active} disabled={u.id === user.id} onChange={(e) => void run(() => call('admin_set_active', { user: u.id, active: e.target.checked }))} slotProps={{ input: { 'aria-label': `${u.displayName} is active` } }} /></TableCell>
                    <TableCell align="right"><Tooltip title="Reset password"><IconButton size="small" aria-label={`Reset password for ${u.displayName}`} onClick={() => { setResetPassword(''); setResetting(u); }}><Icon name="key" size={18} /></IconButton></Tooltip></TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </Box>
        </CardContent>
      </Card>

      <Card>
        <CardContent>
          <Typography variant="h4" sx={{ mb: 1.5 }}>Company settings</Typography>
          {current && (
            <Stack spacing={1} sx={{ maxWidth: 640 }}>
              <FieldRow label="Stalled after (days)">
                <TextField type="number" value={current.stalledDays} onChange={(e) => setForm({ ...current, stalledDays: Number(e.target.value) })} sx={{ width: 120 }} slotProps={{ htmlInput: { min: 1, 'aria-label': 'Stalled after, in days' } }} />
              </FieldRow>
              <FieldRow label="Link base address">
                <TextField fullWidth placeholder="https://links.yourcompany.example" value={current.linkBase} onChange={(e) => setForm({ ...current, linkBase: e.target.value })} helperText="When set, all new links use https://. See the deploy folder for the page to host." slotProps={{ htmlInput: { 'aria-label': 'Link base address' } }} />
              </FieldRow>
              <Box><Button variant="contained" disabled={!form} onClick={() => void run(() => call('settings_set', { settings: current }), 'Saved.').then((ok) => ok && setForm(null))}>Save</Button></Box>
            </Stack>
          )}
          <Divider sx={{ my: 2 }} />
          <Box sx={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 3 }}>
            {(['Category', 'Source'] as const).map((n) => (
              <PickEditor key={n} listName={n} project={null} values={(picks.data ?? []).filter((p) => p.listName === n)} refresh={() => void qc.invalidateQueries({ queryKey: ['picks', 'company'] })} />
            ))}
          </Box>
        </CardContent>
      </Card>

      <Card>
        <CardContent>
          <Typography variant="h4" sx={{ mb: 1 }}>Database connection profile</Typography>
          <List dense disablePadding>
            <ListItem disableGutters><Typography variant="body2">Server: {profile.data?.host}:{profile.data?.port}, database {profile.data?.database}, TLS {profile.data?.sslMode}</Typography></ListItem>
          </List>
          <Typography variant="caption" color="text.secondary">To move users to another server, give them a new profile file with these fields: host, port, database, sslMode.</Typography>
        </CardContent>
      </Card>

      <Dialog open={creating} onClose={() => setCreating(false)} fullWidth maxWidth="xs">
        <DialogTitle>Create user</DialogTitle>
        <DialogContent>
          <Stack spacing={1.5} sx={{ pt: 0.5 }}>
            <TextField label="Login name" helperText="Lower-case letters, digits, dots, hyphens, underscores" value={newUser.login} onChange={(e) => setNewUser({ ...newUser, login: e.target.value })} />
            <TextField label="Display name" value={newUser.name} onChange={(e) => setNewUser({ ...newUser, name: e.target.value })} />
            <TextField label="Email" value={newUser.email} onChange={(e) => setNewUser({ ...newUser, email: e.target.value })} />
            <TextField label="First password" type="password" autoComplete="new-password" helperText="At least 8 characters" value={newUser.password} onChange={(e) => setNewUser({ ...newUser, password: e.target.value })} />
            <FormControlLabel control={<Checkbox checked={newUser.isAdmin} onChange={(e) => setNewUser({ ...newUser, isAdmin: e.target.checked })} />} label={<Typography variant="body2">Admin</Typography>} />
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setCreating(false)}>Cancel</Button>
          <Button
            variant="contained"
            disabled={!newUser.login || !newUser.name || newUser.password.length < 8}
            onClick={() => void run(() => call('admin_create_user', newUser), 'User created.').then((ok) => {
              if (ok) {
                setCreating(false);
                setNewUser({ login: '', name: '', email: '', password: '', isAdmin: false });
              }
            })}
          >
            Create
          </Button>
        </DialogActions>
      </Dialog>

      <Dialog open={!!resetting} onClose={() => setResetting(null)} fullWidth maxWidth="xs">
        <DialogTitle>Reset password for {resetting?.displayName}</DialogTitle>
        <DialogContent>
          <TextField fullWidth sx={{ mt: 0.5 }} label="New password" type="password" autoComplete="new-password" value={resetPassword} onChange={(e) => setResetPassword(e.target.value)} helperText="At least 8 characters" />
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setResetting(null)}>Cancel</Button>
          <Button variant="contained" disabled={resetPassword.length < 8} onClick={() => { const u = resetting!; setResetting(null); void run(() => call('admin_reset_password', { user: u.id, password: resetPassword }), 'Password reset.'); }}>Reset</Button>
        </DialogActions>
      </Dialog>
    </Stack>
  );
}
