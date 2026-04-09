---
title: Extensions
description: TipTap editor extensions
part_of: '[README](/apps/web/src/lib/README.md)'
exclude:
  - '*.lock'
  - '**/*.ts'
---

# Extensions

Custom TipTap editor extensions.

## Files

| File | Purpose |
|------|---------|
| `AttachmentExtension.ts` | Attachment node extension |
| `HtmlBlock.ts` | Raw HTML block node extension with preview/source toggle |
| `AttachmentPickerNode.ts` | Attachment picker node view; removes its placeholder atom before running the follow-up insert callback, restores a nearby text selection so inline media/HTML attachment inserts cannot land at the document root, and stops ProseMirror from hijacking clicks inside the picker UI. |
| `TableControls.ts` | Notion-style inline table controls (row/column grips, add buttons) |
| `TemplateVariable.ts` | Render-time template variable pills (`{{ variable }}`) with live value resolution |
| `ConditionalBlock.ts` | Conditional block markers (`{{#if}}`, `{{#for-audience}}`, `{{else}}`, `{{/if}}`) with branch decorations |
| `directiveUtils.ts` | Generic markdown directive tokenizer/parser/renderer factories for inline (`:name[content]{attrs}`) and block (`:::name{attrs}`) directives; supports quoted multi-word attributes |
| `EditorGutter.ts` | Generic gutter infrastructure — reserves left padding when directive indicators are present, provides utilities for gutter dots, continuous position-aware dashes, and bars |
| `VisibilityMark.ts` | Inline audience-visibility directive mark (`:vis[text]{audience1 audience2}`) with continuous inline-code dotted underlines, gutter-dot reveal highlights, and preview-aware filter mode |
| `VisibilityBlock.ts` | Block audience-visibility directive markers (`:::vis{"Adam Harris" audience2}` / `:::`) with full-block selection wrapping, editable enclosing-block audiences, continuous gutter dashes with tooltip labels, and filter mode |

Focused Vitest coverage in `AttachmentPickerNode.test.ts` and
`HtmlAttachmentIsolation.test.ts` now also verifies that HTML attachment
insertion via Diaryx's inline-image path stays schema-valid, including the
deferred picker callback path, the live `.focus().setImage(...)` insert path
used by `Editor.svelte`, and a concurrent external markdown `setContent` sync.
