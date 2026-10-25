import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import { CommandPaletteState, Command } from '../types';
import { commandRegistry } from '../CommandRegistry';
import { useKeyboardShortcuts } from '../../../hooks/useKeyboardShortcuts';
import { createLogger } from '../../../utils/loggerConfig';
import { jevRankPalette, semanticSuggestions } from '../../../services/jev/jevClient';

const logger = createLogger('useCommandPalette');

export function useCommandPalette() {
  // Jev option (off by default): extra suggestions appended after the lexical
  // results. Always [] unless the server reports palette-search enabled AND a
  // confident ranking arrives; the lexical list is never reordered or removed.
  const [suggestedCommands, setSuggestedCommands] = useState<Command[]>([]);
  const jevSeq = useRef(0);

  const [state, setState] = useState<CommandPaletteState>({
    isOpen: false,
    searchQuery: '',
    selectedIndex: 0,
    filteredCommands: [],
    recentCommands: []
  });

  
  useEffect(() => {
    if (state.isOpen) {
      const filtered = state.searchQuery 
        ? commandRegistry.searchCommands(state.searchQuery)
        : commandRegistry.getRecentCommands().length > 0
          ? commandRegistry.getRecentCommands()
          : commandRegistry.getAllCommands().filter(cmd => cmd.enabled !== false);
      
      setState(prev => ({
        ...prev,
        filteredCommands: filtered,
        selectedIndex: 0
      }));
    }
  }, [state.searchQuery, state.isOpen]);

  useEffect(() => {
    const seq = ++jevSeq.current;
    setSuggestedCommands([]);
    const query = state.searchQuery.trim();
    if (!state.isOpen || !query) return;
    const timer = setTimeout(async () => {
      const allEnabled = commandRegistry.getAllCommands().filter(cmd => cmd.enabled !== false);
      let ranking = null;
      try {
        ranking = await jevRankPalette(
          query,
          allEnabled.map(c => ({ id: c.id, title: c.title, description: c.description, category: c.category })),
        );
      } catch {
        ranking = null; // fail-open: lexical results only
      }
      if (seq !== jevSeq.current) return; // a newer query superseded this one
      setSuggestedCommands(semanticSuggestions(state.filteredCommands, ranking, allEnabled));
    }, 350);
    return () => clearTimeout(timer);
    // filteredCommands is recomputed from searchQuery in the effect above.
  }, [state.searchQuery, state.isOpen, state.filteredCommands]);

  
  useEffect(() => {
    setState(prev => ({
      ...prev,
      recentCommands: commandRegistry.getRecentCommands().map(cmd => cmd.id)
    }));
  }, []);

  
  const open = useCallback(() => {
    setState(prev => ({
      ...prev,
      isOpen: true,
      searchQuery: '',
      selectedIndex: 0
    }));
  }, []);

  
  const close = useCallback(() => {
    setState(prev => ({
      ...prev,
      isOpen: false,
      searchQuery: '',
      selectedIndex: 0
    }));
  }, []);

  
  const toggle = useCallback(() => {
    setState(prev => ({
      ...prev,
      isOpen: !prev.isOpen,
      searchQuery: '',
      selectedIndex: 0
    }));
  }, []);

  
  const setSearchQuery = useCallback((query: string) => {
    setState(prev => ({
      ...prev,
      searchQuery: query,
      selectedIndex: 0
    }));
  }, []);

  
  const suggestedRef = useRef<Command[]>([]);
  suggestedRef.current = suggestedCommands;

  const navigateUp = useCallback(() => {
    setState(prev => ({
      ...prev,
      selectedIndex: Math.max(0, prev.selectedIndex - 1)
    }));
  }, []);

  const navigateDown = useCallback(() => {
    setState(prev => ({
      ...prev,
      selectedIndex: Math.min(prev.filteredCommands.length + suggestedRef.current.length - 1, prev.selectedIndex + 1)
    }));
  }, []);

  
  const executeCommand = useCallback(async (command: Command) => {
    try {
      commandRegistry.addRecentCommand(command.id);
      close();
      await command.handler();
    } catch (error) {
      logger.error('Failed to execute command:', error);
    }
  }, [close]);

  const executeSelectedCommand = useCallback(async () => {
    const selectedCommand = [...state.filteredCommands, ...suggestedCommands][state.selectedIndex];
    if (selectedCommand) {
      await executeCommand(selectedCommand);
    }
  }, [state.filteredCommands, suggestedCommands, state.selectedIndex, executeCommand]);

  
  useKeyboardShortcuts({
    'command-palette-toggle': {
      key: 'k',
      ctrl: true,
      description: 'Toggle command palette',
      handler: toggle,
      category: 'General'
    }
  });

  return {
    ...state,
    suggestedCommands,
    open,
    close,
    toggle,
    setSearchQuery,
    navigateUp,
    navigateDown,
    executeCommand,
    executeSelectedCommand
  };
}