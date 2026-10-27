# Mutating tools are default-deny and allowlisted

Every read-only tool may inspect any unit, but the two mutating tools (`start_unit`,
`stop_unit`) operate only on units listed in `[systemd] manage_units`, which is empty by
default. A tool that changes host state is not enabled until the operator names the exact
units it may touch. This keeps a compromised or misled agent from stopping `sshd` or
`dbus`, and it is enforced twice: by the polkit rule (authority) and by the tool itself
(defence in depth).
