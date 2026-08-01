<script lang="ts">
  import { onMount } from "svelte";
  import { message } from "@tauri-apps/plugin-dialog";
  import {
    deleteFrpProfile,
    listFrpProfiles,
    saveFrpProfile,
    type FrpProfileDto,
  } from "$lib/api/settings";
  import SecretInput from "$lib/components/SecretInput.svelte";

  let profiles = $state<FrpProfileDto[]>([]);
  let loading = $state(true);
  let saving = $state(false);
  let editingId = $state<string | null>(null);
  let name = $state("");
  let server = $state("");
  let serverPort = $state(7000);
  let token = $state("");
  let cloudflareAccountId = $state("");
  let cloudflareTunnelId = $state("");
  let cloudflareZoneId = $state("");
  let cloudflareTunnelToken = $state("");
  let cloudflareApiToken = $state("");
  let makeDefault = $state(false);

  async function refresh() {
    loading = true;
    try {
      profiles = await listFrpProfiles();
      if (!editingId && profiles.length === 0) {
        makeDefault = true;
      }
    } finally {
      loading = false;
    }
  }

  function resetForm() {
    editingId = null;
    name = "";
    server = "";
    serverPort = 7000;
    token = "";
    cloudflareAccountId = "";
    cloudflareTunnelId = "";
    cloudflareZoneId = "";
    cloudflareTunnelToken = "";
    cloudflareApiToken = "";
    makeDefault = false;
  }

  function editProfile(profile: FrpProfileDto) {
    editingId = profile.id;
    name = profile.name;
    server = profile.server;
    serverPort = profile.serverPort;
    token = "";
    cloudflareAccountId = profile.cloudflareAccountId;
    cloudflareTunnelId = profile.cloudflareTunnelId;
    cloudflareZoneId = profile.cloudflareZoneId;
    cloudflareTunnelToken = "";
    cloudflareApiToken = "";
    makeDefault = profile.isDefault;
  }

  async function save() {
    const cloudflareValues = [cloudflareAccountId, cloudflareTunnelId, cloudflareZoneId].filter(
      (value) => value.trim(),
    );
    if (!name.trim()) {
      await message("请填写配置名称。", { title: "无法保存", kind: "warning" });
      return;
    }
    if (!server.trim() && cloudflareValues.length === 0) {
      await message("请至少填写 FRP 服务器或完整的 Cloudflare Named Tunnel 标识。", {
        title: "无法保存",
        kind: "warning",
      });
      return;
    }
    if (cloudflareValues.length > 0 && cloudflareValues.length !== 3) {
      await message("Cloudflare Account ID、Tunnel ID 与 Zone ID 必须同时填写。", {
        title: "无法保存",
        kind: "warning",
      });
      return;
    }
    saving = true;
    try {
      await saveFrpProfile(
        {
          id: editingId ?? "",
          name: name.trim(),
          server: server.trim(),
          serverPort,
          cloudflareAccountId: cloudflareAccountId.trim(),
          cloudflareTunnelId: cloudflareTunnelId.trim(),
          cloudflareZoneId: cloudflareZoneId.trim(),
        },
        token.trim() || undefined,
        cloudflareTunnelToken.trim() || undefined,
        cloudflareApiToken.trim() || undefined,
        makeDefault,
      );
      resetForm();
      await refresh();
    } catch (error) {
      await message(String(error), { title: "保存失败", kind: "error" });
    } finally {
      saving = false;
    }
  }

  async function removeProfile(profile: FrpProfileDto) {
    try {
      await deleteFrpProfile(profile.id);
      if (editingId === profile.id) {
        resetForm();
      }
      await refresh();
    } catch (error) {
      await message(String(error), { title: "删除失败", kind: "error" });
    }
  }

  onMount(refresh);
</script>

