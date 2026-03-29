<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';

  let isDragging = false;
  let startX = 0;
  let startY = 0;
  let clickCount = 0;
  let clickTimer: ReturnType<typeof setTimeout> | null = null;

  function toPhys(v: number) { return Math.round(v * window.devicePixelRatio); }

  function onPointerDown(e: PointerEvent) {
    (e.target as Element).setPointerCapture(e.pointerId);
    isDragging = false;
    startX = toPhys(e.screenX);
    startY = toPhys(e.screenY);
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

  function onPointerMove(e: PointerEvent) {
    if (e.buttons === 0) return;
    // Mark as dragging after a few pixels of movement (matches Rust-side threshold)
    if (!isDragging) {
      const dx = toPhys(e.screenX) - startX;
      const dy = toPhys(e.screenY) - startY;
      if (Math.sqrt(dx * dx + dy * dy) >= 3) isDragging = true;
    }
    invoke('drag_move', { x: toPhys(e.screenX), y: toPhys(e.screenY) });
  }

  function onPointerUp(e: PointerEvent) {
    (e.target as Element).releasePointerCapture(e.pointerId);
    invoke('drag_end');
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
</script>

<div
  class="hit-surface"
  onpointerdown={onPointerDown}
  onpointermove={onPointerMove}
  onpointerup={onPointerUp}
  oncontextmenu={onContextMenu}
  onkeydown={onKeyDown}
  role="button"
  tabindex="0"
  aria-label="Clyde desktop pet"
></div>

<style>
  .hit-surface {
    width: 100%;
    height: 100%;
    background: transparent;
    cursor: pointer;
    user-select: none;
    -webkit-user-select: none;
  }
</style>
