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
- The editor's main-process side (autosave, projects, media library, MCP bridge) lives in `app-host.cjs`. `main.cjs` creates it for the standalone app, and Canopy creates it to show Forma in one of its panes, with the space's own data folder. Inside Canopy, files sent from the space arrive as layers (`src/canopy.js`) and the sidebar opens the space's projects.

## Use

Forma opens on a start screen. Choose **Animate shapes** (an example scene to play with), **Stop-motion from pictures** (pick a few drawings and Forma lines them up as poses), or **Start from an empty canvas**, in landscape or portrait. **Your projects** lists everything saved on this device, and **Open a file** opens a `.forma` file or an editable JSON project. The arrow next to the project name in the header brings the start screen back at any time.

The desktop app autosaves each project as you work and restores the latest scene when it reopens. Rename the project in the header. **Ctrl+Z** undoes and **Ctrl+Shift+Z** or **Ctrl+Y** redoes. The previous session is kept as a separate recovery copy. Everything runs locally, and the interface fonts are bundled, so Forma looks and exports the same offline.

The editor has four areas: tools on the left, the canvas in the middle, the timeline under it, and the inspector on the right.

1. **Add layers.** **Shape** opens a drawer with a rectangle, circle, triangle, diamond, arrow, or text that follows a shape. **Text** adds words (attached to the selected shape, or on their own when nothing is selected). **Picture** imports PNG, JPEG, WebP, SVG, MP4, WebM, or Ogg files; you can also drag files onto the editor. The desktop app copies imported media into its media library once, by content, so even very large videos stay out of memory, undo history, and autosaves. **Layers** lists everything in the scene.
2. **Edit.** Drag a layer, use its eight resize handles (Shift stretches a corner freely), or use the **Layer** tab: **Look** sets the fill, opacity and blur, and **Position & size** opens exact numbers. **Layer angle** or the round handle turns a layer for the whole scene. Select an SVG layer and choose **Edit SVG artwork** to recolor parts or edit its markup; markup is sanitized locally.
3. **Select several.** Shift-click layers on the canvas, the timeline, or the Layers list. They move, copy, duplicate and delete together, and **Arrange** aligns them or spaces them evenly. **Ctrl+A** selects every visible layer; **Escape** goes back to one.
4. **Keyframes.** Move the playhead and change a layer to key it there, or use **Add keyframe**. With the playhead on a keyframe, **Easing** in the Layer tab chooses how it moves into the next one: Smooth, Steady, Hold, Speed up, Slow down, or **Custom…**, a curve you shape by dragging two handles. Right-clicking a timeline diamond offers the same, plus moving it to an exact time or deleting it. The **Animate** switch next to **Fill** turns fill changes into color keyframes, shown as dots under the layer's row.
5. **Clips and stop-motion.** Every layer has a bar on the timeline showing when it is visible. Drag the bar to move the layer and its animation in time, or drag either end to trim it. Drags snap to the playhead, other clips, and 0.1 s steps; hold Alt to place freely.
   To make a picture appear to move, the way you would in a video editor: set when it stops showing (**When it's visible → Until**), click **Next pose after this clip** (Ctrl+Shift+V), and drag the copy to its next position. Repeat for each pose. The copies sit side by side on one timeline row, and **Onion skin** shows the previous pose faintly while you place the next one. Copy a different picture with Ctrl+C and press Ctrl+Shift+V on a clip to put that picture next instead, for example a stick figure drawn in several poses. **Stop-motion from pictures** on the start screen does all of this in one step for a set of drawings. **S** splits a clip at the playhead, **Ctrl+D** duplicates, and right-clicking a clip shows all of these.
6. **Animate.** The bar under the canvas has one-click **Move**, **Spin**, **Fade** and **Drop in** for the selected layer; the **Animate** tab has their settings. **Draw move** (or Move) lets you drag a destination arrow or record a mouse path; each move gets its own timeline clip with editable start, length, distance, spin, and direction. **Drop in** makes a layer fall and bounce into place. **Spin** adds rotation clips on top of any movement, **Fade out** hides a layer and can bring it back at a set time or when another layer's animation ends, and **Enter from outside** sets up entrances.
7. **Play** to preview. Video layers play in sync with their own sound, starting when their clip starts.
8. **Export.** **Video** renders every frame at its exact time to MP4 (H.264 with AAC where the computer supports it) or WebM, at 24, 30 or 60 fps and normal or large size. It runs faster than real time, includes sound from video layers, and can be stopped. **Animated SVG** loops in any browser and embeds its images; video layers appear as their poster frame. **Picture** saves the current frame as a PNG. **Project file (.forma)** packs the editable project with its media into one file that opens in Forma on any computer.

Installed copies also open `.forma` files from the file manager. If a `default-project.json` exists in Forma's user data folder, **Load default project** in the Layers drawer opens it.

**Scene** in the tool rail (or the format button above the canvas) chooses 960 × 600 landscape or 1080 × 1920 portrait. Changing the format scales existing layers. The scene can be any length; longer animations and clips extend it automatically, and Forma will not shorten it below existing keyframes or clips. Pick a background from the swatches or the custom color picker.
