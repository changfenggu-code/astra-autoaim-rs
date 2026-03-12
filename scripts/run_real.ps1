$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot\..
cargo run -p astra-app -- .\config\app.example.yaml