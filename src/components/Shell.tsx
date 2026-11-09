import { useState, type FormEvent, type ReactNode } from 'react';
import { Alert, Box, Button, CircularProgress, InputAdornment, Stack, TextField, Tooltip, Typography } from '@mui/material';
import { useLocation, useNavigate } from 'react-router-dom';
import { useApp } from '../context';
import { tokens } from '../theme';
import { Icon } from './common';

const NAV = [
  { to: '/', label: 'Dashboard', icon: 'dashboard', match: (p: string) => p === '/' },
  { to: '/a3', label: 'A3 Sheets', icon: 'description', match: (p: string) => p.startsWith('/a3') },
  { to: '/wall', label: 'Wall', icon: 'campaign', match: (p: string) => p.startsWith('/wall') },
  { to: '/settings', label: 'Settings', icon: 'settings', match: (p: string) => p.startsWith('/settings') },
];

export function Shell({ children }: { children: ReactNode }) {
  const { jobs, offline, openNewA3, openLink, user } = useApp();
  const nav = useNavigate();
  const { pathname } = useLocation();
  const [search, setSearch] = useState('');
  const jobList = Object.values(jobs);

  // The search box opens an A3 by number or pasted link, and otherwise searches the list.
  const onSearch = async (e: FormEvent) => {
    e.preventDefault();
    const text = search.trim();
    if (!text) return;
    if (await openLink(text)) {
      setSearch('');
      return;
    }
    nav(`/a3?search=${encodeURIComponent(text)}`);
  };

  return (
    <Box sx={{ height: '100%', display: 'grid', gridTemplateColumns: '84px minmax(0, 1fr)', gridTemplateRows: '60px minmax(0, 1fr)' }}>
      <Stack
        direction="row"
        sx={{ gridColumn: '1 / 3', alignItems: 'center', gap: 2, px: 2, bgcolor: '#fff', borderBottom: `1px solid ${tokens.line}` }}
      >
        <Typography variant="h3" sx={{ width: 96, flex: 'none', fontWeight: 500, color: tokens.primary }}>A3 Sheet</Typography>
        <Box component="form" onSubmit={onSearch} sx={{ flex: 1, maxWidth: 620 }}>
          <TextField
            fullWidth
            placeholder="Search, or paste an A3 number or link"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            aria-label="Search"
            slotProps={{
              input: {
                sx: { borderRadius: 999, bgcolor: tokens.fill },
                startAdornment: (
                  <InputAdornment position="start">
                    <Icon name="search" color={tokens.textSecondary} />
                  </InputAdornment>
                ),
              },
            }}
          />
        </Box>
        <Box sx={{ flex: 1 }} />
        {jobList.length > 0 && (
          <Tooltip title={jobList.map((j) => j.fileName).join(', ')}>
            <Stack direction="row" spacing={1} sx={{ alignItems: 'center', color: tokens.textSecondary }} role="status">
              <CircularProgress size={16} />
              <Typography variant="body2">Reading {jobList.length === 1 ? jobList[0].fileName : `${jobList.length} files`}</Typography>
            </Stack>
          </Tooltip>
        )}
        <Button variant="contained" startIcon={<Icon name="add" />} onClick={openNewA3}>New A3</Button>
        <Tooltip title={user.displayName}>
          <Box
            sx={{ width: 32, height: 32, flex: 'none', borderRadius: '50%', bgcolor: tokens.primary, color: '#fff', display: 'grid', placeItems: 'center', fontSize: 14, fontWeight: 500 }}
            aria-label={`Signed in as ${user.displayName}`}
          >
            {user.displayName.slice(0, 1).toUpperCase()}
          </Box>
        </Tooltip>
      </Stack>

      <Stack component="nav" aria-label="Main" sx={{ py: 1, alignItems: 'center', bgcolor: '#fff', borderRight: `1px solid ${tokens.line}` }} spacing={0.5}>
        {NAV.map((n) => {
          const active = n.match(pathname);
          return (
            <Box
              key={n.to}
              component="button"
              type="button"
              aria-current={active ? 'page' : undefined}
              onClick={() => nav(n.to)}
              sx={{
                all: 'unset',
                cursor: 'pointer',
                width: 72,
                py: 0.75,
                textAlign: 'center',
                borderRadius: 2,
                color: active ? tokens.primary : tokens.textSecondary,
                '&:hover': { bgcolor: tokens.fill },
                '&:focus-visible': { outline: `2px solid ${tokens.primary}` },
              }}
            >
              <Box sx={{ mx: 'auto', width: 48, py: 0.25, borderRadius: 999, bgcolor: active ? tokens.primaryLight : 'transparent' }}>
                <Icon name={n.icon} size={22} filled={active} />
              </Box>
              <Typography variant="caption" sx={{ display: 'block', mt: 0.25, fontWeight: active ? 500 : 400 }}>{n.label}</Typography>
            </Box>
          );
        })}
      </Stack>

      <Box component="main" sx={{ overflow: 'auto', minWidth: 0, position: 'relative' }}>
        {offline && (
          <Alert severity="warning" icon={<Icon name="cloud_off" />} sx={{ borderRadius: 0 }}>
            The connection to the database was lost. Your unsaved text is kept on this computer and saves when the connection returns.
          </Alert>
        )}
        {children}
      </Box>
    </Box>
  );
}
