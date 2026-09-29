#!/usr/bin/env node
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const {Server} = require('@modelcontextprotocol/sdk/server/index.js');
const {StdioServerTransport} = require('@modelcontextprotocol/sdk/server/stdio.js');
const {ListToolsRequestSchema, CallToolRequestSchema} = require('@modelcontextprotocol/sdk/types.js');
const {manifest, validate, fail} = require('./validation.cjs');

function discoveryPath() {
  if (process.env.FORMA_BRIDGE_FILE) return path.resolve(process.env.FORMA_BRIDGE_FILE);
  const base = process.platform === 'win32' ? process.env.APPDATA || path.join(os.homedir(), 'AppData', 'Roaming')
    : process.platform === 'darwin' ? path.join(os.homedir(), 'Library', 'Application Support')
    : process.env.XDG_CONFIG_HOME || path.join(os.homedir(), '.config');
  return path.join(base, 'forma-animation-studio', 'mcp-bridge.json');
}
async function callBridge(name, args) {
  validate(name, args);
  let discovery;
  try { discovery = JSON.parse(await fs.readFile(discoveryPath(), 'utf8')); }
  catch { throw fail('APP_UNAVAILABLE', 'Open the Forma desktop app and click MCP off to enable control, or launch it with --mcp.'); }
  if (discovery.version !== 1 || !Number.isInteger(discovery.port) || discovery.port < 1 || discovery.port > 65535 || !/^[a-f0-9]{64}$/.test(discovery.token)) throw fail('INVALID_DISCOVERY', 'The Forma bridge file is invalid; toggle MCP off and on in Forma.');
  let response;
  try {
    response = await fetch('http://127.0.0.1:' + discovery.port + '/tools/' + name, {
      method: 'POST', headers: {'Content-Type': 'application/json', Authorization: 'Bearer ' + discovery.token},
      body: JSON.stringify(args), signal: AbortSignal.timeout(90000), redirect: 'error'
    });
  } catch { throw fail('APP_UNAVAILABLE', 'Forma is unavailable or the tool timed out. Check the desktop app and inspect the scene before retrying an edit.'); }
  const body = await response.json();
  if (!response.ok || body.error) throw fail(body.error?.code || 'BRIDGE_ERROR', body.error?.message || 'Forma rejected the tool.');
  return body.result;
}
const server = new Server({name: 'forma', version: manifest.version}, {
  capabilities: {tools: {}},
  instructions: 'Control the open Forma desktop editor. Start with forma_get_scene. Coordinates are layer centers in pixels. Back up important scenes using forma_export(format=project) before replacement. Preview frames to assess results. Exports go to the desktop app export folder. Control must be enabled in Forma. Tool errors do not mean an edit is safe to retry without inspecting the current scene.'
});
server.setRequestHandler(ListToolsRequestSchema, async () => ({tools: manifest.tools.map(({name, description, inputSchema, annotations}) => ({name, description, inputSchema, annotations}))}));
server.setRequestHandler(CallToolRequestSchema, async request => {
  try {
    const result = await callBridge(request.params.name, request.params.arguments || {});
    if (request.params.name === 'forma_preview') {
      const {data, ...metadata} = result;
      return {content: [{type: 'text', text: JSON.stringify(metadata)}, {type: 'image', data, mimeType: 'image/png'}], structuredContent: metadata};
    }
    return {content: [{type: 'text', text: JSON.stringify(result)}], structuredContent: result};
  } catch (error) {
    return {isError: true, content: [{type: 'text', text: JSON.stringify({code: error.code || 'TOOL_ERROR', message: error.message})}]};
  }
});
server.connect(new StdioServerTransport()).catch(error => {console.error('Forma MCP:', error.message); process.exitCode = 1;});
