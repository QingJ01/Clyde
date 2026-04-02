<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { getCurrentWindow } from '@tauri-apps/api/window';

  interface Session {
    id: string;
    state: string;
    agent: string;
    pid: number | null;
    updated_secs_ago: number;
  }

  interface MenuData {
    sessions: Session[];
    is_dnd: boolean;
    is_mini: boolean;
    lang: string;
    size: string;
    opacity: number;
    position_locked: boolean;
    click_through: boolean;
    auto_hide_fullscreen: boolean;
    auto_dnd_meetings: boolean;
    auto_start_with_claude: boolean;
    environment_controls_supported: boolean;
  }

  let data: MenuData | null = $state(null);
  let activeSubmenu: string | null = $state(null);
  let closing = false;

  const stateIcons: Record<string, string> = {
    working: '⚡', typing: '⚡', thinking: '💭', juggling: '🎪',
    idle: '💤', sleeping: '😴', error: '⚠️',
  };
  const stateLabelsEn: Record<string, string> = {
    working: 'Working', typing: 'Working', thinking: 'Thinking',
    juggling: 'Juggling', idle: 'Idle', sleeping: 'Sleeping',
  };
  const stateLabelsZh: Record<string, string> = {
    working: '工作中', typing: '工作中', thinking: '思考中',
    juggling: '多任务', idle: '空闲', sleeping: '睡眠',
  };

  function t(key: string): string {
    if (!data) return key;
    const zh: Record<string, string> = {
      size: '大小', miniMode: '极简模式', dnd: '勿扰模式',
      opacity: '透明度', lockPosition: '锁定位置', clickThrough: '点击穿透',
      hideOnFullscreen: '全屏时自动隐藏', autoDndMeetings: '会议/共享时自动勿扰',
      autoStart: '随 Claude Code 启动', sessions: '会话', language: '语言', quit: '退出',
      noSessions: '没有活跃会话', justNow: '刚刚', macOnly: '仅 macOS',
    };
    const en: Record<string, string> = {
      size: 'Size', miniMode: 'Mini Mode', dnd: 'Sleep (Do Not Disturb)',
      opacity: 'Opacity', lockPosition: 'Lock Position', clickThrough: 'Click Through',
      hideOnFullscreen: 'Hide on Fullscreen', autoDndMeetings: 'Auto DND During Meetings',
      autoStart: 'Start with Claude Code', sessions: 'Sessions', language: 'Language', quit: 'Quit',
      noSessions: 'No active sessions', justNow: 'just now', macOnly: 'macOS only',
    };
    return (data.lang === 'zh' ? zh[key] : en[key]) ?? key;
  }

  function stateLabel(s: string): string {
    if (!data) return s;
    return (data.lang === 'zh' ? stateLabelsZh[s] : stateLabelsEn[s]) ?? s;
  }

  function platformLimitedLabel(key: string): string {
    const label = t(key);
    return data?.environment_controls_supported ? label : `${label} (${t('macOnly')})`;
  }

  function sessionAgeLabel(seconds: number): string {
    if (!data) return t('justNow');
    if (seconds < 5) return t('justNow');
    if (data.lang === 'zh') {
      if (seconds < 60) return `${seconds}秒前`;
      if (seconds < 3600) return `${Math.floor(seconds / 60)}分钟前`;
      if (seconds < 86400) return `${Math.floor(seconds / 3600)}小时前`;
      return `${Math.floor(seconds / 86400)}天前`;
    }

    if (seconds < 60) return `${seconds}s ago`;
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
    if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
    return `${Math.floor(seconds / 86400)}d ago`;
  }

  function agentMeta(agent: string): { label: string; kind: string } {
    const normalized = agent.trim().toLowerCase();
    if (normalized.includes('claude')) return { label: 'Claude', kind: 'claude' };
    if (normalized.includes('codex')) return { label: 'Codex', kind: 'codex' };
    if (normalized.includes('copilot')) return { label: 'Copilot', kind: 'copilot' };
    if (!normalized) return { label: 'Unknown', kind: 'unknown' };
    return { label: agent.trim(), kind: 'generic' };
  }

  async function action(id: string) {
    if (closing) return;
    closing = true;
    await invoke('menu_action', { id });
    closeMenu();
  }

  async function closeMenu() {
    try { await getCurrentWindow().close(); } catch {}
  }

  function toggleSubmenu(name: string) {
    activeSubmenu = activeSubmenu === name ? null : name;
  }

  onMount(() => {
    let unlistenFocus: (() => void) | undefined;

    const setup = async () => {
      data = await invoke('get_menu_data');

      // Close on blur (click outside)
      const win = getCurrentWindow();
      unlistenFocus = await win.onFocusChanged(({ payload: focused }) => {
        if (!focused && !closing) closeMenu();
      });

      // Close on Escape
      const onKey = (e: KeyboardEvent) => {
        if (e.key === 'Escape') closeMenu();
      };
      window.addEventListener('keydown', onKey);

      return () => {
        unlistenFocus?.();
        window.removeEventListener('keydown', onKey);
      };
    };

    let cleanup: (() => void) | undefined;
    setup().then((fn) => { cleanup = fn; });

    return () => {
      cleanup?.();
    };
  });
