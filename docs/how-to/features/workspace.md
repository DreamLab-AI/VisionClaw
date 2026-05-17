---
title: Workspace Management
description: Create, organise, and restore named graph configurations using the Workspace Manager
category: how-to
tags: [workspace, layout, collaboration, sync]
updated-date: 2026-04-18
---

# Workspace Management

Workspaces let you save and restore named graph configurations — layout, physics settings, filter state — and share them with collaborators.

---

## Prerequisites

Workspace management must be enabled. If the `WorkspaceManager` shows a "Workspace management is disabled" message, toggle it on in settings:

```
workspace.enabled = true
```

---

## Access the Workspace Manager

The Workspace Manager is accessible two ways:

1. **Command Palette**: Press Ctrl+K / Cmd+K, type `workspace`.
2. **Enterprise drawer**: Press Ctrl+Shift+E / Cmd+Shift+E, navigate to the Workspace section.

---

## Create a Workspace

1. Enter a **name** in the "Enter workspace name..." field. Names are required; the Create button is disabled until a non-empty name is provided.
2. Optionally enter a **description**. Defaults to `"New workspace"` if left blank.
3. Click **Create Workspace** (or press Enter in the name field).

A success toast confirms creation. Workspaces are created with type `personal` by default. The new workspace appears immediately in the **Active** tab.

---

## Workspace Tabs

The manager shows three tabs:

| Tab | Contents |
|-----|---------|
| **Active** | Workspaces with status `active` — shown with member count and last-accessed time |
| **Favorites** | Workspaces you have starred |
| **Archived** | Workspaces moved to the archive |

Each tab shows the count in its label (e.g. "Active (3)").

---

## Workspace Card Actions

Each card exposes three icon buttons:

| Icon | Action |
|------|--------|
| Star | Toggle favourite — adds to / removes from the Favorites tab |
| Settings (gear) | Archive the workspace — moves it from Active to Archived |
| Folder (restore) | Restore an archived workspace back to Active |
| Trash | Delete — prompts for confirmation; permanent and irreversible |

The workspace card also shows:
- Member count and type badge (`personal`, `team`, or `public`)
- Time since last access (e.g. "2h ago")
- Creation date

---

## Real-Time Sync

`useWorkspaces` is called with `enableRealtime: true`, meaning changes made in other browser tabs are reflected live without a manual refresh. If a workspace is created, archived, or deleted in another tab, the list updates automatically.

---

## Workspace Settings

The bottom of the manager card contains toggle buttons for per-workspace features. These map directly to settings store paths:

| Toggle | Setting path | Default behaviour |
|--------|-------------|-------------------|
| Auto Save | `workspace.autoSave.enabled` | Persist workspace state automatically on change |
| Sync Settings | `workspace.sync.enabled` | Sync workspace to server |
| Collaboration | `workspace.collaboration.enabled` | Allow multi-user workspace sharing |
| Auto Backup | `workspace.backup.enabled` | Periodic backup of workspace state |

Additional read-only values shown:
- `workspace.limits.maxWorkspaces` — maximum number of workspaces allowed
- `workspace.layout.default` — default layout mode applied to new workspaces

---

## Workspace data model (API)

From `workspaceApi.ts`:

```typescript
interface Workspace {
  id: string;
  name: string;
  description: string;
  type: 'personal' | 'team' | 'public';
  status: 'active' | 'archived';
  favorite: boolean;
  memberCount: number;
  lastAccessed: Date;
  createdAt: Date;
}
```

---

## Troubleshooting

**"Error loading workspaces" banner**: A network error prevented the workspace list from loading. Click **Try Again** to retry the fetch. Check the backend health dashboard if the error persists.

**Create button stays disabled**: The name field is empty or contains only whitespace.

**Workspace missing from Active tab**: It may have been archived. Check the Archived tab; use the restore (folder) icon to move it back.

---

## See Also

- [Command Palette](command-palette.md) — keyboard-first navigation
- [Configuration](../operations/configuration.md) — `workspace.*` environment variables
- [System Health Monitoring](monitoring.md) — verify backend is healthy before troubleshooting workspace sync
