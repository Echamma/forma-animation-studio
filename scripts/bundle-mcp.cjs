const esbuild = require('esbuild');
const path = require('node:path');
esbuild.buildSync({entryPoints: [path.join(__dirname, '..', 'mcp', 'server.cjs')],
  bundle: true, platform: 'node', target: 'node22', format: 'cjs',
  outfile: path.join(__dirname, '..', 'dist', 'forma-mcp.cjs'), minify: true});
console.log('Built dist/forma-mcp.cjs (requires Node.js 22 or later)');
