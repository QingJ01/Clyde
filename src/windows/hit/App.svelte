<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { onDestroy, onMount } from 'svelte';

  interface InteractionState {
    position_locked: boolean;
    click_through: boolean;
  }

  interface HitRegion {
    x: number;
    y: number;
    width: number;
    height: number;
  }

  interface HitLayoutPayload {
    width: number;
    height: number;
    regions: HitRegion[];
    pointer_alpha?: number;
  }

  const DRAG_THRESHOLD_PHYSICAL_PX = 3;

  let isDragging = false;
  let pointerActive = false;
  let activePointerId: number | null = null;
  let startX = 0;
  let startY = 0;
  let clickCount = 0;
  let clickTimer: ReturnType<typeof setTimeout> | null = null;
  let snapSide: 'left' | 'right' | null = null;
  let positionLocked = false;
  let promptedLockedMenu = false;
  let regions: HitRegion[] = [];
  let pointerAlpha = 0;
  let unlistenSnap: UnlistenFn | null = null;
  let unlistenInteraction: UnlistenFn | null = null;
  let unlistenLayout: UnlistenFn | null = null;

  function onPointerDown(e: PointerEvent) {
    if (e.button !== 0) return; // Only handle left click — right click goes to onContextMenu
    isDragging = false;
    pointerActive = true;
    activePointerId = e.pointerId;
    promptedLockedMenu = false;
    startX = e.screenX;
    startY = e.screenY;
    // Capture pointer so events keep flowing even if cursor leaves the hit window
    // (critical for mini mode where the hit window is very small).
    (e.target as Element)?.setPointerCapture(e.pointerId);
    if (positionLocked) return;

    invoke('drag_start', { x: startX, y: startY });

    clickCount++;
    if (clickTimer) clearTimeout(clickTimer);
    clickTimer = setTimeout(() => {
      const count = clickCount;
      clickCount = 0;
      if (!isDragging) {
        if (count === 2) invoke('hit_double_click');
        else if (count >= 4) invoke('hit_flail');
      }
    }, 300);
  }

  function dragDistanceThresholdLogicalPx() {
    return DRAG_THRESHOLD_PHYSICAL_PX / (window.devicePixelRatio || 1);
  }

  function onPointerMove(e: PointerEvent) {
    if (!pointerActive) return;
    if (activePointerId !== null && e.pointerId !== activePointerId) return;
    if (e.buttons === 0) return;
    if (positionLocked) {
      const dx = e.screenX - startX;
      const dy = e.screenY - startY;
      if (!promptedLockedMenu && Math.sqrt(dx * dx + dy * dy) >= dragDistanceThresholdLogicalPx()) {
        promptedLockedMenu = true;
        invoke('show_context_menu');
      }
      return;
    }
    // Mark as dragging after crossing the same physical-pixel threshold as Rust.
    if (!isDragging) {
      const dx = e.screenX - startX;
      const dy = e.screenY - startY;
      if (Math.sqrt(dx * dx + dy * dy) >= dragDistanceThresholdLogicalPx()) isDragging = true;
    }
    invoke('drag_move', { x: e.screenX, y: e.screenY });
  }

  function onPointerUp(e: PointerEvent) {
    if (!pointerActive) return;
    if (activePointerId !== null && e.pointerId !== activePointerId) return;
    (e.target as Element)?.releasePointerCapture(e.pointerId);
    pointerActive = false;
    activePointerId = null;
    snapSide = null;
    promptedLockedMenu = false;
    if (!positionLocked) {
      invoke('drag_end');
    }
  }

  function onPointerCancel(e: PointerEvent) {
    if (!pointerActive) return;
    (e.target as Element)?.releasePointerCapture(e.pointerId);
    pointerActive = false;
    activePointerId = null;
    snapSide = null;
    promptedLockedMenu = false;
    if (!positionLocked) {
      invoke('drag_end');
    }
  }

  function onKeyDown(e: KeyboardEvent) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      invoke('hit_double_click');
    }
  }

  function onContextMenu(e: MouseEvent) {
    e.preventDefault();
    invoke('show_context_menu');
  }

  onMount(() => {
    const setup = async () => {
      const interaction = await invoke<InteractionState>('get_interaction_state');
      positionLocked = interaction.position_locked ?? false;

      unlistenSnap = await listen<{ active: boolean; side: 'left' | 'right' | null }>('snap-preview', ({ payload }) => {
        snapSide = payload.active ? payload.side : null;
      });

      unlistenInteraction = await listen<InteractionState>('interaction-state-changed', ({ payload }) => {
        positionLocked = payload.position_locked ?? false;
      });

      unlistenLayout = await listen<HitLayoutPayload>('hit-layout-changed', ({ payload }) => {
        regions = payload.regions ?? [];
        pointerAlpha = payload.pointer_alpha ?? 0;
      });

      const initialLayout = await invoke<HitLayoutPayload | null>('get_current_hit_layout');
      if (initialLayout) {
        regions = initialLayout.regions ?? [];
        pointerAlpha = initialLayout.pointer_alpha ?? 0;
      }

      window.addEventListener('pointermove', onPointerMove);
      window.addEventListener('pointerup', onPointerUp);
      window.addEventListener('pointercancel', onPointerCancel);
    };
    setup();
  });

  onDestroy(() => {
    window.removeEventListener('pointermove', onPointerMove);
    window.removeEventListener('pointerup', onPointerUp);
    window.removeEventListener('pointercancel', onPointerCancel);
    unlistenSnap?.();
    unlistenInteraction?.();
    unlistenLayout?.();
    if (clickTimer) clearTimeout(clickTimer);
  });
