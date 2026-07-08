<script lang="ts">
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import RegisterPanel from "./lib/components/RegisterPanel.svelte";
  import BreakpointList from "./lib/components/BreakpointList.svelte";
  import WatchpointList from "./lib/components/WatchpointList.svelte";
  import DisasmPanel from "./lib/components/DisasmPanel.svelte";
  import AsmEditor from "./lib/components/AsmEditor.svelte";
  import MemoryView from "./lib/components/MemoryView.svelte";
  import TraceLog from "./lib/components/TraceLog.svelte";
  import Toast from "./lib/components/Toast.svelte";
  import OnboardingBanner from "./lib/components/OnboardingBanner.svelte";
  import TabBar from "./lib/components/TabBar.svelte";
  import IoPanel from "./lib/components/IoPanel.svelte";
  import VideoModal from "./lib/components/VideoModal.svelte";
  import VideoPanel from "./lib/components/VideoPanel.svelte";
  import TerminalPanel from "./lib/components/TerminalPanel.svelte";
  import Splitter from "./lib/components/Splitter.svelte";
  import AppHeader from "./lib/components/AppHeader.svelte";
  import StatusBar from "./lib/components/StatusBar.svelte";
  import ShortcutsOverlay from "./lib/components/ShortcutsOverlay.svelte";
  import { t, locale, toggleLocale } from "./lib/i18n";
  import { theme, cycleTheme } from "./lib/theme";
  import { showToast } from "./lib/toast";
  import { registerShortcuts } from "./lib/shortcuts";
  import {
    layout,
    togglePanel,
    setPanel,
    toggleCollapsed,
    patchSizes,
    redistribute,
    getLayout,
    defaultLayoutState,
    type PanelId,
    type SidebarSection,
  } from "./lib/layout";
  import type { AciaConfig, AciaTerminalState, CpuState, CpuVariant, DisasmLine, MachineInfo, MachineKind, MachineState, TraceEntry, VideoFrame } from "./lib/types";
  import * as api from "./lib/api";

  let cpu = $state<CpuState | null>(null);
  let running = $state(false);
  let disasmLines = $state<DisasmLine[]>([]);
  let memoryAddr = $state(0x0100);
  let memoryBytes = $state<number[]>([]);
  let trace = $state<TraceEntry[]>([]);
  let breakpoints = $state(new Set<number>());
  let watchpoints = $state(new Set<number>());
  let breakpointTexts = $state(new Map<number, string>());
  let asmSource = $state(`        ORG $0100
start   LDA  #$42
        NOP
        BRA  start
        END`);
  let asmErrors = $state<{ line: number; message: string }[]>([]);
  let loadAddr = $state(0x0100);
  let resetPc = $state(0x0100);
  let appliedLoadAddr = $state(0x0100);
  let appliedResetPc = $state(0x0100);
  let memoryFollowPc = $state(
    typeof localStorage !== "undefined" &&
      localStorage.getItem("memoryFollowPc") === "true"
  );
  let traceMaxDisplay = $state(
    (() => {
      const raw = typeof localStorage !== "undefined" ? localStorage.getItem("traceMaxDisplay") : null;
      const n = parseInt(raw ?? "50", 10);
      return Number.isFinite(n) && n > 0 ? n : 50;
    })()
  );
  let runSpeedIndex = $state(
    (() => {
      const raw = typeof localStorage !== "undefined" ? localStorage.getItem("runSpeedIndex") : null;
      const n = parseInt(raw ?? "0", 10);
      return Number.isFinite(n) && n >= 0 ? n : 0;
    })()
  );
  let configDirty = $derived(
    loadAddr !== appliedLoadAddr || resetPc !== appliedResetPc
  );
  let breakpointEntries = $derived(
    [...breakpoints]
      .sort((a, b) => a - b)
      .map((address) => ({
        address,
        text: breakpointTexts.get(address) ?? "",
      }))
  );
  let traceId = 0;
  let tickRaf = 0;
  let pendingTick: api.TickPayload | null = null;
  let disasmBase = 0;
  let disasmRefreshing = false;
  let busyCount = $state(0);
  let busy = $derived(busyCount > 0);
  let assembling = $state(false);
  let lastTrap = $state<string | null>(null);
  let compactLayout = $state(false);
  let activeMainTab = $state("disasm");
  let activeBottomTab = $state("memory");
  let showShortcuts = $state(false);
  let showVideoModal = $state(false);

  const LAYOUT_LIMITS = {
    splitterPx: 10,
    mainMinPx: 300,
    bottomMinPx: 200,
    sidebarMinPx: 240,
    sidebarMaxPx: 460,
    disasmMinPx: 280,
    asmMinPx: 280,
    videoMinPx: 240,
    videoMaxPx: 640,
  } as const;

  function clamp(value: number, min: number, max: number): number {
    return Math.min(max, Math.max(min, value));
  }

  type ResizeOp =
    | { kind: "main-bottom" }
    | { kind: "sidebar-main" }
    | { kind: "disasm-asm" }
    | { kind: "center-video" }
    | { kind: "group"; group: "bottom" | "sidebarRows"; a: string; b: string; axis: "x" | "y" };

  let activeResize = $state<ResizeOp | null>(null);
  let lastPointer = $state({ x: 0, y: 0 });
  let contentSplit: HTMLDivElement | undefined = $state();
  let workspaceSplit: HTMLElement | undefined = $state();
  let mainColumns: HTMLDivElement | undefined = $state();
  let bottomPanelsEl: HTMLDivElement | undefined = $state();
  let sidebarEl: HTMLElement | undefined = $state();

  let layoutStyle = $derived(
    `--main-pct: ${$layout.sizes.mainPct}%; --sidebar-px: ${$layout.sizes.sidebarPx}px; --disasm-pct: ${$layout.sizes.disasmPct}%; --video-px: ${$layout.sizes.videoPx}px;`
  );

  let machineProfiles = $state<MachineInfo[]>([]);
  let machineState = $state<MachineState>({
    kind: "bare",
    io_registers: [],
    acia: { enabled: false, base_addr: 0xffa0, baud: 9600, e_clock_hz: 1_000_000 },
  });
  let aciaEnabled = $state(false);
  let videoFrame = $state<VideoFrame | null>(null);
  let aciaTerminal = $state<AciaTerminalState | null>(null);
  let machineChanging = $state(false);

  // ---- Derived panel visibility / stacks ----

  let sidebarItems = $derived.by(() => {
    const items: { id: SidebarSection; share: number; collapsed: boolean }[] = [];
    if ($layout.visible.registers)
      items.push({ id: "registers", share: $layout.sizes.sidebarRows.registers, collapsed: $layout.collapsed.registers });
    if ($layout.visible.io)
      items.push({ id: "io", share: $layout.sizes.sidebarRows.io, collapsed: $layout.collapsed.io });
    if ($layout.visible.breakpoints)
      items.push({ id: "breakpoints", share: $layout.sizes.sidebarRows.breakpoints, collapsed: $layout.collapsed.breakpoints });
    if ($layout.visible.watchpoints)
      items.push({ id: "watchpoints", share: $layout.sizes.sidebarRows.watchpoints, collapsed: $layout.collapsed.watchpoints });
    return items;
  });
  let sidebarVisible = $derived(sidebarItems.length > 0);

  let bottomItems = $derived.by(() => {
    const items: { id: "memory" | "trace" | "terminal"; share: number }[] = [];
    if ($layout.visible.memory) items.push({ id: "memory", share: $layout.sizes.bottom.memory });
    if ($layout.visible.trace) items.push({ id: "trace", share: $layout.sizes.bottom.trace });
    if ($layout.visible.terminal && aciaEnabled) items.push({ id: "terminal", share: $layout.sizes.bottom.terminal });
    return items;
  });
  let bottomVisible = $derived(bottomItems.length > 0);

  let videoDockedVisible = $derived(
    $layout.visible.video && $layout.videoDocked && machineState.kind !== "bare"
  );

  let disasmVisible = $derived($layout.visible.disasm);
  let asmVisible = $derived($layout.visible.asm);

  // ---- Compact tabs (filtered by visibility) ----

  const mainTabs = $derived(
    [
      { id: "disasm", label: $t("disasm.title"), show: $layout.visible.disasm },
      { id: "asm", label: $t("asm.title"), show: $layout.visible.asm },
      { id: "registers", label: $t("registers.title"), show: $layout.visible.registers },
      { id: "io", label: $t("machine.ioTitle"), show: $layout.visible.io },
    ].filter((x) => x.show)
  );

  const bottomTabs = $derived(
    [
      { id: "memory", label: $t("memory.title"), show: $layout.visible.memory },
      { id: "trace", label: $t("trace.title"), show: $layout.visible.trace },
      { id: "terminal", label: $t("acia.title"), show: $layout.visible.terminal && aciaEnabled },
      { id: "breakpoints", label: $t("breakpoints.title"), show: $layout.visible.breakpoints },
      { id: "watchpoints", label: $t("watchpoints.title"), show: $layout.visible.watchpoints },
    ].filter((x) => x.show)
  );

  $effect(() => {
    if (compactLayout && !mainTabs.some((x) => x.id === activeMainTab)) {
      activeMainTab = mainTabs[0]?.id ?? "disasm";
    }
  });
  $effect(() => {
    if (compactLayout && !bottomTabs.some((x) => x.id === activeBottomTab)) {
      activeBottomTab = bottomTabs[0]?.id ?? "memory";
    }
  });

  function translate(key: string) {
    return get(t)(key);
  }

  async function withBusy<T>(fn: () => Promise<T>): Promise<T> {
    busyCount++;
    try {
      return await fn();
    } finally {
      busyCount--;
    }
  }

  function syncBreakpointTextsFromDisasm() {
    const next = new Map(breakpointTexts);
    for (const line of disasmLines) {
      if (breakpoints.has(line.address)) {
        next.set(line.address, line.text);
      }
    }
    breakpointTexts = next;
  }

  async function fetchBreakpointText(addr: number) {
    const lines = await api.disassembleRange(addr, 16);
    if (lines[0]) {
      const next = new Map(breakpointTexts);
      next.set(addr, lines[0].text);
      breakpointTexts = next;
    }
  }

  async function syncBreakpointsFromBackend() {
    const bps = await api.getBreakpoints();
    breakpoints = new Set(bps);
    for (const addr of bps) {
      if (!breakpointTexts.has(addr)) {
        await fetchBreakpointText(addr);
      }
    }
    watchpoints = new Set(await api.getWatchpoints());
  }

  async function refreshMachineState() {
    machineState = await api.getMachineState();
    aciaEnabled = machineState.acia.enabled;
    videoFrame = await api.getVideoFrame();
    if (aciaEnabled) {
      aciaTerminal = await api.getAciaTerminal();
    } else {
      aciaTerminal = null;
    }
  }

  async function handleAciaConfigChange(patch: Partial<AciaConfig>) {
    const next: AciaConfig = { ...machineState.acia, ...patch };
    if (patch.enabled !== undefined) {
      aciaEnabled = patch.enabled;
    }
    const dto = await api.setAciaConfig(next);
    machineState = { ...dto, acia: next };
    if (!aciaEnabled) {
      aciaTerminal = null;
      if (compactLayout && activeBottomTab === "terminal") {
        activeBottomTab = "memory";
      }
    } else {
      aciaTerminal = await api.getAciaTerminal();
    }
  }

  async function handleAciaSend(text: string) {
    if (running) {
      await api.aciaSendInput(text);
      aciaTerminal = await api.getAciaTerminal();
      return;
    }
    try {
      aciaTerminal = await api.aciaSendAndRun(text, 50000);
      cpu = await api.getCpuState();
    } catch {
      await api.aciaSendInput(text);
      aciaTerminal = await api.getAciaTerminal();
    }
  }

  async function refresh() {
    cpu = await api.getCpuState();
    await refreshDisasm();
    await refreshMemory();
    trace = (await api.getTrace()).map((s) => ({ ...s, id: ++traceId })).slice(-traceMaxDisplay);
    await syncBreakpointsFromBackend();
    await refreshMachineState();
  }

  async function refreshDisasm() {
    if (!cpu) return;
    const start = Math.max(0, cpu.pc - 32) & 0xfff0;
    disasmBase = start;
    disasmLines = await api.disassembleRange(start, 96);
    syncBreakpointTextsFromDisasm();
  }

  function pcInDisasmRange(pc: number) {
    return pc >= disasmBase && pc < disasmBase + 96;
  }

  function applyTick(payload: api.TickPayload) {
    cpu = payload.cpu;
    lastTrap = payload.step.trap;
    if (payload.step.trap || payload.cpu.halted) {
      running = false;
    }
    trace = [
      ...trace.slice(-(traceMaxDisplay - 1)),
      { ...payload.step, id: ++traceId },
    ];
  }

  async function maybeRefreshVideoOnTick() {
    if (machineState.kind === "bare") return;
    videoFrame = await api.getVideoFrame();
  }

  async function maybeRefreshTerminalOnTick() {
    if (!aciaEnabled) return;
    aciaTerminal = await api.getAciaTerminal();
  }

  function trapLabel(trap: string | null): string | null {
    if (!trap) return null;
    const map: Record<string, string> = {
      Breakpoint: "status.trapBreakpoint",
      Watchpoint: "status.trapWatchpoint",
      Halted: "status.trapHalted",
      IllegalOpcode: "status.trapIllegal",
      Swi: "status.trapSwi",
    };
    const key = map[trap];
    return key ? translate(key) : trap;
  }

  async function maybeRefreshDisasmOnTick(pc: number) {
    if (pcInDisasmRange(pc) || disasmRefreshing) return;
    disasmRefreshing = true;
    try {
      const start = Math.max(0, pc - 32) & 0xfff0;
      disasmBase = start;
      disasmLines = await api.disassembleRange(start, 96);
      syncBreakpointTextsFromDisasm();
    } finally {
      disasmRefreshing = false;
    }
  }

  async function maybeFollowMemory(pc: number) {
    if (!memoryFollowPc) return;
    const aligned = pc & 0xfff0;
    if (memoryAddr !== aligned) {
      memoryAddr = aligned;
      await refreshMemory();
    }
  }

  async function refreshMemory() {
    const chunk = await api.getMemory(memoryAddr, 256);
    memoryBytes = chunk.bytes;
  }

  async function handleApplyConfig() {
    await api.setLoadConfig(loadAddr, resetPc);
    appliedLoadAddr = loadAddr;
    appliedResetPc = resetPc;
  }

  async function ensureConfigApplied(): Promise<boolean> {
    if (!configDirty) return true;
    await handleApplyConfig();
    showToast(translate("config.unapplied"), "warning");
    return true;
  }

  async function handleReset() {
    await withBusy(async () => {
      await api.pauseEmulator();
      running = false;
      await api.resetEmulator();
      await api.clearTrace();
      trace = [];
      await refresh();
    });
  }

  async function handleStep() {
    await withBusy(async () => {
      await api.pauseEmulator();
      running = false;
      const step = await api.stepEmulator();
      lastTrap = step.trap;
      trace = [...trace, { ...step, id: ++traceId }];
      cpu = await api.getCpuState();
      if (!pcInDisasmRange(cpu.pc)) {
        await refreshDisasm();
      }
      await maybeFollowMemory(cpu.pc);
    });
  }

  async function handleRun() {
    if (running) return;
    running = true;
    lastTrap = null;
    try {
      await api.runEmulator();
    } catch {
      running = false;
    }
  }

  async function handlePause() {
    await api.pauseEmulator();
    running = false;
    await refreshDisasm();
  }

  async function handleImport() {
    await withBusy(async () => {
      try {
        await ensureConfigApplied();
        const result = await api.importBinary(
          loadAddr,
          translate("dialog.importTitle")
        );
        if (result) {
          cpu = result;
          await refresh();
          showToast(translate("toast.importSuccess"), "success");
        }
      } catch {
        showToast(translate("toast.importError"), "error");
      }
    });
  }

  async function handleExport() {
    await withBusy(async () => {
      try {
        const ok = await api.exportBinary(
          memoryAddr,
          256,
          translate("dialog.exportTitle")
        );
        if (ok) {
          showToast(translate("toast.exportSuccess"), "success");
        }
      } catch {
        showToast(translate("toast.exportError"), "error");
      }
    });
  }

  async function handleAssemble() {
    assembling = true;
    try {
      await ensureConfigApplied();
      const result = await api.assembleSource(asmSource, loadAddr, true);
      asmErrors = result.errors;
      if (result.errors.length === 0) {
        loadAddr = result.origin;
        appliedLoadAddr = result.origin;
        if (aciaEnabled) {
          await api.resetEmulator();
          cpu = await api.getCpuState();
        }
        await refresh();
        showToast(translate("toast.assembleSuccess"), "success");
      } else {
        showToast(translate("toast.assembleError"), "error");
      }
    } catch {
      showToast(translate("toast.assembleError"), "error");
    } finally {
      assembling = false;
    }
  }

  async function handleSetPc(addr: number) {
    await withBusy(async () => {
      cpu = await api.setCpuRegister("PC", addr);
      if (!pcInDisasmRange(cpu.pc)) {
        await refreshDisasm();
      }
    });
  }

  async function handleRunTo(addr: number) {
    if (running) return;
    await withBusy(async () => {
      await api.pauseEmulator();
      running = false;
      const hadBp = breakpoints.has(addr);
      if (!hadBp) {
        await api.setBreakpoint(addr);
        breakpoints.add(addr);
        breakpoints = new Set(breakpoints);
        await fetchBreakpointText(addr);
      }
      running = true;
      const stopPromise = api.waitForEmulatorStop();
      try {
        await api.runEmulator();
        await stopPromise;
      } finally {
        running = false;
        if (!hadBp) {
          await api.clearBreakpoint(addr);
          breakpoints.delete(addr);
          const next = new Map(breakpointTexts);
          next.delete(addr);
          breakpointTexts = next;
          breakpoints = new Set(breakpoints);
        }
        await refresh();
      }
    });
  }

  async function setupAciaEchoDemo() {
    if (machineState.kind !== "bare") {
      await handleMachineChange("bare");
    }
    const dto = await api.setAciaConfig({
      enabled: true,
      base_addr: 0xffa0,
      baud: 1_000_000,
      e_clock_hz: 1_000_000,
    });
    machineState = { ...dto, acia: { ...dto.acia } };
    aciaEnabled = true;
    aciaTerminal = await api.getAciaTerminal();
  }

  async function handleLoadExample(source: string, exampleId?: string) {
    asmSource = source;
    if (exampleId !== "aciaecho") return;
    await withBusy(async () => {
      try {
        await setupAciaEchoDemo();
        const result = await api.assembleSource(asmSource, loadAddr, true);
        asmErrors = result.errors;
        if (result.errors.length > 0) {
          showToast(translate("toast.assembleError"), "error");
          return;
        }
        loadAddr = result.origin;
        appliedLoadAddr = result.origin;
        await api.resetEmulator();
        cpu = await api.getCpuState();
        if (aciaEnabled) {
          aciaTerminal = await api.aciaRunSteps(200);
        }
        await refresh();
      } catch {
        showToast(translate("toast.sessionError"), "error");
      }
    });
  }

  function dismissOnboarding() {
    showOnboarding = false;
    localStorage.setItem("onboardingDismissed", "true");
  }

  let showOnboarding = $state(
    typeof localStorage !== "undefined" &&
      localStorage.getItem("onboardingDismissed") !== "true"
  );

  async function handleToggleBreakpoint(addr: number) {
    await withBusy(async () => {
      if (breakpoints.has(addr)) {
        breakpoints.delete(addr);
        const next = new Map(breakpointTexts);
        next.delete(addr);
        breakpointTexts = next;
        await api.clearBreakpoint(addr);
      } else {
        breakpoints.add(addr);
        await api.setBreakpoint(addr);
        await fetchBreakpointText(addr);
      }
      breakpoints = new Set(breakpoints);
    });
  }

  async function handleToggleBreakpointAtPc() {
    if (!cpu) return;
    await handleToggleBreakpoint(cpu.pc);
  }

  async function handleClearAllBreakpoints() {
    await withBusy(async () => {
      await api.clearAllBreakpoints();
      breakpoints = new Set();
      breakpointTexts = new Map();
    });
  }

  async function handleToggleWatchpoint(addr: number) {
    await withBusy(async () => {
      if (watchpoints.has(addr)) {
        watchpoints.delete(addr);
        await api.clearWatchpoint(addr);
      } else {
        watchpoints.add(addr);
        await api.setWatchpoint(addr);
      }
      watchpoints = new Set(watchpoints);
    });
  }

  async function handleClearAllWatchpoints() {
    await withBusy(async () => {
      await api.clearAllWatchpoints();
      watchpoints = new Set();
    });
  }

  async function handleWatchpointGoto(addr: number) {
    memoryAddr = addr & 0xfff0;
    await refreshMemory();
  }

  async function handleSaveSession() {
    await withBusy(async () => {
      try {
        const ok = await api.saveSessionDialog(
          asmSource,
          translate("dialog.sessionSaveTitle")
        );
        if (ok) showToast(translate("toast.sessionSaved"), "success");
      } catch {
        showToast(translate("toast.sessionError"), "error");
      }
    });
  }

  async function handleLoadSession() {
    await withBusy(async () => {
      try {
        await api.pauseEmulator();
        running = false;
        const result = await api.loadSessionDialog(
          translate("dialog.sessionLoadTitle")
        );
        if (!result) return;
        cpu = result.cpu;
        breakpoints = new Set(result.breakpoints);
        watchpoints = new Set(result.watchpoints);
        breakpointTexts = new Map();
        for (const addr of result.breakpoints) {
          await fetchBreakpointText(addr);
        }
        if (result.asm_source) {
          asmSource = result.asm_source;
        }
        loadAddr = result.load_config.load_addr;
        resetPc = result.load_config.reset_pc;
        appliedLoadAddr = loadAddr;
        appliedResetPc = resetPc;
        machineState = result.machine;
        await api.clearTrace();
        trace = [];
        await refresh();
        showToast(translate("toast.sessionLoaded"), "success");
      } catch {
        showToast(translate("toast.sessionError"), "error");
      }
    });
  }

  async function handleBreakpointGoto(addr: number) {
    disasmBase = Math.max(0, addr - 32) & 0xfff0;
    disasmLines = await api.disassembleRange(disasmBase, 96);
    syncBreakpointTextsFromDisasm();
    memoryAddr = addr & 0xfff0;
    await refreshMemory();
  }

  async function handleSetRegister(register: string, value: number) {
    await withBusy(async () => {
      cpu = await api.setCpuRegister(register, value);
      if (!pcInDisasmRange(cpu.pc)) {
        await refreshDisasm();
      }
    });
  }

  async function handleToggleFlag(flag: string) {
    await withBusy(async () => {
      cpu = await api.toggleCpuFlag(flag);
    });
  }

  async function handleInterrupt(type: "irq" | "firq" | "nmi") {
    if (running) return;
    await withBusy(async () => {
      try {
        if (type === "irq") {
          cpu = await api.triggerIrq();
          showToast(translate("toast.interruptTriggered"), "info");
        } else if (type === "firq") {
          cpu = await api.triggerFirq();
          showToast(translate("toast.interruptTriggered"), "info");
        } else {
          cpu = await api.triggerNmi();
          if (cpu.lds_encountered) {
            showToast(translate("toast.interruptTriggered"), "info");
          } else {
            showToast(translate("toast.nmiIgnored"), "warning");
          }
        }
      } catch {
        /* ignore */
      }
    });
  }

  async function handleSpeedChange(index: number) {
    runSpeedIndex = index;
    localStorage.setItem("runSpeedIndex", String(index));
    const preset = api.RUN_SPEED_PRESETS[index] ?? api.RUN_SPEED_PRESETS[0];
    await api.setRunSpeed(preset.config);
  }

  async function handleTraceDepthChange(value: number) {
    traceMaxDisplay = value;
    localStorage.setItem("traceMaxDisplay", String(value));
    await api.setTraceLimit(value);
    trace = trace.slice(-value);
  }

  async function handleTraceNavigate(addr: number) {
    await handleBreakpointGoto(addr);
  }

  async function handleMemoryGoto(addr: number) {
    memoryAddr = addr;
    await refreshMemory();
  }

  async function handleMemoryEdit(addr: number, value: number) {
    await api.writeMemory(addr, [value]);
    await refreshMemory();
  }

  async function handleIoWrite(addr: number, value: number) {
    await api.writeMemory(addr, [value]);
    await refreshMachineState();
  }

  async function handleClearTrace() {
    await api.clearTrace();
    trace = [];
  }

  async function handleCpuVariantChange(variant: CpuVariant) {
    if (cpu?.variant === variant) return;
    await withBusy(async () => {
      try {
        await api.pauseEmulator();
        running = false;
        cpu = await api.setCpuVariant(variant);
        await refresh();
        showToast(translate("cpu.changed"), "info");
      } catch {
        showToast(translate("toast.sessionError"), "error");
      }
    });
  }

  async function handleMachineChange(kind: MachineKind) {
    if (machineChanging || machineState.kind === kind) return;
    machineChanging = true;
    await withBusy(async () => {
      try {
        await api.pauseEmulator();
        running = false;
        const result = await api.setMachineProfile(kind);
        cpu = result.cpu;
        loadAddr = result.load_config.load_addr;
        resetPc = result.load_config.reset_pc;
        appliedLoadAddr = loadAddr;
        appliedResetPc = resetPc;
        machineState = result.machine;
        aciaEnabled = machineState.acia.enabled;
        if (kind === "bare") {
          showVideoModal = false;
          setPanel("video", false);
        }

        breakpoints = new Set();
        watchpoints = new Set();
        breakpointTexts = new Map();
        await api.clearTrace();
        trace = [];
        await refresh();
        showToast(translate("machine.changed"), "info");
      } catch {
        showToast(translate("toast.sessionError"), "error");
      } finally {
        machineChanging = false;
      }
    });
  }

  // ---- Layout actions ----

  function handleToggleVideo() {
    if (machineState.kind === "bare") {
      showToast(translate("machine.videoEmpty"), "info");
      return;
    }
    togglePanel("video");
  }

  function handleVideoFullscreen() {
    showVideoModal = true;
  }

  function handleVideoDockClose() {
    setPanel("video", false);
  }

  function handleResetLayout() {
    layout.set(defaultLayoutState());
  }

  function handlePanelClose(id: PanelId) {
    setPanel(id, false);
  }

  // ---- Resize engine ----

  function applyResize(op: ResizeOp, clientX: number, clientY: number): void {
    const L = LAYOUT_LIMITS;
    const sizes = getLayout().sizes;
    if (op.kind === "main-bottom" && contentSplit) {
      const rect = contentSplit.getBoundingClientRect();
      const y = clientY - rect.top;
      const minMain = L.mainMinPx;
      const maxMain = rect.height - L.splitterPx - L.bottomMinPx;
      if (maxMain < minMain) return;
      const mainPct = (clamp(y, minMain, maxMain) / rect.height) * 100;
      patchSizes({ mainPct });
      return;
    }
    if (op.kind === "sidebar-main" && workspaceSplit) {
      const rect = workspaceSplit.getBoundingClientRect();
      const x = clientX - rect.left;
      const hasVideo = videoDockedVisible;
      const reserved = L.splitterPx + (hasVideo ? L.splitterPx + L.videoMinPx : 0);
      const maxSidebar = Math.min(L.sidebarMaxPx, rect.width - reserved - L.disasmMinPx - L.asmMinPx);
      if (maxSidebar < L.sidebarMinPx) return;
      patchSizes({ sidebarPx: clamp(x, L.sidebarMinPx, maxSidebar) });
      return;
    }
    if (op.kind === "disasm-asm" && mainColumns) {
      const rect = mainColumns.getBoundingClientRect();
      const x = clientX - rect.left;
      const maxDisasm = rect.width - L.splitterPx - L.asmMinPx;
      if (maxDisasm < L.disasmMinPx) return;
      const disasmPct = (clamp(x, L.disasmMinPx, maxDisasm) / rect.width) * 100;
      patchSizes({ disasmPct });
      return;
    }
    if (op.kind === "center-video" && workspaceSplit) {
      const rect = workspaceSplit.getBoundingClientRect();
      const videoLeft = rect.right - clientX;
      const maxVideo = Math.min(L.videoMaxPx, rect.width - L.splitterPx - L.sidebarMinPx - L.disasmMinPx - L.asmMinPx - L.splitterPx);
      if (maxVideo < L.videoMinPx) return;
      patchSizes({ videoPx: clamp(videoLeft, L.videoMinPx, maxVideo) });
      return;
    }
  }

  function startResize(op: ResizeOp, event: PointerEvent): void {
    if (compactLayout) return;
    event.preventDefault();
    activeResize = op;
    lastPointer = { x: event.clientX, y: event.clientY };
    const el = event.currentTarget as HTMLElement | null;
    if (el?.setPointerCapture) {
      try {
        el.setPointerCapture(event.pointerId);
      } catch {
        /* ignore */
      }
    }
    if (!resizeListenersAttached) {
      window.addEventListener("pointermove", onResizePointerMove);
      window.addEventListener("pointerup", stopResize);
      window.addEventListener("pointercancel", stopResize);
      resizeListenersAttached = true;
    }
  }

  let resizeListenersAttached = false;

  function onResizePointerMove(event: PointerEvent): void {
    if (!activeResize) return;
    if (activeResize.kind === "group") {
      const el = activeResize.group === "bottom" ? bottomPanelsEl : sidebarEl;
      if (!el) return;
      const rect = el.getBoundingClientRect();
      const total = activeResize.axis === "x" ? rect.width : rect.height;
      if (total <= 0) return;
      const delta = activeResize.axis === "x" ? event.clientX - lastPointer.x : event.clientY - lastPointer.y;
      lastPointer = { x: event.clientX, y: event.clientY };
      const deltaPct = (delta / total) * 100;
      redistribute(activeResize.group, activeResize.a, activeResize.b, deltaPct);
    } else {
      applyResize(activeResize, event.clientX, event.clientY);
    }
  }

  function stopResize(): void {
    if (!resizeListenersAttached) return;
    window.removeEventListener("pointermove", onResizePointerMove);
    window.removeEventListener("pointerup", stopResize);
    window.removeEventListener("pointercancel", stopResize);
    resizeListenersAttached = false;
    activeResize = null;
  }

  function handleSplitterKeydown(op: ResizeOp, event: KeyboardEvent): void {
    if (compactLayout) return;
    const step = event.shiftKey ? 8 : 3;
    let delta = 0;
    if (op.kind === "group") {
      if (op.axis === "x") {
        if (event.key === "ArrowRight") delta = step;
        else if (event.key === "ArrowLeft") delta = -step;
        else return;
      } else {
        if (event.key === "ArrowDown") delta = step;
        else if (event.key === "ArrowUp") delta = -step;
        else return;
      }
      event.preventDefault();
      redistribute(op.group, op.a, op.b, delta);
      return;
    }
    // pixel/percent based
    const pxStep = event.shiftKey ? 80 : 24;
    if (op.kind === "main-bottom") {
      if (event.key === "ArrowDown") delta = pxStep;
      else if (event.key === "ArrowUp") delta = -pxStep;
      else return;
      event.preventDefault();
      const rect = contentSplit?.getBoundingClientRect();
      if (!rect) return;
      const sizes = getLayout().sizes;
      const mainPx = (sizes.mainPct / 100) * rect.height + delta;
      const minMain = LAYOUT_LIMITS.mainMinPx;
      const maxMain = rect.height - LAYOUT_LIMITS.splitterPx - LAYOUT_LIMITS.bottomMinPx;
      if (maxMain < minMain) return;
      patchSizes({ mainPct: (clamp(mainPx, minMain, maxMain) / rect.height) * 100 });
      return;
    }
    if (op.kind === "sidebar-main") {
      if (event.key === "ArrowRight") delta = pxStep;
      else if (event.key === "ArrowLeft") delta = -pxStep;
      else return;
      event.preventDefault();
      const sizes = getLayout().sizes;
      patchSizes({ sidebarPx: clamp(sizes.sidebarPx + delta, LAYOUT_LIMITS.sidebarMinPx, LAYOUT_LIMITS.sidebarMaxPx) });
      return;
    }
    if (op.kind === "disasm-asm") {
      if (event.key === "ArrowRight") delta = pxStep;
      else if (event.key === "ArrowLeft") delta = -pxStep;
      else return;
      event.preventDefault();
      const rect = mainColumns?.getBoundingClientRect();
      if (!rect) return;
      const sizes = getLayout().sizes;
      const disasmPx = (sizes.disasmPct / 100) * rect.width + delta;
      const maxDisasm = rect.width - LAYOUT_LIMITS.splitterPx - LAYOUT_LIMITS.asmMinPx;
      if (maxDisasm < LAYOUT_LIMITS.disasmMinPx) return;
      patchSizes({ disasmPct: (clamp(disasmPx, LAYOUT_LIMITS.disasmMinPx, maxDisasm) / rect.width) * 100 });
      return;
    }
    if (op.kind === "center-video") {
      if (event.key === "ArrowLeft") delta = pxStep;
      else if (event.key === "ArrowRight") delta = -pxStep;
      else return;
      event.preventDefault();
      const sizes = getLayout().sizes;
      patchSizes({ videoPx: clamp(sizes.videoPx + delta, LAYOUT_LIMITS.videoMinPx, LAYOUT_LIMITS.videoMaxPx) });
      return;
    }
  }

  // ---- Status bar derived ----

  const pendingText = $derived.by(() => {
    if (!cpu) return null;
    const parts: string[] = [];
    if (cpu.irq_pending) parts.push("IRQ");
    if (cpu.firq_pending) parts.push("FIRQ");
    if (cpu.nmi_pending) parts.push("NMI");
    return parts.length ? parts.join(" ") : null;
  });

  const cpuLabel = $derived(cpu?.variant === "hd6309" ? $t("cpu.hd6309") : $t("cpu.mc6809"));
  const machineLabel = $derived(
    machineState.kind === "coco2" ? $t("machine.coco2") : machineState.kind === "dragon32" ? $t("machine.dragon32") : $t("machine.bare"),
  );
  const trapText = $derived(!running && lastTrap ? trapLabel(lastTrap) : null);

  $effect(() => {
    localStorage.setItem("memoryFollowPc", String(memoryFollowPc));
  });

  onMount(() => {
    const mq = window.matchMedia("(max-width: 1200px)");
    compactLayout = mq.matches;
    const onResize = (e: MediaQueryListEvent) => {
      compactLayout = e.matches;
    };
    mq.addEventListener("change", onResize);

    let unlistenTick: (() => void) | undefined;
    let unlistenStopped: (() => void) | undefined;

    (async () => {
      try {
        machineProfiles = await api.listMachineProfiles();
        const boot = await api.setMachineProfile("bare");
        cpu = boot.cpu;
        loadAddr = boot.load_config.load_addr;
        resetPc = boot.load_config.reset_pc;
        appliedLoadAddr = loadAddr;
        appliedResetPc = resetPc;
        machineState = boot.machine;
        aciaEnabled = machineState.acia.enabled;
        await handleApplyConfig();
        await api.assembleSource(asmSource, loadAddr, true);
        await api.resetEmulator();
        await handleSpeedChange(runSpeedIndex);
        await api.setTraceLimit(traceMaxDisplay);
        await refresh();
      } catch (e) {
        console.error("Startup failed:", e);
        showToast(translate("toast.sessionError"), "error");
      }

      unlistenTick = await api.onEmulatorTick((payload) => {
        pendingTick = payload;
        if (!tickRaf) {
          tickRaf = requestAnimationFrame(() => {
            tickRaf = 0;
            if (pendingTick) {
              const tick = pendingTick;
              applyTick(tick);
              pendingTick = null;
              void maybeRefreshDisasmOnTick(tick.cpu.pc);
              void maybeFollowMemory(tick.cpu.pc);
              void maybeRefreshVideoOnTick();
              void maybeRefreshTerminalOnTick();
            }
          });
        }
      });

      unlistenStopped = await api.onEmulatorStopped(() => {
        running = false;
        void refresh();
      });
    })().catch(() => {});

    const unregisterShortcuts = registerShortcuts([
      { key: "F5", handler: () => { if (busy) return; void handleRun(); } },
      { key: "F5", shift: true, handler: () => { if (busy) return; void handlePause(); } },
      { key: "F10", handler: () => { if (busy) return; void handleStep(); } },
      { key: "F5", ctrl: true, shift: true, handler: () => { if (busy) return; void handleReset(); } },
      { key: "F9", handler: () => { if (busy) return; void handleToggleBreakpointAtPc(); } },
      { key: "v", handler: () => handleToggleVideo() },
      { key: "t", handler: () => cycleTheme() },
      { key: "l", handler: () => toggleLocale() },
      { key: "?", shift: true, handler: () => { showShortcuts = !showShortcuts; } },
      { key: "Escape", handler: () => { if (showShortcuts) showShortcuts = false; } },
    ]);

    return () => {
      cancelAnimationFrame(tickRaf);
      tickRaf = 0;
      stopResize();
      unlistenTick?.();
      unlistenStopped?.();
      unregisterShortcuts();
      mq.removeEventListener("change", onResize);
    };
  });
