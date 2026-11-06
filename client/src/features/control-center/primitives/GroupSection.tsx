/**
 * GroupSection — renders one RegistryGroup: subgroup dividers + SettingRow list.
 * design-spec.md §2, §3.1, §4.2.
 *
 * Hydration (`ensureLoaded(group.loadPaths)`) is WP2's concern — this
 * component only invokes the `onFirstMount` callback once, on mount; the
 * caller wires it to `useEnsureGroupLoaded`.
 */

import React, { useEffect, useRef } from 'react';
import type { RegistryField, RegistryGroup } from '../registry/types';
import { SettingRow, testIdFor } from './SettingRow';
import '../styles/control-center.css';

export interface GroupSectionProps {
  group: RegistryGroup;
  /** Values for this group's transient localKey fields, keyed by localKey. */
  localValues?: Record<string, unknown>;
  onLocalChange?: (localKey: string, value: unknown) => void;
  /** Called exactly once on mount — wire to useEnsureGroupLoaded(group). */
  onFirstMount?: (group: RegistryGroup) => void;
  /** Field keys to render disabled (e.g. dimmed while a co-driving macro is active). */
  disabledKeys?: Set<string>;
  className?: string;
}

function isFieldVisible(field: RegistryField, localValues: Record<string, unknown> | undefined): boolean {
  if (!field.showWhen) return true;
  const current = localValues?.[field.showWhen.localKey];
  return current === field.showWhen.equals;
}

export const GroupSection: React.FC<GroupSectionProps> = ({
  group,
  localValues,
  onLocalChange,
  onFirstMount,
  disabledKeys,
  className,
}) => {
  const mountedRef = useRef(false);

  useEffect(() => {
    if (mountedRef.current) return;
    mountedRef.current = true;
    onFirstMount?.(group);
    // Intentionally run once on mount only — group identity does not change.
  }, []);

  const visibleFields = group.fields.filter((field) => isFieldVisible(field, localValues));

  return (
    <div
      data-testid={`group-${group.id}`}
      role="region"
      aria-label={group.label}
      className={className}
    >
      <h3 className="cc-title flex items-center gap-2 sticky top-0 py-1 z-10">
        <group.icon size={14} aria-hidden="true" />
        {group.label}
      </h3>
      {group.description && <p className="cc-helper-text mb-2">{group.description}</p>}

      <div className="flex flex-col gap-0.5">
        {visibleFields.map((field, idx) => {
          const prevSubgroup = idx > 0 ? visibleFields[idx - 1].subgroup : undefined;
          const showDivider = !!field.subgroup && field.subgroup !== prevSubgroup;

          return (
            // Field keys are unique per source group, not per section: key by test id.
            <React.Fragment key={testIdFor(field, group.id)}>
              {showDivider && (
                <div className="cc-subgroup-label mt-2 mb-1 pb-1 border-b border-border/40">
                  {field.subgroup}
                </div>
              )}
              <SettingRow
                field={field}
                groupId={group.id}
                localValues={localValues}
                onLocalChange={onLocalChange}
                disabled={disabledKeys?.has(field.key)}
              />
            </React.Fragment>
          );
        })}
      </div>
    </div>
  );
};

GroupSection.displayName = 'GroupSection';

export default GroupSection;
