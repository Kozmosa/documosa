# Simple Editor Anthropic Redesign

## Context

The existing Simple Markdown Editor at `/simple/editor` already supports the core quick-draft workflow: upload Markdown, edit through Tiptap, auto-save locally, download Markdown, and clear the draft. This redesign keeps that behavior unchanged and reworks the interface into an Anthropic-style quiet writing tool.

The design mode is the default Anthropic style: warm, restrained, editorial, and low-distraction. The target use case is a temporary Markdown draft rather than long-form publishing, conversion auditing, or a full document workspace.

## Goals

- Make the editor feel like a calm local writing surface, not a dashboard or admin tool.
- Preserve all current Simple Editor behavior and data flow.
- Keep file actions visible in a thin top toolbar without making them dominate the page.
- Apply Anthropic visual language: warm beige base, raised paper surface, serif editorial typography, restrained orange primary action, and strong handling for destructive actions.
- Improve desktop and narrow-screen readability without adding new product scope.

## Non-goals

- No changes to Markdown/block conversion logic.
- No changes to localStorage key, debounce timing, upload replacement confirmation, download behavior, or clear confirmation.
- No document library, history sidebar, outline panel, slash-command feature, or Markdown source split view.
- No new route or backend API.
- No broad redesign of the main Documosa app.

## Recommended approach

Use an **editing paper layout**.

The page sits on a warm beige background with subtle organic depth. The content is centered in a raised paper panel that contains a thin file toolbar and a calm writing surface. This keeps the quick-draft workflow direct while shifting the visual emphasis from controls to writing.

Alternative approaches considered:

1. **Immersive blank canvas** — quieter, but file operations become too hidden for a utility centered on upload/download.
2. **Editor workbench** — lower implementation risk, but visually remains close to a generic tool page.
3. **Editing paper layout** — best balance: visible controls, strong writing focus, and clear Anthropic visual identity.

## Page structure

### Background and shell

- Use a warm base surface instead of pure white.
- Add one or two subtle radial color washes at low opacity to create organic depth.
- Keep the main content width constrained, with generous outer padding on desktop and practical padding on small screens.
- Avoid a full-width application header. The page should feel like a writing sheet placed on a desk, not a control console.

### Title area

The title area sits above the paper panel:

- Small uppercase product marker: `Documosa`.
- Editorial title: `Simple Markdown Editor`.
- Short supporting line: `A quiet local draft space for quick Markdown edits.`
- Save status on the right on wide screens; below the title stack on narrow screens.

The title uses display/serif styling. Metadata and status use the UI sans font.

### Paper panel

The panel contains two regions:

1. **Thin toolbar**
   - Filename input takes the main horizontal space.
   - Upload is a low-emphasis outline action.
   - Download is the only primary orange action.
   - Clear is visibly destructive using error color treatment, not muted gray.
   - Conversion status appears inline as compact text.

2. **Writing surface**
   - Warm near-white editor area inside the paper panel.
   - Main prose column constrained to roughly 680–760px.
   - Minimum editor height remains comfortable for quick drafts.
   - Empty/loading states stay quiet and do not shift layout dramatically.

## Editor typography

The Tiptap editor keeps its current document model and supported blocks, but receives Anthropic-style presentation:

- Body text uses a serif/editorial feel with increased line height and comfortable paragraph rhythm.
- Headings are more deliberate and restrained, with larger spacing before than after.
- Inline code uses the mono font and a warm muted background.
- Code blocks use a rounded warm-gray container with horizontal overflow preserved.
- Blockquotes use a restrained orange left border and muted text.
- Lists keep clear indentation and spacing without looking dense.
- Placeholder text changes from `Type / for commands...` to a quick-draft prompt such as `Paste or start writing Markdown…`.

The editor should remain a single writing column. Do not introduce side panels, inspector panes, block metadata chrome, or document-outline UI.

## Interaction and state design

### Auto-save

- `Saved…`, `Saving…`, and `Auto-save unavailable` appear as compact status text.
- The status should not occupy a full-width banner in the normal state.
- If local storage fails, editing remains available and the status text clearly reports the problem.

### Upload and conversion

- Upload behavior remains unchanged: uploading over non-empty content asks for confirmation.
- While converting, disable file actions that could conflict with conversion.
- Show `Converting…` inline in the toolbar or title/status region.

### Download

- Download remains the primary action and uses the Anthropic orange accent.
- It should stay visually discoverable because exporting is central to the quick-draft workflow.

### Clear

- Clear remains confirmed before resetting the local draft.
- The button should look destructive using the error color, because clearing can discard unsaved work.
- Do not make Clear the same visual weight as Upload or Download.

### Errors

- Conversion and upload/download errors appear in a narrow alert above the paper panel or at the top of the panel.
- Error color uses the Anthropic error token, with enough contrast to communicate risk.
- Alerts should not cover the editor or require dismissal before the user can continue.

## Responsive behavior

- On wide screens, the title/status row and toolbar can be horizontal.
- On medium screens, the filename input remains full-width and buttons wrap below it.
- On narrow screens, controls stack in logical order: filename, Upload/Download, Clear/status.
- Touch targets should remain at least 44px tall on mobile.
- The editor prose width should shrink naturally without horizontal page scrolling.

## Implementation boundaries

Primary files expected to change:

- `web/src/SimpleEditorPage.tsx` for page layout, toolbar placement, status placement, and visual classes.
- `web/src/TiptapEditor.tsx` for editor prose, block, placeholder, quote, code, and list styling.

Existing behavior to preserve:

- `DRAFT_KEY` value.
- Draft shape: `filename`, `blocks`, `updatedAt`.
- Auto-save debounce timing.
- Upload confirmation when replacing content.
- Clear confirmation when draft has content.
- Markdown conversion API calls through `simpleEditorApi`.
- Download filename fallback.

No new dependencies are required.

## Verification

Automated checks:

- Run the web build or typecheck path used by this project.
- Run the existing Playwright simple editor test if available in the local environment.

Manual browser checks:

1. Open `/simple/editor`.
2. Confirm the page renders as a warm paper-style writing tool.
3. Upload a Markdown file and verify content appears in the editor.
4. Edit content and confirm auto-save status updates.
5. Refresh and confirm the draft restores.
6. Download and inspect the Markdown output.
7. Clear a non-empty draft and confirm the warning appears.
8. Try a narrow viewport and confirm toolbar wrapping remains usable.
9. Trigger or simulate a conversion error and confirm the alert is visible without covering the editor.

## Acceptance criteria

- The simple editor reads visually as a quiet writing tool rather than a generic app page.
- Upload, edit, auto-save, refresh restore, download, and clear still work.
- Download is the only primary action.
- Clear is clearly destructive.
- Normal save/conversion state does not consume a full horizontal status bar.
- The layout remains usable on narrow screens.
