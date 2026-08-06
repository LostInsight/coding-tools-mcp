<script lang="ts">
  import { confirm, open } from "@tauri-apps/plugin-dialog";
  import { Eye, EyeOff, FolderOpen, PlugZap, RefreshCw, Save, Trash2 } from "@lucide/svelte";

  import {
    clearPaseoMonitorSnapshots,
    listPaseoFilterOptions,
    testPaseoConnection,
    type PaseoConnectionTestResult,
    type PaseoIntegrationSettings,
    type PaseoIntegrationSettingsInput,
    type PaseoFilterOptions,
  } from "$lib/api/paseo";
  import { showToast } from "$lib/stores/toast";
  import { paseoIntegrationConfig, type PaseoIntegrationConfig } from "$lib/types";

  interface Props {
    workspaceId: string;
    settings: PaseoIntegrationSettings;
    onSave: (
      input: PaseoIntegrationSettingsInput,
    ) => Promise<PaseoIntegrationSettings>;
  }

  const READ_ONLY_TOOLS = [
    "paseo_health",
    "paseo_list_agents",
    "paseo_get_agent_activity",
    "paseo_list_pending_permissions",
    "paseo_diagnose_agent",
    "paseo_monitor_snapshot",
  ];

  let { workspaceId, settings, onSave }: Props = $props();

  let draft = $state<PaseoIntegrationConfig>(paseoIntegrationConfig());
  let connectionType = $state<"local" | "remote">("local");
  let host = $state("");
  let hostVisible = $state(false);
  let includePatterns = $state("");
  let labels = $state("");
  let excludePatterns = $state("");
  let saving = $state(false);
  let testing = $state(false);
  let clearing = $state(false);
  let testResult = $state<PaseoConnectionTestResult | null>(null);
  let filterOptions = $state<PaseoFilterOptions | null>(null);
  let loadingFilters = $state(false);

  const exposedTools = $derived.by(() => {
    if (!draft.enabled) return [];
    const tools = [...READ_ONLY_TOOLS];
    if (draft.access_mode === "assist" || draft.access_mode === "control") {
      tools.push("paseo_send_agent_prompt");
    }
    if (draft.access_mode === "control") {
      tools.push(
        "paseo_stop_agent",
        "paseo_allow_permission",
        "paseo_deny_permission",
        "paseo_create_agent",
      );
    }
    return tools;
  });

  const dirty = $derived(
    JSON.stringify(buildConfig()) !== JSON.stringify(settings.config) ||
      connectionType !== settings.connection_type ||
      (connectionType === "remote" && host !== (settings.host ?? "")),
  );

  $effect(() => {
    draft = cloneConfig(settings.config);
    connectionType = settings.connection_type;
    host = settings.host ?? "";
    hostVisible = false;
    testResult = null;
    includePatterns = settings.config.monitor.agent_name_patterns.join(", ");
    labels = settings.config.monitor.agent_labels.join(", ");
    excludePatterns = settings.config.monitor.exclude_name_patterns.join(", ");
  });

  function cloneConfig(config: PaseoIntegrationConfig): PaseoIntegrationConfig {
    return {
      ...config,
      monitor: {
        ...config.monitor,
        agent_name_patterns: [...config.monitor.agent_name_patterns],
        agent_labels: [...config.monitor.agent_labels],
        agent_ids: [...config.monitor.agent_ids],
        workspaces: [...config.monitor.workspaces],
        exclude_name_patterns: [...config.monitor.exclude_name_patterns],
      },
    };
  }

  function splitValues(value: string): string[] {
    return value
      .split(",")
      .map((item) => item.trim())
      .filter(Boolean);
  }

  function buildConfig(): PaseoIntegrationConfig {
    return {
      ...draft,
      host_configured: connectionType === "remote" && host.trim().length > 0,
      monitor: {
        ...draft.monitor,
        agent_name_patterns: splitValues(includePatterns),
        agent_labels: splitValues(labels),
        exclude_name_patterns: splitValues(excludePatterns),
      },
    };
  }

  function buildInput(): PaseoIntegrationSettingsInput {
    return {
      config: buildConfig(),
      host: connectionType === "remote" ? host.trim() : "",
    };
  }

  async function chooseBinary() {
    const selected = await open({
      title: "选择 Paseo CLI",
      directory: false,
      multiple: false,
    });
    if (typeof selected === "string") {
      draft.binary_path = selected;
      testResult = null;
    }
  }

  async function testConnection() {
    if (testing) return;
    testing = true;
    testResult = null;
    try {
      testResult = await testPaseoConnection(workspaceId, buildInput());
      showToast(testResult.ok ? "Paseo 连接正常" : "Paseo 连接失败", {
        kind: testResult.ok ? "success" : "warning",
      });
    } catch (error) {
      testResult = { ok: false, error: { message: String(error) } };
      showToast("Paseo 连接测试失败", { kind: "error" });
    } finally {
      testing = false;
    }
  }

  function toggleValue(values: string[], value: string): string[] {
    return values.includes(value)
      ? values.filter((item) => item !== value)
      : [...values, value];
  }

  async function refreshFilterOptions() {
    if (loadingFilters) return;
    loadingFilters = true;
    try {
      filterOptions = await listPaseoFilterOptions(workspaceId, buildInput());
      showToast("Paseo Agent 列表已刷新", { kind: "success" });
    } catch (error) {
      showToast(`读取 Paseo Agent 失败：${String(error)}`, { kind: "error" });
    } finally {
      loadingFilters = false;
    }
  }

  async function save() {
    if (saving || !dirty) return;
    saving = true;
    try {
      await onSave(buildInput());
      showToast("Paseo Integration 配置已保存", { kind: "success" });
    } catch (error) {
      showToast(`Paseo 配置保存失败：${String(error)}`, { kind: "error" });
    } finally {
      saving = false;
    }
  }

  async function clearSnapshots() {
    if (clearing) return;
    const approved = await confirm("清除当前工作区的 Paseo 监控快照？", {
      title: "清除监控快照",
      kind: "warning",
      okLabel: "清除",
      cancelLabel: "取消",
    });
    if (!approved) return;
    clearing = true;
    try {
      const removed = await clearPaseoMonitorSnapshots(workspaceId);
      showToast(`已清除 ${removed} 个 Paseo 监控快照`, { kind: "success" });
    } finally {
      clearing = false;
    }
  }
