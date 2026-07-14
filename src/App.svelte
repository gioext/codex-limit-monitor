<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { RefreshCw } from '@lucide/svelte';

  type CodexLimitData = {
    remainingPercent: number;
    usedPercent: number;
    weeklyResetAt: number;
    windowDurationMins: number;
    resetAvailableCount: number;
    resetExpiries: number[];
    expiryDetailsAvailable: boolean;
    planType: string | null;
    lastUpdatedAt: number;
  };

  let data = $state<CodexLimitData | null>(null);
  let errorMessage = $state<string | null>(null);
  let isRefreshing = $state(false);

  const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

  function previewData(): CodexLimitData {
    const now = Math.floor(Date.now() / 1000);
    return {
      remainingPercent: 90,
      usedPercent: 10,
      weeklyResetAt: now + 6 * 86_400 + 18 * 3_600,
      windowDurationMins: 10_080,
      resetAvailableCount: 3,
      resetExpiries: [now + 12 * 86_400, now + 17 * 86_400, now + 29 * 86_400],
      expiryDetailsAvailable: true,
      planType: 'pro',
      lastUpdatedAt: now,
    };
  }

  function applyData(nextData: CodexLimitData) {
    data = nextData;
    errorMessage = null;
  }

  async function fetchLimits() {
    if (isRefreshing) return;
    isRefreshing = true;
    try {
      if (import.meta.env.DEV && !isTauri) {
        await new Promise((resolve) => setTimeout(resolve, 250));
        applyData(previewData());
      } else {
        applyData(await invoke<CodexLimitData>('fetch_codex_limit_data'));
      }
    } catch (error) {
      errorMessage = String(error);
    } finally {
      isRefreshing = false;
    }
  }

  onMount(() => {
    fetchLimits();

    if (!isTauri) return;

    const unlistenUpdated = listen<CodexLimitData>('limits-updated', (event) => {
      applyData(event.payload);
    });
    const unlistenError = listen<string>('limits-error', (event) => {
      errorMessage = event.payload;
    });
    const unlistenRefresh = listen('force-refresh', () => fetchLimits());

    return () => {
      unlistenUpdated.then((unlisten) => unlisten());
      unlistenError.then((unlisten) => unlisten());
      unlistenRefresh.then((unlisten) => unlisten());
    };
  });

  function handleDragStart(event: MouseEvent) {
    if (event.button !== 0 || !isTauri) return;
    event.preventDefault();
    getCurrentWindow().startDragging();
  }

  function formatDateTime(timestamp: number, includeYear = false) {
    return new Intl.DateTimeFormat('ja-JP', {
      ...(includeYear ? { year: 'numeric' as const } : {}),
      month: 'numeric',
      day: 'numeric',
      weekday: 'short',
      hour: '2-digit',
      minute: '2-digit',
      hour12: false,
    }).format(new Date(timestamp * 1000));
  }

  function formatSyncTime(timestamp: number) {
    return new Intl.DateTimeFormat('ja-JP', {
      hour: '2-digit',
      minute: '2-digit',
      hour12: false,
    }).format(new Date(timestamp * 1000));
  }

  function formatRemaining(timestamp: number) {
    const totalMinutes = Math.max(0, Math.ceil((timestamp * 1000 - Date.now()) / 60_000));
    const days = Math.floor(totalMinutes / 1_440);
    const hours = Math.floor((totalMinutes % 1_440) / 60);
    if (days > 0) return `あと${days}日${hours}時間`;
    if (hours > 0) return `あと${hours}時間`;
    return `あと${totalMinutes}分`;
  }

  function remainingTone(percent: number) {
    if (percent <= 15) return 'critical';
    if (percent <= 35) return 'warning';
    return 'healthy';
  }
</script>

<main class="widget">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <header class="widget-header" onmousedown={handleDragStart}>
    <div class="widget-title">
      <span class:error={Boolean(errorMessage)} class="status-dot"></span>
      CODEX LIMIT MONITOR
    </div>
    <div class="header-actions">
      {#if data}
        <span class="sync-time">Sync: {formatSyncTime(data.lastUpdatedAt)}</span>
      {/if}
      <button
        type="button"
        class="refresh-button"
        class:refreshing={isRefreshing}
        aria-label="利用枠を更新"
        title="利用枠を更新"
        disabled={isRefreshing}
        onmousedown={(event) => event.stopPropagation()}
        onclick={fetchLimits}
      >
        <RefreshCw size={13} strokeWidth={1.8} />
      </button>
    </div>
  </header>

  <section class="content-area" aria-live="polite">
    {#if data}
      <div class="usage-summary">
        <div>
          <p class="eyebrow">1週間の残量</p>
          <div class="usage-value-row">
            <strong class="usage-value">{data.remainingPercent}<span>%</span></strong>
            <span class="usage-note">使用済み {data.usedPercent}%</span>
          </div>
        </div>
        <span class="window-label">7 DAYS</span>
      </div>

      <div
        class="limit-bar {remainingTone(data.remainingPercent)}"
        role="progressbar"
        aria-label="1週間の利用枠残量"
        aria-valuemin="0"
        aria-valuemax="100"
        aria-valuenow={data.remainingPercent}
      >
        <span style={`width: ${data.remainingPercent}%`}></span>
      </div>

      <div class="reset-row">
        <div>
          <p class="row-label">次回のリセット</p>
          <p class="row-value">{formatDateTime(data.weeklyResetAt, true)}</p>
        </div>
        <span class="countdown">{formatRemaining(data.weeklyResetAt)}</span>
      </div>

      <div class="divider"></div>

      <section class="credit-section" aria-labelledby="credit-title">
        <div class="credit-heading">
          <div>
            <p class="eyebrow" id="credit-title">利用枠リセット</p>
            <p class="credit-description">現在の週次利用枠を全回復できます</p>
          </div>
          <div class="credit-count-wrap">
            <button
              type="button"
              class="credit-count-trigger"
              aria-label={`利用枠リセット ${data.resetAvailableCount}回。有効期限を表示`}
              aria-describedby="reset-expiry-tooltip"
            >
              <strong class="credit-count">{data.resetAvailableCount}<span>回</span></strong>
            </button>
            <div class="expiry-tooltip" id="reset-expiry-tooltip" role="tooltip">
              <p class="tooltip-title">リセットの有効期限</p>
              {#if data.resetAvailableCount === 0}
                <p class="tooltip-empty">使用できるリセットはありません</p>
              {:else if data.expiryDetailsAvailable && data.resetExpiries.length > 0}
                <div class="tooltip-expiries">
                  {#each data.resetExpiries as expiry, index}
                    <div class="tooltip-expiry-row">
                      <span>{index + 1}回目</span>
                      <time datetime={new Date(expiry * 1000).toISOString()}>{formatDateTime(expiry)}</time>
                    </div>
                  {/each}
                </div>
              {:else}
                <p class="tooltip-empty">有効期限を取得できません</p>
              {/if}
            </div>
          </div>
        </div>
      </section>

      <footer class="widget-footer">
        <span>Codex {data.planType?.toUpperCase() ?? ''}</span>
        <span>5分ごとに自動更新</span>
      </footer>
    {:else if errorMessage}
      <div class="error-state">
        <span class="error-mark">!</span>
        <p>Codexの利用枠を取得できません</p>
        <span>{errorMessage}</span>
        <button type="button" onclick={fetchLimits}>再読み込み</button>
      </div>
    {:else}
      <div class="loading-state" aria-label="読み込み中">
        <span class="spinner"></span>
      </div>
    {/if}
  </section>
</main>
