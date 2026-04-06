<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import { currentSvg, currentState, dndEnabled, currentLang } from '../../lib/stores';
  import { get } from 'svelte/store';

  interface TaskItem { id: string; text: string; order: number; }
  let tasks: TaskItem[] = $state([]);

  import _idleFollowRaw from '../../../assets/svg/clyde-idle-follow.svg?raw';

  const rawModules = import.meta.glob('../../../assets/svg/*.svg', {
    query: '?raw',
    import: 'default',
    eager: true,
  }) as Record<string, string>;

  function stripSvgSize(raw: string): string {
    return raw.replace(/\s+width="[^"]*"/, '').replace(/\s+height="[^"]*"/, '');
  }

  // Pre-process all SVGs at init time — avoids regex on every state change
  const svgCache: Record<string, string> = {};
  for (const [key, raw] of Object.entries(rawModules)) {
    const filename = key.split('/').pop() ?? key;
    svgCache[filename] = stripSvgSize(raw);
  }
  if (_idleFollowRaw && !svgCache['clyde-idle-follow.svg']) {
    svgCache['clyde-idle-follow.svg'] = stripSvgSize(_idleFollowRaw);
  }

  function getSvg(filename: string): string {
    return svgCache[filename] ?? svgCache['clyde-idle-follow.svg'] ?? '';
  }

  let svgContent = $state(getSvg(get(currentSvg)));
  let flipped = $state(false);
  let unlisten: UnlistenFn[] = [];
  let isReacting = false;
  let reactTimer: ReturnType<typeof setTimeout> | null = null;
  let snapPreview = $state(false);
  let opacity = $state(1);

  function movePupils(dx: number, dy: number) {
    // Eyes follow cursor (larger movement)
    const eyes = document.getElementById('eyes-js');
    if (eyes) eyes.style.transform = `translate(${dx * 0.6}px, ${dy * 0.4}px)`;

    // Body tilts slightly toward cursor
    const body = document.getElementById('body-js');
    if (body) body.style.transform = `translate(${dx * 0.15}px, 0)`;

    // Shadow stretches opposite to lean
    const shadow = document.getElementById('shadow-js');
    if (shadow) shadow.style.transform = `scaleX(${1 + Math.abs(dx) * 0.03})`;
  }

  function playReaction(svgFile: string, durationMs: number) {
    if (reactTimer) clearTimeout(reactTimer);
    isReacting = true;
    currentSvg.set(svgFile);
    svgContent = getSvg(svgFile);
    reactTimer = setTimeout(() => { isReacting = false; }, durationMs);
  }

  onMount(() => {
    const setup = async () => {
      const config = await invoke<{ opacity: number }>('get_pet_config');
      opacity = config.opacity ?? 1;

      unlisten.push(await listen<{ state: string; svg: string; flip?: boolean }>('state-change', ({ payload }) => {
        if (isReacting) return;
        currentState.set(payload.state as any);
        currentSvg.set(payload.svg);
        svgContent = getSvg(payload.svg);
        flipped = payload.flip ?? false;
      }));

      unlisten.push(await listen<{ dx: number; dy: number }>('eye-move', ({ payload }) => {
        movePupils(payload.dx, payload.dy);
      }));

      unlisten.push(await listen<{ enabled: boolean }>('dnd-change', ({ payload }) => {
        dndEnabled.set(payload.enabled);
      }));

      unlisten.push(await listen<{ svg: string; duration_ms: number }>('play-click-reaction', ({ payload }) => {
        playReaction(payload.svg, payload.duration_ms);
      }));

      unlisten.push(await listen<{ opacity: number }>('pet-config-changed', ({ payload }) => {
        opacity = payload.opacity ?? 1;
      }));

      unlisten.push(await listen('start-drag-reaction', () => {
        currentSvg.set('clyde-react-drag.svg');
        svgContent = getSvg('clyde-react-drag.svg');
      }));

      unlisten.push(await listen<{ active: boolean }>('snap-preview', ({ payload }) => {
        snapPreview = payload.active;
      }));

      // Load tasks and listen for changes
      tasks = await invoke<TaskItem[]>('get_tasks');
      unlisten.push(await listen<TaskItem[]>('tasks-changed', ({ payload }) => {
        tasks = payload;
      }));

      unlisten.push(await listen('trigger-yawn', () => { invoke('trigger_sleep_sequence'); }));
      unlisten.push(await listen('trigger-wake', () => { invoke('trigger_wake'); }));
      unlisten.push(await listen('mini-peek-in', () => { invoke('mini_peek_in'); }));
      unlisten.push(await listen('mini-peek-out', () => { invoke('mini_peek_out'); }));
      unlisten.push(await listen<string>('set-size', ({ payload }) => { invoke('set_window_size', { size: payload }); }));
      unlisten.push(await listen<string>('set-lang', ({ payload }) => {
        currentLang.set(payload);
        invoke('set_lang', { lang: payload });
      }));
    };
    setup();
  });

  onDestroy(() => {
    unlisten.forEach(u => u());
    if (reactTimer) clearTimeout(reactTimer);
  });
</script>

<div id="pet-container" class:snap-preview={snapPreview} style:opacity={opacity}>
  {#if tasks.length > 0}
    <div class="task-panel">
      {#each tasks.slice(0, 3) as task, i}
        <div class="task-item">
          <span class="task-index">{i + 1}</span>
          <span class="task-text">{task.text}</span>
        </div>
      {/each}
    </div>
  {/if}
  <div class="svg-wrapper" style:transform={flipped ? 'scaleX(-1)' : ''}>
    {@html svgContent}
  </div>
</div>

<style>
  #pet-container {
    width: 100%;
    height: 100%;
    position: relative;
    background: transparent;
    overflow: hidden;
  }
  .svg-wrapper {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }
  .svg-wrapper :global(svg) {
    display: block;
    width: 100%;
    height: 100%;
  }
  /* Snap preview: scale down + slight transparency when near screen edge during drag */
  #pet-container.snap-preview {
    transform: scale(0.7);
    opacity: 0.6;
    transition: transform 150ms ease-out, opacity 150ms ease-out;
  }
  #pet-container:not(.snap-preview) {
    transition: transform 150ms ease-out, opacity 150ms ease-out;
  }
  /* ── Task panel ── */
  .task-panel {
    position: absolute;
    top: 6px;
    left: 6px;
    z-index: 10;
    display: flex;
    flex-direction: column;
    gap: 3px;
    max-width: 70%;
    pointer-events: none;
  }
  .task-item {
    display: flex;
    align-items: baseline;
    gap: 4px;
    background: rgba(0, 0, 0, 0.45);
    backdrop-filter: blur(6px);
    -webkit-backdrop-filter: blur(6px);
    border-radius: 4px;
    padding: 2px 6px;
    line-height: 1.3;
  }
  .task-index {
    font-size: 9px;
    font-weight: 700;
    color: rgba(255, 255, 255, 0.5);
    flex-shrink: 0;
    min-width: 10px;
  }
  .task-text {
    font-size: 10px;
    font-weight: 500;
    color: rgba(255, 255, 255, 0.88);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif;
  }

  /* Smooth eye/body tracking — interpolate between 50ms tick updates */
  .svg-wrapper :global(#eyes-js),
  .svg-wrapper :global(#body-js),
  .svg-wrapper :global(#shadow-js) {
    transition: transform 80ms ease-out;
  }
</style>