</script>

{#if data}
<div class="menu">
  <!-- Size -->
  <div class="item has-sub" role="button" tabindex="-1" onmouseenter={() => activeSubmenu = 'size'} onmouseleave={() => activeSubmenu = null}>
    <span>{t('size')}</span>
    <span class="arrow">›</span>
    {#if activeSubmenu === 'size'}
      <div class="submenu">
        <button class="item" class:checked={data.size === 'S'} onclick={() => action('size-s')}>S</button>
        <button class="item" class:checked={data.size === 'M'} onclick={() => action('size-m')}>M</button>
        <button class="item" class:checked={data.size === 'L'} onclick={() => action('size-l')}>L</button>
      </div>
    {/if}
  </div>

  <!-- Mini Mode -->
  <button class="item" onclick={() => action('mini')}>
    <span>{t('miniMode')}</span>
    {#if data.is_mini}<span class="check">✓</span>{/if}
  </button>

  <!-- Opacity -->
  <div class="item has-sub" role="button" tabindex="-1" onmouseenter={() => activeSubmenu = 'opacity'} onmouseleave={() => activeSubmenu = null}>
    <span>{t('opacity')}</span>
    <span class="value">{data.opacity}%</span>
    <span class="arrow">›</span>
    {#if activeSubmenu === 'opacity'}
      <div class="submenu">
        {#each [100, 90, 80, 70, 60, 50, 40] as level}
          <button class="item" class:checked={data.opacity === level} onclick={() => action(`opacity-${level}`)}>{level}%</button>
        {/each}
      </div>
    {/if}
  </div>

  <button class="item" onclick={() => action('lock-position')}>
    <span>{t('lockPosition')}</span>
    {#if data.position_locked}<span class="check">✓</span>{/if}
  </button>

  <button class="item" onclick={() => action('click-through')}>
    <span>{t('clickThrough')}</span>
    {#if data.click_through}<span class="check">✓</span>{/if}
  </button>

  <button
    class="item"
    class:disabled={!data.environment_controls_supported}
    onclick={() => action('hide-on-fullscreen')}
    disabled={!data.environment_controls_supported}
  >
    <span>{platformLimitedLabel('hideOnFullscreen')}</span>
    {#if data.auto_hide_fullscreen}<span class="check">✓</span>{/if}
  </button>

  <button
    class="item"
    class:disabled={!data.environment_controls_supported}
    onclick={() => action('auto-dnd-meetings')}
    disabled={!data.environment_controls_supported}
  >
    <span>{platformLimitedLabel('autoDndMeetings')}</span>
    {#if data.auto_dnd_meetings}<span class="check">✓</span>{/if}
  </button>

  <!-- DND -->
  <button class="item" onclick={() => action('dnd')}>
    <span>{t('dnd')}</span>
    {#if data.is_dnd}<span class="check">✓</span>{/if}
  </button>

  <button class="item" onclick={() => action('autostart')}>
    <span>{t('autoStart')}</span>
    {#if data.auto_start_with_claude}<span class="check">✓</span>{/if}
  </button>

  <div class="sep"></div>

  <!-- Sessions -->
  <div class="item has-sub" role="button" tabindex="-1" onmouseenter={() => activeSubmenu = 'sessions'} onmouseleave={() => activeSubmenu = null}>
    <span>{t('sessions')} ({data.sessions.length})</span>
    <span class="arrow">›</span>
    {#if activeSubmenu === 'sessions'}
      <div class="submenu submenu-sessions">
        {#if data.sessions.length === 0}
          <div class="item disabled">{t('noSessions')}</div>
        {:else}
          {#each data.sessions as sess}
            {@const meta = agentMeta(sess.agent)}
            <button class="item session-item" onclick={() => action(`session-${sess.id}`)}>
              <span class="session-icon">{stateIcons[sess.state] ?? '⚡'}</span>
              <span class={`session-agent agent-${meta.kind}`}>{meta.label}</span>
              <span class="session-state">{stateLabel(sess.state)}</span>
              <span class="session-time">{sessionAgeLabel(sess.updated_secs_ago)}</span>
            </button>
          {/each}
        {/if}
      </div>
    {/if}
  </div>

  <div class="sep"></div>

  <!-- Language -->
  <div class="item has-sub" role="button" tabindex="-1" onmouseenter={() => activeSubmenu = 'language'} onmouseleave={() => activeSubmenu = null}>
    <span>{t('language')}</span>
    <span class="arrow">›</span>
    {#if activeSubmenu === 'language'}
      <div class="submenu">
        <button class="item" class:checked={data.lang === 'en'} onclick={() => action('lang-en')}>English</button>
        <button class="item" class:checked={data.lang === 'zh'} onclick={() => action('lang-zh')}>中文</button>
      </div>
    {/if}
  </div>

  <!-- Quit -->
  <button class="item quit" onclick={() => action('quit')}>
    <span>{t('quit')}</span>
  </button>
</div>
{/if}

<style>
  .menu {
    background: rgba(255, 255, 255, 0.82);
    backdrop-filter: blur(24px) saturate(180%);
    -webkit-backdrop-filter: blur(24px) saturate(180%);
    border-radius: 12px;
    border: 1px solid rgba(0, 0, 0, 0.08);
    box-shadow:
      0 12px 40px rgba(0, 0, 0, 0.12),
      0 2px 8px rgba(0, 0, 0, 0.06);
    padding: 6px;
    min-width: 220px;
    user-select: none;
    -webkit-user-select: none;
  }

  .item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 8px 14px;
    border-radius: 8px;
    font-size: 13.5px;
    color: #1d1d1f;
    cursor: pointer;
    border: none;
    background: none;
    text-align: left;
    position: relative;
    transition: background 0.1s;
    letter-spacing: -0.01em;
  }
  .item:hover {
    background: rgba(0, 0, 0, 0.06);
  }
  .item.disabled {
    color: #999;
    cursor: default;
  }
  .item:disabled {
    color: #999;
    cursor: default;
  }
  .item.disabled:hover {
    background: none;
  }
  .item:disabled:hover {
    background: none;
  }
  .item.quit {
    color: #e53e3e;
  }
  .item.checked::after {
    content: '✓';
    font-size: 12px;
    color: #3b82f6;
    margin-left: 8px;
  }

  .check {
    font-size: 12px;
    color: #3b82f6;
  }

  .arrow {
    font-size: 16px;
    color: #aaa;
    font-weight: 300;
  }

  .value {
    margin-left: auto;
    margin-right: 10px;
    color: #6b7280;
    font-size: 12px;
  }

  .sep {
    height: 1px;
    background: rgba(0, 0, 0, 0.08);
    margin: 4px 10px;
  }

  .has-sub {
    cursor: default;
  }

  .submenu {
    position: absolute;
    left: calc(100% + 6px);
    top: -6px;
    background: rgba(255, 255, 255, 0.88);
    backdrop-filter: blur(24px) saturate(180%);
    -webkit-backdrop-filter: blur(24px) saturate(180%);
    border-radius: 12px;
    border: 1px solid rgba(0, 0, 0, 0.08);
    box-shadow:
      0 12px 40px rgba(0, 0, 0, 0.12),
      0 2px 8px rgba(0, 0, 0, 0.06);
    padding: 6px;
    min-width: 180px;
    z-index: 10;
  }

  .submenu-sessions {
    min-width: 260px;
  }

  .session-item {
    gap: 8px;
    justify-content: flex-start;
  }
  .session-icon {
    font-size: 14px;
    width: 20px;
    text-align: center;
    flex-shrink: 0;
  }
  .session-agent {
    display: inline-flex;
    align-items: center;
    min-width: 58px;
    padding: 3px 8px;
    border-radius: 999px;
    font-weight: 600;
    font-size: 11.5px;
    letter-spacing: 0.01em;
    color: #1d1d1f;
    background: rgba(17, 24, 39, 0.08);
    border: 1px solid rgba(17, 24, 39, 0.08);
  }
  .session-agent.agent-claude {
    color: #7c2d12;
    background: rgba(251, 146, 60, 0.18);
    border-color: rgba(249, 115, 22, 0.2);
  }
  .session-agent.agent-codex {
    color: #0f4c5c;
    background: rgba(45, 212, 191, 0.18);
    border-color: rgba(13, 148, 136, 0.2);
  }
  .session-agent.agent-copilot {
    color: #1d4ed8;
    background: rgba(96, 165, 250, 0.18);
    border-color: rgba(59, 130, 246, 0.2);
  }
  .session-state {
    font-size: 12px;
    color: #666;
  }
  .session-time {
    font-size: 11px;
    color: #aaa;
    margin-left: auto;
  }
</style>
