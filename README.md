# A3 Sheet

A Windows desktop app that turns existing problem-solving files into a reviewed 4-step A3, keeps the evidence with it, and shares early warnings. It is built to `A3 Sheet App Functional Specification.md` (Draft 1), using the spec's default for each of decisions D1 to D6.

## Layout

| Path | What it is |
| --- | --- |
| `crates/a3-core` | Everything without a window: rules, database access (`Repository`), file storage (`AttachmentStore`), file reading, the AI jobs and their guardrails (`AiProvider`), email drafts and export |
| `crates/a3-core/migrations` | The schema. Permissions, step rules, status and user administration live in the database, so a direct connection cannot get around them |
| `crates/a3-core/src/bin/a3-admin.rs` | The admin migration tool: `migrate`, and `bootstrap` for the first admin |
| `src-tauri` | The Tauri 2 shell: commands, Windows Credential Manager, background jobs, links, PDF printing |
| `src` | The interface: React, MUI and Tiptap |
| `e2e` | End-to-end test that drives the real exe, plus sample-file and mock-AI helpers |
| `deploy/link-redirect` | The static `https://` page that forwards to the app (D2, cloud phase) |
| `scripts` | Dev database, PDFium download, setup, end-to-end run |

## Getting started (development)

You need Rust, Node 20 or later, and the Visual Studio C++ build tools. No administrator rights are needed.

```powershell
.\scripts\setup-dev.ps1 -AdminLogin paul -AdminName "Paul" -AdminEmail paul@example.com
npx tauri dev
```

