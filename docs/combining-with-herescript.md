# Combining fuselage with herescript

[herescript](https://github.com/sfkleach/herescript) generalises shebang scripts,
allowing a script file to pass itself as an argument to the interpreter. Combined
with `fuselage`, this makes it possible to write a single executable file that
carries its own filesystem payload as embedded base64 data.

## How it works

`fuselage` automatically recognises files that are neither zip nor squashfs by
attempting base64 decoding, stripping any leading lines that begin with `#`.
This means a herescript file — which begins with `#!` and `#:` directive lines —
can be passed directly to `--static` or `--dynamic` as the archive source.

herescript exposes the path of the script itself via `${HERESCRIPT_FILE}`, so
the script can mount itself:

```
#!/usr/bin/herescript /path/to/fuselage
#: --static myapp:${HERESCRIPT_FILE}
#: --run myapp/entrypoint.sh
<base64-encoded zip or squashfs image>
```

When this file is executed:

1. herescript parses the `#:` directive lines and builds the `fuselage` command.
2. `fuselage` receives the script path as the `--static` archive.
3. `fuselage` strips the `#` lines, base64-decodes the remainder, and detects
   the archive format (zip or squashfs).
4. The archive is extracted into a read-only namespace-private directory.
5. `--run` resolves and execs the entry point from inside that directory.

## Creating a herescript bundle

### Step 1 — Build your payload

Create the directory tree you want to bundle:

```bash
mkdir -p myapp/bin
cat > myapp/bin/hello.sh <<'EOF'
#!/bin/sh
echo "Hello from the bundle!"
EOF
chmod +x myapp/bin/hello.sh
```

### Step 2 — Pack it as a zip

```bash
cd myapp && zip -r ../myapp.zip . && cd ..
```

Or as squashfs (smaller, faster to mount with `--cache-static`):

```bash
mksquashfs myapp myapp.sfs -comp zstd -noappend -quiet
```

### Step 3 — Base64-encode and write the script

```bash
cat > mybundle <<'SHEBANG'
#!/usr/bin/herescript /usr/local/bin/fuselage
#: --static myapp:${HERESCRIPT_FILE}
#: --run myapp/bin/hello.sh
SHEBANG
base64 myapp.zip >> mybundle     # or myapp.sfs for squashfs
chmod +x mybundle
```

### Step 4 — Run it

```bash
./mybundle
# Hello from the bundle!
```

## Caching large payloads

For large squashfs images the first run is slow (base64 decode + extraction).
Add `--cache-static` to cache the decoded archive across runs:

```
#!/usr/bin/herescript /usr/local/bin/fuselage
#: --cache-static
#: --static myapp:${HERESCRIPT_FILE}
#: --run myapp/bin/hello.sh
<base64-encoded squashfs image>
```

The cache is keyed by SHA-256 content hash and stored under `~/.fuselage/cache/`.
Old entries are automatically reaped after 30 days (configurable via
`FUSELAGE_CACHE_MAX_AGE_DAYS`).

## Notes

- The base64 data may be split across multiple lines of any width — `fuselage`
  reassembles it before decoding.
- Any line beginning with `#` is treated as a comment and skipped, so herescript
  directive lines (`#:`) are handled correctly.
- The decoded archive is written into the private tmpfs and never touches
  persistent storage (unless `--cache-static` is enabled).
- `--dynamic` can also be used instead of `--static` if the bundled filesystem
  needs to be writable.
