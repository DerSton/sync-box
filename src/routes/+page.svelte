<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import {
    savedConnections,
    activeConnectionId,
    activeStats,
    connectedIds,
    jobs,
    formatBytes,
    type ConnectionConfig,
    type StorageStats,
    type UploadJob,
    type UploadProgress,
  } from '$lib/stores';
  import ConnectionModal from '$lib/ConnectionModal.svelte';
  import FolderBrowser from '$lib/FolderBrowser.svelte';
  import JobPanel from '$lib/JobPanel.svelte';

  let showModal = $state(false);
  let editConfig = $state<ConnectionConfig | null>(null);
  let connecting = $state<string | null>(null);
  let connectError = $state('');

  onMount(async () => {
    // Load saved connections
    try {
      const configs = await invoke<ConnectionConfig[]>('get_saved_connections');
      savedConnections.set(configs);
    } catch {}

    // Listen for upload progress events
    listen<UploadProgress>('upload-progress', (event) => {
      const p = event.payload;
      jobs.update((list: UploadJob[]) =>
        list.map((job: UploadJob) =>
          job.id === p.job_id
            ? {
                ...job,
                transferred_bytes: p.transferred_bytes,
                total_bytes: p.total_bytes,
                speed_bps: p.speed_bps,
                eta_seconds: p.eta_seconds,
                status: p.status,
                current_file: p.current_file,
              }
            : job
        )
      );
    });
  });

  async function connect(config: ConnectionConfig) {
    if ($connectedIds.has(config.id)) {
      activeConnectionId.set(config.id);
      await refreshStats(config.id);
      return;
    }
    connecting = config.id;
    connectError = '';
    try {
      const stats = await invoke<StorageStats>('connect_storage_box', { config });
      connectedIds.update((s: Set<string>) => new Set([...s, config.id]));
      activeConnectionId.set(config.id);
      activeStats.set(stats);
    } catch (e) {
      connectError = String(e);
    } finally {
      connecting = null;
    }
  }

  async function refreshStats(id: string) {
    try {
      const stats = await invoke<StorageStats>('get_storage_stats', { connectionId: id });
      activeStats.set(stats);
    } catch {}
  }

  async function disconnect(id: string) {
    await invoke('disconnect_storage_box', { connectionId: id }).catch(() => {});
    connectedIds.update((s: Set<string>) => { s.delete(id); return new Set(s); });
    if ($activeConnectionId === id) {
      activeConnectionId.set(null);
      activeStats.set(null);
    }
  }

  async function deleteConnection(config: ConnectionConfig) {
    await disconnect(config.id);
    await invoke('delete_connection', { id: config.id }).catch(() => {});
    savedConnections.update((list: ConnectionConfig[]) => list.filter((c: ConnectionConfig) => c.id !== config.id));
  }

  function openNewConnection() {
    editConfig = null;
    showModal = true;
  }

  function openEditConnection(config: ConnectionConfig) {
    editConfig = config;
    showModal = true;
  }

  let usedPercent = $derived(
    $activeStats && $activeStats.total_bytes > 0
      ? ($activeStats.used_bytes / $activeStats.total_bytes) * 100
      : 0
  );

  let activeConfig = $derived(
    $savedConnections.find((c: ConnectionConfig) => c.id === $activeConnectionId) ?? null
  );
</script>

<!-- Connection Modal -->
<ConnectionModal bind:open={showModal} {editConfig} />

