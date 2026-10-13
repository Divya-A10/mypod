# LocalPod UI Redesign — Phase 2: Audit and Plan

**Date:** 2026-10-13  
**Scope:** Read-only audit and design plan. No application, configuration, test, or existing documentation files were changed for this plan.

## Audit scope and evidence

This plan is based on source inspection of the current React/Tauri frontend, its FastAPI library endpoints and metadata model, existing tests, package/configuration files, and the public Spotify observation note. No account, CSV, music folder, or SQLite database contents were accessed. No application was launched as part of this audit.

The working tree already contains many modified and untracked files, including the frontend and backend implementation. Preserve those changes when implementing a later phase. The intended new plan filename was checked and did not exist before creation.

The public Spotify note records a logged-out desktop page only. It describes a dark three-region composition, left library/navigation area, top controls, dense horizontal content sections, rectangular song artwork and circular artist artwork. It does **not** verify signed-in library screens, mobile behavior, or a working playback bar/player. Treat those as design references or open questions, not observed facts.

## 1. Current UI architecture and key files

- [`apps/desktop/src/App.tsx`](../apps/desktop/src/App.tsx): one React single-page `App` component with local view state (`Home`, `Search`, `Your Library`, `Liked Songs`) and helper components for track rows, collection cards, status/empty states, and the iPod overlay. There is no routing library or URL-backed navigation. The desktop sidebar and mobile navigation are both rendered from this component.
- [`apps/desktop/src/styles.css`](../apps/desktop/src/styles.css): all application styling, including desktop layout, artwork placeholders, playback bar, iPod shell/wheel, and responsive breakpoints. The file uses highly compressed one-line rule groups, which makes incremental visual changes harder to review.
- [`apps/desktop/src/api.ts`](../apps/desktop/src/api.ts): `Track` response type, `GET /tracks` client and duration formatter. A track contains path, storage type, title, artist, album, duration, format, file size, timestamps, and missing status; it exposes no artwork field or playback operations.
- [`apps/desktop/src/main.tsx`](../apps/desktop/src/main.tsx): React entry point and global stylesheet import.
- [`apps/desktop/package.json`](../apps/desktop/package.json): React 19, Vite 7, TypeScript, and Tauri CLI; scripts include `dev`, `build`, `preview`, and `tauri`. There is no frontend test script or installed component/browser test framework declared here.
- [`apps/desktop/vite.config.ts`](../apps/desktop/vite.config.ts): Vite development server at `127.0.0.1:1420`, with `/api` proxied to FastAPI at `127.0.0.1:8000`.
- [`apps/desktop/src-tauri/tauri.conf.json`](../apps/desktop/src-tauri/tauri.conf.json): desktop shell configuration, initial 1320×860 window with 760×600 minimum, CSP, and `bundle.active: false`. It describes a desktop shell, not an existing iPhone application target.
- [`backend/app/api/library.py`](../backend/app/api/library.py): `GET /tracks`, `GET /tracks/{track_id}`, and explicit `POST /scan`. The frontend currently calls only the track-list endpoint.
- [`backend/app/library/metadata.py`](../backend/app/library/metadata.py): reads title, artist, album, and duration tags. It does not expose embedded album artwork.
- [`tests/test_library.py`](../tests/test_library.py): backend scanner and track API tests, including manual scanning, listing, metadata fallback, path constraints, and symlink behavior. These are not frontend interaction tests.
- [`tests/test_health.py`](../tests/test_health.py), [`tests/test_spotify_preview.py`](../tests/test_spotify_preview.py), and [`tests/test_recommender.py`](../tests/test_recommender.py): backend test areas unrelated to the proposed UI interactions.

### Existing navigation and data flow

`App` fetches the first 100 tracks on mount, supports loading subsequent pages with a button, and computes artists/albums from the loaded track records. Search filters only the currently loaded tracks by title, artist, and album. The library tabs are local UI state; Artists and Albums are derived from loaded tracks, while Playlists and Liked Songs show unsupported/empty states. Navigation changes React state and does not update browser history.

In Vite development, `getTracks` uses `/api/tracks`; Vite strips `/api` and proxies to the local backend. In production it requests `http://127.0.0.1:8000/tracks` directly. The API returns metadata and no playback or artwork resource. The backend scanner remains an explicit separate operation; this UI does not trigger scanning automatically.

## 2. Existing issues and evidence

