import React from 'react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react';

const demo = { running: false, error: null as string | null, busy: false, toggle: vi.fn(async () => {}) };

vi.mock('../../../bots/agentDemo', () => ({ useAgentDemo: () => demo }));
vi.mock('../../../bots/contexts/BotsDataContext', () => ({ useBotsDataOptional: () => null }));
vi.mock('../../../bots/components/AgentDetailPanel', () => ({ AgentDetailPanel: () => <div /> }));
vi.mock('../../../bots/components/SwarmObservabilityPanel', () => ({ SwarmObservabilityPanel: () => <div /> }));
vi.mock('../../../bots/chain/ChainPaymentsPanel', () => ({ ChainPaymentsPanel: () => <div /> }));

import { AgentOpsSurface, OPEN_AGENT_OPS_EVENT } from '../AgentOpsSurface';

const openPanel = () => {
  render(<AgentOpsSurface />);
  window.dispatchEvent(new Event(OPEN_AGENT_OPS_EVENT));
};

describe('Agents panel: the shared demo control (ADR-2142)', () => {
  beforeEach(() => { demo.running = false; demo.error = null; demo.busy = false; demo.toggle.mockClear(); });
  afterEach(cleanup);

  it('offers Start when the demo is not running and calls the shared toggle', async () => {
    openPanel();
    const btn = await screen.findByTestId('agent-demo-toggle');
    expect(btn).toHaveTextContent('Start Agent Demo');
    expect(btn).toHaveAttribute('aria-pressed', 'false');
    fireEvent.click(btn);
    expect(demo.toggle).toHaveBeenCalledTimes(1);
  });

  it('reads as running when the SERVER says so (e.g. started from the headset)', async () => {
    demo.running = true;
    openPanel();
    const btn = await screen.findByTestId('agent-demo-toggle');
    expect(btn).toHaveTextContent('Stop Agent Demo');
    expect(btn).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByTestId('agent-demo-row')).toHaveTextContent('Demo running for all clients');
  });

  it('shows the server refusal rather than pretending it started, and disables the button while busy', async () => {
    demo.error = 'the graph has no nodes the demo agents can work on';
    demo.busy = true;
    openPanel();
    await waitFor(() => expect(screen.getByTestId('agent-demo-error')).toHaveTextContent('no nodes'));
    expect(screen.getByTestId('agent-demo-toggle')).toBeDisabled();
  });
});
