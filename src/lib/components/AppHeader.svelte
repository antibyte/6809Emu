<script lang="ts">
  import { t } from "../i18n";
  import { theme } from "../theme";
  import { locale } from "../i18n";
  import Icon from "./Icon.svelte";
  import Popover from "./Popover.svelte";
  import { layout, togglePanel, type PanelId } from "../layout";
  import type { CpuVariant, MachineInfo, MachineKind } from "../types";

  let {
    running,
    busy,
    onRun,
    onPause,
    onStep,
    onReset,
    onInterrupt,
    onImport,
    onExport,
    onSaveSession,
    onLoadSession,
    cpuVariant,
    onCpuVariantChange,
    machineKind,
    machineProfiles,
    onMachineChange,
    machineChanging,
    loadAddr,
    resetPc,
    onLoadAddrChange,
    onResetPcChange,
    configDirty,
    onApplyConfig,
    aciaEnabled,
    onAciaToggle,
    aciaBase,
    onAciaBaseChange,
    aciaBaud,
    onAciaBaudChange,
    onCycleTheme,
    onToggleLocale,
    videoAvailable,
    videoActive,
    onToggleVideo,
    onOpenShortcuts,
    onResetLayout,
  }: {
    running: boolean;
    busy: boolean;
    onRun: () => void;
    onPause: () => void;
    onStep: () => void;
    onReset: () => void;
    onInterrupt: (type: "irq" | "firq" | "nmi") => void;
    onImport: () => void;
    onExport: () => void;
    onSaveSession: () => void;
    onLoadSession: () => void;
    cpuVariant: CpuVariant;
    onCpuVariantChange: (v: CpuVariant) => void;
    machineKind: MachineKind;
    machineProfiles: MachineInfo[];
    onMachineChange: (k: MachineKind) => void;
    machineChanging: boolean;
    loadAddr: number;
    resetPc: number;
    onLoadAddrChange: (v: number) => void;
    onResetPcChange: (v: number) => void;
    configDirty: boolean;
    onApplyConfig: () => void;
    aciaEnabled: boolean;
    onAciaToggle: (enabled: boolean) => void;
    aciaBase: number;
    onAciaBaseChange: (v: number) => void;
    aciaBaud: number;
    onAciaBaudChange: (v: number) => void;
    onCycleTheme: () => void;
    onToggleLocale: () => void;
    videoAvailable: boolean;
    videoActive: boolean;
    onToggleVideo: () => void;
    onOpenShortcuts: () => void;
    onResetLayout: () => void;
  } = $props();

  const themeLabel = $derived(
    $theme === "dark" ? $t("theme.dark") : $theme === "light" ? $t("theme.light") : $t("theme.highContrast"),
  );
  const localeLabel = $derived($locale === "de" ? "DE" : "EN");

  const viewPanels = $derived([
    { id: "registers" as PanelId, icon: "registers" as const, label: $t("registers.title") },
    { id: "io" as PanelId, icon: "io" as const, label: $t("machine.ioTitle") },
    { id: "breakpoints" as PanelId, icon: "breakpoint" as const, label: $t("breakpoints.title") },
    { id: "watchpoints" as PanelId, icon: "watch" as const, label: $t("watchpoints.title") },
    { id: "disasm" as PanelId, icon: "disasm" as const, label: $t("disasm.title") },
    { id: "asm" as PanelId, icon: "code" as const, label: $t("asm.title") },
    { id: "memory" as PanelId, icon: "memory" as const, label: $t("memory.title") },
    { id: "trace" as PanelId, icon: "trace" as const, label: $t("trace.title") },
    { id: "terminal" as PanelId, icon: "terminal" as const, label: $t("acia.title") },
    { id: "video" as PanelId, icon: "video" as const, label: $t("machine.videoTitle") },
  ]);

  function hexInput(handler: (v: number) => void, max: number) {
    return (e: Event) => {
      const v = parseInt((e.target as HTMLInputElement).value, 16);
      if (!isNaN(v) && v >= 0 && v <= max) handler(v);
    };
  }

  const baudOptions = [300, 1200, 2400, 4800, 9600, 19200, 38400];
</script>

