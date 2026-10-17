import { createContext, useContext } from 'react';
import type { Session } from '../session';
import type { FrameState } from './frameState';

export interface SceneCtx {
  session: Session;
  frame: FrameState;
}

export const SceneContext = createContext<SceneCtx | null>(null);

export function useScene(): SceneCtx {
  const c = useContext(SceneContext);
  if (!c) throw new Error('SceneContext missing');
  return c;
}
