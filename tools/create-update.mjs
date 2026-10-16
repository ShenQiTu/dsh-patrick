// Publisher-side only. The private key is an explicit file OUTSIDE this repository.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {execFileSync} from 'node:child_process';

const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const options=Object.fromEntries(process.argv.slice(2).reduce((a,v,i,all)=>i%2?a:[...a,[v,all[i+1]]],[]));
if(Object.keys(options).some(k=>!['--input','--output','--key','--version'].includes(k))||Object.values(options).some(v=>!v))throw Error('Use --input DIR --output NEW_DIR --key PRIVATE_PEM [--version VERSION]');
const version=options['--version']||JSON.parse(fs.readFileSync(path.join(root,'package.json'),'utf8')).version;
if(!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version))throw Error('Stable semantic version required');
for(const arg of ['--input','--output','--key'])if(!options[arg])throw Error('Missing '+arg);
const input=fs.realpathSync(options['--input']),out=path.resolve(options['--output']),keyPath=fs.realpathSync(options['--key']);
if(!path.relative(root,keyPath).startsWith('..'+path.sep)&&path.relative(root,keyPath)!=='..')throw Error('Private signing key must stay outside the source repository');
if(fs.existsSync(out))throw Error('Output directory must be new; existing releases are not overwritten');
const privateKey=crypto.createPrivateKey(fs.readFileSync(keyPath));
const pub=Buffer.from(crypto.createPublicKey(privateKey).export({format:'jwk'}).x,'base64url');
if(pub.toString('base64')!==fs.readFileSync(path.join(root,'src-tauri/update-public-key.txt'),'utf8').trim())throw Error('Signing key does not match the application public key');
const files=['DSH Patrick.exe','runtime/node.exe','THIRD_PARTY_NOTICES.md'];
for(const name of files){const stat=fs.lstatSync(path.join(input,name));if(!stat.isFile()||stat.isSymbolicLink())throw Error('Expected regular component: '+name);}
const actual=execFileSync('powershell.exe',['-NoProfile','-NonInteractive','-Command','[Diagnostics.FileVersionInfo]::GetVersionInfo($env:DSH_PATRICK_VERSION_EXE).ProductVersion'],{encoding:'utf8',windowsHide:true,env:{...process.env,DSH_PATRICK_VERSION_EXE:path.join(input,files[0])}}).trim();
if(actual!==version)throw Error('EXE product version must match manifest: '+actual+' != '+version);
const nodeVersion=execFileSync(path.join(input,'runtime/node.exe'),['--version'],{encoding:'utf8',windowsHide:true,timeout:10000}).trim();
if(!/^v\d+\.\d+\.\d+$/.test(nodeVersion))throw Error('The bundled runtime is not Node.js');
const assets=files.map((name,i)=>({path:name,asset:i===0?`DSH-Patrick-v${version}-windows-x64.exe`:i===1?'patrick-node-windows-x64.exe':name}));
const payload={schema:1,version,platform:'windows-x86_64',dataSchema:1,files:assets.map(f=>{
 const bytes=fs.readFileSync(path.join(input,f.path));
 return {path:f.path,url:`https://github.com/ShenQiTu/dsh-patrick/releases/download/v${version}/${f.asset}`,size:bytes.length,sha256:crypto.createHash('sha256').update(bytes).digest('hex')};
})};
const bytes=Buffer.from(JSON.stringify(payload));const signature=crypto.sign(null,bytes,privateKey);
if(!crypto.verify(null,bytes,crypto.createPublicKey(privateKey),signature))throw Error('Signing self-check failed');
fs.mkdirSync(out,{recursive:true});
for(const asset of assets)fs.copyFileSync(path.join(input,asset.path),path.join(out,asset.asset),fs.constants.COPYFILE_EXCL);
fs.writeFileSync(path.join(out,'portable-latest.json'),JSON.stringify({payload:bytes.toString('base64'),signature:signature.toString('base64')},null,2)+'\n',{flag:'wx'});
// Build the ordinary download ZIP from the same three files, never from app-data.
const pack=path.join(out,`DSH Patrick v${version} portable`),archive=`DSH-Patrick-v${version}-portable.zip`;
fs.mkdirSync(path.join(pack,'runtime'),{recursive:true});
for(const f of files)fs.copyFileSync(path.join(input,f),path.join(pack,f),fs.constants.COPYFILE_EXCL);
execFileSync('powershell.exe',['-NoProfile','-NonInteractive','-Command',"$ErrorActionPreference='Stop'; Compress-Archive -LiteralPath $env:PATRICK_PACK_DIR -DestinationPath $env:PATRICK_PACK_ZIP -CompressionLevel Optimal"],{windowsHide:true,env:{...process.env,PATRICK_PACK_DIR:pack,PATRICK_PACK_ZIP:path.join(out,archive)},timeout:180000});
for(const f of files)fs.unlinkSync(path.join(pack,f));fs.rmdirSync(path.join(pack,'runtime'));fs.rmdirSync(pack);
const hashes=[...assets.map(a=>a.asset),'portable-latest.json',archive].map(n=>crypto.createHash('sha256').update(fs.readFileSync(path.join(out,n))).digest('hex')+'  '+n).join('\n')+'\n';
fs.writeFileSync(path.join(out,'SHA256SUMS.txt'),hashes,{flag:'wx'});
console.log('SIGNED_RELEASE_ASSETS_PASS version='+version+' components=3 portable_zip=YES private_key_copied=NO uploaded=NO');
