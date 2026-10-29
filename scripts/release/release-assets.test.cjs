const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');
const {spawnSync} = require('node:child_process');
const {intake, intakeLinux, names, verifyStage, json} = require('./release-assets.cjs');
const tmp = () => fs.mkdtempSync(path.join(os.tmpdir(), 'ct-release-'));
const hash = f => crypto.createHash('sha256').update(fs.readFileSync(f)).digest('hex');
const cli = path.resolve('crates/ct-ui/node_modules/@tauri-apps/cli/tauri.js');
function tauri(args, env={}) { const r=spawnSync(process.execPath,[cli,...args],{encoding:'utf8',env:{...process.env,...env}}); assert.equal(r.status,0,r.stderr + r.stdout); }
function fixture() {
 const root=tmp(), stage=tmp(), tag='v1.2.3', commit='a'.repeat(40);
 for (const arch of ['arm64','x64']) { const dir=path.join(root,arch); fs.mkdirSync(dir); const prefix=`ContextTrace-1.2.3-macos-${arch}`; const files={}; for (const n of [`${prefix}.dmg`,`${prefix}-cli.tar.gz`,`${prefix}-updater.app.tar.gz`]) { fs.writeFileSync(path.join(dir,n),n); files[n]=hash(path.join(dir,n)); } fs.writeFileSync(path.join(dir,'evidence.json'),JSON.stringify({schema:1,tag,commit,arch,files})); }
 return {root,stage,tag,commit};
}
test('intake accepts complete exact evidence and rejects malformed or partial input', () => {
 const f=fixture(); intake(f.root,f.stage,f.tag,f.commit); assert.equal(fs.readdirSync(f.stage).length,6);
 assert.throws(()=>intake(f.root,tmp(),f.tag,'b'.repeat(40)));
 fs.writeFileSync(path.join(f.root,'arm64','extra'),'bad'); assert.throws(()=>intake(f.root,tmp(),f.tag,f.commit)); fs.unlinkSync(path.join(f.root,'arm64','extra'));
 const evidence=path.join(f.root,'arm64','evidence.json'); const original=fs.readFileSync(evidence,'utf8'); fs.writeFileSync(evidence,original.replace('"schema":1','"schema":1,"schema":1')); assert.throws(()=>intake(f.root,tmp(),f.tag,f.commit)); fs.writeFileSync(evidence,original);
 const obj=JSON.parse(original); obj.files['../bad']='0'.repeat(64); fs.writeFileSync(evidence,JSON.stringify(obj)); assert.throws(()=>intake(f.root,tmp(),f.tag,f.commit));
 fs.writeFileSync(evidence,original); fs.appendFileSync(path.join(f.root,'arm64',Object.keys(JSON.parse(original).files)[0]),'changed'); assert.throws(()=>intake(f.root,tmp(),f.tag,f.commit));
 fs.rmSync(path.join(f.root,'x64'),{recursive:true}); assert.throws(()=>intake(f.root,tmp(),f.tag,f.commit));
 assert.throws(()=>json('{"x":1,"\\u0078":2}'));
});
test('Tauri-generated signatures verify in Windows-only and macOS feeds and tampering fails', () => {
 const f=fixture(), key=path.join(tmp(),'key'); tauri(['signer','generate','--ci','--password','','--write-keys',key]);
 const pub=fs.readFileSync(key+'.pub','utf8').trim(); const privateKey=fs.readFileSync(key,'utf8');
 for (const [mac,linux] of [[false,false],[true,false],[false,true],[true,true]]) {
  const stage=tmp(), tag=f.tag, version='1.2.3', all=names(version,mac,linux); if(mac) intake(f.root,stage,tag,f.commit);
  const platforms={'windows-x86_64':`ContextTrace-${version}-windows-x64-setup.exe`}; if(mac) { platforms['darwin-aarch64']=`ContextTrace-${version}-macos-arm64-updater.app.tar.gz`; platforms['darwin-x86_64']=`ContextTrace-${version}-macos-x64-updater.app.tar.gz`; }
  if(linux) platforms['linux-x86_64']=`ContextTrace-${version}-linux-x64.AppImage`;
  for(const n of all) if(!fs.existsSync(path.join(stage,n)) && !n.endsWith('.sig') && n!=='latest.json' && n!=='SHA256SUMS.txt') fs.writeFileSync(path.join(stage,n),n);
  const feed={version,platforms:{}}; for(const [platform,n] of Object.entries(platforms)) { tauri(['signer','sign',path.join(stage,n)],{TAURI_SIGNING_PRIVATE_KEY:privateKey,TAURI_SIGNING_PRIVATE_KEY_PASSWORD:''}); feed.platforms[platform]={url:`https://github.com/aneskurtovic/ContextTrace/releases/download/${tag}/${n}`,signature:fs.readFileSync(path.join(stage,n+'.sig'),'utf8').trim()}; }
  fs.writeFileSync(path.join(stage,'latest.json'),JSON.stringify(feed)); fs.writeFileSync(path.join(stage,'SHA256SUMS.txt'),all.filter(n=>n!=='SHA256SUMS.txt').map(n=>`${hash(path.join(stage,n))} *${n}`).join('\n'));
  verifyStage(stage,tag,mac,pub,linux); assert.throws(()=>verifyStage(stage,tag,!mac,pub,linux));
  feed.platforms['windows-x86_64'].url += '?wrong'; fs.writeFileSync(path.join(stage,'latest.json'),JSON.stringify(feed)); fs.writeFileSync(path.join(stage,'SHA256SUMS.txt'),all.filter(n=>n!=='SHA256SUMS.txt').map(n=>`${hash(path.join(stage,n))} *${n}`).join('\n')); assert.throws(()=>verifyStage(stage,tag,mac,pub,linux));
  feed.platforms['windows-x86_64'].url = feed.platforms['windows-x86_64'].url.replace('?wrong',''); fs.writeFileSync(path.join(stage,'latest.json'),JSON.stringify(feed));
  fs.appendFileSync(path.join(stage,platforms['windows-x86_64']),'bad'); fs.writeFileSync(path.join(stage,'SHA256SUMS.txt'),all.filter(n=>n!=='SHA256SUMS.txt').map(n=>`${hash(path.join(stage,n))} *${n}`).join('\n')); assert.throws(()=>verifyStage(stage,tag,mac,pub,linux));
 }
});

test('Linux intake binds exact files to the tag and commit', () => {
 const root=tmp(), stage=tmp(), tag='v1.2.3', commit='a'.repeat(40), prefix='ContextTrace-1.2.3-linux-x64', files={};
 for (const name of [`${prefix}.deb`,`${prefix}.AppImage`,`${prefix}-cli.tar.gz`]) { fs.writeFileSync(path.join(root,name),name); files[name]=hash(path.join(root,name)); }
 const evidence=path.join(root,'evidence.json'); fs.writeFileSync(evidence,JSON.stringify({schema:1,tag,commit,arch:'x64',files}));
 intakeLinux(root,stage,tag,commit); assert.equal(fs.readdirSync(stage).length,3);
 assert.throws(()=>intakeLinux(root,tmp(),tag,'b'.repeat(40)));
 fs.appendFileSync(path.join(root,`${prefix}.deb`),'bad'); assert.throws(()=>intakeLinux(root,tmp(),tag,commit));
});
