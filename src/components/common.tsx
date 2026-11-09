import type { ReactNode } from 'react';
import { Box, Chip, Stack, Tooltip, Typography } from '@mui/material';
import type { Letter, SheetRow } from '../api';
import { LETTERS, STEP_NAMES } from '../api';
import { tokens } from '../theme';

export function Icon({ name, size = 20, color, filled }: { name: string; size?: number; color?: string; filled?: boolean }) {
  return (
    <span
      className="ms"
      aria-hidden="true"
      style={{ fontSize: size, color, fontVariationSettings: filled ? "'FILL' 1" : undefined }}
    >
      {name}
    </span>
  );
}

/** The four-segment step strip. Colour carries status only, never step identity (section 6). */
export function StepStrip({ strip, size = 'small', current }: { strip: string; size?: 'small' | 'medium'; current?: Letter | null }) {
  const h = size === 'small' ? 18 : 24;
  const w = size === 'small' ? 22 : 32;
  return (
    <Stack direction="row" spacing={0.25} role="img" aria-label={LETTERS.map((l, i) => `${l} ${strip[i] === 'C' ? 'closed' : 'open'}`).join(', ')}>
      {LETTERS.map((l, i) => {
        const closed = strip[i] === 'C';
        return (
          <Tooltip key={l} title={`${l}. ${STEP_NAMES[l]}: ${closed ? 'Closed' : 'Open'}`}>
            <Box
              sx={{
                width: w,
                height: h,
                lineHeight: `${h - 2}px`,
                textAlign: 'center',
                fontSize: size === 'small' ? 11 : 13,
                fontWeight: 500,
                borderRadius: 0.5,
                border: `1px solid ${closed ? tokens.successFill : current === l ? tokens.primary : tokens.line}`,
                bgcolor: closed ? tokens.successFill : '#fff',
                color: closed ? '#fff' : current === l ? tokens.primary : tokens.textSecondary,
              }}
            >
              {l}
            </Box>
          </Tooltip>
        );
      })}
    </Stack>
  );
}

export function StatusChip({ status, withdrawn }: { status: string; withdrawn?: boolean }) {
  if (withdrawn) return <Chip size="small" label="Withdrawn" sx={{ bgcolor: '#F1F3F4', color: tokens.textSecondary }} />;
  const closed = status === 'Closed';
  return <Chip size="small" label={status} sx={{ bgcolor: closed ? tokens.successLight : tokens.primaryLight, color: closed ? tokens.successText : tokens.primaryText }} />;
}

export function HealthFlags({ row }: { row: Pick<SheetRow, 'overdue' | 'stalled' | 'reopened'> }) {
  return (
    <Stack direction="row" spacing={0.5} useFlexGap sx={{ flexWrap: 'wrap' }}>
      {row.overdue && <FlagChip label="Overdue" />}
      {row.stalled && <FlagChip label="Stalled" />}
      {row.reopened && <FlagChip label="Reopened" />}
    </Stack>
  );
}

function FlagChip({ label }: { label: string }) {
  return <Chip size="small" label={label} sx={{ height: 20, bgcolor: tokens.warningLight, color: tokens.warningText, fontSize: 12 }} />;
}

export function SeverityChip({ severity }: { severity: string | null | undefined }) {
  if (!severity) return <Typography variant="caption" color="text.secondary">Not set</Typography>;
  const colors: Record<string, [string, string]> = {
    High: [tokens.errorLight, tokens.errorText],
    Medium: [tokens.warningLight, tokens.warningText],
    Low: ['#F1F3F4', tokens.textSecondary],
  };
  const [bg, fg] = colors[severity] ?? colors.Low;
  return <Chip size="small" label={severity} sx={{ height: 20, bgcolor: bg, color: fg, fontSize: 12 }} />;
}

/** Click-to-source chip (FR-A3-05). */
export function SourceChip({ label, onClick, fromImage, removed }: { label: string; onClick?: () => void; fromImage?: boolean; removed?: boolean }) {
  return (
    <Chip
      size="small"
      icon={<Icon name="description" size={14} />}
      label={removed ? 'Source removed' : label}
      onClick={removed ? undefined : onClick}
      sx={{
        height: 22,
        maxWidth: 280,
        fontSize: 12,
        bgcolor: removed ? '#F1F3F4' : fromImage ? tokens.warningLight : tokens.primaryLight,
        color: removed ? tokens.textSecondary : fromImage ? tokens.warningText : tokens.primaryText,
        '& .MuiChip-icon': { color: 'inherit', ml: 0.75 },
      }}
    />
  );
}

/** One-click choices (principle 1). */
export function ChipPicker<T extends string>({
  options,
  value,
  onChange,
  disabled,
  ariaLabel,
}: {
  options: readonly T[];
  value: T | null | undefined;
  onChange: (v: T) => void;
  disabled?: boolean;
  ariaLabel?: string;
}) {
  return (
    <Stack direction="row" spacing={0.75} useFlexGap sx={{ flexWrap: 'wrap' }} role="radiogroup" aria-label={ariaLabel}>
      {options.map((o) => (
        <Chip
          key={o}
          label={o}
          size="small"
          role="radio"
          aria-checked={value === o}
          disabled={disabled}
          clickable={!disabled}
          onClick={() => onChange(o)}
          variant={value === o ? 'filled' : 'outlined'}
          color={value === o ? 'primary' : 'default'}
        />
      ))}
    </Stack>
  );
}

export function PageHeader({ title, subtitle, actions }: { title: ReactNode; subtitle?: ReactNode; actions?: ReactNode }) {
  return (
    <Stack direction="row" spacing={2} sx={{ mb: 2, alignItems: 'center', justifyContent: 'space-between', flexWrap: 'wrap' }} useFlexGap>
      <Box sx={{ minWidth: 0 }}>
        <Typography variant="h1">{title}</Typography>
        {subtitle && <Typography variant="body2" color="text.secondary">{subtitle}</Typography>}
      </Box>
      {actions && <Stack direction="row" spacing={1} useFlexGap sx={{ flexWrap: 'wrap' }}>{actions}</Stack>}
    </Stack>
  );
}

export function EmptyState({ icon, title, body, action }: { icon: string; title: string; body?: string; action?: ReactNode }) {
  return (
    <Stack spacing={1} sx={{ alignItems: 'center', textAlign: 'center', py: 6, px: 2 }}>
      <Icon name={icon} size={36} color={tokens.textSecondary} />
      <Typography variant="h3">{title}</Typography>
      {body && <Typography variant="body2" color="text.secondary" sx={{ maxWidth: 420 }}>{body}</Typography>}
      {action}
    </Stack>
  );
}

export function FieldRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <Box sx={{ display: 'grid', gridTemplateColumns: '180px minmax(0, 1fr)', gap: 2, alignItems: 'center', minHeight: 40 }}>
      <Typography variant="body2" color="text.secondary">{label}</Typography>
      <Box sx={{ minWidth: 0 }}>{children}</Box>
    </Box>
  );
}
