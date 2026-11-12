## Agent worktrees

Multiple agent panes may share a linked Git worktree. The tree keeps each
worktree in a fixed `ContainerKind::Worktree` directly under its project,
identified by the repository common directory and exact worktree root. Its
members cannot be moved outside that container by reparenting, splitting,
restructuring, or undo. A single member keeps the compact agent-and-branch
presentation; multiple members appear below an expandable worktree row using
the configured branch icon. Closing one member retains its siblings; closing
the final member removes only the empty tree container.

An agent pane may own a linked Git worktree. `ilium-core` persists the pane's
`launch_cwd` and `PaneWorkspace` provenance; `Tree::pane_cwd` is the sole
directory source for launch, transcript lookup, and restore. For a project
launched in a repository subdirectory, the same subdirectory is selected
inside the new worktree. A missing worktree remains visible in the tree and
does not resume its agent from the wrong checkout.

`ilium-server` serializes creation within each repository, validates branch
and path again after the client preview, and keeps Git mutation separate from
tree mutation. The server removes only worktrees with verified Ilium ownership
metadata, after exact Git registration and process-tree checks.
Removal also checks ignored and untracked content; ordinary Git worktree
removal alone would discard some ignored files. A retained Ilium worktree
keeps its ownership marker after pane close and regains that identity when
reopened. Foreign existing worktrees remain unowned.
New ownership markers record a custody format revision. Before spawning a
terminal in an owned worktree, the server durably publishes a per-spawn ticket
in Git's metadata for that worktree. Safe removal requires zero tickets after
supervised process termination and a directory-user scan. A failed proof,
crash, unreadable process state, or legacy marker leaves the worktree in place
for manual inspection. The server never silently adopts a foreign worktree.
Prune inventory and mutation both recheck the exact Git registration, marker,
pane references, directory users, and file/branch state. The interactive
close offer is advisory; the same server gates run again for removal.
The configured close policy is stored in the JSON session snapshot per pane,
bound to its worktree identity, rather than in the tree's positional IPC
payload. A missing policy defaults to keeping the worktree.
If marker creation or a post-create command fails after Git has created the
checkout, the server retains it and reports its path. Hooks, filters, or the
setup command may have written files that cannot safely be rolled back.
Branch switching in the current checkout is deferred: that checkout may also
be used by other panes and the user's shell. A dedicated branch for an agent
therefore uses a linked worktree instead of changing the shared checkout.

One session-owned Git status coordinator checks each workspace's HEAD files
every ten seconds, with at most two probes in flight. Full porcelain status
is requested on branch-line hover and agent completion. Status events are
runtime-only, change-only, and replayed to attaching clients. The second
tree line presents the branch and missing/conflict marker; its hover text
explains provenance and the latest verified status.

