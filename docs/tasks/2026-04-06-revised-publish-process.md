# Task: Revised Publish Process

After some trial runs of the publication workflow, I want to make a few changes.

- I want the release.yml to be renamed release-draft.yml and to always publish a draft.
- Then I want a second workflow release-publish.yml to:
  - change it from draft to non-draft and then to 
  - trigger the `cargo publish` using the trusted publisher feature.

From a security viewpoint this links the integrity of the published crate to the
github.com account integrity, since the improvement from unlinking them is
not worth the friction.

## Step 1: Release drafts

- Rename the release.yml workflow as release-draft.yml.
- Ensure that it only releases drafts or pre-releases.

## Step 2: New Worflow

- Add a new workflow release-publish.yml
- That checks the git tag has the right pattern (v[0-9]+[.][0-9]+[.][0-9]+) 
- Then flips the release from draft
- And runs `cargo publish` assuming GitHub is a trusted publisher

## Step 3: Tidy up working-practices

- Revise the Justfile so that the two phase publication is retained.
- Update working-practices documents so that they are in line with the 
  new process.
