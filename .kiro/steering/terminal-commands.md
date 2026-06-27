---
inclusion: auto
---

# Terminal Command Best Practices

## Avoiding Shell Hangs

This project runs on macOS with zsh. The terminal tool frequently hangs because zsh stays alive after a command completes, waiting for the next input. The tool interprets this as "still running" and times out.

### Rules

1. **Prefer file tools over shell commands.** Use `read_file`, `list_directory`, `grep_search`, `fs_write` instead of `cat`, `ls`, `grep`, `echo >`.

2. **Never use shell for read-only file operations.** The workspace has dedicated tools that are faster and don't hang.

3. **When shell is unavoidable** (e.g., running `cargo check`, `pnpm exec tsc`, `sqlite3`):
   - Always use `ignoreWarning: true` with a short `timeout` (5000-10000ms) for quick commands
   - For compilation commands that take > 10s, use `control_bash_process` with action `"start"` instead, then poll with `get_process_output`
   - Never chain commands with `&&` or `;` — run them separately

4. **For build verification:**
   - Use `control_bash_process` action="start" for `cargo check` and `cargo build` (they take 30s+ first time)
   - Use `get_process_output` to poll results
   - TypeScript check (`pnpm exec tsc --noEmit`) typically completes silently in < 5s — if it times out with no output, it means success (exit 0)

5. **For database operations** (sqlite3):
   - Use timeout: 5000 with ignoreWarning: true
   - If output shows the result followed by a shell prompt (`%`), the command succeeded — ignore the timeout error

6. **Never use `cd`** — use the `cwd` parameter instead.

## File Access

- The workspace is `/Users/a19150/Project/Cognest`
- The Vault is at `~/CognestVault` (outside workspace) — use shell commands with cwd for vault operations since file tools can't access it
- Always use absolute paths or `~` expansion when referencing the vault
