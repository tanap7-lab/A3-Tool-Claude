import React from 'react';
import ReactDOM from 'react-dom/client';
import { CssBaseline, ThemeProvider } from '@mui/material';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { HashRouter } from 'react-router-dom';
import '@fontsource/roboto/400.css';
import '@fontsource/roboto/500.css';
import 'material-symbols/outlined.css';
import './styles.css';
import { theme } from './theme';
import { App } from './App';

// Section 5: clients poll every 60 seconds and on window focus.
const queryClient = new QueryClient({
  defaultOptions: {
    queries: { refetchInterval: 60_000, refetchOnWindowFocus: true, staleTime: 5_000, retry: 1 },
  },
});

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <ThemeProvider theme={theme}>
      <CssBaseline />
      <QueryClientProvider client={queryClient}>
        <HashRouter>
          <App />
        </HashRouter>
      </QueryClientProvider>
    </ThemeProvider>
  </React.StrictMode>,
);
