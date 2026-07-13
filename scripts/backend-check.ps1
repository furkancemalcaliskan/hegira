param(
    [switch]$WithIgnoredDbTests
)

$ErrorActionPreference = "Stop"

function Run-Step {
    param(
        [string]$Name,
        [string[]]$Command
    )

    Write-Host "==> $Name"
    & $Command[0] @($Command[1..($Command.Length - 1)])
    if ($LASTEXITCODE -ne 0) {
        throw "$Name failed with exit code $LASTEXITCODE"
    }
}

Run-Step "Format" @("cargo", "fmt", "--all", "--", "--check")
Run-Step "Clippy" @("cargo", "clippy", "--features", "ssr,test-support", "--", "-D", "warnings")
Run-Step "SSR check" @("cargo", "check", "--features", "ssr")
Run-Step "Hydrate check" @("cargo", "check", "--features", "hydrate", "--target", "wasm32-unknown-unknown")
Run-Step "OpenAPI check" @("cargo", "check", "--features", "openapi")
Run-Step "DbMigrator check" @("cargo", "check", "-p", "db_migrator", "--features", "ssr")
Run-Step "Library tests" @("cargo", "test", "--features", "ssr", "--lib")
Run-Step "Capability test support" @(
    "cargo",
    "test",
    "--features",
    "ssr,test-support",
    "--test",
    "capability_test_support"
)
Run-Step "API identity tests" @("cargo", "test", "--features", "ssr", "--test", "api_identity")

if ($WithIgnoredDbTests) {
    if (-not $env:DATABASE_URL) {
        throw "DATABASE_URL must be set when -WithIgnoredDbTests is used"
    }

    Run-Step "DB-backed API identity tests" @(
        "cargo",
        "test",
        "--features",
        "ssr",
        "--test",
        "api_identity",
        "--",
        "--ignored"
    )
    Run-Step "DB-backed identity persistence tests" @(
        "cargo",
        "test",
        "--features",
        "ssr",
        "--test",
        "identity_persistence",
        "--",
        "--ignored"
    )
    Run-Step "DB-backed durable jobs tests" @(
        "cargo",
        "test",
        "--features",
        "ssr",
        "--test",
        "durable_jobs",
        "--",
        "--ignored"
    )
}
