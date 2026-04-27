# lattice-sandbox-local

## Purpose

Local subprocess-based sandbox implementation. Executes commands as local subprocesses with no isolation (development/testing only).

## Key Types

- `LocalSandbox` — executes commands via `tokio::process::Command`

## Platform Support

LocalSandbox automatically selects the appropriate shell based on the target platform:

- **Unix/Linux/macOS**: Uses `sh -c <command>`
- **Windows**: Uses `cmd.exe /C <command>`

The shell selection happens at compile time using `#[cfg(unix)]` and `#[cfg(windows)]`.

### Example

```rust
use lattice_sandbox_local::LocalSandbox;
use lattice_core::Sandbox;

let sandbox = LocalSandbox::new();

// On Unix: executes as `sh -c "echo hello"`
// On Windows: executes as `cmd.exe /C "echo hello"`
let result = sandbox
    .execute("tool", serde_json::json!({ "command": "echo hello" }))
    .await?;
```

## Security Warning

**This provides no isolation.** Commands run with the same OS user privileges as the Lattice process. Do not use in untrusted environments.

## Design Decisions

- Platform detection is compile-time, not runtime, for zero overhead
- The first parameter to `execute()` (`tool: &str`) is currently ignored
- Commands are extracted from the `params` JSON object's `"command"` field

## Known Issues

None currently.

## Dependencies

- Depends on: `lattice-core`
- Depended on by: `lattice-tools` (BashTool)
