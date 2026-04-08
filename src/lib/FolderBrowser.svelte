<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { open as dialogOpen } from '@tauri-apps/plugin-dialog';
  import { activeStats, jobs, formatBytes, type FileEntry } from './stores';
  import { get } from 'svelte/store';

  let { connectionId }: { connectionId: string } = $props();

  let currentPath = $state('');
  let entries = $state<FileEntry[]>([]);
  let dirSizes = $state<Record<string, number>>({});
  let loading = $state(false);
  let error = $state('');
  let newFolderName = $state('');
  let showNewFolder = $state(false);
  let creatingFolder = $state(false);

  $effect(() => {
    if (connectionId) {
      const homeDir = get(activeStats)?.home_dir ?? '.';
      loadDir(homeDir);
    }
  });

  async function loadDir(path: string) {
    loading = true;
    error = '';
    dirSizes = {};
    try {
      entries = await invoke<FileEntry[]>('list_directory', { connectionId, path });
      currentPath = path;
      // Load dir sizes in background (non-blocking)
      invoke<Record<string, number>>('get_dir_sizes', { connectionId, path })
        .then(sizes => { dirSizes = sizes; })
        .catch(() => {});
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function navigate(entry: FileEntry) {
    if (entry.is_dir) loadDir(entry.path);
  }

  function goUp() {
    const homeDir = get(activeStats)?.home_dir ?? '.';
    if (currentPath === homeDir || currentPath === '/') return;
    const parts = currentPath.replace(/\/$/, '').split('/');
    parts.pop();
    loadDir(parts.join('/') || homeDir);
  }

  function isAtRoot(): boolean {
    const homeDir = get(activeStats)?.home_dir ?? '.';
    return currentPath === homeDir || currentPath === '/';
  }

  async function createFolder() {
    if (!newFolderName.trim()) return;
    creatingFolder = true;
    try {
      const newPath = `${currentPath.replace(/\/$/, '')}/${newFolderName.trim()}`;
      await invoke('create_directory', { connectionId, path: newPath });
      newFolderName = '';
      showNewFolder = false;
      await loadDir(currentPath);
    } catch (e) {
      error = String(e);
    } finally {
      creatingFolder = false;
    }
  }

  async function uploadHere() {
    try {
      const selected = await dialogOpen({ multiple: true, directory: false });
      if (!selected) return;
      const paths = Array.isArray(selected) ? selected : [selected];
      if (paths.length === 0) return;

      const jobId: string = await invoke('start_upload_job', {
        connectionId,
        localPaths: paths,
        remotePath: currentPath,
      });

      jobs.update(list => [{
        id: jobId,
        connection_id: connectionId,
        connection_name: '',
        remote_path: currentPath,
        files: paths,
        total_bytes: 0,
        transferred_bytes: 0,
        status: 'Queued' as const,
        speed_bps: 0,
        eta_seconds: 0,
        started_at: Date.now() / 1000,
        finished_at: null,
        current_file: '',
      }, ...list]);
    } catch (e) {
      error = String(e);
    }
  }

  function entrySize(entry: FileEntry): string {
    if (entry.is_dir) {
      const s = dirSizes[entry.name];
      return s != null ? formatBytes(s) : '…';
    }
    return formatBytes(entry.size);
  }
</script>

<div class="flex flex-col h-full">
  <!-- Path bar -->
  <div class="flex items-center gap-2 p-2 border-b border-surface-600">
    <button
      class="btn btn-sm preset-tonal"
      onclick={goUp}
      disabled={isAtRoot()}
      title="Go up"
    >↑</button>
    <span class="font-mono text-sm text-surface-200 truncate flex-1">{currentPath}</span>
    <button class="btn btn-sm preset-tonal" onclick={() => loadDir(currentPath)} title="Refresh">↻</button>
  </div>

  {#if error}
    <div class="text-error-400 text-xs p-2">{error}</div>
  {/if}

  <!-- Entries -->
  <div class="flex-1 overflow-y-auto">
    {#if loading}
      <div class="p-4 text-surface-400 text-sm text-center">Loading…</div>
    {:else if entries.length === 0}
      <div class="p-4 text-surface-400 text-sm text-center">Empty folder</div>
    {:else}
      {#each entries as entry}
        <button
          class="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-surface-700 transition-colors"
          onclick={() => navigate(entry)}
        >
          <span class="text-base">{entry.is_dir ? '📁' : '📄'}</span>
          <span class="text-sm flex-1 truncate">{entry.name}</span>
          <span class="text-xs text-surface-400 shrink-0 w-16 text-right">{entrySize(entry)}</span>
        </button>
      {/each}
    {/if}
  </div>

  <!-- Actions -->
  <div class="border-t border-surface-600 p-2">
    {#if showNewFolder}
      <div class="flex gap-1">
        <input
          class="input input-sm flex-1 text-sm"
          type="text"
          bind:value={newFolderName}
          placeholder="Folder name"
          onkeydown={(e) => e.key === 'Enter' && createFolder()}
        />
        <button class="btn btn-sm preset-filled-primary-500" onclick={createFolder} disabled={creatingFolder}>
          {creatingFolder ? '…' : 'Create'}
        </button>
        <button class="btn btn-sm preset-tonal" onclick={() => (showNewFolder = false)}>✕</button>
      </div>
    {:else}
      <div class="flex gap-2">
        <button class="btn btn-sm preset-tonal flex-1" onclick={() => (showNewFolder = true)}>+ Folder</button>
        <button class="btn btn-sm preset-filled-primary-500 flex-1" onclick={uploadHere}>↑ Upload here</button>
      </div>
    {/if}
  </div>
</div>
