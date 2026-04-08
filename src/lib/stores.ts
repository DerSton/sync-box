import { writable } from 'svelte/store';

export interface ConnectionConfig {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  password: string;
}

export interface StorageStats {
  used_bytes: number;
  total_bytes: number;
  home_dir: string;
}

export interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
}

export interface UploadProgress {
  job_id: string;
  transferred_bytes: number;
  total_bytes: number;
  speed_bps: number;
  eta_seconds: number;
  status: JobStatus;
  current_file: string;
}

export type JobStatus =
  | 'Queued'
  | 'Running'
  | 'Completed'
  | { Failed: string };

export interface UploadJob {
  id: string;
  connection_id: string;
  connection_name: string;
  remote_path: string;
  files: string[];
  total_bytes: number;
  transferred_bytes: number;
  status: JobStatus;
  speed_bps: number;
  eta_seconds: number;
  started_at: number;
  finished_at: number | null;
  current_file: string;
}

export const savedConnections = writable<ConnectionConfig[]>([]);
export const activeConnectionId = writable<string | null>(null);
export const activeStats = writable<StorageStats | null>(null);
export const connectedIds = writable<Set<string>>(new Set());
export const jobs = writable<UploadJob[]>([]);

export function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(k)), sizes.length - 1);
  const val = bytes / Math.pow(k, i);
  // max 3 digits before decimal: >= 100 → no decimal, else 1 decimal
  const formatted = val >= 100 ? Math.round(val).toString() : val.toFixed(1);
  return `${formatted} ${sizes[i]}`;
}

export function formatSpeed(bps: number): string {
  return `${formatBytes(bps)}/s`;
}

export function formatEta(seconds: number): string {
  if (seconds <= 0 || !isFinite(seconds)) return '—';
  if (seconds < 60) return `${Math.round(seconds)}s`;
  if (seconds < 3600) return `${Math.round(seconds / 60)}m`;
  return `${(seconds / 3600).toFixed(1)}h`;
}

export function jobProgress(job: UploadJob): number {
  if (job.total_bytes === 0) return 0;
  return Math.min(100, (job.transferred_bytes / job.total_bytes) * 100);
}

export function isJobStatus(status: JobStatus, type: 'Queued' | 'Running' | 'Completed' | 'Failed'): boolean {
  if (type === 'Failed') return typeof status === 'object' && 'Failed' in status;
  return status === type;
}

export function getFailedMessage(status: JobStatus): string {
  if (typeof status === 'object' && 'Failed' in status) return status.Failed;
  return '';
}
