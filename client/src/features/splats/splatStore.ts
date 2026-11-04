import { create } from 'zustand';
import { createLogger } from '../../utils/loggerConfig';
import { fetchSplatListing } from './api';
import type { SplatMode } from './fit';
import type { SplatStatus } from './SplatController';
import type { SplatAsset, SplatVariant } from './types';

const logger = createLogger('SplatStore');

export type ListingStatus = 'idle' | 'loading' | 'ready' | 'error';

export interface SplatState {
  assets: SplatAsset[];
  listing: ListingStatus;
  listingError: string | null;
  /** the asset being shown, or null for none */
  activeId: string | null;
  mode: SplatMode;
  /** render status of the active asset, written by the scene's controller */
  status: SplatStatus;
  /** the variant actually loaded or being loaded */
  variant: SplatVariant | null;
  statusMessage: string | null;

  loadListing: () => Promise<void>;
  open: (id: string) => void;
  enter: () => void;
  leave: () => void;
  close: () => void;
  setRenderStatus: (status: SplatStatus, variant: SplatVariant | null, message?: string) => void;
}

/** Splat scene state. Listing and the viewer's choice only; GPU objects live in the controller. */
export const useSplatStore = create<SplatState>((set, get) => ({
  assets: [],
  listing: 'idle',
  listingError: null,
  activeId: null,
  mode: 'framed',
  status: 'idle',
  variant: null,
  statusMessage: null,

  loadListing: async () => {
    if (get().listing === 'loading') return;
    set({ listing: 'loading', listingError: null });
    try {
      const { assets } = await fetchSplatListing();
      set({ assets, listing: 'ready' });
      // an asset withdrawn server-side must not stay open
      const active = get().activeId;
      if (active && !assets.some((a) => a.id === active)) get().close();
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      logger.warn(`splat listing failed: ${message}`);
      set({ listing: 'error', listingError: message });
    }
  },

  open: (id) => {
    if (!get().assets.some((a) => a.id === id)) return;
    set({ activeId: id, mode: 'framed' });
  },
  enter: () => {
    if (get().activeId) set({ mode: 'entered' });
  },
  leave: () => set({ mode: 'framed' }),
  close: () => set({ activeId: null, mode: 'framed', status: 'idle', variant: null, statusMessage: null }),

  setRenderStatus: (status, variant, message) => set({ status, variant, statusMessage: message ?? null }),
}));
