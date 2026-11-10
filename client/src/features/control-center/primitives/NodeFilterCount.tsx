/**
 * NodeFilterCount — readonly "Showing N of M nodes" for the node-filter panel (ADR-2192).
 * N is what the server streams to this client after scope, predicates and the `maxNodes`
 * cap; M is the nodes in this client's graph view. Both come from the server's
 * `filter_update_success` reply, so the line shows what the server actually sent.
 */
import React from 'react';
import type { RegistryField } from '../registry/types';
import { useNodeFilterCountStore, formatFilterCounts } from '../../graph/explore/nodeFilterCountStore';

export interface NodeFilterCountProps {
  field: RegistryField;
  testId: string;
}

export const NodeFilterCount: React.FC<NodeFilterCountProps> = ({ field, testId }) => {
  const visible = useNodeFilterCountStore((s) => s.visible);
  const total = useNodeFilterCountStore((s) => s.total);
  return (
    <div id={testId} data-testid={testId} role="status" aria-label={field.label} className="cc-helper-text">
      {formatFilterCounts({ visible, total })}
    </div>
  );
};

NodeFilterCount.displayName = 'NodeFilterCount';
