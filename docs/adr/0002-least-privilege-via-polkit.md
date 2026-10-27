# Least privilege for unit control via a generated polkit rule

Starting and stopping units requires the `org.freedesktop.systemd1.manage-units`
authorisation. Gilfoyle runs unprivileged and never escalates in-process. Instead, the
operator installs a polkit rule that grants the `gilfoyle` user the right to start and
stop only the units in the allowlist. The rule is generated from `[systemd] manage_units`
(`gilfoyle systemd polkit-rule`), so the configuration is the single source of truth and
the operator applies the output with `sudo`.

Considered options: a dedicated root helper daemon (rejected — more code, a socket and more
surface to harden); running gilfoyle as root (rejected — the largest possible blast
radius).

Consequences: polkit is the enforcement point. Gilfoyle also checks the allowlist itself
as defence in depth, and warns at startup when the installed rule looks out of sync with
the configuration.
