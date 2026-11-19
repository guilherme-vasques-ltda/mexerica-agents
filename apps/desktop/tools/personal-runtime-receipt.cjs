// Produtor SIM30/31. Apenas dados oficiais; nunca executa binário baixado.
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto')
let format
let selectedBuild=null,packLock=null
const RELEASE='https://github.com/electron/electron/releases/download/v44.5.1/'
function need(v){if(!v)throw new Error('Não foi possível verificar o runtime deste pacote.')}
function file(file,limit){const fd=fs.openSync(file,fs.constants.O_RDONLY|fs.constants.O_NOFOLLOW);try{const s=fs.fstatSync(fd);need(s.isFile() && s.nlink===1 && s.size<=limit);const b=fs.readFileSync(fd);need(b.length===s.size);return b}finally{fs.closeSync(fd)}}
function fileHash(file,limit){const fd=fs.openSync(file,fs.constants.O_RDONLY|fs.constants.O_NOFOLLOW);try{const s=fs.fstatSync(fd);need(s.isFile() && s.nlink===1 && s.size<=limit);const h=crypto.createHash('sha256'),b=Buffer.alloc(65536);let at=0;for(;;){const n=fs.readSync(fd,b,0,b.length,at);if(!n)break;h.update(b.subarray(0,n));at+=n;need(at<=limit)}need(at===s.size);return h.digest('hex')}finally{fs.closeSync(fd)}}
async function download(initial,to,digest,limit,asset){
  try{
  try{need(fileHash(to,limit)===digest);return}catch(e){if(e.code!=='ENOENT')throw e}
  const permitted=asset?['github.com','objects.githubusercontent.com','release-assets.githubusercontent.com']:['raw.githubusercontent.com']
  let url=new URL(initial),response
  for(let redirects=0;;redirects++){
    need(url.protocol==='https:' && (!url.port || url.port==='443') && !url.username && !url.password && !url.hash && permitted.includes(url.hostname))
    response=await fetch(url,{redirect:'manual',signal:AbortSignal.timeout(30000),cache:'no-store'})
    if([301,302,303,307,308].includes(response.status)){need(redirects<3 && response.headers.get('location'));url=new URL(response.headers.get('location'),url);await response.body?.cancel();continue}break
  }
  need(response.ok && response.body);const tmp=to+'.download',fd=fs.openSync(tmp,fs.constants.O_WRONLY|fs.constants.O_CREAT|fs.constants.O_EXCL|fs.constants.O_NOFOLLOW,0o600),h=crypto.createHash('sha256');let size=0
  try{for await(const chunk of response.body){size+=chunk.length;need(size<=limit);h.update(chunk);let at=0;while(at<chunk.length)at+=fs.writeSync(fd,chunk,at,chunk.length-at)}need(h.digest('hex')===digest);fs.fsyncSync(fd)}finally{fs.closeSync(fd)}
  // Destination is build-owned; never replace an unrelated cache entry.
  fs.linkSync(tmp,to);fs.unlinkSync(tmp)
  }catch{throw new Error('Não foi possível conferir o arquivo oficial do runtime.')}
}
async function extract(zip,dir,links){
  const entries=await require('unzipper').Open.file(zip);need(entries.files.length<=4096);const target=dir+'.extract';fs.mkdirSync(target,{mode:0o700});const seen=new Set(),deferred=[];let total=0
  for(const e of entries.files){const name=e.path;need(typeof name==='string' && !name.includes('\\') && !name.includes('\0') && !name.startsWith('/') && !/^[A-Za-z]:/.test(name) && !name.split('/').some(v=>v==='.' || v==='..') && !seen.has(name));seen.add(name);total+=e.uncompressedSize;need(Number.isSafeInteger(total) && total<=3*1024*1024*1024 && e.uncompressedSize<=1024*1024*1024)
    const to=path.join(target,name),mode=e.externalFileAttributes>>>16;need(!mode || [0,0o040000,0o100000,0o120000].includes(mode&0o170000));fs.mkdirSync(path.dirname(to),{recursive:true,mode:0o700})
    if((mode&0o170000)===0o120000){need(e.uncompressedSize<=4096);const raw=await e.buffer();const expected=links.find(v=>v[0]===name);need(expected && raw.toString('utf8')===expected[1]);deferred.push([to,expected[1]]);continue}
    if(e.type==='Directory'){fs.mkdirSync(to,{recursive:true,mode:0o755});continue}
    const fd=fs.openSync(to,fs.constants.O_CREAT|fs.constants.O_EXCL|fs.constants.O_WRONLY|fs.constants.O_NOFOLLOW,(mode&0o111)?0o755:0o644);let size=0
    try{for await(const chunk of e.stream()){size+=chunk.length;need(size<=e.uncompressedSize);let at=0;while(at<chunk.length)at+=fs.writeSync(fd,chunk,at,chunk.length-at)}need(size===e.uncompressedSize);fs.fsyncSync(fd)}finally{fs.closeSync(fd)}
  }
  need(deferred.length===links.length);for(const [to,link] of deferred){need(path.resolve(path.dirname(to),link).startsWith(path.resolve(target)+path.sep));fs.symlinkSync(link,to)}
  fs.renameSync(target,dir)
}
async function verifyExtracted(zip,dir,links){
  const archive=await require('unzipper').Open.file(zip),expected=new Set(['']);need(archive.files.length<=4096)
  for(const e of archive.files){const name=e.path.replace(/\/$/,'');need(!name.startsWith('/') && !name.includes('\\') && !name.split('/').some(v=>v==='..' || v==='.'));expected.add(name);for(let parent=path.posix.dirname(name);parent!=='.';parent=path.posix.dirname(parent))expected.add(parent)
    const to=path.join(dir,name),st=fs.lstatSync(to),link=links.find(v=>v[0]===e.path)
    if(link){need(st.isSymbolicLink() && fs.readlinkSync(to)===link[1]);continue}if(e.type==='Directory'){need(st.isDirectory());continue}
    need(st.isFile() && !st.isSymbolicLink() && st.size===e.uncompressedSize);const h=crypto.createHash('sha256');let total=0;for await(const chunk of e.stream()){total+=chunk.length;need(total<=e.uncompressedSize);h.update(chunk)}need(total===e.uncompressedSize && fileHash(to,1024*1024*1024)===h.digest('hex'))
  }
  const walk=(relative)=>{for(const name of fs.readdirSync(path.join(dir,relative))){const rel=path.posix.join(relative,name);need(expected.has(rel));if(fs.lstatSync(path.join(dir,rel)).isDirectory())walk(rel)}};walk('')
}
function inspect(file,p,role){const fd=fs.openSync(file,fs.constants.O_RDONLY|fs.constants.O_NOFOLLOW);try{const size=fs.fstatSync(fd).size;return format.image({size,read(at,length){const b=Buffer.alloc(length);need(fs.readSync(fd,b,0,length,at)===length);return b}},p[2],p[3],role,p[11][3])}finally{fs.closeSync(fd)}}
async function verifyDefaultAsar(zip,dist,p){
  const member='Electron.app/Contents/Resources/default_app.asar',archive=await require('unzipper').Open.file(zip),entries=archive.files.filter(e=>e.path===member)
  need(archive.files.length<=4096 && entries.length===1);const entry=entries[0];need(entry.type==='File' && (entry.externalFileAttributes>>>16)===0o100644 && entry.uncompressedSize===110862)
  const chunks=[];let total=0
  for await(const chunk of entry.stream()){total+=chunk.length;need(total<=110862);chunks.push(chunk)}need(total===110862)
  const raw=Buffer.concat(chunks);format.asar.defaultAsar({size:raw.length,read(at,length){need(at>=0 && length>=0 && at+length<=raw.length);return raw.subarray(at,at+length)}},p)
  const name=path.join(dist,member),fd=fs.openSync(name,fs.constants.O_RDONLY|fs.constants.O_NOFOLLOW)
  try{
    const first=fs.fstatSync(fd,{bigint:true}),identity=s=>[s.dev,s.ino,s.size,s.mode,s.uid,s.nlink,s.mtimeNs,s.ctimeNs].join(':')
    need(first.isFile() && first.nlink===1n && !(first.mode&0o022n) && (first.uid===0n || first.uid===BigInt(process.getuid())))
    format.asar.defaultAsar({size:Number(first.size),read(at,length){const b=Buffer.alloc(length);need(fs.readSync(fd,b,0,length,at)===length);return b}},p)
    const current=fs.lstatSync(name,{bigint:true});need(!current.isSymbolicLink() && identity(first)===identity(current) && identity(first)===identity(fs.fstatSync(fd,{bigint:true})))
  }finally{fs.closeSync(fd)}
}
module.exports=async function personalRuntimeReceipt(context){
  format=(await import(require('node:url').pathToFileURL(path.join(__dirname,'../src/main/fork-cloud/personal-runtime-format.mjs')).href)).default
  const project=context.packager.projectDir,platform=context.electronPlatformName,arch=require('builder-util').Arch[context.arch]
  if(!['linux','darwin'].includes(platform))return
  need(['arm64','x64'].includes(arch) && context.packager.info.framework.version==='44.5.1');const variant=project+':'+platform+':'+arch;need(selectedBuild===null || selectedBuild===variant);selectedBuild=variant
  const policies=JSON.parse(file(path.join(project,'resources/personal-runtime-policy-v1.json'),256*1024)),p=format.policy(policies.find(v=>v[2]===platform && v[3]===arch)),inputs=JSON.parse(file(path.join(project,'resources/personal-runtime-build-inputs-v1.json'),256*1024))
  need(inputs.schema===1 && format.hash(file(path.join(project,'package-lock.json'),4*1024*1024))===p[11][1])
  need(format.hash(Buffer.from(JSON.stringify(inputs.builderFiles.map(v=>[v.path,v.sha256]))))===p[11][2])
  for(const f of inputs.builderFiles){need(f.path.startsWith('node_modules/') && !f.path.includes('..') && fileHash(path.join(project,f.path),256*1024)===f.sha256)}
  need(JSON.stringify(inputs.sourceFiles.map(v=>[v.url,v.sha256]))===JSON.stringify(p[10]))
  need(context.packager.config.electronFuses==null && !context.packager.config.downloadAlternateFFmpeg)
  const expectedDist=path.join(project,'build/personal-runtime',`electron-44.5.1-${platform}-${arch}`);need(context.packager.config.electronDist==null || path.resolve(project,context.packager.config.electronDist)===expectedDist)
  const root=path.join(project,'build/personal-runtime'),cache=path.join(root,'cache');fs.mkdirSync(cache,{recursive:true,mode:0o700})
  if(packLock===null){const lock=path.join(root,'pack.lock'),fd=fs.openSync(lock,fs.constants.O_WRONLY|fs.constants.O_CREAT|fs.constants.O_EXCL|fs.constants.O_NOFOLLOW,0o600),identity=fs.fstatSync(fd);packLock={lock,fd};process.once('exit',()=>{try{const now=fs.lstatSync(lock);fs.closeSync(fd);if(now.dev===identity.dev && now.ino===identity.ino)fs.unlinkSync(lock)}catch{}})}
  let total=0
  for(const f of inputs.sourceFiles){total+=f.bytes;need(f.bytes<=256*1024 && total<=4*1024*1024);await download(f.url,path.join(cache,f.sha256+'.source'),f.sha256,256*1024,false)}
  const patches=inputs.sourceFiles.find(v=>v.url.endsWith('/patches/node/.patches'));need(patches);need(file(path.join(cache,patches.sha256+'.source'),256*1024).toString().split('\n').filter(v=>v && !v.startsWith('#')).join('\n')===inputs.patchOrder.join('\n'))
  const sums=path.join(cache,'SHASUMS256.txt');await download(RELEASE+'SHASUMS256.txt',sums,p[7],65536,true);format.assetSum(file(sums,65536),p[8],p[9])
  const zip=path.join(cache,p[8]);await download(RELEASE+p[8],zip,p[9],1024*1024*1024,true)
  const dist=path.join(root,`electron-44.5.1-${platform}-${arch}`),linkEntry=inputs.archiveLinks.find(v=>v[0]===p[8]);need(linkEntry)
  let existing=false;try{need(fs.lstatSync(dist).isDirectory() && !fs.lstatSync(dist).isSymbolicLink());existing=true}catch(e){if(e.code!=='ENOENT')throw e}
  if(existing)await verifyExtracted(zip,dist,linkEntry[1]);else await extract(zip,dist,linkEntry[1]);const images=platform==='linux'?[inspect(path.join(dist,'electron'),p,'executable')]:[inspect(path.join(dist,'Electron.app/Contents/MacOS/Electron'),p,'executable'),inspect(path.join(dist,'Electron.app/Contents/Frameworks/Electron Framework.framework/Versions/A/Electron Framework'),p,'framework')]
  if(platform==='darwin')await verifyDefaultAsar(zip,dist,p)
  const receipt=['donna/personal-linux/runtime-receipt/v1',1,platform,arch,'44.5.1','24.21.0','1.52.1',p[5],p[6],format.hash(Buffer.from(JSON.stringify(p[10]))),p[8],p[9],images,format.hash(Buffer.from(JSON.stringify(p)))];format.receipt(receipt,p)
  const out=path.join(project,'out/main');fs.mkdirSync(out,{recursive:true});const fileOut=path.join(out,'personal-runtime-receipt.json'),raw=Buffer.from(JSON.stringify(receipt));need(raw.length<=65536);fs.writeFileSync(fileOut,raw,{mode:0o600});const fd=fs.openSync(fileOut,'r');try{fs.fsyncSync(fd)}finally{fs.closeSync(fd)}
  context.packager.config.electronDist=dist
}