| Issue | Evidence and impact |
|---|---|
| The iPod reads as a generic silver rounded panel rather than a physical device. | `App.tsx` `IpodPlayer` renders a simple header, rectangular screen, and single circle (`IpodPlayer`, lines 81–129). `styles.css` `.ipod-body` is a broad 440px panel with a smooth gradient and 48px radius; it has no visible casing seam, bezel contour, button/port detail, or distinct body-to-wheel proportions (line 70). The current treatments are clean but lack the layered materials and physical construction cues that make an object recognizable. |
| iPod metadata and progress are placeholders, not the selected track. | `IpodPlayer` hardcodes “Nothing playing,” generic copy, 0:00, —:—, and a progress bar fixed at zero (`App.tsx`, lines 109–116). It receives only `onClose`, not track or playback state (lines 81–82). |
| The wheel looks interactive but does not control playback. | Wheel buttons call `selectPlaybackAction`, which changes `focusedControl` and a notice only (`App.tsx`, lines 98–101, 118–127). Left/right keys cycle a focus string and Enter shows a message; they do not invoke a media engine (lines 87–96). The physical positions are also confusing: previous is at the top, play at the right, next at the bottom, while MENU overlaps the previous position (lines 120–124 and CSS line 70). |
| The playback bar displays a full transport layout while every playback-related control is disabled. | Footer labels the current state “Nothing playing” and “Local playback is not connected”; shuffle, previous, play, next, repeat, seek, favorites, and volume are disabled (`App.tsx`, lines 263–266). Only opening the Now Playing overlay works. Disabled controls must not be presented as operating playback. |
| Artwork is deliberately synthetic and cannot show a real album cover yet. | `Art` generates a hue from the label and draws CSS orbits plus “myPod · artwork unavailable” (`App.tsx`, lines 35–50). `Track` has no artwork property (`api.ts`, lines 1–15), and metadata extraction reads only title, artist, album, and duration (`metadata.py`, lines 24–38). |
| Collection cards are visual category placeholders. | `CollectionCard` draws a glow and generic icon rather than an album image/artist image (`App.tsx`, lines 71–79). Artist and album lists are inferred from tracks, not retrieved as entities with distinct artwork or detail endpoints (`App.tsx`, lines 178–185, 250–253). |
| Playlist and liked-track views are not backed by data. | Library Playlists shows “Playlist support is coming later,” and Liked Songs says no favorites collection exists (`App.tsx`, lines 252–259). Sidebar playlist creation is disabled (line 205). These should remain explicit unavailable states until corresponding APIs exist. |
| Search advertises broader coverage than it implements. | The prompt says tracks, artists, and albums, but the query is applied only to loaded track title, artist, and album strings; it is not a separate artist/album/playlist search, and a result may be absent until more pages load (`App.tsx`, lines 178–185, 239–243). |
| Library API errors have an overly broad explanation. | `loadLibrary` catches every non-abort error and reports that the app “could not reach” the API (`App.tsx`, lines 144–156). This can represent a backend/network failure, a non-OK HTTP response, or a server-side error. The source does not establish which occurred in any particular run. Display the actual actionable state (offline, request failed, or empty library) without exposing internals. |
| Desktop density is constrained by a large hero and card-first Home layout. | Home reserves a 252px minimum-height welcome panel and only two quick links before the “Made from your library” section (`styles.css`, line 44; `App.tsx`, lines 220–237). The public Spotify reference showed denser rows of horizontal sections. |
| Mobile layout is a responsive transformation, but some controls become very small or disappear. | At 760px the desktop sidebar is hidden, four-item bottom navigation appears, transport and row-play controls disappear, and the bottom bar is reduced to metadata plus a player link (`styles.css`, lines 76–80). At 390px further columns disappear (lines 82–84). Bottom nav text is 9px and controls have no explicit minimum target size (line 78). A full-screen player wrapper is present, but its device is still a desktop-style 420px body (`styles.css`, line 79). |
| Responsive behavior does not establish an iPhone product experience. | CSS has 1080px, 760px, and 390px breakpoints (`styles.css`, lines 73–84); Tauri configuration only declares a desktop window (`tauri.conf.json`, lines 12–20). No iPhone build target or mobile-specific app shell is present in the inspected config. |

## 3. Proposed desktop and iPhone layouts

### Desktop

Keep the dark, local-first identity and green/lime accent already in the source; do not copy Spotify logos, exact artwork, or product naming. Rework the composition into a stable three-part shell:

1. **Left rail/library (about 240–280px):** brand at top; Home and Search; a library header with clearly named local collection filters; scrollable album/artist/playlist entries when data exists; a quiet “On this device” status. Keep the rail independently scrollable so the content remains navigable in smaller desktop windows.
2. **Main view:** compact top navigation/search row, then a scrollable content region. Reduce the oversized hero. Use data-driven horizontal sections and compact track rows; give each row a strong title, subdued artist/album metadata, consistent artwork slot, and clear hover/focus action area. Keep an intentional empty state when no library is indexed.
3. **Persistent playback bar:** fixed at the bottom with artwork, title/artist, a central transport group, seek/time row, and secondary playback tools. When no playback engine is connected, show a plainly labeled “Playback unavailable” state and do not render controls that imply they work; the Now Playing entry may still open an informative player/selection surface.

Use a restrained type scale (large view title, 18–22px section titles, 13–15px track titles, smaller but readable secondary labels), high-contrast foregrounds, generous section spacing, and dense but breathable rows. Use horizontal scrolling only for card shelves; keep track lists in a table-like layout on wide screens.

### iPhone

Treat narrow screens as a dedicated composition, not a scaled desktop. Use a compact brand/top row, prominent search entry, vertically stacked content sections with deliberate horizontal shelves, and a persistent bottom navigation for Home, Search, Library, and Player. Preserve safe-area insets. Make rows readable and tappable, avoid tiny metadata, and use a full-screen player route/sheet with a clear close/back action. Keep transport controls visible in the player and remove only genuinely secondary desktop tools. The compact bottom bar should identify the current track when available and provide a clear route into the full player; if playback is unavailable, say so rather than placing inert transport icons there.

The existing `max-width: 760px` breakpoint can remain a starting point, but the implementation should validate actual narrow widths, safe areas, orientation, and text zoom. Set touch targets to at least 44×44 CSS pixels (or provide equivalent padded hit areas) and verify the bottom bar does not obscure scroll content.

## 4. Silver iPod-style player concept and interaction map

### Physical construction

Build a distinct, original myPod device as layered HTML/CSS rather than a bitmap. Preserve the recognizably classic proportions: a tall, softly radiused silver face, narrow polished outer lip, darker inset inner seam, subtle directional metal gradient, top faceplate with small myPod mark, black recessed display, and a large circular click wheel occupying the lower half. Keep reflections understated and use several small tonal edges/shadows so the shell reads as machined metal instead of a plain gradient card. At desktop, center the device in a dimmed but non-obscuring overlay or dedicated Now Playing view. On iPhone, use a full-height player surface with a smaller but prominent device and scroll-safe controls.

The display should be driven by one shared player view model: actual selected/current track title, artist and album, actual local artwork if the application can safely provide it, duration and elapsed time only when supplied by playback. If no selected track exists, show a designed neutral “Choose a track” state. If a track exists but playback is not implemented, identify it as selected/ready and explicitly state playback is unavailable; do not animate time or progress.

For missing artwork, use an intentional myPod placeholder system (consistent colors, simple original geometric motif, optional initials/title treatment), not the current per-label random-looking gradient with the repeated “artwork unavailable” caption. Use the same placeholder component in track rows, cards, bottom bar, and device screen. Real artwork is contingent on a local artwork source and API/UI delivery path; none is present in the inspected track contract.

### Click-wheel zones

Use five separate, visible, semantic buttons with non-overlapping hit areas and accessible names. Place MENU at 12 o’clock, PREVIOUS at 9 o’clock, NEXT at 3 o’clock, PLAY/PAUSE at 6 o’clock, and a raised circular SELECT button in the center. Avoid having MENU overlap another action. Clearly differentiate center-button elevation and wheel ring from the brushed-metal body. On touch, each hit region should be comfortably sized; on keyboard, tab order should follow MENU → PREVIOUS → SELECT → NEXT → PLAY/PAUSE (or a documented equivalent).

| Zone | Defined behavior | When no playback engine exists |
|---|---|---|
| MENU | Return one level in the player’s own menu/navigation stack; at its root, close the player or return to the prior app view. | Fully functional navigation; it must not change track or playback state. |
| PREVIOUS | Ask the shared playback controller to seek to the start or select the previous queue item, following a documented threshold/queue rule. | Disabled with an adjacent “Playback unavailable” explanation. Do not change the selected track to imply a skip occurred. |
| SELECT | Activate the focused player-menu item or open details for the currently selected library track. | Functional for UI selection/details; no play side effect. |
| NEXT | Ask the shared playback controller to advance to the next queue item. | Disabled and explicitly explained. |
| PLAY/PAUSE | Toggle the real player state for the current queue item. | Disabled and explicitly explained; never toggle a pretend playing state. |

The current click wheel is not a circular touch/rotation control. First make its five zones functional. A rotary/touch gesture is optional follow-up: if implemented, constrain it to navigating a real player menu or queue, provide a clear focus indicator, and expose an equivalent keyboard interaction. Do not add gesture complexity until the basic accessible buttons work.

