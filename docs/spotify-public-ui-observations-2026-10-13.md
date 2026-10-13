# Spotify public web UI observations — 2026-10-13

## Scope and access

- Observation was limited to `https://open.spotify.com/` in a newly launched, isolated Chrome profile at `/private/tmp/mypod-rea-spotify-public-profile-2026-10-13`.
- No account was used, no saved credentials or existing Chrome profile were accessed, and no sign-in, playback, save, follow, or other page controls were activated.
- The page rendered without a blocking login or consent gate. Persistent **Log in** and **Sign up** prompts were visible; the page remained viewable without using them.
- No signed-in Spotify screens or private account data were inspected.

## REA observations

- Browser: Chrome `155.0.8059.40`.
- REA target listing used `list-browser-targets` against `http://127.0.0.1:9222` with the exact-origin filter `https://open.spotify.com`. One page target matched: title `Spotify - Web Player: Music for everyone`, URL `https://open.spotify.com/`, target ID `C42AC13468FB0D76D090C2B669F949C9`. Attachment state was unknown. Other non-page targets were excluded and not inspected.
- `inspect-web-page` was run for that target with the exact-origin scope and `--include-accessibility-text`; the returned view was filtered to the target and accessibility tree. REA reported 1,451 accessibility nodes. The tree included a `Main` navigation landmark, a `Home` button, a `Skip to main content` link, and a complementary landmark named `Now playing bar`.
- REA's documented default page inspection also observes page-scoped DOM, accessibility, script/resource metadata, safe response metadata, attach-window network and console metadata, WebSocket frame sizes, workers, and quota. The CLI output filter narrowed the returned evidence; it did not disable those default observations. No optional storage keys/fingerprints, script source, console text, or body-shape capture was selected.
- The tree included play-labeled buttons on content cards. These were observed as labels only; none were activated. The visible screenshot did not show active transport controls.
- `capture-web-screenshot` captured the visible viewport at 2400 × 1638 pixels. The image was reviewed in memory and was not saved as a file. No response bodies, script source, storage keys, storage fingerprints, or console text were requested. No cookies or credentials were collected.
- The screenshot capture did not scroll or navigate. No separate network analysis was performed, and no request or response bodies were collected.

## Directly observed layout

- The page uses a dark, three-region composition: a compact top navigation bar, a left library/sidebar column, and a larger content area.
- The top bar places home and search affordances at the left, with product links, app-install and account-entry actions toward the right.
- The sidebar is labeled **Your Library** and shows prompts to create a playlist and browse podcasts, followed by language and footer links.
- The main content is organized into titled horizontal sections. The visible sections were **Trending songs** and **Popular artists**, each with a **Show all** affordance. Song cards use rectangular cover art; artist cards use circular portraits. A horizontal continuation control is visible at the edge of the artist row.
- Text is a clean sans-serif hierarchy: bright, heavier section headings; smaller white card titles; and subdued gray supporting text. The surfaces are black or charcoal, while artwork supplies most of the color. A persistent purple-to-blue sign-up banner spans the bottom of the viewport.
- The layout is relatively dense: multiple song cards and artist cards appear in one horizontal row, with imagery carrying much of each card’s visual weight.

## Interpretation for LocalPod (not a confirmed product decision)

- The page suggests testing a clear split between persistent navigation and a larger, section-based browsing area, with distinct artwork shapes for album/song and artist cards.
- LocalPod could adapt the density and heading hierarchy while keeping its own branding, using actual local artwork when available and clear placeholders when it is not.
- This logged-out page does not establish the design or behavior of Spotify’s signed-in player. The visible sign-up banner occupies the bottom edge, while the accessibility tree separately identifies a `Now playing bar`; no active playback controls were visible. Do not use this capture to infer queue, transport, or playback behavior.

## Limitations and confidence

- These notes describe one public desktop page at one large viewport. The page’s rendered viewport was 2400 × 1638; the layout at ordinary laptop widths, on mobile, or after resizing was not observed.
- No navigation links, search, card actions, keyboard controls, or player controls were activated. Focus order, responsive transitions, sticky behavior while scrolling, and interaction outcomes remain unknown.
- Editorial content may vary by locale, time, or browser state. Individual track and artist names are intentionally omitted from these notes.
- Colors, spacing, and typography are visual estimates from the screenshot, not measured CSS values. No Spotify implementation or private interface was inspected.
