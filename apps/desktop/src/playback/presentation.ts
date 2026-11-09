/** Shared visual state for every play/pause control. The caller supplies the
 * controller's confirmed playing state; loading, paused, ended, and error all
 * continue to present Play. */
export function playPausePresentation(isPlaying: boolean): { icon: "play" | "pause"; label: "Play" | "Pause" } {
  return isPlaying ? { icon: "pause", label: "Pause" } : { icon: "play", label: "Play" };
}
