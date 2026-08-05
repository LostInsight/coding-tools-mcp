<script lang="ts">
  import { onMount } from "svelte";
  import { message } from "@tauri-apps/plugin-dialog";
  import {
    deleteCloudflareProfile,
    deleteFrpProfile,
    listCloudflareProfiles,
    listFrpProfiles,
    saveCloudflareProfile,
    saveFrpProfile,
    type CloudflareProfileDto,
    type FrpProfileDto,
  } from "$lib/api/settings";
  import SecretInput from "$lib/components/SecretInput.svelte";

  type TunnelConfigTab = "frp" | "cloudflare";

  let activeTab = $state<TunnelConfigTab>("frp");
  let frpProfiles = $state<FrpProfileDto[]>([]);
  let cloudflareProfiles = $state<CloudflareProfileDto[]>([]);
  let frpLoading = $state(true);
  let cloudflareLoading = $state(true);
  let frpSaving = $state(false);
  let cloudflareSaving = $state(false);
  let editingId = $state<string | null>(null);
  let name = $state("");
  let server = $state("");
  let serverPort = $state(7000);
  let token = $state("");
  let cloudflareEditingId = $state<string | null>(null);
  let cloudflareName = $state("");
  let cloudflareAccountId = $state("");
  let cloudflareTunnelId = $state("");
  let cloudflareZoneId = $state("");
  let cloudflareTunnelToken = $state("");
  let cloudflareApiToken = $state("");
  let makeDefault = $state(false);
  let cloudflareMakeDefault = $state(false);

  async function refreshFrp() {
    frpLoading = true;
    try {
      frpProfiles = await listFrpProfiles();
      if (!editingId && !name && frpProfiles.length === 0) {
        makeDefault = true;
      }
    } finally {
      frpLoading = false;
    }
  }

  async function refreshCloudflare() {
    cloudflareLoading = true;
    try {
      cloudflareProfiles = await listCloudflareProfiles();
      if (!cloudflareEditingId && !cloudflareName && cloudflareProfiles.length === 0) {
        cloudflareMakeDefault = true;
      }
    } finally {
      cloudflareLoading = false;
    }
  }

  function resetFrpForm() {
    editingId = null;
    name = "";
    server = "";
    serverPort = 7000;
    token = "";
    makeDefault = false;
  }

  function resetCloudflareForm() {
    cloudflareEditingId = null;
    cloudflareName = "";
    cloudflareAccountId = "";
    cloudflareTunnelId = "";
    cloudflareZoneId = "";
    cloudflareTunnelToken = "";
    cloudflareApiToken = "";
    cloudflareMakeDefault = false;
  }

  function editFrpProfile(profile: FrpProfileDto) {
    activeTab = "frp";
    editingId = profile.id;
    name = profile.name;
    server = profile.server;
    serverPort = profile.serverPort;
    token = "";
    makeDefault = profile.isDefault;
  }

  function editCloudflareProfile(profile: CloudflareProfileDto) {
    activeTab = "cloudflare";
    cloudflareEditingId = profile.id;
    cloudflareName = profile.name;
    cloudflareAccountId = profile.accountId;
    cloudflareTunnelId = profile.tunnelId;
    cloudflareZoneId = profile.zoneId;
    cloudflareTunnelToken = "";
    cloudflareApiToken = "";
    cloudflareMakeDefault = profile.isDefault;
  }

  async function saveFrp() {
    if (!name.trim() || !server.trim()) {
      await message("请填写 FRP 配置名称和服务器地址。", { title: "无法保存", kind: "warning" });
      return;
    }
    frpSaving = true;
    try {
      await saveFrpProfile(
        {
          id: editingId ?? "",
          name: name.trim(),
          server: server.trim(),
          serverPort,
        },
        token.trim() || undefined,
        makeDefault,
      );
      resetFrpForm();
      await refreshFrp();
    } catch (error) {
      await message(String(error), { title: "保存失败", kind: "error" });
    } finally {
      frpSaving = false;
    }
  }

  async function saveCloudflare() {
    if (
      !cloudflareName.trim() ||
      !cloudflareAccountId.trim() ||
      !cloudflareTunnelId.trim() ||
      !cloudflareZoneId.trim()
    ) {
      await message("请填写名称、Account ID、Tunnel ID 和 Zone ID。", {
        title: "无法保存",
        kind: "warning",
      });
      return;
    }
    cloudflareSaving = true;
    try {
      await saveCloudflareProfile(
        {
          id: cloudflareEditingId ?? "",
          name: cloudflareName.trim(),
          accountId: cloudflareAccountId.trim(),
          tunnelId: cloudflareTunnelId.trim(),
          zoneId: cloudflareZoneId.trim(),
        },
        cloudflareTunnelToken.trim() || undefined,
        cloudflareApiToken.trim() || undefined,
        cloudflareMakeDefault,
      );
      resetCloudflareForm();
      await refreshCloudflare();
    } catch (error) {
      await message(String(error), { title: "保存失败", kind: "error" });
    } finally {
      cloudflareSaving = false;
    }
  }

  async function removeFrpProfile(profile: FrpProfileDto) {
    try {
      await deleteFrpProfile(profile.id);
      if (editingId === profile.id) {
        resetFrpForm();
      }
      await refreshFrp();
    } catch (error) {
      await message(String(error), { title: "删除失败", kind: "error" });
    }
  }

  async function removeCloudflareProfile(profile: CloudflareProfileDto) {
    try {
      await deleteCloudflareProfile(profile.id);
      if (cloudflareEditingId === profile.id) {
        resetCloudflareForm();
      }
      await refreshCloudflare();
    } catch (error) {
      await message(String(error), { title: "删除失败", kind: "error" });
    }
  }

  onMount(() => {
    void Promise.all([refreshFrp(), refreshCloudflare()]);
  });
