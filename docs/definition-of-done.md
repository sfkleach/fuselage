# Definition of Done

## Task (feature) criteria, maintained at end of task

### Documentation consistency

- All documents are reviewed for consistency and inconsistencies should be corrected. Documents are normally in `docs/` and task-documents are in `docs/tasks/`. Task documents that are superseded by later changes do not need correction; it is sufficient to mark them as superseded.
- The `CHANGELOG.md` must reflect the new feature.

### Automated testing

- The complete set of tests must pass. These can be automatically run using `just test`, which is responsible for running unit tests, functional tests, formatting checks, checking all executables build, and running `gosec`.
- The `test` recipe in `Justfile` must try building all the executables with all combinations of build flags.

## Step (story) criteria, constantly maintained

### Comment Guidelines

- Comments should be proper sentences, with correct grammar and punctuation,
  including the use of capitalization and periods.
  - EXCEPT for comments that are simply single words or short phrases
    such as `// TODO: ...` or `// Deprecated` or bullet-points.
- Where defensive checks are added, include a comment explaining why they are
  appropriate (not necessary, since defensive checks are not necessary).

### Programming Style Guidelines

For projects we own, including this one, we adopt the following single, uniform, good practice for our own projects and work entirely cross-platform with no use of "smart" defaults (e.g. Git's autocrlf).

- I prefer LF to CRLF/CR line endings in source code files and documentation files.
- I prefer text files to use new-line (LF) as a terminator rather than a separator
  i.e. newlines at the end of non-empty files, including on Windows.
- And lines should not have trailing whitespace EXCEPT in Markdown files where
  trailing whitespace indicates a line break. In those cases, use a single space
  at the end of the line to indicate a line break.
- We use 120 as the maximum line-length and not 80 characters. The detailed guideline
  is that the length first-to-last non-whitespace character should be 80 characters
  and that an additional 40 characters of indentation is allowed.
- Indentation in source files should use spaces only, no tabs EXCEPT in Golang or 
  Makefiles where tabs are effectively required.
- Use 4 spaces per indentation level EXCEPT when working in YAML/JSON files where 2 spaces per indentation level is more practical owning to higher nesting levels.
- UTF-8 encoding should be used for all text files EXCEPT when working with compilers/interpreters that do not support UTF-8.

### Developer documentation guidelines

- Use Unix-style paths (forward slashes) in code and documentation, even on Windows.
- Use Markdown for documentation files wherever possible with the .md file extension.
