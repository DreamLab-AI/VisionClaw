/** Community colours (click to isolate), tier legend/override, language and test filters. */
import { useMemo } from 'react';
import { communityCss } from '../graph';
import type { Lang, Tier } from '../protocol';
import { dispatch, useCs } from '../store';

const LANG_LABEL: Partial<Record<Lang, string>> = {
  type_script: 'TypeScript', java_script: 'JavaScript', gd_script: 'GDScript', rust: 'Rust', python: 'Python', go: 'Go',
  cpp: 'C/C++', cuda: 'CUDA', shell: 'Shell', nix: 'Nix', markdown: 'Markdown', other: 'Other',
};

export function Legend() {
  const model = useCs((s) => s.model);
  const filter = useCs((s) => s.filter);
  const tierMode = useCs((s) => s.tierMode);
  const current = useCs((s) => s.currentTier);
  const communities = useMemo(() => {
    if (!model) return [];
    const counts = new Map<number, number>();
    for (let i = 0; i < model.n; i++) if (model.tier[i] === 2 || model.tier[i] === 1) counts.set(model.community[i], (counts.get(model.community[i]) ?? 0) + 1);
    return [...counts.entries()].sort((a, b) => b[1] - a[1]).slice(0, 24);
  }, [model]);
  if (!model) return null;
  const toggleLang = (l: Lang) => {
    const has = filter.langs.includes(l);
    dispatch({ type: 'filter', patch: { langs: has ? filter.langs.filter((x) => x !== l) : [...filter.langs, l] } });
  };
  return (
    <div className="cs-panel cs-legend" aria-label="Legend and filters">
      <div className="cs-section">Tier</div>
      <div className="cs-row">
        {(['auto', 'l0', 'l1', 'l2'] as const).map((t) => (
          <button key={t} className={tierMode === t ? 'cs-on' : ''} onClick={() => dispatch({ type: 'tier_mode', mode: t as 'auto' | Tier })}>
            {t === 'auto' ? `auto (${current.toUpperCase()})` : t.toUpperCase()}
          </button>
        ))}
      </div>
      <div className="cs-dim cs-small">L0 directories · L1 files · L2 symbols</div>
      <div className="cs-section">Languages</div>
      <div className="cs-row cs-wrap">
        {model.langs.map((l) => (
          <button key={l} className={filter.langs.includes(l) ? 'cs-on' : ''} onClick={() => toggleLang(l)}>
            {LANG_LABEL[l] ?? l}
          </button>
        ))}
      </div>
      <label className="cs-check">
        <input type="checkbox" checked={filter.hideTests} onChange={(e) => dispatch({ type: 'filter', patch: { hideTests: e.target.checked } })} /> hide tests
      </label>
      <div className="cs-section">
        Communities
        {filter.community !== null && (
          <button className="cs-link" onClick={() => dispatch({ type: 'filter', patch: { community: null } })}>
            show all
          </button>
        )}
      </div>
      <div className="cs-swatches">
        {communities.map(([c, count]) => (
          <button
            key={c}
            className={`cs-swatch${filter.community === c ? ' cs-on' : ''}`}
            title={`community ${c}: ${count} nodes — click to isolate`}
            onClick={() => dispatch({ type: 'filter', patch: { community: filter.community === c ? null : c } })}
          >
            <span style={{ background: communityCss(c) }} />
            {c}
          </button>
        ))}
      </div>
    </div>
  );
}
