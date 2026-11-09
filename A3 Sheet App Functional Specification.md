# A3 Sheet App: Functional Specification

Nov 9, 2026 · @Paul · Draft 1 for review

## 1. Summary and scope

The app is a Windows desktop tool that turns existing problem-solving files into a reviewed 4-step A3, keeps the evidence with it, and shares early warnings across the organisation.

A user drops in the reports they already have. The app extracts the content, AI places it into the four steps, and the user approves it with clicks. Management sees status without anyone writing a report, because status comes from the steps themselves.

**Design principles.** Every requirement below follows from these six.

1. **Click before type.** Typing stays available, but it is the fallback.
2. **AI proposes, the user approves.** No AI text reaches an A3, the wall or an export without a user's Accept.
3. **Every AI statement traces to a source.** If it cannot be traced, it is not shown.
4. **AI says little.** One line first, at most three items at a time. The user asks for more.
5. **Status is derived, never typed.** Step open/closed state is the only input.
6. **The app works without AI.** Records, attachments, exports and the wall all function with no API key set.

**Out of scope for this version:** dark mode, macOS/Linux/mobile/web clients, real-time co-editing, mandatory method forms (5-Why, fishbone, FMEA), multi-level approval workflows, and sending email from the app.

## 2. Decisions to confirm

Six decisions need your sign-off before build. Each has a default the rest of this spec assumes, and D1 and D2 are the ones that are costly to change later.

| # | Decision | Default in this spec | Why | If you choose otherwise |
| --- | --- | --- | --- | --- |
| D1 | How the exe reaches the cloud database | Each user has their own PostgreSQL login, row-level security enforces permissions, and the exe connects directly over TLS | Honours the direct-to-PostgreSQL design. No shared database password sits inside the exe, where anyone could extract it | If IT will not expose PostgreSQL to user networks, add a thin Rust API server. The repository layer (section 5) contains that change |
| D2 | How A3 links work | `a3sheet://a3/A3-2026-0042` until the cloud phase, then an `https://` link that opens the app | Custom-scheme links need no server, but email systems may strip or block them (section 11) | Host the small `https://` redirect page earlier, if links in email matter from day one |
| D3 | Who can read what | Every signed-in user can read every A3. Only project members can edit | Wall posts link to A3s, and management needs the full picture | Restricted projects are possible, but wall links then lead some readers to "no access" |
| D4 | A3 content sent to AI | Attachment text goes to the AI provider each user configures. A per-project switch can turn AI off | Users supply their own API, so the data path must be explicit | Point every user at a company-approved or private endpoint (OpenAI-compatible URLs are supported) |
| D5 | Where attachments are stored | In PostgreSQL, 50 MB per file, behind a storage interface | Matches your brief and keeps backup simple | Move to object storage in the cloud phase if volume grows. The interface isolates the change |
| D6 | Who closes a step | The A3 owner, in order A to D, with no approver | Fastest path. Management sees every close and reopen | Add a reviewer sign-off on step D only, as a small extension |

**Assumptions**

- Windows 10 or 11, 64-bit. Up to 200 users in the first year.
- English interface. Documents in other languages depend on the chosen AI model.
- An admin creates user accounts. There is no self-registration.
- pgAdmin is the management tool only. Development also needs a local PostgreSQL server (version 16 or later), which pgAdmin connects to.

## 3. Users and core journey

Three roles cover everyone, and management needs no special role because the management view is open to all signed-in users.

| Role | Can do |
| --- | --- |
| User (everyone) | Read all A3s and the wall, mark warnings read, create follow-up tasks, draft emails, export, see the management view |
| Project member | All of the above, plus create and edit A3s in that project and post warnings from them. The A3 owner closes and reopens steps |
| Admin | Create users, reset access, set company pick-lists and the link base address |

**The fast path.** A user who already has a report should reach a reviewed A3 in under 10 minutes, typing only corrections.

1. Click **New A3** and drop in the files. The project defaults to the last one used.
2. The app reads the files locally. AI proposes the title, metadata and content for each step.
3. On one review screen, the user clicks Accept, Edit or Reject on each proposal.
4. AI reviews the result once and shows at most three findings.
5. If others should know, the user clicks **Post warning**, checks the AI draft and posts it.
6. As work continues, the user drops in new files. AI shows only what is new or conflicting.
7. The owner closes each step, then exports the A3.

**How typing is avoided**

| Input | How it is filled |
| --- | --- |
| Title, problem date, product, area, category | Proposed by AI from the files, with a one-click Accept |
| A3 number, date registered, owner, last updated | Automatic |
| Project, team, severity, source of problem | Default value, chips or a pick-list |
| Target close date | Chips: +30, +60, +90 days, or a date picker |
| Status and current step | Derived from the steps. Never entered |
| The four step descriptions | Accepted from file proposals. Screenshots can be pasted |
| Early warning text | AI draft from the A3, confirmed by the user |
| Follow-up tasks | One-click suggestions from the warning's quick check |
| Task result | Three chips: Not affected, Affected, Not applicable |
| Email to colleagues | Generated from the warning, then opened as a draft |
| Closing a step, reopening a step | One click, with a reason chip on reopen |

## 4. The A3 method model

Each step answers one question, and the app checks only for the few things that make that answer credible.