<section class="page-scroll">
  <header class="page-header">
    <p class="page-kicker">全局设置</p>
    <h2 class="page-title">隧道配置</h2>
    <p class="mt-2 max-w-2xl text-sm text-[var(--color-text-muted)]">
      管理共享的 FRP 与 Cloudflare Named Tunnel 参数。工作区留空时会继承默认配置，工作区填写的值始终优先。
    </p>
  </header>

  <div class="page-body flex flex-col gap-6">
    <div class="tx-card p-4">
      <h3 class="text-sm font-semibold">{editingId ? "编辑配置" : "新建配置"}</h3>
      <form
        class="mt-4 grid gap-3"
        onsubmit={(event) => {
          event.preventDefault();
          void save();
        }}
      >
        <label class="grid gap-1">
          <span class="text-xs text-[var(--color-text-muted)]">名称</span>
          <input
            type="text"
            class="tx-input"
            placeholder="公司隧道"
            bind:value={name}
          />
        </label>
        <label class="grid gap-1">
          <span class="text-xs text-[var(--color-text-muted)]">FRP 服务器域名（可选）</span>
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
        <div class="border-t border-[var(--color-border)] pt-3">
          <p class="text-xs font-medium text-[var(--color-text-secondary)]">Cloudflare Named Tunnel（可选）</p>
          <div class="mt-3 grid gap-3 sm:grid-cols-2">
            <label class="grid gap-1">
              <span class="text-xs text-[var(--color-text-muted)]">Account ID</span>
              <input type="text" class="tx-input tx-mono" bind:value={cloudflareAccountId} />
            </label>
            <label class="grid gap-1">
              <span class="text-xs text-[var(--color-text-muted)]">Tunnel ID</span>
              <input type="text" class="tx-input tx-mono" bind:value={cloudflareTunnelId} />
            </label>
          </div>
          <label class="mt-3 grid gap-1">
            <span class="text-xs text-[var(--color-text-muted)]">Zone ID</span>
            <input type="text" class="tx-input tx-mono" bind:value={cloudflareZoneId} />
          </label>
          <div class="mt-3 grid gap-3 sm:grid-cols-2">
            <label class="grid gap-1">
              <span class="text-xs text-[var(--color-text-muted)]">
                Tunnel Token {editingId ? "（留空则保持不变）" : ""}
              </span>
              <SecretInput
                bind:value={cloudflareTunnelToken}
                placeholder="cloudflared tunnel token"
                showCopy={false}
              />
            </label>
            <label class="grid gap-1">
              <span class="text-xs text-[var(--color-text-muted)]">
                API Token {editingId ? "（留空则保持不变）" : ""}
              </span>
              <SecretInput
                bind:value={cloudflareApiToken}
                placeholder="Cloudflare API token"
                showCopy={false}
              />
            </label>
          </div>
        </div>
        <label class="flex items-center gap-2 text-sm text-[var(--color-text-secondary)]">
          <input type="checkbox" class="h-4 w-4" bind:checked={makeDefault} />
          作为默认隧道配置
        </label>
        <div class="flex gap-2 pt-1">
          <button
            type="submit"
            class="rounded-md bg-[var(--color-accent)] px-3 py-1.5 text-sm font-medium text-white disabled:opacity-50"
            disabled={saving}
          >
            {saving ? "保存中…" : editingId ? "更新" : "添加"}
          </button>
          {#if editingId}
            <button
              type="button"
              class="tx-btn-ghost"
              onclick={resetForm}
            >
              取消
            </button>
          {/if}
        </div>
      </form>
    </div>

    <div class="tx-card p-4">
      <h3 class="text-sm font-semibold">已保存的配置</h3>
      {#if loading}
        <p class="mt-4 text-sm text-[var(--color-text-muted)]">加载中…</p>
      {:else if profiles.length === 0}
        <p class="mt-4 text-sm text-[var(--color-text-muted)]">暂无隧道配置。</p>
      {:else}
        <ul class="mt-4 space-y-2">
          {#each profiles as profile (profile.id)}
            <li
              class="tx-panel flex items-center justify-between gap-3 px-3 py-2"
            >
              <div class="min-w-0">
                <p class="truncate text-sm font-medium">
                  {profile.name}{profile.isDefault ? " · 默认" : ""}
                </p>
                <p class="truncate font-mono text-xs text-[var(--color-text-muted)]">
                  {profile.server ? `${profile.server}:${profile.serverPort}` : "未配置 FRP"}
                  {#if profile.cloudflareAccountId}
                    · Cloudflare Named Tunnel 已配置
                  {/if}
                  · FRP Token {profile.hasToken ? "已配置" : "未配置"}
                  · Cloudflare Token {profile.hasCloudflareTunnelToken ? "已配置" : "未配置"}
                  · API Token {profile.hasCloudflareApiToken ? "已配置" : "未配置"}
                </p>
              </div>
              <div class="flex shrink-0 gap-2">
                <button
                  type="button"
                  class="text-xs text-[var(--color-accent)] hover:underline"
                  onclick={() => editProfile(profile)}
                >
                  编辑
                </button>
                <button
                  type="button"
                  class="text-xs text-red-400 hover:underline"
                  onclick={() => removeProfile(profile)}
                >
                  删除
                </button>
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </div>
</section>