</script>

<section class="page-scroll">
  <header class="page-header">
    <p class="page-kicker">全局设置</p>
    <h2 class="page-title">隧道配置</h2>
    <p class="mt-2 max-w-2xl text-sm text-[var(--color-text-muted)]">
      FRP 与 Cloudflare Named Tunnel 分别保存和管理。切换配置页会保留当前未保存的输入，但不会将 token 写入浏览器存储。
    </p>
  </header>

  <div class="page-body flex flex-col gap-6">
    <div class="flex border-b border-[var(--color-border)]" role="tablist" aria-label="隧道配置类型">
      <button
        type="button"
        role="tab"
        aria-selected={activeTab === "frp"}
        class={`border-b-2 px-3 py-2 text-sm ${activeTab === "frp" ? "border-[var(--color-accent)] text-[var(--color-text-primary)]" : "border-transparent text-[var(--color-text-muted)]"}`}
        onclick={() => (activeTab = "frp")}
      >
        FRP 配置
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={activeTab === "cloudflare"}
        class={`border-b-2 px-3 py-2 text-sm ${activeTab === "cloudflare" ? "border-[var(--color-accent)] text-[var(--color-text-primary)]" : "border-transparent text-[var(--color-text-muted)]"}`}
        onclick={() => (activeTab = "cloudflare")}
      >
        Cloudflare Named Tunnel
      </button>
    </div>

    {#if activeTab === "frp"}
    <div class="tx-card p-4">
      <h3 class="text-sm font-semibold">{editingId ? "编辑 FRP 配置" : "新建 FRP 配置"}</h3>
      <form
        class="mt-4 grid gap-3"
        onsubmit={(event) => {
          event.preventDefault();
          void saveFrp();
        }}
      >
        <label class="grid gap-1">
          <span class="text-xs text-[var(--color-text-muted)]">名称</span>
          <input
            type="text"
            class="tx-input"
            placeholder="公司 FRP"
            bind:value={name}
          />
        </label>
        <label class="grid gap-1">
          <span class="text-xs text-[var(--color-text-muted)]">FRP 服务器域名</span>
          <input
            type="text"
            class="tx-input tx-mono"
            placeholder="frp.example.com"
            bind:value={server}
          />
        </label>
        <label class="grid gap-1">
          <span class="text-xs text-[var(--color-text-muted)]">FRP 端口</span>
          <input
            type="number"
            min="1"
            max="65535"
            class="tx-input"
            bind:value={serverPort}
          />
        </label>
        <label class="grid gap-1">
          <span class="text-xs text-[var(--color-text-muted)]">
            Token {editingId ? "（留空则保持不变）" : ""}
          </span>
          <SecretInput
            bind:value={token}
            placeholder="frp auth token"
            showCopy={false}
          />
        </label>
        <label class="flex items-center gap-2 text-sm text-[var(--color-text-secondary)]">
          <input type="checkbox" class="h-4 w-4" bind:checked={makeDefault} />
          作为默认 FRP 配置
        </label>
        <div class="flex gap-2 pt-1">
          <button
            type="submit"
            class="rounded-md bg-[var(--color-accent)] px-3 py-1.5 text-sm font-medium text-white disabled:opacity-50"
            disabled={frpSaving}
          >
            {frpSaving ? "保存中…" : editingId ? "更新" : "添加"}
          </button>
          {#if editingId}
            <button
              type="button"
              class="tx-btn-ghost"
              onclick={resetFrpForm}
            >
              取消
            </button>
          {/if}
        </div>
      </form>
    </div>

    {/if}

    {#if activeTab === "frp"}
    <div class="tx-card p-4">
      <h3 class="text-sm font-semibold">已保存的 FRP 配置</h3>
      {#if frpLoading}
        <p class="mt-4 text-sm text-[var(--color-text-muted)]">加载中…</p>
      {:else if frpProfiles.length === 0}
        <p class="mt-4 text-sm text-[var(--color-text-muted)]">暂无 FRP 配置。</p>
      {:else}
        <ul class="mt-4 space-y-2">
          {#each frpProfiles as profile (profile.id)}
            <li
              class="tx-panel flex items-center justify-between gap-3 px-3 py-2"
            >
              <div class="min-w-0">
                <p class="truncate text-sm font-medium">
                  {profile.name}{profile.isDefault ? " · 默认" : ""}
                </p>
                <p class="truncate font-mono text-xs text-[var(--color-text-muted)]">
                  {profile.server}:{profile.serverPort} · Token {profile.hasToken ? "已配置" : "未配置"}
                </p>
              </div>
              <div class="flex shrink-0 gap-2">
                <button
                  type="button"
                  class="text-xs text-[var(--color-accent)] hover:underline"
                  onclick={() => editFrpProfile(profile)}
                >
                  编辑
                </button>
                <button
                  type="button"
                  class="text-xs text-red-400 hover:underline"
                  onclick={() => void removeFrpProfile(profile)}
                >
                  删除
                </button>
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
    {/if}

    {#if activeTab === "cloudflare"}
      <div class="tx-card p-4">
        <h3 class="text-sm font-semibold">
          {cloudflareEditingId ? "编辑 Cloudflare 配置" : "新建 Cloudflare 配置"}
        </h3>
        <form
          class="mt-4 grid gap-3"
          onsubmit={(event) => {
            event.preventDefault();
            void saveCloudflare();
          }}
        >
          <label class="grid gap-1">
            <span class="text-xs text-[var(--color-text-muted)]">名称</span>
            <input type="text" class="tx-input" placeholder="生产 Named Tunnel" bind:value={cloudflareName} />
          </label>
          <div class="grid gap-3 sm:grid-cols-2">
            <label class="grid gap-1">
              <span class="text-xs text-[var(--color-text-muted)]">Account ID</span>
              <input type="text" class="tx-input tx-mono" bind:value={cloudflareAccountId} />
            </label>
            <label class="grid gap-1">
              <span class="text-xs text-[var(--color-text-muted)]">Tunnel ID</span>
              <input type="text" class="tx-input tx-mono" bind:value={cloudflareTunnelId} />
            </label>
          </div>
          <label class="grid gap-1">
            <span class="text-xs text-[var(--color-text-muted)]">Zone ID</span>
            <input type="text" class="tx-input tx-mono" bind:value={cloudflareZoneId} />
          </label>
          <div class="grid gap-3 sm:grid-cols-2">
            <label class="grid gap-1">
              <span class="text-xs text-[var(--color-text-muted)]">Tunnel Token {cloudflareEditingId ? "（留空则保持不变）" : ""}</span>
              <SecretInput bind:value={cloudflareTunnelToken} placeholder="cloudflared tunnel token" showCopy={false} />
            </label>
            <label class="grid gap-1">
              <span class="text-xs text-[var(--color-text-muted)]">API Token {cloudflareEditingId ? "（留空则保持不变）" : ""}</span>
              <SecretInput bind:value={cloudflareApiToken} placeholder="Cloudflare API token" showCopy={false} />
            </label>
          </div>
          <label class="flex items-center gap-2 text-sm text-[var(--color-text-secondary)]">
            <input type="checkbox" class="h-4 w-4" bind:checked={cloudflareMakeDefault} />
            作为默认 Cloudflare 配置
          </label>
          <div class="flex gap-2 pt-1">
            <button type="submit" class="rounded-md bg-[var(--color-accent)] px-3 py-1.5 text-sm font-medium text-white disabled:opacity-50" disabled={cloudflareSaving}>
              {cloudflareSaving ? "保存中…" : cloudflareEditingId ? "更新" : "添加"}
            </button>
            {#if cloudflareEditingId}
              <button type="button" class="tx-btn-ghost" onclick={resetCloudflareForm}>取消</button>
            {/if}
          </div>
        </form>
      </div>

      <div class="tx-card p-4">
        <h3 class="text-sm font-semibold">已保存的 Cloudflare 配置</h3>
        {#if cloudflareLoading}
          <p class="mt-4 text-sm text-[var(--color-text-muted)]">加载中…</p>
        {:else if cloudflareProfiles.length === 0}
          <p class="mt-4 text-sm text-[var(--color-text-muted)]">暂无 Cloudflare 配置。</p>
        {:else}
          <ul class="mt-4 space-y-2">
            {#each cloudflareProfiles as profile (profile.id)}
              <li class="tx-panel flex items-center justify-between gap-3 px-3 py-2">
                <div class="min-w-0">
                  <p class="truncate text-sm font-medium">
                    {profile.name}{profile.isDefault ? " · 默认" : ""}
                  </p>
                  <p class="truncate font-mono text-xs text-[var(--color-text-muted)]">
                    Account/Tunnel/Zone 已配置 · Tunnel Token {profile.hasTunnelToken ? "已配置" : "未配置"} · API Token {profile.hasApiToken ? "已配置" : "未配置"}
                  </p>
                </div>
                <div class="flex shrink-0 gap-2">
                  <button type="button" class="text-xs text-[var(--color-accent)] hover:underline" onclick={() => editCloudflareProfile(profile)}>编辑</button>
                  <button type="button" class="text-xs text-red-400 hover:underline" onclick={() => void removeCloudflareProfile(profile)}>删除</button>
                </div>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}
  </div>
</section>
