<#
.SYNOPSIS
  spec 38 Phase B: `play build` を 1 モデルで 2 つの依頼に回し、測定の記録 (--report の JSON) を集める。

.DESCRIPTION
  依頼は 2 つ (requests/*.txt):
    r1_skeleton  — ウィザードが作る骨格 (package.yaml + scenarios/main.yaml) から短い盤面を組む
    r2_lakeside  — 同梱の湖畔の洋館にキャラクターと場所を足す
  どちらも出力フォルダの中に写した**写し**に対して回し、--write は付けない (元のパッケージには書かない)。

  モデルの設定は spec 35 の run.ps1 と同じ (Lorekeel の登録モデルと同じ値)。CLI は OS の資格情報ストアを
  読まないので、API キーは実行のたびに伏せ字で入力する (このプロセスの環境変数にだけ渡し、終わったら
  元に戻す。ファイルには書かない)。grok だけは repo の .env の設定と鍵をそのまま使う。

.EXAMPLE
  ./measure/spec38_phase_b/run.ps1 -Preset dryrun   # 課金なしの結線確認 (計画の呼び出しで接続に失敗する)
  ./measure/spec38_phase_b/run.ps1 -Preset muse
  ./measure/spec38_phase_b/run.ps1 -Preset gemini -Request r1_skeleton
#>
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('muse', 'gpt6', 'opus', 'gemini', 'grok', 'sakura', 'dryrun')]
    [string]$Preset,

    [ValidateSet('all', 'r1_skeleton', 'r2_lakeside')]
    [string]$Request = 'all',

    # 出力先 (既定は measure/spec38_phase_b/out/<preset>-<日時>)。
    [string]$OutDir = '',

    [switch]$NoBuild
)

$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$repo = (Resolve-Path (Join-Path $here '..\..')).Path

# Lorekeel の登録モデルと同じ値 (spec 35 の run.ps1 と同じ、2026-10-16 時点)。
# 編集 (EDITOR_LLM_*) は思考の深さを継がないので Effort は使わない (spec 29 決定 10 / profile_overrides)。
$presets = @{
    muse   = @{ BaseUrl = 'https://api.meta.ai/v1/'; Model = 'muse-spark-1.3-contributor'; MaxTokens = '16384' }
    gpt6   = @{ BaseUrl = 'https://api.openai.com/v1/'; Model = 'gpt-6-luna'; MaxTokens = '160000' }
    opus   = @{ BaseUrl = 'https://api.anthropic.com/v1'; Model = 'claude-opus-5-5'; MaxTokens = '16000' }
    gemini = @{ BaseUrl = 'https://generativelanguage.googleapis.com/v1beta'; Model = 'gemini-4-argon'; MaxTokens = '16000' }
    sakura = @{ BaseUrl = 'https://api.ai.sakura.ad.jp/v1/'; Model = 'preview/Qwen3.6-35B-A3B'; MaxTokens = '' }
    dryrun = @{ BaseUrl = 'http://127.0.0.1:9/v1'; Model = 'dryrun-model'; MaxTokens = '' }
}

$targets = if ($Request -eq 'all') { @('r1_skeleton', 'r2_lakeside') } else { @($Request) }

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

# 依頼ごとの出発点 (出力フォルダの中に作る写し)。
function New-Skeleton([string]$dir) {
    # ウィザード (harness::editor::create_package) が書く骨格と同じ内容。
    New-Item -ItemType Directory -Force -Path (Join-Path $dir 'scenarios') | Out-Null
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    $manifest = "title: 霧の灯台`ndescription: `"`"`nauthor: `"`"`nversion: `"0.1`"`nentry: scenarios/main.yaml`nworld: `"`"`nplayer:`n  name: `"`"`n  profile: `"`"`n"
    $scenario = "title: `"`"`nstart: start`ngoal: { kind: flag_is, key: goal_flag, value: true }`nallowed_flags: [goal_flag]`nlocations:`n  start:`n    description: `"`"`n"
    [IO.File]::WriteAllText((Join-Path $dir 'package.yaml'), $manifest, $utf8)
    [IO.File]::WriteAllText((Join-Path $dir 'scenarios\main.yaml'), $scenario, $utf8)
}

$vars = 'LLM_BASE_URL', 'LLM_MODEL', 'LLM_API_KEY', 'LLM_EFFORT', 'LLM_MAX_TOKENS',
        'EDITOR_LLM_BASE_URL', 'EDITOR_LLM_MODEL', 'EDITOR_LLM_API_KEY'
$saved = @{}
foreach ($v in $vars) { $saved[$v] = [Environment]::GetEnvironmentVariable($v, 'Process') }

try {
    # 編集用の別指定がシェルに残っていると、そちらが選ばれて測りたいモデルにならない。
    foreach ($v in 'EDITOR_LLM_BASE_URL', 'EDITOR_LLM_MODEL', 'EDITOR_LLM_API_KEY', 'LLM_EFFORT') {
        [Environment]::SetEnvironmentVariable($v, $null, 'Process')
    }
    if ($Preset -eq 'grok') {
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
        if ($p.MaxTokens) { $env:LLM_MAX_TOKENS = $p.MaxTokens } else { [Environment]::SetEnvironmentVariable('LLM_MAX_TOKENS', $null, 'Process') }
        Write-Host ("[model] {0} / {1} (max_tokens={2})" -f $p.BaseUrl, $p.Model, $(if ($p.MaxTokens) { $p.MaxTokens } else { '.env の値' }))
    }

    foreach ($name in $targets) {
        $work = Join-Path $OutDir "$name.pkg"
        if (Test-Path $work) { Remove-Item -Recurse -Force $work }
        if ($name -eq 'r1_skeleton') { New-Skeleton $work }
        else { Copy-Item -Recurse (Join-Path $repo 'packages\lakeside_manor') $work }

        $req = (Get-Content -Raw -Encoding utf8 (Join-Path $here "requests\$name.txt")).Trim()
        $report = Join-Path $OutDir "$name.json"
        $stdout = Join-Path $OutDir "$name.stdout.txt"
        $stderr = Join-Path $OutDir "$name.stderr.txt"
        $argLine = "build `"$work`" `"$req`" --report `"$report`""
        $t0 = Get-Date
        Write-Host ("[run] {0} ... ({1})" -f $name, $t0.ToString('HH:mm:ss'))
        # 標準出力はファイルへ直に繋ぐ (PowerShell のパイプを通すと日本語が文字化けしうる)。
        $proc = Start-Process -FilePath $exe -ArgumentList $argLine -WorkingDirectory $repo -NoNewWindow -Wait -PassThru `
            -RedirectStandardOutput $stdout -RedirectStandardError $stderr
        $elapsed = [int]((Get-Date) - $t0).TotalSeconds
        Write-Host ("[done] {0}: exit={1} / {2} 秒" -f $name, $proc.ExitCode, $elapsed)
    }
}
finally {
    foreach ($v in $vars) { [Environment]::SetEnvironmentVariable($v, $saved[$v], 'Process') }
    $key = $null
}

Write-Host ""
Write-Host "[出力] $OutDir"
Write-Host "[集計] python -I measure/spec38_phase_b/report.py `"$OutDir`""
