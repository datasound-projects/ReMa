# Portfolio Studio

CVs and cover letters built in ReMa. This page records how the studio is built, what was verified, and what it does not do.

## Architecture

| Part | Where | Notes |
| --- | --- | --- |
| Document model | `src-tauri/src/models/portfolio.rs` | `PortfolioDocument { kind (cv / cover_letter), templateId, pageSize (a4 / letter), accent, style, content, letter, sourceDocumentId }`. `content` is the header (with an optional photo data URL) and ordered sections (summary, experience, projects, education, skills, languages, certifications, publications, links, custom). `style` holds the user's design choices; empty fields mean "the template's own". Migration `0019_portfolio_documents.sql` adds the new columns with defaults, so existing CVs open unchanged. |
| Validation and storage | `src-tauri/src/services/portfolio.rs`, `src-tauri/src/db/portfolio.rs` | Limits on sections, entries, tags and text; colors, style options and the photo (PNG/JPEG/WebP data URL up to 400 KB) are checked. `create` starts blank, from sample content, from the Custom Profile, or from reviewed import content. |
| Import | `src-tauri/src/services/portfolio_import.rs` | The picked file becomes a Profile document (never changed). Contact details are found deterministically; the model chosen in Settings reads the rest as JSON. Entries whose employer or title is not in the text are dropped, dates the text does not contain are cleared and flagged, and the result is a proposal (`PortfolioImport`) reviewed in the interface. Without a model, the text is kept in one section. Files without a text layer import empty with a clear note. |
| AI assistant | `src-tauri/src/services/portfolio_ai.rs` | One request per action on the selection, a section or the whole document, through the same provider integration as chat. The system prompt forbids invented facts; afterwards ReMa lists figures (numbers, years, percentages) and new entries that are not in the document, the selection, the job posting or the source material. Nothing is applied by the backend. |
| Templates | `src/lib/portfolio/templates.ts` | 20 CV templates in ten categories (two each) and 5 cover letter templates, each with style tags. Old template ids (`modern`, `ats`, …) map to the closest new design. |
| Style customization | `src/lib/portfolio/style.ts` | Curated palettes, font pairings, type scale, margins, line and section spacing, column layouts, patterns, dividers, header and side panel treatments, justified text, photo. `applyStyle` produces the effective template; unreadable custom colors fall back to the template's, panel text is chosen for contrast. Reset = empty style. |
| Layout engine | `src/lib/portfolio/layout.ts` | Pure: document + effective template → pdfmake definition. Used for thumbnails, the editor's PDF preview and the export. Inline `**bold**`, `_italic_` and `[text](url)` markup; bullets from lines starting with `- `; patterns only behind color bands and tinted panels; page numbers on multi-page documents. |
| Editor | `src/components/portfolio/PortfolioEditor.tsx` + `canvas/`, `ContentPanel.tsx`, `DesignPanel.tsx`, `AiPanel.tsx` | One document model drives the canvas, the forms and the PDF. Undo/redo (`src/hooks/useHistory.ts`, typing coalesced), autosave 700 ms after the last change, browser-side draft recovery (`src/lib/portfolio/recovery.ts`). |
| Canvas | `src/components/portfolio/canvas/DocumentCanvas.tsx` | An editable page styled from the effective template (same fonts, loaded with the FontFace API). Single-line fields are `contentEditable` plain text; descriptions, summaries and the letter body are line editors with bullets and inline formatting (HTML ↔ markup in `src/lib/portfolio/markup.ts`). A floating toolbar on a text selection offers bold, italic, link and "AI". Sections and entries show move / hide / duplicate / delete / add actions when hovered or selected. Page breaks are shown as approximate guides; the PDF preview shows exact pagination. |

## Verified

- `pnpm test`: 171 tests, including rendering every template on A4 and US Letter with full content (projects, certifications, publications, links, custom and hidden sections), a 24-role CV plus a 140-bullet entry across pages, short and nearly empty documents on one page, and every template with a custom style, band header, pattern, side panel and photo. Text is read back with PDF.js and checked against the template's margins.
- Editor tests (`PortfolioEditor.test.tsx`): canvas edits save and are mirrored in the form, section actions with undo/redo, design choices and Reset Style, draft recovery, AI proposals (propose, accept, undo).
- Home tests: tabs, actions, 20 CV templates with category filter, 5 letter templates, new document dialog.
- `cargo test`: 826 tests, including the import (drops entries not in the text, no-model fallback, scanned files, letters) and the assistant (figure and new-entry warnings, model and instruction requirements).
- `pnpm typecheck`, `pnpm lint`, `cargo clippy --all-targets`, `vite build`: clean.
- Every template was rendered to PNG through Chromium and inspected.

## Limitations

- **Editing an original PDF in place is not supported.** Import always rebuilds the document in a template from the extracted text; the original file stays unchanged in the Profile. The import dialog says so. Filling form fields inside an existing PDF is not implemented.
- **No OCR.** Scanned PDFs and images have no text layer; they import as an empty document with a note. Uncertain text highlighting therefore only applies to what the model marked as unclear in real text (entries with unreadable dates, employers or titles).
- **Canvas pagination is approximate.** The editable page is continuous with dashed page guides computed from the paper size; the PDF preview mode (and the export) is exact. Column layouts and header treatments on the canvas are CSS approximations of the PDF; the PDF preview is the reference.
- **Selection-scope AI edits on formatted text.** A proposal replaces the selected text inside its field; if the field carries inline markup that the browser selection does not match, the field's markup is dropped for that replacement.
- **Round photos** are masked in the browser; the layout receives the masked image from the editor, so thumbnails of documents with round photos show a square photo until the editor has produced the mask.
- **Fonts.** Only the five bundled families are available (all open-license); font pairings combine them.
