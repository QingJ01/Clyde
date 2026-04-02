<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import BubbleCard from './BubbleCard.svelte';

  const params = new URLSearchParams(window.location.search);
  const entryId = params.get('entry_id') ?? '';

  let bubbleData: any = $state(null);
  let resizeObserver: ResizeObserver | null = null;
  let rootEl: HTMLDivElement | null = null;

  onMount(async () => {
    bubbleData = await invoke('get_bubble_data', { id: entryId });

    if (bubbleData && rootEl) {
      resizeObserver = new ResizeObserver(([entry]) => {
        const height = Math.ceil(entry.contentRect.height);
        invoke('bubble_height_measured', { id: entryId, height });
      });
      resizeObserver.observe(rootEl);
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
      isElicitation={bubbleData.is_elicitation ?? false}
      modeLabel={bubbleData.mode_label ?? ''}
      modeDescription={bubbleData.mode_description ?? ''}
    />
  {:else}
    <div class="loading">Loading...</div>
  {/if}
</div>

<style>
  :global(html, body) {
    background: transparent;
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
