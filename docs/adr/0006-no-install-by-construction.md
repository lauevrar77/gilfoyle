# Gilfoyle never installs or removes a package, by construction

Installing and removing packages is out of scope, as a hard constraint. It is enforced
in several independent places rather than by a single check:

- The package operation vocabulary is a closed enum with exactly two variants, `Refresh`
  and `Upgrade`. There is no `Install` or `Remove`, so `[packagekit] allow` cannot name
  one: an unknown entry fails the configuration load.
- The driven port exposes only `list_installed`, `check_updates`, `plan`, `apply` and
  `refresh`; the adapter's method dispatch can therefore only ever call `GetPackages`,
  `GetUpdates`, `RefreshCache` and `UpdatePackages`.
- `UpdatePackages` always carries the `ONLY_TRUSTED` flag; `PURGE`, `ALLOW_REINSTALL` and
  `ALLOW_DOWNGRADE` are never set.
- Before applying, the upgrade is simulated. If the plan would install, remove, obsolete
  or downgrade a package, it is refused unless the operator explicitly sets
  `allow_transitive_changes = true`. Even then, no install or remove *operation* exists;
  the operator only accepts the transitive effect of an update.
- The generated polkit rule explicitly refuses the install, uninstall and signing-key
  actions for the gilfoyle user.

Untrusted packages (a missing signature, a signing key that would have to be trusted, or
a EULA that would have to be accepted) are always refused: gilfoyle never answers
`InstallSignature` or `AcceptEula`.
