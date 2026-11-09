import { createTheme } from '@mui/material/styles';

/** Section 6 design tokens. */
export const tokens = {
  primary: '#1A73E8',
  primaryDark: '#1765CC',
  primaryLight: '#E8F0FE',
  success: '#1E8E3E',
  successLight: '#E6F4EA',
  warning: '#F9AB00',
  warningLight: '#FEF7E0',
  error: '#D93025',
  errorLight: '#FCE8E6',
  // Section 15 asks for 4.5:1 text contrast, which the section 6 colours miss on their pale fills
  // and under white text. These darker shades of the same hues carry text. See theme.test.ts.
  primaryText: '#1765CC',
  successText: '#137333',
  successFill: '#188038',
  warningText: '#8F4E00',
  errorText: '#C5221F',
  text: '#202124',
  textSecondary: '#5F6368',
  line: '#DADCE0',
  fill: '#F8F9FA',
};

export const theme = createTheme({
  palette: {
    mode: 'light',
    primary: { main: tokens.primary, dark: tokens.primaryDark, light: tokens.primaryLight },
    success: { main: tokens.success, light: tokens.successLight },
    warning: { main: tokens.warning, dark: tokens.warningText, light: tokens.warningLight },
    error: { main: tokens.error, light: tokens.errorLight },
    text: { primary: tokens.text, secondary: tokens.textSecondary },
    divider: tokens.line,
    background: { default: tokens.fill, paper: '#FFFFFF' },
  },
  shape: { borderRadius: 8 },
  typography: {
    fontFamily: 'Roboto, "Segoe UI", Arial, sans-serif',
    fontSize: 14,
    h1: { fontSize: 22, fontWeight: 400, lineHeight: 1.3 },
    h2: { fontSize: 18, fontWeight: 500, lineHeight: 1.35 },
    h3: { fontSize: 16, fontWeight: 500, lineHeight: 1.4 },
    h4: { fontSize: 14, fontWeight: 500, lineHeight: 1.4 },
    body1: { fontSize: 14, lineHeight: 1.5 },
    body2: { fontSize: 13, lineHeight: 1.45 },
    caption: { fontSize: 12, lineHeight: 1.4 },
    button: { fontSize: 14, fontWeight: 500, textTransform: 'none' },
  },
  components: {
    MuiCssBaseline: {
      styleOverrides: { ':root': { colorScheme: 'light' }, body: { backgroundColor: tokens.fill } },
    },
    MuiButton: {
      defaultProps: { disableElevation: true },
      styleOverrides: { root: { borderRadius: 999, paddingInline: 16, minHeight: 36 }, sizeSmall: { minHeight: 30, paddingInline: 12, fontSize: 13 } },
    },
    MuiAlert: {
      styleOverrides: {
        root: {
          variants: [
            { props: { variant: 'filled', severity: 'success' }, style: { backgroundColor: tokens.successFill, color: '#fff' } },
            { props: { variant: 'filled', severity: 'warning' }, style: { backgroundColor: tokens.warningText, color: '#fff' } },
          ],
        },
      },
    },
    MuiChip: { styleOverrides: { root: { borderRadius: 999, fontWeight: 400 } } },
    MuiCard: { defaultProps: { variant: 'outlined' }, styleOverrides: { root: { borderRadius: 8, borderColor: tokens.line } } },
    MuiPaper: { styleOverrides: { outlined: { borderColor: tokens.line } } },
    MuiDialog: { styleOverrides: { paper: { borderRadius: 12 } } },
    MuiTab: { styleOverrides: { root: { minHeight: 44, textTransform: 'none', fontSize: 14, fontWeight: 500 } } },
    MuiTabs: { styleOverrides: { root: { minHeight: 44 } } },
    MuiTableCell: { styleOverrides: { root: { borderColor: tokens.line, fontSize: 13 }, head: { color: tokens.textSecondary, fontWeight: 500, whiteSpace: 'nowrap' } } },
    MuiTextField: { defaultProps: { size: 'small' } },
    MuiSelect: { defaultProps: { size: 'small' } },
    MuiTooltip: { defaultProps: { arrow: true, enterDelay: 400 } },
  },
});
