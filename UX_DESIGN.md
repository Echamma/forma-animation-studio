# Forma workspace design

## Goal

Make the editor understandable to someone creating their first short animation while keeping detailed controls available for repeat use. The 1.2 redesign builds on the earlier workspace and moves it onto the Canopy design system: a start screen that asks what you want to make, tools you can see without opening a menu, and one-click animations next to the artwork. It is a design hypothesis, not a claim of user research.

## Main journey

1. **Start:** the start screen offers three ways in: an example to animate, stop-motion from a set of pictures, or an empty canvas, in landscape or portrait. Saved projects are listed below as cards.
2. **Create:** the tool rail on the left adds a Shape (from a small drawer), Text, or a Picture. Layers opens the same drawer on the list of layers.
3. **Edit:** choose artwork on the canvas, then use the Layer tab. Look and Easing sit at the top; exact position and size are folded away under **Position & size**.
4. **Animate:** the bar under the selected layer offers Move, Spin, Fade, and Drop in. The Animate tab holds their settings.
5. **Preview:** move the playhead or press Play in the timeline.
6. **Export:** Export in the header opens a dialog that asks what to save (video, animated SVG, picture, or project file) and summarizes the result before you save.

The quick guide gives this sequence in the app. The canvas and timeline take up the main workspace, and the Add drawer closes once you pick something. Each inspector tab has one job: Layer edits the selected layer, Animate animates it, and Scene sets up the whole canvas. Selecting a motion or rotation clip opens Animate automatically. The inspector header shows the selected layer's color or picture and where the next change will be keyed.

Labels describe results rather than mechanics: easing reads Smooth, Steady, Hold, Speed up, and Slow down; the clip copy button reads **Next pose after this clip**; video size reads Normal and Large.

## Design system

The tokens in `style.css` follow Canopy: a warm, light workspace around the artwork, with a deep green header that separates project actions from editing. Nunito is used for headings and Nunito Sans for everything else, both bundled. The canvas keeps its own dotted pasteboard so the artwork stays the focus.

| Role | Token | Value |
| --- | --- | --- |
| App background | `--surface-0` | `#F4F1E8` |
| Panel surface | `--surface-1` | `#FFFFFF` |
| Sunken surface | `--surface-2` | `#ECE8DB` |
| Primary text | `--ink` | `#1C2A21` |
| Supporting text | `--ink-muted` | `#56635A` |
| Main action | `--brand` | `#2F6B45` |
| Selected and highlight | `--brand-soft` | `#E3EED0` |
| Header | `--canopy-deep` | `#1F3D2B` |
| Forma accent, playhead | `--app-clay` | `#B85A2E` |
| Borders | `--border` | `#DDD8C8` |

Radii are 8, 12, and 20 pixels, plus fully rounded pills for toggles and the quick-animate bar. Buttons and fields keep 40 pixel minimum heights (32 for compact ones), visible focus states, and text labels. Timeline clips use green for moves, sky blue for spins, and sun yellow for fades. Under 1180 pixels the inspector narrows; under 900 pixels the inspector moves below the canvas and the drawer overlays the workspace.

## Usability validation to run with people

These tasks have not been tested with participants yet. Observe whether first-time users can complete each without guidance:

1. Start a stop-motion project from three pictures and play it.
2. Add an image and change its size.
3. Move it between two points in time and make it slow down at the end.
4. Fade it out, then make it reappear after another layer's animation.
5. Find the canvas format and export a video.

Record completion, time spent, wrong turns, and the words participants use for the controls. Revise labels or ordering where users hesitate.