</script>

<div class="hit-root" style={`--pointer-alpha:${pointerAlpha}`}>
  {#each regions as region, index (index)}
    <div
      class="hit-zone"
      class:locked={positionLocked}
      class:snap-left={snapSide === 'left'}
      class:snap-right={snapSide === 'right'}
      style:left={`${region.x}px`}
      style:top={`${region.y}px`}
      style:width={`${region.width}px`}
      style:height={`${region.height}px`}
      onpointerdown={onPointerDown}
      oncontextmenu={onContextMenu}
      onkeydown={onKeyDown}
      role="button"
      tabindex="0"
      aria-label="Clyde desktop pet"
    ></div>
  {/each}
</div>

<style>
  .hit-root {
    position: relative;
    width: 100%;
    height: 100%;
    background: transparent;
    pointer-events: none;
  }

  .hit-zone {
    position: absolute;
    background: rgba(0, 0, 0, var(--pointer-alpha, 0));
    cursor: grab;
    pointer-events: auto;
    touch-action: none;
    user-select: none;
    -webkit-user-select: none;
  }

  .hit-zone.locked {
    cursor: not-allowed;
  }

  .hit-zone::after {
    content: '';
    position: absolute;
    inset: 0;
    border-radius: 16px;
    opacity: 0;
    transition: opacity 120ms ease, box-shadow 120ms ease, border-color 120ms ease;
    pointer-events: none;
    border: 2px solid transparent;
  }

  .hit-zone.snap-left::after,
  .hit-zone.snap-right::after {
    opacity: 1;
    box-shadow: 0 0 0 2px rgba(59, 130, 246, 0.12), 0 10px 24px rgba(59, 130, 246, 0.2);
  }

  .hit-zone.snap-left::after {
    border-left-color: rgba(59, 130, 246, 0.9);
    background: linear-gradient(90deg, rgba(59, 130, 246, 0.22), transparent 42%);
  }

  .hit-zone.snap-right::after {
    border-right-color: rgba(59, 130, 246, 0.9);
    background: linear-gradient(270deg, rgba(59, 130, 246, 0.22), transparent 42%);
  }
</style>
