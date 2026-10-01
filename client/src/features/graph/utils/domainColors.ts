/** Shared corpus domain colours for nodes, labels and hulls. */
export const DEFAULT_DOMAIN_COLOR = '#90A4AE';

export const DOMAIN_COLORS: Readonly<Record<string, string>> = Object.freeze({
  'artificial-intelligence': '#4FC3F7',
  'blockchain': '#81C784',
  'robotics': '#FFB74D',
  'spatial-computing': '#CE93D8',
  'distributed-collaboration': '#4DB6AC',
  'infrastructure': '#FFD54F',
  'space-science-and-systems': '#646b9f',
  'earth-observation-and-geospatial-sensing': '#438273',
  AI: '#4FC3F7',
  BC: '#81C784',
  RB: '#FFB74D',
  MV: '#CE93D8',
  TC: '#FFD54F',
  NGM: '#4DB6AC',
  // Retain colours used by older graph metadata.
  DT: '#EF5350',
  SEC: '#FF7043',
  INFRA: '#78909C',
});

export function getDomainColor(domain?: string): string {
  if (!domain) return DEFAULT_DOMAIN_COLOR;
  const value = domain.trim();
  const alias = value.toUpperCase();
  const key = Object.prototype.hasOwnProperty.call(DOMAIN_COLORS, alias)
    ? alias
    : value.toLowerCase();
  return Object.prototype.hasOwnProperty.call(DOMAIN_COLORS, key)
    ? DOMAIN_COLORS[key]
    : DEFAULT_DOMAIN_COLOR;
}
