/**
 * Command-bar Jev flow (VC-06-S1-01, VC-06-S1-02, VC-34-S1-01).
 *
 * Runs only where the existing CommandInput would otherwise fall through to
 * the settings assistant (no deterministic keyword match). Returns a *review*
 * for the user, or `null` meaning "take the pre-change path". Jev never
 * applies anything: every review needs a user click, and applying a patch goes
 * through the existing settings store (authenticated sync path).
 */
import {
  jevUnderstandCommand,
  type CommandUnderstanding,
  type JevApi,
} from './jevClient';
import { buildPatch, shortlistSettings, type PatchField, type SettingsPatch } from './settingsPatch';

/** Typed intent → existing command-registry id. Anything unmapped is advice only. */
export const INTENT_TO_COMMAND_ID: Record<string, string | undefined> = {
  help: 'nav.help',
  settings_change: 'nav.settings',
  graph_search: undefined,
  view_save_load: undefined,
  analytics: undefined,
};

export type JevReview =
  | { kind: 'patch'; patch: SettingsPatch; confidence: number }
  | { kind: 'route'; classId: string; note: string }
  | { kind: 'intent'; intentId: string; commandId?: string; note: string };

export interface FlowField extends PatchField {
  current?: unknown;
}

const ROUTE_NOTES: Record<string, string> = {
  temporary_physics_action:
    'Looks like a one-off simulation action, not a saved preference. Use the physics controls (pause/resume/reset layout) instead.',
  saved_view_action: 'Looks like a saved-view action, not a settings change. Use the saved views controls instead.',
  clarify: 'This could mean a saved preference or a one-off action. Please say which, e.g. “set repulsion to 200” or “pause the simulation”.',
};

/** Plan a review for `command`. `null` ⇒ caller keeps its existing behaviour. */
export async function planJevCommand(
  command: string,
  fields: FlowField[],
  api?: JevApi,
): Promise<JevReview | null> {
  let u: CommandUnderstanding | null = null;
  try {
    const candidates = shortlistSettings(command, fields);
    u = await jevUnderstandCommand(command, candidates, api);
  } catch {
    return null;
  }
  if (!u) return null;

  // VC-06-S1-02: a non-persistent class never reaches the persisted-settings
  // assistant without the user's say-so.
  if (u.actionClass && u.actionClass.id !== 'persistent_setting') {
    return { kind: 'route', classId: u.actionClass.id, note: ROUTE_NOTES[u.actionClass.id] ?? ROUTE_NOTES.clarify };
  }

  // VC-06-S1-01: Jev chose the field; code assembles the patch.
  if (u.setting) {
    const field = fields.find((f) => f.path === u.setting!.id);
    if (field) {
      const r = buildPatch(command, field, field.current);
      if (r.ok) return { kind: 'patch', patch: r.patch, confidence: u.setting.confidence };
      if (r.reason === 'missing-value') {
        return {
          kind: 'route',
          classId: 'clarify',
          note: `That looks like “${field.label}”, but no value or direction was given. Try “set ${field.label.toLowerCase()} to …”.`,
        };
      }
    }
    return null;
  }

  // VC-34-S1-01: typed intent, advice or a registry command id — never an endpoint.
  if (u.intent) {
    const commandId = INTENT_TO_COMMAND_ID[u.intent.id];
    return {
      kind: 'intent',
      intentId: u.intent.id,
      commandId,
      note: `Looks like a “${u.intent.id.replace(/_/g, ' ')}” request.`,
    };
  }
  return null;
}
