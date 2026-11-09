import { createContext, useContext } from 'react';
import type { AppUser, JobEvent } from './api';

export interface Toast {
  severity: 'success' | 'info' | 'warning' | 'error';
  message: string;
}

export interface AppCtx {
  user: AppUser;
  setUser: (u: AppUser) => void;
  jobs: Record<string, JobEvent>;
  offline: boolean;
  notify: (t: Toast) => void;
  openNewA3: () => void;
  openLink: (text: string) => Promise<boolean>;
  signOut: () => Promise<void>;
}

export const AppContext = createContext<AppCtx | null>(null);

export function useApp(): AppCtx {
  const c = useContext(AppContext);
  if (!c) throw new Error('AppContext is missing');
  return c;
}
