[CmdletBinding()]
param(
    [ValidateSet("Start", "Stop", "Status")]
    [string]$Action = "Start",
    [switch]$NoBuild,
    [int]$StartupTimeoutSeconds = 45,
    [int]$ShutdownTimeoutSeconds = 12
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$runDir = Join-Path $root ".run"
$statePath = Join-Path $runDir "local-cluster.json"
$targetDir = Join-Path $root "target\debug"
$protoc = Join-Path $root "..\deps-rust\tools\protoc.exe"
$smokeBinary = Join-Path $targetDir "xkk-cluster-smoke.exe"
$script:started = @()
$internalServicePorts = @(3101, 3301)
$expectedRegistrationKeys = @(
    "/local/1/1",
    "/local/2/1",
    "/local/3/1",
    "/local/4/1",
    "/local/5/1"
)

$services = @(
    [pscustomobject]@{
        Name = "public"
        DisplayName = "Public"
        Binary = "xkk-public.exe"
        Config = "public.yaml"
        TcpPorts = @(3301)
        UdpPorts = @()
        RequiredLinks = 2
    },
    [pscustomobject]@{
        Name = "logic"
        DisplayName = "Logic"
        Binary = "xkk-logic.exe"
        Config = "logic.yaml"
        TcpPorts = @(3101)
        UdpPorts = @()
        RequiredLinks = 2
    },
    [pscustomobject]@{
        Name = "gate"
        DisplayName = "Gate"
        Binary = "xkk-gate.exe"
        Config = "gate.yaml"
        TcpPorts = @(3201, 3203)
        UdpPorts = @(3202)
        RequiredLinks = 2
    },
    [pscustomobject]@{
        Name = "query"
        DisplayName = "Query"
        Binary = "xkk-query.exe"
        Config = "query.yaml"
        TcpPorts = @(3401)
        UdpPorts = @()
        RequiredLinks = 0
    },
    [pscustomobject]@{
        Name = "auth"
        DisplayName = "Auth"
        Binary = "xkk-auth.exe"
        Config = "auth.yaml"
        TcpPorts = @(3501)
        UdpPorts = @()
        RequiredLinks = 0
    }
)

function Test-TcpPort {
    param([int]$Port)

    $client = [System.Net.Sockets.TcpClient]::new()
    try {
        $connect = $client.ConnectAsync("127.0.0.1", $Port)
        return $connect.Wait(300) -and $client.Connected
    }
    catch {
        return $false
    }
    finally {
        $client.Dispose()
    }
}

function Test-UdpPortInUse {
    param([int]$Port)

    if (-not (Get-Command Get-NetUDPEndpoint -ErrorAction SilentlyContinue)) {
        return $false
    }
    return $null -ne (Get-NetUDPEndpoint -LocalPort $Port -ErrorAction SilentlyContinue)
}

function Read-Log {
    param([string]$Path)

    if (-not (Test-Path -LiteralPath $Path)) {
        return ""
    }
    return [string](Get-Content -LiteralPath $Path -Raw -ErrorAction SilentlyContinue)
}

function Count-LogMatches {
    param(
        [string]$Path,
        [string]$Pattern
    )

    return [regex]::Matches([string](Read-Log $Path), $Pattern).Count
}

function Count-ServiceConnections {
    param([int]$ProcessId)

    return @(Get-NetTCPConnection `
        -State Established `
        -OwningProcess $ProcessId `
        -ErrorAction SilentlyContinue | Where-Object {
            $_.LocalPort -in $internalServicePorts -or
            $_.RemotePort -in $internalServicePorts
        }).Count
}

function Get-StateProcesses {
    if (-not (Test-Path -LiteralPath $statePath)) {
        return @()
    }
    $state = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
    return @($state.processes)
}

function Stop-ProcessEntries {
    param([array]$Entries)

    foreach ($entry in $Entries) {
        $pidValue = if ($entry.PSObject.Properties.Name -contains "Pid") {
            [int]$entry.Pid
        }
        else {
            [int]$entry.Process.Id
        }
        if (Get-Process -Id $pidValue -ErrorAction SilentlyContinue) {
            Stop-Process -Id $pidValue -Force -ErrorAction SilentlyContinue
        }
    }
}

function Send-ConsoleCtrlC {
    param([int]$ProcessId)

    if (-not ("XkkConsoleSignal" -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class XkkConsoleSignal
{
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool FreeConsole();

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool AttachConsole(uint processId);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool SetConsoleCtrlHandler(IntPtr handler, bool add);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool GenerateConsoleCtrlEvent(uint ctrlEvent, uint processGroupId);
}
'@
    }

    $null = [XkkConsoleSignal]::FreeConsole()
    if (-not [XkkConsoleSignal]::AttachConsole([uint32]$ProcessId)) {
        $null = [XkkConsoleSignal]::AttachConsole([uint32]::MaxValue)
        return $false
    }

    try {
        $null = [XkkConsoleSignal]::SetConsoleCtrlHandler([IntPtr]::Zero, $true)
        return [XkkConsoleSignal]::GenerateConsoleCtrlEvent(0, 0)
    }
    finally {
        Start-Sleep -Milliseconds 100
        $null = [XkkConsoleSignal]::FreeConsole()
        $null = [XkkConsoleSignal]::AttachConsole([uint32]::MaxValue)
        $null = [XkkConsoleSignal]::SetConsoleCtrlHandler([IntPtr]::Zero, $false)
    }
}

function Stop-ProcessEntriesGracefully {
    param([array]$Entries)

    $processIds = @($Entries | ForEach-Object {
        if ($_.PSObject.Properties.Name -contains "Pid") {
            [int]$_.Pid
        }
        else {
            [int]$_.Process.Id
        }
    } | Where-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue })

    for ($index = $processIds.Count - 1; $index -ge 0; $index--) {
        $null = Send-ConsoleCtrlC $processIds[$index]
    }

    $deadline = [DateTime]::UtcNow.AddSeconds($ShutdownTimeoutSeconds)
    do {
        $remaining = @($processIds | Where-Object {
            Get-Process -Id $_ -ErrorAction SilentlyContinue
        })
        if ($remaining.Count -eq 0) {
            return 0
        }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)

    foreach ($processId in $remaining) {
        Stop-Process -Id $processId -Force -ErrorAction SilentlyContinue
    }
    return $remaining.Count
}

function Assert-StartedProcesses {
    foreach ($entry in $script:started) {
        if (-not (Get-Process -Id $entry.Process.Id -ErrorAction SilentlyContinue)) {
            $stdout = (Get-Content -LiteralPath $entry.Stdout -Tail 30 -ErrorAction SilentlyContinue) -join [Environment]::NewLine
            $stderr = (Get-Content -LiteralPath $entry.Stderr -Tail 30 -ErrorAction SilentlyContinue) -join [Environment]::NewLine
            throw "$($entry.Name) exited during startup.`nstdout:`n$stdout`nstderr:`n$stderr"
        }
    }
}

function Wait-For {
    param(
        [scriptblock]$Condition,
        [string]$Description
    )

    $deadline = [DateTime]::UtcNow.AddSeconds($StartupTimeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        Assert-StartedProcesses
        if (& $Condition) {
            return
        }
        Start-Sleep -Milliseconds 200
    }
    throw "timed out waiting for $Description"
}

function Start-ServiceProcess {
    param([pscustomobject]$Service)

    $binary = Join-Path $targetDir $Service.Binary
    $config = Join-Path $root ("config\" + $Service.Config)
    if (-not (Test-Path -LiteralPath $binary)) {
        throw "missing service binary: $binary"
    }
    if (-not (Test-Path -LiteralPath $config)) {
        throw "missing service config: $config"
    }

    $stdout = Join-Path $runDir ($Service.Name + ".stdout.log")
    $stderr = Join-Path $runDir ($Service.Name + ".stderr.log")
    Remove-Item -LiteralPath $stdout, $stderr -Force -ErrorAction SilentlyContinue
    $process = Start-Process `
        -FilePath $binary `
        -ArgumentList @("--config", ('"{0}"' -f $config)) `
        -WorkingDirectory $root `
        -RedirectStandardOutput $stdout `
        -RedirectStandardError $stderr `
        -WindowStyle Hidden `
        -PassThru

    $entry = [pscustomobject]@{
        Name = $Service.Name
        Process = $process
        Stdout = $stdout
        Stderr = $stderr
    }
    $script:started += $entry
    return $entry
}

function Test-HttpReady {
    param([int]$Port)

    try {
        $response = Invoke-WebRequest -Uri "http://127.0.0.1:$Port/readyz" -TimeoutSec 1
        return $response.StatusCode -eq 200
    }
    catch {
        return $false
    }
}

function Test-QueryReady {
    return Test-HttpReady 3401
}

function Test-AuthReady {
    return Test-HttpReady 3501
}

function Get-EtcdctlPath {
    $command = Get-Command etcdctl -ErrorAction SilentlyContinue
    if ($null -ne $command) {
        return $command.Source
    }

    $process = Get-Process -Name etcd -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -eq $process -or [string]::IsNullOrWhiteSpace($process.Path)) {
        return $null
    }
    $path = Join-Path (Split-Path -Parent $process.Path) "etcdctl.exe"
    if (Test-Path -LiteralPath $path) {
        return $path
    }
    return $null
}

function Get-ServiceRegistrationKeys {
    $etcdctl = Get-EtcdctlPath
    if ($null -eq $etcdctl) {
        return @()
    }

    $keys = @(& $etcdctl --endpoints=http://127.0.0.1:2379 get /local/ --prefix --keys-only 2>$null)
    if ($LASTEXITCODE -ne 0) {
        return @()
    }
    return @($keys | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
}

function Test-ServiceRegistrations {
    param([bool]$Present)

    $keys = @(Get-ServiceRegistrationKeys)
    $matching = @($expectedRegistrationKeys | Where-Object { $_ -in $keys }).Count
    if ($Present) {
        return $matching -eq $expectedRegistrationKeys.Count
    }
    return $matching -eq 0
}

function Invoke-JsonPost {
    param(
        [string]$Uri,
        [hashtable]$Body
    )

    return Invoke-RestMethod `
        -Method Post `
        -Uri $Uri `
        -ContentType "application/json" `
        -Body ($Body | ConvertTo-Json -Depth 8 -Compress) `
        -TimeoutSec 5
}

function Assert-OkResponse {
    param(
        [object]$Response,
        [string]$Operation
    )

    if ($null -eq $Response.status -or [int]$Response.status.code -ne 0) {
        $detail = $Response | ConvertTo-Json -Depth 8 -Compress
        throw "$Operation failed: $detail"
    }
}

function Invoke-BusinessSmoke {
    if (-not (Test-Path -LiteralPath $smokeBinary)) {
        throw "missing cluster smoke binary: $smokeBinary"
    }

    $suffix = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    $account = "cluster-smoke-$suffix"
    $deviceId = "cluster-smoke-device-$suffix"
    $credential = "local-smoke-credential"
    $login = Invoke-JsonPost "http://127.0.0.1:3501/v1/auth/login" @{
        account = $account
        credential = $credential
        device = @{
            device_id = $deviceId
            platform = "windows"
            client_version = "cluster-smoke"
        }
    }
    Assert-OkResponse $login "Auth login"
    $roles = @($login.roles)
    if ($roles.Count -ne 1 -or [int64]$roles[0].gid -le 0 -or [string]::IsNullOrWhiteSpace($login.token)) {
        throw "Auth login returned an invalid role or token"
    }

    $gid = [int64]$roles[0].gid
    $admission = Invoke-JsonPost "http://127.0.0.1:3501/v1/auth/use-role" @{
        gid = $gid
        token = [string]$login.token
        device_id = $deviceId
    }
    Assert-OkResponse $admission "Auth use-role"
    $endpoints = @($admission.endpoints)
    $tcp = @($endpoints | Where-Object { $_.transport -eq "tcp" })
    if ($endpoints.Count -ne 3 -or $tcp.Count -ne 1) {
        throw "Auth use-role did not publish the configured TCP, KCP, and WebSocket endpoints"
    }

    $gateAddress = "{0}:{1}" -f $tcp[0].host, $tcp[0].port
    $smokeOutput = @(& $smokeBinary $gateAddress ([string]$gid) ([string]$login.token) $deviceId 2>&1)
    if ($LASTEXITCODE -ne 0) {
        throw "Gate business smoke failed:`n$($smokeOutput -join [Environment]::NewLine)"
    }

    $gamer = Invoke-JsonPost "http://127.0.0.1:3401/v1/query/gamers" @{
        gamer_ids = @($gid)
    }
    Assert-OkResponse $gamer "Query gamer"
    $players = @($gamer.players)
    if ($players.Count -ne 1 -or [int64]$players[0].gid -ne $gid) {
        throw "Query gamer did not return the smoke player"
    }

    $key = Invoke-JsonPost "http://127.0.0.1:3401/v1/query/config/key" @{
        version = "local"
    }
    Assert-OkResponse $key "Query config key"
    if ([string]::IsNullOrWhiteSpace($key.key)) {
        throw "Query config key is empty"
    }

    $manifest = Invoke-JsonPost "http://127.0.0.1:3401/v1/query/config/manifest" @{
        version = ""
    }
    Assert-OkResponse $manifest "Query config manifest"
    if ([string]::IsNullOrWhiteSpace($manifest.version)) {
        throw "Query config manifest version is empty"
    }

    return [pscustomobject]@{
        Account = $account
        Gid = $gid
    }
}

function Assert-ShutdownDrained {
    $gateMatches = @(Select-String -LiteralPath (Join-Path $runDir "gate.stdout.log") -Pattern "Gate client sessions drained")
    $logicMatches = @(Select-String -LiteralPath (Join-Path $runDir "logic.stdout.log") -Pattern "Logic Runtime drained")
    if ($gateMatches.Count -eq 0 -or $logicMatches.Count -eq 0) {
        throw "shutdown drain summaries are missing"
    }
    $gateLine = [string]$gateMatches[-1].Line
    $logicLine = [string]$logicMatches[-1].Line
    foreach ($field in @(
        '"online_players": 0',
        '"retained_players": 0',
        '"outbox_messages": 0',
        '"connection_workers": 0',
        '"rpc_pending": 0',
        '"rpc_inbound_active": 0'
    )) {
        if ($gateLine -notlike "*$field*") {
            throw "Gate shutdown did not drain $field"
        }
    }
    foreach ($field in @(
        '"logic_inflight": 0',
        '"logic_queued": 0',
        '"logic_active_gids": 0',
        '"dirty_players": 0',
        '"rpc_pending": 0',
        '"rpc_inbound_active": 0'
    )) {
        if ($logicLine -notlike "*$field*") {
            throw "Logic shutdown did not drain $field"
        }
    }
}

function Show-Status {
    $entries = @(Get-StateProcesses)
    if ($entries.Count -eq 0) {
        throw "local cluster is not running"
    }

    $linksHealthy = $true
    $rows = @(foreach ($entry in $entries) {
        $alive = $null -ne (Get-Process -Id ([int]$entry.Pid) -ErrorAction SilentlyContinue)
        $definitions = @($services | Where-Object Name -eq $entry.Name)
        if ($definitions.Count -ne 1) {
            throw "unknown service in local cluster state: $($entry.Name)"
        }
        $definition = $definitions[0]
        $links = if ($definition.RequiredLinks -gt 0) {
            Count-ServiceConnections ([int]$entry.Pid)
        }
        else {
            0
        }
        if ($links -lt $definition.RequiredLinks) {
            $linksHealthy = $false
        }
        [pscustomobject]@{
            Service = $entry.Name
            Pid = [int]$entry.Pid
            Alive = $alive
            Links = $links
        }
    })
    $rows | Format-Table -AutoSize
    $dead = @($rows | Where-Object { -not $_.Alive })
    $authReady = Test-AuthReady
    $queryReady = Test-QueryReady
    $registrationsReady = Test-ServiceRegistrations $true
    "Auth ready: $authReady"
    "Query ready: $queryReady"
    "Etcd registrations: $registrationsReady"

    if (
        $dead.Count -ne 0 -or
        -not $linksHealthy -or
        -not $authReady -or
        -not $queryReady -or
        -not $registrationsReady
    ) {
        throw "local cluster is not healthy"
    }
}

if ($StartupTimeoutSeconds -le 0) {
    throw "StartupTimeoutSeconds must be positive"
}
if ($ShutdownTimeoutSeconds -le 0) {
    throw "ShutdownTimeoutSeconds must be positive"
}

New-Item -ItemType Directory -Path $runDir -Force | Out-Null

if ($Action -eq "Stop") {
    $entries = @(Get-StateProcesses)
    $forcedCount = Stop-ProcessEntriesGracefully $entries
    Remove-Item -LiteralPath $statePath -Force -ErrorAction SilentlyContinue
    if ($forcedCount -ne 0) {
        throw "local cluster required force-stopping $forcedCount process(es) after timeout"
    }

    $registrationDeadline = [DateTime]::UtcNow.AddSeconds(3)
    while (-not (Test-ServiceRegistrations $false) -and [DateTime]::UtcNow -lt $registrationDeadline) {
        Start-Sleep -Milliseconds 100
    }
    if (-not (Test-ServiceRegistrations $false)) {
        throw "local cluster stopped but etcd registrations remain"
    }
    Assert-ShutdownDrained
    "Local cluster stopped gracefully; etcd registrations removed."
    return
}

if ($Action -eq "Status") {
    Show-Status
    return
}

$existing = @(Get-StateProcesses)
$aliveExisting = @($existing | Where-Object {
    Get-Process -Id ([int]$_.Pid) -ErrorAction SilentlyContinue
})
if ($aliveExisting.Count -ne 0) {
    throw "local cluster is already running; use -Action Status or -Action Stop"
}
Remove-Item -LiteralPath $statePath -Force -ErrorAction SilentlyContinue

foreach ($dependency in @(
    [pscustomobject]@{ Name = "etcd"; Port = 2379 },
    [pscustomobject]@{ Name = "MongoDB"; Port = 27017 },
    [pscustomobject]@{ Name = "Redis"; Port = 6379 }
)) {
    if (-not (Test-TcpPort $dependency.Port)) {
        throw "$($dependency.Name) is not reachable on 127.0.0.1:$($dependency.Port)"
    }
}
if ($null -eq (Get-EtcdctlPath)) {
    throw "etcdctl.exe is required beside the running etcd.exe or on PATH"
}

foreach ($service in $services) {
    foreach ($port in $service.TcpPorts) {
        if (Test-TcpPort $port) {
            throw "TCP port $port is already in use"
        }
    }
    foreach ($port in $service.UdpPorts) {
        if (Test-UdpPortInUse $port) {
            throw "UDP port $port is already in use"
        }
    }
}

if (-not $NoBuild) {
    if (-not (Test-Path -LiteralPath $protoc)) {
        throw "missing protoc: $protoc"
    }
    $env:PROTOC = $protoc
    Push-Location $root
    try {
        & cargo build --workspace --bins
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }
}

try {
    foreach ($service in $services) {
        $entry = Start-ServiceProcess $service
        foreach ($port in $service.TcpPorts) {
            Wait-For { Test-TcpPort $port } "$($service.DisplayName) TCP port $port"
        }
        Wait-For {
            (Read-Log $entry.Stdout) -like "*$($service.DisplayName) service started*"
        } "$($service.DisplayName) registration"
    }

    Wait-For { Test-QueryReady } "Query readiness"
    Wait-For { Test-AuthReady } "Auth readiness"
    Wait-For { Test-ServiceRegistrations $true } "five etcd service registrations"
    foreach ($service in $services.Where({ $_.RequiredLinks -gt 0 })) {
        $log = Join-Path $runDir ($service.Name + ".stdout.log")
        Wait-For {
            (Count-LogMatches $log "xservice link ready") -ge $service.RequiredLinks
        } "$($service.DisplayName) service links"
    }
    foreach ($service in $services) {
        $log = Join-Path $runDir ($service.Name + ".stdout.log")
        if ((Count-LogMatches $log "xservice hello rejected") -ne 0) {
            throw "$($service.DisplayName) rejected an internal Hello during startup"
        }
    }

    $smoke = Invoke-BusinessSmoke

    $state = [pscustomobject]@{
        StartedAt = [DateTime]::UtcNow.ToString("O")
        SmokeAccount = $smoke.Account
        SmokeGid = $smoke.Gid
        Processes = @($script:started | ForEach-Object {
            [pscustomobject]@{
                Name = $_.Name
                Pid = $_.Process.Id
            }
        })
    }
    $state | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath $statePath

    $script:started | ForEach-Object {
        [pscustomobject]@{
            Service = $_.Name
            Pid = $_.Process.Id
        }
    } | Format-Table -AutoSize
    "Business smoke passed. Account: $($smoke.Account), gid: $($smoke.Gid)"
    "Local cluster ready. Auth: http://127.0.0.1:3501/readyz"
    "Local cluster ready. Query: http://127.0.0.1:3401/readyz"
    "Runtime files: $runDir"
}
catch {
    Stop-ProcessEntries $script:started
    Remove-Item -LiteralPath $statePath -Force -ErrorAction SilentlyContinue
    throw
}