</script>

<form
  class="grid gap-5"
  onsubmit={(event) => {
    event.preventDefault();
    void save();
  }}
>
  <div class="flex flex-wrap items-center justify-between gap-3">
    <label class="flex items-center gap-2 text-sm font-medium">
      <input type="checkbox" bind:checked={draft.enabled} />
      <span>启用 Paseo Integration</span>
    </label>
    <span class="text-xs text-[var(--color-text-muted)]">
      {exposedTools.length} 个 MCP 工具
    </span>
  </div>

  <div class="grid gap-4 md:grid-cols-2">
    <label class="grid gap-1">
      <span class="text-xs text-[var(--color-text-muted)]">访问模式</span>
      <select
        class="rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-2.5 py-1.5 text-sm"
        bind:value={draft.access_mode}
        disabled={!draft.enabled}
      >
        <option value="read_only">Read only</option>
        <option value="assist">Assist</option>
        <option value="control">Control</option>
      </select>
    </label>

    <div class="grid gap-1">
      <span class="text-xs text-[var(--color-text-muted)]">连接</span>
      <div class="grid grid-cols-2 rounded-md border border-[var(--color-border)] p-0.5">
        <button
          type="button"
          class="rounded px-2 py-1 text-sm"
          class:bg-[var(--color-surface-hover)]={connectionType === "local"}
          onclick={() => (connectionType = "local")}
        >Local daemon</button>
        <button
          type="button"
          class="rounded px-2 py-1 text-sm"
          class:bg-[var(--color-surface-hover)]={connectionType === "remote"}
          onclick={() => (connectionType = "remote")}
        >Remote host</button>
      </div>
    </div>
  </div>

  <div class="grid gap-1">
    <span class="text-xs text-[var(--color-text-muted)]">Paseo binary</span>
    <div class="flex gap-2">
      <input
        type="text"
        class="min-w-0 flex-1 rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-2.5 py-1.5 font-mono text-sm"
        placeholder="自动检测（留空）"
        bind:value={draft.binary_path}
      />
      <button
        type="button"
        class="tx-btn-ghost inline-flex h-9 w-9 items-center justify-center"
        title="选择 Paseo CLI"
        onclick={() => void chooseBinary()}
      ><FolderOpen size={16} /></button>
    </div>
  </div>

  {#if connectionType === "remote"}
    <label class="grid gap-1">
      <span class="text-xs text-[var(--color-text-muted)]">Remote host</span>
      <div class="relative">
        <input
          type={hostVisible ? "text" : "password"}
          class="w-full rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] py-1.5 pl-2.5 pr-10 font-mono text-sm"
          placeholder="host:port 或 tcp://host:port"
          bind:value={host}
          autocomplete="off"
        />
        <button
          type="button"
          class="absolute inset-y-0 right-0 flex w-9 items-center justify-center text-[var(--color-text-muted)]"
          title={hostVisible ? "隐藏 host" : "显示 host"}
          onclick={() => (hostVisible = !hostVisible)}
        >
          {#if hostVisible}<EyeOff size={15} />{:else}<Eye size={15} />{/if}
        </button>
      </div>
      {#if settings.host_display}
        <span class="text-xs text-[var(--color-text-muted)]">已配置：{settings.host_display}</span>
      {/if}
    </label>
  {/if}

  <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
    <label class="grid gap-1">
      <span class="text-xs text-[var(--color-text-muted)]">命令超时（ms）</span>
      <input type="number" min="1000" max="120000" step="1000" class="rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-2.5 py-1.5 text-sm" bind:value={draft.command_timeout_ms} />
    </label>
    <label class="grid gap-1">
      <span class="text-xs text-[var(--color-text-muted)]">Activity tail</span>
      <input type="number" min="1" max="100" class="rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-2.5 py-1.5 text-sm" bind:value={draft.monitor.activity_tail} />
    </label>
    <label class="grid gap-1">
      <span class="text-xs text-[var(--color-text-muted)]">停滞阈值（分钟）</span>
      <input type="number" min="1" max="240" class="rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-2.5 py-1.5 text-sm" bind:value={draft.monitor.stalled_after_minutes} />
    </label>
    <label class="grid gap-1">
      <span class="text-xs text-[var(--color-text-muted)]">重复错误阈值</span>
      <input type="number" min="1" max="10" class="rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-2.5 py-1.5 text-sm" bind:value={draft.monitor.repeat_error_threshold} />
    </label>
  </div>

  <div class="grid gap-4 md:grid-cols-3">
    <label class="grid gap-1">
      <span class="text-xs text-[var(--color-text-muted)]">包含名称（glob，逗号分隔）</span>
      <input type="text" class="rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-2.5 py-1.5 text-sm" bind:value={includePatterns} />
    </label>
    <label class="grid gap-1">
      <span class="text-xs text-[var(--color-text-muted)]">Agent labels（逗号分隔）</span>
      <input type="text" class="rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-2.5 py-1.5 text-sm" bind:value={labels} />
    </label>
    <label class="grid gap-1">
      <span class="text-xs text-[var(--color-text-muted)]">排除名称（glob，逗号分隔）</span>
      <input type="text" class="rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-2.5 py-1.5 text-sm" bind:value={excludePatterns} />
    </label>
  </div>

  <div class="grid gap-3 border-t border-[var(--color-border)] pt-4">
    <div class="flex items-center justify-between gap-3">
      <span class="text-sm font-medium">Agent 与 Workspace 筛选</span>
      <button type="button" class="tx-btn-ghost inline-flex h-9 w-9 items-center justify-center" title="刷新 Paseo 列表" disabled={loadingFilters || !draft.enabled} onclick={() => void refreshFilterOptions()}>
        <RefreshCw size={16} class={loadingFilters ? "animate-spin" : ""} />
      </button>
    </div>
    {#if filterOptions}
      <div class="grid gap-4 lg:grid-cols-2">
        <fieldset class="grid max-h-48 content-start gap-2 overflow-auto">
          <legend class="mb-2 text-xs text-[var(--color-text-muted)]">Agent IDs</legend>
          {#each filterOptions.agents as agent (agent.id)}
            <label class="flex min-w-0 items-start gap-2 text-sm">
              <input type="checkbox" checked={draft.monitor.agent_ids.includes(agent.id)} onchange={() => (draft.monitor.agent_ids = toggleValue(draft.monitor.agent_ids, agent.id))} />
              <span class="min-w-0">
                <span class="block truncate">{agent.name ?? agent.id}</span>
                <code class="block truncate text-xs text-[var(--color-text-muted)]">{agent.id}</code>
              </span>
            </label>
          {/each}
        </fieldset>
        <fieldset class="grid max-h-48 content-start gap-2 overflow-auto">
          <legend class="mb-2 text-xs text-[var(--color-text-muted)]">Workspaces</legend>
          {#each filterOptions.workspaces as workspace (workspace)}
            <label class="flex min-w-0 items-start gap-2 text-sm">
              <input type="checkbox" checked={draft.monitor.workspaces.includes(workspace)} onchange={() => (draft.monitor.workspaces = toggleValue(draft.monitor.workspaces, workspace))} />
              <code class="min-w-0 truncate text-xs">{workspace}</code>
            </label>
          {/each}
        </fieldset>
      </div>
    {/if}
  </div>

  <div class="flex flex-wrap gap-x-5 gap-y-2 text-sm">
    <label class="flex items-center gap-2"><input type="checkbox" bind:checked={draft.monitor.enabled} /><span>启用监控快照</span></label>
    <label class="flex items-center gap-2"><input type="checkbox" bind:checked={draft.monitor.workspace_only} /><span>仅当前工作区代理</span></label>
  </div>

  {#if draft.enabled && draft.access_mode !== "read_only"}
    <div class="rounded-md border border-[var(--warning)]/40 bg-[var(--warning)]/10 px-3 py-2 text-xs text-[var(--color-text-secondary)]">
      {draft.access_mode === "control"
        ? "Control 可发送提示、停止代理、精确批准或拒绝权限请求，并在当前工作区创建后台代理；所有控制操作均要求显式确认。"
        : "Assist 可向现有代理发送文本提示；不会批准权限或自动停止代理。"}
    </div>
  {/if}

  {#if exposedTools.length > 0}
    <div class="flex flex-wrap gap-1.5">
      {#each exposedTools as tool}
        <code class="rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-1.5 py-0.5 text-xs">{tool}</code>
      {/each}
    </div>
  {/if}

  {#if testResult}
    <div class="text-xs text-[var(--color-text-secondary)]" role="status">
      {testResult.ok
        ? `CLI ${testResult.cli_version ?? "unknown"} · daemon reachable · ${testResult.connection_type ?? connectionType}`
        : `${testResult.error?.code ?? "PASEO_TEST_FAILED"}: ${testResult.error?.message ?? "连接不可用"}`}
    </div>
  {/if}

  <div class="flex flex-wrap items-center justify-between gap-2 border-t border-[var(--color-border)] pt-4">
    <button type="button" class="tx-btn-ghost inline-flex items-center gap-1.5 text-[var(--danger)]" disabled={clearing} onclick={() => void clearSnapshots()}>
      <Trash2 size={15} />{clearing ? "清除中…" : "清除快照"}
    </button>
    <div class="flex flex-wrap gap-2">
      <button type="button" class="tx-btn-ghost inline-flex items-center gap-1.5" disabled={testing || !draft.enabled} onclick={() => void testConnection()}>
        <PlugZap size={15} />{testing ? "测试中…" : "测试连接"}
      </button>
      <button type="submit" class="inline-flex items-center gap-1.5 rounded-md bg-[var(--color-accent)] px-3 py-1.5 text-sm font-medium text-white disabled:opacity-50" disabled={saving || !dirty}>
        <Save size={15} />{saving ? "保存中…" : "保存 Paseo"}
      </button>
    </div>
  </div>
</form>
