<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';

  let {
    id,
    windowKind = 'ApprovalRequest',
    toolName = '',
    toolInput = {},
    suggestions = [],
    sessionId,
    isElicitation = false,
    modeLabel = '',
    modeDescription = '',
  }: {
    id: string;
    windowKind?: string;
    toolName?: string;
    toolInput?: Record<string, unknown>;
    suggestions?: unknown[];
    sessionId: string;
    isElicitation?: boolean;
    modeLabel?: string;
    modeDescription?: string;
  } = $props();

  const isModeNotice = $derived(windowKind === 'ModeNotice');
  const hasInput = $derived(Object.keys(toolInput).length > 0);

  const TOOL_BADGES: Record<string, string> = {
    Bash: 'BASH', Read: 'READ', Write: 'WRITE', Edit: 'EDIT',
    Glob: 'GLOB', Grep: 'GREP', Agent: 'AGENT',
    WebFetch: 'WEB', WebSearch: 'WEB',
    NotebookEdit: 'NB',
  };
  const badge = $derived(TOOL_BADGES[toolName] ?? toolName.slice(0, 5).toUpperCase());

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

  async function applySuggestion(suggestion: unknown) {
    await invoke('resolve_permission', { id, decision: 'allow', selectedSuggestion: suggestion });
  }

  function suggestionLabel(sug: unknown): string {
    if (typeof sug !== 'object' || sug === null) return String(sug);
    const obj = sug as Record<string, unknown>;
    const type = obj.type as string | undefined;
    if (type === 'addRules' && obj.behavior === 'allow') {
      const tool = (obj as any).tool_name ?? toolName;
      return `Always allow ${tool}`;
    }
    if (type === 'setMode' && obj.mode === 'acceptEdits') {
      return 'Switch to Accept Edits';
    }
    if (type === 'addRules') {
      const tool = (obj as any).tool_name ?? toolName;
      const behavior = obj.behavior ?? 'allow';
      return `${behavior} ${tool}`;
    }
    return 'Apply suggested permission';
  }

  async function goTerminal() {
    await invoke('focus_terminal_for_session', { sessionId });
    await deny();
  }

  async function dismiss() {
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      await getCurrentWindow().close();
    } catch {}
  }
</script>

