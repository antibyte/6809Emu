<script lang="ts">
  import { onMount } from "svelte";
  import type { EditorView } from "@codemirror/view";
  import { t } from "../i18n";
  import { ASM_EXAMPLES } from "../examples";
  import { createAsmEditor, setAsmEditorDoc } from "../codemirror/create-editor";

  let {
    source = $bindable(),
    errors,
    assembling = false,
    onAssemble,
    onLoadExample,
  }: {
    source: string;
    errors: { line: number; message: string }[];
    assembling?: boolean;
    onAssemble: () => void;
    onLoadExample: (source: string, exampleId?: string) => void;
  } = $props();

  let editorHost: HTMLDivElement | undefined = $state();
  let editorView: EditorView | undefined = $state();
  let selectedExample = $state("");

  onMount(() => {
    if (!editorHost) return;
    editorView = createAsmEditor({
      parent: editorHost,
      doc: source,
      onChange: (value) => {
        source = value;
      },
    });
    return () => editorView?.destroy();
  });

  $effect(() => {
    if (editorView && source !== editorView.state.doc.toString()) {
      setAsmEditorDoc(editorView, source);
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
    background: rgba(255, 71, 87, 0.08);
    border: 1px solid rgba(255, 71, 87, 0.3);
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