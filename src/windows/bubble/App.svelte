<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import BubbleCard from './BubbleCard.svelte';

  const params = new URLSearchParams(window.location.search);
  const entryId = params.get('entry_id') ?? '';

  let bubbleData: any = $state(null);
  let resizeObserver: ResizeObserver | null = null;

  onMount(async () => {
    bubbleData = await invoke('get_bubble_data', { id: entryId });

    if (bubbleData) {
      resizeObserver = new ResizeObserver(([entry]) => {
        const height = Math.ceil(entry.contentRect.height);
        invoke('bubble_height_measured', { id: entryId, height });
      });
      resizeObserver.observe(document.body);
    }
  });

  onDestroy(() => {
    resizeObserver?.disconnect();
  });
</script>

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
  <div style="padding:16px;color:#666">Loading...</div>
{/if}
