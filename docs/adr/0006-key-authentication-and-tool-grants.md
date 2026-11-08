# Authenticate by public key only, and grant tools per key

Status: accepted

Gilfoyle has no tokens and no passwords: a client authenticates with an SSH key, and the
node maps the verified key to a principal. Each authorized key may carry a `tools` grant;
the tools a key may call are the intersection of its grant and the global `[tools].enabled`,
with an unknown tool name failing configuration, and a key without a grant inheriting the
global set. Mutating tools keep their own discipline unchanged: `start_unit` and `stop_unit`
act only on `[systemd] manage_units`, and polkit remains the authority.

Considered options: keeping bearer tokens (rejected — a replayable shared secret, redundant
once SSH provides key authentication); one identity key reused across hosts (rejected — no
per-host revocation); OpenSSH certificates (deferred — keys only for now).

Consequences: the bearer machinery is removed (`hash-token`, `TokenStore`, constant-time
comparison), and rotating or revoking an authorized key requires a restart (no SIGHUP
reload). Read-only keys exist from the first version, so a key handed to a reader cannot
reach a mutation even on a node that manages units.
