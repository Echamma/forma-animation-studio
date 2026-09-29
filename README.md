# Forma — shape animation studio

This is the **Linux desktop branch**. The original browser app is preserved on `main`.

Download the x86-64 AppImage or Debian package from [GitHub Releases](https://github.com/Echamma/forma-animation-studio/releases). For the AppImage, run `chmod +x Forma-Animation-Studio-1.0.0-x86_64.AppImage`, then `./Forma-Animation-Studio-1.0.0-x86_64.AppImage`. If FUSE is unavailable, use `./Forma-Animation-Studio-1.0.0-x86_64.AppImage --appimage-extract-and-run`. For Debian/Ubuntu, install with `sudo apt install ./Forma-Animation-Studio-1.0.0-amd64.deb` and launch Forma from your application menu.

To develop on Linux, install Node.js 22, run `npm ci`, then `npm start`. `npm run build` creates AppImage and `.deb` packages. `npm test` checks editing, animation state, PNG/SVG output, and saving recovery data when closing. Headless test environments need Xvfb: `xvfb-run -a npm test`. GitHub builds and tests this branch on Ubuntu 22.04 and publishes packages with SHA-256 checksums after successful checks.

The desktop app saves the current animation when you close it and restores it on reopening. Save project still exports a separate editable JSON backup. Processing and exports run locally. The optional Google Fonts stylesheet uses system fonts when offline.

Open `index.html` in a modern browser. No installation or build step is required.

1. Add a rectangle, circle, triangle, diamond, or arrow. Arrow layers support the same resizing, rotation, motion paths, keyframes, colors, and exports as other shapes.
2. Select a layer, then drag it or edit its position, size, rotation, opacity, and fill.
3. Use **Image, SVG, or video** to import PNG, JPEG, WebP, SVG, MP4, WebM, or Ogg files. Imported media becomes a normal layer and is embedded in the saved project. Individual media files can be up to 30 MB.
4. Select an SVG layer and choose **Edit SVG artwork** to edit its markup or recolor all visible fills and strokes. SVG markup is sanitized locally before it is added to the canvas.
5. Move the timeline playhead and adjust a shape or media layer to create a keyframe automatically. Use **Add keyframe** to hold its current pose at another time.
6. To animate an entrance, select a keyframe and use **Place outside frame** (Left, Right, Above, or Below). The surrounding pasteboard keeps off-frame layers visible and draggable while editing. Move the playhead and position the layer inside the frame at another keyframe.
7. Under **Draw motion**, choose **Draw destination arrow** and drag on the canvas to send the layer to the arrow tip. Choose **Record mouse path** and hold-drag to make it follow the path you draw. Set the movement duration before drawing.
8. Add a spin in degrees to rotate the layer while it moves. Enable **Rotate to face movement** to align it with the arrow or with each turn in a freehand path. These options can be combined.
9. Press Play to review the animation. Video layers synchronize to the scene timeline. Playback clips everything outside the artboard. Click a timeline diamond to inspect a keyframe. Choose smooth, linear, or hold interpolation per layer.
10. Export an animated SVG, video, or PNG of the current frame. Video export includes live frames from imported video layers. Animated SVG uses a poster frame for video layers because portable SVG viewers do not reliably support embedded video. Keep the browser tab visible while recording video. The format depends on browser support, usually WebM.
11. Save the editable JSON project and use Open project to resume later. Saved projects include imported media, so their file size can be much larger. Save before closing or reloading; there is no automatic persistence.

Choose 960 × 600 landscape or 1080 × 1920 portrait (9:16) under Scene Settings. Changing formats scales existing shape positions and sizes. Scene duration supports 1–30 seconds. Animated SVG samples motion at 60 frames per second and loops indefinitely. Color is constant per shape; position, size, rotation, and opacity are animated.

Choose a background from the quick swatches in the left toolbox or use the adjacent custom color picker. The same color remains available under Scene Settings and is included in every export.

Fonts use Google Fonts when available and system fallbacks otherwise. All animation and export processing runs in the browser.
