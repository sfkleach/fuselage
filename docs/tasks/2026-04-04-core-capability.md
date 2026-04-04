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
