# Offline incremental Git synchronization

Git mode sends the committed history needed to advance one checked-out branch from baseline B
to target T. It packages a native Git bundle as one `.vftgit` file, then uses the existing
one-way VFT file channel. It needs no ACKs, network between endpoints, shared filesystem or
capture-order continuity. Uncommitted source changes are not included. This is an update of an
existing complete repository, not initial cloning, working-directory mirroring or an archive.

## Requirements and baseline

Both native endpoints need Git on PATH for Git mode (the CLI accepts Git 2.48 or newer).
The exercised Linux runtimes are Git 2.50.1 and 2.56.1; accepting another version is not
evidence of testing it. Ordinary file sending/receiving does not invoke Git. Browser sending needs only
a previously prepared package, not an installed Git executable.

Use complete non-bare repositories with the same object format (`sha1` or `sha256`). On the
receiver, check out the intended branch, finish any merge/rebase/cherry-pick/bisect operation,
and ensure the index and tracked worktree are clean. Read the baseline there:

```sh
git rev-parse HEAD
```

Provide that **full lowercase commit OID** to the sender through a user-chosen route. A
shortened hash is not accepted. The receiver must already have the full history and objects
reachable from B; merely sharing its HEAD value is insufficient. The sender requires B to be
an ancestor of T and pins T with an operation-owned reference before building the bundle.
A moving source branch cannot change the already selected target. When B equals T, the sender
reports that no update is needed and creates no package or visual window.

## Send and receive

On the receiving machine, from the repository or any of its subdirectories:

```sh
vft-recv --git --auto
```

On the sending machine, substitute the full receiver OID below:

```sh
vft-send --git --git-base FULL_RECEIVER_OID
# Select another committed ref and an explicit package location:
vft-send --git-repo /path/to/source --git-base FULL_RECEIVER_OID --git-ref main --git-package update.vftgit
```

`--git` means `--git-repo .`; an explicit `--git-repo PATH` takes precedence regardless of
option order and also enables Git mode by itself. A path inside a repository resolves to its
root. Non-repositories fail before any visual window or capture session starts.
`--git-ref` defaults to `HEAD`; use a named ref or a full OID, not an arbitrary revision
expression. The package records the resolved commit, not a branch to execute on the receiver.

`--git-branch` defaults to the receiver's current branch. An explicit value must identify that
same checked-out branch; VFT never checks out another branch. Detached and unborn HEADs fail.
At startup VFT records the branch and HEAD and holds a repository task lock. It checks them
again, together with the worktree/index, immediately before applying. **Do not edit the
receiving worktree, move refs, run another Git operation or change repository configuration
while it is receiving/applying.** The lock excludes other VFT Git sessions, not arbitrary
editors or Git commands. Git's own index/ref locks protect individual Git operations; the
entire worktree update is not an atomic filesystem transaction.

## Package locations and browser sending

Without `--git-package`, the sender uses the OS temporary-directory API and creates a private
`vft-git-<random ID>/update.vftgit`. The complete package is retained after sending, cancellation
or a later display failure. Only this operation's incomplete output and private scratch data
are cleaned on a normal preparation failure. An explicit package path must have an existing
parent; it is opened with create-new semantics. Existing files are never overwritten.

To prepare a package without a visual window, including for the browser sender:

```sh
vft-send --git --git-base FULL_RECEIVER_OID --git-prepare-only
```

The actual path is printed. Select that file in the existing browser sender, or later send it
as an ordinary file with `vft-send /printed/path/update.vftgit`. Keep it unchanged throughout
transmission. The receiver still needs Git mode to apply it automatically. Browser source and
VFT frame semantics are unchanged.

Receiver output precedence in Git mode is:

| Supplied options | Saved package |
| --- | --- |
| `--output FILE` | Exactly FILE; overrides `--output-dir` |
| `--output-dir DIR`, no `--output` | Safe manifest filename inside the existing DIR |
| Neither | New OS temporary directory `vft-git-recv-<random ID>/received.vftgit` |

Every output uses create-new semantics. Existing diagnostic/output paths and unsafe manifest
names are rejected using the ordinary receiver rules. The CLI/TUI reports the actual path.
The receiver retains a completely saved package whether Git application succeeds, fails or
is cancelled. OS cleanup policies still apply to files retained in the system temporary
location; copy an important package elsewhere for long-term retention.

The ordinary receiver's default **256 MiB** file limit includes the application header and
bundle. `--max-file-size BYTES` also governs offline application. It is a package-byte limit,
not a memory/disk limit on Git's decompressed objects. Recovery remains memory-backed with a
separate 1,000,000-symbol limit. Git needs additional scratch and object-store disk space.

