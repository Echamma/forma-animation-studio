const { app, BrowserWindow } = require('electron');
const fs = require('node:fs/promises');
const path = require('node:path');
const assert = require('node:assert/strict');
const output = path.join(__dirname, '..', 'test-output');
app.setPath('userData', path.join(output, 'profile-' + process.pid));
process.env.FORMA_TEST = '1';
require('../main.cjs');
const timeout = setTimeout(() => { console.error('Forma desktop checks timed out'); app.exit(1); }, 30000);
app.whenReady().then(async () => {
  try {
    app.removeAllListeners('window-all-closed');
    await fs.mkdir(output, { recursive: true });
    const win = BrowserWindow.getAllWindows()[0];
    if (win.webContents.isLoading()) await new Promise(resolve => win.webContents.once('did-finish-load', resolve));
    const result = await win.webContents.executeJavaScript(`(async()=>{
      await window.FormaSession.ready;
      const checks=[]; const assert=(ok,message)=>{if(!ok)throw new Error(message);checks.push(message);};
      assert(valid(scene),'Default project validates'); assert(scene.shapes.length===3,'Three animated example layers load');
      assert(state(scene.shapes[0],0).x!==state(scene.shapes[0],2).x,'Keyframes change shape positions');
      document.querySelector('[data-add="arrow"]').click(); assert(scene.shapes.some(s=>s.type==='arrow'),'Arrow can be added');
      document.getElementById('add-linked-text').click(); assert(scene.shapes.some(s=>s.type==='text'),'Linked text can be added');
      assert(valid(JSON.parse(JSON.stringify(scene))),'Edited project survives JSON roundtrip');
      const animated=svgText(true); assert(animated.includes('animateTransform'),'Animated SVG contains motion'); assert(!animated.includes('NaN'),'Export has finite coordinates');
      assert(window.MediaRecorder && HTMLCanvasElement.prototype.captureStream,'Video export APIs are available');
      const canvas=document.createElement('canvas');canvas.width=scene.width;canvas.height=scene.height;paint(canvas.getContext('2d'),2);
      const png=canvas.toDataURL('image/png'); assert(png.startsWith('data:image/png;base64,'),'PNG renders');
      scene.bg='#123456';refresh(); return {checks,animated,png};
    })()`);
    await fs.writeFile(path.join(output, 'animation.svg'), result.animated);
    const png = Buffer.from(result.png.split(',')[1], 'base64');
    assert.equal(png.readUInt32BE(16),960); assert.equal(png.readUInt32BE(20),600);
    await fs.writeFile(path.join(output,'frame.png'),png);
    const closed = new Promise(resolve => win.once('closed', resolve)); win.close(); await closed;
    const recovered = JSON.parse(await fs.readFile(path.join(app.getPath('userData'),'last-session.json'),'utf8'));
    assert.equal(recovered.bg,'#123456');
    console.log('PASS: ' + result.checks.length + ' Forma desktop checks, PNG/SVG export, and recovery on close.');
    clearTimeout(timeout); app.exit(0);
  } catch (error) { console.error(error); clearTimeout(timeout); app.exit(1); }
});
