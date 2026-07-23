param(
    [switch]$Split
)

$ErrorActionPreference = "Stop"

$libFeatures = "hydrate"
$args = @("leptos", "build", "-p", "hegira", "--release")
if ($Split) {
    $args += "--split"
    $libFeatures = "hydrate,wasm-split"
}
$args += "--lib-features"
$args += $libFeatures

cargo @args

$pkg = Join-Path "target" "site/pkg"
if (!(Test-Path $pkg)) {
    throw "WASM package directory was not found: $pkg"
}

Get-ChildItem $pkg -Filter "*.wasm" |
    Sort-Object Length -Descending |
    Select-Object Name,
        @{Name = "KB"; Expression = { [math]::Round($_.Length / 1KB, 2) }},
        @{Name = "MB"; Expression = { [math]::Round($_.Length / 1MB, 3) }}
