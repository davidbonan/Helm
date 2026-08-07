# helm — dev servers (`helm run`)

[helm](https://github.com/davidbonan/Helm) runs **one dev server per worktree**: the
command is configured per project, the port assigned per worktree. Before starting a
dev server yourself in a shell, ask helm about the worktree you are working in. If
`helm` is not on `PATH`, ignore all of this and work as you would otherwise.

```sh
helm run status [path] [--json]    # is a server up there (path defaults to .)
helm run list [--json]             # every worktree helm knows: state, port, command
helm run start [path]              # start it — no-op if already running
helm run relaunch [path]           # explicit restart
helm run stop [path]               # stop it (kills the process tree)
helm run logs [path] [-n 40]       # tail of what the server printed
```

- **Ask before spawning.** `running` ⇒ use the **port it reports**, do not start a
  second server. `stopped` / `exited` ⇒ `helm run start`.
- **Start through helm, not in your own terminal**: the output lands in the Run strip
  where the user watches it, and `$PORT` is resolved for that worktree.
- **No command travels**: you cannot pass a command line. What runs is what the
  project configures (helm's Run strip / Preferences), else what its manifest implies.
- **`--json`** to parse: `status` / `list` give `[{worktree, project, branch, state,
  port, command, launch_command, error, exit_code}]`, `state` ∈
  `running|stopped|exited|failed`. On `exited`, `exit_code` says whether it was
  stopped or died.
- **`start` reports the state right after the spawn**, not a health check: a bad
  command answers `running`, then `exited` a moment later. Re-check `status` before
  trusting the server.
- **Debug via `logs`**, not by restarting: it tails the strip's output (stdout only,
  so `| grep` works; `--json` gives `{entry, lines}`). A **stopped** strip returns
  nothing — its buffer died with the pane.
- **Exit codes**: `0` answered · `1` refused (path not a repo, no run command
  configured) · `2` misuse · `3` **no answer to be had** — helm is down, or running
  with its window hidden/minimized (it then draws no frame, and the answer rides on a
  frame). On `3`, fall back to your own shell.
- **Never `stop` / `relaunch` a server you did not start**, unless asked: the user may
  be reading it.