{#if isModeNotice}
  <div class="bubble">
    <div class="glow glow-mode"></div>
    <div class="header">
      <div class="header-copy">
        <span class="eyebrow">Claude</span>
        <span class="title">Mode Changed</span>
      </div>
      <span class="badge badge-mode">{modeLabel}</span>
    </div>

    <div class="code-block mode-block">
      <pre>{modeDescription}</pre>
    </div>

    <div class="actions">
      <button class="btn btn-primary" onclick={dismiss} aria-label="Dismiss">OK</button>
    </div>
  </div>
{:else}
  <div class="bubble">
    <div class="glow"></div>
    <div class="header">
      <div class="header-copy">
        <span class="eyebrow">{isElicitation ? 'Reply Needed' : 'Claude Wants Access'}</span>
        <span class="title">{isElicitation ? 'Terminal Response Required' : 'Permission Request'}</span>
      </div>
      <span class="badge">{badge}</span>
    </div>

    {#if hasInput}
      <div class="section-label">Request Payload</div>
      <div class="code-block">
        <pre>{formatInput(toolInput)}</pre>
      </div>
    {/if}

    <div class="actions">
      {#if isElicitation}
        <button class="btn btn-primary" onclick={goTerminal} aria-label="Go to terminal to respond">Open Terminal</button>
        <button class="btn btn-secondary" onclick={deny} aria-label="Dismiss notification">Dismiss</button>
      {:else}
        <button class="btn btn-primary" onclick={allow} aria-label="Allow permission">Allow</button>
        <button class="btn btn-secondary" onclick={deny} aria-label="Deny permission">Deny</button>
      {/if}
    </div>

    {#if suggestions.length > 0}
      <div class="section-label suggestions-label">Suggested Actions</div>
      <div class="suggestions">
        {#each suggestions as sug}
          <button class="suggestion" onclick={() => applySuggestion(sug)} aria-label="Apply suggestion: {suggestionLabel(sug)}">
            {suggestionLabel(sug)}
          </button>
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .bubble {
    --surface-top: rgba(18, 20, 28, 0.95);
    --surface-bottom: rgba(9, 11, 17, 0.92);
    --surface-border: rgba(216, 165, 108, 0.14);
    --surface-shadow: rgba(5, 7, 12, 0.42);
    --copy-primary: #f5f1e8;
    --copy-secondary: #bdb3a3;
    --accent: #d8a56c;
    --accent-strong: #f2c48f;
    position: relative;
    overflow: hidden;
    background:
      linear-gradient(180deg, rgba(255, 255, 255, 0.035), rgba(255, 255, 255, 0) 28%),
      linear-gradient(160deg, var(--surface-top), var(--surface-bottom));
    backdrop-filter: blur(26px) saturate(155%);
    -webkit-backdrop-filter: blur(26px) saturate(155%);
    color: var(--copy-primary);
    border-radius: 18px;
    padding: 18px;
    font-size: 13px;
    border: 1px solid var(--surface-border);
    box-shadow:
      0 22px 44px var(--surface-shadow),
      0 0 0 1px rgba(0, 0, 0, 0.24);
  }

  .glow {
    position: absolute;
    top: -34px;
    right: -30px;
    width: 128px;
    height: 128px;
    border-radius: 999px;
    background: radial-gradient(circle, rgba(216, 165, 108, 0.26), rgba(216, 165, 108, 0) 72%);
    pointer-events: none;
  }

  .glow-mode {
    background: radial-gradient(circle, rgba(106, 155, 232, 0.24), rgba(106, 155, 232, 0) 72%);
  }

  .header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 14px;
  }

  .header-copy {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }

  .eyebrow {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--copy-secondary);
  }

  .title {
    font-weight: 650;
    font-size: 15px;
    color: var(--copy-primary);
    letter-spacing: -0.02em;
  }

  .badge {
    flex-shrink: 0;
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.08em;
    background: rgba(216, 165, 108, 0.14);
    color: var(--accent-strong);
    padding: 5px 9px;
    border-radius: 999px;
    border: 1px solid rgba(216, 165, 108, 0.16);
    white-space: nowrap;
  }

  .badge-mode {
    background: rgba(106, 155, 232, 0.14);
    color: #93bcff;
    border-color: rgba(106, 155, 232, 0.16);
  }

  .section-label {
    margin: 0 0 8px;
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--copy-secondary);
  }

  .suggestions-label {
    margin-top: 14px;
  }

  .code-block {
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.03), rgba(255, 255, 255, 0.014));
    border: 1px solid rgba(255, 255, 255, 0.075);
    border-radius: 12px;
    padding: 12px 13px;
    margin-bottom: 14px;
    overflow: hidden;
    max-height: 96px;
  }

  .mode-block {
    margin-bottom: 16px;
  }

  .code-block pre {
    font-family: 'Cascadia Code', 'Fira Code', 'SF Mono', 'Consolas', monospace;
    font-size: 11.5px;
    line-height: 1.6;
    color: #cdc4b7;
    white-space: pre-wrap;
    word-break: break-all;
    margin: 0;
  }

  .actions {
    display: flex;
    gap: 10px;
    margin-bottom: 0;
  }

  .btn {
    flex: 1;
    min-height: 40px;
    padding: 10px 0;
    border-radius: 11px;
    font-size: 13px;
    font-weight: 650;
    cursor: pointer;
    transition: transform 0.15s ease, box-shadow 0.15s ease, background 0.15s ease, border-color 0.15s ease;
    border: 1px solid transparent;
    letter-spacing: -0.015em;
  }

  .btn-primary {
    background: linear-gradient(135deg, #dfa66d, #be7f4f);
    color: #1f1307;
    box-shadow: 0 10px 22px rgba(190, 127, 79, 0.24);
  }

  .btn-primary:hover {
    background: linear-gradient(135deg, #e8b27b, #ca8a59);
    box-shadow: 0 14px 26px rgba(190, 127, 79, 0.3);
    transform: translateY(-1px);
  }

  .btn-primary:active {
    transform: translateY(0);
    box-shadow: 0 6px 14px rgba(190, 127, 79, 0.2);
  }

  .btn-secondary {
    background: rgba(255, 255, 255, 0.045);
    color: #ddd3c4;
    border-color: rgba(255, 255, 255, 0.08);
  }

  .btn-secondary:hover {
    background: rgba(255, 255, 255, 0.085);
    border-color: rgba(255, 255, 255, 0.13);
    color: #f0e7d7;
    transform: translateY(-1px);
  }

  .btn-secondary:active {
    transform: translateY(0);
  }

  .btn:focus-visible,
  .suggestion:focus-visible {
    outline: 2px solid rgba(216, 165, 108, 0.72);
    outline-offset: 2px;
  }

  .suggestions {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .suggestion {
    width: 100%;
    padding: 10px 12px;
    border-radius: 11px;
    border: 1px solid rgba(255, 255, 255, 0.065);
    background: rgba(255, 255, 255, 0.045);
    color: #ddd3c4;
    text-align: left;
    font-size: 12px;
    line-height: 1.4;
    cursor: pointer;
    transition: transform 0.15s ease, background 0.15s ease, color 0.15s ease, border-color 0.15s ease;
  }

  .suggestion:hover {
    background: rgba(216, 165, 108, 0.1);
    border-color: rgba(216, 165, 108, 0.16);
    color: #f6e7d1;
    transform: translateY(-1px);
  }
</style>