### Overlay and keyboard behavior

Make the desktop overlay a true modal: move focus into it, keep Tab focus inside while open, close with Escape, restore focus to the invoking control, and provide an accessible name/description. On mobile, use an accessible full-screen sheet/page with a persistent close/back control. Honor `prefers-reduced-motion` for opening transitions. Current Escape handling exists, but focus trapping/restoration and a clear dialog focus model are not evident in `IpodPlayer`.

## 5. Playback/API dependencies and honest control availability

### Available now

- Fetch and paginate library track metadata with `GET /tracks`.
- Display title, artist, album, duration, file format, and local/missing metadata already returned by the API.
- Search loaded track metadata locally; refresh the list; derive artist/album groupings from the loaded tracks.
- Navigate between the existing local views and open/close the iPod overlay.
- Show loading, empty, and failed-request states, with clearer error distinctions planned.

### Not available in the inspected architecture

- No audio playback service, audio element, playback state store, queue, seek/volume API, or play/pause/previous/next backend endpoint is present in the inspected frontend/API integration.
- No artwork field or artwork-serving endpoint is present in the track API type. Metadata extraction does not read embedded artwork.
- No playlist or favorites endpoints/models are currently used by this UI.
- Shuffle/repeat modes have no state or backend behavior.

Therefore, transport, seek, volume, shuffle/repeat, favorite, and “play row” controls cannot honestly be enabled until an audio engine and a shared playback contract are designed and implemented. UI work can improve their layout and unavailable-state explanation, but must not simulate successful playback. A future playback integration should expose a single shared state/controller (current track, queue, status, elapsed/duration, volume, repeat/shuffle) consumed by both footer and iPod. Add controls only when their operation has a real defined result. The player can still show a deliberately empty state or a selected track’s static metadata without claiming it is playing.

The generic API error text is not proof of a particular backend failure. The code maps any request failure into one sentence. A later UI pass should distinguish request/network errors from a valid empty library, preserve retry, and avoid claiming the API is unreachable if the server returned an error response.

## 6. Accessibility requirements

- Use semantic landmarks (`header`, labeled navigation, `main`, player dialog/page) and meaningful headings in order.
- Use native buttons/inputs with accessible names. Do not use disabled appearance without a nearby textual reason; unavailable actions should not appear active or announce false capability.
- Maintain visible high-contrast `:focus-visible` states throughout all navigation and click-wheel controls; do not rely on hover.
- Provide keyboard operation for navigation, search, library tabs, player open/close, and all enabled wheel actions. Avoid global key handlers that hijack arrows/Enter while an input or unrelated control has focus.
- Make touch targets at least 44×44 CSS pixels, including wheel zones, close, mobile navigation, and transport buttons. Respect safe-area insets and leave content unobscured by persistent bars.
- Use text contrast that remains readable for secondary metadata; do not encode playing/selected/error states only by color.
- Add `aria-current` or selected-state semantics to active navigation and tabs. Use appropriate list/table semantics for tracks and labels for artwork (decorative placeholders should be hidden from assistive tech; meaningful local artwork should have concise alt text or be associated with the track name).
- Announce loading/error changes and actual playback-state changes without repeatedly announcing elapsed time. Reduce or remove decorative motion under `prefers-reduced-motion`.
- For the modal player, manage focus entry, containment, Escape close, and focus restoration. Ensure the mobile full-screen player has a reachable close control and predictable reading order.

## 7. Incremental implementation sequence

### Step 1 — Extract stable UI boundaries and correct states

Separate the current monolithic view into small components without changing API behavior: app shell/navigation, library views, shared artwork placeholder, playback bar, and player shell. Define explicit loading, error, empty, selected-track, and playback-unavailable states. Do not invent sample library data.

**Acceptance:** Existing views and API loading still work; no scanner or CSV import behavior changes; no control claims to play audio. TypeScript/Vite production build succeeds.

**Checks:** `npm run build` in `apps/desktop`; retain and run the existing backend test suite when implementation is approved. A browser smoke checklist should cover empty, populated (synthetic UI-only response), API-error, and loaded-more states without reading user library/database data.

### Step 2 — Desktop shell and library hierarchy

Refine sidebar, compact top search/navigation, denser section shelves, artist/album cards, track rows, spacing, and persistent bar. Keep the accent and visual language original. Keep unsupported playlist/favorite states honest.

**Acceptance:** At desktop sizes, navigation and library area remain stable; content scrolls independently; track/artist hierarchy is clear; empty and error states are legible; the footer does not cover content.

