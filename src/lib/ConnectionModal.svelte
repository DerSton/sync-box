<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { savedConnections, type ConnectionConfig } from './stores';
  import { v4 as uuidv4 } from 'uuid';

  let { open = $bindable(false), editConfig = null as ConnectionConfig | null } = $props();

  let name = $state('');
  let host = $state('');
  let port = $state(23);
  let username = $state('');
  let password = $state('');
  let error = $state('');
  let saving = $state(false);

  $effect(() => {
    if (open) {
      if (editConfig) {
        name = editConfig.name;
        host = editConfig.host;
        port = editConfig.port;
        username = editConfig.username;
        password = editConfig.password;
      } else {
        name = '';
        host = '';
        port = 23;
        username = '';
        password = '';
      }
      error = '';
    }
  });

  async function save() {
    if (!name || !host || !username || !password) {
      error = 'All fields are required';
      return;
    }
    saving = true;
    error = '';
    try {
      const config: ConnectionConfig = {
        id: editConfig?.id ?? uuidv4(),
        name,
        host,
        port,
        username,
        password,
      };
      await invoke('save_connection', { config });
      savedConnections.update(list => {
        const idx = list.findIndex(c => c.id === config.id);
        if (idx >= 0) { list[idx] = config; return [...list]; }
        return [...list, config];
      });
      open = false;
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }
</script>

{#if open}
  <!-- backdrop -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="fixed inset-0 bg-black/60 z-40 flex items-center justify-center"
    onclick={() => (open = false)}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <!-- modal -->
    <div
      class="card bg-surface-800 p-6 w-full max-w-md z-50 rounded-lg shadow-xl"
      onclick={(e) => e.stopPropagation()}
      role="presentation"
    >
      <h2 class="h3 mb-4">{editConfig ? 'Edit Connection' : 'New Connection'}</h2>

      <div class="space-y-3">
        <label class="label">
          <span class="label-text text-sm">Name</span>
          <input class="input" type="text" bind:value={name} placeholder="My Storage Box" />
        </label>
        <label class="label">
          <span class="label-text text-sm">Host</span>
          <input class="input" type="text" bind:value={host} placeholder="u123456.your-storagebox.de" />
        </label>
        <div class="grid grid-cols-3 gap-2">
          <label class="label col-span-2">
            <span class="label-text text-sm">Username</span>
            <input class="input" type="text" bind:value={username} placeholder="u123456" />
          </label>
          <label class="label">
            <span class="label-text text-sm">Port</span>
            <input class="input" type="number" bind:value={port} min="1" max="65535" />
          </label>
        </div>
        <label class="label">
          <span class="label-text text-sm">Password</span>
          <input class="input" type="password" bind:value={password} />
        </label>
      </div>

      {#if error}
        <p class="text-error-400 text-sm mt-2">{error}</p>
      {/if}

      <div class="flex gap-2 mt-5 justify-end">
        <button class="btn preset-tonal" onclick={() => (open = false)}>Cancel</button>
        <button class="btn preset-filled-primary-500" onclick={save} disabled={saving}>
          {saving ? 'Saving…' : 'Save'}
        </button>
      </div>
    </div>
  </div>
{/if}
