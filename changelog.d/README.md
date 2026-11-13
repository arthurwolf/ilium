# Changelog fragments

Do not edit `CHANGELOG.md` for a user-visible change. Add one file per branch,
`changelog.d/<branch-or-topic>.md`, so two branches never touch the same file:

```
### Added
- One bullet per user-visible addition.

### Fixed
- One bullet per fix.
```

Allowed headings: `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, `Security`.
A fragment is committed with the branch. `python3 tools/compile_changelog.py`
(run when releasing, or by whoever owns the main checkout) folds every fragment
into `## [Unreleased]` of `CHANGELOG.md` and deletes the fragment files.
`--check` validates the fragments without writing.
