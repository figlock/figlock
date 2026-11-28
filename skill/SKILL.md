---
name: figlock
description: Use secrets (API tokens, passwords, env files) from the user's FigLock vault without ever seeing them. Use when a task needs a credential, for example calling an authenticated API, running a deploy, or any command that needs a token in its environment.
---

# FigLock: use secrets without holding them

FigLock is the user's password manager. You can use a secret after the user approves it (Touch ID),
and you should do it in a way that keeps the value out of this conversation.

## Rules

1. **Never ask the user to paste a secret into the chat**, and never write a secret into a file, a command
   line argument, a commit or a log. Use the commands below instead.
2. **Prefer `figlock http`.** The broker adds the credential to the request itself and you never receive it.
3. **Use `figlock run` only when a program must have the secret in its environment.** The program gets
   the value, so a program that means to print it can. Its output is scrubbed for accidents only.
4. **Do not use `figlock get --reveal`, `figlock export --include-secrets` or `figlock copy`.** They put
   a value where you can read it. Only the user decides to do that, on their own terminal.
5. **Do not retry a denied request.** If the user declines or the approval times out, say so and stop.
6. **Do not try to list the vault.** Ask the user for the item's title if you do not know it. That
   includes `figlock find --deleted`, `figlock edit --restore-all`, `figlock health` and `figlock recovery-key`: they are for the user.
   The same goes for `figlock forget`, `figlock locate` and `figlock restore-vault`, which change which vault this device uses.

## Call an API: `figlock http`

```
figlock http --item "GitHub token" --reason "list open issues" https://api.github.com/user/repos
figlock http --item "GitHub token" -X POST -H "Accept: application/vnd.github+json" \
  -d '{"title":"bug"}' https://api.github.com/repos/OWNER/REPO/issues
```

- `--item` is the item's title (case does not matter). The request is sent only to hosts on that item's
  allow-list; any other host is refused and nothing is sent.
- `--reason` is shown to the user when they approve. Say what you are doing, briefly and truthfully.
- The default header is `Authorization: Bearer <token>`. For another API use `--auth-header` and
  `--auth-scheme` (an empty scheme sends the bare value), for example `--auth-header X-Api-Key --auth-scheme ""`.
- `-d @-` reads the body from standard input. `--fail` exits with 22 when the server answers 400 or above.
- Output is the response body, like `curl`. Add `-i` to see the status line and headers.

## Run a program with secrets: `figlock run`

```
figlock run --item "Deploy token" --reason "deploy to staging" -- ./deploy.sh
figlock run --bundle "staging env" --reason "run the test suite" -- npm test
figlock run --item "Cloud account" --env AWS_SECRET_ACCESS_KEY=secret -- terraform plan
```

- A token item sets its own variable. An env bundle item sets every variable it holds. `--env NAME=field`
  maps a variable to a field of `--item`.
- Everything after `--` is the program. Secret values in its output appear as `[REDACTED:NAME]`.
- The exit code is the program's.

## Approvals and time

- A new request opens a Touch ID prompt on the user's machine. Use `--ttl` to ask for `once`, `15m`
  (default), `1d` or `until-lock`. Ask for the shortest that fits the job.
- While an approved `15m`, `1d` or `until-lock` grant is live, the same item and mode from the same
  program is answered without a new prompt, so repeat calls inside the window are fine. `once` always asks.
- Make one request at a time and wait for it. Do not retry while a prompt may still be open: the user is
  already being asked, and a second window is noise.
- Exit code 69 means no broker could answer and a person has to act. Do not retry and do not try to start
  anything yourself: tell the user, and stop.
- If a command says the broker is not running or is locked, tell the user to open FigLock and unlock it
  (or run `figlock broker start` in a terminal). You cannot do either for them. When the FigLock app is
  unlocked it answers agents itself and nothing needs starting.
- `figlock broker status` and `figlock broker grants` show what is active (never a token).
- The broker locks itself after 30 minutes without an approved request (the user can change that with
  `figlock broker start --idle-timeout`), and when the Mac sleeps or the screen locks. A locked broker is
  the same case as above: tell the user to unlock it, and do not try to work around it.

## If something goes wrong

- `not allowed` for a host: the item's allow-list does not include it. Tell the user; do not work around it.
- `item not found`: check the title with the user. Titles must match one live item.
- Sync trouble is not yours to fix: `figlock sync status` lists each place the vault syncs to (Google Drive,
  iCloud Drive or another folder) with when it last synced and what went wrong. iCloud Drive off or signed
  out, or a folder still downloading, resolves on the user's side; tell them, and keep using what is local.
- Never include a secret, even a partial one, in your replies or in a summary of what you ran.
