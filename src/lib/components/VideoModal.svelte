<script lang="ts">
  import { onDestroy } from "svelte";
  import { t } from "../i18n";
  import * as api from "../api";
  import Icon from "./Icon.svelte";
  import { fmtAddr } from "../format";
  import type { VideoFrame } from "../types";
  import {
    FramePainter,
    MouseJoystick,
    fitScale,
    isMachineKey,
    joystickAxes,
    keyId,
    modeLabel,
    mouseJoystickEnabled,
  } from "./VideoPanel.svelte";

  let {
    open,
    frame,
    onClose,
    onGoto,
    keyboardEnabled = true,
  }: {
    open: boolean;
    frame: VideoFrame | null;
    onClose: () => void;
    onGoto: (addr: number) => void;
    /** Forward typed keys to the machine while the screen has focus (a frame implies CoCo / Dragon). */
    keyboardEnabled?: boolean;
  } = $props();

  /** Vertical space taken by the backdrop padding, header, body padding, hint line and borders. */
  const CHROME_H = 48 + 49 + 32 + 30 + 6;
  /** Horizontal space taken by the backdrop padding, body padding and borders. */
  const CHROME_W = 48 + 32 + 6;

  let modalEl: HTMLDivElement | undefined = $state();
  let screenEl: HTMLDivElement | undefined = $state();
  let canvasEl: HTMLCanvasElement | undefined = $state();
  let viewportW = $state(0);
  let viewportH = $state(0);
  let focused = $state(false);
  let lastFocused: HTMLElement | null = null;

  const painter = new FramePainter();
  const joystick = new MouseJoystick();
  /** Keys sent down to the machine and not yet released. */
  const pressed = new Set<string>();

  const canType = $derived(keyboardEnabled && !!frame);
  const scale = $derived(
    frame ? fitScale(viewportW - CHROME_W, viewportH - CHROME_H, frame.width, frame.height) : 0,
  );

  $effect(() => {
    const canvas = canvasEl;
    const current = frame;
    if (open && canvas && current) painter.paint(canvas, current);
  });

  $effect(() => {
    if (!open) return;
    lastFocused = document.activeElement as HTMLElement | null;
    const timer = window.setTimeout(() => (screenEl ?? modalEl)?.focus(), 0);
    return () => {
      window.clearTimeout(timer);
      releaseKeys();
      joystick.release();
      lastFocused?.focus?.();
    };
  });

  function send(code: string, down: boolean, key: string) {
    api.machineKeyEvent(code, down, key).catch(() => {
      /* ignore key routing errors while paused/busy */
    });
  }

  function handleScreenKey(e: KeyboardEvent, down: boolean) {
    if (!open || !canType) return;
    // Escape closes the dialog (CLEAR is also on Home).
    if (e.key === "Escape") return;
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

  function releaseKeys() {
    const any = pressed.size > 0;
    pressed.clear();
    if (any) api.machineKeysClear().catch(() => {});
  }

  function handleScreenBlur() {
    focused = false;
    releaseKeys();
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
    screenEl?.setPointerCapture?.(e.pointerId);
  }

  function handlePointerUp(e: PointerEvent) {
    if (e.button === 0) joystick.release();
  }

  function handleKeydown(event: KeyboardEvent) {
    if (open && event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
  }

  function onModalKeydown(event: KeyboardEvent) {
    if (!open || !modalEl) return;
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key === "Tab") {
      const focusables = modalEl.querySelectorAll<HTMLElement>(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
      );
      if (focusables.length === 0) return;
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }
  }

  onDestroy(() => {
    releaseKeys();
    joystick.dispose();
  });
</script>

<svelte:window bind:innerWidth={viewportW} bind:innerHeight={viewportH} onkeydown={handleKeydown} />
<svelte:document onvisibilitychange={handleVisibility} />

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="backdrop" onclick={onClose} role="presentation">
    <div
      class="modal"
      role="dialog"
      aria-modal="true"
      aria-labelledby="video-modal-title"
      tabindex="-1"
      bind:this={modalEl}
      onclick={(e) => e.stopPropagation()}
      onkeydown={onModalKeydown}
    >
      <header class="modal-header">
        <div class="title-group">
          <h2 id="video-modal-title">{$t("machine.videoTitle")}</h2>
          {#if frame}
            <span class="mode mono">{modeLabel(frame.mode)}</span>
            <span class="dims mono">{frame.cols}×{frame.rows}</span>
          {/if}
        </div>
        <div class="actions">
          {#if frame}
            <button
              class="base mono"
              onclick={() => onGoto(frame.base_addr)}
              title={$t("machine.ioGoto")}
            >
              {fmtAddr(frame.base_addr)}
            </button>
            <button
              class="icon-btn"
              class:active={$mouseJoystickEnabled}
              aria-pressed={$mouseJoystickEnabled}
              onclick={toggleMouseJoystick}
              title={$t("video.mouseJoystickHint")}
              aria-label={$t("video.mouseJoystick")}
            >
              <svg class="icon" width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
                <circle cx="8" cy="4" r="2.4" fill="currentColor" />
                <path d="M8 6.4v4.2" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" />
                <path d="M2.5 11.5h11v2.5h-11z" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" />
              </svg>
            </button>
          {/if}
          <button class="icon-btn" onclick={onClose} aria-label={$t("machine.videoClose")}>
            <Icon name="close" size={14} />
          </button>
        </div>
      </header>

      <div class="modal-body">
        {#if !frame}
          <div class="empty">{$t("machine.videoEmpty")}</div>
        {:else}
          <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="screen-wrap"
            class:kbd-focus={focused && canType}
            class:joystick={$mouseJoystickEnabled}
            bind:this={screenEl}
            tabindex={canType ? 0 : -1}
            role={canType ? "application" : undefined}
            aria-label={canType ? $t("machine.kbdCapture") : undefined}
            onfocus={() => (focused = true)}
            onblur={handleScreenBlur}
            onkeydown={(e) => handleScreenKey(e, true)}
            onkeyup={(e) => handleScreenKey(e, false)}
            onpointermove={handlePointerMove}
            onpointerdown={handlePointerDown}
            onpointerup={handlePointerUp}
            onlostpointercapture={() => joystick.release()}
          >
            <canvas
              class="screen"
              bind:this={canvasEl}
              style:width="{frame.width * scale}px"
              style:height="{frame.height * scale}px"
              aria-label={$t("video.screen")}
            ></canvas>
          </div>
          {#if canType}
            <div class="hint mono" class:on={focused}>{$t("video.modalKbdHint")}</div>
          {/if}
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 4000;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    background: rgba(4, 8, 12, 0.74);
    backdrop-filter: blur(6px);
    animation: backdropIn var(--motion-normal) ease;
  }

  @keyframes backdropIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .modal {
    width: fit-content;
    max-width: calc(100vw - 48px);
    max-height: calc(100vh - 48px);
    display: flex;
    flex-direction: column;
    background: var(--bg-1);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-pop);
    overflow: hidden;
    transform-origin: center;
    animation: modalIn var(--motion-slow) var(--ease-tactile);
  }

  .modal:focus-visible {
    outline: none;
  }

  @keyframes modalIn {
    from {
      opacity: 0;
      transform: scale(0.94) translateY(8px);
    }
    to {
      opacity: 1;
      transform: scale(1) translateY(0);
    }
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 10px 16px;
    background: var(--bg-2);
    border-bottom: 1px solid var(--border);
  }

  .title-group {
    display: flex;
    align-items: baseline;
    gap: 12px;
    min-width: 0;
  }

  .title-group h2 {
    margin: 0;
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-dim);
  }

  .mode {
    color: var(--accent);
    font-size: 11.5px;
  }

  .dims {
    color: var(--text-faint);
    font-size: 11px;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }

  .base {
    background: none;
    border: none;
    color: var(--accent);
    cursor: pointer;
    font-size: 12px;
    padding: 4px 8px;
    border-radius: 4px;
  }

  .base:hover {
    background: var(--accent-soft);
  }

  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    background: none;
    border: 1px solid var(--border);
    color: var(--text-dim);
  }

  .icon-btn:hover {
    color: var(--text);
    border-color: var(--accent-dim);
  }

  .icon-btn.active {
    color: var(--accent);
    border-color: var(--accent-line);
    background: var(--accent-soft);
  }

  .icon-btn :global(.icon) {
    margin-right: 0;
  }

  .modal-body {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 16px;
    background: var(--crt-bg);
  }

  .empty {
    padding: 32px 24px;
    color: var(--text-faint);
    text-align: center;
    font-size: 13px;
  }

  .screen-wrap {
    display: flex;
    border-radius: 6px;
    outline: none;
  }

  .screen-wrap.kbd-focus {
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--accent) 55%, transparent);
  }

  .screen-wrap.joystick {
    cursor: crosshair;
    touch-action: none;
  }

  .screen {
    display: block;
    image-rendering: crisp-edges;
    image-rendering: pixelated;
    border: 1px solid var(--crt-border);
    border-radius: 6px;
    box-shadow: 0 0 24px color-mix(in srgb, var(--crt-glow) 35%, transparent);
    user-select: none;
  }

  .hint {
    color: var(--text-faint);
    font-size: 11px;
    line-height: 22px;
  }

  .hint.on {
    color: var(--accent);
  }
</style>
