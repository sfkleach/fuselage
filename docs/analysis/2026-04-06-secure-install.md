# Analysis: Secure Installation of fuselage, 2026-04-06

## Trigger

A security review (Sourcebot) flagged `install.sh` as follows:

> This downloads and executes a remote binary with no checksum or signature
> verification, which is a serious supply-chain risk (e.g., compromised GitHub
> account or MITM). Please add integrity checks (e.g., published SHA256 and/or
> optional GPG verification), or at minimum emit a prominent warning to users
> that no verification is performed.

This analysis examines whether the suggested mitigations are sound, and
what — if anything — can genuinely protect users.

## The fundamental problem with `curl | bash`

When a user runs:

```bash
curl -sSfL https://raw.githubusercontent.com/sfkleach/fuselage/main/install.sh | bash
```

GitHub is the trust anchor for **everything**: the binary, the checksums, the
documentation, the GPG key references, and the script that performs any
verification. A compromised GitHub account can replace all of these
simultaneously. Any verification mechanism implemented inside `install.sh` can
be silently removed by the same attacker.

This is the irreducible vulnerability of the `curl | bash` model. HTTPS
provides transport integrity (protecting against MITM and CDN corruption) but
does not protect against a compromised source.

## Analysis of suggested mitigations

### SHA256 checksums published to the GitHub release

If checksums are generated and uploaded by GitHub Actions, a compromised
GitHub account can upload a matching checksum for a malicious binary in the
same workflow. Checksums hosted on GitHub protect against MITM and accidental
corruption only. Against a compromised GitHub account: no protection.

### GPG signature verification

GPG is the canonical answer to this class of problem, but it has a critical
bootstrapping weakness: the user must obtain the signing key fingerprint
out-of-band, from a source they independently trust, *before* any compromise
occurs.

- If the public key reference is in the GitHub-hosted documentation or
  `install.sh`, a compromised account replaces it with the attacker's key.
- If the user is directed to import a "new" key following a key rotation, they
  cannot distinguish a legitimate rotation from an attacker substituting their
  own key — unless they independently verify the transition.
- Key revocation does not help: an attacker simply presents a new unrevoked key
  and updates all reachable references to it.

GPG provides genuine protection only for users who:
1. Obtained the key fingerprint out-of-band before any compromise, AND
2. Independently verify any subsequent key rotation

This is a small minority of users. For everyone else, GPG verification via
`install.sh` is security theatre.

### Two-platform checksum comparison

A more promising approach: publish checksums to a second platform (e.g.
GitLab) with independent credentials, and have users compare the checksums
from both platforms before installing.

This is **sound as a manual procedure**:
- Attacker compromises GitHub → can replace binary and GitHub-hosted checksum
- Attacker cannot update GitLab without separate credentials
- User fetches checksum from both platforms independently and compares
- Disagreement signals a compromise

However, this cannot be implemented inside `install.sh` — because `install.sh`
is itself served from the compromised platform. A modified `install.sh` simply
skips the comparison. The two-platform approach only works if the user
performs the comparison themselves, outside of any GitHub-served script.

### "At minimum emit a prominent warning"

Honest, but provides zero additional security. It makes the risk visible
without reducing it.

## What actually protects against a compromised GitHub account

| Path | Trust anchor | Protects against GitHub compromise |
|---|---|---|
| `curl \| bash install.sh` | GitHub | No |
| Manual two-platform checksum comparison | GitHub + GitLab | Yes — if performed independently |
| `cargo install fuselage` | crates.io | Yes — separate trust domain |
| Build from source | User's own inspection | Yes |

## Conclusions

1. **`install.sh` is a convenience installer.** HTTPS provides transport
   integrity. No mechanism inside the script can protect against a compromised
   GitHub account. This should be stated honestly in the README.

2. **The recommended secure installation path is `cargo install fuselage`.**
   crates.io is a separate trust domain; the release is independently actioned
   from the developer's local machine rather than from GitHub Actions alone.

3. **Security-conscious users who want to verify a binary download manually**
   should compare the SHA256 checksum published on GitHub against the checksum
   published independently on GitLab (or another independent platform). The
   procedure should be documented clearly.

4. **GPG signed tags** add value as an additional signal for users who
   independently maintain the signing key fingerprint. The public key
   fingerprint should be published on GitLab alongside the checksums so that
   users with a prior relationship can verify tag signatures. This is not
   a first-time-user protection.

5. **The Sourcebot recommendation** protects against MITM and accidental
   corruption — real but narrow threats given HTTPS. It does not protect
   against the supply-chain attack it names. Implementing it as described
   would be security theatre against that specific threat.

## Future work

- Publish a manual verification procedure in the README.
- Publish checksums to an independent platform (GitLab) as a separate manual
  step in the release process, so that security-conscious users have a
  genuine second-factor verification path.
- Publish GPG signing key fingerprint on GitLab for users who wish to verify
  tag signatures.
- Consider a YubiKey for signing to close the "compromised local machine"
  gap in the GPG story.
