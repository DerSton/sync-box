<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { jobs, formatBytes, formatSpeed, formatEta, jobProgress, isJobStatus, getFailedMessage } from './stores';
  import type { UploadJob } from './stores';

  let activeJobs = $derived($jobs.filter(j => isJobStatus(j.status, 'Running') || isJobStatus(j.status, 'Queued')));
  let history = $derived($jobs.filter(j => isJobStatus(j.status, 'Completed') || isJobStatus(j.status, 'Failed')).slice(0, 20));

  function statusLabel(job: UploadJob): string {
    if (isJobStatus(job.status, 'Queued')) return 'Queued';
    if (isJobStatus(job.status, 'Running')) return 'Running';
    if (isJobStatus(job.status, 'Completed')) return 'Done';
    if (isJobStatus(job.status, 'Failed')) return getFailedMessage(job.status) === 'Cancelled' ? 'Cancelled' : 'Failed';
    return '?';
  }

  function statusColor(job: UploadJob): string {
    if (isJobStatus(job.status, 'Completed')) return 'text-success-400';
    if (isJobStatus(job.status, 'Failed')) return getFailedMessage(job.status) === 'Cancelled' ? 'text-warning-400' : 'text-error-400';
    if (isJobStatus(job.status, 'Running')) return 'text-primary-400';
    return 'text-surface-400';
  }

  async function cancelJob(job: UploadJob) {
    await invoke('cancel_job', { jobId: job.id }).catch(() => {});
    jobs.update(list => list.map(j =>
      j.id === job.id ? { ...j, status: { Failed: 'Cancelled' } } : j
    ));
  }
</script>

<div class="flex flex-col h-full overflow-hidden">
  <!-- Active Jobs -->
  <div class="p-3 border-b border-surface-600">
    <h3 class="text-xs font-semibold uppercase text-surface-400 mb-2">Active Jobs</h3>
    {#if activeJobs.length === 0}
      <p class="text-surface-500 text-sm">No active uploads</p>
    {:else}
      <div class="space-y-3">
        {#each activeJobs as job (job.id)}
          <div class="bg-surface-700 rounded p-2">
            <div class="flex items-center justify-between mb-1">
              <span class="text-xs truncate flex-1 text-surface-200">
                {job.connection_name} → {job.remote_path}
              </span>
              <div class="flex items-center gap-2 ml-2 shrink-0">
                <span class="text-xs {statusColor(job)}">{statusLabel(job)}</span>
                <button
                  class="text-xs text-error-400 hover:text-error-300 px-1"
                  onclick={() => cancelJob(job)}
                  title="Cancel"
                >✕</button>
              </div>
            </div>

            {#if job.current_file}
              <p class="text-xs text-surface-400 truncate mb-1">↑ {job.current_file}</p>
            {/if}

            <div class="w-full bg-surface-600 rounded-full h-1.5 mb-1">
              <div
                class="bg-primary-500 h-1.5 rounded-full transition-all"
                style="width: {jobProgress(job)}%"
              ></div>
            </div>

            <div class="flex justify-between text-xs text-surface-400">
              <span>{formatBytes(job.transferred_bytes)} / {formatBytes(job.total_bytes)}</span>
              <span>
                {#if isJobStatus(job.status, 'Running')}
                  {formatSpeed(job.speed_bps)} · ETA {formatEta(job.eta_seconds)}
                {:else}
                  {Math.round(jobProgress(job))}%
                {/if}
              </span>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>

  <!-- History -->
  <div class="flex-1 overflow-y-auto p-3">
    <h3 class="text-xs font-semibold uppercase text-surface-400 mb-2">History</h3>
    {#if history.length === 0}
      <p class="text-surface-500 text-sm">No completed jobs</p>
    {:else}
      <div class="space-y-1">
        {#each history as job (job.id)}
          <div class="flex items-center gap-2 text-xs py-1 border-b border-surface-700">
            <span class="{statusColor(job)} w-14 shrink-0">{statusLabel(job)}</span>
            <span class="truncate flex-1 text-surface-300">
              {job.connection_name} → {job.remote_path}
            </span>
            <span class="text-surface-400 shrink-0">{formatBytes(job.total_bytes)}</span>
          </div>
          {#if isJobStatus(job.status, 'Failed') && getFailedMessage(job.status) !== 'Cancelled'}
            <p class="text-error-400 text-xs pl-14 pb-1">{getFailedMessage(job.status)}</p>
          {/if}
        {/each}
      </div>
    {/if}
  </div>
</div>
