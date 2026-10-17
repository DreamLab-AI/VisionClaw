/** Typed commands in the voice grammar; shows answers and numbered candidates. */
import { useEffect, useRef, useState } from 'react';
import type { Session } from '../session';
import { dispatch, useCs } from '../store';

export function CommandPalette({ session }: { session: Session }) {
  const [text, setText] = useState('');
  const [hist, setHist] = useState<string[]>([]);
  const [hpos, setHpos] = useState(-1);
  const input = useRef<HTMLInputElement>(null);
  const query = useCs((s) => s.query);
  const errors = useCs((s) => s.errors);
  const lastError = errors.length ? errors[errors.length - 1] : null;
  const showError = lastError && Date.now() - lastError.ts < 6000 ? lastError : null;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing = document.activeElement instanceof HTMLInputElement || document.activeElement instanceof HTMLTextAreaElement;
      if ((e.key === 'k' && (e.ctrlKey || e.metaKey)) || (e.key === '/' && !typing)) {
        e.preventDefault();
        input.current?.focus();
      } else if (e.key === 'Escape' && !typing) {
        dispatch({ type: 'select', id: null });
        dispatch({ type: 'clear_query' });
      } else if (e.key === 'Backspace' && !typing && e.altKey) {
        dispatch({ type: 'back' });
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const run = (t: string) => {
    const v = t.trim();
    if (!v) return;
    session.command(v);
    setHist((h) => [...h.filter((x) => x !== v), v].slice(-50));
    setHpos(-1);
    setText('');
  };

  return (
    <div className="cs-palette">
      <input
        ref={input}
        className="cs-input"
        value={text}
        placeholder="focus parseCommand · callers of apply_delta · path from a to b · help   (/ or Ctrl+K)"
        aria-label="Command"
        spellCheck={false}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') run(text);
          else if (e.key === 'Escape') {
            setText('');
            input.current?.blur();
          } else if (e.key === 'ArrowUp' && hist.length) {
            const p = hpos < 0 ? hist.length - 1 : Math.max(0, hpos - 1);
            setHpos(p);
            setText(hist[p]);
            e.preventDefault();
          } else if (e.key === 'ArrowDown' && hpos >= 0) {
            const p = hpos + 1;
            if (p >= hist.length) {
              setHpos(-1);
              setText('');
            } else {
              setHpos(p);
              setText(hist[p]);
            }
            e.preventDefault();
          }
        }}
      />
      {(query || showError) && (
        <div className="cs-panel cs-results">
          {query && (
            <>
              <div className="cs-results-head">
                <span className="cs-mono cs-dim">{query.command}</span>
                <button className="cs-x" aria-label="Dismiss" onClick={() => dispatch({ type: 'clear_query' })}>
                  ×
                </button>
              </div>
              {query.answer && <pre className="cs-answer">{query.answer}</pre>}
              {query.candidates.length > 0 && (
                <ol className="cs-candidates">
                  {query.candidates.map(([label, id], k) => (
                    <li key={`${id}-${k}`}>
                      <button
                        onClick={() => {
                          dispatch({ type: 'select', id, fly: true });
                          dispatch({ type: 'clear_query' });
                        }}
                      >
                        <span className="cs-num">{k + 1}</span> {label}
                      </button>
                    </li>
                  ))}
                </ol>
              )}
            </>
          )}
          {showError && <div className="cs-warn">{showError.message}</div>}
        </div>
      )}
    </div>
  );
}
