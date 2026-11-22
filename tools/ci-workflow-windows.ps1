param([switch]$AsStandardUser)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$root = $PSScriptRoot
$admins = [Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')
$principal = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())

function Get-BuildReport($records, $source, $cargoHome, $tracked) {
    $origins = [Collections.Generic.HashSet[string]]::new()
    $lines = [Collections.Generic.List[string]]::new()
    $source = $source.Replace('\', '/').TrimEnd('/') + '/'
    $registry = $cargoHome.Replace('\', '/').TrimEnd('/') + '/registry/src/'
    foreach ($record in $records) {
        if ($record.Length -gt 1048576) { $null = $origins.Add('unknown'); continue }
        try { $item = ConvertFrom-Json -InputObject $record -AsHashtable -Depth 32 -ErrorAction Stop } catch { $null = $origins.Add('unknown'); continue }
        if ($item -isnot [Collections.IDictionary] -or $item.reason -cne 'compiler-message' -or $item.message.level -cne 'error') { continue }
        $origin = 'unknown'
        $manifest = ([string]$item.manifest_path).Replace('\', '/')
        if ($manifest -ieq ($source + 'host/Cargo.toml') -or $manifest -ieq ($source + 'host/crates/memory/Cargo.toml')) {
            $origin = 'workspace'
        } elseif ($manifest.StartsWith($registry, [StringComparison]::OrdinalIgnoreCase) -and $manifest.Substring($registry.Length) -cmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/Cargo\.toml$' -and $manifest -notmatch '/\.{1,2}/' -and $item.package_id -cmatch '^registry\+') {
            $origin = 'dependency'
        }
        $null = $origins.Add($origin)
        if ($lines.Count -ge 40) { continue }
        $code = [string]$item.message.code.code
        $lines.Add($(if ($code -cmatch '\AE[0-9]{4}\z') { "error[$code]" } else { 'error: compilation failed' }))
        $span = $item.message.spans | Where-Object { $_.is_primary -eq $true } | Select-Object -First 1
        if ($null -eq $span -or $lines.Count -ge 40) { continue }
        $path = '<externo>'
        if ($origin -ceq 'workspace') {
            $file = ([string]$span.file_name).Replace('\', '/')
            $member = $manifest.Substring($source.Length) -replace 'Cargo\.toml$', ''
            $candidates = if ($file.StartsWith($source, [StringComparison]::OrdinalIgnoreCase)) { $file.Substring($source.Length) } else { $member + $file; $file; 'host/' + $file }
            foreach ($candidate in $candidates) {
                if ($candidate -cmatch '\Ahost/(?:[A-Za-z0-9_-]+/)*[A-Za-z0-9_.-]+\.rs\z' -and $tracked -ccontains $candidate) { $path = $candidate; break }
            }
        }
        $line = [string]$span.line_start
        $column = [string]$span.column_start
        if ($line -cmatch '\A[1-9][0-9]{0,8}\z' -and $column -cmatch '\A[1-9][0-9]{0,8}\z') {
            $location = '--> {0}:{1}:{2}' -f $path, $line, $column
            if ($location.Length -le 300) { $lines.Add($location) }
        }
    }
    if ($origins.Count -eq 1 -and $origins.Contains('workspace')) { 'BUILD_FAILED_WORKSPACE' }
    elseif ($origins.Count -eq 1 -and $origins.Contains('dependency')) { 'BUILD_FAILED_DEPENDENCY' }
    else { 'BUILD_FAILED' }
    $lines
}

if ($AsStandardUser) {
    $expectedSid = Get-Content -Raw -LiteralPath (Join-Path $root 'user.sid')
    if ($principal.Identity.User.Value -cne $expectedSid.Trim() -or $principal.IsInRole($admins)) {
        throw 'The proof must run as the temporary standard user without elevation.'
    }
    $work = Join-Path $root 'work'
    $env:TEMP = $env:TMP = Join-Path $work 'temp'
    $env:CARGO_HOME = Join-Path $work 'cargo'
    $env:CARGO_TARGET_DIR = Join-Path $work 'target'
    $env:CARGO_NET_OFFLINE = 'true'
    $env:CARGO_TERM_COLOR = 'never'
    $vs = (Get-Content -Raw -LiteralPath (Join-Path $root 'vs.txt')).Trim()
    Import-Module (Join-Path $vs 'Common7/Tools/Microsoft.VisualStudio.DevShell.dll')
    Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64' | Out-Null
    $rustBin = (Get-Content -Raw -LiteralPath (Join-Path $root 'rust-bin.txt')).Trim()
    $env:PATH = "$rustBin;$env:PATH"
    $env:RUSTC = Join-Path $rustBin 'rustc.exe'
    Set-Location -LiteralPath (Join-Path $root 'source')
    $module = 'fork::motor_mexerica::windows_laboratorio::'
    $testArgs = @('--locked', '--target', 'x86_64-pc-windows-msvc', '--manifest-path', 'host/Cargo.toml', '-p', 'codync-host', '--bin', 'donnabot-host', $module, '--', '--test-threads=1')
    $buildLog = Join-Path $work 'build.jsonl'
    try {
        cargo test --no-run --message-format=json @testArgs > $buildLog 2> $null
        if ($LASTEXITCODE -ne 0) { throw 'Build failed.' }
    } catch {
        try {
            $tracked = Get-Content -LiteralPath (Join-Path $root 'tracked-rust.txt')
            Get-BuildReport (Get-Content -LiteralPath $buildLog) (Join-Path $root 'source') $env:CARGO_HOME $tracked |
                Set-Content -LiteralPath (Join-Path $work 'build-report.txt')
        } catch {
            Set-Content -LiteralPath (Join-Path $work 'build-report.txt') -Value 'BUILD_FAILED'
        }
        exit 20
    }
    try {
        $listed = @(cargo test @testArgs --list 2> $null | Where-Object { $_ -cne '' } | Sort-Object -CaseSensitive)
        if ($LASTEXITCODE -ne 0) { exit 22 }
        $expected = @('catalog_child', 'simulacao_nao_readmite_com_termino_indeterminado', 'windows_nativo_catalogo_e_recusa_antes_de_retomar', 'windows_nativo_identidade_da_imagem_recusa_copia_ou_hash_errado' | ForEach-Object { "${module}${_}: test" })
        $expected = @($expected + '4 tests, 0 benchmarks' | Sort-Object -CaseSensitive)
        if (($listed -join "`n") -cne ($expected -join "`n")) { exit 22 }
    } catch {
        exit 22
    }
    try {
        cargo test @testArgs > (Join-Path $work 'test.log') 2> $null
        if ($LASTEXITCODE -ne 0) { exit 21 }
    } catch {
        exit 21
    }
    exit 0
}

if (-not $IsWindows -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted' -or -not $principal.IsInRole($admins)) {
    throw 'Bootstrapping a standard user requires a GitHub-hosted Windows VM.'
}

$root = Join-Path $env:ProgramData ('windows-proof-' + [guid]::NewGuid().ToString('N'))
$user = $null
$password = [Security.SecureString]::new()
$result = 1
try {
    $rootAcl = [Security.AccessControl.DirectorySecurity]::new()
    $rootAcl.SetAccessRuleProtection($true, $false)
    foreach ($sid in @($principal.Identity.User, [Security.Principal.SecurityIdentifier]::new('S-1-5-18'))) {
        $rootAcl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($sid, 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow'))
    }
    New-Item -ItemType Directory -Path $root | Out-Null
    Set-Acl -LiteralPath $root -AclObject $rootAcl
    $source = (New-Item -ItemType Directory -Path (Join-Path $root 'source')).FullName
    $work = (New-Item -ItemType Directory -Path (Join-Path $root 'work')).FullName
    New-Item -ItemType Directory -Path (Join-Path $work 'temp') | Out-Null

    $archive = Join-Path $root 'source.tar'
    git archive --format=tar --output=$archive HEAD
    & "$env:SystemRoot\System32\tar.exe" -xf $archive -C $source
    Remove-Item -LiteralPath $archive
    Copy-Item -LiteralPath $PSCommandPath -Destination (Join-Path $root 'proof.ps1')
    git ls-files -- 'host/*.rs' | Set-Content -LiteralPath (Join-Path $root 'tracked-rust.txt')
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    $vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $vs) { throw 'Visual Studio C++ tools are required.' }
    Set-Content -LiteralPath (Join-Path $root 'vs.txt') -Value $vs

    $env:RUSTUP_HOME = Join-Path $root 'rustup'
    $env:CARGO_HOME = Join-Path $work 'cargo'
    $pin = @(Get-Content -LiteralPath 'rust-toolchain.toml' | Select-String '^channel = "([0-9]+\.[0-9]+\.[0-9]+)"$')
    if ($pin.Count -ne 1) { throw 'Expected a single pinned Rust toolchain.' }
    $toolchain = $pin[0].Matches[0].Groups[1].Value + '-x86_64-pc-windows-msvc'
    rustup toolchain install $toolchain --profile minimal
    $rustBin = Split-Path -Parent (rustup which --toolchain $toolchain cargo)
    Set-Content -LiteralPath (Join-Path $root 'rust-bin.txt') -Value $rustBin
    & (Join-Path $rustBin 'cargo.exe') fetch --locked --target x86_64-pc-windows-msvc --manifest-path (Join-Path $source 'host/Cargo.toml')

    foreach ($char in 'aA9!'.ToCharArray()) { $password.AppendChar($char) }
    $alphabet = 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789'
    for ($i = 0; $i -lt 32; $i++) {
        $password.AppendChar($alphabet[[Security.Cryptography.RandomNumberGenerator]::GetInt32($alphabet.Length)])
    }
    $password.MakeReadOnly()
    $name = 'proof-' + [guid]::NewGuid().ToString('N').Substring(0, 12)
    $user = New-LocalUser -Name $name -Password $password
    $users = Get-LocalGroup -SID 'S-1-5-32-545'
    if (-not (Get-LocalGroupMember -Group $users | Where-Object SID -EQ $user.SID)) {
        Add-LocalGroupMember -Group $users -Member $user
    }
    if (Get-LocalGroupMember -SID $admins | Where-Object SID -EQ $user.SID) {
        throw 'The proof account must not be an administrator.'
    }
    Set-Content -LiteralPath (Join-Path $root 'user.sid') -Value $user.SID.Value

    $rootAcl = Get-Acl -LiteralPath $root
    $rootAcl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($user.SID, 'ReadAndExecute', 'ContainerInherit,ObjectInherit', 'None', 'Allow'))
    Set-Acl -LiteralPath $root -AclObject $rootAcl
    $workAcl = Get-Acl -LiteralPath $work
    $workAcl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($user.SID, 'Modify', 'ContainerInherit,ObjectInherit', 'None', 'Allow'))
    Set-Acl -LiteralPath $work -AclObject $workAcl

    $credential = [pscredential]::new(".\$name", $password)
    $child = Start-Process -FilePath (Join-Path $PSHOME 'pwsh.exe') -Credential $credential -UseNewEnvironment -LoadUserProfile -WorkingDirectory $root -ArgumentList '-NoProfile', '-NonInteractive', '-File', ('"{0}"' -f (Join-Path $root 'proof.ps1')), '-AsStandardUser' -PassThru -Wait
    switch ($child.ExitCode) {
        20 {
            $report = Get-Item -LiteralPath (Join-Path $work 'build-report.txt') -ErrorAction SilentlyContinue
            if ($report -and -not ($report.Attributes -band [IO.FileAttributes]::ReparsePoint) -and $report.Length -le 13000) {
                Get-Content -LiteralPath $report.FullName -TotalCount 41
            }
            $result = 20
        }
        21 { $result = 21 }
        22 { $result = 22 }
        0 { Get-Content -LiteralPath (Join-Path $work 'test.log'); $result = 0 }
        default { throw 'The non-elevated proof process did not report a known result.' }
    }
} finally {
    $password.Dispose()
    try {
        if ($null -ne $user) {
            try {
                Get-CimInstance -ClassName Win32_UserProfile -Filter "SID='$($user.SID.Value)'" | Remove-CimInstance
            } finally {
                Remove-LocalUser -SID $user.SID
            }
        }
    } finally {
        if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
    }
}
exit $result
