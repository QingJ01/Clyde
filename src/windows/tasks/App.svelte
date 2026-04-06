<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';

  interface TaskItem { id: string; text: string; order: number; }

  let tasks: TaskItem[] = $state([]);
  let dragIdx: number | null = $state(null);
  let dropIdx: number | null = $state(null);
  let pointerY = 0;
  let rowHeight = 32;

  async function reload() {
    tasks = await invoke<TaskItem[]>('get_tasks');
  }

  async function save() {
    await invoke('set_tasks', { tasks: [...tasks] });
  }

  function onInput(i: number, e: Event) {
    tasks[i].text = (e.target as HTMLInputElement).value;
  }

  async function onBlur() {
    await save();
  }

  function onKeyDown(e: KeyboardEvent) {
    if (e.key === 'Enter') (e.target as HTMLElement).blur();
  }

  async function addTask() {
    if (tasks.length >= 5) return;
    await invoke('add_task', { text: '' });
    await reload();
    requestAnimationFrame(() => {
      const inputs = document.querySelectorAll<HTMLInputElement>('.task-input');
      inputs[inputs.length - 1]?.focus();
    });
  }

  async function removeTask(id: string) {
    await invoke('remove_task', { id });
    await reload();
  }

  // Pointer-based drag reorder
  function onGripDown(i: number, e: PointerEvent) {
    e.preventDefault();
    dragIdx = i;
    dropIdx = i;
    pointerY = e.clientY;
    const el = (e.target as HTMLElement).closest('.task-row') as HTMLElement;
    if (el) rowHeight = el.offsetHeight + 3; // row + gap

    const onMove = (me: PointerEvent) => {
      if (dragIdx === null) return;
      const delta = me.clientY - pointerY;
      const shift = Math.round(delta / rowHeight);
      let newIdx = dragIdx + shift;
      newIdx = Math.max(0, Math.min(tasks.length - 1, newIdx));
      dropIdx = newIdx;
    };

    const onUp = () => {
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
      if (dragIdx !== null && dropIdx !== null && dragIdx !== dropIdx) {
        const moved = tasks.splice(dragIdx, 1)[0];
        tasks.splice(dropIdx, 0, moved);
        tasks = tasks.map((t, i) => ({ ...t, order: i }));
        save();
      }
      dragIdx = null;
      dropIdx = null;
    };

    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
  }

  async function close() {
    await save();
    await invoke('close_tasks_editor');
  }

  onMount(() => { reload(); });
</script>

<div class="panel">
  <div class="header">
    <span class="title" data-tauri-drag-region>MY TASKS</span>
    <button class="close-btn" onclick={close}>
      <svg width="10" height="10" viewBox="0 0 10 10">
        <path d="M1 1L9 9M9 1L1 9" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
      </svg>
    </button>
  </div>

  <div class="task-list">
    {#each tasks as task, i (task.id)}
      <div
        class="task-row"
        class:dragging={dragIdx === i}
        class:drop-above={dropIdx === i && dragIdx !== null && dragIdx > i}
        class:drop-below={dropIdx === i && dragIdx !== null && dragIdx < i}
      >
        <span
          class="grip"
          onpointerdown={(e: PointerEvent) => onGripDown(i, e)}
          role="button"
          tabindex="-1"
        >⠿</span>
        <input
          class="task-input"
          type="text"
          value={task.text}
          placeholder="输入任务..."
          oninput={(e: Event) => onInput(i, e)}
          onblur={onBlur}
          onkeydown={onKeyDown}
        />
        <button class="del-btn" onclick={() => removeTask(task.id)}>
          <svg width="8" height="8" viewBox="0 0 8 8">
            <path d="M1 1L7 7M7 1L1 7" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
          </svg>
        </button>
      </div>
    {/each}
  </div>

  {#if tasks.length < 5}
    <button class="add-btn" onclick={addTask}>+ 添加任务</button>
  {/if}
</div>

<style>
  .panel {
    width: 100%;
    height: 100%;
    background: rgb(30, 30, 30);
    border-radius: 10px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    display: flex;
    flex-direction: column;
    padding: 8px;
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif;
    color: rgba(255, 255, 255, 0.9);
    user-select: none;
  }

  .header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 6px;
    padding: 2px 2px;
    cursor: grab;
  }
  .title {
    flex: 1;
    font-size: 10px;
    font-weight: 600;
    color: rgba(255, 255, 255, 0.4);
    letter-spacing: 1px;
    cursor: grab;
    padding: 2px 0;
  }
  .close-btn {
    background: none;
    border: none;
    color: rgba(255, 255, 255, 0.3);
    cursor: pointer;
    width: 20px;
    height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
    transition: background 80ms, color 80ms;
  }
  .close-btn:hover {
    background: rgba(255, 80, 80, 0.4);
    color: rgba(255, 255, 255, 0.9);
  }

  .task-list {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 3px;
    overflow-y: auto;
  }

  .task-row {
    display: flex;
    align-items: center;
    gap: 4px;
    background: rgba(255, 255, 255, 0.05);
    border-radius: 5px;
    padding: 5px 6px;
    transition: background 80ms;
    border: 1px solid transparent;
  }
  .task-row:hover {
    background: rgba(255, 255, 255, 0.1);
  }
  .task-row.dragging {
    opacity: 0.4;
  }
  .task-row.drop-above {
    border-top: 2px solid rgba(100, 160, 255, 0.7);
  }
  .task-row.drop-below {
    border-bottom: 2px solid rgba(100, 160, 255, 0.7);
  }

  .grip {
    cursor: grab;
    color: rgba(255, 255, 255, 0.2);
    font-size: 11px;
    flex-shrink: 0;
    width: 14px;
    text-align: center;
    touch-action: none;
  }
  .grip:active { cursor: grabbing; }

  .task-input {
    flex: 1;
    background: none;
    border: none;
    outline: none;
    color: rgba(255, 255, 255, 0.88);
    font-size: 12px;
    font-family: inherit;
    padding: 2px 0;
    min-width: 0;
  }
  .task-input::placeholder {
    color: rgba(255, 255, 255, 0.2);
  }
  .task-input:focus {
    color: #fff;
  }

  .del-btn {
    background: none;
    border: none;
    color: rgba(255, 255, 255, 0.15);
    cursor: pointer;
    width: 18px;
    height: 18px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
    flex-shrink: 0;
    transition: background 80ms, color 80ms;
  }
  .del-btn:hover {
    background: rgba(255, 80, 80, 0.3);
    color: rgba(255, 120, 120, 0.9);
  }

  .add-btn {
    background: rgba(255, 255, 255, 0.04);
    border: 1px dashed rgba(255, 255, 255, 0.12);
    border-radius: 5px;
    color: rgba(255, 255, 255, 0.35);
    font-size: 11px;
    font-family: inherit;
    padding: 6px;
    cursor: pointer;
    margin-top: 4px;
    transition: background 80ms, color 80ms;
  }
  .add-btn:hover {
    background: rgba(255, 255, 255, 0.1);
    color: rgba(255, 255, 255, 0.7);
  }
</style>
