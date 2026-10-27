# Forma MCP

Forma MCP gives Codex, Claude Code, and Claude Desktop typed tools for the **live desktop editor**. Changes appear on the canvas. The original browser-only app on `main` cannot accept MCP connections.

## Install

1. Download Forma **1.1.0 or later** and `forma-mcp.cjs` from [the release](https://github.com/Echamma/forma-animation-studio/releases/tag/linux-v1.1.0). Windows uses the portable `.exe`; Linux uses the AppImage or Debian package.
2. Install Node.js 22 or later. Save `forma-mcp.cjs` in a permanent folder. The file is bundled and needs no `npm install`.
3. Open Forma and click **MCP off** in the header. It becomes **MCP on**. This allows local assistant control until you turn it off or close the app. You can also start the desktop executable with `--mcp`.
4. Register the server using its **absolute path**:

```sh
codex mcp add forma -- node "/absolute/path/to/forma-mcp.cjs"
claude mcp add --transport stdio --scope user forma -- node "/absolute/path/to/forma-mcp.cjs"
```

For Windows, for example:

```powershell
codex mcp add forma -- node "C:\Tools\Forma\forma-mcp.cjs"
claude mcp add --transport stdio --scope user forma -- node "C:\Tools\Forma\forma-mcp.cjs"
```

Restart Codex after changing its configuration. In Claude Code, inspect `/mcp` to check the connection. The server can start while Forma is closed; editing tools then explain how to open the app and enable control. This is a local integration; the web chat versions of Claude and ChatGPT do not connect to this stdio process.

Claude Desktop accepts this entry in its MCP configuration. Replace the path with your downloaded file; merge the entry with any existing servers:

```json
{
  "mcpServers": {
    "forma": {
      "command": "node",
      "args": ["C:/Tools/Forma/forma-mcp.cjs"]
    }
  }
}
```

Use a full path to your `node` executable if the client cannot find it. The [Claude Code MCP documentation](https://code.claude.com/docs/en/mcp) describes registration and configuration scopes. The implementation uses the official [MCP SDK's local stdio transport](https://ts.sdk.modelcontextprotocol.io/server).

## What the assistant can do

| Tool | Behavior |
| --- | --- |
| `forma_get_scene` | Inspect dimensions, layers, keyframes, selection, playback, and current transforms. Asset payloads are omitted by default. |
| `forma_create_scene` | Start an empty project; `replace=true` confirms replacing the current scene. |
| `forma_update_scene` | Edit dimensions, duration, or background; dimensions do not scale existing layers. |
| `forma_add_shape` | Add circles, rectangles, triangles, diamonds, arrows, and text. |
| `forma_update_shape` | Set name, color, interpolation, text, or a text layer's follow target. |
| `forma_set_keyframes` | Merge or replace complete poses; supports position, dimensions, rotation, and opacity. |
| `forma_delete_shape` | Delete a layer with `confirm=true`; attached text is detached at its current pose. |
| `forma_set_playback` | Play, pause, or seek. |
| `forma_preview` | Return an inline PNG frame for the assistant to inspect. |
| `forma_export` | Save PNG, static SVG, animated SVG, WebM, or an editable JSON project. |
| `forma_open_project` | Open a local Forma JSON project with `replace=true`. |
| `forma_import_asset` | Import local PNG, JPEG, WebP, and sanitized SVG artwork. |

Try this prompt:

> Use Forma to inspect my current scene and save a JSON backup. Make a 4-second animation with a coral circle moving left to right on a dark background. Add a caption that follows it. Show me previews at 0, 2, and 4 seconds, then export the animated SVG and WebM.

Coordinates describe each layer's **center**, in pixels from the top left. Width/height are pixels, rotation is degrees, opacity is 0–100, and keyframe time is seconds. Canvas dimensions support 100–4000 pixels and scene durations of 1 second or longer. Limits are 100 layers and 1000 keyframes per layer. For example:

```json
{
  "id": "layer-id-from-forma_add_shape",
  "keyframes": [
    {"t": 0, "x": 100, "y": 300, "w": 160, "h": 160, "r": 0, "o": 100},
    {"t": 4, "x": 860, "y": 300, "w": 160, "h": 160, "r": 360, "o": 100}
  ]
}
```

Text linked to another layer follows its transform; its offset is derived from its current pose when linked. Use an empty `linkId` to detach it at its current pose. Editing keyframes clears that layer's drawn-motion metadata so it cannot later overwrite those keyframes. Video assets can be imported through the desktop UI and existing video layers can be animated through MCP.

## Files and connection

Exports go to **Documents/Forma Exports** by default; each export returns its absolute path and byte count. Only simple filenames with the correct extension are accepted. Existing exports require `overwrite=true`. Set `FORMA_EXPORT_DIR` in the **desktop app's** environment to choose another export folder. Do not set it only on the MCP server.

Forma does not impose a file size cap on project or image imports; very large files still depend on available device memory and disk space. Files to import must use absolute local paths. PNG previews are capped at 1600 pixels per dimension; PNG exports use full scene dimensions. WebM records one complete loop in real time and keeps the app open until completion. Animated SVG uses poster frames for video layers, matching the editor's existing export behavior. Audio is not exported.

The bridge is disabled by default, listens only on `127.0.0.1` at a random port, and requires a random local bearer token. Browser origins are rejected. There is no arbitrary JavaScript execution or remote URL import tool. Requests are serialized and validated against the contract before reaching the editor. Turning MCP off blocks new/queued commands; a command already running may finish. Closing Forma removes its discovery file. Scene edits are autosaved locally while you work.

The token is discovered automatically from:

- Windows: `%APPDATA%/forma-animation-studio/mcp-bridge.json`
- Linux: `$XDG_CONFIG_HOME/forma-animation-studio/mcp-bridge.json`, or `~/.config/forma-animation-studio/mcp-bridge.json`
- macOS (source development): `~/Library/Application Support/forma-animation-studio/mcp-bridge.json`

On Unix the discovery file is owner-readable/writable only. Windows relies on the user's application-data directory permissions. For a custom profile, set `FORMA_BRIDGE_FILE` to the same absolute path in **both** the app and MCP server environment. No token belongs in the client configuration.

## Develop and verify

```sh
git clone --branch linux https://github.com/Echamma/forma-animation-studio.git
cd forma-animation-studio
npm ci
npm run start:mcp
```

You can register `/absolute/path/to/mcp/server.cjs` instead of the bundled entry point for development. `npm run mcp:build` bundles the distributable file in `dist/forma-mcp.cjs`. `npm test` builds that bundle, checks the generated contract, tests the editor, and runs a real stdio MCP client against all twelve tools and the bundle. On headless Linux, use `xvfb-run -a npm test`.

`openapi.json` is the contract source. `npm run mcp:contract` resolves schemas and generates `tool_manifest.json`. Both the stdio server and the desktop bridge validate arguments against that manifest. Tests reject contract drift. Tool names remain stable; breaking changes should use new tool IDs.

Errors use a stable `code` and `message` inside MCP tool results with `isError=true`. Invalid scenes are rejected before replacing the current project. If a timeout or recovery-write failure occurs after an edit, inspect the scene before retrying the command. Client cancellation does not roll back an edit already sent to the app.
