# Release Credentials

This document covers the one-time setup required on each workstation before
running `just draft-release` or `just publish-release`.

## crates.io token

`cargo publish` reads the token from `~/.cargo/credentials.toml`. Run once
per machine:

```bash
cargo login
```

Paste the token when prompted. It is stored in `~/.cargo/credentials.toml`
and all subsequent `cargo publish` calls use it silently.

To obtain or regenerate the token:
1. Log in at [crates.io](https://crates.io).
2. Go to **Account Settings → API Tokens → New Token**.
3. Scopes: `publish-new` and `publish-update`.
4. Copy the token — it is shown only once.

The token must NOT be added to GitHub Actions secrets. See
[docs/decisions/0002-designing-installsh](../decisions/0002-designing-installsh/0002-designing-installsh.md)
for why `cargo publish` runs locally rather than from CI.

## SSH signing key

Required for `git tag -s` (signed release tags). See
[ssh-signing.md](ssh-signing.md) for full setup instructions.

Quick check — confirm git is configured to use SSH signing:

```bash
git config --global gpg.format        # should show: ssh
git config --global user.signingkey   # should show path to your .pub file
git config --global commit.gpgsign    # should show: true
git config --global tag.gpgsign       # should show: true
```

If any of these are missing, follow the setup steps in
[ssh-signing.md](ssh-signing.md).

## GitHub CLI authentication

`just draft-release` and `just publish-release` use `gh`. Confirm it is
authenticated:

```bash
gh auth status
```

If not, run:

```bash
gh auth login
```