</script>

<div class="app">
  {#if showOnboarding}
    <OnboardingBanner onDismiss={dismissOnboarding} />
  {/if}

  <AppHeader
    {running}
    {busy}
    onRun={handleRun}
    onPause={handlePause}
    onStep={handleStep}
    onReset={handleReset}
    onInterrupt={handleInterrupt}
    onImport={handleImport}
    onExport={handleExport}
    onSaveSession={handleSaveSession}
    onLoadSession={handleLoadSession}
    cpuVariant={cpu?.variant ?? "mc6809"}
    onCpuVariantChange={handleCpuVariantChange}
    machineKind={machineState.kind}
    {machineProfiles}
    onMachineChange={handleMachineChange}
    {machineChanging}
    {loadAddr}
    {resetPc}
    onLoadAddrChange={(v) => (loadAddr = v)}
    onResetPcChange={(v) => (resetPc = v)}
    {configDirty}
    onApplyConfig={handleApplyConfig}
    {aciaEnabled}
    onAciaToggle={(enabled) => void handleAciaConfigChange({ enabled })}
    aciaBase={machineState.acia.base_addr}
    onAciaBaseChange={(v) => void handleAciaConfigChange({ base_addr: v })}
    aciaBaud={machineState.acia.baud}
    onAciaBaudChange={(v) => void handleAciaConfigChange({ baud: v })}
    onCycleTheme={cycleTheme}
    onToggleLocale={toggleLocale}
    videoAvailable={machineState.kind !== "bare"}
    videoActive={$layout.visible.video}
    onToggleVideo={handleToggleVideo}
    onOpenShortcuts={() => (showShortcuts = true)}
    onResetLayout={handleResetLayout}
  />

  <div
    class="content-split"
    class:compact={compactLayout}
    class:resizing={activeResize !== null}
    bind:this={contentSplit}
    style={layoutStyle}
  >
    <main
      class="workspace"
      class:compact={compactLayout}
      class:has-sidebar={sidebarVisible && !compactLayout}
      class:has-video={videoDockedVisible && !compactLayout}
      bind:this={workspaceSplit}
    >
      {#if compactLayout}
        <TabBar tabs={mainTabs} active={activeMainTab} onSelect={(id) => (activeMainTab = id)} />
      {/if}

      {#if !compactLayout && sidebarVisible}
        <aside class="sidebar" bind:this={sidebarEl}>
          {#each sidebarItems as item, i}
            {#if i > 0 && !sidebarItems[i - 1].collapsed && !item.collapsed}
              <Splitter
                orientation="horizontal"
                label={$t("layout.resizeSidebar")}
                valueNow={item.share}
                valueMin={8}
                valueMax={80}
                active={activeResize?.kind === "group" && activeResize.group === "sidebarRows"}
                onPointerDown={(e) => startResize({ kind: "group", group: "sidebarRows", a: sidebarItems[i - 1].id, b: item.id, axis: "y" }, e)}
                onKeydown={(e) => handleSplitterKeydown({ kind: "group", group: "sidebarRows", a: sidebarItems[i - 1].id, b: item.id, axis: "y" }, e)}
              />
            {/if}
            <section
              class="side-section"
              class:collapsed={item.collapsed}
              style={item.collapsed ? "flex: 0 0 auto" : `flex: ${item.share} 1 0`}
            >
              {#if item.id === "registers"}
                <RegisterPanel
                  {cpu}
                  collapsed={item.collapsed}
                  onToggleCollapse={() => toggleCollapsed("registers")}
                  onSetRegister={handleSetRegister}
                  onToggleFlag={handleToggleFlag}
                />
              {:else if item.id === "io"}
                <IoPanel
                  kind={machineState.kind}
                  registers={machineState.io_registers}
                  collapsed={item.collapsed}
                  onToggleCollapse={() => toggleCollapsed("io")}
                  onGoto={handleMemoryGoto}
                  onWrite={handleIoWrite}
                />
              {:else if item.id === "breakpoints"}
                <BreakpointList
                  entries={breakpointEntries}
                  collapsed={item.collapsed}
                  onToggleCollapse={() => toggleCollapsed("breakpoints")}
                  onRemove={handleToggleBreakpoint}
                  onClearAll={handleClearAllBreakpoints}
                  onGoto={handleBreakpointGoto}
                />
              {:else if item.id === "watchpoints"}
                <WatchpointList
                  addresses={[...watchpoints].sort((a, b) => a - b)}
                  collapsed={item.collapsed}
                  onToggleCollapse={() => toggleCollapsed("watchpoints")}
                  onRemove={handleToggleWatchpoint}
                  onClearAll={handleClearAllWatchpoints}
                  onGoto={handleWatchpointGoto}
                />
              {/if}
            </section>
          {/each}
        </aside>
      {/if}

      {#if !compactLayout && sidebarVisible}
        <Splitter
          orientation="vertical"
          label={$t("layout.resizeSidebar")}
          valueNow={$layout.sizes.sidebarPx}
          valueMin={LAYOUT_LIMITS.sidebarMinPx}
          valueMax={LAYOUT_LIMITS.sidebarMaxPx}
          active={activeResize?.kind === "sidebar-main"}
          onPointerDown={(e) => startResize({ kind: "sidebar-main" }, e)}
          onKeydown={(e) => handleSplitterKeydown({ kind: "sidebar-main" }, e)}
        />
      {/if}

      <div
        class="main-columns"
        bind:this={mainColumns}
        class:tab-hidden={compactLayout && activeMainTab !== "disasm" && activeMainTab !== "asm"}
        class:only-disasm={disasmVisible && !asmVisible && !compactLayout}
        class:only-asm={!disasmVisible && asmVisible && !compactLayout}
      >
        <section class="center" class:tab-hidden={compactLayout && activeMainTab !== "disasm"} class:hidden={!disasmVisible && !compactLayout}>
          <DisasmPanel
            lines={disasmLines}
            pc={cpu?.pc ?? 0}
            {running}
            {breakpoints}
            onToggleBreakpoint={handleToggleBreakpoint}
            onSetPc={handleSetPc}
            onRunTo={handleRunTo}
          />
        </section>
        {#if disasmVisible && asmVisible && !compactLayout}
          <Splitter
            orientation="vertical"
            label={$t("layout.resizeDisasmAsm")}
            valueNow={$layout.sizes.disasmPct}
            valueMin={30}
            valueMax={70}
            active={activeResize?.kind === "disasm-asm"}
            onPointerDown={(e) => startResize({ kind: "disasm-asm" }, e)}
            onKeydown={(e) => handleSplitterKeydown({ kind: "disasm-asm" }, e)}
          />
        {/if}
        <section class="right" class:tab-hidden={compactLayout && activeMainTab !== "asm"} class:hidden={!asmVisible && !compactLayout}>
          <AsmEditor
            bind:source={asmSource}
            errors={asmErrors}
            {assembling}
            onAssemble={handleAssemble}
            onLoadExample={handleLoadExample}
          />
        </section>
      </div>

      {#if videoDockedVisible}
        <Splitter
          orientation="vertical"
          label={$t("machine.videoTitle")}
          valueNow={$layout.sizes.videoPx}
          valueMin={LAYOUT_LIMITS.videoMinPx}
          valueMax={LAYOUT_LIMITS.videoMaxPx}
          active={activeResize?.kind === "center-video"}
          onPointerDown={(e) => startResize({ kind: "center-video" }, e)}
          onKeydown={(e) => handleSplitterKeydown({ kind: "center-video" }, e)}
        />
        <section class="video-dock">
          <VideoPanel
            frame={videoFrame}
            onGoto={handleMemoryGoto}
            onFullscreen={handleVideoFullscreen}
            onClose={handleVideoDockClose}
          />
        </section>
      {/if}

      {#if compactLayout}
        <section class="compact-panel" class:tab-hidden={activeMainTab !== "registers"}>
          <RegisterPanel
            {cpu}
            onSetRegister={handleSetRegister}
            onToggleFlag={handleToggleFlag}
          />
        </section>
        <section class="compact-panel" class:tab-hidden={activeMainTab !== "io"}>
          <IoPanel
            kind={machineState.kind}
            registers={machineState.io_registers}
            onGoto={handleMemoryGoto}
            onWrite={handleIoWrite}
          />
        </section>
      {/if}
    </main>

    {#if !compactLayout && bottomVisible}
      <Splitter
        orientation="horizontal"
        label={$t("layout.resizeMainBottom")}
        valueNow={$layout.sizes.mainPct}
        valueMin={38}
        valueMax={78}
        active={activeResize?.kind === "main-bottom"}
        onPointerDown={(e) => startResize({ kind: "main-bottom" }, e)}
        onKeydown={(e) => handleSplitterKeydown({ kind: "main-bottom" }, e)}
      />
    {/if}

    <footer class="bottom" class:compact={compactLayout}>
      {#if compactLayout}
        <TabBar tabs={bottomTabs} active={activeBottomTab} onSelect={(id) => (activeBottomTab = id)} />
      {/if}

      <div class="bottom-panels" bind:this={bottomPanelsEl}>
        {#if !compactLayout}
          {#each bottomItems as item, i}
            {#if i > 0}
              <Splitter
                orientation="vertical"
                label={$t("layout.resizeMainBottom")}
                valueNow={item.share}
                valueMin={8}
                valueMax={80}
                active={activeResize?.kind === "group" && activeResize.group === "bottom"}
                onPointerDown={(e) => startResize({ kind: "group", group: "bottom", a: bottomItems[i - 1].id, b: item.id, axis: "x" }, e)}
                onKeydown={(e) => handleSplitterKeydown({ kind: "group", group: "bottom", a: bottomItems[i - 1].id, b: item.id, axis: "x" }, e)}
              />
            {/if}
            <section class="bottom-cell" style={`flex: ${item.share} 1 0`}>
              {#if item.id === "memory"}
                <MemoryView
                  address={memoryAddr}
                  bytes={memoryBytes}
                  bind:followPc={memoryFollowPc}
                  {watchpoints}
                  onGoto={handleMemoryGoto}
                  onEdit={handleMemoryEdit}
                  onToggleWatchpoint={handleToggleWatchpoint}
                  onClose={() => handlePanelClose("memory")}
                />
              {:else if item.id === "trace"}
                <TraceLog
                  entries={trace}
                  maxDisplay={traceMaxDisplay}
                  onMaxDisplayChange={handleTraceDepthChange}
                  onClear={handleClearTrace}
                  onNavigate={handleTraceNavigate}
                  onClose={() => handlePanelClose("trace")}
                />
              {:else if item.id === "terminal"}
                <TerminalPanel
                  terminal={aciaTerminal}
                  baseAddr={machineState.acia.base_addr}
                  onSend={(text) => void handleAciaSend(text)}
                  onClose={() => handlePanelClose("terminal")}
                />
              {/if}
            </section>
          {/each}
        {:else}
          {#each bottomItems as item}
            <section class="bottom-cell" class:tab-hidden={activeBottomTab !== item.id}>
              {#if item.id === "memory"}
                <MemoryView
                  address={memoryAddr}
                  bytes={memoryBytes}
                  bind:followPc={memoryFollowPc}
                  {watchpoints}
                  onGoto={handleMemoryGoto}
                  onEdit={handleMemoryEdit}
                  onToggleWatchpoint={handleToggleWatchpoint}
                  onClose={() => handlePanelClose("memory")}
                />
              {:else if item.id === "trace"}
                <TraceLog
                  entries={trace}
                  maxDisplay={traceMaxDisplay}
                  onMaxDisplayChange={handleTraceDepthChange}
                  onClear={handleClearTrace}
                  onNavigate={handleTraceNavigate}
                  onClose={() => handlePanelClose("trace")}
                />
              {:else if item.id === "terminal"}
                <TerminalPanel
                  terminal={aciaTerminal}
                  baseAddr={machineState.acia.base_addr}
                  onSend={(text) => void handleAciaSend(text)}
                  onClose={() => handlePanelClose("terminal")}
                />
              {/if}
            </section>
          {/each}
          {#if $layout.visible.breakpoints}
            <section class="bottom-cell debug-cell" class:tab-hidden={activeBottomTab !== "breakpoints"}>
              <BreakpointList
                entries={breakpointEntries}
                onRemove={handleToggleBreakpoint}
                onClearAll={handleClearAllBreakpoints}
                onGoto={handleBreakpointGoto}
              />
            </section>
          {/if}
          {#if $layout.visible.watchpoints}
            <section class="bottom-cell debug-cell" class:tab-hidden={activeBottomTab !== "watchpoints"}>
              <WatchpointList
                addresses={[...watchpoints].sort((a, b) => a - b)}
                onRemove={handleToggleWatchpoint}
                onClearAll={handleClearAllWatchpoints}
                onGoto={handleWatchpointGoto}
              />
            </section>
          {/if}
        {/if}
      </div>
    </footer>
  </div>

  <StatusBar
    {running}
    halted={cpu?.halted ?? false}
    {busy}
    pc={cpu?.pc ?? 0}
    cycles={cpu?.total_cycles ?? 0}
    pending={pendingText}
    trap={trapText}
    {cpuLabel}
    {machineLabel}
    speedIndex={runSpeedIndex}
    speedPresets={api.RUN_SPEED_PRESETS}
    onSpeedChange={handleSpeedChange}
    onOpenShortcuts={() => (showShortcuts = true)}
  />
</div>

<VideoModal
  open={showVideoModal}
  frame={videoFrame}
  onClose={() => (showVideoModal = false)}
  onGoto={handleMemoryGoto}
/>

<ShortcutsOverlay open={showShortcuts} onClose={() => (showShortcuts = false)} />

<Toast />

<style>
  .app {
    position: relative;
    z-index: 1;
    display: flex;
    flex-direction: column;
    height: 100dvh;
    padding: 8px;
    gap: 8px;
    min-height: 0;
  }

  .content-split {
    display: grid;
    grid-template-rows: minmax(300px, var(--main-pct)) 10px minmax(200px, 1fr);
    flex: 1;
    min-height: 0;
  }

  .content-split.compact {
    display: flex;
    flex-direction: column;
  }

  .content-split.resizing {
    user-select: none;
    cursor: default;
  }

  .workspace {
    display: grid;
    grid-template-columns: 10px minmax(0, 1fr);
    gap: 0;
    min-height: 0;
    min-width: 0;
  }

  .workspace.has-sidebar {
    grid-template-columns: minmax(240px, var(--sidebar-px)) 10px minmax(0, 1fr);
  }

  .workspace.has-sidebar.has-video {
    grid-template-columns: minmax(240px, var(--sidebar-px)) 10px minmax(0, 1fr) 10px var(--video-px);
  }

  .workspace:not(.has-sidebar).has-video {
    grid-template-columns: minmax(0, 1fr) 10px var(--video-px);
  }

  .sidebar {
    display: flex;
    flex-direction: column;
    gap: 0;
    min-height: 0;
    min-width: 0;
    overflow: hidden;
  }

  .side-section {
    min-height: 0;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .side-section + .side-section {
    margin-top: 0;
  }

  .side-section :global(.panel) {
    height: 100%;
    min-height: 0;
  }

  .main-columns {
    display: grid;
    grid-template-columns: minmax(280px, var(--disasm-pct)) 10px minmax(280px, 1fr);
    min-height: 0;
    min-width: 0;
  }

  .main-columns.only-disasm {
    grid-template-columns: 1fr;
  }

  .main-columns.only-asm {
    grid-template-columns: 1fr;
  }

  .center,
  .right,
  .bottom-cell,
  .video-dock {
    min-height: 0;
    min-width: 0;
  }

  .center,
  .right,
  .video-dock {
    display: flex;
    flex-direction: column;
  }

  .center,
  .right {
    padding: 0;
  }

  .center.hidden,
  .right.hidden {
    display: none;
  }

  .video-dock {
    min-width: var(--video-px);
  }

  .workspace.compact {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
    gap: 6px;
  }

  .workspace.compact .main-columns,
  .workspace.compact .compact-panel {
    flex: 1;
    min-height: 0;
    padding: 0;
  }

  .workspace.compact .main-columns {
    display: flex;
    flex-direction: column;
  }

  .workspace.compact .center,
  .workspace.compact .right {
    flex: 1;
    padding: 0;
  }

  .tab-hidden {
    display: none !important;
  }

  .bottom {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-height: 0;
  }

  .bottom-panels {
    display: flex;
    flex-direction: row;
    gap: 0;
    flex: 1;
    min-height: 0;
    min-width: 0;
  }

  .bottom-cell {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
    overflow: hidden;
  }

  .bottom-cell + .bottom-cell {
    padding-left: 0;
  }

  .bottom.compact .bottom-panels {
    display: flex;
    flex-direction: column;
  }

  .bottom.compact .bottom-cell {
    flex: 1;
    min-height: 0;
  }

  @media (max-width: 700px) {
    .app {
      padding: 6px;
      gap: 6px;
    }
  }
</style>
