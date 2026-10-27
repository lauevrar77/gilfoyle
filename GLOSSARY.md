# Gilfoyle

Gilfoyle is a secure MCP server that administers a Linux host through narrow, allowlisted
capabilities. This glossary fixes the language of that domain.

## Host and units

**Unit**:
A systemd object that is managed as one thing, addressed by a name whose suffix names its
type (`nginx.service`, `sshd.socket`).
_Avoid_: daemon, process

**Service**:
A unit of type `.service`. A subtype of Unit, not a synonym for it.
_Avoid_: unit (when a service specifically is meant)

**Unit name**:
The fully-qualified identifier of a unit, including its type suffix (`nginx.service`). A
short name such as `nginx` is not a unit name until it is normalised.
_Avoid_: service name

**Active state**:
The lifecycle state of a unit: `active`, `inactive`, `failed`, `activating`,
`deactivating`, `reloading`, `maintaining`.
_Avoid_: status, state

**Load state**:
Whether systemd could load a unit's definition: `loaded`, `error`, `masked`.
_Avoid_: —

**Sub state**:
The unit-type-specific refinement of the active state (for example `running`, `dead`,
`exited`).
_Avoid_: —

## Operations

**Job**:
The asynchronous operation systemd enqueues when a unit is started or stopped. It is
tracked by a job path and finishes with a job result.
_Avoid_: task, request

**Job result**:
The outcome of a job: `done`, `failed`, `canceled`, `timeout`, `dependency` or `skipped`.
Only `done` is success.
_Avoid_: status

**Managed unit**:
A unit listed in `[systemd] manage_units`. Only managed units may be started or stopped.
_Avoid_: allowed service

**Allowlist**:
The set of managed units. It is empty by default, so every start and stop is refused until
the operator opts in.
_Avoid_: whitelist, permitted list

## Measurements

**Uptime**:
The time elapsed since the host kernel booted (read from `/proc/uptime`), not the age of
the gilfoyle process.
_Avoid_: process uptime
