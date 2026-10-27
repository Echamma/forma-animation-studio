const esbuild = require('esbuild');
const path = require('node:path');
esbuild.buildSync({entryPoints: [path.join(__dirname, '..', 'src', 'main.js')],
  bundle: true, platform: 'browser', target: 'chrome120', format: 'iife', sourcemap: 'linked',
  outfile: path.join(__dirname, '..', 'build', 'renderer.js')});
console.log('Built build/renderer.js');
