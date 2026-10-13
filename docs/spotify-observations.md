# Spotify interface investigation

## Scope and status

This document records a future, limited investigation of Spotify's observable interface to inform an independently implemented LocalPod. It does not authorize account access or interaction. No Spotify session has been opened and no observations have been collected yet.

Use only Spotify's ordinary interface and an account the investigator is authorized to use. Prefer a non-personal authorized account. If one is unavailable, ask before using the user's existing account. Do not bypass authentication, DRM, access controls, or anti-bot measures. Do not inspect private APIs, extract credentials or session tokens, capture audio or media responses, download artwork, or inspect a Spotify export or personal music files.

REA remains a separate, optional investigation tool. It is not a LocalPod dependency or runtime integration.

## Investigation guardrails

- Start with passive inspection of the currently visible page only. Do not launch a browser or sign in as part of this document's workflow.
- Do not click playback controls or trigger playback. Do not use previous/next, shuffle, repeat, seek, queue edits, playlist edits, account settings, or other actions that could alter listening history, recommendations, playlists, or settings without first asking the user.
- Treat account-specific history and discovery content as private. Ask before opening or recording those areas, even when they appear read-only.
- Keep evidence to concise notes and appropriately redacted references. Do not save raw REA Evidence bundles or screenshots. If a visual reference is necessary later, ask first and redact account details, track/album artwork, and other personal content.
- Keep any optional network capture disabled unless a specific question requires it and the user approves. Never retain request or response bodies, header values, media URLs, audio, credentials, or session data. Review and redact URL query values before recording a reference.
- Record observation separately from interpretation. Do not infer a behavior from a missing or incomplete capture.

## Safe, repeatable workflow

### Phase 0: prerequisites and approval

1. Confirm the chosen account is authorized and non-personal where possible. If only the user's existing account is available, pause and ask before accessing it.
2. Confirm the user approves the exact pages and actions to examine. Begin with passive inspection only.
3. Confirm the REA runtime prerequisites and a Chrome-family browser are available before running REA. Use an isolated browser profile and a loopback-only debugging endpoint if the user later approves CDP inspection. Do not expose the debugging port to the network.
4. Limit the initial scope to the Spotify web interface. Do not assume the desktop application exposes a supported Electron or native UI interface.

### Phase 1: passive baseline

Record the visible page and its viewport without clicking or entering data. Note the navigation regions and visible library/player controls. If REA is later approved and available, a passive page inspection or visible-viewport capture can provide supplementary evidence; request only the minimum page structure or accessibility information needed. Do not opt in to script sources, storage keys, console text, or network bodies for this UX study.

### Phase 2: approved, low-impact observations

Only after approval, inspect one surface at a time and record before/after notes:

| Area | Questions to record | Account-side effect gate |
| --- | --- | --- |
| Navigation and library | Which sections are visible? How are saved items grouped and reached? | Ask before opening private or account-specific pages. |
| Search | Where is search placed? How are results grouped as tracks, artists, albums, and playlists? | Ask before submitting a query if search activity may be retained by the account. |
| Tracks, artists, albums, playlists | What information and actions are visible on each detail view? | Ask before saving, following, adding, removing, or editing anything. |
| Player and queue | Where are controls and queue entry points? What status labels are visible? | Do not play, pause, seek, skip, reorder, or change queue without explicit approval. |
| Previous/next, shuffle, repeat | What labels, accessible names, and visible selected states are presented? | Do not activate these controls without explicit approval. |
| History and discovery | What surfaces exist and how are they presented? | Ask before opening account-specific history or discovery surfaces. |
| Keyboard and responsive layout | Which controls have accessible names? What is visible at documented viewport sizes? | Ask before keyboard actions that activate controls or otherwise affect account state. |

An approved REA browser scenario may use fixed action IDs and capture selected DOM/accessibility state at specific steps. Prefer accessible-role locators and read-only navigation. Network metadata is optional and should remain disabled unless separately approved. Any playback or state-changing workflow remains paused pending explicit approval.

### Recording template

For each observation, record:

| Field | Value |
| --- | --- |
| Observation ID | `SPOT-OBS-###` |
| Date and time | Local date/time; do not include account identifiers |
| Surface and viewport | Page/section and approximate viewport dimensions |
| Preconditions | Public or authorized account view; relevant visible state |
| Action | `passive` or the exact user-approved action |
| Direct observation | Visible text, layout, accessible label, or control state |
| Evidence reference | Short note reference; no raw capture bundle or media |
| Confidence | High / medium / low, with reason |
| Limitations | Unobserved state, incomplete capture, personalization, or ambiguity |
| Account effects | None known / possible / approved effect and its scope |

Keep evidence references redacted and minimal. Do not copy full URLs containing query values; record a sanitized origin and path only when needed.
