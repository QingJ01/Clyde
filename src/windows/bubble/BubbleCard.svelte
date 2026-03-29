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

  // Map tool names to short labels for the badge
  const TOOL_BADGES: Record<string, string> = {
    Bash: 'BASH', Read: 'READ', Write: 'WRITE', Edit: 'EDIT',
    Glob: 'GLOB', Grep: 'GREP', Agent: 'AGENT',
    WebFetch: 'WEB', WebSearch: 'WEB',
    NotebookEdit: 'NB',
  };
  const badge = TOOL_BADGES[toolName] ?? toolName.slice(0, 5).toUpperCase();

  function formatInput(input: Record<string, unknown>): string {
    const entries = Object.entries(input).slice(0, 3);
    return entries.map(([k, v]) => {
      const val = typeof v === 'string' ? v.slice(0, 120) : JSON.stringify(v).slice(0, 120);
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
    <span class="title">Permission Request</span>
    <span class="badge">{badge}</span>
  </div>

  {#if Object.keys(toolInput).length > 0}
    <div class="code-block">
      <pre>{formatInput(toolInput)}</pre>
    </div>
  {/if}

  <div class="actions">
    {#if !isElicitation}
      <button class="btn btn-allow" onclick={allow}>Allow</button>
    {/if}
    <button class="btn btn-deny" onclick={isElicitation ? goTerminal : deny}>
      {isElicitation ? 'Go to Terminal' : 'Deny'}
    </button>
  </div>

  {#if suggestions.length > 0}
    <div class="suggestions">
      {#each suggestions as sug}
        <button class="suggestion" onclick={() => applySuggestion(String(sug))}>
          {String(sug)}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .bubble {
    background: rgba(24, 24, 28, 0.92);
    backdrop-filter: blur(20px);
    -webkit-backdrop-filter: blur(20px);
    color: #e4e4e7;
    border-radius: 14px;
    padding: 16px;
    font-size: 13px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    box-shadow:
      0 8px 32px rgba(0, 0, 0, 0.5),
      0 1px 0 rgba(255, 255, 255, 0.05) inset;
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 12px;
  }

  .title {
    font-weight: 600;
    font-size: 13px;
    color: #fafafa;
    letter-spacing: -0.01em;
  }

  .badge {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.05em;
    background: rgba(199, 134, 80, 0.2);
    color: #e8a76a;
    padding: 3px 8px;
    border-radius: 6px;
    border: 1px solid rgba(199, 134, 80, 0.25);
  }

  .code-block {
    background: rgba(0, 0, 0, 0.35);
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: 8px;
    padding: 10px 12px;
    margin-bottom: 12px;
    overflow: hidden;
    max-height: 80px;
  }

  .code-block pre {
    font-family: 'Cascadia Code', 'Fira Code', 'SF Mono', 'Consolas', monospace;
    font-size: 11.5px;
    line-height: 1.5;
    color: #a1a1aa;
    white-space: pre-wrap;
    word-break: break-all;
    margin: 0;
  }

  .actions {
    display: flex;
    gap: 8px;
    margin-bottom: 0;
  }

  .btn {
    flex: 1;
    padding: 9px 0;
    border-radius: 8px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.15s ease;
    border: none;
    letter-spacing: -0.01em;
  }

  .btn-allow {
    background: linear-gradient(135deg, #c78650, #b5733f);
    color: #fff;
    box-shadow: 0 2px 8px rgba(199, 134, 80, 0.3);
  }
  .btn-allow:hover {
    background: linear-gradient(135deg, #d4935d, #c78650);
    box-shadow: 0 3px 12px rgba(199, 134, 80, 0.4);
    transform: translateY(-1px);
  }
  .btn-allow:active {
    transform: translateY(0);
    box-shadow: 0 1px 4px rgba(199, 134, 80, 0.3);
  }

  .btn-deny {
    background: rgba(255, 255, 255, 0.06);
    color: #a1a1aa;
    border: 1px solid rgba(255, 255, 255, 0.1);
  }
  .btn-deny:hover {
    background: rgba(255, 255, 255, 0.1);
    color: #d4d4d8;
    transform: translateY(-1px);
  }
  .btn-deny:active {
    transform: translateY(0);
  }

  .suggestions {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-top: 10px;
    padding-top: 10px;
    border-top: 1px solid rgba(255, 255, 255, 0.06);
  }

  .suggestion {
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 7px;
    padding: 7px 10px;
    color: #8b8b96;
    cursor: pointer;
    text-align: left;
    font-size: 11.5px;
    transition: all 0.15s ease;
  }
  .suggestion:hover {
    background: rgba(255, 255, 255, 0.08);
    color: #c4c4cc;
    border-color: rgba(255, 255, 255, 0.12);
  }
</style>
