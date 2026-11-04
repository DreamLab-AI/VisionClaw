import { commandRegistry } from '../command-palette/CommandRegistry';
import type { Command } from '../command-palette/types';
import { useSplatStore, type SplatState } from './splatStore';

const CATEGORY = 'splats';
const ID_PREFIX = 'splats.';

type Snapshot = Pick<SplatState, 'assets' | 'activeId' | 'mode'>;

/** The palette entries for the current splat state: open per asset, then enter, leave and close. */
export function buildSplatCommands(state: Snapshot, actions = useSplatStore.getState()): Command[] {
  const active = state.activeId !== null;
  const commands: Command[] = state.assets.map((a) => ({
    id: `${ID_PREFIX}open.${a.id}`,
    title: `Open splat scene: ${a.title}`,
    description: `${a.author} (${a.licence.spdx})`,
    category: CATEGORY,
    keywords: ['gaussian', 'splat', 'scene', a.id, a.title],
    handler: () => actions.open(a.id),
  }));
  commands.push(
    {
      id: `${ID_PREFIX}enter`,
      title: 'Enter splat scene',
      description: 'Make the open splat scene the environment around the graph',
      category: CATEGORY,
      keywords: ['gaussian', 'splat', 'environment'],
      enabled: active && state.mode === 'framed',
      handler: () => actions.enter(),
    },
    {
      id: `${ID_PREFIX}leave`,
      title: 'Leave splat scene',
      description: 'Return the splat scene to a framed volume beside the graph',
      category: CATEGORY,
      keywords: ['gaussian', 'splat', 'exit'],
      enabled: active && state.mode === 'entered',
      handler: () => actions.leave(),
    },
    {
      id: `${ID_PREFIX}close`,
      title: 'Close splat scene',
      description: 'Remove the splat scene and free its GPU memory',
      category: CATEGORY,
      keywords: ['gaussian', 'splat', 'hide'],
      enabled: active,
      handler: () => actions.close(),
    },
    {
      id: `${ID_PREFIX}refresh`,
      title: 'Refresh splat list',
      category: CATEGORY,
      keywords: ['gaussian', 'splat', 'reload'],
      handler: () => actions.loadListing(),
    },
  );
  return commands;
}

/**
 * Keep the palette's splat entries in step with the store. Returns the
 * unregister function for the component's cleanup.
 */
export function registerSplatCommands(): () => void {
  commandRegistry.registerCategory({ id: CATEGORY, name: 'Splats', priority: 6 });
  let ids: string[] = [];
  const sync = () => {
    ids.forEach((id) => commandRegistry.unregisterCommand(id));
    const next = buildSplatCommands(useSplatStore.getState());
    ids = next.map((c) => c.id);
    commandRegistry.registerCommands(next);
  };
  sync();
  const unsubscribe = useSplatStore.subscribe((s, prev) => {
    if (s.assets !== prev.assets || s.activeId !== prev.activeId || s.mode !== prev.mode) sync();
  });
  return () => {
    unsubscribe();
    ids.forEach((id) => commandRegistry.unregisterCommand(id));
    ids = [];
  };
}
