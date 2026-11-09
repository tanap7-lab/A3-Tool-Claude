import { invoke } from '@tauri-apps/api/core';
import type { AppError } from './types';

export * from './types';

/** Every command fails with `{ code, message }`, where `message` is written for the user. */
export function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(command, args);
}

export function errorMessage(e: unknown): string {
  if (e && typeof e === 'object' && 'message' in e) return String((e as AppError).message);
  return String(e);
}

export function errorCode(e: unknown): string | null {
  if (e && typeof e === 'object' && 'code' in e) return String((e as AppError).code);
  return null;
}
