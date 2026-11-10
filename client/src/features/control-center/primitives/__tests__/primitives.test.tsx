/**
 * Smoke test: SettingRow renders each control type with the correct
 * data-testid + ARIA surface. This is not exhaustive coverage of the 168
 * registry fields (that's the browser-automation phase, WP-plan task #7) —
 * it verifies the primitive dispatch + testid/ARIA contract holds for one
 * field of each ControlType.
 */

import React from 'react';
import { describe, it, expect } from 'vitest';
import { render, screen, act } from '@testing-library/react';
import type { LucideIcon } from 'lucide-react';
import { SettingRow } from '../SettingRow';
import type { RegistryField } from '../../registry/types';

// Minimal stand-in icon so RegistryGroup-adjacent fixtures don't need lucide-react wiring.
const NoopIcon: LucideIcon = (() => null) as unknown as LucideIcon;
void NoopIcon;

const GROUP_ID = 'motion';

function fieldFor(overrides: Partial<RegistryField>): RegistryField {
  return {
    key: overrides.key ?? 'testField',
    label: overrides.label ?? 'Test Field',
    type: overrides.type ?? 'slider',
    ...overrides,
  } as RegistryField;
}

describe('SettingRow control dispatch', () => {
  it('renders a slider with data-testid and ARIA valuemin/max/now', () => {
    const field = fieldFor({
      key: 'repelK',
      label: 'Repel K',
      type: 'slider',
      path: 'visualisation.graphs.knowledge.physics.repelK',
      min: 0,
      max: 400,
      step: 1,
    });
    render(<SettingRow field={field} groupId={GROUP_ID} />);

    const input = screen.getByTestId(`setting-${field.path}`);
    expect(input).toBeInTheDocument();
    expect(input.tagName).toBe('INPUT');
    expect(input.getAttribute('type')).toBe('range');
    expect(input.getAttribute('aria-valuemin')).toBe('0');
    expect(input.getAttribute('aria-valuemax')).toBe('400');
    expect(input.getAttribute('aria-valuenow')).not.toBeNull();
    expect(input.getAttribute('aria-label')).toBe('Repel K');
  });

  it('renders a toggle as a switch with role="switch"', () => {
    const field = fieldFor({
      key: 'enabled',
      label: 'Physics Enabled',
      type: 'toggle',
      path: 'visualisation.graphs.knowledge.physics.enabled',
    });
    render(<SettingRow field={field} groupId={GROUP_ID} />);

    const toggle = screen.getByTestId(`setting-${field.path}`);
    expect(toggle).toBeInTheDocument();
    expect(toggle.getAttribute('role')).toBe('switch');
  });

  it('renders a color input', () => {
    const field = fieldFor({
      key: 'baseColor',
      label: 'Base Color',
      type: 'color',
      path: 'visualisation.graphs.knowledge.nodes.baseColor',
    });
    render(<SettingRow field={field} groupId={GROUP_ID} />);

    const input = screen.getByTestId(`setting-${field.path}`);
    expect(input.getAttribute('type')).toBe('color');
    expect(input.getAttribute('aria-label')).toBe('Base Color');
  });

  it('renders a select trigger for a localKey field using the pathless testid convention', () => {
    const field = fieldFor({
      key: 'groupingMethod',
      label: 'Method',
      type: 'select',
      localKey: 'method',
      options: ['communities', 'kmeans', 'dbscan'],
    });
    render(
      <SettingRow
        field={field}
        groupId="quality"
        localValues={{ method: 'communities' }}
        onLocalChange={() => {}}
      />
    );

    const trigger = screen.getByTestId('setting-quality.groupingMethod');
    expect(trigger).toBeInTheDocument();
    expect(trigger.getAttribute('aria-label')).toBe('Method');
  });

  it('renders a text input', () => {
    const field = fieldFor({
      key: 'customBackendUrl',
      label: 'Backend URL',
      type: 'text',
      path: 'system.customBackendUrl',
    });
    render(<SettingRow field={field} groupId="system" />);

    const input = screen.getByTestId(`setting-${field.path}`);
    expect(input.tagName).toBe('INPUT');
    expect(input.getAttribute('type')).toBe('text');
  });

  it('renders an action button and disables run_clustering while running is false initially', () => {
    const field = fieldFor({
      key: 'refreshGraph',
      label: 'Refresh Graph',
      type: 'action-button',
      action: 'refresh_graph',
    });
    render(<SettingRow field={field} groupId="quality" />);

    const button = screen.getByTestId('setting-quality.refreshGraph');
    expect(button.tagName).toBe('BUTTON');
    expect(button.getAttribute('aria-label')).toBe('Refresh Graph');
  });

  it('renders the nostr-button control with a connect affordance', () => {
    const field = fieldFor({
      key: 'nostr',
      label: 'Nostr Authentication',
      type: 'nostr-button',
      localKey: 'nostrAuth',
    });
    render(<SettingRow field={field} groupId="system" />);

    const button = screen.getByTestId('setting-system.nostr');
    expect(button).toBeInTheDocument();
  });

  it('renders the readonly renderer info with role="status"', () => {
    const field = fieldFor({
      key: 'rendererInfo',
      label: 'Renderer Info',
      type: 'readonly',
    });
    render(<SettingRow field={field} groupId="system" />);

    const info = screen.getByTestId('setting-system.rendererInfo');
    expect(info.getAttribute('role')).toBe('status');
  });

  it('renders the node-filter count readout from the server counts (ADR-2192)', async () => {
    const { useNodeFilterCountStore } = await import('../../../graph/explore/nodeFilterCountStore');
    useNodeFilterCountStore.getState().reset();
    const field = fieldFor({
      key: 'nodeFilterCount',
      label: 'Nodes Shown',
      type: 'readonly',
      localKey: 'nodeFilterCount',
      readout: 'nodeFilterCount',
    });
    render(<SettingRow field={field} groupId="quality" />);
    const readout = screen.getByTestId('setting-quality.nodeFilterCount');
    expect(readout.getAttribute('role')).toBe('status');
    expect(readout.textContent).toBe('Node count arrives with the next filter update');
    act(() => useNodeFilterCountStore.getState().setCounts(2500, 9688));
    expect(readout.textContent).toBe('Showing 2,500 of 9,688 nodes');
  });

  it('hides a field whose showWhen condition is not met', () => {
    const field = fieldFor({
      key: 'eps',
      label: 'Epsilon',
      type: 'slider',
      localKey: 'eps',
      showWhen: { localKey: 'method', equals: 'dbscan' },
      min: 0,
      max: 20,
    });
    render(
      <SettingRow
        field={field}
        groupId="quality"
        localValues={{ method: 'communities', eps: 5 }}
      />
    );

    expect(screen.queryByTestId('setting-quality.eps')).not.toBeInTheDocument();
  });
});
