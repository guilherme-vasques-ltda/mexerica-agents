// Own PIN-native packaging admission. Separate from RuntimePolicy12/Receipt14.
// No disk/env/receipt field can fill this build-owned admission. First close
// the Electron FD-load, exact image/signature, parent walk and restart proofs.
const fs=require('node:fs'),path=require('node:path')
const ADMITTED=null
module.exports=async function personalPinReceipt(context){
  if(context.electronPlatformName!=='darwin')return
  const out=path.join(context.packager.projectDir,'out/main/personal-pin-native-receipt.json')
  if(ADMITTED===null){
    // A stale enabled receipt must never survive an unavailable build.
    const unavailable=Buffer.from(JSON.stringify(['donna/personal-linux/pin-native-unavailable/v1',1]))
    fs.mkdirSync(path.dirname(out),{recursive:true})
    // Only replace a recognized build-owned receipt. Never truncate a link or
    // an unrelated pre-existing file before checking it.
    try{
      const old=fs.openSync(out,fs.constants.O_RDONLY|fs.constants.O_NOFOLLOW)
      try{const st=fs.fstatSync(old);if(!st.isFile() || st.nlink!==1 || st.uid!==process.getuid() || st.size>65536)throw new Error('Unsafe PIN build receipt');const value=JSON.parse(fs.readFileSync(old,'utf8'));if(value[0]!=='donna/personal-linux/pin-native-unavailable/v1')throw new Error('Unknown PIN build receipt')}finally{fs.closeSync(old)}
    }catch(e){if(e.code!=='ENOENT')throw e}
    const tmp=out+'.prepared',fd=fs.openSync(tmp,fs.constants.O_WRONLY|fs.constants.O_CREAT|fs.constants.O_EXCL|fs.constants.O_NOFOLLOW,0o600)
    try{fs.writeFileSync(fd,unavailable);fs.fsyncSync(fd)}finally{fs.closeSync(fd)}
    fs.renameSync(tmp,out)
    return
  }
  throw new Error('PIN native packaging admission has no qualified implementation')
}
