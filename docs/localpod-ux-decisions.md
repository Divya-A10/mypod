# LocalPod UX decisions from interface research

## Purpose

This document turns approved observations into product decisions for LocalPod, an independently implemented local-first music player and personal recommendation system. Spotify is a reference for observable interaction patterns only. Do not copy Spotify branding, artwork, audio, proprietary assets, or implementation. No product decisions are confirmed until the user reviews them.

## Decision record

For each proposed improvement, preserve the evidence and uncertainty:

| Field | Value |
| --- | --- |
| Decision ID | `LP-UX-###` |
| Related observation IDs | `SPOT-OBS-###` references |
| Observed pattern | Brief factual description of the interface behavior |
| User benefit | The user need this pattern may serve |
| LocalPod proposal | Adopt, adapt, or reject; explain why it fits LocalPod |
| Confidence and limitations | Evidence quality, unknown behavior, and personalization caveats |
| Product impact | Navigation, library, search, playback, queue, history, discovery, accessibility, or responsive behavior |
| Architecture implications | Data/API/UI needs, if any; distinguish current implementation from future work |
| User decision | Pending / approved / declined |

## Areas to evaluate

- **Navigation and library:** whether LocalPod needs a persistent path among the local library, artists, albums, playlists, and settings.
- **Search:** whether local search should group matches by track, artist, album, and playlist, with clear empty and loading states.
- **Track and collection details:** which local metadata fields are useful and how missing tags should be presented.
- **Persistent player and queue:** whether a persistent player belongs in the future desktop and iPhone-friendly interfaces, and how queue changes should be represented.
- **Playback controls:** accessible labels and visible state for previous/next, shuffle, and repeat. Playback behavior remains a future feature and is not specified by this UX note alone.
- **History and discovery:** which local listening events, if any, the user wants to keep private and local; recommendation behavior remains out of scope until separately designed by the user.
- **Keyboard and responsive behavior:** reachable controls, clear focus, readable layouts, and viewport-specific changes.

## Architecture boundaries

Stage 2's SQLite library indexes local files and metadata. A future LocalPod design may evaluate local playlist membership, a transient queue, playback session state, and optional listening-history records as distinct concepts. Do not infer that Spotify's data model or online behavior should be reproduced.

Keep the existing backend API, SQLite schema, and frontend plans separate until a user-approved product decision requires a concrete change. Do not add a Spotify integration, online provider, tracking, or recommendation implementation based on this investigation. Recommendations remain a user-authored Python feature for a later stage.

## Review checklist

Before implementing a UX proposal:

1. Verify the proposal is supported by one or more observation IDs.
2. Separate observed facts from interpretation and from LocalPod requirements.
3. Check privacy implications for indexed files, history, and account-derived notes.
4. Prefer local, understandable behavior and avoid collecting data that the feature does not need.
5. Ask the user to approve material changes to the product or architecture before implementation.
