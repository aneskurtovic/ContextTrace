const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

function names(version, mac, linux = false) {
  const base = `ContextTrace-${version}-windows-x64`;
  const result = [`${base}-setup.exe`, `${base}-setup.exe.sig`, `${base}-portable.zip`, `${base}-cli.zip`];
  if (mac) for (const arch of ['arm64', 'x64']) {
    const p = `ContextTrace-${version}-macos-${arch}`;
    result.push(`${p}.dmg`, `${p}-cli.tar.gz`, `${p}-updater.app.tar.gz`, `${p}-updater.app.tar.gz.sig`);
  }
  if (linux) { const p = `ContextTrace-${version}-linux-x64`; result.push(`${p}.deb`, `${p}.AppImage`, `${p}.AppImage.sig`, `${p}-cli.tar.gz`); }
  return [...result, 'SHA256SUMS.txt', 'latest.json'];
}
function exact(actual, expected, label) {
  if (actual.length !== expected.length || [...actual].sort().some((x, i) => x !== [...expected].sort()[i])) throw Error(`${label}: unexpected or missing entries`);
}
function entries(dir, expected, type) {
  const list = fs.readdirSync(dir, {withFileTypes:true});
  exact(list.map(x => x.name), expected, dir);
  if (list.some(x => type === 'dir' ? !x.isDirectory() || x.isSymbolicLink() : !x.isFile() || x.isSymbolicLink())) throw Error(`${dir}: invalid entry type`);
}
function hash(file) { return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex'); }
// Detect duplicate keys before parsing, including equivalent escaped keys.
function json(text) {
  let i = 0;
  const ws = () => { while (i < text.length && /\s/.test(text[i])) i++; };
  function string() { const start=i++; while (i<text.length) { if (text[i] === '\\') { i+=2; continue; } if (text[i++] === '"') return JSON.parse(text.slice(start,i)); } throw Error('Invalid JSON string'); }
  function value() { ws(); if (text[i] === '{') { i++; ws(); const keys = new Set(); if (text[i] === '}') { i++; return; } while (true) { ws(); if (text[i] !== '"') throw Error('Invalid JSON object'); const key=string(); if (keys.has(key)) throw Error(`Duplicate JSON key: ${key}`); keys.add(key); ws(); if (text[i++] !== ':') throw Error('Invalid JSON object'); value(); ws(); const c=text[i++]; if (c === '}') return; if (c !== ',') throw Error('Invalid JSON object'); } } else if (text[i] === '[') { i++; ws(); if (text[i] === ']') { i++; return; } while (true) { value(); ws(); const c=text[i++]; if (c === ']') return; if (c !== ',') throw Error('Invalid JSON array'); } } else if (text[i] === '"') string(); else { const m=/^(?:true|false|null|-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?)/.exec(text.slice(i)); if (!m) throw Error('Invalid JSON value'); i+=m[0].length; } }
  value(); ws(); if (i !== text.length) throw Error('Invalid JSON suffix'); return JSON.parse(text);
}
function readJson(file) { return json(fs.readFileSync(file,'utf8').replace(/^\uFEFF/,'')); }
function intake(root, stage, tag, commit) {
  if (!/^v\d+\.\d+\.\d+$/.test(tag) || !/^[0-9a-f]{40}$/.test(commit)) throw Error('Invalid tag or commit');
  if (!path.isAbsolute(root) || fs.lstatSync(root).isSymbolicLink() || !fs.lstatSync(root).isDirectory()) throw Error('Intake root must be an absolute directory, not a link');
  entries(root, ['arm64','x64'], 'dir');
  for (const arch of ['arm64','x64']) {
    const dir = path.join(root,arch), p=`ContextTrace-${tag.slice(1)}-macos-${arch}`;
    const files=[`${p}.dmg`,`${p}-cli.tar.gz`,`${p}-updater.app.tar.gz`];
    entries(dir, ['evidence.json',...files], 'file');
    const e=readJson(path.join(dir,'evidence.json'));
    exact(Object.keys(e), ['schema','tag','commit','arch','files'], 'evidence keys');
    if (e.schema !== 1 || e.tag !== tag || e.commit !== commit || e.arch !== arch || !e.files || typeof e.files !== 'object' || Array.isArray(e.files)) throw Error('Evidence does not match tag, commit or architecture');
    exact(Object.keys(e.files), files, 'evidence files');
    for (const f of files) { if (!/^[0-9a-f]{64}$/.test(e.files[f]) || hash(path.join(dir,f)) !== e.files[f]) throw Error(`Invalid checksum: ${f}`); fs.copyFileSync(path.join(dir,f),path.join(stage,f),fs.constants.COPYFILE_EXCL); if (hash(path.join(stage,f)) !== e.files[f]) throw Error(`Copy changed: ${f}`); }
  }
}
function intakeLinux(root, stage, tag, commit) {
  if (!/^v\d+\.\d+\.\d+$/.test(tag) || !/^[0-9a-f]{40}$/.test(commit)) throw Error('Invalid tag or commit');
  if (!path.isAbsolute(root) || fs.lstatSync(root).isSymbolicLink() || !fs.lstatSync(root).isDirectory()) throw Error('Intake root must be an absolute directory, not a link');
  const p=`ContextTrace-${tag.slice(1)}-linux-x64`, files=[`${p}.deb`,`${p}.AppImage`,`${p}-cli.tar.gz`];
  entries(root,['evidence.json',...files],'file');
  const e=readJson(path.join(root,'evidence.json'));
  exact(Object.keys(e),['schema','tag','commit','arch','files'],'evidence keys');
  if (e.schema!==1 || e.tag!==tag || e.commit!==commit || e.arch!=='x64' || !e.files || typeof e.files!=='object' || Array.isArray(e.files)) throw Error('Evidence does not match tag, commit or architecture');
  exact(Object.keys(e.files),files,'evidence files');
  for (const f of files) { if (!/^[0-9a-f]{64}$/.test(e.files[f]) || hash(path.join(root,f))!==e.files[f]) throw Error(`Invalid checksum: ${f}`); fs.copyFileSync(path.join(root,f),path.join(stage,f),fs.constants.COPYFILE_EXCL); if (hash(path.join(stage,f))!==e.files[f]) throw Error(`Copy changed: ${f}`); }
}
function verifySignature(file, sigfile, pubkey) {
  const pubLines=Buffer.from(pubkey,'base64').toString('utf8').trim().split(/\r?\n/);
  const sigLines=Buffer.from(fs.readFileSync(sigfile,'utf8').trim(),'base64').toString('utf8').trim().split(/\r?\n/);
  if (pubLines.length !== 2 || sigLines.length !== 4 || !sigLines[2].startsWith('trusted comment: ')) throw Error('Invalid minisign format');
  const pub=Buffer.from(pubLines[1],'base64'), sig=Buffer.from(sigLines[1],'base64'), global=Buffer.from(sigLines[3],'base64');
  if (pub.length !== 42 || sig.length !== 74 || global.length !== 64 || pub.subarray(0,2).toString() !== 'Ed' || sig.subarray(0,2).toString() !== 'ED' || !pub.subarray(2,10).equals(sig.subarray(2,10))) throw Error('Invalid minisign key or algorithm');
  const key=crypto.createPublicKey({key:Buffer.concat([Buffer.from('302a300506032b6570032100','hex'),pub.subarray(10)]),format:'der',type:'spki'});
  const digest=crypto.createHash('blake2b512').update(fs.readFileSync(file)).digest();
  if (!crypto.verify(null,digest,key,sig.subarray(10)) || !crypto.verify(null,Buffer.concat([sig.subarray(10),Buffer.from(sigLines[2].slice(17))]),key,global)) throw Error(`Invalid updater signature: ${file}`);
}
function platforms(version, mac, linux = false) {
  const result={'windows-x86_64':`ContextTrace-${version}-windows-x64-setup.exe`};
  if (mac) { result['darwin-aarch64']=`ContextTrace-${version}-macos-arm64-updater.app.tar.gz`; result['darwin-x86_64']=`ContextTrace-${version}-macos-x64-updater.app.tar.gz`; }
  if (linux) result['linux-x86_64']=`ContextTrace-${version}-linux-x64.AppImage`;
  return result;
}
function verifyStage(stage, tag, mac, pubkey, linux = false) {
  const version=tag.slice(1), all=names(version,mac,linux); entries(stage,all,'file');
  const lines=fs.readFileSync(path.join(stage,'SHA256SUMS.txt'),'utf8').trim().split(/\r?\n/);
  const expected=all.filter(x=>x!=='SHA256SUMS.txt');
  if (lines.length !== expected.length) throw Error('Unexpected checksum entries');
  for (const f of expected) { const matches=lines.filter(line => line.slice(66) === f && /^[a-f0-9]{64} \*/.test(line)); if (matches.length !== 1 || matches[0].slice(0,64) !== hash(path.join(stage,f))) throw Error(`Invalid checksum: ${f}`); }
  const feed=readJson(path.join(stage,'latest.json')); const p=platforms(version,mac,linux);
  if (feed.version !== version || !feed.platforms) throw Error('Invalid feed version');
  exact(Object.keys(feed.platforms),Object.keys(p),'feed platforms');
  for (const [platform,file] of Object.entries(p)) { const entry=feed.platforms[platform], signature=fs.readFileSync(path.join(stage,`${file}.sig`),'utf8').trim(); if (!entry || entry.url !== `https://github.com/aneskurtovic/ContextTrace/releases/download/${tag}/${file}` || entry.signature !== signature) throw Error(`Invalid feed entry: ${platform}`); verifySignature(path.join(stage,file),path.join(stage,`${file}.sig`),pubkey); }
}
module.exports={names, intake, intakeLinux, verifyStage, verifySignature, platforms, json};
if (require.main === module) {
  try { const [cmd,...args]=process.argv.slice(2); if(cmd==='intake') intake(...args); else if(cmd==='intake-linux') intakeLinux(...args); else if(cmd==='verify') { const [stage,tag,mode,linux]=args; const pubkey=readJson('crates/ct-ui/src-tauri/tauri.conf.json').plugins.updater.pubkey; verifyStage(stage,tag,mode==='1',pubkey,linux==='1'); } else if(cmd==='names') console.log(JSON.stringify(names(args[0],args[1]==='1',args[2]==='1'))); else throw Error('Unknown command'); }
  catch(e) { console.error(e.message); process.exitCode=1; }
}
