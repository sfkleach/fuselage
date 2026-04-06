# Copilot Review for Publishing fuselage as a Rust Package

# Justfile

Lines 117-125: 

> publish-release currently requires the GitHub release to be a draft (via gh release view ... | grep -q "draft"), but draft-release's documented usage (v0.2.0) produces a non-draft release with the current release.yml logic (drafts are only for tags containing -). This makes the two-step release flow fail for stable tags. Align the tag naming, workflow draft/prerelease rules, and these recipes so stable releases can be drafted then published (or so publish-release supports non-draft stable releases).

Lines 105-115:

> The comment says publish-release "Verify checksums are present" and draft-release "mirror checksums to Codeberg", but neither recipe actually performs those checks/mirroring. Either implement the advertised steps (e.g., verify .sha256 assets exist for all targets and add the mirroring step), or adjust the comments/docs to match the current behavior.

Lines 111-114:

> draft-release watches the most recent release.yml run (--limit=1) without filtering to the tag just pushed. If multiple release runs are in flight, this can watch the wrong run (or race before the tag-triggered run appears). Prefer selecting the run associated with {{VERSION}} (filter JSON by headBranch/headSha/displayTitle as available) and retry until it exists.

My suggestion is don't bother with the watching, if this comment is correct. Make it my responsibility for running both phases.

Lines 125-136:

> publish-release always runs cargo publish, but the accompanying docs state draft (-...) and pre-release (-rc...) tags should not be published to crates.io. Add an explicit tag/version gate (e.g., refuse tags containing -, or only publish when {{VERSION}} matches v<version> from Cargo.toml), or update the docs to reflect the actual behavior.

Lines 120-123

> Parsing gh release view output with grep -q "draft" is brittle (output format can change). Use gh release view --json isDraft,isPrerelease --jq ... to reliably assert draft/prerelease state.

As above.

# docs/process/definition-of-shippable.md

Lines 36-39:

> This decision record says to sign commits/tags with a GPG key, but the newly added working-practices docs standardize on SSH signing (gpg.format ssh, signing keys in GitHub settings). Please reconcile this (either update the decision to SSH signing, or update the working-practices docs to GPG) so the repo has a single, consistent signing approach.

Fair comment.

# docs/tasks/2026-04-05-cargo-install-fuselage.md

Lines 65-66

> This task doc says just publish-release skips cargo publish for pre-release/draft tags, but the publish-release recipe in Justfile currently runs cargo publish unconditionally. Update either the recipe (add gating) or this doc so they match.


# docs/working-practices/ssh-signing.md

Lines 3-5:

> This doc states "SSH keys are already present on all machines" / "SSH keys are already on every machine", which is not generally true (many machines/users won't have an existing ~/.ssh/id_ed25519*). Consider rephrasing to avoid implying keys already exist, and optionally include a brief note on generating a key if needed.

Lines 57-58:

> The "New machines" section again implies SSH keys already exist on every machine. This can mislead; new machines often require generating or provisioning an SSH key before configuring signing.

