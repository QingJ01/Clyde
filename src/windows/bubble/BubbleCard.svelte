<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';

  type SuggestionView = {
    title: string;
    subtitle: string;
  };

  type JsonSchema = {
    type?: string;
    title?: string;
    description?: string;
    enum?: unknown[];
    oneOf?: unknown[];
    anyOf?: unknown[];
    const?: unknown;
    default?: unknown;
    format?: string;
    maxLength?: number;
    properties?: Record<string, unknown>;
    required?: string[];
  };

  type ElicitationField = {
    key: string;
    schema: JsonSchema;
    required: boolean;
  };

  type ChoiceOption = {
    value: unknown;
    label: string;
    description: string;
  };

  const ROOT_FIELD_KEY = '__root__';

  let {
    id,
    windowKind = 'ApprovalRequest',
    toolName = '',
    toolInput = {},
    suggestions = [],
    sessionId,
    agentLabel = 'Claude',
    sessionSummary = '',
    sessionProject = '',
    sessionShortId = '',
    isElicitation = false,
    elicitationMessage = '',
    elicitationSchema = null,
    elicitationMode = '',
    elicitationUrl = '',
    elicitationServerName = '',
    modeLabel = '',
    modeDescription = '',
  }: {
    id: string;
    windowKind?: string;
    toolName?: string;
    toolInput?: Record<string, unknown>;
    suggestions?: unknown[];
    sessionId: string;
    agentLabel?: string;
    sessionSummary?: string;
    sessionProject?: string;
    sessionShortId?: string;
    isElicitation?: boolean;
    elicitationMessage?: string;
    elicitationSchema?: unknown;
    elicitationMode?: string;
    elicitationUrl?: string;
    elicitationServerName?: string;
    modeLabel?: string;
    modeDescription?: string;
  } = $props();

  let elicitationValues = $state<Record<string, unknown>>({});

  const isModeNotice = $derived(windowKind === 'ModeNotice');
  const commandText = $derived(extractCommand(toolInput));
  const headerMeta = $derived([sessionProject, sessionShortId].filter(Boolean).join(' · '));
  const shellName = $derived(detectShell(toolInput, toolName));
  const cwdLabel = $derived(
    compactPath(getString(toolInput, ['cwd', 'workingDirectory', 'working_directory', 'dir', 'path'])),
  );
  const reasonText = $derived(
    compactReason(getString(toolInput, ['justification', 'reason', 'description'])),
  );
  const normalizedElicitationSchema = $derived(normalizeSchema(elicitationSchema));
  const elicitationFields = $derived(extractElicitationFields(normalizedElicitationSchema));
  const singleChoiceField = $derived(getSingleChoiceField(elicitationFields));
  const singleChoiceOptions = $derived(
    singleChoiceField ? extractChoiceOptions(singleChoiceField.schema) : [],
  );
  const badge = $derived(resolveBadge(toolName, isElicitation));
  const canSubmitElicitation = $derived(checkElicitationValidity(elicitationFields, elicitationValues));

  const TOOL_BADGES: Record<string, string> = {
    Bash: 'BASH',
    Read: 'READ',
    Write: 'WRITE',
    Edit: 'EDIT',
    Glob: 'GLOB',
    Grep: 'GREP',
    Agent: 'AGENT',
    WebFetch: 'WEB',
    WebSearch: 'WEB',
    NotebookEdit: 'NB',
  };

  $effect(() => {
    if (!isElicitation) return;
    elicitationValues = buildInitialValues(elicitationFields);
  });

  function resolveBadge(tool: string, elicitation: boolean): string {
    if (elicitation) return 'ASK';
    if (!tool) return 'TASK';
    return TOOL_BADGES[tool] ?? tool.slice(0, 5).toUpperCase();
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
    const match = command.match(
      /^\s*(?:\/[^\s]+\/)?(bash|zsh|sh|fish|pwsh|powershell|cmd)(?:\.exe)?\b/i,
    );
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

  function compactPath(path: string): string {
    if (!path) return '';
    const trimmed = path.replace(/\/+$/, '');
    const name = trimmed.split('/').pop() ?? trimmed;
    return name || trimmed;
  }

  function compactReason(reason: string): string {
    if (!reason) return '';
    const singleLine = reason.replace(/\s+/g, ' ').trim();
    return singleLine.length > 88 ? `${singleLine.slice(0, 88).trimEnd()}...` : singleLine;
  }

  function normalizeSchema(value: unknown): JsonSchema | null {
    if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
    return value as JsonSchema;
  }

  function extractElicitationFields(schema: JsonSchema | null): ElicitationField[] {
    if (!schema) return [];

    if (schema.properties && typeof schema.properties === 'object') {
      const required = new Set(schema.required ?? []);
      return Object.entries(schema.properties).map(([key, value]) => ({
        key,
        schema: normalizeSchema(value) ?? {},
        required: required.has(key),
      }));
    }

    return [{ key: ROOT_FIELD_KEY, schema, required: true }];
  }

  function extractChoiceOptions(schema: JsonSchema): ChoiceOption[] {
    if (Array.isArray(schema.enum)) {
      return schema.enum.map((value) => ({
        value,
        label: formatChoiceValue(value),
        description: '',
      }));
    }

    const variants = Array.isArray(schema.oneOf)
      ? schema.oneOf
      : Array.isArray(schema.anyOf)
        ? schema.anyOf
        : [];

    const options: ChoiceOption[] = [];
    for (const item of variants) {
      const option = normalizeSchema(item);
      if (!option) continue;
      const value =
        option.const !== undefined
          ? option.const
          : Array.isArray(option.enum) && option.enum.length === 1
            ? option.enum[0]
            : undefined;
      if (value === undefined) continue;
      options.push({
        value,
        label: option.title || formatChoiceValue(value),
        description: option.description || '',
      });
    }

    return options;
  }

  function getSingleChoiceField(fields: ElicitationField[]): ElicitationField | null {
    if (fields.length !== 1) return null;
    return extractChoiceOptions(fields[0].schema).length > 0 ? fields[0] : null;
  }

  function defaultValueForField(field: ElicitationField): unknown {
    if (field.schema.default !== undefined) return field.schema.default;

    const options = extractChoiceOptions(field.schema);
    if (options.length > 0) return options[0].value;

    if (field.schema.type === 'boolean') return false;
    if (field.schema.type === 'number' || field.schema.type === 'integer') return '';
    return '';
  }

  function buildInitialValues(fields: ElicitationField[]): Record<string, unknown> {
    const next: Record<string, unknown> = {};
    for (const field of fields) {
      next[field.key] = defaultValueForField(field);
    }
    return next;
  }

  function fieldTitle(field: ElicitationField): string {
    if (field.key === ROOT_FIELD_KEY) return field.schema.title || 'Response';
    return field.schema.title || humanizeKey(field.key);
  }

  function fieldDescription(field: ElicitationField): string {
    return field.schema.description || '';
  }

  function fieldKind(field: ElicitationField): 'choice' | 'boolean' | 'number' | 'textarea' | 'text' {
    if (extractChoiceOptions(field.schema).length > 0) return 'choice';
    if (field.schema.type === 'boolean') return 'boolean';
    if (field.schema.type === 'number' || field.schema.type === 'integer') return 'number';
    if (field.schema.format === 'textarea' || (field.schema.maxLength ?? 0) > 120) return 'textarea';
    return 'text';
  }

  function formatChoiceValue(value: unknown): string {
    if (typeof value === 'string') return value;
    if (typeof value === 'number' || typeof value === 'boolean') return String(value);
    return JSON.stringify(value);
  }

  function stringifyFieldValue(value: unknown): string {
    if (typeof value === 'string') return value;
    if (typeof value === 'number') return String(value);
    return '';
  }

  function parseNumberValue(raw: string): unknown {
    const trimmed = raw.trim();
    if (!trimmed) return '';
    const numeric = Number(trimmed);
    return Number.isFinite(numeric) ? numeric : '';
  }

  function updateFieldValue(key: string, value: unknown) {
    elicitationValues = {
      ...elicitationValues,
      [key]: value,
    };
  }

  function isFilled(value: unknown, field: ElicitationField): boolean {
    if (field.schema.type === 'boolean') return typeof value === 'boolean';
    if (field.schema.type === 'number' || field.schema.type === 'integer') return value !== '';
    if (typeof value === 'string') return value.trim().length > 0;
    return value !== undefined && value !== null;
  }

  function checkElicitationValidity(fields: ElicitationField[], values: Record<string, unknown>): boolean {
    if (fields.length === 0) return false;
    return fields.every((field) => !field.required || isFilled(values[field.key], field));
  }

  function buildElicitationContent(
    override?: { key: string; value: unknown } | null,
  ): unknown {
    const values = override
      ? {
          ...elicitationValues,
          [override.key]: override.value,
        }
      : elicitationValues;

    if (elicitationFields.length === 1 && elicitationFields[0].key === ROOT_FIELD_KEY) {
      return values[ROOT_FIELD_KEY];
    }

    const content: Record<string, unknown> = {};
    for (const field of elicitationFields) {
      const value = values[field.key];
      if (!isFilled(value, field) && !field.required) continue;
      content[field.key] = value;
    }
    return content;
  }

  function requestTitle(): string {
    if (isElicitation) return `${agentLabel} needs a reply`;
    if (toolName === 'Bash' && commandText) return `Allow ${agentLabel} to run this command?`;
    return `Allow ${agentLabel} to use ${toolName || 'this tool'}?`;
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

  async function acceptElicitation() {
    await invoke('resolve_permission', {
      id,
      decision: 'accept',
      elicitationContent: buildElicitationContent(),
    });
  }

  async function acceptChoice(field: ElicitationField, value: unknown) {
    await invoke('resolve_permission', {
      id,
      decision: 'accept',
      elicitationContent: buildElicitationContent({ key: field.key, value }),
    });
  }

  async function declineElicitation() {
    await invoke('resolve_permission', { id, decision: 'decline' });
  }

  async function cancelElicitation() {
    await invoke('resolve_permission', { id, decision: 'cancel' });
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

  async function focusTerminal() {
    await invoke('focus_terminal_for_session', { sessionId });
  }

  async function openElicitationLink() {
    if (!elicitationUrl) return;
    window.open(elicitationUrl, '_blank', 'noopener,noreferrer');
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
        <span class="eyebrow">{agentLabel}</span>
        <span class="title">{sessionSummary || 'Mode Changed'}</span>
        {#if headerMeta}
          <span class="meta">{headerMeta}</span>
        {/if}
      </div>
      <span class="badge badge-mode">{modeLabel}</span>
    </div>

    <div class="code-block mode-block">
      <pre>{modeDescription}</pre>
    </div>

    <div class="actions">
      <button class="btn btn-primary" onclick={dismiss} aria-label="Dismiss">
        OK
      </button>
    </div>
  </div>
{:else}
  <div class="bubble">
    <div class="glow"></div>
    <div class="header">
      <div class="header-copy">
        <span class="eyebrow">
          {isElicitation ? `${agentLabel} Needs Reply` : `${agentLabel} Wants Access`}
        </span>
        <span class="title">
          {sessionSummary || (isElicitation ? 'Reply Required' : 'Permission Request')}
        </span>
        {#if headerMeta}
          <span class="meta">{headerMeta}</span>
        {/if}
      </div>
      <span class="badge">{badge}</span>
    </div>

    <div class="intent">
      <div class="intent-title">{requestTitle()}</div>
      <div class="intent-copy">
        {#if isElicitation}
          Choose a reply here or jump back to the terminal session.
        {:else}
          This only affects the current request.
        {/if}
      </div>
    </div>

    {#if shellName || cwdLabel || elicitationServerName}
      <div class="meta-row">
        {#if shellName}
          <span class="mini-meta">{shellName}</span>
        {/if}
        {#if cwdLabel}
          <span class="mini-meta">{cwdLabel}</span>
        {/if}
        {#if elicitationServerName}
          <span class="mini-meta">Server: {elicitationServerName}</span>
        {/if}
      </div>
    {/if}

    {#if isElicitation && elicitationMessage}
      <div class="prompt-block">
        <div class="section-label">Prompt</div>
        <div class="prompt-copy">{elicitationMessage}</div>
      </div>
    {/if}

    {#if commandText}
      <div class="code-block command-block">
        <pre>{commandText}</pre>
      </div>
    {/if}

    {#if reasonText}
      <div class="reason">
        <span class="reason-label">Reason</span>
        <span class="reason-copy">{reasonText}</span>
      </div>
    {/if}

    {#if isElicitation}
      {#if singleChoiceField && singleChoiceOptions.length > 0}
        <div class="section-label">Options</div>
        <div class="choice-list">
          {#each singleChoiceOptions as option}
            <button
              class="choice-btn"
              onclick={() => acceptChoice(singleChoiceField, option.value)}
              aria-label={`Choose ${option.label}`}
            >
              <span class="choice-title">{option.label}</span>
              {#if option.description}
                <span class="choice-description">{option.description}</span>
              {/if}
            </button>
          {/each}
        </div>
      {:else if elicitationFields.length > 0}
        <div class="section-label">Reply</div>
        <div class="form-fields">
          {#each elicitationFields as field}
            {@const kind = fieldKind(field)}
            {@const options = kind === 'choice' ? extractChoiceOptions(field.schema) : []}
            <label class="field">
              <span class="field-label">
                {fieldTitle(field)}
                {#if field.required}
                  <span class="field-required">Required</span>
                {/if}
              </span>
              {#if fieldDescription(field)}
                <span class="field-description">{fieldDescription(field)}</span>
              {/if}

              {#if kind === 'choice'}
                <div class="choice-grid">
                  {#each options as option}
                    <button
                      class:selected={elicitationValues[field.key] === option.value}
                      class="choice-pill"
                      type="button"
                      onclick={() => updateFieldValue(field.key, option.value)}
                      aria-label={`Select ${option.label}`}
                    >
                      {option.label}
                    </button>
                  {/each}
                </div>
              {:else if kind === 'boolean'}
                <button
                  class:selected={Boolean(elicitationValues[field.key])}
                  class="toggle-pill"
                  type="button"
                  onclick={() => updateFieldValue(field.key, !Boolean(elicitationValues[field.key]))}
                  aria-label={`Toggle ${fieldTitle(field)}`}
                >
                  {Boolean(elicitationValues[field.key]) ? 'Yes' : 'No'}
                </button>
              {:else if kind === 'textarea'}
                <textarea
                  class="input textarea"
                  rows="3"
                  value={stringifyFieldValue(elicitationValues[field.key])}
                  oninput={(event) => updateFieldValue(field.key, (event.currentTarget as HTMLTextAreaElement).value)}
                ></textarea>
              {:else}
                <input
                  class="input"
                  type={kind === 'number' ? 'number' : 'text'}
                  value={stringifyFieldValue(elicitationValues[field.key])}
                  oninput={(event) =>
                    updateFieldValue(
                      field.key,
                      kind === 'number'
                        ? parseNumberValue((event.currentTarget as HTMLInputElement).value)
                        : (event.currentTarget as HTMLInputElement).value,
                    )}
                />
              {/if}
            </label>
          {/each}
        </div>
      {/if}

      {#if elicitationMode || elicitationUrl}
        <div class="reason">
          <span class="reason-label">Mode</span>
          <span class="reason-copy">
            {elicitationMode || 'terminal'}
            {#if elicitationUrl}
              <span class="inline-link">{elicitationUrl}</span>
            {/if}
          </span>
        </div>
      {/if}

      <div class="actions actions-wrap">
        {#if !singleChoiceField && elicitationFields.length > 0}
          <button
            class="btn btn-primary btn-stacked"
            onclick={acceptElicitation}
            aria-label="Submit reply"
            disabled={!canSubmitElicitation}
          >
            <span>Send Reply</span>
            <small>Return this answer to Claude Code</small>
          </button>
        {/if}

        {#if elicitationUrl}
          <button class="btn btn-secondary btn-stacked" onclick={openElicitationLink} aria-label="Open external link">
            <span>Open Link</span>
            <small>Continue in the browser</small>
          </button>
        {/if}

        <button class="btn btn-secondary btn-stacked" onclick={focusTerminal} aria-label="Focus terminal">
          <span>Open Terminal</span>
          <small>Reply there instead</small>
        </button>
        <button class="btn btn-secondary btn-stacked" onclick={declineElicitation} aria-label="Decline this request">
          <span>Decline</span>
          <small>Refuse this prompt</small>
        </button>
        <button class="btn btn-secondary btn-stacked" onclick={cancelElicitation} aria-label="Cancel this request">
          <span>Cancel</span>
          <small>Close without answering</small>
        </button>
      </div>
    {:else}
      <div class="actions">
        <button class="btn btn-primary btn-stacked" onclick={allow} aria-label="Allow this request once">
          <span>Allow Once</span>
          <small>Approve only this request</small>
        </button>
        <button class="btn btn-secondary btn-stacked" onclick={deny} aria-label="Deny this request">
          <span>Deny</span>
          <small>Block this request</small>
        </button>
      </div>

      {#if suggestions.length > 0}
        <div class="section-label suggestions-label">Remember</div>
        <div class="suggestions">
          {#each suggestions as sug}
            {@const suggestion = describeSuggestion(sug)}
            <button
              class="suggestion"
              onclick={() => applySuggestion(sug)}
              aria-label={`Apply suggestion: ${suggestionLabel(sug)}`}
            >
              <span class="suggestion-title">{suggestion.title}</span>
              <span class="suggestion-subtitle">{suggestion.subtitle}</span>
            </button>
          {/each}
        </div>
      {/if}
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

  .meta {
    font-size: 11px;
    line-height: 1.4;
    color: rgba(215, 206, 189, 0.84);
    word-break: break-word;
  }

  .meta-row {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 0 0 12px;
  }

  .mini-meta {
    display: inline-flex;
    align-items: center;
    min-height: 24px;
    padding: 0 9px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid rgba(255, 255, 255, 0.07);
    font-size: 11px;
    color: #d8cdbc;
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

  .prompt-block,
  .code-block {
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.03), rgba(255, 255, 255, 0.014));
    border: 1px solid rgba(255, 255, 255, 0.075);
    border-radius: 12px;
    padding: 12px 13px;
    margin-bottom: 12px;
    overflow: hidden;
  }

  .prompt-copy {
    font-size: 12px;
    line-height: 1.55;
    color: #e7dccd;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .code-block {
    max-height: 88px;
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

  .reason {
    display: flex;
    flex-direction: column;
    gap: 5px;
    margin-bottom: 14px;
    padding: 10px 12px;
    border-radius: 11px;
    background: rgba(255, 255, 255, 0.032);
    border: 1px solid rgba(255, 255, 255, 0.065);
  }

  .reason-label {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--copy-secondary);
  }

  .reason-copy {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 11.5px;
    line-height: 1.45;
    color: #e8ddce;
  }

  .inline-link {
    color: #d9bc95;
    word-break: break-all;
  }

  .choice-list,
  .form-fields,
  .suggestions {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .choice-btn,
  .suggestion {
    width: 100%;
    padding: 11px 12px;
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

  .choice-btn:hover,
  .suggestion:hover {
    background: rgba(216, 165, 108, 0.1);
    border-color: rgba(216, 165, 108, 0.16);
    color: #f6e7d1;
    transform: translateY(-1px);
  }

  .choice-title,
  .suggestion-title {
    display: block;
    font-size: 12px;
    font-weight: 620;
    color: #f1e5d4;
    margin-bottom: 3px;
  }

  .choice-description,
  .suggestion-subtitle {
    display: block;
    font-size: 10.5px;
    line-height: 1.4;
    color: #bfb3a0;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 7px;
  }

  .field-label {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    font-weight: 620;
    color: #f0e6d7;
  }

  .field-required {
    font-size: 10px;
    color: #d9bc95;
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  .field-description {
    font-size: 10.5px;
    line-height: 1.45;
    color: #beb2a0;
  }

  .choice-grid {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .choice-pill,
  .toggle-pill {
    min-height: 32px;
    padding: 0 11px;
    border-radius: 999px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    background: rgba(255, 255, 255, 0.04);
    color: #ddd3c4;
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.15s ease, border-color 0.15s ease, color 0.15s ease;
  }

  .choice-pill.selected,
  .toggle-pill.selected {
    background: rgba(216, 165, 108, 0.16);
    border-color: rgba(216, 165, 108, 0.24);
    color: #f6e9d7;
  }

  .input {
    width: 100%;
    min-height: 36px;
    padding: 9px 11px;
    border-radius: 10px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    background: rgba(255, 255, 255, 0.04);
    color: #f4eadc;
    font-size: 12px;
    outline: none;
    box-sizing: border-box;
  }

  .textarea {
    min-height: 78px;
    resize: vertical;
    font-family: inherit;
  }

  .input:focus,
  .textarea:focus {
    border-color: rgba(216, 165, 108, 0.26);
    background: rgba(255, 255, 255, 0.05);
  }

  .actions {
    display: flex;
    gap: 10px;
    margin-bottom: 0;
  }

  .actions-wrap {
    flex-wrap: wrap;
    margin-top: 14px;
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

  .btn:disabled {
    cursor: not-allowed;
    opacity: 0.5;
    transform: none;
    box-shadow: none;
  }

  .btn-stacked {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 2px;
    min-width: 136px;
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
    text-align: center;
  }

  .btn-primary {
    background: linear-gradient(135deg, #dfa66d, #be7f4f);
    color: #1f1307;
    box-shadow: 0 10px 22px rgba(190, 127, 79, 0.24);
  }

  .btn-primary:hover:not(:disabled) {
    background: linear-gradient(135deg, #e8b27b, #ca8a59);
    box-shadow: 0 14px 26px rgba(190, 127, 79, 0.3);
    transform: translateY(-1px);
  }

  .btn-primary:active:not(:disabled) {
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
  .choice-btn:focus-visible,
  .choice-pill:focus-visible,
  .toggle-pill:focus-visible,
  .suggestion:focus-visible,
  .input:focus-visible,
  .textarea:focus-visible {
    outline: 2px solid rgba(216, 165, 108, 0.72);
    outline-offset: 2px;
  }
</style>
