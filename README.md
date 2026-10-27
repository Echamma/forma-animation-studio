# Forma — shape animation studio

This is the **desktop branch** (Linux and Windows). The original browser app is preserved on `main`.

**Forma 1.2.0** adds video-editor style clips for stop-motion, frame-accurate MP4 and WebM export, easing curves per keyframe, animated fill colors, multi-select, redo, a media library for large videos, and `.forma` project files. Forma 1.1.0 added an MCP server for Codex and Claude: click **MCP off** in the header to let an assistant edit the live scene. See [MCP setup and tool documentation](mcp/README.md).

## Install

Download from [GitHub Releases](https://github.com/Echamma/forma-animation-studio/releases):

- **Windows:** `Forma-Animation-Studio-Setup-1.2.0-x64.exe` installs Forma, opens `.forma` files, and updates itself from new releases. `Forma-Animation-Studio-1.2.0-x64-portable.exe` runs without installing and is updated by hand. Neither is code-signed yet, so Windows SmartScreen asks you to confirm the first launch.
- **Linux AppImage:** `chmod +x Forma-Animation-Studio-1.2.0-x86_64.AppImage`, then run it. It updates itself. If FUSE is unavailable, add `--appimage-extract-and-run`.
- **Debian/Ubuntu:** `sudo apt install ./Forma-Animation-Studio-1.2.0-amd64.deb`, then launch Forma from the application menu.

## Develop

Install Node.js 22 and run `npm ci`, then `npm start`. The editor source lives in `src/` as ES modules; `npm start`, `npm test` and the build scripts bundle it to `build/renderer.js` with esbuild (`npm run bundle` does only that, for example to open `index.html` in a browser as a preview without autosave or the media library).

- `npm test` runs the unit tests (`tests/unit`, plain Node), then the Electron editor and MCP tests. Headless Linux needs Xvfb: `xvfb-run -a npm test`. `npm run test:unit` runs only the fast unit tests.
- `npm run build` creates the AppImage and `.deb`; `npm run build:win` creates the Windows installer and portable `.exe`.
- GitHub Actions tests every push and pull request on Ubuntu and Windows. Pushing a tag like `v1.2.0` that matches `package.json` builds, smoke-tests and publishes both platforms with SHA-256 checksums and update metadata. Pushing a branch never publishes anything.

## Use

The desktop app autosaves each named project as you work and restores the latest scene when it reopens. Rename the project in the header, use **New project** to start another, and **Projects** to reopen one saved on this device. **Ctrl+Z** undoes and **Ctrl+Shift+Z** or **Ctrl+Y** redoes. The previous session is kept as a separate recovery copy. Everything runs locally, and the interface fonts are bundled, so Forma looks and exports the same offline.

1. **Add layers.** Use **Add** for a rectangle, circle, triangle, diamond, arrow, or text that follows a shape, or **Image, SVG, or video** to import PNG, JPEG, WebP, SVG, MP4, WebM, or Ogg files (or drag them onto the editor). The desktop app copies imported media into its media library once, by content, so even very large videos stay out of memory, undo history, and autosaves.
2. **Edit.** Drag a layer, use its eight resize handles (Shift stretches a corner freely), or edit position, size, rotation, opacity, and fill in the inspector. **Layer angle** or the round handle turns a layer for the whole scene. **Look → Blur** softens any layer. Select an SVG layer and choose **Edit SVG artwork** to recolor parts or edit its markup; markup is sanitized locally.
3. **Select several.** Shift-click layers on the canvas, the timeline, or the Layers list. They move, copy, duplicate and delete together, and **Arrange** aligns them or spaces them evenly. **Ctrl+A** selects every visible layer; **Escape** goes back to one.
4. **Keyframes.** Move the playhead and change a layer to key it there, or use **Add keyframe**. Right-click a timeline diamond to move it to an exact time, delete it, or choose how it eases into the next keyframe: ease in, ease out, ease in and out, linear, hold, or a **custom curve** you shape by dragging two handles. **Animate the fill color** turns fill changes into color keyframes, shown as dots under the layer's row.
5. **Clips and stop-motion.** Every layer has a bar on the timeline showing when it is visible. Drag the bar to move the layer and its animation in time, or drag either end to trim it. Drags snap to the playhead, other clips, and 0.1 s steps; hold Alt to place freely.
   To make a picture appear to move, the way you would in a video editor: give a clip an end time (**Timing → Visible until**), click **Continue after this clip** (Ctrl+Shift+V), and drag the copy to its next position. Repeat for each pose. The copies sit side by side on one timeline row, and **Onion skin** shows the previous pose faintly while you place the next one. Copy a different picture with Ctrl+C and press Ctrl+Shift+V on a clip to put that picture next instead, for example a stick figure drawn in several poses. **S** splits a clip at the playhead, **Ctrl+D** duplicates, and right-clicking a clip shows all of these.
6. **Animate.** Under **Draw a move**, drag a destination arrow or record a mouse path; each move gets its own timeline clip with editable start, length, distance, spin, and direction. **Drop selected item onto screen** makes a layer fall and bounce into place. Add **Rotation** clips on top of any movement, or **Fade out** a layer and bring it back at a set time or when another layer's animation ends. **Place outside the canvas** sets up entrances.
7. **Play** to preview. Video layers play in sync with their own sound, starting when their clip starts.
8. **Export.** **Video** renders every frame at its exact time to MP4 (H.264 with AAC where the computer supports it) or WebM, at 24, 30 or 60 fps and normal or double size. It runs faster than real time, includes sound from video layers, and can be cancelled. **Animated SVG** loops in any browser and embeds its images; video layers appear as their poster frame. **PNG** saves the current frame. **Project file (.forma)** packs the editable project with its media into one file that opens in Forma on any computer.

**Open file** opens a `.forma` file or an editable JSON project as a new local project; installed copies also open `.forma` files from the file manager. If a `default-project.json` exists in Forma's user data folder, **Load default project** opens it.

Under **Scene**, choose 960 × 600 landscape or 1080 × 1920 portrait. Changing the format scales existing layers. The scene can be any length; longer animations and clips extend it automatically, and Forma will not shorten it below existing keyframes or clips. Pick a background from the swatches or the custom color picker.
