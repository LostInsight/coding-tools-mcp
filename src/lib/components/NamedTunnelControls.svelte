<script lang="ts">
  import { onMount } from "svelte";
  import {
    getTunnelStatus,
    restartTunnel,
    startTunnel,
    stopTunnel,
    type TunnelService,
    type TunnelStatus,
  } from "$lib/api/tunnel";
  import { showToast } from "$lib/stores/toast";

  interface Props {
    workspaceId: string;
    service: TunnelService;
    disabled?: boolean;
  }

  let { workspaceId, service, disabled = false }: Props = $props();
  let status = $state<TunnelStatus | null>(null);
  let busy = $state(false);

  const running = $derived(status?.state === "running");

  function statusLabel(): string {
    if (status?.state === "running") return "运行中";
    if (status?.state === "error") return "错误";
    return "已停止";
  }

  async function refresh() {
    try {
      status = await getTunnelStatus(workspaceId, service);
    } catch (error) {
      showToast(String(error), { title: "读取隧道状态失败", kind: "error", duration: 8000 });
    }
  }

  async function run(action: "start" | "stop" | "restart") {
    if (busy || disabled) return;
    busy = true;
    try {
      status =
        action === "start"
          ? await startTunnel(workspaceId, service)
          : action === "stop"
            ? await stopTunnel(workspaceId, service)
            : await restartTunnel(workspaceId, service);
      showToast(`Named Tunnel ${statusLabel()}。`, { title: "隧道状态已更新", kind: "success" });
    } catch (error) {
      showToast(String(error), { title: "Named Tunnel 操作失败", kind: "error", duration: 8000 });
      await refresh();
    } finally {
      busy = false;
    }
  }

  onMount(() => {
    void refresh();
  });
</script>

<div class="flex flex-wrap items-center justify-between gap-2 border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2">
  <div class="flex min-w-0 flex-1 flex-wrap items-center gap-2 text-xs">
    <span class="font-medium text-[var(--color-text-secondary)]">Named Tunnel</span>
    <span class:tx-status-running={running} class="text-[var(--color-text-muted)]">{statusLabel()}</span>
    {#if status?.tunnelPid}
      <span class="font-mono text-[11px] text-[var(--color-text-muted)]">PID {status.tunnelPid}</span>
    {/if}
    {#if status?.publicUrl}
      <span class="min-w-0 truncate font-mono text-[11px] text-[var(--color-text-muted)]">{status.publicUrl}</span>
    {/if}
  </div>
  <div class="flex items-center gap-1.5">
    {#if !running}
      <button
        type="button"
        class="tx-btn-ghost px-2.5 py-1 text-xs disabled:opacity-50"
        disabled={busy || disabled}
        onclick={() => void run("start")}
      >
        启动
      </button>
    {:else}
      <button
        type="button"
        class="tx-btn-ghost px-2.5 py-1 text-xs disabled:opacity-50"
        disabled={busy || disabled}
        onclick={() => void run("stop")}
      >
        停止
      </button>
    {/if}
    <button
        type="button"
        class="tx-btn-ghost px-2.5 py-1 text-xs disabled:opacity-50"
        disabled={busy || disabled || !running}
      onclick={() => void run("restart")}
    >
      重启
    </button>
    <button
        type="button"
        class="tx-btn-ghost px-2 py-1 text-xs disabled:opacity-50"
        disabled={busy || disabled}
      onclick={() => void refresh()}
    >
      刷新
    </button>
  </div>
  {#if status?.message}
    <p class="basis-full break-words text-xs text-[var(--color-danger)]">{status.message}</p>
  {/if}
</div>
