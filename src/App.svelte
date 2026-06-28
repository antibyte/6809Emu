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
  import Icon from "./lib/components/Icon.svelte";
  import OnboardingBanner from "./lib/components/OnboardingBanner.svelte";
  import TabBar from "./lib/components/TabBar.svelte";
  import IoPanel from "./lib/components/IoPanel.svelte";
  import VideoModal from "./lib/components/VideoModal.svelte";
  import TerminalPanel from "./lib/components/TerminalPanel.svelte";
  import Splitter from "./lib/components/Splitter.svelte";
  import { t, locale, toggleLocale } from "./lib/i18n";
  import { theme, cycleTheme } from "./lib/theme";
  import { showToast } from "./lib/toast";
  import { registerShortcuts } from "./lib/shortcuts";
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
  let narrowLayout = $state(false);
  let activeMainTab = $state("disasm");

  type LayoutSizes = {
    mainPct: number;
    sidebarPx: number;
    disasmPct: number;
  };

  type ResizeTarget =
    | "main-bottom"
    | "sidebar-main"
    | "disasm-asm"

  const LAYOUT_STORAGE_KEY = "layoutSizes";
  const DEFAULT_LAYOUT_SIZES: LayoutSizes = {
    mainPct: 62,
    sidebarPx: 300,
    disasmPct: 52,
  };
  const LAYOUT_LIMITS = {
    splitterPx: 10,
    mainMinPx: 320,
    bottomMinPx: 220,
    sidebarMinPx: 260,
    sidebarMaxPx: 420,
    disasmMinPx: 300,
    asmMinPx: 300,
  } as const;

  function clamp(value: number, min: number, max: number): number {
    return Math.min(max, Math.max(min, value));
  }

  function readLayoutSizes(): LayoutSizes {
    if (typeof localStorage === "undefined") {
      return { ...DEFAULT_LAYOUT_SIZES };
    }
    try {
      const raw = localStorage.getItem(LAYOUT_STORAGE_KEY);
      if (!raw) return { ...DEFAULT_LAYOUT_SIZES };
      const parsed = JSON.parse(raw) as Partial<LayoutSizes> & { bottomTracePct?: number };
      const next = { ...DEFAULT_LAYOUT_SIZES };
      if (typeof parsed.mainPct === "number" && Number.isFinite(parsed.mainPct)) {
        next.mainPct = clamp(parsed.mainPct, 42, 74);
      }
      if (typeof parsed.sidebarPx === "number" && Number.isFinite(parsed.sidebarPx)) {
        next.sidebarPx = clamp(parsed.sidebarPx, 260, 420);
      }
      if (typeof parsed.disasmPct === "number" && Number.isFinite(parsed.disasmPct)) {
        next.disasmPct = clamp(parsed.disasmPct, 36, 64);
      }
      return next;
    } catch {
      return { ...DEFAULT_LAYOUT_SIZES };
    }
  }

  function persistLayoutSizes(sizes: LayoutSizes = layoutSizes): void {
    if (typeof localStorage === "undefined") return;
    localStorage.setItem(LAYOUT_STORAGE_KEY, JSON.stringify(sizes));
  }

  function updateLayoutSizes(patch: Partial<LayoutSizes>): void {
    const next = { ...layoutSizes, ...patch };
    layoutSizes = next;
    persistLayoutSizes(next);
  }

  let layoutSizes = $state<LayoutSizes>(readLayoutSizes());
  let activeResize = $state<ResizeTarget | null>(null);
  let contentSplit: HTMLDivElement | undefined = $state();
  let workspaceSplit: HTMLElement | undefined = $state();
  let mainColumns: HTMLDivElement | undefined = $state();
  let bottomPanels: HTMLDivElement | undefined = $state();

  let layoutStyle = $derived(
    `--main-pct: ${layoutSizes.mainPct}%; --sidebar-px: ${layoutSizes.sidebarPx}px; --disasm-pct: ${layoutSizes.disasmPct}%;`
  );

  let activeBottomTab = $state("memory");
  let showOnboarding = $state(
    typeof localStorage !== "undefined" &&
      localStorage.getItem("onboardingDismissed") !== "true"
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
  let showVideoModal = $state(false);
  let machineChanging = $state(false);

  const mainTabs = $derived([
    { id: "disasm", label: $t("disasm.title") },
    { id: "asm", label: $t("asm.title") },
    { id: "registers", label: $t("registers.title") },
  ]);

  const bottomTabs = $derived(
    compactLayout
      ? [
          { id: "memory", label: $t("memory.title") },
          { id: "trace", label: $t("trace.title") },
          ...(aciaEnabled
            ? [{ id: "terminal", label: $t("acia.title") }]
            : []),
          { id: "debug", label: $t("tabs.debug") },
        ]
      : [
          { id: "memory", label: $t("memory.title") },
          { id: "trace", label: $t("trace.title") },
        ]
  );

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

  $effect(() => {
    localStorage.setItem("memoryFollowPc", String(memoryFollowPc));
  });

  let resizeListenersAttached = false;

  function applyResize(target: ResizeTarget, clientX: number, clientY: number): void {
    const L = LAYOUT_LIMITS;
    if (target === "main-bottom" && contentSplit) {
      const rect = contentSplit.getBoundingClientRect();
      const y = clientY - rect.top;
      const minMain = L.mainMinPx;
      const maxMain = rect.height - L.splitterPx - L.bottomMinPx;
      if (maxMain < minMain) return;
      const mainPx = clamp(y, minMain, maxMain);
      const mainPct = (mainPx / rect.height) * 100;
      updateLayoutSizes({ mainPct });
      return;
    }
    if (target === "sidebar-main" && workspaceSplit) {
      const rect = workspaceSplit.getBoundingClientRect();
      const x = clientX - rect.left;
      const maxSidebar = Math.min(
        L.sidebarMaxPx,
        rect.width - L.splitterPx - L.disasmMinPx - L.asmMinPx
      );
      if (maxSidebar < L.sidebarMinPx) return;
      const sidebarPx = clamp(x, L.sidebarMinPx, maxSidebar);
      updateLayoutSizes({ sidebarPx });
      return;
    }
    if (target === "disasm-asm" && mainColumns) {
      const rect = mainColumns.getBoundingClientRect();
      const x = clientX - rect.left;
      const maxDisasm = rect.width - L.splitterPx - L.asmMinPx;
      if (maxDisasm < L.disasmMinPx) return;
      const disasmPx = clamp(x, L.disasmMinPx, maxDisasm);
      const disasmPct = (disasmPx / rect.width) * 100;
      updateLayoutSizes({ disasmPct });
      return;
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

  function onResizePointerMove(event: PointerEvent): void {
    if (!activeResize) return;
    applyResize(activeResize, event.clientX, event.clientY);
  }

  function startResize(target: ResizeTarget, event: PointerEvent): void {
    if (compactLayout) return;
    event.preventDefault();
    activeResize = target;
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

  function handleSplitterKeydown(target: ResizeTarget, event: KeyboardEvent): void {
    if (compactLayout) return;
    const step = event.shiftKey ? 80 : 24;
    const horizontal =
      target === "main-bottom";
    const vertical =
      target === "sidebar-main" ||
      target === "disasm-asm";
    let delta = 0;
    if (horizontal) {
      if (event.key === "ArrowDown") delta = step;
      else if (event.key === "ArrowUp") delta = -step;
      else return;
    } else if (vertical) {
      if (event.key === "ArrowRight") delta = step;
      else if (event.key === "ArrowLeft") delta = -step;
      else return;
    } else {
      return;
    }
    event.preventDefault();
    const L = LAYOUT_LIMITS;
    if (target === "main-bottom" && contentSplit) {
      const rect = contentSplit.getBoundingClientRect();
      const mainPx =
        (layoutSizes.mainPct / 100) * rect.height + delta;
      const minMain = L.mainMinPx;
      const maxMain = rect.height - L.splitterPx - L.bottomMinPx;
      if (maxMain < minMain) return;
      const clamped = clamp(mainPx, minMain, maxMain);
      updateLayoutSizes({ mainPct: (clamped / rect.height) * 100 });
      return;
    }
    if (target === "sidebar-main" && workspaceSplit) {
      const next = layoutSizes.sidebarPx + delta;
      const rect = workspaceSplit.getBoundingClientRect();
      const maxSidebar = Math.min(
        L.sidebarMaxPx,
        rect.width - L.splitterPx - L.disasmMinPx - L.asmMinPx
      );
      if (maxSidebar < L.sidebarMinPx) return;
      updateLayoutSizes({ sidebarPx: clamp(next, L.sidebarMinPx, maxSidebar) });
      return;
    }
    if (target === "disasm-asm" && mainColumns) {
      const rect = mainColumns.getBoundingClientRect();
      const disasmPx =
        (layoutSizes.disasmPct / 100) * rect.width + delta;
      const maxDisasm = rect.width - L.splitterPx - L.asmMinPx;
      if (maxDisasm < L.disasmMinPx) return;
      const clamped = clamp(disasmPx, L.disasmMinPx, maxDisasm);
      updateLayoutSizes({ disasmPct: (clamped / rect.width) * 100 });
      return;
    }
  }

  onMount(() => {
    const mq = window.matchMedia("(max-width: 1200px)");
    compactLayout = mq.matches;
    const onResize = (e: MediaQueryListEvent) => {
      compactLayout = e.matches;
    };
    mq.addEventListener("change", onResize);

    const narrowMq = window.matchMedia("(max-width: 700px)");
    narrowLayout = narrowMq.matches;
    const onNarrowResize = (e: MediaQueryListEvent) => {
      narrowLayout = e.matches;
    };
    narrowMq.addEventListener("change", onNarrowResize);

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
    ]);

    return () => {
      cancelAnimationFrame(tickRaf);
      tickRaf = 0;
      stopResize();
      unlistenTick?.();
      unlistenStopped?.();
      unregisterShortcuts();
      mq.removeEventListener("change", onResize);
      narrowMq.removeEventListener("change", onNarrowResize);
    };
  });
</script>

<div class="app">
  {#if showOnboarding}
    <OnboardingBanner onDismiss={dismissOnboarding} />
  {/if}

  <header class="toolbar" class:toolbar-narrow={narrowLayout}>
    <div class="brand">
      <span class="logo">6809</span>
      <span class="title">{$t("app.title")}</span>
    </div>

    <div class="controls">
      <button
        class="primary btn-icon"
        onclick={handleRun}
        disabled={running || busy}
        aria-label={$t("shortcuts.run")}
        title={$t("shortcuts.run")}
      ><Icon name="run" />{$t("toolbar.run")}</button>
      <button
        class="btn-icon"
        onclick={handlePause}
        disabled={!running}
        aria-label={$t("shortcuts.pause")}
        title={$t("shortcuts.pause")}
      ><Icon name="pause" />{$t("toolbar.pause")}</button>
      <button
        class="btn-icon"
        onclick={handleStep}
        disabled={running || busy}
        aria-label={$t("shortcuts.step")}
        title={$t("shortcuts.step")}
      ><Icon name="step" />{$t("toolbar.step")}</button>
      <button
        class="btn-icon"
        onclick={handleReset}
        disabled={busy}
        aria-label={$t("shortcuts.reset")}
        title={$t("shortcuts.reset")}
      ><Icon name="reset" />{$t("toolbar.reset")}</button>
      <span class="divider"></span>
      <button
        onclick={() => handleInterrupt("irq")}
        disabled={running}
        title={$t("interrupts.irq")}
        class:narrow-hide={narrowLayout}
      >{$t("interrupts.irq")}</button>
      <button
        onclick={() => handleInterrupt("firq")}
        disabled={running}
        title={$t("interrupts.firq")}
        class:narrow-hide={narrowLayout}
      >{$t("interrupts.firq")}</button>
      <button
        onclick={() => handleInterrupt("nmi")}
        disabled={running}
        title={$t("interrupts.nmi")}
        class:narrow-hide={narrowLayout}
      >{$t("interrupts.nmi")}</button>
      {#if machineState.kind !== "bare"}
        <button
          class="btn-icon"
          onclick={() => (showVideoModal = true)}
          title={$t("machine.videoOpen")}
          aria-label={$t("machine.videoOpen")}
        >{$t("machine.videoTitle")}</button>
      {/if}

      <span class="divider"></span>
      <button
        class="btn-icon"
        onclick={handleImport}
        disabled={busy}
        aria-label={$t("shortcuts.import")}
        title={$t("shortcuts.import")}
      ><Icon name="import" />{$t("toolbar.import")}</button>
      <button
        class="btn-icon"
        onclick={handleExport}
        disabled={busy}
        aria-label={$t("shortcuts.export")}
        title={$t("shortcuts.export")}
      ><Icon name="export" />{$t("toolbar.export")}</button>
      <span class="divider"></span>
      <button onclick={handleSaveSession} class:narrow-hide={narrowLayout}>{$t("session.save")}</button>
      <button onclick={handleLoadSession} class:narrow-hide={narrowLayout}>{$t("session.load")}</button>
    </div>

    <div class="status-bar">
      {#if cpu}
        <span class="status-item">
          {$t("status.pc")}: <strong class="mono accent">${cpu.pc.toString(16).toUpperCase().padStart(4, "0")}</strong>
        </span>
        <span class="status-item">
          {$t("status.cycles")}: <strong class="mono">{cpu.total_cycles.toLocaleString()}</strong>
        </span>
        <span class="status-item">
          {#if running}
            <span class="pulse">●</span> {$t("status.running")}
          {:else}
            {$t("status.halted")}: {cpu.halted ? $t("status.yes") : $t("status.no")}
          {/if}
        </span>
        {#if cpu.irq_pending || cpu.firq_pending || cpu.nmi_pending}
          <span class="status-item pending">
            {$t("interrupts.pending")}:
            {#if cpu.irq_pending}IRQ{/if}
            {#if cpu.firq_pending}{cpu.irq_pending ? " " : ""}FIRQ{/if}
            {#if cpu.nmi_pending}{(cpu.irq_pending || cpu.firq_pending) ? " " : ""}NMI{/if}
          </span>
        {/if}
        {#if lastTrap && !running}
          <span class="status-item trap">
            {$t("status.trap")}: {trapLabel(lastTrap)}
          </span>
        {/if}
      {/if}
      <button class="theme-btn" onclick={cycleTheme} title={$t("theme.label")} aria-label={$t("theme.label")}>
        {#if $theme === "dark"}{$t("theme.dark")}{:else if $theme === "light"}{$t("theme.light")}{:else}{$t("theme.highContrast")}{/if}
      </button>
      <label class="speed-control" class:narrow-hide={narrowLayout}>
        {$t("speed.label")}:
        <select
          value={runSpeedIndex}
          onchange={(e) => handleSpeedChange(parseInt((e.target as HTMLSelectElement).value, 10))}
        >
          {#each api.RUN_SPEED_PRESETS as preset, i}
            <option value={i}>{preset.label}</option>
          {/each}
        </select>
      </label>
      <button class="locale-btn" onclick={toggleLocale} aria-label="Language">
        {$locale === "de" ? "DE" : "EN"}
      </button>
    </div>
  </header>

  <div class="config-bar" class:dirty={configDirty}>
    <label>
      {$t("cpu.label")}:
      <select
        value={cpu?.variant ?? "mc6809"}
        disabled={running}
        onchange={(e) => handleCpuVariantChange((e.target as HTMLSelectElement).value as CpuVariant)}
      >
        <option value="mc6809">{$t("cpu.mc6809")}</option>
        <option value="hd6309">{$t("cpu.hd6309")}</option>
      </select>
    </label>
    <label>
      {$t("machine.label")}:
      <select
        value={machineState.kind}
        disabled={running || machineChanging}
        onchange={(e) => handleMachineChange((e.target as HTMLSelectElement).value as MachineKind)}
      >
        {#each machineProfiles as profile}
          <option value={profile.kind}>{profile.name}</option>
        {/each}
      </select>
    </label>
    <label>
      {$t("config.loadAddr")}:
      <input
        class="mono"
        value={loadAddr.toString(16).toUpperCase()}
        oninput={(e) => {
          const v = parseInt((e.target as HTMLInputElement).value, 16);
          if (!isNaN(v)) loadAddr = v;
        }}
        size="6"
      />
    </label>
    <label>
      {$t("config.resetPc")}:
      <input
        class="mono"
        value={resetPc.toString(16).toUpperCase()}
        oninput={(e) => {
          const v = parseInt((e.target as HTMLInputElement).value, 16);
          if (!isNaN(v)) resetPc = v;
        }}
        size="6"
      />
    </label>
    <button onclick={handleApplyConfig}>{$t("config.apply")}</button>
    <span class="divider-inline"></span>
    <label class="acia-toggle">
      <input
        type="checkbox"
        checked={aciaEnabled}
        disabled={running}
        onchange={(e) =>
          void handleAciaConfigChange({ enabled: (e.target as HTMLInputElement).checked })}
      />
      {$t("acia.enabled")}
    </label>
    {#if aciaEnabled}
      <label>
        {$t("acia.baseAddr")}:
        <input
          class="mono"
          value={machineState.acia.base_addr.toString(16).toUpperCase()}
          disabled={running}
          onchange={(e) => {
            const v = parseInt((e.target as HTMLInputElement).value, 16);
            if (!isNaN(v)) void handleAciaConfigChange({ base_addr: v });
          }}
          size="6"
        />
      </label>
      <label>
        {$t("acia.baud")}:
        <select
          value={machineState.acia.baud}
          disabled={running}
          onchange={(e) =>
            void handleAciaConfigChange({
              baud: parseInt((e.target as HTMLSelectElement).value, 10),
            })}
        >
          <option value={300}>300</option>
          <option value={1200}>1200</option>
          <option value={2400}>2400</option>
          <option value={4800}>4800</option>
          <option value={9600}>9600</option>
          <option value={19200}>19200</option>
          <option value={38400}>38400</option>
        </select>
      </label>
    {/if}
  </div>

  <div
    class="content-split"
    class:compact={compactLayout}
    class:resizing={activeResize !== null}
    bind:this={contentSplit}
    style={layoutStyle}
  >
  <main class="workspace" class:compact={compactLayout} bind:this={workspaceSplit}>
    {#if compactLayout}
      <TabBar tabs={mainTabs} active={activeMainTab} onSelect={(id) => (activeMainTab = id)} />
    {/if}

    <aside class="sidebar" class:tab-hidden={compactLayout && activeMainTab !== "registers"}>
      <RegisterPanel
        {cpu}
        onSetRegister={handleSetRegister}
        onToggleFlag={handleToggleFlag}
      />
      <IoPanel
        kind={machineState.kind}
        registers={machineState.io_registers}
        onGoto={handleMemoryGoto}
        onWrite={handleIoWrite}
      />
      {#if !compactLayout}
        <div class="debug-stack">
          <BreakpointList
            entries={breakpointEntries}
            onRemove={handleToggleBreakpoint}
            onClearAll={handleClearAllBreakpoints}
            onGoto={handleBreakpointGoto}
          />
          <WatchpointList
            addresses={[...watchpoints].sort((a, b) => a - b)}
            onRemove={handleToggleWatchpoint}
            onClearAll={handleClearAllWatchpoints}
            onGoto={handleWatchpointGoto}
          />
        </div>
      {/if}
    </aside>

    {#if !compactLayout}
      <Splitter
        orientation="vertical"
        label={$t("layout.resizeSidebar")}
        valueNow={layoutSizes.sidebarPx}
        valueMin={LAYOUT_LIMITS.sidebarMinPx}
        valueMax={LAYOUT_LIMITS.sidebarMaxPx}
        active={activeResize === "sidebar-main"}
        onPointerDown={(e) => startResize("sidebar-main", e)}
        onKeydown={(e) => handleSplitterKeydown("sidebar-main", e)}
      />
    {/if}

    <div class="main-columns" bind:this={mainColumns} class:tab-hidden={compactLayout && activeMainTab !== "disasm" && activeMainTab !== "asm"}>
      <section class="center" class:tab-hidden={compactLayout && activeMainTab !== "disasm"}>
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
      {#if !compactLayout}
        <Splitter
          orientation="vertical"
          label={$t("layout.resizeDisasmAsm")}
          valueNow={layoutSizes.disasmPct}
          valueMin={36}
          valueMax={64}
          active={activeResize === "disasm-asm"}
          onPointerDown={(e) => startResize("disasm-asm", e)}
          onKeydown={(e) => handleSplitterKeydown("disasm-asm", e)}
        />
      {/if}
      <section class="right" class:tab-hidden={compactLayout && activeMainTab !== "asm"}>
        <AsmEditor
          bind:source={asmSource}
          errors={asmErrors}
          {assembling}
          onAssemble={handleAssemble}
          onLoadExample={handleLoadExample}
        />
      </section>
    </div>
  </main>

  {#if !compactLayout}
    <Splitter
      orientation="horizontal"
      label={$t("layout.resizeMainBottom")}
      valueNow={layoutSizes.mainPct}
      valueMin={42}
      valueMax={74}
      active={activeResize === "main-bottom"}
      onPointerDown={(e) => startResize("main-bottom", e)}
      onKeydown={(e) => handleSplitterKeydown("main-bottom", e)}
    />
  {/if}

  <footer class="bottom" class:compact={compactLayout}>
    {#if compactLayout}
      <TabBar tabs={bottomTabs} active={activeBottomTab} onSelect={(id) => (activeBottomTab = id)} />
    {/if}

    <div
      class="bottom-panels"
      class:three-cols={aciaEnabled}
      bind:this={bottomPanels}
    >
      <div class="bottom-cell" class:tab-hidden={compactLayout && activeBottomTab !== "memory"}>
        <MemoryView
          address={memoryAddr}
          bytes={memoryBytes}
          bind:followPc={memoryFollowPc}
          {watchpoints}
          onGoto={handleMemoryGoto}
          onEdit={handleMemoryEdit}
          onToggleWatchpoint={handleToggleWatchpoint}
        />
      </div>
      <div class="bottom-cell" class:tab-hidden={compactLayout && activeBottomTab !== "trace"}>
        <TraceLog
          entries={trace}
          maxDisplay={traceMaxDisplay}
          onMaxDisplayChange={handleTraceDepthChange}
          onClear={handleClearTrace}
          onNavigate={handleTraceNavigate}
        />
      </div>
      {#if aciaEnabled}
        <div class="bottom-cell" class:tab-hidden={compactLayout && activeBottomTab !== "terminal"}>
          <TerminalPanel
            terminal={aciaTerminal}
            baseAddr={machineState.acia.base_addr}
            onSend={(text) => void handleAciaSend(text)}
          />
        </div>
      {/if}
      {#if compactLayout}
        <div class="debug-panels" class:tab-hidden={activeBottomTab !== "debug"}>
          <BreakpointList
            entries={breakpointEntries}
            onRemove={handleToggleBreakpoint}
            onClearAll={handleClearAllBreakpoints}
            onGoto={handleBreakpointGoto}
          />
          <WatchpointList
            addresses={[...watchpoints].sort((a, b) => a - b)}
            onRemove={handleToggleWatchpoint}
            onClearAll={handleClearAllWatchpoints}
            onGoto={handleWatchpointGoto}
          />
        </div>
      {/if}
    </div>
  </footer>
  </div>

</div>
<VideoModal
  open={showVideoModal}
  frame={videoFrame}
  onClose={() => (showVideoModal = false)}
  onGoto={handleMemoryGoto}
/>

<Toast />

<style>
  .app {
    position: relative;
    z-index: 1;
    display: flex;
    flex-direction: column;
    height: 100dvh;
    padding: 12px;
    gap: 10px;
    min-height: 0;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 10px 16px;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-bottom: 1px solid var(--accent-dim);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    flex-wrap: wrap;
  }


  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .logo {
    font-family: var(--font-mono);
    font-weight: 700;
    font-size: 18px;
    color: var(--accent);
    text-shadow: 0 0 12px rgba(57, 255, 20, 0.4);
    padding: 4px 8px;
    border: 1px solid var(--accent-dim);
    border-radius: 4px;
  }

  .title {
    font-weight: 600;
    font-size: 15px;
    color: var(--text);
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    flex-wrap: wrap;
  }

  .divider {
    width: 1px;
    height: 24px;
    background: var(--border);
    margin: 0 4px;
  }

  .status-bar {
    display: flex;
    align-items: center;
    gap: 16px;
    font-size: 12px;
    color: var(--text-dim);
    flex-wrap: wrap;
  }

  .status-item strong {
    color: var(--text);
  }

  .status-item.pending {
    color: var(--accent-amber);
  }

  .status-item.trap {
    color: var(--danger);
  }

  .theme-btn {
    padding: 4px 10px;
    font-size: 11px;
    font-weight: 600;
  }

  .btn-icon {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  .speed-control {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    color: var(--text-dim);
  }

  .speed-control select {
    padding: 2px 6px;
    font-size: 11px;
    background: var(--bg-deep);
    border: 1px solid var(--border);
    color: var(--text);
    border-radius: 4px;
  }

  .accent {
    color: var(--accent) !important;
  }

  .pulse {
    color: var(--accent);
    animation: pulse 1.2s ease-in-out infinite;
  }

  @keyframes pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.3;
    }
  }

  .locale-btn {
    padding: 4px 10px;
    font-size: 11px;
    font-weight: 600;
    min-width: 36px;
  }

  .config-bar {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 6px 12px;
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 12px;
    color: var(--text-dim);
    transition: border-color 0.2s;
    flex-wrap: wrap;
  }

  .config-bar.dirty {
    border-color: var(--accent-amber);
  }

  .config-bar label {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .config-bar input,
  .config-bar select {
    width: 72px;
    padding: 4px 6px;
    font-size: 11px;
  }

  .divider-inline {
    width: 1px;
    height: 20px;
    background: var(--border);
    flex-shrink: 0;
  }

  .acia-toggle {
    gap: 8px;
    cursor: pointer;
    user-select: none;
  }

  .acia-toggle input {
    width: auto;
    margin: 0;
    cursor: pointer;
  }

  .config-bar select {
    width: auto;
    min-width: 120px;
    background: var(--bg-deep);
    border: 1px solid var(--border);
    color: var(--text);
    border-radius: 4px;
  }

  .content-split {
    display: grid;
    grid-template-rows: minmax(320px, var(--main-pct)) 10px minmax(220px, 1fr);
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
    grid-template-columns: minmax(260px, var(--sidebar-px)) 10px minmax(0, 1fr);
    gap: 0;
    min-height: 0;
    min-width: 0;
  }

  .sidebar {
    display: grid;
    grid-template-rows: max-content minmax(100px, 0.45fr) minmax(180px, 1fr);
    gap: 10px;
    min-height: 0;
    min-width: 0;
    padding-right: 5px;
    overflow: hidden;
  }

  .debug-stack {
    display: grid;
    grid-template-rows: minmax(90px, 1fr) minmax(90px, 1fr);
    gap: 10px;
    min-height: 0;
  }

  .main-columns {
    display: grid;
    grid-template-columns: minmax(300px, var(--disasm-pct)) 10px minmax(300px, 1fr);
    min-height: 0;
    min-width: 0;
  }

  .center,
  .right,
  .bottom-cell {
    min-height: 0;
    min-width: 0;
  }

  .center,
  .right {
    display: flex;
    flex-direction: column;
  }

  .center {
    padding: 0 5px;
  }

  .right {
    padding-left: 5px;
  }

  .workspace.compact {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
  }

  .workspace.compact .sidebar,
  .workspace.compact .main-columns {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
    padding: 0;
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
    gap: 10px;
    min-height: 0;
  }

  .bottom-panels {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
    flex: 1;
    min-height: 0;
    min-width: 0;
  }

  .bottom-panels.three-cols {
    grid-template-columns: 1fr 1fr 1fr;
  }

  .bottom-cell {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
    padding: 0 5px;
    overflow: hidden;
  }

  .bottom.compact .bottom-panels {
    display: flex;
    flex-direction: column;
  }

  .debug-panels {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
    min-height: 0;
  }

  @media (max-width: 1200px) {
    /* compactLayout driven by matchMedia in script */
  }

  @media (max-width: 700px) {
    .toolbar-narrow {
      gap: 8px;
      padding: 8px 10px;
    }

    .toolbar-narrow .controls {
      gap: 4px;
    }

    .toolbar-narrow .btn-icon {
      gap: 2px;
      padding: 6px 8px;
      font-size: 11px;
    }

    .toolbar-narrow .status-bar {
      gap: 6px;
      font-size: 10px;
    }

    .toolbar-narrow .brand .title {
      display: none;
    }

    .config-bar {
      gap: 6px;
      padding: 4px 8px;
      font-size: 10px;
    }

    .config-bar label {
      gap: 3px;
    }
  }

  @media (max-width: 480px) {
    .config-bar .divider-inline {
      display: none;
    }
  }
</style>