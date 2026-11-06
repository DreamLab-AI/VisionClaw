/**
 * SettingsPanel — the slide-out settings host. design-spec.md §2, §6.1, §6.3.
 *
 * Layout (ADR-2189): a left rail of the seven sections (hotkeys 1–7) and an
 * active body. The body shows the section's subsections, and launchers for the
 * bespoke panels placed in it (Solid Pod in System › Account & sync, the
 * ontology browser in Explore › Ontology). The search box searches EVERY
 * section by label, key, description, dot-path or subsection, and labels each
 * hit with its section › subsection so a result also teaches where it lives.
 *
 * Non-modal region (§6.3): the canvas stays interactive behind it, so this is a
 * `role="region"` slide-out, not a focus trap — the palette remains the only
 * modal surface.
 */

import React, { useEffect, useMemo, useState } from 'react';
import { X } from 'lucide-react';
import { ChevronLeft } from 'lucide-react';
import { REGISTRY, GROUP_BY_ID, ALL_FIELDS, testIdFor } from '../registry/settingsRegistry';
import type { RegistryField, RegistryGroup } from '../registry/types';
import { PANELS } from '../registry/manifest';
import { GlassPanel } from '../primitives/GlassPanel';
import { GroupSection } from '../primitives/GroupSection';
import { SettingRow } from '../primitives/SettingRow';
import { SearchInput } from '../../design-system/components/SearchInput';
import { SolidPanel } from './SolidPanel';
import { OntologyPanel } from './OntologyPanel';
import { useControlCenterUI } from '../state/useControlCenterUI';
import { useEnsureGroupLoaded } from '../state/useEnsureGroupLoaded';
import { useLocalFieldMap } from '../hooks/useSettingField';
import '../styles/control-center.css';

const BESPOKE_IDS = new Set(PANELS.map((p) => p.id));

/**
 * Seed values for every transient `localKey` field across the registry. localKeys
 * are globally unique, so a single panel-scoped map covers all groups. Seeding
 * (not leaving fields undefined) is what makes the Run Grouping method select show
 * a concrete value AND its showWhen-dependent sliders render on first open — see
 * defect-1. Falls back to the select's first option / a slider's min when a field
 * declares no explicit `default`.
 */
const INITIAL_LOCAL_VALUES: Record<string, unknown> = Object.fromEntries(
  ALL_FIELDS.filter((f) => f.localKey).map((f) => [
    f.localKey as string,
    f.default ?? (f.type === 'select' ? f.options?.[0] : f.min),
  ]),
);