## Validation, application and retry

The receive path is:

1. Recover VFT bytes and verify the complete file SHA-256.
2. Create the saved file exclusively, write verified chunks and synchronize it.
3. Validate the versioned application header, exact file length and bundle SHA-256, copying
   only bounded chunks into a private scratch bundle.
4. Recheck repository completeness, object format, startup branch/HEAD and worktree/index.
5. Run `git bundle verify`, require one advertised target matching the header, then import
   objects with `git bundle unbundle` without adopting its advertised ref name.
6. Check imported objects and target history, pin the imported target, and retain the old
   commit under a new `refs/vft/backups/<random ID>` reference.
7. Apply `git merge --ff-only --no-overwrite-ignore --no-autostash` with explicit noninteractive
   options, then verify the current branch, HEAD, index tree and clean tracked worktree.

Importing objects is not completion. Git mode reports **Complete** only after repository
verification succeeds. Reapplying a valid package when the current HEAD is already T succeeds
without another branch update, provided the repository is clean and the package is valid.
A changed baseline, divergent history, staged/worktree modifications, or untracked/ignored
file collisions cause an error. Unrelated untracked files are allowed and left in place.

A failure after receiving leaves the full package and reports an offline retry command:

```sh
vft-recv --git --git-apply /actual/path/received.vftgit
# An explicit repository/branch is also supported:
vft-recv --git-repo /path/to/receiver --git-branch main --git-apply /actual/path/received.vftgit
```

This path opens no capture session. Inspect the reported error, preserve/resolve local changes
using your normal Git workflow, and retry. VFT never hard-resets, cleans user files or performs
an automatic rollback. Backup refs remain after success or a failed update; inspect one with
`git show refs/vft/backups/ID`. Remove an unneeded backup explicitly with
`git update-ref -d refs/vft/backups/ID` after verifying the result.

Preparation can be stopped before transmission (TUI Quit or CLI Ctrl+C). Receiving and pre-update Git checks can
also be cancelled. Once the fast-forward starts, TUI Stop and CLI Ctrl+C wait for that operation and its
final checks to finish. Each Git subprocess has a 120-second hard deadline; a deadline,
forced process termination (including SIGKILL/Task Manager) or filesystem error may leave a partially changed worktree/index.
Inspect actual HEAD, `git status` and any Git lock files before retrying. No atomic rollback
under filesystem failure is promised. The persistent `vft-git.lock` file in the Git common
directory is harmless: its OS lock is released when the owner exits.

Transfer statistics continue to describe verified/saved file bytes and capture time; Git
application time and outcome are reported separately. Successful transport with failed Git
application is not an overall Git synchronization success. Git checkout may apply native
line endings, encodings, executable-bit handling and symlink behavior; identical commit/tree
IDs do not promise identical raw worktree bytes across arbitrary platform configurations.

## Unsupported repositories and controlled execution

VFT rejects shallow/partial/promisor repositories, missing/corrupt objects, alternates/grafts,
replace refs, sparse checkouts and assume-unchanged/skip-worktree index entries. Submodule trees
and `.gitmodules`, as well as LFS pointer blobs, are rejected in the baseline and newly included
commit trees. External clean/smudge/process commands are disabled in child configuration before
worktree inspection. Attribute queries reject files that actually use a filter, including global
and repository-local attribute rules. Merely having an unused LFS driver configured is allowed.
New filter-driver configuration during a session is rejected; driver metadata is limited to
128 names and 4096 combined name bytes. No required transformation is silently skipped.

Git runs with argument arrays and no shell. Child configuration disables hooks (using a private
empty hooks directory), fsmonitor, automatic maintenance, recursive submodule handling and
lazy fetching. All transport protocols and credential prompting are disabled. Inherited
`GIT_*` overrides are cleared; normal repository and user configuration is read but never
modified globally. Output is bounded to 8 MiB stdout and 64 KiB stderr per command, with short
error excerpts. Windows helpers use a hidden process and an owned Job; Unix helpers use a
separate process group, allowing timeout/cancellation to stop this operation's child tree.

The package supplies no local repository path, checkout branch, command or hook. CRC and
SHA-256 are corruption checks, not sender authentication. Only apply history from a trusted
source. See the [application package format](git-package-format.md),
[Git bundle documentation](https://git-scm.com/docs/git-bundle) and
[Git merge documentation](https://git-scm.com/docs/git-merge).
