# Release Credentials

This document covers the one-time setup required on each workstation before
running `just draft-release` or `just publish-release`.

## crates.io (trusted publisher — no token required)

`cargo publish` runs in GitHub Actions via the trusted publisher mechanism.
No crates.io API token is needed on the local machine or in GitHub secrets.

One-time setup on crates.io (per crate, not per machine):
1. Log in at [crates.io](https://crates.io).
2. Go to the `fuselage` crate → **Settings → Trusted Publishing**.
3. Add a trusted publisher: repository `sfkleach/fuselage`, workflow
   `release-publish.yml`.

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