This table drives the placeholder hints in each text box, where AI places imported content, and what the validity review looks for (section 9.3). Text boxes stay free-form, with no forced sub-fields.

| Step | Question | A good answer contains | Typical weakness AI flags |
| --- | --- | --- | --- |
| A. Define the problem | What is wrong, how big is it, and what is the target? | What happened, where, when, which product or process. Measured gap against the standard. Impact. Target | A cause or solution inside the problem statement. No number |
| B. Analyze root cause | Why did it happen, and why was it not caught? | How the problem was narrowed. Causes tested. Confirmed root cause for occurrence and for escape, each with evidence | Cause stated without evidence. Stops at a symptom or at "operator error" |
| C. Implement solution | What did we do, now and permanently? | Temporary containment and permanent solutions, each with owner, date and state. Each permanent solution tied to a root cause | Action with no root cause. Root cause with no action |
| D. Validate and standardize | Did it work, and will it last? | Result measured the same way as in A, over a stated period. Standards updated, by document name. Sharing with similar lines | Different metric from A. No named standard |

Immediate containment belongs in step C, labelled Temporary. Step C can be edited from day one, so containment is recorded when it happens, without waiting for step B to close.

**Step status rules**

| ID | Rule |
| --- | --- |
| FR-ST-01 | Each step is Open or Closed. All four start Open |
| FR-ST-02 | Any open step can be edited at any time. Users often work on B and C together |
| FR-ST-03 | Steps close in order A, B, C, D, so a report can never show validation complete before the cause is closed |
| FR-ST-04 | Closing needs non-empty text and one click by the A3 owner |
| FR-ST-05 | AI findings never block a close. The close dialog shows any open conflict in one line |
| FR-ST-06 | A closed step is read-only until reopened |
| FR-ST-07 | Reopening asks for a reason chip: New evidence, Result not sustained, Correction, Other. It is logged |
| FR-ST-08 | Reopening does not reopen later steps. They stay closed, and the A3 returns to the earliest open step |

**Derived A3 status**

| Step state | A3 status | Current step shown |
| --- | --- | --- |
| A open | Open | A. Define |
| A closed, B open | Open | B. Analyze |
| A and B closed, C open | Open | C. Implement |
| A, B, C closed, D open | Open | D. Validate |
| Any earlier step reopened | Open | Earliest open step |
| All four closed | Closed | None. Closed date is the last close |

Three health flags sit beside the status and are also computed: **Overdue** (target date passed), **Stalled** (no change for 14 days, configurable) and **Reopened**. An owner can also **withdraw** a duplicate or mistaken A3 with a reason. It then leaves all counts but remains retrievable.

## 5. Platform and architecture

The app is a Tauri 2 desktop program with all logic in Rust, a web-technology interface, and PostgreSQL as the only shared store.

&#91;embedded content: application architecture\]

**Technology choices**

