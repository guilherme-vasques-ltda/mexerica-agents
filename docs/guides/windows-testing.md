# Windows testing

The app ships an x64 Windows build (Electron + the bundled `donnabot-host.exe`). Real Windows behavior is
checked in a UTM virtual machine on the Mac. Everyday end-to-end checks stay on the Mac (minimal
e2e); use the VM for a full e2e, or when a change or a bug depends on Windows itself (paths,
`cmd.exe`, `.cmd` shims, the service).

> Procedimento histórico do upstream: além do rename dos binários, o Windows não foi
> adaptado nem testado nesta cópia. Use como referência, não como validação.

## The VM

- **UTM com uma VM Windows 11 ARM64** (o build x64 roda pela emulação Prism),
  8 GB / 4 cores / 64 GB NVMe, conta local com entrada automática. Use nome e credenciais locais seus.
- **Start / stop** from the Mac, never through UTM's window:
  `osascript -e 'tell application "UTM" to start virtual machine named "<sua-vm>"'`
  (`stop … by force` only when a normal stop hangs; `query ip of …` works). `utmctl` fails with
  OSStatus -10004. Pressing UTM's Run button through accessibility crashed UTM once; if UTM is in a
  bad state, quit it and reopen it in the background with `open -g -a UTM`.
- **Shell**: `ssh <alias-da-vm>` (PowerShell). A `~/.ssh/config` alias points at the VM's address with
  a dedicated key.
- **GUI apps** must start in the signed-in session (session 2):
  `schtasks /create /tn X /tr <exe> /sc once /st 23:59 /it /f; schtasks /run /tn X`.
- **Screen** without touching the Mac's UI: QEMU VNC on `127.0.0.1:5905`, e.g.
  `uvx --from vncdotool vncdo --force-caps -s 127.0.0.1::5905 capture x.png`. Input lags a few
  seconds; `--force-caps` is needed for symbols.
- The Mac sleeping pauses the VM: wrap long installs in `caffeinate -i`.
- Rebuilding the VM: keep the unattended install assets in a local folder (e.g. `~/VMs/windows/`, about 20
  minutes). NVMe must be the first drive and both ISOs detached afterwards, or Windows reinstalls on
  every boot.

## Testing a build

1. Install the release under test (`donnabot-windows-x64.exe` from the GitHub release). The first
   launch asks Defender Firewall about `donnabot-host`; the host then starts at sign-in.
2. **Turn analytics off on the test host before anything else**, so the VM never counts as a user:
   `setAnalytics {"enabled": false}`.
3. Drive the host over SSH instead of the GUI where possible. The API is loopback-only, which SSH
   satisfies:

   ```powershell
   $t = Get-Content "$env:USERPROFILE\.donnabot\token"
   Invoke-RestMethod -Method Post -Uri http://127.0.0.1:19422/api/refreshBackends `
     -Headers @{ Authorization = "Bearer $t" } -ContentType application/json -Body '{}'
   ```

   `createBot`, `send` and `history` cover most agent checks; a failed start shows up as an error
   notice in `history`. Host logs are under `%USERPROFILE%\.donnabot`.
4. Agents: Settings → AI accounts includes Motores no Hub for supported harnesses and their
   existing installation/login steps. Codex and Claude Code use the accounts above that section.
   Extensions no longer has an Agents section. Adapters published on npm need Node.js; the
   existing assisted setup sheet in Settings and Host setup service
   can put its pinned copy in `%USERPROFILE%\.donnabot\node` (see
   [agent setup](../features/marketplace.md)). To test the computer's own Node instead,
   `winget install OpenJS.NodeJS`.

## Limits

- An unreleased host can't be built on the Mac for Windows (its C dependencies need a MinGW
  toolchain that isn't installed). CI builds Windows only for pull requests and tags
  (`.github/workflows/host.yml`), so a fix is usually reproduced by hand in the VM first (running
  the exact command the host would) and checked on Windows again with the next release.
- `cfg(windows)` code can still be compile-checked on the Mac by copying it into a scratch crate and
  running clippy with the pinned toolchain's `--target x86_64-pc-windows-gnu`.

Investigations done this way: npm agents failing to start on Windows, 2026-10-14.
