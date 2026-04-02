<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';

  type RequestField = {
    label: string;
    value: string;
    mono?: boolean;
    multiline?: boolean;
  };

  type SuggestionView = {
    title: string;
    subtitle: string;
  };

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
  const commandText = $derived(extractCommand(toolInput));
  const detailFields = $derived(buildDetailFields(toolInput, toolName));
  const extraFields = $derived(buildExtraFields(toolInput));

  const TOOL_BADGES: Record<string, string> = {
    Bash: 'BASH', Read: 'READ', Write: 'WRITE', Edit: 'EDIT',
    Glob: 'GLOB', Grep: 'GREP', Agent: 'AGENT',
    WebFetch: 'WEB', WebSearch: 'WEB',
    NotebookEdit: 'NB',
  };
  const badge = $derived(TOOL_BADGES[toolName] ?? toolName.slice(0, 5).toUpperCase());

  function stringifyValue(value: unknown): string {
    if (typeof value === 'string') return value.trim();
    if (typeof value === 'number' || typeof value === 'boolean') return String(value);
    if (value === null || value === undefined) return '';
    try {
      return JSON.stringify(value, null, 2);
    } catch {
      return String(value);
    }
  }

  function getString(input: Record<string, unknown>, keys: string[]): string {
    for (const key of keys) {
      const value = input[key];
      if (typeof value === 'string' && value.trim()) return value.trim();
    }
    return '';
  }

  function humanizeKey(key: string): string {
    return key
      .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
      .replace(/[_-]+/g, ' ')
      .replace(/\b\w/g, (match) => match.toUpperCase());
  }

  function normalizeShell(value: string): string {
    const cleaned = value.trim().split(/\s+/)[0].split('/').pop() ?? value.trim();
    return cleaned.replace(/\.exe$/i, '');
  }

  function inferShellFromCommand(command: string): string {
    const match = command.match(/^\s*(?:\/[^\s]+\/)?(bash|zsh|sh|fish|pwsh|powershell|cmd)(?:\.exe)?\b/i);
    if (!match) return '';
    return normalizeShell(match[1]);
  }

  function detectShell(input: Record<string, unknown>, tool: string): string {
    const explicit = getString(input, ['shell', 'shellType', 'shell_type', 'executable', 'program']);
    if (explicit) return normalizeShell(explicit);

    const command = extractCommand(input);
    const inferred = command ? inferShellFromCommand(command) : '';
    if (inferred) return `${inferred} (inferred)`;

    if (tool === 'Bash') return 'Default shell';
    return '';
  }

  function extractCommand(input: Record<string, unknown>): string {
    return getString(input, ['command', 'cmd', 'script', 'input']);
  }

  function buildDetailFields(input: Record<string, unknown>, tool: string): RequestField[] {
    const fields: RequestField[] = [{ label: 'Tool', value: tool || 'Unknown' }];

    const shell = detectShell(input, tool);
    if (shell) {
      fields.push({ label: 'Shell', value: shell, mono: true });
    }

    const cwd = getString(input, ['cwd', 'workingDirectory', 'working_directory', 'dir', 'path']);
    if (cwd) {
      fields.push({ label: 'Directory', value: cwd, mono: true });
    }

    const justification = getString(input, ['justification', 'reason', 'description']);
    if (justification) {
      fields.push({
        label: 'Why Claude asked',
        value: justification,
        multiline: justification.includes('\n') || justification.length > 96,
      });
    }

    const timeout = getString(input, ['timeout', 'timeoutMs', 'timeout_ms']);
    if (timeout) {
      fields.push({ label: 'Timeout', value: timeout, mono: true });
    }

    if (typeof input.background === 'boolean') {
      fields.push({ label: 'Background', value: input.background ? 'Yes' : 'No' });
    }

    return fields;
  }

  function buildExtraFields(input: Record<string, unknown>): RequestField[] {
    const handledKeys = new Set([
      'command',
      'cmd',
      'script',
      'input',
      'shell',
      'shellType',
      'shell_type',
      'executable',
      'program',
      'cwd',
      'workingDirectory',
      'working_directory',
      'dir',
      'path',
      'justification',
      'reason',
      'description',
      'timeout',
      'timeoutMs',
      'timeout_ms',
      'background',
    ]);

    return Object.entries(input)
      .filter(([key, value]) => !handledKeys.has(key) && value !== null && value !== undefined && stringifyValue(value))
      .slice(0, 6)
      .map(([key, value]) => {
        const rendered = stringifyValue(value);
        return {
          label: humanizeKey(key),
          value: rendered,
          mono: typeof value !== 'string' || /^[/~.-]|[A-Za-z0-9_-]+$/.test(rendered),
          multiline: rendered.includes('\n') || rendered.length > 88,
        };
      });
  }

  function requestTitle(): string {
    if (isElicitation) return 'Claude needs a reply in Terminal';
    if (toolName === 'Bash' && commandText) return 'Allow Claude to run this command?';
    return `Allow Claude to use ${toolName || 'this tool'}?`;
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
    return describeSuggestion(sug).title;
  }

  function describeSuggestion(sug: unknown): SuggestionView {
    if (typeof sug !== 'object' || sug === null) {
      return {
        title: String(sug),
        subtitle: 'Apply the suggested permission change',
      };
    }
    const obj = sug as Record<string, unknown>;
    const type = obj.type as string | undefined;
    const suggestionTool =
      (obj.toolName as string | undefined) ??
      (obj.tool_name as string | undefined) ??
      toolName;

    if (type === 'addRules' && obj.behavior === 'allow') {
      const rule = typeof obj.ruleContent === 'string' ? obj.ruleContent : '';
      return {
        title: `Always allow matching ${suggestionTool}`,
        subtitle: rule ? `Rule: ${rule}` : 'Create a persistent allow rule',
      };
    }
    if (type === 'setMode' && obj.mode === 'acceptEdits') {
      return {
        title: 'Switch to Accept Edits',
        subtitle: 'Future edit requests can be approved automatically',
      };
    }
    if (type === 'addRules') {
      const behavior = typeof obj.behavior === 'string' ? obj.behavior : 'allow';
      return {
        title: `${humanizeKey(behavior)} ${suggestionTool}`,
        subtitle: 'Apply the suggested rule',
      };
    }
    return {
      title: 'Apply suggested permission',
      subtitle: 'Update future permission handling',
    };
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

      <div class="intent">
        <div class="intent-title">{requestTitle()}</div>
        <div class="intent-copy">
          {#if isElicitation}
            Respond in the terminal session to continue this task.
          {:else}
            The main buttons only decide this request. Suggested actions below change future behavior.
          {/if}
        </div>
      </div>

      {#if hasInput}
        {#if commandText}
          <div class="section-label">Command</div>
          <div class="code-block command-block">
            <pre>{commandText}</pre>
          </div>
        {/if}

        {#if detailFields.length > 0}
          <div class="section-label">Execution Details</div>
          <div class="detail-list">
            {#each detailFields as field}
              <div class="detail-row">
                <span class="detail-label">{field.label}</span>
                {#if field.multiline}
                  <pre class:detail-code={field.mono} class="detail-multiline">{field.value}</pre>
                {:else}
                  <span class:detail-code={field.mono} class="detail-value">{field.value}</span>
                {/if}
              </div>
            {/each}
          </div>
        {/if}

        {#if extraFields.length > 0}
          <div class="section-label extra-label">Other Parameters</div>
          <div class="detail-list detail-list-compact">
            {#each extraFields as field}
              <div class="detail-row">
                <span class="detail-label">{field.label}</span>
                {#if field.multiline}
                  <pre class:detail-code={field.mono} class="detail-multiline">{field.value}</pre>
                {:else}
                  <span class:detail-code={field.mono} class="detail-value">{field.value}</span>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      {/if}

      <div class="actions">
      {#if isElicitation}
        <button class="btn btn-primary btn-stacked" onclick={goTerminal} aria-label="Go to terminal to respond">
          <span>Open Terminal</span>
          <small>Reply there to continue</small>
        </button>
        <button class="btn btn-secondary btn-stacked" onclick={deny} aria-label="Dismiss notification">
          <span>Dismiss</span>
          <small>Ignore this reminder</small>
        </button>
      {:else}
        <button class="btn btn-primary btn-stacked" onclick={allow} aria-label="Allow this request once">
          <span>Allow Once</span>
          <small>Approve only this request</small>
        </button>
        <button class="btn btn-secondary btn-stacked" onclick={deny} aria-label="Deny this request">
          <span>Deny</span>
          <small>Block this request</small>
        </button>
      {/if}
      </div>

    {#if suggestions.length > 0}
      <div class="section-label suggestions-label">Suggested Future Rules</div>
      <div class="suggestions">
        {#each suggestions as sug}
          {@const suggestion = describeSuggestion(sug)}
          <button class="suggestion" onclick={() => applySuggestion(sug)} aria-label="Apply suggestion: {suggestionLabel(sug)}">
            <span class="suggestion-title">{suggestion.title}</span>
            <span class="suggestion-subtitle">{suggestion.subtitle}</span>
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

  .intent {
    margin-bottom: 14px;
    padding: 12px 13px;
    border-radius: 12px;
    background: linear-gradient(180deg, rgba(216, 165, 108, 0.12), rgba(216, 165, 108, 0.04));
    border: 1px solid rgba(216, 165, 108, 0.12);
  }

  .intent-title {
    font-size: 13px;
    font-weight: 650;
    color: #f7ecdc;
    margin-bottom: 5px;
    letter-spacing: -0.015em;
  }

  .intent-copy {
    font-size: 11.5px;
    line-height: 1.45;
    color: #d1c5b4;
  }

  .code-block {
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.03), rgba(255, 255, 255, 0.014));
    border: 1px solid rgba(255, 255, 255, 0.075);
    border-radius: 12px;
    padding: 12px 13px;
    margin-bottom: 14px;
    overflow: hidden;
    max-height: 132px;
  }

  .command-block {
    border-color: rgba(216, 165, 108, 0.12);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.03);
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

  .detail-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-bottom: 14px;
  }

  .detail-list-compact {
    gap: 6px;
  }

  .detail-row {
    display: grid;
    grid-template-columns: 92px minmax(0, 1fr);
    gap: 10px;
    align-items: start;
    padding: 9px 11px;
    border-radius: 11px;
    background: rgba(255, 255, 255, 0.032);
    border: 1px solid rgba(255, 255, 255, 0.065);
  }

  .detail-label {
    padding-top: 1px;
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--copy-secondary);
  }

  .detail-value,
  .detail-multiline {
    min-width: 0;
    margin: 0;
    font-size: 12px;
    line-height: 1.45;
    color: #efe5d6;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .detail-code {
    font-family: 'Cascadia Code', 'Fira Code', 'SF Mono', 'Consolas', monospace;
    color: #d9c9b6;
  }

  .extra-label {
    margin-top: 2px;
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

  .btn-stacked {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 2px;
    padding: 9px 10px;
  }

  .btn-stacked span {
    line-height: 1.15;
  }

  .btn-stacked small {
    font-size: 10px;
    font-weight: 550;
    opacity: 0.78;
    line-height: 1.2;
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

  .suggestion-title {
    display: block;
    font-size: 12px;
    font-weight: 620;
    color: #f1e5d4;
    margin-bottom: 3px;
  }

  .suggestion-subtitle {
    display: block;
    font-size: 10.5px;
    line-height: 1.4;
    color: #bfb3a0;
  }

  .suggestion:hover {
    background: rgba(216, 165, 108, 0.1);
    border-color: rgba(216, 165, 108, 0.16);
    color: #f6e7d1;
    transform: translateY(-1px);
  }
</style>
