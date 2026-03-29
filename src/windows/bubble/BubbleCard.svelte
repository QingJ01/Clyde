<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';

  let {
    id,
    toolName,
    toolInput = {},
    suggestions = [],
    sessionId,
    isElicitation = false,
  }: {
    id: string;
    toolName: string;
    toolInput?: Record<string, unknown>;
    suggestions?: unknown[];
    sessionId: string;
    isElicitation?: boolean;
  } = $props();

  function formatInput(input: Record<string, unknown>): string {
    const entries = Object.entries(input).slice(0, 3);
    return entries.map(([k, v]) => {
      const val = typeof v === 'string' ? v.slice(0, 100) : JSON.stringify(v).slice(0, 100);
      return `${k}: ${val}`;
    }).join('\n');
  }

  async function allow() {
    await invoke('resolve_permission', { id, decision: 'allow' });
  }

  async function deny() {
    await invoke('resolve_permission', { id, decision: 'deny' });
  }

  async function applySuggestion(suggestion: string) {
    await invoke('resolve_permission', { id, decision: 'allow', suggestion });
  }

  async function goTerminal() {
    await invoke('focus_terminal_for_session', { sessionId });
    await deny();
  }
</script>

<div class="bubble">
  <div class="header">
    <span class="tool-name">{toolName}</span>
    {#if isElicitation}
      <span class="badge">Question</span>
    {/if}
  </div>

  {#if Object.keys(toolInput).length > 0}
    <pre class="input-preview">{formatInput(toolInput)}</pre>
  {/if}

  {#if suggestions.length > 0}
    <div class="suggestions">
      {#each suggestions as sug}
        <button class="suggestion" onclick={() => applySuggestion(String(sug))}>{String(sug)}</button>
      {/each}
    </div>
  {/if}

  <div class="actions">
    {#if !isElicitation}
      <button class="btn-allow" onclick={allow}>Allow</button>
    {/if}
    <button class="btn-deny" onclick={isElicitation ? goTerminal : deny}>
      {isElicitation ? 'Go to Terminal' : 'Deny'}
    </button>
  </div>
</div>

<style>
  .bubble {
    background: #1e1e1e;
    color: #e0e0e0;
    border-radius: 10px;
    padding: 14px;
    font-size: 13px;
    border: 1px solid #333;
    box-shadow: 0 4px 20px rgba(0,0,0,0.4);
  }
  .header { display: flex; align-items: center; gap: 8px; margin-bottom: 10px; }
  .tool-name { font-weight: 600; font-size: 14px; color: #fff; }
  .badge { font-size: 11px; background: #3a3; padding: 2px 6px; border-radius: 4px; }
  .input-preview {
    background: #111; border-radius: 6px; padding: 8px;
    font-size: 11px; color: #aaa; margin-bottom: 10px;
    white-space: pre-wrap; word-break: break-all; max-height: 80px; overflow: hidden;
  }
  .suggestions { display: flex; flex-direction: column; gap: 4px; margin-bottom: 10px; }
  .suggestion {
    background: #2a2a2a; border: 1px solid #444; border-radius: 6px;
    padding: 6px 10px; color: #ccc; cursor: pointer; text-align: left; font-size: 12px;
  }
  .suggestion:hover { background: #333; }
  .actions { display: flex; gap: 8px; }
  .btn-allow {
    flex: 1; background: #2563eb; color: #fff; border: none;
    border-radius: 6px; padding: 8px; cursor: pointer; font-weight: 600;
  }
  .btn-allow:hover { background: #1d4ed8; }
  .btn-deny {
    flex: 1; background: #2a2a2a; color: #e0e0e0; border: 1px solid #444;
    border-radius: 6px; padding: 8px; cursor: pointer;
  }
  .btn-deny:hover { background: #333; }
</style>