**Checks:** Production build; manual layout checks at 1320×860 and around 1024px/760px; keyboard-only navigation and focus-visible review. There are no frontend UI tests currently declared.

### Step 3 — Dedicated iPhone composition

Design mobile navigation, search access, compact shelves/rows, safe-area padding, bottom bar, and full-screen player. Increase hit targets and prevent persistent UI overlap. Keep only controls that have a real behavior.

**Acceptance:** At narrow iPhone viewport widths, labels remain readable, page content is not obscured, navigation and player entry remain reachable, and the player can be opened/closed by touch and keyboard. Test rotation and larger text where possible.

**Checks:** Responsive/manual browser QA at representative iPhone CSS widths and safe-area simulation. Tauri configuration currently has desktop window dimensions only, so native iOS packaging remains separate work.

### Step 4 — Physical iPod-style player shell

Replace the flat body with layered faceplate/bezel/body, clear classic wheel geometry, five non-overlapping zones, detailed no-track/selected-track states, and original placeholder artwork. Connect only MENU/SELECT navigation behavior until playback is real.

**Acceptance:** Device reads as a small physical silver player at desktop and on mobile; artwork/metadata match the selected real library record where available; no progress/time is fabricated; all enabled buttons do exactly their documented UI action; modal accessibility works.

**Checks:** Build; visual comparison against the approved iPod design direction, not Spotify assets; keyboard and touch walkthrough; reduced-motion and screen-reader spot checks.

### Step 5 — Playback integration gate (separate approval/design)

Before enabling play, pause, skip, seek, volume, shuffle, or repeat, inspect and agree on a real local audio backend and one shared playback state contract. Add queue behavior, error states, and artwork delivery only after implementation architecture is reviewed. Playlist and favorites remain separate API/model work.

**Acceptance:** Every enabled control changes actual playback or reports a concrete operation failure; footer and device stay synchronized; ended/error/loading states are accurate; no playback claims from sample data.

**Checks:** Unit/integration tests for the playback controller and API boundary, plus an explicitly authorized test audio fixture in an isolated synthetic setup. Do not use personal folders or media for automated tests.

### Testing/tooling note

The frontend package currently has no test script or React/browser test dependencies. For initial work, `npm run build` and a documented manual QA matrix are available. If automated component/browser tests are desired, first choose and approve a testing approach and any dependency changes; do not silently add test packages as part of a UI-only pass. Pure state/formatting logic can be extracted for dependency-free tests if that fits the implementation.

## 8. Risks, open questions, and approval decisions

1. **Playback backend is the main functional blocker.** Confirm whether a local audio engine is already planned outside this inspected UI. Until it exists, transport stays unavailable and the device should display selected metadata without implying playback.
2. **Artwork source is unresolved.** Decide whether to read embedded local artwork, generate/cache thumbnails, or initially use only designed placeholders. Each choice affects backend API, file access, caching, and Tauri CSP/asset delivery. The current API does not expose artwork.
3. **Selected versus currently playing track.** Decide whether selecting a row opens the iPod in a static selected state, and how that differs from the future active queue/current item. Keep the concepts separate in the model.
4. **iPhone target and delivery path.** The current repository has a desktop Tauri shell and no inspected iOS target. Decide whether the first mobile deliverable is responsive web UI or native Tauri iOS, and what device/browser will be used for QA.
5. **Library entities remain partial.** Artists/albums are derived from only loaded tracks. Playlist/favorites APIs are absent. Confirm whether the UI redesign should remain limited to currently supported local-track data until separate model/API work is approved.
6. **Reference coverage is limited.** The observation note is a public logged-out desktop page only. It does not support conclusions about signed-in Spotify, actual playback, queue, responsive mobile, or private collection behavior. Keep the redesign distinctly myPod-branded and use these observations only for broad layout/density cues.
7. **Testing tooling.** Decide whether build plus manual accessibility/responsive QA is sufficient for the first UI increments or whether to approve a frontend test framework separately.

### Decisions requested before implementation

- Approve the visual direction: original myPod dark library UI with the requested silver classic-player-inspired device, using no Spotify marks or artwork.
- Decide whether Phase 3 is visual/interactive selection only while audio remains unavailable, or should wait for an approved audio-engine design.
- Choose responsive web as the first iPhone target, or authorize a separate Tauri iOS architecture phase.
- Confirm the artwork strategy and whether adding an artwork endpoint is in scope for a later backend phase.
- Confirm the initial test strategy without adding dependencies, or separately approve a test framework.

