<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import BubbleCard from './BubbleCard.svelte';

  const params = new URLSearchParams(window.location.search);
  const entryId = params.get('entry_id') ?? '';

  let bubbleData: any = $state(null);
  let resizeObserver: ResizeObserver | null = null;
  let rootEl: HTMLDivElement | null = null;

  function measureHeight() {
    if (!rootEl) return;
    const height = Math.ceil(Math.max(rootEl.scrollHeight, rootEl.getBoundingClientRect().height));
    invoke('bubble_height_measured', { id: entryId, height });
  }

  onMount(async () => {
    bubbleData = await invoke('get_bubble_data', { id: entryId });
    await tick();

    if (bubbleData && rootEl) {
      resizeObserver = new ResizeObserver(([entry]) => {
        const shell = rootEl;
        if (!shell) return;
        const height = Math.ceil(Math.max(shell.scrollHeight, entry.contentRect.height));
        invoke('bubble_height_measured', { id: entryId, height });
      });
      resizeObserver.observe(rootEl);
      measureHeight();
    }
  });

  onDestroy(() => {
    resizeObserver?.disconnect();
  });
</script>

<div class="shell" bind:this={rootEl}>
  {#if bubbleData}
    <BubbleCard
      id={entryId}
      windowKind={bubbleData.window_kind}
      toolName={bubbleData.tool_name ?? ''}
      toolInput={bubbleData.tool_input ?? {}}
      suggestions={bubbleData.suggestions ?? []}
      sessionId={bubbleData.session_id}
      agentLabel={bubbleData.agent_label ?? 'Claude'}
      sessionSummary={bubbleData.session_summary ?? ''}
      sessionProject={bubbleData.session_project ?? ''}
      sessionShortId={bubbleData.session_short_id ?? ''}
      isElicitation={bubbleData.is_elicitation ?? false}
      elicitationMessage={bubbleData.elicitation_message ?? ''}
      elicitationSchema={bubbleData.elicitation_schema ?? null}
      elicitationMode={bubbleData.elicitation_mode ?? ''}
      elicitationUrl={bubbleData.elicitation_url ?? ''}
      elicitationServerName={bubbleData.elicitation_server_name ?? ''}
      modeLabel={bubbleData.mode_label ?? ''}
      modeDescription={bubbleData.mode_description ?? ''}
    />
  {:else}
    <div class="loading">Loading...</div>
  {/if}
</div>

<style>
  :global(html, body) {
    margin: 0;
    padding: 0;
    background: transparent;
    overflow: hidden;
  }

  .shell {
    width: 100%;
    padding: 10px;
    background: transparent;
  }

  .loading {
    padding: 16px;
    color: rgba(240, 231, 215, 0.8);
    font-size: 12px;
  }
</style>
