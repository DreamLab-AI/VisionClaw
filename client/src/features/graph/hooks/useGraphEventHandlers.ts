import { useCallback, useRef, useEffect } from 'react';
import { ThreeEvent } from '@react-three/fiber';
import * as THREE from 'three';
import { throttle } from 'lodash';
import { graphDataManager, type GraphData, type Node } from '../managers/graphDataManager';
// ADR-03 D7: graphWorkerProxy pin/unpin/position calls are server-authoritative now.
// import { graphWorkerProxy } from '../managers/graphWorkerProxy';
import { createBinaryNodeData, BinaryNodeData } from '../../../types/binaryProtocol';
import { createLogger } from '../../../utils/loggerConfig';
import { debugState } from '../../../utils/clientDebugState';
import { useGraphInteraction } from '../../visualisation/hooks/useGraphInteraction';
import { startDragHeartbeat } from './dragHeartbeat';

const logger = createLogger('useGraphEventHandlers');

const DRAG_THRESHOLD = 5; 
const BASE_SPHERE_RADIUS = 0.5;
const POSITION_UPDATE_THROTTLE_MS = 100; 

// Helper function to slugify node labels
const slugifyNodeLabel = (label: string): string => {
  return label
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
};

export const useGraphEventHandlers = (
  meshRef: React.RefObject<THREE.InstancedMesh>,
  dragDataRef: React.MutableRefObject<any>,
  setDragState: React.Dispatch<React.SetStateAction<{ nodeId: string | null; instanceId: number | null }>>,
  graphData: any,
  /** The nodes array that GemNodes is actually rendering (may be a filtered subset of graphData.nodes) */
  displayNodes: Node[],
  camera: THREE.Camera,
  size: { width: number; height: number },
  settings: any,
  setGraphData: React.Dispatch<React.SetStateAction<any>>,
  onDragStateChange?: (isDragging: boolean) => void,
  onNodeSelect?: (nodeId: string | null) => void
) => {
  
  const {
    startInteraction,
    endInteraction,
    updateNodePosition,
    shouldSendPositionUpdates,
    flushPositionUpdates
  } = useGraphInteraction({
    positionUpdateThrottleMs: POSITION_UPDATE_THROTTLE_MS,
    onInteractionStateChange: onDragStateChange
  });

  
  const throttledWebSocketUpdate = useRef(
    throttle((nodeId: string, position: { x: number; y: number; z: number }) => {

      if (shouldSendPositionUpdates()) {
        const numericId = graphDataManager.nodeIdMap.get(nodeId);
        if (numericId !== undefined && graphDataManager.webSocketService?.isReady()) {
          // Canonical drag frame: server-side drag update (JSON) for
          // pin-at-position + fast-settle. The legacy binary position frame
          // (sendNodePositionUpdates) was removed 2026-06-03 — it sent a
          // redundant SECOND frame per drag move, duplicating this update.
          graphDataManager.webSocketService.sendMessage('nodeDragUpdate', {
            nodeId: numericId,
            position,
            timestamp: Date.now()
          });

          if (debugState.isEnabled()) {
            logger.debug(`Throttled WebSocket update for node ${nodeId}`, position);
          }
        }
      }
    }, POSITION_UPDATE_THROTTLE_MS)
  ).current;

  // Keeps the server hold alive while the pointer is still (ADR-2169 timeout).
  const stopHeartbeatRef = useRef<(() => void) | null>(null);
  const stopHeartbeat = () => {
    stopHeartbeatRef.current?.();
    stopHeartbeatRef.current = null;
  };
  useEffect(() => stopHeartbeat, []);

  const handlePointerDown = useCallback((event: ThreeEvent<PointerEvent>) => {
    event.stopPropagation();
    if (!meshRef.current) return;

    const instanceId = event.instanceId;
    if (instanceId === undefined || instanceId < 0 || instanceId >= displayNodes.length) return;

    const node = displayNodes[instanceId];
    if (!node || !node.position) return;

    // CRITICAL: Disable OrbitControls IMMEDIATELY on pointer down
    // This prevents OrbitControls from capturing subsequent move events
    // before React state updates propagate
    if (onDragStateChange) {
      onDragStateChange(true);
    }

    dragDataRef.current = {
      ...dragDataRef.current,
      pointerDown: true,
      nodeId: node.id,
      instanceId: instanceId,
      startPointerPos: new THREE.Vector2(event.nativeEvent.offsetX, event.nativeEvent.offsetY),
      startTime: Date.now(),
      startNodePos3D: new THREE.Vector3(node.position.x, node.position.y, node.position.z),
      currentNodePos3D: new THREE.Vector3(node.position.x, node.position.y, node.position.z),
    };

    // R3F-level pointer capture (NOT DOM capture): event.target here is R3F's
    // capture shim — it registers this mesh in the event manager's capturedMap
    // AND takes DOM capture, so every subsequent pointermove/pointerup for this
    // pointerId is delivered to the mesh even when the cursor outruns the
    // node's screen radius. Capturing only on the DOM canvas (the old code)
    // left R3F routing moves by raycast, so drags stalled the moment the
    // pointer escaped the node between frames.
    try {
      (event.target as unknown as { setPointerCapture?: (id: number) => void })
        ?.setPointerCapture?.(event.pointerId);
    } catch (e) {
      // Pointer capture may fail in some browsers/contexts
    }

    startInteraction(node.id);

    if (debugState.isEnabled()) {
      logger.debug(`Started interaction tracking for node ${node.id}`);
    }
  }, [displayNodes, meshRef, dragDataRef, startInteraction, onDragStateChange]);

  const handlePointerMove = useCallback((event: ThreeEvent<PointerEvent>) => {
    const drag = dragDataRef.current;
    if (!drag.pointerDown) return;

    
    if (!drag.isDragging) {
      const currentPos = new THREE.Vector2(event.nativeEvent.offsetX, event.nativeEvent.offsetY);
      const distance = currentPos.distanceTo(drag.startPointerPos);

      if (distance > DRAG_THRESHOLD) {
        drag.isDragging = true;
        setDragState({ nodeId: drag.nodeId, instanceId: drag.instanceId });

        const numericId = graphDataManager.nodeIdMap.get(drag.nodeId!);
        if (numericId !== undefined) {
          // ADR-03 D7: pin state is server-authoritative; the websocket
          // `nodeDragStart` message below pins the node server-side.
          // graphWorkerProxy.pinNode was removed.

          // Notify server of drag start so it can pin the node server-side.
          // try/catch: a send failure must never abort the drag state machine.
          try {
            if (graphDataManager.webSocketService?.isReady()) {
              graphDataManager.webSocketService.sendMessage('nodeDragStart', {
                nodeId: numericId,
                position: {
                  x: drag.startNodePos3D.x,
                  y: drag.startNodePos3D.y,
                  z: drag.startNodePos3D.z
                }
              });
            }
          } catch (e) {
            logger.warn('nodeDragStart send failed:', e);
          }
          stopHeartbeat();
          stopHeartbeatRef.current = startDragHeartbeat(() => {
            try {
              if (!graphDataManager.webSocketService?.isReady()) return;
              const p = dragDataRef.current.currentNodePos3D;
              graphDataManager.webSocketService.sendMessage('nodeDragUpdate', {
                nodeId: numericId,
                position: { x: p.x, y: p.y, z: p.z },
                timestamp: Date.now()
              });
            } catch (e) {
              logger.warn('nodeDragUpdate heartbeat failed:', e);
            }
          });
        }
        if (debugState.isEnabled()) {
          logger.debug(`Drag started on node ${drag.nodeId}`);
        }
      }
    }

    
    if (drag.isDragging) {
      event.stopPropagation();

      
      

      
      const cameraDirection = new THREE.Vector3();
      camera.getWorldDirection(cameraDirection);
      const planeNormal = cameraDirection.clone().normalize();

      
      
      const plane = new THREE.Plane(planeNormal, -planeNormal.dot(drag.startNodePos3D));

      
      const raycaster = new THREE.Raycaster();
      raycaster.setFromCamera(event.pointer, camera);

      
      const intersection = new THREE.Vector3();
      const intersectionFound = raycaster.ray.intersectPlane(plane, intersection);

      if (intersectionFound && intersection) {
        const numericId = graphDataManager.nodeIdMap.get(drag.nodeId!);
        if (numericId !== undefined) {
          // ADR-03 D7: positions flow through the binary WebSocket protocol
          // (throttledWebSocketUpdate below); worker no longer owns positions.
          void numericId;
        }

        drag.currentNodePos3D.copy(intersection);

        // DrawnPositions.applyDrag (GraphManager's frame pass) reads
        // dragDataRef.current.currentNodePos3D, so we do NOT need to call setGraphData or setMatrixAt here.
        // This avoids expensive React re-renders during drag (was ~16ms/frame).

        updateNodePosition(drag.nodeId!, {
          x: drag.currentNodePos3D.x,
          y: drag.currentNodePos3D.y,
          z: drag.currentNodePos3D.z
        });

        
        throttledWebSocketUpdate(drag.nodeId!, {
          x: drag.currentNodePos3D.x,
          y: drag.currentNodePos3D.y,
          z: drag.currentNodePos3D.z
        });
      }
    }
  }, [camera, settings?.visualisation?.nodes?.nodeSize, meshRef, dragDataRef, setDragState, setGraphData, updateNodePosition, throttledWebSocketUpdate]);

  const handlePointerUp = useCallback((event?: ThreeEvent<PointerEvent>) => {
    const drag = dragDataRef.current;
    if (!drag.pointerDown) {
      return;
    }

    // Release the R3F-level capture (mirror of setPointerCapture above).
    // R3F also cleans its capturedMap via onLostPointerCapture, so this is a
    // defensive release rather than the only path.
    if (event) {
      try {
        (event.target as unknown as { releasePointerCapture?: (id: number) => void })
          ?.releasePointerCapture?.(event.pointerId);
      } catch (e) {
        // Pointer may not have been captured
      }
    }

    if (drag.isDragging) {
      if (debugState.isEnabled()) logger.debug(`Drag ended for node ${drag.nodeId}`);

      const numericId = graphDataManager.nodeIdMap.get(drag.nodeId!);
      if (numericId !== undefined) {
        // ADR-03 D7: unpin is server-authoritative via the websocket
        // `nodeDragEnd` message below; graphWorkerProxy.unpinNode was removed.
        // try/catch: a failure here must never skip the state reset below —
        // an exception used to leave OrbitControls disabled and the drag stuck.
        try {
          flushPositionUpdates();

          // Notify server of drag end so it can unpin the node and run final settle
          if (graphDataManager.webSocketService?.isReady()) {
            graphDataManager.webSocketService.sendMessage('nodeDragEnd', {
              nodeId: numericId
            });
          }
        } catch (e) {
          logger.warn('nodeDragEnd send failed:', e);
        }
      }
    } else {
      // Click action (not a drag) — select this node to highlight adjacent edges
      if (drag.nodeId) {
        if (onNodeSelect) {
          onNodeSelect(drag.nodeId);
        }
        if (debugState.isEnabled()) logger.debug(`Selected node ${drag.nodeId}`);
      }
    }

    stopHeartbeat();
    endInteraction(drag.nodeId);

    // CRITICAL: Re-enable OrbitControls by signaling drag is complete
    if (onDragStateChange) {
      onDragStateChange(false);
    }

    dragDataRef.current.pointerDown = false;
    dragDataRef.current.isDragging = false;
    dragDataRef.current.nodeId = null;
    dragDataRef.current.instanceId = null;
    dragDataRef.current.pendingUpdate = null;
    setDragState({ nodeId: null, instanceId: null });

    if (debugState.isEnabled()) {
      logger.debug(`Ended interaction tracking for node ${drag.nodeId}`);
    }
  }, [displayNodes, dragDataRef, setDragState, endInteraction, flushPositionUpdates, onDragStateChange, onNodeSelect]);

  // Global pointer up listener as safety net
  // Ensures drag ends even if pointer is released outside the canvas
  useEffect(() => {
    const handleGlobalPointerUp = () => {
      if (dragDataRef.current.pointerDown) {
        handlePointerUp();
      }
    };

    // Add listener on window to catch all pointer up events
    window.addEventListener('pointerup', handleGlobalPointerUp);
    window.addEventListener('pointercancel', handleGlobalPointerUp);

    return () => {
      window.removeEventListener('pointerup', handleGlobalPointerUp);
      window.removeEventListener('pointercancel', handleGlobalPointerUp);
    };
  }, [handlePointerUp]);

  return {
    handlePointerDown,
    handlePointerMove,
    handlePointerUp
  };
};