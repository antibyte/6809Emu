<script lang="ts">
  import { save } from "@tauri-apps/plugin-dialog";
  import { writeFile } from "@tauri-apps/plugin-fs";
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";
  import { showToast } from "../toast";
  import {
    cartridgeEject,
    cartridgeInsert,
    cartridgeState,
    cassetteEject,
    cassetteInsert,
    cassetteRewind,
    cassetteState,
    cassetteTakeRecording,
    printerTakeOutput,
    setJoystick,
    type CartridgeState,
    type CassetteState,
    type JoystickPort,
  } from "../machineApi";

  let {
    pollMs = 400,
    onMachineReset,
    onClose,
  }: {
    /** How often tape / cartridge state and printer output are fetched. */
    pollMs?: number;
    /** Called after inserting / removing a cartridge (the backend resets the machine). */
    onMachineReset?: () => void;
    onClose?: () => void;
  } = $props();

  /** Largest cartridge window ($C000-$FEFF). */
  const CART_MAX_BYTES = 0x4000;
  /** Printer text kept in the view (older text is dropped). */
  const PRINTER_MAX_CHARS = 200_000;

  let cassette = $state<CassetteState | null>(null);
  let cartridge = $state<CartridgeState | null>(null);
  /** False once the backend reports that the machine has no board peripherals. */
  let available = $state(true);
  let printerText = $state("");
  let autostart = $state(true);
  let busy = $state(false);
  let lastRecording: Bytes | null = null;

  let tapeInput: HTMLInputElement | undefined = $state();
  let cartInput: HTMLInputElement | undefined = $state();
  let printerEl: HTMLPreElement | undefined = $state();

  type Bytes = Uint8Array<ArrayBuffer>;

  interface Stick {
    x: number;
    y: number;
    button: boolean;
  }
  let sticks = $state<Stick[]>([
    { x: 32, y: 32, button: false },
    { x: 32, y: 32, button: false },
  ]);

  const tapePercent = $derived(
    cassette && cassette.length > 0
      ? Math.min(100, Math.round((cassette.position / cassette.length) * 100))
      : 0
  );

  // ---- polling ----------------------------------------------------------

  let polling = false;

  async function poll() {
    if (polling) return;
    polling = true;
    try {
      const [cas, cart, text] = await Promise.all([
        cassetteState(),
        cartridgeState(),
        printerTakeOutput(),
      ]);
      available = cas !== null;
      cassette = cas;
      cartridge = cart;
      if (text) appendPrinter(text);
    } catch {
      // Backend busy or no CoCo / Dragon board: keep the last state.
    } finally {
      polling = false;
    }
  }

  $effect(() => {
    const interval = Math.max(100, pollMs);
    poll();
    const id = setInterval(poll, interval);
    return () => clearInterval(id);
  });

  // ---- printer ----------------------------------------------------------

  let stickToBottom = true;

  function appendPrinter(text: string) {
    let next = printerText + text;
    if (next.length > PRINTER_MAX_CHARS) {
      next = next.slice(next.length - PRINTER_MAX_CHARS);
    }
    printerText = next;
    queueMicrotask(scrollPrinterToEnd);
  }

  function handlePrinterScroll() {
    const el = printerEl;
    if (!el) return;
    stickToBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
  }

  function scrollPrinterToEnd() {
    const el = printerEl;
    if (!el || !stickToBottom) return;
    el.scrollTop = el.scrollHeight;
  }

  function clearPrinter() {
    printerText = "";
    stickToBottom = true;
  }

  async function copyPrinter() {
    if (!printerText) return;
    try {
      await navigator.clipboard.writeText(printerText);
      showToast($t("peripherals.copied"), "success", 2000);
    } catch (e) {
      showToast(errorText(e), "error");
    }
  }

  async function savePrinter() {
    if (!printerText) return;
    await saveBytes("printer.txt", new TextEncoder().encode(printerText), "Text", ["txt"]);
  }

  // ---- files ------------------------------------------------------------

  function errorText(e: unknown): string {
    return e instanceof Error ? e.message : String(e);
  }

  async function readPicked(input: HTMLInputElement | undefined): Promise<{ name: string; bytes: Bytes } | null> {
    const file = input?.files?.[0];
    if (input) input.value = ""; // allow picking the same file again
    if (!file) return null;
    try {
      return { name: file.name, bytes: new Uint8Array(await file.arrayBuffer()) };
    } catch (e) {
      showToast(`${$t("peripherals.loadFailed")}: ${errorText(e)}`, "error");
      return null;
    }
  }

  function downloadBlob(name: string, bytes: Bytes) {
    const blob = new Blob([bytes], { type: "application/octet-stream" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = name;
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  /** Native save dialog + fs write; falls back to a browser download. */
  async function saveBytes(
    name: string,
    bytes: Bytes,
    filterName: string,
    extensions: string[]
  ): Promise<boolean> {
    let path: string | null;
    try {
      path = await save({
        title: $t("peripherals.save"),
        defaultPath: name,
        filters: [{ name: filterName, extensions }],
      });
    } catch {
      downloadBlob(name, bytes);
      return true;
    }
    if (!path) return false;
    try {
      await writeFile(path, bytes);
      return true;
    } catch (e) {
      showToast(`${$t("peripherals.saveFailed")}: ${errorText(e)}`, "error");
      return false;
    }
  }

  async function run(action: () => Promise<void>) {
    if (busy) return;
    busy = true;
    try {
      await action();
    } catch (e) {
      showToast(errorText(e), "error");
    } finally {
      busy = false;
    }
  }

  // ---- cassette ---------------------------------------------------------

  async function onTapePicked() {
    const picked = await readPicked(tapeInput);
    if (!picked) return;
    await run(async () => {
      cassette = await cassetteInsert(picked.name, Array.from(picked.bytes));
      // The deck refuses empty or absurdly large (> 4 MiB) images.
      if (cassette && !cassette.loaded) {
        showToast(`${$t("peripherals.loadFailed")}: ${picked.name}`, "error");
      }
    });
  }

  function rewindTape() {
    run(async () => {
      cassette = await cassetteRewind();
    });
  }

  function ejectTape() {
    run(async () => {
      cassette = await cassetteEject();
    });
  }

  /** Take the CSAVE recording (or reuse the last one taken). */
  async function takeRecording(): Promise<Bytes | null> {
    const bytes = new Uint8Array(await cassetteTakeRecording());
    if (bytes.length > 0) lastRecording = bytes;
    if (!lastRecording || lastRecording.length === 0) {
      showToast($t("peripherals.nothingRecorded"), "warning");
      return null;
    }
    return lastRecording;
  }

  function saveRecording() {
    run(async () => {
      const bytes = await takeRecording();
      if (bytes) await saveBytes("recording.cas", bytes, "Cassette", ["cas"]);
      cassette = await cassetteState();
    });
  }

  function useRecordingAsTape() {
    run(async () => {
      const bytes = await takeRecording();
      if (bytes) cassette = await cassetteInsert("recording.cas", Array.from(bytes));
    });
  }

  // ---- cartridge --------------------------------------------------------

  async function onCartPicked() {
    const picked = await readPicked(cartInput);
    if (!picked) return;
    if (picked.bytes.length > CART_MAX_BYTES) {
      showToast($t("peripherals.cartridgeTooBig"), "warning");
    }
    await run(async () => {
      cartridge = await cartridgeInsert(picked.name, Array.from(picked.bytes), autostart);
      onMachineReset?.();
    });
  }

  function ejectCartridge() {
    run(async () => {
      cartridge = await cartridgeEject();
      onMachineReset?.();
    });
  }

  // ---- joysticks --------------------------------------------------------

  /** Slider drags are coalesced to one backend update per this many ms. */
  const STICK_SEND_MS = 16;
  const sent = ["", ""];
  const pending: (ReturnType<typeof setTimeout> | null)[] = [null, null];

  function sendStick(port: JoystickPort) {
    const s = sticks[port];
    const key = `${s.x},${s.y},${s.button}`;
    if (key === sent[port]) return;
    sent[port] = key;
    setJoystick(port, s.x, s.y, s.button).catch(() => {
      /* no joystick on this machine */
    });
  }

  /** Coalesce slider drags (a timer, not rAF: it also fires while the
   *  window is not being painted). */
  function queueStick(port: JoystickPort) {
    if (pending[port] !== null) return;
    pending[port] = setTimeout(() => {
      pending[port] = null;
      sendStick(port);
    }, STICK_SEND_MS);
  }

  function setAxis(port: JoystickPort, axis: "x" | "y", value: number) {
    sticks[port][axis] = Math.max(0, Math.min(63, Math.round(value)));
    queueStick(port);
  }

  function setFire(port: JoystickPort, down: boolean) {
    if (sticks[port].button === down) return;
    sticks[port].button = down;
    sendStick(port);
  }

  function centerStick(port: JoystickPort) {
    sticks[port].x = 32;
    sticks[port].y = 32;
    sendStick(port);
  }

  function fireKey(port: JoystickPort, e: KeyboardEvent, down: boolean) {
    if (e.key === " " || e.key === "Enter") {
      e.preventDefault();
      setFire(port, down);
    }
  }

  $effect(() => () => {
    for (const id of pending) if (id !== null) clearTimeout(id);
    // Never leave a fire button held down when the panel goes away.
    for (const port of [0, 1] as JoystickPort[]) {
      const s = sticks[port];
      if (s.button) {
        setJoystick(port, s.x, s.y, false).catch(() => {
          /* no joystick on this machine */
        });
      }
    }
  });

  const PORTS: { port: JoystickPort; label: string }[] = [
    { port: 0, label: "peripherals.right" },
    { port: 1, label: "peripherals.left" },
  ];
</script>

<div class="panel periph-panel panel-primary">
  <div class="panel-header">
    <span class="ph-title">
      <span class="accent-dot"></span>
      {$t("peripherals.title")}
    </span>
    <div class="ph-actions">
      {#if onClose}
        <button class="hdr-btn" onclick={onClose} title={$t("panels.close")} aria-label={$t("panels.close")}>
          <Icon name="close" size={13} />
        </button>
      {/if}
    </div>
  </div>

  <div class="panel-body periph-body">
    {#if !available}
      <div class="empty-line">{$t("peripherals.unavailable")}</div>
    {:else}
      <!-- Cassette -->
      <section class="group" aria-label={$t("peripherals.cassette")}>
        <div class="group-head">
          <span class="group-title">{$t("peripherals.cassette")}</span>
          <span class="chip mono" class:on={cassette?.motor} title={$t("peripherals.motor")}>
            {cassette?.motor ? $t("peripherals.motorOn") : $t("peripherals.motorOff")}
          </span>
        </div>
        <div class="media-line">
          <Icon name="file" size={12} />
          <span class="media-name mono" title={cassette?.name ?? ""}>
            {cassette?.loaded ? cassette.name ?? "—" : $t("peripherals.noTape")}
          </span>
        </div>
        {#if cassette?.loaded}
          <div
            class="progress"
            role="progressbar"
            aria-label={$t("peripherals.position")}
            aria-valuemin={0}
            aria-valuemax={cassette.length}
            aria-valuenow={cassette.position}
          >
            <div class="progress-fill" style="width: {tapePercent}%"></div>
          </div>
          <div class="sub mono">
            {$t("peripherals.position")}: {cassette.position} / {cassette.length} ({tapePercent}%)
          </div>
        {/if}
        <div class="btn-row">
          <button class="btn" disabled={busy} onclick={() => tapeInput?.click()}>
            <Icon name="folder-open" size={12} />{$t("peripherals.insertTape")}
          </button>
          <button class="btn" disabled={busy || !cassette?.loaded} onclick={rewindTape}>
            <Icon name="reset" size={12} />{$t("peripherals.rewind")}
          </button>
          <button class="btn" disabled={busy || !cassette?.loaded} onclick={ejectTape}>
            <Icon name="export" size={12} />{$t("peripherals.eject")}
          </button>
        </div>
        <input
          class="file-input"
          type="file"
          accept=".cas,.CAS"
          bind:this={tapeInput}
          onchange={onTapePicked}
          tabindex="-1"
          aria-hidden="true"
        />
        <div class="rec-line">
          <span class="rec-label">{$t("peripherals.recording")}</span>
          <span class="mono rec-count" class:on={(cassette?.recorded_bytes ?? 0) > 0}>
            {cassette?.recorded_bytes ?? 0} {$t("peripherals.bytes")}
          </span>
        </div>
        <div class="btn-row">
          <button class="btn" disabled={busy} onclick={saveRecording}>
            <Icon name="save" size={12} />{$t("peripherals.saveRecording")}
          </button>
          <button class="btn" disabled={busy} onclick={useRecordingAsTape}>
            <Icon name="play" size={12} />{$t("peripherals.useRecording")}
          </button>
        </div>
        <div class="hint">{$t("peripherals.recordingHint")}</div>
      </section>

      <!-- Printer -->
      <section class="group" aria-label={$t("peripherals.printer")}>
        <div class="group-head">
          <span class="group-title">{$t("peripherals.printer")}</span>
          <div class="head-actions">
            <button class="mini-btn" disabled={!printerText} onclick={copyPrinter}>
              {$t("peripherals.copy")}
            </button>
            <button class="mini-btn" disabled={!printerText} onclick={savePrinter}>
              {$t("peripherals.save")}
            </button>
            <button class="mini-btn" disabled={!printerText} onclick={clearPrinter}>
              {$t("peripherals.clear")}
            </button>
          </div>
        </div>
        {#if printerText}
          <pre class="paper mono" bind:this={printerEl} onscroll={handlePrinterScroll}>{printerText}</pre>
        {:else}
          <div class="paper paper-empty">{$t("peripherals.printerEmpty")}</div>
        {/if}
      </section>

      <!-- Cartridge -->
      <section class="group" aria-label={$t("peripherals.cartridge")}>
        <div class="group-head">
          <span class="group-title">{$t("peripherals.cartridge")}</span>
          {#if cartridge?.loaded}
            <span class="chip mono" class:on={cartridge.autostart}>
              {cartridge.autostart ? $t("peripherals.autostart") : "$C000"}
            </span>
          {/if}
        </div>
        <div class="media-line">
          <Icon name="chip" size={12} />
          <span class="media-name mono" title={cartridge?.name ?? ""}>
            {cartridge?.loaded
              ? `${cartridge.name ?? "—"} · ${cartridge.size} ${$t("peripherals.bytes")}`
              : $t("peripherals.noCartridge")}
          </span>
        </div>
        <div class="btn-row">
          <button class="btn" disabled={busy} onclick={() => cartInput?.click()}>
            <Icon name="folder-open" size={12} />{$t("peripherals.insertCartridge")}
          </button>
          <button class="btn" disabled={busy || !cartridge?.loaded} onclick={ejectCartridge}>
            <Icon name="export" size={12} />{$t("peripherals.eject")}
          </button>
          <label class="check" title={$t("peripherals.autostartHint")}>
            <input type="checkbox" bind:checked={autostart} />
            {$t("peripherals.autostart")}
          </label>
        </div>
        <input
          class="file-input"
          type="file"
          accept=".rom,.ccc,.bin,.dgn"
          bind:this={cartInput}
          onchange={onCartPicked}
          tabindex="-1"
          aria-hidden="true"
        />
        <div class="hint">{$t("peripherals.cartridgeReset")}</div>
      </section>

      <!-- Joysticks -->
      <section class="group" aria-label={$t("peripherals.joysticks")}>
        <div class="group-head">
          <span class="group-title">{$t("peripherals.joysticks")}</span>
        </div>
        {#each PORTS as { port, label } (port)}
          <div class="stick">
            <div class="stick-head">
              <span class="stick-name">{$t(label)}</span>
              <button class="link-btn" onclick={() => centerStick(port)}>{$t("peripherals.center")}</button>
            </div>
            <label class="axis">
              <span class="axis-name mono">X</span>
              <input
                type="range"
                min="0"
                max="63"
                step="1"
                value={sticks[port].x}
                oninput={(e) => setAxis(port, "x", e.currentTarget.valueAsNumber)}
                aria-label={`${$t(label)} X`}
              />
              <span class="axis-val mono">{sticks[port].x}</span>
            </label>
            <label class="axis">
              <span class="axis-name mono">Y</span>
              <input
                type="range"
                min="0"
                max="63"
                step="1"
                value={sticks[port].y}
                oninput={(e) => setAxis(port, "y", e.currentTarget.valueAsNumber)}
                aria-label={`${$t(label)} Y`}
              />
              <span class="axis-val mono">{sticks[port].y}</span>
            </label>
            <button
              class="fire"
              class:down={sticks[port].button}
              aria-pressed={sticks[port].button}
              onpointerdown={(e) => {
                e.currentTarget.setPointerCapture(e.pointerId);
                setFire(port, true);
              }}
              onpointerup={() => setFire(port, false)}
              onpointercancel={() => setFire(port, false)}
              onlostpointercapture={() => setFire(port, false)}
              onkeydown={(e) => fireKey(port, e, true)}
              onkeyup={(e) => fireKey(port, e, false)}
              onblur={() => setFire(port, false)}
            >
              {$t("peripherals.fire")}
            </button>
          </div>
        {/each}
        <div class="hint">{$t("peripherals.joystickHint")}</div>
      </section>
    {/if}
  </div>
</div>

<style>
  .periph-panel {
    height: 100%;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .periph-body {
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
    overflow: auto;
  }

  .empty-line {
    color: var(--text-faint);
    font-size: 11px;
    padding: 8px 4px;
  }

  .group {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding-bottom: 10px;
    border-bottom: 1px solid var(--border);
  }

  .group:last-child {
    border-bottom: none;
    padding-bottom: 0;
  }

  .group-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .group-title {
    color: var(--text-faint);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-size: 9px;
    font-weight: 600;
  }

  .head-actions {
    display: flex;
    gap: 4px;
  }

  .mini-btn {
    background: none;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    color: var(--accent);
    font-size: 10px;
    padding: 1px 6px;
    cursor: pointer;
  }

  .mini-btn:hover:not(:disabled) {
    border-color: var(--accent-line);
    background: var(--accent-soft);
  }

  .mini-btn:disabled {
    color: var(--text-faint);
    cursor: default;
  }

  .chip {
    font-size: 9px;
    font-weight: 700;
    color: var(--text-dim);
    background: var(--bg-1);
    padding: 1px 6px;
    border-radius: 3px;
  }

  .chip.on {
    color: var(--accent);
    background: var(--accent-soft);
  }

  .media-line {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-dim);
    font-size: 11px;
    min-width: 0;
  }

  .media-name {
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .progress {
    height: 4px;
    border-radius: 2px;
    background: var(--bg-0);
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    background: var(--accent);
    transition: width 0.3s ease;
  }

  .sub {
    color: var(--text-faint);
    font-size: 10px;
  }

  .btn-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }

  .btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    background: var(--bg-0);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 4px 9px;
    font-size: 11px;
    cursor: pointer;
  }

  .btn:hover:not(:disabled) {
    border-color: var(--accent-line);
    color: var(--accent);
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .file-input {
    display: none;
  }

  .rec-line {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    font-size: 10.5px;
    padding-top: 4px;
  }

  .rec-label {
    color: var(--text-dim);
  }

  .rec-count {
    color: var(--text-faint);
    font-weight: 600;
  }

  .rec-count.on {
    color: var(--accent);
  }

  .hint {
    color: var(--text-faint);
    font-size: 9.5px;
    line-height: 1.4;
  }

  .paper {
    margin: 0;
    min-height: 64px;
    max-height: 220px;
    overflow: auto;
    padding: 6px 8px;
    background: var(--bg-0);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
    font-size: 11px;
    line-height: 1.35;
    white-space: pre-wrap;
    word-break: break-all;
  }

  .paper-empty {
    color: var(--text-faint);
    font-size: 10.5px;
  }

  .check {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    color: var(--text-dim);
    cursor: pointer;
  }

  .stick {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 6px 8px;
    border-radius: var(--radius-sm);
    background: var(--bg-0);
  }

  .stick-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .stick-name {
    color: var(--text-dim);
    font-size: 10.5px;
    font-weight: 600;
  }

  .link-btn {
    background: none;
    border: none;
    color: var(--accent);
    font-size: 10px;
    cursor: pointer;
    padding: 0;
  }

  .axis {
    display: grid;
    grid-template-columns: 14px 1fr 22px;
    align-items: center;
    gap: 6px;
  }

  .axis input[type="range"] {
    width: 100%;
    accent-color: var(--accent);
  }

  .axis-name {
    color: var(--text-faint);
    font-size: 10px;
  }

  .axis-val {
    color: var(--text);
    font-size: 10.5px;
    text-align: right;
  }

  .fire {
    align-self: flex-start;
    background: var(--bg-1);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 3px 14px;
    font-size: 10.5px;
    font-weight: 600;
    cursor: pointer;
    user-select: none;
    touch-action: none;
  }

  .fire.down {
    background: var(--accent-soft);
    border-color: var(--accent-line);
    color: var(--accent);
  }
</style>
