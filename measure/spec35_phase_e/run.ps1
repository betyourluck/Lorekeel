<#
.SYNOPSIS
  spec 35 Phase E (E-2): 台本プレイを 1 モデルで回し、計器 (play --observe) の jsonl を集める。

.DESCRIPTION
  モデルの設定 (接続先・モデル名・思考の深さ・出力上限) は Lorekeel に登録してあるモデルと同じ値にしてある。
  CLI は OS の資格情報ストアを読まないので、API キーは実行のたびに伏せ字で入力する
  (このプロセスの環境変数にだけ渡し、終わったら元に戻す。ファイルには書かない)。
  grok だけは repo の .env の設定と鍵をそのまま使う。

.EXAMPLE
  ./measure/spec35_phase_e/run.ps1 -Preset muse
  ./measure/spec35_phase_e/run.ps1 -Preset opus -Package friday_lemmon
  ./measure/spec35_phase_e/run.ps1 -Preset dryrun -MaxLines 1   # 課金なしの結線確認
#>
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('muse', 'gpt6', 'opus', 'gemini', 'grok', 'sakura', 'dryrun')]
    [string]$Preset,

    # 回すパッケージ (既定は 3 本すべて)。
    [ValidateSet('all', 'friday_lemmon', 'ui_and_ruka', 'station_of_sand_girl_en')]
    [string]$Package = 'all',

    # 台本の先頭 N 行だけ使う (0 = 全部)。結線確認用。
    [int]$MaxLines = 0,

    # 出力先 (既定は measure/spec35_phase_e/out/<preset>-<日時>)。
    [string]$OutDir = '',

    # パッケージの置き場所 (既定はこの PC の場所)。
    [string]$UiAndRukaPath = (Join-Path $env:APPDATA 'jp.lorekeel.app\packages\ui_and_ruka'),
    [string]$StationPath = 'D:\語り部\station_of_sand_girl_en',

    [int]$Seed = 42,
    [switch]$NoBuild
)

$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$repo = (Resolve-Path (Join-Path $here '..\..')).Path

# Lorekeel の登録モデルと同じ値 (settings.json の kataribe.aiModelProfiles、2026-10-16 時点)。
$presets = @{
    muse   = @{ BaseUrl = 'https://api.meta.ai/v1/'; Model = 'muse-spark-1.3-contributor'; Effort = 'medium'; MaxTokens = '16384' }
    gpt6   = @{ BaseUrl = 'https://api.openai.com/v1/'; Model = 'gpt-6-luna'; Effort = 'medium'; MaxTokens = '160000' }
    opus   = @{ BaseUrl = 'https://api.anthropic.com/v1'; Model = 'claude-opus-5-5'; Effort = ''; MaxTokens = '16000' }
    gemini = @{ BaseUrl = 'https://generativelanguage.googleapis.com/v1beta'; Model = 'gemini-4-argon'; Effort = ''; MaxTokens = '16000' }
    sakura = @{ BaseUrl = 'https://api.ai.sakura.ad.jp/v1/'; Model = 'preview/Qwen3.6-35B-A3B'; Effort = ''; MaxTokens = '' }
    dryrun = @{ BaseUrl = 'http://127.0.0.1:9/v1'; Model = 'dryrun-model'; Effort = ''; MaxTokens = '' }
}

$packages = [ordered]@{
    friday_lemmon           = (Join-Path $repo 'packages\friday_lemmon')
    ui_and_ruka             = $UiAndRukaPath
    station_of_sand_girl_en = $StationPath
}
$targets = if ($Package -eq 'all') { @($packages.Keys) } else { @($Package) }

foreach ($name in $targets) {
    if (-not (Test-Path (Join-Path $packages[$name] 'package.yaml'))) {
        throw "パッケージが見つかりません: $name ($($packages[$name]))"
    }
    if (-not (Test-Path (Join-Path $here "scripts\$name.txt"))) { throw "台本が見つかりません: scripts\$name.txt" }
}

if (-not $NoBuild) {
    Write-Host '[build] cargo build -p harness --bin play'
    Push-Location $repo
    try { cargo build -q -p harness --bin play; if ($LASTEXITCODE -ne 0) { throw 'ビルドに失敗しました' } }
    finally { Pop-Location }
}
$exe = Join-Path $repo 'target\debug\play.exe'