export const SettingsPanel: React.FC = () => {
  const openPanel = useControlCenterUI((s) => s.openPanel);
  const activeGroup = useControlCenterUI((s) => s.activeGroup);
  const openGroup = useControlCenterUI((s) => s.openGroup);
  const closePanel = useControlCenterUI((s) => s.closePanel);
  const ensureGroupLoaded = useEnsureGroupLoaded();

  // Transient inputs for the Run Grouping action (method/params). These are one-shot
  // POST-body inputs, not settings paths, so they live in a panel-local map rather
  // than the settings store. WP1 built GroupSection/SettingRow/SettingAction to accept
  // localValues + onLocalChange; this lifts and threads them (defect-1: WP2 omitted it).
  const { values: localValues, setValue: onLocalChange } = useLocalFieldMap(INITIAL_LOCAL_VALUES);

  const [query, setQuery] = useState('');

  const group = activeGroup && GROUP_BY_ID[activeGroup] ? GROUP_BY_ID[activeGroup] : null;
  const bespoke = PANELS.find((p) => p.id === activeGroup) ?? null;

  const filtered = useMemo((): Array<{ section: RegistryGroup; field: RegistryField }> => {
    if (!query.trim()) return [];
    const q = query.trim().toLowerCase();
    // Match label, key, and description (the spec's user-facing search surface),
    // plus the backend dot-path, the subsection and the section label. Searches
    // every section, so a control is findable without knowing where it lives.
    return REGISTRY.flatMap((section) =>
      section.fields
        .filter(
          (f) =>
            f.label.toLowerCase().includes(q) ||
            f.key.toLowerCase().includes(q) ||
            (f.description ?? '').toLowerCase().includes(q) ||
            (f.path ?? '').toLowerCase().includes(q) ||
            (f.subgroup ?? '').toLowerCase().includes(q) ||
            section.label.toLowerCase().includes(q),
        )
        .map((field) => ({ section, field })),
    );
  }, [query]);

  // Search mode swaps GroupSection (which owns the normal-path hydration) for a
  // flat result list, so hydrate every section that has a hit.
  useEffect(() => {
    const hit = new Set(filtered.map((r) => r.section.id));
    for (const id of hit) ensureGroupLoaded(GROUP_BY_ID[id]);
  }, [filtered, ensureGroupLoaded]);

  const renderBody = () => {
    if (query.trim()) {
      return (
        <div role="region" aria-label="Search results in every section" className="flex flex-col gap-0.5">
          {filtered.length === 0 ? (
            <p className="cc-helper-text p-4 text-center">No settings match “{query}”.</p>
          ) : (
            filtered.map(({ section, field }) => (
              <div key={testIdFor(field, section.id)}>
                <div className="cc-helper-text mt-1 text-[10px]" data-testid="search-result-location">
                  {section.label} › {field.subgroup}
                </div>
                <SettingRow field={field} groupId={section.id} localValues={localValues} onLocalChange={onLocalChange} />
              </div>
            ))
          )}
        </div>
      );
    }
    if (bespoke) {
      const home = GROUP_BY_ID[bespoke.section];
      return (
        <div className="flex flex-col gap-2">
          <button
            type="button"
            onClick={() => openGroup(bespoke.section)}
            className="self-start flex items-center gap-1 text-[11px] text-muted-foreground hover:text-foreground"
          >
            <ChevronLeft size={12} aria-hidden="true" /> {home.label} › {bespoke.subsection}
          </button>
          {bespoke.id === 'solid' ? <SolidPanel /> : <OntologyPanel />}
        </div>
      );
    }
    if (!group) {
      return (
        <p className="cc-helper-text p-4 text-center">
          Pick a section from the rail, or press 1–7.
        </p>
      );
    }
    const launchers = PANELS.filter((p) => p.section === group.id);
    return (
      <>
        <GroupSection
          key={group.id}
          group={group}
          localValues={localValues}
          onLocalChange={onLocalChange}
          onFirstMount={ensureGroupLoaded}
        />
        {launchers.map((p) => (
          <div key={p.id} className="mt-3">
            <div className="cc-subgroup-label mb-1 pb-1 border-b border-border/40">{p.subsection}</div>
            <button
              type="button"
              data-testid={p.testid}
              onClick={() => openGroup(p.id)}
              className="w-full rounded-md border border-border/50 px-2 py-1.5 text-left text-xs text-foreground hover:bg-white/5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            >
              Open {p.label}
            </button>
          </div>
        ))}
      </>
    );
  };

  return (
    <GlassPanel
      elevation="overlay"
      accent
      animated
      role="region"
      aria-label="Settings"
      aria-hidden={!openPanel}
      // Closed, the panel is off-screen: `inert` also removes it from the tab order.
      inert={!openPanel}
      data-testid="settings-panel"
      data-open={openPanel}
      className="fixed left-4 top-4 z-40 w-[380px] max-w-[92vw] flex overflow-hidden"
      style={{
        // ADR-2189 §4: end just above the dock, whatever its height.
        bottom: 'var(--cc-dock-clearance, 9rem)',
        transform: openPanel ? 'translateX(0)' : 'translateX(-112%)',
        opacity: openPanel ? 1 : 0,
        pointerEvents: openPanel ? 'auto' : 'none',
      }}
    >
      {/* Left rail: the seven sections */}
      <nav
        aria-label="Settings sections"
        className="shrink-0 w-14 flex flex-col items-stretch gap-1 p-1.5 border-r border-border/40 overflow-y-auto"
      >
        {REGISTRY.map((g) => {
          const Icon = g.icon;
          const isActive = activeGroup === g.id;
          return (
            <button
              key={g.id}
              type="button"
              data-testid={`group-rail-${g.id}`}
              aria-label={`${g.label} (${g.hotkey})`}
              aria-keyshortcuts={g.hotkey}
              aria-current={isActive ? 'true' : undefined}
              title={`${g.label} · ${g.hotkey}`}
              onClick={() => openGroup(g.id)}
              className={[
                'relative flex flex-col items-center gap-0.5 py-2 rounded-md transition-colors',
                'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
                isActive ? 'bg-primary/20 text-primary' : 'text-muted-foreground hover:text-foreground hover:bg-white/5',
              ].join(' ')}
            >
              {Icon && <Icon size={16} aria-hidden="true" />}
              {g.hotkey && (
                <span className="cc-value-readout leading-none" aria-hidden="true">
                  {g.hotkey}
                </span>
              )}
            </button>
          );
        })}

      </nav>

      {/* Body */}
      <div className="flex-1 min-w-0 flex flex-col">
        <header className="flex items-center gap-2 p-2 border-b border-border/40">
          {/* The filter is intentionally hidden for BESPOKE_IDS (Solid / Ontology):
              those rail entries render their own bespoke panels (SolidPanel /
              OntologyPanel) rather than flat registry rows, so there are no
              `group.fields` to narrow — a search box there would be dead. */}
          {!BESPOKE_IDS.has(activeGroup ?? '') && (
            <div className="flex-1 min-w-0">
              <SearchInput
                value={query}
                onChange={setQuery}
                placeholder="Search all settings…"
                aria-label="Search all settings"
                className="text-xs"
              />
            </div>
          )}
          {BESPOKE_IDS.has(activeGroup ?? '') && (
            <span className="cc-title flex-1 min-w-0 truncate">
              {PANELS.find((p) => p.id === activeGroup)?.label}
            </span>
          )}
          <button
            type="button"
            data-testid="settings-panel-close"
            aria-label="Close settings"
            onClick={closePanel}
            className="shrink-0 flex items-center justify-center h-7 w-7 rounded-md text-muted-foreground hover:text-foreground hover:bg-white/5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            <X size={15} aria-hidden="true" />
          </button>
        </header>

        <div className="flex-1 min-h-0 overflow-y-auto p-2">
          {openPanel && renderBody()}
        </div>
      </div>
    </GlassPanel>
  );
};

SettingsPanel.displayName = 'SettingsPanel';

export default SettingsPanel;
