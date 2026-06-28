<script lang="ts">
  type SplitterOrientation = "vertical" | "horizontal";

  let {
    orientation,
    label,
    valueNow,
    valueMin = 0,
    valueMax = 100,
    active = false,
    onPointerDown,
    onKeydown,
  }: {
    orientation: SplitterOrientation;
    label: string;
    valueNow: number;
    valueMin?: number;
    valueMax?: number;
    active?: boolean;
    onPointerDown: (event: PointerEvent) => void;
    onKeydown: (event: KeyboardEvent) => void;
  } = $props();
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex a11y_no_noninteractive_element_interactions -->
<div
  class="splitter"
  class:vertical={orientation === "vertical"}
  class:horizontal={orientation === "horizontal"}
  class:active={active}
  role="separator"
  tabindex="0"
  aria-orientation={orientation}
  aria-label={label}
  aria-valuemin={valueMin}
  aria-valuemax={valueMax}
  aria-valuenow={Math.round(valueNow)}
  onpointerdown={onPointerDown}
  onkeydown={onKeydown}
></div>

<style>
  .splitter {
    display: block;
    position: relative;
    margin: 0;
    padding: 0;
    border: 0;
    background: transparent;
    min-width: 0;
    min-height: 0;
    flex-shrink: 0;
    box-sizing: border-box;
    touch-action: none;
    user-select: none;
    z-index: 2;
  }

  .splitter.vertical {
    width: 10px;
    cursor: col-resize;
  }

  .splitter.horizontal {
    height: 10px;
    width: 100%;
    cursor: row-resize;
  }

  .splitter::before {
    content: "";
    position: absolute;
    background: var(--border);
    transition:
      background 0.15s,
      box-shadow 0.15s;
  }

  .splitter.vertical::before {
    left: 50%;
    top: 0;
    bottom: 0;
    width: 2px;
    transform: translateX(-50%);
  }

  .splitter.horizontal::before {
    top: 50%;
    left: 0;
    right: 0;
    height: 2px;
    transform: translateY(-50%);
  }

  .splitter:hover::before,
  .splitter:focus-visible::before,
  .splitter.active::before {
    background: var(--accent-dim);
    box-shadow: 0 0 6px rgba(57, 255, 20, 0.25);
  }

  .splitter:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 0;
  }
</style>