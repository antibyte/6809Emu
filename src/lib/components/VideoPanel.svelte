<script module lang="ts">
  import { writable } from "svelte/store";
  import { setJoystick } from "../machineApi";
  import type { VideoFrame } from "../types";

  // Shared by VideoPanel and VideoModal.

  const LITTLE_ENDIAN = new Uint8Array(new Uint32Array([0x0a0b0c0d]).buffer)[0] === 0x0d;

  /** "#rrggbb" → opaque pixel word for an ImageData Uint32 view. */
  function pixelWord(hex: string): number {
    const v = Number.parseInt(hex.replace("#", ""), 16) || 0;
    const r = (v >> 16) & 0xff;
    const g = (v >> 8) & 0xff;
    const b = v & 0xff;
    return LITTLE_ENDIAN
      ? (0xff000000 | (b << 16) | (g << 8) | r) >>> 0
      : ((r << 24) | (g << 16) | (b << 8) | 0xff) >>> 0;
  }

  /** Paints palette-indexed frames onto a canvas; unchanged frames (same hash) are skipped. */
  export class FramePainter {
    #canvas: HTMLCanvasElement | null = null;
    #hash = "";
    #image: ImageData | null = null;
    #words: Uint32Array | null = null;
    #paletteKey = "";
    #lut = new Uint32Array(256);

    paint(canvas: HTMLCanvasElement, frame: VideoFrame): void {
      const { width, height } = frame;
      if (!frame.pixels || !(width > 0) || !(height > 0)) return;
      if (canvas === this.#canvas && frame.hash === this.#hash) return;
      if (canvas.width !== width) canvas.width = width;
      if (canvas.height !== height) canvas.height = height;
      const ctx = canvas.getContext("2d", { alpha: false });
      if (!ctx) return;
      if (!this.#image || this.#image.width !== width || this.#image.height !== height) {
        this.#image = ctx.createImageData(width, height);
        this.#words = new Uint32Array(this.#image.data.buffer);
      }
      const paletteKey = frame.palette.join(",");
      if (paletteKey !== this.#paletteKey) {
        this.#lut.fill(pixelWord("#000000"));
        frame.palette.slice(0, 256).forEach((colour, i) => (this.#lut[i] = pixelWord(colour)));
        this.#paletteKey = paletteKey;
      }
      let indices: string;
      try {
        indices = atob(frame.pixels);
      } catch {
        return;
      }
      const words = this.#words!;
      const lut = this.#lut;
      const count = Math.min(indices.length, words.length);
      for (let i = 0; i < count; i++) {
        words[i] = lut[indices.charCodeAt(i)];
      }
      ctx.putImageData(this.#image, 0, 0);
      this.#canvas = canvas;
      this.#hash = frame.hash;
    }
  }

  /** Scale that fits `w`×`h` into the available box, snapped to whole device pixels when that costs ≤ 15 %. */
  export function fitScale(availW: number, availH: number, w: number, h: number): number {
    if (!(availW > 0) || !(availH > 0) || !(w > 0) || !(h > 0)) return 0;
    const fit = Math.min(availW / w, availH / h);
    const dpr = typeof window !== "undefined" && window.devicePixelRatio > 0 ? window.devicePixelRatio : 1;
    const device = fit * dpr;
    const whole = Math.floor(device);
    return whole >= 1 && whole / device >= 0.85 ? whole / dpr : fit;
  }

  /** Header label of a frame mode ("Text32x16" → "Text"; the size is shown next to it). */
  export function modeLabel(mode: string): string {
    return mode.replace(/^Text\d+x\d+$/, "Text");
  }

  /** Host keys the emulated keyboard never receives: F2–F24 (app shortcuts), Tab (focus). */
  const HOST_KEY = /^(?:Tab|AltGraph|F(?:[2-9]|1\d|2[0-4]))$/;

  /**
   * True when a key event is for the emulated keyboard. Ctrl/Meta/Alt combinations stay with the
   * host (shortcuts), but AltGr characters pass (Windows reports AltGr as Ctrl+Alt).
   */
  export function isMachineKey(e: KeyboardEvent): boolean {
    if (e.isComposing) return false;
    const altGr = e.getModifierState?.("AltGraph") ?? false;
    if (!altGr && (e.ctrlKey || e.metaKey || e.altKey)) return false;
    return !HOST_KEY.test(e.key);
  }

  /** Stable id of a physical key for down/up pairing. */
  export function keyId(e: KeyboardEvent): string {
    return e.code || `key:${e.key}`;
  }

  const MOUSE_JOYSTICK_KEY = "videoMouseJoystick";

  function loadMouseJoystick(): boolean {
    try {
      return localStorage.getItem(MOUSE_JOYSTICK_KEY) === "true";
    } catch {
      return false;
    }
  }

  /** Mouse-as-right-joystick preference (off by default, persisted). */
  export const mouseJoystickEnabled = writable(loadMouseJoystick());

  mouseJoystickEnabled.subscribe((on) => {
    try {
      localStorage.setItem(MOUSE_JOYSTICK_KEY, String(on));
    } catch {
      /* storage unavailable */
    }
  });

  /** Pointer position over the canvas → joystick axes 0..63 across the 256×192 active area. */
  export function joystickAxes(
    pointer: { clientX: number; clientY: number },
    canvas: HTMLCanvasElement,
    frame: VideoFrame,
  ): [number, number] {
    const rect = canvas.getBoundingClientRect();
    if (!(rect.width > 0) || !(rect.height > 0)) return [32, 32];
    const fx = ((pointer.clientX - rect.left) / rect.width) * frame.width - frame.active_x;
    const fy = ((pointer.clientY - rect.top) / rect.height) * frame.height - frame.active_y;
    const axis = (v: number, size: number) => Math.min(63, Math.max(0, Math.floor((v / size) * 64)));
    return [axis(fx, frame.active_width), axis(fy, frame.active_height)];
  }

  /** Drives the right joystick: moves are sent at most once per animation frame, button changes at once. */
  export class MouseJoystick {
    #x = 32;
    #y = 32;
    #button = false;
    #sent = "";
    #raf = 0;

    move(x: number, y: number): void {
      this.#x = x;
      this.#y = y;
      if (!this.#raf) {
        this.#raf = requestAnimationFrame(() => {
          this.#raf = 0;
          this.#flush();
        });
      }
    }

    press(button: boolean): void {
      this.#button = button;
      this.#flush();
    }

    release(): void {
      if (this.#button) this.press(false);
    }

    dispose(): void {
      if (this.#raf) cancelAnimationFrame(this.#raf);
      this.#raf = 0;
      this.release();
    }

    #flush(): void {
      const state = `${this.#x},${this.#y},${this.#button}`;
      if (state === this.#sent) return;
      this.#sent = state;
      setJoystick(0, this.#x, this.#y, this.#button).catch(() => {
        /* no joystick on this machine */
      });
    }
  }
</script>

<script lang="ts">
  import { onDestroy } from "svelte";
  import { t } from "../i18n";
  import * as api from "../api";
  import Icon from "./Icon.svelte";
  import EmptyState from "./EmptyState.svelte";
  import { fmtAddr, fmtByte } from "../format";

  let {
    frame,
    keyboardEnabled = false,
    firmwareLabel = "",
    onGoto,
    onFullscreen,
    onClose,
  }: {
    frame: VideoFrame | null;
    keyboardEnabled?: boolean;
    firmwareLabel?: string;
    onGoto: (addr: number) => void;
    onFullscreen: () => void;
    onClose: () => void;
    /** @deprecated Ignored: the panel sends keys itself via `api.machineKeyEvent(code, down, key)`. */
    onKey?: (code: string, down: boolean) => void;
  } = $props();

  let bodyEl: HTMLDivElement | undefined = $state();
  let canvasEl: HTMLCanvasElement | undefined = $state();
  let focused = $state(false);
  /** Content box of the panel body (space for the screen). */
  let boxW = $state(0);
  let boxH = $state(0);

  const painter = new FramePainter();
  const joystick = new MouseJoystick();
  /** Keys sent down to the machine and not yet released. */
  const pressed = new Set<string>();

  const scale = $derived(frame ? fitScale(boxW - 2, boxH - 2, frame.width, frame.height) : 0);
  const cssWidth = $derived(frame ? frame.width * scale : 0);
  const cssHeight = $derived(frame ? frame.height * scale : 0);
  const details = $derived(
    frame
      ? `${fmtAddr(frame.base_addr)}–${fmtAddr((frame.base_addr + Math.max(frame.vram_bytes, 1) - 1) & 0xffff)} · SAM V=${frame.sam_v} F=${fmtByte(frame.sam_f)} · PIA1 PB=${fmtByte(frame.vdg_ctrl)}`
      : "",
  );

  $effect(() => {
    const canvas = canvasEl;
    const current = frame;
    if (canvas && current) painter.paint(canvas, current);
  });

  $effect(() => {
    const el = bodyEl;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      const box = entries[0]?.contentRect;
      if (!box) return;
      boxW = box.width;
      boxH = box.height;
    });
    ro.observe(el);
    return () => ro.disconnect();
  });

  function send(code: string, down: boolean, key: string) {
    api.machineKeyEvent(code, down, key).catch(() => {
      /* ignore key routing errors while paused/busy */
    });
  }

  function handleKey(e: KeyboardEvent, down: boolean) {
    if (!keyboardEnabled || !focused || !frame) return;
    const id = keyId(e);
    if (down) {
      if (!isMachineKey(e)) return;
      e.preventDefault();
      e.stopPropagation();
      if (e.repeat && pressed.has(id)) return;
      pressed.add(id);
      send(e.code, true, e.key);
    } else if (pressed.delete(id)) {
      e.preventDefault();
      e.stopPropagation();
      send(e.code, false, e.key);
    }
  }

  /** Let go of every key (focus or window lost) so none stays stuck in the matrix. */
  function releaseKeys() {
    const any = pressed.size > 0;
    pressed.clear();
    if (keyboardEnabled && (any || focused)) {
      api.machineKeysClear().catch(() => {});
    }
  }

  function handleBlur() {
    releaseKeys();
    focused = false;
    joystick.release();
  }

  function handleVisibility() {
    if (document.visibilityState === "hidden") {
      releaseKeys();
      joystick.release();
    }
  }

  function toggleMouseJoystick() {
    const on = !$mouseJoystickEnabled;
    mouseJoystickEnabled.set(on);
    if (!on) joystick.release();
  }

  function handlePointerMove(e: PointerEvent) {
    if (!$mouseJoystickEnabled || !frame || !canvasEl) return;
    const [x, y] = joystickAxes(e, canvasEl, frame);
    joystick.move(x, y);
  }

  function handlePointerDown(e: PointerEvent) {
    if (!$mouseJoystickEnabled || !frame || !canvasEl || e.button !== 0) return;
    const [x, y] = joystickAxes(e, canvasEl, frame);
    joystick.move(x, y);
    joystick.press(true);
    bodyEl?.setPointerCapture?.(e.pointerId);
  }

  function handlePointerUp(e: PointerEvent) {
    if (e.button === 0) joystick.release();
  }

  onDestroy(() => {
    releaseKeys();
    joystick.dispose();
  });
</script>

<svelte:window onblur={handleBlur} />
<svelte:document onvisibilitychange={handleVisibility} />

<div class="panel video-panel panel-primary">
  <div class="panel-header">
    <span class="ph-title"><span class="accent-dot"></span>{$t("machine.videoTitle")}</span>
    <div class="ph-actions">
      {#if frame}
        {#if firmwareLabel}
          <span class="fw mono" title={firmwareLabel}>{firmwareLabel}</span>
        {/if}
        {#if keyboardEnabled}
          <span class="kbd-hint mono" class:on={focused}>{$t("machine.kbdHint")}</span>
        {/if}
        <span class="mode mono" title={details}>{modeLabel(frame.mode)}</span>
        <span class="dims mono" title={details}>{frame.cols}×{frame.rows}</span>
        <button
          class="hdr-btn"
          class:active={$mouseJoystickEnabled}
          aria-pressed={$mouseJoystickEnabled}
          onclick={toggleMouseJoystick}
          title={$t("video.mouseJoystickHint")}
          aria-label={$t("video.mouseJoystick")}
        >
          <svg class="icon" width="13" height="13" viewBox="0 0 16 16" aria-hidden="true">
            <circle cx="8" cy="4" r="2.4" fill="currentColor" />
            <path d="M8 6.4v4.2" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" />
            <path d="M2.5 11.5h11v2.5h-11z" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" />
          </svg>
        </button>
        <button class="hdr-btn" onclick={() => onGoto(frame.base_addr)} title={fmtAddr(frame.base_addr)} aria-label={$t("machine.ioGoto")}>
          <Icon name="external" size={13} />
        </button>
        <button class="hdr-btn" onclick={onFullscreen} title={$t("video.fullscreen")} aria-label={$t("video.fullscreen")}>
          <Icon name="expand" size={13} />
        </button>
      {/if}
      <button class="hdr-btn" onclick={onClose} title={$t("panels.close")} aria-label={$t("panels.close")}>
        <Icon name="close" size={13} />
      </button>
    </div>
  </div>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="panel-body crt-body"
    class:kbd-focus={focused && keyboardEnabled}
    class:joystick={$mouseJoystickEnabled && !!frame}
    bind:this={bodyEl}
    tabindex={keyboardEnabled ? 0 : -1}
    role={keyboardEnabled ? "application" : undefined}
    aria-label={keyboardEnabled ? $t("machine.kbdCapture") : undefined}
    onfocus={() => (focused = true)}
    onblur={handleBlur}
    onkeydown={(e) => handleKey(e, true)}
    onkeyup={(e) => handleKey(e, false)}
    onpointermove={handlePointerMove}
    onpointerdown={handlePointerDown}
    onpointerup={handlePointerUp}
    onlostpointercapture={() => joystick.release()}
  >
    {#if !frame}
      <EmptyState icon="video" message={$t("machine.videoEmpty")} size={13} />
    {:else}
      <canvas
        class="screen"
        bind:this={canvasEl}
        style:width="{cssWidth}px"
        style:height="{cssHeight}px"
        aria-label={$t("video.screen")}
      ></canvas>
    {/if}
  </div>
</div>

<style>
  .video-panel {
    height: 100%;
    min-height: 0;
    container-type: inline-size;
  }

  .video-panel .panel-header .ph-title,
  .video-panel .panel-header .mode,
  .video-panel .panel-header .dims,
  .video-panel .panel-header .kbd-hint {
    white-space: nowrap;
  }

  .video-panel .panel-header .ph-title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .video-panel .panel-header .mode {
    color: var(--accent);
    font-size: 10.5px;
  }

  .video-panel .panel-header .dims {
    color: var(--text-faint);
    font-size: 10.5px;
  }

  .video-panel .panel-header .fw {
    color: var(--text-muted);
    font-size: 10px;
    max-width: 12rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .video-panel .panel-header .kbd-hint {
    color: var(--text-faint);
    font-size: 10px;
  }

  .video-panel .panel-header .kbd-hint.on {
    color: var(--accent);
  }

  /* Narrow dock: drop the least important header details first. */
  @container (max-width: 560px) {
    .video-panel .panel-header .fw {
      display: none;
    }
  }

  @container (max-width: 440px) {
    .video-panel .panel-header .kbd-hint {
      display: none;
    }
  }

  @container (max-width: 320px) {
    .video-panel .panel-header .dims {
      display: none;
    }
  }

  .crt-body {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 10px;
    background: var(--crt-bg);
    overflow: hidden;
    outline: none;
    min-height: 0;
    flex: 1;
  }

  .crt-body.kbd-focus {
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 45%, transparent);
  }

  .crt-body.joystick {
    cursor: crosshair;
    touch-action: none;
  }

  .screen {
    display: block;
    flex: none;
    image-rendering: crisp-edges;
    image-rendering: pixelated;
    border: 1px solid var(--crt-border);
    border-radius: 4px;
    box-shadow: 0 0 18px color-mix(in srgb, var(--crt-glow) 35%, transparent);
    user-select: none;
  }
</style>