The setup script downloads a portable PostgreSQL 17 into `.dev\` and runs it on `localhost:5433`. There is no Windows service. It also fetches the matching `pdfium.dll`, applies the schema and creates your admin login. pgAdmin can connect to that server with the details in `.dev\dev.env`. At first sign-in, the app's connection form already holds the localhost values, so you only click **Save**.

```powershell
.\scripts\dev-db.ps1 stop      # stop the server
.\scripts\dev-db.ps1 reset     # erase the dev data and start fresh
```

## Tests

```powershell
$env:DATABASE_URL = ((Get-Content .env) -replace '^DATABASE_URL=','')
cargo test -p a3-core          # 89 tests: rules, permissions over direct SQL, extraction, guardrails, export
npx vitest run                 # 27 interface tests, including the 4.5:1 contrast check
npx tauri build --debug --no-bundle
.\scripts\e2e.ps1              # 70 checks against the real exe. This erases the dev database
```

The end-to-end run drives the app through WebView2 against the real database. It uses a local OpenAI-compatible mock (`e2e/mock-ai.mjs`) that also plants a fake number in every import, so the G5 check has something to catch. It writes screenshots to `.dev\e2e\shots` and every export format to `.dev\e2e\exports`.

## Turning imported notes into readable sections

Import review keeps the attachment's wording for traceability. After accepting the relevant proposals, click **Review & summarize A3** on the Sheet tab. AI reviews all four sections, metadata, accepted facts and available source passages together to propose clearer wording and moves into the appropriate A/B/C/D sections. Configure AI in Settings first.

The default **Readable draft** preview shows each section's **At a glance** story: related evidence synthesized into a coherent account, rather than merely rearranged notes. Usually this is one to three short paragraphs around 60-120 words, but length and paragraph count are guidelines, not rejection rules. Expand **Supporting evidence and details** for the complete underlying content, or switch to **Highlighted changes** to inspect additions and deletions. The story and its evidence are saved together only when you choose **Apply all changes**; **Dismiss** leaves the A3 unchanged. Application is atomic. Changes to any section invalidate an old draft; closed sections remain unchanged. Before/after snapshots and an audit entry are saved on acceptance.

Story paragraphs can combine related points from multiple placed blocks and retain links to all their attachment sources. Secondary detail may be omitted from the short lead, but is retained in full below it; no unaccepted attachment facts or unsupported causal links may be added. The evidence blocks can be reordered or transferred between steps without losing source chips, formatting, tables or images. Detailed rewrites must still preserve every fact. Numbers, dates, names, units and certainty are checked, and an independent model check verifies that the story's claims are supported. Accepted fact locations follow moved source text. If source passages exceed the context budget, the review shows its coverage. **Improve** remains available for up to three individual sentence suggestions.

An incorrectly cited or ungrounded story gets at most one automatic correction, limited to affected sections, followed by validation and fact-checking again. Reasonable drafts are not rejected just for exceeding a word or paragraph target. Equivalent quantity notation is accepted in stories (for example, `~68k` can become `approximately 68,000`, and `0.09 mm` can become `0.09mm`); approximation, values, units and identifiers must still be preserved and pass the semantic fact-check. An optional supporting-detail rewrite that fails a factual check is discarded individually: the original passage stays intact, and the preview's **Review notes** explains this. One rejected detail edit no longer prevents a valid story from being reviewed. Corrections cannot change block placements or bypass the factual checks; if correction fails, nothing is applied.

The editor collapses supporting details by default; exports include both the story and all supporting details. Regenerating replaces an unedited generated lead instead of repeatedly appending summaries. Manual edits to a previous lead are retained as supporting details in the next draft.

Choose **Plain English**, **Executive briefing**, or **Technical report** before generating. You can also change the tone in the comparison preview and click **Regenerate with this tone**. The preview labels the tone actually used; applying is disabled during regeneration or when the selected tone does not match the reviewed draft. Failed regeneration leaves the previous preview intact and shows an error.

Preview differences are cached while the generation timer updates. Very large sections use a coarser changed-passage highlight instead of allocating an unbounded word-diff matrix; both the current and proposed text are retained in full.

Summary requests send section text only once while retaining all four sections and the existing contextual memory. Fact checks compare detailed rewrites in both directions and check story claims for support, in batches of up to eight pairs (or about 12 KB of text), with at most two batches in flight. A truncated check gets one larger-budget retry without regenerating the draft. The AI phase has a three-minute deadline and reports failure without modifying the A3. For supported models on the official DeepSeek endpoint, summary editing/checking uses [non-thinking mode](https://api-docs.deepseek.com/guides/thinking_mode) to avoid the provider's default high-effort reasoning; other AI jobs and other endpoints are unchanged.

## Building the installer

```powershell
npx tauri build      # target\release\bundle\nsis\A3 Sheet_0.1.0_x64-setup.exe
```

The installer is per-user (`installMode: currentUser`), so it needs no administrator rights. It bundles `pdfium.dll` and the WebView2 bootstrapper. Code signing (FR-INS-04) and the updater (FR-INS-05, Phase 4) still need a certificate and an update server.

## Going to the cloud database

1. Restore a `pg_dump` of the dev database on the managed server, or run `a3-admin migrate` there as the schema owner.
2. Each user's login is a PostgreSQL role. `a3-admin bootstrap` creates the first admin, and admins create everyone else in Settings.
3. Give users a profile file (`host`, `port`, `database`, `sslMode: "verify-full"`, optional `sslRootCert`). A remote server without `verify-full` is refused.
4. Put PgBouncer in transaction mode in front if you like. The app does not use LISTEN/NOTIFY. It polls every 60 seconds and on window focus.

## Where this build departs from the spec, or goes beyond it

- **Text colours.** Section 6's success, warning and error colours fall below section 15's 4.5:1 contrast when they carry text on their pale fills or under white text. The section 6 values stay for marks, borders and fills. Text uses darker shades of the same hues, listed in `src/theme.ts` and checked by `src/theme.test.ts`.
- **One table more than the spec's list.** `app_setting` holds the stalled threshold and the link base address. Section 14's tables do not list a home for these admin settings.
- **PDF printing** uses WebView2's DevTools `Page.printToPDF`, not the `PrintToPdf` call. Only the DevTools call produces PDF bookmarks and "Page x of y" footers, and the spec asks for both.
- **Legacy `.doc` and `.ppt`** are stored with a "save as a modern format" prompt. This is the spec's fallback while Phase 0 has not proven a reader.
- **Scanned pages** go to the AI model only after **Test connection** has shown that it can read images. A bundled local OCR engine (Phase 0) is not included.
- **Unsaved text when the connection drops** goes to a recovery file. It saves by itself as soon as the connection returns, and is offered for restore at the next launch.

## Still to do outside the code

- Phase 0 checks on your real mail client (new Outlook and `.eml` drafts) and on how your mail and chat tools treat `a3sheet://` links.
- The AI-quality acceptance criteria name a fixed set of 30 real A3s. The guardrails are tested here with a scripted model, but output quality from your chosen model has to be measured on that set with a real key.
- A code-signing certificate, the update server, and a tested backup and restore on the cloud server.
