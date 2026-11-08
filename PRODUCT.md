# Product

<!-- impeccable:product-schema 1 -->

## Platform

Windows 10/11 desktop, x64.

## Stack

Rust with eframe/egui and native Windows APIs. Portable executable; no webview or background service.

## Users and Purpose

Windows users who want focused RAM monitoring, manual working-set trimming, and reversible login-startup management. The primary interface is Korean; documentation is available in Korean and English.

## Workflows

- Compact widget: RAM usage, a manual trim action, and expansion.
- Expanded widget: memory and total CPU readings, top-five process CPU/RAM with sortable columns, direct cleanup, and links to detailed management.
- Remember widget position, always-on-top setting, and compact/expanded mode. Restore safely after monitor changes.
- Use a nonselectable native drag header; keep action buttons independent.
- Sort process rows by name, numeric PID, CPU, working set, and eligibility/reason in either direction. Preserve selected process identities through sorting and refresh.
- Filter startup entries by enabled, disabled, and uncertain state. Pair visible state text with color and use switches only for directly manageable entries.
- Show command details in a distinct full-width panel with selected-row emphasis; close by button, Escape, or selecting the same row again.
- Register login autostart only after an explicit user action and allow it to be disabled.

## Capabilities and Constraints

- Show actual physical, available, commit, and process working-set memory.
- Manual trimming only: no process termination, automatic trimming, standby-list purge, kernel hooks, or service/driver modification.
- Limit trimming to eligible current-user/current-session processes; revalidate identity before changing a working set.
- Preserve the exact original data when disabling current-user Run registry values and Startup-folder entries; refuse conflicting restoration.
- Show all-user entries read-only and route Windows-managed/unsupported states to Windows settings.
- Do not present Windows-disabled or uncertain entries as enabled.
- Do not manage scheduled tasks, services, drivers, RunOnce, or policy-based startup entries.
- Do not promise sustained free RAM or a performance improvement. Working sets may grow again and subsequent access can become slower.

## Product Principles

- Every machine change follows a direct user action.
- Use documented Windows APIs and preserve recoverability.
- Show measured data, actionable failures, and clear management scope.
- Keep the two requested jobs easy to reach from a compact desktop surface.

## Brand

Memori: restrained ink-blue surfaces, mint instruments and actions, clear Korean labels, and a three-bar memory mark. Keep native keyboard and accessibility behavior.
