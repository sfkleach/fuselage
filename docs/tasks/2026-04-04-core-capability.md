# Task: Fuselage Core Capability

We will be implementing a command-line program `fuselage` that provides 
private emphemal folders for a sub-command. 

```
fuselage [OPTIONS...] [--] COMMAND [ARG...]
fuselage [OPTIONS...] --run PATH [ARG...]
```

The program is specified in [this file](../SPEC.md). A bash prototype is 
provided [here](../fuselage).

## Step 1

Implement the basic usage:

```
fuselage COMMAND [ARG...]
```

This runs the specified sub-command, passing the provided arguments. The COMMAND
is resolved using the same rules as for `env` i.e. if it is simply a name then 
the $PATH is searched, otherwise it is treated as a file path.


## Step 2

When the fuselage binary is setuid as root, set up the TMPDIR and then drop
root privileges. (In this mode it should be possible to run sudo within the
shell.)


## Step 3

Now implement the `-d,--dynamic=[NAME:]ARCHIVE` option. At this point the only archive
format we will support is _zip_. This will mount the contents of the zip archive
in the `$FUSELAGE_DYNAMIC` folder. 

The name of the mount will be specified by NAME or fall-back to the file-name 
stem of ARCHIVE.

## Step 4

Now implement the `--static=[NAME:]ARCHIVE` option. Again we are only looking to
support a zip archive at this point in time. This will mount the contents of the
zip archive in a read only folder in `$FUSELAGE_STATIC`. 

For this iteration we will not implement caching of the extracted folder. In 
Step 5 we will look at different ways to improve the loading speed.

## Step 5, Caching --static

In this step we aim to reduce the amount of time spent unpacking the zip files
when mounted as --static by unpacking into an cache indexed by the hash of the
zip file itself. Subsequent runs can use the cache to skip the unpacking. This
behaviour is gated by the `--cache-static` flag.

### Suggested cache expiry mechanism

This section suggests a mechanism based on last-access times. However if there
are better mechanisms available, please suggest them.

The cache will grow without limit, so caches that have not been used recently
should be reaped. On exit, fuselage will spawn a process that reaps old caches
that exceed a minimum size (the size should be pre-calculated). On start-up,
before trying to load any cached folders, fuselage will "tag" all of the caches
it wants to use as "in use" by updating the last-access time.

While fuselage is running, it should periodically update the last-access time of
any cache in use (which allows for many fuselage processes to be using the same
cache). 

