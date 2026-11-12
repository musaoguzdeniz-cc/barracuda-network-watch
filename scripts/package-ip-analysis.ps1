param(
    [string]$NetworkWatchExe = (Join-Path $PSScriptRoot '../target/release/BarracudaNetworkWatch.exe'),
    [string]$IpIntelExe = (Join-Path $PSScriptRoot '../../barracuda-ip-intelligence/target/release/ipintel.exe'),
    [string]$IpIntelLicense = (Join-Path $PSScriptRoot '../../barracuda-ip-intelligence/LICENSE'),
    [string]$Output = (Join-Path $PSScriptRoot '../target/release/BarracudaNetworkWatch-IP-Analysis-win-x64.zip')
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem

$inputs = [ordered]@{
    'BarracudaNetworkWatch.exe' = $NetworkWatchExe
    'ipintel.exe' = $IpIntelExe
    'IP-ANALIZI.txt' = (Join-Path $PSScriptRoot 'IP-ANALIZI.txt')
    'Network-Watch-LICENSE.txt' = (Join-Path $PSScriptRoot '../LICENSE')
    'IP-Intelligence-LICENSE.txt' = $IpIntelLicense
}
foreach ($entry in @($inputs.Keys)) {
    $item = Get-Item -LiteralPath $inputs[$entry]
    if ($item.PSIsContainer -or $item.Length -eq 0) { throw "Dosya geçersiz: $($item.FullName)" }
    $inputs[$entry] = $item.FullName
}

$archivePath = [IO.Path]::GetFullPath($Output)
if ([IO.Directory]::Exists($archivePath) -or $inputs.Values -contains $archivePath) {
    throw "ZIP yolu bir giriş dosyası veya klasör olamaz: $archivePath"
}
$directory = [IO.Path]::GetDirectoryName($archivePath)
$null = [IO.Directory]::CreateDirectory($directory)
$temporary = Join-Path $directory ([Guid]::NewGuid().ToString('N') + '.partial')
try {
    $zip = [IO.Compression.ZipFile]::Open($temporary, [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($entry in $inputs.Keys) {
            $null = [IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
                $zip, $inputs[$entry], $entry, [IO.Compression.CompressionLevel]::Optimal)
        }
    } finally {
        $zip.Dispose()
    }
    Move-Item -LiteralPath $temporary -Destination $archivePath -Force
} finally {
    if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary }
}

Get-FileHash -LiteralPath $archivePath -Algorithm SHA256
