# 0002 - Designing install.sh, 2026-04-06

## Issue

`install.sh` is a `curl | bash` convenience installer. A security review
flagged it as a supply-chain risk and recommended adding SHA256 checksums
and/or GPG verification.

## Analysis

See [docs/analysis/2026-04-06-secure-install.md](../../analysis/2026-04-06-secure-install.md)
for the full analysis. The key finding is:

> Any verification mechanism implemented inside `install.sh` can be silently
> removed by an attacker who has compromised the GitHub account — because
> `install.sh` is itself served from GitHub. The `curl | bash` model is
> inherently vulnerable to a compromised source regardless of what the script
> does internally.

## Decision

1. **Keep `install.sh`** as a convenience installer for casual use, with a
   prominent warning that it is not suitable for security-sensitive
   installations and that HTTPS is the only transport protection provided.

2. **Recommend `cargo install fuselage`** as the secure installation path.
   crates.io is a separate trust domain and the release is independently
   actioned from the developer's local machine.

3. **Implement SHA256 checksum verification inside `install.sh`**, despite
   it providing no protection against a compromised GitHub account. It does
   guard against accidental download corruption and basic MITM attacks, and
   the implementation cost is negligible. The warning message makes clear
   that this is not a security guarantee against a compromised source.

4. **Sign commits and tags with an SSH key** associated with the GitHub
   account. This is good practice and adds a useful signal for users who
   independently maintain the signing key fingerprint — but it is not
   a protection mechanism for `install.sh` users. SSH signing is preferred
   over GPG because SSH keys are already in use for repository authentication,
   requiring no separate credential infrastructure. See
   [docs/working-practices/ssh-signing.md](../../working-practices/ssh-signing.md).

## Consequences

- `install.sh` is simpler and more honest about its limitations.
- Users are directed to `cargo install fuselage` for secure installation.
- The warning in `install.sh` and the README sets correct expectations.
- Signed commits/tags provide an audit trail and a verification path for
  security-conscious users who independently import the signing key.
