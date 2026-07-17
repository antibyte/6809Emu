<script lang="ts">
  import { onMount } from "svelte";
  import type { EditorView } from "@codemirror/view";
  import { t } from "../i18n";
  import { theme } from "../theme";
  import { ASM_EXAMPLES } from "../examples";
  import { createAsmEditor, setAsmEditorDoc, updateBreakpoints, type AsmEditorHandle } from "../codemirror/create-editor";
  import InstructionDocModal from "./InstructionDocModal.svelte";

  let {
    source = $bindable(),
    errors,
    assembling = false,
    onAssemble,
    onLoadExample,
    breakpointLines = $bindable(new Set<number>()),
    hasAddress = () => false,
    onToggleBreakpointLine = () => {},
  }: {
    source: string;
    errors: { line: number; message: string }[];
    assembling?: boolean;
    onAssemble: () => void;
    onLoadExample: (source: string, exampleId?: string) => void;
    breakpointLines?: Set<number>;
    hasAddress?: (line: number) => boolean;
    onToggleBreakpointLine?: (line: number) => void;
  } = $props();

  let editorHost: HTMLDivElement | undefined = $state();
  let editorView: EditorView | undefined = $state();
  let handle: AsmEditorHandle | undefined = $state();
  let selectedExample = $state("");
  let helpMnemonic = $state<string | null>(null);

  function handleHelp(mnem: string) {
    helpMnemonic = mnem;
  }

  onMount(() => {
    if (!editorHost) return;
    handle = createAsmEditor({
      parent: editorHost,
      doc: source,
      theme: $theme,
      onChange: (value) => {
        source = value;
      },
      onAssemble: () => onAssemble(),
      onHelpMnemonic: handleHelp,
      hasAddress,
      onToggleBreakpointLine,
    });
    editorView = handle.view;
    return () => handle?.view.destroy();
  });

  $effect(() => {
    $theme;
    handle?.setTheme($theme);
  });

  $effect(() => {
    if (editorView && source !== editorView.state.doc.toString()) {
      setAsmEditorDoc(editorView, source);
    }
  });

  // Keep the reactive handlers and breakpoint markers in sync with props.
  $effect(() => {
    if (handle) {
      handle.handlers.hasAddress = hasAddress;
      handle.handlers.onToggleBreakpointLine = onToggleBreakpointLine;
    }
  });

  $effect(() => {
    if (editorView) {
      updateBreakpoints(editorView, breakpointLines);
    }
  });

  function handleExampleChange(e: Event) {
    const id = (e.target as HTMLSelectElement).value;
    if (!id) return;
    const example = ASM_EXAMPLES.find((ex) => ex.id === id);
    if (example) {
      onLoadExample(example.source, example.id);
      selectedExample = "";
    }
  }
</script>

<div class="panel asm-panel">
  <div class="panel-header">
    <span>{$t("asm.title")}</span>
    <div class="header-actions">
      <select class="examples-select" value={selectedExample} onchange={handleExampleChange}>
        <option value="">{$t("examples.pick")}</option>
        {#each ASM_EXAMPLES as ex}
          <option value={ex.id}>{$t(ex.labelKey)}</option>
        {/each}
      </select>
      <button class="primary" onclick={onAssemble} disabled={assembling}>
        {#if assembling}
          <span class="spinner"></span>
        {/if}
        {$t("toolbar.assemble")}
      </button>
    </div>
  </div>
  <div class="panel-body editor-wrap">
    <div class="editor-host" bind:this={editorHost}></div>
    {#if errors.length > 0}
      <div class="errors">
        <strong>{$t("asm.errors")}:</strong>
        {#each errors as err}
          <div class="error-line">{$t("asm.errorLine")} {err.line}: {err.message}</div>
        {/each}
      </div>
    {/if}
  </div>

  <InstructionDocModal
    open={!!helpMnemonic}
    mnemonic={helpMnemonic}
    onClose={() => (helpMnemonic = null)}
  />
</div>

<style>
  .asm-panel {
    flex: 1;
    min-width: 0;
    height: 100%;
    min-height: 0;
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    text-transform: none;
    letter-spacing: 0;
  }

  .examples-select {
    padding: 4px 8px;
    font-size: 11px;
    background: var(--bg-deep);
    border: 1px solid var(--border);
    color: var(--text);
    border-radius: 4px;
  }

  .editor-wrap {
    display: flex;
    flex-direction: column;
    gap: 8px;
    height: 100%;
    padding: 0 !important;
  }

  .editor-host {
    flex: 1;
    min-height: 200px;
    overflow: hidden;
  }

  .editor-host :global(.cm-editor) {
    height: 100%;
  }

  .editor-host :global(.cm-scroller) {
    overflow: auto;
    padding: 8px 0;
  }

  .errors {
    margin: 0 12px 12px;
    background: var(--danger-soft);
    border: 1px solid var(--danger-line);
    border-radius: 6px;
    padding: 8px 12px;
    font-size: 12px;
    color: var(--danger);
  }

  .error-line {
    font-family: var(--font-mono);
    margin-top: 4px;
  }

  .spinner {
    display: inline-block;
    width: 10px;
    height: 10px;
    border: 2px solid transparent;
    border-top-color: currentColor;
    border-radius: 50%;
    animation: spin 0.6s linear infinite;
    margin-right: 6px;
    vertical-align: -1px;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>