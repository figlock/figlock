# Contributing to FigLock

Thanks for helping. Read [AGENTS.md](AGENTS.md) first: it holds the non-negotiables (no plaintext
secrets anywhere, standard crypto primitives only, ciphertext-only storage, no backend of ours), the
repo layout and the working style. [docs/decisions.md](docs/decisions.md) lists what is already decided.

## Before you start

- Open or pick an issue first. Title it `FIG-<n>: <task title>` if it maps to a tracked task.
- Changes to the crypto or sync spec need an ADR in `docs/adr/` and new test vectors before code.
- Never put real secrets, tokens, recovery codes or local file paths in issues, PRs, commits, logs,
  fixtures or screenshots. Use generated values and the test vectors in `docs/spec/`.

## Making a change

1. Branch off `main` as `FIG-<n>-<short-slug>`.
2. Use [conventional commits](https://www.conventionalcommits.org), e.g. `feat(core): ...`,
   `fix(cli): ...`.
3. Add tests. Security-relevant code needs an explicit negative test (wrong key, tampered op, host not
   on the allow-list, and so on).
4. Open a PR titled `FIG-<n> ...` with `Closes #<issue>` in the body.
5. Do not edit `CHANGELOG.md` or other generated files; release notes come from the release tool.

## Style

Plain prose, no em dash character (use `-`). Prefer simple, robust code over clever code.

## Security issues

Do not file them publicly. See [SECURITY.md](SECURITY.md).

## License

By contributing you agree your work is licensed under MIT OR Apache-2.0, as in the README.