<header class="app-header">
  <div class="brand">
    <span class="logo">6809</span>
    <span class="brand-title">{$t("app.title")}</span>
  </div>

  <span class="sep"></span>

  <div class="transport">
    <button class="primary icon-btn" onclick={onRun} disabled={running || busy} title={$t("shortcuts.run")} aria-label={$t("shortcuts.run")}>
      <Icon name="run" size={13} />
    </button>
    <button class="icon-btn" onclick={onPause} disabled={!running} title={$t("shortcuts.pause")} aria-label={$t("shortcuts.pause")}>
      <Icon name="pause" size={13} />
    </button>
    <button class="icon-btn" onclick={onStep} disabled={running || busy} title={$t("shortcuts.step")} aria-label={$t("shortcuts.step")}>
      <Icon name="step" size={13} />
    </button>
    <button class="icon-btn" onclick={onReset} disabled={busy} title={$t("shortcuts.reset")} aria-label={$t("shortcuts.reset")}>
      <Icon name="reset" size={13} />
    </button>
  </div>

  <span class="sep"></span>

  <div class="interrupts">
    <button class="ghost" onclick={() => onInterrupt("irq")} disabled={running} title={$t("interrupts.irq")}>IRQ</button>
    <button class="ghost" onclick={() => onInterrupt("firq")} disabled={running} title={$t("interrupts.firq")}>FIRQ</button>
    <button class="ghost" onclick={() => onInterrupt("nmi")} disabled={running} title={$t("interrupts.nmi")}>NMI</button>
  </div>

  <span class="sep"></span>

  <div class="menus">
    <div class="menu-wrap">
      <Popover label={$t("menu.file")} buttonLabel={$t("menu.file")} icon="file">
        <button class="menu-item" onclick={onImport} disabled={busy}>
          <Icon name="import" size={13} /> {$t("menu.open")}
        </button>
        <button class="menu-item" onclick={onExport} disabled={busy}>
          <Icon name="export" size={13} /> {$t("menu.export")}
        </button>
        <div class="menu-sep"></div>
        <button class="menu-item" onclick={onSaveSession} disabled={busy}>
          <Icon name="save" size={13} /> {$t("menu.saveSession")}
        </button>
        <button class="menu-item" onclick={onLoadSession} disabled={busy}>
          <Icon name="folder-open" size={13} /> {$t("menu.loadSession")}
        </button>
      </Popover>
    </div>

    <div class="menu-wrap">
      <Popover label={$t("setup.title")} buttonLabel={$t("setup.title")} icon="settings" active={configDirty}>
        <div class="menu-label">{$t("setup.cpu")}</div>
        <label class="field">
          <span class="lk">{$t("cpu.label")}</span>
          <select value={cpuVariant} disabled={running} onchange={(e) => onCpuVariantChange((e.target as HTMLSelectElement).value as CpuVariant)}>
            <option value="mc6809">{$t("cpu.mc6809")}</option>
            <option value="hd6309">{$t("cpu.hd6309")}</option>
          </select>
        </label>
        <label class="field">
          <span class="lk">{$t("setup.machine")}</span>
          <select value={machineKind} disabled={running || machineChanging} onchange={(e) => onMachineChange((e.target as HTMLSelectElement).value as MachineKind)}>
            {#each machineProfiles as profile}
              <option value={profile.kind}>{profile.name}</option>
            {/each}
          </select>
        </label>

        <div class="menu-sep"></div>
        <div class="menu-label">{$t("setup.load")} / {$t("setup.reset")}</div>
        <label class="field">
          <span class="lk">{$t("config.loadAddr")}</span>
          <input class="mono" value={loadAddr.toString(16).toUpperCase()} oninput={hexInput(onLoadAddrChange, 0xffff)} size="6" />
        </label>
        <label class="field">
          <span class="lk">{$t("config.resetPc")}</span>
          <input class="mono" value={resetPc.toString(16).toUpperCase()} oninput={hexInput(onResetPcChange, 0xffff)} size="6" />
        </label>

        <div class="menu-sep"></div>
        <div class="menu-label">{$t("setup.acia")}</div>
        <label class="field">
          <span class="lk">{$t("acia.enabled")}</span>
          <input type="checkbox" checked={aciaEnabled} disabled={running} onchange={(e) => onAciaToggle((e.target as HTMLInputElement).checked)} />
        </label>
        {#if aciaEnabled}
          <label class="field">
            <span class="lk">{$t("setup.aciaBase")}</span>
            <input class="mono" value={aciaBase.toString(16).toUpperCase()} disabled={running} onchange={hexInput(onAciaBaseChange, 0xffff)} size="6" />
          </label>
          <label class="field">
            <span class="lk">{$t("setup.aciaBaud")}</span>
            <select value={aciaBaud} disabled={running} onchange={(e) => onAciaBaudChange(parseInt((e.target as HTMLSelectElement).value, 10))}>
              {#each baudOptions as b}
                <option value={b}>{b}</option>
              {/each}
            </select>
          </label>
        {/if}

        <div class="menu-sep"></div>
        <button class="menu-item primary" onclick={onApplyConfig} disabled={!configDirty}>
          <Icon name="check" size={13} /> {$t("setup.apply")}
        </button>
      </Popover>
    </div>

    <div class="menu-wrap">
      <Popover label={$t("panels.toggle")} buttonLabel="" icon="view">
        <div class="menu-label">{$t("panels.toggle")}</div>
        {#each viewPanels as p}
          {@const canShow = p.id !== "video" || videoAvailable}
          <label class="menu-item check" class:disabled={!canShow}>
            <input type="checkbox" checked={$layout.visible[p.id]} disabled={!canShow} onchange={() => togglePanel(p.id)} />
            <Icon name={p.icon} size={13} />
            <span>{p.label}</span>
          </label>
        {/each}
        <div class="menu-sep"></div>
        <button class="menu-item" onclick={onResetLayout}>
          <Icon name="reset" size={13} /> {$t("layout.reset")}
        </button>
      </Popover>
    </div>
  </div>

  <div class="spacer"></div>

  <div class="trailing">
    {#if videoAvailable}
      <button class="hdr-btn" class:active={videoActive} onclick={onToggleVideo} title={$t("machine.videoOpen")} aria-label={$t("machine.videoOpen")}>
        <Icon name="video" size={15} />
      </button>
    {/if}
    <button class="hdr-btn" onclick={onOpenShortcuts} title={$t("shortcuts.open")} aria-label={$t("shortcuts.open")}>
      <Icon name="keyboard" size={15} />
    </button>
    <button class="hdr-btn" onclick={onCycleTheme} title={$t("theme.label")} aria-label={$t("theme.label")}>
      <Icon name="theme" size={15} />
    </button>
    <button class="hdr-btn locale" onclick={onToggleLocale} aria-label="Language">{localeLabel}</button>
  </div>
</header>

<style>
  .app-header {
    position: relative;
    display: flex;
    align-items: center;
    gap: 10px;
    height: var(--header-h);
    padding: 0 12px;
    background: var(--bg-1);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    flex-shrink: 0;
    z-index: 100;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 9px;
    flex-shrink: 0;
  }

  .logo {
    font-family: var(--font-mono);
    font-weight: 700;
    font-size: 12px;
    letter-spacing: 0.02em;
    color: var(--accent);
    background: var(--accent-soft);
    border: 1px solid var(--accent-line);
    border-radius: 4px;
    padding: 3px 7px;
    line-height: 1;
  }

  .brand-title {
    font-weight: 600;
    font-size: 13px;
    color: var(--text);
    white-space: nowrap;
  }

  .sep {
    width: 1px;
    height: 22px;
    background: var(--border);
    flex-shrink: 0;
  }

  .transport,
  .interrupts,
  .menus,
  .trailing {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-shrink: 0;
  }

  .interrupts button {
    padding: 5px 8px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
  }

  .menus {
    gap: 6px;
  }

  .menu-wrap {
    position: relative;
    display: inline-flex;
  }

  .spacer {
    flex: 1;
    min-width: 0;
  }

  .trailing {
    gap: 2px;
  }

  .locale {
    font-size: 11px;
    font-weight: 700;
    min-width: 30px;
    letter-spacing: 0.04em;
  }

  .menu-item.check {
    cursor: pointer;
    user-select: none;
  }

  .menu-item.check.disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .menu-item.check input {
    margin: 0;
    accent-color: var(--accent);
  }

  .menu-item.primary {
    color: var(--accent);
    font-weight: 600;
  }

  .menu-item.primary:disabled {
    opacity: 0.5;
  }

  @media (max-width: 880px) {
    .brand-title {
      display: none;
    }
    .interrupts {
      display: none;
    }
  }

  @media (max-width: 720px) {
    .menus .menu-wrap:nth-child(2) {
      display: none;
    }
  }
</style>
