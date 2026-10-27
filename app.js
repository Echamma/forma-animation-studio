const $=id=>document.getElementById(id), NS='http://www.w3.org/2000/svg';
const motionHelp=$('motion-help'),motionOptions=motionHelp.previousElementSibling.previousElementSibling;
motionOptions.querySelector('label').firstChild.textContent='Animation seconds';
const distanceLabel=document.createElement('label');distanceLabel.innerHTML='Distance %<input id="motion-distance" type="number" min="0" max="500" step="10" value="100">';motionOptions.insertBefore(distanceLabel,motionOptions.lastElementChild);
const motionPanel=document.createElement('div');motionPanel.innerHTML='<div class="section-label">Your moves <span id="motion-count">0</span></div><div id="motion-list" class="motion-list"></div><div id="motion-edit" hidden><div class="field-grid motion-edit-fields"><label>Starts at (s)<input id="motion-start" type="number" min="0" step="0.01"></label><label>Length (s)<input id="motion-length" type="number" min="0.1" step="0.01"></label><label>Distance %<input id="motion-edit-distance" type="number" min="0" max="500" step="1"></label><label>Spin °<input id="motion-edit-spin" type="number" step="1"></label></div><label class="motion-check"><input id="motion-edit-face" type="checkbox"> Rotate to face movement</label><button id="update-motion" class="wide">Apply animation changes</button><button id="preview-motion" class="wide">▶ Preview this animation</button><button id="add-rotation-to-motion" class="wide">＋ Add rotation over this animation</button><label class="motion-check"><input id="live-motion-preview" type="checkbox"> Preview changes automatically</label><button id="delete-motion" class="wide danger">Delete selected animation</button></div>';motionHelp.after(motionPanel);
const rotationPanel=document.createElement('div');rotationPanel.innerHTML='<div class="section-label">Rotation <span>↻</span></div><button id="add-rotation" class="wide">＋ Add rotation</button><p class="help">Select a move first to match its timing, or add rotation at the playhead. 360° makes one full turn; use a negative value for the other direction.</p><div id="rotation-list" class="motion-list"></div><div id="rotation-edit" hidden><div class="field-grid motion-edit-fields"><label>Starts at (s)<input id="rotation-start" type="number" min="0" step="0.01"></label><label>Length (s)<input id="rotation-length" type="number" min="0.1" step="0.01"></label><label>Turn by °<input id="rotation-degrees" type="number" min="-10000" max="10000" step="15"></label><label>Easing<select id="rotation-ease"><option value="linear">Linear</option><option value="smooth">Ease in & out</option></select></label></div><button id="update-rotation" class="wide">Apply rotation changes</button><button id="preview-rotation" class="wide">▶ Preview rotation</button><label class="motion-check"><input id="live-rotation-preview" type="checkbox"> Preview changes automatically</label><button id="delete-rotation" class="wide danger">Delete rotation</button></div>';motionPanel.after(rotationPanel);
const disappearPanel=document.createElement('div');disappearPanel.id='disappear-panel';disappearPanel.innerHTML='<div class="section-label">Fade out <span>◌</span></div><button id="add-disappear" class="wide">＋ Add disappearance</button><div id="disappear-edit" hidden><div class="field-grid"><label>Fade out at (s)<input id="disappear-start" type="number" min="0" step="0.01"></label><label>Fade out over (s)<input id="disappear-length" type="number" min="0" step="0.01"></label></div><label class="full-label">Reappear<select id="reappear-mode"><option value="never">Stay hidden indefinitely</option><option value="time">At a set time</option><option value="clip">When another animation finishes</option></select></label><label id="reappear-time-wrap" class="full-label" hidden>Reappear at (s)<input id="reappear-time" type="number" min="0" step="0.01"></label><label id="reappear-clip-wrap" class="full-label" hidden>After animation<select id="reappear-clip"></select></label><p class="help" id="disappear-help">Fade out starts at the chosen time. Reappearance is instant.</p><button id="update-disappear" class="wide">Apply disappearance</button><button id="preview-disappear" class="wide">▶ Preview disappearance</button><button id="delete-disappear" class="wide danger">Remove disappearance</button></div>';rotationPanel.after(disappearPanel);
const shapeTools=document.querySelector('.shape-tools'),linkedTextButton=document.createElement('button');linkedTextButton.id='add-linked-text';linkedTextButton.className='linked-text-tool';linkedTextButton.innerHTML='<i class="shape text-shape">T</i>Linked text';shapeTools.append(linkedTextButton);
const selectedTitle=$('selected-title'),textPanel=document.createElement('div');textPanel.id='text-panel';textPanel.hidden=true;textPanel.innerHTML='<div class="section-label">Linked text <span>↗</span></div><label class="full-label">Text<input id="text-content" type="text" maxlength="120" value="Your text"></label><label class="full-label">Follows shape<select id="text-link"></select></label>';selectedTitle.after(textPanel);
let scene,selected,time=0,playing=false,previewEnd=null,last=0,drag=null,rotationDrag=null,resizeDrag=null,recording=false,motionMode=null,motionDraft=null,motionGuide=null,selectedMotionId=null,selectedRotationId=null;
const baseRotationLabel=document.createElement('label');baseRotationLabel.title='Rotate this layer at every point in the scene, without making an animation';baseRotationLabel.innerHTML='Layer angle °<input id="base-rotation" type="number" step="1" value="0">';$('o').parentElement.before(baseRotationLabel);
const baseRotationHelp=document.createElement('p');baseRotationHelp.className='help';baseRotationHelp.textContent='Drag the canvas handles to resize. Corners keep proportions; hold Shift to stretch. Use the round handle to rotate.';$('o').closest('.field-grid').after(baseRotationHelp);
$('panel-edit').prepend(textPanel);
function showInspectorTab(name,focus=false){for(const tab of document.querySelectorAll('[data-inspector-tab]')){const active=tab.dataset.inspectorTab===name;tab.setAttribute('aria-selected',String(active));tab.tabIndex=active?0:-1;$('panel-'+tab.dataset.inspectorTab).hidden=!active;if(active&&focus)tab.focus()}document.querySelector('.inspector-content').scrollTop=0}
for(const tab of document.querySelectorAll('[data-inspector-tab]')){tab.onclick=()=>showInspectorTab(tab.dataset.inspectorTab);tab.onkeydown=e=>{if(!['ArrowLeft','ArrowRight','Home','End'].includes(e.key))return;e.preventDefault();const names=['edit','animate','scene'],current=names.indexOf(tab.dataset.inspectorTab),next=e.key==='Home'?0:e.key==='End'?2:(current+(e.key==='ArrowRight'?1:2))%3;showInspectorTab(names[next],true)}}
showInspectorTab('edit');
$('help-open').onclick=$('guide-open').onclick=()=>$('help-dialog').showModal();
function showDrawerTab(name,focus=false){for(const tab of document.querySelectorAll('[data-drawer-tab]')){const active=tab.dataset.drawerTab===name;tab.setAttribute('aria-selected',String(active));tab.tabIndex=active?0:-1;$('drawer-'+tab.dataset.drawerTab).hidden=!active;if(active&&focus)tab.focus()}$('tool-drawer').scrollTop=0}
let drawerReturnFocus=null;
function openDrawer(name){drawerReturnFocus=$(name==='create'?'open-create':'open-layers');showDrawerTab(name);document.querySelector('main').classList.add('drawer-open');$('tool-drawer').setAttribute('aria-hidden','false');$('tool-drawer').removeAttribute('inert');$('drawer-scrim').hidden=false;for(const id of ['open-create','open-layers'])$(id).setAttribute('aria-expanded','true');$('close-drawer').focus()}
function closeDrawer(){document.querySelector('main').classList.remove('drawer-open');$('tool-drawer').setAttribute('aria-hidden','true');$('tool-drawer').setAttribute('inert','');$('drawer-scrim').hidden=true;for(const id of ['open-create','open-layers'])$(id).setAttribute('aria-expanded','false');drawerReturnFocus?.focus()}
$('open-create').onclick=()=>openDrawer('create');$('open-layers').onclick=()=>openDrawer('layers');$('close-drawer').onclick=$('drawer-scrim').onclick=closeDrawer;
for(const tab of document.querySelectorAll('[data-drawer-tab]')){tab.onclick=()=>showDrawerTab(tab.dataset.drawerTab);tab.onkeydown=e=>{if(!['ArrowLeft','ArrowRight'].includes(e.key))return;e.preventDefault();showDrawerTab(tab.dataset.drawerTab==='create'?'layers':'create',true)}}
document.addEventListener('keydown',e=>{if(e.key==='Escape'&&document.querySelector('main').classList.contains('drawer-open'))closeDrawer()});
const props=['x','y','w','h','r','o'];
const keyframeMenu=document.createElement('div');
keyframeMenu.id='keyframe-menu';keyframeMenu.hidden=true;
keyframeMenu.innerHTML='<strong>Keyframe</strong><label>Move to (seconds)<input id="keyframe-move-time" type="number" min="0" step="0.01"></label><button id="keyframe-move" type="button">Move keyframe</button><button id="keyframe-remove" type="button" class="danger">Delete keyframe</button><p id="keyframe-menu-status" role="status"></p>';
document.body.append(keyframeMenu);
let keyframeMenuTarget=null;
function closeKeyframeMenu(){keyframeMenu.hidden=true;keyframeMenuTarget=null}
function keyframeMenuError(message){$('keyframe-menu-status').textContent=message}
function detachMotionKey(s,k){
  const motionId=k.motionId||(s.motions||[]).find(m=>Math.abs(m.startTime-k.t)<.011)?.id;
  if(!motionId)return false;
  s.motions=s.motions.filter(m=>m.id!==motionId);
  for(const key of s.keys)if(key.motionId===motionId)delete key.motionId;
  if(selectedMotionId===motionId){selectedMotionId=null;motionGuide=null}
  return true
}
function removeKeyframe(s,k){
  if(s.keys.length<=1)return false;
  detachMotionKey(s,k);s.keys.splice(s.keys.indexOf(k),1);return true
}
function openKeyframeMenu(e,s,k){
  e.preventDefault();e.stopPropagation();stop();selected=s.id;time=k.t;selectedMotionId=k.motionId||null;selectedRotationId=null;showInspectorTab('edit');refresh();
  keyframeMenuTarget={s,k};$('keyframe-move-time').value=k.t;$('keyframe-remove').disabled=s.keys.length<=1;keyframeMenuError('');
  keyframeMenu.hidden=false;
  keyframeMenu.style.left=`${Math.max(8,Math.min(e.clientX,innerWidth-keyframeMenu.offsetWidth-8))}px`;
  keyframeMenu.style.top=`${Math.max(8,Math.min(e.clientY,innerHeight-keyframeMenu.offsetHeight-8))}px`;
  $('keyframe-move-time').focus();$('keyframe-move-time').select()
}
$('keyframe-move').onclick=()=>{
  if(!keyframeMenuTarget)return;
  const{s,k}=keyframeMenuTarget,entry=$('keyframe-move-time'),newTime=Math.round(Number(entry.value)*100)/100;
  if(!entry.value.trim()||!Number.isFinite(newTime)||newTime<0)return keyframeMenuError('Enter a time of 0 seconds or more.');
  if(s.keys.some(other=>other!==k&&Math.abs(other.t-newTime)<.001))return keyframeMenuError('Another keyframe is already at that time.');
  if(newTime===k.t){closeKeyframeMenu();return}
  extendSceneTo(newTime);const detached=detachMotionKey(s,k);k.t=newTime;s.keys.sort((a,b)=>a.t-b.t);time=newTime;
  closeKeyframeMenu();refresh();toast(detached?'Keyframe moved; animation converted to editable keyframes':'Keyframe moved')
};
$('keyframe-remove').onclick=()=>{
  if(!keyframeMenuTarget)return;
  const{s,k}=keyframeMenuTarget;if(!removeKeyframe(s,k))return keyframeMenuError('Keep at least one keyframe per shape.');
  closeKeyframeMenu();refresh();toast('Keyframe deleted')
};
$('keyframe-move-time').onkeydown=e=>{if(e.key==='Enter')$('keyframe-move').click()};
document.addEventListener('pointerdown',e=>{if(!keyframeMenu.hidden&&!keyframeMenu.contains(e.target))closeKeyframeMenu()});
document.addEventListener('keydown',e=>{if(e.key==='Escape')closeKeyframeMenu()});
const assetCache=new Map();
const audioSourceCache=new WeakMap();let sharedAudioContext;
function example(){scene={version:1,width:960,height:600,duration:4,bg:'#f4f0e8',shapes:[{id:'a',name:'Coral circle',type:'circle',color:'#ed795e',ease:'smooth',keys:[{t:0,x:-120,y:265,w:190,h:190,r:0,o:100},{t:2,x:565,y:275,w:190,h:190,r:0,o:100},{t:4,x:360,y:265,w:190,h:190,r:0,o:100}]},{id:'b',name:'Lavender block',type:'rect',color:'#aaa0ce',ease:'smooth',keys:[{t:0,x:555,y:325,w:180,h:180,r:15,o:100},{t:2,x:370,y:320,w:180,h:180,r:105,o:100},{t:4,x:1080,y:325,w:180,h:180,r:195,o:100}]},{id:'c',name:'Lime triangle',type:'triangle',color:'#c3d77f',ease:'smooth',keys:[{t:0,x:470,y:-90,w:125,h:120,r:0,o:100},{t:2,x:470,y:390,w:125,h:120,r:180,o:100},{t:4,x:470,y:200,w:125,h:120,r:360,o:100}]}]};selected='b';time=0;selectedMotionId=selectedRotationId=null;motionMode=motionDraft=motionGuide=null;$('stage')?.classList.remove('motion-mode');stop();refresh()}
function rawState(s,t=time){let a=s.keys[0],b=a;for(const k of s.keys){if(k.t<=t)a=k;if(k.t>=t){b=k;break}b=k}let f=a===b?0:Math.max(0,Math.min(1,(t-a.t)/(b.t-a.t)));if(s.ease==='smooth')f=f*f*(3-2*f);if(s.ease==='hold')f=0;return Object.fromEntries(props.map(p=>[p,a[p]+(b[p]-a[p])*f]))}
function rotationAt(s,t=time){return(s.rotationClips||[]).reduce((sum,clip)=>{if(t<=clip.startTime)return sum;const f=Math.max(0,Math.min(1,(t-clip.startTime)/clip.duration)),e=clip.ease==='smooth'?f*f*(3-2*f):f;return sum+clip.degrees*e},0)}
function animationClips(excludeId){return scene.shapes.filter(shape=>shape.id!==excludeId).flatMap(shape=>[...(shape.keys.length>1?[{shape,clip:{id:'keys',startTime:shape.keys[0].t,duration:shape.keys.at(-1).t-shape.keys[0].t},label:`${shape.name} · Keyframes`}]:[]),...(shape.motions||[]).map((clip,i)=>({shape,clip,label:`${shape.name} · ${clip.type==='path'?'Path':'Move'} ${i+1}`})),...(shape.rotationClips||[]).map((clip,i)=>({shape,clip,label:`${shape.name} · Rotation ${i+1}`}))])}
function reappearAt(s){const d=s.disappear;if(!d||d.reappearMode==='never')return Infinity;if(d.reappearMode==='time')return d.reappearTime;const target=scene.shapes.find(shape=>shape.id===d.triggerShapeId&&shape.id!==s.id),clip=d.triggerClipId==='keys'&&target?.keys.length>1?{startTime:target.keys[0].t,duration:target.keys.at(-1).t-target.keys[0].t}:target?.motions?.find(item=>item.id===d.triggerClipId)||target?.rotationClips?.find(item=>item.id===d.triggerClipId);return clip?Math.max(d.startTime+d.duration,clip.startTime+clip.duration):Infinity}
function visibilityAt(s,t){const d=s.disappear;if(!d||t<d.startTime)return 1;const end=d.startTime+d.duration,returnTime=reappearAt(s);if(t>=returnTime)return 1;if(d.duration===0||t>=end)return 0;return Math.max(0,1-(t-d.startTime)/d.duration)}
function state(s,t=time){const own=rawState(s,t);own.r+=rotationAt(s,t)+(s.baseRotation||0);own.o*=visibilityAt(s,t);if(s.type!=='text'||!s.linkId)return own;const target=scene.shapes.find(x=>x.id===s.linkId);if(!target)return own;const p=rawState(target,t);p.r+=rotationAt(target,t)+(target.baseRotation||0);const base=s.linkBase||{w:p.w,h:p.h},sx=p.w/(base.w||p.w),sy=p.h/(base.h||p.h),angle=p.r*Math.PI/180,ox=(s.offsetX||0)*sx,oy=(s.offsetY||0)*sy;return{...own,x:p.x+ox*Math.cos(angle)-oy*Math.sin(angle),y:p.y+ox*Math.sin(angle)+oy*Math.cos(angle),w:(s.textWidth||own.w)*Math.abs(sx),h:(s.fontSize||own.h)*Math.abs(sy),r:p.r+(s.rotationOffset||0)+rotationAt(s,t)+(s.baseRotation||0),o:Math.max(0,Math.min(100,p.o*(s.opacityFactor??1)*visibilityAt(target,t)*visibilityAt(s,t)))}}
function editableState(s,t=time){const pose=state(s,t);pose.o=rawState(s,t).o;if(s.type!=='text'||!s.linkId)pose.r-=rotationAt(s,t)+(s.baseRotation||0);return pose}
function element(tag,attrs){const e=document.createElementNS(NS,tag);for(const [k,v]of Object.entries(attrs))e.setAttribute(k,v);return e}
function isAsset(s){return s?.type==='image'||s?.type==='svg'||s?.type==='video'}
function svgData(source){return`data:image/svg+xml;charset=utf-8,${encodeURIComponent(source)}`}
function assetHref(s){return s.type==='svg'?svgData(s.svgSource):s.type==='video'?s.posterData:s.assetData}
function sanitizeSvg(source){
  const doc=new DOMParser().parseFromString(source,'image/svg+xml'),root=doc.documentElement;
  if(!root||root.localName!=='svg'||doc.querySelector('parsererror'))throw Error('Invalid SVG markup');
  root.querySelectorAll('script,style,foreignObject,iframe,object,embed,audio,video').forEach(e=>e.remove());
  [root,...root.querySelectorAll('*')].forEach(e=>{for(const a of [...e.attributes]){const n=a.name.toLowerCase(),v=a.value.trim();if(n.startsWith('on')||n==='srcdoc'||((n==='href'||n.endsWith(':href'))&&!v.startsWith('#')&&!v.startsWith('data:image/'))||(/url\s*\(/i.test(v)&&/url\s*\(/i.test(v.replace(/url\s*\(\s*#[a-z0-9_-]+\s*\)/gi,'')))||/javascript:|@import|expression\s*\(/i.test(v))e.removeAttribute(a.name)}});
  root.setAttribute('xmlns',NS);return new XMLSerializer().serializeToString(root)
}
function recolorSvg(source,color){const doc=new DOMParser().parseFromString(source,'image/svg+xml'),root=doc.documentElement;if(!root||root.localName!=='svg'||doc.querySelector('parsererror'))throw Error('Invalid SVG markup');root.setAttribute('fill',color);[root,...root.querySelectorAll('*')].forEach(e=>{for(const attr of ['fill','stroke']){const value=e.getAttribute(attr);if(value&&value!=='none'&&!value.startsWith('url('))e.setAttribute(attr,color)}const style=e.getAttribute('style');if(style)e.setAttribute('style',style.replace(/(fill|stroke)\s*:\s*(?!none|url\()[^;]+/gi,`$1:${color}`))});return sanitizeSvg(new XMLSerializer().serializeToString(root))}
function mediaElement(s){
  const source=s.type==='video'?s.assetData:assetHref(s);if(assetCache.has(source))return assetCache.get(source);
  if(s.type==='video'){const v=document.createElement('video');v.src=source;v.muted=false;v.loop=true;v.playsInline=true;v.preload='auto';v.style.cssText='width:100%;height:100%;object-fit:fill;display:block;pointer-events:none';assetCache.set(source,v);return v}
  const img=new Image();img.src=source;assetCache.set(source,img);return img
}
function syncVideo(s){const v=mediaElement(s);if(!Number.isFinite(v.duration)||v.duration<=0)return;const target=time%v.duration;if(Math.abs(v.currentTime-target)>.2)v.currentTime=target;if(playing){if(v.paused)v.play().catch(()=>{})}else v.pause()}
function geometry(s,p,exporting=false){if(s.type==='video'&&!exporting){const fo=element('foreignObject',{x:-p.w/2,y:-p.h/2,width:p.w,height:p.h});fo.append(mediaElement(s));return fo}if(isAsset(s))return element('image',{x:-p.w/2,y:-p.h/2,width:p.w,height:p.h,href:assetHref(s),preserveAspectRatio:'none'});if(s.type==='text'){const text=element('text',{x:0,y:0,'text-anchor':'middle','dominant-baseline':'middle','font-family':'DM Sans, sans-serif','font-size':p.h,'font-weight':'600',textLength:p.w,lengthAdjust:'spacingAndGlyphs'});text.textContent=s.text||'Text';return text}if(s.type==='circle')return element('ellipse',{cx:0,cy:0,rx:p.w/2,ry:p.h/2});if(s.type==='triangle')return element('polygon',{points:`0,${-p.h/2} ${p.w/2},${p.h/2} ${-p.w/2},${p.h/2}`});if(s.type==='diamond')return element('polygon',{points:`0,${-p.h/2} ${p.w/2},0 0,${p.h/2} ${-p.w/2},0`});if(s.type==='arrow')return element('polygon',{points:`${-p.w/2},${-p.h*.18} ${p.w*.12},${-p.h*.18} ${p.w*.12},${-p.h/2} ${p.w/2},0 ${p.w*.12},${p.h/2} ${p.w*.12},${p.h*.18} ${-p.w/2},${p.h*.18}`});return element('rect',{x:-p.w/2,y:-p.h/2,width:p.w,height:p.h,rx:3})}
function layerBlur(s){return Number.isFinite(s.blur)?Math.max(0,Math.min(100,s.blur)):0}
function addBlurFilter(defs,s,p){const amount=layerBlur(s);if(!amount)return null;const width=Math.max(p.w,...s.keys.map(k=>k.w)),height=Math.max(p.h,...s.keys.map(k=>k.h)),margin=amount*3+4,id=`blur-${s.id}`,filter=element('filter',{id,filterUnits:'userSpaceOnUse',x:-width/2-margin,y:-height/2-margin,width:width+margin*2,height:height+margin*2});filter.append(element('feGaussianBlur',{stdDeviation:amount}));defs.append(filter);return `url(#${id})`}
function pasteboard(){const m=Math.round(Math.max(scene.width,scene.height)*.55);return{m,x:-m,y:-m,w:scene.width+m*2,h:scene.height+m*2}}
const resizeDirections={nw:[-1,-1],n:[0,-1],ne:[1,-1],e:[1,0],se:[1,1],s:[0,1],sw:[-1,1],w:[-1,0]};
function resizeCursor(sx,sy,angle){const degrees=((Math.atan2(sy,sx)*180/Math.PI+angle)%180+180)%180;return['ew-resize','nwse-resize','ns-resize','nesw-resize'][Math.round(degrees/45)%4]}
function resizedPose(start,angle,handle,delta,freeform=false){
  const [sx,sy]=resizeDirections[handle],radians=angle*Math.PI/180,cos=Math.cos(radians),sin=Math.sin(radians);
  const dx=delta.x*cos+delta.y*sin,dy=-delta.x*sin+delta.y*cos;
  const maxW=Math.max(4000,start.w),maxH=Math.max(4000,start.h),clamp=(value,min,max)=>Math.max(min,Math.min(max,value));
  let w=start.w,h=start.h;
  if(sx&&sy&&!freeform){const scale=clamp(1+(sx*dx*start.w+sy*dy*start.h)/(start.w*start.w+start.h*start.h),Math.max(5/start.w,5/start.h),Math.min(maxW/start.w,maxH/start.h));w=start.w*scale;h=start.h*scale}
  else{if(sx)w=clamp(start.w+sx*dx,5,maxW);if(sy)h=clamp(start.h+sy*dy,5,maxH)}
  if(Math.abs(w-start.w)<.01&&Math.abs(h-start.h)<.01)return{...start};
  w=Math.round(w*10)/10;h=Math.round(h*10)/10;
  const shiftX=sx*(w-start.w)/2,shiftY=sy*(h-start.h)/2;
  return{...start,w,h,x:start.x+shiftX*cos-shiftY*sin,y:start.y+shiftX*sin+shiftY*cos}
}
function render(){
  const svg=$('canvas'),v=pasteboard();
  svg.replaceChildren();svg.setAttribute('viewBox',`${v.x} ${v.y} ${v.w} ${v.h}`);svg.style.background='transparent';
  const defs=element('defs',{}),clip=element('clipPath',{id:'frame-clip'}),marker=element('marker',{id:'motion-arrow-head',viewBox:'0 0 10 10',refX:8,refY:5,markerWidth:7,markerHeight:7,orient:'auto-start-reverse'});
  clip.append(element('rect',{x:0,y:0,width:scene.width,height:scene.height}));marker.append(element('path',{d:'M 0 0 L 10 5 L 0 10 z',fill:'#d2f583'}));defs.append(clip,marker);svg.append(defs);
  svg.append(element('rect',{x:0,y:0,width:scene.width,height:scene.height,fill:scene.bg,class:'artboard'}));
  const objects=element('g',playing||recording?{'clip-path':'url(#frame-clip)'}:{});
  for(const s of scene.shapes){
    const p=state(s);if(motionDraft?.shapeId===s.id&&motionDraft.type==='path'){const lastPoint=motionDraft.points.at(-1);p.x=lastPoint.x;p.y=lastPoint.y}
    if(s.type==='video')syncVideo(s);const g=element('g',{transform:`translate(${p.x} ${p.y}) rotate(${p.r})`,fill:s.color,opacity:p.o/100,class:'object','data-id':s.id});const art=element('g',{}),filter=addBlurFilter(defs,s,p);if(filter)art.setAttribute('filter',filter);art.append(geometry(s,p));g.append(art);
    objects.append(g)
  }
  svg.append(objects);
  if(selected&&!playing&&!recording){const s=scene.shapes.find(shape=>shape.id===selected);if(s){
    const p=state(s),scale=Math.max(.1,Math.min(svg.clientWidth/v.w||1,svg.clientHeight/v.h||1)),smallestSide=Math.min(p.w,p.h)*scale,handleSize=Math.min(12,Math.max(8,smallestSide*.28))/scale,hitSize=Math.min(28,Math.max(12,smallestSide*.5))/scale;
    const control=element('g',{transform:`translate(${p.x} ${p.y}) rotate(${p.r})`,class:'selection-control'});
    control.append(element('rect',{x:-p.w/2,y:-p.h/2,width:p.w,height:p.h,class:'selection-box'}));
    const labels={nw:'top left',n:'top',ne:'top right',e:'right',se:'bottom right',s:'bottom',sw:'bottom left',w:'left'};
    for(const [name,[sx,sy]]of Object.entries(resizeDirections)){
      if(sx===0&&p.w*scale<48||sy===0&&p.h*scale<48)continue;
      const x=sx*p.w/2,y=sy*p.h/2,hit=element('rect',{x:x-hitSize/2,y:y-hitSize/2,width:hitSize,height:hitSize,fill:'transparent',class:'resize-hit','data-resize-id':s.id,'data-resize-handle':name,'aria-label':`Resize ${s.name} from ${labels[name]}`}),visual=element('rect',{x:x-handleSize/2,y:y-handleSize/2,width:handleSize,height:handleSize,class:'resize-knob','pointer-events':'none'}),hint=element('title',{});
      hit.style.cursor=resizeCursor(sx,sy,p.r);hint.textContent=sx&&sy?`Drag to resize proportionally. Hold Shift to stretch ${s.name}.`:`Drag to resize ${s.name}.`;hit.append(hint);control.append(hit,visual)
    }
    svg.append(control);
    const top=-p.h/2,handleY=top-34/scale,rotation=element('g',{transform:`translate(${p.x} ${p.y}) rotate(${p.r})`,class:'rotation-control'}),stem=element('line',{x1:0,y1:top,x2:0,y2:handleY}),handle=element('circle',{cx:0,cy:handleY,r:10/scale,'data-rotate-id':s.id,role:'button','aria-label':'Drag to rotate layer'}),icon=element('text',{x:0,y:handleY,'text-anchor':'middle','dominant-baseline':'middle','pointer-events':'none'});icon.textContent='↻';icon.style.fontSize=`${14/scale}px`;rotation.append(stem,handle,icon);svg.append(rotation)
  }}
  const guide=motionDraft||motionGuide;
  if(guide&&!playing){const group=element('g',{class:'motion-guide'}),points=guide.type==='path'?guide.points:[guide.start,guide.end];if(points.length>1){group.append(element('polyline',{points:points.map(p=>`${p.x},${p.y}`).join(' '),class:'motion-guide-line','marker-end':'url(#motion-arrow-head)'}));group.append(element('circle',{cx:points[0].x,cy:points[0].y,r:10,class:'motion-start'}));group.append(element('circle',{cx:points.at(-1).x,cy:points.at(-1).y,r:7,class:'motion-end'}))}svg.append(group)}
  svg.append(element('rect',{x:0,y:0,width:scene.width,height:scene.height,fill:'none',stroke:'#6d746d','stroke-width':Math.max(2,scene.width/800),class:'frame-edge'}));
  $('time').innerHTML=`${time.toFixed(2)} <span>/ ${scene.duration.toFixed(2)} s</span>`;$('scrub').value=time
}
function fields(){const s=scene.shapes.find(s=>s.id===selected),asset=isAsset(s),isText=s?.type==='text';$('selected-title').textContent=s?s.name:'Select a layer';$('inspector-hint').textContent=s?`Editing at ${time.toFixed(2)} s · Open Animate to add movement.`:'Choose a layer on the canvas or in Layers.';$('base-rotation').disabled=!s;$('base-rotation').value=s?Math.round((s.baseRotation||0)*100)/100:'';$('layer-blur').disabled=!s;$('layer-blur').value=s?layerBlur(s):0;$('layer-blur-value').textContent=(s?layerBlur(s):0)+' px';for(const p of props){$(p).disabled=!s;$(p).value=s?Math.round((p==='o'?editableState(s)[p]:state(s)[p])*100)/100:''}for(const id of ['ease','delete','keyframe','delete-key','draw-arrow','record-path'])$(id).disabled=!s;$('drop-item').disabled=!s||isText&&!!s.linkId;$('color').disabled=!s||asset;$('fill-control').classList.toggle('disabled',asset);$('edit-svg').hidden=s?.type!=='svg';document.querySelectorAll('[data-outside]').forEach(b=>b.disabled=!s);if(s){$('color').value=asset?'#888888':s.color;$('color-value').textContent=asset?(s.type==='svg'?'VECTOR':s.type==='video'?'VIDEO':'IMAGE'):s.color.toUpperCase();$('ease').value=s.ease}if(isText){$('text-content').value=s.text||'';const link=$('text-link');link.replaceChildren();for(const target of scene.shapes.filter(x=>x.id!==s.id&&x.type!=='text')){const option=document.createElement('option');option.value=target.id;option.textContent=target.name;option.selected=target.id===s.linkId;link.append(option)}}}
function lists(){
  $('layers').replaceChildren();$('tracks').replaceChildren();
  for(const s of scene.shapes){
    const b=document.createElement('button');b.className='layer'+(selected===s.id?' active':'');
    const sw=document.createElement('span');sw.className='swatch'+(isAsset(s)?' asset':'');
    if(isAsset(s))sw.style.backgroundImage=`url("${assetHref(s).replace(/"/g,'%22')}")`;else sw.style.background=s.color;
    const name=document.createElement('span');name.textContent=s.name;
    const end=document.createElement('span');end.className='layer-end';end.textContent=s.type==='svg'?'✎':s.type==='video'?'▶':'◇';
    b.append(sw,name,end);b.onclick=()=>{selected=s.id;selectedMotionId=selectedRotationId=null;motionGuide=null;closeDrawer();refresh()};$('layers').append(b);
    const row=document.createElement('div');row.className='track'+(selected===s.id&&!selectedMotionId&&!selectedRotationId?' active':'');
    const label=document.createElement('span');label.className='track-name';label.textContent=s.name;
    const line=document.createElement('div');line.className='track-line';
    for(const k of s.keys){const d=document.createElement('button');d.className='key-dot';d.dataset.keyTime=k.t;d.style.left=`${k.t/scene.duration*100}%`;d.title=`${s.name} at ${k.t.toFixed(2)} seconds — right-click to move or delete`;d.onclick=e=>{e.stopPropagation();stop();time=k.t;selected=s.id;selectedMotionId=k.motionId||null;selectedRotationId=null;refresh()};d.oncontextmenu=e=>openKeyframeMenu(e,s,k);line.append(d)}
    row.append(label,line);row.onclick=()=>{selected=s.id;selectedMotionId=selectedRotationId=null;motionGuide=null;refresh()};$('tracks').append(row);
    if(s.disappear){const d=s.disappear,visibilityRow=document.createElement('div'),visibilityLabel=document.createElement('span'),visibilityLine=document.createElement('div'),bar=document.createElement('button'),end=reappearAt(s);visibilityRow.className='track motion-track disappear-track'+(selected===s.id?' active':'');visibilityLabel.className='track-name';visibilityLabel.textContent='↳ Disappear';visibilityLine.className='track-line';bar.className='motion-bar disappear-bar';bar.style.left=`${d.startTime/scene.duration*100}%`;bar.style.width=`${Math.max(1.5,(Math.min(end,scene.duration)-d.startTime)/scene.duration*100)}%`;bar.title=`Fade out ${d.startTime.toFixed(2)}–${(d.startTime+d.duration).toFixed(2)} s${Number.isFinite(end)?`; reappear ${end.toFixed(2)} s`:'; stay hidden'}`;bar.textContent='◌ Hidden';bar.onclick=e=>{e.stopPropagation();stop();selected=s.id;time=d.startTime;showInspectorTab('animate');refresh();$('disappear-edit').scrollIntoView({block:'nearest'})};visibilityLine.append(bar);visibilityRow.append(visibilityLabel,visibilityLine);visibilityRow.onclick=bar.onclick;$('tracks').append(visibilityRow)}
    for(const [i,m] of (s.motions||[]).entries()){
      const motionRow=document.createElement('div');motionRow.className='track motion-track'+(m.id===selectedMotionId?' active':'');motionRow.dataset.motionId=m.id;
      const motionLabel=document.createElement('span');motionLabel.className='track-name';motionLabel.textContent=`↳ ${m.type==='path'?'Path':'Move'} ${i+1}`;
      const motionLine=document.createElement('div');motionLine.className='track-line';
      const clip=document.createElement('button');clip.className='motion-bar';clip.style.left=`${m.startTime/scene.duration*100}%`;clip.style.width=`${m.duration/scene.duration*100}%`;clip.title=`${s.name}: ${m.startTime.toFixed(2)}–${(m.startTime+m.duration).toFixed(2)} s`;
      const clipText=document.createElement('span');clipText.textContent=m.type==='path'?'Mouse path':'Arrow move';
      const handle=document.createElement('span');handle.className='motion-handle';handle.title='Drag to change animation length';
      clip.append(clipText,handle);clip.onclick=e=>{e.stopPropagation();if(clip.dataset.ignoreClick==='yes')return;selectMotion(s,m)};
      clip.onpointerdown=e=>beginMotionClipDrag(e,s,m,clip,motionLine);
      motionLine.append(clip);motionRow.append(motionLabel,motionLine);motionRow.onclick=()=>selectMotion(s,m);$('tracks').append(motionRow)
    }
    for(const [i,clip] of (s.rotationClips||[]).entries()){
      const rotationRow=document.createElement('div');rotationRow.className='track motion-track rotation-track'+(clip.id===selectedRotationId?' active':'');rotationRow.dataset.rotationId=clip.id;
      const rotationLabel=document.createElement('span');rotationLabel.className='track-name';rotationLabel.textContent=`↳ Rotation ${i+1}`;
      const rotationLine=document.createElement('div');rotationLine.className='track-line';
      const bar=document.createElement('button');bar.className='motion-bar rotation-bar';bar.style.left=`${clip.startTime/scene.duration*100}%`;bar.style.width=`${clip.duration/scene.duration*100}%`;bar.title=`Rotation ${clip.degrees}° · ${clip.startTime.toFixed(2)}–${(clip.startTime+clip.duration).toFixed(2)} s`;
      const text=document.createElement('span');text.textContent=`↻ ${clip.degrees}°`;const handle=document.createElement('span');handle.className='motion-handle';handle.title='Drag to change rotation length';bar.append(text,handle);
      bar.onclick=e=>{e.stopPropagation();if(bar.dataset.ignoreClick!=='yes')selectRotation(s,clip)};
      bar.onpointerdown=e=>beginRotationClipDrag(e,s,clip,bar,rotationLine);
      rotationLine.append(bar);rotationRow.append(rotationLabel,rotationLine);rotationRow.onclick=()=>selectRotation(s,clip);$('tracks').append(rotationRow)
    }
  }
  $('count').textContent=scene.shapes.length;
  $('key-count').textContent=`${scene.shapes.length} layers · ${scene.shapes.reduce((n,s)=>n+s.keys.length,0)} keyframes · ${scene.shapes.reduce((n,s)=>n+(s.motions?.length||0)+(s.rotationClips?.length||0)+(s.disappear?1:0),0)} animations`
}
function renderMotionList(){const s=scene.shapes.find(s=>s.id===selected),motions=s?.motions||[],list=$('motion-list');list.replaceChildren();$('motion-count').textContent=motions.length;if(!motions.length){const p=document.createElement('p');p.className='empty-motion';p.textContent='No drawn animations yet.';list.append(p)}for(const [i,m] of motions.entries()){const b=document.createElement('button');b.className='motion-clip'+(m.id===selectedMotionId?' active':'');b.innerHTML=`<span class="motion-icon">${m.type==='path'?'⌁':'↗'}</span><b>${m.type==='path'?'Mouse path':'Arrow move'} ${i+1}</b><small>${m.startTime.toFixed(1)}–${(m.startTime+m.duration).toFixed(1)} s · ${m.distance}% distance</small>`;b.onclick=()=>selectMotion(s,m);list.append(b)}const active=motions.find(m=>m.id===selectedMotionId);$('motion-edit').hidden=!active}
function renderRotationList(){const s=scene.shapes.find(s=>s.id===selected),list=$('rotation-list');list.replaceChildren();$('add-rotation').disabled=!s;$('add-rotation').textContent=selectedMotionId?'＋ Add rotation over this animation':'＋ Add rotation at playhead';for(const [i,clip] of (s?.rotationClips||[]).entries()){const b=document.createElement('button');b.className='motion-clip'+(clip.id===selectedRotationId?' active':'');b.textContent=`↻ Rotation ${i+1} · ${clip.degrees}° · ${clip.startTime.toFixed(2)}–${(clip.startTime+clip.duration).toFixed(2)} s`;b.onclick=()=>selectRotation(s,clip);list.append(b)}$('rotation-edit').hidden=!(s?.rotationClips||[]).some(clip=>clip.id===selectedRotationId)}
function showReappearFields(){const mode=$('reappear-mode').value;$('reappear-time-wrap').hidden=mode!=='time';$('reappear-clip-wrap').hidden=mode!=='clip'}
function renderDisappear(){const s=scene.shapes.find(shape=>shape.id===selected),d=s?.disappear;$('add-disappear').disabled=!s;$('add-disappear').hidden=!!d;$('disappear-edit').hidden=!d;if(!d)return;$('disappear-start').value=d.startTime;$('disappear-length').value=d.duration;$('reappear-mode').value=d.reappearMode;$('reappear-time').value=d.reappearMode==='time'?d.reappearTime:Math.min(scene.duration,d.startTime+d.duration+1);const select=$('reappear-clip');select.replaceChildren();for(const {shape,clip,label} of animationClips(s.id)){const option=document.createElement('option');option.value=`${shape.id}|${clip.id}`;option.textContent=`${label} · ends ${(clip.startTime+clip.duration).toFixed(2)} s`;select.append(option)}const wanted=`${d.triggerShapeId||''}|${d.triggerClipId||''}`;if([...select.options].some(option=>option.value===wanted))select.value=wanted;else if(d.reappearMode==='clip'){const option=document.createElement('option');option.value='';option.textContent='Linked animation is missing';select.prepend(option);select.value=''}if(!select.options.length){const option=document.createElement('option');option.value='';option.textContent='Animate another layer first';select.append(option)}$('disappear-help').textContent=d.reappearMode==='clip'&&!select.value?'Linked animation is missing. Choose another animation to restore reappearance.':'Fade out starts at the chosen time. Reappearance is instant.';showReappearFields()}
$('reappear-mode').onchange=showReappearFields;
$('add-disappear').onclick=()=>{const s=scene.shapes.find(shape=>shape.id===selected);if(!s)return;stop();s.disappear={startTime:time,duration:Math.min(.5,Math.max(0,scene.duration-time)),reappearMode:'never'};refresh();$('disappear-edit').scrollIntoView({block:'nearest'});toast('Disappearance added')};
$('update-disappear').onclick=()=>{const s=scene.shapes.find(shape=>shape.id===selected);if(!s?.disappear)return;const start=Number($('disappear-start').value),duration=Number($('disappear-length').value),mode=$('reappear-mode').value,reappearTime=Number($('reappear-time').value);if(!$('disappear-start').value.trim()||!$('disappear-length').value.trim()||!Number.isFinite(start)||start<0||!Number.isFinite(duration)||duration<0){toast('Enter a valid fade time and length');return}const d={startTime:Math.round(start*100)/100,duration:Math.round(duration*100)/100,reappearMode:mode};const fadeEnd=d.startTime+d.duration;if(mode==='time'){if(!$('reappear-time').value.trim()||!Number.isFinite(reappearTime)||reappearTime<fadeEnd){toast('Reappearance must follow the fade out');return}d.reappearTime=Math.round(reappearTime*100)/100}else if(mode==='clip'){const [shapeId,clipId]=$('reappear-clip').value.split('|'),match=animationClips(s.id).find(({shape,clip})=>shape.id===shapeId&&clip.id===clipId);if(!match){toast('Choose an animation on another layer');return}if(match.clip.startTime+match.clip.duration<fadeEnd-.001){toast('That animation ends before the fade out finishes');return}d.triggerShapeId=shapeId;d.triggerClipId=clipId}stop();extendSceneTo(Math.max(fadeEnd,mode==='time'?d.reappearTime:0));s.disappear=d;refresh();recordSceneChange('edit disappearance');toast('Disappearance updated')};
$('delete-disappear').onclick=()=>{const s=scene.shapes.find(shape=>shape.id===selected);if(!s?.disappear)return;stop();delete s.disappear;refresh();toast('Disappearance removed')};
$('preview-disappear').onclick=()=>{const s=scene.shapes.find(shape=>shape.id===selected),d=s?.disappear;if(!d)return;stop();time=Math.max(0,d.startTime-.25);previewEnd=Math.min(scene.duration,Math.max(d.startTime+d.duration,reappearAt(s))+(.25));if(!Number.isFinite(previewEnd))previewEnd=scene.duration;playing=true;last=performance.now();$('play').textContent='Ⅱ';$('play').setAttribute('aria-label','Pause preview');render()};
let workspaceZoom=1,workspacePanX=0,workspacePanY=0,workspacePan=null;
function sizePasteboard(center=false){
  const viewport=$('stage-viewport'),v=pasteboard();
  const availableWidth=Math.max(200,viewport.clientWidth-24),availableHeight=Math.max(180,viewport.clientHeight-24);
  const artboardWidth=Math.min(scene.width,availableWidth*.92,availableHeight*.92*scene.width/scene.height);
  $('stage').style.width=`${artboardWidth*v.w/scene.width*workspaceZoom}px`;
  $('stage').style.height=`${artboardWidth*v.h/scene.width*workspaceZoom}px`;
  if(center){workspacePanX=workspacePanY=0}
  $('stage').style.transform=`translate(-50%, -50%) translate(${workspacePanX}px, ${workspacePanY}px)`;
  $('zoom-level').textContent=`${Math.round(workspaceZoom*100)}%`;
}
function setWorkspaceZoom(next,clientX,clientY){
  const viewport=$('stage-viewport'),rect=viewport.getBoundingClientRect(),old=workspaceZoom;
  workspaceZoom=Math.max(.2,Math.min(5,next));
  const x=clientX-rect.left-rect.width/2,y=clientY-rect.top-rect.height/2,ratio=workspaceZoom/old;
  workspacePanX=x-(x-workspacePanX)*ratio;workspacePanY=y-(y-workspacePanY)*ratio;
  sizePasteboard();render()
}
const workspaceViewport=$('stage-viewport');
workspaceViewport.addEventListener('pointerdown',e=>{
  if(e.button!==1)return;
  const svg=$('canvas'),matrix=svg.getScreenCTM();
  if(matrix){const p=new DOMPoint(e.clientX,e.clientY).matrixTransform(matrix.inverse());if(p.x>=0&&p.x<=scene.width&&p.y>=0&&p.y<=scene.height)return}
  e.preventDefault();workspacePan={x:e.clientX,y:e.clientY,panX:workspacePanX,panY:workspacePanY,id:e.pointerId};
  workspaceViewport.setPointerCapture(e.pointerId);workspaceViewport.classList.add('panning')
});
workspaceViewport.addEventListener('pointermove',e=>{if(!workspacePan||e.pointerId!==workspacePan.id)return;workspacePanX=workspacePan.panX+e.clientX-workspacePan.x;workspacePanY=workspacePan.panY+e.clientY-workspacePan.y;sizePasteboard()});
function endWorkspacePan(e){if(!workspacePan||e.pointerId!==workspacePan.id)return;workspacePan=null;workspaceViewport.classList.remove('panning')}
workspaceViewport.addEventListener('pointerup',endWorkspacePan);workspaceViewport.addEventListener('pointercancel',endWorkspacePan);
workspaceViewport.addEventListener('auxclick',e=>{if(e.button===1)e.preventDefault()});
workspaceViewport.addEventListener('wheel',e=>{e.preventDefault();setWorkspaceZoom(workspaceZoom*Math.exp(-e.deltaY*.0015),e.clientX,e.clientY)},{passive:false});
for(const [id,factor] of [['zoom-in',1.25],['zoom-out',.8]])$(id).onclick=()=>{const r=workspaceViewport.getBoundingClientRect();setWorkspaceZoom(workspaceZoom*factor,r.left+r.width/2,r.top+r.height/2)};
$('zoom-level').onclick=()=>{workspaceZoom=1;sizePasteboard(true);render()};
new ResizeObserver(()=>{if(scene)sizePasteboard()}).observe($('stage-viewport'));
function refresh(){const v=pasteboard();sizePasteboard();$('stage').style.setProperty('--stage-ratio',v.w/v.h);render();fields();lists();renderMotionList();renderRotationList();renderDisappear();$('duration').value=scene.duration;$('bg').value=scene.bg;$('background-picker').value=scene.bg;document.querySelectorAll('[data-bg]').forEach(b=>b.classList.toggle('active',b.dataset.bg.toLowerCase()===scene.bg.toLowerCase()));$('format').value=`${scene.width}x${scene.height}`;$('canvas-size').innerHTML=`${scene.width} × ${scene.height} <span> • </span> ${playing?'PREVIEW':'EDIT VIEW'}`;$('scrub').max=scene.duration;document.querySelectorAll('.ruler span').forEach((e,i)=>e.textContent=`${+(scene.duration*i/4).toFixed(2)}s`)}
function keyAt(s,at,p){const t=Math.round(at*100)/100;let k=s.keys.find(k=>Math.abs(k.t-t)<.011);if(k)Object.assign(k,p);else s.keys.push({t,...p});s.keys.sort((a,b)=>a.t-b.t)}
function key(s,p){keyAt(s,time,p)}
function extendSceneTo(end){if(!Number.isFinite(end)||end<0)throw Error('Enter a finite duration');if(end>scene.duration)scene.duration=end}
function motionWindow(){const value=+$('motion-duration').value,duration=Number.isFinite(value)&&value>=.1?value:1;return{start:time,end:time+duration}}
function scaledMotionPoints(m){const scale=m.distance/100,start=m.startPose;return m.sourcePoints.map(p=>({x:start.x+(p.x-start.x)*scale,y:start.y+(p.y-start.y)*scale}))}
function motionGuideFor(m){const points=scaledMotionPoints(m);return{type:m.type,start:points[0],end:points.at(-1),points}}
function restoreMotionKeys(s,m){s.keys=s.keys.filter(k=>k.motionId!==m.id);if(!(m.baseKeys||[]).some(k=>Math.abs(k.t-m.startTime)<.011))s.keys=s.keys.filter(k=>!(Math.abs(k.t-m.startTime)<.011&&props.every(p=>Math.abs(k[p]-m.startPose[p])<.011)));for(const k of m.baseKeys||[]){s.keys=s.keys.filter(x=>Math.abs(x.t-k.t)>.001);s.keys.push({...k})}s.keys.sort((a,b)=>a.t-b.t)}
function applyMotion(s,m){s.keys=s.keys.filter(k=>k.motionId!==m.id);const points=scaledMotionPoints(m),end=Math.min(scene.duration,m.startTime+m.duration),span=end-m.startTime;if(span<=.01)return;const startKey={t:m.startTime,...m.startPose,motionId:m.id};s.keys=s.keys.filter(k=>k.t<m.startTime-.001||k.t>end+.001);s.keys.push(startKey);const samples=m.type==='arrow'?points:points.filter((p,i,a)=>i===0||i===a.length-1||Math.hypot(p.x-a[i-1].x,p.y-a[i-1].y)>3);samples.forEach((p,i)=>{if(i===0)return;const f=i/(samples.length-1),next=samples[Math.min(i+1,samples.length-1)],prev=samples[Math.max(0,i-1)],pose={...m.startPose,x:p.x,y:p.y,r:m.startPose.r+m.spin*f};if(m.face)pose.r=Math.atan2(next.y-prev.y,next.x-prev.x)*180/Math.PI+m.spin*f;s.keys.push({t:Math.round((m.startTime+span*f)*100)/100,...pose,motionId:m.id})});s.keys.sort((a,b)=>a.t-b.t);s.keys=s.keys.filter((k,i,a)=>i===a.length-1||Math.abs(k.t-a[i+1].t)>.001)}
function selectMotion(s,m){stop();showInspectorTab('animate');selected=s.id;selectedMotionId=m.id;selectedRotationId=null;time=m.startTime;$('motion-duration').value=m.duration;$('motion-distance').value=m.distance;$('motion-spin').value=m.spin;$('face-path').checked=m.face;$('motion-start').value=m.startTime.toFixed(2);$('motion-length').value=m.duration.toFixed(2);$('motion-edit-distance').value=m.distance;$('motion-edit-spin').value=m.spin;$('motion-edit-face').checked=m.face;motionGuide=motionGuideFor(m);refresh();$('motion-edit').scrollIntoView({block:'nearest'})}
function setMotionMode(mode){const s=scene.shapes.find(s=>s.id===selected);if(!s)return toast('Select a shape first');stop();motionMode=motionMode===mode?null:mode;motionDraft=null;motionGuide=null;$('stage').classList.toggle('motion-mode',!!motionMode);$('draw-arrow').classList.toggle('active',motionMode==='arrow');$('record-path').classList.toggle('active',motionMode==='path');$('motion-help').textContent=motionMode==='arrow'?'Drag anywhere on the canvas to set the arrow destination.':motionMode==='path'?'Hold and drag on the canvas. The selected shape will follow your mouse.':'Choose a tool, then drag anywhere on the canvas. The motion begins at the playhead.';render()}
function finishMotion(){
  if(!motionDraft)return;
  const s=scene.shapes.find(s=>s.id===motionDraft.shapeId),window=motionWindow(),spin=Math.max(-10000,Math.min(10000,+$('motion-spin').value||0)),face=$('face-path').checked,start=motionDraft.startState,distance=Math.max(0,Math.min(500,+$('motion-distance').value||0));
  if(!s||!Number.isFinite(window.end)||window.end-window.start<.05){motionDraft=null;setMotionMode(null);toast('Enter a valid animation length');return}extendSceneTo(window.end);
  const raw=motionDraft.type==='arrow'?[motionDraft.start,motionDraft.end]:motionDraft.points.filter((p,i,a)=>i===0||Math.hypot(p.x-a[i-1].x,p.y-a[i-1].y)>3),step=Math.max(1,Math.ceil(raw.length/90)),sourcePoints=raw.filter((_,i)=>i%step===0||i===raw.length-1),m={id:crypto.randomUUID(),type:motionDraft.type,startTime:window.start,duration:window.end-window.start,distance,spin,face,startPose:{...start},sourcePoints,baseKeys:[]};
  s.motions??=[];const overlapping=s.motions.filter(x=>x.startTime<window.end&&x.startTime+x.duration>window.start);for(const old of overlapping)restoreMotionKeys(s,old);s.motions=s.motions.filter(x=>!overlapping.includes(x));m.baseKeys=s.keys.filter(k=>k.t>=window.start-.001&&k.t<=window.end+.001&&!k.motionId).map(k=>({...k}));s.motions.push(m);applyMotion(s,m);selectedMotionId=m.id;selectedRotationId=null;motionGuide=motionGuideFor(m);motionDraft=null;motionMode=null;$('stage').classList.remove('motion-mode');$('draw-arrow').classList.remove('active');$('record-path').classList.remove('active');time=window.end;$('motion-help').textContent='Animation selected. Adjust its timing and settings, then preview it.';selectMotion(s,m);recordSceneChange('create motion');toast(`Animation created · ${m.duration.toFixed(1)} s · ${m.distance}% distance`)
}
function stop(){playing=false;previewEnd=null;for(const s of scene?.shapes||[])if(s.type==='video')mediaElement(s).pause();$('play').textContent='▶';$('play').setAttribute('aria-label','Play animation');if(scene)$('canvas-size').innerHTML=`${scene.width} × ${scene.height} <span> • </span> EDIT VIEW`}
function tick(now){if(playing){time+=(now-last)/1000;const end=previewEnd??scene.duration;if(time>=end){if(previewEnd!==null){time=end;stop();fields()}else if($('loop').checked)time%=scene.duration;else{time=scene.duration;stop();fields()}}render()}last=now;requestAnimationFrame(tick)}
$('play').onclick=()=>{if(playing){stop();fields();render()}else{if(motionMode)setMotionMode(null);if(time>=scene.duration)time=0;playing=true;last=performance.now();$('play').textContent='Ⅱ';$('play').setAttribute('aria-label','Pause animation');$('canvas-size').innerHTML=`${scene.width} × ${scene.height} <span> • </span> CLIPPED PREVIEW`;render()}};
$('start').onclick=()=>{stop();time=0;refresh()};$('scrub').oninput=e=>{stop();time=+e.target.value;render();fields()};
function setLinkedTextPose(s,pose){const target=scene.shapes.find(x=>x.id===s.linkId);if(!target)return;const p=rawState(target);p.r+=rotationAt(target,time)+(target.baseRotation||0);const base=s.linkBase||{w:p.w,h:p.h},sx=p.w/(base.w||p.w),sy=p.h/(base.h||p.h),angle=-p.r*Math.PI/180,dx=pose.x-p.x,dy=pose.y-p.y;s.offsetX=(dx*Math.cos(angle)-dy*Math.sin(angle))/(sx||1);s.offsetY=(dx*Math.sin(angle)+dy*Math.cos(angle))/(sy||1);s.textWidth=pose.w/(Math.abs(sx)||1);s.fontSize=pose.h/(Math.abs(sy)||1);s.rotationOffset=pose.r-p.r-rotationAt(s,time)-(s.baseRotation||0);s.opacityFactor=p.o?pose.o/p.o:1}
for(const p of props)$(p).oninput=()=>{const s=scene.shapes.find(s=>s.id===selected);if(!s)return;stop();const v=+$(p).value;if(!Number.isFinite(v))return;const n=editableState(s);n[p]=p==='r'&&!(s.type==='text'&&s.linkId)?v-rotationAt(s,time)-(s.baseRotation||0):p==='o'?Math.max(0,Math.min(100,v)):p==='w'||p==='h'?Math.max(5,Math.min(2000,v)):Math.max(-10000,Math.min(10000,v));if(s.type==='text'&&s.linkId)setLinkedTextPose(s,n);else key(s,n);render();lists()};
$('base-rotation').oninput=()=>{const s=scene.shapes.find(s=>s.id===selected),value=+$('base-rotation').value;if(!s||!Number.isFinite(value))return;stop();s.baseRotation=Math.max(-10000,Math.min(10000,value));render();$('r').value=Math.round(state(s).r*100)/100};
$('layer-blur').oninput=()=>{const s=scene.shapes.find(s=>s.id===selected);if(!s)return;stop();s.blur=+$('layer-blur').value;$('layer-blur-value').textContent=`${s.blur} px`;render()};
$('color').oninput=()=>{const s=scene.shapes.find(s=>s.id===selected);if(s){s.color=$('color').value;render();fields();lists()}};$('ease').onchange=()=>{const s=scene.shapes.find(s=>s.id===selected);if(s){s.ease=$('ease').value;render()}};
function setBackground(color){scene.bg=color;$('bg').value=color;$('background-picker').value=color;document.querySelectorAll('[data-bg]').forEach(b=>b.classList.toggle('active',b.dataset.bg.toLowerCase()===color.toLowerCase()));render()}
$('bg').oninput=()=>setBackground($('bg').value);$('background-picker').oninput=()=>setBackground($('background-picker').value);document.querySelectorAll('[data-bg]').forEach(b=>b.onclick=()=>setBackground(b.dataset.bg));$('duration').onchange=()=>{const next=+$('duration').value;if(!Number.isFinite(next)||next<1){$('duration').value=scene.duration;toast('Enter a scene length of at least 1 second');return}const latest=Math.max(0,...scene.shapes.flatMap(s=>[...s.keys.map(k=>k.t),...(s.motions||[]).map(m=>m.startTime+m.duration),...(s.rotationClips||[]).map(c=>c.startTime+c.duration),...(s.disappear?[s.disappear.startTime+s.disappear.duration,...(s.disappear.reappearMode==='time'?[s.disappear.reappearTime]:[])]:[])]));if(next+1e-9<latest){$('duration').value=scene.duration;toast(`Move animations or keyframes before shortening below ${latest.toFixed(2)} s`);return}stop();scene.duration=next;time=Math.min(time,next);refresh()};
$('format').onchange=()=>{stop();const [width,height]=$('format').value.split('x').map(Number),sx=width/scene.width,sy=height/scene.height,scale=Math.min(sx,sy);for(const s of scene.shapes){for(const k of s.keys){k.x*=sx;k.y*=sy;k.w*=scale;k.h*=scale}if(s.type==='text'){s.offsetX*=scale;s.offsetY*=scale;s.textWidth*=scale;s.fontSize*=scale;if(s.linkBase){s.linkBase.w*=scale;s.linkBase.h*=scale}}for(const m of s.motions||[]){m.startPose.x*=sx;m.startPose.y*=sy;m.startPose.w*=scale;m.startPose.h*=scale;for(const p of m.sourcePoints){p.x*=sx;p.y*=sy}for(const k of m.baseKeys||[]){k.x*=sx;k.y*=sy;k.w*=scale;k.h*=scale}}}scene.width=width;scene.height=height;if(selectedMotionId){const s=scene.shapes.find(s=>s.id===selected),m=s?.motions?.find(m=>m.id===selectedMotionId);if(m)motionGuide=motionGuideFor(m)}refresh();toast(width<height?'Canvas changed to 9:16 portrait':'Canvas changed to landscape')};
document.querySelectorAll('[data-add]').forEach(b=>b.onclick=()=>{stop();const type=b.dataset.add,id=crypto.randomUUID(),size=Math.round(Math.min(scene.width,scene.height)*.18),w=type==='arrow'?size*1.55:size,h=type==='arrow'?size*.8:size;scene.shapes.push({id,name:`${type[0].toUpperCase()+type.slice(1)} ${scene.shapes.length+1}`,type,color:['#ed795e','#aaa0ce','#c3d77f','#83bfc0'][scene.shapes.length%4],ease:'smooth',keys:[{t:0,x:scene.width/2,y:scene.height/2,w,h,r:0,o:100}]});selected=id;selectedMotionId=selectedRotationId=null;motionGuide=null;closeDrawer();refresh()});
$('add-linked-text').onclick=()=>{const target=scene.shapes.find(s=>s.id===selected);if(!target||target.type==='text')return toast('Select a shape, image, SVG, or video to link the text to');stop();const p=rawState(target),id=crypto.randomUUID(),fontSize=Math.max(24,Math.min(scene.width,scene.height)*.07),textWidth=fontSize*4.2,offsetY=-(p.h/2+fontSize);scene.shapes.push({id,name:`Text for ${target.name}`,type:'text',text:'Your text',linkId:target.id,linkBase:{w:p.w,h:p.h},offsetX:0,offsetY,textWidth,fontSize,rotationOffset:0,opacityFactor:1,color:'#ffffff',ease:'smooth',keys:[{t:0,x:p.x,y:p.y+offsetY,w:textWidth,h:fontSize,r:p.r,o:p.o}]});selected=id;selectedMotionId=selectedRotationId=null;motionGuide=null;closeDrawer();refresh();$('text-content').focus();$('text-content').select();toast(`Text linked to ${target.name}`)};
$('text-content').oninput=()=>{const s=scene.shapes.find(s=>s.id===selected);if(s?.type==='text'){s.text=$('text-content').value||'Text';s.name=s.text.slice(0,32)||'Text';$('selected-title').textContent=s.name;render();lists()}};
$('text-link').onchange=()=>{const s=scene.shapes.find(s=>s.id===selected),target=scene.shapes.find(x=>x.id===$('text-link').value);if(s?.type!=='text'||!target)return;const visible=state(s),p=rawState(target),angle=-p.r*Math.PI/180,dx=visible.x-p.x,dy=visible.y-p.y;s.linkId=target.id;s.linkBase={w:p.w,h:p.h};s.offsetX=dx*Math.cos(angle)-dy*Math.sin(angle);s.offsetY=dx*Math.sin(angle)+dy*Math.cos(angle);s.textWidth=visible.w;s.fontSize=visible.h;s.rotationOffset=visible.r-p.r;s.opacityFactor=p.o?visible.o/p.o:1;refresh();toast(`Text now follows ${target.name}`)};
function readDataUrl(file){return new Promise((resolve,reject)=>{const r=new FileReader();r.onload=()=>resolve(r.result);r.onerror=()=>reject(r.error);r.readAsDataURL(file)})}
function imageInfo(src){return new Promise((resolve,reject)=>{const img=new Image();img.onload=()=>resolve({width:img.naturalWidth,height:img.naturalHeight});img.onerror=reject;img.src=src})}
function videoInfo(src){return new Promise((resolve,reject)=>{const v=document.createElement('video');v.muted=true;v.preload='auto';v.src=src;v.onloadedmetadata=()=>{const capture=()=>{try{const max=720,scale=Math.min(1,max/Math.max(v.videoWidth,v.videoHeight)),c=document.createElement('canvas');c.width=Math.max(1,Math.round(v.videoWidth*scale));c.height=Math.max(1,Math.round(v.videoHeight*scale));c.getContext('2d').drawImage(v,0,0,c.width,c.height);resolve({width:v.videoWidth,height:v.videoHeight,duration:v.duration,poster:c.toDataURL('image/jpeg',.82)})}catch(error){reject(error)}};v.onseeked=capture;v.currentTime=Math.min(.05,Math.max(0,v.duration/10));if(v.currentTime===0)setTimeout(capture,50)};v.onerror=reject;v.load()})}
function svgInfo(source){const doc=new DOMParser().parseFromString(source,'image/svg+xml'),root=doc.documentElement,box=(root.getAttribute('viewBox')||'').trim().split(/[ ,]+/).map(Number),width=parseFloat(root.getAttribute('width')),height=parseFloat(root.getAttribute('height'));return{width:Number.isFinite(width)&&width>0?width:Number.isFinite(box[2])&&box[2]>0?box[2]:300,height:Number.isFinite(height)&&height>0?height:Number.isFinite(box[3])&&box[3]>0?box[3]:300}}
function fittedAssetSize(width,height){const max=Math.min(scene.width,scene.height)*.38,ratio=Math.max(.05,Math.min(20,width/height));return ratio>=1?{w:max,h:max/ratio}:{w:max*ratio,h:max}}
$('import-asset').onclick=()=>$('asset-file').click();
async function importMediaFiles(files){
  const items=[...files],errors=[];let added=0;stop();
  for(const file of items){
    try{
      if(scene.shapes.length>=100)throw Error('Scene is full (100 layers)');
      const id=crypto.randomUUID(),name=file.name.replace(/\.[^.]+$/,'').slice(0,70)||'Media';let shape;
      if(file.type==='image/svg+xml'||/\.svg$/i.test(file.name)){
        const source=sanitizeSvg(await file.text()),info=svgInfo(source),size=fittedAssetSize(info.width,info.height);
        shape={id,name,type:'svg',svgSource:source,color:'#888888',ease:'smooth',keys:[{t:0,x:scene.width/2,y:scene.height/2,w:size.w,h:size.h,r:0,o:100}]};
      }else if(['video/mp4','video/webm','video/ogg'].includes(file.type)||/\.(mp4|webm|ogv)$/i.test(file.name)){
        const data=await readDataUrl(file),info=await videoInfo(data),size=fittedAssetSize(info.width,info.height);
        shape={id,name,type:'video',assetData:data,posterData:info.poster,mediaDuration:info.duration,color:'#888888',ease:'smooth',keys:[{t:0,x:scene.width/2,y:scene.height/2,w:size.w,h:size.h,r:0,o:100}]};
      }else if(['image/png','image/jpeg','image/webp'].includes(file.type)||/\.(png|jpe?g|webp)$/i.test(file.name)){
        const data=await readDataUrl(file),info=await imageInfo(data),size=fittedAssetSize(info.width,info.height);
        shape={id,name,type:'image',assetData:data,color:'#888888',ease:'smooth',keys:[{t:0,x:scene.width/2,y:scene.height/2,w:size.w,h:size.h,r:0,o:100}]};
      }else throw Error('Unsupported media type');
      const offset=(added%7-3)*Math.min(36,scene.width*.035);
      shape.keys[0].x+=offset;shape.keys[0].y+=offset;
      scene.shapes.push(shape);selected=id;added++;refresh();recordSceneChange('import');
    }catch(error){errors.push(`${file.name}: ${error.message||'Could not import'}`)}
  }
  if(added)closeDrawer();
   if(added||errors.length)toast(`${added} media layer${added===1?'':'s'} added${errors.length?`; ${errors.length} skipped (${errors[0]})`:''}`);
  return{added,errors}
}
$('asset-file').multiple=true;
$('asset-file').onchange=e=>{const files=[...e.target.files];e.target.value='';importMediaFiles(files)};
let mediaDragDepth=0;
document.addEventListener('dragenter',e=>{if(!e.dataTransfer?.types?.includes('Files'))return;e.preventDefault();mediaDragDepth++;document.body.classList.add('media-dragging')});
document.addEventListener('dragover',e=>{if(!e.dataTransfer?.types?.includes('Files'))return;e.preventDefault();e.dataTransfer.dropEffect='copy'});
document.addEventListener('dragleave',e=>{if(!e.dataTransfer?.types?.includes('Files'))return;e.preventDefault();if(--mediaDragDepth<=0){mediaDragDepth=0;document.body.classList.remove('media-dragging')}});
document.addEventListener('drop',e=>{if(!e.dataTransfer?.files?.length)return;e.preventDefault();mediaDragDepth=0;document.body.classList.remove('media-dragging');importMediaFiles(e.dataTransfer.files)});
document.querySelectorAll('[data-outside]').forEach(b=>b.onclick=()=>{const s=scene.shapes.find(s=>s.id===selected);if(!s)return;stop();const p=editableState(s),gap=Math.max(40,Math.min(scene.width,scene.height)*.05);if(b.dataset.outside==='left')p.x=-p.w/2-gap;if(b.dataset.outside==='right')p.x=scene.width+p.w/2+gap;if(b.dataset.outside==='top')p.y=-p.h/2-gap;if(b.dataset.outside==='bottom')p.y=scene.height+p.h/2+gap;if(s.type==='text'&&s.linkId)setLinkedTextPose(s,p);else key(s,p);refresh();toast(`Placed outside at ${time.toFixed(2)} s`) });
$('draw-arrow').onclick=()=>setMotionMode('arrow');$('record-path').onclick=()=>setMotionMode('path');
$('drop-item').onclick=()=>{
  const s=scene.shapes.find(shape=>shape.id===selected),requested=Number($('drop-duration').value);
  if(!s||s.type==='text'&&s.linkId)return;
  if(!Number.isFinite(requested)||requested<.4||requested>10)return toast('Choose a drop length from 0.4 to 10 seconds');
  const duration=Math.round(requested*100)/100,start=Math.round(time*100)/100,end=Math.round((start+duration)*100)/100;
  if((s.motions||[]).some(m=>m.startTime<end-.005&&m.startTime+m.duration>start+.005)||s.keys.some(k=>k.t>start+.005&&k.t<=end+.005))return toast('Move the playhead to a clear section of this layer first');
  const landing=editableState(s),above=-landing.h/2-Math.max(40,scene.height*.08);
  if(landing.x+landing.w/2<0||landing.x-landing.w/2>scene.width||landing.y+landing.h/2<0||landing.y-landing.h/2>scene.height)return toast('Place the item on the canvas where it should land first');
  stop();if(motionMode)setMotionMode(null);extendSceneTo(end);
  const at=(fraction,changes)=>({t:Math.round((start+duration*fraction)*100)/100,...landing,...changes});
  const squashY=landing.y+landing.h*.09;
  s.keys=s.keys.filter(k=>k.t<start-.005||k.t>end+.005);
  s.keys.push(
    at(0,{y:above}),
    at(.18,{y:above+(landing.y-above)*.07}),
    at(.60,{y:squashY,w:landing.w*1.13,h:landing.h*.82}),
    at(.71,{y:landing.y-landing.h*.17,w:landing.w*.94,h:landing.h*1.06}),
    at(.80,{y:landing.y-landing.h*.28}),
    at(.91,{y:landing.y+landing.h*.04,w:landing.w*1.06,h:landing.h*.93}),
    at(1,{}));
  s.keys.sort((a,b)=>a.t-b.t);selectedMotionId=selectedRotationId=null;motionGuide=null;
  time=start;refresh();recordSceneChange('add drop animation');toast('Drop animation added');previewMotion({startTime:start,duration})
};
function selectedAnimation(){const s=scene.shapes.find(s=>s.id===selected);return{s,m:s?.motions?.find(m=>m.id===selectedMotionId)}}
function editMotion({preview=false,quiet=false}={}){
  const{s,m}=selectedAnimation();if(!m)return false;
  const requestedStart=+$('motion-start').value,requestedLength=+$('motion-length').value;
  if(!Number.isFinite(requestedStart)||requestedStart<0||!Number.isFinite(requestedLength)||requestedLength<.1||!Number.isFinite(requestedStart+requestedLength)){toast('Enter a valid start and animation length');selectMotion(s,m);return false}
  const start=Math.round(requestedStart*100)/100,length=Math.round(requestedLength*100)/100;
  if(s.motions.some(other=>other!==m&&start<other.startTime+other.duration-.001&&start+length>other.startTime+.001)){toast('This animation overlaps another animation on the same shape');selectMotion(s,m);return false}
  stop();extendSceneTo(start+length);restoreMotionKeys(s,m);
  m.startTime=start;m.duration=length;m.distance=Math.max(0,Math.min(500,+$('motion-edit-distance').value||0));m.spin=Math.max(-10000,Math.min(10000,+$('motion-edit-spin').value||0));m.face=$('motion-edit-face').checked;
  m.baseKeys=s.keys.filter(k=>k.t>=start-.001&&k.t<=start+length+.001&&!k.motionId).map(k=>({...k}));
  applyMotion(s,m);motionGuide=motionGuideFor(m);time=start;selectMotion(s,m);
  recordSceneChange('edit motion');if(!quiet)toast(`Animation updated · ${length.toFixed(2)} s`);
  if(preview)previewMotion(m);
  return true
}
function previewMotion(m){if(!m)return;stop();time=m.startTime;previewEnd=Math.min(scene.duration,m.startTime+m.duration);playing=true;last=performance.now();$('play').textContent='Ⅱ';$('play').setAttribute('aria-label','Pause preview');render()}
$('update-motion').onclick=()=>editMotion({preview:$('live-motion-preview').checked});
$('preview-motion').onclick=()=>{const{m}=selectedAnimation();previewMotion(m)};
let editPreviewTimer;
for(const id of ['motion-start','motion-length','motion-edit-distance','motion-edit-spin','motion-edit-face'])$(id).addEventListener(id==='motion-edit-face'?'change':'input',()=>{if(!$('live-motion-preview').checked)return;clearTimeout(editPreviewTimer);editPreviewTimer=setTimeout(()=>editMotion({preview:true,quiet:true}),300)});
$('delete-motion').onclick=()=>{const{s,m}=selectedAnimation();if(!m)return;stop();restoreMotionKeys(s,m);s.motions=s.motions.filter(x=>x.id!==m.id);time=m.startTime;selectedMotionId=selectedRotationId=null;motionGuide=null;$('motion-help').textContent='Animation deleted. Choose a tool to draw another motion.';refresh();toast('Animation deleted')};
function beginMotionClipDrag(e,s,m,clip,line){
  if(e.button!==0)return;e.stopPropagation();
  const resizing=!!e.target.closest('.motion-handle'),initialX=e.clientX,initialStart=m.startTime,initialLength=m.duration;
  let moved=false,start=initialStart,length=initialLength;
  clip.setPointerCapture(e.pointerId);
  clip.onpointermove=move=>{const delta=(move.clientX-initialX)/line.getBoundingClientRect().width*scene.duration;if(Math.abs(delta)<.015)return;moved=true;
    if(resizing)length=Math.max(.1,Math.min(scene.duration-initialStart,Math.round((initialLength+delta)*100)/100));
    else start=Math.max(0,Math.min(scene.duration-initialLength,Math.round((initialStart+delta)*100)/100));
    clip.style.left=`${start/scene.duration*100}%`;clip.style.width=`${length/scene.duration*100}%`;clip.title=`${start.toFixed(2)}–${(start+length).toFixed(2)} s`
  };
  clip.onpointerup=()=>{clip.onpointermove=null;clip.onpointerup=null;if(!moved)return;clip.dataset.ignoreClick='yes';selectMotion(s,m);$('motion-start').value=start.toFixed(2);$('motion-length').value=length.toFixed(2);editMotion({preview:$('live-motion-preview').checked})};
  clip.onpointercancel=()=>{clip.onpointermove=null;clip.onpointerup=null;lists()}
}
function selectedRotation(){const s=scene.shapes.find(s=>s.id===selected);return{s,clip:s?.rotationClips?.find(clip=>clip.id===selectedRotationId)}}
function selectRotation(s,clip){stop();showInspectorTab('animate');selected=s.id;selectedMotionId=null;selectedRotationId=clip.id;motionGuide=null;time=clip.startTime;$('rotation-start').value=clip.startTime.toFixed(2);$('rotation-length').value=clip.duration.toFixed(2);$('rotation-degrees').value=clip.degrees;$('rotation-ease').value=clip.ease;refresh();$('rotation-edit').scrollIntoView({block:'nearest'})}
$('add-rotation-to-motion').onclick=()=>$('add-rotation').click();
$('add-rotation').onclick=()=>{const s=scene.shapes.find(s=>s.id===selected);if(!s)return;stop();const m=s.motions?.find(m=>m.id===selectedMotionId),start=m?.startTime??time,requested=+$('motion-duration').value,duration=m?.duration??(Number.isFinite(requested)&&requested>=.1?requested:1),clip={id:crypto.randomUUID(),startTime:start,duration,degrees:360,ease:'linear'};if(!Number.isFinite(start+duration)){toast('Enter a valid rotation length');return}extendSceneTo(start+duration);s.rotationClips??=[];s.rotationClips.push(clip);selectRotation(s,clip);toast(m?'Rotation added over selected animation':'Rotation added at playhead')};
function editRotation({preview=false,quiet=false}={}){const{s,clip}=selectedRotation();if(!clip)return false;const requestedStart=+$('rotation-start').value,requestedLength=+$('rotation-length').value;if(!Number.isFinite(requestedStart)||requestedStart<0||!Number.isFinite(requestedLength)||requestedLength<.1||!Number.isFinite(requestedStart+requestedLength)){toast('Enter a valid rotation start and length');selectRotation(s,clip);return false}const start=Math.round(requestedStart*100)/100,length=Math.round(requestedLength*100)/100,degrees=Math.max(-10000,Math.min(10000,+$('rotation-degrees').value||0));stop();extendSceneTo(start+length);Object.assign(clip,{startTime:start,duration:length,degrees,ease:$('rotation-ease').value});selectRotation(s,clip);recordSceneChange('edit rotation');if(!quiet)toast(`Rotation updated · ${degrees}° over ${length.toFixed(2)} s`);if(preview)previewMotion(clip);return true}
$('update-rotation').onclick=()=>editRotation({preview:$('live-rotation-preview').checked});
$('preview-rotation').onclick=()=>{const{clip}=selectedRotation();previewMotion(clip)};
let rotationPreviewTimer;for(const id of ['rotation-start','rotation-length','rotation-degrees','rotation-ease'])$(id).addEventListener(id==='rotation-ease'?'change':'input',()=>{if(!$('live-rotation-preview').checked)return;clearTimeout(rotationPreviewTimer);rotationPreviewTimer=setTimeout(()=>editRotation({preview:true,quiet:true}),300)});
$('delete-rotation').onclick=()=>{const{s,clip}=selectedRotation();if(!clip)return;stop();s.rotationClips=s.rotationClips.filter(x=>x.id!==clip.id);selectedRotationId=null;time=clip.startTime;refresh();toast('Rotation animation deleted')};
function beginRotationClipDrag(e,s,clip,bar,line){if(e.button!==0)return;e.stopPropagation();const resizing=!!e.target.closest('.motion-handle'),initialX=e.clientX,initialStart=clip.startTime,initialLength=clip.duration;let moved=false,start=initialStart,length=initialLength;bar.setPointerCapture(e.pointerId);bar.onpointermove=move=>{const delta=(move.clientX-initialX)/line.getBoundingClientRect().width*scene.duration;if(Math.abs(delta)<.015)return;moved=true;if(resizing)length=Math.max(.1,Math.min(scene.duration-initialStart,Math.round((initialLength+delta)*100)/100));else start=Math.max(0,Math.min(scene.duration-initialLength,Math.round((initialStart+delta)*100)/100));bar.style.left=`${start/scene.duration*100}%`;bar.style.width=`${length/scene.duration*100}%`};bar.onpointerup=()=>{bar.onpointermove=null;bar.onpointerup=null;if(!moved)return;bar.dataset.ignoreClick='yes';selectRotation(s,clip);$('rotation-start').value=start.toFixed(2);$('rotation-length').value=length.toFixed(2);editRotation({preview:$('live-rotation-preview').checked})};bar.onpointercancel=()=>{bar.onpointermove=null;bar.onpointerup=null;lists()}}
let selectedSvgPart=null;
function svgPreview(){
  const preview=$('svg-preview');preview.replaceChildren();selectedSvgPart=null;$('svg-part').textContent='Select a part above';
  for(const id of ['svg-fill','svg-stroke','svg-delete-part'])$(id).disabled=true;
  try{const source=sanitizeSvg($('svg-source').value),doc=new DOMParser().parseFromString(source,'image/svg+xml'),root=document.importNode(doc.documentElement,true);preview.append(root);$('svg-status').textContent='';return root}catch{$('svg-status').textContent='Fix the SVG markup to preview it.'}
}
function svgPartColor(part,attribute){const value=part.getAttribute(attribute)||part.style[attribute];return /^#[0-9a-f]{6}$/i.test(value)?value:'#d2f583'}
$('svg-preview').onclick=e=>{const root=$('svg-preview').firstElementChild,part=e.target;if(!root||part===root||!(part instanceof SVGElement))return;selectedSvgPart=part;$('svg-part').textContent=`Selected: ${part.tagName.toLowerCase()}`;for(const attr of ['fill','stroke']){$('svg-'+attr).disabled=false;$('svg-'+attr).value=svgPartColor(part,attr)}$('svg-delete-part').disabled=false};
for(const attr of ['fill','stroke'])$('svg-'+attr).oninput=()=>{if(!selectedSvgPart)return;selectedSvgPart.style.removeProperty(attr);selectedSvgPart.setAttribute(attr,$('svg-'+attr).value);$('svg-source').value=new XMLSerializer().serializeToString($('svg-preview').firstElementChild)};
$('svg-delete-part').onclick=()=>{if(!selectedSvgPart)return;selectedSvgPart.remove();selectedSvgPart=null;$('svg-source').value=new XMLSerializer().serializeToString($('svg-preview').firstElementChild);svgPreview()};
$('svg-source').oninput=()=>svgPreview();
$('edit-svg').onclick=()=>{const s=scene.shapes.find(s=>s.id===selected);if(s?.type!=='svg')return;$('svg-source').value=s.svgSource;$('svg-status').textContent='';svgPreview();$('svg-dialog').showModal()};
$('recolor-svg').onclick=()=>{try{$('svg-source').value=recolorSvg($('svg-source').value,$('svg-color').value);svgPreview();$('svg-status').textContent='Artwork recolored. Apply the changes to update the layer.'}catch{$('svg-status').textContent='The SVG markup is not valid yet.'}};
$('apply-svg').onclick=()=>{const s=scene.shapes.find(s=>s.id===selected);if(s?.type!=='svg')return;try{const old=assetHref(s);s.svgSource=sanitizeSvg($('svg-source').value);assetCache.delete(old);$('svg-dialog').close();render();lists();toast('SVG artwork updated')}catch{$('svg-status').textContent='Fix the SVG markup before applying it.'}};
$('delete').onclick=()=>{const s=scene.shapes.find(s=>s.id===selected);if(!s)return;for(const linked of scene.shapes.filter(x=>x.linkId===s.id)){const p=state(linked);linked.keys=[{t:0,...p}];delete linked.linkId;delete linked.linkBase}if(s.type==='video')mediaElement(s).pause();if(isAsset(s))assetCache.delete(s.type==='video'?s.assetData:assetHref(s));scene.shapes=scene.shapes.filter(x=>x.id!==selected);selected=scene.shapes.at(-1)?.id;selectedMotionId=selectedRotationId=null;motionGuide=null;refresh()};$('keyframe').onclick=()=>{const s=scene.shapes.find(s=>s.id===selected);if(s){stop();key(s,editableState(s));refresh();toast('Keyframe added at '+time.toFixed(2)+' s')}};$('delete-key').onclick=()=>{const s=scene.shapes.find(s=>s.id===selected);if(!s)return;const k=s.keys.find(k=>Math.abs(k.t-time)<.011);if(!k)return toast('Select a keyframe on the timeline first');if(!removeKeyframe(s,k))return toast('Keep at least one keyframe per shape');refresh()};
function point(e){const svg=$('canvas'),p=new DOMPoint(e.clientX,e.clientY).matrixTransform(svg.getScreenCTM().inverse());return{x:p.x,y:p.y}}
$('canvas').onpointerdown=e=>{
  if(e.button!=null&&e.button!==0)return;
  if(motionMode){const s=scene.shapes.find(s=>s.id===selected);if(!s)return;stop();const p=point(e),startState=editableState(s);motionDraft={type:motionMode,shapeId:s.id,start:{x:startState.x,y:startState.y},end:p,startState,points:motionMode==='path'?[{x:startState.x,y:startState.y},p]:undefined};$('canvas').setPointerCapture(e.pointerId);render();return}
  const resizeHandle=e.target.closest('[data-resize-id]');if(resizeHandle){const s=scene.shapes.find(s=>s.id===resizeHandle.getAttribute('data-resize-id'));if(!s)return;e.preventDefault?.();stop();selected=s.id;selectedMotionId=selectedRotationId=null;motionGuide=null;resizeDrag={id:s.id,handle:resizeHandle.getAttribute('data-resize-handle'),start:point(e),value:editableState(s),angle:state(s).r};$('canvas').setPointerCapture(e.pointerId);refresh();return}
  const handle=e.target.closest('[data-rotate-id]');if(handle){const s=scene.shapes.find(s=>s.id===handle.getAttribute('data-rotate-id'));if(!s)return;stop();selected=s.id;selectedMotionId=selectedRotationId=null;motionGuide=null;const center=state(s),cursor=point(e);rotationDrag={id:s.id,center:{x:center.x,y:center.y},lastAngle:Math.atan2(cursor.y-center.y,cursor.x-center.x)*180/Math.PI};$('canvas').setPointerCapture(e.pointerId);refresh();return}
  const g=e.target.closest('[data-id]');if(!g)return;stop();selected=g.dataset.id;selectedMotionId=selectedRotationId=null;motionGuide=null;const s=scene.shapes.find(s=>s.id===selected);drag={start:point(e),value:editableState(s),id:s.id};$('canvas').setPointerCapture(e.pointerId);refresh()
};
$('canvas').onpointermove=e=>{
  if(rotationDrag){const s=scene.shapes.find(s=>s.id===rotationDrag.id);if(!s)return;const cursor=point(e),angle=Math.atan2(cursor.y-rotationDrag.center.y,cursor.x-rotationDrag.center.x)*180/Math.PI,delta=((angle-rotationDrag.lastAngle+540)%360)-180;s.baseRotation=Math.max(-10000,Math.min(10000,(s.baseRotation||0)+delta));rotationDrag.lastAngle=angle;render();fields();return}
  if(resizeDrag){const s=scene.shapes.find(s=>s.id===resizeDrag.id);if(!s)return;const cursor=point(e),delta={x:cursor.x-resizeDrag.start.x,y:cursor.y-resizeDrag.start.y},pose=resizedPose(resizeDrag.value,resizeDrag.angle,resizeDrag.handle,delta,e.shiftKey);if(s.type==='text'&&s.linkId)setLinkedTextPose(s,pose);else key(s,pose);render();fields();return}
  if(motionDraft){const p=point(e);motionDraft.end=p;if(motionDraft.type==='path'){const lastPoint=motionDraft.points.at(-1);if(Math.hypot(p.x-lastPoint.x,p.y-lastPoint.y)>3)motionDraft.points.push(p)}render();return}
  if(!drag)return;const p=point(e),s=scene.shapes.find(s=>s.id===drag.id),pose={...drag.value,x:drag.value.x+p.x-drag.start.x,y:drag.value.y+p.y-drag.start.y};if(s.type==='text'&&s.linkId)setLinkedTextPose(s,pose);else key(s,pose);render();fields()
};
$('canvas').onpointerup=()=>{if(rotationDrag){rotationDrag=null;fields();recordSceneChange('canvas gesture');return}if(resizeDrag){resizeDrag=null;lists();recordSceneChange('canvas gesture');return}if(motionDraft){finishMotion();return}drag=null;lists();recordSceneChange('canvas gesture')};$('canvas').onpointercancel=()=>{motionDraft=null;rotationDrag=null;resizeDrag=null;drag=null;render();recordSceneChange('canvas gesture')};
function toast(message){$('toast').textContent=message;$('toast').classList.add('visible');clearTimeout(toast.timer);toast.timer=setTimeout(()=>$('toast').classList.remove('visible'),2600)}
function download(blob,name){const u=URL.createObjectURL(blob),a=document.createElement('a');a.href=u;a.download=name;a.click();setTimeout(()=>URL.revokeObjectURL(u),10000)}
$('open').onclick=()=>$('file').click();
function ensureProjectIdentity(){scene.projectId=/^[0-9a-f-]{36}$/i.test(scene.projectId||'')?scene.projectId:crypto.randomUUID();scene.projectName=(typeof scene.projectName==='string'&&scene.projectName.trim()?scene.projectName.trim():'Untitled exploration').slice(0,80)}
function syncProjectName(){ensureProjectIdentity();$('project-name').value=scene.projectName;document.title=`${scene.projectName} — Forma`}
$('project-name').addEventListener('input',()=>{scene.projectName=$('project-name').value.slice(0,80);document.title=`${scene.projectName||'Untitled exploration'} — Forma`});
$('project-name').addEventListener('blur',()=>{scene.projectName=scene.projectName.trim()||'Untitled exploration';syncProjectName();recordSceneChange('rename project')});
const projectDialog=document.createElement('dialog');projectDialog.id='projects-dialog';projectDialog.innerHTML='<form method="dialog"><button class="close" aria-label="Close projects">×</button></form><div class="eyebrow">SAVED ON THIS DEVICE</div><h2>Your projects</h2><p class="muted">Forma saves changes automatically. Choose a project to continue.</p><div id="projects-list"></div><p id="projects-status" role="status"></p>';document.body.append(projectDialog);
$('projects').onclick=async()=>{try{await window.FormaSession.flushAutosave();const projects=await window.formaDesktop.listProjects(),list=$('projects-list');list.replaceChildren();for(const item of projects){const button=document.createElement('button');button.className='project-entry';const title=document.createElement('b'),meta=document.createElement('small');title.textContent=item.name+(item.id===scene.projectId?' · Current':'');meta.textContent=`${item.layerCount} layers · ${new Date(item.savedAt).toLocaleString()}`;button.append(title,meta);button.onclick=async()=>{if(item.id===scene.projectId){projectDialog.close();return}try{await window.FormaSession.flushAutosave();const saved=await window.formaDesktop.openLocalProject(item.id);if(!saved||!restoreProject(saved))throw Error('Project could not be opened');recordSceneChange('switch project');await window.FormaSession.flushAutosave();projectDialog.close();toast('Project opened')}catch(error){$('projects-status').textContent=error.message}};list.append(button)}if(!projects.length)list.textContent='No saved projects yet.';$('projects-status').textContent='';projectDialog.showModal()}catch(error){toast(error.message||'Could not list projects')}};
$('new-project').onclick=async()=>{try{await window.FormaSession.flushAutosave();example();scene.projectId=crypto.randomUUID();scene.projectName='Untitled exploration';syncProjectName();recordSceneChange('new project');await window.FormaSession.flushAutosave();toast('New project created')}catch(error){toast(error.message||'Could not create project')}};
function valid(s){return s&&s.version===1&&Number.isFinite(s.width)&&Number.isFinite(s.height)&&s.width>=100&&s.width<=4000&&s.height>=100&&s.height<=4000&&Number.isFinite(s.duration)&&s.duration>=1&&/^#[0-9a-f]{6}$/i.test(s.bg)&&Array.isArray(s.shapes)&&s.shapes.length<=100&&new Set(s.shapes.map(x=>x.id)).size===s.shapes.length&&s.shapes.every(x=>{const assetOk=x.type==='image'?typeof x.assetData==='string'&&/^data:image\/(png|jpeg|webp);/i.test(x.assetData):x.type==='svg'?typeof x.svgSource==='string':x.type==='video'?typeof x.assetData==='string'&&/^data:video\/(mp4|webm|ogg);/i.test(x.assetData)&&typeof x.posterData==='string'&&x.posterData.startsWith('data:image/')&&Number.isFinite(x.mediaDuration):true,textOk=x.type!=='text'||typeof x.text==='string'&&x.text.length<=120&&typeof x.linkId==='string'&&Number.isFinite(x.offsetX)&&Number.isFinite(x.offsetY)&&Number.isFinite(x.textWidth)&&Number.isFinite(x.fontSize);return typeof x.id==='string'&&typeof x.name==='string'&&x.name.length<100&&['circle','rect','triangle','diamond','arrow','text','image','svg','video'].includes(x.type)&&assetOk&&textOk&&(x.blur===undefined||Number.isFinite(x.blur)&&x.blur>=0&&x.blur<=100)&&(x.baseRotation===undefined||Number.isFinite(x.baseRotation)&&Math.abs(x.baseRotation)<=10000)&&(x.disappear===undefined||x.disappear&&Number.isFinite(x.disappear.startTime)&&x.disappear.startTime>=0&&Number.isFinite(x.disappear.duration)&&x.disappear.duration>=0&&x.disappear.startTime+x.disappear.duration<=s.duration+.001&&['never','time','clip'].includes(x.disappear.reappearMode)&&(x.disappear.reappearMode!=='time'||Number.isFinite(x.disappear.reappearTime)&&x.disappear.reappearTime>=x.disappear.startTime+x.disappear.duration&&x.disappear.reappearTime<=s.duration+.001)&&(x.disappear.reappearMode!=='clip'||typeof x.disappear.triggerShapeId==='string'&&typeof x.disappear.triggerClipId==='string'))&&(x.rotationClips===undefined||Array.isArray(x.rotationClips)&&x.rotationClips.length<=1000&&new Set(x.rotationClips.map(c=>c.id)).size===x.rotationClips.length&&x.rotationClips.every(c=>typeof c.id==='string'&&Number.isFinite(c.startTime)&&c.startTime>=0&&Number.isFinite(c.duration)&&c.duration>0&&c.startTime+c.duration<=s.duration+.001&&Number.isFinite(c.degrees)&&Math.abs(c.degrees)<=10000&&['linear','smooth'].includes(c.ease)))&&['smooth','linear','hold'].includes(x.ease)&&/^#[0-9a-f]{6}$/i.test(x.color)&&Array.isArray(x.keys)&&x.keys.length>0&&x.keys.length<=1000&&x.keys.every(k=>Number.isFinite(k.t)&&k.t>=0&&k.t<=s.duration&&props.every(p=>Number.isFinite(k[p])&&Math.abs(k[p])<=10000)&&k.w>0&&k.h>0&&k.o>=0&&k.o<=100)&&new Set(x.keys.map(k=>k.t)).size===x.keys.length})}
$('file').onchange=async e=>{try{const f=e.target.files[0];if(!f)return;const s=JSON.parse(await f.text());s.width??=960;s.height??=600;if(Array.isArray(s.shapes))for(const shape of s.shapes)if(shape.type==='svg')shape.svgSource=sanitizeSvg(shape.svgSource);if(!valid(s))throw Error();s.shapes.forEach(x=>x.keys.sort((a,b)=>a.t-b.t));await window.FormaSession.flushAutosave();stop();assetCache.clear();scene=s;scene.projectId=crypto.randomUUID();scene.projectName=typeof s.projectName==='string'&&s.projectName.trim()?s.projectName.trim():f.name.replace(/\.[^.]+$/,'');syncProjectName();selected=s.shapes[0]?.id;selectedMotionId=selectedRotationId=null;motionGuide=null;time=0;refresh();recordSceneChange('open project');toast('Project opened')}catch{toast('This file is not a valid Forma project')}e.target.value=''};$('reset').onclick=async()=>{try{const saved=await window.formaDesktop?.loadDefault();if(saved&&restoreProject(saved)){scene.projectId=crypto.randomUUID();syncProjectName();recordSceneChange('load default');toast('Default project loaded');return}}catch{}for(const media of assetCache.values())if(media.tagName==='VIDEO')media.pause();assetCache.clear();example();scene.projectId=crypto.randomUUID();syncProjectName();recordSceneChange('load example')};
function svgText(animated=false){const svg=element('svg',{xmlns:NS,width:scene.width,height:scene.height,viewBox:`0 0 ${scene.width} ${scene.height}`,overflow:'hidden'});const defs=element('defs',{});svg.append(defs);svg.append(element('rect',{width:scene.width,height:scene.height,fill:scene.bg}));for(const s of scene.shapes){const p=state(s,animated?0:time),g=element('g',{fill:s.color});const filter=addBlurFilter(defs,s,p);if(filter)g.setAttribute('filter',filter);const shape=geometry(s,{...p,w:1,h:1},true);if(s.type==='rect'){shape.setAttribute('rx','0.015');shape.setAttribute('ry','0.015')}g.setAttribute('transform',`translate(${p.x} ${p.y})`);const rot=element('g',{transform:`rotate(${p.r})`}),scale=element('g',{transform:`scale(${p.w} ${p.h})`,opacity:p.o/100});scale.append(shape);rot.append(scale);g.append(rot);if(animated){const steps=Math.max(1,Math.min(12000,Math.ceil(scene.duration*60))),values=[];for(let i=0;i<=steps;i++)values.push(state(s,i/steps*scene.duration));for(const [parent,type,fn]of [[g,'translate',p=>`${p.x} ${p.y}`],[rot,'rotate',p=>`${p.r}`],[scale,'scale',p=>`${p.w} ${p.h}`]])parent.append(element('animateTransform',{attributeName:'transform',type,dur:`${scene.duration}s`,repeatCount:'indefinite',values:values.map(fn).join(';'),calcMode:'linear'}));scale.append(element('animate',{attributeName:'opacity',dur:`${scene.duration}s`,repeatCount:'indefinite',values:values.map(p=>p.o/100).join(';')}))}svg.append(g)}return new XMLSerializer().serializeToString(svg)}
function waitForMedia(media){return new Promise((resolve,reject)=>{if(media.tagName==='VIDEO'&&media.readyState>=2||media.tagName==='IMG'&&media.complete)return resolve();const ready=()=>{cleanup();resolve()},fail=()=>{cleanup();reject(Error('Media could not be decoded'))},cleanup=()=>{media.removeEventListener('loadeddata',ready);media.removeEventListener('load',ready);media.removeEventListener('error',fail)};media.addEventListener(media.tagName==='VIDEO'?'loadeddata':'load',ready,{once:true});media.addEventListener('error',fail,{once:true})})}
async function prepareAssets(at=time,playVideos=false){for(const s of scene.shapes)if(isAsset(s)){const media=mediaElement(s);await waitForMedia(media);if(s.type==='video'&&Number.isFinite(media.duration)){const target=at%media.duration;if(Math.abs(media.currentTime-target)>.04)await new Promise(resolve=>{media.addEventListener('seeked',resolve,{once:true});media.currentTime=target});if(playVideos)await media.play().catch(()=>{})}}}
function paint(ctx,t){ctx.fillStyle=scene.bg;ctx.fillRect(0,0,scene.width,scene.height);for(const s of scene.shapes){const p=state(s,t);ctx.save();ctx.translate(p.x,p.y);ctx.rotate(p.r*Math.PI/180);ctx.globalAlpha=p.o/100;ctx.filter=layerBlur(s)?'blur('+layerBlur(s)+'px)':'none';if(isAsset(s)){const media=mediaElement(s);if(media.tagName==='VIDEO'?media.readyState>=2:media.complete)ctx.drawImage(media,-p.w/2,-p.h/2,p.w,p.h)}else if(s.type==='text'){ctx.fillStyle=s.color;ctx.font=`600 ${p.h}px DM Sans, sans-serif`;ctx.textAlign='center';ctx.textBaseline='middle';const measured=Math.max(1,ctx.measureText(s.text||'Text').width);ctx.scale(p.w/measured,1);ctx.fillText(s.text||'Text',0,0)}else{ctx.fillStyle=s.color;ctx.beginPath();if(s.type==='circle')ctx.ellipse(0,0,p.w/2,p.h/2,0,0,Math.PI*2);else if(s.type==='rect')ctx.roundRect(-p.w/2,-p.h/2,p.w,p.h,3);else if(s.type==='arrow'){ctx.moveTo(-p.w/2,-p.h*.18);ctx.lineTo(p.w*.12,-p.h*.18);ctx.lineTo(p.w*.12,-p.h/2);ctx.lineTo(p.w/2,0);ctx.lineTo(p.w*.12,p.h/2);ctx.lineTo(p.w*.12,p.h*.18);ctx.lineTo(-p.w/2,p.h*.18);ctx.closePath()}else{ctx.moveTo(0,-p.h/2);ctx.lineTo(p.w/2,s.type==='diamond'?0:p.h/2);if(s.type==='diamond')ctx.lineTo(0,p.h/2);ctx.lineTo(-p.w/2,s.type==='diamond'?0:p.h/2);ctx.closePath()}ctx.fill()}ctx.restore()}}
$('export').onclick=()=>{$('export-status').textContent='';$('export-dialog').showModal()};$('svg-export').onclick=()=>{download(new Blob([svgText(true)],{type:'image/svg+xml'}),'forma-animation.svg');$('export-status').textContent=scene.shapes.some(s=>s.type==='video')?'Animated SVG exported with poster frames for video layers.':'Animated SVG exported.'};$('png-export').onclick=async()=>{try{await prepareAssets(time);const c=document.createElement('canvas');c.width=scene.width;c.height=scene.height;paint(c.getContext('2d'),time);c.toBlob(b=>{download(b,'forma-frame.png');$('export-status').textContent='Current frame exported.'})}catch{$('export-status').textContent='A media layer could not be prepared for export.'}};
$('video-export').onclick=async()=>{
  if(recording)return;
  if(!window.MediaRecorder||!HTMLCanvasElement.prototype.captureStream){$('export-status').textContent='Video recording is unavailable in this browser.';return}
  stop();recording=true;
  const c=document.createElement('canvas');c.width=scene.width;c.height=scene.height;
  const ctx=c.getContext('2d');let stream,audioContext,audioDestination,recorder;
  const audioSources=[];
  const finish=()=>{
    stream?.getTracks().forEach(track=>track.stop());
    for(const s of scene.shapes)if(s.type==='video')mediaElement(s).pause();
    for(const source of audioSources){source.disconnect(audioDestination);source.connect(audioContext.destination)}
    recording=false;render();
  };
  try{
    await prepareAssets(0);
    paint(ctx,0);stream=c.captureStream(60);
    const videos=scene.shapes.filter(s=>s.type==='video');
    if(videos.length){
      audioContext=sharedAudioContext??=new AudioContext();audioDestination=audioContext.createMediaStreamDestination();
      for(const s of videos){const video=mediaElement(s);let source=audioSourceCache.get(video);if(!source){source=audioContext.createMediaElementSource(video);audioSourceCache.set(video,source)}source.disconnect();source.connect(audioDestination);audioSources.push(source)}
      for(const track of audioDestination.stream.getAudioTracks())stream.addTrack(track);
      await audioContext.resume();
    }
    const mime=['video/webm;codecs=vp9,opus','video/webm;codecs=vp8,opus','video/webm','video/mp4'].find(t=>MediaRecorder.isTypeSupported(t));
    recorder=new MediaRecorder(stream,mime?{mimeType:mime}:{});
    const chunks=[];recorder.ondataavailable=e=>{if(e.data.size)chunks.push(e.data)};
    recorder.onstop=()=>{finish();download(new Blob(chunks,{type:recorder.mimeType}),'forma-animation.'+(recorder.mimeType.includes('mp4')?'mp4':'webm'));$('export-status').textContent=scene.shapes.some(s=>s.type==='video')?'Video exported with imported audio.':'Video exported.'};
    recorder.onerror=()=>{$('export-status').textContent='Could not record the video.';finish()};
    recorder.start();
    await Promise.all(videos.map(s=>mediaElement(s).play()));
    const start=performance.now();
    function frame(now){if(!recording)return;const t=Math.min((now-start)/1000,scene.duration);paint(ctx,t);$('export-status').textContent=`Recording ${t.toFixed(1)} / ${scene.duration} s — keep this tab visible`;if(t<scene.duration)requestAnimationFrame(frame);else recorder.stop()}
    requestAnimationFrame(frame)
  }catch(error){finish();$('export-status').textContent='Could not record video with audio. Check that the imported video plays in Forma.'}
};
example();requestAnimationFrame(tick);




let historyReady=false,lastProjectJson='',undoHistory=[],gestureStart=null,autosaveTimer=null,pendingAutosaveJson=null,saveRevision=0,lastInputSource='',lastInputAt=0,lastGestureScan=0;
const assetVersions=new Map(),assetRegistry=new Map(),savedAssetTokens=new Set(),assetFields=['assetData','posterData','svgSource'];
function packScene(){
  const shapes=scene.shapes.map(shape=>{
    const copy={...shape};
    for(const field of assetFields){
      const value=shape[field];if(typeof value!=='string')continue;
      const slot=shape.id+':'+field,versions=assetVersions.get(slot)||[];
      let entry=versions.find(version=>version.value===value);
      if(!entry){entry={token:crypto.randomUUID(),value};versions.push(entry);assetVersions.set(slot,versions);assetRegistry.set(entry.token,value)}
      copy[field+'Ref']=entry.token;delete copy[field]
    }
    return copy
  });
  return JSON.stringify({...scene,shapes})
}
function unpackScene(snapshot){
  const project=JSON.parse(snapshot);
  for(const shape of project.shapes)for(const field of assetFields){const token=shape[field+'Ref'];if(!token)continue;const value=assetRegistry.get(token);if(value===undefined)throw Error('Undo media is unavailable');shape[field]=value;delete shape[field+'Ref']}
  return project
}
function autosavePayload(projectJson=packScene()){
  const assets=[];
  for(const shape of JSON.parse(projectJson).shapes)for(const field of assetFields){const token=shape[field+'Ref'];if(token&&!savedAssetTokens.has(token))assets.push({token,data:assetRegistry.get(token)})}
  return{revision:++saveRevision,projectJson,assets}
}
function sendAutosave(payload){
  return window.formaDesktop.autosave(payload).then(saved=>{
    if(saved)for(const item of payload.assets)savedAssetTokens.add(item.token);
    if(payload.revision===saveRevision)$('autosave-status').textContent=saved?'Saved locally':'Saving…';
    return saved
  },error=>{$('autosave-status').textContent='Autosave failed';toast(error.message||'Autosave failed');throw error})
}
function pushUndo(snapshot){
  undoHistory.push(snapshot);
  let bytes=undoHistory.reduce((total,item)=>total+item.length,0);
  while(undoHistory.length>30||bytes>180*1024*1024&&undoHistory.length>1)bytes-=undoHistory.shift().length;
}
function scheduleAutosave(projectJson,delay=150){
  if(!window.formaDesktop?.autosave)return;
  pendingAutosaveJson=projectJson;$('autosave-status').textContent='Saving…';clearTimeout(autosaveTimer);
  autosaveTimer=setTimeout(()=>{
    autosaveTimer=null;const payload=autosavePayload(pendingAutosaveJson);pendingAutosaveJson=null;
    sendAutosave(payload).catch(()=>{});
  },delay)
}
function recordSceneChange(source='edit',recordHistory=true){
  if(!historyReady)return false;
  const current=packScene();if(current===lastProjectJson)return false;
  if(recordHistory&&gestureStart===null){
    const now=performance.now(),grouped=source.startsWith('input:')&&source===lastInputSource&&now-lastInputAt<1000;
    if(!grouped)pushUndo(lastProjectJson);
    lastInputSource=source.startsWith('input:')?source:'';lastInputAt=now;
  }
  lastProjectJson=current;scheduleAutosave(current,source.startsWith('input:')||source==='gesture'?150:0);return true
}
$('undo-button').onclick=()=>undoScene();
function undoScene(){
  recordSceneChange('undo preparation');
  const previous=undoHistory.pop();if(!previous)return toast('Nothing to undo');
  stop();closeKeyframeMenu();assetCache.clear();scene=unpackScene(previous);syncProjectName();
  selected=scene.shapes.some(s=>s.id===selected)?selected:scene.shapes[0]?.id;
  selectedMotionId=selectedRotationId=null;motionGuide=motionDraft=motionMode=null;$('stage').classList.remove('motion-mode');
  time=Math.min(time,scene.duration);lastProjectJson=previous;lastInputSource='';gestureStart=null;refresh();scheduleAutosave(previous,0);toast('Undid last change')
}
document.addEventListener('click',()=>queueMicrotask(()=>recordSceneChange('click')));
document.addEventListener('input',e=>{const source='input:'+(e.target.id||e.target.name||e.target.tagName);queueMicrotask(()=>recordSceneChange(source))});
document.addEventListener('change',()=>queueMicrotask(()=>recordSceneChange('change')));
document.addEventListener('pointerdown',e=>{if(e.target.closest?.('#canvas,.motion-bar'))gestureStart=lastProjectJson},true);
document.addEventListener('pointermove',()=>{if(gestureStart!==null&&performance.now()-lastGestureScan>150){lastGestureScan=performance.now();queueMicrotask(()=>recordSceneChange('gesture',false))}});
function endSceneGesture(){if(gestureStart===null)return;recordSceneChange('gesture',false);if(lastProjectJson!==gestureStart)pushUndo(gestureStart);gestureStart=null;lastInputSource=''}
document.addEventListener('pointerup',endSceneGesture);document.addEventListener('pointercancel',endSceneGesture);
document.addEventListener('keydown',e=>{if((e.ctrlKey||e.metaKey)&&!e.shiftKey&&e.key.toLowerCase()==='z'&&!e.target.closest?.('textarea,[contenteditable="true"]')&&!document.body.classList.contains('mcp-working')){e.preventDefault();e.stopImmediatePropagation();undoScene()}},true);
document.addEventListener('visibilitychange',()=>{if(document.hidden)window.FormaSession?.flushAutosave?.().catch(()=>{})});
window.addEventListener('blur',()=>{if(pendingAutosaveJson)window.FormaSession?.flushAutosave?.().catch(()=>{})});
window.FormaSession={snapshot:()=>JSON.parse(JSON.stringify(scene)),autosavePayload,flushAutosave:async()=>{recordSceneChange('flush');clearTimeout(autosaveTimer);autosaveTimer=null;pendingAutosaveJson=null;if(window.formaDesktop?.autosave)await sendAutosave(autosavePayload())}};
function restoreProject(saved){
  if(Array.isArray(saved?.shapes))for(const shape of saved.shapes){
    const refs=assetFields.map(field=>({field,token:shape[field+'Ref'],value:shape[field]}));
    for(const{field}of refs)delete shape[field+'Ref'];
    if(shape.type==='svg')shape.svgSource=sanitizeSvg(shape.svgSource);
    for(const{field,token,value}of refs)if(token&&shape[field]===value){
      const slot=shape.id+':'+field,versions=assetVersions.get(slot)||[];
      if(!versions.some(entry=>entry.token===token))versions.push({token,value});
      assetVersions.set(slot,versions);assetRegistry.set(token,value);savedAssetTokens.add(token)
    }
  }
  if(!valid(saved))return false;
  stop();assetCache.clear();scene=saved;syncProjectName();selected=scene.shapes[0]?.id;selectedMotionId=selectedRotationId=null;motionGuide=null;time=0;refresh();return true
}
window.FormaSession.ready=(window.formaDesktop?window.formaDesktop.loadRecovery().then(saved=>{if(saved&&restoreProject(saved))toast('Previous animation restored')}).catch(()=>toast('Could not restore the previous session')):Promise.resolve()).finally(()=>{ensureProjectIdentity();syncProjectName();lastProjectJson=packScene();historyReady=true;if(!window.formaDesktop)$('autosave-status').textContent='Browser preview only'});
