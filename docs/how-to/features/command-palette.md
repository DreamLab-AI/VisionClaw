---
title: Command Palette
description: Use and extend the command palette — fuzzy search, keyboard navigation, custom command registration
category: how-to
tags: [command-palette, keyboard, shortcuts]
updated-date: 2026-04-18
---

# Command Palette

The command palette provides keyboard-first access to all registered commands. Open it from anywhere in the application.

---

## Open and Close

| Action | Keys |
|--------|------|
| Open | Ctrl+K (Windows/Linux) / Cmd+K (Mac) |
| Close | Escape |

---

## Search

Type to filter commands. The registry uses a scored search across multiple fields:

| Match type | Score |
|-----------|-------|
| Exact title match | 1.0 |
| Title starts with query | 0.8 |
| Title contains query | 0.6 |
| Description contains query | 0.4 |
| Keyword match | 0.3 |
| Fuzzy title match | variable (> threshold) |

The default fuzzy search threshold is `0.3`. Results are sorted by descending score. Commands with `enabled: false` are excluded.

---

## Keyboard Navigation

| Key | Action |
|-----|--------|
| Arrow Up / Down | Move selection |
| Enter | Execute selected command |
| Escape | Close without executing |

---

## Command Structure

Every command conforms to the `Command` interface:

```typescript
interface Command {
  id: string;                  // unique identifier, e.g. 'nav.settings'
  title: string;               // display name shown in the palette
  description?: string;        // subtitle shown below the title
  category: string;            // groups commands visually
  keywords?: string[];         // additional search terms
  icon?: React.ComponentType;  // lucide-react icon
  shortcut?: {
    key: string;
    ctrl?: boolean;
    alt?: boolean;
    shift?: boolean;
    meta?: boolean;
  };
  handler: () => void | Promise<void>;
  enabled?: boolean;           // false = hidden from search
}
```

---

## Default Categories and Commands

Categories are ordered by priority (lower number = higher priority):

### Navigation (priority 1)

| Command ID | Title | Shortcut |
|-----------|-------|---------|
| `nav.settings` | Open Settings | — |
| `nav.help` | Show Help | — |

### Settings (priority 2)

| Command ID | Title | Shortcut |
|-----------|-------|---------|
| `settings.undo` | Undo Settings Change | Ctrl+Z |
| `settings.redo` | Redo Settings Change | Ctrl+Shift+Z |
| `settings.reset` | Reset All Settings | — |
| `settings.export` | Export Settings | — |
| `settings.import` | Import Settings | — |

### View (priority 3)

| Command ID | Title | Shortcut |
|-----------|-------|---------|
| `view.fullscreen` | Toggle Fullscreen | Ctrl+Shift+F |
| `view.theme.toggle` | Toggle Theme | — |
| `view.refresh` | Refresh View | Ctrl+R |

### Help (priority 4)

| Command ID | Title | Shortcut |
|-----------|-------|---------|
| `help.search` | Search Help Topics | — |
| `help.keyboard` | Show Keyboard Shortcuts | Shift+? |
| `help.tour` | Start Tutorial Tour | — |
| `onboarding.welcome` | Start Welcome Tour | — |
| `onboarding.settings` | Start Settings Tour | — |
| `onboarding.advanced` | Advanced Features Tour | — |
| `onboarding.reset` | Reset All Tours | — |

### System (priority 5)

| Command ID | Title | Shortcut |
|-----------|-------|---------|
| `system.reload` | Reload Application | Ctrl+Shift+R |
| `system.save` | Save All Changes | Ctrl+S |

---

## Recent Commands

The registry tracks the last N executed commands (default: 5). Recent commands are prepended to the palette list when the search query is empty.

Recent command IDs are persisted to `localStorage` under `commandPalette.recentCommands` as a JSON string array.

Configure the limit:

```typescript
import { CommandRegistry } from '@/features/command-palette/CommandRegistry';

const registry = new CommandRegistry({ maxRecentCommands: 10 });
```

The global singleton `commandRegistry` is created with default options (5 recent commands).

---

## Register a Custom Command

Import the global registry and call `registerCommand` or `registerCommands`:

```typescript
import { commandRegistry } from '@/features/command-palette/CommandRegistry';

commandRegistry.registerCommand({
  id: 'myfeature.do-something',
  title: 'Do Something',
  description: 'Performs a custom action',
  category: 'system',
  keywords: ['custom', 'action'],
  handler: async () => {
    await performCustomAction();
  },
});
```

Commands registered after the palette is open are available on the next search immediately — the registry notifies all listeners on registration.

### Unregister a command

```typescript
commandRegistry.unregisterCommand('myfeature.do-something');
```

### Register a category

```typescript
import { commandRegistry } from '@/features/command-palette/CommandRegistry';

commandRegistry.registerCategory({
  id: 'myfeature',
  name: 'My Feature',
  priority: 6,   // rendered after system (priority 5)
});
```

---

## CommandRegistry options

```typescript
interface CommandRegistryOptions {
  maxRecentCommands?: number;     // default: 5
  fuzzySearchThreshold?: number;  // default: 0.3  (0–1 scale)
}
```

---

## See Also

- [Onboarding](onboarding.md) — the command palette step in the Welcome Tour
- [Keyboard Shortcuts](../../reference/glossary.md) — full shortcut reference (Shift+? in-app)
- [Navigation Guide](../navigation-guide.md) — mouse and keyboard graph controls