| Layer | Choice | Reason |
| --- | --- | --- |
| App shell | Tauri 2 with WebView2 | Small installer, Rust backend, NSIS bundling built in |
| Interface | TypeScript, React, MUI, custom theme | Mature tabs, tables and accessibility. [Material Web](https://github.com/material-components/material-web/discussions/5642) is in maintenance mode, so it is avoided |
| Text editor | Tiptap (ProseMirror) | Stores content as structured JSON, which export and AI both need. Rust UI toolkits have no equivalent |
| Database access | `sqlx` with PostgreSQL and TLS | Compile-time checked queries and versioned migrations |
| Excel reading | [`calamine`](https://docs.rs/crate/mdkit/0.7.2/source/README.md) | Covers XLSX, XLS, XLSM and more |
| PDF reading | [`pdfium-render`](https://docs.rs/crate/mdkit/0.7.2/source/README.md), with `pdfium.dll` bundled | Page-level text, and page images for scanned files |
| Word and PowerPoint reading | Direct OOXML parsing (`zip`, `quick-xml`), or [`office_oxide`](https://docs.rs/crate/office_oxide/0.1.2/source/README.md) | We need slide and paragraph positions for citations. `office_oxide` also reads .doc and .ppt but is pre-1.0, so Phase 0 tests it |
| PDF export | WebView2 [PrintToPdf](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2printsettings) | Same HTML template as the HTML export. Custom page size for A3 (16.54 x 11.69 inches) |
| DOCX export | `docx-rs` | Native Word tables and heading styles |
| Secrets | `keyring` (Windows Credential Manager) | Database password and AI key never stored in the database or a plain file |
| AI | `reqwest`, one adapter per provider | Anthropic, OpenAI, Google and any OpenAI-compatible URL |
| Links | [`tauri-plugin-deep-link`](https://v2.tauri.app/plugin/deep-linking/) with single-instance | On Windows a link starts a new process with the URL as an argument, so single-instance routes it to the open window |

**Scanned files.** Windows' built-in OCR is not the plan, because Microsoft [supports it only for packaged apps with package identity](https://learn.microsoft.com/uwp/api/windows.media.ocr), and an NSIS install has none. Scanned pages go to the user's AI model if it can read images. Phase 0 also evaluates a bundled local OCR engine for customers who cannot send images out.

**Installer**

| ID | Requirement |
| --- | --- |
| FR-INS-01 | NSIS setup.exe built by Tauri with `installMode: currentUser`, which [needs no administrator rights and installs under %LOCALAPPDATA%](https://v2.tauri.app/distribute/windows-installer/) |
| FR-INS-02 | Start menu shortcut and uninstaller. All registry entries under HKCU, including the `a3sheet://` scheme |
| FR-INS-03 | WebView2 is present on current Windows. The installer embeds the bootstrapper as a fallback for machines without it |
| FR-INS-04 | Installer and exe are code-signed before wide release, to avoid SmartScreen warnings |
| FR-INS-05 | In-app updates through Tauri's updater, which keeps the per-user install free of admin prompts (Phase 4) |

**Database environments**

|  | Development | Cloud (later) |
| --- | --- | --- |
| Server | Local PostgreSQL, managed with pgAdmin | Managed PostgreSQL |
| Connection | Localhost | TLS with `verify-full`, network allow-list or VPN |
| Sign-in | Per-user PostgreSQL logins | The same, so no behaviour changes on move |
| Pooling | None | PgBouncer. Each client holds at most 3 connections, released when idle |
| Schema changes | Run by developer | Run by an admin migration tool. Users have no schema rights |

The app checks the schema version at sign-in and asks for an update if it is out of step. Moving to the cloud is a `pg_dump` and restore plus a new connection profile.

**Boundaries that protect later change.** Three Rust interfaces isolate what is likely to move: `Repository` (direct database now, API server later if D1 changes), `AttachmentStore` (PostgreSQL now, object storage later) and `AiProvider`.

**Refresh.** Clients poll every 60 seconds and on window focus. PostgreSQL LISTEN/NOTIFY is avoided because it does not work through transaction-mode connection pooling.

## 6. Navigation, screens and visual design

The app has four main areas, and each A3 opens as a tab container so metadata, content and evidence sit one click apart.

**App shell.** A left navigation rail holds Dashboard, A3 Sheets, Wall and Settings. The top bar holds a search box that also opens an A3 by number or pasted link, the **New A3** button, and a job indicator for background extraction.

&#91;embedded content: A3 page wireframe, Sheet tab in Quadrant view\]

**A3 page.** The header is always visible: number, title, status chip, a four-segment step strip (A B C D, each open or closed), Copy link, Export, Post warning. Below it sits the tab container.

| Tab | Contents |
| --- | --- |
| Overview (default) | All metadata (section 7), step status and dates, health flags, count of proposals waiting for review |
| Sheet | The four text boxes, with a switch between Quadrant view and Accordion view |
| Attachments | Files, versions, extraction state, and an Add button |
| Early Warnings | Warnings posted from this A3, with their updates and check results |
| History | Who changed what, step closes and reopens, version compare and restore |

In Quadrant view, boxes sit where they will print: A top-left, B bottom-left, C top-right, D bottom-right. An Assistant panel on the right collapses to a slim bar, and shows AI items for the current A3 only.

**Visual design.** The look is Google-inspired: white surfaces, thin grey lines, blue actions, generous spacing and pill-shaped buttons. No Google logos or product names are used.

| Token | Value | Use |
| --- | --- | --- |
| Primary | `#1A73E8` | Main buttons, links, active tab |
| Success | `#1E8E3E` | Closed steps, Not affected |
| Warning | `#F9AB00` | Early warnings, Overdue, Stalled |
| Error | `#D93025` | Conflicts, Affected, failures |
| Text | `#202124`, secondary `#5F6368` | All text |
| Lines and fills | `#DADCE0`, `#F8F9FA` | Borders, page background |
| Type | Roboto, 14 px body, 22 px page title | Throughout |
| Icons | Material Symbols, outlined | Throughout |
| Shape | 8 px cards, pill buttons, chips | Throughout |

Colour always carries status, never step identity, so red never implies that step B is at fault.

**Light mode only.** There is no theme setting. The app forces `color-scheme: light`, regardless of the Windows setting.

## 7. A3 record and step editors

An A3 is one metadata record plus four step records, and only the Overview fields and step text are ever edited by hand.

**Metadata (Overview tab)**

| Field | Filled by | Editable |
| --- | --- | --- |
| A3 number, format `A3-2026-0042` | Automatic, unique, permanent | No |
| Title (up to 90 characters) | AI proposal from files, or typed | Yes |
| Project | Last used | Yes |
| Owner | Current user | Yes, transfer |
| Team members | People picker | Yes |
| Date registered | Automatic | No |
| Problem occurred on | AI proposal or date picker | Yes |
| Target close date | Chips or date picker | Yes |
| Category | Pick-list: Quality, Safety, Delivery, Cost, Process, Other | Yes |
| Severity | Chips: High, Medium, Low | Yes |
| Source | Pick-list: Customer, Internal, Supplier, Audit, Field | Yes |
| Area or line, product or part | Project pick-list with type-ahead. AI proposal matched to it | Yes |
| Status, current step, step close dates, closed date | Derived (section 4) | No |
| Last updated, link | Automatic | No |

**Requirements**

| ID | Requirement |
| --- | --- |
| FR-A3-01 | Create an A3 from files, from blank, or from an Affected follow-up task |
| FR-A3-02 | Step boxes offer bold, italic, bullets, numbering, simple tables and pasted images. Nothing else |
| FR-A3-03 | Autosave two seconds after typing stops |
| FR-A3-04 | Each empty box shows a one-line hint from section 4. The hint disappears on typing |
| FR-A3-05 | Sentences that came from a file carry a small source chip. Clicking opens that page, slide or cell |
| FR-A3-06 | Editing a sourced sentence keeps the chip, marked "edited" |
| FR-A3-07 | Only one person edits a given step at a time. Others see "Being edited by \[name\]", and the lock expires after 2 idle minutes |
| FR-A3-08 | Saves check a version number, so a stale save cannot overwrite newer text |
| FR-A3-09 | Snapshot on every Accept, step close and reopen, and at most every 10 minutes while editing |
| FR-A3-10 | History shows snapshots side by side. The owner can restore one, which creates a new snapshot |
| FR-A3-11 | The Sheet tab shows a fit meter for each quadrant, so overflow is visible before export (section 11) |

## 8. Attachments and import

Every file is stored with its A3 and read locally, and its content reaches a step only through one review screen.

**Supported files**

| Type | Extensions | How text is read | Citation points to |
| --- | --- | --- | --- |
| PDF with text | .pdf | Local | Page |
| Scanned PDF, images | .pdf, .png, .jpg | AI vision if available, marked "read from image" | Page or image |
| Word | .docx | Local | Heading and paragraph |
| Excel | .xlsx, .xlsm, .xls, .csv | Local | Sheet and cell range |
| PowerPoint | .pptx (including notes) | Local | Slide |
| Text | .txt, .md | Local | Line |
| Email | .eml, .msg | Local, Phase 2 extension | Message part |
| Legacy Office | .doc, .ppt | Local if Phase 0 proves reliable. Otherwise stored, with a prompt to save as a modern format | Paragraph or slide |
| Anything else | Any | Stored only | None |

**Attachment requirements**

| ID | Requirement |
| --- | --- |
| FR-ATT-01 | Add by drag and drop, file picker or paste. Up to 50 MB per file, 20 files at once |
| FR-ATT-02 | Each file is stored in the database with its A3 and can be previewed, opened in its default app, or saved anywhere |
| FR-ATT-03 | The list shows name, type, added by, date, the steps it informs, and reading state |
| FR-ATT-04 | Adding a file with the same name but new content creates a new version. Identical content is detected and not stored twice |
| FR-ATT-05 | Deleting needs the owner, is logged, and is reversible for 30 days. Citations to it show "source removed" |
| FR-ATT-06 | Reading runs in the background. The user can keep working, and interrupted jobs resume at next launch |

**Large spreadsheets.** A data sheet is not sent row by row. AI receives column names, row counts, and the first 50 rows of each sheet. The user can pick which sheets count. Figures AI quotes must exist in the cells it read.

**Pictures inside documents.** A picture is read only when its slide or page has little or no text. Charts and photos otherwise stay in the original file, and the user can paste them into a step.

**The review screen**

First import and later imports use the same screen. Proposals are grouped by step on the left, and the source appears on the right with the passage highlighted.

| ID | Requirement |
| --- | --- |
| FR-IMP-01 | Each proposal card shows target step, type (Add, Update or Conflict), proposed text and a source chip |
| FR-IMP-02 | Actions are Accept, Edit and Reject. Accept all is available per step |
| FR-IMP-03 | Nothing is written to a step until accepted |
| FR-IMP-04 | Import keeps the source's wording, with only light condensing. Polishing is a separate, optional action (section 9.4) |
| FR-IMP-05 | Metadata proposals appear as a compact block at the top |
| FR-IMP-06 | A Conflict card shows current and new text side by side, with Keep current, Use new, Keep both |
| FR-IMP-07 | Rejected proposals are remembered and not proposed again from the same source |
| FR-IMP-08 | Unfinished reviews are saved. The A3 header shows "5 to review" |
| FR-IMP-09 | A proposal for a closed step shows "Reopen B to apply" |
| FR-IMP-10 | Without an AI key, extracted text appears in a side pane and the user drags passages into a step. Source chips still apply |

## 9. AI behaviour

AI does a small set of fixed jobs inside one A3, shows little by default, and has every output checked by Rust code before a user sees it.

### 9.1 What AI does and how much it shows

| Job | Runs when | User sees first | Limit |
| --- | --- | --- | --- |
| Import mapping | File added to an empty step | Proposal cards | One card per distinct point |
| New-information check | File added when text exists | "3 updates suggested: B (2), D (1)", then cards | New and conflicting points only |
| Validity review | After an import review ends, on step close, on request | One-line findings | 3 shown, the rest behind "More" |
| Phrasing | User clicks Improve on a step or selection | Inline before and after | 3 per step |
| Early warning draft | User clicks Post warning | Pre-filled card | Field limits in section 10 |
| Condense to fit | Quadrant overflows at export | Inline before and after | Only for the overflowing quadrant |
| Ask | User types or picks a suggested question | Short answer with source chips | 60 words, then "Tell me more" |

AI items never appear as pop-ups. There are no scores, confidence percentages or long explanations. Each item offers **Why?**, which reveals up to two sentences.

### 9.2 Context and memory

The AI's memory belongs to the A3, not to a chat or a user. It is rebuilt from the database on every call, so every user gets the same understanding of the same A3.

| Memory layer | Contents | Sent |
| --- | --- | --- |
| Sheet state | All metadata and the current text of all four steps, with status | Always, in full |
| Fact ledger | Each distinct point: text, step, source location, state (accepted, rejected, superseded) | Always, compact form |
| Decision log | Rejected proposals, dismissed findings, reopen reasons | Always |
| Source index | One-paragraph summary of each attachment | Always |
| Source passages | The passages most relevant to this job, found by PostgreSQL full-text search | Up to the token budget |
| Linked warnings | Warnings posted from this A3 and their check results | Always |
| Ask history | Last 6 exchanges plus a rolling summary | Ask only |

The sheet state is never cut. If passages exceed the model's budget, the least relevant are dropped and the item states "based on 40 of 62 pages".

### 9.3 Validity review

The review works from a fixed list of checks, so it stays focused and predictable. It runs only on the current step and earlier ones, so it never complains that D is empty while the user works on A.

| Code | Check | Step |
| --- | --- | --- |
| V1 | Numbers, dates or facts disagree between steps or with a source | Any |
| V2 | Problem statement contains a cause or a solution | A |
| V3 | No measured gap or no target | A |
| V4 | Root cause has no supporting evidence in the A3 or attachments | B |
| V5 | Stated root cause is a symptom or blames a person | B |
| V6 | Root cause has no matching action | C |
| V7 | Permanent action has no matching root cause | C |
| V8 | Action has no owner or date | C |
| V9 | Result is not measured the same way as the problem in A | D |
| V10 | No standard or document is named as updated | D |

Findings rank by type: contradictions, then broken logic, then missing detail. Each shows one line of up to 20 words, a source chip, **Show me** and **Dismiss**. Dismissed findings stay dismissed unless the related text changes.

The label reads "Checked against this A3's content". AI can test logic and consistency against what is provided. It cannot confirm that a stated fact is true on the shop floor.

### 9.4 Phrasing suggestions

Phrasing rewrites a sentence for clarity and keeps its meaning. It adds no sentences, and each suggestion can be accepted on its own.

| Rule | Detail |
| --- | --- |
| Keep the facts | Every number, unit, date, name and part number stays as written |
| Keep the certainty | "Suspected" cannot become "confirmed". "May" cannot become "will" |
| Add nothing | No new causes, actions, reasons or recommendations |
| Show the change | Struck-out and added words, inline in the box |
| Pick the best few | Up to three per step. If the text is already clear, AI says nothing |

### 9.5 New attachments

When a file arrives, AI compares each point in it to the fact ledger and sorts it into one of four groups.

| Result | Meaning | What the user sees |
| --- | --- | --- |
| New | Not yet in the A3 | Add card, with the suggested step |
| Update | Same item, newer value | Update card, showing old and new |
| Conflict | Disagrees, and neither clearly replaces the other | Conflict card |
| Confirms | Already covered | Nothing, only "Confirms 6 points" on the attachment row |

### 9.6 Guardrails against hallucination

The model is told to stay grounded, and the app then verifies that it did.

&#91;embedded content: AI output checks, from memory to saved text\]

| ID | Guardrail | Enforced by |
| --- | --- | --- |
| G1 | Closed world: use only the supplied A3 context. No outside engineering knowledge or typical causes | Prompt |
| G2 | Structured JSON output only. Invalid output gets one retry, then a visible failure | Rust |
| G3 | Every item cites a source passage or step sentence by ID. Unknown IDs are discarded | Rust |
| G4 | Each cited quote must actually be in that passage, ignoring spacing and case. If not, the item is discarded | Rust |
| G5 | For rewrites and drafts, every number, unit, date, code and proper noun must already exist in the input | Rust |
| G6 | For rewrites and drafts, a second model call answers one question: does this claim anything the input does not? | Model |
| G7 | "Nothing to report" is a valid result. AI is never required to fill a quota | Prompt, schema |
| G8 | Attachment text is marked as data, and instructions inside files are ignored. AI has no tools and can change nothing | Prompt, design |
| G9 | A user accepts every change before it takes effect | App |

Discarded items are logged for quality tracking and not shown. Text read from images is less certain, so its cards say "read from image, check the numbers".

Every AI call records job, model, sources used, outcome and token count. A job that fails shows one plain message with Retry, and never partial or unchecked content.

## 10. Early warning wall

An early warning is a short card, posted from any step of an A3, that tells everyone what happened and what to check on their own project or line.

**Posting.** Clicking **Post warning** opens a card pre-filled by AI from the A3's current content. The user adjusts it, previews it, then posts. Nothing is posted automatically.

| Card field | Filled by | Limit |
| --- | --- | --- |
| Headline | AI | 80 characters |
| What happened | AI | 40 words |
| Who should check | AI suggests areas and products, from the project pick-lists | Chips |
| Quick check | AI, using only checks or conditions stated in the A3 | 2 lines |
| Severity | From the A3, changeable | High, Medium, Low |
| Cause status | Automatic: Cause not confirmed (B open), Root cause confirmed (B closed), Solution verified (D closed) | None |
| Source A3 link, author, time | Automatic | None |

An honest card matters most early on. While step B is open, the AI draft may not state a cause, and the card always shows "Cause not confirmed". If the A3 states no check to run, Quick check stays blank for the author to fill.

**Wall and reader actions**

| ID | Requirement |
| --- | --- |
| FR-EW-01 | The wall lists warnings newest first and is visible to all users. Filters: Unread, Severity, Area, My tasks |
| FR-EW-02 | Each reader marks a card **Read** or **Unread**. Opening a card does not mark it read, because Read means "I have taken note" |
| FR-EW-03 | The author can add an update, such as "Root cause found", to the same card. Readers see it as Unread again, tagged Updated |
| FR-EW-04 | The author can withdraw a card with a reason. It stays visible, labelled Withdrawn |
| FR-EW-05 | Cards move to Archive 90 days after the last update. Search still finds them |
| FR-EW-06 | The source A3's Early Warnings tab lists its cards and their check results |

**Follow-up tasks**

| ID | Requirement |
| --- | --- |
| FR-TSK-01 | A reader clicks **Create follow-up**. AI offers up to three tasks based on the Quick check, each added with one click |
| FR-TSK-02 | Task fields: description, assignee (default me), project or line, due date (chips: Today, +3, +7 days) |
| FR-TSK-03 | Closing a task records one result: Not affected, Affected or Not applicable |
| FR-TSK-04 | Affected offers **Create linked A3**, pre-filled with the warning and linked in both directions |
| FR-TSK-05 | The card shows a roll-up, for example "4 checks: 3 Not affected, 1 Affected" |

**Email draft**

The app prepares the email and the user sends it. Content comes from a fixed template using the card's approved text, so no new AI wording is introduced.

| ID | Requirement |
| --- | --- |
| FR-EML-01 | Subject: `Early warning: [headline] (A3-2026-0042)`. Body: what happened, quick check, cause status, links to the warning and the A3, and the A3 number as plain text |
| FR-EML-02 | **Open in mail app** writes an `.eml` file with the `X-Unsent: 1` header and opens it |
| FR-EML-03 | **Copy email** places subject and formatted body on the clipboard. It is always offered |
| FR-EML-04 | The app never sends email |

Copy email exists because `.eml` drafts are not fully dependable. Users of new Outlook have [reported drafts opening with the body and attachments missing](https://learn.microsoft.com/en-gb/answers/questions/2242426/new-outlook-opening-eml-file-with-x-unsent-1-remov), while classic Outlook works, and no official fix has been posted. Phase 0 tests your actual mail client.

## 11. Export and links

Any A3 exports in two layouts and three formats, using approved content only.

&#91;embedded content: export layouts\]

| Layout | PDF | DOCX | HTML |
| --- | --- | --- | --- |
| 4-quadrant | One A3 landscape page | A3 landscape, 2 x 2 table | 2 x 2 grid in one self-contained file |
| Accordion | A4 portrait, sections A to D fully expanded, with PDF bookmarks | A4 portrait, A to D as Word headings, which Word can collapse | A to D expand and collapse on click, all open by default |

PDF cannot collapse, so its accordion layout is the same vertical order, fully expanded.

| ID | Requirement |
| --- | --- |
| FR-EXP-01 | The export dialog has two choices, layout and format, with last-used defaults and a preview |
| FR-EXP-02 | Every export has a header: title, A3 number, owner, project, date registered, target date, status, step strip, link |
| FR-EXP-03 | Each section shows its step letter, name, Open or Closed, and close date |
| FR-EXP-04 | Optional: attachment list (on by default) and source notes (off by default) |
| FR-EXP-05 | Pending proposals and AI findings are never exported |
| FR-EXP-06 | Footer: export date, snapshot number, "Page x of y" |
| FR-EXP-07 | HTML is one file with inline styles and images, and works without a network |
| FR-EXP-08 | File name: `A3-2026-0042_Short-title_quadrant.pdf` |

**When a quadrant overflows.** Text first scales from 11 pt down to 8 pt, and images shrink to fit. If it still does not fit, a "continued" page is added, because content must never be cut. The dialog warns beforehand and offers Condense to fit (section 9.1), which is subject to the same accept step and guardrails.

**Links**

| ID | Requirement |
| --- | --- |
| FR-LNK-01 | Every A3 has a permanent link, shown on the Overview tab and in the header, and copied with one click |
| FR-LNK-02 | Warnings have their own links, which open the card with its source A3 alongside |
| FR-LNK-03 | Clicking a link focuses the running app and opens the item. If the app is closed, it launches and signs in first |
| FR-LNK-04 | Link parameters are validated against a strict pattern. Anything else is ignored |
| FR-LNK-05 | A person without the app gets the A3 number as plain text, and can ask a colleague for an export |
| FR-LNK-06 | An admin can set a link base address. All new links then use `https://` |

**Link risk in email and chat.** Custom-scheme links work on any machine with the app, but mail and chat tools may not pass them through. Users [report that Outlook removes such links and Teams rejects them as invalid](https://techcommunity.microsoft.com/discussions/microsoftteams/custom-protocol-links-not-supported/2413432). The fix is a small static `https://` page that forwards to the app, and it is planned for the cloud phase (D2). Until then, emails also carry the A3 number, and the app's search box opens it.

## 12. A3 list, dashboard and management view

The dashboard answers "what needs me today", and the management view answers "where do we stand", both from data the app already holds.

**A3 list**

| ID | Requirement |
| --- | --- |
| FR-LST-01 | Columns: number, title, project, owner, step strip, status, health flag, severity, target date, last updated |
| FR-LST-02 | One-click filters: My A3s, Open, Overdue, Stalled, Closed. Further filters by project, owner, category, area and date range |
| FR-LST-03 | Search covers title, step text and attachment text |
| FR-LST-04 | Sorting on any column, 50 rows per page. Clicking a row opens the A3 |

**Dashboard, My Work tab (landing page)**

| Card | Shows | Actions in place |
| --- | --- | --- |
| New early warnings | My unread warnings, newest 5, then "See all" | Mark read, Create follow-up, Email draft, Open A3 |
| My open A3s | Owned or member A3s: step strip, target date, next thing to do, such as "5 to review" or "B is open" | Open |
| My follow-up tasks | Open tasks, soonest due first | Record result |

**Dashboard, Management tab**

This tab has one filter row (project, area, period), and every number clicks through to the matching A3 list.

| Measure | Rule |
| --- | --- |
| Open A3s by current step | Count by earliest open step, A to D |
| Age of open A3s | Days since registered, in bands: 0-30, 31-60, 61-90, over 90 |
| Overdue and Stalled | Lists using the section 4 rules, oldest first |
| Closed in period | Count, and average days from registered to closed |
| Reopened in period | Count, by reason chip |
| Warning reach | For each active warning: percentage marked read, checks open and done, Affected count |

Managers need no extra input from A3 owners. The numbers come only from step states, dates and task results.

## 13. Settings

Settings has two main pages for every user, Projects and AI, and a small Admin page.

**Projects**

| ID | Requirement |
| --- | --- |
| FR-SET-01 | Any user can create a project: name, short code, description. The creator becomes its owner |
| FR-SET-02 | The page lists the projects I own or belong to, and lets me open one or set it as default |
| FR-SET-03 | A project owner adds or removes members with a people picker |
| FR-SET-04 | A project owner maintains its Area and Product pick-lists. AI can propose entries from imported files, for the owner to accept |
| FR-SET-05 | A project owner can turn AI off for that project (D4) and can archive the project |

**AI API**

| ID | Requirement |
| --- | --- |
| FR-AI-01 | Provider list: Anthropic, OpenAI, Google, or Custom (any OpenAI-compatible URL, including company-hosted models) |
| FR-AI-02 | The API key field is masked and stored only in Windows Credential Manager. The key never reaches the database |
| FR-AI-03 | The model list is fetched live from the provider, so new models appear without an app update. Manual entry is the fallback |
| FR-AI-04 | **Test connection** reports three things: key works, structured output works, image reading works |
| FR-AI-05 | A plain notice states that A3 and attachment text is sent to the chosen provider |
| FR-AI-06 | AI settings are per user. Each user's calls use their own key and bill to their account |
| FR-AI-07 | Without a valid key, AI buttons show "Set up AI in Settings". Everything else works |

**Account.** Display name, email, change password, default project, sign out.

**Admin.** Create or deactivate users, reset passwords, company pick-lists (Category, Source), stalled threshold, link base address, database connection profile.

## 14. Data model

All shared data lives in PostgreSQL in 22 tables. A3 status is calculated by a view, so it can never disagree with the steps.

| Table | Key contents | Notes |
| --- | --- | --- |
| `app_user` | Name, email, PostgreSQL role name, is\_admin, active | One row per login |
| `project`, `project_member` | Name, code, owner, ai\_enabled. Membership |  |
| `pick_list_value` | List name, value, project (null means company-wide) |  |
| `a3_sheet` | Number, title, project, owner, all editable metadata, withdrawn fields | No status column |
| `a3_step` | Sheet, letter A-D, content JSON, plain text, state, closed\_at, version | Exactly four rows per sheet |
| `a3_step_snapshot` | Step, content, author, time, reason | Version history |
| `attachment` | Sheet, file name, version, blob hash, type, read state, added by, deleted\_at |  |
| `attachment_blob`, `attachment_chunk` | SHA-256 hash, size. 1 MB `bytea` chunks | De-duplicated, streamed |
| `source_segment` | Attachment, location (page, slide, cell range), text, read-from-image flag, search vector | Citation target |
| `a3_fact` | Sheet, step, text, source segment, state | Fact ledger (section 9.2) |
| `ai_proposal` | Sheet, step, type, proposed text, source, state, decided by | Import and new-information cards |
| `ai_finding` | Sheet, check code, text, source, state | Validity review |
| `ai_run` | Job, model, user, sources, outcome, tokens, discarded count | Audit, no API keys |
| `ai_message` | Sheet, role, text, sources, user | Ask history |
| `early_warning`, `early_warning_update` | Sheet, card fields, cause status at posting, state. Updates |  |
| `warning_read_state` | Warning, user, read\_at, unread flag | One row per reader |
| `follow_up_task` | Warning, description, assignee, target area, due, result, linked A3 |  |
| `audit_event` | Actor, object, action, reason, time | Append-only |
| `schema_meta` | Schema version | Checked at sign-in |

**Rules**

- `v_a3_status` derives status, current step and closed date from `a3_step`. Lists and dashboards read this view.
- Row-level security applies to every content table. All users can read. Writes are limited to project members and the A3 owner, using `session_user` to identify the login.
- Steps store structured JSON for the editor and export, plus plain text for AI and search.
- Early warnings keep their own copy of the approved text, so later A3 edits never change what people were told.
- Numbers come from a per-year sequence, so they stay unique across concurrent users.

## 15. Non-functional requirements and security

These targets assume a cloud database with up to 200 users, and they apply to the release build.

| Area | Requirement |
| --- | --- |
| Install | No administrator prompt at any point. Runs on a standard Windows user account |
| First run | Connection details come from a small profile file supplied by the admin, or are entered once |
| Speed | Open an A3 within 2 seconds. List and dashboard within 2 seconds. Local text reading of a 20-page PDF within 10 seconds |
| AI feedback | A job shows progress within 1 second and never blocks editing |
| Connection loss | A banner appears. Unsaved text is kept in a local recovery file and saved when the connection returns |
| Database security | TLS `verify-full` in the cloud. SCRAM-SHA-256 logins. Row-level security on all content tables |
| Credentials | Database password and AI key only in Windows Credential Manager, and never written to logs |
| Untrusted files | Parsed with size, time and decompression limits. A failing file is marked Unreadable and cannot crash the app |
| Interface security | Strict content security policy. Pasted and imported content is sanitised |
| Logs | Local rotating logs with errors and timings. No A3 or attachment text |
| Audit | Step closes and reopens, accepts, deletions, warnings and sign-ins are recorded |
| Accessibility | Full keyboard use on the review screen. Text contrast at least 4.5:1 |
| Backup | Nightly in the cloud, with 30-day retention and a tested restore before go-live |

## 16. Delivery phases

The build runs in five phases, with AI added only once the core record is reliable.

&#91;embedded content: delivery phases and gates\]

Phase 0 removes the technical unknowns, so effort estimates for Phases 1 to 4 should be made after it finishes.

## 17. Acceptance criteria

The release is accepted when each of these passes on a standard, non-administrator Windows account.

**Install and platform**

- [ ] Setup completes with no administrator prompt, and the app runs from the user's profile folder
- [ ] The app shows light mode with the system set to dark
- [ ] A user without project membership cannot change an A3, even through a direct database connection

**A3 and status**

- [ ] A3 status and current step match the section 4 table for every combination of open and closed steps
- [ ] Step B cannot close while A is open
- [ ] Every A3 opens from its link, with the app both running and closed

**Import**

- [ ] A sample PDF, DOCX, XLSX and PPTX each produce proposals, and each proposal opens its correct source location
- [ ] No step text changes until a proposal is accepted
- [ ] A second file that repeats known points and adds two new ones produces exactly those two Add cards
- [ ] A file with a changed value that contradicts a closed step produces a Conflict with "Reopen to apply"

**AI quality** (on a fixed test set of 30 A3s)

- [ ] Every displayed AI item cites a source that exists and contains the quoted text
- [ ] An injected fake number in a model response is discarded by the G5 check
- [ ] A file containing "ignore your instructions" does not change AI behaviour
- [ ] No phrasing suggestion changes a number, date, name or certainty word
- [ ] A complete, consistent A3 can return zero findings
- [ ] No more than three items show at once before the user asks for more
- [ ] A new session recalls an earlier rejected proposal and does not offer it again

**Early warnings**

- [ ] A warning posted while B is open shows "Cause not confirmed", and its draft names no cause
- [ ] Read state is per user. An update returns the card to Unread
- [ ] An Affected result creates a linked A3 with the warning pre-filled
- [ ] The email draft or Copy email produces subject, body and both links in the company's mail client

**Export**

- [ ] 4-quadrant PDF, DOCX and HTML place A top-left, B bottom-left, C top-right, D bottom-right
- [ ] Accordion exports show A, B, C, D in vertical order, and HTML sections collapse
- [ ] An over-long A3 loses no content on export

**Without AI**

- [ ] With no API key, the user can create an A3, attach and read files, close steps, post a warning and export

## 18. References

Tooling facts were checked on 9 November 2026. Library maturity and mail-client behaviour change quickly, so Phase 0 re-tests them.

- [Tauri: Windows Installer, install modes](https://v2.tauri.app/distribute/windows-installer/)
- [Tauri: configuration reference, NSISInstallerMode](https://v2.tauri.app/reference/config/)
- [Tauri: Deep Linking plugin](https://v2.tauri.app/plugin/deep-linking/)
- [Microsoft: WebView2 print settings](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2printsettings)
- [Microsoft: Windows.Media.Ocr namespace](https://learn.microsoft.com/uwp/api/windows.media.ocr)
- [Microsoft Q&A: new Outlook, EML with X-Unsent](https://learn.microsoft.com/en-gb/answers/questions/2242426/new-outlook-opening-eml-file-with-x-unsent-1-remov)
- [Microsoft Tech Community: custom protocol links](https://techcommunity.microsoft.com/discussions/microsoftteams/custom-protocol-links-not-supported/2413432)
- [Material Web maintenance-mode announcement](https://github.com/material-components/material-web/discussions/5642)
- [mdkit README: survey of Rust document extractors](https://docs.rs/crate/mdkit/0.7.2/source/README.md)
- [office\_oxide README](https://docs.rs/crate/office_oxide/0.1.2/source/README.md)
