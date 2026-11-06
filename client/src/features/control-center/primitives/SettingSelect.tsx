/**
 * SettingSelect — design-system (Radix) Select. design-spec.md §2.
 * Radix renders an ARIA combobox/listbox rather than a native <select>;
 * data-testid lives on the trigger, which is the interactive, focusable
 * element CDP automation drives.
 */

import React, { useCallback } from 'react';
import type { RegistryField } from '../registry/types';
import {
  Select,
  SelectTrigger,
  SelectValue,
  SelectContent,
  SelectItem,
} from '../../design-system/components/Select';
import { emitEchoPulse } from '../echo/echoPulseBus';

export interface SettingSelectProps {
  field: RegistryField;
  testId: string;
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
}

/**
 * Registry `options` are the REAL stored enum values (lowercase/kebab, per the
 * frozen backend contract). The *stored* value must therefore be what
 * `SelectItem.value` carries — that is what lets Radix mark the current option
 * `aria-selected` (matching against the store value) and echo it in the trigger.
 * The visible label is a light title-case view derived here, so no per-field
 * label map is needed in the registry (RegistryField.options is `string[]` only).
 */
function labelize(value: string): string {
  if (!value) return value;
  return value.charAt(0).toUpperCase() + value.slice(1);
}

export const SettingSelect: React.FC<SettingSelectProps> = ({ field, testId, value, onChange, disabled }) => {
  // ADR-2189 §6: a field may name its options ("sssp" → "Graph distance (SSSP)").
  const display = (option: string) => field.optionLabels?.[option] ?? labelize(option);
  const handleValueChange = useCallback(
    (next: string) => {
      onChange(next);
      emitEchoPulse({ origin: 'camera-center', strength: 0.3 });
    },
    [onChange]
  );

  return (
    <Select value={value} onValueChange={handleValueChange} disabled={disabled}>
      <SelectTrigger id={testId} data-testid={testId} aria-label={field.label} className="h-7 text-xs">
        {/* Render the current value's label deterministically so the trigger
            reflects the store even before the listbox is first opened (Radix
            otherwise resolves item text lazily on open). Empty value falls
            through to the placeholder. */}
        <SelectValue placeholder={field.options?.[0] ? display(field.options[0]) : undefined}>
          {value ? display(value) : null}
        </SelectValue>
      </SelectTrigger>
      <SelectContent>
        {field.options?.map((option) => (
          <SelectItem key={option} value={option}>
            {display(option)}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
};

SettingSelect.displayName = 'SettingSelect';

export default SettingSelect;
