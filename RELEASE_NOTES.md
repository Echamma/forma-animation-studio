# Forma Animation Studio — Desktop and MCP 1.1.0

Control the live Forma animation editor from Codex, Claude Code, or Claude Desktop through a local MCP server. Twelve tools cover scene creation, shape/text editing, keyframes, linked captions, playback, inline PNG previews, local image/SVG imports, project loading, and PNG/SVG/WebM/JSON exports.

Download the desktop app for your system and `forma-mcp.cjs`. Install Node.js 22 or later, then register the bundled server with your client. Open Forma and click **MCP off** to enable assistant control. Setup commands and Claude/Codex examples are in `mcp/README.md` on the `linux` branch.

Includes x86-64 Linux AppImage and Debian/Ubuntu packages, a Windows portable executable, and a standalone MCP bundle. The bridge is disabled by default and uses an authenticated loopback connection. MCP scene edits save recovery data immediately; closing the desktop app also saves the current animation. Save project exports a separate editable backup.

Verified with a real MCP stdio client, editor tests, all twelve tools, file imports, exports, authentication, validation, recovery, and packaged-app startup on Linux and Windows. The original browser app remains on `main`; desktop/MCP source is on `linux`.
