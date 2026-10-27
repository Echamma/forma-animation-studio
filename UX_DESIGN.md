# Forma workspace design

## Goal

Make the editor understandable to someone creating their first short animation while keeping detailed controls available for repeat use. This redesign is based on the existing interface and the request for a more human friendly app; it is a design hypothesis, not a claim of user research.

## Main journey

1. **Create:** use Add above the canvas to open the shape and media drawer.
2. **Edit:** choose artwork on the canvas or open Layers, then change position, size, look, or keyframes in the inspector.
3. **Animate:** open the Animate tab for drawn movement, rotation, and disappearance.
4. **Preview:** move the playhead or press Play in the timeline.
5. **Export:** choose SVG, video, or a PNG frame from the header.

The quick guide gives this sequence in the app. The canvas and timeline occupy the main workspace. Add and Layers open a temporary drawer so the artwork has room when those tools are not in use. Each inspector tab has one job: Edit the selected layer, Animate the selected layer, or set up the entire Scene. Selecting a motion or rotation clip opens Animate automatically.

## Design system

The tokens in `style.css` define a warm, light workspace around the artwork. The dark green header distinguishes project actions from editing controls. The canvas keeps its own neutral pasteboard so artwork remains the focus.

| Role | Token | Value |
| --- | --- | --- |
| App background | `--bg` | `#f2f4ef` |
| Panel surface | `--panel` | `#fffefa` |
| Primary text | `--text` | `#1f2b25` |
| Supporting text | `--muted` | `#58675d` |
| Main action | `--accent` | `#286244` |
| Main action hover | `--accent-strong` | `#174c32` |
| Highlight | `--lime` | `#d8f4a5` |
| Borders | `--line` | `#dce3da` |

Buttons and fields use 40 pixel minimum heights, visible focus outlines, and text labels. Timeline colors distinguish moves (green), rotation (purple), and disappearance (red). At narrower widths, the canvas and timeline stay together, the inspector moves below, and the drawer overlays the workspace when opened.

## Usability validation to run with people

These tasks have not been tested with participants yet. Observe whether first time users can complete each without guidance:

1. Add an image and change its size.
2. Move it between two points in time and preview the result.
3. Fade it out, then make it reappear after another layer's animation.
4. Find the canvas format and export a video.

Record completion, time spent, wrong turns, and the words participants use for the controls. Revise labels or ordering where users hesitate.
