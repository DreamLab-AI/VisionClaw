/**
 * ShortcutSheet — the generated keyboard-shortcut list ('?', ADR-2189 §2).
 *
 * Lists the control centre's own bindings (sections 1–7 come from the registry)
 * and every shortcut registered at runtime in keyboardShortcutRegistry, so what it
 * shows is what is bound. It answers both events that used to reach no listener:
 * `controlcenter:help` (the '?' key) and `show-keyboard-shortcuts` (the command
 * palette and onboarding).
 */
import React, { useEffect, useState } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '../../design-system/components/Dialog';
import { keyboardShortcutRegistry, formatShortcut, type KeyboardShortcut } from '../../../hooks/useKeyboardShortcuts';
import { useControlCenterUI } from '../state/useControlCenterUI';
import { SECTION_BINDINGS, CONTROL_CENTRE_BINDINGS, SHORTCUT_SHEET_EVENTS, type Binding } from './controlCenterBindings';

const Row: React.FC<{ keys: string; description: string }> = ({ keys, description }) => (
  <div className="flex items-center justify-between gap-4 py-1">
    <span className="text-xs text-muted-foreground">{description}</span>
    <kbd className="shrink-0 rounded border border-border/60 bg-white/5 px-1.5 py-0.5 font-mono text-[11px] text-foreground">
      {keys}
    </kbd>
  </div>
);

const Group: React.FC<{ title: string; rows: Array<Pick<Binding, 'keys' | 'description'>> }> = ({ title, rows }) =>
  rows.length === 0 ? null : (
    <section aria-label={title} className="mb-3">
      <h3 className="cc-subgroup-label mb-1 pb-1 border-b border-border/40">{title}</h3>
      {rows.map((r) => (
        <Row key={`${title}:${r.keys}:${r.description}`} keys={r.keys} description={r.description} />
      ))}
    </section>
  );

/**
 * Live shortcuts from keyboardShortcutRegistry. Read from getShortcuts(), whose
 * entries keep their real `key` (getShortcutsByCategory() overwrites `key` with
 * the registration id, which would print the id instead of the key).
 */
function useRegisteredShortcuts(): KeyboardShortcut[] {
  const [list, setList] = useState<KeyboardShortcut[]>(() => [...keyboardShortcutRegistry.getShortcuts().values()]);
  useEffect(() => {
    const update = () => setList([...keyboardShortcutRegistry.getShortcuts().values()]);
    update();
    keyboardShortcutRegistry.addListener(update);
    return () => keyboardShortcutRegistry.removeListener(update);
  }, []);
  return list;
}

export const ShortcutSheet: React.FC = () => {
  const open = useControlCenterUI((s) => s.shortcutSheetOpen);
  const setOpen = useControlCenterUI((s) => s.setShortcutSheetOpen);
  const registered = useRegisteredShortcuts();

  useEffect(() => {
    const show = () => setOpen(true);
    for (const name of SHORTCUT_SHEET_EVENTS) window.addEventListener(name, show);
    return () => {
      for (const name of SHORTCUT_SHEET_EVENTS) window.removeEventListener(name, show);
    };
  }, [setOpen]);

  const app: Array<Pick<Binding, 'keys' | 'description'>> = [];
  registered.forEach((s) => {
    if (s.enabled === false) return;
    const category = s.category && s.category !== 'General' ? `${s.category}: ` : '';
    app.push({ keys: formatShortcut(s), description: `${category}${s.description}` });
  });

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent data-testid="shortcut-sheet" className="max-w-md">
        <DialogHeader>
          <DialogTitle>Keyboard shortcuts</DialogTitle>
          <DialogDescription>Number keys and / do nothing while you are typing in a field.</DialogDescription>
        </DialogHeader>
        <div className="max-h-[60vh] overflow-y-auto pr-1">
          <Group title="Sections" rows={SECTION_BINDINGS} />
          <Group title="Control centre" rows={CONTROL_CENTRE_BINDINGS} />
          <Group title="App" rows={app} />
        </div>
      </DialogContent>
    </Dialog>
  );
};

ShortcutSheet.displayName = 'ShortcutSheet';

export default ShortcutSheet;
