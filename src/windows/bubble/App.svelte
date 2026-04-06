<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWindow, PhysicalPosition } from '@tauri-apps/api/window';
  import BubbleCard from './BubbleCard.svelte';

  const params = new URLSearchParams(window.location.search);
  const entryId = params.get('entry_id') ?? '';

  let bubbleData: any = $state(null);
  let resizeObserver: ResizeObserver | null = null;
  let rootEl: HTMLDivElement | null = null;
  let dragging = $state(false);
  let dragStartPointerX = 0;
  let dragStartPointerY = 0;
  let dragStartWindowX = 0;
  let dragStartWindowY = 0;
  let dragScaleFactor = 1;
  let dragStartReady = false;
  let activePointerId: number | null = null;
  let pointerCaptureEl: HTMLElement | null = null;
  let closing = $state(false);
  let unlistenPrepareClose: (() => void) | null = null;

  function measureHeight() {
    if (!rootEl) return;
    const height = Math.ceil(Math.max(rootEl.scrollHeight, rootEl.getBoundingClientRect().height));
    invoke('bubble_height_measured', { id: entryId, height });
  }

  async function onPointerDown(event: PointerEvent) {
    if (event.button !== 0 || !event.isPrimary) return;

    const target = event.target as HTMLElement | null;
    const dragHandle = target?.closest('.bubble-drag-handle') as HTMLElement | null;
    if (!dragHandle) return;

    const currentWindow = getCurrentWindow();

    dragging = true;
    activePointerId = event.pointerId;
    pointerCaptureEl = dragHandle;
    pointerCaptureEl.setPointerCapture(event.pointerId);
    dragStartPointerX = event.screenX;
    dragStartPointerY = event.screenY;
    dragStartReady = false;

    window.addEventListener('pointermove', onPointerMove);
    window.addEventListener('pointerup', onPointerUp);
    window.addEventListener('pointercancel', onPointerUp);

    try {
      const [position, scaleFactor] = await Promise.all([currentWindow.outerPosition(), currentWindow.scaleFactor()]);
      if (!dragging || activePointerId !== event.pointerId) return;
      dragStartWindowX = position.x;
      dragStartWindowY = position.y;
      dragScaleFactor = scaleFactor || 1;
      dragStartReady = true;
    } catch {
      dragging = false;
      dragStartReady = false;
      if (pointerCaptureEl && activePointerId !== null && pointerCaptureEl.hasPointerCapture(activePointerId)) {
        pointerCaptureEl.releasePointerCapture(activePointerId);
      }
      activePointerId = null;
      pointerCaptureEl = null;
      window.removeEventListener('pointermove', onPointerMove);
      window.removeEventListener('pointerup', onPointerUp);
      window.removeEventListener('pointercancel', onPointerUp);
    }
  }

  async function onPointerMove(event: PointerEvent) {
    if (!dragging || event.pointerId !== activePointerId || !dragStartReady) return;

    const pointerDx = event.screenX - dragStartPointerX;
    const pointerDy = event.screenY - dragStartPointerY;
    const nextX = Math.round(dragStartWindowX + pointerDx * dragScaleFactor);
    const nextY = Math.round(dragStartWindowY + pointerDy * dragScaleFactor);

    await getCurrentWindow().setPosition(new PhysicalPosition(nextX, nextY));
  }

  async function onPointerUp(event: PointerEvent) {
    if (!dragging || event.pointerId !== activePointerId) return;

    dragging = false;
    dragStartReady = false;
    if (pointerCaptureEl && activePointerId !== null && pointerCaptureEl.hasPointerCapture(activePointerId)) {
      pointerCaptureEl.releasePointerCapture(activePointerId);
    }
    activePointerId = null;
    pointerCaptureEl = null;
    window.removeEventListener('pointermove', onPointerMove);
    window.removeEventListener('pointerup', onPointerUp);
    window.removeEventListener('pointercancel', onPointerUp);

    const position = await getCurrentWindow().outerPosition();
    await invoke('bubble_drag_finished', { id: entryId, x: position.x, y: position.y });
  }

  onMount(async () => {
    window.addEventListener('pointerdown', onPointerDown);

    unlistenPrepareClose = await listen<{ id: string }>('bubble-prepare-close', async ({ payload }) => {
      if (payload?.id !== entryId || closing) return;
      closing = true;
      window.setTimeout(() => {
        invoke('finalize_bubble_close', { id: entryId });
      }, 200);
    });

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
    unlistenPrepareClose?.();
    window.removeEventListener('pointerdown', onPointerDown);
    window.removeEventListener('pointermove', onPointerMove);
    window.removeEventListener('pointerup', onPointerUp);
    window.removeEventListener('pointercancel', onPointerUp);
  });
</script>

<div class="shell" class:closing={closing} bind:this={rootEl}>
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
      updateVersion={bubbleData.update_version ?? ''}
      updateUrl={bubbleData.update_url ?? ''}
      updateNotes={bubbleData.update_notes ?? ''}
      updateLang={bubbleData.update_lang ?? 'en'}
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
    opacity: 1;
    transform: translateY(0);
    animation: bubble-enter 200ms ease-out;
  }

  .shell.closing {
    animation: bubble-exit 200ms ease-in forwards;
  }

  @keyframes bubble-enter {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  @keyframes bubble-exit {
    from {
      opacity: 1;
      transform: translateY(0);
    }
    to {
      opacity: 0;
      transform: translateY(4px);
    }
  }

  .loading {
    padding: 16px;
    color: rgba(240, 231, 215, 0.8);
    font-size: 12px;
  }
</style>