<!-- App Layout -->
<div class="flex flex-col h-screen bg-surface-900 text-surface-50">

  <!-- Top Bar -->
  <header class="flex items-center gap-3 px-4 py-2 bg-surface-800 border-b border-surface-700 shrink-0">
    <!-- Connection Dropdown -->
    <div class="flex items-center gap-2 flex-1">
      <span class="text-sm font-semibold text-surface-300 shrink-0">Storage Box:</span>
      <select
        class="select text-sm bg-surface-700 border-surface-600 rounded px-2 py-1 max-w-xs"
        value={$activeConnectionId ?? ''}
        onchange={(e) => {
          const id = (e.target as HTMLSelectElement).value;
          if (!id) { activeConnectionId.set(null); activeStats.set(null); return; }
          const cfg = $savedConnections.find((c: ConnectionConfig) => c.id === id);
          if (cfg) connect(cfg);
        }}
      >
        <option value="">— Select connection —</option>
        {#each $savedConnections as cfg}
          <option value={cfg.id}>
            {cfg.name} {$connectedIds.has(cfg.id) ? '●' : '○'}
          </option>
        {/each}
      </select>

      {#if connecting}
        <span class="text-xs text-surface-400">Connecting…</span>
      {/if}
      {#if connectError}
        <span class="text-xs text-error-400 truncate max-w-xs">{connectError}</span>
      {/if}
    </div>

    <!-- Storage stats -->
    {#if $activeStats}
      <div class="flex items-center gap-2 shrink-0">
        <div class="text-xs text-surface-300">
          {#if $activeStats.total_bytes > 0}
            {formatBytes($activeStats.used_bytes)} / {formatBytes($activeStats.total_bytes)}
          {:else}
            {formatBytes($activeStats.used_bytes)} used
          {/if}
        </div>
        {#if $activeStats.total_bytes > 0}
          <div class="w-24 bg-surface-600 rounded-full h-1.5" title="{usedPercent.toFixed(1)}%">
            <div
              class="h-1.5 rounded-full transition-all {usedPercent > 90 ? 'bg-error-500' : usedPercent > 70 ? 'bg-warning-500' : 'bg-success-500'}"
              style="width: {usedPercent}%"
            ></div>
          </div>
        {/if}
      </div>
    {/if}

    <!-- Actions -->
    <div class="flex gap-2 shrink-0">
      {#if $activeConnectionId}
        <button
          class="btn btn-sm preset-tonal text-xs"
          onclick={() => activeConfig && openEditConnection(activeConfig)}
          title="Edit connection"
        >✎</button>
        <button
          class="btn btn-sm preset-tonal text-xs"
          onclick={() => $activeConnectionId && disconnect($activeConnectionId)}
          title="Disconnect"
        >✕</button>
      {/if}
      <button class="btn btn-sm preset-filled-primary-500 text-xs" onclick={openNewConnection}>
        + New
      </button>
    </div>
  </header>

  <!-- Main Content -->
  {#if !$activeConnectionId}
    <!-- No connection selected -->
    <div class="flex-1 flex flex-col items-center justify-center gap-4 text-center p-8">
      <p class="text-surface-400 text-lg">No storage box connected</p>
      {#if $savedConnections.length > 0}
        <p class="text-surface-500 text-sm">Select a connection from the dropdown above</p>
        <div class="space-y-2 w-full max-w-sm">
          {#each $savedConnections as cfg}
            <button
              class="w-full btn preset-tonal flex justify-between items-center"
              onclick={() => connect(cfg)}
              disabled={connecting === cfg.id}
            >
              <span>{cfg.name}</span>
              <span class="text-xs text-surface-400">{cfg.host}</span>
            </button>
          {/each}
        </div>
      {:else}
        <button class="btn preset-filled-primary-500" onclick={openNewConnection}>
          Add your first connection
        </button>
      {/if}
    </div>
  {:else}
    <!-- Two-panel layout -->
    <div class="flex flex-1 overflow-hidden">
      <!-- Left: Folder Browser -->
      <div class="w-2/5 border-r border-surface-700 overflow-hidden flex flex-col">
        <div class="px-3 py-1.5 bg-surface-800 border-b border-surface-700 text-xs font-semibold text-surface-400 uppercase">
          Browser
        </div>
        <div class="flex-1 overflow-hidden">
          <FolderBrowser connectionId={$activeConnectionId} />
        </div>
      </div>

      <!-- Right: Jobs -->
      <div class="flex-1 overflow-hidden flex flex-col">
        <div class="px-3 py-1.5 bg-surface-800 border-b border-surface-700 text-xs font-semibold text-surface-400 uppercase">
          Upload Jobs
        </div>
        <div class="flex-1 overflow-hidden">
          <JobPanel />
        </div>
      </div>
    </div>
  {/if}
</div>