if (-not $OutDir) {
    $OutDir = Join-Path $here ("out\{0}-{1}" -f $Preset, (Get-Date -Format 'yyyyMMdd-HHmmss'))
}
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

# 環境変数: このプロセスにだけ設定し、終わったら元に戻す。
$vars = 'LLM_BASE_URL', 'LLM_MODEL', 'LLM_API_KEY', 'LLM_EFFORT', 'LLM_MAX_TOKENS'
$saved = @{}
foreach ($v in $vars) { $saved[$v] = [Environment]::GetEnvironmentVariable($v, 'Process') }

try {
    if ($Preset -eq 'grok') {
        # repo の .env をそのまま使う (CLI が dotenv で読む)。シェルに残った値で上書きしないよう消しておく。
        foreach ($v in $vars) { [Environment]::SetEnvironmentVariable($v, $null, 'Process') }
        Write-Host '[model] grok: repo の .env の設定と鍵を使います'
    }
    else {
        $p = $presets[$Preset]
        if ($Preset -eq 'dryrun') {
            $key = 'dryrun'
        }
        else {
            $sec = Read-Host -AsSecureString "API キー ($Preset / $($p.Model))"
            $bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($sec)
            try { $key = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr) }
            finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr) }
            if (-not $key) { throw 'API キーが空です' }
        }
        $env:LLM_BASE_URL = $p.BaseUrl
        $env:LLM_MODEL = $p.Model
        $env:LLM_API_KEY = $key
        # 空は「送らない」(= 環境変数ごと消す。.env にも無いので既定になる)。出力上限だけは .env の 4096 が残る。
        if ($p.Effort) { $env:LLM_EFFORT = $p.Effort } else { [Environment]::SetEnvironmentVariable('LLM_EFFORT', $null, 'Process') }
        if ($p.MaxTokens) { $env:LLM_MAX_TOKENS = $p.MaxTokens } else { [Environment]::SetEnvironmentVariable('LLM_MAX_TOKENS', $null, 'Process') }
        Write-Host ("[model] {0} / {1} (effort={2}, max_tokens={3})" -f $p.BaseUrl, $p.Model, $(if ($p.Effort) { $p.Effort } else { '未設定' }), $(if ($p.MaxTokens) { $p.MaxTokens } else { '.env の値' }))
    }

    foreach ($name in $targets) {
        $script = Join-Path $here "scripts\$name.txt"
        if ($MaxLines -gt 0) {
            $cut = Join-Path $OutDir "$name.script.txt"
            Get-Content -Encoding utf8 $script | Select-Object -First $MaxLines | Set-Content -Encoding utf8 $cut
            $script = $cut
        }
        $jsonl = Join-Path $OutDir "$name.jsonl"
        $stdout = Join-Path $OutDir "$name.stdout.txt"
        $stderr = Join-Path $OutDir "$name.stderr.txt"
        $save = Join-Path $OutDir "$name.save.yaml"
        $argLine = "--package `"$($packages[$name])`" --seed $Seed --save `"$save`" --observe `"$jsonl`""
        $t0 = Get-Date
        Write-Host ("[run] {0} ... ({1})" -f $name, $t0.ToString('HH:mm:ss'))
        # 標準入出力はファイルへ直に繋ぐ (PowerShell のパイプを通すと日本語の台本が文字化けしうる)。
        $proc = Start-Process -FilePath $exe -ArgumentList $argLine -WorkingDirectory $repo -NoNewWindow -Wait -PassThru `
            -RedirectStandardInput $script -RedirectStandardOutput $stdout -RedirectStandardError $stderr
        $lines = if (Test-Path $jsonl) { @(Get-Content -Encoding utf8 $jsonl).Count } else { 0 }
        $elapsed = [int]((Get-Date) - $t0).TotalSeconds
        Write-Host ("[done] {0}: exit={1} / jsonl {2} 行 / {3} 秒" -f $name, $proc.ExitCode, $lines, $elapsed)
    }
}
finally {
    foreach ($v in $vars) { [Environment]::SetEnvironmentVariable($v, $saved[$v], 'Process') }
    $key = $null
}

Write-Host ""
Write-Host "[出力] $OutDir"
Write-Host "[集計] python -I measure/spec35_phase_e/report.py `"$OutDir`""
