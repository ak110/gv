param(
    [ValidateSet('base', 'candidate')]
    [string]$Variant,
    [ValidateSet('isolated', 'default')]
    [string]$Desktop
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or
    $env:RUNNER_OS -ne 'Windows' -or $env:GITHUB_EVENT_NAME -ne 'push' -or
    $env:GITHUB_REF -cne 'refs/heads/pwi-ae3f-gv-bookmark-probe') {
    throw 'この観測は専用ブランチの使い捨て GitHub Windows runner だけで実行できる。'
}

[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

if ($Variant -and $Desktop) {
    # Windows PowerShell の .NET Framework に付属する UI Automation を使う。
    try {
        Add-Type -AssemblyName System.Drawing, UIAutomationClient, UIAutomationTypes, WindowsBase
        $references = @(
            'System.dll'
            'System.Core.dll'
            [System.Drawing.Bitmap].Assembly.Location
            [System.Windows.Automation.AutomationElement].Assembly.Location
            [System.Windows.Automation.ControlType].Assembly.Location
            [System.Windows.Rect].Assembly.Location
        )
        $source = Get-Content -Raw -Encoding UTF8 -LiteralPath (Join-Path $PSScriptRoot 'bookmark_probe.cs')
        Add-Type -TypeDefinition $source -ReferencedAssemblies $references
        $exe = Join-Path $env:RUNNER_TEMP "bookmark-probe-$Variant/gv.exe"
        $caseDir = Join-Path $env:RUNNER_TEMP "bookmark-probe-$Variant-$Desktop"
        exit [BookmarkProbe]::Run($exe, $caseDir, $Desktop -eq 'isolated')
    } catch {
        [Console]::Error.WriteLine("観測ハーネスの初期化に失敗: $($_.FullyQualifiedErrorId)")
        exit 1
    }
}

function Invoke-BookmarkObservation {
    param([string]$Build, [string]$Mode)

    Write-Information "観測開始: $Build desktop=$Mode" -InformationAction Continue
    $start = [System.Diagnostics.ProcessStartInfo]::new()
    $start.FileName = Join-Path $env:SystemRoot 'System32/WindowsPowerShell/v1.0/powershell.exe'
    $start.UseShellExecute = $false
    foreach ($argument in @('-NoProfile', '-NonInteractive', '-File', $PSCommandPath, '-Variant', $Build, '-Desktop', $Mode)) {
        $start.ArgumentList.Add($argument)
    }
    $worker = [System.Diagnostics.Process]::Start($start)
    try {
        if (-not $worker.WaitForExit(100000)) {
            $worker.Kill($true)
            if (-not $worker.WaitForExit(5000)) { throw '観測プロセスの終了を確認できない。' }
            Write-Information "観測が100秒を超えたためプロセスツリーを終了: $Build desktop=$Mode" -InformationAction Continue
            return 124
        }
        return $worker.ExitCode
    } finally {
        $worker.Dispose()
    }
}

$mode = 'isolated'
$baseResult = Invoke-BookmarkObservation -Build base -Mode $mode
if ($baseResult -eq 20) {
    Write-Information 'base の隔離デスクトップで標準ダイアログに到達できなかったため、両版を runner の既定デスクトップで比較する。' -InformationAction Continue
    $mode = 'default'
    $baseResult = Invoke-BookmarkObservation -Build base -Mode $mode
}
$candidateResult = Invoke-BookmarkObservation -Build candidate -Mode $mode
Write-Information "比較結果: desktop=$mode base=$baseResult candidate=$candidateResult" -InformationAction Continue
if ($baseResult -ne 0 -or $candidateResult -ne 0) {
    throw '両版の標準保存・読み込みを立証できなかった。観測ログを確認してください。'
}
