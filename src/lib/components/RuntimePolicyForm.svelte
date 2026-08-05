<script lang="ts">
  export interface RuntimePolicyDraft {
    toolProfile: string;
    permissionMode: string;
    allowedCommands: string;
    workspaceLocalEntries: boolean;
    workspaceScriptExtensions: string;
    filesystemMode: string;
    allowedPaths: string;
    deniedPaths: string;
    deniedDrives: string;
  }

  interface Props {
    toolProfile: string;
    permissionMode: string;
    allowedCommands: string;
    workspaceLocalEntries: boolean;
    workspaceScriptExtensions: string;
    filesystemMode: string;
    allowedPaths: string;
    deniedPaths: string;
    deniedDrives: string;
    onSave: (draft: RuntimePolicyDraft) => void | Promise<void>;
  }

  const TOOL_PROFILE_OPTIONS = [
    { value: "full", label: "完整工具" },
    { value: "read-only", label: "只读工具" },
    { value: "compat-readonly-all", label: "兼容只读" },
  ] as const;

  const PERMISSION_MODE_OPTIONS = [
    { value: "trusted", label: "受信任" },
    { value: "safe", label: "安全受限" },
    { value: "dangerous", label: "完全放开" },
  ] as const;

  let { toolProfile, permissionMode, allowedCommands, workspaceLocalEntries, workspaceScriptExtensions, filesystemMode, allowedPaths, deniedPaths, deniedDrives, onSave }: Props = $props();

  let draftProfile = $state("full");
  let draftMode = $state("trusted");
  let draftCommands = $state("");
  let draftLocalEntries = $state(true);
  let draftExtensions = $state(".exe,.bat,.cmd,.ps1");
  let draftFilesystemMode = $state("workspace_only");
  let draftAllowedPaths = $state("");
  let draftDeniedPaths = $state("");
  let draftDeniedDrives = $state("");
  let saving = $state(false);

  const dirty = $derived(
    draftProfile !== toolProfile || draftMode !== permissionMode || draftCommands !== allowedCommands || draftLocalEntries !== workspaceLocalEntries || draftExtensions !== workspaceScriptExtensions || draftFilesystemMode !== filesystemMode || draftAllowedPaths !== allowedPaths || draftDeniedPaths !== deniedPaths || draftDeniedDrives !== deniedDrives,
  );

  $effect(() => {
    draftProfile = toolProfile;
    draftMode = permissionMode;
    draftCommands = allowedCommands;
    draftLocalEntries = workspaceLocalEntries;
    draftExtensions = workspaceScriptExtensions;
    draftFilesystemMode = filesystemMode;
    draftAllowedPaths = allowedPaths;
    draftDeniedPaths = deniedPaths;
    draftDeniedDrives = deniedDrives;
  });

  async function save() {
    if (saving || !dirty) return;
    saving = true;
    try {
      await onSave({ toolProfile: draftProfile, permissionMode: draftMode, allowedCommands: draftCommands.trim(), workspaceLocalEntries: draftLocalEntries, workspaceScriptExtensions: draftExtensions.trim(), filesystemMode: draftFilesystemMode, allowedPaths: draftAllowedPaths.trim(), deniedPaths: draftDeniedPaths.trim(), deniedDrives: draftDeniedDrives.trim() });
    } finally {
      saving = false;
    }
  }
</script>

<form
  class="grid gap-3"
  onsubmit={(event) => {
    event.preventDefault();
    void save();
  }}
>
  <label class="grid gap-1">
    <span class="text-xs text-[var(--color-text-muted)]">工具档位</span>
    <select
      class="rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-2.5 py-1.5 text-sm"
      bind:value={draftProfile}
    >
      {#each TOOL_PROFILE_OPTIONS as option}
        <option value={option.value}>{option.label}</option>
      {/each}
    </select>
  </label>
  <label class="grid gap-1">
    <span class="text-xs text-[var(--color-text-muted)]">文件访问模式</span>
    <select class="rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-2.5 py-1.5 text-sm" bind:value={draftFilesystemMode}>
      <option value="workspace_only">仅 Workspace</option>
      <option value="workspace_and_skills">Workspace 与 Skills</option>
      <option value="allowlist">仅显式允许目录</option>
    </select>
  </label>
  <label class="grid gap-1">
    <span class="text-xs text-[var(--color-text-muted)]">允许目录（逗号分隔，优先级最高）</span>
    <input type="text" class="rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-2.5 py-1.5 font-mono text-sm" placeholder="C:\\Users\\Alvin" bind:value={draftAllowedPaths} />
  </label>
  <label class="grid gap-1">
    <span class="text-xs text-[var(--color-text-muted)]">禁止目录（逗号分隔）</span>
    <input type="text" class="rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-2.5 py-1.5 font-mono text-sm" placeholder="C:\\Windows" bind:value={draftDeniedPaths} />
  </label>
  <label class="grid gap-1">
    <span class="text-xs text-[var(--color-text-muted)]">禁止驱动器（逗号分隔）</span>
    <input type="text" class="rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-2.5 py-1.5 font-mono text-sm" placeholder="C:,D:" bind:value={draftDeniedDrives} />
  </label>
  <label class="grid gap-1">
    <span class="text-xs text-[var(--color-text-muted)]">系统命令（逗号分隔）</span>
    <input type="text" class="rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-2.5 py-1.5 font-mono text-sm" placeholder="python,git,curl,powershell,..." bind:value={draftCommands} />
  </label>
  <label class="flex items-center gap-2 text-sm">
    <input type="checkbox" bind:checked={draftLocalEntries} />
    <span>允许执行 Workspace 内本地入口</span>
  </label>
  <label class="grid gap-1">
    <span class="text-xs text-[var(--color-text-muted)]">本地脚本扩展名（逗号分隔）</span>
    <input type="text" class="rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-2.5 py-1.5 font-mono text-sm" placeholder=".exe,.bat,.cmd,.ps1" bind:value={draftExtensions} disabled={!draftLocalEntries} />
  </label>
  <label class="grid gap-1">
    <span class="text-xs text-[var(--color-text-muted)]">权限模式</span>
    <select
      class="rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-2.5 py-1.5 text-sm"
      bind:value={draftMode}
    >
      {#each PERMISSION_MODE_OPTIONS as option}
        <option value={option.value}>{option.label}</option>
      {/each}
    </select>
  </label>
  <p class="text-xs text-[var(--color-text-muted)]">
    Workspace 本地入口按当前工作目录解析；系统命令与脚本类型均可按项目配置。当前执行边界仍为 policy_only。
  </p>
  <div class="flex justify-end pt-1">
    <button
      type="submit"
      class="rounded-md bg-[var(--color-accent)] px-3 py-1.5 text-sm font-medium text-white disabled:opacity-50"
      disabled={saving || !dirty}
    >
      {saving ? "保存中…" : "保存策略"}
    </button>
  </div>
</form>
