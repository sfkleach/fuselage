# SSH Signing for Git Commits and Tags

Git supports SSH keys for signing commits and tags (since Git 2.34, GitHub
support since 2022). If you already use SSH keys for authentication, reusing
one for signing requires minimal additional setup and no extra credential
management.

## Setup

```bash
# Tell git to use SSH for signing.
git config --global gpg.format ssh

# Specify which SSH key to use (use your existing key).
git config --global user.signingkey ~/.ssh/id_ed25519.pub

# Sign all commits and tags by default.
git config --global commit.gpgsign true
git config --global tag.gpgsign true
```

Use the `.pub` file — git reads the public key path and uses the corresponding
private key via `ssh-agent`.

## Add the key to GitHub for "Verified" badges

GitHub needs to know your signing key to display "Verified" on commits and tags.
If your SSH key is already added to GitHub for authentication, you also need to
add it explicitly as a **signing key**:

1. Go to **GitHub → Settings → SSH and GPG keys → New SSH key**
2. Set **Key type** to **Signing Key** (not Authentication Key)
3. Paste the contents of `~/.ssh/id_ed25519.pub`

You may have the same key added twice — once for authentication, once for
signing. That is correct and expected.

## Verify the setup

```bash
# Check git config.
git config --global gpg.format        # should show: ssh
git config --global user.signingkey   # should show path to your .pub file
git config --global commit.gpgsign    # should show: true
git config --global tag.gpgsign       # should show: true

# Test signing a tag.
git tag -s test-sign -m "test" && git tag -v test-sign && git tag -d test-sign
```

## Passphrase caching

`ssh-agent` caches your key passphrase automatically. On most systems it is
started at login and keys are added on first use. No extra configuration needed.

## New machines

If an SSH key exists on the machine, run the four `git config` commands above
and add the key as a signing key on GitHub (if not already done). If no SSH key
exists yet, generate one first:

```bash
ssh-keygen -t ed25519 -C "your_email@example.com"
```

Then follow the setup steps above.

## Migration to YubiKey

The `ed25519-sk` key type is the YubiKey-backed variant of `ed25519` — the
natural upgrade path. The private key never leaves the hardware.

### Prerequisites

```bash
# Install the FIDO2 library (Ubuntu/Debian).
sudo apt-get install libfido2-dev

# Fedora/RHEL.
sudo dnf install libfido2
```

Requires YubiKey firmware 5.2.3 or later (all current models qualify).

### Generate the key on the YubiKey

```bash
# The -O resident flag stores the key handle on the YubiKey itself.
ssh-keygen -t ed25519-sk -O resident -f ~/.ssh/id_ed25519_sk
```

You will be prompted to touch the YubiKey during generation.

### Add to GitHub

Add `~/.ssh/id_ed25519_sk.pub` to GitHub twice:
1. **Authentication Key** — for `git push`, `ssh` etc.
2. **Signing Key** — for "Verified" badges on commits and tags.

### Update git config

```bash
git config --global user.signingkey ~/.ssh/id_ed25519_sk.pub
```

The other config lines (`gpg.format ssh`, `commit.gpgsign`, `tag.gpgsign`)
remain unchanged.

### Using on a new machine

No private key to copy — the YubiKey is the key. On each new machine:

```bash
# Export the public key from the resident key stored on the YubiKey.
ssh-keygen -K
# Writes id_ed25519_sk_rk.pub (and a stub private key) to the current directory.
mv id_ed25519_sk_rk.pub ~/.ssh/id_ed25519_sk.pub
mv id_ed25519_sk_rk ~/.ssh/id_ed25519_sk
git config --global user.signingkey ~/.ssh/id_ed25519_sk.pub
```

Then plug in the YubiKey whenever signing — touch is required for each
signature.

### Retire the old software key

Once the YubiKey key is working, optionally remove the old `id_ed25519` key
from GitHub and delete it from machines where it is no longer needed.

## Signing a release tag

```bash
git tag -s vX.Y.Z -m "Release vX.Y.Z"
git push origin vX.Y.Z

# Verify locally.
git tag -v vX.Y.Z
```
