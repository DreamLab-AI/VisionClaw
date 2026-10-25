import React, { useEffect, useRef } from 'react';
import { Terminal, Search } from 'lucide-react';
import { cn } from '../../../utils/classNameUtils';
import { SearchInput } from '../../design-system/components';
import { useCommandPalette } from '../hooks/useCommandPalette';
import { Command } from '../types';
import { formatShortcut } from '../../../hooks/useKeyboardShortcuts';
import { useFocusTrap, useAnnounce } from '../../../utils/accessibility';

export function CommandPalette() {
  const {
    isOpen,
    searchQuery,
    selectedIndex,
    filteredCommands,
    suggestedCommands,
    setSearchQuery,
    navigateUp,
    navigateDown,
    executeSelectedCommand,
    executeCommand,
    close
  } = useCommandPalette();

  const dialogRef = useFocusTrap(isOpen);
  const containerRef = useRef<HTMLDivElement>(null);
  const selectedRef = useRef<HTMLDivElement>(null);
  const announce = useAnnounce();

  
  useEffect(() => {
    if (!isOpen) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      switch (e.key) {
        case 'ArrowUp':
          e.preventDefault();
          navigateUp();
          break;
        case 'ArrowDown':
          e.preventDefault();
          navigateDown();
          break;
        case 'Enter':
          e.preventDefault();
          executeSelectedCommand();
          break;
        case 'Escape':
          e.preventDefault();
          close();
          break;
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isOpen, navigateUp, navigateDown, executeSelectedCommand, close]);

  
  useEffect(() => {
    if (selectedRef.current && containerRef.current) {
      const container = containerRef.current;
      const selected = selectedRef.current;
      const containerRect = container.getBoundingClientRect();
      const selectedRect = selected.getBoundingClientRect();

      if (selectedRect.bottom > containerRect.bottom) {
        selected.scrollIntoView({ block: 'end', behavior: 'smooth' });
      } else if (selectedRect.top < containerRect.top) {
        selected.scrollIntoView({ block: 'start', behavior: 'smooth' });
      }
    }
  }, [selectedIndex]);

  
  useEffect(() => {
    if (isOpen && filteredCommands.length > 0) {
      announce(`${filteredCommands.length} commands available. Use arrow keys to navigate.`, 'polite');
    } else if (isOpen && searchQuery && filteredCommands.length === 0) {
      announce('No commands found', 'polite');
    }
  }, [isOpen, filteredCommands.length, searchQuery, announce]);

  
  useEffect(() => {
    if (!isOpen) return;

    const handleClickOutside = (e: MouseEvent) => {
      const dialog = document.getElementById('command-palette-dialog');
      if (dialog && !dialog.contains(e.target as Node)) {
        close();
      }
    };

    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, [isOpen, close]);

  if (!isOpen) return null;

  const renderCommand = (command: Command, index: number) => {
    const isSelected = index === selectedIndex;
    const shortcutStr = command.shortcut ? formatShortcut({
      key: command.shortcut.key,
      ctrl: command.shortcut.ctrl,
      alt: command.shortcut.alt,
      shift: command.shortcut.shift,
      meta: command.shortcut.meta,
      description: '',
      handler: () => {}
    }) : null;

    return (
      <div
        key={command.id}
        ref={isSelected ? selectedRef : null}
        role="option"
        aria-selected={isSelected}
        id={`command-${command.id}`}
        className={cn(
          "px-3 py-2 cursor-pointer transition-colors rounded-md mx-2",
          "flex items-center justify-between group",
          isSelected ? "bg-primary/10 text-primary" : "hover:bg-muted"
        )}
        onClick={() => executeCommand(command)}
        onMouseEnter={() => {
          
          const newIndex = filteredCommands.indexOf(command);
          if (newIndex !== -1 && newIndex !== selectedIndex) {
            
          }
        }}
      >
        <div className="flex items-center gap-3 flex-1 min-w-0">
          {command.icon && (
            <command.icon className="h-4 w-4 flex-shrink-0 opacity-60" />
          )}
          <div className="flex-1 min-w-0">
            <div className="font-medium truncate">{command.title}</div>
            {command.description && (
              <div className="text-xs text-muted-foreground truncate">
                {command.description}
              </div>
            )}
          </div>
        </div>
        {shortcutStr && (
          <kbd className={cn(
            "px-2 py-0.5 text-xs rounded",
            "bg-muted border border-border",
            "font-mono text-muted-foreground",
            "group-hover:bg-background"
          )}>
            {shortcutStr}
          </kbd>
        )}
      </div>
    );
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center pt-[15vh]"
      role="dialog"
      aria-modal="true"
      aria-labelledby="command-palette-title"
    >
      {}
      <div className="absolute inset-0 bg-background/80 backdrop-blur-sm" aria-hidden="true" />

      {}
      <div
        ref={dialogRef as React.RefObject<HTMLDivElement>}
        id="command-palette-dialog"
        className={cn(
          "relative w-full max-w-2xl",
          "bg-background border rounded-lg shadow-2xl",
          "animate-in fade-in-0 zoom-in-95 duration-200"
        )}
      >
        {}
        <div className="p-4 border-b">
          <label htmlFor="command-search" className="sr-only">Search commands</label>
          <SearchInput
            id="command-search"
            value={searchQuery}
            onChange={setSearchQuery}
            placeholder="Type a command or search..."
            autoFocus
            className="w-full"
            role="combobox"
            aria-expanded={true}
            aria-controls="command-list"
            aria-activedescendant={selectedIndex >= 0 ? `command-${filteredCommands[selectedIndex]?.id}` : undefined}
          />
        </div>

        {}
        <div
          ref={containerRef}
          id="command-list"
          className="max-h-[400px] overflow-y-auto py-2"
          role="listbox"
          aria-label="Available commands"
        >
          {filteredCommands.length === 0 && suggestedCommands.length === 0 ? (
            <div className="px-4 py-8 text-center text-muted-foreground">
              {searchQuery ? 'No commands found' : 'No recent commands'}
            </div>
          ) : (
            <>
              {searchQuery === '' && filteredCommands.length > 0 && (
                <div className="px-4 py-1.5 text-xs font-medium text-muted-foreground uppercase tracking-wider">
                  Recent Commands
                </div>
              )}
              {filteredCommands.map((command, index) => renderCommand(command, index))}
              {suggestedCommands.length > 0 && (
                <>
                  <div className="px-4 py-1.5 text-xs font-medium text-muted-foreground uppercase tracking-wider">
                    Suggested (semantic)
                  </div>
                  {suggestedCommands.map((command, i) => renderCommand(command, filteredCommands.length + i))}
                </>
              )}
            </>
          )}
        </div>

        {}
        <div className="p-3 border-t flex items-center justify-between text-xs text-muted-foreground">
          <div className="flex items-center gap-4">
            <span className="flex items-center gap-1">
              <kbd className="px-1.5 py-0.5 bg-muted rounded font-mono">↑↓</kbd>
              Navigate
            </span>
            <span className="flex items-center gap-1">
              <kbd className="px-1.5 py-0.5 bg-muted rounded font-mono">Enter</kbd>
              Select
            </span>
            <span className="flex items-center gap-1">
              <kbd className="px-1.5 py-0.5 bg-muted rounded font-mono">Esc</kbd>
              Close
            </span>
          </div>
          <div className="flex items-center gap-1">
            <Terminal className="h-3 w-3" aria-hidden="true" />
            <span id="command-palette-title" className="sr-only">Command Palette</span>
            <span aria-hidden="true">Command Palette</span>
          </div>
        </div>
      </div>
    </div>
  );
}